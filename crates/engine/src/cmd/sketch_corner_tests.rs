//! Sketch Fillet and Chamfer at corners (#46): line–line, line–arc and arc–arc corners, the
//! corners of a constrained rectangle, several corners at once, and radii too large for the
//! corner (a clear error that leaves the sketch as it was).

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::Session;

fn new_sketch() -> Session {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn curves(r: &Value) -> Vec<String> {
    r["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sk(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn solved(s: &Session) -> bool {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.ok()
}

fn area(s: &Session) -> f64 {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.iter().map(|p| p.area).sum()
}

#[test]
fn fillet_and_chamfer_corners() {
    use std::f64::consts::PI;
    // A constrained 40 × 20 rectangle: a fillet r5 on one corner keeps it solved and closed,
    // with the area of the corner taken off.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let a0 = area(&s);
    run(&mut s, "sketch.fillet", json!({"a": c[0], "b": c[1], "radius": 5}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{}", area(&s));
    // Several corners at once: the rest of the rectangle's corners.
    let pts: Vec<String> = {
        let k = sk(&s);
        let li = |id: &str| k.curve_index(id).unwrap();
        [(1, 2), (2, 3), (3, 0)]
            .iter()
            .map(|(x, y)| {
                let (cx, cy) = (&k.curves[li(&c[*x])].kind, &k.curves[li(&c[*y])].kind);
                let shared = cx.point_ids().into_iter().find(|p| cy.point_ids().contains(p)).unwrap();
                k.points[shared].id.clone()
            })
            .collect()
    };
    run(&mut s, "sketch.fillet", json!({"points": pts, "radius": 5}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - 4.0 * (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{}", area(&s));
    // Too large for a corner: a clear error, nothing changed.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": c[0], "b": c[1], "radius": 30})).unwrap_err().to_string();
    assert!(e.contains("too large") || e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
    let e = s.execute("sketch.chamfer.equal_distance", &json!({"a": c[0], "b": c[1], "distance": 25})).unwrap_err().to_string();
    assert!(e.contains("too large") || e.contains("does not fit") || e.contains("longer"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
    // A line meeting an arc at a corner (down onto its top), and two arcs (a slot's end rounded
    // into a line).
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [0, 10]]})))[0].clone();
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{l}.end"), "end": [10, 0], "through": [7.0710678, 7.0710678]})))
        [0]
    .clone();
    run(&mut s, "sketch.fillet", json!({"a": l, "b": a, "radius": 2}));
    assert!(solved(&s));
    let mut s = new_sketch();
    let a1 = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [-10, 0], "end": [0, 0], "through": [-5, -5]})))[0].clone();
    let a2 = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{a1}.end"), "end": [10, 0], "through": [5, 2]})))[0].clone();
    run(&mut s, "sketch.fillet", json!({"a": a1, "b": a2, "radius": 1}));
    assert!(solved(&s));
    // Chamfers keep a closed profile.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let a0 = area(&s);
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": c[0], "b": c[1], "distance": 4}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - 8.0)).abs() < 1e-6, "{}", area(&s));
}

/// Each chamfer cuts several corners at once (`points`), as a fillet rounds them: every corner
/// alike, in one edit, and a corner that can't be cut, or is named twice, refuses them all.
#[test]
fn chamfers_cut_several_corners() {
    let corners = json!(["l1.start", "l1.end", "l2.end", "l3.end"]);
    for (cmd, sizes, cut) in [
        ("sketch.chamfer.equal_distance", json!({"distance": 4}), 8.0),
        ("sketch.chamfer.two_distance", json!({"distance": 2, "distance2": 6}), 6.0),
        ("sketch.chamfer.distance_angle", json!({"distance": 3, "angle": 45}), 4.5),
    ] {
        let mut s = new_sketch();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
        run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 30}));
        let mut p = sizes.clone();
        p["points"] = corners.clone();
        let out = run(&mut s, cmd, p);
        assert_eq!(curves(&out).len(), 4, "{cmd}: {out}");
        assert_eq!(sk(&s).curves.len(), 8, "{cmd}");
        assert_eq!(out["sketch"]["dof"], 0, "{cmd}: {out}");
        assert_eq!(out["created"]["dimensions"].as_array().unwrap().len(), 8, "{cmd}: {out}");
        assert!(solved(&s), "{cmd}");
        assert!((area(&s) - (1200.0 - 4.0 * cut)).abs() < 1e-6, "{cmd}: {}", area(&s));
        let k = sk(&s);
        let first = out["created"]["dimensions"][0].as_str().unwrap();
        let param = k.constraints.iter().find(|c| c.id == first).unwrap().param.as_ref().unwrap();
        run(&mut s, "parameters.change", json!({"name": param, "expression": "5 mm"}));
        let cut = if cmd.ends_with("two_distance") { 15.0 } else { 12.5 };
        assert!(solved(&s), "{cmd}");
        assert!((area(&s) - (1200.0 - 4.0 * cut)).abs() < 1e-6, "{cmd}: {}", area(&s));
    }
    // A corner named by its two lines, beside one named by its point.
    let mut s = new_sketch();
    let c = curves(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})));
    run(&mut s, "sketch.chamfer.equal_distance", json!({"points": [{"a": c[0], "b": c[1]}, "l3.end"], "distance": 4}));
    assert!((area(&s) - (1200.0 - 2.0 * 8.0)).abs() < 1e-6, "{}", area(&s));
    // Two lines that share no end are brought to where they meet, beside a corner they already
    // make; named again by its point once they meet, that corner is listed twice.
    let mut s = new_sketch();
    let la = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let lb = curves(&run(&mut s, "sketch.line", json!({"points": [[20, 5], [20, 20]], "connect": false})))[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{lb}.end"), [0, 20]]}));
    let e = s.execute("sketch.chamfer.equal_distance", &json!({"points": [{"a": la, "b": lb}, format!("{la}.end")], "distance": 4}));
    assert!(e.unwrap_err().to_string().contains("listed twice"));
    assert_eq!(sk(&s).curves.len(), 3);
    let out = run(&mut s, "sketch.chamfer.equal_distance", json!({"points": [{"a": la, "b": lb}, format!("{lb}.end")], "distance": 4}));
    assert_eq!(curves(&out).len(), 2, "{out}");
    assert!(solved(&s));
    let refused = [
        // 20 mm back from both ends of a 30 mm side: the second corner doesn't fit.
        json!({"points": corners, "distance": 20}),
        // One corner by both of its lines' ends: cut once, the second name would be a new corner.
        json!({"points": ["l1.end", "l2.start"], "distance": 4}),
        json!({"points": [], "distance": 1}),
    ];
    for p in refused {
        let mut s = new_sketch();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        assert!(s.execute("sketch.chamfer.equal_distance", &p).is_err(), "{p}");
        assert_eq!(sk(&s).curves.len(), 4, "{p}");
        assert!((area(&s) - 1200.0).abs() < 1e-6, "{p}");
    }
}

/// #10865: the inputs remain driving dimensions, so cutting a corner adds no freedom.
#[test]
fn chamfers_keep_input_dimensions() {
    for fixed in [false, true] {
        for (cmd, sizes, second_value, second_edit, edited_cut) in [
            ("sketch.chamfer.equal_distance", json!({"distance": 3}), 3.0, "5 mm", 12.5),
            ("sketch.chamfer.two_distance", json!({"distance": 3, "distance2": 4}), 4.0, "6 mm", 15.0),
            ("sketch.chamfer.distance_angle", json!({"distance": 3, "angle": 30}), 30f64.to_radians(), "45 deg", 12.5),
        ] {
            let mut s = new_sketch();
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
            if fixed {
                run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
                run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 20}));
            }
            let before = s.doc.to_json();
            let mut p = sizes;
            p["a"] = json!("l1");
            p["b"] = json!("l2");
            let out = run(&mut s, cmd, p);
            assert_eq!(out["sketch"]["dof"], if fixed { 0 } else { 2 }, "{cmd}: {out}");
            let dimensions = out["created"]["dimensions"].as_array().unwrap();
            assert_eq!(dimensions.len(), 2, "{cmd}: {out}");
            let k = sk(&s);
            let dims: Vec<_> = dimensions.iter().map(|id| k.constraints.iter().find(|c| c.id == id.as_str().unwrap()).unwrap()).collect();
            assert!((dims[0].kind.value().unwrap() - 3.0).abs() < 1e-9);
            assert!((dims[1].kind.value().unwrap() - second_value).abs() < 1e-9);
            let params: Vec<_> = dims.iter().map(|c| c.param.clone().unwrap()).collect();
            let second_name = match cmd {
                "sketch.chamfer.equal_distance" => "chamfer_distance",
                "sketch.chamfer.two_distance" => "chamfer_second_distance",
                _ => "chamfer_angle",
            };
            assert_eq!(params, ["chamfer_distance", second_name], "{cmd}");
            assert_eq!(params[0] == params[1], cmd.ends_with("equal_distance"));
            assert_eq!(matches!(dims[1].kind, solidcraft_sketch::ConstraintKind::Angle { .. }), cmd.ends_with("distance_angle"));
            let after = s.doc.to_json();
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc.to_json(), before);
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc.to_json(), after);
            let doc = solidcraft_doc::Document::from_json(&after).unwrap();
            assert_eq!(doc.sketch(s.active_sketch.unwrap()).unwrap(), s.doc.sketch(s.active_sketch.unwrap()).unwrap());
            if fixed {
                run(&mut s, "parameters.change", json!({"name": params[0], "expression": "5 mm"}));
                run(&mut s, "parameters.change", json!({"name": params[1], "expression": second_edit}));
                let info = run(&mut s, "sketch.inspect", json!({}));
                assert_eq!(info["dof"], 0, "{cmd}: {info}");
                assert!(solved(&s), "{cmd}");
                assert!((area(&s) - (600.0 - edited_cut)).abs() < 1e-6, "{cmd}: {}", area(&s));
            }
        }
    }
}

