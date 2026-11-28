//! The ACP backend (desktop only): starts the agent the user picked as a child process and holds
//! the protocol's side of the conversation (initialize, the session, prompts, permission questions,
//! sign-in). The panel sees only [`Event`]s.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

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

/// Give a cancelled prompt time to finish before disconnecting an unresponsive agent.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// Variables the agent doesn't inherit: a control token set for the app's own `--control` port
/// would otherwise reach the agent and outrank the chat endpoint's own token file.
const ENV_REMOVE: &[&str] = &["SOLIDCRAFT_CONTROL_TOKEN"];

/// Cleanup finishes before a replacement starts. Neither operation holds the window thread.
struct Restart {
    result: Receiver<Result<AgentProcess, String>>,
    worker: std::thread::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
}

impl Restart {
    fn stop(self, wait: bool) {
        self.cancelled.store(true, Ordering::Release);
        if wait {
            let _ = self.worker.join();
            if let Ok(Ok(mut proc)) = self.result.try_recv() {
                proc.shutdown(true);
            }
        }
        // Otherwise dropping the receiver disposes of a replacement already queued or sent
        // later. AgentProcess::drop ends its process tree off the window thread.
    }
}

pub struct AcpBackend {
    proc: Option<AgentProcess>,
    argv: Vec<String>,
    /// Native path for spawning, before the ACP path below is translated for WSL.
    process_cwd: std::path::PathBuf,
    wake: Arc<dyn Fn() + Send + Sync>,
    restart: Option<Restart>,
    restart_error: Option<String>,
    retired_turn: bool,
    cwd: std::path::PathBuf,
    bridge: Option<acp::McpBridge>,
    /// The agent's `initialize` reply: which routes the system note can take into a session.
    init: Value,
    /// The system note the next session gets ([`acp::system_note`]).
    note: String,
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
    cancelling: Option<(u64, Instant)>,
    exited: bool,
    config_pending: Option<(u64, String)>,
    modern_config: bool,
    /// The agent is Claude Code's adapter, as its command says ([`acp::is_claude_adapter`]),
    /// decided once when it starts: only then are its read tools on.
    claude: bool,
    /// Its own tool calls in the running turn ([`acp::ClaudeCalls`]).
    calls: acp::ClaudeCalls,
}

impl AcpBackend {
    /// Start `argv` and say hello (`initialize`); the session follows when the agent answers.
    pub fn start(argv: &[String], ctx: StartContext) -> Result<AcpBackend, String> {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::from(ctx.wake);
        let process_wake = wake.clone();
        let proc = AgentProcess::spawn(argv, &ctx.cwd, ENV_REMOVE, move || process_wake())?;
        let process_cwd = ctx.cwd.clone();
        // An agent inside WSL is told the same folder and bridge as WSL shows them (#5546).
        let (cwd, bridge) =
            if acp::runs_in_wsl(argv) { (acp::wsl_path(&ctx.cwd), ctx.bridge.as_ref().map(acp::McpBridge::in_wsl)) } else { (ctx.cwd, ctx.bridge) };
        let mut b = AcpBackend {
            proc: Some(proc),
            argv: argv.to_vec(),
            process_cwd,
            wake,
            restart: None,
            restart_error: None,
            retired_turn: false,
            cwd,
            bridge,
            init: Value::Null,
            note: acp::SYSTEM_NOTE.to_string(),
            next_id: 0,
            pending: HashMap::new(),
            session: None,
            agent: String::new(),
            images: false,
            documents: false,
            auth: Vec::new(),
            permissions: HashMap::new(),
            next_key: 0,
            cancelling: None,
            exited: false,
            config_pending: None,
            modern_config: false,
            claude: acp::is_claude_adapter(argv),
            calls: acp::ClaudeCalls::default(),
        };
        b.request(Pending::Initialize, "initialize", acp::initialize_params(env!("CARGO_PKG_VERSION")));
        Ok(b)
    }

