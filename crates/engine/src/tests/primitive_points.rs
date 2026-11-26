//! Primitive placement points (`base`, `corner`, `center`) take expressions like their sizes do,
//! follow parameter changes, and are refused when they cannot be read (user report: a cylinder
//! with `base: ["basin_width/2", …]` silently landed at the origin).

use serde_json::{Value, json};

use super::run;
use crate::*;

fn bbox(s: &Session, body: &str) -> ([f64; 3], [f64; 3]) {
    let b = s.model.state().body(body).map(|b| b.body.clone()).unwrap();
    let m = solidcraft_kernel::measure(&b).unwrap();
    ([m.bbox.min.x, m.bbox.min.y, m.bbox.min.z], [m.bbox.max.x, m.bbox.max.y, m.bbox.max.z])
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn cylinder_base_expressions_follow_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "basin_width", "expression": "100 mm"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": ["basin_width/2", "10 mm", "-2"], "body_name": "Pin"}));
    let (lo, hi) = bbox(&s, "Pin");
    assert!(near(lo, [45.0, 5.0, -2.0]) && near(hi, [55.0, 15.0, 8.0]), "{lo:?} {hi:?}");
    // A parameter change moves it on recompute.
    run(&mut s, "parameters.change", json!({"name": "basin_width", "expression": "60 mm"}));
    let (lo, _) = bbox(&s, "Pin");
    assert!(near(lo, [25.0, 5.0, -2.0]), "{lo:?}");
    // And it survives a save and reopen.
    let text = s.doc.to_json();
    assert!(text.contains("basin_width/2"), "{text}");
    let s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
    let (lo, _) = bbox(&s2, "Pin");
    assert!(near(lo, [25.0, 5.0, -2.0]), "{lo:?}");
}

/// A cylinder's axis takes unitless expressions, as its base does (#18822): it turns when the
/// parameter changes, follows a rename, and survives a save and reopen. Plain numbers are still
/// stored as normalized numbers, and an axis that is zero for the values now is refused.
#[test]
fn cylinder_axis_expressions_follow_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "45 deg"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 10, "axis": ["0", "-sin(t)", "-cos(t)"], "body_name": "Slant"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 10, "axis": [0, -1, -1], "base": [20, 0, 0], "body_name": "Fixed"}));
    let shifted = |(lo, hi): ([f64; 3], [f64; 3])| ([lo[0] - 20.0, lo[1], lo[2]], [hi[0] - 20.0, hi[1], hi[2]]);
    let (slant, fixed) = (bbox(&s, "Slant"), shifted(bbox(&s, "Fixed")));
    assert!(near(slant.0, fixed.0) && near(slant.1, fixed.1), "{slant:?} {fixed:?}");
    assert!(slant.0[1] < -7.0 && slant.0[2] < -7.0, "{slant:?}");
    // Turned to lie along -Y when the parameter changes.
    run(&mut s, "parameters.change", json!({"name": "t", "expression": "90 deg"}));
    let (lo, hi) = bbox(&s, "Slant");
    assert!(near(lo, [-1.0, -10.0, -1.0]) && near(hi, [1.0, 0.0, 1.0]), "{lo:?} {hi:?}");
    // A rename follows into the axis, and the parameter can't be deleted while it uses it.
    assert!(s.execute("parameters.delete", &json!({"name": "t"})).is_err());
    run(&mut s, "parameters.rename", json!({"name": "t", "new_name": "tilt"}));
    run(&mut s, "parameters.change", json!({"name": "tilt", "expression": "0 deg"}));
    let (lo, hi) = bbox(&s, "Slant");
    assert!(near(lo, [-1.0, -1.0, -10.0]) && near(hi, [1.0, 1.0, 0.0]), "{lo:?} {hi:?}");
    // Saved as expressions; plain numbers stay numbers, normalized as before.
    let text = s.doc.to_json();
    let v: Value = serde_json::from_str(&text).unwrap();
    let feats = v["features"].as_array().unwrap();
    assert_eq!(feats[0]["axis"], json!([0.0, "-sin(tilt)", "-cos(tilt)"]), "{v}");
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let fixed = feats[1]["axis"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
    assert!(fixed.len() == 3 && fixed[0] == 0.0 && (fixed[1] + h).abs() < 1e-12 && (fixed[2] + h).abs() < 1e-12, "{v}");
    let mut s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
    let (lo, _) = bbox(&s2, "Slant");
    assert!(near(lo, [-1.0, -1.0, -10.0]), "{lo:?}");
    run(&mut s2, "parameters.change", json!({"name": "tilt", "expression": "90 deg"}));
    let (lo, _) = bbox(&s2, "Slant");
    assert!(near(lo, [-1.0, -10.0, -1.0]), "{lo:?}");
    // An axis that is zero for the values now is refused, not built along some other line.
    let before = s.doc.features.len();
    assert!(s.execute("solid.cylinder", &json!({"radius": 1, "height": 2, "axis": ["sin(tilt)", 0, 0]})).is_err());
    assert_eq!(s.doc.features.len(), before);
}

