//! SolidCraft's control channel: the loopback listeners (the app's port and the agent chat's own
//! endpoint) with their token files, served by the shared control server (`solidcraft_control`)
//! with what SolidCraft's requests mean ([`SolidCraft`]).

use std::net::TcpListener;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use solidcraft_control::server::{self, Hub, Protocol, Summary};
use solidcraft_control::{ControlRequest, auth};
use solidcraft_engine::MAX_LINE;
use solidcraft_engine::control_auth::APP;
use solidcraft_engine::control_pending;
use solidcraft_ui_egui::control_switch;

/// Wakes the app's event loop for a control request: eframe runs the app's logic pass for it at
/// once, without waiting for the compositor's next frame (#743). The proxy arrives when the
/// event loop starts (eframe's `event_loop_ready` hook); until then the requests ride the usual
/// `request_repaint` repaint.
#[derive(Clone, Default)]
pub struct Wake {
    proxy: std::sync::Arc<std::sync::Mutex<Option<eframe::EventLoopProxy<eframe::UserEvent>>>>,
}

impl Wake {
    /// Keep this launch's proxy; the event loop's `event_loop_ready` hook fills it once.
    pub fn new(proxy: std::sync::Arc<std::sync::Mutex<Option<eframe::EventLoopProxy<eframe::UserEvent>>>>) -> Self {
        Self { proxy }
    }

    /// The event loop started; remember its proxy.
    pub fn set(&self, proxy: eframe::EventLoopProxy<eframe::UserEvent>) {
        *self.proxy.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(proxy);
    }

    /// Run the app's logic pass now, so a request just sent is served without waiting for a GUI
    /// frame: a window the compositor has stopped giving frames to (off-screen, another
    /// workspace) would otherwise answer each request once per second (#743).
    fn run_logic_now(&self) {
        if let Some(proxy) = self.proxy.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
            let _ = proxy.send_event(eframe::UserEvent::RunLogicNow { viewport_id: eframe::egui::ViewportId::ROOT });
        }
    }
}

/// The token files this launch wrote (its control port, the agent chat's endpoint) and the
/// token in each, removed when the app ends unless another launch has written the file since.
static TOKEN_FILES: std::sync::Mutex<Vec<(std::path::PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

/// What SolidCraft's requests mean to the control server: commands by their ids, batches and
/// scripts by their steps, and which ones read, write a file or change the design.
struct SolidCraft;

impl Protocol for SolidCraft {
    fn summary(&self, method: &str, p: &Value) -> Summary {
        let command = match method {
            "engine.execute" | "command" => p.get("command").and_then(Value::as_str).and_then(solidcraft_engine::find_command).map(|c| c.id),
            _ => None,
        };
        let steps = match method {
            "engine.batch" | "engine.script" => p.get("commands").and_then(Value::as_array).map(Vec::len),
            _ => None,
        };
        Summary {
            what: control_pending::describe(method, p),
            effect: control_pending::effect(method, p),
            command,
            steps,
            long: method == "engine.batch",
        }
    }

    fn document(&self) -> &'static str {
        "design"
    }

    fn change_event(&self) -> &'static str {
        "design.changed"
    }

    fn revision_method(&self) -> &'static str {
        "document.revision"
    }

    fn docs(&self) -> &'static str {
        "docs/control-protocol.md"
    }

    /// `param_check`: the levels `engine.execute`, `engine.batch` and `engine.script` take as a
    /// request's own parameter check (#9184).
    fn capabilities(&self) -> Value {
        json!({"param_check": solidcraft_engine::schema::ParamCheck::ALL.map(|l| l.name())})
    }
}

/// The app's control server, for both of its listeners, so `control.running` sees every request.
fn hub(ctx: egui::Context, wake: Wake) -> Arc<Hub> {
    static HUB: OnceLock<Arc<Hub>> = OnceLock::new();
    Arc::clone(HUB.get_or_init(|| {
        Arc::new(Hub::new(
            SolidCraft,
            move || {
                ctx.request_repaint();
                wake.run_logic_now();
            },
            MAX_LINE,
        ))
    }))
}