    fn request(&mut self, what: Pending, method: &str, params: Value) {
        if self.proc.is_none() {
            return;
        }
        self.next_id += 1;
        self.pending.insert(self.next_id, what);
        self.send(&acp::request(self.next_id, method, params));
    }

    fn send(&self, message: &Value) {
        if let Some(proc) = &self.proc {
            proc.send(message);
        }
    }

    fn new_session(&mut self) {
        self.config_pending = None;
        self.pending.retain(|_, request| *request != Pending::SetConfig);
        let params = acp::session_new_params(&self.cwd, self.bridge.as_ref(), &self.init, &self.note, self.claude);
        self.request(Pending::SessionNew, "session/new", params);
    }

    fn handle(&mut self, msg: Incoming, out: &mut Vec<Event>) {
        match msg {
            Incoming::Reply { id, result } => {
                let Some(what) = self.pending.remove(&id) else { return };
                if self.cancelling.is_some_and(|(request, _)| request == id) {
                    self.cancelling = None;
                }
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
                            // New chat has opened successfully: retire the old turn before Ready.
                            // Its late reply and cancellation deadline must not end the new turn.
                            if std::mem::take(&mut self.retired_turn) {
                                out.push(Event::TurnEnded { reason: "cancelled".into() });
                            }
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
            Incoming::Tool { id, title, kind, status, input, output, claude_tool } => {
                let unasked = self.calls.update(&id, claude_tool.as_deref(), status);
                out.push(Event::Tool { id, title, kind, status, input, output });
                if let Some(tool) = unasked {
                    out.push(Event::Unasked { tool });
                }
            }
            // Accept updates only during the matching session's prompt. A prompt reply earlier in
            // this poll can start a queued turn when the chat reads the events.
            Incoming::Plan { session, entries }
                if self.session.as_deref() == Some(session.as_str()) && self.pending.values().any(|p| *p == Pending::Prompt) =>
            {
                out.push(Event::Plan(entries));
            }
            Incoming::Plan { .. } => {}
            Incoming::Permission { rpc, title, kind, options, reads, writes, plan, call, claude_tool } => {
                let claude_tool = self.calls.question(call.as_deref(), claude_tool.as_deref());
                self.next_key += 1;
                self.permissions.insert(self.next_key, rpc);
                out.push(Event::Permission { key: self.next_key, title, kind, options, reads, writes, plan, claude_tool });
            }
            Incoming::AuthStatus(s) => out.push(Event::AuthStatus(s)),
            Incoming::ModeChanged(m) if !self.modern_config => out.push(Event::Modes { current: m, available: Vec::new() }),
            Incoming::ModelChanged(m) if !self.modern_config => out.push(Event::Models { current: m, available: Vec::new() }),
            Incoming::ConfigOptions { session, options } if self.session.as_deref() == Some(session.as_str()) => {
                out.push(Event::ConfigOptions(options))
            }
            Incoming::ModeChanged(_) | Incoming::ModelChanged(_) | Incoming::ConfigOptions { .. } => {}
            Incoming::Unsupported { rpc, method } => {
                self.send(&acp::reply_error(rpc, acp::METHOD_NOT_FOUND, &format!("SolidCraft does not serve `{method}`")));
            }
            Incoming::Ignored => {}
        }
    }

    fn finish_exit(&mut self, tail: Vec<String>) -> Event {
        self.exited = true;
        self.session = None;
        self.pending.clear();
        self.cancelling = None;
        self.config_pending = None;
        Event::Exited { tail }
    }

    fn poll_at(&mut self, now: Instant) -> Vec<Event> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        if let Some(error) = self.restart_error.take() {
            out.push(self.finish_exit(vec![error]));
            return out;
        }
        if let Some(restart) = &self.restart {
            match restart.result.try_recv() {
                Ok(Ok(proc)) => {
                    self.restart = None;
                    self.proc = Some(proc);
                    self.request(Pending::Initialize, "initialize", acp::initialize_params(env!("CARGO_PKG_VERSION")));
                }
                Err(TryRecvError::Empty) => return out,
                result => {
                    self.restart = None;
                    let error = match result {
                        Ok(Err(error)) => error,
                        _ => solidcraft_ui_text::tr("ui.agent_chat_exited").to_string(),
                    };
                    out.push(self.finish_exit(vec![error]));
                    return out;
                }
            }
        }
        for _ in 0..512 {
            let Some(proc) = &self.proc else { break };
            match proc.from_agent.try_recv() {
                Ok(FromAgent::Line(l)) => {
                    if let Some(m) = acp::parse(&l) {
                        self.handle(m, &mut out);
                    }
                }
                Ok(FromAgent::Exited(tail)) => {
                    out.push(self.finish_exit(tail));
                    break;
                }
                Err(_) => break,
            }
        }
        // Drain replies first: an acknowledgement already waiting must keep the connection.
        if self.cancelling.is_some_and(|(_, started)| now.saturating_duration_since(started) >= CANCEL_GRACE) {
            self.shutdown(false);
            out.push(self.finish_exit(vec![solidcraft_ui_text::tr("ui.agent_chat_cancel_timeout").to_string()]));
        }
        out
    }
}

