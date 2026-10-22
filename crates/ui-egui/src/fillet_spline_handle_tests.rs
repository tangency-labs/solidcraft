use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::doc::{FeatureKind, FilletStyle, kernel};

/// A plate whose top narrows from 20 to 2 mm along 40 mm, its top front edge rounded at 3 mm
/// with Taper to Fit.
fn fixture() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [40, 0], [40, 2], [0, 20]], "closed": true})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 15, "body_name": "Plate"})).unwrap();
    h.app.run("solid.fillet", json!({"edges": [[20, 0, 15]], "radius": 3, "fit": "taper", "name": "Round"})).unwrap();
    h.frames(4);
    h
}

/// Open the fillet for editing and wait for its preview.
fn open(h: &mut Harness) {
    h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "fit"}})).unwrap();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Round"}})).unwrap();
    let began = std::time::Instant::now();
    while (h.app.preview.busy || h.app.preview.tapered.is_empty()) && began.elapsed() < std::time::Duration::from_secs(60) {
        h.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    h.frames(4);
}

fn volume(h: &Harness) -> f64 {
    kernel::measure(&h.app.session.world_state().unwrap().body("Plate").unwrap().body).unwrap().volume
}

#[test]
fn dragging_a_taper_handle_makes_the_fillet_a_spline() {
    let mut h = fixture();
    let tapered = volume(&h);
    open(&mut h);
    let (_, taper) = h.app.preview.tapered.first().cloned().expect("the preview's taper");
    let law =
        if taper.start.dist(solidcraft_engine::geom::Vec3::new(0.0, 0.0, 15.0)) < 1e-9 { taper.spline.clone() } else { taper.spline.reversed() };
    // A handle per control radius, each that radius into the body from the edge.
    let into = solidcraft_engine::geom::Vec3::new(0.0, 1.0, -1.0) * std::f64::consts::FRAC_1_SQRT_2;
    let at = |u: f64, r: f64| {
        let p = solidcraft_engine::geom::Vec3::new(40.0 * u, 0.0, 15.0) + into * r;
        json!([p.x, p.y, p.z])
    };
    let g = law.greville();
    assert_eq!(g.len(), law.radii.len());
    for (k, (u, r)) in g.iter().zip(&law.radii).enumerate() {
        let k = if taper.spline == law { k } else { g.len() - 1 - k };
        let shown = h.at(&json!({"handle": format!("fillet_r{k}")})).unwrap();
        let want = h.at(&json!({"world": at(*u, *r)})).unwrap();
        assert!((shown[0] - want[0]).hypot(shown[1] - want[1]) < 1.0, "handle {k}: {shown:?} vs {want:?}");
    }
    // The middle control radius pulled out to 4 mm (free drag): the fillet keeps the taper's
    // spline as its own with that radius changed.
    let mid = g.len() / 2;
    let k = if taper.spline == law { mid } else { g.len() - 1 - mid };
    h.step(&json!({"drag": [{"handle": format!("fillet_r{k}")}, {"world": at(g[mid], 4.0)}], "ctrl": true})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.extra.get("type").and_then(Value::as_str), Some("spline"));
    assert!(d.extra.get("fit").is_none());
    h.step(&json!({"widget": "OK"})).unwrap();
    let FeatureKind::Fillet { style: FilletStyle::Spline { knots, radii, .. }, radius, .. } = &h.app.session.doc.find_feature("Round").unwrap().kind
    else {
        panic!("a spline fillet")
    };
    assert_eq!(knots, &taper.spline.knots);
    let values: Vec<f64> =
        std::iter::once(radius).chain(radii).map(|e| h.app.session.doc.eval(e, solidcraft_engine::doc::expr::Kind::Length).unwrap()).collect();
    let moved: Vec<usize> = (0..values.len()).filter(|i| (values[*i] - taper.spline.radii[*i]).abs() > 1e-3).collect();
    assert_eq!(moved, [k], "{values:?} vs {:?}", taper.spline.radii);
    assert!((values[k] - 4.0).abs() < 0.05, "{values:?}");
    assert!(volume(&h) < tapered, "{} vs {tapered}", volume(&h));
}

/// A 40x30x20 box, its two top edges along x rounded by one cubic radius spline of two spans
/// from x = 0: control radii 2, 3, 4, 3, 2.
fn spline_fixture() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [40, 0], [40, 30], [0, 30]], "closed": true})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 20, "body_name": "Box"})).unwrap();
    h.app
        .run(
            "solid.fillet",
            json!({"edges": [[20, 0, 20], [20, 30, 20]], "radius": 2, "type": "spline", "start": [0, 0, 20],
                "knots": [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0], "radii": [3, 4, 3, 2], "name": "Round"}),
        )
        .unwrap();
    h.frames(4);
    h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "fit"}})).unwrap();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Round"}})).unwrap();
    let began = std::time::Instant::now();
    while h.app.preview.busy && began.elapsed() < std::time::Duration::from_secs(60) {
        h.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    h.frames(4);
    h
}

