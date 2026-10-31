use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::ConstraintKind;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

#[test]
fn redundant_dimensions_are_refused_driven_ones_measure() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "infer": true}))["curves"])[0].clone();
    // Already horizontal (inferred): kept as redundant, unless asked to refuse it.
    let e = s.execute("sketch.constraint.horizontal_vertical", &json!({"line": l, "if_redundant": "refuse"})).unwrap_err().to_string();
    assert!(e.contains("over-constrain") && e.contains("already determined by"), "{e}");
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{l}.start"), "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [l], "value": 10}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 0, "{si}");
    // A second length on it is already determined: refused, unless driven.
    assert!(s.execute("sketch.dimension", &json!({"entities": [format!("{l}.start"), format!("{l}.end")]})).is_err());
    let d = run(&mut s, "sketch.dimension", json!({"entities": [format!("{l}.start"), format!("{l}.end")], "driven": true}));
    assert_eq!(d["driven"], true);
    // The driven dimension follows the driving one.
    let pname = si["constraints"].as_array().unwrap().iter().find_map(|c| c["param"].as_str().map(str::to_string)).unwrap();
    run(&mut s, "parameters.change", json!({"name": pname, "expression": "25 mm"}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let driven = g["glyphs"].as_array().unwrap().iter().find(|x| x["driven"] == true).unwrap().clone();
    assert!((driven["value"].as_f64().unwrap() - 25.0).abs() < 1e-7, "{driven}");
    assert_eq!(g["fully_constrained"], true);
}

/// The same constraint between the same two lines, picked again (in either order), adds nothing:
/// the one already there is reported back as redundant, and the sketch keeps one glyph (#5224).
#[test]
fn the_same_constraint_twice_adds_nothing() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[1, 1], [10, 1]]}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 7]]}))["curves"])[0].clone();
    let first = run(&mut s, "sketch.constraint.parallel", json!({"a": a, "b": b}));
    for (x, y) in [(&a, &b), (&b, &a), (&a, &b)] {
        let r = run(&mut s, "sketch.constraint.parallel", json!({"a": x, "b": y}));
        assert_eq!(r["redundant"], true, "{r}");
        assert_eq!(r["constraint"], first["constraint"], "the one already there: {r}");
    }
    let e = s.execute("sketch.constraint.parallel", &json!({"a": a, "b": b, "if_redundant": "refuse"})).unwrap_err().to_string();
    assert!(e.contains("already determined"), "{e}");
    let parallels = sketch(&s).constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Parallel { .. })).count();
    assert_eq!(parallels, 1);
    let g = run(&mut s, "sketch.glyphs", json!({}));
    assert_eq!(g["glyphs"].as_array().map(Vec::len), Some(1), "{g}");
    // The sketch model itself hands back the one already there.
    let mut sk = sketch(&s);
    let n = sk.constraints.len();
    let k = sk.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Parallel { .. })).unwrap().clone();
    let (ia, ib) = match k.kind {
        ConstraintKind::Parallel { a, b } => (a, b),
        _ => (0, 0),
    };
    assert_eq!(sk.add_constraint(ConstraintKind::Parallel { a: ib, b: ia }, None).unwrap(), k.id);
    assert_eq!(sk.constraints.len(), n);
}

/// `measured` is the dimension on the solved sketch: a trough drawn 99.84 long and dimensioned
/// 100 reports 100, not the length it had before solving.
#[test]
fn a_dimension_reports_the_solved_value() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [99.84, 40.2]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 100}));
    assert!((d["measured"].as_f64().unwrap() - 100.0).abs() < 1e-6, "{d}");
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": "40 mm"}));
    assert!((d["measured"].as_f64().unwrap() - 40.0).abs() < 1e-6, "{d}");
    // A driven dimension measures the solved geometry too.
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["p1", "p3"], "driven": true}));
    assert!((d["measured"].as_f64().unwrap() - 100.0_f64.hypot(40.0)).abs() < 1e-6, "{d}");
    let c = run(&mut s, "sketch.circle.center", json!({"center": [50, 20], "radius": 7.3}));
    let c = ids(&c["curves"])[0].clone();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [c], "value": 15}));
    assert!((d["measured"].as_f64().unwrap() - 15.0).abs() < 1e-6, "{d}");
}

#[test]
fn curvature_polygon_arc_length_linear_diameter() {
    let mut s = new_sketch();
    // A horizontal line ending where a control-point spline starts: G2 keeps the spline's
    // start straight (zero curvature) and tangent.
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-10, 0], [0, 0]], "infer": true}))["curves"])[0].clone();
    let sp =
        ids(&run(&mut s, "sketch.spline.control_point", json!({"points": [format!("{l}.end"), [5, 2], [10, 6], [15, 15]]}))["curves"])[0].clone();
    run(&mut s, "sketch.constraint.curvature", json!({"a": l, "b": sp}));
    let sk = sketch(&s);
    let pts: Vec<Vec2> = sk.curves[sk.curve_index(&sp).unwrap()].kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect();
    // Tangent: first leg along +x; zero curvature: the second leg too (collinear first three).
    assert!((pts[1] - pts[0]).y.abs() < 1e-5, "{pts:?}");
    assert!((pts[2] - pts[1]).cross(pts[1] - pts[0]).abs() < 1e-3, "{pts:?}");

    // Polygon constraint on a rough quadrilateral: becomes a square-ish regular polygon.
    let q = run(&mut s, "sketch.line", json!({"points": [[100, 0], [111, 1], [110, 9], [99, 10]], "closed": true}));
    let ql = ids(&q["curves"]);
    run(&mut s, "sketch.constraint.polygon", json!({"lines": ql}));
    let sk = sketch(&s);
    let lens: Vec<f64> = ql.iter().map(|i| sk.polyline(sk.curve_index(i).unwrap())).map(|p| p[0].dist(p[p.len() - 1])).collect();
    assert!(lens.iter().all(|x| (x - lens[0]).abs() < 1e-6), "{lens:?}");

    // Arc length.
    let a = ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [0, 50], "start": [10, 50], "end": [0, 60]}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [a], "type": "arc_length", "value": 20}));
    let sk = sketch(&s);
    let ai = sk.curve_index(&a).unwrap();
    let len = match sk.segs(ai)[0] {
        solidcraft_geom::Seg2::Arc { radius, sweep, .. } => radius * sweep,
        _ => 0.0,
    };
    assert!((len - 20.0).abs() < 1e-6, "{len}");

    // Linear diameter about a centerline.
    let cl = ids(&run(&mut s, "sketch.line", json!({"points": [[200, 0], [200, 50]]}))["curves"])[0].clone();
    run(&mut s, "sketch.centerline", json!({"curves": [cl]}));
    run(&mut s, "sketch.point", json!({"point": [207, 10], "id": "rim"}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["rim", cl], "value": 30}));
    assert_eq!(d["type"], "LinearDiameterDimension", "{d}");
    let sk = sketch(&s);
    let rim = sk.point(sk.point_index("rim").unwrap()).unwrap();
    let line = sk.polyline(sk.curve_index(&cl).unwrap());
    let (a, b) = (line[0], line[line.len() - 1]);
    let dist = (b - a).normalized().unwrap().cross(rim - a).abs();
    assert!((dist - 15.0).abs() < 1e-6, "{rim:?} {dist}");
}

#[test]
fn glyphs_and_snaps() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10], "connect": false}));
    let l = ids(&r["curves"]);
    let g = run(&mut s, "sketch.glyphs", json!({}));
    assert_eq!(g["glyphs"].as_array().unwrap().len(), 4);
    assert!(g["glyphs"][0]["at"].is_array());
    let sn = run(&mut s, "sketch.snap", json!({"at": [20.3, 0.2], "radius": 1}));
    assert_eq!(sn["snaps"][0]["type"], "coincident", "{sn}");
    let sn = run(&mut s, "sketch.snap", json!({"at": [10.2, 0.3], "radius": 1}));
    assert_eq!(sn["snaps"][0]["type"], "midpoint", "{sn}");
    assert_eq!(sn["snaps"][0]["curve"], json!(l[0]));
    let sn = run(&mut s, "sketch.snap", json!({"at": [40, 10.4], "from": [30, 10], "radius": 1}));
    assert!(sn["snaps"].as_array().unwrap().iter().any(|x| x["type"] == "horizontal"), "{sn}");
}

#[test]
fn auto_constrain_fully_constrains_a_rough_sketch() {
    let mut s = new_sketch();
    // A slightly crooked closed rectangle and a circle inside.
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0.004], [40.003, 30], [0, 30.002]], "closed": true}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
    let r = run(&mut s, "sketch.auto_constrain", json!({}));
    assert_eq!(r["dof"], 0, "{r}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["fully_constrained"], true, "{si}");
    let names: Vec<String> = si["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    assert!(names.iter().filter(|n| *n == "Horizontal").count() >= 2, "{names:?}");
    assert!(names.iter().any(|n| n == "DiameterDimension"), "{names:?}");
    // From a datum, and finishing.
    let mut s = new_sketch();
    run(&mut s, "sketch.point", json!({"point": [5, 5], "id": "d"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [20, 10]]}));
    let r = run(&mut s, "sketch.finish_auto_constrain", json!({"datum": "d"}));
    assert!(s.active_sketch.is_none());
    assert!(r["added"].as_array().unwrap().len() >= 3, "{r}");
}

#[test]
fn auto_constrain_dimensions_construction_curves_without_moving_them() {
    for command in ["sketch.auto_constrain", "sketch.auto_constrain_from_datum", "sketch.finish_auto_constrain"] {
        for (create, params, dimension) in [
            ("sketch.circle.center", json!({"center": [20, 15], "radius": 5}), "diameter"),
            ("sketch.line", json!({"points": [[10, 20], [30, 35]]}), "length"),
            ("sketch.arc.center_point", json!({"center": [20, 15], "start": [25, 15], "end": [20, 20]}), "radius"),
        ] {
            let mut s = new_sketch();
            let curves = run(&mut s, create, params)["curves"].clone();
            run(&mut s, "sketch.construction", json!({"curves": curves, "value": true}));
            let id = s.active_sketch.unwrap();
            let before = sketch(&s);
            let r = run(&mut s, command, json!({"datum": "origin"}));
            assert_eq!(r["dof"], 0, "{command}, {create}: {r}");
            let si = run(&mut s, "sketch.inspect", json!({"sketch": id}));
            assert_eq!(si["fully_constrained"], true, "{si}");
            assert!(si["constraints"].as_array().unwrap().iter().any(|c| c["type"] == dimension), "{si}");
            let state = s.model.state();
            let after = &state.sketch(id).unwrap().sketch;
            assert_eq!(before.curves.len(), after.curves.len());
            assert_eq!(before.points.len(), after.points.len());
            for (a, b) in before.curves.iter().zip(&after.curves) {
                assert_eq!(a.id, b.id);
                assert!(b.construction);
            }
            for (i, _) in before.points.iter().enumerate() {
                assert!(before.point(i).unwrap().dist(after.point(i).unwrap()) < 1e-6);
            }
            for i in 0..before.curves.len() {
                if let Some(radius) = before.radius(i) {
                    assert!((radius - after.radius(i).unwrap()).abs() < 1e-6);
                }
            }
        }
    }
}

