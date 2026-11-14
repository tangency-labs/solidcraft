//! Insert Airfoil: once the coordinate file is picked, the section is placed on the sketch (its
//! chord in millimetres, which share of the chord the position places and the angle turns about,
//! the position, the angle, and whether it is mirrored) with a live preview, and OK adds it
//! (#14202).

use crate::ui_kit::BackingTooltipExt as _;
use serde_json::{Value, json};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row};

/// Largest chord the dialog takes, in mm.
const MAX_CHORD: f64 = 1e6;

/// The section being placed: its file, and how it goes into the sketch. `x` and `y` are where
/// the pivot lands; the section turns about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Airfoil {
    pub path: String,
    /// Millimetres, above 0.
    pub chord: f64,
    /// A share of the chord: 0 the leading edge, 0.25 the quarter chord, 1 the trailing edge.
    pub pivot: f64,
    pub x: f64,
    pub y: f64,
    /// Degrees, counter-clockwise.
    pub angle: f64,
    /// Mirrored across the chord line.
    pub flip: bool,
}

/// Open the placement for the picked `path`. The chord starts at 100 mm and the pivot at the
/// quarter chord, where wing sections are stacked and turned; the position, angle and mirror
/// start at the command's own defaults.
pub fn open(app: &mut SolveApp, path: String) {
    app.dialog =
        Some(Dialog::new(Kind::InsertAirfoil(Airfoil { path, chord: 100.0, pivot: 0.25, x: 0.0, y: 0.0, angle: 0.0, flip: false }), Vec::new()));
}

pub fn title() -> &'static str {
    "INSERT AIRFOIL"
}

pub fn rows(ui: &mut egui::Ui, a: &mut Airfoil) {
    let name = std::path::Path::new(&a.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| a.path.clone());
    form_row(ui, solidcraft_ui_text::tr("ui.file"), |ui| {
        ui.label(name).backing_on_hover_text(&a.path);
    });
    form_row(ui, solidcraft_ui_text::tr("insert_airfoil.chord"), |ui| {
        let r = crate::ui_kit::number_field(ui, &mut a.chord, 0.001..=MAX_CHORD, 1.0, " mm");
        crate::scenario::publish_handle("dialog:insert_airfoil:chord", r.rect.center());
    });
    form_row(ui, solidcraft_ui_text::tr("insert_airfoil.pivot"), |ui| {
        let r = crate::ui_kit::number_field(ui, &mut a.pivot, -10.0..=10.0, 0.01, "")
            .backing_on_hover_text(solidcraft_ui_text::tr("insert_airfoil.pivot_help"));
        crate::scenario::publish_handle("dialog:insert_airfoil:pivot", r.rect.center());
    });
    for (label, v, handle) in [("X", &mut a.x, "dialog:insert_airfoil:x"), ("Y", &mut a.y, "dialog:insert_airfoil:y")] {
        form_row(ui, label, |ui| {
            let r = crate::ui_kit::number_field(ui, v, -1e9..=1e9, 0.5, " mm");
            crate::scenario::publish_handle(handle, r.rect.center());
        });
    }
    form_row(ui, solidcraft_ui_text::tr("ui.angle"), |ui| {
        let r = crate::ui_kit::number_field(ui, &mut a.angle, -360.0..=360.0, 1.0, "\u{b0}");
        crate::scenario::publish_handle("dialog:insert_airfoil:angle", r.rect.center());
    });
    form_row(ui, "", |ui| {
        let r = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("ui.flip"), &mut a.flip);
        crate::scenario::publish_handle("dialog:insert_airfoil:flip", r.rect.center());
    });
}