/// Inside a component turned a quarter turn about Z, an axis of expressions is turned into the
/// component's frame (#18822), as `[x, y, z]` or `{x, y, z}`, landing where the same direction
/// given as numbers lands. X, Y or Z by name stays the component's own axis, as before, for the
/// cylinder, the cone and the coil alike.
#[test]
fn cylinder_axis_forms_turn_with_a_turned_component() {
    let world_box_of = |cmd: &str, params: Value| {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "t", "expression": "45 deg"}));
        let c = run(&mut s, "component.create", json!({"name": "Arm", "grounded": false}));
        let (cid, occ) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "occurrence.move", json!({"occurrence": occ, "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]}));
        run(&mut s, "component.activate", json!({"component": cid}));
        run(&mut s, cmd, params);
        let b = s.world_state().unwrap().body("C").unwrap().mesh().bounds();
        ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z])
    };
    let world_box = |axis: Value| world_box_of("solid.cylinder", json!({"radius": 1, "height": 10, "axis": axis, "body_name": "C"}));
    let same = |a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])| near(a.0, b.0) && near(a.1, b.1);
    let slant = world_box(json!([0, -1, -1]));
    assert!(slant.0[1] < -6.0 && slant.0[2] < -6.0, "{slant:?}");
    for axis in [json!(["0", "-sin(t)", "-cos(t)"]), json!({"x": "0", "y": "-sin(t)", "z": "-cos(t)"}), json!([0, "-sin(t) * sqrt(2)", -1])] {
        assert!(same(world_box(axis.clone()), slant), "{axis}: {:?} vs {slant:?}", world_box(axis.clone()));
    }
    // The component's X is the world's Y after the quarter turn; its Y is the world's -X.
    let own_x = world_box(json!([0, 1, 0]));
    assert!(own_x.1[1] > 9.0 && own_x.1[0] < 1.5, "{own_x:?}");
    for axis in [json!("X"), json!(" x ")] {
        assert!(same(world_box(axis.clone()), own_x), "{axis}: {:?} vs {own_x:?}", world_box(axis.clone()));
    }
    let cone = |axis: Value| world_box_of("solid.cone", json!({"radius": 2, "top_radius": 1, "height": 10, "axis": axis, "body_name": "C"}));
    assert!(same(cone(json!("X")), cone(json!([0, 1, 0]))), "{:?} vs {:?}", cone(json!("X")), cone(json!([0, 1, 0])));
    assert!(same(cone(json!(" y ")), cone(json!([-1, 0, 0]))), "{:?}", cone(json!(" y ")));
    let coil = |axis: Value| {
        world_box_of("solid.coil", json!({"diameter": 10, "revolutions": 2, "pitch": 5, "section_size": 1, "axis": axis, "body_name": "C"}))
    };
    let (named, numbers) = (coil(json!("X")), coil(json!([0, 1, 0])));
    assert!(named.1[1] > 9.0 && numbers.1[1] > 9.0, "{named:?} vs {numbers:?}");
}

/// An axis that uses no parameter is saved normalized, as numbers, whatever form it was given
/// in; a short axis keeps its direction; the parameters it uses are named as its Axis.
#[test]
fn cylinder_axis_constants_short_axes_and_users() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "axis": [0, "0", "2"], "body_name": "A"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "axis": [0, 0, 1e-11], "base": [10, 0, 0], "body_name": "B"}));
    let v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert_eq!(v["features"][0]["axis"], json!([0.0, 0.0, 1.0]), "{v}");
    assert_eq!(v["features"][1]["axis"], json!([0.0, 0.0, 1.0]), "{v}");
    // A long axis is a direction too, given as numbers or by expressions.
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "axis": [1e300, 1e300, 0], "base": [0, 10, 0], "body_name": "L"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "axis": ["1e12", 0, 0], "base": [0, 20, 0], "body_name": "M"}));
    let v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let long: Vec<f64> = v["features"][2]["axis"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!((long[0] - h).abs() < 1e-12 && (long[1] - h).abs() < 1e-12 && long[2] == 0.0, "{v}");
    assert_eq!(v["features"][3]["axis"], json!([1.0, 0.0, 0.0]), "{v}");
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "45 deg"}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 10, "axis": ["0", "0", "sin(t)"], "base": [20, 0, 0], "body_name": "C"}));
    for tiny in ["1e-9 deg", "1e-300 deg"] {
        run(&mut s, "parameters.change", json!({"name": "t", "expression": tiny}));
        let (lo, hi) = bbox(&s, "C");
        assert!(near(lo, [19.0, -1.0, 0.0]) && near(hi, [21.0, 1.0, 10.0]), "{tiny}: {lo:?} {hi:?}");
    }
    // A parameter added as a plain number is a length: refused as it is, with the way out.
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "2"}));
    let length = s.execute("solid.cylinder", &json!({"radius": 1, "height": 2, "axis": ["n", 0, 0]})).unwrap_err().to_string();
    assert!(length.contains("n / 1 mm"), "{length}");
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "axis": ["n / 1 mm", 0, 0], "base": [30, 0, 0], "body_name": "D"}));
    let refused = s.execute("parameters.delete", &json!({"name": "t"})).unwrap_err().to_string();
    assert!(refused.contains("Axis") && !refused.contains("Position"), "{refused}");
}

