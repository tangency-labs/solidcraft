use serde_json::{Value, json};
use solidcraft_geom::Vec2;

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

/// Both sides keep their 10 mm height and their ends mirror in x about `axis_x`.
fn assert_sides_mirror_with_height(s: &Session, left: &str, right: &str, axis_x: f64) {
    let sk = sketch(s);
    let at = |r: String| sk.point(sk.resolve_point(&r).unwrap()).unwrap();
    for side in [left, right] {
        let (a, b) = (at(format!("{side}.start")), at(format!("{side}.end")));
        assert!((a.y - b.y).abs() > 9.999 && (a.y - b.y).abs() < 10.001, "{side} height {:?} {:?}", a, b);
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
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], -20.0);
}

#[test]
fn symmetry_of_a_dimensioned_rectangle_side_is_accepted() {
    // #2961: with a length on a side, the same symmetry was refused as "neither entity can move".
    let (mut s, sides) = rectangle_and_axis(true);
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[3], "b": sides[1], "line": "axis"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], -20.0);
}

#[test]
fn symmetry_of_rectangle_sides_with_swapped_arguments() {
    let (mut s, sides) = rectangle_and_axis(true);
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[1], "b": sides[3], "line": "axis"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], -20.0);
}

#[test]
fn symmetry_of_rectangle_sides_about_the_y_axis() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-12, 0], "p1": [8, 10]}));
    let sides = ids(&r["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [sides[3].clone()], "value": 10}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": sides[3], "b": sides[1], "line": "y"}));
    assert_sides_mirror_with_height(&s, &sides[3], &sides[1], 0.0);
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
    ] {
        let before = s.doc.to_json();
        let undo = s.undo.len();
        assert!(s.execute("sketch.constraint.midpoint", &params).is_err(), "{params}");
        assert_eq!(s.doc.to_json(), before, "{params}");
        assert_eq!(s.undo.len(), undo, "{params}");
    }
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

#[test]
fn concentric_circles_can_be_dimensioned_relative_to_each_other() {
    let mut s = new_sketch();
    let c1 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 35}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [&c1], "type": "diameter", "value": 70}));
    let c2 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 45}))["curves"])[0].clone();

    // Select inner circle and outer circle and dimension it to 18mm.
    let d = run(&mut s, "sketch.dimension", json!({"entities": [&c1, &c2], "value": 18}));
    assert_eq!(d["type"], "LinearDimension");
    assert!((d["measured"].as_f64().unwrap() - 18.0).abs() < 1e-6, "{d}");

    // Outer circle radius is now 35 + 18 = 53mm (diameter 106mm).
    let sk = sketch(&s);
    let r2 = sk.radius(sk.curve_index(&c2).unwrap()).unwrap();
    assert!((r2 - 53.0).abs() < 1e-6, "r2: {r2}");

    // Editing inner circle from 70mm to 80mm keeps the 18mm offset (outer circle becomes 40 + 18 = 58mm).
    let pname = d["param"].as_str().unwrap();
    let p1 = sk.constraints.iter().find_map(|c| if c.param.as_deref() != Some(pname) { c.param.clone() } else { None }).unwrap();
    run(&mut s, "parameters.change", json!({"name": p1, "expression": "80 mm"}));
    let sk = sketch(&s);
    let r2 = sk.radius(sk.curve_index(&c2).unwrap()).unwrap();
    assert!((r2 - 58.0).abs() < 1e-6, "updated r2: {r2}");
}

#[test]
fn non_concentric_circles_dimension_distance_between_centers() {
    let mut s = new_sketch();
    let c1 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    let c2 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 10}))["curves"])[0].clone();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [&c1, &c2], "value": 60}));
    assert!((d["measured"].as_f64().unwrap() - 60.0).abs() < 1e-6, "{d}");
    let sk = sketch(&s);
    let (p1, p2) = (sk.center(sk.curve_index(&c1).unwrap()).unwrap(), sk.center(sk.curve_index(&c2).unwrap()).unwrap());
    assert!((p1.dist(p2) - 60.0).abs() < 1e-6, "distance: {}", p1.dist(p2));
}
