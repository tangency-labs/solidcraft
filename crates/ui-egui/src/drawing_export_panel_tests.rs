use super::*;
use crate::scenario::Harness;

/// A box with a drawing open: a front view on the first sheet, a top view above it, and a second,
/// empty sheet. The Drawing workspace is shown.
fn drawing(h: &mut Harness) -> (u64, u64, u64) {
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 100, "width": 50, "height": 20})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let sheet = h.app.session.drawing("test").unwrap().sheets[0].id;
    let base =
        h.app.run("drawing.view.base", json!({"sheet": ids::encode(sheet), "view": "front", "scale": "1:2", "position_mm": [150, 120]})).unwrap();
    let front = ids::decode(base["view"].as_str().unwrap()).unwrap();
    let projected = h.app.run("drawing.view.projected", json!({"parent": ids::encode(front), "side": "above"})).unwrap();
    let top = ids::decode(projected["view"].as_str().unwrap()).unwrap();
    h.app.run("drawing.sheet.add", json!({})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    (sheet, front, top)
}

fn wait(h: &mut Harness) {
    let began = std::time::Instant::now();
    loop {
        h.frames(1);
        let busy = h.app.drawing_workspace.export.as_ref().is_some_and(Export::busy);
        if h.app.file_load.idle() && !h.app.file_picker_busy() && !busy {
            break;
        }
        assert!(began.elapsed().as_secs() < 30, "export did not finish");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    h.frames(2);
}

/// A sheet export of the drawing that isn't the window's (an agent's, through the control channel).
fn other_export(h: &mut Harness, sheet: u64, file: &str) -> std::sync::mpsc::Receiver<Value> {
    let path = h.dir.join(file).to_string_lossy().into_owned();
    let mut request = crate::file_load::Request::new(&path, crate::file_load::Kind::ExportSheet);
    request.options = json!({"sheet": ids::encode(sheet), "format": "svg", "path": path}).as_object().cloned().unwrap_or_default();
    let (tx, rx) = std::sync::mpsc::channel();
    request.reply = Some(tx);
    crate::file_load::begin_request(&mut h.app, request).unwrap();
    rx
}

/// Hold the next file worker until `release` is sent; `started` says when it is waiting.
fn hold_next_worker(h: &mut Harness) -> (std::sync::mpsc::Receiver<std::thread::ThreadId>, std::sync::mpsc::Sender<()>) {
    let (started_tx, started) = std::sync::mpsc::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    h.app.file_load.worker_gate = Some((started_tx, release_rx));
    (started, release)
}

fn until_started(h: &mut Harness, started: &std::sync::mpsc::Receiver<std::thread::ThreadId>) {
    let began = std::time::Instant::now();
    while started.try_recv().is_err() {
        h.frames(1);
        assert!(began.elapsed().as_secs() < 30, "the export did not start");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn answer_saves_with(h: &mut Harness, file: &str) {
    let path = h.dir.join(file).to_string_lossy().into_owned();
    h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(path.clone())))));
}

#[test]
fn a_blueprint_theme_starts_on_its_blue_and_every_other_theme_on_white() {
    assert_eq!(starting_paper("blueprint"), Paper::Blueprint);
    assert_eq!(starting_paper("Prussian Blue"), Paper::PrussianBlue);
    for theme in ["dark", "light", "cga", "solidcraft-95", ""] {
        assert_eq!(starting_paper(theme), Paper::White, "{theme}");
    }
}

#[test]
fn file_names_take_the_design_and_sheet_and_lose_what_no_file_system_takes() {
    assert_eq!(file_stem("Bracket", "Sheet 1"), "Bracket - Sheet 1");
    assert_eq!(file_stem("a/b", "c:d*?\"<>|"), "a-b - c-d------");
    assert_eq!(file_stem("", ""), "-");
    assert_eq!(file_stem("x".repeat(400).as_str(), "y").chars().count(), MAX_NAME);
    // One sheet takes the file chosen; several take its name with each sheet's, made distinct.
    let one = paths_for(Path::new("/out/plate.svg"), &[(1, "Sheet 1".into())], "svg");
    assert_eq!(one, [(1, PathBuf::from("/out/plate.svg"))]);
    let two = paths_for(Path::new("/out/plate.png"), &[(1, "A/B".into()), (2, "A-B".into())], "png");
    assert_eq!(two, [(1, PathBuf::from("/out/plate - A-B.png")), (2, PathBuf::from("/out/plate - A-B (2).png"))]);
}

