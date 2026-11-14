//! Opening a large design keeps the window alive: frames keep coming, the title bar still
//! moves the window, and the result is the synchronous open's. The design stays usable meanwhile
//! (a result lands after the person's edits), or waits behind a card with "Wait for files to open".

use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportCommand, ViewportId, pos2, vec2};
use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;

/// The whole app in a headless 1600 x 1000 window with the custom title bar.
struct Win {
    ctx: egui::Context,
    app: SolveApp,
    time: f64,
    /// The window's size in points; a test that changes it moves the viewport's layout.
    size: egui::Vec2,
}

impl Win {
    fn new() -> Self {
        // Tests read the status lines and widget texts: install the host's catalogs first, so
        // every id resolves the same way whichever test this binary runs first.
        crate::i18n::install();
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.custom_titlebar = true;
        let mut w = Win { ctx: crate::fonts::test_context(), app, time: 0.0, size: vec2(1600.0, 1000.0) };
        for _ in 0..3 {
            w.frame(vec![]);
        }
        w
    }

    /// One frame: the viewport commands it sent.
    fn frame(&mut self, events: Vec<Event>) -> Vec<ViewportCommand> {
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
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
    let world = s.world_state().unwrap();
    let bodies: Vec<_> = world.bodies.iter().map(|b| (b.name.clone(), b.mesh().triangles.len())).collect();
    let errors = s.model.results.iter().filter(|r| r.error.is_some()).count();
    let b = solidcraft_engine::view::bounds(s).unwrap();
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

    // The card over the design, with its Cancel: the design waits (Preferences > Wait for files).
    let mut w = Win::new();
    w.app.preferences.file_operations_wait = true;
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
    // The card over the design, with its Cancel: the design waits (Preferences > Wait for files).
    let mut w = Win::new();
    w.app.preferences.file_operations_wait = true;
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
    // The card over the design, with its Cancel: the design waits (Preferences > Wait for files).
    h.app.preferences.file_operations_wait = true;
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

/// Insert Design reads the saved design beside the window, as Insert STEP does (#11593, #12108):
/// the overlay names the file and the rebuild's progress, the design waits meanwhile, Cancel
/// keeps it as it was, and the component comes in once it is read, as one undo step.
#[test]
fn insert_design_reads_the_saved_design_while_the_window_draws() {
    let mut h = crate::scenario::Harness::new();
    // The card over the design, with its Cancel: the design waits (Preferences > Wait for files).
    h.app.preferences.file_operations_wait = true;
    h.app.home.open = false;
    // A plate with two holes (three features: the read can be held at the third), saved without
    // its model so inserting it rebuilds the features.
    let part = h.dir.join("plate.solidcraft");
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 40, "width": 20, "height": 5})).unwrap();
    for x in [10, 30] {
        s.execute("solid.cylinder", &json!({"radius": 2, "height": 200, "base": [x, 10, -1], "operation": "cut"})).unwrap();
    }
    s.execute("file.save", &json!({"path": part, "cache": false})).unwrap();
    let plate = s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    // The design it goes into already has a body of its own.
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0]})).unwrap();
    let volume = |app: &mut SolveApp| app.session.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    let near = |app: &mut SolveApp, v: f64| {
        let got = volume(app);
        assert!((got - v).abs() < 1e-3 * v, "{got} mm³, not {v}");
    };
    let links = |app: &mut SolveApp| app.session.execute("component.links", &json!({})).unwrap()["links"].as_array().map_or(0, Vec::len);
    let until = |h: &mut crate::scenario::Harness, done: &dyn Fn(&SolveApp) -> bool| {
        let began = std::time::Instant::now();
        while !done(&h.app) {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "Insert Design went on");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.frames(3);
    };
    // Insert Design, and the plate picked in its file dialog.
    let insert = |h: &mut crate::scenario::Harness| {
        h.step(&json!({"choose_file": "plate.solidcraft"})).unwrap();
        h.app.start("component.insert_design");
        h.frames(2);
    };

    // Held before the plate's third feature: the window draws, saying what it reads.
    let design = h.app.session.doc.clone();
    h.app.file_load.hold_at = Some(2);
    insert(&mut h);
    assert!(h.app.file_load.busy(), "Insert Design reads the design off the window thread");
    until(&mut h, &|app| app.file_load.features().is_some_and(|(done, total)| done >= 2 && total == 3));
    assert!(h.app.file_load.busy(), "Insert Design is still reading the plate");
    assert!(h.widget(solidcraft_ui_text::tr("file_load.inserting")).is_some(), "the overlay says a file is going in");
    assert!(h.widget("plate.solidcraft…").is_some(), "and names the design being read");
    assert_eq!(h.app.file_load.features(), Some((2, 3)), "and the plate's rebuild progress, held before its third feature");
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &design), "the design waits");
    // Cancel on the overlay keeps the design as it was.
    let [x, y] = h.widget(solidcraft_ui_text::tr("ui.cancel")).expect("the overlay's Cancel");
    let at = pos2(x as f32, y as f32);
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    for event in [Event::PointerMoved(at), button(true), button(false)] {
        h.frame_with(vec![event]);
    }
    assert!(!h.app.file_load.busy(), "Cancel gives the window back at once");
    until(&mut h, &|app| app.file_load.idle());
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &design), "a cancelled insert changes nothing");
    assert_eq!(links(&mut h.app), 0);
    near(&mut h.app, 1000.0);

    // Run through, it brings the plate in as a linked component and says so.
    insert(&mut h);
    until(&mut h, &|app| app.file_load.idle());
    assert!(h.widget(solidcraft_ui_text::tr("file_load.inserting")).is_none(), "the overlay goes once it is done");
    assert_eq!(links(&mut h.app), 1, "the plate is a linked component");
    near(&mut h.app, 1000.0 + plate);
    assert_eq!(h.app.status.as_ref().map(|s| s.0.as_str()), Some("Inserted plate"));
    // One undo step takes it back.
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(links(&mut h.app), 0);
    near(&mut h.app, 1000.0);
}

