//! The navigation bridge: a 3D input device's own program (a homemade device's driver app) moves
//! the view over a loopback connection, the way a 3D mouse driver hands its axes to the programs
//! it supports, so the device emulates no mouse and the pointer stays the user's.
//!
//! The device app listens on `127.0.0.1:<port>`; with Preferences › Navigation › "Navigate with a
//! device app" on (off by default), SolidCraft connects to it, says hello and reads one JSON
//! object per line (docs/3d-mouse.md). The reader thread only sums the motion; the egui thread
//! hands it to the view once per frame (`space_mouse::bridge_frame`). A device the app says it
//! reads is left to it by the 3D mouse reader ([`Claim`]), so the view doesn't move twice.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use egui::Context;
use serde_json::{Value, json};
use solidcraft_ui_egui::SolveApp;
use solidcraft_ui_egui::space_mouse::{BRIDGE_VIEWS, BridgeMotion, bridge_frame, bridge_step};

/// The longest line read; a longer one is skipped to its end.
const MAX_LINE: usize = 4096;
/// Standard views waiting for the next frame; past this many, more are a flood and dropped.
const MAX_VIEWS: usize = 8;
/// The longest name a device app may give for itself or its device, in characters.
const MAX_NAME: usize = 64;
/// How long a connection attempt waits.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// How long a read waits for data before the reader looks at the preferences again.
const READ_WAKE: Duration = Duration::from_millis(250);
/// The wait before connecting again when no device app answered or one went away.
const RETRY_AFTER: Duration = Duration::from_secs(1);
/// In zoom mode, a motion of this many points along Y is one zoom step: with 8 % a step, the 1 %
/// a point that a zoom drag moves.
const POINTS_PER_ZOOM_STEP: f64 = 8.0;

/// What the device's motion drives, as the device app last said (`mode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Orbit,
    Pan,
    Zoom,
}

/// What the reader thread and the egui thread share.
#[derive(Debug, Default)]
struct Shared {
    /// The port the preferences ask to connect to; none while the bridge is off.
    wanted: Option<u16>,
    /// The device app connected, by the name it gave ("?" until it gives one).
    connected: Option<String>,
    /// The device the connected app reads, as it named it: the 3D mouse reader leaves that one alone.
    device: Option<String>,
    /// Motion since the egui thread last took it.
    motion: BridgeMotion,
    /// Standard views asked for since then.
    views: Vec<String>,
}

fn lock(shared: &Mutex<Shared>) -> Option<MutexGuard<'_, Shared>> {
    shared.lock().ok()
}