#[test]
fn auto_constrain_construction_circle_respects_options_and_existing_constraints() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5, "construction": true}));
    let r = run(&mut s, "sketch.auto_constrain", json!({"dimensions": false}));
    assert_eq!(r["dof"], 3, "{r}");
    assert!(r["added"].as_array().unwrap().is_empty());
    run(&mut s, "sketch.auto_constrain", json!({}));
    let constrained = sketch(&s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "sketch.inspect", json!({}))["dof"], 3);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(sketch(&s), constrained);
    let r = run(&mut s, "sketch.auto_constrain", json!({}));
    assert!(r["added"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(sketch(&s), constrained);

    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5, "construction": true}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "c1"}));
    let fixed = sketch(&s);
    let r = run(&mut s, "sketch.auto_constrain", json!({}));
    assert!(r["added"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(sketch(&s), fixed);
}

#[test]
fn internal_freedom_queries_skip_redundancy_and_keep_constraint_acceptance() {
    use solidcraft_sketch::{ConstraintKind, Sketch, profiling::capture};

    let mut sk = Sketch::new();
    let q = sk.add_point(Vec2::new(10.0, 2.0), None).unwrap();
    let k = ConstraintKind::Distance { p: 0, q, value: 10.0 };
    sk.add_constraint(k.clone(), None).unwrap();
    let redundant = sk.add_constraint(k.clone(), None).unwrap();
    let (dof, profile) = capture(|| super::dof_now(&sk));
    assert_eq!(dof, 1);
    assert_eq!(profile.counters.redundancy_checks, 0);

    let mut doc = solidcraft_doc::Document::default();
    let before = sk.clone();
    let (rejected, profile) = capture(|| super::try_add(&mut sk, &mut doc, k, dof, false));
    assert_eq!(rejected, None, "a duplicate must not be accepted as a DOF reduction");
    assert_eq!(sk, before);
    assert_eq!(profile.counters.redundancy_checks, 0);

    let (accepted, profile) = capture(|| super::try_add(&mut sk, &mut doc, ConstraintKind::DistanceX { p: 0, q, value: 6.0 }, dof, false));
    assert_eq!(accepted, Some(0));
    assert_eq!(profile.counters.redundancy_checks, 0);
    let full = solidcraft_sketch::solve(&mut sk);
    assert!(full.ok());
    assert_eq!(full.dof, 0);
    assert!(full.redundant.contains(&redundant), "the final full report must retain diagnostics");
}

#[test]
fn drawing_infers_tangent_and_perpendicular() {
    let mut s = new_sketch();
    let l1 = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 10]], "infer": true}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("{l1}.end"), [20, 0]], "infer": true}));
    assert_eq!(r["constraints"].as_array().unwrap().len(), 1, "{r}");
    let a = ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [50, 0], "start": [60, 0], "end": [50, 10]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("{a}.end"), [30, 10]], "infer": true}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    let names: Vec<&str> = si["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Perpendicular") && names.contains(&"Tangent"), "{names:?} {r}");
}

#[test]
fn smart_constrain_and_driven_toggle() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0.5]]}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 6]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [a]}));
    assert_eq!(r["type"], "Horizontal");
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [a, b]}));
    assert_eq!(r["type"], "Parallel");
    let c1 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 5}))["curves"])[0].clone();
    let c2 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50.3, 0.2], "radius": 3}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [c1, c2]}));
    assert_eq!(r["type"], "Concentric");
    // Driven toggle.
    let d = run(&mut s, "sketch.dimension", json!({"entities": [c1], "value": 10}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let did = g["glyphs"].as_array().unwrap().iter().find(|x| x["param"] == d["param"]).unwrap()["id"].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.toggle_driven", json!({"constraint": did}));
    assert_eq!(r["driven"], true);
    assert!(s.doc.param(d["param"].as_str().unwrap()).is_none(), "the parameter goes");
    let r = run(&mut s, "sketch.toggle_driven", json!({"constraint": did}));
    assert_eq!(r["driven"], false);
}

/// #7499: a circle's or an arc's dimension switches between its radius and its diameter, the
/// circle keeping its size: the parameter is halved or doubled, an expression wrapped.
#[test]
fn a_radius_dimension_switches_to_a_diameter_and_back() {
    use solidcraft_sketch::ConstraintKind;
    let mut s = new_sketch();
    let dim = |s: &Session, param: &str| sketch(s).constraints.iter().find(|c| c.param.as_deref() == Some(param)).unwrap().clone();
    let c1 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [c1]}));
    let p1 = d["param"].as_str().unwrap().to_string();
    let did = dim(&s, &p1).id;
    assert!(matches!(dim(&s, &p1).kind, ConstraintKind::Diameter { .. }), "a circle is dimensioned by its diameter");
    let r = run(&mut s, "sketch.toggle_diameter", json!({"constraint": did}));
    assert_eq!(r["diameter"], false, "{r}");
    assert!(matches!(dim(&s, &p1).kind, ConstraintKind::Radius { value, .. } if (value - 10.0).abs() < 1e-9), "{:?}", dim(&s, &p1).kind);
    assert_eq!(s.doc.param(&p1).unwrap().expr, "10 mm");
    assert!((sketch(&s).radius(0).unwrap() - 10.0).abs() < 1e-9, "the circle keeps its size");
    // Asking for what it already is changes nothing; then back to the diameter.
    let r = run(&mut s, "sketch.toggle_diameter", json!({"constraint": did, "diameter": false}));
    assert_eq!(r["diameter"], false, "{r}");
    run(&mut s, "sketch.toggle_diameter", json!({"constraint": did}));
    assert!(matches!(dim(&s, &p1).kind, ConstraintKind::Diameter { value, .. } if (value - 20.0).abs() < 1e-9));
    assert_eq!(s.doc.param(&p1).unwrap().expr, "20 mm");

    // An expression is wrapped, so it still reads the parameters it names.
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "30 mm"}));
    let c2 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 10}))["curves"])[0].clone();
    let p2 = run(&mut s, "sketch.dimension", json!({"entities": [c2], "value": "w"}))["param"].as_str().unwrap().to_string();
    let d2 = dim(&s, &p2).id;
    run(&mut s, "sketch.toggle_diameter", json!({"constraint": d2}));
    assert_eq!(s.doc.param(&p2).unwrap().expr, "(w) / 2");
    assert!((sketch(&s).radius(1).unwrap() - 15.0).abs() < 1e-6, "{:?}", sketch(&s).radius(1));

    // Another user of the parameter would change size with it: refused, nothing changes.
    run(&mut s, "parameters.add", json!({"name": "k", "expression": format!("{p1} + 1 mm")}));
    let e = s.execute("sketch.toggle_diameter", &json!({"constraint": did})).unwrap_err().to_string();
    assert!(e.contains("also used by parameter k"), "{e}");
    assert!(matches!(dim(&s, &p1).kind, ConstraintKind::Diameter { .. }));
    assert_eq!(s.doc.param(&p1).unwrap().expr, "20 mm");

    // An arc's driven radius becomes its driven diameter; a line's length is no radius.
    let a = ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [0, 80], "start": [20, 80], "sweep": 90}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [a], "driven": true}));
    let arc = sketch(&s).curves.iter().position(|c| c.id == a).unwrap();
    let driven = sketch(&s).constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Radius { c, .. } if c == arc)).unwrap().clone();
    assert!(driven.driven && driven.param.is_none());
    run(&mut s, "sketch.toggle_diameter", json!({"constraint": driven.id}));
    let now = sketch(&s).constraints.iter().find(|c| c.id == driven.id).unwrap().clone();
    assert!(now.driven && matches!(now.kind, ConstraintKind::Diameter { value, .. } if (value - 40.0).abs() < 1e-6), "{now:?}");
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, -50], [10, -50]]}))["curves"])[0].clone();
    let pl = run(&mut s, "sketch.dimension", json!({"entities": [l], "value": 10}))["param"].as_str().unwrap().to_string();
    let dl = dim(&s, &pl).id;
    let e = s.execute("sketch.toggle_diameter", &json!({"constraint": dl})).unwrap_err().to_string();
    assert!(e.contains("is not a circle's or an arc's radius or diameter dimension"), "{e}");
}

/// #4691: a point put on a line's midpoint snap is held there (a MidPoint constraint), and one
/// put on a curve stays on it, as a line's end does: they no longer move freely.
#[test]
fn a_point_on_a_midpoint_or_a_curve_snap_is_held_there() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let m = run(&mut s, "sketch.point", json!({"point": format!("mid:{l}")}))["point"].as_str().unwrap().to_string();
    let on = run(&mut s, "sketch.point", json!({"point": format!("on:{l}:5,0.2")}))["point"].as_str().unwrap().to_string();
    let si = run(&mut s, "sketch.inspect", json!({}));
    let names: Vec<&str> = si["constraints"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(names.contains(&"MidPoint") && names.contains(&"Coincident"), "{si}");
    // Lengthen the line: the midpoint's point follows the middle, the other stays on the line.
    run(&mut s, "sketch.move_point", json!({"point": format!("{l}.end"), "to": [40, 0]}));
    let sk = sketch(&s);
    let at = |id: &str| sk.resolve_point(id).and_then(|i| sk.point(i)).unwrap();
    assert!(at(&m).dist(Vec2::new(20.0, 0.0)) < 1e-7, "{:?}", at(&m));
    assert!(at(&on).y.abs() < 1e-7, "{:?}", at(&on));
    // A snap that can't be read is refused and makes nothing (it was answered as a point).
    let points = sk.points.len();
    for bad in ["on:c1:abc", "on:c1", "on:c1:1,2,3", "on:l1:1"] {
        let e = s.execute("sketch.point", &json!({"point": bad.replace("l1", &l)})).unwrap_err().to_string();
        assert!(e.contains("is not a snap it takes"), "{bad}: {e}");
    }
    assert_eq!(sketch(&s).points.len(), points, "nothing made");
}

#[test]
fn drawing_from_a_line_midpoint_keeps_it_there() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("mid:{l}"), [10, 15]]}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["constraints"].as_array().unwrap().iter().any(|c| c["name"] == "MidPoint"), "{si}");
    // Lengthen the first line: the second one's start follows the middle.
    run(&mut s, "sketch.move_point", json!({"point": format!("{l}.end"), "to": [40, 0]}));
    let sk = sketch(&s);
    let n = ids(&r["curves"])[0].clone();
    let st = sk.resolve_point(&format!("{n}.start")).and_then(|i| sk.point(i)).unwrap();
    let a = sk.resolve_point(&format!("{l}.start")).and_then(|i| sk.point(i)).unwrap();
    let b = sk.resolve_point(&format!("{l}.end")).and_then(|i| sk.point(i)).unwrap();
    assert!(st.dist((a + b) * 0.5) < 1e-7, "{st:?}");
}