#[test]
fn primitive_points_take_expressions() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": ["w", 0, 0], "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": "w", "width": 10, "height": 10, "center": [0, "w", 0], "body_name": "B"}));
    run(&mut s, "solid.sphere", json!({"radius": 2, "center": ["w * 2", 0, 0], "body_name": "C"}));
    run(&mut s, "solid.torus", json!({"major": 5, "minor": 1, "center": [0, 0, "w"], "body_name": "D"}));
    run(&mut s, "solid.coil", json!({"diameter": 10, "revolutions": 2, "pitch": 5, "section_size": 1, "base": ["-w", 0, 0], "body_name": "E"}));
    assert!(near(bbox(&s, "A").0, [40.0, 0.0, 0.0]));
    assert!(near(bbox(&s, "B").0, [-20.0, 35.0, -5.0]), "{:?}", bbox(&s, "B"));
    assert!(near(bbox(&s, "C").0, [78.0, -2.0, -2.0]));
    assert!(near(bbox(&s, "D").0, [-6.0, -6.0, 39.0]));
    assert!((bbox(&s, "E").0[0] + 45.5).abs() < 0.2, "{:?}", bbox(&s, "E"));
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "20 mm"}));
    assert!(near(bbox(&s, "A").0, [20.0, 0.0, 0.0]));
    assert!(near(bbox(&s, "B").0, [-10.0, 15.0, -5.0]), "{:?}", bbox(&s, "B"));
    assert!(near(bbox(&s, "C").0, [38.0, -2.0, -2.0]));
    assert!(near(bbox(&s, "D").0, [-6.0, -6.0, 19.0]));
    assert!((bbox(&s, "E").0[0] + 25.5).abs() < 0.2, "{:?}", bbox(&s, "E"));
}

/// Points and axes that cannot be read are refused, never silently put at the origin.
#[test]
fn unreadable_primitive_points_are_refused() {
    let mut s = Session::default();
    let cases: Vec<(&str, Value)> = vec![
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": ["nope/2", 0, 0]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": [1, 2]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": "XY"})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "base": [1, 2, "5 deg"]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "axis": ["x", 0, 1]})),
        ("solid.cylinder", json!({"radius": 5, "height": 10, "axis": [0, 0, 0]})),
        ("solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [0, null, 0]})),
        ("solid.box", json!({"length": 1, "width": 1, "height": 1, "center": {"x": 1}})),
        ("solid.sphere", json!({"radius": 1, "center": ["a +", 0, 0]})),
        ("solid.torus", json!({"major": 5, "minor": 1, "center": [1e300, 0, 0]})),
        ("solid.coil", json!({"diameter": 10, "revolutions": 2, "pitch": 5, "section_size": 1, "base": [true, 0, 0]})),
    ];
    for (cmd, p) in cases {
        let before = s.doc.features.len();
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p} should be refused");
        assert_eq!(s.doc.features.len(), before, "{cmd} {p} added a feature");
    }
}

/// Renaming a parameter follows into placement points; deleting it is refused while a point
/// uses it. Plain-number points are stored as numbers, as before.
#[test]
fn point_parameters_rename_and_store() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "off", "expression": "30 mm"}));
    run(&mut s, "solid.sphere", json!({"radius": 2, "center": ["off", 0, 0], "body_name": "S"}));
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [1.5, -2, 0], "body_name": "B"}));
    assert!(s.execute("parameters.delete", &json!({"name": "off"})).is_err());
    run(&mut s, "parameters.rename", json!({"name": "off", "new_name": "shift"}));
    run(&mut s, "parameters.change", json!({"name": "shift", "expression": "10 mm"}));
    assert!(near(bbox(&s, "S").0, [8.0, -2.0, -2.0]), "{:?}", bbox(&s, "S"));
    let v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let feats = v["features"].as_array().unwrap();
    assert_eq!(feats[0]["center"], json!(["shift", 0.0, 0.0]), "{v}");
    assert_eq!(feats[1]["corner"], json!([1.5, -2.0, 0.0]), "{v}");
}

