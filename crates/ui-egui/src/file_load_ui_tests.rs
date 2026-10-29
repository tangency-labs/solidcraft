//! Opening a large design keeps the window alive: frames keep coming, the title bar still
//! moves the window, the design underneath waits, and the result is the synchronous open's.

use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportCommand, ViewportId, pos2, vec2};
use serde_json::json;
use solidcraft_engine::Session;

use crate::SolveApp;

/// The whole app in a headless 1600 x 1000 window with the custom title bar.
struct Win {
    ctx: egui::Context,
    app: SolveApp,
    time: f64,
}

impl Win {
    fn new() -> Self {
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.custom_titlebar = true;
        let mut w = Win { ctx: crate::fonts::test_context(), app, time: 0.0 };
        for _ in 0..3 {
            w.frame(vec![]);
        }
        w
    }

    /// One frame: the viewport commands it sent.
    fn frame(&mut self, events: Vec<Event>) -> Vec<ViewportCommand> {
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(self.time),
            focused: true,
            events,
            ..Default::default()
        };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out.viewport_output.remove(&ViewportId::ROOT).map(|v| v.commands).unwrap_or_default()
    }

    /// The frames of a press-drag-release along `path`: the viewport commands they sent.
    fn gesture(&mut self, path: &[Pos2]) -> Vec<ViewportCommand> {
        let button = |p: Pos2, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut events = vec![vec![Event::PointerMoved(path[0])], vec![button(path[0], true)]];
        events.extend(path[1..].iter().map(|p| vec![Event::PointerMoved(*p)]));
        events.push(vec![button(*path.last().unwrap(), false)]);
        events.into_iter().flat_map(|e| self.frame(e)).collect()
    }
}