/// A name a device app gave, without control characters and cut to [`MAX_NAME`]; none when empty.
fn name(value: Option<&Value>) -> Option<String> {
    let name: String = value?.as_str()?.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// A number of a message, finite and in range; none when it isn't one.
fn number(msg: &Value, key: &str) -> f64 {
    bridge_step(msg.get(key).and_then(Value::as_f64).unwrap_or(0.0))
}

/// Takes one line from the device app into `shared`. Lines that aren't a message this version
/// knows are ignored. Returns whether the view has something new to show.
fn consume(line: &[u8], mode: &mut Mode, shared: &mut Shared) -> bool {
    let Ok(msg) = serde_json::from_slice::<Value>(line) else { return false };
    let add = |shared: &mut Shared, motion: BridgeMotion| {
        shared.motion = shared.motion.plus(motion);
        !motion.is_still()
    };
    match msg.get("t").and_then(Value::as_str) {
        Some("axis") => {
            let (x, y) = (number(&msg, "x"), number(&msg, "y"));
            let motion = match mode {
                Mode::Orbit => BridgeMotion { orbit: [x, y], ..Default::default() },
                Mode::Pan => BridgeMotion { pan: [x, y], ..Default::default() },
                // Up zooms in, as a zoom drag does.
                Mode::Zoom => BridgeMotion { zoom: -y / POINTS_PER_ZOOM_STEP, ..Default::default() },
            };
            add(shared, motion)
        }
        Some("zoom") => add(shared, BridgeMotion { zoom: number(&msg, "d"), ..Default::default() }),
        Some("mode") => {
            let on = msg.get("on").and_then(Value::as_bool).unwrap_or(false);
            *mode = match msg.get("mode").and_then(Value::as_str) {
                Some("pan") if on => Mode::Pan,
                Some("zoom") if on => Mode::Zoom,
                _ => Mode::Orbit,
            };
            false
        }
        Some("view") => {
            let view = msg.get("name").and_then(Value::as_str).and_then(|n| BRIDGE_VIEWS.iter().find(|v| **v == n));
            match view {
                Some(view) if shared.views.len() < MAX_VIEWS => {
                    shared.views.push((*view).to_string());
                    true
                }
                _ => false,
            }
        }
        Some("hello") => {
            shared.connected = name(msg.get("server")).or_else(|| shared.connected.clone());
            shared.device = name(msg.get("device"));
            true
        }
        _ => false,
    }
}

/// Splits what arrives into lines, across reads. A line longer than [`MAX_LINE`] is dropped up to
/// its newline, so a peer that never sends one can't make the reader hold more than that.
#[derive(Default)]
struct Lines {
    line: Vec<u8>,
    skipping: bool,
}

impl Lines {
    fn feed(&mut self, mut data: &[u8], mut each: impl FnMut(&[u8])) {
        while !data.is_empty() {
            let end = data.iter().position(|b| *b == b'\n');
            let (part, rest) = match end {
                Some(i) => (data.get(..i).unwrap_or_default(), data.get(i + 1..).unwrap_or_default()),
                None => (data, &[][..]),
            };
            if !self.skipping {
                let room = (MAX_LINE + 1).saturating_sub(self.line.len());
                self.line.extend_from_slice(part.get(..room.min(part.len())).unwrap_or_default());
                if self.line.len() > MAX_LINE {
                    self.line.clear();
                    self.skipping = true;
                }
            }
            if end.is_some() {
                if !self.skipping {
                    each(self.line.strip_suffix(b"\r").unwrap_or(&self.line));
                }
                self.line.clear();
                self.skipping = false;
            }
            data = rest;
        }
    }
}

/// What SolidCraft says first, so the device app knows which program it is driving.
fn hello() -> Value {
    json!({"t": "hello", "app": "solidcraft", "version": env!("CARGO_PKG_VERSION")})
}

/// Reads one connection until the device app closes it, it fails, or the preferences no longer
/// ask for this port; then forgets what it had.
fn serve(mut stream: TcpStream, port: u16, ctx: &Context, shared: &Mutex<Shared>) {
    // Without a read timeout the reader couldn't notice the bridge being switched off.
    if stream.set_read_timeout(Some(READ_WAKE)).is_err() || writeln!(stream, "{}", hello()).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    log::info!("navigation bridge: connected to 127.0.0.1:{port}");
    if let Some(mut shared) = lock(shared) {
        shared.connected = Some("?".into());
    }
    ctx.request_repaint();
    let (mut lines, mut mode) = (Lines::default(), Mode::default());
    let mut chunk = [0_u8; 4096];
    while lock(shared).is_some_and(|shared| shared.wanted == Some(port)) {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => {
                let mut news = false;
                if let Some(mut shared) = lock(shared) {
                    lines.feed(chunk.get(..size).unwrap_or_default(), |line| news |= consume(line, &mut mode, &mut shared));
                }
                if news {
                    ctx.request_repaint();
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {}
            Err(error) => {
                log::info!("navigation bridge: connection lost: {error}");
                break;
            }
        }
    }
    if let Some(mut shared) = lock(shared) {
        *shared = Shared { wanted: shared.wanted, ..Shared::default() };
    }
    ctx.request_repaint();
}

/// Connects to the device app while the preferences ask for it, again whenever it goes away.
fn reader(ctx: Context, shared: Arc<Mutex<Shared>>) {
    loop {
        let Some(port) = lock(&shared).and_then(|shared| shared.wanted) else {
            std::thread::sleep(READ_WAKE);
            continue;
        };
        if let Ok(stream) = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), CONNECT_TIMEOUT) {
            serve(stream, port, &ctx, &shared);
        }
        std::thread::sleep(RETRY_AFTER);
    }
}

