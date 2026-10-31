//! The ACP backend (desktop only): starts the agent the user picked as a child process and holds
//! the protocol's side of the conversation (initialize, the session, prompts, permission questions,
//! sign-in). The panel sees only [`Event`]s.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::acp::{self, Incoming};
use super::backend::{ChatBackend, Event, PromptPart, StartContext, TestError};
use super::process::{AgentProcess, FromAgent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Initialize,
    SessionNew,
    Prompt,
    Authenticate,
    SetMode,
    SetModel,
    SetConfig,
}

/// Variables the agent doesn't inherit: a control token set for the app's own `--control` port
/// would otherwise reach the agent and outrank the chat endpoint's own token file.
const ENV_REMOVE: &[&str] = &["SOLIDCRAFT_CONTROL_TOKEN"];

pub struct AcpBackend {
    proc: AgentProcess,
    cwd: std::path::PathBuf,
    bridge: Option<acp::McpBridge>,
    /// The agent's `initialize` reply: which routes the system note can take into a session.
    init: Value,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    session: Option<String>,
    agent: String,
    images: bool,
    documents: bool,
    auth: Vec<acp::AuthMethod>,
    /// Open permission questions: our key → the agent's request id.
    permissions: HashMap<u64, Value>,
    next_key: u64,
    exited: bool,
    config_pending: Option<(u64, String)>,
    modern_config: bool,
}

impl AcpBackend {
    /// Start `argv` and say hello (`initialize`); the session follows when the agent answers.
    pub fn start(argv: &[String], ctx: StartContext) -> Result<AcpBackend, String> {
        let wake = ctx.wake;
        let proc = AgentProcess::spawn(argv, &ctx.cwd, ENV_REMOVE, wake)?;
        // An agent inside WSL is told the same folder and bridge as WSL shows them (#5546).
        let (cwd, bridge) =
            if acp::runs_in_wsl(argv) { (acp::wsl_path(&ctx.cwd), ctx.bridge.as_ref().map(acp::McpBridge::in_wsl)) } else { (ctx.cwd, ctx.bridge) };
        let mut b = AcpBackend {
            proc,
            cwd,
            bridge,
            init: Value::Null,
            next_id: 0,
            pending: HashMap::new(),
            session: None,
            agent: String::new(),
            images: false,
            documents: false,
            auth: Vec::new(),
            permissions: HashMap::new(),
            next_key: 0,
            exited: false,
            config_pending: None,
            modern_config: false,
        };
        b.request(Pending::Initialize, "initialize", acp::initialize_params(env!("CARGO_PKG_VERSION")));
        Ok(b)
    }

    fn request(&mut self, what: Pending, method: &str, params: Value) {
        self.next_id += 1;
        self.pending.insert(self.next_id, what);
        self.proc.send(&acp::request(self.next_id, method, params));
    }

    fn new_session(&mut self) {
        self.config_pending = None;
        self.pending.retain(|_, request| *request != Pending::SetConfig);
        let params = acp::session_new_params(&self.cwd, self.bridge.as_ref(), &self.init);
        self.request(Pending::SessionNew, "session/new", params);
    }

