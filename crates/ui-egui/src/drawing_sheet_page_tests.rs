use serde_json::{Value, json};
use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_engine::geom::Loop2Geom;
use solidcraft_engine::geom::paper::{Dashes, Group, Item, ObjectKind, Scene};

use super::*;
use crate::scenario::Harness;

/// A 100 × 80 × 20 mm plate (W23's fixture without its bores) on ISO A3 at 1:2: its front view and
/// the view right of it, the front's top edge dimensioned, in the Drawing workspace, with every view
/// drawn. Returns the harness and the front view's id.
fn plate_sheet() -> (Harness, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let front = h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [140, 180]})).unwrap();
    let front = front["view"].as_str().unwrap().to_owned();
    h.app.run("drawing.view.projected", json!({"parent": front, "side": "right"})).unwrap();
    h.app.run("drawing.dimension.add", json!({"view": front, "kind": {"length": {"edge": [50, 0, 20]}}})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    (h, front)
}

/// Frames until no view or dimension is waiting to be drawn.
fn settle(h: &mut Harness) {
    for _ in 0..5_000 {
        h.frame();
        if page(h)["working"] == false {
            h.frames(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the sheet never finished drawing: {}", page(h));
}

fn page(h: &mut Harness) -> Value {
    h.call("ui.inspect", json!({}))["result"]["sheet_page"].clone()
}

/// What the last frame showed, without running another.
fn inspect_now(h: &Harness) -> Value {
    super::inspect(&h.app)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// 100 % is (96 / 25.4) × calibration points per paper millimetre over the UI zoom, so a
/// millimetre is the same number of physical pixels at every UI scale; a calibration out of range
/// is not used.
#[test]
fn true_size_is_one_millimetre_at_any_ui_scale() {
    for native in [1.0f32, 1.25, 2.0] {
        for calibration in [0.5f32, 1.0, 1.25, 2.0] {
            let pixels: Vec<f64> = [0.75f32, 1.0, 1.5, 2.5].iter().map(|zoom| true_size(calibration, *zoom) * f64::from(native * zoom)).collect();
            let want = 96.0 / 25.4 * f64::from(native * calibration);
            assert!(pixels.iter().all(|p| (p - want).abs() < 1e-9), "{native} {calibration}: {pixels:?}");
        }
    }
    for hostile in [0.0f32, -1.0, 2.5, f32::NAN, f32::INFINITY] {
        assert_eq!(true_size(hostile, 1.0), true_size(1.0, 1.0), "calibration {hostile}");
    }
    for hostile in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(true_size(1.0, hostile), true_size(1.0, 1.0), "UI zoom {hostile}");
    }
    // On the page: at 100 % and UI scale 100 %, a 50 mm edge is 188.98 points (W23-4).
    let (mut h, _) = plate_sheet();
    assert_eq!(h.call("ui.view", json!({"view": "actual_size"}))["ok"], true);
    h.frames(2);
    let p = page(&mut h);
    assert!((number(&p["zoom"]) - 1.0).abs() < 1e-9, "{p}");
    assert!((50.0 * number(&p["points_per_mm"]) - 188.976).abs() < 1e-3, "{p}");
    let visible = p["visible_mm"].as_array().unwrap().iter().map(number).collect::<Vec<_>>();
    let canvas = h.app.viewport.rect.unwrap();
    assert!(((visible[2] - visible[0]) * number(&p["points_per_mm"]) - f64::from(canvas.width())).abs() < 1e-3, "{p}");
    // A calibration of 1.25 makes 100 % a quarter larger.
    h.app.preferences.drawing_calibration = 1.25;
    h.step(&json!({"click": {"handle": "sheet:actual_size"}})).unwrap();
    h.frames(2);
    let p = page(&mut h);
    assert!((number(&p["points_per_mm"]) - 1.25 * 96.0 / 25.4).abs() < 1e-9 && (number(&p["zoom"]) - 1.0).abs() < 1e-9, "{p}");
}

/// Fit Sheet shows the whole sheet, and comes back after any zoom; typed and wheeled zooms stay
/// within 5 % to 3200 %, and a zoom that isn't a number changes nothing.
#[test]
fn fit_sheet_shows_the_whole_sheet_and_zoom_stays_in_range() {
    let (mut h, _) = plate_sheet();
    let whole = |p: &Value| {
        let v = p["visible_mm"].as_array().unwrap().iter().map(number).collect::<Vec<_>>();
        v[0] <= 0.0 && v[1] <= 0.0 && v[2] >= 420.0 && v[3] >= 297.0
    };
    let p = page(&mut h);
    assert_eq!(p["size_mm"], json!([420.0, 297.0]));
    assert!(whole(&p), "the first frame fits the sheet: {p}");
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let canvas = h.app.viewport.rect.unwrap();
    assert!(canvas.contains_rect(paper), "{paper:?} in {canvas:?}");
    // Fitted above the navigation bar, as the design's layout board has it.
    let bar = h.app.viewport.layout.as_ref().unwrap().navigation;
    assert!(bar.is_positive() && !paper.intersects(bar), "{paper:?} clear of the navigation bar {bar:?}");
    // A fitted sheet is fitted again when the window changes size, until something moves it.
    h.call("ui.resize", json!({"width": 900, "height": 600}));
    h.frames(4);
    assert!(whole(&page(&mut h)), "refitted in a smaller window: {}", page(&mut h));
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.frames(4);
    let ctx = h.ctx.clone();
    set_zoom(&mut h.app, &ctx, 1.5);
    h.frames(2);
    h.call("ui.resize", json!({"width": 1200, "height": 800}));
    h.frames(4);
    assert!((number(&page(&mut h)["zoom"]) - 1.5).abs() < 1e-9, "a chosen zoom stays through a resize");
    for (zoom, want) in [(1000.0, 32.0), (0.001, 0.05), (2.0, 2.0)] {
        let ctx = h.ctx.clone();
        set_zoom(&mut h.app, &ctx, zoom);
        h.frames(2);
        assert!((number(&page(&mut h)["zoom"]) - want).abs() < 1e-9, "{zoom}");
    }
    let ctx = h.ctx.clone();
    set_zoom(&mut h.app, &ctx, f64::NAN);
    set_zoom(&mut h.app, &ctx, f64::INFINITY);
    h.frames(2);
    assert!((number(&page(&mut h)["zoom"]) - 2.0).abs() < 1e-9);
    assert!(!whole(&page(&mut h)), "at 200 % the sheet is larger than the canvas");
    // The wheel zooms in no further than 3200 %.
    let centre = h.app.viewport.rect.unwrap().center();
    for _ in 0..80 {
        h.app.synthetic.push(egui::Event::PointerMoved(centre));
        h.app.synthetic.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
    }
    // egui's smoothed scrolling runs on for some frames after the last wheel event.
    h.app.synthetic.push(egui::Event::PointerGone);
    h.frames(120);
    let zoom = number(&page(&mut h)["zoom"]);
    assert!(zoom > 2.0 && zoom <= 32.0 + 1e-9, "{zoom}");
    // Fit (F6 runs view.fit), Home and the card's Fit sheet all frame it again.
    for fit in [json!({"command": "view.fit"}), json!({"command": "view.home"})] {
        set_zoom(&mut h.app, &ctx, 4.0);
        h.frames(2);
        h.app.run(fit["command"].as_str().unwrap(), json!({})).unwrap();
        h.frames(2);
        assert!(whole(&page(&mut h)), "{fit}");
    }
    set_zoom(&mut h.app, &ctx, 4.0);
    h.frames(2);
    h.step(&json!({"click": {"handle": "sheet:fit"}})).unwrap();
    h.frames(2);
    assert!(whole(&page(&mut h)));
    assert!(h.call("ui.view", json!({"view": "front"}))["ok"] == false, "a sheet has no 3D views");
}

/// The card shows whole percent, and tenths below 10 %; typing a shown value back keeps it.
#[test]
fn the_zoom_readout_is_rounded() {
    for (zoom, shown) in [(0.72996, 73.0), (1.0, 100.0), (0.0549, 5.5), (0.05, 5.0), (0.0996, 10.0), (31.99, 3199.0)] {
        assert_eq!(shown_percent(zoom), shown, "{zoom}");
    }
}

/// The pan tool drags the sheet with the pointer on the page's own camera: the Design camera, the
/// design and its undo steps stay as they were.
#[test]
fn pan_moves_the_sheet_and_leaves_the_design_camera() {
    let (mut h, _) = plate_sheet();
    let (cam, doc, undo) = (h.app.cam, h.app.session.doc.to_json(), h.app.session.undo.len());
    h.app.run("view.tool", json!({"tool": "pan"})).unwrap();
    let before = crate::scenario::rect_of("sheet:paper").unwrap();
    let from = h.app.viewport.rect.unwrap().center();
    h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 120.0, "y1": from.y + 40.0, "steps": 8}));
    h.frames(2);
    let after = crate::scenario::rect_of("sheet:paper").unwrap();
    let moved = after.min - before.min;
    assert!((moved.x - 120.0).abs() < 2.0 && (moved.y - 40.0).abs() < 2.0, "{moved:?}");
    assert!((after.width() - before.width()).abs() < 1e-3, "panning doesn't zoom");
    assert_eq!(h.app.cam, cam);
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
}