#[test]
fn drawing_onto_a_curve_keeps_the_point_on_it() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("on:{c}:7.2,7.1"), [30, 30]]}));
    let n = ids(&r["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [c], "value": 16}));
    let sk = sketch(&s);
    let st = sk.resolve_point(&format!("{n}.start")).and_then(|i| sk.point(i)).unwrap();
    let cc = sk.center(sk.curve_index(&c).unwrap()).unwrap();
    assert!((st.dist(cc) - 8.0).abs() < 1e-7, "{st:?} {cc:?}");
}

#[test]
fn drawing_at_a_curve_crossing_keeps_the_point_on_both() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.circle.center", json!({"center": [10, 0], "radius": 10}))["curves"])[0].clone();
    let hi = [5.0, 5.0 * 3.0f64.sqrt()];
    let r = run(&mut s, "sketch.line", json!({"points": [format!("x:{a}+{b}:{},{}", hi[0], hi[1]), [40, 10]], "infer": false, "connect": false}));
    let n = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let st = sk.resolve_point(&format!("{n}.start")).and_then(|i| sk.point(i)).unwrap();
    assert!(st.dist(Vec2::new(hi[0], hi[1])) < 1e-6, "{st:?}");
    let start = sk.resolve_point(&format!("{n}.start")).unwrap();
    let ia = sk.curve_index(&a).unwrap();
    let ib = sk.curve_index(&b).unwrap();
    let on = |c: usize| {
        sk.constraints.iter().any(|k| matches!(k.kind, crate::sketch::ConstraintKind::PointOnCurve { p, c: curve } if p == start && curve == c))
    };
    assert!(on(ia) && on(ib), "{:?}", sk.constraints);
    run(&mut s, "sketch.dimension", json!({"entities": [a], "value": 12}));
    let sk = sketch(&s);
    let st = sk.resolve_point(&format!("{n}.start")).and_then(|i| sk.point(i)).unwrap();
    assert!(sk.shape(ia).expect("circle a").dist(st) < 1e-5, "on the resized circle: {st:?}");
    assert!(sk.shape(ib).expect("circle b").dist(st) < 1e-5, "still on the other circle: {st:?}");
    assert!(st.y > 0.0, "the upper crossing: {st:?}");
}

#[test]
fn drawing_at_a_curve_crossing_refuses_hostile_arguments() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "infer": false}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 10], [10, 10]], "infer": false}))["curves"])[0].clone();
    for p in [
        json!({"points": ["x:", [1, 1]]}),
        json!({"points": [format!("x:{a}:1,1"), [1, 1]]}),
        json!({"points": [format!("x:{a}+{a}:5,0"), [1, 1]]}),
        json!({"points": [format!("x:{a}+{b}:nan,0"), [1, 1]]}),
        json!({"points": [format!("x:{a}+{b}:1,2,3"), [1, 1]]}),
        json!({"points": ["x:missing+also:0,0", [1, 1]]}),
        json!({"points": [format!("x:{a}+{b}:5,5"), [1, 1]]}),
        json!({"points": [format!("x:{}+{b}:0,0", "z".repeat(200)), [1, 1]]}),
    ] {
        assert!(s.execute("sketch.line", &p).is_err(), "{p}");
    }
}

/// A faucet spout path: a vertical riser, a horizontal top and an outlet sloping down, joined
/// by fillets. The riser points up and the outlet down-left, so their directions are 150° apart
/// while the lines meet at 30°.
fn spout() -> Session {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[795, 955], [795, 1068], [689, 1068], [661, 1020]], "ids": ["l1", "l2", "l3"]}));
    run(&mut s, "sketch.fillet", json!({"point": "l1.end", "radius": 50}));
    run(&mut s, "sketch.fillet", json!({"point": "l2.end", "radius": 35}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1", "mode": "vertical"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l2", "mode": "horizontal"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l1.start"], "type": "horizontal", "value": 795}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l1.start"], "type": "vertical", "value": 955}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l2.start"], "type": "vertical", "value": 1068}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l3.end", "l1.start"], "type": "horizontal", "value": 134}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l3.end"], "type": "vertical", "value": 1020}));
    assert_eq!(d["sketch"]["dof"], 1, "{d}");
    s
}

/// The angle (degrees, 0..=90) between two lines of the active sketch.
fn line_angle(s: &Session, a: &str, b: &str) -> f64 {
    let sk = sketch(s);
    let dir = |l: &str| {
        let c = sk.curve_index(l).unwrap();
        let (p, q) = crate::cmd::sketch::line_pts(&sk, c).unwrap();
        q - p
    };
    let (u, v) = (dir(a), dir(b));
    let t = u.cross(v).abs().atan2(u.dot(v)).to_degrees();
    t.min(180.0 - t)
}

#[test]
fn an_angle_between_lines_takes_the_reading_nearest_its_value_and_reports_degrees() {
    // The riser and the outlet are not adjacent and meet at 30°; their directions are 150°
    // apart. Asking for 30° used to swing the outlet round and was refused as over-constraining.
    for ents in [["l1", "l3"], ["l3", "l1"]] {
        let mut s = spout();
        let d = run(&mut s, "sketch.dimension", json!({"entities": ents, "type": "angle", "value": "30 deg"}));
        assert_eq!(d["sketch"]["dof"], 0, "{d}");
        assert_eq!(d["unit"], "deg", "{d}");
        assert!((d["measured"].as_f64().unwrap() - 30.0).abs() < 1e-6, "{d}");
        assert!((line_angle(&s, "l1", "l3") - 30.0).abs() < 1e-6);
    }
    // The other reading still works, and the adjacent top line measures 60° the same way.
    let mut s = spout();
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "l3"], "value": "150 deg"}));
    assert_eq!(d["sketch"]["dof"], 0, "{d}");
    assert!((line_angle(&s, "l1", "l3") - 30.0).abs() < 1e-6);
    let mut s = spout();
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l2", "l3"], "value": "60 deg"}));
    assert!((d["measured"].as_f64().unwrap() - 60.0).abs() < 1e-6, "{d}");
    // Without a value the acute angle is kept as it is.
    let mut s = spout();
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "l3"]}));
    let e = d["expression"].as_str().unwrap();
    assert!(e.ends_with(" deg") && (e.trim_end_matches(" deg").parse::<f64>().unwrap() - 30.3).abs() < 0.1, "{d}");
    // Lengths report millimetres.
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "driven": true}));
    assert_eq!(d["unit"], "mm", "{d}");
}

#[test]
fn an_angle_between_two_lines_from_one_point_measures_the_angle_between_them() {
    // #2724: two radii of a circle 120° apart measured 60°, and 150° apart 30°.
    for deg in [30.0_f64, 60.0, 90.0, 120.0, 150.0] {
        let far = [20.0 * deg.to_radians().cos(), 20.0 * deg.to_radians().sin()];
        // Both drawn out from the centre, the second drawn in towards it, or a corner.
        let shapes = [
            vec![json!([[0, 0], [20, 0]]), json!([[0, 0], far])],
            vec![json!([[0, 0], [20, 0]]), json!([far, [0, 0]])],
            vec![json!([[20, 0], [0, 0], far])],
        ];
        for (i, lines) in shapes.iter().enumerate() {
            let mut s = new_sketch();
            for pts in lines {
                run(&mut s, "sketch.line", json!({"points": pts, "connect": false}));
            }
            let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "l2"]}));
            assert!((d["measured"].as_f64().unwrap() - deg).abs() < 1e-4, "{deg}° shape {i}: {d}");
        }
        // A value turns the corner's sides to it, keeping them on the same side of each other.
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [[20, 0], [0, 0], far], "connect": false}));
        let want = deg + 10.0;
        let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "l2"], "value": format!("{want} deg")}));
        assert!((d["measured"].as_f64().unwrap() - want).abs() < 1e-6, "{deg}°: {d}");
        let sk = sketch(&s);
        let line = |l: &str| crate::cmd::sketch::line_pts(&sk, sk.curve_index(l).unwrap()).unwrap();
        let ((a0, a1), (b0, b1)) = (line("l1"), line("l2"));
        assert!(a1.dist(b0) < 1e-9, "the corner stays joined");
        let (ra, rb) = (a0 - a1, b1 - b0);
        let between = ra.cross(rb).abs().atan2(ra.dot(rb)).to_degrees();
        assert!((between - want).abs() < 1e-6, "{deg}°: the sides are {between}° apart");
    }
    // Lines that share no end keep the acute reading.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]], "connect": false}));
    run(&mut s, "sketch.line", json!({"points": [[30, 10], [40, 27.3205]], "connect": false}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "l2"]}));
    assert!((d["measured"].as_f64().unwrap() - 60.0).abs() < 1e-3, "{d}");
}

#[test]
fn symmetry_moves_a_free_line_between_two_fixed_lines() {
    // #1796: with both mirrored lines fixed, the symmetry line between them (a free construction
    // line) is the one to move; it was refused as "neither entity can move".
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 20]], "ids": ["left"]}));
    run(&mut s, "sketch.line", json!({"points": [[10, 0], [10, 20]], "ids": ["right"]}));
    run(&mut s, "sketch.line", json!({"points": [[3, 0], [3, 20]], "construction": true, "ids": ["mid"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["left", "right"]}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "left", "b": "right", "line": "mid"}));
    let sk = sketch(&s);
    let at = |r: &str| sk.point(sk.resolve_point(r).unwrap()).unwrap();
    for end in ["mid.start", "mid.end"] {
        assert!((at(end).x - 5.0).abs() < 1e-7, "{end} at {:?}", at(end));
    }
    for (end, x) in [("left.start", 0.0), ("left.end", 0.0), ("right.start", 10.0), ("right.end", 10.0)] {
        assert!((at(end).x - x).abs() < 1e-9, "{end} moved to {:?}", at(end));
    }
}

/// A 20 x 10 rectangle (sides l1..l4; left = l4 and right = l2 run in opposite directions) and a
/// free vertical line at x = -20 as the symmetry line (#2940, #2961).
fn rectangle_and_axis(dimension_left: bool) -> (Session, Vec<String>) {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-12, 0], "p1": [8, 10]}));
    let sides = ids(&r["curves"]);
    run(&mut s, "sketch.line", json!({"points": [[-20, -5], [-20, 15]], "construction": true, "ids": ["axis"]}));
    if dimension_left {
        run(&mut s, "sketch.dimension", json!({"entities": [sides[3].clone()], "value": 10}));
    }
    (s, sides)
}

/// Both sides keep their `height` and their ends mirror in x about `axis_x`.
fn assert_sides_mirror_with_height(s: &Session, left: &str, right: &str, axis_x: f64, height: f64) {
    let sk = sketch(s);
    let at = |r: String| sk.point(sk.resolve_point(&r).unwrap()).unwrap();
    for side in [left, right] {
        let (a, b) = (at(format!("{side}.start")), at(format!("{side}.end")));
        assert!(((a.y - b.y).abs() - height).abs() < 1e-3, "{side} height {:?} {:?}", a, b);
        assert!((a.x - b.x).abs() < 1e-7, "{side} not vertical: {:?} {:?}", a, b);
    }
    let (l, r) = (at(format!("{left}.start")), at(format!("{right}.start")));
    assert!((l.x + r.x - 2.0 * axis_x).abs() < 1e-6, "sides not mirrored: {l:?} {r:?}");
}

#[test]
fn symmetry_of_rectangle_sides_keeps_their_height() {
    // #2940: the opposite sides of a rectangle run in opposite directions, so pairing their ends
    // by index forced both heights to 0.
    let (mut s, sides) = rectangle_and_axis(false);
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[3], "b": sides[1], "line": "axis"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], -20.0, 10.0);
}

