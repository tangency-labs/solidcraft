use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, CurveKind};

use crate::Session;

#[path = "sketch_pattern_tests.rs"]
mod pattern_tests;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    let id = s.active_sketch.unwrap();
    s.model.state().sketch(id).unwrap().sketch.clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn profiles(s: &Session) -> Vec<f64> {
    let id = s.active_sketch.unwrap();
    let mut a: Vec<f64> = s.model.state().sketch(id).unwrap().profiles.iter().map(|p| p.area).collect();
    a.sort_by(|x, y| x.total_cmp(y));
    a
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn explicit_freeform_break_preserves_exact_geometry_and_undo() {
    for (command, params) in [
        ("sketch.spline.control_point", json!({"points": [[0,0], [0,12], [20,12], [20,0]], "connect": false})),
        ("sketch.spline.control_point_5", json!({"points": [[0,0], [0,12], [5,20], [12,20], [20,12], [20,0]], "connect": false})),
        ("sketch.spline.fit_point", json!({"points": [[0,0], [6,12], [14,8], [20,0]], "connect": false})),
        ("sketch.conic", json!({"start": [0,0], "end": [20,0], "apex": [10,20], "rho": 0.65, "connect": false})),
    ] {
        let mut s = new_sketch();
        let id = ids(&run(&mut s, command, params)["curves"])[0].clone();
        let before = sketch(&s);
        let exact = before.exact_segs(before.curve_index(&id).unwrap()).unwrap();
        let a = exact.first().unwrap().point_at(0.25);
        let b = exact.last().unwrap().point_at(0.75);
        let out = run(&mut s, "sketch.break", json!({"curve": id, "points": [b, a, a]}));
        assert!(ids(&out["curves"]).len() >= 3, "{command}: {out}");
        let after = sketch(&s);
        let segs: Vec<_> = after.curves.iter().enumerate().flat_map(|(i, _)| after.exact_segs(i).unwrap_or_default()).collect();
        for old in &exact {
            for k in 0..=40 {
                let q = old.point_at(k as f64 / 40.0);
                let d = segs.iter().map(|seg| seg.point_at(seg.closest_param(q)).dist(q)).fold(f64::INFINITY, f64::min);
                assert!(d < 1e-6, "{command}: geometry moved {d} at {q:?}");
            }
        }
        let encoded = serde_json::to_value(&after).unwrap();
        let restored: solidcraft_sketch::Sketch = serde_json::from_value(encoded).unwrap();
        assert_eq!(restored.curves.len(), after.curves.len());
        run(&mut s, "edit.undo", json!({}));
        assert!(sketch(&s).curve_index(&id).is_some());
    }
}

#[test]
fn explicit_ellipse_break_keeps_closed_profile() {
    let mut s = new_sketch();
    let id = ids(&run(&mut s, "sketch.ellipse", json!({"center": [0,0], "major": [10,0], "minor": [0,4]}))["curves"])[0].clone();
    let area = profiles(&s)[0];
    let out = run(&mut s, "sketch.break", json!({"curve": id, "points": [[10,0], [0,4], [10,0]]}));
    assert_eq!(ids(&out["curves"]).len(), 4, "{out}");
    let after = sketch(&s);
    assert!(after.curves.iter().all(|c| matches!(c.kind, CurveKind::Conic { .. })));
    assert_eq!(profiles(&s).len(), 1);
    assert!((profiles(&s)[0] - area).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    let original = sketch(&s);
    let q = original.exact_segs(original.curve_index(&id).unwrap()).unwrap()[0].point_at(0.4);
    let out = run(&mut s, "sketch.break", json!({"curve": id, "points": [q]}));
    assert_eq!(ids(&out["curves"]).len(), 5, "a single cut inside a quadrant: {out}");
    assert_eq!(profiles(&s).len(), 1);
    assert!((profiles(&s)[0] - area).abs() < 1e-6);
}

#[test]
fn explicit_break_rejects_bad_points_atomically_and_merges_duplicates() {
    for freeform in [false, true] {
        let mut s = new_sketch();
        let out = if freeform {
            run(&mut s, "sketch.spline.control_point", json!({"points": [[0,0], [0,10], [20,10], [20,0]], "connect": false}))
        } else {
            run(&mut s, "sketch.line", json!({"points": [[0,0], [20,0]], "connect": false}))
        };
        let id = ids(&out["curves"])[0].clone();
        let before = serde_json::to_value(sketch(&s)).unwrap();
        for points in [json!([]), json!("wrong"), json!([[8, 4], ["bad", 0]]), json!([[0, 0]]), json!([[1e20, 0]]), json!(vec![[10, 5]; 1001])] {
            assert!(s.execute("sketch.break", &json!({"curve": id, "points": points})).is_err());
            assert_eq!(serde_json::to_value(sketch(&s)).unwrap(), before);
        }
        let out = run(&mut s, "sketch.break", json!({"curve": id, "points": [[10,7], [10,7]]}));
        assert_eq!(ids(&out["curves"]).len(), 2, "{out}");
    }
}

#[test]
fn analytic_trim_and_break_use_freeform_cutters() {
    let mut s = new_sketch();
    let line = ids(&run(&mut s, "sketch.line", json!({"points": [[-5,5], [25,5]], "connect": false}))["curves"])[0].clone();
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0,0], [0,10], [20,10], [20,0]], "connect": false}));
    let out = run(&mut s, "sketch.break", json!({"curve": line, "at": [10,5]}));
    assert_eq!(ids(&out["curves"]).len(), 3, "{out}");
    let sk = sketch(&s);
    for id in ids(&out["curves"]) {
        let shape = sk.shape(sk.curve_index(&id).unwrap()).unwrap();
        assert!(matches!(shape, solidcraft_sketch::Shape::Line { a, b } if (a.y - 5.0).abs() < 1e-6 && (b.y - 5.0).abs() < 1e-6));
    }

    let mut s = new_sketch();
    let circle = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0,0], "radius": 5}))["curves"])[0].clone();
    run(&mut s, "sketch.ellipse", json!({"center": [0,0], "major": [8,0], "minor": [0,3]}));
    run(&mut s, "sketch.trim", json!({"curve": circle, "at": [5,0]}));
    let sk = sketch(&s);
    let ci = sk.curve_index(&circle).unwrap();
    assert!(matches!(sk.curves[ci].kind, CurveKind::Arc { .. }));
    assert!(sk.shape(ci).unwrap().dist(Vec2::new(-5.0, 0.0)) < 1e-6);
}

#[test]
fn trim_extend_break() {
    let mut s = new_sketch();
    // A cross: horizontal line crossed by a vertical one.
    let h = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let v = ids(&run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 5]]}))["curves"])[0].clone();
    // Trim the right half of the horizontal line.
    run(&mut s, "sketch.trim", json!({"curve": h, "at": [15, 0]}));
    let sk = sketch(&s);
    let sh = sk.shape(sk.curve_index(&h).unwrap()).unwrap();
    assert!(
        matches!(sh, solidcraft_sketch::Shape::Line { a, b } if a.dist(Vec2::new(0.0, 0.0)) < 1e-9 && b.dist(Vec2::new(10.0, 0.0)) < 1e-9),
        "{sh:?}"
    );
    // Trim the lower half of the vertical line: the original id survives on the upper piece.
    run(&mut s, "sketch.trim", json!({"curve": v, "at": [10, -3]}));
    let sk = sketch(&s);
    assert_eq!(sk.curves.len(), 2);
    // Extend: a short line reaching toward the vertical one.
    let e = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 3], [4, 3]]}))["curves"])[0].clone();
    run(&mut s, "sketch.extend", json!({"curve": e, "at": [4, 3]}));
    let sk = sketch(&s);
    let end = sk.resolve_point(&format!("{e}.end")).and_then(|i| sk.point(i)).unwrap();
    assert!(end.dist(Vec2::new(10.0, 3.0)) < 1e-7, "{end:?}");
    // Break the horizontal line where... nothing crosses it inside: error.
    assert!(s.execute("sketch.break", &json!({"curve": h, "at": [5, 0]})).is_err());
    // Break the vertical at the extended line: two pieces.
    let r = run(&mut s, "sketch.break", json!({"curve": v, "at": [10, 1]}));
    assert_eq!(ids(&r["curves"]).len(), 2, "{r}");
    // A lone curve trims away entirely.
    let lone = ids(&run(&mut s, "sketch.line", json!({"points": [[50, 50], [60, 50]]}))["curves"])[0].clone();
    run(&mut s, "sketch.trim", json!({"curve": lone, "at": [55, 50]}));
    assert!(sketch(&s).curve_index(&lone).is_none());
}

#[test]
fn trim_circle_makes_an_arc_and_a_profile() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [[-20, 0], [20, 0]]}));
    run(&mut s, "sketch.trim", json!({"curve": c, "at": [0, -10]}));
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(matches!(sk.curves[ci].kind, CurveKind::Arc { .. }));
    // Upper half disc closes against the line.
    let a = profiles(&s);
    assert_eq!(a.len(), 1, "{a:?}");
    assert!(close(a[0], std::f64::consts::PI * 50.0), "{a:?}");
}

#[test]
fn exact_extend_ellipse_preserves_support_target_and_history() {
    for (y, expected_x) in [(3.0, 12.0), (5.0, 20.0)] {
        let mut s = new_sketch();
        let ellipse = ids(&run(&mut s, "sketch.ellipse", json!({"center":[20,0],"major":[30,0],"minor_radius":5}))["curves"])[0].clone();
        run(&mut s, "sketch.constraint.fix", json!({"entity":ellipse}));
        let line = ids(&run(&mut s, "sketch.line", json!({"points":[[0,y],[5,y]],"connect":false,"infer":false}))["curves"])[0].clone();
        let before = s.doc.to_json();
        let target = sketch(&s).exact_segs(0).unwrap();
        run(&mut s, "sketch.extend", json!({"curve":line,"at":[5,y]}));
        let sk = sketch(&s);
        let end = sk.point(sk.resolve_point(&format!("{line}.end")).unwrap()).unwrap();
        assert!(end.dist(Vec2::new(expected_x, y)) < 1e-7, "exact ellipse contact {end:?}");
        assert_eq!(sk.exact_segs(0).unwrap(), target);
        let persisted = s.doc.to_json();
        let reopened = solidcraft_doc::format::read(&persisted).unwrap();
        assert_eq!(reopened.to_json(), persisted);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), persisted);
    }
}

#[test]
fn exact_extend_arc_rejects_interior_hit_atomically() {
    let mut s = new_sketch();
    let arc = ids(&run(&mut s, "sketch.arc.center_point", json!({"center":[0,0],"start":[10,0],"end":[0,10],"connect":false}))["curves"])[0].clone();
    let x = 50.0_f64.sqrt();
    let cutter = ids(&run(&mut s, "sketch.line", json!({"points":[[x,5],[x,9]],"connect":false,"infer":false}))["curves"])[0].clone();
    run(&mut s, "sketch.constraint.fix", json!({"entity":cutter}));
    let before = s.doc.to_json();
    let history = s.undo.len();
    assert!(s.execute("sketch.extend", &json!({"curve":arc,"at":[10,0]})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), history);
}

#[test]
fn exact_extend_refuses_ambiguous_stop_without_history_change() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[5,0]],"connect":false,"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[[10,-5],[10,5]],"connect":false,"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[[8,-2],[12,2]],"connect":false,"infer":false}));
    let before = s.doc.to_json();
    let history = s.undo.len();
    let error = s.execute("sketch.extend", &json!({"curve":"l1","at":[5,0]})).unwrap_err();
    assert!(error.to_string().contains("multiple curves"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), history);
}

#[test]
fn fillet_and_chamfers() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // Pin the rectangle so a dimension change only moves the fillet.
    run(&mut s, "sketch.constraint.fix", json!({"entity": l[2]}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": l[3]}));
    let f = run(&mut s, "sketch.fillet", json!({"a": l[0], "b": l[1], "radius": 5}));
    assert!(f["param"].is_string());
    let a = profiles(&s);
    assert!(close(a[0], 1200.0 - (25.0 - std::f64::consts::PI * 25.0 / 4.0)), "{a:?}");
    // The fillet radius is a dimension: change it.
    let pname = f["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": pname, "expression": "8 mm"}));
    let a = profiles(&s);
    assert!(close(a[0], 1200.0 - (64.0 - std::f64::consts::PI * 64.0 / 4.0)), "{a:?}");
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": l[2], "b": l[3], "distance": 4}));
    let a2 = profiles(&s)[0];
    assert!(close(a[0] - a2, 8.0), "{a2}");
    run(&mut s, "sketch.chamfer.two_distance", json!({"a": l[1], "b": l[2], "distance": 2, "distance2": 6}));
    let a3 = profiles(&s)[0];
    assert!(close(a2 - a3, 6.0), "{a3}");
    // Distance-angle at the last corner (origin, 90 degrees): 45 deg gives an isosceles cut.
    run(&mut s, "sketch.chamfer.distance_angle", json!({"a": l[3], "b": l[0], "distance": 3, "angle": 45}));
    let a4 = profiles(&s)[0];
    assert!(close(a3 - a4, 4.5), "{a4}");
    assert!(s.execute("sketch.fillet", &json!({"a": l[0], "b": l[2], "radius": 1})).is_err(), "no shared corner");
}