/// A primitive placed by an expression in a moved component lands in that component's frame
/// and keeps following its parameter.
#[test]
fn point_expressions_in_a_moved_component() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "x0", "expression": "10 mm"}));
    let c = run(&mut s, "component.create", json!({"name": "Arm", "grounded": false}));
    let (cid, occ) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 50, 0]}));
    run(&mut s, "component.activate", json!({"component": cid}));
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 2, "base": ["x0", 60, 0], "body_name": "Pin"}));
    let wlo = |s: &Session| s.world_state().unwrap().body("Pin").unwrap().mesh().bounds().min;
    let lo = wlo(&s);
    assert!((lo.x - 9.0).abs() < 1e-3 && (lo.y - 59.0).abs() < 1e-3, "{lo:?}");
    run(&mut s, "parameters.change", json!({"name": "x0", "expression": "20 mm"}));
    let lo = wlo(&s);
    assert!((lo.x - 19.0).abs() < 1e-3 && (lo.y - 59.0).abs() < 1e-3, "{lo:?}");
}

// The cone primitive (#5641): sizes from the analytic formulas, placement, operations, hostile
// sizes, saving and editing.

/// V = πh(R² + Rr + r²)/3.
fn cone_volume(r: f64, top: f64, h: f64) -> f64 {
    std::f64::consts::PI * h * (r * r + r * top + top * top) / 3.0
}

/// The base, the top and the side: πR² + πr² + π(R + r)·√(h² + (R − r)²).
fn cone_area(r: f64, top: f64, h: f64) -> f64 {
    std::f64::consts::PI * (r * r + top * top + (r + top) * (h * h + (r - top) * (r - top)).sqrt())
}

fn measured(s: &Session, body: &str) -> solidcraft_kernel::BodyMeasure {
    solidcraft_kernel::measure(&s.model.state().body(body).map(|b| b.body.clone()).unwrap()).unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

#[test]
fn cone_and_frustum_match_the_analytic_volume_and_area() {
    let mut s = Session::default();
    run(&mut s, "solid.cone", json!({"radius": 10, "top_radius": 4, "height": 15, "body_name": "Frustum"}));
    run(&mut s, "solid.cone", json!({"diameter": 12, "height": 9, "base": [40, 0, 0], "body_name": "Cone"}));
    run(&mut s, "solid.cone", json!({"radius": 5, "top_diameter": 16, "height": 6, "base": [0, 40, 0], "body_name": "Cup"}));
    for (body, r, top, h) in [("Frustum", 10.0, 4.0, 15.0), ("Cone", 6.0, 0.0, 9.0), ("Cup", 5.0, 8.0, 6.0)] {
        let m = measured(&s, body);
        assert!(rel(m.volume, cone_volume(r, top, h)) < 2e-3, "{body}: volume {} vs {}", m.volume, cone_volume(r, top, h));
        assert!(rel(m.area, cone_area(r, top, h)) < 2e-3, "{body}: area {} vs {}", m.area, cone_area(r, top, h));
    }
    // A full cone has its base and its side, a frustum its top too (each one merged surface).
    let faces = |body: &str| measured(&s, body).merged.faces;
    assert_eq!((faces("Cone"), faces("Frustum")), (2, 3));
    let (lo, hi) = bbox(&s, "Cone");
    assert!(near(lo, [34.0, -6.0, 0.0]) && near(hi, [46.0, 6.0, 9.0]), "{lo:?} {hi:?}");
    let (lo, hi) = bbox(&s, "Cup");
    assert!(near(lo, [-8.0, 32.0, 0.0]) && near(hi, [8.0, 48.0, 6.0]), "{lo:?} {hi:?}");
}

#[test]
fn cone_takes_any_axis_base_and_a_negative_height() {
    let mut s = Session::default();
    run(&mut s, "solid.cone", json!({"radius": 10, "height": 20, "base": [5, -3, 2], "axis": "X", "body_name": "AlongX"}));
    let (lo, hi) = bbox(&s, "AlongX");
    assert!(near(lo, [5.0, -13.0, -8.0]) && near(hi, [25.0, 7.0, 12.0]), "{lo:?} {hi:?}");
    // A negative height goes down the axis from the base.
    run(&mut s, "solid.cone", json!({"radius": 3, "top_radius": 1, "height": -8, "base": [0, 50, 0], "body_name": "Down"}));
    let (lo, hi) = bbox(&s, "Down");
    assert!(near(lo, [-3.0, 47.0, -8.0]) && near(hi, [3.0, 53.0, 0.0]), "{lo:?} {hi:?}");
    // A slanted axis: the apex is the height along it; the volume does not depend on it.
    let d = [1.0 / 3f64.sqrt(); 3];
    run(&mut s, "solid.cone", json!({"radius": 4, "height": 12, "base": [100, 0, 0], "axis": [1, 1, 1], "body_name": "Slanted"}));
    // (Revolved in that frame directly, the apex came out wrong: 3 % too little volume, 11 %
    // too much area.)
    let m = measured(&s, "Slanted");
    assert!(rel(m.volume, cone_volume(4.0, 0.0, 12.0)) < 2e-3, "{}", m.volume);
    assert!(rel(m.area, cone_area(4.0, 0.0, 12.0)) < 2e-3, "{}", m.area);
    run(
        &mut s,
        "solid.cone",
        json!({"radius": 4, "top_radius": 2, "height": 12, "base": [0, 0, 0], "axis": [1, 1, 1], "body_name": "SlantedFrustum"}),
    );
    let m = measured(&s, "SlantedFrustum");
    assert!(rel(m.volume, cone_volume(4.0, 2.0, 12.0)) < 2e-3 && rel(m.area, cone_area(4.0, 2.0, 12.0)) < 2e-3, "{m:?}");
    let (_, hi) = bbox(&s, "Slanted");
    assert!(hi.iter().zip([100.0, 0.0, 0.0]).zip(d).all(|((h, b), d)| *h >= b + 12.0 * d - 1e-3), "the apex lies inside: {hi:?}");
    // The axis is followed as the parameters of an expression base move it.
    run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    run(&mut s, "solid.cone", json!({"radius": 2, "height": 4, "base": [0, 0, "lift"], "axis": "Y", "body_name": "Lifted"}));
    assert!(near(bbox(&s, "Lifted").0, [-2.0, 0.0, 5.0]), "{:?}", bbox(&s, "Lifted"));
    run(&mut s, "parameters.change", json!({"name": "lift", "expression": "17 mm"}));
    assert!(near(bbox(&s, "Lifted").0, [-2.0, 0.0, 15.0]), "{:?}", bbox(&s, "Lifted"));
}

#[test]
fn cone_joins_cuts_and_intersects_like_the_other_primitives() {
    // A 5 mm block with the cone standing on its bottom: the lower half of the cone (radius 6 to
    // 4) is in it, the upper half (4 to 2) above it.
    let block = 20.0 * 20.0 * 5.0;
    let (inside, above) = (cone_volume(6.0, 4.0, 5.0), cone_volume(4.0, 2.0, 5.0));
    for (operation, want) in [("join", block + above), ("cut", block - inside), ("intersect", inside)] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [-10, -10, 0], "body_name": "Block"}));
        run(&mut s, "solid.cone", json!({"radius": 6, "top_radius": 2, "height": 10, "operation": operation, "targets": ["Block"]}));
        let state = s.model.state();
        assert_eq!(state.bodies.len(), 1, "{operation}: {:?}", state.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
        let got = solidcraft_kernel::measure(&state.bodies[0].body).unwrap().volume;
        assert!(rel(got, want) < 2e-3, "{operation}: {got} vs {want}");
    }
    // A new body leaves the block alone.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [-10, -10, 0], "body_name": "Block"}));
    run(&mut s, "solid.cone", json!({"radius": 6, "top_radius": 2, "height": 10, "operation": "new_body", "body_name": "Spike"}));
    assert!(rel(measured(&s, "Block").volume, block) < 1e-9);
    assert!(rel(measured(&s, "Spike").volume, cone_volume(6.0, 2.0, 10.0)) < 2e-3);
}