#[test]
fn symmetry_of_a_dimensioned_rectangle_side_is_accepted() {
    // #2961: with a length on a side, the same symmetry was refused as "neither entity can move".
    let (mut s, sides) = rectangle_and_axis(true);
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[3], "b": sides[1], "line": "axis"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], -20.0, 10.0);
}

#[test]
fn symmetry_of_rectangle_sides_with_swapped_arguments() {
    let (mut s, sides) = rectangle_and_axis(true);
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[1], "b": sides[3], "line": "axis"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], -20.0, 10.0);
}

#[test]
fn symmetry_of_rectangle_sides_about_the_y_axis() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-12, 0], "p1": [8, 10]}));
    let sides = ids(&r["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [sides[3].clone()], "value": 10}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[3], "b": sides[1], "line": "y"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], 0.0, 10.0);
}

#[test]
fn symmetry_slides_a_rectangle_whose_width_is_dimensioned() {
    // #4030: with the width fixed, the second side can't move on its own and the axis never
    // moves, so the symmetry was refused as "neither entity can move". The whole rectangle slides
    // sideways instead, keeping its 42 mm width.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-19, -16], "p1": [23, 10], "construction": true}));
    let sides = ids(&r["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [sides[0].clone()], "value": 42}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[3], "b": sides[1], "line": "y"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], 0.0, 26.0);
    let sk = sketch(&s);
    let at = |r: String| sk.point(sk.resolve_point(&r).unwrap()).unwrap();
    let (l, r) = (at(format!("{}.start", sides[3])), at(format!("{}.start", sides[1])));
    assert!((r.x - l.x - 42.0).abs() < 1e-6 && (l.x + 21.0).abs() < 1e-6, "sides at ±21: {l:?} {r:?}");
}

#[test]
fn symmetry_pairs_the_ends_of_fixed_lines_drawn_the_opposite_way() {
    // #1796: the sides of a face outline run in opposite directions (up one side, down the
    // other). Pairing start with start asked for a reflection no line gives, and centring a
    // construction line between them was refused as "neither entity can move".
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 20]], "ids": ["left"]}));
    run(&mut s, "sketch.line", json!({"points": [[10, 20], [10, 0]], "ids": ["right"]}));
    run(&mut s, "sketch.line", json!({"points": [[3, 2], [3, 18]], "construction": true, "ids": ["mid"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["left", "right"]}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "left", "b": "right", "line": "mid"}));
    let sk = sketch(&s);
    let at = |r: &str| sk.point(sk.resolve_point(r).unwrap()).unwrap();
    for end in ["mid.start", "mid.end"] {
        assert!((at(end).x - 5.0).abs() < 1e-7, "{end} at {:?}", at(end));
    }
    assert!((at("mid.end").y - at("mid.start").y - 16.0).abs() < 1e-7, "the line keeps its length");
}

#[test]
fn symmetry_pairs_ends_by_their_mirror_images() {
    // Free lines: each `b` end goes to the mirror image of the `a` end nearest it, so a line
    // drawn either way round mirrors without flipping, and lines meeting at the axis (a V) or
    // end to end along it keep their pairs.
    for (a, b, axis, want) in [
        ([[0.0, 0.0], [0.0, 20.0]], [[10.0, 21.0], [10.0, 1.0]], [[5.0, -5.0], [5.0, 30.0]], [[10.0, 20.0], [10.0, 0.0]]),
        ([[0.0, 0.0], [4.0, 10.0]], [[10.0, 1.0], [6.0, 9.0]], [[5.0, -5.0], [5.0, 30.0]], [[10.0, 0.0], [6.0, 10.0]]),
        ([[0.0, 0.0], [10.0, 0.0]], [[29.0, 1.0], [21.0, -1.0]], [[15.0, -5.0], [15.0, 5.0]], [[30.0, 0.0], [20.0, 0.0]]),
    ] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": a, "ids": ["a"]}));
        run(&mut s, "sketch.line", json!({"points": b, "ids": ["b"]}));
        run(&mut s, "sketch.line", json!({"points": axis, "construction": true, "ids": ["m"]}));
        run(&mut s, "sketch.constraint.fix", json!({"entities": ["a", "m"]}));
        run(&mut s, "sketch.constraint.symmetry", json!({"a": "a", "b": "b", "line": "m"}));
        let sk = sketch(&s);
        let at = |r: &str| sk.point(sk.resolve_point(r).unwrap()).unwrap();
        for (end, [x, y]) in ["b.start", "b.end"].into_iter().zip(want) {
            assert!(at(end).dist(Vec2::new(x, y)) < 1e-6, "{a:?} {b:?}: {end} at {:?}, want [{x}, {y}]", at(end));
        }
    }
}

#[test]
fn midpoint_alignment_persists_undoes_and_survives_endpoint_edits() {
    use solidcraft_sketch::ConstraintKind;
    for mode in ["horizontal", "vertical"] {
        for reverse in [false, true] {
            let mut s = new_sketch();
            run(&mut s, "sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false}));
            run(&mut s, "sketch.point", json!({"point":[24,40],"id":"target"}));
            let before = s.doc.to_json();
            let undo = s.undo.len();
            let refs = if reverse { json!(["target",{"midpoint":"l1"}]) } else { json!([{"midpoint":"l1"},"target"]) };
            let result = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points":refs,"mode":mode}));
            assert_eq!(result["created"]["points"].as_array().unwrap().len(), 1);
            assert_eq!(result["created"]["constraints"].as_array().unwrap().len(), 2);
            assert_eq!(result["sketch"]["dof"], 5, "H/V removes one degree of freedom; the midpoint adds none");
            assert!(result["created"]["curves"].as_array().unwrap().is_empty());
            assert_eq!(s.undo.len(), undo + 1);
            let after = s.doc.to_json();
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc.to_json(), before);
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc.to_json(), after);
            let mut reopened = Session::new(solidcraft_doc::Document::from_json(&after).unwrap());
            reopened.active_sketch = s.active_sketch;
            for end in [[50., -15.], [-20., 65.]] {
                run(&mut reopened, "sketch.move_point", json!({"point":"l1.end","to":end}));
                let sk = sketch(&reopened);
                let point = sk.point(sk.resolve_point("target").unwrap()).unwrap();
                let mid = line_midpoint(&sk, "l1");
                assert!((if mode == "horizontal" { point.y - mid.y } else { point.x - mid.x }).abs() < 1e-6);
                assert_eq!(sk.points.len(), 5);
            }
            let sk = sketch(&reopened);
            assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })));
            assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::HorizontalPoints { .. } | ConstraintKind::VerticalPoints { .. })));
            run(&mut reopened, "sketch.delete", json!({"entities":["l1"]}));
            assert!(!sketch(&reopened).constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })));
            assert!(sketch(&reopened).constraints.iter().any(|c| c.id == result["constraint"].as_str().unwrap()));
        }
    }
}

#[test]
fn midpoint_alignment_rejects_invalid_or_conflicting_references_atomically() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false}));
    run(&mut s, "sketch.circle.center", json!({"center":[40,50],"radius":3}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["l1","c1"]}));
    for points in [
        json!([{"midpoint":"l1"},"c1.center"]),
        json!([{"midpoint":"c1"},"origin"]),
        json!([{"midpoint":"missing"},"origin"]),
        json!([{"midpoint":"l1"},{"midpoint":"l1"}]),
        json!([{"midpoint":12},"origin"]),
        json!([{"midpoint":"l1"}]),
        json!([{"midpoint":"l1"},"origin","c1.center"]),
    ] {
        let before = s.doc.to_json();
        let undo = s.undo.len();
        assert!(s.execute("sketch.constraint.horizontal_vertical", &json!({"points":points,"mode":"vertical"})).is_err(), "{points}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

fn line_midpoint(sk: &solidcraft_sketch::Sketch, id: &str) -> Vec2 {
    // Read the curve's current endpoints: after a corner cut an original endpoint
    // id can still name the retained construction side's corner.
    let solidcraft_sketch::CurveKind::Line { a, b } = sk.curves[sk.curve_index(id).unwrap()].kind else { panic!("expected line") };
    (sk.point(a).unwrap() + sk.point(b).unwrap()) * 0.5
}

#[test]
fn midpoint_alignment_reuses_associative_points_and_rejects_self_alignment() {
    use solidcraft_sketch::ConstraintKind;
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false}));
    // A drawing-time midpoint is an ordinary, reusable point too.
    run(&mut s, "sketch.line", json!({"points":["mid:l1",[20,40]],"infer":false,"connect":false}));
    run(&mut s, "sketch.point", json!({"point":[24,50],"id":"target"}));
    let count = sketch(&s).points.len();
    let result = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points":[{"midpoint":"l1"},"target"],"mode":"vertical"}));
    assert!(result["created"]["points"].as_array().unwrap().is_empty());
    assert_eq!(result["created"]["constraints"].as_array().unwrap().len(), 1);
    let sk = sketch(&s);
    assert_eq!(sk.points.len(), count);
    assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).count(), 1);
    for refs in [json!([{"midpoint":"l1"},"l2.start"]), json!(["l2.start",{"midpoint":"l1"}])] {
        let before = s.doc.to_json();
        let undo = s.undo.len();
        assert!(s.execute("sketch.constraint.horizontal_vertical", &json!({"points":refs})).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

#[test]
fn midpoint_alignment_constraints_delete_independently_and_leave_the_point() {
    use solidcraft_sketch::ConstraintKind;
    for delete_midpoint in [false, true] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false}));
        run(&mut s, "sketch.point", json!({"point":[24,40],"id":"target"}));
        run(&mut s, "sketch.constraint.fix", json!({"entity":"l1"}));
        let result = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points":[{"midpoint":"l1"},"target"],"mode":"vertical"}));
        let sk = sketch(&s);
        let helper = result["created"]["points"][0].as_str().unwrap().to_string();
        let mid = sk.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).unwrap().id.clone();
        let hv = result["constraint"].as_str().unwrap().to_string();
        let deleted = if delete_midpoint { &mid } else { &hv };
        let retained = if delete_midpoint { &hv } else { &mid };
        let before = s.doc.to_json();
        run(&mut s, "sketch.delete", json!({"entities":[deleted]}));
        let sk = sketch(&s);
        assert!(sk.resolve_point(&helper).is_some());
        assert!(sk.constraints.iter().any(|c| &c.id == retained));
        assert!(!sk.constraints.iter().any(|c| &c.id == deleted));
        assert_eq!(sk.points.len(), 5);
        let after = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), after);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&after).unwrap());
        reopened.active_sketch = s.active_sketch;
        assert_eq!(sketch(&reopened).points.len(), 5);
        if delete_midpoint {
            // Once its defining relation is deleted it is an ordinary free point.
            run(&mut reopened, "sketch.move_point", json!({"point":helper,"to":[55,65]}));
            let sk = sketch(&reopened);
            let helper = sk.point(sk.resolve_point(&helper).unwrap()).unwrap();
            let target = sk.point(sk.resolve_point("target").unwrap()).unwrap();
            assert!((helper.x - target.x).abs() < 1e-6);
            assert!(helper.dist(line_midpoint(&sk, "l1")) > 1.0);
        } else {
            run(&mut reopened, "sketch.move_point", json!({"point":"target","to":[55,65]}));
            let sk = sketch(&reopened);
            let helper = sk.point(sk.resolve_point(&helper).unwrap()).unwrap();
            assert!(helper.dist(line_midpoint(&sk, "l1")) < 1e-6);
            assert!((sk.point(sk.resolve_point("target").unwrap()).unwrap().x - helper.x).abs() > 1.0);
        }
    }
}