impl ChatBackend for AcpBackend {
    fn poll(&mut self) -> Vec<Event> {
        self.poll_at(Instant::now())
    }

    fn prompt(&mut self, parts: &[PromptPart]) {
        self.calls.clear();
        if let Some(s) = self.session.clone() {
            self.request(Pending::Prompt, "session/prompt", acp::prompt_params(&s, parts));
        }
    }

    fn cancel(&mut self) {
        // Open permission questions are answered "cancelled" first, as the protocol asks.
        for (_, rpc) in self.permissions.drain() {
            if let Some(proc) = &self.proc {
                proc.send(&acp::permission_reply(rpc, None));
            }
        }
        if let Some(s) = &self.session {
            self.send(&acp::notification("session/cancel", json!({"sessionId": s})));
            // Repeated Stop presses must not postpone the fallback. An idle session needs none.
            if self.cancelling.is_none() {
                self.cancelling =
                    self.pending.iter().find_map(|(&id, request)| (*request == Pending::Prompt).then_some(id)).map(|id| (id, Instant::now()));
            }
        }
    }

    fn new_session(&mut self) {
        if self.restart.is_some() || self.proc.is_none() {
            return;
        }
        self.retired_turn = self.pending.values().any(|p| *p == Pending::Prompt);
        self.cancel();
        let Some(mut proc) = self.proc.take() else { return };
        self.session = None;
        self.init = Value::Null;
        self.auth.clear();
        self.pending.clear();
        self.cancelling = None;
        self.modern_config = false;
        self.config_pending = None;
        self.exited = false;
        let (tx, result) = channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let (argv, cwd, wake) = (self.argv.clone(), self.process_cwd.clone(), self.wake.clone());
        match std::thread::Builder::new().name("agent-chat-restart".into()).spawn(move || {
            proc.shutdown(true);
            if stop.load(Ordering::Acquire) {
                return;
            }
            let process_wake = wake.clone();
            let next = AgentProcess::spawn(&argv, &cwd, ENV_REMOVE, move || process_wake());
            let _ = tx.send(next);
            wake();
        }) {
            Ok(worker) => self.restart = Some(Restart { result, worker, cancelled }),
            Err(error) => {
                self.restart_error = Some(error.to_string());
                (self.wake)();
            }
        }
    }

    fn set_instructions(&mut self, text: &str) {
        self.note = acp::system_note(text);
    }

    fn set_allow_paths(&mut self, paths: &[std::path::PathBuf]) {
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.allow_paths = paths.to_vec();
        }
    }

    fn answer_permission(&mut self, key: u64, option: Option<&str>) {
        if let Some(rpc) = self.permissions.remove(&key) {
            self.send(&acp::permission_reply(rpc, option));
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
        self.proc.as_ref().and_then(AgentProcess::pid)
    }

    fn claude_code(&self) -> bool {
        self.claude
    }

    fn shutdown(&mut self, wait: bool) {
        if let Some(restart) = self.restart.take() {
            restart.stop(wait);
        }
        if let Some(proc) = self.proc.as_mut() {
            proc.shutdown(wait);
        }
    }
}