#[test]
fn chamfer_parameter_names_preserve_existing_parameters() {
    let mut s = Session::default();
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 10}));
    let plane_param = s.doc.features[0].param_names[0].clone();
    run(&mut s, "parameters.rename", json!({"name": plane_param, "new_name": "chamfer_angle"}));
    run(&mut s, "parameters.add", json!({"name": "chamfer_distance", "expression": "7 mm"}));
    run(&mut s, "parameters.add", json!({"name": "chamfer_second_distance", "expression": "label", "unit": "text"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    let width = sk(&s).constraints.iter().find_map(|c| c.param.clone()).unwrap();
    run(&mut s, "parameters.rename", json!({"name": width, "new_name": "chamfer_distance_2"}));
    for (cmd, corners, sizes, expected) in [
        ("sketch.chamfer.two_distance", ["l1", "l2"], json!({"distance": 3, "distance2": 4}), ["chamfer_distance_3", "chamfer_second_distance_2"]),
        ("sketch.chamfer.distance_angle", ["l2", "l3"], json!({"distance": 3, "angle": 30}), ["chamfer_distance_4", "chamfer_angle_2"]),
        ("sketch.chamfer.distance_angle", ["l3", "l4"], json!({"distance": 3, "angle": 30}), ["chamfer_distance_5", "chamfer_angle_3"]),
    ] {
        let mut p = sizes;
        p["a"] = json!(corners[0]);
        p["b"] = json!(corners[1]);
        let out = run(&mut s, cmd, p);
        let k = sk(&s);
        let names: Vec<_> = out["created"]["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| k.constraints.iter().find(|c| c.id == id.as_str().unwrap()).unwrap().param.as_deref().unwrap())
            .collect();
        assert_eq!(names, expected, "{cmd}");
        assert!(solved(&s));
    }
    assert_eq!(s.doc.param("chamfer_distance").unwrap().expr, "7 mm");
    assert_eq!(s.doc.eval("chamfer_distance_2", solidcraft_doc::expr::Kind::Length).unwrap(), 30.0);
    assert_eq!(s.doc.text_param("chamfer_second_distance").unwrap().text, "label");
    assert_eq!(s.doc.features[0].param_names, ["chamfer_angle"]);
    assert_eq!(s.doc.eval("chamfer_angle", solidcraft_doc::expr::Kind::Length).unwrap(), 10.0);
}

/// The entered angle is measured toward the sharp corner, regardless of line directions.
#[test]
fn chamfer_angle_dimensions_follow_line_directions() {
    use solidcore_record::Vec2;
    use solidcraft_sketch::ConstraintKind;
    for degrees in [-120f64, -90.0, -60.0, 60.0, 90.0, 120.0] {
        for reverse_a in [false, true] {
            for reverse_b in [false, true] {
                for swap in [false, true] {
                    let mut s = new_sketch();
                    let c = Vec2::new(10.0, 10.0);
                    let phi = degrees.to_radians();
                    let (u, v) = (Vec2::X, Vec2::new(phi.cos(), phi.sin()));
                    let corner = json!([c.x, c.y]);
                    let far_a = json!([40, 10]);
                    let far = c + v * 30.0;
                    let far_b = json!([far.x, far.y]);
                    let a = if reverse_a { json!([far_a, corner]) } else { json!([corner, far_a]) };
                    let b = if reverse_b { json!([far_b, corner]) } else { json!([corner, far_b]) };
                    run(&mut s, "sketch.line", json!({"points": a, "ids": ["a"], "infer": false}));
                    run(&mut s, "sketch.line", json!({"points": b, "ids": ["b"], "infer": false}));
                    run(&mut s, "sketch.constraint.fix", json!({"entities": ["a.start", "a.end", "b.start", "b.end"], "fixed": true}));
                    assert_eq!(run(&mut s, "sketch.inspect", json!({}))["dof"], 0);
                    let (a, b, u, v) = if swap { ("b", "a", v, u) } else { ("a", "b", u, v) };
                    let out = run(&mut s, "sketch.chamfer.distance_angle", json!({"a": a, "b": b, "distance": 3, "angle": 30}));
                    assert_eq!(out["sketch"]["dof"], 0, "{degrees} {reverse_a} {reverse_b} {swap}: {out}");
                    let k = sk(&s);
                    let distance = k.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Distance { .. })).unwrap().param.clone().unwrap();
                    let angle = k.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Angle { .. })).unwrap().param.clone().unwrap();
                    for (d, a) in [(3.0, 30f64), (5.0, 40f64)] {
                        let size = s.execute("parameters.change", &json!({"name": distance, "expression": format!("{d} mm")}));
                        assert!(size.is_ok(), "{degrees} {reverse_a} {reverse_b} {swap}: {size:?}");
                        let size = s.execute("parameters.change", &json!({"name": angle, "expression": format!("{a} deg")}));
                        assert!(size.is_ok(), "{degrees} {reverse_a} {reverse_b} {swap}: {size:?}");
                        let k = sk(&s);
                        let (pa, pb) = k.curves[k.curve_index(&curves(&out)[0]).unwrap()].kind.ends().unwrap();
                        let opposite = std::f64::consts::PI - phi.abs() - a.to_radians();
                        let second = d * a.to_radians().sin() / opposite.sin();
                        let expected = [c + u * d, c + v * second];
                        for (p, want) in [pa, pb].into_iter().zip(expected) {
                            assert!(k.point(p).unwrap().dist(want) < 1e-6, "{degrees} {reverse_a} {reverse_b} {swap}: {:?} vs {want:?}", k.point(p));
                        }
                        assert!(solved(&s));
                    }
                }
            }
        }
    }
}

#[test]
fn chamfer_dimensions_keep_the_exact_entered_distances() {
    for units in ["mm", "in"] {
        for distance in [1e-7, 1.23456789] {
            let mut s = new_sketch();
            run(&mut s, "document.units", json!({"units": units}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
            let out = run(&mut s, "sketch.chamfer.two_distance", json!({"a": "l1", "b": "l2", "distance": distance, "distance2": 2.0 * distance}));
            let k = sk(&s);
            for (id, expected) in out["created"]["dimensions"].as_array().unwrap().iter().zip([distance, 2.0 * distance]) {
                let dimension = k.constraints.iter().find(|c| c.id == id.as_str().unwrap()).unwrap();
                let value = s.doc.eval(dimension.param.as_ref().unwrap(), solidcraft_doc::expr::Kind::Length).unwrap();
                assert_eq!(value, expected, "{units}: {out}");
            }
        }
    }
}

/// A chamfer where the two lines have no corner is refused, as a fillet there is: two lines
/// lying on each other made a chamfer line of no length between their cut-back ends (found by a
/// local trim fuzzer), and two lines in line with each other have no corner either.
#[test]
fn a_chamfer_where_two_lines_lie_in_line_is_refused() {
    for far in [[0, 10], [0, 30]] {
        let mut s = new_sketch();
        let a = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 20]], "infer": false})))[0].clone();
        let b = curves(&run(&mut s, "sketch.line", json!({"points": [far, [0, 20]], "infer": false, "connect": false})))[0].clone();
        run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{a}.end"), "b": format!("{b}.end")}));
        let n = sk(&s).curves.len();
        for cmd in ["sketch.chamfer.equal_distance", "sketch.fillet"] {
            let p = if cmd == "sketch.fillet" { json!({"a": a, "b": b, "radius": 2}) } else { json!({"a": a, "b": b, "distance": 2}) };
            let e = s.execute(cmd, &p).unwrap_err().to_string();
            assert!(e.contains("parallel"), "{cmd} at {far:?}: {e}");
            assert_eq!(sk(&s).curves.len(), n, "{cmd} at {far:?}");
        }
    }
}