/// Sizes a cone cannot have are refused with an error: no panic, no feature added.
#[test]
fn hostile_cone_sizes_are_refused() {
    let mut s = Session::default();
    let cases: Vec<Value> = vec![
        json!({"radius": 0, "height": 10}),
        json!({"radius": -5, "height": 10}),
        json!({"radius": 5, "top_radius": -1, "height": 10}),
        json!({"radius": 5, "height": 0}),
        json!({"radius": 5}),
        json!({"height": 5}),
        json!({"radius": 1e300, "height": 10}),
        json!({"radius": 5, "top_radius": 1e300, "height": 10}),
        json!({"radius": 5, "height": 1e300}),
        json!({"radius": 5, "height": -1e300}),
        json!({"radius": "0/0", "height": 10}),
        json!({"radius": "sqrt(-1)", "height": 10}),
        json!({"radius": 5, "top_radius": "nope", "height": 10}),
        json!({"radius": 5, "height": "5 deg"}),
        json!({"radius": 5, "height": 10, "axis": [0, 0, 0]}),
        json!({"radius": 5, "height": 10, "axis": [0, 0, "0/0"]}),
        json!({"radius": 5, "height": 10, "axis": "W"}),
        json!({"radius": 5, "height": 10, "base": [1e300, 0, 0]}),
        json!({"radius": 5, "height": 10, "base": [0, "x +", 0]}),
        json!({"radius": 5, "height": 10, "operation": "explode"}),
        json!({"radius": 5, "height": 10, "operation": "cut", "targets": ["Nowhere"]}),
        json!({"radius": [5], "height": {"a": 1}}),
        json!(null),
        json!("cone"),
    ];
    for p in cases {
        let before = s.doc.features.len();
        let r = s.execute("solid.cone", &p);
        assert!(r.is_err(), "{p} should be refused: {r:?} {:?}", s.doc.features.last().map(|f| &f.kind));
        assert_eq!(s.doc.features.len(), before, "{p} added a feature");
    }
    // A parameter that later makes the cone impossible fails the feature, never the program,
    // and a good value brings it back.
    run(&mut s, "parameters.change", json!({"name": "rr", "expression": "5 mm"}));
    let made = run(&mut s, "solid.cone", json!({"radius": "rr", "height": 10, "body_name": "Flex"}));
    let id = made["feature"].as_u64().unwrap();
    for bad in ["-5 mm", "0 mm", "1e300 mm"] {
        let _ = s.execute("parameters.change", &json!({"name": "rr", "expression": bad}));
        assert!(s.model.result(id).is_none_or(|r| r.error.is_some()) || s.model.state().body("Flex").is_none(), "{bad}");
    }
    run(&mut s, "parameters.change", json!({"name": "rr", "expression": "3 mm"}));
    assert!(rel(measured(&s, "Flex").volume, cone_volume(3.0, 0.0, 10.0)) < 2e-3);
}

