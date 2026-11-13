//! Sizes a command works out from what it was given keep the meaning those inputs had when the
//! design's units change (#716): a sphere or a cylinder made with a bare `diameter`, a coil's
//! pitch or revolutions from its height, a box placed by its centre, a Press Pull. A radius from
//! a bare diameter used to be stored as `(20) / 2`: a unit-less expression that is not a bare
//! number is read in the design's units of the moment, so switching a millimetre design to
//! inches made such a sphere 25.4 times larger.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// A body's volume (mm³) and bounding box (min x, y, z, max x, y, z in mm).
#[derive(Debug, Clone, Copy)]
struct Shape {
    volume: f64,
    bbox: [f64; 6],
}

impl Shape {
    fn size(&self, axis: usize) -> f64 {
        self.bbox[axis + 3] - self.bbox[axis]
    }
    fn same(&self, other: &Shape) -> bool {
        rel(self.volume, other.volume) < 1e-9 && self.bbox.iter().zip(&other.bbox).all(|(a, b)| (a - b).abs() < 1e-7)
    }
}

/// Every body by name.
fn shapes(s: &mut Session) -> Vec<(String, Shape)> {
    measured(s, json!({}))
}

fn measured(s: &mut Session, p: Value) -> Vec<(String, Shape)> {
    let m = run(s, "inspect.measure", p);
    let bodies = m["bodies"].as_array().cloned().unwrap_or_default();
    bodies
        .iter()
        .map(|b| {
            let c = |k: &str, i: usize| b["bbox"][k][i].as_f64().unwrap_or(f64::NAN);
            let bbox = [c("min", 0), c("min", 1), c("min", 2), c("max", 0), c("max", 1), c("max", 2)];
            (b["name"].as_str().unwrap_or_default().to_string(), Shape { volume: b["volume_mm3"].as_f64().unwrap_or(f64::NAN), bbox })
        })
        .collect()
}

/// One body, measured alone.
fn shape(s: &mut Session, body: &str) -> Shape {
    measured(s, json!({ "bodies": [body] })).into_iter().next().map(|(_, x)| x).unwrap_or_else(|| panic!("no body {body}"))
}

/// Switch the design to each unit in turn; every body keeps its volume and its bounding box.
fn keeps_shape_through(s: &mut Session, units: &[&str]) {
    let before = shapes(s);
    assert!(!before.is_empty());
    for u in units {
        let r = run(s, "document.units", json!({ "units": u }));
        let now = shapes(s);
        assert_eq!(now.len(), before.len(), "{u}: {r}");
        for ((name, a), (_, b)) in before.iter().zip(&now) {
            assert!(a.same(b), "{name} after switching to {u} ({r}): {a:?} became {b:?}");
        }
    }
}

/// The expressions of a feature's inputs.
fn inputs(s: &Session, feature: usize) -> Vec<String> {
    s.doc.features.get(feature).map(|f| f.kind.inputs().into_iter().map(|(_, e, _)| e).collect()).unwrap_or_default()
}

#[test]
fn bare_diameters_keep_their_size_when_the_units_change() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"diameter": 20, "body_name": "Ball"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 20, "height": 10, "base": [50, 0, 0], "body_name": "Can"}));
    let ball = shape(&mut s, "Ball");
    assert!(rel(ball.volume, 4.0 / 3.0 * PI * 1000.0) < 1e-3, "{ball:?}");
    // The bounding box comes from the tessellation: a little inside the surface.
    assert!((ball.size(0) - 20.0).abs() < 1e-3 && (ball.bbox[0] + 10.0).abs() < 1e-3, "{ball:?}");
    let can = shape(&mut s, "Can");
    assert!(rel(can.volume, PI * 100.0 * 10.0) < 1e-3, "{can:?}");
    assert!((can.bbox[0] - 40.0).abs() < 1e-3 && (can.bbox[3] - 60.0).abs() < 1e-3 && (can.size(2) - 10.0).abs() < 1e-6, "{can:?}");
    keeps_shape_through(&mut s, &["in", "cm", "ft", "mm", "in"]);
    // The radius says what the diameter was, with the unit it was typed in.
    assert_eq!(inputs(&s, 0), ["(20 mm) / 2"]);
    assert_eq!(inputs(&s, 1), ["(20 mm) / 2", "10"]);
}