/// Remove this launch's token files and discovery file (at exit), leaving any that another
/// launch has written since (#8794).
pub fn remove_token_file() {
    if let Ok(files) = TOKEN_FILES.lock() {
        for (path, token) in files.iter() {
            auth::remove_token_file(path, token);
        }
    }
    if let Some((port, token)) = APP_PORT.lock().ok().and_then(|p| p.clone()) {
        APP.remove_current_port(port, &token);
    }
}

/// The port the app's own channel listens on and its token, for removing the discovery file at exit.
static APP_PORT: std::sync::Mutex<Option<(u16, String)>> = std::sync::Mutex::new(None);

/// Open the app's control channel (at launch, or turned on in the settings): its listener, its
/// token file and the discovery file. Stopping it removes both files again.
pub fn start(listen_on: auth::Listen, ctx: egui::Context, wake: Wake) -> Result<control_switch::Started, control_switch::StartError> {
    use control_switch::StartError;
    let (tx, rx) = channel::<ControlRequest>();
    // Every connection must show the token before anything runs (docs/control-protocol.md).
    let Some(token) = APP.env_token().or_else(server::new_token) else {
        solidcraft_engine::note!("solidcraft: control server not started: no random token available");
        return Err(StartError::Failed("no random token available".into()));
    };
    let listener = match listen_on {
        auth::Listen::Exact(port) => TcpListener::bind(("127.0.0.1", port)),
        auth::Listen::Default => auth::bind_with_fallback(APP.default_port, auth::PORT_TRIES),
    };
    let listener = listener.map_err(|e| {
        solidcraft_engine::note!("solidcraft: control server failed to bind 127.0.0.1: {e}");
        match (listen_on, e.kind()) {
            // In use; Windows reports a port another program holds exclusively as denied.
            (auth::Listen::Exact(port), std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied) => StartError::PortTaken(port),
            (auth::Listen::Default, std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied) => StartError::PortTaken(APP.default_port),
            _ => StartError::Failed(e.to_string()),
        }
    })?;
    let (port, stop) =
        listen_on_listener(listener, token.clone(), tx, ctx, false, wake).ok_or_else(|| StartError::Failed("the listener did not start".into()))?;
    if matches!(listen_on, auth::Listen::Default) {
        solidcraft_engine::note!("solidcraft: control channel on the default port 127.0.0.1:{port} (Preferences > Assistant turns it off)");
    }
    // The discovery file: where `solidcraft-cli mcp` finds this app with no arguments.
    match APP.write_current_port(port) {
        Ok(_) => {
            if let Ok(mut p) = APP_PORT.lock() {
                *p = Some((port, token.clone()));
            }
        }
        Err(e) => solidcraft_engine::note!("solidcraft: could not write the control discovery file ({e})"),
    }
    let stop = Box::new(move || {
        stop.stop();
        if let Ok(mut p) = APP_PORT.lock()
            && p.as_ref().is_some_and(|(held, _)| *held == port)
        {
            *p = None;
        }
        APP.remove_current_port(port, &token);
        if let Some(path) = APP.token_path(port) {
            auth::remove_token_file(&path, &token);
            if let Ok(mut files) = TOKEN_FILES.lock() {
                files.retain(|(p, _)| *p != path);
            }
        }
        solidcraft_engine::note!("solidcraft: control channel on 127.0.0.1:{port} closed");
    });
    Ok(control_switch::Started { port, requests: rx, stop })
}

/// The agent chat's own endpoint for its agent's MCP server (docs/agent-chat.md): a free loopback
/// port with a fresh token of its own (never `SOLIDCRAFT_CONTROL_TOKEN`), written to the usual
/// token file, where the MCP server finds it, so the token never passes through the agent. Its
/// requests go to `tx`, where the chat's gate reads them; one may wait long for the user's Allow.
pub fn listen_agent(tx: Sender<ControlRequest>, ctx: egui::Context, wake: &Wake) -> Option<u16> {
    let Some(token) = server::new_token() else {
        solidcraft_engine::note!("solidcraft: agent chat endpoint not started: no random token available");
        return None;
    };
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(e) => {
            solidcraft_engine::note!("solidcraft: control server failed to bind 127.0.0.1: {e}");
            return None;
        }
    };
    // The chat's endpoint lives as long as the app.
    listen_on_listener(listener, token, tx, ctx, true, wake.clone()).map(|(port, _)| port)
}