/// A design whose timeline takes a while to rebuild: a plate with `n` posts joined and `n`
/// holes cut, saved into `dir`.
fn large_design(dir: &std::path::Path, n: usize) -> String {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 400, "width": 400, "height": 10, "corner": [-200, -200, 0]})).unwrap();
    for i in 0..n {
        let (x, y) = ((i % 12) as f64 * 30.0 - 165.0, (i / 12) as f64 * 30.0 - 165.0);
        s.execute("solid.cylinder", &json!({"radius": 6, "height": 20, "base": [x, y, 5], "operation": "join"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 2, "height": 40, "base": [x + 9.0, y + 9.0, -5], "operation": "cut"})).unwrap();
    }
    let path = dir.join("large.solidcraft").to_string_lossy().to_string();
    s.execute("file.save", &json!({"path": path})).unwrap();
    path
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("solidcraft-file-load-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// What two sessions must agree on after opening the same file.
fn fingerprint(s: &Session) -> serde_json::Value {
    let world = s.world_state();
    let bodies: Vec<_> = world.bodies.iter().map(|b| (b.name.clone(), b.mesh().triangles.len())).collect();
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let b = solidcraft_engine::view::bounds(s);
    json!({
        "name": s.doc.name,
        "path": s.path,
        "features": s.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        "bodies": bodies,
        "errors": errors,
        "bounds": [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z],
        "dirty": s.is_dirty(),
    })
}

#[test]
fn the_window_keeps_drawing_and_moving_while_a_large_design_opens() {
    let dir = scratch("drag");
    let path = large_design(&dir, 4);
    let mut sync = Session::default();
    sync.execute("doc.open", &json!({"path": path})).unwrap();

    let mut w = Win::new();
    let blank = w.app.session.doc.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    w.app.file_load.worker_gate = Some((started_tx, release_rx));
    w.app.open_path_async(&path);
    // The deadline only bounds a broken test; responsiveness is proved by ordering, not time.
    let worker = started_rx.recv_timeout(std::time::Duration::from_secs(60)).expect("file worker started");
    assert_ne!(worker, std::thread::current().id(), "opening runs on another thread");
    for _ in 0..3 {
        w.frame(vec![]);
        assert!(w.app.file_load.busy(), "frames finish before the file worker is released");
        assert!(std::sync::Arc::ptr_eq(&w.app.session.doc, &blank), "the old design stays intact");
    }
    let bar = pos2(500.0, 17.0);
    let viewport = w.app.viewport.rect.map_or(pos2(800.0, 500.0), |r| r.center());
    let cmds = w.gesture(&[bar, bar + vec2(12.0, 4.0), bar + vec2(40.0, 10.0)]);
    assert!(cmds.iter().any(|c| matches!(c, ViewportCommand::StartDrag)), "the title bar drag was handled during the open");
    let cam = serde_json::to_value(w.app.cam).unwrap();
    w.gesture(&[viewport, viewport + vec2(30.0, 0.0), viewport + vec2(80.0, 20.0)]);
    assert_eq!(serde_json::to_value(w.app.cam).unwrap(), cam, "the design underneath took no camera input");
    assert!(w.app.tool.is_none());
    assert!(crate::scenario::handle_at("file_loading:cancel").is_some(), "the Opening overlay with Cancel was shown");
    assert!(w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_err());
    assert!(w.app.file_load.busy(), "all window interactions completed before releasing the worker");
    assert!(std::sync::Arc::ptr_eq(&w.app.session.doc, &blank));
    release_tx.send(()).expect("release the file worker after window interactions");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while w.app.file_load.busy() && std::time::Instant::now() < deadline {
        w.frame(vec![]);
        std::thread::yield_now();
    }
    assert!(!w.app.file_load.busy(), "the released open finished");
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(!std::sync::Arc::ptr_eq(&w.app.session.doc, &blank));
    assert_eq!(fingerprint(&w.app.session), fingerprint(&sync), "the background open matches a synchronous one");
    assert_eq!(fingerprint(&w.app.session)["errors"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_stops_the_open_and_keeps_the_current_design() {
    let dir = scratch("cancel");
    let path = large_design(&dir, 4);
    let mut w = Win::new();
    w.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let current = w.app.session.doc.clone();
    // The worker waits before the third feature until Cancel: the load is certainly still
    // running when Cancel is clicked, on a fast machine or a slow one.
    w.app.file_load.hold_at = Some(2);
    w.app.open_path_async(&path);
    let began = std::time::Instant::now();
    while w.app.file_load.features().is_none_or(|(done, _)| done < 2) {
        assert!(began.elapsed() < std::time::Duration::from_secs(120), "the load reached the held feature");
        w.frame(vec![]);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // The overlay (and any saved preview the worker sent) is laid out and shown.
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(w.app.file_load.busy());
    let cancel = crate::scenario::handle_at("file_loading:cancel").expect("the overlay's Cancel button");
    w.gesture(&[cancel]);
    assert!(!w.app.file_load.busy(), "Cancel gives the window back at once");
    for _ in 0..100_000 {
        if w.app.file_load.idle() {
            break;
        }
        w.frame(vec![]);
    }
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(std::sync::Arc::ptr_eq(&w.app.session.doc, &current), "the current design stays");
    assert_eq!(w.app.docs.count(), 1, "no tab was opened");
    assert!(w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_ok(), "editing continues");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Update Links reads the linked designs again off the window thread (#5329): the overlay says
/// so and names the design being read, the assembly stays as it was meanwhile (Cancel keeps it),
/// and the new bodies come in once it is done, as one undo step.
#[test]
fn update_links_names_the_design_it_reads_while_the_window_draws() {
    let dir = scratch("links");
    let part = dir.join("plate.solidcraft");
    let case = dir.join("case.solidcraft");
    // A plate with two holes (three features: the read can be held at the third), saved without
    // its model so reading it rebuilds the features.
    let save_part = |height: f64| {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 20, "height": height})).unwrap();
        for x in [10, 30] {
            s.execute("solid.cylinder", &json!({"radius": 2, "height": 200, "base": [x, 10, -1], "operation": "cut"})).unwrap();
        }
        s.execute("file.save", &json!({"path": part, "cache": false})).unwrap();
        s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap()
    };
    let volume = |app: &mut SolveApp| app.session.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    // The holes come through the link's exchange as surfaces of their own: a close match.
    let near = |app: &mut SolveApp, v: f64| {
        let got = volume(app);
        assert!((got - v).abs() < 1e-3 * v, "{got} mm³, not {v}");
    };
    let thin = save_part(5.0);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("component.insert_design", json!({"path": part})).unwrap();
    h.app.run("file.save", json!({"path": case})).unwrap();
    near(&mut h.app, thin);
    let thick = save_part(12.0);
    let until = |h: &mut crate::scenario::Harness, done: &dyn Fn(&SolveApp) -> bool| {
        let began = std::time::Instant::now();
        while !done(&h.app) {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "Update Links went on");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.frames(3);
    };

    // Held before the part's third feature: the window draws, saying what it reads.
    let assembly = h.app.session.doc.clone();
    h.app.file_load.hold_at = Some(2);
    h.app.start("component.update_links");
    assert!(h.app.file_load.busy(), "Update Links runs off the window thread");
    until(&mut h, &|app| app.file_load.features().is_some_and(|(done, total)| done >= 2 && total == 3));
    assert!(h.app.file_load.busy(), "Update Links is still reading the part");
    assert!(h.widget("Updating links").is_some(), "the overlay says Update Links is running");
    assert!(h.widget("plate (1 of 1)…").is_some(), "and names the linked design being read");
    assert!(h.widget("Rebuilding feature 3 of 3").is_some(), "with the part's rebuild progress");
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &assembly), "the assembly waits");
    // Cancel keeps the assembly as it was.
    h.app.file_load.cancel();
    until(&mut h, &|app| app.file_load.idle());
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &assembly), "a cancelled update changes nothing");
    near(&mut h.app, thin);

    // Run through, it brings the edited part in, and says it is done.
    h.app.start("component.update_links");
    until(&mut h, &|app| app.file_load.idle());
    assert!(h.widget("Updating links").is_none(), "the overlay goes once it is done");
    near(&mut h.app, thick);
    assert_eq!(h.app.status.as_ref().map(|s| s.0.as_str()), Some("Update Links done"));
    h.app.run("edit.undo", json!({})).unwrap();
    // One undo step takes it back.
    near(&mut h.app, thin);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A design may link to a file on another computer (`\\host\share\…`); on Windows reading it, or
/// only asking whether it is there, connects to that computer and may send it the user's sign-in.
/// Update Links and Open Linked Design ask first, naming the server: Cancel reads nothing, Allow
/// once reads it for that action alone, Always allow this server keeps it in the preferences.
/// (Unix: the crafted link names a file beside the design, so nothing leaves this computer.)
#[cfg(unix)]
#[test]
fn update_links_and_open_linked_design_ask_before_a_network_location() {
    use crate::network_prompt::{Answer, NetworkPrompt, Then, answer};
    let dir = scratch("network");
    let part = dir.join("plate.solidcraft");
    let case = dir.join("case.solidcraft");
    let crafted = r"\\evil.example\share\plate.solidcraft";
    let save_part = |height: f64| {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": height})).unwrap();
        s.execute("file.save", &json!({"path": part})).unwrap();
        // The same file under the crafted name, beside the design (a plain file name on Unix).
        std::fs::copy(&part, format!("{}/{crafted}", dir.display())).unwrap();
    };
    let volume = |app: &mut SolveApp| app.session.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    save_part(5.0);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let comp = h.app.run("component.insert_design", json!({"path": part})).unwrap()["component"].as_u64().unwrap();
    h.app.run("file.save", json!({"path": case})).unwrap();
    let feature = h.app.session.doc.links[&comp].feature;
    h.app.session.doc_mut().links.insert(comp, solidcraft_engine::doc::DesignLink { path: crafted.into(), feature, ..Default::default() });
    save_part(8.0);
    let until_idle = |h: &mut crate::scenario::Harness| {
        let began = std::time::Instant::now();
        while !h.app.file_load.idle() {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "Update Links went on");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.frames(3);
    };

    // Update Links asks, naming the server, and Cancel reads nothing.
    let assembly = h.app.session.doc.clone();
    h.app.start("component.update_links");
    let asked = Some(NetworkPrompt { hosts: vec!["evil.example".into()], then: Then::UpdateLinks });
    assert_eq!(h.app.network_prompt, asked);
    assert!(h.app.file_load.idle(), "nothing is read while it asks");
    h.frames(2);
    assert!(h.widget("Always allow this server").is_some() && h.widget("Allow once").is_some(), "the question is shown");
    answer(&mut h.app, Answer::Cancel);
    h.frames(2);
    assert!(h.app.network_prompt.is_none() && h.app.file_load.idle());
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &assembly), "Cancel changes nothing");
    assert!((volume(&mut h.app) - 500.0).abs() < 1e-3);

    // Allow once: the link is read for that update alone.
    h.app.start("component.update_links");
    answer(&mut h.app, Answer::Once);
    until_idle(&mut h);
    assert!((volume(&mut h.app) - 800.0).abs() < 1e-3, "{}", volume(&mut h.app));
    let links = h.app.session.execute("component.links", &json!({})).unwrap();
    assert!(links["links"][0]["found"].is_null(), "allowed once, not for good: {links}");
    assert!(h.app.preferences.network_hosts.is_empty());

    // Open Linked Design asks the same; Always allow keeps the server and opens the file.
    let link = links["links"][0].clone();
    crate::network_prompt::open_linked(&mut h.app, &link);
    assert_eq!(h.app.network_prompt.as_ref().map(|p| p.hosts.clone()), Some(vec!["evil.example".to_string()]));
    assert!(matches!(h.app.network_prompt.as_ref().map(|p| &p.then), Some(Then::OpenLinked(_))));
    assert!(h.app.file_load.idle(), "nothing is opened while it asks");
    answer(&mut h.app, Answer::Always);
    assert_eq!(h.app.preferences.network_hosts, vec!["evil.example".to_string()]);
    assert!(!h.app.file_load.idle(), "the linked design opens");
    until_idle(&mut h);
    h.app.preferences.network_hosts.clear();
    crate::prefs::apply_network(&h.app.preferences);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A control-channel (MCP) open is answered when the file is in, with the synchronous