#[test]
fn offset_chain_mirror_patterns_move_scale() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // Outward offset by 2 (toward a point outside).
    let o = run(&mut s, "sketch.offset", json!({"curves": [l[0]], "distance": 2, "side": [20, -10]}));
    assert_eq!(ids(&o["curves"]).len(), 4, "{o}");
    let a = profiles(&s);
    assert!(close(a[1], 44.0 * 34.0 - 1200.0) || close(a.iter().sum::<f64>(), 44.0 * 34.0), "{a:?}");
    // Circle offset inward.
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "sketch.offset", json!({"curves": [c], "distance": 3, "side": [100, 0]}));
    // Mirror a circle about a vertical line.
    let m = ids(&run(&mut s, "sketch.line", json!({"points": [[120, -50], [120, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.mirror", json!({"entities": [c], "line": m}));
    let mc = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    assert!(sk.center(sk.curve_index(&mc).unwrap()).unwrap().dist(Vec2::new(140.0, 0.0)) < 1e-7);
    // Circular pattern of the mirrored circle: 4 around (120, 0).
    let r = run(&mut s, "sketch.pattern.circular", json!({"entities": [mc], "center": [120, 0], "count": 4}));
    assert_eq!(ids(&r["curves"]).len(), 3);
    let sk = sketch(&s);
    let c2 = sk.center(sk.curve_index(&ids(&r["curves"])[0]).unwrap()).unwrap();
    assert!(c2.dist(Vec2::new(120.0, 20.0)) < 1e-7, "{c2:?}");
    // Rectangular pattern 3 x 2.
    let small = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 100], "radius": 1}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.pattern.rectangular", json!({"entities": [small], "count": 3, "spacing": 5, "count2": 2, "spacing2": 4}));
    assert_eq!(ids(&r["curves"]).len(), 5);
    // Move and scale.
    run(&mut s, "sketch.move", json!({"entities": [small], "translate": [0, 10]}));
    run(&mut s, "sketch.scale", json!({"entities": [small], "base": [0, 110], "factor": 3}));
    let sk = sketch(&s);
    let ci = sk.curve_index(&small).unwrap();
    assert!(sk.center(ci).unwrap().dist(Vec2::new(0.0, 110.0)) < 1e-7);
    assert!(close(sk.radius(ci).unwrap(), 3.0));
    let r = run(&mut s, "sketch.move", json!({"entities": [small], "angle": 90, "center": [0, 0], "copy": true}));
    let cp = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    assert!(sk.center(sk.curve_index(&cp).unwrap()).unwrap().dist(Vec2::new(-110.0, 0.0)) < 1e-7);
}

/// #1485: after a circle is trimmed between two construction lines, clicking the trimmed end
/// picks the construction line's end lying there, not the arc's own; the tangent arc still starts
/// from the trimmed arc, tangent to it.
#[test]
fn a_tangent_arc_starts_from_a_trimmed_end() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["guide_a"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 10]], "ids": ["guide_b"]}));
    run(&mut s, "sketch.construction", json!({"curves": ["guide_a", "guide_b"]}));
    run(&mut s, "sketch.trim", json!({"curve": c, "at": [7.07, 7.07]}));
    let sk = sketch(&s);
    let picked = sk.resolve_point("guide_a.end").unwrap();
    assert!(sk.point(picked).unwrap().dist(Vec2::new(10.0, 0.0)) < 1e-6);
    let trimmed = sk.curves.iter().position(|cu| !cu.construction && matches!(cu.kind, CurveKind::Arc { .. })).expect("the trimmed arc");
    let r = run(&mut s, "sketch.arc.tangent", json!({"start": "guide_a.end", "end": [14, 6]}));
    let sk = sketch(&s);
    let a = sk.curve_index(&ids(&r["curves"])[0]).unwrap();
    let a_ends: Vec<usize> = sk.curves[a].kind.point_ids();
    let t_ends: Vec<usize> = sk.curves[trimmed].kind.point_ids();
    assert!(a_ends.iter().any(|q| t_ends.contains(q) && *q != picked), "the new arc shares the trimmed arc's own end");
    assert!(
        sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Tangent { a: x, b: y } if (x, y) == (trimmed, a) || (x, y) == (a, trimmed))),
        "and is tangent to it"
    );
}

#[test]
fn three_point_arc_keeps_referenced_through_points_when_dimensions_change() {
    for (through, scale) in [(json!("height.end"), 1.0), (json!({"ref":"height.end"}), 1.0), (json!("mid:height"), 2.0)] {
        for reverse in [false, true] {
            let mut s = new_sketch();
            run(&mut s, "sketch.line", json!({"points":[[-20,0],[20,0]],"ids":["base"]}));
            run(&mut s, "sketch.constraint.fix", json!({"entity":"base"}));
            run(&mut s, "sketch.line", json!({"points":["origin",[0,10.0 * scale]],"ids":["height"],"infer":true}));
            let dim = run(&mut s, "sketch.dimension", json!({"entities":["height"],"value":10.0 * scale}));
            let before = s.doc.to_json();
            let (start, end) = if reverse { ("base.end", "base.start") } else { ("base.start", "base.end") };
            let out = run(&mut s, "sketch.arc.three_point", json!({"start":start,"end":end,"through":through,"id":"arc"}));
            let sk = sketch(&s);
            let arc = sk.curve_index("arc").unwrap();
            let constraint = sk.constraints.iter().find(|k| matches!(k.kind, ConstraintKind::PointOnCurve { c, .. } if c == arc)).unwrap();
            assert!(ids(&out["created"]["constraints"]).contains(&constraint.id), "{out}");
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc.to_json(), before, "the arc and its attachment are one undo step");
            run(&mut s, "edit.redo", json!({}));
            run(&mut s, "parameters.change", json!({"name":dim["param"],"expression":format!("{} mm", 15.0 * scale)}));

            let sid = s.active_sketch.unwrap();
            // Saving and reopening preserves the ordinary point-on-curve constraint too.
            let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
            for session in [&s, &reopened] {
                let state = session.model.state();
                let ss = state.sketch(sid).unwrap();
                let sk = &ss.sketch;
                let arc = sk.curve_index("arc").unwrap();
                assert!(ss.report.ok(), "{:?}", ss.report);
                assert!(close(sk.radius(arc).unwrap(), 125.0 / 6.0), "{:?}", sk.radius(arc));
                let shape = sk.shape(arc).unwrap();
                for point in [Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(0.0, 15.0)] {
                    assert!(shape.dist(point) < 1e-6, "{through}, reverse={reverse}: {point:?}");
                }
            }
        }
    }
}

#[test]
fn three_point_arc_coordinates_stay_free_and_invalid_references_are_atomic() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":["origin",[0,10]],"ids":["height"]}));
    let out = run(&mut s, "sketch.arc.three_point", json!({"start":[-20,0],"end":[20,0],"through":[0,10],"id":"arc","connect":false}));
    assert!(ids(&out["created"]["constraints"]).is_empty(), "coordinates must not fix the arc or attach it to a nearby point");
    let before = s.doc.to_json();
    for through in [json!("missing.end"), json!({"ref":"missing.end"}), json!([null, 10]), json!("origin"), json!("mid:height")] {
        // The midpoint snap creates a point before the collinear arc is refused.
        assert!(s.execute("sketch.arc.three_point", &json!({"start":[0,-10],"end":[0,20],"through":through})).is_err(), "{through}");
        assert_eq!(s.doc.to_json(), before, "failed through point {through} changed the sketch");
    }
}

#[test]
fn new_creation_tools() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.line.midpoint", json!({"mid": [0, 0], "end": [10, 5]}));
    let l = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let start = sk.resolve_point(&format!("{l}.start")).and_then(|i| sk.point(i)).unwrap();
    assert!(start.dist(Vec2::new(-10.0, -5.0)) < 1e-9);
    // Tangent arc off the end of a horizontal line.
    let h = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 20]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.arc.tangent", json!({"start": format!("{h}.end"), "end": [20, 30]}));
    let a = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ai = sk.curve_index(&a).unwrap();
    assert!(sk.center(ai).unwrap().dist(Vec2::new(10.0, 30.0)) < 1e-7, "{:?}", sk.center(ai));
    assert!(close(sk.radius(ai).unwrap(), 10.0));
    // 2-tangent circle in the corner of two lines.
    let x = ids(&run(&mut s, "sketch.line", json!({"points": [[100, 0], [150, 0]]}))["curves"])[0].clone();
    let y = ids(&run(&mut s, "sketch.line", json!({"points": [[100, 0], [100, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.circle.two_tangent", json!({"curves": [x, y], "radius": 5, "near": [106, 104 - 100]}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(sk.center(ci).unwrap().dist(Vec2::new(105.0, 5.0)) < 1e-6, "{:?}", sk.center(ci));
    // 3-tangent circle inside a triangle: the incircle of the 3-4-5 triangle (r = 1).
    let t = run(&mut s, "sketch.line", json!({"points": [[200, 0], [203, 0], [200, 4]], "closed": true}));
    let tl = ids(&t["curves"]);
    let r = run(&mut s, "sketch.circle.three_tangent", json!({"curves": tl, "near": [201, 1.2]}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(close(sk.radius(ci).unwrap(), 1.0), "{:?}", sk.radius(ci));
    assert!(sk.center(ci).unwrap().dist(Vec2::new(201.0, 1.0)) < 1e-6);
    // Slots.
    let before = profiles(&s).len();
    run(&mut s, "sketch.slot.center_point", json!({"center": [0, -100], "end": [10, -100], "width": 4}));
    run(&mut s, "sketch.slot.arc_three_point", json!({"start": [100, -100], "through": [110, -90], "end": [120, -100], "width": 2}));
    run(&mut s, "sketch.slot.arc_center", json!({"center": [200, -100], "start": [210, -100], "end": [200, -90], "width": 2}));
    let a = profiles(&s);
    assert_eq!(a.len(), before + 3, "{a:?}");
    let pi = std::f64::consts::PI;
    assert!(a.iter().any(|x| close(*x, 20.0 * 4.0 + pi * 4.0)), "{a:?}");
    // Half-circle arc slot: centre arc length pi*10, width 2, plus a full cap circle.
    assert!(a.iter().any(|x| close(*x, pi * 10.0 * 2.0 + pi)), "{a:?}");
    assert!(a.iter().any(|x| close(*x, pi * 5.0 * 2.0 + pi)), "{a:?}");
}

/// #3141: a 2-tangent circle asked for near a point on one of its curves (where a click on that
/// curve puts it) is solved from the touching circle nearest that point: it lands in the corner,
/// the rectangle stays as drawn, and fixed sides no longer make it fail.
#[test]
fn two_tangent_circle_from_a_click_on_a_curve_leaves_the_curves_as_drawn() {
    let side = |s: &Session, sides: &[String], on: fn(Vec2) -> bool| -> String {
        let sk = sketch(s);
        sides
            .iter()
            .find(|c| {
                let i = sk.curve_index(c).unwrap();
                matches!(sk.shape(i), Some(solidcraft_sketch::Shape::Line { a, b }) if on(a) && on(b))
            })
            .unwrap()
            .clone()
    };
    let points = |s: &Session| sketch(s).points.iter().map(|p| p.pos).collect::<Vec<_>>();
    for fixed in [false, true] {
        let mut s = new_sketch();
        let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-800, 0], "p1": [-300, 500]}));
        let sides = ids(&r["curves"]);
        if fixed {
            run(&mut s, "sketch.constraint.fix", json!({"entities": sides}));
        }
        let before = points(&s);
        let bottom = side(&s, &sides, |p| p.y.abs() < 1e-9);
        let left = side(&s, &sides, |p| (p.x + 800.0).abs() < 1e-9);
        // Midway between a click on the bottom side and one on the left side.
        let r = run(&mut s, "sketch.circle.two_tangent", json!({"curves": [bottom, left], "radius": 50, "near": [-675, 125]}));
        let c = ids(&r["curves"])[0].clone();
        let sk = sketch(&s);
        let ci = sk.curve_index(&c).unwrap();
        assert!(sk.center(ci).unwrap().dist(Vec2::new(-750.0, 50.0)) < 1e-6, "fixed {fixed}: {:?}", sk.center(ci));
        assert!(close(sk.radius(ci).unwrap(), 50.0));
        let after = points(&s);
        assert!(before.iter().zip(&after).all(|(a, b)| a.dist(*b) < 1e-9), "fixed {fixed}: the rectangle did not move: {after:?}");
        // Asked for right on the left side: still a touching circle next to it.
        let r = run(&mut s, "sketch.circle.two_tangent", json!({"curves": [bottom, left], "radius": 20, "near": [-800, 30]}));
        let c = ids(&r["curves"])[0].clone();
        let sk = sketch(&s);
        let o = sk.center(sk.curve_index(&c).unwrap()).unwrap();
        assert!(close(o.y, 20.0) && close((o.x + 800.0).abs(), 20.0), "fixed {fixed}: {o:?}");
    }
}

/// #3099: a centre circle drawn through a point stays on it. Centred on a line's midpoint and
/// through its end, the circle's radius follows the line when the line is dimensioned.
#[test]
fn centre_circle_through_a_point_keeps_it_on_the_circle() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 30], [40, 30]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.circle.center", json!({"center": format!("mid:{l}"), "radius": 20, "through": format!("{l}.end")}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let (li, ci) = (sk.curve_index(&l).unwrap(), sk.curve_index(&c).unwrap());
    let end = sk.resolve_point(&format!("{l}.end")).unwrap();
    assert!(sk.constraints.iter().any(|k| k.kind == ConstraintKind::PointOnCurve { p: end, c: ci }), "{:?}", sk.constraints);
    run(&mut s, "sketch.dimension", json!({"entities": [l], "value": 60}));
    let sk = sketch(&s);
    let line = sk.shape(li).unwrap();
    let solidcraft_sketch::Shape::Line { a, b } = line else { panic!("{line:?}") };
    assert!(close(a.dist(b), 60.0));
    assert!(close(sk.radius(ci).unwrap(), 30.0), "the circle followed the line's end: {:?}", sk.radius(ci));
    // Through a plain position: nothing to keep it on.
    let r = run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "through": [105, 0]}));
    let ci = sketch(&s).curve_index(&ids(&r["curves"])[0]).unwrap();
    assert!(!sketch(&s).constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { c, .. } if c == ci)));
}