    fn handle(&mut self, msg: Incoming, out: &mut Vec<Event>) {
        match msg {
            Incoming::Reply { id, result } => {
                let Some(what) = self.pending.remove(&id) else { return };
                match (what, result) {
                    (Pending::Initialize, Ok(v)) => {
                        self.agent = acp::agent_info(&v);
                        self.images = acp::takes_images(&v);
                        self.documents = acp::takes_documents(&v);
                        self.auth = acp::auth_methods(&v);
                        self.init = v;
                        self.new_session();
                    }
                    (Pending::SessionNew, Ok(v)) => match v.get("sessionId").and_then(Value::as_str) {
                        Some(s) => {
                            self.session = Some(s.to_string());
                            match acp::config_options(&v) {
                                acp::ConfigParse::Snapshot(options) => {
                                    self.modern_config = true;
                                    out.push(Event::ConfigOptions(options));
                                }
                                _ => {
                                    self.modern_config = false;
                                    if let Some((current, available)) = acp::modes(&v) {
                                        out.push(Event::Modes { current, available });
                                    }
                                    if let Some((current, available)) = acp::models(&v) {
                                        out.push(Event::Models { current, available });
                                    }
                                }
                            }
                            out.push(Event::Documents(self.documents));
                            out.push(Event::Ready { agent: self.agent.clone(), images: self.images });
                        }
                        None => out.push(Event::Error("session/new: no sessionId".into())),
                    },
                    (Pending::Prompt, Ok(v)) => {
                        let reason = v.get("stopReason").and_then(Value::as_str).unwrap_or("");
                        out.push(Event::TurnEnded { reason: if reason == "end_turn" { String::new() } else { reason.to_string() } });
                    }
                    // Signed in: try the session again.
                    (Pending::Authenticate, Ok(_)) => self.new_session(),
                    (Pending::SetMode | Pending::SetModel, Ok(_)) => {}
                    (Pending::SetConfig, Ok(v)) => {
                        if let Some((request_id, config_id)) = self.config_pending.take()
                            && request_id == id
                        {
                            let result = match acp::config_options(&v) {
                                acp::ConfigParse::Snapshot(o) => Ok(o),
                                _ => Err("The agent returned no valid configuration options.".into()),
                            };
                            out.push(Event::ConfigOptionFinished { id: config_id, result });
                        }
                    }
                    // `session/set_model` is still unstable in ACP: an agent can list `models` and
                    // still not serve it (#9970). It keeps its own model; that is no error to show.
                    (Pending::SetModel, Err((acp::METHOD_NOT_FOUND, message))) => {
                        log::debug!("agent does not serve session/set_model: {message}");
                    }
                    (what, Err((code, message))) => {
                        let config_error = what == Pending::SetConfig;
                        if what == Pending::SetConfig
                            && let Some((request_id, config_id)) = self.config_pending.take()
                            && request_id == id
                        {
                            out.push(Event::ConfigOptionFinished { id: config_id, result: Err(message.clone()) });
                        }
                        let auth = code == acp::AUTH_REQUIRED || message.to_ascii_lowercase().contains("auth");
                        if auth {
                            self.session = None;
                            self.modern_config = false;
                            self.config_pending = None;
                            self.pending.retain(|_, request| *request != Pending::SetConfig);
                            if what == Pending::Prompt {
                                out.push(Event::TurnEnded { reason: "auth".into() });
                            }
                            out.push(Event::AuthRequired { methods: self.auth.clone() });
                        } else if what == Pending::SetModel {
                            out.push(Event::ModelRejected(message));
                        } else if !config_error {
                            if what == Pending::Prompt {
                                out.push(Event::TurnEnded { reason: "error".into() });
                            }
                            out.push(Event::Error(message));
                        }
                    }
                }
            }
            Incoming::Text { thought, text } => out.push(Event::Text { thought, text }),
            Incoming::Tool { id, title, kind, status, input, output } => out.push(Event::Tool { id, title, kind, status, input, output }),
            Incoming::Plan(p) => out.push(Event::Plan(p)),
            Incoming::Permission { rpc, title, options } => {
                self.next_key += 1;
                self.permissions.insert(self.next_key, rpc);
                out.push(Event::Permission { key: self.next_key, title, options });
            }
            Incoming::AuthStatus(s) => out.push(Event::AuthStatus(s)),
            Incoming::ModeChanged(m) if !self.modern_config => out.push(Event::Modes { current: m, available: Vec::new() }),
            Incoming::ModelChanged(m) if !self.modern_config => out.push(Event::Models { current: m, available: Vec::new() }),
            Incoming::ConfigOptions { session, options } if self.session.as_deref() == Some(session.as_str()) => {
                out.push(Event::ConfigOptions(options))
            }
            Incoming::ModeChanged(_) | Incoming::ModelChanged(_) | Incoming::ConfigOptions { .. } => {}
            Incoming::Unsupported { rpc, method } => {
                self.proc.send(&acp::reply_error(rpc, acp::METHOD_NOT_FOUND, &format!("SolidCraft does not serve `{method}`")));
            }
            Incoming::Ignored => {}
        }
    }
}