/// A round that would use up a whole arc is refused, as one too large for two lines is: a
/// quarter arc of radius 5 running into a line along it, rounded with radius 5, was left as an
/// arc of no length (found by a local trim fuzzer).
#[test]
fn a_fillet_that_would_use_up_an_arc_is_refused() {
    let mut s = new_sketch();
    // Centre (15, 5): from (10, 5) down to (15, 0), where the line along y = 0 leaves it.
    let through = [15.0 - 2.5 * 2f64.sqrt(), 5.0 - 2.5 * 2f64.sqrt()];
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [10, 5], "end": [15, 0], "through": through})))[0].clone();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [format!("{a}.end"), [-35, 0]], "infer": false})))[0].clone();
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": a, "b": l, "radius": 5})).unwrap_err().to_string();
    assert!(e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
}

/// A round where a line runs on along an arc's end, off only by rounding, is refused: there is no
/// corner, and the round came out as an arc about 1e-7 mm long (found by a local trim fuzzer,
/// after earlier rounds left the line a hair off the arc's tangent).
#[test]
fn a_fillet_where_a_line_runs_on_smoothly_from_an_arc_is_refused() {
    let mut s = new_sketch();
    let h = 0.5 * 2f64.sqrt();
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [1, 0], "end": [0, 1], "through": [h, h]})))[0].clone();
    // Off the arc's tangent at (0, 1) by 1e-7 rad.
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [format!("{a}.end"), [-10, 1.000001]], "infer": false})))[0].clone();
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": a, "b": l, "radius": 5})).unwrap_err().to_string();
    assert!(e.contains("smoothly") || e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
}

/// An ellipse's construction axes take no part in a corner: a line drawn from the major end
/// meets the major axis there, and Fillet or Chamfer at that point, or on that pair, is refused
/// instead of rounding the line into the axis.
#[test]
fn an_ellipse_s_axis_is_not_a_side_of_a_corner() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [20, 0], "minor_radius": 8}));
    let major = r["created"]["curves"][1].as_str().unwrap().to_string();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [format!("{major}.end"), [20, 15]], "infer": false, "connect": false})))[0].clone();
    let before = sk(&s);
    for (cmd, size) in [("sketch.fillet", "radius"), ("sketch.chamfer.equal_distance", "distance")] {
        for corner in [json!({"point": format!("{l}.start")}), json!({"a": l, "b": major})] {
            let mut p = corner;
            p[size] = json!(2);
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}: {:?}", sk(&s).curves);
            assert_eq!(sk(&s), before, "{cmd} {p}");
        }
    }
}

/// The ids of the constraints driven by the model parameter `name`.
fn driven_by(s: &Session, name: &str) -> Vec<String> {
    sk(s).constraints.iter().filter(|c| c.param.as_deref() == Some(name)).map(|c| c.id.clone()).collect()
}

#[test]
fn fully_defined_rectangles_keep_their_size_when_all_corners_are_filleted() {
    for center in [false, true] {
        for square in [false, true] {
            let mut s = new_sketch();
            let height = if square { 40.0 } else { 20.0 };
            let r = if center {
                run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [20, height / 2.0]}))
            } else {
                run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, height]}))
            };
            let anchor = if center { "p5" } else { "p1" };
            run(&mut s, "sketch.constraint.coincident", json!({"a": anchor, "b": "origin"}));
            let width = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
            if square {
                run(&mut s, "sketch.constraint.equal", json!({"a": "l1", "b": "l2"}));
            } else {
                run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": height}));
            }
            let info = run(&mut s, "sketch.inspect", json!({}));
            assert_eq!(info["dof"], 0, "{info}");
            let points: Vec<Value> = ["bottom_left", "bottom_right", "top_right", "top_left"].iter().map(|name| r["corners"][name].clone()).collect();
            for (i, point) in points.iter().enumerate() {
                let f = run(&mut s, "sketch.fillet", json!({"point": point, "radius": 3}));
                assert_eq!(f["sketch"]["solved"], true, "center={center}, square={square}: {f}");
                assert_eq!(f["sketch"]["dof"], 0, "center={center}, square={square}: {f}");
                let expected = 40.0 * height - (i + 1) as f64 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
                assert!((area(&s) - expected).abs() < 1e-5, "corner={i}, center={center}, square={square}: {} != {expected}", area(&s));
            }
            let expected = 40.0 * height - 4.0 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
            assert!((area(&s) - expected).abs() < 1e-5, "center={center}, square={square}: {} != {expected}", area(&s));
            run(&mut s, "parameters.change", json!({"name": width["param"], "expression": "50 mm"}));
            assert!(solved(&s));
            let expected = 50.0 * if square { 50.0 } else { height } - 4.0 * (9.0 - std::f64::consts::PI * 9.0 / 4.0);
            assert!((area(&s) - expected).abs() < 1e-5, "center={center}, square={square}: {} != {expected}", area(&s));
            let id = s.active_sketch.unwrap();
            let saved = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
            assert_eq!(saved.sketch(id).unwrap(), s.doc.sketch(id).unwrap());
            run(&mut s, "edit.undo", json!({}));
            run(&mut s, "edit.redo", json!({}));
            assert!((area(&s) - expected).abs() < 1e-5);
        }
    }
}

#[test]
fn filleting_a_side_keeps_its_original_midpoint() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-20, 0], "p1": [20, 30]}));
    run(&mut s, "sketch.constraint.midpoint", json!({"point": "origin", "line": "l1"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 30}));
    for point in ["p1", "p2"] {
        let f = run(&mut s, "sketch.fillet", json!({"point": point, "radius": 3}));
        assert_eq!(f["sketch"]["solved"], true, "{f}");
        assert_eq!(f["sketch"]["dof"], 0, "{f}");
        let (lo, hi) = sk(&s).bounds().unwrap();
        assert!((lo.x + 20.0).abs() < 1e-6 && (hi.x - 20.0).abs() < 1e-6, "{lo:?} {hi:?}");
    }
}

/// The spout of a basin: a vertical riser and a horizontal top, the top's length dimensioned
/// to the corner and the corner's height dimensioned from the origin. Filleting the corner used
/// to drop the height and leave the length on the shortened straight part, so the top came out
/// a fillet radius too long. The corner now stays as a virtual sharp corner (a point
/// on both lines' extensions) and the dimensions measure to it.
#[test]
fn a_fillet_keeps_the_dimensions_of_its_corner_on_a_virtual_sharp() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 100], [-80, 100]], "ids": ["riser", "top"]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "riser", "mode": "vertical"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "top", "mode": "horizontal"}));
    let height = run(&mut s, "sketch.dimension", json!({"entities": ["origin", "riser.end"], "type": "vertical", "value": 100}));
    let height = height["param"].as_str().unwrap().to_string();
    let len = run(&mut s, "sketch.dimension", json!({"entities": ["top"], "value": 80}));
    let len = len["param"].as_str().unwrap().to_string();
    let (height_ids, len_ids) = (driven_by(&s, &height), driven_by(&s, &len));
    let f = run(&mut s, "sketch.fillet", json!({"point": "riser.end", "radius": 20}));
    assert!(solved(&s), "{f}");
    // Both dimensions are kept (same ids) and reported as moved to the virtual corner.
    assert_eq!(driven_by(&s, &height), height_ids);
    assert_eq!(driven_by(&s, &len), len_ids);
    let corner = f["virtual_corners"][0].as_str().unwrap_or_default().to_string();
    assert!(!corner.is_empty(), "{f}");
    let moved: Vec<String> = f["moved_to_virtual_corner"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    for id in height_ids.iter().chain(&len_ids) {
        assert!(moved.contains(id), "{id}: {f}");
    }
    // The virtual corner sits where the corner was, and the part keeps its size.
    let k = sk(&s);
    let vc = k.point(k.point_index(&corner).unwrap()).unwrap();
    assert!(vc.dist(solidcore_record::Vec2::new(0.0, 100.0)) < 1e-6, "{vc:?}");
    let top_end = k.point(k.resolve_point("top.end").unwrap()).unwrap();
    assert!(top_end.dist(solidcore_record::Vec2::new(-80.0, 100.0)) < 1e-6, "{top_end:?}");
    let (lo, hi) = k.bounds().unwrap();
    assert!((hi.x - lo.x - 80.0).abs() < 1e-6 && (hi.y - lo.y - 100.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // It is drawn with extension lines from both lines' ends to it.
    let vcs = k.virtual_corners();
    assert_eq!(vcs.len(), 1, "{vcs:?}");
    assert_eq!(vcs[0].1.len(), 2, "{vcs:?}");
    assert!(vcs[0].1.iter().all(|(a, b)| a.dist(vc) < 1e-9 && (b.dist(vc) - 20.0).abs() < 1e-6), "{vcs:?}");
    // The dimensions still drive the part, measured to the virtual corner.
    run(&mut s, "parameters.change", json!({"name": len, "expression": "90 mm"}));
    run(&mut s, "parameters.change", json!({"name": height, "expression": "120 mm"}));
    assert!(solved(&s));
    let (lo, hi) = sk(&s).bounds().unwrap();
    assert!((hi.x - lo.x - 90.0).abs() < 1e-6 && (hi.y - lo.y - 120.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // It is kept in the design file, and undo and redo take it away and bring it back.
    let id = s.active_sketch.unwrap();
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(doc.sketch(id).unwrap(), s.doc.sketch(id).unwrap());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(sk(&s).point_index(&corner).is_none());
    run(&mut s, "edit.redo", json!({}));
    assert!(sk(&s).point_index(&corner).is_some());
    // A fillet without anything measuring to its corner leaves no virtual corner behind.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [10, 100], [-80, 100]], "ids": ["riser", "top"]}));
    let n = sk(&s).points.len();
    let f = run(&mut s, "sketch.fillet", json!({"point": "riser.end", "radius": 20}));
    assert_eq!(f["virtual_corners"], json!([]), "{f}");
    // Corner gone, two tangent points and the arc's centre added.
    assert_eq!(sk(&s).points.len(), n + 2);
}

/// Lines meeting almost straight or almost folded back: a fillet or chamfer with dimensions on
/// the corner either works or refuses with an error, never a panic.
#[test]
fn a_fillet_of_a_nearly_straight_or_folded_corner_with_dimensions_does_not_panic() {
    for (x, y) in [(100.0, 0.05), (100.0, -0.05), (100.0, 1e-9), (-100.0, 0.05), (-100.0, 1e-9)] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0], [100.0 + x, y]], "ids": ["a", "b"]}));
        run(&mut s, "sketch.dimension", json!({"entities": ["a"], "value": 100}));
        for r in [0.01, 1.0, 50.0] {
            let _ = s.execute("sketch.fillet", &json!({"point": "a.end", "radius": r}));
            let _ = s.execute("sketch.chamfer.equal_distance", &json!({"point": "a.end", "distance": r}));
        }
        assert!(sk(&s).points.iter().all(|p| p.pos.is_finite()));
    }
}