/// Serve `listener` (loopback) with `token`; the port it listens on and how to stop it.
/// `patient`: every request may wait as long as a batch (a person decides on it).
fn listen_on_listener(
    listener: TcpListener,
    token: String,
    tx: Sender<ControlRequest>,
    ctx: egui::Context,
    patient: bool,
    wake: Wake,
) -> Option<(u16, server::Stop)> {
    let port = listener.local_addr().ok()?.port();
    match APP.token_path(port) {
        Some(path) => match APP.write_token_file(&path, &token) {
            Ok(()) => {
                solidcraft_engine::note!("solidcraft: control server listening on 127.0.0.1:{port} (token in {})", path.display());
                if let Ok(mut files) = TOKEN_FILES.lock() {
                    files.push((path, token.clone()));
                }
            }
            Err(e) => solidcraft_engine::note!(
                "solidcraft: control server listening on 127.0.0.1:{port}, but its token file {} could not be written ({e}); \
                 clients need SOLIDCRAFT_CONTROL_TOKEN",
                path.display()
            ),
        },
        None => {
            solidcraft_engine::note!(
                "solidcraft: control server listening on 127.0.0.1:{port}; no data folder for its token, clients need SOLIDCRAFT_CONTROL_TOKEN"
            )
        }
    }
    match hub(ctx, wake).listen_stoppable(listener, token, tx, patient) {
        Ok(stop) => Some((port, stop)),
        Err(e) => {
            solidcraft_engine::note!("solidcraft: control server not started: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #743: the wake's proxy arrives when the event loop starts. Clones share the launch's
    /// holder, and a wake without a proxy yet (the loop not started) is a no-op, not an error.
    #[test]
    fn the_wake_shares_its_proxy_holder() {
        let wake = Wake::default();
        wake.run_logic_now();
        let other = wake.clone();
        assert!(std::sync::Arc::ptr_eq(&wake.proxy, &other.proxy));
        assert!(wake.proxy.lock().unwrap().is_none());
    }

    use solidcraft_control::server::Waits;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    /// #3654: `control.running` answers on every connection, at once, with what any connection
    /// has the app running or queued (command ids, never parameters); a timed-out request stays
    /// listed until the app answers it, and an idle app lists nothing.
    #[test]
    fn every_connection_sees_what_the_app_is_running() {
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        // The window: one request at a time, as `drain_control` takes them; the export is held
        // until the test lets it go.
        let (gate_tx, gate_rx) = channel::<()>();
        std::thread::spawn(move || {
            for req in rx {
                req.started.store(true, Ordering::Relaxed);
                if req.method == "engine.execute" {
                    let _ = gate_rx.recv();
                }
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
        let waits = Waits { short: Duration::from_millis(100), long: Duration::from_secs(60), token: Waits::default().token };
        let hub = Arc::new(Hub::new(SolidCraft, || {}, MAX_LINE).with_waits(waits));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (tx, hub) = (tx.clone(), Arc::clone(&hub));
                std::thread::spawn(move || hub.serve(stream, token, tx, false));
            }
        });
        let open = || {
            let s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut out = s.try_clone().unwrap();
            let mut lines = BufReader::new(s);
            move |v: Value| -> Value {
                writeln!(out, "{v}").unwrap();
                let mut l = String::new();
                lines.read_line(&mut l).unwrap();
                serde_json::from_str(&l).unwrap()
            }
        };
        let running = |ask: &mut dyn FnMut(Value) -> Value, until: &dyn Fn(&[Value]) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let r = ask(json!({"id": 0, "method": "control.running", "token": token}));
                let rows = r["result"]["running"].as_array().cloned().unwrap_or_default();
                if until(&rows) || Instant::now() > deadline {
                    return (r, rows);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let mut watcher = open();
        let (r, rows) = running(&mut watcher, &|_| true);
        assert!(r["ok"] == true && rows.is_empty(), "idle: {r}");
        // What the app's control channel does beyond the methods every version has (#9184).
        let r = watcher(json!({"id": 0, "method": "control.capabilities", "token": token}));
        assert_eq!((&r["ok"], &r["result"]["param_check"]), (&json!(true), &json!(["off", "warn", "strict"])), "{r}");

        let mut export = open();
        let path = "/private/part.step";
        let params = json!({"command": "file.export", "params": {"path": path}});
        let r = export(json!({"id": 7, "method": "engine.execute", "params": params, "token": token}));
        assert_eq!(r["running"], true, "{r}");
        // A batch from another connection waits behind the export.
        let mut batch = open();
        let queued = std::thread::spawn(move || {
            let steps = json!([{"command": "solid.box"}, {"command": "solid.box"}]);
            batch(json!({"id": "b", "method": "engine.batch", "params": {"commands": steps}, "token": token}))
        });
        // The window may not have taken the export yet, even though it timed out.
        let (r, rows) = running(&mut watcher, &|rows| rows.len() == 2 && rows[0]["queued"] == false);
        assert_eq!(rows.len(), 2, "{r}");
        let (e, b) = (&rows[0], &rows[1]);
        assert_eq!(
            (&e["id"], &e["method"], &e["command"], &e["queued"]),
            (&json!(7), &json!("engine.execute"), &json!("file.export"), &json!(false)),
            "{r}"
        );
        assert!(e["running_s"].as_f64().is_some_and(|s| s >= 0.1), "{r}");
        assert_eq!((&b["id"], &b["method"], &b["steps"], &b["queued"]), (&json!("b"), &json!("engine.batch"), &json!(2), &json!(true)), "{r}");
        assert_ne!(e["connection"], b["connection"], "{r}");
        assert!(rows.iter().all(|row| row.get("self").is_none()), "{r}");
        assert!(!r.to_string().contains(path), "parameters stay with their connection: {r}");
        // The connection whose request timed out may ask too, and sees its own marked.
        let r = export(json!({"id": 8, "method": "control.running"}));
        let own: Vec<&Value> = r["result"]["running"].as_array().unwrap().iter().filter(|row| row["self"] == true).collect();
        assert_eq!(own.len(), 1, "{r}");
        assert_eq!(own[0]["command"], "file.export", "{r}");
        // So may it ask what the app can do: answered at once, not refused as busy.
        let r = export(json!({"id": 8, "method": "control.capabilities"}));
        assert_eq!(r["result"]["param_check"], json!(["off", "warn", "strict"]), "{r}");

        gate_tx.send(()).unwrap();
        assert_eq!(queued.join().unwrap()["result"], "engine.batch");
        let (r, rows) = running(&mut watcher, &|rows| rows.is_empty());
        assert!(rows.is_empty(), "done: {r}");
        // Asking `control.running` did not take the export's reply from its connection.
        let r = export(json!({"id": 9, "method": "control.pending"}));
        assert_eq!((&r["result"]["pending"], &r["result"]["done"]), (&json!(7), &json!(true)), "{r}");
    }

    /// A timed-out request that only reads is not said to "still apply", nor is one that only
    /// writes a file (an export, #1989); a change is, by name.
    #[test]
    fn timeout_text_tells_reads_from_changes() {
        let says =
            |method: &str, p: Value| server::timeout_message(&control_pending::describe(method, &p), control_pending::effect(method, &p), "design");
        let read = says("ui.inspect", json!({}));
        assert!(read.starts_with("timeout") && read.contains("only reads") && !read.contains("will still apply"), "{read}");
        let read = says("engine.execute", json!({"command": "sketch.inspect"}));
        assert!(read.contains("only reads") && read.contains("sketch.inspect"), "{read}");
        let change = says("engine.execute", json!({"command": "solid.fillet"}));
        assert!(change.starts_with("timeout") && change.contains("will still apply") && change.contains("solid.fillet"), "{change}");
        let export = says("engine.execute", json!({"command": "file.export", "params": {"path": "/tmp/wall.step"}}));
        assert!(export.contains("will still write the file") && !export.contains("apply") && export.contains("file.export"), "{export}");
        let save = says("engine.execute", json!({"command": "file.save"}));
        assert!(save.contains("will still write the file") && !save.contains("apply"), "{save}");
        let render = says("ui.render", json!({"path": "/tmp/a.png"}));
        assert!(render.contains("will still write the file"), "{render}");
    }
}