/// A cone is saved with its sizes as expressions and opens again as it was; Edit Feature changes
/// it in place.
#[test]
fn cone_saves_reopens_and_edits() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "tip", "expression": "3 mm"}));
    run(
        &mut s,
        "solid.cone",
        json!({"radius": 8, "top_radius": "tip", "height": 12, "base": [1, 2, 3], "axis": "Y", "name": "Nozzle", "body_name": "N"}),
    );
    let text = s.doc.to_json();
    let v: Value = serde_json::from_str(&text).unwrap();
    let f = &v["features"][0];
    assert_eq!((&f["type"], &f["top_radius"], &f["axis"]), (&json!("cone"), &json!("tip"), &json!([0.0, 1.0, 0.0])), "{f}");
    let mut s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
    assert!(rel(measured(&s2, "N").volume, cone_volume(8.0, 3.0, 12.0)) < 2e-3);
    assert_eq!(bbox(&s2, "N"), bbox(&s, "N"));
    // Edit Feature: a full cone, taller, cutting nothing (it is the only body: a new body).
    run(&mut s2, "timeline.edit", json!({"feature": "Nozzle", "set": {"top_radius": "0", "height": "20"}}));
    assert!(rel(measured(&s2, "N").volume, cone_volume(8.0, 0.0, 20.0)) < 2e-3);
    let (lo, hi) = bbox(&s2, "N");
    assert!(near(lo, [-7.0, 2.0, -5.0]) && near(hi, [9.0, 22.0, 11.0]), "{lo:?} {hi:?}");
    // Rebuilding it from the command form works too, and undo brings the edit back.
    run(
        &mut s2,
        "timeline.redefine",
        json!({"feature": "Nozzle", "command": "solid.cone", "params": {"diameter": 10, "top_diameter": 4, "height": 6}}),
    );
    assert!(rel(measured(&s2, "N").volume, cone_volume(5.0, 2.0, 6.0)) < 2e-3);
    run(&mut s2, "edit.undo", json!({}));
    assert!(rel(measured(&s2, "N").volume, cone_volume(8.0, 0.0, 20.0)) < 2e-3);
    // The parameter still drives it after the round trip.
    let mut s3 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
    run(&mut s3, "parameters.change", json!({"name": "tip", "expression": "5 mm"}));
    assert!(rel(measured(&s3, "N").volume, cone_volume(8.0, 5.0, 12.0)) < 2e-3);
}

/// `params` with `axis`, `base` and `body_name` set.
fn with_axis(params: &Value, axis: Value, base: [f64; 3], name: &str) -> Value {
    let mut p = params.clone();
    p["axis"] = axis;
    p["base"] = json!(base);
    p["body_name"] = json!(name);
    p
}

/// Two boxes alike once the second is moved back by `dx` along X.
fn same_box_shifted(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3]), dx: f64) -> bool {
    near(a.0, [b.0[0] - dx, b.0[1], b.0[2]]) && near(a.1, [b.1[0] - dx, b.1[1], b.1[2]])
}