#[test]
fn midpoint_alignment_keeps_original_side_after_fillet_and_chamfer() {
    use solidcraft_sketch::ConstraintKind;
    for (command, params) in
        [("sketch.fillet", json!({"point":"l1.end","radius":3})), ("sketch.chamfer.equal_distance", json!({"point":"l1.end","distance":3}))]
    {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[10,10],[50,10],[50,40]],"infer":true}));
        run(&mut s, "sketch.point", json!({"point":[32,60],"id":"target"}));
        let aligned = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points":[{"midpoint":"l1"},"target"],"mode":"vertical"}));
        let helper = aligned["created"]["points"][0].as_str().unwrap();
        let before = sketch(&s);
        let original_mid = before.point(before.resolve_point(helper).unwrap()).unwrap();
        run(&mut s, command, params);
        let after = s.doc.to_json();
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&after).unwrap());
        reopened.active_sketch = s.active_sketch;
        let sk = sketch(&reopened);
        let (point, line) = sk
            .constraints
            .iter()
            .find_map(|c| match c.kind {
                ConstraintKind::Midpoint { p, l } => Some((p, l)),
                _ => None,
            })
            .unwrap();
        assert!(sk.curves[line].construction);
        let mid = sk.point(point).unwrap();
        assert!(mid.dist(original_mid) < 1e-5, "{command}: {mid:?} vs {original_mid:?}");
        assert!((sk.point(sk.resolve_point("target").unwrap()).unwrap().x - mid.x).abs() < 1e-6);
        assert!(mid.dist(line_midpoint(&sk, "l1")) > 0.5, "midpoint belongs to original full side");
        assert!(mid.dist(line_midpoint(&sk, &sk.curves[line].id)) < 1e-6);
    }
}

#[test]
fn midpoint_alignment_remaps_after_unrelated_geometry_is_deleted() {
    use solidcraft_sketch::ConstraintKind;
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[-30,-20],[-10,-10]],"infer":false,"connect":false}));
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false}));
    run(&mut s, "sketch.point", json!({"point":[24,40],"id":"target"}));
    let result = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points":[{"midpoint":"l2"},"target"],"mode":"vertical"}));
    run(&mut s, "sketch.delete", json!({"entities":["l1"]}));
    run(&mut s, "sketch.move_point", json!({"point":"l2.end","to":[60,-10]}));
    let sk = sketch(&s);
    let helper = sk.point(sk.resolve_point(result["created"]["points"][0].as_str().unwrap()).unwrap()).unwrap();
    assert!(helper.dist(line_midpoint(&sk, "l2")) < 1e-6);
    assert!((sk.point(sk.resolve_point("target").unwrap()).unwrap().x - helper.x).abs() < 1e-6);
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { l: 0, .. })));
    // Deleting the helper itself removes its H/V and Midpoint relationships.
    run(&mut s, "sketch.delete", json!({"entities":[result["created"]["points"][0]]}));
    assert!(sketch(&s).constraints.is_empty());
}

fn line_ends(sk: &solidcraft_sketch::Sketch, id: &str) -> (Vec2, Vec2) {
    let solidcraft_sketch::CurveKind::Line { a, b } = sk.curves[sk.curve_index(id).unwrap()].kind else { panic!("expected line") };
    (sk.point(a).unwrap(), sk.point(b).unwrap())
}

#[test]
fn midpoint_of_two_lines_centres_the_second_on_the_first() {
    use solidcraft_sketch::ConstraintKind;
    for reverse in [false, true] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[0,0],[60,0]],"infer":false,"connect":false}));
        run(&mut s, "sketch.line", json!({"points":[[5,-10],[25,-10]],"infer":false,"connect":false}));
        let (first, second) = if reverse { ("l2", "l1") } else { ("l1", "l2") };
        let stays = line_ends(&sketch(&s), first);
        let length = {
            let (a, b) = line_ends(&sketch(&s), second);
            a.dist(b)
        };
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let result = run(&mut s, "sketch.constraint.midpoint", json!({"a":first,"b":second}));
        assert_eq!(result["created"]["points"].as_array().unwrap().len(), 1);
        assert_eq!(result["created"]["constraints"].as_array().unwrap().len(), 2);
        assert_eq!(s.undo.len(), undo + 1);
        let sk = sketch(&s);
        assert_eq!(line_ends(&sk, first), stays, "the first pick stays put");
        let (a, b) = line_ends(&sk, second);
        assert!((a.dist(b) - length).abs() < 1e-6, "the second keeps its length");
        assert!(line_midpoint(&sk, first).dist(line_midpoint(&sk, second)) < 1e-6);
        assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).count(), 2);

        // Saved as ordinary points and constraints: it reopens and keeps holding.
        let after = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), after);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&after).unwrap());
        reopened.active_sketch = s.active_sketch;
        run(&mut reopened, "sketch.move_point", json!({"point":"l1.end","to":[80,15]}));
        let sk = sketch(&reopened);
        assert!(line_midpoint(&sk, "l1").dist(line_midpoint(&sk, "l2")) < 1e-6);
    }
}

#[test]
fn midpoint_takes_a_point_and_a_line_in_either_pick_order() {
    for (a, b) in [("p", "l1"), ("l1", "p")] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[0,0],[40,0]],"infer":false,"connect":false}));
        run(&mut s, "sketch.point", json!({"point":[7,9],"id":"p"}));
        run(&mut s, "sketch.constraint.midpoint", json!({"a":a,"b":b}));
        let sk = sketch(&s);
        let p = sk.point(sk.resolve_point("p").unwrap()).unwrap();
        assert!(p.dist(line_midpoint(&sk, "l1")) < 1e-6, "{a} then {b}");
    }
}

#[test]
fn midpoint_of_two_lines_reuses_an_existing_midpoint() {
    use solidcraft_sketch::ConstraintKind;
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[60,0]],"infer":false,"connect":false}));
    run(&mut s, "sketch.line", json!({"points":[[5,-10],[25,-10]],"infer":false,"connect":false}));
    // l3 starts at the midpoint of l2: that point is reused, not duplicated.
    run(&mut s, "sketch.line", json!({"points":["mid:l2",[15,-30]],"infer":false,"connect":false}));
    let count = sketch(&s).points.len();
    let result = run(&mut s, "sketch.constraint.midpoint", json!({"a":"l1","b":"l2"}));
    assert!(result["created"]["points"].as_array().unwrap().is_empty());
    let sk = sketch(&s);
    assert_eq!(sk.points.len(), count);
    assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).count(), 2);
    assert!(line_midpoint(&sk, "l1").dist(line_midpoint(&sk, "l2")) < 1e-6);
    // Applying it again finds nothing to add.
    let before = s.doc.to_json();
    assert!(s.execute("sketch.constraint.midpoint", &json!({"a":"l2","b":"l1"})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn midpoint_rejects_wrong_picks_without_changing_the_sketch() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[60,0]],"infer":false,"connect":false}));
    run(&mut s, "sketch.circle.center", json!({"center":[40,50],"radius":3}));
    run(&mut s, "sketch.point", json!({"point":[7,9],"id":"p"}));
    for params in [
        json!({"a":"l1","b":"l1"}),
        json!({"a":"l1","b":"c1"}),
        json!({"a":"c1","b":"l1"}),
        json!({"a":"p","b":"origin"}),
        json!({"a":"p","b":"c1"}),
        json!({"a":"l1","b":"missing"}),
        json!({"a":"l1"}),
        json!({"a":12,"b":"l1"}),
        // The line's own end: it would shrink the line to a point.
        json!({"a":"l1.end","b":"l1"}),
        json!({"a":"l1","b":"l1.start"}),
        json!({"point":"l1.end","line":"l1"}),
    ] {
        let before = s.doc.to_json();
        let undo = s.undo.len();
        assert!(s.execute("sketch.constraint.midpoint", &params).is_err(), "{params}");
        assert_eq!(s.doc.to_json(), before, "{params}");
        assert_eq!(s.undo.len(), undo, "{params}");
    }
}

/// A two-curve constraint picked on one curve twice is refused, leaving the sketch and the undo
/// history as they were. Parallel, collinear, equal or concentric with itself always held (a
/// constraint that did nothing); perpendicular or tangent to itself never could, and the refusal
/// blamed geometry that can't move.
#[test]
fn a_two_curve_constraint_on_one_curve_twice_is_refused() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[60,10]],"infer":false,"connect":false}));
    run(&mut s, "sketch.circle.center", json!({"center":[40,50],"radius":3}));
    for (cmd, curve) in [
        ("parallel", "l1"),
        ("perpendicular", "l1"),
        ("collinear", "l1"),
        ("tangent", "c1"),
        ("equal", "l1"),
        ("equal", "c1"),
        ("concentric", "c1"),
        ("parallel", "x"),
    ] {
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let err = match s.execute(&format!("sketch.constraint.{cmd}"), &json!({"a":curve,"b":curve})) {
            Ok(v) => panic!("{cmd} of {curve} with itself was accepted: {v}"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("same curve"), "{cmd} {curve}: {err}");
        assert_eq!(s.doc.to_json(), before, "{cmd} {curve}");
        assert_eq!(s.undo.len(), undo, "{cmd} {curve}");
    }
    run(&mut s, "sketch.constraint.parallel", json!({"a":"l1","b":"x"}));
}

/// A parameter's row in `parameters.list` (Null when there is none).
fn param_row(s: &mut Session, name: &str) -> Value {
    let l = run(s, "parameters.list", json!({}));
    l["parameters"].as_array().and_then(|a| a.iter().find(|p| p["name"] == name)).cloned().unwrap_or(Value::Null)
}

/// The distance between a line's ends on the solved sketch.
fn line_length(s: &Session, line: &str) -> f64 {
    let sk = sketch(s);
    let at = |r: &str| sk.point(sk.resolve_point(r).unwrap()).unwrap();
    at(&format!("{line}.start")).dist(at(&format!("{line}.end")))
}

/// A session with the user parameter `len = 60 mm` and a sketch holding one 30 mm line `la`
/// whose start is on the origin.
fn sketch_with_len() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "60 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0]], "ids": ["la"], "infer": false}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "la.start", "b": "origin"}));
    s
}