#[test]
fn ellipse_splines_conic() {
    let pi = std::f64::consts::PI;
    let mut s = new_sketch();
    let e = run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor": [0, 4]}));
    let eid = ids(&e["curves"])[0].clone();
    let a = profiles(&s);
    assert!((a[0] - pi * 40.0).abs() / (pi * 40.0) < 2e-3, "{a:?}");
    // A point constrained onto the ellipse lands on it.
    run(&mut s, "sketch.point", json!({"point": [3, 5], "id": "q"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "q", "b": eid}));
    let sk = sketch(&s);
    let q = sk.point(sk.point_index("q").unwrap()).unwrap();
    let exact = sk.exact_segs(sk.curve_index(&eid).unwrap()).unwrap();
    let distance = exact.iter().map(|seg| q.dist(seg.point_at(seg.closest_param(q)))).fold(f64::INFINITY, f64::min);
    assert!(distance < 1e-6, "on the exact ellipse: {q:?}");
    // Fit spline closed by a line: one profile.
    let sp = run(&mut s, "sketch.spline.fit_point", json!({"points": [[100, 0], [110, 8], [120, 3], [130, 0]]}));
    let sid = ids(&sp["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sid}.end"), format!("{sid}.start")]}));
    assert_eq!(profiles(&s).len(), 2);
    // Moving a fit point reshapes the spline (it is a sketch point).
    let sk = sketch(&s);
    let si = sk.curve_index(&sid).unwrap();
    let pts = sk.curves[si].kind.point_ids();
    let pid = sk.points[pts[1]].id.clone();
    run(&mut s, "sketch.move_point", json!({"point": pid, "to": [110, 12]}));
    let sk = sketch(&s);
    assert!(sk.polyline(si).iter().any(|q| q.dist(Vec2::new(110.0, 12.0)) < 1e-9));
    // Control point splines (degree 3 and 5) and a conic.
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0, 50], [0, 60], [10, 60], [10, 50]]}));
    run(&mut s, "sketch.spline.control_point_5", json!({"points": [[20, 50], [20, 60], [25, 65], [30, 60], [35, 62], [40, 50]]}));
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let k = run(&mut s, "sketch.conic", json!({"start": [210, 0], "end": [200, 10], "apex": [210, 10], "rho": w / (1.0 + w)}));
    let kid = ids(&k["curves"])[0].clone();
    // A line tangent to the conic at its end: the tangent arc tool continues it.
    let r = run(&mut s, "sketch.arc.tangent", json!({"start": format!("{kid}.end"), "end": [190, 0]}));
    let aid = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    // The conic is a quarter circle about (200, 0) leaving (200, 10) heading -x: the tangent
    // arc from there to (190, 0) continues the same circle.
    let ai = sk.curve_index(&aid).unwrap();
    assert!(sk.center(ai).unwrap().dist(Vec2::new(200.0, 0.0)) < 1e-6, "{:?}", sk.center(ai));
    assert!(s.execute("sketch.spline.fit_point", &json!({"points": [[0, 0]]})).is_err());
    assert!(s.execute("sketch.conic", &json!({"start": [0, 0], "end": [1, 0], "apex": [0, 1], "rho": 1.5})).is_err());
    // Mirror and pattern copy free-form curves too.
    let m = ids(&run(&mut s, "sketch.line", json!({"points": [[-50, -50], [-50, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.mirror", json!({"entities": [eid, sid], "line": m}));
    assert_eq!(ids(&r["curves"]).len(), 2);
}

#[test]
fn text_makes_profiles_that_extrude_and_edit() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.text", json!({"text": "SO", "at": [0, 0], "height": 10}));
    let link = r["link"].as_str().unwrap().to_string();
    let a = profiles(&s);
    // S (one region) + O (ring = outer with a hole, plus the counter as its own region).
    assert!(a.len() >= 2, "{a:?}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 0);
    // Edit the text: the outlines change, the link stays.
    let r2 = run(&mut s, "sketch.edit_text", json!({"link": link, "text": "I", "height": 20}));
    assert_eq!(r2["link"], json!(link));
    let a = profiles(&s);
    assert_eq!(a.len(), 1, "{a:?}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2}));
    let m = run(&mut s, "inspect.measure", json!({}));
    let v = m["total"]["volume_mm3"].as_f64().unwrap();
    // An "I" 20 mm tall is a bar a couple of mm wide.
    assert!(v > 40.0 && v < 200.0, "{v}");
    assert!(s.execute("sketch.text", &json!({"text": "", "at": [0, 0]})).is_err());
}

/// Volumes (mm³) of the bodies in the design, smallest first.
fn body_volumes(s: &mut Session) -> Vec<f64> {
    let m = run(s, "inspect.measure", json!({}));
    let mut v: Vec<f64> = m["bodies"].as_array().unwrap().iter().map(|b| b["volume_mm3"].as_f64().unwrap()).collect();
    v.sort_by(f64::total_cmp);
    v
}

#[test]
fn text_extrudes_with_its_counters_and_beside_other_curves() {
    // Letter areas in mm² at a capital height of 10 mm, summed from the Liberation Sans contours (shoelace
    // formula, quadratic pieces flattened finely), each outer contour minus its counters:
    //   A 39.2159 - 9.0647   B 68.9283 - 12.9336 - 16.6277   8 56.8394 - 10.6451 - 13.7347   I 13.5557
    // Extruded 2 mm, so the volumes are twice that. Faceted walls: within 0.02 mm³.
    let near = |got: &[f64], want: &[f64]| {
        assert_eq!(got.len(), want.len(), "{got:?} against {want:?}");
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.02), "{got:?} against {want:?}");
    };
    // Letters with counters, alone: a letter is one body with its counters as holes. (A counter used to be
    // extruded as a body of its own, or the extrude failed with "profile has no area".)
    for (text, want) in
        [("AB", vec![2.0 * (39.2159 - 9.0647), 2.0 * (68.9283 - 12.9336 - 16.6277)]), ("8", vec![2.0 * (56.8394 - 10.6451 - 13.7347)])]
    {
        let mut s = new_sketch();
        run(&mut s, "sketch.text", json!({"text": text, "at": [0, 0], "height": 10}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 2}));
        near(&body_volumes(&mut s), &want);
    }
    // Text beside a drawn rectangle is extruded with it (it used to be left out): the 30 x 20 x 2 plate and an "I".
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
    run(&mut s, "sketch.text", json!({"text": "I", "at": [40, 0], "height": 10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2}));
    near(&body_volumes(&mut s), &[2.0 * 13.5557, 1200.0]);
    // A counter can still be picked on its own, by a point inside it: the inner contour of an O, 47.4584 mm².
    let mut s = new_sketch();
    run(&mut s, "sketch.text", json!({"text": "O", "at": [0, 0], "height": 10}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "profiles": [{"point": [5.65, 5.0]}]}));
    near(&body_volumes(&mut s), &[2.0 * 47.4584]);
}

#[test]
fn text_embossed_or_debossed_leaves_its_counters_alone() {
    // A 40 x 30 x 5 plate (6000 mm³) with 10 mm tall text raised from, or cut 1 mm into, its top. Only the letters
    // change the volume, by their area in mm² (see above) x 1 mm; the counters of the O and of A and B stay as
    // they are. (Emboss and deboss used to fail with "boolean Cut: no result" or "boolean Union: no result",
    // because each counter was a tool body of its own, filling the hole of its letter.)
    let (o, ab) = (83.3133 - 47.4584, (39.2159 - 9.0647) + (68.9283 - 12.9336 - 16.6277));
    for (text, mode, change) in [("O", "deboss", -o), ("O", "emboss", o), ("AB", "deboss", -ab)] {
        let mut s = new_sketch();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "body_name": "Plate"}));
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Lid"}));
        run(&mut s, "sketch.create", json!({"plane": "Lid", "name": "Text"}));
        run(&mut s, "sketch.text", json!({"text": text, "at": [10, 10], "height": 10}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.emboss", json!({"sketch": "Text", "depth": 1, "mode": mode}));
        let v = body_volumes(&mut s);
        assert!(v.len() == 1 && (v[0] - (6000.0 + change)).abs() < 0.02, "{text} {mode}: {v:?} against {}", 6000.0 + change);
    }
}

#[test]
fn text_takes_a_font_and_edit_text_keeps_or_changes_it() {
    // The repository's second font is BIZ UDPGothic. Its "I" is a polygon of two 606 x 145 bars and a 188 x 1369
    // stem in font units, with a capital height of 1567 units, so at 10 mm its area is
    // (2 x 606 x 145 + 188 x 1369) x (10 / 1567)^2 = 17.6385 mm². The built-in Liberation Sans "I" is 13.5557 mm².
    let font = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf");
    let near = |s: &Session, want: f64| {
        let a = profiles(s);
        assert!(a.len() == 1 && (a[0] - want).abs() < 1e-3, "{a:?} against {want}");
    };
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.text", json!({"text": "I", "at": [0, 0], "height": 10}));
    near(&s, 13.5557);
    let link = r["link"].as_str().unwrap().to_string();
    // A font by file path changes the outlines; the same edit with the built-in family's name changes them back.
    run(&mut s, "sketch.edit_text", json!({"link": link, "font": font}));
    near(&s, 17.6385);
    run(&mut s, "sketch.edit_text", json!({"link": link, "font": "Liberation Sans"}));
    near(&s, 13.5557);
    run(&mut s, "sketch.edit_text", json!({"link": link, "font": font}));
    // An edit that does not name a font keeps it (the height scales the area by 4), and an empty name is the built-in font.
    run(&mut s, "sketch.edit_text", json!({"link": link, "height": 20}));
    near(&s, 4.0 * 17.6385);
    run(&mut s, "sketch.edit_text", json!({"link": link, "font": ""}));
    near(&s, 4.0 * 13.5557);
    run(&mut s, "sketch.edit_text", json!({"link": link, "font": font}));
    // The font is kept in the design: after saving and reopening, the text is still in it.
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let source = reopened.doc.sketch(s.active_sketch.unwrap()).unwrap().link(&link).unwrap().source.clone();
    assert!(matches!(&source, solidcraft_sketch::LinkSource::Text { font: Some(f), .. } if f == font), "{source:?}");
    // A new text takes the font too. Unknown fonts and values that are not names are errors.
    let mut s = new_sketch();
    run(&mut s, "sketch.text", json!({"text": "I", "at": [0, 0], "height": 10, "font": font}));
    near(&s, 17.6385);
    let e = s.execute("sketch.text", &json!({"text": "I", "at": [0, 0], "font": "No Such Family 12345"})).unwrap_err().to_string();
    assert!(e.contains("unknown font `No Such Family 12345`"), "{e}");
    assert!(s.execute("sketch.text", &json!({"text": "I", "at": [0, 0], "font": 12})).is_err());
    assert!(s.execute("sketch.text", &json!({"text": "I", "at": [0, 0], "font": "C:/no/such/dir/font.ttf"})).is_err());
    near(&s, 17.6385);
}

#[test]
fn blend_curve_joins_tangentially() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[20, 10], [20, 20]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.blend_curve", json!({"a": format!("{a}.end"), "b": format!("{b}.start")}));
    assert_eq!(r["sketch"]["solved"], true, "{r}");
    let sp = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let pts: Vec<Vec2> = sk.curves[sk.curve_index(&sp).unwrap()].kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect();
    // Leaves along +x, arrives going +y (control polygon end legs).
    let d0 = (pts[1] - pts[0]).normalized().unwrap();
    let d1 = (pts[3] - pts[2]).normalized().unwrap();
    assert!(d0.y.abs() < 1e-6 && d0.x > 0.0, "{d0:?}");
    assert!(d1.x.abs() < 1e-6 && d1.y > 0.0, "{d1:?}");
}