const CONE: (&str, &str) = ("solid.cone", r#"{"radius": 2, "top_radius": 1, "height": 10}"#);
const COIL: (&str, &str) = ("solid.coil", r#"{"diameter": 10, "revolutions": 2, "pitch": 5, "section_size": 1}"#);

/// A cone's and a coil's axis take unitless expressions, as the cylinder's does (#21932): the
/// same body as that direction in numbers, turned when the parameter changes, and kept as
/// expressions through a save and reopen.
#[test]
fn cone_and_coil_axis_expressions_follow_parameters() {
    for (cmd, size) in [CONE, COIL] {
        let size: Value = serde_json::from_str(size).unwrap();
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "t", "expression": "45 deg"}));
        run(&mut s, cmd, with_axis(&size, json!(["0", "-sin(t)", "-cos(t)"]), [0.0; 3], "Slant"));
        run(&mut s, cmd, with_axis(&size, json!([0, -1, -1]), [100.0, 0.0, 0.0], "Fixed"));
        let slant = bbox(&s, "Slant");
        assert!(same_box_shifted(slant, bbox(&s, "Fixed"), 100.0), "{cmd}: {slant:?} vs {:?}", bbox(&s, "Fixed"));
        assert!(slant.0[1] < -6.0 && slant.0[2] < -6.0, "{cmd}: {slant:?}");
        // Turned to lie along -Y when the parameter changes.
        run(&mut s, "parameters.change", json!({"name": "t", "expression": "90 deg"}));
        run(&mut s, cmd, with_axis(&size, json!([0, -1, 0]), [200.0, 0.0, 0.0], "AlongY"));
        let slant = bbox(&s, "Slant");
        assert!(same_box_shifted(slant, bbox(&s, "AlongY"), 200.0), "{cmd}: {slant:?} vs {:?}", bbox(&s, "AlongY"));
        // Saved as expressions; reopened, it still follows the parameter.
        let text = s.doc.to_json();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["features"][0]["axis"], json!([0.0, "-sin(t)", "-cos(t)"]), "{cmd}: {v}");
        let mut s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
        assert!(same_box_shifted(bbox(&s2, "Slant"), bbox(&s2, "AlongY"), 200.0), "{cmd}");
        run(&mut s2, "parameters.change", json!({"name": "t", "expression": "45 deg"}));
        assert!(same_box_shifted(bbox(&s2, "Slant"), bbox(&s2, "Fixed"), 100.0), "{cmd}");
    }
}

/// A cone's and a coil's axis of plain numbers is a direction however long or short (#21932),
/// as the cylinder's is, saved normalized as numbers. A saved axis of numbers, as older designs
/// hold, reads back to the same body.
#[test]
fn cone_and_coil_take_long_and_short_plain_axes() {
    for (cmd, size) in [CONE, COIL] {
        let size: Value = serde_json::from_str(size).unwrap();
        let mut s = Session::default();
        run(&mut s, cmd, with_axis(&size, json!([1, 0, 0]), [0.0; 3], "Unit"));
        run(&mut s, cmd, with_axis(&size, json!([1e9, 0, 0]), [100.0, 0.0, 0.0], "Long"));
        run(&mut s, cmd, with_axis(&size, json!([1e-12, 0, 0]), [200.0, 0.0, 0.0], "Short"));
        run(&mut s, cmd, with_axis(&size, json!([0, -1, -1]), [300.0, 0.0, 0.0], "Slant"));
        let unit = bbox(&s, "Unit");
        assert!(same_box_shifted(unit, bbox(&s, "Long"), 100.0), "{cmd}: {unit:?} vs {:?}", bbox(&s, "Long"));
        assert!(same_box_shifted(unit, bbox(&s, "Short"), 200.0), "{cmd}: {unit:?} vs {:?}", bbox(&s, "Short"));
        let text = s.doc.to_json();
        let v: Value = serde_json::from_str(&text).unwrap();
        for i in 0..3 {
            assert_eq!(v["features"][i]["axis"], json!([1.0, 0.0, 0.0]), "{cmd}: {v}");
        }
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let slant: Vec<f64> = v["features"][3]["axis"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        assert!(slant.len() == 3 && slant[0] == 0.0 && (slant[1] + h).abs() < 1e-12 && (slant[2] + h).abs() < 1e-12, "{cmd}: {v}");
        let s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
        assert!(near(bbox(&s2, "Slant").0, bbox(&s, "Slant").0) && near(bbox(&s2, "Slant").1, bbox(&s, "Slant").1), "{cmd}");
    }
}

/// A cone's or a coil's axis that is zero, or not a finite number, is refused with what is
/// wrong, and adds no feature (#21932).
#[test]
fn cone_and_coil_refuse_zero_and_non_finite_axes() {
    for (cmd, size) in [CONE, COIL] {
        let size: Value = serde_json::from_str(size).unwrap();
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "t", "expression": "0 deg"}));
        for (axis, says) in [
            (json!([0, 0, 0]), "non-zero"),
            (json!(["sin(t)", 0, 0]), "[0, 0, 0] with these parameter values"),
            (json!(["1/0", 0, 1]), "axis x"),
            (json!(["nope", 0, 1]), "axis x"),
        ] {
            let before = s.doc.features.len();
            let err = s.execute(cmd, &with_axis(&size, axis.clone(), [0.0; 3], "Z")).unwrap_err().to_string();
            assert!(err.contains(says), "{cmd} {axis}: {err}");
            assert_eq!(s.doc.features.len(), before, "{cmd} {axis} added a feature");
        }
    }
}

