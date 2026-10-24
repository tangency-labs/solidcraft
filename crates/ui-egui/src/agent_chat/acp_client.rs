//! The ACP backend (desktop only): starts the agent the user picked as a child process and holds
//! the protocol's side of the conversation (initialize, the session, prompts, permission questions,
//! sign-in). The panel sees only [`Event`]s.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::acp::{self, Incoming};
use super::backend::{ChatBackend, Event, PromptPart, StartContext};
use super::process::{AgentProcess, FromAgent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Initialize,
    SessionNew,
    Prompt,
    Authenticate,
    SetMode,
    SetModel,
}

/// Variables the agent doesn't inherit: a control token set for the app's own `--control` port
/// would otherwise reach the agent and outrank the chat endpoint's own token file.
const ENV_REMOVE: &[&str] = &["SOLIDCRAFT_CONTROL_TOKEN"];

pub struct AcpBackend {
    proc: AgentProcess,
    cwd: std::path::PathBuf,
    bridge: Option<acp::McpBridge>,
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
    model_option: Option<String>,
    selected_model: Option<String>,
}

impl AcpBackend {
    /// Start `argv` and say hello (`initialize`); the session follows when the agent answers.
    pub fn start(argv: &[String], ctx: StartContext) -> Result<AcpBackend, String> {
        let wake = ctx.wake;
        let proc = AgentProcess::spawn(argv, &ctx.cwd, ENV_REMOVE, wake)?;
        let mut b = AcpBackend {
            proc,
            cwd: ctx.cwd,
            bridge: ctx.bridge,
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
            model_option: None,
            selected_model: None,
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
        let params = acp::session_new_params(&self.cwd, self.bridge.as_ref());
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
                        self.new_session();
                    }
                    (Pending::SessionNew, Ok(v)) => match v.get("sessionId").and_then(Value::as_str) {
                        Some(s) => {
                            self.session = Some(s.to_string());
                            self.model_option = acp::model_config(&v).and_then(|c| c["id"].as_str()).map(str::to_string);
                            if let Some((current, available)) = acp::modes(&v) {
                                out.push(Event::Modes { current, available });
                            }
                            if let Some((current, available)) = acp::models(&v) {
                                out.push(Event::Models { current, available });
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
                    (Pending::SetModel, Ok(v)) => {
                        if let Some((current, available)) = acp::models(&v) {
                            self.selected_model = None;
                            out.push(Event::Models { current, available });
                        } else if let Some(current) = self.selected_model.take() {
                            out.push(Event::Models { current, available: Vec::new() });
                        }
                    }
                    (Pending::SetMode, Ok(_)) => {}
                    (what, Err((code, message))) => {
                        let auth = code == acp::AUTH_REQUIRED || message.to_ascii_lowercase().contains("auth");
                        if auth {
                            self.session = None;
                            if what == Pending::Prompt {
                                out.push(Event::TurnEnded { reason: "auth".into() });
                            }
                            out.push(Event::AuthRequired { methods: self.auth.clone() });
                        } else {
                            if what == Pending::Prompt {
                                out.push(Event::TurnEnded { reason: "error".into() });
                            }
                            out.push(Event::Error(message));
                        }
                    }
                }
            }
            Incoming::Text { thought, text } => out.push(Event::Text { thought, text }),
            Incoming::Tool { id, title, kind, status, output } => out.push(Event::Tool { id, title, kind, status, output }),
            Incoming::Plan(p) => out.push(Event::Plan(p)),
            Incoming::Permission { rpc, title, options } => {
                self.next_key += 1;
                self.permissions.insert(self.next_key, rpc);
                out.push(Event::Permission { key: self.next_key, title, options });
            }
            Incoming::AuthStatus(s) => out.push(Event::AuthStatus(s)),
            Incoming::ModeChanged(m) => out.push(Event::Modes { current: m, available: Vec::new() }),
            Incoming::ModelChanged(m) => out.push(Event::Models { current: m, available: Vec::new() }),
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
            self.selected_model = Some(model.to_string());
            if let Some(option) = self.model_option.clone() {
                self.request(Pending::SetModel, "session/set_config_option", json!({"sessionId": s, "configId": option, "value": model}));
            } else {
                self.request(Pending::SetModel, "session/set_model", acp::set_model_params(&s, model));
            }
        }
    }

    fn model_pending(&self) -> bool {
        self.pending.values().any(|p| *p == Pending::SetModel)
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
pub fn test_agent(argv: &[String], cwd: &std::path::Path) -> Result<String, String> {
    let mut p = AgentProcess::spawn(argv, cwd, ENV_REMOVE, || {})?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let wait_for = |p: &AgentProcess, id: u64| -> Result<Value, String> {
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match p.from_agent.recv_timeout(left) {
                Ok(FromAgent::Line(l)) => {
                    if let Some(Incoming::Reply { id: got, result }) = acp::parse(&l)
                        && got == id
                    {
                        return result.map_err(|(code, m)| {
                            if code == acp::AUTH_REQUIRED || m.to_ascii_lowercase().contains("auth") {
                                crate::i18n::tr("ui.agent_chat_state_auth").to_string()
                            } else {
                                m
                            }
                        });
                    }
                }
                Ok(FromAgent::Exited(tail)) => {
                    return Err(tail.last().cloned().unwrap_or_else(|| crate::i18n::tr("ui.agent_chat_exited").to_string()));
                }
                Err(_) => return Err(crate::i18n::tr("ui.ai_no_answer").to_string()),
            }
        }
    };
    p.send(&acp::request(1, "initialize", acp::initialize_params(env!("CARGO_PKG_VERSION"))));
    let init = wait_for(&p, 1);
    let result = init.and_then(|v| {
        p.send(&acp::request(2, "session/new", acp::session_new_params(cwd, None)));
        wait_for(&p, 2).map(|_| {
            let who = acp::agent_info(&v);
            crate::i18n::format("ui.ai_test_acp_ok", &[("agent", if who.is_empty() { "?" } else { &who })])
        })
    });
    p.shutdown(true);
    result
}