/// `name = expression` in a dimension's value adds the user parameter and dimensions with it
/// (#2916): the expression may be anything the parameter editor takes (arithmetic, units,
/// parentheses, functions, other parameters), and is stored as typed on the new parameter.
#[test]
fn a_dimension_value_can_define_a_user_parameter() {
    let cases = [
        ("w = 20", "20"),
        ("w=40/2", "40/2"),
        ("  w = (10 + 30) / 2 ", "(10 + 30) / 2"),
        ("w = 1 in - 5.4 mm", "1 in - 5.4 mm"),
        ("w = len / 3", "len / 3"),
        ("w = sqrt(400)", "sqrt(400)"),
        ("#w = 20", "20"),
        ("#w = len / 3", "len / 3"),
        ("w = 2 * (len / 6)", "2 * (len / 6)"),
        ("w = min(len, 20 mm)", "min(len, 20 mm)"),
    ];
    for (value, stored) in cases {
        let mut s = sketch_with_len();
        let d = run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": value}));
        assert_eq!(d["defined"], "w", "{value}: {d}");
        assert_eq!(d["expression"], "w", "{value}: the dimension refers to the parameter: {d}");
        assert!((d["measured"].as_f64().unwrap() - 20.0).abs() < 1e-6, "{value}: {d}");
        assert!((line_length(&s, "la") - 20.0).abs() < 1e-6, "{value}");
        let w = param_row(&mut s, "w");
        assert_eq!(w["source"], "user", "{value}: {w}");
        assert_eq!(w["expression"], stored, "{value}: {w}");
        assert_eq!(w["unit"], "mm", "{value}: {w}");
        assert!((w["value"].as_f64().unwrap() - 20.0).abs() < 1e-9, "{value}: {w}");
        // The dimension follows the parameter from now on.
        run(&mut s, "parameters.change", json!({"name": "w", "expression": "30 mm"}));
        assert!((line_length(&s, "la") - 30.0).abs() < 1e-6, "{value}");
    }
}

/// A bare right-hand side is in the design's unit, like a bare dimension value, and the new
/// parameter carries that unit; an angle's parameter is in degrees.
#[test]
fn a_defined_parameter_takes_the_design_unit_or_degrees() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0]], "ids": ["la"], "infer": false}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": "w = 2"}));
    assert_eq!(d["defined"], "w", "{d}");
    assert!((d["measured"].as_f64().unwrap() - 50.8).abs() < 1e-6, "{d}");
    // Stored with the design's unit spelled out, as a bare dimension value is.
    let w = param_row(&mut s, "w");
    assert_eq!((w["unit"].as_str(), w["expression"].as_str()), (Some("in"), Some("2 in")), "{w}");
    assert!((line_length(&s, "la") - 50.8).abs() < 1e-6);
    // A unit in the expression wins over the design's.
    run(&mut s, "sketch.line", json!({"points": [[0, 50], [30, 50]], "ids": ["lb"], "infer": false}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["lb"], "value": "h = 20 mm"}));
    assert!((d["measured"].as_f64().unwrap() - 20.0).abs() < 1e-6, "{d}");
    assert_eq!((param_row(&mut s, "h")["unit"].as_str(), param_row(&mut s, "h")["expression"].as_str()), (Some("in"), Some("20 mm")));

    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["a"], "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 10]], "ids": ["b"], "infer": false}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["a", "b"], "type": "angle", "value": "ang = 60"}));
    assert_eq!((d["defined"].as_str(), d["unit"].as_str()), (Some("ang"), Some("deg")), "{d}");
    assert!((d["measured"].as_f64().unwrap() - 60.0).abs() < 1e-6, "{d}");
    let ang = param_row(&mut s, "ang");
    assert_eq!((ang["unit"].as_str(), ang["expression"].as_str()), (Some("deg"), Some("60")), "{ang}");
    assert!((line_angle(&s, "a", "b") - 60.0).abs() < 1e-6);
}

/// A definition that cannot be made is refused with the reason, and leaves no parameter behind:
/// a taken or reserved name, no expression, a driven dimension, a left side that is not a name
/// (which stays the old parse error), and a dimension the sketch refuses.
#[test]
fn a_definition_that_cannot_be_made_is_refused_and_leaves_nothing() {
    let refused = [
        ("w = 20", json!({"name": "w", "expression": "5 mm"}), "there is already a parameter `w`: type `w` to use it"),
        ("len = 20", Value::Null, "there is already a parameter `len`"),
        ("mm = 5", Value::Null, "`mm` is a unit name, so it cannot be a parameter name"),
        ("sin = 5", Value::Null, "`sin` is a function name, so it cannot be a parameter name"),
        ("pi = 5", Value::Null, "`pi` is a constant, so it cannot be a parameter name"),
        ("w =", Value::Null, "`w =` needs a value after the `=`"),
        ("w = ", Value::Null, "needs a value"),
        ("2 = 3", Value::Null, "unexpected `=`"),
        ("w == 3", Value::Null, "unexpected `=`"),
        ("w = nope", Value::Null, "nope"),
        ("w = 1 deg", Value::Null, ""),
        ("w = w", Value::Null, ""),
    ];
    for (value, add, message) in refused {
        let mut s = sketch_with_len();
        if !add.is_null() {
            run(&mut s, "parameters.add", add);
        }
        let before = run(&mut s, "parameters.list", json!({}))["count"].clone();
        let e = s.execute("sketch.dimension", &json!({"entities": ["la"], "value": value})).unwrap_err().to_string();
        assert!(e.contains(message), "{value}: {e}");
        assert_eq!(run(&mut s, "parameters.list", json!({}))["count"], before, "{value}: a refused definition added a parameter");
        assert_eq!(sketch(&s).constraints.iter().filter(|c| c.param.is_some()).count(), 0, "{value}: a refused definition added a dimension");
    }
    // A driven (reference) dimension has no value, so it defines nothing.
    let mut s = sketch_with_len();
    let e = s.execute("sketch.dimension", &json!({"entities": ["la"], "value": "w = 20", "driven": true})).unwrap_err().to_string();
    assert!(e.contains("driven"), "{e}");
    assert!(param_row(&mut s, "w").is_null());
    // A dimension the sketch already determines is refused as before, and its parameter with it.
    let mut s = sketch_with_len();
    run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": 30}));
    let e = s.execute("sketch.dimension", &json!({"entities": ["la.start", "la.end"], "value": "w = 30"})).unwrap_err().to_string();
    assert!(e.contains("already determined"), "{e}");
    assert!(param_row(&mut s, "w").is_null(), "the parameter of a refused dimension was kept");
    // Placed as driven instead (`if_redundant`), it has no value and defines nothing.
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["la.start", "la.end"], "value": "w = 30", "if_redundant": "driven"}));
    assert_eq!((d["driven"].as_bool(), d.get("defined")), (Some(true), None), "{d}");
    assert!(param_row(&mut s, "w").is_null(), "a driven dimension kept the parameter");
}

/// The parameter and the dimension are one command, so one undo removes both and one redo
/// brings both back.
#[test]
fn undoing_a_defining_dimension_removes_the_parameter_too() {
    let mut s = sketch_with_len();
    let params = run(&mut s, "parameters.list", json!({}))["count"].as_u64().unwrap();
    run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": "w = 20"}));
    assert_eq!(run(&mut s, "parameters.list", json!({}))["count"].as_u64().unwrap(), params + 2, "the user parameter and the dimension's own");
    run(&mut s, "edit.undo", json!({}));
    assert!(param_row(&mut s, "w").is_null());
    assert_eq!(run(&mut s, "parameters.list", json!({}))["count"].as_u64().unwrap(), params);
    assert!((line_length(&s, "la") - 30.0).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(param_row(&mut s, "w")["expression"], "20");
    assert!((line_length(&s, "la") - 20.0).abs() < 1e-6);
}

/// Coincident on two circles (or arcs) lays the second on the first: one centre and one
/// radius, in one step, and the first stays. It used to be refused ("two curves must both be
/// straight lines").
#[test]
fn coincident_refuses_a_lines_two_ends() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[60,0]],"infer":false,"connect":false}));
    run(&mut s, "sketch.line", json!({"points":[[0,20],[60,20]],"infer":false,"connect":false}));
    // The line would shrink to a point, leaving a later Parallel or Perpendicular on it no
    // direction to hold.
    for params in [json!({"a":"l1.start","b":"l1.end"}), json!({"a":"l1.end","b":"l1.start"})] {
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let err = s.execute("sketch.constraint.coincident", &params).unwrap_err().to_string();
        assert!(err.contains("shrink to a point"), "{params}: {err}");
        assert_eq!(s.doc.to_json(), before, "{params}");
        assert_eq!(s.undo.len(), undo, "{params}");
    }
    // Ends of two different lines still join.
    run(&mut s, "sketch.constraint.coincident", json!({"a":"l1.end","b":"l2.end"}));
}

/// A constraint whose solve finds only a far solution is not allowed to throw points far away:
/// the held solve is tried without it, or the constraint is refused. Here a centre that is
/// already the midpoint of two lines is made the midpoint of a third, and the solve threw two
/// line ends to x = -2.2e8 mm while the sketch said solved (found by a local sketch fuzzer).
#[test]
fn a_constraint_never_throws_the_sketch_far_away() {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.arc.three_point", json!({"end": [-15, 6], "start": [-24, -21], "through": [-12, -1]})),
        ("sketch.arc.three_point", json!({"end": [-18, -27], "start": [3, 13], "through": [22, 21]})),
        ("sketch.dimension", json!({"entities": ["a1.start", "a2.center"], "value": 29})),
        ("sketch.dimension", json!({"entities": ["a2.end", "a1.start"], "value": 38})),
        ("sketch.circle.center", json!({"center": [-23, -10], "radius": 7})),
        ("sketch.constraint.coincident", json!({"a": "a2.start", "b": "a1.end"})),
        ("sketch.rectangle.two_point", json!({"p0": [22, -9], "p1": [19, -10]})),
        ("sketch.line", json!({"ids": ["l16"], "points": [[-14, -29], [-5, 5]]})),
        ("sketch.circle.center", json!({"center": [-14, 19], "radius": 14})),
        ("sketch.constraint.fix", json!({"entity": "c1.center"})),
        ("sketch.rectangle.two_point", json!({"p0": [-10, 23], "p1": [16, 13]})),
        ("sketch.line", json!({"ids": ["l22"], "points": [[-8, 21], [25, 14]]})),
        ("sketch.circle.center", json!({"center": [6, 10], "radius": 10})),
        ("sketch.circle.center", json!({"center": [8, -12], "radius": 11})),
        ("sketch.rectangle.two_point", json!({"p0": [-4, -7], "p1": [-5, -27]})),
        ("sketch.constraint.concentric", json!({"a": "a1", "b": "c4"})),
        ("sketch.rectangle.two_point", json!({"p0": [-12, -14], "p1": [-1, -17]})),
        ("sketch.constraint.midpoint", json!({"line": "l12", "point": "c4.center"})),
        ("sketch.constraint.midpoint", json!({"line": "l14", "point": "c4.center"})),
        ("sketch.rectangle.two_point", json!({"p0": [23, 22], "p1": [-28, 8]})),
        ("sketch.constraint.fix", json!({"entity": "p15"})),
        ("sketch.dimension", json!({"entities": ["p17", "a1.start"], "value": 35})),
        ("sketch.constraint.coincident", json!({"a": "p19", "b": "p12"})),
        ("sketch.constraint.tangent", json!({"a": "c1", "b": "l11"})),
        ("sketch.line", json!({"closed": true, "points": [[8, -29], [-5, 24], [-9, -6], [26, -8]]})),
        ("sketch.constraint.perpendicular", json!({"a": "l15", "b": "l17"})),
        ("sketch.dimension", json!({"entities": ["l12"], "value": 13})),
        ("sketch.dimension", json!({"entities": ["l26", "l17"], "type": "angle", "value": 14})),
        ("sketch.constraint.fix", json!({"entity": "p18"})),
        ("sketch.constraint.tangent", json!({"a": "l23", "b": "c4"})),
        ("sketch.constraint.coincident", json!({"a": "origin", "b": "l23.start"})),
        ("sketch.constraint.midpoint", json!({"line": "l18", "point": "c4.center"})),
    ] {
        let _ = s.execute(id, &p);
    }
    let sk = sketch(&s);
    let far = sk.points.iter().map(|p| p.pos.len()).fold(0.0_f64, f64::max);
    assert!(far < 1e4, "a point went {far} mm away");
}