/// Each sheet colour is drawn in its tokens, standing off the backdrop, and is screen only: the
/// design, its undo steps and the exported SVG are the same bytes under every one.
#[test]
fn sheet_colour_is_screen_only() {
    let (mut h, _) = plate_sheet();
    let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
    let svg = |h: &mut Harness| h.app.run("drawing.export", json!({"sheet": "Sheet 1"})).unwrap()["svg"].as_str().unwrap().to_owned();
    let first = svg(&mut h);
    let t = Tokens::get();
    let mut seen = Vec::new();
    for colour in SheetColour::ALL {
        h.app.preferences.drawing_sheet_colour = colour.id().into();
        h.frames(2);
        let p = page(&mut h);
        let (paper, ink) = colour.paper(&t, crate::theme::is_dark());
        let (stage, paper) = stage_and_paper(paper, t.viewport_top);
        assert_eq!((&p["stage"], &p["paper"]), (&json!(hex(stage)), &json!(hex(paper))), "{colour:?}");
        assert_eq!(p["ink"], json!(hex(ink)), "{colour:?}");
        seen.push(p["paper"].clone());
        assert_eq!(svg(&mut h), first, "{colour:?}");
    }
    assert_ne!(seen[1], seen[2], "White and Blueprint differ");
    assert_ne!(seen[2], seen[3], "Blueprint and Prussian Blue differ");
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
    let bad = h.call("ui.prefs", json!({"drawing_sheet_colour": "blue"}));
    assert_eq!(bad["ok"], false, "{bad}");
    let bad = h.call("ui.prefs", json!({"drawing_calibration": 3.0}));
    assert_eq!(bad["ok"], false, "{bad}");
}