#[test]
fn centerline_is_left_out_of_profiles() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    let l = ids(&r["curves"]);
    let m = ids(&run(&mut s, "sketch.line", json!({"points": [[5, -5], [5, 15]]}))["curves"])[0].clone();
    assert_eq!(profiles(&s).len(), 2);
    run(&mut s, "sketch.centerline", json!({"curves": [m]}));
    assert_eq!(profiles(&s).len(), 1);
    assert!(s.execute("sketch.centerline", &json!({"curves": ["nope"]})).is_err());
    let _ = l;
}

#[test]
fn free_form_profiles_extrude_exactly() {
    let pi = std::f64::consts::PI;
    let vol = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    // Ellipse a = 10, b = 4: π a b h.
    let mut s = new_sketch();
    run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}));
    let a = profiles(&s)[0];
    assert!((a - pi * 40.0).abs() < 1e-9, "{a}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    let v = vol(&mut s);
    assert!((v - pi * 40.0 * 5.0).abs() / (pi * 200.0) < 2e-4, "{v}");
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["bodies"][0]["faces"], 3, "an exact ellipse: top, bottom and one side ({m})");
    // A region closed by a fit spline and a line.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 8], [20, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sp}.end"), format!("{sp}.start")]}));
    let a = profiles(&s)[0];
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2}));
    let v = vol(&mut s);
    assert!((v - a * 2.0).abs() / (a * 2.0) < 2e-4, "{v} vs {}", a * 2.0);
    let m = run(&mut s, "inspect.measure", json!({}));
    assert!(m["bodies"][0]["faces"].as_u64().unwrap() <= 5, "{m}");
}

#[test]
fn degree_five_and_cut_free_form_profiles_extrude_exactly() {
    let vol = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    // A degree-5 control spline closed by a line: one B-spline side face.
    let mut s = new_sketch();
    let sp =
        ids(&run(&mut s, "sketch.spline.control_point_5", json!({"points": [[0, 0], [4, 10], [10, 12], [16, 12], [22, 10], [26, 0]]}))["curves"])[0]
            .clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sp}.end"), format!("{sp}.start")]}));
    let a = profiles(&s)[0];
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 3}));
    let v = vol(&mut s);
    assert!((v - a * 3.0).abs() / (a * 3.0) < 2e-4, "{v} vs {}", a * 3.0);
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["bodies"][0]["faces"], 4, "{m}");
    // A fit spline arch cut by a line: both sides extrude with exact curved faces.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [6, 9], [14, 9], [20, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sp}.end"), format!("{sp}.start")]}));
    run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 20]]}));
    let areas = profiles(&s);
    assert_eq!(areas.len(), 2, "{areas:?}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "profiles": [0]}));
    let v = vol(&mut s);
    assert!((v - areas[0] * 2.0).abs() / (areas[0] * 2.0) < 2e-4, "{v} vs {}", areas[0] * 2.0);
    let m = run(&mut s, "inspect.measure", json!({}));
    assert!(m["bodies"][0]["faces"].as_u64().unwrap() <= 7, "no polyline facets: {m}");
}

#[test]
fn trim_and_break_free_form_curves_exactly() {
    let pi = std::f64::consts::PI;
    let mut s = new_sketch();
    // An ellipse cut by a line through its centre: trimming the lower half leaves the upper
    // half, which closes with the line into half the ellipse.
    let e = ids(&run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}))["curves"])[0].clone();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-15, 0], [15, 0]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.trim", json!({"curve": e, "at": [0, -4]}));
    assert!(!r["result"]["pieces"].as_array().unwrap().is_empty(), "{r}");
    let a = profiles(&s);
    assert_eq!(a.len(), 1, "{a:?}");
    assert!((a[0] - pi * 20.0).abs() < 1e-6, "{a:?}");
    let _ = l;
    // A fit spline broken where a line crosses it: two pieces that still trace the curve.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 10], [20, 0], [30, 5]]}))["curves"])[0].clone();
    let before = sketch(&s).polyline(sketch(&s).curve_index(&sp).unwrap());
    run(&mut s, "sketch.line", json!({"points": [[15, -10], [15, 20]]}));
    let r = run(&mut s, "sketch.break", json!({"curve": sp, "at": [15, 5]}));
    let pieces = ids(&r["curves"]);
    assert_eq!(pieces.len(), 4, "three Béziers, the middle one split: {r}");
    let sk = sketch(&s);
    // Every original sample lies on one of the pieces.
    let polys: Vec<Vec<Vec2>> = pieces.iter().map(|id| sk.polyline(sk.curve_index(id).unwrap())).collect();
    for q in before.iter().step_by(5) {
        let d = polys
            .iter()
            .flat_map(|p| {
                p.windows(2).map(|w| {
                    let (a, b) = (w[0], w[1]);
                    let t = ((*q - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0);
                    q.dist(a + (b - a) * t)
                })
            })
            .fold(f64::INFINITY, f64::min);
        assert!(d < 0.05, "{q:?} {d}");
    }
}

#[test]
fn ellipse_radii_dimensions() {
    let mut s = new_sketch();
    let e = ids(&run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [e], "type": "major", "value": 12}));
    run(&mut s, "sketch.dimension", json!({"entities": [e], "value": 5}));
    let a = profiles(&s)[0];
    assert!((a - std::f64::consts::PI * 60.0).abs() < 1e-6, "{a}");
}

/// #1026: a construction diagonal ending at a rectangle's corner is not one of the corner's
/// sides. The corner is rounded, and the diagonal keeps its end on the virtual sharp corner.
#[test]
fn fillet_a_corner_a_construction_diagonal_ends_in() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [50, 0], [50, 30], [0, 30]], "closed": true, "ids": ["a", "b", "c", "d"]}));
    let diag = ids(&run(&mut s, "sketch.line", json!({"points": ["c.end", "a.end"], "construction": true}))["curves"])[0].clone();
    let f = run(&mut s, "sketch.fillet", json!({"point": "a.end", "radius": 2}));
    let arc = ids(&f["curves"])[0].clone();
    let sk = sketch(&s);
    let ai = sk.curve_index(&arc).unwrap();
    assert!((sk.radius(ai).unwrap() - 2.0).abs() < 1e-7);
    let c = sk.center(ai).unwrap();
    assert!(c.dist(Vec2::new(48.0, 2.0)) < 1e-6, "{c:?}");
    // The diagonal still runs from (0, 30) to the sharp corner (50, 0).
    let di = sk.curve_index(&diag).unwrap();
    let Some(CurveKind::Line { a, b }) = sk.curves.get(di).map(|x| &x.kind) else { panic!("the diagonal is a line") };
    let (pa, pb) = (sk.point(*a).unwrap(), sk.point(*b).unwrap());
    assert!(pa.dist(Vec2::new(0.0, 30.0)) < 1e-6 && pb.dist(Vec2::new(50.0, 0.0)) < 1e-6, "{pa:?} {pb:?}");
    assert!(sk.curves.get(di).unwrap().construction);
    // Three lines at a corner, none of them construction, stay ambiguous.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [50, 0], [50, 30]], "ids": ["a", "b"]}));
    run(&mut s, "sketch.line", json!({"points": ["a.end", [80, 20]]}));
    let e = s.execute("sketch.fillet", &json!({"point": "a.end", "radius": 2})).unwrap_err().to_string();
    assert!(e.contains("exactly two lines") && e.contains("`a` and `b`"), "{e}");
}

#[test]
fn fillet_a_line_and_an_arc() {
    let mut s = new_sketch();
    // A line into a quarter arc meeting at a sharp corner.
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let a =
        ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [30, 0], "start": format!("{l}.end"), "end": [30, -10]}))["curves"])[0].clone();
    let _ = a;
    let f = run(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 2}));
    let arc = ids(&f["curves"])[0].clone();
    let sk = sketch(&s);
    let ai = sk.curve_index(&arc).unwrap();
    assert!((sk.radius(ai).unwrap() - 2.0).abs() < 1e-7);
    // The fillet touches the line (its centre 2 below it) and the arc's circle (outside it: 10 + 2).
    let c = sk.center(ai).unwrap();
    assert!((c.y - 2.0).abs() < 1e-6 || (c.y + 2.0).abs() < 1e-6, "{c:?}");
    let d = c.dist(Vec2::new(30.0, 0.0));
    assert!((d - 12.0).abs() < 1e-6, "{d} {c:?}");
}

#[test]
fn fillet_a_line_and_an_arc_drawn_to_meet_at_a_point() {
    // #1208: the arc starts at the line's end by coordinates, so the corner is two points held
    // together by a Coincident constraint. By point and by naming the two curves.
    for how in [json!({"point": "l1.end"}), json!({"a": "l1", "b": "a1"})] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "ids": ["l1"]}));
        run(&mut s, "sketch.arc.center_point", json!({"center": [50, 0], "start": [60, 0], "end": [40, 0], "id": "a1"}));
        let mut p = how.clone();
        p["radius"] = json!(3);
        let f = run(&mut s, "sketch.fillet", p);
        let sk = sketch(&s);
        let ai = sk.curve_index(&ids(&f["curves"])[0]).unwrap();
        assert!((sk.radius(ai).unwrap() - 3.0).abs() < 1e-7, "{how}");
        // Tangent to the line (centre 3 above it) and outside the R10 arc (13 from its centre).
        let c = sk.center(ai).unwrap();
        let want = Vec2::new(50.0 - 160f64.sqrt(), 3.0);
        assert!(c.dist(want) < 1e-6, "{how}: {c:?} vs {want:?}");
        // The line now ends where the fillet touches it, and the arc where the fillet touches it.
        let l1 = sk.curve_index("l1").unwrap();
        let CurveKind::Line { b, .. } = sk.curves[l1].kind else { panic!("l1 is a line") };
        assert!(sk.point(b).unwrap().dist(Vec2::new(want.x, 0.0)) < 1e-6, "{how}");
        let a1 = sk.curve_index("a1").unwrap();
        let on_arc = Vec2::new(50.0, 0.0) + (want - Vec2::new(50.0, 0.0)) * (10.0 / 13.0);
        assert!(sk.curves[a1].kind.point_ids().iter().any(|i| sk.point(*i).unwrap().dist(on_arc) < 1e-6), "{how}");
        // No coincident pair is left at the old corner (only the line's start on the origin).
        let coincident = sk.constraints.iter().filter(|k| matches!(k.kind, ConstraintKind::Coincident { .. })).count();
        assert_eq!(coincident, 1, "{how}");
    }
}

#[test]
fn chamfer_two_lines_drawn_to_meet_at_a_point() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.line", json!({"points": [[40, 0], [40, 30]], "ids": ["l2"]}));
    let c = run(&mut s, "sketch.chamfer.equal_distance", json!({"point": "l1.end", "distance": 5}));
    let sk = sketch(&s);
    let ci = sk.curve_index(&ids(&c["curves"])[0]).unwrap();
    let CurveKind::Line { a, b } = sk.curves[ci].kind else { panic!("the chamfer is a line") };
    let mut ends = [sk.point(a).unwrap(), sk.point(b).unwrap()];
    ends.sort_by(|p, q| p.x.total_cmp(&q.x));
    assert!(ends[0].dist(Vec2::new(35.0, 0.0)) < 1e-6 && ends[1].dist(Vec2::new(40.0, 5.0)) < 1e-6, "{ends:?}");
}

