//! The client against the shared server ([`crate::server::Hub`]) on an ephemeral loopback port.

use super::*;
use crate::ControlRequest;
use crate::server::{Hub, Protocol, Stop, Summary, Waits as ServerWaits};
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::channel;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn no_dir() -> Option<PathBuf> {
    None
}

fn no_env(_: &str) -> Option<OsString> {
    None
}

fn utc(secs: u64) -> String {
    secs.to_string()
}

/// An app with no token files and no environment: a client sends only the token it is given.
const APP: App = App { name: "Probe", data_dir: no_dir, env: no_env, utc, default_port: 0 };

/// An app whose requests all may change its document.
struct Probe;

impl Protocol for Probe {
    fn summary(&self, method: &str, _: &Value) -> Summary {
        Summary { what: format!("`{method}`"), effect: Effect::Changes, command: None, steps: None, long: false }
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
    fn capabilities(&self) -> Value {
        json!({})
    }
}

/// The app on `port` (0: an ephemeral one) behind [`TOKEN`]: its window answers each request after
/// `delay` with the request's method and parameters, and the server says a request is still
/// running after `wait`.
fn serve(port: u16, delay: Duration, wait: Duration) -> Stop {
    // A port just closed may take a moment to be free again on a loaded machine.
    let started = Instant::now();
    let mut bound = TcpListener::bind(("127.0.0.1", port));
    while bound.is_err() && started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
        bound = TcpListener::bind(("127.0.0.1", port));
    }
    let listener = bound.unwrap();
    let (tx, rx) = channel::<ControlRequest>();
    std::thread::spawn(move || {
        for req in rx {
            std::thread::sleep(delay);
            let _ = req.reply.send(json!({"ok": true, "result": {"method": req.method, "params": req.params}}));
        }
    });
    let waits = ServerWaits { short: wait, long: wait, token: Duration::from_secs(5) };
    let hub = Arc::new(Hub::new(Probe, || {}, 1 << 20).with_waits(waits));
    hub.listen_stoppable(listener, TOKEN.into(), tx, false).unwrap()
}

fn addr(port: u16) -> String {
    format!("127.0.0.1:{port}")
}

/// A port nothing listens on.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
fn a_call_is_one_round_trip_with_the_token() {
    let app = serve(0, Duration::ZERO, Duration::from_secs(10));
    let mut c = Connection::new(APP, &addr(app.port()), Some(TOKEN));
    assert!(!c.is_connected());
    let r = c.call(Request::new("document.inspect", json!({"a": 1}))).unwrap();
    assert_eq!(r, json!({"method": "document.inspect", "params": {"a": 1}}));
    assert!(c.is_connected());
    let serial = c.serial();
    // the next call goes on the same connection
    c.call(Request::new("document.inspect", json!({}))).unwrap();
    assert_eq!(c.serial(), serial);
    // `try_connect` sees the app there and leaves no connection open
    let mut fresh = Connection::new(APP, &addr(app.port()), Some(TOKEN));
    fresh.try_connect().unwrap();
    assert!(!fresh.is_connected());
}

#[test]
fn a_wrong_token_or_none_is_refused() {
    let app = serve(0, Duration::ZERO, Duration::from_secs(10));
    for token in [Some("fedcba9876543210fedcba9876543210"), None] {
        let mut c = Connection::new(APP, &addr(app.port()), token);
        match c.call(Request::new("document.inspect", json!({}))) {
            Err(CallError::Refused(e)) => assert!(e.starts_with("unauthorized"), "{token:?}: {e}"),
            other => panic!("{token:?}: {other:?}"),
        }
    }
}