/// Two lines that share no end are brought to where they meet before the corner is rounded or
/// cut (#6912, #7759, #7985). A line that stops short is extended, and one that runs past the
/// other is cut there, losing its shorter piece; the far ends stay. Parallel lines, an end held
/// by other geometry and a dimensioned length are refused, and change nothing.
#[test]
fn a_fillet_or_chamfer_brings_two_separate_lines_to_where_they_meet() {
    use solidcraft_sketch::CurveKind;
    let lines = |s: &mut Session, a: [[f64; 2]; 2], b: [[f64; 2]; 2]| {
        let la = curves(&run(s, "sketch.line", json!({"points": a, "connect": false})))[0].clone();
        let lb = curves(&run(s, "sketch.line", json!({"points": b, "connect": false})))[0].clone();
        (la, lb)
    };
    let ends = |s: &Session, id: &str| {
        let k = sk(s);
        let CurveKind::Line { a, b } = k.curves[k.curve_index(id).unwrap()].kind else { panic!("{id} is a line") };
        (k.point(a).unwrap(), k.point(b).unwrap())
    };
    let near = |q: Vec2, x: f64, y: f64| q.dist(Vec2::new(x, y)) < 1e-6;
    // Stopping short of each other (a gap), and crossing with a short piece past the corner.
    for (a, b) in [([[0.0, 0.0], [10.0, 0.0]], [[20.0, 5.0], [20.0, 20.0]]), ([[0.0, 0.0], [24.0, 0.0]], [[20.0, -4.0], [20.0, 20.0]])] {
        let mut s = new_sketch();
        let (la, lb) = lines(&mut s, a, b);
        let r = run(&mut s, "sketch.fillet", json!({"a": la, "b": lb, "radius": 3}));
        assert!(solved(&s), "{r}");
        let k = sk(&s);
        let arc = k.curve_index(&curves(&r)[0]).unwrap();
        let CurveKind::Arc { c, .. } = k.curves[arc].kind else { panic!("an arc") };
        assert!(near(k.point(c).unwrap(), 17.0, 3.0), "{:?}", k.point(c));
        let ((a0, a1), (b0, b1)) = (ends(&s, &la), ends(&s, &lb));
        assert!(near(a0, 0.0, 0.0) && near(a1, 17.0, 0.0), "{la}: {a0:?} {a1:?}");
        assert!(near(b0, 20.0, 3.0) && near(b1, 20.0, 20.0), "{lb}: {b0:?} {b1:?}");
        assert_eq!(k.curves.len(), 3, "the two lines and the arc, nothing left past the corner");
        assert!(k.points.iter().all(|q| !near(q.pos, 24.0, 0.0) && !near(q.pos, 20.0, -4.0) && !near(q.pos, 20.0, 0.0)), "{:?}", k.points);
    }
    // A chamfer the same way.
    let mut s = new_sketch();
    let (la, lb) = lines(&mut s, [[0.0, 0.0], [10.0, 0.0]], [[20.0, 5.0], [20.0, 20.0]]);
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": la, "b": lb, "distance": 4}));
    assert!(solved(&s));
    let ((_, a1), (b0, _)) = (ends(&s, &la), ends(&s, &lb));
    assert!(near(a1, 16.0, 0.0) && near(b0, 20.0, 4.0), "{a1:?} {b0:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sk(&s).curves.len(), 2, "undo brings back the two separate lines");

    // Refused: parallel lines, an end joined to a third line, a dimensioned length.
    let mut s = new_sketch();
    let (pa, pb) = lines(&mut s, [[0.0, 0.0], [10.0, 0.0]], [[0.0, 5.0], [10.0, 5.0]]);
    let (la, lb) = lines(&mut s, [[0.0, 20.0], [10.0, 20.0]], [[20.0, 25.0], [20.0, 40.0]]);
    run(&mut s, "sketch.line", json!({"points": [format!("{la}.end"), [10, 30]], "connect": false}));
    let (da, db) = lines(&mut s, [[0.0, 50.0], [10.0, 50.0]], [[20.0, 55.0], [20.0, 70.0]]);
    run(&mut s, "sketch.dimension", json!({"entities": [da], "value": 10}));
    let before = sk(&s);
    for (a, b, why) in [(&pa, &pb, "parallel"), (&la, &lb, "held by other geometry"), (&da, &db, "length is held")] {
        let e = s.execute("sketch.fillet", &json!({"a": a, "b": b, "radius": 2})).unwrap_err().to_string();
        assert!(e.contains(why), "{a}, {b}: {e}");
    }
    assert_eq!(sk(&s), before, "refusals change nothing");
}

// ---------------------------------------------------------------------------------------------
// Corners with a conic or a control point spline as a side (#9178)

/// Distance from `q` to the nearest of the exact pieces `segs`.
fn dist_to(segs: &[solidcore_record::Seg2], q: Vec2) -> f64 {
    segs.iter().map(|s| s.point_at(s.closest_param(q)).dist(q)).fold(f64::INFINITY, f64::min)
}

/// The unit tangent of curve `ci` where it starts or ends at point `p`, in its direction of
/// travel.
fn end_tangent(k: &solidcraft_sketch::Sketch, ci: usize, p: usize) -> Vec2 {
    let segs = k.exact_pieces(ci);
    let (a, _) = k.curves[ci].kind.ends().unwrap();
    let d = if a == p { segs.first().unwrap().derivative(0.0) } else { segs.last().unwrap().derivative(1.0) };
    d.normalized().unwrap()
}

/// The sketch's degrees of freedom.
fn dof(s: &Session) -> usize {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.dof
}

/// The checks every round of such a corner passes: the arc has radius `r`, each side keeps its id
/// and ends where the arc does, tangent to it, and the whole kept side lies on the side's original
/// curve (`before`), so the side was cut back and not reshaped. The sketch solves.
fn check_round(s: &Session, before: &solidcraft_sketch::Sketch, arc_id: &str, sides: [&str; 2], r: f64) {
    assert!(solved(s), "the sketch solves after the round");
    let k = sk(s);
    let arc = k.curve_index(arc_id).unwrap();
    let solidcraft_sketch::CurveKind::Arc { c, a, b } = k.curves[arc].kind else { panic!("{arc_id} is an arc") };
    let centre = k.point(c).unwrap();
    for end in [a, b] {
        let q = k.point(end).unwrap();
        assert!((q.dist(centre) - r).abs() < 1e-9, "radius at {q:?}: {}", q.dist(centre));
    }
    for id in sides {
        let ci = k.curve_index(id).unwrap_or_else(|| panic!("`{id}` keeps its id"));
        let (s0, s1) = k.curves[ci].kind.ends().unwrap();
        let shared = [s0, s1].into_iter().find(|p| *p == a || *p == b).unwrap_or_else(|| panic!("`{id}` ends where the arc does"));
        let (u, v) = (end_tangent(&k, ci, shared), end_tangent(&k, arc, shared));
        assert!(u.cross(v).abs() < 1e-7, "`{id}` is tangent to the arc: {u:?} vs {v:?}");
        let orig = before.exact_pieces(before.curve_index(id).unwrap());
        for piece in k.exact_pieces(ci) {
            for i in 0..=16 {
                let q = piece.point_at(i as f64 / 16.0);
                let d = dist_to(&orig, q);
                assert!(d < 1e-9, "{q:?} on `{id}` is off its original curve by {d}");
            }
        }
    }
}

/// A line up to (0, 0) and a quarter circle of radius 10 about (0, 10) from there to (10, 10):
/// drawn as an arc, and as the conic that is exactly that arc (its apex where the end tangents
/// meet, weight cos(45°), `rho` = w / (1 + w)). The round of radius 2 lies outside the circle,
/// 12 from (0, 10) and 2 from the line: centre (2, 10 − √140), the tangent point on the line
/// straight left of it, and the one on the circle 10 from (0, 10) towards the centre. Both
/// rounds give that arc. The conic's kept part is still that circle, so its `rho` is
/// w / (1 + w) again, with w the cosine of half its sweep.
#[test]
fn a_conic_that_is_a_circular_arc_fillets_like_the_arc() {
    let w = std::f64::consts::FRAC_PI_4.cos();
    let rho = w / (1.0 + w);
    let want_c = Vec2::new(2.0, 10.0 - 140f64.sqrt());
    let want_line = Vec2::new(0.0, want_c.y);
    let want_round = Vec2::new(0.0, 10.0) + (want_c - Vec2::new(0.0, 10.0)) * (10.0 / 12.0);
    let mut got = Vec::new();
    for conic in [false, true] {
        let mut s = new_sketch();
        let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, -20], [0, 0]], "connect": false})))[0].clone();
        let c = if conic {
            curves(&run(&mut s, "sketch.conic", json!({"start": format!("{l}.end"), "apex": [10, 0], "end": [10, 10], "rho": rho})))[0].clone()
        } else {
            let h = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
            curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{l}.end"), "end": [10, 10], "through": [h, 10.0 - h]})))[0].clone()
        };
        let before = sk(&s);
        let f = run(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 2}));
        let arc = curves(&f)[0].clone();
        check_round(&s, &before, &arc, [&l, &c], 2.0);
        let k = sk(&s);
        let solidcraft_sketch::CurveKind::Arc { c: ci, a, b } = k.curves[k.curve_index(&arc).unwrap()].kind else { panic!("an arc") };
        let (centre, pa, pb) = (k.point(ci).unwrap(), k.point(a).unwrap(), k.point(b).unwrap());
        let (on_line, on_round) = if pa.x.abs() < pb.x.abs() { (pa, pb) } else { (pb, pa) };
        assert!(centre.dist(want_c) < 1e-9, "conic {conic}: centre {centre:?}");
        assert!(on_line.dist(want_line) < 1e-9, "conic {conic}: {on_line:?}");
        assert!(on_round.dist(want_round) < 1e-9, "conic {conic}: {on_round:?}");
        got.push((centre, on_line, on_round));
        if conic {
            let solidcraft_sketch::CurveKind::Conic { rho: kept, .. } = k.curves[k.curve_index(&c).unwrap()].kind else {
                panic!("`{c}` stays a conic")
            };
            let (to_end, to_cut) = (Vec2::new(10.0, 0.0), on_round - Vec2::new(0.0, 10.0));
            let half = 0.5 * to_end.cross(to_cut).atan2(to_end.dot(to_cut)).abs();
            let w = half.cos();
            assert!((kept - w / (1.0 + w)).abs() < 1e-12, "rho {kept} for a sweep of {}°", 2.0 * half.to_degrees());
        }
    }
    let (arc, conic) = (got[0], got[1]);
    assert!(arc.0.dist(conic.0) < 1e-9 && arc.1.dist(conic.1) < 1e-9 && arc.2.dist(conic.2) < 1e-9, "{arc:?} vs {conic:?}");
}

