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

#[test]
fn a_refusals_reason_is_read_to_its_closing_bracket() {
    let error = "drawing.export: not written: views on the sheet failed: 0123456789abc (not supported yet: shaded (lines)), zyx (other)";
    assert_eq!(bracketed_after(error, "0123456789abc ("), Some("not supported yet: shaded (lines)"));
    assert_eq!(bracketed_after(error, "zyx ("), Some("other"));
    assert_eq!(bracketed_after(error, "nothing ("), None);
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
    assert!(h.widget(crate::i18n::tr("ui.drawing_export.title")).is_some());
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
    let (_, _, top) = drawing(&mut h);
    h.app.run("drawing.view.edit", json!({"view": ids::encode(top), "set": {"visual_style": "shaded_visible"}})).unwrap();
    let name = view_name(h.app.session.drawing("test").unwrap(), top);
    assert_eq!(name, crate::i18n::tr("ui.drawing_export.view_top"));
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
    assert_eq!(view_name(&d, front), crate::i18n::tr("ui.drawing_export.view_front"));
    assert_eq!(view_name(&d, top), crate::i18n::tr("ui.drawing_export.view_top"));
    let again =
        h.app.run("drawing.view.base", json!({"sheet": ids::encode(sheet), "view": "front", "scale": "1:4", "position_mm": [320, 80]})).unwrap();
    let again = ids::decode(again["view"].as_str().unwrap()).unwrap();
    let d = h.app.session.drawing("test").unwrap().clone();
    let front_name = crate::i18n::tr("ui.drawing_export.view_front");
    assert!(view_name(&d, front).starts_with(front_name) && view_name(&d, front) != view_name(&d, again));
}