impl ChatBackend for AcpBackend {
    fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        for _ in 0..512 {
            match self.proc.from_agent.try_recv() {
                Ok(FromAgent::Line(l)) => {
                    if let Some(m) = acp::parse(&l) {
                        self.handle(m, &mut out);
                    }
                }
                Ok(FromAgent::Exited(tail)) => {
                    self.exited = true;
                    self.session = None;
                    self.pending.clear();
                    self.config_pending = None;
                    out.push(Event::Exited { tail });
                    break;
                }
                Err(_) => break,
            }
        }
        out
    }

    fn prompt(&mut self, parts: &[PromptPart]) {
        if let Some(s) = self.session.clone() {
            self.request(Pending::Prompt, "session/prompt", acp::prompt_params(&s, parts));
        }
    }

    fn cancel(&mut self) {
        // Open permission questions are answered "cancelled" first, as the protocol asks.
        for (_, rpc) in self.permissions.drain() {
            self.proc.send(&acp::permission_reply(rpc, None));
        }
        if let Some(s) = &self.session {
            self.proc.send(&acp::notification("session/cancel", json!({"sessionId": s})));
        }
    }

    fn new_session(&mut self) {
        if self.pending.values().any(|p| *p == Pending::Prompt) {
            self.cancel();
        }
        self.session = None;
        self.modern_config = false;
        self.config_pending = None;
        self.pending.retain(|_, request| *request != Pending::SetConfig);
        self.new_session();
    }

    fn answer_permission(&mut self, key: u64, option: Option<&str>) {
        if let Some(rpc) = self.permissions.remove(&key) {
            self.proc.send(&acp::permission_reply(rpc, option));
        }
    }

    fn authenticate(&mut self, method: &str) {
        self.request(Pending::Authenticate, "authenticate", json!({"methodId": method}));
    }

    fn set_mode(&mut self, mode: &str) {
        if let Some(s) = self.session.clone() {
            self.request(Pending::SetMode, "session/set_mode", json!({"sessionId": s, "modeId": mode}));
        }
    }

    fn set_model(&mut self, model: &str) {
        if let Some(s) = self.session.clone() {
            self.request(Pending::SetModel, "session/set_model", acp::set_model_params(&s, model));
        }
    }

    fn set_config_option(&mut self, id: &str, value: &str) {
        if self.config_pending.is_some() {
            return;
        }
        if let Some(s) = self.session.clone() {
            self.request(Pending::SetConfig, "session/set_config_option", acp::set_config_option_params(&s, id, value));
            self.config_pending = Some((self.next_id, id.to_string()));
        }
    }

    fn pid(&self) -> Option<u32> {
        self.proc.pid()
    }

    fn shutdown(&mut self, wait: bool) {
        self.proc.shutdown(wait);
    }
}