/// A drawing whose tip is two quadratic Béziers meeting at a right angle, imported as two conics.
const CURVED_TIP_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 30" width="20mm" height="30mm"><path d="M0,0 L20,0 L20,10 Q20,20 10,30 Q0,20 0,10 Z"/></svg>"#;

/// The tip of an imported drawing, where two conics meet, is rounded: an arc tangent to both,
/// each conic cut back exactly, keeping its id, one closed profile, and the sketch's degrees of
/// freedom as they were (the corner point gives way to the arc's two ends and centre, which the
/// tangencies and the radius hold).
#[test]
fn a_fillet_rounds_a_corner_of_two_conics() {
    let mut s = new_sketch();
    run(&mut s, "sketch.insert_svg", json!({"text": CURVED_TIP_SVG}));
    let before = sk(&s);
    let dof_before = dof(&s);
    assert!(matches!(before.curves[before.curve_index("n1").unwrap()].kind, solidcraft_sketch::CurveKind::Conic { .. }));
    let n = before.curves.len();
    let f = run(&mut s, "sketch.fillet", json!({"point": "n1.end", "radius": 1}));
    let arc = curves(&f)[0].clone();
    check_round(&s, &before, &arc, ["n1", "n2"], 1.0);
    let k = sk(&s);
    assert_eq!(k.curves.len(), n + 1, "the two conics and the arc, nothing else");
    assert_eq!(s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.len(), 1, "one closed profile");
    assert_eq!(f["virtual_corners"], json!([]), "{f}");
    assert_eq!(dof(&s), dof_before, "the fillet leaves the degrees of freedom as they were");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sk(&s), before, "undo brings the sharp tip back");
}

/// A line meeting a conic at a real corner (the conic's apex off the line), a line meeting a
/// control point spline at its start, and a control point spline ending where a line starts: each
/// corner rounds and the side is cut back exactly. The conic leaves the degrees of freedom as they
/// were. The spline keeps its id, its degree and its far end. The control points away from the cut
/// keep their ids and places, and the degrees of freedom change by two for each control point the
/// spline gains or loses.
#[test]
fn a_fillet_rounds_a_corner_of_a_line_with_a_conic_or_a_control_spline() {
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let c = curves(&run(&mut s, "sketch.conic", json!({"start": format!("{l}.end"), "apex": [20, 5], "end": [20, 10], "rho": 0.5})))[0].clone();
    let (before, dof_before) = (sk(&s), dof(&s));
    let f = run(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}));
    check_round(&s, &before, &curves(&f)[0], [&l, &c], 1.0);
    let k = sk(&s);
    let end = k.point(k.resolve_point(&format!("{l}.end")).unwrap()).unwrap();
    assert!(end.x < 10.0 - 1e-6 && end.y.abs() < 1e-12, "the line is cut back along itself: {end:?}");
    assert_eq!(dof(&s), dof_before, "a conic side leaves the degrees of freedom as they were");

    for at_start in [true, false] {
        let mut s = new_sketch();
        let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
        let (corner, points) = if at_start {
            (format!("{l}.end"), json!([format!("{l}.end"), [15, 5], [20, 4], [25, 10], [30, 6]]))
        } else {
            (format!("{l}.start"), json!([[-30, 6], [-25, 10], [-20, 4], [-15, 5], format!("{l}.start")]))
        };
        let sp = curves(&run(&mut s, "sketch.spline.control_point", json!({"points": points})))[0].clone();
        let (before, dof_before) = (sk(&s), dof(&s));
        let bi = before.curve_index(&sp).unwrap();
        let solidcraft_sketch::CurveKind::Spline { pts: old, .. } = before.curves[bi].kind.clone() else { panic!("a spline") };
        // The two control points at the far end, by id.
        let far_ids: Vec<String> = if at_start { old[3..].iter() } else { old[..2].iter() }.map(|p| before.points[*p].id.clone()).collect();
        let f = run(&mut s, "sketch.fillet", json!({"point": corner, "radius": 1}));
        check_round(&s, &before, &curves(&f)[0], [&l, &sp], 1.0);
        let k = sk(&s);
        let si = k.curve_index(&sp).unwrap();
        let solidcraft_sketch::CurveKind::Spline { pts, control, periodic, degree, .. } = &k.curves[si].kind else { panic!("`{sp}` stays a spline") };
        assert!(*control && !*periodic && *degree == 3, "{:?}", k.curves[si].kind);
        for id in &far_ids {
            let p = k.point_index(id).unwrap_or_else(|| panic!("`{id}` stays"));
            assert!(pts.contains(&p), "`{id}` is still a control point of `{sp}`");
            assert_eq!(k.points[p].pos, before.points[before.point_index(id).unwrap()].pos, "`{id}` stays where it was");
        }
        let gained = pts.len() as isize - old.len() as isize;
        assert_eq!(dof(&s) as isize - dof_before as isize, 2 * gained, "at_start {at_start}: {} control points from {}", pts.len(), old.len());
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(sk(&s), before);
    }
}