/// Reload from file reads beside the window, as an insert does: the design waits, Cancel changes
/// nothing, and a file that is gone is said in the status line.
#[test]
fn reloading_a_point_cloud_runs_off_the_window_thread() {
    let dir = scratch("reload-cloud");
    let path = dir.join("scan.xyz");
    std::fs::write(&path, "0 0 0\n1 0 0\n").unwrap();
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.preferences.file_operations_wait = true;
    let id = h.app.run("pointcloud.insert", json!({"path": path})).unwrap()["cloud"].as_u64().unwrap();
    h.app.tree.cloud_panel = Some(crate::browser::CloudPanel::new(&h.app, id));
    h.frames(4);
    std::fs::write(&path, "0 0 0\n1 0 0\n2 0 0\n").unwrap();
    let points = |app: &SolveApp| app.session.doc.point_clouds[0].len();
    let until_idle = |h: &mut crate::scenario::Harness| {
        let began = std::time::Instant::now();
        while !h.app.file_load.idle() {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "the reload went on");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.frames(3);
    };
    let hold = |h: &mut crate::scenario::Harness| {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        h.app.file_load.worker_gate = Some((started_tx, release_rx));
        (started_rx, release_tx)
    };

    // The card's button: held on its worker, the window draws and the design waits.
    let design = h.app.session.doc.clone();
    let (started, release) = hold(&mut h);
    // Frame by frame: a scenario step would wait for the file operation it starts.
    let [x, y] = h.widget(solidcraft_ui_text::tr("ui.point_cloud_reload")).expect("the card offers a reload");
    let at = pos2(x as f32, y as f32);
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    for event in [Event::PointerMoved(at), button(true), button(false)] {
        h.frame_with(vec![event]);
    }
    let worker = started.recv_timeout(std::time::Duration::from_secs(60)).expect("the worker started");
    assert_ne!(worker, std::thread::current().id());
    h.frames(3);
    assert!(h.widget("Reloading point cloud").is_some(), "the overlay says what runs");
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &design), "the design waits");
    // Cancel keeps it as it was.
    h.app.file_load.cancel();
    drop(release);
    until_idle(&mut h);
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &design), "a cancelled reload changes nothing");

    // Run through, it brings the new points in, and says it is done, in one undo step.
    let r = crate::file_load::begin_request(&mut h.app, {
        let mut request = crate::file_load::Request::new("", crate::file_load::Kind::ReloadCloud);
        request.options.insert("cloud".into(), json!(id));
        request
    });
    assert!(r.is_ok() && h.app.file_load.busy());
    until_idle(&mut h);
    assert_eq!(points(&h.app), 3);
    assert_eq!(h.app.status.as_ref().map(|s| s.0.as_str()), Some("Reload Point Cloud done"));
    // The open card folds its edits from here on, so an edit does not take the reload with it.
    assert_eq!(h.app.tree.cloud_panel.as_ref().map(|p| p.depth), Some(h.app.session.undo.len()));
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(points(&h.app), 2);

    // A file that is gone: said, and the design is as it was.
    std::fs::remove_file(&path).unwrap();
    let design = h.app.session.doc.clone();
    let mut request = crate::file_load::Request::new("", crate::file_load::Kind::ReloadCloud);
    request.options.insert("cloud".into(), json!(id));
    crate::file_load::begin_request(&mut h.app, request).unwrap();
    until_idle(&mut h);
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &design));
    let status = h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(status.contains("scan.xyz"), "{status}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A request's own `param_check` (#9184) holds on a command the file loader runs, which never
/// reaches `control::handle`: a word that is no level is refused, `strict` refuses a broken call
/// before the file is read, `warn` reads it and says what it found, and every reply says the level.
/// Any spelling the engine takes goes to the file loader too.
#[test]
fn a_file_load_is_checked_at_the_requests_own_level() {
    let dir = scratch("reload-check");
    let path = dir.join("scan.xyz");
    std::fs::write(&path, "0 0 0\n1 0 0\n").unwrap();
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let id = h.app.run("pointcloud.insert", json!({"path": path})).unwrap()["cloud"].as_u64().unwrap();
    std::fs::write(&path, "0 0 0\n1 0 0\n2 0 0\n").unwrap();
    let points = |app: &SolveApp| app.session.doc.point_clouds[0].len();
    let misspelt = |level: &str| json!({"command": "pointcloud.reload", "params": {"cloud": id, "clowd": 1}, "param_check": level});

    let (v, _) = h.call_queued("engine.execute", misspelt("loud"));
    assert!(v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("`param_check` must be off, warn or strict")), "{v}");
    let (v, _) = h.call_queued("engine.execute", misspelt("strict"));
    assert_eq!((&v["ok"], &v["param_check"]), (&json!(false), &json!("strict")), "{v}");
    assert!(v["error"].as_str().is_some_and(|e| e.contains("clowd")), "{v}");
    assert_eq!(points(&h.app), 2, "refused unread");

    let (v, _) = h.call_queued("engine.execute", misspelt("warn"));
    assert_eq!((&v["ok"], &v["param_check"]), (&json!(true), &json!("warn")), "{v}");
    assert!(v["result"]["param_warnings"][0].as_str().is_some_and(|w| w.contains("clowd")), "{v}");
    assert_eq!(points(&h.app), 3);

    // Asked nothing, told nothing; the worker's thread shows the file loader took this spelling.
    std::fs::write(&path, "0 0 0\n1 0 0\n2 0 0\n3 0 0\n").unwrap();
    let (started_tx, started) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    release.send(()).unwrap();
    h.app.file_load.worker_gate = Some((started_tx, release_rx));
    let (v, _) = h.call_queued("engine.execute", json!({"command": "POINTCLOUD.RELOAD", "params": {"cloud": id}}));
    assert_eq!((&v["ok"], v.get("param_check")), (&json!(true), None), "{v}");
    assert_ne!(started.try_recv().expect("the file loader ran it"), std::thread::current().id());
    assert_eq!(points(&h.app), 4);
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

/// Reload from file on a cloud whose file a design names on another computer asks first, naming
/// the server; Cancel reads nothing, and a local file does not ask.
#[test]
fn reloading_a_point_cloud_on_a_network_location_asks_first() {
    use crate::network_prompt::{Answer, NetworkPrompt, Then, answer, reload_cloud};
    let dir = scratch("reload-cloud-network");
    let path = dir.join("scan.xyz");
    std::fs::write(&path, "0 0 0\n1 0 0\n").unwrap();
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let id = h.app.run("pointcloud.insert", json!({"path": path})).unwrap()["cloud"].as_u64().unwrap();
    let local = h.app.session.doc.point_clouds[0].source.clone();
    let until_idle = |h: &mut crate::scenario::Harness| {
        let began = std::time::Instant::now();
        while !h.app.file_load.idle() {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "the reload went on");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.frames(3);
    };

    let crafted = r"\\evil.example\share\scan.xyz";
    h.app.session.doc_mut().point_clouds[0].source = crafted.into();
    let design = h.app.session.doc.clone();
    reload_cloud(&mut h.app, id, crafted);
    assert_eq!(h.app.network_prompt, Some(NetworkPrompt { hosts: vec!["evil.example".into()], then: Then::ReloadCloud(id) }));
    assert!(h.app.file_load.idle(), "nothing is read while it asks");
    answer(&mut h.app, Answer::Cancel);
    h.frames(2);
    assert!(h.app.network_prompt.is_none() && h.app.file_load.idle());
    assert!(std::sync::Arc::ptr_eq(&h.app.session.doc, &design), "Cancel changes nothing");

    // Allow once holds for that reload alone, and the preferences keep nothing.
    reload_cloud(&mut h.app, id, crafted);
    answer(&mut h.app, Answer::Once);
    assert!(!h.app.file_load.idle(), "the file operation runs");
    until_idle(&mut h);
    assert!(h.app.preferences.network_hosts.is_empty());
    let refused = h.app.session.execute("pointcloud.reload", &json!({"cloud": id})).unwrap_err().to_string();
    assert!(refused.contains("evil.example") && refused.contains("Reload from file"), "{refused}");

    // A local file does not ask.
    h.app.session.doc_mut().point_clouds[0].source = local.clone();
    std::fs::write(&path, "0 0 0\n1 0 0\n2 0 0\n").unwrap();
    reload_cloud(&mut h.app, id, &local);
    assert!(h.app.network_prompt.is_none());
    until_idle(&mut h);
    assert_eq!(h.app.session.doc.point_clouds[0].len(), 3);
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
        let b = solidcraft_engine::view::bounds(&w.app.session).unwrap();
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

/// The normal off-thread opening path uses the original file digest and the same remembered
/// answer as synchronous opening, without adding a second offer.
#[test]
fn stand_up_memory_applies_after_an_off_thread_open() {
    let dir = scratch("stand-up-memory");
    let path = dir.join("plate.step").to_string_lossy().to_string();
    let mut src = Session::default();
    src.execute("solid.box", &json!({"length": 10, "width": 40, "height": 5})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap().replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'");
    std::fs::write(&path, text).unwrap();
    let mut w = Win::new();
    w.app.preferences.stand_up_unknown = crate::prefs::StandUpUnknown::Ask;
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let open = |w: &mut Win| {
        let (req, reply) = crate::control::ControlRequest::new("engine.execute", json!({"command":"doc.open", "params":{"path":path}}));
        tx.send(req).unwrap();
        for _ in 0..100_000 {
            w.frame(vec![]);
            if let Ok(reply) = reply.try_recv() {
                assert_eq!(reply["ok"], true, "{reply}");
                assert!(reply["result"]["content_sha256"].is_string());
                return;
            }
        }
        panic!("the import was not answered");
    };
    open(&mut w);
    assert!(w.app.stand_up.is_some());
    crate::prefs::control(&mut w.app, &json!({"stand_up":true})).unwrap();
    let stood = solidcraft_engine::view::bounds(&w.app.session).unwrap();
    open(&mut w);
    assert!(w.app.stand_up.is_none());
    assert_eq!(solidcraft_engine::view::bounds(&w.app.session).unwrap(), stood);
    std::fs::remove_dir_all(dir).unwrap();
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
    let b = solidcraft_engine::view::bounds(&w.app.session).unwrap();
    assert!((b.min.x - 100.0).abs() < 1e-6 && (b.max.x - 160.0).abs() < 1e-6, "moved by the matrix: {b:?}");
    assert!(w.app.session.doc.components.iter().any(|c| c.name == "Bracket"), "named as asked");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #10198: the web build cannot start the meshing thread. An opened design is still drawn whole
/// and its "Loading parts" note goes, instead of an empty view with "Loading parts n/n" for good.
#[test]
fn an_opened_design_is_drawn_whole_when_meshing_has_no_thread() {
    let dir = scratch("progressive-no-thread");
    let path = large_design(&dir, 6);
    let mut w = Win::new();
    let (tx, rx) = std::sync::mpsc::channel();
    w.app.control_rx = Some(rx);
    let (open, open_rx) = crate::control::ControlRequest::new("engine.execute", json!({"command": "doc.open", "params": {"path": path}}));
    tx.send(open).unwrap();
    crate::open_meshing::without_threads(|| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while open_rx.try_recv().is_err() {
            w.frame(vec![]);
            std::thread::sleep(std::time::Duration::from_millis(1));
            assert!(std::time::Instant::now() < deadline, "the open answers");
        }
        for _ in 0..100 {
            if !crate::open_meshing::active(&w.app) {
                break;
            }
            w.frame(vec![]);
        }
    });
    assert!(!crate::open_meshing::active(&w.app), "the parts are in: {:?}", crate::open_meshing::progress(&w.app));
    let world = w.app.session.world_state().unwrap();
    assert!(!world.bodies.is_empty());
    assert_eq!(crate::open_meshing::shown_world(&w.app).bodies.len(), world.bodies.len(), "the whole design is drawn");
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
        assert!(done <= total && total == w.app.session.world_state().unwrap().bodies.len(), "Loading parts {done}/{total}");
        seen.push(shown.bodies.len());
        w.frame(vec![]);
    }
    assert!(seen.windows(2).all(|p| p[0] <= p[1]), "the picture only grows: {seen:?}");
    let world = w.app.session.world_state().unwrap();
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
    assert!(std::sync::Arc::ptr_eq(&crate::open_meshing::shown_world(&w.app), &w.app.session.world_state().unwrap()));
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
    eprintln!("{path}: slowest frame {:.1} ms while {}, {} bodies", slowest.0, slowest.1, w.app.session.world_state().unwrap().bodies.len());
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
    assert!(w.app.session.world_state().unwrap().bodies.len() >= 200);
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

    // The card over the design, with its Cancel: the design waits (Preferences > Wait for files).
    let mut w = Win::new();
    w.app.preferences.file_operations_wait = true;
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

/// An opened design comes up at the home view, framed on its own model: the angle the design
/// before it was left at doesn't carry over, and a design opened while the Home screen is over
/// the viewport is framed when the viewport is drawn, not against the layout of what was there
/// before (which left the design wherever the camera happened to be).
#[test]
fn an_opened_design_comes_up_at_the_home_view() {
    use solidcraft_viewport::StandardView;
    let dir = scratch("home-view");
    let path = large_design(&dir, 4);
    let home_angles = {
        let mut cam = solidcraft_viewport::Camera::default();
        cam.set_view(StandardView::Iso);
        (cam.yaw, cam.pitch, cam.roll)
    };
    let mut w = Win::new();
    // The view the design before it was left at: straight down, zoomed right in.
    w.app.cam.set_view(StandardView::Top);
    w.app.cam.distance = 12.0;
    w.app.cam.target = solidcore_record::Vec3::new(50.0, -70.0, 3.0);
    // Opened from the Home screen, which covers the viewport while the design goes in: the
    // viewport is not drawn, so the layout a fit would be framed into is the one left over from
    // what was there before, which cannot frame anything.
    let revision = w.app.session.revision;
    w.app.home.show(revision);
    let stale = Rect::from_min_size(Pos2::ZERO, vec2(24.0, 24.0));
    w.app.open_path_async(&path);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while w.app.file_load.busy() {
        w.app.viewport.rect = Some(stale);
        w.frame(vec![]);
        assert!(std::time::Instant::now() < deadline, "the open finishes");
        std::thread::yield_now();
    }
    // The design is meshed and framed while the Home screen is still over the viewport, as it is
    // for a design opened from it: nothing draws the viewport in between, so nothing frames the
    // design again afterwards.
    while crate::open_meshing::active(&w.app) {
        w.app.viewport.rect = Some(stale);
        w.frame(vec![]);
        assert!(std::time::Instant::now() < deadline, "the meshing finishes");
        std::thread::yield_now();
    }
    for _ in 0..4 {
        w.app.viewport.rect = Some(stale);
        w.frame(vec![]);
    }
    // The Home screen closes and the viewport is drawn for the first time since the open.
    w.app.home.open = false;
    for _ in 0..4 {
        w.frame(vec![]);
    }
    assert_eq!((w.app.cam.yaw, w.app.cam.pitch, w.app.cam.roll), home_angles, "opened at the home view");
    // Framed on its model, as a fit asked for now frames it: not left at the default camera.
    let opened = w.app.cam;
    w.app.fit_view();
    let refitted = w.app.cam;
    assert!((refitted.target - opened.target).len() < 1e-6, "framed on the model: {opened:?} vs {refitted:?}");
    assert!((refitted.distance - opened.distance).abs() < 1e-6, "framed on the model: {opened:?} vs {refitted:?}");
    // A synchronous open (the control channel's) into a blank design, which keeps its tab and so
    // its camera, comes up at the home view too.
    let other = scratch("home-view-sync");
    let second = large_design(&other, 2);
    crate::documents::new_design(&mut w.app);
    w.frame(vec![]);
    w.app.cam.set_view(StandardView::Front);
    w.app.open_path(&second);
    for _ in 0..4 {
        w.frame(vec![]);
    }
    assert_eq!((w.app.cam.yaw, w.app.cam.pitch, w.app.cam.roll), home_angles, "a synchronous open too");
    let _ = std::fs::remove_dir_all(&other);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A design a newer SolidCraft saved opens in the window: the status line says so without a
/// dialog, Messages names what wasn't read, and editing that feature says why it can't be edited.
#[test]
fn a_newer_design_opens_with_a_notice_and_no_dialog() {
    let mut app = SolveApp::new(Session::default(), Default::default());
    let dir = scratch("newer_design");
    let mut design = Session::default();
    design.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4})).unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&design.doc.to_json()).unwrap();
    let (reads, newer) = (solidcraft_engine::doc::format::FORMAT, solidcraft_engine::doc::format::FORMAT + 1);
    file["format"] = json!(solidcraft_engine::doc::format::format_string(newer));
    file["features"].as_array_mut().unwrap().push(json!({"id": 900, "name": "Lattice1", "type": "gyroid_lattice"}));
    let path = dir.join("newer.json");
    std::fs::write(&path, file.to_string()).unwrap();

    app.open_path(&path.to_string_lossy());

    let (text, _, error) = app.status.clone().unwrap();
    assert_eq!(text, format!("Saved by a newer SolidCraft (format {newer}, this version reads up to {reads}). If something's missing, update."));
    assert!(!error);
    assert!(app.dialog.is_none());
    assert!(app.messages.iter().any(|m| m.text.contains("Lattice1") && m.text.contains("Saving keeps it")));
    assert_eq!(app.session.model.state().bodies.len(), 1);
    app.edit_feature(900);
    let (text, _, error) = app.status.clone().unwrap();
    assert!(error && text.contains("gyroid_lattice"), "{text}");
    assert!(app.dialog.is_none() && !app.ui.palette_open);
}