/// The dialog's spline: knots and control radii.
fn dialog_spline(h: &Harness) -> kernel::RadiusSpline {
    let d = h.app.dialog.as_ref().unwrap();
    let crate::dialogs::Kind::Fillet { radius, .. } = &d.kind else { panic!("a fillet dialog") };
    let knots: Vec<f64> = d.extra["knots"].as_array().unwrap().iter().filter_map(Value::as_f64).collect();
    let rest: Vec<String> = d.extra["radii"].as_array().unwrap().iter().map(|r| r.as_str().map_or_else(|| r.to_string(), str::to_owned)).collect();
    let values =
        std::iter::once(radius).chain(&rest).map(|e| h.app.session.doc.eval(e, solidcraft_engine::doc::expr::Kind::Length).unwrap()).collect();
    FilletStyle::spline(&knots, values)
}

/// A point `r` into the box from the top edge at y (0 or 30), x = 40 u.
fn into_box(u: f64, y: f64, r: f64) -> Value {
    let side = if y == 0.0 { 1.0 } else { -1.0 };
    let p = solidcraft_engine::geom::Vec3::new(40.0 * u, y + side * r * std::f64::consts::FRAC_1_SQRT_2, 20.0 - r * std::f64::consts::FRAC_1_SQRT_2);
    json!({"world": [p.x, p.y, p.z]})
}

#[test]
fn every_edge_of_a_spline_fillet_has_handles_for_its_one_spline() {
    let mut h = spline_fixture();
    let law = dialog_spline(&h);
    let g = law.greville();
    for (k, (u, r)) in g.iter().zip(&law.radii).enumerate() {
        for (name, y) in [(format!("fillet_r{k}"), 0.0), (format!("fillet_e1_r{k}"), 30.0)] {
            let shown = h.at(&json!({"handle": name})).unwrap();
            let want = h.at(&into_box(*u, y, *r)).unwrap();
            assert!((shown[0] - want[0]).hypot(shown[1] - want[1]) < 1.0, "{name}: {shown:?} vs {want:?}");
        }
    }
    // The back edge's middle handle pulled to 5 mm: the shared spline's middle radius.
    h.step(&json!({"drag": [{"handle": "fillet_e1_r2"}, into_box(g[2], 30.0, 5.0)], "ctrl": true})).unwrap();
    let moved = dialog_spline(&h);
    assert_eq!(moved.knots, law.knots);
    let changed: Vec<usize> = (0..law.radii.len()).filter(|i| (moved.radii[*i] - law.radii[*i]).abs() > 1e-3).collect();
    assert_eq!(changed, [2], "{:?}", moved.radii);
    assert!((moved.radii[2] - 5.0).abs() < 0.05, "{:?}", moved.radii);
}

#[test]
fn double_clicks_add_and_remove_control_radii() {
    let mut h = spline_fixture();
    let law = dialog_spline(&h);
    let edges = h.app.dialog.as_ref().unwrap().inputs[0].items.len();
    // On the curve at x = 12: one more control radius, the same curve.
    h.step(&json!({"click": into_box(0.3, 0.0, law.eval(0.3)), "double": true})).unwrap();
    let more = dialog_spline(&h);
    assert_eq!((more.radii.len(), more.knots.len()), (law.radii.len() + 1, law.knots.len() + 1));
    let worst = (0..=100).map(|i| f64::from(i) / 100.0).map(|t| (more.eval(t) - law.eval(t)).abs()).fold(0.0, f64::max);
    // Control radii are kept to the dialog's precision.
    assert!(worst < 1e-3, "the curve moved by {worst}");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), edges, "the double-click picked something");
    // A middle handle double-clicked: gone again. An end handle stays.
    // A pause between gestures, so egui doesn't take the next two clicks for a triple click.
    h.frames(30);
    h.step(&json!({"click": {"handle": "fillet_r1"}, "double": true})).unwrap();
    assert_eq!(dialog_spline(&h).radii.len(), law.radii.len());
    h.frames(30);
    h.step(&json!({"click": {"handle": "fillet_r0"}, "double": true})).unwrap();
    assert_eq!(dialog_spline(&h).radii.len(), law.radii.len());
    // The fillet builds with the edited spline.
    h.step(&json!({"widget": "OK"})).unwrap();
    let FeatureKind::Fillet { style: FilletStyle::Spline { radii, .. }, .. } = &h.app.session.doc.find_feature("Round").unwrap().kind else {
        panic!("a spline fillet")
    };
    assert_eq!(radii.len() + 1, law.radii.len());
    assert!(h.app.session.world_state().unwrap().body("Box").is_some());
}

#[test]
fn a_taper_over_several_edges_keeps_the_radius_arrow() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [40, 0], [40, 2], [0, 20]], "closed": true})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 15, "body_name": "Plate"})).unwrap();
    h.app.run("solid.fillet", json!({"edges": [[20, 0, 15], [20, 0, 0]], "radius": 3, "fit": "taper", "name": "Round"})).unwrap();
    h.frames(4);
    open(&mut h);
    assert_eq!(h.app.preview.tapered.len(), 2, "both edges taper");
    // One spline would replace every edge's taper, so none is offered to drag.
    assert!(crate::scenario::handles("fillet_").is_empty(), "{:?}", crate::scenario::handles("fillet_"));
    assert!(h.at(&json!({"handle": "arrow"})).is_ok());
}