/// A drawing whose side runs into a cubic Bézier at a right angle: the import makes the cubic a
/// control point spline, and the corner rounds.
#[test]
fn a_fillet_rounds_the_corner_of_an_imported_cubic() {
    let svg =
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" width="20mm" height="20mm"><path d="M0,0 L20,0 C20,10 10,20 0,20 Z"/></svg>"#;
    let mut s = new_sketch();
    run(&mut s, "sketch.insert_svg", json!({"text": svg}));
    let before = sk(&s);
    let cubic = before.curves.iter().find(|c| matches!(c.kind, solidcraft_sketch::CurveKind::Spline { .. })).expect("a spline").clone();
    assert!(matches!(cubic.kind, solidcraft_sketch::CurveKind::Spline { control: true, .. }), "{:?}", cubic.kind);
    let (a, _) = cubic.kind.ends().unwrap();
    let line = before.curves.iter().find(|c| matches!(c.kind, solidcraft_sketch::CurveKind::Line { .. }) && c.kind.uses(a)).unwrap().id.clone();
    let f = run(&mut s, "sketch.fillet", json!({"point": before.points[a].id, "radius": 1}));
    check_round(&s, &before, &curves(&f)[0], [&line, &cubic.id], 1.0);
}

/// An arc meeting a conic, given as `a` and `b`, and a conic between two lines whose two corners
/// are rounded together (`points`), each with its own arc tangent to the conic.
#[test]
fn a_fillet_rounds_an_arc_and_conic_corner_and_both_corners_of_one_conic() {
    let mut s = new_sketch();
    let arc = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [-10, 5], "end": [0, 5], "through": [-5, 1]})))[0].clone();
    let c = curves(&run(&mut s, "sketch.conic", json!({"start": format!("{arc}.end"), "apex": [10, 10], "end": [10, 15], "rho": 0.4})))[0].clone();
    let before = sk(&s);
    let f = run(&mut s, "sketch.fillet", json!({"a": arc, "b": c, "radius": 1}));
    check_round(&s, &before, &curves(&f)[0], [&arc, &c], 1.0);

    let mut s = new_sketch();
    let a = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let c = curves(&run(&mut s, "sketch.conic", json!({"start": format!("{a}.end"), "apex": [20, 5], "end": [20, 10], "rho": 0.5})))[0].clone();
    let b = curves(&run(&mut s, "sketch.line", json!({"points": [format!("{c}.end"), [30, 15]], "connect": false})))[0].clone();
    let before = sk(&s);
    let f = run(&mut s, "sketch.fillet", json!({"points": [format!("{a}.end"), format!("{c}.end")], "radius": 1}));
    let arcs = curves(&f);
    assert_eq!(arcs.len(), 2, "{f}");
    check_round(&s, &before, &arcs[0], [&a, &c], 1.0);
    check_round(&s, &before, &arcs[1], [&c, &b], 1.0);
}

/// Where a line runs on along a conic's end tangent there is no corner: the drawing's side meets
/// its first conic that way, as does a line towards a conic's apex. Both are refused as smooth.
#[test]
fn a_fillet_where_a_line_runs_on_smoothly_into_a_conic_is_refused() {
    let mut s = new_sketch();
    run(&mut s, "sketch.insert_svg", json!({"text": CURVED_TIP_SVG}));
    let before = sk(&s);
    let e = s.execute("sketch.fillet", &json!({"point": "l2.end", "radius": 1})).unwrap_err().to_string();
    assert!(e.contains("smoothly"), "{e}");
    assert_eq!(sk(&s), before);
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    run(&mut s, "sketch.conic", json!({"start": format!("{l}.end"), "apex": [20, 0], "end": [20, 10], "rho": 0.5}));
    let before = sk(&s);
    let e = s.execute("sketch.fillet", &json!({"point": format!("{l}.end"), "radius": 1})).unwrap_err().to_string();
    assert!(e.contains("smoothly"), "{e}");
    assert_eq!(sk(&s), before);
}

/// What a round with a conic or spline side refuses, each with a message naming the cause, and
/// changing nothing: a radius too large, a chamfer there, a side of a kind fillets do not take
/// (an ellipse, a closed spline through the corner, a fit point spline, also one with tangent
/// handles), a corner that would have to stay as a virtual sharp corner (a dimension measures to
/// it, or a construction line or curve ends there too, which is never taken as a side), a point
/// held on the cut conic, a conic whose apex point is held, a spline control point that the cut
/// moves and that another curve holds, and a spline that starts and ends at the corner.
#[test]
fn rounds_with_a_conic_or_spline_side_refuse_with_the_cause_named() {
    let line_conic = || {
        let mut s = new_sketch();
        let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
        let c = curves(&run(&mut s, "sketch.conic", json!({"start": format!("{l}.end"), "apex": [20, 5], "end": [20, 10], "rho": 0.5})))[0].clone();
        (s, l, c)
    };
    let refused = |s: &mut Session, cmd: &str, p: Value, why: &str| {
        let before = sk(s);
        let e = match s.execute(cmd, &p) {
            Ok(v) => panic!("{cmd} {p} is not refused: {v}"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains(why), "{cmd} {p}: {e}");
        assert_eq!(sk(s), before, "{cmd} {p} changes nothing");
    };
    let (mut s, l, c) = line_conic();
    refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 50}), "does not fit");
    refused(
        &mut s,
        "sketch.chamfer.equal_distance",
        json!({"point": format!("{l}.end"), "distance": 1}),
        &format!("chamfers work on lines: `{c}` is a conic"),
    );
    refused(&mut s, "sketch.chamfer.equal_distance", json!({"a": l, "b": c, "distance": 1}), &format!("chamfers work on lines: `{c}` is a conic"));
    let e = curves(&run(&mut s, "sketch.ellipse", json!({"center": [50, 50], "major": [70, 50], "minor_radius": 8})))[0].clone();
    refused(
        &mut s,
        "sketch.fillet",
        json!({"a": l, "b": e, "radius": 1}),
        &format!("fillets work on lines, arcs, conics and control point splines: `{e}` is an ellipse"),
    );
    let arc = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [40, 0], "end": [50, 10], "through": [47, 3]})))[0].clone();
    refused(&mut s, "sketch.fillet", json!({"a": arc, "b": e, "radius": 1}), &format!("`{e}` is an ellipse"));
    refused(&mut s, "sketch.fillet", json!({"a": arc, "b": c, "radius": 1}), "the curves must share a corner point");
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let closed =
        curves(&run(&mut s, "sketch.spline.fit_point", json!({"points": [format!("{l}.end"), [5, -5], [0, -8]], "periodic": true})))[0].clone();
    refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}), &format!("`{closed}` is a closed spline"));
    // A fit point spline, also one with tangent handles (their lines end at the corner too).
    for handles in [false, true] {
        let mut s = new_sketch();
        let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
        let sp = curves(&run(&mut s, "sketch.spline.fit_point", json!({"points": [format!("{l}.end"), [15, 5], [20, 4], [25, 10]]})))[0].clone();
        if handles {
            run(&mut s, "sketch.spline.handles", json!({"curve": sp}));
        }
        refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}), &format!("`{sp}`, a fit point spline"));
    }
    // A dimension to the corner.
    let (mut s, l, _) = line_conic();
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", format!("{l}.end")], "type": "horizontal", "value": 10}));
    refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}), "virtual sharp corner");
    // A construction line or curve ending at the corner too: the line and the conic are the
    // sides, and the corner has to stay for the construction curve.
    for guide in ["sketch.line", "sketch.conic"] {
        let (mut s, l, _) = line_conic();
        let p = if guide == "sketch.line" {
            json!({"points": [format!("{l}.end"), [10, -5]], "connect": false})
        } else {
            json!({"start": format!("{l}.end"), "apex": [10, -5], "end": [5, -8], "rho": 0.5})
        };
        let g = curves(&run(&mut s, guide, p))[0].clone();
        run(&mut s, "sketch.construction", json!({"curves": [g], "value": true}));
        refused(
            &mut s,
            "sketch.fillet",
            json!({"point": format!("{l}.end"), "radius": 1}),
            &format!("virtual sharp corner, and this one would have to: `{g}` uses it too"),
        );
    }
    // A point held on the conic, which the cut shortens.
    let (mut s, l, c) = line_conic();
    let pin = curves(&run(&mut s, "sketch.line", json!({"points": [[19, 4], [25, 0]], "connect": false})))[0].clone();
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{pin}.start"), "b": c}));
    let k = {
        let k = sk(&s);
        let ci = k.curve_index(&c).unwrap();
        k.constraints
            .iter()
            .find(|x| matches!(x.kind, solidcraft_sketch::ConstraintKind::PointOnCurve { c, .. } if c == ci))
            .expect("a point on the conic")
            .id
            .clone()
    };
    refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}), &format!("`{k}` holds something on `{c}`"));
    // The conic's apex point held by another curve.
    let (mut s, l, c) = line_conic();
    run(&mut s, "sketch.line", json!({"points": [format!("{c}.apex"), [30, 30]], "connect": false}));
    refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}), "apex");
    // A control point that the cut moves, held by another curve.
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let h = curves(&run(&mut s, "sketch.line", json!({"points": [[15, 5], [15, 20]], "connect": false})))[0].clone();
    let sp = curves(&run(
        &mut s,
        "sketch.spline.control_point",
        json!({"points": [format!("{l}.end"), format!("{h}.start"), [20, 4], [25, 10], [30, 6]]}),
    ))[0]
        .clone();
    refused(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 1}), &format!("`{sp}`'s control point"));
    // A spline that starts and ends at the corner.
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let sp = curves(&run(&mut s, "sketch.spline.control_point", json!({"points": [format!("{l}.end"), [15, 5], [20, -5], format!("{l}.end")]})))[0]
        .clone();
    refused(&mut s, "sketch.fillet", json!({"a": l, "b": sp, "radius": 1}), &format!("`{sp}` starts and ends at the corner"));
}

