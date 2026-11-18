//! MCP over the streamable-HTTP transport (`/mcp`): each session is the stdio server's own
//! `solidcraft_mcp::Server` over a headless session, on a thread of its own (a session is not
//! shared between threads). HTTP connections hand it one message at a time and wait for its reply.

use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_engine::io::vfs::PathScope;
use solidcraft_engine::schema::ParamCheck;

use super::http::{HttpError, Response};

/// A message for a session and where its reply goes (`None`: a notification, no reply).
type Job = (String, Sender<Option<String>>);

/// What every session is made with.
#[derive(Clone)]
pub struct SessionConfig {
    pub scope: PathScope,
    pub options: solidcraft_mcp::Options,
    pub param_check: ParamCheck,
    pub plugins: bool,
    pub idle: Duration,
    pub max_sessions: usize,
    /// The design `serve --dir DESIGN.sc` named: each session starts with it open.
    pub open: Option<std::path::PathBuf>,
}

/// The live sessions, by id.
pub struct Sessions {
    config: SessionConfig,
    live: Arc<Mutex<HashMap<String, Sender<Job>>>>,
}

impl Sessions {
    pub fn new(config: SessionConfig) -> Self {
        Sessions { config, live: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// `POST /mcp` with `body`, for the session `id` names (none for `initialize`).
    pub fn post(&self, id: Option<&str>, body: &[u8]) -> Result<Response, HttpError> {
        let text = std::str::from_utf8(body).map_err(|_| HttpError::new(400, "the body is not UTF-8 text"))?;
        let msg: Value = serde_json::from_str(text).map_err(|e| HttpError::new(400, format!("the body is not JSON: {e}")))?;
        let initialize = msg.get("method").and_then(Value::as_str) == Some("initialize");
        let (id, tx) = if initialize {
            self.start()?
        } else {
            let id = id.ok_or_else(|| HttpError::new(400, "send the Mcp-Session-Id header from the initialize reply"))?;
            let tx = self.lookup(id).ok_or_else(|| HttpError::new(404, "no such session (it ended or the server restarted); initialize again"))?;
            (id.to_string(), tx)
        };
        // One line, as on stdio: the server's own size and batch checks apply.
        let line = msg.to_string();
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx.send((line, reply_tx)).is_err() {
            self.end(&id);
            return Err(HttpError::new(404, "the session ended; initialize again"));
        }
        let reply = reply_rx.recv().map_err(|_| HttpError::new(500, "the session stopped while answering"))?;
        let response = match reply {
            Some(reply) => Response::new(200, "application/json", reply.into_bytes()).with("Cache-Control", "no-store"),
            None => Response::empty(202),
        };
        Ok(response.with("Mcp-Session-Id", id))
    }

    /// `DELETE /mcp`: end the session now.
    pub fn delete(&self, id: Option<&str>) -> Result<Response, HttpError> {
        let id = id.ok_or_else(|| HttpError::new(400, "send the Mcp-Session-Id header of the session to end"))?;
        if self.end(id) { Ok(Response::empty(204)) } else { Err(HttpError::new(404, "no such session")) }
    }

    fn lookup(&self, id: &str) -> Option<Sender<Job>> {
        self.live.lock().ok()?.get(id).cloned()
    }

    fn end(&self, id: &str) -> bool {
        self.live.lock().ok().and_then(|mut l| l.remove(id)).is_some()
    }

    /// A new session on a thread of its own.
    fn start(&self) -> Result<(String, Sender<Job>), HttpError> {
        let id = solidcraft_control::server::new_token().ok_or_else(|| HttpError::new(500, "no random numbers for a session id"))?;
        let (tx, rx) = mpsc::channel::<Job>();
        {
            let mut live = self.live.lock().map_err(|_| HttpError::new(500, "the session table is broken"))?;
            if live.len() >= self.config.max_sessions {
                return Err(HttpError::new(503, format!("{} MCP sessions are open, the most this server takes; end one first", live.len())));
            }
            live.insert(id.clone(), tx.clone());
        }
        let config = self.config.clone();
        let live = Arc::clone(&self.live);
        let thread_id = id.clone();
        let spawned = std::thread::Builder::new().name("mcp-session".into()).spawn(move || {
            let start = config.open.as_ref().map(|p| {
                solidcraft_engine::io::vfs::scoped(Some(config.scope.clone()), || crate::load_checked(&p.to_string_lossy(), config.param_check))
            });
            let mut headless = match start {
                Some(Ok(session)) => solidcraft_mcp::Headless::new(session),
                Some(Err(e)) => {
                    solidcraft_engine::note!("solidcraft serve: a new MCP session starts empty: {e}");
                    solidcraft_mcp::Headless::default()
                }
                None => solidcraft_mcp::Headless::default(),
            };
            headless.session.param_check = config.param_check;
            let backend = ServeBackend { inner: headless, plugins: config.plugins };
            let mut server = solidcraft_mcp::Server::new(Box::new(backend)).with_path_scope(Some(config.scope)).with_options(config.options);
            loop {
                match rx.recv_timeout(config.idle) {
                    Ok((line, reply)) => {
                        let _ = reply.send(server.handle_line(&line));
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if let Ok(mut l) = live.lock() {
                            l.remove(&thread_id);
                        }
                        solidcraft_engine::note!(
                            "solidcraft serve: an MCP session ended after {} minutes without a request",
                            config.idle.as_secs() / 60
                        );
                        return;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        if let Err(e) = spawned {
            self.end(&id);
            return Err(HttpError::new(503, format!("could not start a session: {e}")));
        }
        Ok((id, tx))
    }
}

/// A headless session with plugins refused unless the server allows them.
struct ServeBackend {
    inner: solidcraft_mcp::Headless,
    plugins: bool,
}

/// Whether a control call runs a plugin command anywhere in it (`execute`, a batch, a script).
pub fn names_plugin_command(params: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    match params {
        Value::Object(o) => o.iter().any(|(k, v)| {
            (matches!(k.as_str(), "command" | "id") && v.as_str().is_some_and(|c| c.starts_with("plugin."))) || names_plugin_command(v, depth + 1)
        }),
        Value::Array(a) => a.iter().any(|v| names_plugin_command(v, depth + 1)),
        _ => false,
    }
}

impl solidcraft_mcp::Backend for ServeBackend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if !self.plugins && matches!(method, "engine.execute" | "command" | "engine.batch" | "engine.script") && names_plugin_command(&params, 0) {
            return Err("plugins are off on this server (its operator can start it with --allow-plugins)".into());
        }
        self.inner.call(method, params)
    }
    fn supports_plugin_tools(&mut self) -> bool {
        self.plugins && self.inner.supports_plugin_tools()
    }
    fn has_ui(&self) -> bool {
        self.inner.has_ui()
    }
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn design_path(&self) -> Option<String> {
        self.inner.design_path()
    }
    fn in_process(&self) -> bool {
        self.inner.in_process()
    }
    fn notice(&self) -> Option<String> {
        self.inner.notice()
    }
    fn param_check(&self) -> ParamCheck {
        self.inner.param_check()
    }
}

/// The `info` reply's MCP part.
pub fn info(config: &SessionConfig) -> Value {
    json!({"path": "/mcp", "max_sessions": config.max_sessions, "plugins": config.plugins})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_commands_are_found_wherever_a_call_names_them() {
        assert!(names_plugin_command(&json!({"command": "plugin.run", "params": {}}), 0));
        assert!(names_plugin_command(&json!({"commands": [{"command": "solid.box"}, {"command": "plugin.install"}]}), 0));
        assert!(names_plugin_command(&json!([{"id": "plugin.list"}]), 0));
        assert!(!names_plugin_command(&json!({"command": "solid.box", "params": {"body_name": "plugin.x"}}), 0));
    }
}
