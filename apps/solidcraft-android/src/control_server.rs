//! Loopback JSON-lines control server: one request per line, one reply per line.
//! Agents and tests drive the app through it (adapted from CADCraft, MIT OR Apache-2.0).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_engine::MAX_LINE;
use solidcraft_engine::control_auth;
use solidcraft_ui_egui::ControlRequest;

/// Connections served at once, so a flood must not be able to spawn threads without end.
const MAX_CONNECTIONS: usize = 16;
/// Lines that are not JSON before the connection is closed, so garbage cannot loop forever.
const MAX_BAD_LINES: usize = 8;

static OPEN: AtomicUsize = AtomicUsize::new(0);
/// The token files this launch wrote (its `--control` port, the agent chat's endpoint), removed
/// when the app ends.
static TOKEN_FILES: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A new random token: 32 bytes from the operating system, as hex.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Remove this launch's token file (at exit).
pub fn remove_token_file() {
    if let Ok(files) = TOKEN_FILES.lock() {
        for p in files.iter() {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn start(port: u16, ctx: egui::Context) -> Receiver<ControlRequest> {
    let (tx, rx) = channel::<ControlRequest>();
    // Every connection must show the token before anything runs (docs/control-protocol.md).
    match control_auth::env_token().or_else(new_token) {
        Some(token) => {
            listen(port, token, tx, ctx, false);
        }
        None => log::error!("solidcraft: control server not started: no random token available"),
    }
    rx
}

/// The agent chat's own endpoint for its agent's MCP server (docs/agent-chat.md): a free loopback
/// port with a fresh token of its own (never `SOLIDCRAFT_CONTROL_TOKEN`), written to the usual
/// token file, where the MCP server finds it, so the token never passes through the agent. Its
/// requests go to `tx`, where the chat's gate reads them; one may wait long for the user's Allow.
pub fn listen_agent(tx: Sender<ControlRequest>, ctx: egui::Context) -> Option<u16> {
    let Some(token) = new_token() else {
        log::error!("solidcraft: agent chat endpoint not started: no random token available");
        return None;
    };
    listen(0, token, tx, ctx, true)
}

/// Listen on `port` (0: a free port the system picks) with `token`; the port it listens on.
/// `patient`: every request may wait as long as a batch (a person decides on it).
fn listen(port: u16, token: String, tx: Sender<ControlRequest>, ctx: egui::Context, patient: bool) -> Option<u16> {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            log::error!("solidcraft: control server failed to bind 127.0.0.1:{port}: {e}");
            return None;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    match control_auth::token_path(port) {
        Some(path) => match control_auth::write_token_file(&path, &token) {
            Ok(()) => {
                log::error!("solidcraft: control server listening on 127.0.0.1:{port} (token in {})", path.display());
                if let Ok(mut files) = TOKEN_FILES.lock() {
                    files.push(path);
                }
            }
            Err(e) => log::error!(
                "solidcraft: control server listening on 127.0.0.1:{port}, but its token file {} could not be written ({e}); \
                 clients need SOLIDCRAFT_CONTROL_TOKEN",
                path.display()
            ),
        },
        None => {
            log::error!(
                "solidcraft: control server listening on 127.0.0.1:{port}; no data folder for its token, clients need SOLIDCRAFT_CONTROL_TOKEN"
            )
        }
    }
    let token = std::sync::Arc::new(token);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if OPEN.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                OPEN.fetch_sub(1, Ordering::Relaxed);
                let mut refused = stream;
                let _ = writeln!(refused, "{}", json!({"ok": false, "error": "too many connections"}));
                continue;
            }
            let tx = tx.clone();
            let ctx = ctx.clone();
            let token = token.clone();
            std::thread::spawn(move || {
                serve(stream, &token, tx, ctx, patient);
                OPEN.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Some(port)
}

/// One line without its newline, or `None` at the end of the stream. A client that sends
/// gigabytes with no newline would otherwise make this thread allocate until the process is
/// killed, so the line is refused as soon as it passes `max`.
fn read_line(r: &mut impl BufRead, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let (done, used) = {
            let available = r.fill_buf()?;
            if available.is_empty() {
                return Ok((!line.is_empty()).then_some(line));
            }
            match available.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    line.extend_from_slice(available.get(..i).unwrap_or(&[]));
                    (true, i + 1)
                }
                None => {
                    line.extend_from_slice(available);
                    (false, available.len())
                }
            }
        };
        r.consume(used);
        if done {
            return Ok(Some(line));
        }
        if line.len() > max {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "request too large"));
        }
    }
}

/// The reply to a connection that didn't show the token; the connection is then closed.
fn unauthorized() -> Value {
    json!({"ok": false, "error": "unauthorized: send the control token as \"token\" (see docs/control-protocol.md)"})
}

/// Refuse the connection: the reply, then the end of what we send, then what the client had
/// already sent read and dropped (a little, briefly). Closing a socket with unread input resets
/// the connection, and a reset can reach the client before it has read the refusal.
fn refuse(out: &mut TcpStream, reader: &mut BufReader<TcpStream>) {
    use std::io::Read;
    let _ = writeln!(out, "{}", unauthorized());
    let _ = out.shutdown(std::net::Shutdown::Write);
    let _ = reader.get_ref().set_read_timeout(Some(Duration::from_millis(500)));
    let _ = std::io::copy(&mut reader.by_ref().take(64 * 1024), &mut std::io::sink());
}