#[test]
fn offset_free_form_curves() {
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 8], [20, 0], [30, 6]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.offset", json!({"curves": [sp], "distance": 2, "side": [10, 20]}));
    let made = ids(&r["curves"]);
    assert_eq!(made.len(), 1, "{r}");
    let sk = sketch(&s);
    let orig = sk.polyline(sk.curve_index(&sp).unwrap());
    let off = sk.polyline(sk.curve_index(&made[0]).unwrap());
    let dist = |q: Vec2| {
        orig.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let t = ((q - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0);
                q.dist(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    for q in off.iter().step_by(4) {
        assert!((dist(*q) - 2.0).abs() < 0.08, "{q:?} {}", dist(*q));
    }
    // An ellipse grows by the offset all round.
    let e = ids(&run(&mut s, "sketch.ellipse", json!({"center": [100, 0], "major": [110, 0], "minor_radius": 5}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.offset", json!({"curves": [e], "distance": 1, "side": [100, 20]}));
    assert_eq!(ids(&r["curves"]).len(), 1, "{r}");
}

#[test]
fn irregular_closed_spline_offset_certificate_edits_undoes_and_reopens() {
    // Public-safe reproduction of #971: the fit uses chord-length parameters,
    // while its exact offset's speed varies across eight unequal source spans.
    let mut s = new_sketch();
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({
            "points":[[-17,15],[0,8],[17,13],[15,0],[5,-7],[-1,-14],[-9,-1],[-21,-1]],
            "periodic":true
        }),
    );
    let original = sketch(&s);
    let before = s.doc.to_json();
    let result = run(&mut s, "sketch.offset", json!({"curves":["s1"],"distance":2.0,"chain":true}));
    let output = result["curves"][0].as_str().unwrap().to_string();
    let parameter = result["param"].as_str().unwrap().to_string();
    let point_ids = sketch(&s).curves[1].kind.point_ids();
    let verify = |s: &Session, distance: f64| {
        let state = s.world_state();
        let sk = &state.sketch(1).unwrap().sketch;
        assert_eq!(sk.curves[0], original.curves[0]);
        assert_eq!(&sk.points[..original.points.len()], &original.points);
        let index = sk.curve_index(&output).unwrap();
        assert_eq!(sk.curves[index].kind.point_ids(), point_ids);
        assert!(matches!(sk.curves[index].kind, CurveKind::Spline { periodic: true, .. }));
        let exact = sk.exact_segs(0).unwrap();
        let fitted = sk.exact_segs(index).unwrap();
        let mut max_error = 0.0_f64;
        for curve in &fitted {
            for j in 0..=8 {
                let p = curve.point_at(j as f64 / 8.0);
                let normal_distance = exact.iter().map(|s| s.point_at(s.closest_param(p)).dist(p)).fold(f64::INFINITY, f64::min);
                max_error = max_error.max((normal_distance - distance).abs());
            }
        }
        assert!(max_error < solidcraft_sketch::OFFSET_TOLERANCE, "normal-distance error {max_error}");
        eprintln!("#971 offset {distance} mm: maximum sampled normal-distance error {max_error} mm");
        assert_eq!(sk.freeform_offsets.len(), 1);
        assert_eq!(sk.freeform_offsets[0].divisions, 32);
        sk.validate_freeform_offsets().unwrap();
        sk.validate_periodic_splines().unwrap();
        assert_eq!(state.sketch(1).unwrap().profiles.len(), 2);
    };
    verify(&s, 2.0);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    verify(&s, 2.0);
    run(&mut s, "parameters.change", json!({"name":parameter,"expression":"3 mm"}));
    verify(&s, 3.0);
    run(&mut s, "edit.undo", json!({}));
    verify(&s, 2.0);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    verify(&reopened, 2.0);
    run(&mut reopened, "parameters.change", json!({"name":parameter,"expression":"3 mm"}));
    verify(&reopened, 3.0);
}

#[test]
fn certified_offset_follows_dimension_drag_reopen_break_and_undo() {
    let mut s = new_sketch();
    let ellipse = ids(&run(&mut s, "sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5,"connect":false}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities":[ellipse],"type":"major","value":10}));
    let before = s.doc.to_json();
    let out = run(&mut s, "sketch.offset", json!({"curves":[ellipse],"distance":-1,"chain":false}));
    let output = out["curves"][0].as_str().unwrap().to_string();
    assert_eq!(sketch(&s).freeform_offsets.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    let result = run(&mut s, "parameters.change", json!({"name":"d1","expression":12}));
    assert_eq!(result["errors"], json!([]));
    let verify = |s: &Session| {
        let sk = sketch(s);
        let source = sk.curve_index(&ellipse).unwrap();
        let (c, m) = match sk.curves[source].kind {
            CurveKind::Ellipse { c, m, .. } => (sk.point(c).unwrap(), sk.point(m).unwrap()),
            _ => panic!("ellipse"),
        };
        let derived = sk.exact_segs(sk.curve_index(&output).unwrap()).unwrap();
        let want = m + (m - c).normalized().unwrap();
        assert!(derived[0].point_at(0.0).dist(want) < 1e-7, "updated offset {want:?}");
    };
    verify(&s);
    run(&mut s, "sketch.drag", json!({"entity":ellipse,"from":[0,0],"to":[4,3]}));
    verify(&s);
    let mut reopened = Session {
        doc: std::sync::Arc::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap()),
        active_sketch: s.active_sketch,
        ..Session::default()
    };
    reopened.refresh();
    verify(&reopened);
    let attached = reopened.doc.to_json();
    run(&mut reopened, "sketch.offset.break", json!({"curve":output}));
    let sk = sketch(&reopened);
    assert!(sk.freeform_offsets.is_empty());
    assert!(sk.curves[sk.curve_index(&output).unwrap()].kind.point_ids().iter().all(|i| !sk.points[*i].fixed));
    run(&mut reopened, "edit.undo", json!({}));
    assert_eq!(reopened.doc.to_json(), attached);
}

#[test]
fn certified_offset_delete_output_undo_reopen_and_refuse_dependent_source() {
    let mut s = new_sketch();
    run(&mut s, "sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5}));
    let out = run(&mut s, "sketch.offset", json!({"curves":["e1"],"distance":-1,"chain":false}));
    let id = out["curves"][0].as_str().unwrap().to_string();
    let attached = s.doc.to_json();
    let history = s.undo.len();
    assert!(s.execute("sketch.delete", &json!({"entities":["e1"]})).is_err());
    assert_eq!(s.doc.to_json(), attached);
    assert_eq!(s.undo.len(), history);
    run(&mut s, "sketch.delete", json!({"entities":[id]}));
    let sk = sketch(&s);
    assert!(sk.freeform_offsets.is_empty());
    assert!(sk.curve_index(&id).is_none());
    assert!(sk.curve_index("e1").is_some());
    let deleted = s.doc.to_json();
    let reopened = solidcraft_doc::format::read(&deleted).unwrap();
    assert_eq!(reopened.to_json(), deleted);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), attached);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), deleted);
    // The central removal path also handles a deleted fit node, which removes its curve.
    run(&mut s, "edit.undo", json!({}));
    let sk = sketch(&s);
    let node = sk.points[sk.curves[sk.curve_index(&id).unwrap()].kind.point_ids()[3]].id.clone();
    run(&mut s, "sketch.delete", json!({"entities":[node]}));
    assert!(sketch(&s).freeform_offsets.is_empty());
    assert!(sketch(&s).curve_index(&id).is_none());
    assert_eq!(sketch(&s).points.len(), 3, "all private derived nodes must be removed");
}

#[test]
fn periodic_closed_offset_chain_break_drag_reopen_and_material_volume() {
    let mut s = new_sketch();
    run(&mut s, "sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5}));
    run(&mut s, "sketch.dimension", json!({"entities":["e1"],"type":"major","value":10}));
    let source = sketch(&s).points.clone();
    let a = run(&mut s, "sketch.offset", json!({"curves":["e1"],"distance":-1,"chain":false}))["curves"][0].as_str().unwrap().to_string();
    let before_second = s.doc.to_json();
    let b = run(&mut s, "sketch.offset", json!({"curves":[a],"distance":-0.5,"chain":false}))["curves"][0].as_str().unwrap().to_string();
    assert_eq!(&sketch(&s).points[..source.len()], source.as_slice());
    let check = |session: &Session| {
        let sk = sketch(session);
        assert_eq!(sk.freeform_offsets.iter().map(|r| r.certificate).collect::<Vec<_>>(), [2, 2]);
        assert_eq!(solidcraft_sketch::find_profiles(&sk).len(), 3);
        for id in [&a, &b] {
            assert!(matches!(sk.curves[sk.curve_index(id).unwrap()].kind, CurveKind::Spline { periodic: true, .. }));
        }
    };
    check(&s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before_second);
    run(&mut s, "edit.redo", json!({}));
    check(&s);
    run(&mut s, "parameters.change", json!({"name":"d1","expression":12}));
    check(&s);
    let mut reopened =
        Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    reopened.refresh();
    check(&reopened);
    let attached = reopened.doc.to_json();
    let exact_before = sketch(&reopened).exact_segs(sketch(&reopened).curve_index(&b).unwrap()).unwrap();
    run(&mut reopened, "sketch.offset.break", json!({"curve":b}));
    let sk = sketch(&reopened);
    assert_eq!(sk.exact_segs(sk.curve_index(&b).unwrap()).unwrap(), exact_before);
    let points = sk.curves[sk.curve_index(&b).unwrap()].kind.point_ids();
    let first = sk.points[points[0]].id.clone();
    let from = sk.points[points[0]].pos;
    run(&mut reopened, "sketch.drag", json!({"entity":first,"from":from,"to":[from.x+0.2,from.y+0.1]}));
    let moved = sketch(&reopened);
    moved.validate_periodic_splines().unwrap();
    let indices = moved.curves[moved.curve_index(&b).unwrap()].kind.point_ids();
    assert_eq!(moved.point(indices[0]), moved.point(*indices.last().unwrap()));
    assert!(moved.point(indices[0]).unwrap().dist(from) > 0.1);
    assert_eq!(solidcraft_doc::format::read(&reopened.doc.to_json()).unwrap().to_json(), reopened.doc.to_json());
    run(&mut reopened, "edit.undo", json!({}));
    run(&mut reopened, "edit.undo", json!({}));
    assert_eq!(reopened.doc.to_json(), attached);
    // An independent ring area multiplied by height probes actual profile material.
    let regions = solidcraft_sketch::find_profiles(&sketch(&s));
    let selected = regions.iter().enumerate().min_by(|(_, x), (_, y)| x.area.total_cmp(&y.area)).unwrap();
    let n = 4096;
    let h = std::f64::consts::TAU / n as f64;
    let perimeter = (0..=n)
        .map(|i| {
            let t = i as f64 * h;
            (144.0 * t.sin().powi(2) + 25.0 * t.cos().powi(2)).sqrt()
                * if i == 0 || i == n {
                    1.0
                } else if i % 2 == 0 {
                    2.0
                } else {
                    4.0
                }
        })
        .sum::<f64>()
        * h
        / 3.0;
    let analytic_ring = 0.5 * perimeter + 1.25 * std::f64::consts::PI;
    assert!((selected.1.area - analytic_ring).abs() < 0.03);
    let expected = analytic_ring * 3.0;
    let sketch_id = s.active_sketch.unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":sketch_id,"profiles":[selected.0],"distance":3,"operation":"new"}));
    let state = s.world_state();
    let volume = solidcraft_kernel::measure(&state.bodies.last().unwrap().body).unwrap().volume;
    assert!((volume - expected).abs() < 0.05, "{volume} vs {expected}");
}

#[test]
fn periodic_fit_creator_keeps_legacy_and_refuses_hostile_intent_atomically() {
    let mut s = new_sketch();
    let p = json!({"points":[[0,0],[4,0],[3,2],[-1,1]],"periodic":true});
    let id = run(&mut s, "sketch.spline.fit_point", p.clone())["curves"][0].as_str().unwrap().to_string();
    let sk = sketch(&s);
    let i = sk.curve_index(&id).unwrap();
    assert!(matches!(sk.curves[i].kind, CurveKind::Spline { periodic: true, .. }));
    sk.validate_periodic_splines().unwrap();
    assert_eq!(solidcraft_sketch::find_profiles(&sk).len(), 1);
    let before = s.doc.to_json();
    let history = s.undo.len();
    for (cmd, p) in [
        ("sketch.spline.control_point", p),
        ("sketch.spline.fit_point", json!({"points":[[0,0],[1,0],[0,0],[1,0]],"periodic":true})),
        ("sketch.spline.fit_point", json!({"points":[[0,0],[1,0],[1,1]],"periodic":"yes"})),
    ] {
        assert!(s.execute(cmd, &p).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), history);
    }
    let indices = sk.curves[i].kind.point_ids();
    let point = sk.points[indices[1]].id.clone();
    let target = sk.points[indices[0]].pos;
    let redo = s.redo.len();
    assert!(s.execute("sketch.drag", &json!({"entity":point,"from":sk.points[indices[1]].pos,"to":target})).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), history);
    assert_eq!(s.redo.len(), redo);
    let legacy =
        run(&mut s, "sketch.spline.fit_point", json!({"points":[[0,0],[4,0],[3,2],[-1,1],[0,0]]}))["curves"][0].as_str().unwrap().to_string();
    assert!(matches!(sketch(&s).curves[sketch(&s).curve_index(&legacy).unwrap()].kind, CurveKind::Spline { periodic: false, .. }));
}