/// Test connection: start the agent, say hello, open a session without tools, end it. What it
/// answered (its name and version), or what went wrong (it isn't signed in, it didn't answer).
pub fn test_agent(argv: &[String], cwd: &std::path::Path) -> Result<String, TestError> {
    let mut p = AgentProcess::spawn(argv, cwd, ENV_REMOVE, || {}).map_err(TestError::Other)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let wait_for = |p: &AgentProcess, id: u64| -> Result<Value, TestError> {
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match p.from_agent.recv_timeout(left) {
                Ok(FromAgent::Line(l)) => {
                    if let Some(Incoming::Reply { id: got, result }) = acp::parse(&l)
                        && got == id
                    {
                        return result.map_err(|(code, m)| {
                            if code == acp::AUTH_REQUIRED || m.to_ascii_lowercase().contains("auth") {
                                TestError::AuthRequired
                            } else {
                                TestError::Other(m)
                            }
                        });
                    }
                }
                Ok(FromAgent::Exited(tail)) => {
                    return Err(TestError::Other(tail.last().cloned().unwrap_or_else(|| crate::i18n::tr("ui.agent_chat_exited").to_string())));
                }
                Err(_) => return Err(TestError::Other(crate::i18n::tr("ui.ai_no_answer").to_string())),
            }
        }
    };
    p.send(&acp::request(1, "initialize", acp::initialize_params(env!("CARGO_PKG_VERSION"))));
    let init = wait_for(&p, 1);
    let session_cwd = if acp::runs_in_wsl(argv) { acp::wsl_path(cwd) } else { cwd.to_path_buf() };
    let result = init.and_then(|v| {
        p.send(&acp::request(2, "session/new", acp::session_new_params(&session_cwd, None, &v)));
        wait_for(&p, 2).map(|_| {
            let who = acp::agent_info(&v);
            crate::i18n::format("ui.ai_test_acp_ok", &[("agent", if who.is_empty() { "?" } else { &who })])
        })
    });
    p.shutdown(true);
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::agent_chat::backend::StartContext;

    fn snapshot(session: &str, value: &str) -> Value {
        json!({"sessionId":session,"configOptions":[{"id":"provider.model","category":"model","name":"Model","type":"select","currentValue":value,"options":[{"value":"fast","name":"Fast"},{"value":"smart","name":"Smart"}]}]})
    }

    #[test]
    fn config_updates_and_replies_are_isolated_to_the_active_session() {
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut backend = AcpBackend::start(&["sh".into(), "-c".into(), "cat >/dev/null".into()], ctx).unwrap();
        let mut events = Vec::new();
        backend.handle(Incoming::Reply { id: 1, result: Ok(json!({"agentInfo":{"name":"fake"}})) }, &mut events);
        backend.handle(Incoming::Reply { id: 2, result: Ok(snapshot("s1", "fast")) }, &mut events);
        assert_eq!(backend.session.as_deref(), Some("s1"));
        events.clear();
        backend.set_config_option("provider.model", "smart");
        let old_request = backend.config_pending.as_ref().map(|p| p.0).unwrap();
        ChatBackend::new_session(&mut backend);
        let new_session_request = backend.next_id;
        assert!(backend.config_pending.is_none());
        let old_notification = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"config_option_update","configOptions":[]}}}"#;
        backend.handle(acp::parse(old_notification).unwrap(), &mut events);
        assert!(events.is_empty());
        backend.handle(Incoming::Reply { id: new_session_request, result: Ok(snapshot("s2", "fast")) }, &mut events);
        assert_eq!(backend.session.as_deref(), Some("s2"));
        events.clear();
        backend.set_config_option("provider.model", "smart");
        let new_request = backend.config_pending.as_ref().map(|p| p.0).unwrap();
        backend.handle(Incoming::Reply { id: old_request, result: Ok(snapshot("s1", "smart")) }, &mut events);
        backend.handle(Incoming::Reply { id: old_request, result: Err((-32001, "old rejected request".into())) }, &mut events);
        backend.handle(acp::parse(old_notification).unwrap(), &mut events);
        assert!(events.is_empty(), "old requests and session notifications are ignored");
        assert_eq!(backend.config_pending.as_ref().map(|p| p.0), Some(new_request), "an old reply cannot clear the new session's pending request");
        let new_notification = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s2","update":{"sessionUpdate":"config_option_update","configOptions":[]}}}"#;
        backend.handle(acp::parse(new_notification).unwrap(), &mut events);
        assert!(matches!(events.as_slice(), [Event::ConfigOptions(options)] if options.is_empty()));
        assert_eq!(backend.config_pending.as_ref().map(|p| p.0), Some(new_request), "a notification is not an acknowledgement");
        events.clear();
        backend.handle(Incoming::Reply { id: new_request, result: Ok(snapshot("s2", "smart")) }, &mut events);
        assert!(backend.config_pending.is_none());
        assert!(matches!(events.as_slice(), [Event::ConfigOptionFinished { result: Ok(options), .. }] if options[0].current_value == "smart"));
        backend.shutdown(true);
    }

    /// A refused model is the picker's business, not an error of the chat; signing in stays as it was.
    #[test]
    fn a_refused_model_is_reported_as_such() {
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut backend = AcpBackend::start(&["sh".into(), "-c".into(), "cat >/dev/null".into()], ctx).unwrap();
        let mut events = Vec::new();
        backend.handle(Incoming::Reply { id: 1, result: Ok(json!({"agentInfo":{"name":"fake"}})) }, &mut events);
        backend.handle(Incoming::Reply { id: 2, result: Ok(json!({"sessionId":"s1"})) }, &mut events);
        events.clear();
        backend.set_model("zzz");
        backend.handle(Incoming::Reply { id: backend.next_id, result: Err((-32602, "no such model".into())) }, &mut events);
        assert!(matches!(events.as_slice(), [Event::ModelRejected(message)] if message == "no such model"), "{events:?}");
        events.clear();
        backend.set_model("fast");
        backend.handle(Incoming::Reply { id: backend.next_id, result: Ok(json!({})) }, &mut events);
        assert!(events.is_empty(), "{events:?}");
        backend.set_model("zzz");
        backend.handle(Incoming::Reply { id: backend.next_id, result: Err((acp::AUTH_REQUIRED, "sign in".into())) }, &mut events);
        assert!(matches!(events.as_slice(), [Event::AuthRequired { .. }]), "{events:?}");
        backend.shutdown(true);
    }
}