/// Imported STEP bodies have no editor. Both edit entry points explain this without opening
/// the palette, changing its text, or leaving the timeline rolled back.
#[test]
fn editing_an_imported_body_reports_nothing_to_edit() {
    let dir = scratch("import-edit");
    let path = step_box(&dir);
    for double_click in [false, true] {
        let mut h = crate::scenario::Harness::new();
        h.app.open_path(&path);
        h.app.home.open = false;
        let feature = h.app.session.doc.features.iter().find(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Import { .. })).unwrap();
        let (id, name) = (feature.id, feature.name.clone());
        h.app.run("solid.box", json!({"length": 2, "width": 3, "height": 4, "corner": [40,0,0]})).unwrap();
        h.settle();
        let before = h.app.session.doc.to_json();
        h.app.sbox.text = "kept search".into();
        if double_click {
            h.frames(4);
            h.step(&json!({"click": {"handle": format!("timeline:{name}")}, "double": true})).unwrap();
        } else {
            let reply = h.call("ui.editFeature", json!({"feature": id}));
            assert_eq!(reply["ok"], true, "{reply}");
            assert_eq!(reply["result"]["dialog"], serde_json::Value::Null, "{reply}");
            assert_eq!(reply["result"]["marker"], serde_json::Value::Null, "{reply}");
        }
        h.settle();
        assert!(!h.app.ui.palette_open, "no raw command palette");
        assert!(h.app.dialog.is_none() && h.app.editing_state.is_none());
        assert_eq!(h.app.sbox.text, "kept search");
        assert_eq!(h.app.session.doc.to_json(), before, "the timeline is restored");
        let (text, _, error) = h.app.status.as_ref().expect("a status explains the missing editor");
        assert_eq!(text, "Imported body: nothing to edit");
        assert!(!error, "an informational notice");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A STEP file of a 10 x 20 x 30 box in `dir`.
fn step_box(dir: &std::path::Path) -> String {
    let path = dir.join("box.step").to_string_lossy().to_string();
    let mut src = Session::default();
    src.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    path
}

/// Frames until `done`, with `what` said if it never is.
fn frames_until(w: &mut Win, what: &str, done: impl Fn(&SolveApp) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !done(&w.app) {
        w.frame(vec![]);
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::yield_now();
    }
}

/// The next file operation's worker waits for the returned sender before it starts.
fn hold_next(w: &mut Win) -> (std::sync::mpsc::Receiver<std::thread::ThreadId>, std::sync::mpsc::Sender<()>) {
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    w.app.file_load.worker_gate = Some((started_tx, release_rx));
    (started_rx, release_tx)
}

fn bodies(app: &SolveApp) -> usize {
    app.session.world_state().map_or(0, |state| state.bodies.len())
}

#[test]
fn opening_a_file_from_home_shows_progress_and_cancel_until_the_design_lands() {
    let dir = scratch("home-open");
    let path = large_design(&dir, 0);
    for size in [vec2(1280.0, 720.0), vec2(640.0, 480.0)] {
        let mut w = Win::new();
        w.size = size;
        w.app.home.open = true;
        let (started, release) = hold_next(&mut w);
        crate::file_load::begin(&mut w.app, &path, false);
        started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
        for _ in 0..3 {
            w.frame(vec![]);
        }
        assert!(w.app.home.open && w.app.file_load.busy(), "Home stays open while the worker waits");
        let cancel = crate::scenario::handle_at("file_loading:cancel").expect("Home shows the loading card's Cancel button");
        assert!(Rect::from_min_size(Pos2::ZERO, size).contains(cancel), "Cancel is inside the window at {size:?}");
        assert!(crate::scenario::rect_of("operations:panel").is_none(), "Home has no operations panel");
        release.send(()).unwrap();
        frames_until(&mut w, "the file opened from Home lands", |app| app.file_load.idle());
        w.frame(vec![]);
        assert!(!w.app.home.open, "the loaded design replaces Home");
        assert_eq!(bodies(&w.app), 1);
        assert!(w.app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &path)));
        assert!(crate::scenario::handle_at("file_loading:cancel").is_none(), "the completed load's card disappears");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn returning_home_during_a_file_open_shows_cancel_and_preserves_the_current_design() {
    let dir = scratch("home-cancel");
    let path = large_design(&dir, 0);
    let mut w = Win::new();
    w.size = vec2(1280.0, 720.0);
    w.app.home.open = false;
    w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
    let original = w.app.session.doc.clone();
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &path, false);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(crate::scenario::rect_of("operations:panel").is_some(), "the workspace shows the operation");
    assert!(crate::scenario::handle_at("file_loading:cancel").is_none());
    w.app.home.open = true;
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(w.app.home.open && w.app.file_load.busy());
    assert!(crate::scenario::rect_of("operations:panel").is_none());
    let cancel = crate::scenario::handle_at("file_loading:cancel").expect("Cancel stays available after returning Home");
    w.gesture(&[cancel]);
    assert!(!w.app.file_load.busy(), "Home's Cancel button cancels the open");
    drop(release);
    w.frame(vec![]);
    assert!(w.app.home.open);
    assert!(std::sync::Arc::ptr_eq(&original, &w.app.session.doc));
    assert_eq!((w.app.docs.count(), bodies(&w.app)), (1, 1));
    assert!(crate::scenario::handle_at("file_loading:cancel").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// With the design usable (the default), a file opening is a row of the operations panel, not a
/// card over the design: the design takes edits meanwhile, and the file opens in a tab of its own
/// beside the edited design, which keeps the edits.
#[test]
fn a_file_opening_is_a_row_of_the_operations_panel_and_the_design_takes_edits_meanwhile() {
    let dir = scratch("usable-open");
    let path = large_design(&dir, 4);
    let mut w = Win::new();
    w.app.home.open = false;
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &path, false);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert!(crate::scenario::handle_at("file_loading:cancel").is_none(), "no card over the design");
    let rows = crate::operations::current(&w.app);
    let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().to_string();
    assert!(rows.iter().any(|r| r.key.starts_with("file:") && r.label.contains(&name) && r.cancellable()), "{rows:?}");
    assert!(crate::scenario::rect_of("operations:panel").is_some(), "the panel shows it");
    // Waiting for files, the card shows it instead, and the panel doesn't again.
    w.app.preferences.file_operations_wait = true;
    for _ in 0..2 {
        w.frame(vec![]);
    }
    assert!(crate::scenario::handle_at("file_loading:cancel").is_some() && crate::scenario::rect_of("operations:panel").is_none());
    w.app.preferences.file_operations_wait = false;
    w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).expect("the design takes edits while the file opens");
    let edited = w.app.session.doc.clone();
    release.send(()).unwrap();
    frames_until(&mut w, "the open lands", |app| app.file_load.idle());
    assert_eq!(w.app.docs.count(), 2, "a tab of its own");
    assert!(w.app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &path)));
    crate::documents::switch(&mut w.app, 0);
    assert!(std::sync::Arc::ptr_eq(&edited, &w.app.session.doc), "the edited design keeps its edits");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file that finishes opening while the person is in a dialog goes in a tab behind the one