/// A constraint that can only be met by shrinking curves to a point is refused, as one that
/// throws the sketch far away is: making a rectangle's corner the midpoint of the opposite side,
/// or two opposite corners coincident, squeezed the whole rectangle to a point while the sketch
/// said solved (found by a local sketch fuzzer).
#[test]
fn a_constraint_that_would_shrink_a_rectangle_to_a_point_is_refused() {
    for (id, p) in
        [("sketch.constraint.midpoint", json!({"line": "l4", "point": "p2"})), ("sketch.constraint.coincident", json!({"a": "p1", "b": "p3"}))]
    {
        let mut s = new_sketch();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-30, 4], "p1": [11, 27]}));
        let before = sketch(&s);
        let e = s.execute(id, &p).unwrap_err().to_string();
        assert!(e.contains("shrink") && e.contains("to a point"), "{id} {p}: {e}");
        assert_eq!(sketch(&s), before, "{id} {p}");
    }
}

/// Two rectangles, a corner of one made coincident with a corner of the other and then a second
/// pair of corners made coincident: one solve squeezed a 49 mm side to about 1.2e-6 mm, just
/// longer than the check took for a point, and the next took it the rest of the way, so the
/// constraint was taken (found by a local sketch fuzzer).
#[test]
fn a_constraint_that_squeezes_a_side_to_a_hair_is_refused() {
    let mut s = new_sketch();
    for (id, p) in [
        ("sketch.rectangle.two_point", json!({"p0": [27, 29], "p1": [1, 19]})),
        ("sketch.rectangle.two_point", json!({"p0": [23, -8], "p1": [-21, -20]})),
        ("sketch.constraint.perpendicular", json!({"a": "l5", "b": "l4"})),
        ("sketch.constraint.coincident", json!({"a": "p5", "b": "p1"})),
    ] {
        run(&mut s, id, p);
    }
    let before = sketch(&s);
    let e = s.execute("sketch.constraint.coincident", &json!({"a": "p6", "b": "p3"})).unwrap_err().to_string();
    assert!(e.contains("shrink") && e.contains("to a point"), "{e}");
    assert_eq!(sketch(&s), before);
}

#[test]
fn coincident_circles_lie_on_each_other() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":10}));
    run(&mut s, "sketch.circle.center", json!({"center":[3,1],"radius":8}));
    let undo = s.undo.len();
    let r = run(&mut s, "sketch.constraint.coincident", json!({"a":"c1","b":"c2"}));
    assert_eq!(r["constraints"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(s.undo.len(), undo + 1);
    let sk = sketch(&s);
    let circle = |id: &str| {
        let c = sk.curve_index(id).unwrap();
        match sk.curves[c].kind {
            solidcraft_sketch::CurveKind::Circle { c, r } => (sk.point(c).unwrap(), r),
            _ => panic!("{id} is a circle"),
        }
    };
    let ((c1, r1), (c2, r2)) = (circle("c1"), circle("c2"));
    assert!(c1.dist(Vec2::new(0.0, 0.0)) < 1e-6 && (r1 - 10.0).abs() < 1e-6, "the first stays");
    assert!(c2.dist(c1) < 1e-6 && (r2 - r1).abs() < 1e-6, "the second lies on it: {c2:?} r {r2}");
    // Again: nothing to add.
    assert!(s.execute("sketch.constraint.coincident", &json!({"a":"c1","b":"c2"})).is_err());
    // An arc onto a circle.
    run(&mut s, "sketch.arc.three_point", json!({"start":[20,0],"through":[25,5],"end":[30,0]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a":"c1","b":"a1"}));
    let sk = sketch(&s);
    let a = sk.curve_index("a1").unwrap();
    let solidcraft_sketch::CurveKind::Arc { c, a: start, .. } = sk.curves[a].kind else { panic!("a1 is an arc") };
    assert!(sk.point(c).unwrap().dist(Vec2::new(0.0, 0.0)) < 1e-6);
    assert!((sk.point(start).unwrap().dist(sk.point(c).unwrap()) - 10.0).abs() < 1e-6);
}

#[test]
fn midpoint_edge_dimensions_track_the_midpoint_and_reuse_it() {
    for reverse in [false, true] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[0,10],[20,10]]}));
        run(&mut s, "sketch.line", json!({"points":[[-10,-10],[-10,40]]}));
        run(&mut s, "sketch.constraint.fix", json!({"entity":"l2"}));
        let length = run(&mut s, "sketch.dimension", json!({"entities":["l1"],"value":20}))["param"].as_str().unwrap().to_string();
        let baseline = s.doc.to_json();
        let entities = if reverse { json!(["l2", "mid:l1"]) } else { json!(["mid:l1", "l2"]) };
        let d = run(&mut s, "sketch.dimension", json!({"entities":entities,"value":25}));
        assert_eq!(d["unit"], "mm");
        assert!((d["measured"].as_f64().unwrap() - 25.).abs() < 1e-6);
        let param = d["param"].as_str().unwrap().to_string();
        let accepted = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), baseline, "the helper and dimension undo together");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), accepted);
        run(&mut s, "parameters.change", json!({"name":length,"expression":"40 mm"}));
        run(&mut s, "parameters.change", json!({"name":param,"expression":"30 mm"}));
        run(&mut s, "sketch.dimension", json!({"entities":entities,"driven":true}));
        let sk = sketch(&s);
        let mids: Vec<_> = sk
            .constraints
            .iter()
            .filter_map(|c| match c.kind {
                solidcraft_sketch::ConstraintKind::Midpoint { p, l } if l == sk.curve_index("l1").unwrap() => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(mids.len(), 1, "another dimension reuses the constrained midpoint");
        let mid = sk.point(mids[0]).unwrap();
        let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
        let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
        assert!(mid.dist((a + b) * 0.5) < 1e-6);
        assert!((mid.x - 20.).abs() < 1e-6, "distance from the fixed x=-10 edge is 30 mm");
        assert!((a.dist(b) - 40.).abs() < 1e-6);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        reopened.active_sketch = s.active_sketch;
        assert_eq!(sketch(&reopened), sk);
    }
}

#[test]
fn invalid_midpoint_dimension_references_leave_the_design_unchanged() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,10],[20,10]]}));
    run(&mut s, "sketch.circle.center", json!({"center":[30,30],"radius":5}));
    let baseline = s.doc.to_json();
    for entities in [json!(["mid:missing", "l1"]), json!(["mid:c1", "l1"]), json!(["mid:l1", "missing"])] {
        assert!(s.execute("sketch.dimension", &json!({"entities":entities,"value":10})).is_err());
        assert_eq!(s.doc.to_json(), baseline);
    }
}

#[test]
fn midpoint_pair_orientation_uses_the_text_box_and_respects_explicit_types() {
    use solidcraft_sketch::ConstraintKind as K;
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,10],[20,10]],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[[40,20],[40,40]],"infer":false}));
    for (ty, at, axis) in [
        ("auto", [25., 33.], 'x'),
        ("auto", [43., 20.], 'y'),
        ("auto", [25., 20.], 'd'),
        // The box's outer corners keep the distance between them, as before #7355.
        ("auto", [50., 45.], 'd'),
        ("auto", [55., 35.], 'd'),
        ("auto", [0., 0.], 'd'),
        ("auto", [-5., 5.], 'd'),
        ("distance", [25., 33.], 'd'),
        ("horizontal", [43., 20.], 'x'),
        ("vertical", [25., 33.], 'y'),
    ] {
        let mut trial = s.scratch();
        run(&mut trial, "sketch.dimension", json!({"entities":["mid:l1","mid:l2"],"type":ty,"text_at":at,"driven":true}));
        let sk = sketch(&trial);
        let c = sk.constraints.iter().rev().find(|c| c.kind.is_dimension()).unwrap();
        assert!(
            match axis {
                'x' => matches!(c.kind, K::DistanceX { .. }),
                'y' => matches!(c.kind, K::DistanceY { .. }),
                _ => matches!(c.kind, K::Distance { .. }),
            },
            "{ty} at {at:?}: {:?}",
            c.kind
        );
        let expected = match axis {
            'x' => 30.,
            'y' => 20.,
            _ => 1300_f64.sqrt(),
        };
        assert!((solidcraft_sketch::measure_dimension(&sk, &c.kind).unwrap() - expected).abs() < 1e-6);
    }
}

/// Four circles, `c1` dimensioned to radius 5 (diameter 10).
fn four_circles() -> Session {
    let mut s = new_sketch();
    for (x, r) in [(0, 5), (20, 3), (40, 7), (60, 4)] {
        run(&mut s, "sketch.circle.center", json!({"center": [x, 0], "radius": r}));
    }
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "value": 10}));
    s
}

fn radius_of(s: &Session, id: &str) -> f64 {
    let sk = sketch(s);
    sk.radius(sk.curve_index(id).unwrap()).unwrap()
}

#[test]
fn equal_over_a_list_of_curves_makes_them_all_equal_in_one_step() {
    // #4985: whatever the order, the dimensioned circle keeps its size and the others take it.
    for order in [["c1", "c2", "c3", "c4"], ["c4", "c3", "c2", "c1"]] {
        let mut s = four_circles();
        let r1 = radius_of(&s, "c1");
        let undo = s.undo.len();
        let out = run(&mut s, "sketch.constraint.equal", json!({"curves": order}));
        assert_eq!(out["constraints"].as_array().unwrap().len(), 3, "{order:?}");
        assert_eq!(s.undo.len(), undo + 1, "one undo step");
        for id in ["c1", "c2", "c3", "c4"] {
            assert!((radius_of(&s, id) - r1).abs() < 1e-6, "{order:?} {id}: {}", radius_of(&s, id));
        }
        // Applying it again adds nothing: those pairs are already equal.
        let again = run(&mut s, "sketch.constraint.equal", json!({"curves": order}));
        assert!(again["constraints"].as_array().unwrap().is_empty());
    }
}

fn offset_lines(vertical: bool) -> Session {
    let mut s = new_sketch();
    let points = if vertical { json!([[0, 0], [0, 40]]) } else { json!([[0, 0], [40, 0]]) };
    run(&mut s, "sketch.line", json!({"points": points, "ids": ["base"], "infer": false}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "base"}));
    let points = if vertical { json!([[10, 43], [10, 48]]) } else { json!([[43, 10], [48, 10]]) };
    run(&mut s, "sketch.line", json!({"points": points, "ids": ["slider"], "infer": false, "connect": false}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "slider", "mode": if vertical { "vertical" } else { "horizontal" }}));
    run(&mut s, "sketch.dimension", json!({"entities": ["slider"], "value": 5}));
    run(
        &mut s,
        "sketch.dimension",
        json!({"entities": ["base.end", "slider.start"], "type": if vertical { "horizontal" } else { "vertical" }, "value": 10}),
    );
    assert_eq!(run(&mut s, "sketch.inspect", json!({}))["dof"], 1);
    s
}