#[test]
fn a_freeform_corner_keeps_what_its_line_side_and_its_partner_point_hold() {
    let line_conic = |s: &mut Session| {
        let l = curves(&run(s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
        let c = curves(&run(s, "sketch.conic", json!({"start": format!("{l}.end"), "apex": [20, 5], "end": [20, 10], "rho": 0.5})))[0].clone();
        (l, c)
    };
    // An equal length on the line side would measure the shortened line.
    let mut s = new_sketch();
    let (l, _) = line_conic(&mut s);
    let other = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 20]], "connect": false})))[0].clone();
    run(&mut s, "sketch.constraint.equal", json!({"a": l, "b": other}));
    let before = sk(&s);
    let e = s.execute("sketch.fillet", &json!({"point": format!("{l}.end"), "radius": 1})).unwrap_err().to_string();
    assert!(e.contains(&format!("measures the length or midpoint of `{l}`")), "{e}");
    assert_eq!(sk(&s), before);
    // A rational spline whose weights differ: its conic pieces run on their own parameter.
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let sp = curves(&run(&mut s, "sketch.spline.control_point", json!({"points": [format!("{l}.end"), [15, 5], [20, 10]]})))[0].clone();
    let mut k = sk(&s);
    let ci = k.curve_index(&sp).unwrap();
    if let Some(solidcraft_sketch::CurveKind::Spline { degree, nurbs, .. }) = k.curves.get_mut(ci).map(|c| &mut c.kind) {
        *degree = 2;
        *nurbs = Some(solidcraft_sketch::Nurbs { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], weights: vec![1.0, 2.0, 3.0] });
    }
    let e = crate::cmd::sketch_corner_freeform::round_freeform_corner(&mut k, &json!({"point": format!("{l}.end")}), 1.0, "sketch.fillet")
        .unwrap_err()
        .to_string();
    assert!(e.contains(&format!("`{sp}`, a rational spline whose weights differ")), "{e}");
    // A corner drawn as two points held coincident is welded only where the partner has no place
    // of its own: a fixed partner keeps its point.
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false})))[0].clone();
    let c = curves(&run(&mut s, "sketch.conic", json!({"start": [10, 0], "apex": [20, 5], "end": [20, 10], "rho": 0.5})))[0].clone();
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{l}.end"), "b": format!("{c}.start")}));
    let mut k = sk(&s);
    let partner = k.resolve_point(&format!("{c}.start")).unwrap();
    let id = k.points[partner].id.clone();
    k.points[partner].fixed = true;
    super::weld_corner(&mut k, &json!({"a": l, "b": c}));
    let kept = k.point_index(&id).expect("the fixed partner stays");
    assert!(k.points[kept].fixed);
    assert_ne!(k.resolve_point(&format!("{l}.end")), k.resolve_point(&format!("{c}.start")));
}

/// A 3D sketch with drawing off the plane turned on.
fn new_sketch_3d() -> Session {
    let mut s = new_sketch();
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    s
}

/// The fit points of drawn 3D curve `id`.
fn fit(s: &Session, id: &str) -> Vec<solidcore_record::Vec3> {
    sk(s).wires.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no 3D curve {id}")).fit.clone()
}

/// The exact arc of drawn 3D curve `id`.
fn arc3(s: &Session, id: &str) -> solidcraft_sketch::WireArc {
    sk(s).wires.iter().find(|w| w.id == id).and_then(|w| w.arc).unwrap_or_else(|| panic!("{id} is not a 3D arc"))
}

fn wires_solved(s: &Session) -> bool {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.wires_ok()
}

fn near3(a: solidcore_record::Vec3, b: [f64; 3]) -> bool {
    a.dist(solidcore_record::Vec3::new(b[0], b[1], b[2])) < 1e-6
}

/// #21319: Fillet on two drawn 3D lines that share an end point, for a tube bend. A line from
/// (0,0,0) to (100,0,0) and one on to (100,0,80), rounded with radius 20: each line loses
/// 20 / tan(45°) = 20 mm, and the arc is the quarter circle about (80,0,20). The path is then
/// 80 + 10π + 60 = 171.416 mm long, and a pipe of 8 mm diameter with a 1 mm wall along it has
/// π(4² − 3²) × 171.416 = 3769.63 mm³. One undo takes the fillet back.
#[test]
fn a_fillet_rounds_the_corner_of_two_3d_lines_with_a_tangent_arc() {
    use std::f64::consts::PI;
    let mut s = new_sketch_3d();
    let c = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [100, 0, 0], [100, 0, 80]]})));
    let before = s.doc.to_json();
    let r = run(&mut s, "sketch.fillet", json!({"a": c[0], "b": c[1], "radius": 20}));
    let arc = curves(&r)[0].clone();
    assert_eq!(curves(&r).len(), 1);
    assert!(wires_solved(&s), "{r}");
    assert!(near3(fit(&s, &c[0])[0], [0.0, 0.0, 0.0]) && near3(fit(&s, &c[0])[1], [80.0, 0.0, 0.0]), "{:?}", fit(&s, &c[0]));
    assert!(near3(fit(&s, &c[1])[0], [100.0, 0.0, 20.0]) && near3(fit(&s, &c[1])[1], [100.0, 0.0, 80.0]), "{:?}", fit(&s, &c[1]));
    let a = arc3(&s, &arc);
    assert!(near3(a.center, [80.0, 0.0, 20.0]) && (a.radius - 20.0).abs() < 1e-9, "{a:?}");
    assert!((solidcraft_sketch::arc_sweep(&a) - PI / 2.0).abs() < 1e-9, "{}", solidcraft_sketch::arc_sweep(&a));
    assert!(near3(a.start, [80.0, 0.0, 0.0]) && near3(a.end, [100.0, 0.0, 20.0]), "{a:?}");
    // Tangent to each line, and a radius dimension with a parameter of its own.
    let k = sk(&s);
    let tangents =
        k.wire_constraints.iter().filter(|x| matches!(&x.kind, solidcraft_sketch::WireConstraintKind::Tangent { arc: on, .. } if *on == arc));
    assert_eq!(tangents.count(), 2, "{:?}", k.wire_constraints);
    let param = r["param"].as_str().expect("a radius parameter").to_string();
    let radius = k.wire_constraints.iter().find(|x| matches!(&x.kind, solidcraft_sketch::WireConstraintKind::Radius { arc: on, .. } if *on == arc));
    assert_eq!(radius.and_then(|x| x.param.clone()), Some(param), "{:?}", k.wire_constraints);
    // The lines no longer meet at the corner: their joint is gone.
    assert!(
        !k.wire_constraints.iter().any(|x| matches!(x.kind, solidcraft_sketch::WireConstraintKind::Coincident { .. })),
        "{:?}",
        k.wire_constraints
    );
    // One undo takes the whole fillet back, and one redo brings it again.
    let after = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), after);
    // The pipe along the bent path.
    run(&mut s, "sketch.finish", json!({}));
    let sketch = s.doc.features.last().unwrap().id;
    run(&mut s, "solid.pipe", json!({"path_sketch": sketch, "path": [c[0], arc, c[1]], "diameter": 8, "wall": 1}));
    let world = s.world_state().unwrap();
    let volume = solidcraft_kernel::measure(&world.bodies.last().unwrap().body).unwrap().volume;
    let expected = 7.0 * PI * (80.0 + 10.0 * PI + 60.0);
    assert!((volume - expected).abs() < 0.5, "{volume} against {expected}");
}