#[test]
fn bare_diameters_typed_in_an_inch_design_mean_inches() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "solid.sphere", json!({"diameter": 2, "body_name": "Ball"}));
    // The base is a point: its bare numbers are millimetres.
    run(&mut s, "solid.cylinder", json!({"diameter": 1, "height": 2, "base": [100, 0, 0], "body_name": "Can"}));
    let ball = shape(&mut s, "Ball");
    assert!(rel(ball.volume, 4.0 / 3.0 * PI * 25.4f64.powi(3)) < 1e-3, "{ball:?}");
    assert!((ball.size(0) - 50.8).abs() < 1e-3, "{ball:?}");
    let can = shape(&mut s, "Can");
    assert!(rel(can.volume, PI * 12.7 * 12.7 * 50.8) < 1e-3, "{can:?}");
    assert!((can.bbox[0] - 87.3).abs() < 1e-3 && (can.size(2) - 50.8).abs() < 1e-6, "{can:?}");
    keeps_shape_through(&mut s, &["mm", "cm", "in"]);
}

/// A body's extent: the diagonal of its vertices' bounding box. Measuring a coil takes seconds;
/// this does not.
fn extent(s: &Session, body: &str) -> f64 {
    s.model.state().body(body).map_or(f64::NAN, |b| b.body.size())
}

#[test]
fn coil_pitch_and_revolutions_keep_their_size_when_the_units_change() {
    use std::f64::consts::TAU;
    let mut s = Session::default();
    // Height and revolutions give the pitch.
    run(&mut s, "solid.coil", json!({"diameter": 40, "revolutions": 2, "height": 16, "section_size": 4, "body_name": "A"}));
    // Height and pitch give the revolutions: a height with its unit next to a bare pitch, both
    // millimetres here.
    run(&mut s, "solid.coil", json!({"diameter": 40, "height": "16 mm", "pitch": 8, "section_size": 4, "base": [100, 0, 0], "body_name": "B"}));
    let made = shapes(&mut s);
    let want = PI * 4.0 * TAU * 20.0 * 2.0;
    for (body, x) in &made {
        assert!(rel(x.volume, want) < 1e-3, "{body}: {x:?}");
        assert!((x.size(2) - made[0].1.size(2)).abs() < 1e-6, "{body} is as tall as A: {made:?}");
    }
    assert_eq!(inputs(&s, 0)[1], "(16 mm) / (2)", "A's pitch");
    assert_eq!(inputs(&s, 1)[2], "(16 mm) / (8 mm)", "B's revolutions");
    let sizes = |s: &Session| [extent(s, "A"), extent(s, "B")];
    let before = sizes(&s);
    for u in ["in", "cm", "mm"] {
        run(&mut s, "document.units", json!({ "units": u }));
        let now = sizes(&s);
        assert!(now.iter().zip(&before).all(|(a, b)| rel(*a, *b) < 1e-9), "{u}: {before:?} became {now:?}");
    }
    // In an inch design the same bare numbers are inches.
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "solid.coil", json!({"diameter": 2, "revolutions": 2, "height": 0.8, "section_size": 0.2, "body_name": "A"}));
    let a = shape(&mut s, "A");
    assert!(rel(a.volume, PI * 2.54f64.powi(2) * TAU * 25.4 * 2.0) < 1e-3, "{a:?}");
    let before = extent(&s, "A");
    run(&mut s, "document.units", json!({"units": "mm"}));
    assert!(rel(extent(&s, "A"), before) < 1e-9, "{before} became {}", extent(&s, "A"));
}

#[test]
fn a_box_placed_by_its_centre_reads_its_sizes_in_the_design_units() {
    // An inch design: bare sizes are inches, the centre's bare numbers millimetres.
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 1, "height": 1, "center": [0, 0, 0], "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 1, "height": 1, "center": ["10 mm", 100, 0], "body_name": "B"}));
    let a = shape(&mut s, "A");
    assert!((a.bbox[0] + 25.4).abs() < 1e-6 && (a.bbox[3] - 25.4).abs() < 1e-6 && (a.bbox[1] + 12.7).abs() < 1e-6, "{a:?}");
    let b = shape(&mut s, "B");
    assert!((b.bbox[0] + 15.4).abs() < 1e-6 && (b.bbox[3] - 35.4).abs() < 1e-6 && (b.bbox[1] - 87.3).abs() < 1e-6, "{b:?}");
    keeps_shape_through(&mut s, &["mm", "in"]);
    // A millimetre design: a centre in inches and a bare size in millimetres.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "center": ["0.5 in", 0, 0], "body_name": "C"}));
    let c = shape(&mut s, "C");
    assert!((c.bbox[0] - 12.2).abs() < 1e-6 && (c.bbox[3] - 13.2).abs() < 1e-6, "{c:?}");
    keeps_shape_through(&mut s, &["in", "mm"]);
}