#[test]
fn analytic_offset_partial_delete_releases_survivors_preserves_source_and_undo() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,30]}));
    let out = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":-1}));
    let outputs = ids(&out["curves"]);
    assert_eq!(outputs.len(), 4);
    let before = s.doc.to_json();
    run(&mut s, "sketch.delete", json!({"entities":[outputs[0]]}));
    let sk = sketch(&s);
    assert!(sk.offsets.is_empty());
    for id in outputs.iter().skip(1) {
        let curve = &sk.curves[sk.curve_index(id).unwrap()];
        assert!(curve.kind.point_ids().iter().all(|i| !sk.points[*i].fixed));
    }
    assert!(sk.curve_index("l1").is_some());
    let saved = s.doc.to_json();
    assert_eq!(solidcraft_doc::format::read(&saved).unwrap().to_json(), saved);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn periodic_offset_extrusion_survives_post_extrude_span_changes_and_cold_reopen() {
    let mut s = new_sketch();
    run(&mut s, "sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":5}));
    run(&mut s, "sketch.dimension", json!({"entities":["e1"],"type":"major","value":10}));
    let a = run(&mut s, "sketch.offset", json!({"curves":["e1"],"distance":-1,"chain":false}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.offset", json!({"curves":[a],"distance":-0.5,"chain":false}));
    let sk_id = s.active_sketch.unwrap();
    let initial = solidcraft_sketch::find_profiles(&sketch(&s));
    let (index, ring) = initial.iter().enumerate().min_by(|(_, a), (_, b)| a.area.total_cmp(&b.area)).unwrap();
    let initial_count = ring.outer_curves.len();
    let boundary = ring.whole_curve_boundaries().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":sk_id,"profiles":[index],"distance":3,"operation":"new"}));
    let initial_doc = s.doc.to_json();
    for major in [12.0, 16.0, 20.0] {
        run(&mut s, "parameters.change", json!({"name":"d1","expression":major}));
        let state = s.world_state();
        let sk = state.sketch(sk_id).unwrap();
        let ring = sk.profiles.iter().find(|p| p.whole_curve_boundaries().as_ref() == Some(&boundary)).unwrap();
        assert_ne!(ring.outer_curves.len(), initial_count, "the repro must actually change graph span multiplicity");
        // Length-only intent leaves the centre and axis pose free. Measure that rigid frame,
        // then derive independent ellipse normal probes (not fitted-output points).
        let CurveKind::Ellipse { c, m, r } = sk.sketch.curves[sk.sketch.curve_index("e1").unwrap()].kind else { panic!("ellipse") };
        let centre = sk.sketch.point(c).unwrap();
        let axis = sk.sketch.point(m).unwrap() - centre;
        assert!((axis.len() - major).abs() < 1e-6);
        assert!((r - 5.0).abs() < 1e-6);
        let u = axis / axis.len();
        let v = Vec2::new(-u.y, u.x);
        let t: f64 = 0.2;
        let ellipse = centre + u * (major * t.cos()) + v * (5.0 * t.sin());
        let gradient = u * (t.cos() / major) + v * (t.sin() / 5.0);
        let normal = gradient / gradient.len();
        eprintln!("major={major} centre={centre:?} axis={axis:?}");
        assert!(ring.region.contains(ellipse + normal * 1.25), "major={major}, ring area={}", ring.area);
        assert!(!ring.region.contains(ellipse + normal * 0.5));
        assert!(!ring.region.contains(ellipse + normal * 1.75));
        let n = 4096;
        let h = std::f64::consts::TAU / n as f64;
        let perimeter = (0..=n)
            .map(|i| {
                let t = i as f64 * h;
                (major * major * t.sin().powi(2) + 25.0 * t.cos().powi(2)).sqrt()
                    * if i == 0 || i == n {
                        1.0
                    } else if i % 2 == 0 {
                        2.0
                    } else {
                        4.0
                    }
            })
            .sum::<f64>()
            * h
            / 3.0;
        let expected = (0.5 * perimeter + 1.25 * std::f64::consts::PI) * 3.0;
        assert_eq!(state.bodies.len(), 1);
        let body = &state.bodies[0].body;
        for (distance, material) in [(1.25, true), (0.5, false), (1.75, false)] {
            let point = ellipse + normal * distance;
            assert_eq!(body.contains(solidcraft_geom::Vec3::new(point.x, point.y, 1.5)), material);
        }
        // Repeat at the exact major-axis seam as a physical material control.
        for (distance, material) in [(1.25, true), (0.5, false), (1.75, false)] {
            let point = centre + u * (major + distance);
            assert_eq!(ring.region.contains(point), material);
            assert_eq!(body.contains(solidcraft_geom::Vec3::new(point.x, point.y, 1.5)), material);
        }
        let volume = solidcraft_kernel::measure(body).unwrap().volume;
        assert!((volume - expected).abs() < 0.05, "{major}: {volume} vs independent {expected}");
        let mut cold = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
        cold.refresh();
        let cold_state = cold.world_state();
        assert_eq!(cold_state.bodies.len(), 1);
        let cold_volume = solidcraft_kernel::measure(&cold_state.bodies[0].body).unwrap().volume;
        assert!((cold_volume - volume).abs() < 1e-8);
        let edited = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), initial_doc);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), edited);
        run(&mut s, "edit.undo", json!({}));
    }
}

/// The bounding box of a set of sketch curves, from their polylines.
fn bounds(sk: &solidcraft_sketch::Sketch, curves: &[String]) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for id in curves {
        let i = sk.curve_index(id).unwrap();
        for p in sk.polyline(i) {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    (lo, hi)
}

fn box_is(sk: &solidcraft_sketch::Sketch, curves: &[String], lo: [f64; 2], hi: [f64; 2]) -> bool {
    let (a, b) = bounds(sk, curves);
    a.dist(Vec2::new(lo[0], lo[1])) < 1e-6 && b.dist(Vec2::new(hi[0], hi[1])) < 1e-6
}

fn side_curves(v: &Value, i: usize) -> Vec<String> {
    ids(&v["sides"][i]["curves"])
}

#[test]
fn offset_symmetric_puts_the_same_distance_on_both_sides_of_a_chain() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // A 40 by 30 rectangle offset symmetrically by 5: one chain 5 inside (5,5)-(35,25), a
    // 30 by 20 rectangle, and one 5 outside (-5,-5)-(45,35), a 50 by 40 rectangle.
    let o = run(&mut s, "sketch.offset", json!({"curves": [l[0]], "distance": 5, "symmetric": true}));
    assert_eq!(o["sides"].as_array().map(Vec::len), Some(2), "{o}");
    assert_eq!(ids(&o["curves"]).len(), 8, "{o}");
    let (a, b) = (side_curves(&o, 0), side_curves(&o, 1));
    assert_eq!((a.len(), b.len()), (4, 4), "{o}");
    let sk = sketch(&s);
    // The chain's walk direction decides which side is first, so match on the sizes.
    let inner = box_is(&sk, &a, [5.0, 5.0], [35.0, 25.0]) && box_is(&sk, &b, [-5.0, -5.0], [45.0, 35.0]);
    let outer = box_is(&sk, &b, [5.0, 5.0], [35.0, 25.0]) && box_is(&sk, &a, [-5.0, -5.0], [45.0, 35.0]);
    assert!(inner || outer, "{:?} {:?}", bounds(&sk, &a), bounds(&sk, &b));
    // The two distances are equal and opposite.
    let d: Vec<f64> = o["sides"].as_array().unwrap().iter().map(|x| x["distance"].as_f64().unwrap()).collect();
    assert!(close(d[0], -d[1]) && close(d[0].abs(), 5.0), "{d:?}");
    // Each side has four parallels and a persistent offset-distance driver.
    assert_eq!(ids(&o["constraints"]).len(), 10, "{o}");
    assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Parallel { .. })).count(), 8);
    assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::OffsetDistance { .. })).count(), 2);
}

#[test]
fn offset_two_sides_takes_an_independent_distance_the_other_way() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // `side` sends `distance` 5 inward, so `distance2` 3 goes outward: (5,5)-(35,25), a 30 by
    // 20 rectangle, and (-3,-3)-(43,33), a 46 by 36 one.
    let o = run(&mut s, "sketch.offset", json!({"curves": [l[0]], "distance": 5, "distance2": 3, "side": [20, 15]}));
    assert_eq!(o["sides"].as_array().map(Vec::len), Some(2), "{o}");
    let sk = sketch(&s);
    assert!(box_is(&sk, &side_curves(&o, 0), [5.0, 5.0], [35.0, 25.0]), "{:?}", bounds(&sk, &side_curves(&o, 0)));
    assert!(box_is(&sk, &side_curves(&o, 1), [-3.0, -3.0], [43.0, 33.0]), "{:?}", bounds(&sk, &side_curves(&o, 1)));
}

#[test]
fn offset_two_sides_of_one_curve_follows_the_sign_of_the_distance() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // The bottom edge runs (0,0)-(40,0). Chain off, distance -5 and distance2 3: one line at
    // y = -5 and one at y = 3, each 40 long.
    let f = run(&mut s, "sketch.offset", json!({"curves": [l[0]], "distance": -5, "distance2": 3, "chain": false}));
    assert_eq!(f["sides"].as_array().map(Vec::len), Some(2), "{f}");
    let sk = sketch(&s);
    let (one, two) = (bounds(&sk, &side_curves(&f, 0)), bounds(&sk, &side_curves(&f, 1)));
    assert!(close(one.0.y, one.1.y) && close(two.0.y, two.1.y), "{one:?} {two:?}");
    assert!(close(one.0.y, -5.0) && close(two.0.y, 3.0), "{one:?} {two:?}");
    assert!(close(one.1.x - one.0.x, 40.0) && close(two.1.x - two.0.x, 40.0), "{one:?} {two:?}");
}

#[test]
fn offset_both_sides_of_a_rounded_boundary_keep_separate_associations() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-38, -33], "p1": [38, 33]}));
    run(&mut s, "sketch.fillet", json!({"points": ["l1.start", "l1.end", "l2.end", "l3.end"], "radius": "1 mm"}));
    // A 76 by 66 boundary with 1 mm corners, offset symmetrically by 0.2: 75.6 by 65.6 with
    // 0.8 mm corners inside, and 76.4 by 66.4 with 1.2 mm corners outside.
    let o = run(&mut s, "sketch.offset", json!({"curves": ["l1"], "distance": 0.2, "symmetric": true}));
    let sides = o["sides"].as_array().unwrap().clone();
    assert_eq!(sides.len(), 2, "{o}");
    // Each side is its own offset association; a single side still reports the one it always did.
    let associations: Vec<&str> = sides.iter().filter_map(|x| x["association"].as_str()).collect();
    assert_eq!(associations.len(), 2, "{o}");
    assert_ne!(associations[0], associations[1], "{o}");
    assert!(o["association"].is_null(), "two sides name no single association: {o}");
    let sk = sketch(&s);
    assert_eq!(sk.offsets.len(), 2);
    for association in &associations {
        assert_eq!(sk.offset_boundary(association).unwrap().len(), 8, "{association}");
    }
    let radii = |curves: &[String]| {
        let mut r: Vec<f64> = curves
            .iter()
            .filter_map(|id| {
                let CurveKind::Arc { c, a, .. } = sk.curves.get(sk.curve_index(id)?)?.kind else { return None };
                Some(sk.point(c)?.dist(sk.point(a)?))
            })
            .collect();
        r.sort_by(f64::total_cmp);
        r
    };
    let (a, b) = (side_curves(&o, 0), side_curves(&o, 1));
    let (ra, rb) = (radii(&a), radii(&b));
    assert_eq!((ra.len(), rb.len()), (4, 4), "{ra:?} {rb:?}");
    let shrunk = ra.iter().all(|r| close(*r, 0.8)) && rb.iter().all(|r| close(*r, 1.2));
    let grown = rb.iter().all(|r| close(*r, 0.8)) && ra.iter().all(|r| close(*r, 1.2));
    assert!(shrunk || grown, "{ra:?} {rb:?}");
    let small = if shrunk { &a } else { &b };
    let large = if shrunk { &b } else { &a };
    assert!(box_is(&sk, small, [-37.8, -32.8], [37.8, 32.8]), "{:?}", bounds(&sk, small));
    assert!(box_is(&sk, large, [-38.2, -33.2], [38.2, 33.2]), "{:?}", bounds(&sk, large));
}

