use super::transpile::transpile;
use crate::{Error, Result};
use rquickjs::{Context, Ctx, Function, Module, Runtime, Value, WriteOptions};
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MEMORY: usize = 64 * 1024 * 1024;
const CACHE: usize = 256;
// Runtime drops before bytecode: QuickJS may reference ROM_DATA until its heap is freed.
struct JsEngine {
    runtime: Runtime,
    code: HashMap<String, Vec<u8>>,
}
impl JsEngine {
    fn new() -> Result<Self> {
        let runtime = Runtime::new().map_err(js_error)?;
        runtime.set_memory_limit(MEMORY);
        runtime.set_max_stack_size(512 * 1024);
        Ok(Self {
            runtime,
            code: HashMap::new(),
        })
    }
    fn compile(&mut self, source: &str) -> Result<()> {
        if self.code.contains_key(source) {
            return Ok(());
        }
        let body = transpile(source)?;
        let module_source = format!(
            "export default function(){{\n{}\n{}\n}}",
            include_str!("helpers.js"),
            body
        );
        let context = Context::full(&self.runtime).map_err(js_error)?;
        let code = context.with(|ctx| {
            Module::declare(ctx.clone(), "expression", module_source)
                .and_then(|m| m.write(WriteOptions::default()))
                .map_err(|e| caught(&ctx, e))
        })?;
        self.code.insert(source.into(), code);
        Ok(())
    }
    fn evaluate(
        &mut self,
        source: &str,
        data: &str,
        sample: impl Fn(f64) -> String + 'static,
        deadline: Instant,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<f32>> {
        self.compile(source)?;
        let expired = Arc::new(AtomicBool::new(false));
        let mark = expired.clone();
        self.runtime.set_interrupt_handler(Some(Box::new(move || {
            let stop = cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;
            if stop {
                mark.store(true, Ordering::Relaxed);
            }
            stop
        })));
        let result = (|| {
            let context = Context::full(&self.runtime).map_err(js_error)?;
            context.with(|ctx| {
                let operation = || -> rquickjs::Result<Vec<f32>> {
                    ctx.globals().set("__msData", data)?;
                    ctx.globals().set("__msSample", Function::new(ctx.clone(), sample)?)?;
                    // No external clocks, module loader, timers, native IO or async host APIs.
                    ctx.eval::<(), _>("globalThis.Date=undefined; globalThis.Promise=undefined; globalThis.SharedArrayBuffer=undefined; globalThis.Atomics=undefined;")?;
                    // SAFETY: only bytecode produced above by this exact runtime is loaded.
                    // Its allocation remains pinned until the runtime is dropped.
                    let module = unsafe { Module::load(ctx.clone(), &self.code[source]) }?;
                    let (module, promise) = module.eval()?;
                    promise.finish::<()>()?;
                    let function: Function = module.get("default")?;
                    let result: Value = function.call(())?;
                    if let Some(n) = result.as_number() { return Ok(vec![n as f32]); }
                    if let Some(a) = result.as_array() {
                        if !(1..=4).contains(&a.len()) { return Err(rquickjs::Error::new_from_js_message("Array", "property", "expected 1..4 components")); }
                        return (0..a.len()).map(|i| a.get::<Value>(i)?.as_number().map(|n| n as f32)
                            .ok_or_else(|| rquickjs::Error::new_from_js_message("component", "number", "expected numeric components"))).collect();
                    }
                    Err(rquickjs::Error::new_from_js_message("expression", "property", "expected Number or Array"))
                };
                operation().map_err(|e| caught(&ctx, e))
            })
        })();
        self.runtime.set_interrupt_handler(None);
        self.runtime.run_gc();
        if expired.load(Ordering::Relaxed) {
            return Err(Error::Invalid(
                "expression cancelled or execution budget exceeded".into(),
            ));
        }
        let result = result?;
        if result.iter().any(|v| !v.is_finite()) {
            return Err(Error::Invalid("expression result must be finite".into()));
        }
        Ok(result)
    }
}
fn js_error(e: rquickjs::Error) -> Error {
    Error::Invalid(format!("JavaScript: {e}"))
}
fn caught(ctx: &Ctx<'_>, e: rquickjs::Error) -> Error {
    if e.is_exception() {
        let exception = ctx.catch();
        let message = exception
            .as_object()
            .and_then(|o| o.get::<_, String>("message").ok())
            .or_else(|| exception.as_string().and_then(|s| s.to_string().ok()))
            .unwrap_or_else(|| e.to_string());
        return Error::Invalid(format!("JavaScript: {message}"));
    }
    js_error(e)
}
thread_local! { static ENGINE: RefCell<Option<JsEngine>> = const { RefCell::new(None) }; }
fn with_engine<T>(f: impl FnOnce(&mut JsEngine) -> Result<T>) -> Result<T> {
    ENGINE.with(|slot| {
        let mut slot = slot
            .try_borrow_mut()
            .map_err(|_| Error::Invalid("recursive expression evaluation".into()))?;
        if slot.as_ref().is_none_or(|e| e.code.len() >= CACHE) {
            *slot = Some(JsEngine::new()?);
        }
        let result = f(slot.as_mut().unwrap());
        // Dynamically constructed async functions must not leave jobs or contexts
        // alive for the next frame. Such expressions are outside this profile.
        if slot.as_ref().unwrap().runtime.is_job_pending() {
            *slot = None;
            return Err(Error::Invalid(
                "asynchronous expressions are unsupported".into(),
            ));
        }
        result
    })
}
pub(super) fn compile(source: &str) -> Result<()> {
    with_engine(|e| e.compile(source))
}
pub(super) fn evaluate(
    source: &str,
    data: &str,
    sample: impl Fn(f64) -> String + 'static,
    frame_deadline: Instant,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<f32>> {
    let deadline = frame_deadline.min(Instant::now() + Duration::from_millis(20));
    if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
        return Err(Error::Invalid(
            "expression cancelled or frame budget exceeded".into(),
        ));
    }
    with_engine(|e| e.evaluate(source, data, sample, deadline, cancel))
}