/// A refused sheet names the views the status lists as failed on it, with their reasons, and
/// nothing is read from the refusal's text; a status that names none leaves the friendly error.
#[test]
fn a_refused_sheet_names_its_failed_view_from_the_status() {
    let mut h = Harness::new();
    let (sheet, front, top) = drawing(&mut h);
    let d = h.app.session.drawing("test").unwrap().clone();
    let refusal = json!({"ok": false, "error": "drawing.export: not written"});
    let views = vec![
        (front, sheet, "ok".to_owned(), String::new()),
        (top, sheet, "failed".to_owned(), "not supported yet: shaded".to_owned()),
        (top, sheet + 1, "failed".to_owned(), "another sheet".to_owned()),
    ];
    let lines = outcome_lines(Some(&d), &views, sheet, &refusal);
    let said: Vec<&str> = lines.iter().map(|(_, l)| l.as_str()).collect();
    assert_eq!(lines.len(), 1, "{said:?}");
    assert!(matches!(lines[0].0, StatusTone::Error));
    assert!(lines[0].1.contains(view_name(&d, top).as_str()) && lines[0].1.contains("not supported yet: shaded"), "{said:?}");
    let none = outcome_lines(Some(&d), &views[..1], sheet, &refusal);
    assert_eq!(none.len(), 1);
    assert!(none[0].1.contains(&crate::friendly_error(ID, "drawing.export: not written")), "{}", none[0].1);
}

#[test]
fn sheets_are_listed_with_their_paper_size() {
    use solidcraft_engine::geom::drawing_sheet::PaperSize;
    assert_eq!(size_name(&SheetSize::Standard(PaperSize::A3)), "A3");
    assert_eq!(size_name(&SheetSize::Standard(PaperSize::AnsiB)), "ANSI B");
    assert_eq!(size_name(&SheetSize::Standard(PaperSize::ArchE1)), "ARCH E1");
    assert_eq!(size_name(&SheetSize::Standard(PaperSize::Letter)), "Letter");
    assert_eq!(size_name(&SheetSize::Custom { sides_mm: [300.0, 200.0] }), "300 × 200 mm");
}

/// The ribbon's Export opens the window only with a drawing open; `engine.execute` runs the command
/// with no window, and the window changes nothing in the design or its undo.
#[test]
fn export_opens_its_window_from_the_ribbon_and_never_from_a_programmatic_call() {
    let mut h = Harness::new();
    h.app.home.open = false;
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.app.start(ID);
    assert!(h.app.drawing_workspace.export.is_none(), "no drawing open: nothing opens");
    crate::drawing_workspace::enter(&mut h.app, false).unwrap();
    let (sheet, ..) = drawing(&mut h);
    let reply = h.call("engine.execute", json!({"command": ID, "params": {"sheet": ids::encode(sheet)}}));
    assert!(reply["result"]["svg"].as_str().is_some_and(|svg| svg.starts_with("<?xml") || svg.starts_with("<svg")), "{reply}");
    assert!(h.app.drawing_workspace.export.is_none(), "a programmatic call opens no window");
    let (revision, undo) = (h.app.session.revision, h.app.session.undo.len());
    h.app.start(ID);
    h.frames(3);
    let export = h.app.drawing_workspace.export.as_ref().expect("the window is open");
    assert_eq!((export.format, export.paper, export.sheets.len()), (Format::Svg, Paper::White, 1));
    assert!(crate::scenario::rect_of("drawing:settings").is_some());
    assert!(h.widget(solidcraft_ui_text::tr("ui.drawing_export.title")).is_some());
    h.step(&json!({"click": {"handle": "drawing:export:close"}})).unwrap();
    h.frames(2);
    assert!(h.app.drawing_workspace.export.is_none());
    assert_eq!((h.app.session.revision, h.app.session.undo.len()), (revision, undo));
}