#[test]
fn offset_both_sides_of_a_circle_are_concentric() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    // Symmetric by 3 on a 10 mm circle: a 7 mm circle and a 13 mm one, both about the centre.
    let o = run(&mut s, "sketch.offset", json!({"curves": [c], "distance": 3, "symmetric": true}));
    let sk = sketch(&s);
    let radius = |id: &str| match sk.curves.get(sk.curve_index(id).unwrap()).unwrap().kind {
        CurveKind::Circle { c, r } => (sk.point(c).unwrap(), r),
        _ => panic!("not a circle"),
    };
    let made = ids(&o["curves"]);
    assert_eq!(made.len(), 2, "{o}");
    let (p0, r0) = radius(&made[0]);
    let (p1, r1) = radius(&made[1]);
    assert!(p0.dist(Vec2::default()) < 1e-9 && p1.dist(Vec2::default()) < 1e-9);
    assert!(close(r0, 7.0) && close(r1, 13.0), "{r0} {r1}");
    // A circle whose inward side would be offset away to nothing is refused, both sides with it.
    assert!(s.execute("sketch.offset", &json!({"curves": [c], "distance": 10, "symmetric": true})).is_err());
}

#[test]
fn offset_refuses_the_combinations_that_would_be_guesswork() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    let refused = |s: &mut Session, p: Value| s.execute("sketch.offset", &p).unwrap_err().to_string();
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 5, "symmetric": true, "side": [20, 15]}));
    assert!(e.contains("no side left to pick"), "{e}");
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 5, "symmetric": true, "distance2": 3}));
    assert!(e.contains("`distance2` or `symmetric`"), "{e}");
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 5, "distance2": -3}));
    assert!(e.contains("`distance2` must be a positive length"), "{e}");
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 5, "distance2": 0}));
    assert!(e.contains("`distance2` must be a positive length"), "{e}");
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 5, "direction": "negative"}));
    assert!(e.contains("no negative direction"), "{e}");
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 5, "direction": "sideways"}));
    assert!(e.contains("unknown direction"), "{e}");
    // An inward side the shape cannot hold refuses the whole command, the outward side with it.
    let e = refused(&mut s, json!({"curves": [l[0]], "distance": 20, "symmetric": true}));
    assert!(e.contains("larger than the shape allows"), "{e}");
    assert_eq!(sketch(&s).curves.len(), 4, "a refused offset leaves the sketch alone");
    // `direction: symmetric` is the word `solid.extrude` uses, and it means the same here.
    let o = run(&mut s, "sketch.offset", json!({"curves": [l[0]], "distance": 5, "direction": "symmetric"}));
    assert_eq!(o["sides"].as_array().map(Vec::len), Some(2), "{o}");
}

#[test]
fn offset_one_side_still_reports_one_set_of_curves_and_its_association() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-38, -33], "p1": [38, 33]}));
    run(&mut s, "sketch.fillet", json!({"points": ["l1.start", "l1.end", "l2.end", "l3.end"], "radius": "1 mm"}));
    let o = run(&mut s, "sketch.offset", json!({"curves": ["l1"], "distance": 0.2, "side": [0, 0]}));
    assert_eq!(o["sides"].as_array().map(Vec::len), Some(1), "{o}");
    let association = o["association"].as_str().unwrap().to_string();
    assert_eq!(o["sides"][0]["association"].as_str(), Some(association.as_str()), "{o}");
    assert_eq!(ids(&o["curves"]), ids(&o["sides"][0]["curves"]), "{o}");
    assert!(close(o["sides"][0]["distance"].as_f64().unwrap().abs(), 0.2), "{o}");
}

#[test]
fn offset_driver_edits_reopens_and_undoes_without_moving_sources() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]}));
    let original = sketch(&s).points.clone();
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":"2 mm","side":[20,-10]}));
    let param = result["param"].as_str().unwrap().to_string();
    let outputs = ids(&result["curves"]);
    let check = |s: &Session, distance: f64| {
        let state = s.world_state();
        let sk = &state.sketch(1).unwrap().sketch;
        assert!((sk.offset_distance(&outputs[0]).unwrap().abs() - distance).abs() < 1e-8);
        let constraint = sk.constraints.iter().find(|c| c.param.as_deref() == Some(&param)).unwrap();
        assert!(solidcraft_sketch::dim_frame(sk, &constraint.kind).is_some());
        for (a, b) in sk.points.iter().zip(&original) {
            assert!(a.pos.dist(b.pos) < 1e-7);
        }
        let boundary =
            outputs.iter().flat_map(|id| sk.segs(sk.curve_index(id).unwrap())).flat_map(|s| [s.point_at(0.0), s.point_at(1.0)]).collect::<Vec<_>>();
        let min_x = boundary.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let max_x = boundary.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = boundary.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let max_y = boundary.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        assert!((min_x + distance).abs() < 1e-7 && (max_x - 40.0 - distance).abs() < 1e-7);
        assert!((min_y + distance).abs() < 1e-7 && (max_y - 30.0 - distance).abs() < 1e-7);
    };
    check(&s, 2.0);
    run(&mut s, "parameters.change", json!({"name":param,"expression":"4 mm"}));
    check(&s, 4.0);
    run(&mut s, "edit.undo", json!({}));
    check(&s, 2.0);
    run(&mut s, "edit.redo", json!({}));
    check(&s, 4.0);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened, 4.0);
    run(&mut reopened, "parameters.change", json!({"name":param,"expression":"3 mm"}));
    check(&reopened, 3.0);
}

#[test]
fn offset_driver_covers_disconnected_selections_and_refuses_bad_values_atomically() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    run(&mut s, "sketch.circle.center", json!({"center":[30,0],"radius":10}));
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1","c1"],"distance":"2 mm","chain":false}));
    assert_eq!(result["curves"].as_array().unwrap().len(), 2);
    let param = result["param"].as_str().unwrap();
    run(&mut s, "parameters.change", json!({"name":param,"expression":"3 mm"}));
    assert!(sketch(&s).offsets.iter().all(|offset| (offset.distance.abs() - 3.0).abs() < 1e-8));
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for value in [json!(0), json!("0 mm"), json!("1 deg"), json!("1e20 mm")] {
        assert!(s.execute("sketch.offset", &json!({"curves":["l1"],"distance":value})).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
    assert!(s.execute("sketch.offset", &json!({"curves":["l1","missing"],"distance":1})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn offset_driver_delete_and_break_remove_annotation_and_preserve_geometry() {
    for source in ["rectangle", "ellipse"] {
        let mut s = new_sketch();
        let id = if source == "rectangle" {
            run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]}));
            "l1"
        } else {
            run(&mut s, "sketch.ellipse", json!({"center":[0,0],"major":[20,0],"minor_radius":10}));
            "e1"
        };
        let result = run(&mut s, "sketch.offset", json!({"curves":[id],"distance":1}));
        let outputs = ids(&result["curves"]);
        let param = result["param"].as_str().unwrap();
        run(&mut s, "sketch.offset.break", json!({"curve":outputs[0]}));
        assert!(!sketch(&s).constraints.iter().any(|c| c.param.as_deref() == Some(param)));
        assert!(s.doc.param(param).is_none());
        assert!(outputs.iter().all(|id| sketch(&s).curve_index(id).is_some()));
        run(&mut s, "edit.undo", json!({}));
        assert!(sketch(&s).constraints.iter().any(|c| c.param.as_deref() == Some(param)));
        run(&mut s, "sketch.delete", json!({"entities":outputs}));
        assert!(s.doc.param(param).is_none());
    }
}

#[test]
fn offset_driver_uses_design_units_and_dimension_delete_freezes_current_distance() {
    let mut s = new_sketch();
    run(&mut s, "document.units", json!({"units":"in"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[100,0]]}));
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":"0.1","chain":false}));
    let param = result["param"].as_str().unwrap();
    assert!((sketch(&s).offsets[0].distance - 2.54).abs() < 1e-8);
    run(&mut s, "parameters.change", json!({"name":param,"expression":"0.2 in"}));
    let sk = sketch(&s);
    let annotation = sk.constraints.iter().find(|c| c.param.as_deref() == Some(param)).unwrap().id.clone();
    run(&mut s, "sketch.delete", json!({"entities":[annotation]}));
    assert!((sketch(&s).offsets[0].distance - 5.08).abs() < 1e-8);
    assert!(s.doc.param(param).is_none());
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!((reopened.world_state().sketch(1).unwrap().sketch.offsets[0].distance - 5.08).abs() < 1e-8);
}

#[test]
fn offset_driver_arc_first_retires_breaks_undoes_and_reopens() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-20,-15],"p1":[20,15]}));
    let fillet = run(&mut s, "sketch.fillet", json!({"points":["l1.start","l1.end","l2.end","l3.end"],"radius":"2 mm"}));
    let arc = fillet["curves"][0].as_str().unwrap();
    let result = run(&mut s, "sketch.offset", json!({"curves":[arc],"distance":"1 mm"}));
    let param = result["param"].as_str().unwrap();
    run(&mut s, "parameters.change", json!({"name":param,"expression":"3 mm"}));
    let sk = sketch(&s);
    let driver = sk.constraints.iter().find(|c| c.param.as_deref() == Some(param)).unwrap();
    let solidcraft_sketch::ConstraintKind::OffsetDistance { output, .. } = &driver.kind else {
        panic!("missing offset driver");
    };
    let record = sk.offsets.iter().find(|o| o.owns_output(output)).unwrap();
    assert!(!record.outputs.contains(output), "the dimension's first output arc must be absent from the live boundary");
    assert!(record.topology.as_ref().unwrap().roles.iter().any(|r| r.output == *output && r.retired));
    let survivor = sk.offsets[0].outputs.first().unwrap().clone();
    let before = sk.segs(sk.curve_index(&survivor).unwrap());
    run(&mut s, "sketch.offset.break", json!({"curve":survivor}));
    assert!(s.doc.param(param).is_none());
    assert!(sketch(&s).offsets.is_empty());
    assert_eq!(sketch(&s).segs(sketch(&s).curve_index(&survivor).unwrap()), before);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.world_state().sketch(1).unwrap().sketch.offsets.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.param(param).is_some());
    run(&mut s, "edit.redo", json!({}));
    assert!(s.doc.param(param).is_none());
}

#[test]
fn offset_driver_symmetric_disconnected_sources_share_one_distance() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    run(&mut s, "sketch.line", json!({"points":[[20,20],[30,20]]}));
    let source = sketch(&s).points.clone();
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1","l2"],"distance":"2 mm","symmetric":true,"chain":false}));
    assert_eq!(ids(&result["curves"]).len(), 4);
    assert_eq!(result["sides"].as_array().unwrap().len(), 4);
    let param = result["param"].as_str().unwrap();
    assert!(result["sides"].as_array().unwrap().iter().all(|side| side["param"] == param));
    run(&mut s, "parameters.change", json!({"name":param,"expression":"3 mm"}));
    let sk = sketch(&s);
    assert_eq!(sk.offsets.len(), 4);
    assert!(sk.offsets.iter().all(|offset| (offset.distance.abs() - 3.0).abs() < 1e-8));
    assert!(sk.points.iter().zip(&source).all(|(a, b)| a.pos.dist(b.pos) < 1e-8));
    // Breaking one side keeps the shared parameter while another association uses it.
    let first = result["curves"][0].as_str().unwrap();
    run(&mut s, "sketch.offset.break", json!({"curve":first}));
    assert!(s.doc.param(param).is_some());
    run(&mut s, "parameters.change", json!({"name":param,"expression":"4 mm"}));
    assert!(sketch(&s).offsets.iter().all(|offset| (offset.distance.abs() - 4.0).abs() < 1e-8));
}

#[test]
fn offset_driver_two_sides_edit_independently_and_reopen() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]}));
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":"2 mm","distance2":"1 mm","side":[20,15]}));
    let primary = result["param"].as_str().unwrap();
    let secondary = result["param2"].as_str().unwrap();
    assert_ne!(primary, secondary);
    let inside = side_curves(&result, 0);
    let outside = side_curves(&result, 1);
    run(&mut s, "parameters.change", json!({"name":primary,"expression":"4 mm"}));
    assert!(box_is(&sketch(&s), &inside, [4.0, 4.0], [36.0, 26.0]));
    assert!(box_is(&sketch(&s), &outside, [-1.0, -1.0], [41.0, 31.0]));
    run(&mut s, "parameters.change", json!({"name":secondary,"expression":"3 mm"}));
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut reopened, "sketch.edit", json!({"sketch":1}));
    assert!(box_is(&sketch(&reopened), &inside, [4.0, 4.0], [36.0, 26.0]));
    assert!(box_is(&sketch(&reopened), &outside, [-3.0, -3.0], [43.0, 33.0]));
    run(&mut s, "edit.undo", json!({}));
    assert!(box_is(&sketch(&s), &outside, [-1.0, -1.0], [41.0, 31.0]));
    run(&mut s, "edit.redo", json!({}));
    assert!(box_is(&sketch(&s), &outside, [-3.0, -3.0], [43.0, 33.0]));
    let before = s.doc.to_json();
    for second in [json!(0), json!(-1), json!("1 deg"), json!("1e20 mm")] {
        assert!(s.execute("sketch.offset", &json!({"curves":["l1"],"distance":1,"distance2":second})).is_err());
        assert_eq!(s.doc.to_json(), before);
    }
}