/// A box placed by its centre grows on both sides when its size changes later, also after a save and
/// reopen; a box placed by its corner keeps growing from that corner (#14487).
#[test]
fn a_box_placed_by_its_centre_stays_centred_when_its_size_changes() {
    let near = |a: [f64; 6], b: [f64; 6]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 10, "center": [0, 0, 0], "name": "Box", "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 10, "corner": [50, 0, 0], "name": "Corner", "body_name": "B"}));
    assert!(near(shape(&mut s, "A").bbox, [-10.0, -5.0, -5.0, 10.0, 5.0, 5.0]), "{:?}", shape(&mut s, "A"));
    run(&mut s, "timeline.edit", json!({"feature": "Box", "set": {"length": "30"}}));
    let a = shape(&mut s, "A");
    assert!(near(a.bbox, [-15.0, -5.0, -5.0, 15.0, 5.0, 5.0]), "{a:?}");
    assert!(rel(a.volume, 3000.0) < 1e-9, "{a:?}");
    run(&mut s, "timeline.edit", json!({"feature": "Corner", "set": {"length": "30"}}));
    assert!(near(shape(&mut s, "B").bbox, [50.0, 0.0, 0.0, 80.0, 10.0, 10.0]), "{:?}", shape(&mut s, "B"));
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut reopened, "timeline.edit", json!({"feature": "Box", "set": {"width": "20", "height": "4"}}));
    assert!(near(shape(&mut reopened, "A").bbox, [-15.0, -10.0, -2.0, 15.0, 10.0, 2.0]), "{:?}", shape(&mut reopened, "A"));
    keeps_shape_through(&mut reopened, &["in", "mm"]);
}

#[test]
fn press_pull_keeps_its_distance_when_the_units_change() {
    // A face pushed in: the cut's depth is the distance's size.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.press_pull", json!({"face": [20, 15, 20], "distance": -5}));
    assert!(rel(shape(&mut s, "Block").volume, 40.0 * 30.0 * 15.0) < 1e-9);
    keeps_shape_through(&mut s, &["in", "mm"]);
    // A fillet's face: its radius grows by the distance.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 0, 10]], "radius": 3, "name": "Round"}));
    let corner = |r: f64| (4.0 - PI) * r * r / 4.0 * 20.0;
    let on = |r: f64| {
        let d = r - r / 2f64.sqrt();
        json!([40.0 - d, d, 10.0])
    };
    run(&mut s, "solid.press_pull", json!({"face": on(3.0), "distance": 2}));
    assert!(rel(shape(&mut s, "Block").volume, 24000.0 - corner(5.0)) < 1e-4);
    keeps_shape_through(&mut s, &["in", "cm", "mm"]);
    // Pulled again in an inch design: 0.1 more is 2.54 mm more.
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "solid.press_pull", json!({"face": on(5.0), "distance": 0.1}));
    assert!(rel(shape(&mut s, "Block").volume, 24000.0 - corner(7.54)) < 1e-4, "{:?}", inputs(&s, 1));
    keeps_shape_through(&mut s, &["mm", "in"]);
}

/// A diameter that refers to a unit-less parameter is left as typed: the parameter is read in
/// the design's units, and when they change it keeps the old one written in, so the part keeps
/// its size as it did before.
#[test]
fn a_diameter_from_a_unitless_parameter_is_left_as_typed() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "20", "unit": ""}));
    run(&mut s, "solid.sphere", json!({"diameter": "w", "body_name": "Ball"}));
    assert_eq!(inputs(&s, 0), ["(w) / 2"]);
    assert!(rel(shape(&mut s, "Ball").volume, 4.0 / 3.0 * PI * 1000.0) < 1e-3);
    keeps_shape_through(&mut s, &["in", "mm"]);
}

/// Beside a unit-less expression that is not a bare number, a bare number stays bare, as it
/// did: the whole expression is read in the design's units.
#[test]
fn bare_numbers_beside_a_unitless_expression_stay_bare() {
    let mut s = Session::default();
    run(&mut s, "solid.coil", json!({"diameter": 40, "height": 16, "pitch": "2 * 4", "section_size": 4, "body_name": "A"}));
    assert_eq!(inputs(&s, 0)[2], "(16) / (2 * 4)", "the revolutions");
    assert!(s.model.result(s.doc.features[0].id).is_some_and(|r| r.error.is_none()));
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 10, "center": ["2 * 5", 0, 0], "body_name": "B"}));
    // The centre is kept as typed; the box stays centred on it (#14487).
    assert_eq!(s.doc.features[1].kind.point_expr().map(|p| p[0].to_string()), Some("2 * 5".to_string()));
    let b = shape(&mut s, "B");
    assert!(b.bbox[0].abs() < 1e-6 && (b.bbox[3] - 20.0).abs() < 1e-6, "{b:?}");
}
