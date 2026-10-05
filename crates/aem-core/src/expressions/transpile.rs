use crate::{ensure, Error, Result};
use swc_common::{sync::Lrc, FileName, SourceMap, Spanned, DUMMY_SP};
use swc_ecma_ast::*;
use swc_ecma_parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax};
use swc_ecma_visit::{VisitMut, VisitMutWith};

/// Parse real JavaScript before adapting AE's vector operators and completion value.
pub(super) fn transpile(source: &str) -> Result<String> {
    ensure(
        !source.trim().is_empty() && source.len() <= 8192,
        "expression source must be 1..8192 bytes",
    )?;
    let cm: Lrc<SourceMap> = Default::default();
    let file = cm.new_source_file(
        FileName::Custom("expression.js".into()).into(),
        source.to_owned(),
    );
    let lexer = Lexer::new(
        Syntax::Es(EsSyntax::default()),
        EsVersion::Es2018,
        StringInput::from(&*file),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let mut script = parser.parse_script().map_err(|e| {
        Error::Invalid(format!(
            "JavaScript syntax: {:?} at byte {}",
            e.kind(),
            e.span().lo.0
        ))
    })?;
    if let Some(e) = parser.take_errors().first() {
        return Err(Error::Invalid(format!(
            "JavaScript syntax: {:?} at byte {}",
            e.kind(),
            e.span().lo.0
        )));
    }
    let mut adapter = Adapter::default();
    script.visit_mut_with(&mut adapter);
    ensure(
        !adapter.invalid,
        "expression exceeds AST limits, uses reserved __ms identifiers or asynchronous JavaScript",
    )?;
    if let Some(tail) = script.body.last_mut() {
        completion(tail);
    }
    Ok(swc_ecma_codegen::to_code(&script))
}

fn completion(stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) => {
            *stmt = Stmt::Return(ReturnStmt {
                span: e.span,
                arg: Some(e.expr.clone()),
            })
        }
        Stmt::Block(b) => {
            if let Some(s) = b.stmts.last_mut() {
                completion(s);
            }
        }
        Stmt::If(i) => {
            completion(&mut i.cons);
            if let Some(s) = &mut i.alt {
                completion(s);
            }
        }
        // Other completion forms deliberately return undefined and receive a type error.
        _ => {}
    }
}

#[derive(Default)]
struct Adapter {
    depth: usize,
    count: usize,
    invalid: bool,
}
fn call<const N: usize>(name: &str, args: [Box<Expr>; N]) -> Expr {
    Expr::Call(CallExpr {
        span: DUMMY_SP,
        ctxt: Default::default(),
        callee: Callee::Expr(Box::new(Expr::Ident(Ident::new(
            name.into(),
            DUMMY_SP,
            Default::default(),
        )))),
        args: args
            .into_iter()
            .map(|expr| ExprOrSpread { spread: None, expr })
            .collect(),
        type_args: None,
    })
}
impl VisitMut for Adapter {
    fn visit_mut_function(&mut self, f: &mut Function) {
        if f.is_async {
            self.invalid = true;
        } else {
            f.visit_mut_children_with(self);
        }
    }
    fn visit_mut_ident(&mut self, id: &mut Ident) {
        self.invalid |= id.sym.starts_with("__ms");
    }
    fn visit_mut_stmt(&mut self, stmt: &mut Stmt) {
        self.depth += 1;
        self.count += 1;
        if self.depth > 128 || self.count > 4096 {
            self.invalid = true;
        } else {
            stmt.visit_mut_children_with(self);
        }
        self.depth -= 1;
    }
    fn visit_mut_expr(&mut self, expr: &mut Expr) {
        self.depth += 1;
        self.count += 1;
        if self.depth > 128 || self.count > 4096 {
            self.invalid = true;
            self.depth -= 1;
            return;
        }
        expr.visit_mut_children_with(self);
        match expr {
            Expr::Await(_) => self.invalid = true,
            Expr::Arrow(a) if a.is_async => self.invalid = true,
            Expr::Bin(b) => {
                let name = match b.op {
                    BinaryOp::Add => "__msAdd",
                    BinaryOp::Sub => "__msSub",
                    BinaryOp::Mul => "__msMul",
                    BinaryOp::Div => "__msDiv",
                    BinaryOp::Mod => "__msMod",
                    _ => "",
                };
                if !name.is_empty() {
                    *expr = call(name, [b.left.clone(), b.right.clone()]);
                }
            }
            Expr::Unary(u) if u.op == UnaryOp::Minus => {
                *expr = call("__msNeg", [u.arg.clone()]);
            }
            Expr::Assign(a) => {
                let name = match a.op {
                    AssignOp::AddAssign => "__msAdd",
                    AssignOp::SubAssign => "__msSub",
                    AssignOp::MulAssign => "__msMul",
                    AssignOp::DivAssign => "__msDiv",
                    AssignOp::ModAssign => "__msMod",
                    _ => "",
                };
                // Identifier assignments evaluate their left side once. Member assignments
                // retain JS semantics, avoiding duplicate getters or index side effects.
                if let AssignTarget::Simple(SimpleAssignTarget::Ident(i)) = &a.left {
                    if !name.is_empty() {
                        *a.right =
                            call(name, [Box::new(Expr::Ident(i.id.clone())), a.right.clone()]);
                        a.op = AssignOp::Assign;
                    }
                }
            }
            _ => {}
        }
        self.depth -= 1;
    }
}
