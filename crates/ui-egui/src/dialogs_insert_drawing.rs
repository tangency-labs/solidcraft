//! Insert SVG / Insert DXF: once the file is picked, the drawing is placed on the sketch (where
//! its middle or the file's origin goes, its size in millimetres or as a factor, and an angle)
//! with a live preview, and OK adds it (#2605, #3443, #22057).

use crate::ui_kit::BackingTooltipExt as _;
use serde_json::{Value, json};
use solidcore_record::{Seg2, Vec2};
use solidcraft_engine::geom::{Seg2Geom as _, Similarity2};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row};

/// Largest scale the insert commands take.
pub(crate) const MAX_SCALE: f64 = 1e5;

/// The smallest join tolerance the insert commands take (mm): the preview imports with it, so it
/// never joins ends the confirmed sketch might keep apart (see [`preview_placement`]).
const MIN_TOLERANCE_MM: f64 = 1e-9;

/// Largest Simplify distance the window offers, in mm (the command brings a bigger one inside
/// this too).
pub(crate) const MAX_SIMPLIFY_MM: f64 = 100.0;

/// What X and Y place, and what the drawing scales and turns about (#22057).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// The drawing's middle, as it always was.
    #[default]
    Middle,
    /// The file's own origin: at X 0, Y 0, scale 1 and angle 0 the drawing keeps its coordinates.
    Origin,
}

impl Align {
    /// The insert command's `align` value.
    fn word(self) -> &'static str {
        match self {
            Align::Middle => "center",
            Align::Origin => "origin",
        }
    }
}

/// The drawing being placed: the insert command, its file, and where, how big and how turned
/// it goes. `x` and `y` are where the drawing's middle lands, or the file's origin with
/// [`Align::Origin`]; it scales and turns about that point.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub command: String,
    pub path: String,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    /// Degrees, counter-clockwise.
    pub angle: f64,
    /// The drawing's own width and height in the file, when it could be read.
    pub natural: Option<(f64, f64)>,
    /// How far, in mm, a removed edge may lie from the outline kept: lines that meet end to end
    /// are followed with fewer edges (see `sketch.insert_svg`'s `simplify`). 0 keeps every edge as
    /// drawn. A drawing with more edges than the largest sketch takes starts at a small distance
    /// it can be lowered to 0 from.
    pub simplify: f64,
    /// The middle of the drawing's box in the file's own coordinates, when it could be read.
    pub middle: Option<(f64, f64)>,
    pub align: Align,
}

/// Open the placement for `path`, picked for `command` (`sketch.insert_svg` / `_dxf`).
pub fn open(app: &mut SolveApp, command: &str, path: String) {
    let bounds =
        app.session.execute("sketch.drawing_bounds", &json!({"path": path, "format": if command.ends_with("dxf") { "dxf" } else { "svg" }})).ok();
    let natural = bounds
        .as_ref()
        .and_then(|b| Some((b.get("width")?.as_f64()?, b.get("height")?.as_f64()?)))
        .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 1e-9 && *h > 1e-9);
    let edges = bounds.as_ref().and_then(|b| b.get("edges")?.as_u64()).unwrap_or(0);
    let simplify = suggested_simplify(edges, solidcraft_engine::sketch::limits::max_points(), natural);
    let corner = |b: &Value, key: &str| -> Option<(f64, f64)> { Some((b.get(key)?.get(0)?.as_f64()?, b.get(key)?.get(1)?.as_f64()?)) };
    let middle = bounds
        .as_ref()
        .and_then(|b| Some((corner(b, "min")?, corner(b, "max")?)))
        .map(|(lo, hi)| ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
        .filter(|(x, y)| x.is_finite() && y.is_finite());
    let place = Place { command: command.to_string(), path, x: 0.0, y: 0.0, scale: 1.0, angle: 0.0, natural, middle, align: Align::Middle, simplify };
    app.dialog = Some(Dialog::new(Kind::InsertDrawing(place), Vec::new()));
}