/// Which 3D mouse the connected device app reads, for the 3D mouse reader to leave alone. Only
/// the Windows and macOS readers know a device's strings to compare.
#[derive(Clone)]
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
pub struct Claim(Arc<Mutex<Shared>>);

#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
impl Claim {
    /// Whether a device with these manufacturer and product strings is the one the connected
    /// device app reads (its name appears in one, ignoring case).
    pub fn claims(&self, strings: &[String]) -> bool {
        let Some(device) = lock(&self.0).and_then(|shared| shared.device.clone()) else { return false };
        let device = device.to_lowercase();
        strings.iter().any(|s| s.to_lowercase().contains(&device))
    }
}

pub struct NavBridge {
    shared: Arc<Mutex<Shared>>,
}

impl NavBridge {
    pub fn start(ctx: &Context) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (worker_ctx, worker_state) = (ctx.clone(), Arc::clone(&shared));
        if let Err(error) = std::thread::Builder::new().name("solidcraft-nav-bridge".into()).spawn(move || reader(worker_ctx, worker_state)) {
            log::warn!("navigation bridge: reader could not start: {error}");
        }
        Self { shared }
    }

    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    pub fn claim(&self) -> Claim {
        Claim(Arc::clone(&self.shared))
    }

    /// Each frame: passes the preferences to the reader, and the motion and views since the last
    /// frame to the view. Motion that arrived while the window had no focus is dropped with it.
    pub fn tick(&mut self, app: &mut SolveApp, ctx: &Context) {
        let wanted = Some(app.preferences.nav_bridge_port).filter(|port| app.preferences.nav_bridge_enabled && *port != 0);
        let Some((motion, views, connected)) = lock(&self.shared).map(|mut shared| {
            shared.wanted = wanted;
            (std::mem::take(&mut shared.motion), std::mem::take(&mut shared.views), shared.connected.clone())
        }) else {
            return;
        };
        app.space_mouse.bridge = connected.filter(|_| wanted.is_some());
        if !motion.is_still() || !views.is_empty() {
            bridge_frame(app, ctx, motion, &views);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::time::Instant;

    fn feed(shared: &mut Shared, mode: &mut Mode, text: &str) -> bool {
        let mut news = false;
        Lines::default().feed(text.as_bytes(), |line| news |= consume(line, mode, shared));
        news
    }

    #[test]
    fn axis_motion_orbits_pans_or_zooms_as_the_mode_says() {
        let (mut shared, mut mode) = (Shared::default(), Mode::default());
        assert!(feed(&mut shared, &mut mode, "{\"t\":\"axis\",\"x\":3,\"y\":-2}\n{\"t\":\"axis\",\"x\":1.5,\"y\":0}\n"));
        assert_eq!(shared.motion, BridgeMotion { orbit: [4.5, -2.0], ..Default::default() });
        feed(&mut shared, &mut mode, "{\"t\":\"mode\",\"mode\":\"pan\",\"on\":true}\n{\"t\":\"axis\",\"x\":5,\"y\":6}\n");
        assert_eq!(mode, Mode::Pan);
        assert_eq!(shared.motion.pan, [5.0, 6.0]);
        feed(&mut shared, &mut mode, "{\"t\":\"mode\",\"mode\":\"zoom\",\"on\":true}\n{\"t\":\"axis\",\"x\":0,\"y\":-16}\n");
        assert_eq!(shared.motion.zoom, 2.0, "up zooms in, 8 points a step");
        // Off, whichever mode it names, is back to orbit.
        feed(&mut shared, &mut mode, "{\"t\":\"mode\",\"mode\":\"pan\",\"on\":false}\n");
        assert_eq!(mode, Mode::Orbit);
        // The ring's steps add up with the zoom mode's.
        feed(&mut shared, &mut mode, "{\"t\":\"zoom\",\"d\":-0.5}\n");
        assert_eq!(shared.motion.zoom, 1.5);
    }

    #[test]
    fn views_are_queued_by_their_known_names_only() {
        let (mut shared, mut mode) = (Shared::default(), Mode::default());
        assert!(feed(&mut shared, &mut mode, "{\"t\":\"view\",\"name\":\"iso\"}\n{\"t\":\"view\",\"name\":\"fit\"}\n"));
        assert!(!feed(&mut shared, &mut mode, "{\"t\":\"view\",\"name\":\"ISO\"}\n{\"t\":\"view\",\"name\":\"../etc\"}\n{\"t\":\"view\"}\n"));
        assert_eq!(shared.views, ["iso", "fit"]);
        let flood = "{\"t\":\"view\",\"name\":\"top\"}\n".repeat(100);
        feed(&mut shared, &mut mode, &flood);
        assert_eq!(shared.views.len(), MAX_VIEWS);
    }

    #[test]
    fn hostile_lines_never_move_the_view_out_of_range() {
        let (mut shared, mut mode) = (Shared::default(), Mode::default());
        let lines = [
            "not json",
            "{\"t\":\"axis\",\"x\":\"5\",\"y\":null}",
            "{\"t\":\"axis\",\"x\":1e308,\"y\":-1e308}",
            "{\"t\":\"axis\",\"x\":1e999}",
            "{\"t\":\"zoom\",\"d\":[1]}",
            "{\"t\":42}",
            "[1,2,3]",
            "{\"t\":\"mode\",\"mode\":{\"x\":1},\"on\":\"yes\"}",
            "\u{0}\u{1}\u{2}",
        ];
        for line in lines {
            Lines::default().feed(format!("{line}\n").as_bytes(), |l| {
                consume(l, &mut mode, &mut shared);
            });
        }
        let limit = solidcraft_ui_egui::space_mouse::BRIDGE_MAX_STEP;
        let values = shared.motion.orbit.into_iter().chain(shared.motion.pan).chain([shared.motion.zoom]);
        for value in values {
            assert!(value.is_finite() && value.abs() <= limit, "{value}");
        }
        assert_eq!(mode, Mode::Orbit);
        // Invalid UTF-8 is no message either.
        assert!(!consume(&[0xff, 0xfe, b'{', b'}'], &mut mode, &mut shared));
    }

    #[test]
    fn lines_split_across_reads_and_long_ones_are_skipped_to_their_end() {
        let mut got = Vec::new();
        let mut lines = Lines::default();
        for part in ["{\"t\":", "\"zoom\"}\r\n{\"a\"", ":1}\n"] {
            lines.feed(part.as_bytes(), |line| got.push(String::from_utf8_lossy(line).to_string()));
        }
        assert_eq!(got, ["{\"t\":\"zoom\"}", "{\"a\":1}"]);
        got.clear();
        let long = vec![b'x'; MAX_LINE * 3];
        lines.feed(&long, |line| got.push(String::from_utf8_lossy(line).to_string()));
        lines.feed(&long, |line| got.push(String::from_utf8_lossy(line).to_string()));
        assert!(lines.line.len() <= MAX_LINE + 1, "never holds more than one line");
        lines.feed(b"tail\nnext\n", |line| got.push(String::from_utf8_lossy(line).to_string()));
        assert_eq!(got, ["next"], "the long line is dropped to its newline, the next one read");
    }

    #[test]
    fn the_device_app_names_itself_and_the_device_it_leaves_alone() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let claim = Claim(Arc::clone(&shared));
        let strings = ["STMicroelectronics".to_string(), "MAKMOUSE 3D".to_string()];
        assert!(!claim.claims(&strings), "nothing is claimed before an app says so");
        let mut mode = Mode::default();
        if let Ok(mut shared) = shared.lock() {
            let hello = format!("{{\"t\":\"hello\",\"server\":\"Dev\\u0007App {}\",\"device\":\"makmouse\"}}\n", "x".repeat(200));
            feed(&mut shared, &mut mode, &hello);
            assert_eq!(shared.connected.as_deref().map(|n| n.chars().count()), Some(MAX_NAME));
            assert!(shared.connected.as_deref().is_some_and(|n| n.starts_with("DevApp")));
        }
        assert!(claim.claims(&strings), "the name matches ignoring case");
        assert!(!claim.claims(&["3Dconnexion".to_string(), "SpaceMouse Compact".to_string()]), "another 3D mouse stays the reader's");
    }

    /// Waits until `check` holds, or fails after a few seconds.
    fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_connection_says_hello_reads_the_stream_and_ends_when_the_bridge_is_switched_off() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Mutex::new(Shared { wanted: Some(port), ..Default::default() }));
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || serve(client, port, &Context::default(), &shared))
        };
        let mut hello_line = String::new();
        BufReader::new(server.try_clone().unwrap()).read_line(&mut hello_line).unwrap();
        let hello: Value = serde_json::from_str(&hello_line).unwrap();
        assert_eq!((hello["t"].as_str(), hello["app"].as_str()), (Some("hello"), Some("solidcraft")));
        server
            .write_all(b"{\"t\":\"hello\",\"server\":\"MAKMOUSE\",\"device\":\"MAKMOUSE\"}\n{\"t\":\"axis\",\"x\":2,\"y\":1}\n{\"t\":\"view\",\"name\":\"top\"}\n")
            .unwrap();
        wait_for("the stream", || shared.lock().is_ok_and(|s| s.motion.orbit == [2.0, 1.0] && s.views == ["top"]));
        assert_eq!(shared.lock().unwrap().connected.as_deref(), Some("MAKMOUSE"));
        // Switched off: the connection ends and what it had is forgotten.
        shared.lock().unwrap().wanted = None;
        worker.join().unwrap();
        let after = shared.lock().unwrap();
        assert!(after.connected.is_none() && after.device.is_none() && after.motion.is_still() && after.views.is_empty());
    }

    #[test]
    fn a_closed_connection_forgets_the_app() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Mutex::new(Shared { wanted: Some(port), ..Default::default() }));
        let client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let (server, _) = listener.accept().unwrap();
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || serve(client, port, &Context::default(), &shared))
        };
        wait_for("the connection", || shared.lock().is_ok_and(|s| s.connected.is_some()));
        drop(server);
        worker.join().unwrap();
        let after = shared.lock().unwrap();
        assert_eq!((after.connected.as_deref(), after.wanted), (None, Some(port)), "still wanted, so the reader connects again");
    }

    #[test]
    fn each_frame_hands_the_view_the_motion_and_passes_the_preferences_on() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default());
        app.home.open = false;
        let ctx = Context::default();
        let _ = ctx.run_ui(egui::RawInput { focused: true, ..Default::default() }, |_| {});
        let mut bridge = NavBridge { shared: Arc::new(Mutex::new(Shared::default())) };
        let queue = |bridge: &NavBridge| {
            let mut shared = bridge.shared.lock().unwrap();
            shared.connected = Some("MAKMOUSE".into());
            shared.motion = BridgeMotion { orbit: [10.0, 0.0], ..Default::default() };
        };
        // Off (the default): nothing is wanted, nothing reaches the view.
        queue(&bridge);
        bridge.tick(&mut app, &ctx);
        assert_eq!(bridge.shared.lock().unwrap().wanted, None);
        assert!(app.viewport.bridge_motion.is_none() && app.space_mouse.bridge.is_none());
        // On: the port is passed to the reader and the motion to the view, once.
        app.preferences.nav_bridge_enabled = true;
        app.preferences.nav_bridge_port = 9123;
        queue(&bridge);
        bridge.tick(&mut app, &ctx);
        assert_eq!(bridge.shared.lock().unwrap().wanted, Some(9123));
        assert_eq!(app.space_mouse.bridge.as_deref(), Some("MAKMOUSE"));
        assert_eq!(app.viewport.bridge_motion.map(|m| m.orbit), Some([10.0, 0.0]));
        assert!(bridge.shared.lock().unwrap().motion.is_still(), "taken, not handed over twice");
        // Port 0 is no port.
        app.preferences.nav_bridge_port = 0;
        bridge.tick(&mut app, &ctx);
        assert_eq!(bridge.shared.lock().unwrap().wanted, None);
    }
}