/// command's reply, and the window draws frames while it waits. Commands sent meanwhile wait.
#[test]
fn a_control_open_answers_after_loading_without_freezing_the_window() {
    let dir = scratch("control");
    let path = large_design(&dir, 4);
    let mut sync = Session::default();
    let expected = sync.execute("doc.open", &json!({"path": path})).unwrap();
    let mut w = Win::new();
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let (open, open_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "doc.open", "params": {"path": path}}));
    let (count, count_rx) = crate::control::ControlRequest::new("document.inspect", json!({}));
    let (edit, edit_rx) =
        crate::control::ControlRequest::new("engine.execute", json!({"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}));
    for r in [open, count, edit] {
        tx.send(r).unwrap();
    }
    let mut frames = 0;
    let reply = loop {
        frames += 1;
        w.frame(vec![]);
        if let Ok(v) = open_rx.try_recv() {
            break v;
        }
        assert!(frames < 100_000);
    };
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"], expected);
    assert!(count_rx.try_recv().is_ok(), "inspection answered while the file loaded");
    // Its bodies are meshed after it opened, and its Home picture drawn then, off the UI thread.
    while crate::open_meshing::active(&w.app) {
        w.frame(vec![]);
    }
    assert!(crate::home::remembering(&w.app.home), "the Home picture is drawn off the UI thread");
    for _ in 0..3 {
        w.frame(vec![]);
    }
    let edited = edit_rx.try_recv().expect("the held edit ran after the open");
    assert_eq!(edited["ok"], true, "{edited}");
    assert_eq!(w.app.session.doc.features.len(), sync.doc.features.len() + 1, "the edit went into the opened design");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A control-channel (MCP) import hands its `up` and `names` to the command, as the engine
/// session takes them, and a caller that said which way is up is not offered to stand the part up.
#[test]
fn a_control_import_passes_up_and_names_on() {
    let dir = scratch("import-options");
    let path = dir.join("tall_in_y.step").to_string_lossy().to_string();
    let mut src = Session::default();
    src.execute("solid.box", &json!({"length": 10, "width": 40, "height": 5})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    // Another program's file: SolidCraft's own exports are not offered (#5150).
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'")).unwrap();

    // The reply, the part's height and whether the offer is up, for an import by this window.
    let import = |params: serde_json::Value| {
        let mut w = Win::new();
        // This test is about the caller's `up`, not the note's lying-on-its-back guess (#7717): every
        // file from another program gets the offer, as before.
        w.app.preferences.stand_up_unknown = crate::prefs::StandUpUnknown::Ask;
        let (tx, rx) = std::sync::mpsc::channel();
        w.app.control_rx = Some(rx);
        let (req, reply_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "file.insert_step", "params": params.clone()}));
        tx.send(req).unwrap();
        let mut reply = None;
        for _ in 0..100_000 {
            w.frame(vec![]);
            reply = reply_rx.try_recv().ok();
            if reply.is_some() {
                break;
            }
        }
        let reply = reply.expect("the import was answered");
        assert_eq!(reply["ok"], true, "{params}: {reply}");
        let mut sync = Session::default();
        assert_eq!(reply["result"], sync.execute("file.insert_step", &params).unwrap(), "the window's reply is the session's");
        let b = solidcraft_engine::view::bounds(&w.app.session);
        (reply, [b.max.x - b.min.x, b.max.y - b.min.y, b.max.z - b.min.z], w.app.stand_up.is_some())
    };

    // As before: the file's axes, and the offer to stand it up.
    let (_, size, offered) = import(json!({"path": path}));
    assert!(size[1] > 39.9 && offered, "{size:?} {offered}");
    // Y up said by the caller: stood up by the import, nothing left to offer.
    let (reply, size, offered) = import(json!({"path": path, "up": "y", "names": "product"}));
    assert!(size[2] > 39.9 && !offered, "{size:?} {offered}: {reply}");
    assert!(reply["result"]["stand_up"].is_u64(), "{reply}");
    // Z up said by the caller: kept as it is, and not asked about either.
    let (_, size, offered) = import(json!({"path": path, "up": "z"}));
    assert!(size[1] > 39.9 && !offered, "{size:?} {offered}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #4676: a control-channel import's `matrix` and `name` reach the command, as they do when it
/// runs in a session: the part lands where the matrix puts it, under the name given.
#[test]
fn a_control_import_passes_its_matrix_and_name_on() {
    let dir = scratch("import-matrix");
    let path = dir.join("plate.step").to_string_lossy().to_string();
    let mut src = Session::default();
    src.execute("solid.box", &json!({"length": 60, "width": 40, "height": 5})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    let params = json!({"path": path, "matrix": [1, 0, 0, 100, 0, 1, 0, 0, 0, 0, 1, 0], "name": "Bracket"});
    let mut w = Win::new();
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let (req, reply_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "file.insert_step", "params": params.clone()}));
    tx.send(req).unwrap();
    let mut reply = None;
    for _ in 0..100_000 {
        w.frame(vec![]);
        reply = reply_rx.try_recv().ok();
        if reply.is_some() {
            break;
        }
    }
    let reply = reply.expect("the import was answered");
    assert_eq!(reply["ok"], true, "{reply}");
    let mut sync = Session::default();
    assert_eq!(reply["result"], sync.execute("file.insert_step", &params).unwrap(), "the window's reply is the session's");
    let b = solidcraft_engine::view::bounds(&w.app.session);
    assert!((b.min.x - 100.0).abs() < 1e-6 && (b.max.x - 160.0).abs() < 1e-6, "moved by the matrix: {b:?}");
    assert!(w.app.session.doc.components.iter().any(|c| c.name == "Bracket"), "named as asked");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An opened design is in the window before its bodies are meshed: the view draws the bodies
/// meshed so far (never meshing one itself), framed from their outlines, until all are.
#[test]
fn an_opened_design_shows_its_bodies_as_they_are_meshed() {
    let dir = scratch("progressive");
    let path = large_design(&dir, 24);
    let mut w = Win::new();
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let (open, open_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "doc.open", "params": {"path": path}}));
    tx.send(open).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while open_rx.try_recv().is_err() {
        w.frame(vec![]);
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(std::time::Instant::now() < deadline, "the open answers");
    }
    let mut seen = Vec::new();
    while crate::open_meshing::active(&w.app) {
        let shown = crate::open_meshing::shown_world(&w.app);
        assert!(shown.bodies.iter().all(|b| b.cached_mesh().is_some()), "only meshed bodies are drawn");
        let (done, total) = crate::open_meshing::progress(&w.app).unwrap();
        assert!(done <= total && total == w.app.session.world_state().bodies.len(), "Loading parts {done}/{total}");
        seen.push(shown.bodies.len());
        w.frame(vec![]);
    }
    assert!(seen.windows(2).all(|p| p[0] <= p[1]), "the picture only grows: {seen:?}");
    let world = w.app.session.world_state();
    // Then the whole design (in the order its bodies came, kept until the design changes).
    let names = |st: &solidcraft_engine::doc::ModelState| {
        let mut n: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
        n.sort();
        n
    };
    assert_eq!(names(&crate::open_meshing::shown_world(&w.app)), names(&world), "then the whole design");
    assert!(world.bodies.iter().all(|b| b.cached_mesh().is_some()));
    // Framed from the meshes now, as a synchronous open frames it.
    let framed = w.app.cam;
    w.app.fit_view();
    // Floating viewport framing can round the same bounds by a few ulps.
    let refitted = w.app.cam;
    assert!((refitted.target - framed.target).len() < 1e-9);
    assert!((refitted.distance - framed.distance).abs() < 1e-9);
    assert_eq!(
        (refitted.yaw, refitted.pitch, refitted.roll, refitted.fov, refitted.up),
        (framed.yaw, framed.pitch, framed.roll, framed.fov, framed.up)
    );
    // An edit after that: the viewport draws the session's world again.
    w.app.session.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "operation": "new"})).unwrap();
    w.frame(vec![]);
    assert!(std::sync::Arc::ptr_eq(&crate::open_meshing::shown_world(&w.app), &w.app.session.world_state()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A screenshot asked for right after an open shows the whole design: it waits for the meshing,
/// and what was asked after it waits for it.
#[test]
fn a_screenshot_after_an_open_waits_for_the_whole_design() {
    let dir = scratch("progressive-shot");
    let path = large_design(&dir, 24);
    let mut w = Win::new();
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let (open, open_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "doc.open", "params": {"path": path}}));
    tx.send(open).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while open_rx.try_recv().is_err() {
        w.frame(vec![]);
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(std::time::Instant::now() < deadline, "the open answers");
    }
    if !crate::open_meshing::active(&w.app) {
        return; // Meshed already: nothing to wait for on this machine.
    }
    let (shot, _shot_rx) = crate::control::ControlRequest::new("ui.screenshot", json!({}));
    let (count, count_rx) = crate::control::ControlRequest::new("document.inspect", json!({}));
    tx.send(shot).unwrap();
    tx.send(count).unwrap();
    w.frame(vec![]);
    if crate::open_meshing::active(&w.app) {
        assert!(count_rx.try_recv().is_err(), "a request after the screenshot waits for it");
    }
    while crate::open_meshing::active(&w.app) {
        w.frame(vec![]);
    }
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(count_rx.try_recv().is_ok(), "answered once the design is meshed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An assembly of a few rounded parts placed many times, as a STEP file: an import that takes a
/// while to read, place and mesh.
fn step_assembly(dir: &std::path::Path) -> String {
    let mut s = Session::default();
    // Distinct parts (each meshed on its own), each placed a few times (placed copies share it).
    for k in 0..40 {
        let len = 30.0 + k as f64;
        let part = s.execute("component.create", &json!({"name": format!("Bracket {k}")})).unwrap()["component"].as_u64().unwrap();
        s.execute("solid.box", &json!({"length": len, "width": 20, "height": 8, "operation": "new"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 6, "height": 20, "base": [10, 10, 0], "operation": "join"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 2.5, "height": 40, "base": [len - 10.0, 10, -10], "operation": "cut"})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[len / 2.0, 0, 8]], "radius": 3})).unwrap();
        s.execute("solid.sphere", &json!({"radius": 4, "center": [10, 10, 24], "operation": "join"})).unwrap();
        s.execute("component.activate", &json!({"component": 0})).unwrap();
        for i in 1..5 {
            s.execute("occurrence.copy", &json!({"component": part, "translate": [i as f64 * 80.0, k as f64 * 40.0, 0]})).unwrap();
        }
    }
    let path = dir.join("assembly.step").to_string_lossy().to_string();
    s.execute("file.export", &json!({"path": path})).unwrap();
    path
}