/// A starting Simplify distance (mm) for a drawing with `edges` entities: none while the largest
/// sketch takes it as drawn, else a two-hundredth of its size (a smoothed outline of a dense
/// drawing usually loses most of its edges there), rounded up to 0.01 mm. The window shows it and
/// it can be lowered to 0; a drawing that still does not fit says so in its preview.
pub fn suggested_simplify(edges: u64, limit: usize, natural: Option<(f64, f64)>) -> f64 {
    if edges <= limit as u64 {
        return 0.0;
    }
    let Some((w, h)) = natural else { return 0.0 };
    let span = w.max(h);
    if !(span.is_finite() && span > 1e-9) {
        return 0.0;
    }
    ((span / 200.0) * 100.0).ceil().max(1.0) / 100.0
}

pub fn title(p: &Place) -> &'static str {
    if p.command.ends_with("dxf") { "INSERT DXF" } else { "INSERT SVG" }
}

pub fn rows(ui: &mut egui::Ui, p: &mut Place) {
    let name = std::path::Path::new(&p.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.path.clone());
    form_row(ui, "File", |ui| {
        ui.label(name).backing_on_hover_text(&p.path);
    });
    // What X and Y place: the drawing's middle, or the file's own origin so the drawing keeps the
    // coordinates it was drawn at (#22057).
    form_row(ui, solidcraft_ui_text::tr("ui.insert_drawing.place_by"), |ui| {
        let mut selected = usize::from(p.align == Align::Origin);
        let labels = [solidcraft_ui_text::tr("ui.insert_drawing.middle"), solidcraft_ui_text::tr("ui.insert_drawing.file_origin")];
        let targets = ["dialog:insert:middle", "dialog:insert:origin"];
        if crate::ui_kit::segmented(ui, &mut selected, &labels, crate::ui_kit::Segments { targets: &targets, ..Default::default() }) {
            p.align = if selected == 1 { Align::Origin } else { Align::Middle };
        }
    });
    for (label, v, handle) in [("X", &mut p.x, "dialog:insert:x"), ("Y", &mut p.y, "dialog:insert:y")] {
        form_row(ui, label, |ui| {
            let r = crate::ui_kit::number_field(ui, v, -1e9..=1e9, 0.5, " mm");
            crate::scenario::publish_handle(handle, r.rect.center());
        });
    }
    // The size in millimetres follows the scale and the other way round; the aspect is kept.
    if let Some((nw, nh)) = p.natural {
        for (label, natural, handle) in [("Width", nw, "dialog:insert:width"), ("Height", nh, "dialog:insert:height")] {
            form_row(ui, label, |ui| {
                let mut mm = natural * p.scale;
                let r = crate::ui_kit::number_field(ui, &mut mm, 0.001..=natural * MAX_SCALE, 0.5, " mm");
                if r.changed() && mm.is_finite() && mm > 0.0 {
                    p.scale = (mm / natural).clamp(0.0001, MAX_SCALE);
                }
                crate::scenario::publish_handle(handle, r.rect.center());
            });
        }
    }
    form_row(ui, "Scale", |ui| {
        let r = crate::ui_kit::number_field(ui, &mut p.scale, 0.0001..=MAX_SCALE, 0.01, "");
        crate::scenario::publish_handle("dialog:insert:scale", r.rect.center());
    });
    form_row(ui, "Angle", |ui| {
        let r = crate::ui_kit::number_field(ui, &mut p.angle, -360.0..=360.0, 1.0, "\u{b0}");
        crate::scenario::publish_handle("dialog:insert:angle", r.rect.center());
    });
    crate::ui_kit::with_row_label_tooltip(ui, solidcraft_ui_text::tr("ui.insert_simplify_tip"), |ui| {
        form_row(ui, solidcraft_ui_text::tr("ui.insert_simplify"), |ui| {
            let r = crate::ui_kit::number_field(ui, &mut p.simplify, 0.0..=MAX_SIMPLIFY_MM, 0.01, " mm");
            crate::scenario::publish_handle("dialog:insert:simplify", r.rect.center());
        });
    });
}