/// shown: the dialog stays open on the design it was opened for, and the status says where the
/// file went.
#[test]
fn a_file_opened_while_a_dialog_is_open_goes_in_a_tab_behind() {
    let dir = scratch("open-behind");
    let path = large_design(&dir, 2);
    let mut w = Win::new();
    w.app.home.open = false;
    w.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    w.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    w.app.finish_sketch();
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &path, false);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    w.app.start("solid.extrude");
    for _ in 0..2 {
        w.frame(vec![]);
    }
    assert!(w.app.dialog.is_some(), "Extrude opened while the file opens");
    let design = w.app.session.doc.clone();
    release.send(()).unwrap();
    frames_until(&mut w, "the open lands", |app| app.file_load.idle());
    assert_eq!((w.app.docs.count(), w.app.docs.active), (2, 0), "in a new tab, behind");
    assert!(w.app.dialog.is_some() && std::sync::Arc::ptr_eq(&design, &w.app.session.doc), "the dialog stays on its design");
    assert!(w.app.status.as_ref().is_some_and(|(s, _, error)| !error && s.contains("new tab")), "{:?}", w.app.status);
    crate::dialogs::cancel(&mut w.app);
    crate::documents::switch(&mut w.app, 1);
    assert!(w.app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &path)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An insert whose design changes while it runs runs again for the design as it is, and lands