#[test]
fn component_dimensions_beside_a_perpendicular_dimension_solve() {
    use solidcraft_sketch::ConstraintKind;
    for vertical in [false, true] {
        for ty in if vertical { ["vertical", "distance_y"] } else { ["horizontal", "distance_x"] } {
            for value in [0.0, 0.5, 1.0] {
                let mut s = offset_lines(vertical);
                let out = run(&mut s, "sketch.dimension", json!({"entities": ["base.end", "slider.start"], "type": ty, "value": value}));
                assert_eq!(out["sketch"]["dof"], 0, "{ty}, {value}: {out}");
                let sk = sketch(&s);
                let k = &sk.constraints.last().unwrap().kind;
                assert!(
                    if vertical { matches!(k, ConstraintKind::DistanceY { .. }) } else { matches!(k, ConstraintKind::DistanceX { .. }) },
                    "{k:?}"
                );
                let a = sk.point(sk.resolve_point("base.end").unwrap()).unwrap();
                let b = sk.point(sk.resolve_point("slider.start").unwrap()).unwrap();
                let d = b - a;
                assert!((if vertical { d.y } else { d.x }.abs() - value).abs() < 1e-6, "{d:?}");
                assert!((if vertical { d.x } else { d.y }.abs() - 10.0).abs() < 1e-6, "{d:?}");
            }
        }
    }
}

#[test]
fn equal_over_a_list_refuses_bad_lists_and_conflicts_without_changing_the_sketch() {
    let mut s = four_circles();
    let before = s.doc.to_json();
    for bad in [json!({"curves": ["c1"]}), json!({"curves": ["c1", "c1"]}), json!({"curves": ["c1", "nope"]}), json!({"curves": vec!["c1"; 201]})] {
        assert!(s.execute("sketch.constraint.equal", &bad).is_err(), "{bad}");
        assert_eq!(s.doc.to_json(), before, "{bad}");
    }
    // Two circles dimensioned to different sizes can't be made equal: nothing changes.
    run(&mut s, "sketch.dimension", json!({"entities": ["c2"], "value": 6}));
    let before = s.doc.to_json();
    assert!(s.execute("sketch.constraint.equal", &json!({"curves": ["c1", "c2", "c3"]})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

/// MidPoint on a point and a line, picked either way round: the point goes to the line's
/// middle and the line stays. Picked point first, the line used to turn about its far end to
/// bring its middle to the point (the end of another line clicked, then the line).
#[test]
fn midpoint_moves_the_point_whichever_is_picked_first() {
    for (a, b) in [("l2.start", "l1"), ("l1", "l2.start")] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[0,0],[20,0]],"infer":false,"connect":false}));
        run(&mut s, "sketch.line", json!({"points":[[5,5],[15,15]],"infer":false,"connect":false}));
        let stays = line_ends(&sketch(&s), "l1");
        run(&mut s, "sketch.constraint.midpoint", json!({"a": a, "b": b}));
        let sk = sketch(&s);
        assert_eq!(line_ends(&sk, "l1"), stays, "{a}, {b}: the line stays");
        let (start, _) = line_ends(&sk, "l2");
        assert!(start.dist(Vec2::new(10.0, 0.0)) < 1e-6, "{a}, {b}: the point is at the middle: {start:?}");
    }
    // A point that can't move: the line comes to it instead.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[20,0]],"infer":false,"connect":false}));
    run(&mut s, "sketch.point", json!({"point":[5,5],"id":"p"}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["p"],"fixed":true}));
    run(&mut s, "sketch.constraint.midpoint", json!({"a":"p","b":"l1"}));
    assert!(line_midpoint(&sketch(&s), "l1").dist(Vec2::new(5.0, 5.0)) < 1e-6);
}

#[test]
fn conflicting_component_dimension_is_still_refused() {
    let mut s = offset_lines(false);
    let before = serde_json::to_value(sketch(&s)).unwrap();
    let error =
        s.execute("sketch.dimension", &json!({"entities": ["base.end", "slider.start"], "type": "vertical", "value": 11})).unwrap_err().to_string();
    assert!(error.contains("would over-constrain the sketch (conflicts with"), "{error}");
    assert_eq!(serde_json::to_value(sketch(&s)).unwrap(), before);
}

/// #754: an arc and one of its end points, in either order, dimension the arc's length (the
/// pair was refused). A line's end that meets the arc's end counts too; another point does not.
#[test]
fn an_arc_and_its_end_point_dimension_the_arc_length() {
    let arc_length = |s: &Session, a: &str| {
        let sk = sketch(s);
        match sk.segs(sk.curve_index(a).unwrap())[0] {
            solidcraft_geom::Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
            _ => f64::NAN,
        }
    };
    for (entities, value) in [(json!(["a1", "a1.end"]), 20.0), (json!(["a1.start", "a1"]), 18.0), (json!(["a1", "l1.start"]), 25.0)] {
        let mut s = new_sketch();
        run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [10, 0], "end": [0, 10], "id": "a1"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 10], [-15, 10]], "ids": ["l1"]}));
        let r = run(&mut s, "sketch.dimension", json!({"entities": entities, "value": value}));
        assert_eq!(r["type"], "ArcLengthDimension", "{entities}: {r}");
        assert!((arc_length(&s, "a1") - value).abs() < 1e-6, "{entities}: {}", arc_length(&s, "a1"));
    }
    let mut s = new_sketch();
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [10, 0], "end": [0, 10], "id": "a1"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p9"}));
    let e = s.execute("sketch.dimension", &json!({"entities": ["a1", "p9"]})).map(|v| v["type"].clone());
    assert!(!matches!(e, Ok(ref t) if t == "ArcLengthDimension"), "a point off the arc's ends is not an arc length: {e:?}");
}

/// #7464: the distance between a slot's two arc caps measures the centre-to-centre distance
/// (the slot's length), not the gap between their inner edges.
#[test]
fn slot_caps_dimension_is_the_centre_to_centre_distance() {
    let mut s = new_sketch();
    // Centre [0,0], one arc centre [20,0], width 10: caps at ±20 of radius 5, centre-to-centre 40.
    let sl = run(&mut s, "sketch.slot.center_point", json!({"center": [0, 0], "end": [20, 0], "width": 10}));
    // The slot returns its curves as [bottom line, right cap, top line, left cap].
    let curves = ids(&sl["curves"]);
    let d = run(&mut s, "sketch.dimension", json!({"entities": [curves[1], curves[3]], "driven": true}));
    assert!((d["measured"].as_f64().unwrap() - 40.0).abs() < 1e-6, "{d}");
    let sk = sketch(&s);
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Distance { .. })),
        "{:?}",
        sk.constraints.iter().map(|c| &c.kind).collect::<Vec<_>>()
    );
    // Two circles drawn apart, not joined as a slot, still measure the gap between their edges (#4064).
    let mut s = new_sketch();
    let c1 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}))["curves"])[0].clone();
    let c2 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 5}))["curves"])[0].clone();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [c1, c2], "driven": true}));
    assert!((d["measured"].as_f64().unwrap() - 20.0).abs() < 1e-6, "{d}");
}

/// Two arcs joined by two lines but bulging inward are not a slot's caps: their distance stays
/// the gap between the edges, not a centre-to-centre distance.
#[test]
fn inward_bulging_arc_pairs_keep_the_gap_dimension() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 10], [40, 10]], "ids": ["l2"]}));
    run(&mut s, "sketch.arc.three_point", json!({"start": "l1.start", "through": [3, 5], "end": "l2.start", "id": "ca"}));
    run(&mut s, "sketch.arc.three_point", json!({"start": "l1.end", "through": [37, 5], "end": "l2.end", "id": "cb"}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["ca", "cb"], "driven": true}));
    // The shape spans x 0..40; the gap between the inward edges is 34 mm, not an overall length.
    assert!((d["measured"].as_f64().unwrap() - 34.0).abs() < 1e-6, "{d}");
    let sk = sketch(&s);
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::CircleCircleDistance { .. })),
        "{:?}",
        sk.constraints.iter().map(|c| &c.kind).collect::<Vec<_>>()
    );
}

/// #9735: the over-constrain conflict list named the refused dimension's own id — the trial
/// sketch already holds the new constraint under the id it would get, and the next constraint
/// added takes that id. The list names only constraints the sketch already has.
#[test]
fn over_constrain_message_leaves_out_the_refused_dimensions_own_id() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 50}));
    // The sketch holds k1..k6: the refused dimension would take k7, which is not yet one of them.
    let e = s.execute("sketch.dimension", &json!({"entities": ["l3"], "value": 30})).unwrap_err().to_string();
    assert!(e.contains("would over-constrain the sketch (conflicts with"), "{e}");
    assert!(e.contains("k3") && e.contains("k4") && e.contains("k6"), "{e}");
    assert!(!e.contains("k7"), "{e}: the refused dimension's own id must not be listed");
    // The next constraint takes k7: the id the message must not have burned.
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 25}));
    assert_eq!(sketch(&s).constraints.last().unwrap().id, "k7");
}

/// #9735: "neither entity can move … (they are fixed or fully constrained)" fired for sketches
/// with free degrees of freedom. The refusal now lists the constraints the new one conflicts
/// with by id, as the over-constrain message does.
#[test]
fn conflict_lists_the_constraints_instead_of_claiming_the_entities_are_fixed() {
    // A rectangle from (10,10): 4 degrees of freedom, nothing fixed; its bottom side cannot be
    // made parallel to its right side — the conflict is with the rectangle's own constraints.
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [50, 30]}));
    let before = serde_json::to_value(sketch(&s)).unwrap();
    let e = s.execute("sketch.constraint.parallel", &json!({"a": "l1", "b": "l2"})).unwrap_err().to_string();
    assert!(!e.contains("fixed or fully constrained"), "{e}");
    // The horizontal on l1 (k1) and the vertical on l2 (k3) are what the parallel conflicts with.
    assert!(e.contains("conflicts with k1, k3"), "{e}");
    assert!(!e.contains("k5"), "{e}: the refused constraint's own id must not be listed");
    assert_eq!(serde_json::to_value(sketch(&s)).unwrap(), before, "the sketch does not change");
    // A rectangle drawn from the origin (2 degrees of freedom): the same refusals name the
    // constraints they conflict with, not fixed entities.
    for (cmd, params) in [
        ("sketch.constraint.parallel", json!({"a": "l1", "b": "l2"})),
        ("sketch.constraint.coincident", json!({"a": "p1", "b": "p3"})),
        ("sketch.constraint.perpendicular", json!({"a": "l1", "b": "l3"})),
    ] {
        let mut s = new_sketch();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        let e = s.execute(cmd, &params).unwrap_err().to_string();
        assert!(!e.contains("fixed or fully constrained"), "{cmd} {params}: {e}");
        assert!(e.contains("conflicts with"), "{cmd} {params}: {e}");
    }
}