/// Open `path` through the window as a person would, every frame timed, until it is in and meshed.
/// The slowest frame: ms and the phase it was in.
fn open_timed(path: &str) -> (Win, (f64, &'static str)) {
    let mut w = Win::new();
    w.app.frame_watch.slowest = (0.0, "idle");
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let (open, open_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "doc.open", "params": {"path": path}}));
    tx.send(open).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let mut answered = false;
    while !answered || crate::open_meshing::active(&w.app) || !w.app.file_load.idle() {
        w.frame(vec![]);
        answered |= open_rx.try_recv().is_ok();
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(std::time::Instant::now() < deadline, "{path} opens");
    }
    let slowest = w.app.frame_watch.slowest;
    eprintln!("{path}: slowest frame {:.1} ms while {}, {} bodies", slowest.0, slowest.1, w.app.session.world_state().bodies.len());
    (w, slowest)
}

/// The window stays responsive through every phase of opening a design: a STEP assembly imported,
/// the design saved with its model and opened again, and saved without it and opened again. No
/// frame on the window thread takes longer than the budget (the test names the phase that did).
#[test]
fn opening_a_large_design_never_stalls_the_window() {
    /// The frame budget: a frame that draws the design takes a fraction of this on an idle machine,
    /// but a whole test run on a loaded one has every other binary of the suite asking for the CPU
    /// at the same time, and the window thread is descheduled with the rest of them, so the same
    /// frame then takes seconds. A window that works answers at once, so raising the budget does
    /// not make the test take longer. The freeze this test watches for lasted minutes, not
    /// seconds: a window stuck for a minute is still caught here. This is the limit #6072 gave the
    /// agent probe, which the same contention kept from starting within three seconds.
    const BUDGET_MS: f64 = 20_000.0;
    let dir = scratch("watchdog");
    let step = step_assembly(&dir);
    let (mut w, (ms, phase)) = open_timed(&step);
    assert!(ms < BUDGET_MS, "STEP import: a {ms:.0} ms frame while {phase}");
    assert!(w.app.session.world_state().bodies.len() >= 200);
    for (name, cache) in [("cached.solidcraft", true), ("plain.solidcraft", false)] {
        let path = dir.join(name).to_string_lossy().to_string();
        w.app.session.execute("file.save_as", &json!({"path": path, "cache": cache})).unwrap();
        let (_, (ms, phase)) = open_timed(&path);
        assert!(ms < BUDGET_MS, "{name}: a {ms:.0} ms frame while {phase}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// #3171: the second STEP open completes even if the cancelled worker has not returned.
#[test]
fn cancelling_a_step_open_releases_the_slot_before_the_worker_returns() {
    let dir = scratch("step-cancel-reopen");
    let path = dir.join("box.step").to_string_lossy().to_string();
    let mut src = Session::default();
    src.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    let mut expected = Session::default();
    expected.execute("doc.open", &json!({"path": path})).unwrap();

    let mut w = Win::new();
    let blank = w.app.session.doc.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    w.app.file_load.worker_gate = Some((started_tx, release_rx));
    let (first, first_reply) = crate::control::ControlRequest::new("engine.execute", json!({"command":"doc.open", "params":{"path":path}}));
    assert!(crate::file_load::control(&mut w.app, first).is_none());
    started_rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    for _ in 0..3 {
        w.frame(vec![]);
    }
    let cancel = crate::scenario::handle_at("file_loading:cancel").unwrap();
    w.gesture(&[cancel]);
    assert!(w.app.file_load.idle(), "Cancel releases the slot while the first worker is still held");
    let cancelled = first_reply.try_recv().expect("the cancelled caller is answered immediately");
    assert_eq!(cancelled["ok"], false);
    assert!(cancelled["error"].as_str().unwrap().contains("cancelled"));
    assert!(std::sync::Arc::ptr_eq(&blank, &w.app.session.doc));

    let (second, second_reply) = crate::control::ControlRequest::new("engine.execute", json!({"command":"doc.open", "params":{"path":path}}));
    assert!(crate::file_load::control(&mut w.app, second).is_none());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let reply = loop {
        w.frame(vec![]);
        if let Ok(reply) = second_reply.try_recv() {
            break reply;
        }
        assert!(std::time::Instant::now() < deadline, "the second import completes before releasing the first");
        std::thread::yield_now();
    };
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(fingerprint(&w.app.session), fingerprint(&expected));
    assert_eq!(w.app.docs.count(), 1);
    release_tx.send(()).unwrap();
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert_eq!(fingerprint(&w.app.session), fingerprint(&expected));
    let _ = std::fs::remove_dir_all(&dir);
}