/// How long a request waits for the app's reply: most requests, and a batch (or every request,
/// on a patient connection).
#[derive(Clone, Copy)]
struct Waits {
    short: Duration,
    long: Duration,
}

const WAITS: Waits = Waits { short: Duration::from_secs(60), long: Duration::from_secs(3600) };

/// A request that timed out but is still running in the app (#1618): its id and method, and where
/// its reply will land.
struct Pending {
    id: Value,
    method: String,
    reply: Receiver<Value>,
}

fn serve(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context, patient: bool) {
    serve_with(stream, token, tx, ctx, patient, WAITS);
}

fn serve_with(stream: TcpStream, token: &str, tx: Sender<ControlRequest>, ctx: egui::Context, patient: bool, waits: Waits) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    let mut reader = BufReader::new(read);
    let mut bad = 0usize;
    let mut authenticated = false;
    let mut pending: Option<Pending> = None;
    loop {
        let line = match read_line(&mut reader, MAX_LINE) {
            Ok(Some(l)) => String::from_utf8_lossy(&l).into_owned(),
            Ok(None) => break,
            Err(e) => {
                let _ = writeln!(out, "{}", json!({"ok": false, "error": e.to_string()}));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                // The first request must carry the token; later ones on the same connection may.
                if !authenticated {
                    match msg.get("token").and_then(Value::as_str) {
                        Some(t) if control_auth::tokens_match(t, token) => authenticated = true,
                        _ => {
                            refuse(&mut out, &mut reader);
                            break;
                        }
                    }
                }
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                log::info!("control: {method}");
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                // A request that timed out may have finished since (#1618).
                let late = pending.as_ref().and_then(|p| match p.reply.try_recv() {
                    Ok(reply) => Some(reply),
                    Err(TryRecvError::Disconnected) => Some(json!({"ok": false, "error": "the app ended the request without a reply"})),
                    Err(TryRecvError::Empty) => None,
                });
                let mut r = if method == "control.pending" {
                    match (pending.take(), late) {
                        (None, _) => json!({"ok": true, "result": {"pending": null}}),
                        (Some(p), Some(reply)) => json!({"ok": true, "result": {"pending": p.id, "method": p.method, "done": true, "reply": reply}}),
                        (Some(p), None) => {
                            let r = json!({"ok": true, "result": {"pending": p.id, "method": p.method, "done": false}});
                            pending = Some(p);
                            r
                        }
                    }
                } else if let Some(p) = pending.as_ref().filter(|_| late.is_none()) {
                    // Nothing more is queued behind it, to run unseen when it ends.
                    json!({"ok": false, "busy": true, "error": format!("busy: request {} (`{}`) is still running; ask `control.pending` for its result", p.id, p.method)})
                } else {
                    if late.is_some() {
                        // Finished, and the client moved on without asking: it is dropped.
                        pending = None;
                    }
                    // A batch replaces many short requests: the whole of it may take long (#743).
                    let wait = if patient || method == "engine.batch" { waits.long } else { waits.short };
                    let (req, rrx) = ControlRequest::new(method.clone(), params);
                    if tx.send(req).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                    match rrx.recv_timeout(wait) {
                        Ok(r) => r,
                        Err(_) => {
                            let r = json!({"ok": false, "running": true, "error": format!("timeout: `{method}` is still running in the app and will still apply; ask `control.pending` for its result")});
                            pending = Some(Pending { id: id.clone(), method, reply: rrx });
                            r
                        }
                    }
                };
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(_) if !authenticated => {
                refuse(&mut out, &mut reader);
                break;
            }
            Err(e) => {
                bad += 1;
                json!({"ok": false, "error": format!("bad JSON: {e}")})
            }
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
        if bad >= MAX_BAD_LINES {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1618: a request that outlasts its wait says it is still running; the connection keeps its
    /// reply for `control.pending`, and refuses other requests as `busy` until it lands, so none
    /// is queued to run unseen behind it.
    #[test]
    fn a_timed_out_request_can_be_asked_for_later() {
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        std::thread::spawn(move || {
            for req in rx {
                seen.lock().unwrap().push(req.method.clone());
                if req.method == "slow" {
                    std::thread::sleep(Duration::from_millis(400));
                }
                let _ = req.reply.send(json!({"ok": true, "result": req.method}));
            }
        });
        let waits = Waits { short: Duration::from_millis(100), long: Duration::from_millis(100) };
        std::thread::spawn(move || {
            if let Some(stream) = listener.incoming().flatten().next() {
                serve_with(stream, token, tx, egui::Context::default(), false, waits);
            }
        });
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut out = s.try_clone().unwrap();
        let mut lines = BufReader::new(s);
        let mut ask = |v: Value| -> Value {
            writeln!(out, "{v}").unwrap();
            let mut l = String::new();
            lines.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap()
        };
        let r = ask(json!({"id": 1, "method": "slow", "token": token}));
        assert!(r["ok"] == false && r["running"] == true && r["error"].as_str().is_some_and(|e| e.starts_with("timeout")), "{r}");
        let r = ask(json!({"id": 2, "method": "ui.view"}));
        assert!(r["busy"] == true && r["id"] == 2, "{r}");
        let r = ask(json!({"id": 3, "method": "control.pending"}));
        assert_eq!((&r["result"]["pending"], &r["result"]["done"]), (&json!(1), &json!(false)), "{r}");
        std::thread::sleep(Duration::from_millis(600));
        let r = ask(json!({"id": 4, "method": "control.pending"}));
        assert_eq!(
            (&r["result"]["pending"], &r["result"]["done"], &r["result"]["reply"]["result"]),
            (&json!(1), &json!(true), &json!("slow")),
            "{r}"
        );
        let r = ask(json!({"id": 5, "method": "control.pending"}));
        assert_eq!(r["result"]["pending"], Value::Null, "{r}");
        let r = ask(json!({"id": 6, "method": "ui.view"}));
        assert_eq!((&r["ok"], &r["result"]), (&json!(true), &json!("ui.view")), "{r}");
        assert_eq!(*ran.lock().unwrap(), ["slow", "ui.view"], "the busy request never reached the app");
    }

    /// #205: a connection runs nothing until a request carries the launch's token; a wrong or
    /// missing token, or garbage, is refused and the connection closed. Once shown, the token
    /// need not be repeated on that connection.
    #[test]
    fn requests_run_only_after_the_token() {
        use std::io::Read;
        let token = "0123456789abcdef0123456789abcdef";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel::<ControlRequest>();
        let ran = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = ran.clone();
        std::thread::spawn(move || {
            for req in rx {
                seen.lock().unwrap().push(req.method.clone());
                let _ = req.reply.send(json!({"ok": true, "result": null}));
            }
        });
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || serve(stream, token, tx, egui::Context::default(), false));
            }
        });
        let session = |lines: &[String]| {
            let mut s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
            let _ = s.shutdown(std::net::Shutdown::Write);
            let mut text = String::new();
            let _ = s.read_to_string(&mut text);
            text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>()
        };
        for first in [
            json!({"id": 1, "method": "app.quit", "params": {"force": true}}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123456789abcdef0123456789abcdee"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": "0123"}).to_string(),
            json!({"id": 1, "method": "app.quit", "token": 7}).to_string(),
            "not json".to_string(),
        ] {
            let replies = session(&[first.clone(), json!({"id": 2, "method": "app.quit", "token": token}).to_string()]);
            assert_eq!(replies.len(), 1, "{first}: closed after refusing: {replies:?}");
            assert!(replies[0]["error"].as_str().is_some_and(|e| e.starts_with("unauthorized")), "{first}: {replies:?}");
        }
        assert!(ran.lock().unwrap().is_empty(), "nothing ran without the token");
        let replies = session(&[
            json!({"id": 1, "method": "ui.inspect", "token": token}).to_string(),
            json!({"id": 2, "method": "engine.commands"}).to_string(),
        ]);
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert!(replies.iter().all(|r| r["ok"] == true), "{replies:?}");
        assert_eq!(*ran.lock().unwrap(), ["ui.inspect", "engine.commands"]);
    }

    /// A request longer than the cap is refused instead of allocated.
    #[test]
    fn lines_past_the_cap_are_refused() {
        let flood = vec![b'A'; MAX_LINE + 2];
        let mut r = BufReader::new(flood.as_slice());
        let e = read_line(&mut r, MAX_LINE).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("request too large"), "{e}");
    }

    /// Ordinary framing: lines split on newlines, blank lines kept, no trailing newline needed.
    #[test]
    fn lines_split_on_newlines() {
        let text = b"{\"a\":1}\n\n{\"b\":2}\nlast".to_vec();
        let mut r = BufReader::new(text.as_slice());
        let mut got = Vec::new();
        while let Some(line) = read_line(&mut r, MAX_LINE).unwrap() {
            got.push(String::from_utf8(line).unwrap());
        }
        assert_eq!(got, vec!["{\"a\":1}", "", "{\"b\":2}", "last"]);
        assert!(read_line(&mut r, MAX_LINE).unwrap().is_none());
    }

    /// A line that spans several reads of the underlying stream still arrives whole.
    #[test]
    fn lines_longer_than_one_buffer_arrive_whole() {
        let long = "x".repeat(200_000);
        let text = format!("{long}\n{long}\n").into_bytes();
        let mut r = BufReader::with_capacity(64, text.as_slice());
        assert_eq!(read_line(&mut r, MAX_LINE).unwrap().unwrap().len(), long.len());
        assert_eq!(read_line(&mut r, MAX_LINE).unwrap().unwrap().len(), long.len());
        assert!(read_line(&mut r, MAX_LINE).unwrap().is_none());
        // The same line against a smaller cap is refused.
        let mut r = BufReader::with_capacity(64, text.as_slice());
        assert!(read_line(&mut r, 1000).is_err());
    }
}