/// A block 20 x 20 x 10 at `x` with a through hole of diameter 4 drilled from its top face
/// along `direction`; the block's volume after the hole.
fn drilled_block(s: &mut Session, name: &str, x: f64, direction: Value) -> f64 {
    run(s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0], "body_name": name}));
    run(s, "solid.hole", json!({"position": [x + 10.0, 14, 10], "diameter": 4, "direction": direction}));
    volume(s, name)
}

fn volume(s: &Session, body: &str) -> f64 {
    let b = s.model.state().body(body).map(|b| b.body.clone()).unwrap();
    solidcraft_kernel::measure(&b).unwrap().volume
}

/// A hole's `direction` takes unitless expressions, as a cylinder's axis does (#21932): the same
/// cut as that direction in numbers, turned when the parameter changes, kept through a save and
/// reopen. A direction of numbers is saved as numbers, normalized, as before.
#[test]
fn hole_direction_expressions_follow_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "45 deg"}));
    let slant = drilled_block(&mut s, "A", 0.0, json!(["0", "-sin(t)", "-cos(t)"]));
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let fixed = drilled_block(&mut s, "B", 100.0, json!([0, -h, -h]));
    // Through 10 mm of block at 45 degrees: about pi * 2^2 * 10 * sqrt(2) cut away.
    assert!((slant - fixed).abs() < 1e-6 * fixed, "{slant} vs {fixed}");
    assert!((4000.0 - slant - 40.0 * std::f64::consts::PI * 2f64.sqrt()).abs() < 2.0, "{slant}");
    // Turned to 20 degrees off vertical when the parameter changes: a shorter cut.
    run(&mut s, "parameters.change", json!({"name": "t", "expression": "20 deg"}));
    // sin and cos of 20 degrees.
    let (sin, cos) = (0.3420201433256687, 0.9396926207859084);
    let twenty = drilled_block(&mut s, "C", 200.0, json!([0, -sin, -cos]));
    let turned = volume(&s, "A");
    assert!((turned - twenty).abs() < 1e-6 * twenty && turned > slant + 10.0, "{turned} vs {twenty}, was {slant}");
    // Saved as expressions; a direction of numbers stays numbers.
    let text = s.doc.to_json();
    let v: Value = serde_json::from_str(&text).unwrap();
    let holes: Vec<&Value> = v["features"].as_array().unwrap().iter().filter(|f| f.get("diameter").is_some()).collect();
    assert_eq!(holes[0]["direction"], json!([0.0, "-sin(t)", "-cos(t)"]), "{v}");
    assert!(holes[1]["direction"].as_array().unwrap().iter().all(Value::is_number), "{v}");
    let mut s2 = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
    assert!((volume(&s2, "A") - twenty).abs() < 1e-6 * twenty);
    assert!((volume(&s2, "B") - fixed).abs() < 1e-6 * fixed);
    run(&mut s2, "parameters.change", json!({"name": "t", "expression": "45 deg"}));
    assert!((volume(&s2, "A") - fixed).abs() < 1e-6 * fixed);
}

/// A hole's `direction` that is zero, not a finite number, or an axis named by letter is
/// refused with what is wrong, and adds no feature (#21932).
#[test]
fn hole_refuses_zero_and_non_finite_directions() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "0 deg"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "A"}));
    for (direction, says) in [
        (json!([0, 0, 0]), "non-zero"),
        (json!("Z"), "non-zero"),
        (json!(["sin(t)", 0, 0]), "`direction` is [0, 0, 0] with these parameter values"),
        (json!(["1/0", 0, -1]), "direction x"),
        (json!(["nope", 0, -1]), "direction x"),
    ] {
        let before = s.doc.features.len();
        let err = s.execute("solid.hole", &json!({"position": [10, 10, 10], "diameter": 4, "direction": direction})).unwrap_err().to_string();
        assert!(err.contains(says), "{direction}: {err}");
        assert_eq!(s.doc.features.len(), before, "{direction} added a feature");
    }
}

/// A hole's `direction` of expressions that turns zero when a parameter changes fails its
/// rebuild naming the `direction`, the hole's own field, not an axis (#21932).
#[test]
fn hole_direction_turned_zero_names_the_direction() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "90 deg"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.hole", json!({"position": [10, 10, 10], "diameter": 4, "direction": [0, 0, "-sin(t)"], "name": "H"}));
    let changed = s.execute("parameters.change", &json!({"name": "t", "expression": "0 deg"}));
    let error = s.model.results.iter().find(|r| r.name == "H").and_then(|r| r.error.clone());
    let says = format!("{changed:?} {error:?}");
    assert!(says.contains("the direction is zero") && !says.contains("axis"), "{says}");
}