impl Drop for AcpBackend {
    fn drop(&mut self) {
        self.shutdown(false);
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
                    return Err(TestError::Other(tail.last().cloned().unwrap_or_else(|| solidcraft_ui_text::tr("ui.agent_chat_exited").to_string())));
                }
                Err(_) => return Err(TestError::Other(solidcraft_ui_text::tr("ui.ai_no_answer").to_string())),
            }
        }
    };
    p.send(&acp::request(1, "initialize", acp::initialize_params(env!("CARGO_PKG_VERSION"))));
    let init = wait_for(&p, 1);
    let session_cwd = if acp::runs_in_wsl(argv) { acp::wsl_path(cwd) } else { cwd.to_path_buf() };
    let result = init.and_then(|v| {
        p.send(&acp::request(2, "session/new", acp::session_new_params(&session_cwd, None, &v, acp::SYSTEM_NOTE, acp::is_claude_adapter(argv))));
        wait_for(&p, 2).map(|_| {
            let who = acp::agent_info(&v);
            solidcraft_ui_text::format("ui.ai_test_acp_ok", &[("agent", if who.is_empty() { "?" } else { &who })])
        })
    });
    p.shutdown(true);
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::agent_chat::backend::StartContext;

    fn alive(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            return !stat.rsplit(')').next().is_some_and(|s| s.trim_start().starts_with('Z'));
        }
        let Ok(pid) = i32::try_from(pid) else { return false };
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
    }

    #[test]
    fn a_new_chat_releases_the_previous_agents_helpers() {
        let dir = std::env::temp_dir().join(format!("sc-acp-session-cleanup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("helpers.log");
        let script = r#"
log="$1"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"agentInfo":{"name":"fake"}}}\n' "$id" ;;
    *'"method":"session/new"'*) sleep 300 & helper=$!; printf '%s\n' "$helper" >> "$log"; printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"%s"}}\n' "$id" "$helper" ;;
  esac