/// Every choice, in either theme, stands off the stage behind it, and White is never pure white.
#[test]
fn every_sheet_colour_stands_off_the_backdrop() {
    for t in [Tokens::light(), Tokens::dark()] {
        for dark in [false, true] {
            for colour in SheetColour::ALL {
                let (paper, ink) = colour.paper(&t, dark);
                let (stage, shown) = stage_and_paper(paper, t.viewport_top);
                assert!((lightness(shown) - lightness(stage)).abs() >= STAND_OFF, "{colour:?} {dark}");
                assert!((lightness(shown) - lightness(ink)).abs() > 0.5, "{colour:?} {dark}: ink reads on its paper");
            }
        }
        assert_ne!(t.paper_white, Color32::WHITE);
        // White keeps its colour in the light theme: the stage behind it goes darker instead.
        assert_eq!(stage_and_paper(t.paper_white, Tokens::light().viewport_top).1, t.paper_white);
        assert_eq!(SheetColour::Auto.paper(&t, false), SheetColour::White.paper(&t, false));
        assert_eq!(SheetColour::Auto.paper(&t, true), (t.paper_dark, t.paper_dark_ink));
    }
    // "Follows the theme" in the Blueprint theme is its blueprint blue (D24, D25).
    let blueprint = crate::theme_files::resolve(crate::theme_files::find("blueprint", &[]).unwrap());
    assert_eq!(SheetColour::Auto.paper(&blueprint, true).0, Tokens::dark().paper_blueprint);
    for colour in SheetColour::ALL {
        assert_eq!(SheetColour::parse(colour.id()), Some(colour));
    }
    assert_eq!(SheetColour::parse("Blueprint"), None);
    assert_eq!(SheetColour::parse("warm"), None);
}