/// after the edit: the edit and the inserted body are both there.
#[test]
fn an_insert_runs_again_for_a_design_edited_meanwhile_and_lands_after_the_edit() {
    let dir = scratch("insert-again");
    let step = step_box(&dir);
    let mut w = Win::new();
    w.app.home.open = false;
    w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-50, 0, 0]})).unwrap();
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &step, true);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    w.frame(vec![]);
    w.app.run("solid.sphere", json!({"diameter": 4, "center": [-80, 0, 0]})).expect("an edit while the file is inserted");
    let key = crate::operations::current(&w.app).into_iter().find(|r| r.key.starts_with("file:")).unwrap().key;
    release.send(()).unwrap();
    // The same row while it runs again, then it lands.
    frames_until(&mut w, "the insert lands", |app| {
        let rows = crate::operations::current(app);
        assert!(app.file_load.idle() || rows.iter().any(|r| r.key == key), "one row throughout: {rows:?}");
        app.file_load.idle()
    });
    assert_eq!(bodies(&w.app), 3, "the box, the sphere added meanwhile and the inserted body: {:?}", w.app.status);
    assert_eq!(w.app.docs.count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A result waiting to land does not hold the file worker: another open runs meanwhile. Closing
/// the tab the waiting design belongs to answers its request instead of leaving it waiting.
#[test]
fn a_waiting_result_does_not_hold_the_queue_and_a_closed_tab_releases_it() {
    let dir = scratch("waiting-queue");
    let step = step_box(&dir);
    let other = large_design(&dir, 2);
    let mut w = Win::new();
    w.app.home.open = false;
    w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).unwrap();
    // An insert for the design in tab 0, with another tab shown when its result comes in.
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &step, true);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    crate::documents::open_tab(&mut w.app, Session::default()).unwrap();
    release.send(()).unwrap();
    frames_until(&mut w, "the insert is ready", |app| crate::operations::current(app).iter().any(|r| r.waiting));
    // An open runs while that result waits: the waiting one is out of the worker's way.
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &other, false);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    release.send(()).unwrap();
    frames_until(&mut w, "the open runs while the insert waits", |app| {
        app.session.path.as_deref().is_some_and(|p| crate::documents::same_path(p, &other))
            && crate::operations::current(app).iter().any(|r| r.waiting)
    });
    // Closing the waiting design's tab answers its request: nothing waits for it any more.
    crate::documents::close(&mut w.app, 0, true);
    frames_until(&mut w, "the waiting result is answered", |app| !crate::operations::current(app).iter().any(|r| r.waiting) && app.file_load.idle());
    assert_eq!(w.app.docs.count(), 1, "the closed tab released its request");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An insert for a design whose tab is no longer shown waits, ready, for it: the design shown