/// The command OK runs (and the preview shows).
pub fn commands(a: &Airfoil) -> Result<Vec<(String, Value)>, String> {
    if !(a.chord.is_finite() && a.chord > 0.0 && a.chord <= MAX_CHORD) {
        return Err("the chord must be above 0".into());
    }
    if !a.pivot.is_finite() {
        return Err("the pivot must be a number".into());
    }
    if !(a.x.is_finite() && a.y.is_finite()) {
        return Err("the position must be a number".into());
    }
    if !a.angle.is_finite() {
        return Err("the angle must be a number".into());
    }
    Ok(vec![(
        "sketch.insert_airfoil".to_string(),
        json!({"path": a.path, "chord": a.chord, "pivot": a.pivot, "at": [a.x, a.y], "angle": a.angle, "flip": a.flip}),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    /// A six-point section with an open trailing edge, as a Selig file.
    fn dat_file(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-insert-airfoil-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, "Six\n1 0.002\n0.5 0.06\n0.1 0.04\n0 0\n0.1 -0.03\n0.5 -0.04\n1 -0.002\n").unwrap();
        path.to_string_lossy().to_string()
    }

    fn points(h: &Harness) -> Vec<solidcore_record::Vec2> {
        let st = h.app.session.world_state().unwrap();
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        sk.points.iter().map(|p| p.pos).collect()
    }

    /// How many curves the active sketch holds (a new sketch has points of its own, no curves).
    fn curves(h: &Harness) -> usize {
        let st = h.app.session.world_state().unwrap();
        st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.len()
    }

    fn has(pts: &[solidcore_record::Vec2], x: f64, y: f64) -> bool {
        pts.iter().any(|p| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9)
    }

    /// Insert Airfoil asks for the file, then places it: the chord, pivot, position and angle
    /// set where the section goes before OK adds it, in one undo step.
    #[test]
    fn a_picked_airfoil_is_placed_before_it_is_added() {
        let mut h = Harness::new();
        let path = dat_file("six.dat");
        let picked = path.clone();
        h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(Some(picked.clone())))));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        h.app.start("sketch.insert_airfoil");
        h.frames(3);
        let Some(Kind::InsertAirfoil(a)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no placement: {:?}", h.app.status) };
        assert_eq!((a.path.as_str(), a.chord, a.pivot, a.x, a.y, a.angle, a.flip), (path.as_str(), 100.0, 0.25, 0.0, 0.0, 0.0, false));
        assert!(handle_at("dialog:insert_airfoil:chord").is_some() && handle_at("dialog:insert_airfoil:flip").is_some(), "the fields are drawn");
        assert_eq!(curves(&h), 0, "nothing is added before OK");
        // The section is previewed where it would go: the spline and the trailing-edge line.
        h.step(&json!({"until": {"count": {"name": "sketch_preview_curves", "value": 2}}})).unwrap();
        if let Some(Kind::InsertAirfoil(a)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (a.chord, a.pivot, a.x, a.y) = (200.0, 0.5, 10.0, 5.0);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let pts = points(&h);
        // The pivot (half the chord) sits at (10, 5): the nose 100 mm before it, the tail 100 mm
        // after it, 0.4 mm apart; the spline and the line across the open trailing edge.
        assert!(has(&pts, -90.0, 5.0) && has(&pts, 110.0, 5.4) && has(&pts, 110.0, 4.6), "{pts:?}");
        assert_eq!(curves(&h), 2);
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert_eq!(curves(&h), 0, "one undo removes the section");
    }

    /// The angle turns the section about the pivot, and Flip mirrors it across the chord line.
    #[test]
    fn the_angle_turns_about_the_pivot_and_flip_mirrors() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, dat_file("turned.dat"));
        h.frames(2);
        assert!(handle_at("dialog:insert_airfoil:pivot").is_some() && handle_at("dialog:insert_airfoil:angle").is_some());
        if let Some(Kind::InsertAirfoil(a)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (a.chord, a.pivot, a.x, a.y, a.angle, a.flip) = (100.0, 0.25, 10.0, 5.0, 90.0, true);
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        let pts = points(&h);
        // Turned a quarter: the chord runs up y, the nose 25 mm below the pivot, the tail 75 mm
        // above it; mirrored, the upper side (the file's +y) lies at +x.
        assert!(has(&pts, 10.0, -20.0) && has(&pts, 16.0, 30.0) && has(&pts, 6.0, 30.0), "{pts:?}");
    }

    /// Cancel adds nothing; a chord of 0 or below is refused, as is one that is not a number.
    #[test]
    fn cancelling_the_placement_adds_nothing_and_a_bad_chord_is_refused() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, dat_file("cancel.dat"));
        h.frames(2);
        let good = Airfoil { path: "x.dat".into(), chord: 100.0, pivot: 0.25, x: 0.0, y: 0.0, angle: 0.0, flip: false };
        assert!(commands(&good).is_ok());
        for chord in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            assert_eq!(commands(&Airfoil { chord, ..good.clone() }).unwrap_err(), "the chord must be above 0");
        }
        assert!(commands(&Airfoil { pivot: f64::NAN, ..good.clone() }).is_err());
        assert!(commands(&Airfoil { angle: f64::INFINITY, ..good.clone() }).is_err());
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert_eq!(curves(&h), 0, "nothing added");
    }
}