fn square(at: Vec2, side: f64) -> Loop2 {
    let p = [at, at + Vec2::new(side, 0.0), at + Vec2::new(side, side), at + Vec2::new(0.0, side)];
    Loop2 { segs: (0..4).map(|i| Seg2::Line { a: p[i], b: p[(i + 1) % 4] }).collect() }
}

/// A stroke, a dashed stroke, an arc and a fill with a hole each become what they are: the dashes
/// at their lengths, the arc within the tolerance, the fill's triangles its area.
#[test]
fn marks_draw_each_item_of_the_scene() {
    let line =
        Item::Stroke { path: vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }], closed: false, width_mm: 0.5, pattern: None };
    let dashed = Item::Stroke {
        path: vec![Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(30.0, 10.0) }],
        closed: false,
        width_mm: 0.35,
        pattern: Some(Dashes { lengths_mm: vec![5.0, 5.0], phase_mm: 0.0 }),
    };
    let arc = Item::Stroke {
        path: vec![Seg2::Arc { center: Vec2::new(50.0, 50.0), radius: 20.0, start: 0.0, sweep: std::f64::consts::PI }],
        closed: false,
        width_mm: 0.5,
        pattern: None,
    };
    let fill =
        Item::Fill { region: Region2 { outer: square(Vec2::new(60.0, 0.0), 10.0), holes: vec![square(Vec2::new(62.0, 2.0), 2.0).reversed()] } };
    let scene = Scene {
        size_mm: Vec2::new(100.0, 100.0),
        groups: vec![Group { object: 1, kind: ObjectKind::View, items: vec![line, dashed, arc, fill] }],
        fonts: Vec::new(),
        clips: Vec::new(),
    };
    let marks = Marks::of(&scene, 7, 0.01);
    assert!(marks.error.is_none(), "{:?}", marks.error);
    let lines: Vec<&Vec<Vec2>> = marks.marks.iter().filter_map(|m| if let Mark::Line { points, .. } = m { Some(points) } else { None }).collect();
    assert_eq!(lines.len(), 1 + 3 + 1, "the line, three dashes, the arc");
    for dash in &lines[1..4] {
        let length: f64 = dash.windows(2).map(|w| w[0].dist(w[1])).sum();
        assert!((length - 5.0).abs() < 1e-9, "{dash:?}");
    }
    assert!(lines[4].iter().all(|p| (p.dist(Vec2::new(50.0, 50.0)) - 20.0).abs() <= 0.01 + 1e-9) && lines[4].len() > 20);
    let Some(Mark::Fill { triangles, .. }) = marks.marks.last() else { panic!("{:?}", marks.marks) };
    let area: f64 = triangles.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() / 2.0).sum();
    assert!((area - 96.0).abs() < 1e-6, "{area}");
    // A scene that isn't valid draws nothing and says why, never panics.
    let mut bad = scene.clone();
    bad.size_mm = Vec2::new(f64::NAN, 1.0);
    let marks = Marks::of(&bad, 8, 0.01);
    assert!(marks.marks.is_empty() && marks.error.is_some());
}