/// The window's placement as the geometry layer's similarity: scale, then turn, then move. The
/// preview and the insert therefore move a drawing with one implementation.
pub fn placement(p: &Place) -> Option<Similarity2> {
    Similarity2::new(p.scale, p.angle.to_radians(), Vec2::new(p.x, p.y))
}

/// Where the drawing's point `q` — as the preview has it, with its anchor at the sketch's origin
/// — goes when [`Place`] puts it down. The confirm command applies the same similarity to the
/// file's coordinates.
pub fn placed(q: Vec2, p: &Place) -> Vec2 {
    placement(p).map_or(q, |m| m.apply(q))
}

/// A piece of the drawing, as the preview has it, where [`Place`] puts it: the similarity keeps
/// every piece's own kind, so lines stay lines and arcs keep their sweep (exactly).
pub fn placed_seg(seg: Seg2, place: &Place) -> Seg2 {
    placement(place).map_or(seg, |m| seg.similar(&m))
}

/// The command the live preview runs: the drawing as it is drawn, its middle at the sketch's
/// origin, whatever the window's position, size and angle are. The preview is then computed once
/// and moved for each value ([`placed`]) instead of reading, adding and evaluating the whole
/// drawing again (a 5,000-segment drawing cost 59-120 ms per change; see `docs/perf.md`).
pub fn preview_placement(p: &Place, cmds: &mut [(String, Value)]) {
    for (command, params) in cmds.iter_mut() {
        if *command != p.command || !params.is_object() {
            continue;
        }
        params["at"] = json!([0.0, 0.0]);
        params["scale"] = json!(1.0);
        params["angle"] = json!(0.0);
        // The window imports the drawing at scale 1, so the confirmed command's join tolerance (in
        // placed mm) cannot be reproduced exactly there: at another scale it joins a different
        // file-unit gap. The preview keeps every end as drawn instead, rather than showing a
        // join the confirmed sketch may not make; the gaps it shows are at most the tolerance.
        params["tolerance"] = json!(MIN_TOLERANCE_MM);
    }
}