/// gets nothing; showing the tab again lands it there.
#[test]
fn an_insert_waits_for_its_designs_tab() {
    let dir = scratch("insert-tab");
    let step = step_box(&dir);
    let mut w = Win::new();
    w.app.home.open = false;
    w.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [-50, 0, 0]})).unwrap();
    let (started, release) = hold_next(&mut w);
    crate::file_load::begin(&mut w.app, &step, true);
    started.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
    crate::documents::open_tab(&mut w.app, Session::default()).unwrap();
    release.send(()).unwrap();
    frames_until(&mut w, "the insert is ready", |app| crate::operations::current(app).iter().any(|r| r.waiting));
    for _ in 0..5 {
        w.frame(vec![]);
    }
    assert_eq!(bodies(&w.app), 0, "the design shown gets nothing");
    assert!(crate::operations::current(&w.app).iter().any(|r| r.waiting && r.status().is_some()), "it says it is ready");
    crate::documents::switch(&mut w.app, 0);
    frames_until(&mut w, "the insert lands in its own design", |app| app.file_load.idle());
    assert_eq!(bodies(&w.app), 2, "{:?}", w.app.status);
    crate::documents::switch(&mut w.app, 1);
    assert_eq!(bodies(&w.app), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The offer to stand a just-opened part up weighs how its bodies rest, which meshes them. While
/// an open meshes the bodies beside the window, meshing them for the offer waited for that work:
/// the window stood still for over a second on a large assembly. The offer is now worked out
/// beside the window and made on a later frame, and a design changed meanwhile gets none.
#[test]
fn the_stand_up_offer_waits_beside_the_window_for_the_bodies_meshed_on_open() {
    let dir = scratch("stand-up-later");
    let path = dir.join("on_its_back.step").to_string_lossy().to_string();
    let mut src = Session::default();
    // Tall in Z, thin in Y: it rests on faces across Y, a Y-up part lying on its back here.
    src.execute("solid.box", &json!({"length": 10, "width": 5, "height": 40})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'")).unwrap();
    let opened = |w: &mut Win| {
        let result = w.app.session.execute("file.insert_step", &json!({"path": path})).unwrap();
        crate::open_meshing::start(&mut w.app, None);
        assert!(crate::open_meshing::active(&w.app), "the bodies are being meshed");
        crate::stand_up::offer(&mut w.app, &path, &[], Some(&result));
        assert!(w.app.stand_up.is_none() && w.app.stand_up_later.is_some(), "not made on the window thread while the bodies are meshed");
    };
    let mut w = Win::new();
    assert_eq!(w.app.preferences.stand_up_unknown, crate::prefs::StandUpUnknown::Note);
    opened(&mut w);
    for _ in 0..10_000 {
        w.frame(vec![]);
        if w.app.stand_up.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(w.app.stand_up.is_some() && w.app.stand_up_note, "the note is offered once the estimate is made");
    assert!(w.app.stand_up_later.is_none());
    // Changed before the estimate is made: no offer.
    let mut w = Win::new();
    opened(&mut w);
    w.app.session.execute("solid.box", &json!({"length": 4, "width": 4, "height": 4, "corner": [40, 0, 0]})).unwrap();
    w.frame(vec![]);
    assert!(w.app.stand_up.is_none() && w.app.stand_up_later.is_none(), "a changed design is not offered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An offer waiting for its estimate is not made when the offer was turned off meanwhile, and a
/// file from another program gets no estimate at all when such files get no offer.
#[test]
fn a_waiting_stand_up_offer_follows_the_preferences_when_it_is_made() {
    let dir = scratch("stand-up-later-prefs");
    let path = dir.join("on_its_back.step").to_string_lossy().to_string();
    let mut src = Session::default();
    src.execute("solid.box", &json!({"length": 10, "width": 5, "height": 40})).unwrap();
    src.execute("file.export", &json!({"path": path})).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("'SolidCraft','SolidCraft'", "'SolidCraft','Example CAD'")).unwrap();
    let opened = |w: &mut Win| {
        let result = w.app.session.execute("file.insert_step", &json!({"path": path})).unwrap();
        crate::open_meshing::start(&mut w.app, None);
        crate::stand_up::offer(&mut w.app, &path, &[], Some(&result));
    };
    // Turned off while the estimate is made: no offer.
    let mut w = Win::new();
    opened(&mut w);
    assert!(w.app.stand_up_later.is_some());
    w.app.preferences.offer_stand_up = false;
    for _ in 0..10_000 {
        w.frame(vec![]);
        if w.app.stand_up_later.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(w.app.stand_up_later.is_none() && w.app.stand_up.is_none(), "an offer turned off meanwhile is not made");
    // Another program's files get no offer: nothing waits for an estimate.
    let mut w = Win::new();
    w.app.preferences.stand_up_unknown = crate::prefs::StandUpUnknown::Off;
    opened(&mut w);
    assert!(w.app.stand_up_later.is_none() && w.app.stand_up.is_none(), "no estimate for an offer that won't be made");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #15588: opening or inserting a STEP file that may need more memory than the machine can spare
/// asks first, naming the memory; Cancel reads nothing, Open Anyway reads it. A control-channel
/// open never asks: its command refuses until it passes `force: true`.
#[test]
fn a_step_file_that_may_need_more_memory_asks_before_it_opens() {
    let dir = scratch("needs-memory");
    let path = dir.join("part.step");
    let body =
        solidcraft_engine::kernel::box_solid(solidcore_record::Vec3::new(0.0, 0.0, 0.0), solidcore_record::Vec3::new(10.0, 10.0, 10.0)).unwrap();
    std::fs::write(&path, solidcraft_engine::kernel::step_export(&[&body], "part").unwrap()).unwrap();
    let path = path.to_string_lossy().to_string();
    let size = std::fs::metadata(&path).unwrap().len() as usize;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let until_idle = |h: &mut crate::scenario::Harness| {
        let began = std::time::Instant::now();
        while !h.app.file_load.idle() {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "the open went on");
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        h.frames(3);
    };
    let bodies = |h: &crate::scenario::Harness| h.app.session.model.state().bodies.len();
    solidcraft_engine::kernel::step_with_memory_to_spare(size, || {
        // Open asks, naming the file; Cancel reads nothing.
        crate::file_load::begin(&mut h.app, &path, false);
        assert_eq!(h.app.memory_prompts.len(), 1);
        assert!(h.app.file_load.idle(), "nothing is read while it asks");
        h.frames(2);
        assert!(h.widget("part.step may need more memory").is_some(), "the question is shown");
        assert!(h.widget("Open Anyway").is_some() && h.widget("Cancel").is_some());
        crate::open_memory::answer_prompt(&mut h.app, false);
        h.frames(2);
        assert!(h.app.memory_prompts.is_empty() && h.app.file_load.idle());
        assert_eq!(bodies(&h), 0, "Cancel reads nothing");

        // Open Anyway (as the control channel's `ui.documents` answers it) opens the file.
        crate::file_load::begin(&mut h.app, &path, false);
        let state = crate::documents::control(&mut h.app, &json!({"needs_memory": "anyway"}));
        assert_eq!(state["needs_memory"], 0, "{state}");
        until_idle(&mut h);
        assert_eq!(bodies(&h), 1, "{:?}", h.app.status);

        // Insert asks the same.
        crate::file_load::begin(&mut h.app, &path, true);
        assert_eq!(h.app.memory_prompts.len(), 1);
        crate::open_memory::answer_prompt(&mut h.app, true);
        until_idle(&mut h);
        assert_eq!(bodies(&h), 2);
    });
    // With the memory there, nothing asks.
    crate::file_load::begin(&mut h.app, &path, true);
    assert!(h.app.memory_prompts.is_empty());
    until_idle(&mut h);
    assert_eq!(bodies(&h), 3);
    let _ = std::fs::remove_dir_all(&dir);
}
