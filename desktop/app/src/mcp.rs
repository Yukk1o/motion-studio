//! MCP stdio adapter and the in-process tool boundary reserved for an embedded agent.
//! No shell execution, arbitrary scripts or network access is exposed.
use crate::engine::{Engine, REQUEST_LIMIT};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

pub const PROTOCOL: &str = "2025-11-25";
#[derive(Clone, Copy)]
pub enum Access {
    ReadOnly,
    Edit,
}

/// An embedded agent can use this router with the existing window's Engine.
/// Revision-checked editing is atomic on the same worker as pointer edits.
pub struct ToolRouter<'a> {
    pub engine: &'a Engine,
    pub access: Access,
}
impl ToolRouter<'_> {
    pub fn call(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let args = arguments
            .as_object()
            .ok_or("tool arguments must be an object")?;
        let fields: &[&str] = match name {
            "motion_state" => &[],
            "motion_edit" => &["commands", "expectedRevision"],
            "motion_seek" => &["frame"],
            "motion_history" => &["operation"],
            "motion_save" => &[],
            _ => return Err("unknown editor tool".into()),
        };
        if args.keys().any(|key| !fields.contains(&key.as_str())) {
            return Err("unknown tool argument".into());
        }
        let write = matches!(
            name,
            "motion_edit" | "motion_history" | "motion_save" | "motion_seek"
        );
        if write && matches!(self.access, Access::ReadOnly) {
            return Err("editor writes are disabled for this agent".into());
        }
        match name {
            "motion_state" => self.engine.state(),
            "motion_edit" => {
                let commands = args
                    .get("commands")
                    .and_then(Value::as_array)
                    .ok_or("commands must be an array")?;
                if commands.is_empty() {
                    return Err("commands must not be empty".into());
                }
                let revision = args
                    .get("expectedRevision")
                    .and_then(Value::as_u64)
                    .ok_or("expectedRevision must be a nonnegative integer")?;
                self.engine.edit_at_revision(
                    serde_json::to_string(commands).map_err(|e| e.to_string())?,
                    revision,
                )
            }
            "motion_seek" => self.engine.seek(
                args.get("frame")
                    .and_then(Value::as_f64)
                    .ok_or("frame must be a number")?,
            ),
            "motion_history" => {
                self.engine
                    .history(match args.get("operation").and_then(Value::as_str) {
                        Some("undo") => 0,
                        Some("redo") => 1,
                        _ => return Err("operation must be undo or redo".into()),
                    })
            }
            "motion_save" => self.engine.save(),
            _ => unreachable!(),
        }
    }
}
fn schema(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn tool(name: &str, description: &str, input_schema: Value, read_only: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":input_schema,
        "annotations":{"readOnlyHint":read_only,"destructiveHint":false,"openWorldHint":false}})
}
pub fn tools() -> Value {
    json!({"tools":[
        tool("motion_state","Get the current project, revision, sampled transforms and timeline. The stdio host is headless.",schema(json!({}),&[]),true),
        tool("motion_edit","Apply an atomic batch of Motion Studio core commands with one undo step. Fetch state first and provide its revision. Use position/rotation/scale, set_vector/set_scalar/set_component, animate, curve, add, add_shape, duplicate, delete, flags, parent, move_layer_clip or trim_layer_clip. All commands use the shared core JSON API.",schema(json!({
            "commands":{"type":"array","minItems":1,"items":{"type":"object","required":["op"],"properties":{"op":{"type":"string"}}}},
            "expectedRevision":{"type":"integer","minimum":0}
        }),&["commands","expectedRevision"]),false),
        tool("motion_seek","Seek to a fractional composition frame in [0, frames).",schema(json!({"frame":{"type":"number","minimum":0}}),&["frame"]),false),
        tool("motion_history","Undo or redo the most recent committed operation.",schema(json!({"operation":{"type":"string","enum":["undo","redo"]}}),&["operation"]),false),
        tool("motion_save","Atomically save the current project and validate its assets.",schema(json!({}),&[]),false)
    ]})
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
struct Server<'a> {
    router: ToolRouter<'a>,
    notify: &'a dyn Fn(),
    negotiated: bool,
    initialized: bool,
}
impl Server<'_> {
    fn respond(&mut self, message: Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let valid_id = id
            .as_ref()
            .is_none_or(|id| id.is_string() || id.is_i64() || id.is_u64());
        if message["jsonrpc"] != "2.0" || !message["method"].is_string() || !valid_id {
            return Some(error(Value::Null, -32600, "invalid JSON-RPC request"));
        }
        let method = message["method"].as_str().unwrap();
        if id.is_none() {
            // Notifications never receive responses. Initialization is complete
            // only after the client's initialized notification.
            if method == "notifications/initialized" && self.negotiated {
                self.initialized = true;
            }
            return None;
        }
        let id = id.unwrap();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "initialize" => {
                let Some(version) = params["protocolVersion"].as_str() else {
                    return Some(error(id, -32602, "protocolVersion is required"));
                };
                if !params["capabilities"].is_object() || !params["clientInfo"]["name"].is_string()
                {
                    return Some(error(
                        id,
                        -32602,
                        "client capabilities and clientInfo are required",
                    ));
                }
                let version = if matches!(version, "2025-11-25" | "2025-06-18") {
                    version
                } else {
                    PROTOCOL
                };
                self.negotiated = true;
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"motion-studio","version":env!("CARGO_PKG_VERSION")},
                    "instructions":"Read motion_state before editing. Supply expectedRevision for every atomic batch; edits share the editor undo history."})
            }
            "ping" => json!({}),
            _ if !self.initialized => {
                return Some(error(
                    id,
                    -32002,
                    "client must initialize the MCP session first",
                ))
            }
            "tools/list" => tools(),
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Some(error(id, -32602, "tool name is required"));
                };
                if !tools()["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["name"] == name)
                {
                    return Some(error(id, -32602, "unknown tool"));
                }
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match self.router.call(name, arguments) {
                    Ok(value) => {
                        if name != "motion_state" {
                            (self.notify)();
                        }
                        json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
                    }
                    Err(message) => {
                        json!({"content":[{"type":"text","text":message}],"isError":true})
                    }
                }
            }
            _ => return Some(error(id, -32601, "method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}
/// Read a bounded JSON line without allocating an unbounded line before checking.
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Result<Value, String>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if bytes.is_empty() && !oversized {
                Ok(None)
            } else {
                Ok(Some(if oversized {
                    Err("message exceeds 256 KiB".into())
                } else {
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
                }))
            };
        }
        let n = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        let newline = chunk[n - 1] == b'\n';
        if bytes.len() + n > REQUEST_LIMIT {
            oversized = true;
        }
        if !oversized {
            bytes.extend_from_slice(&chunk[..n]);
        }
        reader.consume(n);
        if newline {
            return Ok(Some(if oversized {
                Err("message exceeds 256 KiB".into())
            } else {
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())
            }));
        }
    }
}
fn run(
    engine: &Engine,
    access: Access,
    notify: &dyn Fn(),
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<(), String> {
    let mut server = Server {
        router: ToolRouter { engine, access },
        notify,
        negotiated: false,
        initialized: false,
    };
    while let Some(message) = read_message(reader).map_err(|e| e.to_string())? {
        let response = match message {
            Ok(value) => server.respond(value),
            Err(message) => Some(error(Value::Null, -32700, &message)),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut *writer, &response).map_err(|e| e.to_string())?;
            writer.write_all(b"\n").map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
pub fn serve(
    engine: std::sync::Arc<Engine>,
    notify: Option<Box<dyn Fn() + Send>>,
) -> Result<(), String> {
    let access = if std::env::args().any(|a| a == "--mcp-read-only") {
        Access::ReadOnly
    } else {
        Access::Edit
    };
    let noop = || {};
    let notify = notify.as_deref().map(|f| f as &dyn Fn()).unwrap_or(&noop);
    run(
        &engine,
        access,
        notify,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_and_live_notifications_keep_the_shared_engine_consistent() {
        let folder = tempfile::tempdir().unwrap();
        let engine = Engine::start(
            folder.path().join("default"),
            aem_desktop_media::platform(),
            8 << 30,
        )
        .unwrap();
        let count = std::cell::Cell::new(0);
        let notify = || count.set(count.get() + 1);
        let mut server = Server {
            router: ToolRouter {
                engine: &engine,
                access: Access::Edit,
            },
            notify: &notify,
            negotiated: false,
            initialized: false,
        };
        server.respond(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        let invalid = server
            .respond(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .unwrap();
        assert!(invalid.get("error").is_some());
        server.respond(json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}}));
        server.respond(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        let result=server.respond(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"motion_seek","arguments":{"frame":21.5}}})).unwrap();
        assert_eq!(result["result"]["isError"], false);
        assert_eq!(count.get(), 1);
        assert_eq!(engine.state().unwrap()["frame"], 21.5);
        engine.history(2).unwrap();
        let revision = engine.state().unwrap()["revision"].clone();
        assert!(server.router.call("motion_edit",json!({"commands":[{"op":"set_color","object":0,"value":[0,0,0,1]}],"expectedRevision":revision})).is_err());
        engine.history(4).unwrap();
    }
    #[test]
    fn bounded_reader_recovers_after_oversized_or_malformed_input() {
        let mut bytes = vec![b' '; REQUEST_LIMIT + 10];
        bytes.extend_from_slice(b"\n{}\n");
        let mut reader = io::Cursor::new(bytes);
        assert!(read_message(&mut reader).unwrap().unwrap().is_err());
        assert_eq!(
            read_message(&mut reader).unwrap().unwrap().unwrap(),
            json!({})
        );
        assert!(read_message(&mut reader).unwrap().is_none());
    }
    #[test]
    fn mcp_round_trip_edits_seeks_undoes_and_saves() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().join("default");
        let engine = Engine::start(root.clone(), aem_desktop_media::platform(), 8 << 30).unwrap();
        let revision = engine.state().unwrap()["revision"].as_u64().unwrap();
        let requests = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"motion_edit","arguments":{"expectedRevision":revision,"commands":[{"op":"add_shape","id":1,"name":"Rectangle","shape":"rectangle","size":[100,100],"position":[960,540,0]}]}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"motion_seek","arguments":{"frame":21.5}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"motion_save","arguments":{}}}),
        ];
        let text = requests
            .iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>();
        let mut output = Vec::new();
        run(
            &engine,
            Access::Edit,
            &|| {},
            &mut io::Cursor::new(text),
            &mut output,
        )
        .unwrap();
        let replies = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), 4);
        assert_eq!(replies[1]["result"]["isError"], false, "{replies:?}");
        assert_eq!(replies[2]["result"]["structuredContent"]["frame"], 21.5);
        let router = ToolRouter {
            engine: &engine,
            access: Access::Edit,
        };
        assert!(
            router
                .call(
                    "motion_edit",
                    json!({"commands":[{"op":"delete","object":1}],"expectedRevision":revision})
                )
                .is_err(),
            "stale edits must fail"
        );
        router
            .call("motion_history", json!({"operation":"undo"}))
            .unwrap();
        assert!(engine.state().unwrap()["project"]["layers"]
            .as_array()
            .unwrap()
            .is_empty());
        router
            .call("motion_history", json!({"operation":"redo"}))
            .unwrap();
        assert_eq!(
            engine.state().unwrap()["project"]["layers"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(root.join("project.json").exists());
        let readonly = ToolRouter {
            engine: &engine,
            access: Access::ReadOnly,
        };
        assert!(readonly.call("motion_save", json!({})).is_err());
        assert!(readonly.call("motion_state", json!({})).is_ok());
    }
}
