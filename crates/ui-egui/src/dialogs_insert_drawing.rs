//! Insert SVG / Insert DXF: once the file is picked, the drawing is placed on the sketch (where
//! its middle or the file's origin goes, its size in millimetres or as a factor, and an angle)
//! with a live preview, and OK adds it (#2605, #3443, #22057).

use crate::ui_kit::BackingTooltipExt as _;
use serde_json::{Value, json};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row};

/// Largest scale the insert commands take.
pub(crate) const MAX_SCALE: f64 = 1e5;

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
    /// The drawing's own width and height in the file, when it could be read (at `dpi`).
    pub natural: Option<(f64, f64)>,
    /// The dots per inch an SVG's pixels are read at; 96 is CSS's, and what the command does
    /// without one. Not used for a DXF (#20240).
    pub dpi: f64,
    /// The middle of the drawing's box in the file's own coordinates, when it could be read.
    pub middle: Option<(f64, f64)>,
    pub align: Align,
}

/// The dots per inch an SVG's pixels are read at unless the dialog says otherwise.
pub(crate) const DEFAULT_DPI: f64 = 96.0;

/// Open the placement for `path`, picked for `command` (`sketch.insert_svg` / `_dxf`).
pub fn open(app: &mut SolveApp, command: &str, path: String) {
    let (natural, middle) = bounds(app, command, &path, DEFAULT_DPI);
    let place =
        Place { command: command.to_string(), path, x: 0.0, y: 0.0, scale: 1.0, angle: 0.0, natural, dpi: DEFAULT_DPI, middle, align: Align::Middle };
    app.dialog = Some(Dialog::new(Kind::InsertDrawing(place), Vec::new()));
}

/// The drawing's own width and height, and the middle of its box, in the file; an SVG's pixels
/// are read at `dpi`.
fn bounds(app: &mut SolveApp, command: &str, path: &str, dpi: f64) -> (Option<(f64, f64)>, Option<(f64, f64)>) {
    let svg = !command.ends_with("dxf");
    let mut params = json!({"path": path, "format": if svg { "svg" } else { "dxf" }});
    if svg && dpi != DEFAULT_DPI {
        params["dpi"] = json!(dpi);
    }
    let bounds = app.session.execute("sketch.drawing_bounds", &params).ok();
    let natural = bounds
        .as_ref()
        .and_then(|b| Some((b.get("width")?.as_f64()?, b.get("height")?.as_f64()?)))
        .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 1e-9 && *h > 1e-9);
    let corner = |b: &Value, key: &str| -> Option<(f64, f64)> { Some((b.get(key)?.get(0)?.as_f64()?, b.get(key)?.get(1)?.as_f64()?)) };
    let middle = bounds
        .as_ref()
        .and_then(|b| Some((corner(b, "min")?, corner(b, "max")?)))
        .map(|(lo, hi)| ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
        .filter(|(x, y)| x.is_finite() && y.is_finite());
    (natural, middle)
}

pub fn title(p: &Place) -> &'static str {
    if p.command.ends_with("dxf") { "INSERT DXF" } else { "INSERT SVG" }
}

pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, p: &mut Place) {
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
    // An SVG's pixels are read at this resolution: a file drawn at 72 dpi comes in at the size it
    // was drawn at with 72 (#20240). The file's own size follows it.
    if !p.command.ends_with("dxf") {
        form_row(ui, "Resolution", |ui| {
            let r = crate::ui_kit::number_field(ui, &mut p.dpi, 1.0..=10000.0, 1.0, " dpi");
            crate::scenario::publish_handle("dialog:insert:dpi", r.rect.center());
            if r.changed() && p.dpi.is_finite() && p.dpi >= 1.0 {
                (p.natural, p.middle) = bounds(app, &p.command, &p.path, p.dpi);
            }
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
    let mut params = json!({"path": p.path, "at": [p.x, p.y], "scale": p.scale, "angle": p.angle, "align": p.align.word()});
    if !p.command.ends_with("dxf") && p.dpi != DEFAULT_DPI {
        if !(p.dpi.is_finite() && (1.0..=10000.0).contains(&p.dpi)) {
            return Err("the resolution must be from 1 to 10000 dpi".into());
        }
        params["dpi"] = json!(p.dpi);
    }
    Ok(vec![(p.command.clone(), params)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    fn svg_file(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-insert-place-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><rect x="0" y="0" width="20" height="10"/></svg>"#).unwrap();
        path.to_string_lossy().to_string()
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

    /// An SVG is read at 96 dpi unless the Resolution row says otherwise, and only an SVG has the
    /// row (#20240): the command carries `dpi` only when it is not the default.
    #[test]
    fn an_svg_is_placed_at_the_resolution_it_was_drawn_at() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("dpi.svg"));
        h.frames(2);
        assert!(handle_at("dialog:insert:dpi").is_some(), "the resolution is drawn for an SVG");
        let svg = Place {
            command: "sketch.insert_svg".into(),
            path: "a.svg".into(),
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            angle: 0.0,
            natural: None,
            dpi: DEFAULT_DPI,
            middle: None,
            align: Align::Middle,
        };
        assert!(commands(&svg).unwrap()[0].1.get("dpi").is_none(), "96 dpi is what the command does without one");
        assert_eq!(commands(&Place { dpi: 72.0, ..svg.clone() }).unwrap()[0].1["dpi"], json!(72.0));
        for bad in [0.0, 0.5, 20000.0, f64::NAN] {
            assert!(commands(&Place { dpi: bad, ..svg.clone() }).is_err(), "{bad}");
        }
        let dxf = Place { command: "sketch.insert_dxf".into(), dpi: 72.0, ..svg };
        assert!(commands(&dxf).unwrap()[0].1.get("dpi").is_none(), "a DXF has no pixels");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_dxf", svg_file("dpi.dxf"));
        h.frames(2);
        assert!(handle_at("dialog:insert:dpi").is_none(), "no resolution for a DXF");
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
            dpi: DEFAULT_DPI,
            middle: None,
            align: Align::Middle,
        };
        assert!(commands(&bad).is_err());
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert!(lines(&h).is_empty(), "nothing added");
    }
}