/// SVG and PNG on a chosen paper reach their files through the background export, PDF and DXF stay
/// faded, and OK closes the window once every sheet is written.
#[test]
fn the_window_writes_svg_and_png_on_the_chosen_paper() {
    let mut h = Harness::new();
    drawing(&mut h);
    h.call("ui.prefs", json!({"theme": "blueprint"}));
    h.app.start(ID);
    h.frames(3);
    assert_eq!(h.app.drawing_workspace.export.as_ref().map(|e| e.paper), Some(Paper::Blueprint), "a blueprint theme starts on blue");
    // PDF is planned: clicking it changes nothing.
    h.step(&json!({"click": {"handle": "drawing:export:format:pdf"}})).unwrap();
    assert_eq!(h.app.drawing_workspace.export.as_ref().map(|e| e.format), Some(Format::Svg));
    h.step(&json!({"click": {"handle": "drawing:export:paper:white"}})).unwrap();
    assert_eq!(h.app.drawing_workspace.export.as_ref().map(|e| e.paper), Some(Paper::White));
    answer_saves_with(&mut h, "plate.svg");
    h.step(&json!({"click": {"handle": "dialog:Apply"}})).unwrap();
    wait(&mut h);
    let svg = std::fs::read_to_string(h.dir.join("plate.svg")).unwrap();
    assert!(svg.contains("<svg") && !svg.to_lowercase().contains("#123a76"), "white paper");
    assert!(h.app.drawing_workspace.export.is_some(), "Apply keeps the window open");

    h.step(&json!({"click": {"handle": "drawing:export:format:png"}})).unwrap();
    assert_eq!(h.app.drawing_workspace.export.as_ref().map(|e| e.format), Some(Format::Png));
    h.step(&json!({"click": {"handle": "drawing:export:paper:blueprint"}})).unwrap();
    h.step(&json!({"click": {"handle": "drawing:export:sheet:Sheet 2"}})).unwrap();
    if let Some(export) = h.app.drawing_workspace.export.as_mut() {
        export.dpi = Some(50.0);
    }
    h.frames(2);
    assert_eq!(h.app.drawing_workspace.export.as_ref().map(|e| (e.format, e.paper, e.sheets.len())), Some((Format::Png, Paper::Blueprint, 2)));
    answer_saves_with(&mut h, "plate.png");
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    wait(&mut h);
    let files: Vec<_> = std::fs::read_dir(&h.dir).unwrap().flatten().map(|e| e.file_name()).collect();
    let said = h.app.drawing_workspace.export.as_ref().map(|e| e.outcome.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>());
    assert!(h.dir.join("plate - Sheet 1.png").exists(), "{files:?} {said:?}");
    for name in ["plate - Sheet 1.png", "plate - Sheet 2.png"] {
        let png = std::fs::read(h.dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(png.starts_with(b"\x89PNG"), "{name}");
        // The header's colour type: 2 is RGB, as a blue paper always is.
        assert_eq!(png.get(25), Some(&2), "{name}");
    }
    assert!(h.app.drawing_workspace.export.is_none(), "OK closes the window once both sheets are written");

    // On white paper the PNG's colour is the window's choice: black and white is one bit a pixel.
    h.app.start(ID);
    h.frames(3);
    h.step(&json!({"click": {"handle": "drawing:export:format:png"}})).unwrap();
    h.step(&json!({"click": {"handle": "drawing:export:paper:white"}})).unwrap();
    h.step(&json!({"click": {"handle": "drawing:export:colour"}})).unwrap();
    h.step(&json!({"click": {"handle": "drawing:export:colour:black_white"}})).unwrap();
    assert_eq!(h.app.drawing_workspace.export.as_ref().map(|e| e.colour), Some(Colour::BlackWhite));
    if let Some(export) = h.app.drawing_workspace.export.as_mut() {
        export.dpi = Some(50.0);
    }
    // The resolution set here adds its reset button, which moves OK down.
    h.frames(2);
    answer_saves_with(&mut h, "plate-bw.png");
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    wait(&mut h);
    let png = std::fs::read(h.dir.join("plate-bw.png")).unwrap();
    assert_eq!((png.get(24), png.get(25)), (Some(&1), Some(&0)), "1-bit greyscale");
}

/// A view that can't be drawn is named in the window before the export, and the refused sheet
/// names it after; the window stays open and writes no file.
#[test]
fn a_view_that_cant_be_drawn_is_named_instead_of_refused_silently() {
    let mut h = Harness::new();
    let (sheet, _, top) = drawing(&mut h);
    h.app.run("drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}})).unwrap();
    // The sheet page has drawn its views, so the status lists the one that can't be drawn as failed.
    solidcraft_engine::drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    for _ in 0..10 {
        if !solidcraft_engine::drawing_scene::step(&mut h.app.session) {
            break;
        }
    }
    let name = view_name(h.app.session.drawing("test").unwrap(), top);
    assert_eq!(name, solidcraft_ui_text::tr("ui.drawing_export.view_top"));
    h.app.start(ID);
    h.frames(3);
    answer_saves_with(&mut h, "refused.svg");
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    wait(&mut h);
    assert!(!h.dir.join("refused.svg").exists());
    let export = h.app.drawing_workspace.export.as_ref().expect("a refusal keeps the window open");
    let said: Vec<&str> = export.outcome.iter().map(|(_, line)| line.as_str()).collect();
    assert!(said.iter().any(|line| line.contains(&name) && line.contains("shaded")), "{said:?}");
    assert!(!said.iter().any(|line| line.contains(&ids::encode(top))), "named, not by id: {said:?}");
}

/// Two views that look the same way on one sheet take their numbers; a view seen another way is
/// numbered by its place.
#[test]
fn views_are_named_as_people_know_them() {
    let mut h = Harness::new();
    let (sheet, front, top) = drawing(&mut h);
    let d = h.app.session.drawing("test").unwrap().clone();
    assert_eq!(view_name(&d, front), solidcraft_ui_text::tr("ui.drawing_export.view_front"));
    assert_eq!(view_name(&d, top), solidcraft_ui_text::tr("ui.drawing_export.view_top"));
    let again =
        h.app.run("drawing.view.base", json!({"sheet": ids::encode(sheet), "view": "front", "scale": "1:4", "position_mm": [320, 80]})).unwrap();
    let again = ids::decode(again["view"].as_str().unwrap()).unwrap();
    let d = h.app.session.drawing("test").unwrap().clone();
    let front_name = solidcraft_ui_text::tr("ui.drawing_export.view_front");
    assert!(view_name(&d, front).starts_with(front_name) && view_name(&d, front) != view_name(&d, again));
}

/// Cancel while sheets are being written stops them: the one waiting is never written, the one
/// under way stops, and the window stays to say so. Export pressed again meanwhile keeps the window
/// and its sheets instead of leaving them writing with nothing to say what they did. An export that
/// isn't the window's, waiting behind it, goes on.
#[test]
fn cancel_while_sheets_are_written_stops_them_and_says_so() {
    let mut h = Harness::new();
    let (sheet, ..) = drawing(&mut h);
    h.app.start(ID);
    h.frames(3);
    h.step(&json!({"click": {"handle": "drawing:export:sheet:Sheet 2"}})).unwrap();
    h.frames(2);
    // The first sheet's worker waits to be let go, so both sheets are still to be written.
    let (started, release) = hold_next_worker(&mut h);
    answer_saves_with(&mut h, "held.svg");
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    until_started(&mut h, &started);
    let other = other_export(&mut h, sheet, "other.svg");
    let jobs = |h: &Harness| h.app.drawing_workspace.export.as_ref().map(|e| e.jobs.len());
    assert_eq!(jobs(&h), Some(2));

    // The window's own guards, called as its buttons do: while a file job runs the app holds clicks
    // and keys, but not once the progress card's Cancel has let go of a sheet still finishing.
    open(&mut h.app);
    h.frames(1);
    assert_eq!(jobs(&h), Some(2), "a second Export keeps the sheets under way");

    close(&mut h.app);
    h.frames(1);
    let export = h.app.drawing_workspace.export.as_ref().expect("the window stays while the sheets stop");
    assert!(export.stopping && !export.close_when_done);
    release.send(()).unwrap();
    wait(&mut h);
    for name in ["held - Sheet 1.svg", "held - Sheet 2.svg"] {
        assert!(!h.dir.join(name).exists(), "{name} written after Cancel");
    }
    let export = h.app.drawing_workspace.export.as_ref().expect("the window says what Cancel stopped");
    let said: Vec<&str> = export.outcome.iter().map(|(_, line)| line.as_str()).collect();
    let stopped = solidcraft_ui_text::tr("ui.drawing_export.stopped");
    assert!(said.len() == 2 && said.iter().all(|line| line.contains(stopped)), "{said:?}");
    assert!(!export.busy() && !export.stopping);
    assert!(h.app.file_load.idle());
    assert_eq!(other.try_recv().ok().map(|r| r["ok"].clone()), Some(json!(true)), "the other export went on");
    assert!(h.dir.join("other.svg").exists());

    // With nothing under way, Cancel closes the window.
    h.step(&json!({"click": {"handle": "dialog:Cancel"}})).unwrap();
    h.frames(2);
    assert!(h.app.drawing_workspace.export.is_none());
}

/// The status is read when the window opens, after a change to the design and after the drawing's
/// views are drawn, and not otherwise: each read measures every dimension of the drawing.
#[test]
fn the_status_is_read_again_only_when_what_it_says_can_change() {
    let mut h = Harness::new();
    let (sheet, front, _) = drawing(&mut h);
    h.app.start(ID);
    h.frames(3);
    let status = |h: &Harness| h.app.drawing_workspace.export.as_ref().and_then(|e| e.status.clone());
    let forget = |h: &mut Harness| {
        if let Some(export) = h.app.drawing_workspace.export.as_mut() {
            export.status = None;
        }
    };
    assert!(status(&h).is_some(), "read as the window opens");
    forget(&mut h);
    // Two seconds of frames with nothing changed read nothing.
    h.frames(120);
    assert!(status(&h).is_none(), "read again with nothing changed");

    // The views drawn, as the sheet page draws them, change what it says, not the design.
    let revision = h.app.session.revision;
    solidcraft_engine::drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    for _ in 0..10 {
        if !solidcraft_engine::drawing_scene::step(&mut h.app.session) {
            break;
        }
    }
    assert_eq!(h.app.session.revision, revision);
    h.frames(1);
    let read = status(&h).expect("read again once the views are drawn");
    let front_view = read["views"].as_array().and_then(|views| views.iter().find(|v| v["view"] == json!(ids::encode(front)))).cloned();
    assert_eq!(front_view.map(|v| v["status"].clone()), Some(json!("current")), "{read}");

    forget(&mut h);
    h.frames(3);
    assert!(status(&h).is_none());
    h.app.run("drawing.view.edit", json!({"view": ids::encode(front), "set": {"scale": "1:4"}})).unwrap();
    h.frames(1);
    assert!(status(&h).is_some(), "read again after a change to the design");
}

/// An import or export under way that isn't the window's is never stopped by its Cancel: only the
/// window's sheets waiting behind it are dropped.
#[test]
fn cancel_stops_only_the_windows_own_sheets() {
    let mut h = Harness::new();
    let (sheet, ..) = drawing(&mut h);
    h.app.start(ID);
    h.frames(3);
    let (started, release) = hold_next_worker(&mut h);
    let other = other_export(&mut h, sheet, "other.svg");
    until_started(&mut h, &started);
    answer_saves_with(&mut h, "mine.svg");
    // OK, as the window's own button would (the other export's progress card covers it now).
    export_now(&mut h.app, true);
    let began = std::time::Instant::now();
    while !h.app.drawing_workspace.export.as_ref().is_some_and(Export::busy) {
        h.frames(1);
        assert!(began.elapsed().as_secs() < 30, "the window's sheet was not queued");
    }
    close(&mut h.app);
    h.frames(1);
    release.send(()).unwrap();
    wait(&mut h);
    assert_eq!(other.try_recv().ok().map(|r| r["ok"].clone()), Some(json!(true)), "the other export went on");
    assert!(h.dir.join("other.svg").exists() && !h.dir.join("mine.svg").exists());
    let export = h.app.drawing_workspace.export.as_ref().expect("the window says what Cancel stopped");
    let stopped = solidcraft_ui_text::tr("ui.drawing_export.stopped");
    let said: Vec<&str> = export.outcome.iter().map(|(_, line)| line.as_str()).collect();
    assert!(said.len() == 1 && said.iter().all(|line| line.contains(stopped)), "{said:?}");
}