#[test]
fn an_app_not_running_says_how_to_start_it() {
    let port = free_port();
    let mut c = Connection::new(APP, &addr(port), Some(TOKEN));
    let e = c.call(Request::new("document.inspect", json!({}))).unwrap_err();
    let CallError::NotRunning(m) = &e else { panic!("{e:?}") };
    assert!(m.starts_with(&format!("no Probe app is listening at 127.0.0.1:{port}")), "{m}");
    assert!(m.contains(&format!("`probe --control {port}`")), "{m}");
    assert_eq!(c.try_connect().unwrap_err().kind(), std::io::ErrorKind::NotConnected);
}

/// An app that goes away during a call is lost, not "not running"; the next call says it is not
/// running, and once it runs again the next call reaches it.
#[test]
fn an_app_gone_during_a_call_is_lost_and_reached_again_once_back() {
    let app = serve(0, Duration::ZERO, Duration::from_secs(10));
    let port = app.port();
    let mut c = Connection::new(APP, &addr(port), Some(TOKEN));
    c.call(Request::new("document.inspect", json!({}))).unwrap();
    app.stop();
    let e = c.call(Request::new("document.inspect", json!({}))).unwrap_err();
    let CallError::Lost(m) = &e else { panic!("{e:?}") };
    assert!(m.contains("may have quit") && m.contains("Inspect the design") && m.contains(&format!("`probe --control {port}`")), "{m}");
    assert!(matches!(c.call(Request::new("document.inspect", json!({}))), Err(CallError::NotRunning(_))));
    let _back = serve(port, Duration::ZERO, Duration::from_secs(10));
    assert_eq!(c.call(Request::new("ui.view", json!({}))).unwrap()["method"], json!("ui.view"));
}

/// A server that reads a request and closes the connection without a reply: the call is lost
/// (after one more try on a fresh connection), and the reply is never taken for "not running".
#[test]
fn a_connection_closed_without_a_reply_is_lost() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            let _ = BufReader::new(stream).read_line(&mut line);
        }
    });
    let mut c = Connection::new(APP, &addr(port), Some(TOKEN));
    let e = c.call(Request::new("document.inspect", json!({}))).unwrap_err();
    assert!(matches!(e, CallError::Lost(_)), "{e:?}");
    assert!(!c.is_connected());
}

/// A request the app says is still running is waited for through `control.pending`, on the same
/// connection, and its own reply is the call's result.
#[test]
fn a_request_still_running_is_waited_for_through_pending() {
    let app = serve(0, Duration::from_millis(400), Duration::from_millis(50));
    let mut c = Connection::new(APP, &addr(app.port()), Some(TOKEN));
    c.set_waits(Waits { pending: Duration::from_secs(10), settle: Duration::from_secs(10), poll: Duration::from_millis(5) });
    let r = c.call(Request::new("solid.slow", json!({"n": 2}))).unwrap();
    assert_eq!(r, json!({"method": "solid.slow", "params": {"n": 2}}));
    assert!(!c.has_outstanding());
    assert_eq!(c.pending(), json!({"pending": null}));
}

/// Past the wait, the call says the request is still running, and `pending` later has its end.
#[test]
fn a_request_still_running_past_the_wait_is_reported_and_its_end_kept() {
    let app = serve(0, Duration::from_millis(400), Duration::from_millis(20));
    let mut c = Connection::new(APP, &addr(app.port()), Some(TOKEN));
    c.set_waits(Waits { pending: Duration::from_millis(30), settle: Duration::from_secs(10), poll: Duration::from_millis(5) });
    let mut slow = Request::new("solid.slow", json!({}));
    slow.what = "`solid.slow` (now)".into();
    let e = c.call(slow).unwrap_err();
    let CallError::StillRunning(m) = &e else { panic!("{e:?}") };
    assert!(m.starts_with(STILL_RUNNING) && m.contains("`solid.slow` (now)") && m.contains("will still apply"), "{m}");
    assert!(c.has_outstanding());
    let end = c.pending();
    assert_eq!(end["done"], json!(true), "{end}");
    assert_eq!(end["reply"]["result"]["method"], json!("solid.slow"), "{end}");
}