/// #21319: the corner need not lie on the sketch plane, nor be square. Lines from (0,0,0) to
/// (60,0,0) and on to (90,40,30) meet at θ with cos θ = −30 / √3400. Radius 10 takes
/// d = 10 / tan(θ/2) off each line; the arc lies in the plane of the two lines, its centre
/// 10 / sin(θ/2) from the corner along the bisector, and its ends square to the lines.
#[test]
fn a_fillet_on_3d_lines_lies_in_the_plane_of_the_two_lines() {
    use solidcore_record::Vec3;
    let mut s = new_sketch_3d();
    let c = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [60, 0, 0], [90, 40, 30]]})));
    let r = run(&mut s, "sketch.fillet", json!({"a": c[1], "b": c[0], "radius": 10}));
    assert!(wires_solved(&s), "{r}");
    let corner = Vec3::new(60.0, 0.0, 0.0);
    let (u, v) = (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(30.0, 40.0, 30.0) * (1.0 / 3400f64.sqrt()));
    let half = 0.5 * (-30.0 / 3400f64.sqrt()).acos();
    let d = 10.0 / half.tan();
    let (ta, tb) = (corner + u * d, corner + v * d);
    assert!(fit(&s, &c[0])[1].dist(ta) < 1e-6 && fit(&s, &c[1])[0].dist(tb) < 1e-6, "{:?} {:?}", fit(&s, &c[0]), fit(&s, &c[1]));
    let a = arc3(&s, &curves(&r)[0]);
    let bisector = (u + v).normalized().unwrap();
    assert!(a.center.dist(corner + bisector * (10.0 / half.sin())) < 1e-6, "{a:?}");
    assert!((a.radius - 10.0).abs() < 1e-9, "{a:?}");
    assert!(a.normal.cross(u.cross(v).normalized().unwrap()).len() < 1e-9, "the arc's plane is the lines' plane: {a:?}");
    assert!((ta - a.center).dot(u).abs() < 1e-6 && (tb - a.center).dot(v).abs() < 1e-6, "{a:?}");
    // The arc turns by the angle the path turns: π − θ.
    assert!((solidcraft_sketch::arc_sweep(&a) - (std::f64::consts::PI - 2.0 * half)).abs() < 1e-9, "{}", solidcraft_sketch::arc_sweep(&a));
}

/// #21319: the radius stays editable. A path placed by fixed points keeps its sharp corner as a
/// 3D point on both lines, which takes the corner's Fix; changing the radius parameter to 30 then
/// moves the tangent points to 30 mm from the corner, and the far ends stay.
#[test]
fn a_3d_fillet_radius_can_be_edited_and_a_fixed_corner_stays_as_a_point() {
    let mut s = new_sketch_3d();
    let c = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [100, 0, 0], [100, 0, 80]]})));
    run(
        &mut s,
        "sketch.constraint.fix",
        json!({"entities": [format!("{}.start", c[0]), format!("{}.end", c[0]), format!("{}.end", c[1])], "fixed": true}),
    );
    let r = run(&mut s, "sketch.fillet", json!({"a": c[0], "b": c[1], "radius": 20}));
    assert!(wires_solved(&s), "{r}");
    let corner = r["virtual_corners"][0].as_str().expect("the corner is kept").to_string();
    assert!(near3(fit(&s, &corner)[0], [100.0, 0.0, 0.0]));
    assert_eq!(r["moved_to_virtual_corner"].as_array().map(Vec::len), Some(1), "{r}");
    let k = sk(&s);
    let fixed = |wire: &str, index: usize| {
        k.wire_constraints
            .iter()
            .any(|x| matches!(&x.kind, solidcraft_sketch::WireConstraintKind::Fix { p, .. } if p.wire == wire && p.index == index))
    };
    assert!(fixed(&corner, 0) && !fixed(&c[0], 1), "the Fix moved to the corner point: {:?}", k.wire_constraints);
    run(&mut s, "parameters.change", json!({"name": r["param"], "expression": "30 mm"}));
    assert!(wires_solved(&s));
    assert!(near3(fit(&s, &c[0])[0], [0.0, 0.0, 0.0]) && near3(fit(&s, &c[0])[1], [70.0, 0.0, 0.0]), "{:?}", fit(&s, &c[0]));
    assert!(near3(fit(&s, &c[1])[0], [100.0, 0.0, 30.0]) && near3(fit(&s, &c[1])[1], [100.0, 0.0, 80.0]), "{:?}", fit(&s, &c[1]));
    let a = arc3(&s, &curves(&r)[0]);
    assert!(near3(a.center, [70.0, 0.0, 30.0]) && (a.radius - 30.0).abs() < 1e-6, "{a:?}");
}

/// #21319: several 3D corners in one call share one radius, as planar corners do: the first arc
/// carries the dimension and the others are equal to it.
#[test]
fn a_fillet_rounds_several_3d_corners_with_one_radius() {
    let mut s = new_sketch_3d();
    let c = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [100, 0, 0], [100, 0, 80], [100, 60, 80]]})));
    let r = run(&mut s, "sketch.fillet", json!({"points": [{"a": c[0], "b": c[1]}, {"a": c[1], "b": c[2]}], "radius": 15}));
    assert!(wires_solved(&s), "{r}");
    let arcs = curves(&r);
    assert_eq!(arcs.len(), 2);
    assert!(near3(arc3(&s, &arcs[0]).center, [85.0, 0.0, 15.0]) && near3(arc3(&s, &arcs[1]).center, [100.0, 15.0, 65.0]));
    assert!(near3(fit(&s, &c[1])[0], [100.0, 0.0, 15.0]) && near3(fit(&s, &c[1])[1], [100.0, 0.0, 65.0]), "{:?}", fit(&s, &c[1]));
    let k = sk(&s);
    let radii = k.wire_constraints.iter().filter(|x| matches!(x.kind, solidcraft_sketch::WireConstraintKind::Radius { .. })).count();
    let equal = k
        .wire_constraints
        .iter()
        .any(|x| matches!(&x.kind, solidcraft_sketch::WireConstraintKind::Equal { a, b, arcs: true } if *a == arcs[0] && *b == arcs[1]));
    assert!(radii == 1 && equal, "{:?}", k.wire_constraints);
}

/// #21319: what Fillet does not round in 3D is refused with its reason, and the design stays as
/// it was: lines that share no end point, lines in one straight line, a radius that needs more
/// of a line than there is, a 3D arc or spline, and 3D corners mixed with planar ones. Chamfer
/// still takes no 3D lines.
#[test]
fn a_fillet_on_3d_lines_refuses_what_has_no_corner_to_round() {
    let mut s = new_sketch_3d();
    let c = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [100, 0, 0], [100, 0, 80]]})));
    let apart = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 50, 0], [0, 50, 40]]})))[0].clone();
    let straight = curves(&run(&mut s, "sketch.line3d", json!({"points": [[0, 90, 0], [10, 90, 5], [20, 90, 10]]})));
    let arc = curves(&run(&mut s, "sketch.arc3d", json!({"start": [100, 0, 80], "through": [110, 0, 90], "end": [100, 0, 100]})))[0].clone();
    let spline = curves(&run(&mut s, "sketch.spline3d", json!({"points": [[0, 0, 0], [-10, 5, 10], [-20, 0, 20]]})))[0].clone();
    let flat = curves(&run(&mut s, "sketch.line", json!({"points": [[0, -50], [20, -50], [20, -30]]})));
    let before = s.doc.to_json();
    let mut refused = |cmd: &str, p: Value, why: &str| {
        let e = s.execute(cmd, &p).unwrap_err().to_string();
        assert!(e.contains(why), "{cmd} {p}: {e}");
        assert_eq!(s.doc.to_json(), before, "{cmd} {p}");
    };
    refused("sketch.fillet", json!({"a": c[0], "b": apart, "radius": 5}), "share an end point");
    refused("sketch.fillet", json!({"a": c[0], "b": c[0], "radius": 5}), "two different 3D lines");
    refused("sketch.fillet", json!({"a": straight[0], "b": straight[1], "radius": 5}), "one straight line");
    // Radius 90 needs 90 mm of each line, and the second is 80 mm long.
    refused("sketch.fillet", json!({"a": c[0], "b": c[1], "radius": 90}), "too large for that corner (it needs 90.000 mm of each line");
    for other in [&arc, &spline] {
        refused("sketch.fillet", json!({"a": c[1], "b": other, "radius": 5}), "not supported yet: filleting 3D or linked sketch curves");
    }
    refused(
        "sketch.fillet",
        json!({"points": [{"a": c[0], "b": c[1]}, {"a": flat[0], "b": flat[1]}], "radius": 5}),
        "3D corners and corners on the sketch plane in separate steps",
    );
    refused(
        "sketch.chamfer.equal_distance",
        json!({"a": c[0], "b": c[1], "distance": 5}),
        "not supported yet: chamfering 3D or linked sketch curves",
    );
}