/// The command OK runs (and the preview shows).
pub fn commands(p: &Place) -> Result<Vec<(String, Value)>, String> {
    if !(p.x.is_finite() && p.y.is_finite()) {
        return Err("the position must be a number".into());
    }
    if !(p.scale.is_finite() && p.scale > 1e-9 && p.scale <= MAX_SCALE) {
        return Err("the scale must be above 0".into());
    }
    if !p.angle.is_finite() {
        return Err("the angle must be a number".into());
    }
    Ok(vec![(
        p.command.clone(),
        json!({"path": p.path, "at": [p.x, p.y], "scale": p.scale, "angle": p.angle, "align": p.align.word(), "simplify": p.simplify}),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    fn svg_file(name: &str) -> String {
        svg_text_file(
            name,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><rect x="0" y="0" width="20" height="10"/></svg>"#,
        )
    }

    fn svg_text_file(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-insert-place-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path.to_string_lossy().to_string()
    }

    /// A half circle of radius 10 mm as `n` straight chords: dense enough that Simplify takes
    /// most of them away.
    fn dense_arc_svg_n(name: &str, n: usize) -> String {
        let mut path = String::from("M 10 0");
        for k in 1..=n {
            let a = std::f64::consts::PI * k as f64 / n as f64;
            path.push_str(&format!(" L {:.5} {:.5}", 10.0 * a.cos(), 10.0 * a.sin()));
        }
        svg_text_file(
            name,
            &format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20mm\" height=\"10mm\" viewBox=\"-10 0 20 10\"><path d=\"{path}\"/></svg>"),
        )
    }

    fn lines(h: &Harness) -> Vec<(solidcore_record::Vec2, solidcore_record::Vec2)> {
        let st = h.app.session.world_state().unwrap();
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        (0..sk.curves.len())
            .filter_map(|i| match sk.shape(i) {
                Some(solidcraft_engine::sketch::Shape::Line { a, b }) => Some((a, b)),
                _ => None,
            })
            .collect()
    }

    /// Insert SVG asks for the file, then places it: X, Y and Scale move and size the drawing
    /// before OK adds it, in one undo step.
    #[test]
    fn a_picked_svg_is_placed_and_scaled_before_it_is_added() {
        let mut h = Harness::new();
        let path = svg_file("plate.svg");
        let picked = path.clone();
        h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(Some(picked.clone())))));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        h.app.start("sketch.insert_svg");
        h.frames(3);
        let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no placement: {:?}", h.app.status) };
        assert_eq!((p.path.as_str(), p.x, p.y, p.scale, p.angle, p.natural), (path.as_str(), 0.0, 0.0, 1.0, 0.0, Some((20.0, 10.0))));
        assert!(handle_at("dialog:insert:scale").is_some(), "the scale is drawn");
        assert!(lines(&h).is_empty(), "nothing is added before OK");
        // The drawing is previewed where it would go.
        h.step(&json!({"until": {"count": {"name": "sketch_preview_curves", "value": 4}}})).unwrap();
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (p.x, p.y, p.scale) = (5.0, -3.0, 2.0);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let pts: Vec<_> = lines(&h).into_iter().flat_map(|(a, b)| [a, b]).collect();
        assert_eq!(pts.len(), 8, "the rectangle's four lines");
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.x), u.max(p.x)));
        let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.y), u.max(p.y)));
        assert!((hi_x - lo_x - 40.0).abs() < 1e-6 && (hi_y - lo_y - 20.0).abs() < 1e-6, "twice its size: {lo_x}..{hi_x} × {lo_y}..{hi_y}");
        assert!(((lo_x + hi_x) / 2.0 - 5.0).abs() < 1e-6, "its middle is at X: {lo_x}..{hi_x}");
        assert!(((lo_y + hi_y) / 2.0 + 3.0).abs() < 1e-6, "its middle is at Y: {lo_y}..{hi_y}");
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert!(lines(&h).is_empty(), "one undo removes the drawing");
    }

    /// The preview is imported once, as drawn with its middle at the sketch's origin, and put
    /// down by the window's placement as it is drawn: changing X, Y, scale or angle starts no
    /// import and no evaluation, and the drawing moves at once (docs/perf.md).
    #[test]
    fn the_preview_is_imported_once_and_moved_to_the_placement() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("placed-once.svg"));
        h.frames(4);
        h.step(&json!({"until": {"count": {"name": "sketch_preview_curves", "value": 4}}})).unwrap();
        h.frame();
        let bounds = h.app.preview.bounds;
        assert!((bounds.min.x + 10.0).abs() < 1e-6 && (bounds.max.x - 10.0).abs() < 1e-6, "as drawn, about the origin: {bounds:?}");
        assert!((bounds.min.y + 5.0).abs() < 1e-6 && (bounds.max.y - 5.0).abs() < 1e-6, "as drawn, about the origin: {bounds:?}");
        let last = h.app.preview.ms;
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (p.x, p.y, p.scale, p.angle) = (5.0, -3.0, 2.0, 90.0);
        }
        h.frame();
        assert!(!h.app.preview.busy, "the preview is not imported again");
        assert_eq!(h.app.preview.ms, last, "no preview was computed for the new placement");
        assert_eq!(h.app.preview.sketches.len(), 1, "the drawing it has stays up");
        assert_eq!(h.app.preview.sketches[0].curves.len(), 4);
        // Twice its size, turned a quarter, its middle at (5, -3): 40 x 20 mm about the origin
        // then moved there.
        let bounds = h.app.preview.bounds;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(near(bounds.min.x, -5.0) && near(bounds.max.x, 15.0), "placed: {bounds:?}");
        assert!(near(bounds.min.y, -23.0) && near(bounds.max.y, 17.0), "placed: {bounds:?}");
        // OK adds the drawing there, the same placement the preview showed.
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        let pts: Vec<_> = lines(&h).into_iter().flat_map(|(a, b)| [a, b]).collect();
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.x), u.max(p.x)));
        let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.y), u.max(p.y)));
        assert!(near(hi_x - lo_x, 20.0) && near(hi_y - lo_y, 40.0), "turned: {lo_x}..{hi_x} x {lo_y}..{hi_y}");
        assert!(near((lo_x + hi_x) / 2.0, 5.0) && near((lo_y + hi_y) / 2.0, -3.0), "placed: {lo_x}..{hi_x}, {lo_y}..{hi_y}");
    }

    /// The preview's command imports the drawing as drawn (its middle at the sketch's origin),
    /// while OK's keeps the placement: the window moves the preview instead of importing again.
    #[test]
    fn the_preview_command_imports_the_drawing_as_drawn() {
        let p = Place {
            command: "sketch.insert_svg".into(),
            path: "x.svg".into(),
            x: 5.0,
            y: -3.0,
            scale: 2.0,
            angle: 90.0,
            natural: Some((20.0, 10.0)),
            middle: Some((10.0, 5.0)),
            align: Align::Middle,
            simplify: 0.0,
        };
        let mut p = p;
        p.simplify = 0.2;
        let mut shown = commands(&p).unwrap();
        assert_eq!(shown[0].1["at"], json!([5.0, -3.0]));
        preview_placement(&p, &mut shown);
        assert_eq!(shown[0].1["at"], json!([0.0, 0.0]));
        assert_eq!(shown[0].1["scale"], json!(1.0));
        assert_eq!(shown[0].1["angle"], json!(0.0));
        assert_eq!(shown[0].1["align"], json!("center"));
        // Simplifying is not a placement: the preview imports the drawing at the window's
        // distance, as OK does (measured on the drawing, so the same at any scale).
        assert_eq!(shown[0].1["simplify"], json!(0.2));
        // The preview imports at scale 1, where the confirmed command's placed-mm join tolerance
        // cannot be reproduced: it keeps every end as drawn instead of showing a join the sketch
        // may not make.
        assert_eq!(shown[0].1["tolerance"], json!(MIN_TOLERANCE_MM));
        // The command OK runs is untouched.
        let ok = commands(&p).unwrap();
        assert_eq!(ok[0].1["at"], json!([5.0, -3.0]));
        assert_eq!(ok[0].1["scale"], json!(2.0));
        assert_eq!(ok[0].1["angle"], json!(90.0));
        assert!(ok[0].1.get("tolerance").is_none(), "OK keeps the command's own default: {}", ok[0].1);
    }

    /// A point of the drawing moves exactly as `sketch.insert_svg`'s placement moves it, and a
    /// piece keeps its kind (an arc's sweep, a conic's weight).
    #[test]
    fn the_placement_moves_a_point_and_a_piece_as_the_import_does() {
        use solidcore_record::Seg2;
        let p = Place {
            command: "sketch.insert_svg".into(),
            path: "x.svg".into(),
            x: 5.0,
            y: -3.0,
            scale: 2.0,
            angle: 90.0,
            natural: None,
            middle: None,
            align: Align::Middle,
            simplify: 0.0,
        };
        let q = placed(solidcore_record::Vec2::new(10.0, 5.0), &p);
        assert!((q.x + 5.0).abs() < 1e-9 && (q.y - 17.0).abs() < 1e-9, "twice, a quarter turn, then moved: {q:?}");
        let arc = Seg2::Arc { center: solidcore_record::Vec2::new(0.0, 0.0), radius: 3.0, start: 0.0, sweep: 1.5 };
        match placed_seg(arc, &p) {
            Seg2::Arc { center, radius, start, sweep } => {
                assert!((center.x - 5.0).abs() < 1e-9 && (center.y + 3.0).abs() < 1e-9);
                assert!((radius - 6.0).abs() < 1e-9 && (start - std::f64::consts::FRAC_PI_2).abs() < 1e-9 && (sweep - 1.5).abs() < 1e-9);
            }
            other => panic!("an arc stays an arc: {other:?}"),
        }
        let conic = Seg2::Conic {
            a: solidcore_record::Vec2::ZERO,
            apex: solidcore_record::Vec2::new(1.0, 1.0),
            b: solidcore_record::Vec2::new(2.0, 0.0),
            w: 0.5,
        };
        match placed_seg(conic, &p) {
            Seg2::Conic { w, .. } => assert!((w - 0.5).abs() < 1e-12, "a conic keeps its weight"),
            other => panic!("a conic stays a conic: {other:?}"),
        }
    }

    fn dense_arc_svg(name: &str) -> String {
        dense_arc_svg_n(name, 800)
    }

    /// A drawing with more edges than the largest sketch takes opens with a Simplify distance
    /// the window shows, so it can come in without a person guessing one.
    #[test]
    fn a_drawing_past_the_sketch_limit_opens_with_a_simplify_distance() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", dense_arc_svg_n("dense-past-limit.svg", 25_000));
        h.frames(2);
        let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no placement: {:?}", h.app.status) };
        assert!(p.simplify > 0.0 && p.simplify <= MAX_SIMPLIFY_MM, "a Simplify distance is offered: {}", p.simplify);
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let n = lines(&h).len();
        assert!(n > 0 && n <= solidcraft_engine::sketch::limits::DEFAULT_POINTS, "the drawing comes in: {n}");
    }

    /// The Simplify distance starts at 0 for a drawing the sketch takes as drawn, is drawn as a
    /// field, and reaches the import: OK then follows the outline with fewer edges.
    #[test]
    fn the_simplify_distance_reaches_the_import() {
        assert_eq!(suggested_simplify(100, 20_000, Some((50.0, 50.0))), 0.0, "a drawing the sketch takes is not simplified");
        assert_eq!(suggested_simplify(30_000, 20_000, Some((50.0, 50.0))), 0.25, "a two-hundredth of its size");
        assert_eq!(suggested_simplify(30_000, 20_000, Some((3.0, 1.0))), 0.02, "at least 0.01 mm, rounded up");
        assert_eq!(suggested_simplify(30_000, 20_000, None), 0.0, "no size, nothing to go by");
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", dense_arc_svg("dense-arc.svg"));
        h.frames(4);
        let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no placement: {:?}", h.app.status) };
        assert_eq!(p.simplify, 0.0, "800 edges fit a sketch as drawn");
        assert!(handle_at("dialog:insert:simplify").is_some(), "the Simplify field is drawn");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.simplify = 0.05;
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let n = lines(&h).len();
        assert!(n > 1 && n < 100, "the dense arc comes in with far fewer edges: {n}");
    }

    /// #22057: placed by its file origin, a drawing keeps the coordinates it was drawn at: a
    /// rectangle drawn at x 30 to 50 lands at x 30 to 50, not centred on the sketch origin.
    #[test]
    fn placed_by_the_file_origin_a_drawing_keeps_its_coordinates() {
        let dir = std::env::temp_dir().join(format!("solidcraft-insert-origin-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("offset.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><rect x="30" y="20" width="20" height="10"/></svg>"#)
            .unwrap();
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", path.to_string_lossy().to_string());
        h.frames(2);
        let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the placement is open") };
        assert_eq!(p.align, Align::Middle, "it opens on the middle, as before");
        assert_eq!(commands(p).unwrap()[0].1["align"], "center");
        h.step(&json!({"click": {"handle": "dialog:insert:origin"}})).unwrap();
        h.frames(2);
        let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("still open") };
        assert_eq!(p.align, Align::Origin);
        assert_eq!(commands(p).unwrap()[0].1["align"], "origin");
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let pts: Vec<_> = lines(&h).into_iter().flat_map(|(a, b)| [a, b]).collect();
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.x), u.max(p.x)));
        assert!((lo_x - 30.0).abs() < 1e-6 && (hi_x - 50.0).abs() < 1e-6, "where it was drawn: {lo_x}..{hi_x}");
    }

    /// The width in millimetres sets the scale, and the angle turns the drawing about its middle.
    #[test]
    fn a_width_and_an_angle_size_and_turn_the_drawing_about_its_middle() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("turned.svg"));
        h.frames(2);
        assert!(handle_at("dialog:insert:width").is_some() && handle_at("dialog:insert:angle").is_some(), "the new fields are drawn");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            let (nw, _) = p.natural.unwrap();
            // 40 mm wide, as the Width field sets it, turned a quarter, middle at (5, -3).
            p.scale = 40.0 / nw;
            (p.x, p.y, p.angle) = (5.0, -3.0, 90.0);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        let pts: Vec<_> = lines(&h).into_iter().flat_map(|(a, b)| [a, b]).collect();
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.x), u.max(p.x)));
        let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.y), u.max(p.y)));
        assert!((hi_x - lo_x - 20.0).abs() < 1e-6 && (hi_y - lo_y - 40.0).abs() < 1e-6, "turned: {lo_x}..{hi_x} x {lo_y}..{hi_y}");
        assert!(((lo_x + hi_x) / 2.0 - 5.0).abs() < 1e-6 && ((lo_y + hi_y) / 2.0 + 3.0).abs() < 1e-6, "about its middle");
    }

    /// The drawing's handles in the viewport edit the same values: a corner scales about the
    /// opposite one, the box moves, and the knob turns.
    #[test]
    fn the_handles_in_the_viewport_scale_move_and_turn_the_drawing() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("handles.svg"));
        h.frames(4);
        let place = |h: &Harness| match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
            Some(Kind::InsertDrawing(p)) => p,
            other => panic!("no placement: {other:?}"),
        };
        // The corner at (+10, +5) dragged to (+30, +15): twice the size, the corner at (-10, -5) stays.
        h.step(&json!({"drag": [{"handle": "insert:handle:3"}, {"world": [30, 15, 0]}]})).unwrap();
        h.frames(3);
        let p = place(&h);
        assert!((p.scale - 2.0).abs() < 0.05, "scaled: {}", p.scale);
        assert!((p.x - 10.0).abs() < 0.5 && (p.y - 5.0).abs() < 0.5, "about the far corner: {}, {}", p.x, p.y);
        // The box dragged by its middle by (+5, -4).
        let (x, y) = (p.x, p.y);
        h.step(&json!({"drag": [{"world": [x, y, 0]}, {"world": [x + 5.0, y - 4.0, 0]}]})).unwrap();
        h.frames(3);
        let p = place(&h);
        assert!((p.x - x - 5.0).abs() < 0.5 && (p.y - y + 4.0).abs() < 0.5, "moved: {}, {}", p.x, p.y);
        // The knob above the box, dragged to the left of the middle: a quarter turn.
        h.step(&json!({"drag": [{"handle": "insert:handle:5"}, {"world": [p.x - 30.0, p.y, 0]}]})).unwrap();
        h.frames(3);
        assert!((place(&h).angle - 90.0).abs() < 2.0, "turned: {}", place(&h).angle);
    }

    /// Cancel adds nothing; a bad scale is refused.
    #[test]
    fn cancelling_the_placement_adds_nothing() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("cancel.svg"));
        h.frames(2);
        let bad = Place {
            command: "sketch.insert_svg".into(),
            path: "x.svg".into(),
            x: 0.0,
            y: 0.0,
            scale: 0.0,
            angle: 0.0,
            natural: None,
            middle: None,
            align: Align::Middle,
            simplify: 0.0,
        };
        assert!(commands(&bad).is_err());
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert!(lines(&h).is_empty(), "nothing added");
    }
}