#[test]
fn offset_driver_later_groups_never_walk_outputs_created_by_earlier_groups() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    run(&mut s, "sketch.line", json!({"points":[[10,2],[20,2]]}));
    // The first offset ends exactly on the second source; it must not join its chain.
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1","l2"],"distance":2}));
    assert_eq!(ids(&result["curves"]).len(), 2);
    let sk = sketch(&s);
    assert_eq!(sk.offsets.len(), 2);
    assert_eq!(sk.offsets[0].sources, vec![("l1".into(), true)]);
    assert_eq!(sk.offsets[1].sources, vec![("l2".into(), true)]);
}

#[test]
fn offset_driver_format18_offsets_keep_fixed_distances_on_upgrade() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]]}));
    let result = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":2}));
    let param = result["param"].as_str().unwrap();
    let mut doc = (*s.doc).clone();
    doc.sketch_mut(1).unwrap().constraints.retain(|c| !matches!(c.kind, ConstraintKind::OffsetDistance { .. }));
    doc.params.retain(|p| p.name != param);
    let mut json: Value = serde_json::from_str(&doc.to_json()).unwrap();
    json["format"] = json!("solidcraft/18");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&json.to_string()).unwrap());
    assert_eq!(reopened.doc.format, solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT), "upgraded on opening");
    run(&mut reopened, "sketch.edit", json!({"sketch":1}));
    let sk = sketch(&reopened);
    assert!((sk.offsets[0].distance - 2.0).abs() < 1e-8);
    assert!(!sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::OffsetDistance { .. })));
    assert_eq!(ids(&result["curves"]).len(), sk.offsets[0].outputs.len());
}

#[test]
fn several_texts_edit_in_one_rebuild() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Labels"}));
    let a = run(&mut s, "sketch.text", json!({"text": "I", "at": [0, 0], "height": 10}))["link"].as_str().unwrap().to_string();
    let b = run(&mut s, "sketch.text", json!({"text": "I", "at": [30, 0], "height": 10}))["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Labels", "distance": 2}));
    let volume = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    let v0 = volume(&mut s);
    run(&mut s, "sketch.edit", json!({"sketch": "Labels"}));
    let (before, steps) = (s.evaluations, s.undo.len());
    let r = run(&mut s, "sketch.edit_text", json!({"edits": [{"link": a, "text": "II"}, {"link": b, "text": "II"}]}));
    assert_eq!(s.evaluations, before + 1, "one rebuild for both texts");
    assert_eq!(s.undo.len(), steps + 1, "one undo step for both texts");
    assert_eq!(r["edits"][1]["link"], json!(b), "{r}");
    // All or nothing: a bad second edit leaves the first text as it was.
    assert!(s.execute("sketch.edit_text", &json!({"edits": [{"link": a, "text": "III"}, {"link": "nope", "text": "I"}]})).is_err());
    assert!(s.execute("sketch.edit_text", &json!({"edits": []})).is_err());
    // A live frame takes the same list.
    let r = run(&mut s, "sketch.edit_text", json!({"edits": [{"link": a, "height": 12}, {"link": b, "height": 12}], "live": true}));
    assert_eq!(r["edits"][0]["link"], json!(a), "{r}");
    run(&mut s, "sketch.edit_text", json!({"edits": [{"link": a, "height": 10}, {"link": b, "height": 10}], "live": true}));
    run(&mut s, "sketch.drag_end", json!({}));
    run(&mut s, "sketch.finish", json!({}));
    let v1 = volume(&mut s);
    assert!((v1 / v0 - 2.0).abs() < 0.05, "two bars per label now: {v0} -> {v1}");
}

#[test]
fn trim_keeps_the_rest_of_a_dimensioned_line_where_it_was() {
    // A 40 x 40 square, its right side dimensioned with the Dimension tool (a line length).
    let mut s = new_sketch();
    let sq = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 40], [0, 40]], "closed": true, "infer": true}))["curves"]);
    let (right, top) = (sq[1].clone(), sq[2].clone());
    run(&mut s, "sketch.dimension", json!({"entities": [right]}));
    // A step drawn across the top right corner, its ends on the top and right sides.
    let step = ids(&run(&mut s, "sketch.line", json!({"points": [[23, 40], [23, 23], [40, 23]], "infer": true}))["curves"]);
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", step[0]), "b": top}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.end", step[1]), "b": right}));
    let corner = |s: &Session| {
        let sk = sketch(s);
        sk.point(sk.resolve_point(&format!("{}.end", step[1])).unwrap()).unwrap()
    };
    let forty = |s: &Session| {
        sketch(s).constraints.iter().filter(|c| c.kind.value().is_some_and(|v| close(v, 40.0))).map(|c| c.kind.clone()).collect::<Vec<_>>()
    };

    // Trim the right side above the step: the 40 stays between the square's corners, which are
    // both still there, instead of stretching the 23 mm that is left back to 40.
    run(&mut s, "sketch.trim", json!({"curve": right, "at": [40, 30]}));
    assert!(corner(&s).dist(Vec2::new(40.0, 23.0)) < 1e-6, "the step moved to {:?}", corner(&s));
    let sk = sketch(&s);
    let ends = sk.shape(sk.curve_index(&right).unwrap()).unwrap();
    assert!(matches!(ends, solidcraft_sketch::Shape::Line { a, b } if close(a.dist(b), 23.0)), "{ends:?}");
    assert!(matches!(forty(&s)[..], [ConstraintKind::Distance { .. }]), "{:?}", forty(&s));

    // Trim the top side right of the step: the corner the 40 measured to goes, and the 40 with it.
    run(&mut s, "sketch.trim", json!({"curve": top, "at": [30, 40]}));
    assert!(corner(&s).dist(Vec2::new(40.0, 23.0)) < 1e-6, "the step moved to {:?}", corner(&s));
    assert!(forty(&s).is_empty(), "{:?}", forty(&s));
}

/// #3052: the piece Trim shows under the cursor is the one `sketch.trim` takes away there: its
/// ends lie on the crossings either side of the cursor (or the curve's own end), and after the
/// trim no curve runs through its middle any more. A line crossed twice, its middle and an end
/// piece; a circle crossed by a line, the half round the cursor; a line nothing crosses, whole.
#[test]
fn trim_piece_is_what_a_trim_there_takes_away() {
    let near = |a: Vec2, b: [f64; 2]| (a - Vec2::new(b[0], b[1])).len() < 1e-6;
    let crossed_line = || {
        let mut s = new_sketch();
        let line = ids(&run(&mut s, "sketch.line", json!({"points": [[0,0], [30,0]], "connect": false}))["curves"])[0].clone();
        run(&mut s, "sketch.line", json!({"points": [[10,-5], [10,5]], "connect": false}));
        run(&mut s, "sketch.line", json!({"points": [[20,-5], [20,5]], "connect": false}));
        (s, line)
    };
    let crossed_circle = || {
        let mut s = new_sketch();
        let circle = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0,0], "radius": 10}))["curves"])[0].clone();
        run(&mut s, "sketch.line", json!({"points": [[-20,0], [20,0]], "connect": false}));
        (s, circle)
    };
    let cases: [(&dyn Fn() -> (Session, String), [f64; 2], [[f64; 2]; 2], [f64; 2]); 4] = [
        (&crossed_line, [15.0, 0.0], [[10.0, 0.0], [20.0, 0.0]], [15.0, 0.0]),
        (&crossed_line, [4.0, 0.0], [[0.0, 0.0], [10.0, 0.0]], [5.0, 0.0]),
        (&crossed_circle, [0.0, 10.0], [[10.0, 0.0], [-10.0, 0.0]], [0.0, 10.0]),
        (&crossed_circle, [0.0, -10.0], [[10.0, 0.0], [-10.0, 0.0]], [0.0, -10.0]),
    ];
    for (make, at, ends, inside) in cases {
        let (mut s, curve) = make();
        let piece = super::trim_piece(&sketch(&s), &curve, Vec2::new(at[0], at[1])).unwrap();
        let (first, last) = (piece[0], piece[piece.len() - 1]);
        assert!((near(first, ends[0]) && near(last, ends[1])) || (near(first, ends[1]) && near(last, ends[0])), "{at:?}: {first:?} {last:?}");
        let mid = if piece.len() > 2 { piece[piece.len() / 2] } else { (first + last) * 0.5 };
        assert!(near(mid, inside), "{at:?}: {mid:?}");
        run(&mut s, "sketch.trim", json!({"curve": curve, "at": at}));
        let sk = sketch(&s);
        assert!(!(0..sk.curves.len()).filter_map(|i| sk.shape(i)).any(|sh| sh.dist(mid) < 1e-6), "the trim at {at:?} took the piece shown away");
    }
    let mut s = new_sketch();
    let lone = ids(&run(&mut s, "sketch.line", json!({"points": [[0,0], [8,0]], "connect": false}))["curves"])[0].clone();
    let piece = super::trim_piece(&sketch(&s), &lone, Vec2::new(3.0, 0.0)).unwrap();
    assert!(near(piece[0], [0.0, 0.0]) && near(piece[piece.len() - 1], [8.0, 0.0]), "{piece:?}");
}

/// An offset's ends follow its source curves, so trimming a piece off it or extending it is
/// refused (the trim used to report success and leave the curve whole). Trimming away a whole
/// offset curve still works, and once the association is broken the trim cuts it.
#[test]
fn trim_and_extend_refuse_an_offsets_ends_until_its_association_is_broken() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let out = ids(&run(&mut s, "sketch.offset", json!({"curves": ["l1", "l2", "l3", "l4"], "distance": "2 mm", "side": [-5, -5]}))["curves"]);
    assert_eq!(out, ["l5", "l6", "l7", "l8"]);
    run(&mut s, "sketch.line", json!({"points": [[20, -10], [20, 10]]}));
    let before = serde_json::to_value(sketch(&s)).unwrap();
    let e = s.execute("sketch.trim", &json!({"curve": "l5", "at": [30, -2]})).unwrap_err().to_string();
    assert!(e.contains("offset") && e.contains("break the offset association"), "{e}");
    let e = s.execute("sketch.extend", &json!({"curve": "l5", "at": [42, -2]})).unwrap_err().to_string();
    assert!(e.contains("offset"), "{e}");
    assert_eq!(serde_json::to_value(sketch(&s)).unwrap(), before, "a refused trim changes nothing");

    run(&mut s, "sketch.offset.break", json!({"curve": "l5"}));
    run(&mut s, "sketch.trim", json!({"curve": "l5", "at": [30, -2]}));
    let sk = sketch(&s);
    let sh = sk.shape(sk.curve_index("l5").unwrap()).unwrap();
    assert!(
        matches!(sh, solidcraft_sketch::Shape::Line { a, b } if a.dist(Vec2::new(-2.0, -2.0)) < 1e-9 && b.dist(Vec2::new(20.0, -2.0)) < 1e-9),
        "{sh:?}"
    );
}

/// #2874: an offset's corners follow its source curves, so filleting them is refused before
/// anything changes (the whole batch), instead of leaving full circles and an unsolved sketch.
/// Once the offset association is broken the same corners round normally.
#[test]
fn fillet_refuses_an_offsets_corners_until_its_association_is_broken() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let out = ids(&run(&mut s, "sketch.offset", json!({"curves": ["l1", "l2", "l3", "l4"], "distance": "2 mm", "side": [-5, -5]}))["curves"]);
    assert_eq!(out, ["l5", "l6", "l7", "l8"]);
    let before = serde_json::to_value(sketch(&s)).unwrap();
    let corners = json!({"points": ["l5.start", "l5.end", "l6.end", "l7.end"], "radius": "2 mm"});
    let e = s.execute("sketch.fillet", &corners).unwrap_err().to_string();
    assert!(e.contains("offset") && e.contains("break the offset association"), "{e}");
    assert_eq!(serde_json::to_value(sketch(&s)).unwrap(), before, "a refused fillet changes nothing");
    let e = s.execute("sketch.chamfer.equal_distance", &json!({"point": "l6.end", "distance": 1})).unwrap_err().to_string();
    assert!(e.contains("offset"), "{e}");

    run(&mut s, "sketch.offset.break", json!({"curve": "l5"}));
    let f = run(&mut s, "sketch.fillet", corners);
    assert_eq!(f["sketch"]["solved"], json!(true), "{f}");
    let sk = sketch(&s);
    let arcs = ids(&f["curves"]);
    assert_eq!(arcs.len(), 4);
    for arc in &arcs {
        let ai = sk.curve_index(arc).unwrap();
        assert!(matches!(sk.curves[ai].kind, CurveKind::Arc { .. }), "{arc} is a trimmed arc");
        assert!((sk.radius(ai).unwrap() - 2.0).abs() < 1e-7);
    }
}