done
"#;
        let argv = vec!["sh".into(), "-c".into(), script.into(), "fake-agent".into(), log.to_string_lossy().into_owned()];
        let ctx = StartContext { cwd: dir.clone(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut backend = AcpBackend::start(&argv, ctx).unwrap();
        let ready = |backend: &mut AcpBackend| {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let events = backend.poll();
                if events.iter().any(|event| matches!(event, Event::Ready { .. })) {
                    break;
                }
                assert!(!events.iter().any(|event| matches!(event, Event::Exited { .. } | Event::Error(_))), "{events:?}");
                assert!(Instant::now() < deadline, "the fake agent did not become ready");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        ready(&mut backend);
        for _ in 0..2 {
            ChatBackend::new_session(&mut backend);
            ChatBackend::new_session(&mut backend);
            ready(&mut backend);
        }
        let helpers: Vec<u32> = std::fs::read_to_string(&log).unwrap().lines().map(|line| line.parse().unwrap()).collect();
        assert_eq!(helpers.len(), 3);
        let deadline = Instant::now() + Duration::from_secs(8);
        while helpers[..2].iter().copied().any(alive) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let living: Vec<_> = helpers.iter().copied().filter(|pid| alive(*pid)).collect();
        backend.shutdown(true);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(living, helpers[2..], "only the current chat's helper may remain alive");
    }

    fn ready_backend() -> Result<AcpBackend, String> {
        let ctx = StartContext { cwd: std::env::temp_dir(), bridge: None, tools: None, wake: Box::new(|| {}) };
        let mut backend = AcpBackend::start(&["sh".into(), "-c".into(), "cat >/dev/null".into()], ctx)?;
        let mut events = Vec::new();
        backend.handle(Incoming::Reply { id: 1, result: Ok(json!({"agentInfo":{"name":"fake"}})) }, &mut events);
        backend.handle(Incoming::Reply { id: 2, result: Ok(json!({"sessionId":"s1"})) }, &mut events);
        Ok(backend)
    }

    #[test]
    fn plan_updates_belong_to_the_active_session_and_pending_prompt() {
        let mut backend = ready_backend().unwrap();
        let mut events = Vec::new();
        let plan = |session: &str, content: &str, status: &str| {
            let message = json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": session, "update": {"sessionUpdate": "plan", "entries": [{"content": content, "status": status}]}
                }
            });
            acp::parse(&message.to_string()).unwrap()
        };

        backend.handle(plan("s1", "Before prompt", "in_progress"), &mut events);
        backend.prompt(&[PromptPart::Text("make a plate".into())]);
        let prompt = backend.next_id;
        backend.handle(plan("old-session", "Wrong session", "in_progress"), &mut events);
        assert!(events.is_empty());

        backend.handle(plan("s1", "Make plate", "in_progress"), &mut events);
        backend.handle(plan("s1", "Make plate", "completed"), &mut events);
        backend.handle(Incoming::Reply { id: prompt, result: Ok(json!({"stopReason":"end_turn"})) }, &mut events);
        backend.handle(plan("s1", "Late update from the previous prompt", "in_progress"), &mut events);
        assert!(
            matches!(events.as_slice(), [Event::Plan(active), Event::Plan(completed), Event::TurnEnded { .. }]
                if active.as_slice() == [("Make plate".to_owned(), "in_progress".to_owned())]
                    && completed.as_slice() == [("Make plate".to_owned(), "completed".to_owned())]),
            "{events:?}"
        );

        events.clear();
        backend.prompt(&[PromptPart::Text("add a hole".into())]);
        backend.handle(plan("s1", "Drill hole", "in_progress"), &mut events);
        assert!(matches!(events.as_slice(), [Event::Plan(active)] if active.as_slice() == [("Drill hole".to_owned(), "in_progress".to_owned())]));
        backend.shutdown(true);
    }

    /// Wait for native cleanup, then supply the replacement's initialize reply.
    fn initialize_restarted_backend(backend: &mut AcpBackend) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while backend.restart.is_some() {
            assert!(backend.poll().is_empty());
            assert!(Instant::now() < deadline, "restart did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut events = Vec::new();
        backend.handle(Incoming::Reply { id: backend.next_id, result: Ok(json!({"agentInfo":{"name":"fake"}})) }, &mut events);
        assert!(events.is_empty());
    }

    #[test]
    fn a_failed_restart_releases_the_old_process_and_reports_one_exit() {
        let mut backend = ready_backend().unwrap();
        let old = backend.pid().unwrap();
        // The agent's executable can disappear after the first connection.
        backend.argv = vec![std::env::temp_dir().join(format!("missing-sc-agent-{}", std::process::id())).to_string_lossy().into_owned()];
        ChatBackend::new_session(&mut backend);
        let deadline = Instant::now() + Duration::from_secs(20);
        let events = loop {
            let events = backend.poll();
            if !events.is_empty() {
                break events;
            }
            assert!(Instant::now() < deadline, "a failed restart must not stay Starting");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(matches!(events.as_slice(), [Event::Exited { tail }] if !tail.is_empty()), "{events:?}");
        assert!(!alive(old));
        assert!(backend.pid().is_none());
        assert!(backend.poll().is_empty());
    }

    #[test]
    fn disconnecting_during_restart_disposes_of_the_replacement() {
        for queued in [false, true] {
            for wait in [false, true] {
                let mut backend = ready_backend().unwrap();
                let old = backend.pid().unwrap();
                ChatBackend::new_session(&mut backend);
                let mut pids = vec![old];
                if queued {
                    // Let the replacement arrive without installing it in the backend.
                    let restart = backend.restart.as_ref().unwrap();
                    let deadline = Instant::now() + Duration::from_secs(20);
                    while !restart.worker.is_finished() {
                        assert!(Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    // Inspect and put the result back so shutdown owns its cleanup.
                    let replacement = restart.result.try_recv().unwrap().unwrap();
                    pids.push(replacement.pid().unwrap());
                    let (tx, rx) = channel();
                    assert!(tx.send(Ok(replacement)).is_ok());
                    backend.restart.as_mut().unwrap().result = rx;
                }
                backend.shutdown(wait);
                assert!(backend.restart.is_none() && backend.pid().is_none());
                if !wait {
                    let deadline = Instant::now() + Duration::from_secs(8);
                    while pids.iter().copied().any(alive) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                assert!(pids.iter().copied().all(|pid| !alive(pid)), "disconnect left a process alive: {pids:?}");
            }
        }
    }

    #[test]
    fn an_unresponsive_cancel_disconnects_the_agent() {
        let mut backend = ready_backend().unwrap();
        backend.prompt(&[PromptPart::Text("stalled search".into())]);
        backend.cancel();
        let started = backend.cancelling.unwrap().1;
        assert!(backend.poll_at(started + CANCEL_GRACE - Duration::from_millis(1)).is_empty());
        backend.cancel();
        let events = backend.poll_at(started + CANCEL_GRACE);
        assert!(matches!(events.as_slice(), [Event::Exited { .. }]), "Stop must not wait indefinitely: {events:?}");
        assert!(backend.pid().is_none());
        assert!(backend.session.is_none() && backend.pending.is_empty());
        assert!(backend.poll().is_empty(), "the exit is reported only once");
    }

    #[test]
    fn a_cancelled_prompt_reply_keeps_the_connection() {
        for reply in [Ok(json!({"stopReason":"cancelled"})), Err((-32603, "operation aborted".into()))] {
            let mut backend = ready_backend().unwrap();
            backend.prompt(&[PromptPart::Text("search".into())]);
            let request = backend.next_id;
            backend.cancel();
            let mut events = Vec::new();
            backend.handle(Incoming::Reply { id: request, result: reply }, &mut events);
            assert!(events.iter().any(|e| matches!(e, Event::TurnEnded { .. })));
            assert!(backend.poll_at(Instant::now() + CANCEL_GRACE).is_empty());
            assert!(backend.pid().is_some() && !backend.exited);
            backend.shutdown(true);
        }
    }

    #[test]
    fn cancelling_an_idle_session_does_not_disconnect_it() {
        let mut backend = ready_backend().unwrap();
        backend.cancel();
        assert!(backend.poll_at(Instant::now() + CANCEL_GRACE).is_empty());
        assert!(backend.pid().is_some() && !backend.exited);
        backend.shutdown(true);
    }

    #[test]
    fn a_new_session_retires_the_old_cancelled_prompt() {
        let mut backend = ready_backend().unwrap();
        backend.prompt(&[PromptPart::Text("old search".into())]);
        let old_prompt = backend.next_id;
        backend.cancel();
        ChatBackend::new_session(&mut backend);
        initialize_restarted_backend(&mut backend);
        let mut events = Vec::new();
        backend.handle(Incoming::Reply { id: backend.next_id, result: Ok(json!({"sessionId":"s2"})) }, &mut events);
        assert!(matches!(events.first(), Some(Event::TurnEnded { reason }) if reason == "cancelled"));
        assert!(matches!(events.last(), Some(Event::Ready { .. })));
        backend.prompt(&[PromptPart::Text("new search".into())]);
        assert!(backend.poll_at(Instant::now() + CANCEL_GRACE).is_empty(), "a new session has no stale cancellation deadline");
        backend.cancel();
        events.clear();
        backend.handle(Incoming::Reply { id: old_prompt, result: Ok(json!({"stopReason":"cancelled"})) }, &mut events);
        assert!(events.is_empty(), "an old reply must not end the new turn or clear its cancellation deadline");
        assert!(matches!(backend.poll_at(Instant::now() + CANCEL_GRACE).as_slice(), [Event::Exited { .. }]));
    }

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
        initialize_restarted_backend(&mut backend);
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