/// The plate's sheet draws its border, views, dimension and title block text: every text run's
/// glyphs are filled, and all of it lies on the paper.
#[test]
fn the_plate_sheet_draws_its_lines_and_text() {
    let (mut h, _) = plate_sheet();
    let sheet = h.app.drawing_workspace.page.sheet.unwrap();
    let view = solidcraft_engine::drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    let glyphs: usize = view.scene.groups.iter().flat_map(|g| &g.items).map(|i| if let Item::Text(run) = i { run.glyphs.len() } else { 0 }).sum();
    assert!(glyphs > 20, "the title block has text: {glyphs}");
    assert!(view.scene.groups.iter().any(|g| g.kind == ObjectKind::Dimension), "the dimension is drawn");
    let marks = Marks::of(&view.scene, view.generation, 0.01);
    assert!(marks.error.is_none());
    let fills = marks.marks.iter().filter(|m| matches!(m, Mark::Fill { .. })).count();
    assert!(fills >= glyphs / 2, "{fills} fills for {glyphs} glyphs (a space has none)");
    let paper = Aabb2 { min: Vec2::ZERO, max: Vec2::new(420.0, 297.0) }.inflated(1.0);
    for m in &marks.marks {
        let b = match m {
            Mark::Line { bounds, .. } | Mark::Fill { bounds, .. } => bounds,
            Mark::Picture(b) => b,
        };
        assert!(paper.contains(b.min) && paper.contains(b.max), "{b:?}");
    }
    assert_eq!(page(&mut h)["views"].as_array().unwrap().len(), 2);
    assert!(page(&mut h)["views"].as_array().unwrap().iter().all(|v| v["status"] == "drawn" && v["words"] == json!([])));
}

/// A view's state is in words beside it: one whose settings changed says it needs an update until it
/// is drawn again, and one with nothing to show says why.
#[test]
fn a_views_state_is_in_words() {
    let (mut h, front) = plate_sheet();
    h.app.run("drawing.view.edit", json!({"view": front, "set": {"scale": "1:5"}})).unwrap();
    // The first frame after the edit: the view is drawn beside the window from now on.
    h.frame();
    assert!(crate::scenario::rect_of(&format!("sheet:view:{front}:state")).is_some(), "its words are beside it");
    let p = inspect_now(&h);
    let state = p["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(front)).unwrap().clone();
    assert_eq!(state["out_of_date"], true, "{p}");
    assert_eq!(state["words"], json!(["Needs update"]), "{p}");
    assert_eq!(p["working"], true);
    settle(&mut h);
    let p = page(&mut h);
    assert!(p["views"].as_array().unwrap().iter().all(|v| v["out_of_date"] == false), "{p}");

    // A view with nothing to draw, or one that can't be drawn, says why, out of date or not.
    use solidcraft_engine::drawing_view::Unmeasured;
    let nothing = words(&Status::Placeholder(Unmeasured::NothingChosen), false);
    assert_eq!(nothing, [format!("Nothing to draw: {}", Unmeasured::NothingChosen)]);
    let failed = words(&Status::Failed("too many lines".into()), true);
    assert_eq!(failed, ["Can't draw this view: too many lines", "Needs update"]);
    assert!(words(&Status::Drawn, false).is_empty());
}

/// A view shows its boundary only while the pointer is near its edge (D17 of the drawing UI
/// design); a dimension or the title block shows its box whenever the pointer is over it.
#[test]
fn a_view_is_outlined_only_near_its_edge() {
    let hit = |kind, on_edge| drawing_scene::Hit { object: 1, kind, on_edge };
    assert!(outlined(&hit(ObjectKind::View, true)));
    assert!(!outlined(&hit(ObjectKind::View, false)), "inside a view the sheet stays clean");
    assert!(outlined(&hit(ObjectKind::Dimension, false)));
    assert!(outlined(&hit(ObjectKind::TitleBlock, false)));
}

/// Without a drawing open the page shows the workspace's note, and the control channel's sheet
/// page is empty; outside Drawing there is none either.
#[test]
fn no_drawing_open_shows_the_note() {
    let mut h = Harness::new();
    h.app.home.open = false;
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h), Value::Null);
    assert!(h.widget("No drawing is open.").is_some());
    let (mut h, _) = plate_sheet();
    crate::drawing_workspace::enter(&mut h.app, false).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h), Value::Null);
}
