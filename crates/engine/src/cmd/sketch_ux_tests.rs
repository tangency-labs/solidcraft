//! Sketch interaction tests: dimension placement, Fix/Unfix, the construction toggle, and
//! which entity moves when a two-entity constraint is applied.

use serde_json::{Value, json};
use solidcraft_sketch::{DimFrame, Sketch, dim_frame, dim_layout};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> Sketch {
    let id = s.active_sketch.expect("a sketch is being edited");
    s.doc.sketch(id).expect("the sketch").clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

// ---------------------------------------------------------------------------------------------
// Dimensions

#[test]
fn every_dimension_kind_has_a_drawable_frame() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 40]}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 20], "radius": 8}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [100, 0], "start": [110, 0], "end": [100, 10]}));
    run(&mut s, "sketch.line", json!({"points": [[-40, 0], [-10, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[-40, 0], [-20, 25]]}));
    for (ents, ty) in [
        (json!(["l1"]), "auto"),
        (json!(["l2"]), "vertical"),
        (json!(["l1.start", "c1.center"]), "horizontal"),
        (json!(["c1"]), "auto"),
        (json!(["a1"]), "radius"),
        (json!(["a1"]), "arc_length"),
        (json!(["l5", "l6"]), "angle"),
    ] {
        run(&mut s, "sketch.dimension", json!({"entities": ents, "type": ty}));
    }
    run(&mut s, "sketch.dimension", json!({"entities": ["c1.center", "l2"], "driven": true}));
    let sk = sketch(&s);
    let dims: Vec<_> = sk.constraints.iter().filter(|c| c.kind.is_dimension()).collect();
    assert_eq!(dims.len(), 8);
    let mut kinds = Vec::new();
    for c in dims {
        let f = dim_frame(&sk, &c.kind).unwrap_or_else(|| panic!("no frame for {c:?}"));
        let l = dim_layout(&f, None, 0.1, solidcraft_geom::Vec2::new(1.0, 0.5));
        assert!(!l.lines.is_empty() && !l.arrows.is_empty(), "{c:?}");
        assert!(l.text.is_finite());
        kinds.push(std::mem::discriminant(&f));
    }
    for f in [
        DimFrame::Linear { p0: Default::default(), p1: Default::default(), dir: Default::default() },
        DimFrame::Radial { center: Default::default(), radius: 1.0, diameter: true, arc: None },
        DimFrame::Angular { vertex: Default::default(), d0: Default::default(), sweep: 1.0 },
        DimFrame::ArcLength { center: Default::default(), radius: 1.0, start: 0.0, sweep: 1.0 },
    ] {
        assert!(kinds.contains(&std::mem::discriminant(&f)));
    }
}

#[test]
fn dimension_text_is_stored_relative_to_the_dimension_and_follows_it() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "connect": false}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "text_at": [20, -8]}));
    let param = d["param"].as_str().unwrap_or_default().to_string();
    let text = |s: &Session| {
        let sk = sketch(s);
        let c = sk.constraints.iter().find(|c| c.kind.is_dimension()).cloned().expect("dimension");
        let f = dim_frame(&sk, &c.kind).expect("frame");
        dim_layout(&f, c.text, 0.1, Default::default()).text
    };
    assert!(text(&s).dist(solidcraft_geom::Vec2::new(20.0, -8.0)) < 1e-9);
    // Moved by the parameter name; then the line moves and the text goes with it.
    run(&mut s, "sketch.dimension_text", json!({"dimension": param, "at": [30, 12]}));
    assert!(text(&s).dist(solidcraft_geom::Vec2::new(30.0, 12.0)) < 1e-9);
    run(&mut s, "sketch.move_point", json!({"point": "l1.start", "to": [0, 10]}));
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [40, 10]}));
    // Still 10 along and 12 across from the middle of the (moved) line.
    let sk = sketch(&s);
    let Some(DimFrame::Linear { p0, p1, dir }) = sk.constraints.iter().find_map(|c| dim_frame(&sk, &c.kind)) else { panic!("linear frame") };
    assert!(p0.y > 5.0, "the line moved: {p0:?}");
    let off = text(&s) - (p0 + p1) * 0.5;
    assert!((off.dot(dir) - 10.0).abs() < 1e-9 && (off.dot(dir.perp()) - 12.0).abs() < 1e-9, "{off:?}");
    // Reset puts it back to the default place; undo restores the drag.
    run(&mut s, "sketch.dimension_text", json!({"dimension": param, "reset": true}));
    assert!(sketch(&s).constraints.iter().all(|c| c.text.is_none()));
    assert!(s.execute("sketch.dimension_text", &json!({"dimension": "nope", "at": [0, 0]})).is_err());
    assert!(s.execute("sketch.dimension_text", &json!({"dimension": param, "at": [f64::NAN, 0]})).is_err());
}

#[test]
fn deleting_a_dimension_frees_its_parameter() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["c1"]}));
    let param = d["param"].as_str().unwrap_or_default().to_string();
    let id = sketch(&s).constraints.iter().find(|c| c.param.as_deref() == Some(param.as_str())).map(|c| c.id.clone()).expect("dim");
    run(&mut s, "sketch.delete", json!({"entities": [id]}));
    assert!(sketch(&s).constraints.iter().all(|c| !c.kind.is_dimension()));
    assert!(s.doc.param(&param).is_none());
}

// ---------------------------------------------------------------------------------------------
// Fix / Unfix

fn pt(s: &Session, id: &str) -> solidcraft_geom::Vec2 {
    let sk = sketch(s);
    sk.resolve_point(id).and_then(|i| sk.point(i)).unwrap_or_else(|| panic!("no point {id}"))
}

fn dof(s: &mut Session) -> i64 {
    run(s, "sketch.inspect", json!({}))["dof"].as_i64().unwrap_or(-1)
}

#[test]
fn fixed_points_survive_drags_and_solves_and_unfix_frees_them() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0], [30, 20]], "connect": false}));
    let free = dof(&mut s);
    let r = run(&mut s, "sketch.constraint.fix", json!({"entity": "l1.end"}));
    assert_eq!(r["result"]["fixed"], true);
    assert_eq!(dof(&mut s), free - 2);
    let corner = pt(&s, "l1.end");
    // Dragging the fixed point does nothing; dragging its neighbours never moves it.
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [50, 50]}));
    assert_eq!(pt(&s, "l1.end"), corner);
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l2", "mode": "vertical"}));
    run(&mut s, "sketch.move_point", json!({"point": "l2.end", "to": [40, 25]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 12}));
    assert!(pt(&s, "l1.end").dist(corner) < 1e-12, "{:?}", pt(&s, "l1.end"));
    assert!((pt(&s, "l1.start").dist(corner) - 12.0).abs() < 1e-6);
    // Unfix: the point moves again.
    let r = run(&mut s, "sketch.constraint.fix", json!({"entity": "l1.end"}));
    assert_eq!(r["result"]["fixed"], false);
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [30, -5]}));
    assert!(pt(&s, "l1.end").dist(corner) > 1.0);
}

#[test]
fn fixing_curves_holds_their_points_and_radius() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 5}));
    run(&mut s, "sketch.line", json!({"points": [[30, 0], [50, 0]]}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 40], "start": [10, 40], "end": [0, 50]}));
    run(&mut s, "sketch.ellipse", json!({"center": [60, 40], "major": [70, 40], "minor_radius": 4}));
    let free = dof(&mut s);
    // Multi-select fixes them all (circle 3, line 4, arc 5, ellipse 5 degrees of freedom).
    let r = run(&mut s, "sketch.constraint.fix", json!({"entities": ["c1", "l1", "a1", "e1"]}));
    assert_eq!(r["result"]["changed"], 4);
    assert_eq!(dof(&mut s), free - 17);
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["curves"].as_array().is_some_and(|c| c.iter().all(|c| c["fixed"] == true)), "{si}");
    // A fixed circle keeps centre and radius; a dimension on it conflicts.
    run(&mut s, "sketch.move_point", json!({"point": "c1.center", "to": [0, 0]}));
    assert_eq!(pt(&s, "c1.center"), solidcraft_geom::Vec2::new(10.0, 10.0));
    assert!(s.execute("sketch.dimension", &json!({"entities": ["c1"], "value": 20})).is_err());
    let r0 = sketch(&s).radius(0);
    assert_eq!(r0, Some(5.0));
    // Toggle without `fixed`: all fixed, so all are unfixed.
    let r = run(&mut s, "sketch.constraint.fix", json!({"entities": ["c1", "l1", "a1", "e1"]}));
    assert_eq!(r["result"]["fixed"], false);
    assert_eq!(dof(&mut s), free);
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "radius", "value": 7}));
    assert!((sketch(&s).radius(0).unwrap_or(0.0) - 7.0).abs() < 1e-9);
    // Mixed selection: one fixed, one free → both end up fixed.
    run(&mut s, "sketch.constraint.fix", json!({"entity": "l1"}));
    let r = run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1", "l1.start", "a1"]}));
    assert_eq!(r["result"]["fixed"], true);
    assert!(sketch(&s).curves.iter().filter(|c| c.id == "l1" || c.id == "a1").all(|c| c.fixed));
}

#[test]
fn projected_geometry_counts_as_fixed() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [10, 0, 5]}]}));
    let sk = sketch(&s);
    let ci = sk.curves.iter().position(|c| c.link.is_some()).unwrap_or_else(|| panic!("projected: {r}"));
    let id = sk.curves[ci].id.clone();
    assert!(sk.curve_locked(ci));
    let r = run(&mut s, "sketch.constraint.fix", json!({"entity": id}));
    // Already fixed: the toggle unfixes, which projected geometry ignores.
    assert_eq!(r["result"]["changed"], 0);
    assert!(sketch(&s).curve_locked(ci));
}

// ---------------------------------------------------------------------------------------------
// Construction toggle

fn profiles(s: &mut Session) -> i64 {
    run(s, "sketch.solve", json!({}))["profiles"].as_i64().unwrap_or(-1)
}

fn curve_ids(v: &Value) -> Vec<String> {
    v["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[test]
fn construction_toggle_takes_shapes_out_of_profiles_and_back() {
    let mut s = new_sketch();
    let rect = curve_ids(&run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]})));
    let slot = curve_ids(&run(&mut s, "sketch.slot.center_to_center", json!({"p0": [40, 5], "p1": [60, 5], "width": 6})));
    let poly = curve_ids(&run(&mut s, "sketch.polygon.inscribed", json!({"center": [0, 40], "radius": 8, "sides": 6})));
    let circ = curve_ids(&run(&mut s, "sketch.circle.center", json!({"center": [40, 40], "radius": 5})));
    let ell = curve_ids(&run(&mut s, "sketch.ellipse", json!({"center": [70, 40], "major": [80, 40], "minor_radius": 4})));
    assert_eq!(profiles(&mut s), 5);
    for (shape, left) in [(&rect, 4), (&slot, 3), (&poly, 2), (&circ, 1), (&ell, 0)] {
        let r = run(&mut s, "sketch.construction", json!({ "curves": shape }));
        assert_eq!(r["construction"], true);
        assert_eq!(profiles(&mut s), left, "{shape:?}");
    }
    // Back again in one go.
    let all: Vec<String> = [rect, slot, poly, circ, ell].concat();
    let r = run(&mut s, "sketch.construction", json!({ "curves": all }));
    assert_eq!(r["construction"], false);
    assert_eq!(profiles(&mut s), 5);
    // A mixed selection becomes construction as a whole.
    run(&mut s, "sketch.construction", json!({"curves": ["l1"]}));
    let r = run(&mut s, "sketch.construction", json!({"curves": ["l1", "l2", "l3", "l4"]}));
    assert_eq!(r["construction"], true);
    assert!(sketch(&s).curves.iter().filter(|c| ["l1", "l2", "l3", "l4"].contains(&c.id.as_str())).all(|c| c.construction));
    assert!(s.execute("sketch.construction", &json!({"curves": ["nope"]})).is_err());
}

#[test]
fn circles_and_arcs_honour_construction() {
    let cases = [
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [8, 0], "end": [-8, 0]})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [8, 0], "sweep": -90})),
        ("sketch.arc.three_point", json!({"start": [8, 0], "end": [-8, 0], "through": [0, 8]})),
        ("sketch.circle.two_point", json!({"p0": [-8, 0], "p1": [8, 0]})),
        ("sketch.circle.three_point", json!({"p0": [8, 0], "p1": [0, 8], "p2": [-8, 0]})),
    ];
    for (cmd, params) in cases {
        for construction in [false, true] {
            let mut s = new_sketch();
            let mut p = params.clone();
            if construction {
                p["construction"] = json!(true);
            }
            run(&mut s, cmd, p);
            let sk = sketch(&s);
            let curve = sk.curves.last().expect("created curve");
            assert_eq!(curve.construction, construction, "{cmd} {params}");
        }
    }
}

#[test]
fn centerline_can_become_construction_directly() {
    let mut s = new_sketch();
    let l = curve_ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]})));
    run(&mut s, "sketch.centerline", json!({ "curves": l, "value": true }));
    let r = run(&mut s, "sketch.construction", json!({ "curves": l, "value": true }));
    assert_eq!(r["changed"], 1);
    let sk = sketch(&s);
    assert!(sk.curves.iter().filter(|c| l.contains(&c.id)).all(|c| c.construction && !c.centerline));
    // And back to a centerline.
    run(&mut s, "sketch.centerline", json!({ "curves": l, "value": true }));
    let sk = sketch(&s);
    assert!(sk.curves.iter().filter(|c| l.contains(&c.id)).all(|c| c.centerline && !c.construction));
}

#[test]
fn projected_geometry_can_become_construction() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [10, 0, 5]}]}));
    let id = sketch(&s).curves.iter().find(|c| c.link.is_some()).map(|c| c.id.clone()).expect("projected");
    let r = run(&mut s, "sketch.construction", json!({ "curves": [id.clone()] }));
    assert_eq!(r["changed"], 1);
    let sk = sketch(&s);
    assert!(sk.curves.iter().any(|c| c.id == id && c.construction && c.link.is_some()));
}

// ---------------------------------------------------------------------------------------------
// Two-entity constraints: the first pick stays, the second moves

/// Positions of an entity's points (and its radius).
fn snap(s: &Session, id: &str) -> (Vec<solidcraft_geom::Vec2>, Option<f64>) {
    let sk = sketch(s);
    if let Some(i) = sk.curve_index(id) {
        let pts = sk.curves[i].kind.point_ids().into_iter().filter_map(|p| sk.point(p)).collect();
        return (pts, sk.radius(i));
    }
    (vec![pt(s, id)], None)
}

fn same(a: &(Vec<solidcraft_geom::Vec2>, Option<f64>), b: &(Vec<solidcraft_geom::Vec2>, Option<f64>)) -> bool {
    a.0.len() == b.0.len() && a.0.iter().zip(&b.0).all(|(p, q)| p.dist(*q) < 1e-9) && a.1.zip(b.1).is_none_or(|(x, y)| (x - y).abs() < 1e-9)
}

/// A free line l1 (5,3)–(45,3) and a circle c1 at (20,18) with radius 5.
fn line_and_circle() -> Session {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[5, 3], [45, 3]]}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 18], "radius": 5}));
    s
}

/// Apply `cmd` with `a` then `b`; the first must not move, the second must.
fn check_order(mut s: Session, cmd: &str, a: &str, b: &str) {
    let (a0, b0) = (snap(&s, a), snap(&s, b));
    run(&mut s, cmd, json!({"a": a, "b": b}));
    assert!(same(&snap(&s, a), &a0), "{cmd}: first `{a}` moved: {a0:?} -> {:?}", snap(&s, a));
    assert!(!same(&snap(&s, b), &b0), "{cmd}: second `{b}` did not move");
}

#[test]
fn tangent_moves_only_the_second_pick() {
    // Line then circle: only the circle moves (and keeps its size).
    let mut s = line_and_circle();
    let l0 = snap(&s, "l1");
    run(&mut s, "sketch.constraint.tangent", json!({"a": "l1", "b": "c1"}));
    assert!(same(&snap(&s, "l1"), &l0), "line moved: {:?}", snap(&s, "l1"));
    let c = snap(&s, "c1");
    assert!((c.1.unwrap_or(0.0) - 5.0).abs() < 1e-9 && (c.0[0].y - 8.0).abs() < 1e-6, "{c:?}");
    // Circle then line: only the line moves.
    check_order(line_and_circle(), "sketch.constraint.tangent", "c1", "l1");
}

#[test]
fn a_fixed_second_pick_makes_the_first_move() {
    let mut s = line_and_circle();
    run(&mut s, "sketch.constraint.fix", json!({"entity": "l1"}));
    let l0 = snap(&s, "l1");
    let c0 = snap(&s, "c1");
    run(&mut s, "sketch.constraint.tangent", json!({"a": "c1", "b": "l1"}));
    assert!(same(&snap(&s, "l1"), &l0));
    assert!(!same(&snap(&s, "c1"), &c0));
    // Fixed first: the second moves as usual.
    let mut s = line_and_circle();
    run(&mut s, "sketch.constraint.fix", json!({"entity": "c1"}));
    let c0 = snap(&s, "c1");
    run(&mut s, "sketch.constraint.tangent", json!({"a": "c1", "b": "l1"}));
    assert!(same(&snap(&s, "c1"), &c0));
}

#[test]
fn two_fully_constrained_entities_conflict_and_nothing_moves() {
    let mut s = line_and_circle();
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start", "l1.end", "c1.center"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "radius", "value": 5}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["curves"].as_array().is_some_and(|c| c.iter().all(|c| c["fully_constrained"] == true)), "{si}");
    let before = sketch(&s);
    let e = s.execute("sketch.constraint.tangent", &json!({"a": "l1", "b": "c1"}));
    assert!(e.is_err(), "{e:?}");
    assert_eq!(sketch(&s), before);
}

#[test]
fn line_pairs_keep_the_first_line() {
    let lines = || {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 2]], "connect": false}));
        run(&mut s, "sketch.line", json!({"points": [[5, 20], [25, 35]]}));
        s
    };
    let len = |s: &Session, l: &str| {
        let p = snap(s, l).0;
        p[0].dist(p[1])
    };
    for cmd in ["sketch.constraint.parallel", "sketch.constraint.perpendicular", "sketch.constraint.equal", "sketch.constraint.collinear"] {
        check_order(lines(), cmd, "l1", "l2");
        check_order(lines(), cmd, "l2", "l1");
        // The second line turns and slides; only Equal changes its length.
        let mut s = lines();
        let l0 = len(&s, "l2");
        run(&mut s, cmd, json!({"a": "l1", "b": "l2"}));
        if cmd != "sketch.constraint.equal" {
            assert!((len(&s, "l2") - l0).abs() < 1e-6, "{cmd}: {l0} -> {}", len(&s, "l2"));
        } else {
            assert!((len(&s, "l2") - len(&s, "l1")).abs() < 1e-6);
        }
    }
}

#[test]
fn circle_pairs_keep_the_first_circle() {
    let circles = || {
        let mut s = new_sketch();
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "connect": false}));
        run(&mut s, "sketch.circle.center", json!({"center": [30, 10], "radius": 8}));
        s
    };
    for cmd in ["sketch.constraint.concentric", "sketch.constraint.equal", "sketch.constraint.tangent"] {
        check_order(circles(), cmd, "c1", "c2");
        check_order(circles(), cmd, "c2", "c1");
    }
    // Equal resizes the second to the first.
    let mut s = circles();
    run(&mut s, "sketch.constraint.equal", json!({"a": "c2", "b": "c1"}));
    assert!((snap(&s, "c1").1.unwrap_or(0.0) - 8.0).abs() < 1e-9);
}

#[test]
fn point_constraints_keep_the_first_pick() {
    let pts = || {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0]]}));
        run(&mut s, "sketch.line", json!({"points": [[10, 10], [20, 25]]}));
        s
    };
    check_order(pts(), "sketch.constraint.coincident", "l2.start", "l1.end");
    check_order(pts(), "sketch.constraint.coincident", "l1.end", "l2.start");
    // Curve picked first, point second: the point goes onto the line.
    check_order(pts(), "sketch.constraint.coincident", "l1", "l2.start");
    // Midpoint: the point stays, the line moves.
    let mut s = pts();
    let p0 = snap(&s, "l2.end");
    run(&mut s, "sketch.constraint.midpoint", json!({"point": "l2.end", "line": "l1"}));
    assert!(same(&snap(&s, "l2.end"), &p0));
    // Symmetry: the first point and the line stay, the second mirrors.
    let mut s = pts();
    run(&mut s, "sketch.point", json!({"point": [3, 7]}));
    run(&mut s, "sketch.point", json!({"point": [4, -9]}));
    let (a0, l0) = (snap(&s, "p1"), snap(&s, "l1"));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "p1", "b": "p2", "line": "l1"}));
    assert!(same(&snap(&s, "p1"), &a0) && same(&snap(&s, "l1"), &l0));
    assert!(pt(&s, "p2").dist(solidcraft_geom::Vec2::new(3.0, -7.0)) < 1e-6, "{:?}", pt(&s, "p2"));
}

#[test]
fn coincident_line_pairs_resolve_ties_independently_of_pick_order() {
    for (a, b) in [("l1", "l2"), ("l2", "l1")] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false,"connect":false}));
        run(&mut s, "sketch.line", json!({"points":[[10,20],[30,20]],"infer":false,"connect":false}));
        run(&mut s, "sketch.constraint.coincident", json!({"a":a,"b":b}));
        let sk = sketch(&s);
        let p = sk.resolve_point("l1.start").unwrap();
        let q = sk.resolve_point("l2.start").unwrap();
        assert!(sk.point(p).unwrap().dist(sk.point(q).unwrap()) < 1e-6);
        let count = sk.constraints.len();
        let result = run(&mut s, "sketch.constraint.coincident", json!({"a":b,"b":a}));
        assert_eq!(result["existing"], true);
        assert_eq!(sketch(&s).constraints.len(), count);
    }
}

#[test]
fn coincident_refuses_invalid_curve_pairs_atomically() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,10],[30,10]],"infer":false,"connect":false}));
    run(&mut s, "sketch.circle.center", json!({"center":[40,40],"radius":5}));
    let before = serde_json::to_value(&s.doc).unwrap();
    let undo = s.undo.len();
    for (a, b, message) in [("l1", "l1", "distinct lines"), ("l1", "c1", "straight lines"), ("c1", "l1", "straight lines")] {
        let error = s.execute("sketch.constraint.coincident", &json!({"a":a,"b":b})).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

// ---------------------------------------------------------------------------------------------
// Sketch Palette options

#[test]
fn palette_options_are_kept_with_each_sketch() {
    let mut s = new_sketch();
    let o = run(&mut s, "sketch.options", json!({}));
    assert_eq!(o["options"]["show_dimensions"], true);
    assert_eq!(o["options"]["slice"], false);
    let o = run(&mut s, "sketch.options", json!({"show_dimensions": false, "snap": false, "slice": true, "sketch_3d": true}));
    assert_eq!(o["options"]["show_dimensions"], false);
    assert_eq!(o["options"]["snap"], false);
    assert_eq!(o["options"]["slice"], true);
    let v = sketch(&s).view;
    assert!(v.hide_dimensions && v.no_snap && v.slice && v.three_d && !v.hide_points);
    // Another sketch starts with the defaults; the first keeps its options after finishing.
    let first = s.active_sketch.expect("sketch");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    assert_eq!(sketch(&s).view, solidcraft_sketch::SketchView::default());
    let o = run(&mut s, "sketch.options", json!({"sketch": first}));
    assert_eq!(o["options"]["show_dimensions"], false);
    // Saved with the design.
    let text = serde_json::to_string(&s.doc.sketch(first).expect("first").clone()).expect("json");
    let back: Sketch = serde_json::from_str(&text).expect("parse");
    assert!(back.view.slice);
    assert!(s.execute("sketch.options", &json!({"bogus": true})).is_err());
    // Undo restores the option.
    run(&mut s, "sketch.options", json!({"show_points": false}));
    run(&mut s, "edit.undo", json!({}));
    assert!(!sketch(&s).view.hide_points);
}

/// Construction Geometries is a display option of one sketch, saved with it, and it changes
/// nothing about the geometry: construction curves were already outside every profile, and
/// they still are while hidden. Centerlines are a line type of their own and keep showing. A
/// design written before the option existed carries no key for it and loads with construction
/// geometry shown.
#[test]
fn hiding_construction_geometry_is_per_sketch_saved_and_leaves_profiles_and_centerlines_alone() {
    use crate::view::sketch_curve_visible;
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 45], [40, 45]], "connect": false, "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[0, 55], [40, 55]], "connect": false, "infer": false}));
    run(&mut s, "sketch.construction", json!({"curves": ["l5"], "value": true}));
    run(&mut s, "sketch.centerline", json!({"curves": ["l6"], "value": true}));
    let (rect, construction, centerline) = (0, 4, 5);
    let profiles = run(&mut s, "sketch.inspect", json!({}))["profiles"].clone();
    assert_eq!(profiles.as_array().map(Vec::len), Some(1), "the rectangle is one profile: {profiles}");

    assert_eq!(run(&mut s, "sketch.options", json!({}))["options"]["show_construction"], true);
    let o = run(&mut s, "sketch.options", json!({"show_construction": false}));
    assert_eq!(o["options"]["show_construction"], false);
    let sk = sketch(&s);
    assert!(sk.view.hide_construction);
    assert!(!sketch_curve_visible(&sk, construction), "construction geometry is hidden");
    assert!(sketch_curve_visible(&sk, centerline), "a centerline is its own line type");
    assert!(sketch_curve_visible(&sk, rect), "ordinary geometry stays");
    // Hiding is display only: the curves, their line types and the profiles are untouched.
    let after = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(after["profiles"], profiles);
    assert_eq!(after["curves"][construction]["construction"], true);
    assert!(sketch(&s).curves[centerline].centerline, "the centerline keeps its line type");
    // Undo puts the option back.
    run(&mut s, "edit.undo", json!({}));
    assert!(!sketch(&s).view.hide_construction);
    run(&mut s, "sketch.options", json!({"show_construction": false}));

    // Another sketch shows construction geometry; the first keeps the option after finishing.
    let first = s.active_sketch.expect("sketch");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    assert_eq!(run(&mut s, "sketch.options", json!({}))["options"]["show_construction"], true);
    assert_eq!(run(&mut s, "sketch.options", json!({"sketch": first}))["options"]["show_construction"], false);

    // Saved with the design.
    let text = serde_json::to_string(s.doc.sketch(first).expect("first")).expect("json");
    assert!(text.contains("hide_construction"), "{text}");
    let back: Sketch = serde_json::from_str(&text).expect("parse");
    assert!(back.view.hide_construction);

    // An older design file has no `hide_construction` key at all: `SketchView`'s serde default
    // fills it in, and construction geometry shows, the way it did before the option existed.
    let older = r#"{"points":[{"id":"origin","pos":[0.0,0.0],"fixed":true}],"curves":[],"constraints":[],
        "view":{"hide_projected":true,"slice":true}}"#;
    let old: Sketch = serde_json::from_str(older).expect("an older sketch still loads");
    assert!(old.view.hide_projected && old.view.slice && !old.view.hide_construction);
}

// ---------------------------------------------------------------------------------------------
// Dragging

fn drag(s: &mut Session, e: &str, from: [f64; 2], to: [f64; 2]) {
    run(s, "sketch.drag", json!({"entity": e, "from": from, "to": to}));
}

fn v(x: f64, y: f64) -> solidcraft_geom::Vec2 {
    solidcraft_geom::Vec2::new(x, y)
}

/// The corners of the polygon drawn by `out`, in order, and its support circle's centre and radius.
fn polygon_shape(s: &Session, out: &Value) -> (Vec<solidcraft_geom::Vec2>, solidcraft_geom::Vec2, f64) {
    use solidcraft_sketch::CurveKind;
    let sk = sketch(s);
    let corners = out["curves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| match &sk.curves[sk.curve_index(id.as_str().unwrap()).unwrap()].kind {
            CurveKind::Line { a, .. } => sk.point(*a).unwrap(),
            other => panic!("a side is a line: {other:?}"),
        })
        .collect();
    let support = &sk.curves[sk.curve_index(out["support_circle"].as_str().unwrap()).unwrap()].kind;
    let CurveKind::Circle { c, r } = support else { panic!("support circle: {support:?}") };
    (corners, sk.point(*c).unwrap(), *r)
}

/// A regular polygon: corners equally far from `c`, sides equally long.
fn assert_regular(corners: &[solidcraft_geom::Vec2], c: solidcraft_geom::Vec2, what: &str) {
    let n = corners.len();
    let reach: Vec<f64> = corners.iter().map(|q| q.dist(c)).collect();
    let sides: Vec<f64> = (0..n).map(|i| corners[i].dist(corners[(i + 1) % n])).collect();
    let even = |xs: &[f64]| xs.iter().all(|x| (x - xs[0]).abs() < 1e-6);
    assert!(even(&reach) && even(&sides), "{what}: corners {reach:?}, sides {sides:?}");
}

/// #7395: a circumscribed polygon had equal sides tangent to its circle only, which leaves every
/// other corner free (dof 9 for a hexagon); dragging its centre or a corner pulled it out of
/// shape. It is held regular now, as an inscribed one: dof 4 (centre, size, turn), and a drag
/// moves or turns it without deforming it.
#[test]
fn dragging_a_circumscribed_polygon_keeps_it_regular() {
    for sides in [4, 5, 6] {
        for drag_corner in [false, true] {
            let mut s = new_sketch();
            let out = run(&mut s, "sketch.polygon.circumscribed", json!({"center": [50, 50], "radius": 10, "sides": sides}));
            assert_eq!(out["sketch"]["dof"], 4, "{sides} sides: centre, size and turn only: {}", out["sketch"]);
            let (corners, c, r) = polygon_shape(&s, &out);
            assert_regular(&corners, c, "as drawn");
            assert!((r - 10.0).abs() < 1e-9);
            let support = out["support_circle"].as_str().unwrap();
            if drag_corner {
                let from = corners[0];
                let to = from + v(3.0, -4.0);
                drag(&mut s, &format!("{}.start", out["curves"][0].as_str().unwrap()), [from.x, from.y], [to.x, to.y]);
            } else {
                drag(&mut s, &format!("{support}.center"), [50.0, 50.0], [70.0, 60.0]);
            }
            run(&mut s, "sketch.drag_end", json!({}));
            let (corners, c, r) = polygon_shape(&s, &out);
            let what = format!("{sides} sides, dragged by its {}", if drag_corner { "corner" } else { "centre" });
            assert_regular(&corners, c, &what);
            let apothem = corners[0].dist(corners[1]) / (2.0 * (std::f64::consts::PI / sides as f64).tan());
            assert!((apothem - r).abs() < 1e-6, "{what}: the sides touch the support circle (apothem {apothem}, radius {r})");
            // A drag may change a free size by a hair (an inscribed polygon's too); the polygon
            // went from radius 10 to 8.02.
            if !drag_corner {
                assert!((r - 10.0).abs() < 1e-2 && c.dist(v(70.0, 60.0)) < 1e-6, "{what}: moved whole, radius {r}, centre {c:?}");
            }
        }
    }
}

#[test]
fn dragging_a_line_slides_it_and_an_end_keeps_the_other_end() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [40, 20]]}));
    drag(&mut s, "l1", [25.0, 15.0], [30.0, 25.0]);
    assert!(pt(&s, "l1.start").dist(v(15.0, 20.0)) < 1e-9 && pt(&s, "l1.end").dist(v(45.0, 30.0)) < 1e-9);
    drag(&mut s, "l1.end", [45.0, 30.0], [50.0, 0.0]);
    assert!(pt(&s, "l1.end").dist(v(50.0, 0.0)) < 1e-9);
    assert!(pt(&s, "l1.start").dist(v(15.0, 20.0)) < 1e-9);
}

#[test]
fn dragging_a_rectangle_side_moves_that_side_only() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [40, 30]}));
    // l2 is the right side (x = 40): dragged right, it stays vertical, the left side stays.
    let left = snap(&s, "l4");
    drag(&mut s, "l2", [40.0, 20.0], [50.0, 20.0]);
    assert!(same(&snap(&s, "l4"), &left), "left side moved");
    let (a, b) = (pt(&s, "l2.start"), pt(&s, "l2.end"));
    assert!((a.x - 50.0).abs() < 1e-6 && (b.x - 50.0).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn dragging_circles_and_arcs() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "connect": false}));
    // The edge changes the radius about a fixed centre; the centre moves the circle.
    drag(&mut s, "c1", [5.0, 0.0], [0.0, 8.0]);
    assert!((snap(&s, "c1").1.unwrap_or(0.0) - 8.0).abs() < 1e-9);
    assert!(pt(&s, "c1.center").dist(v(0.0, 0.0)) < 1e-9);
    drag(&mut s, "c1.center", [0.0, 0.0], [20.0, 5.0]);
    assert!(pt(&s, "c1.center").dist(v(20.0, 5.0)) < 1e-9);
    assert!((snap(&s, "c1").1.unwrap_or(0.0) - 8.0).abs() < 1e-9);
    run(&mut s, "sketch.arc.center_point", json!({"center": [50, 0], "start": [60, 0], "end": [50, 10]}));
    drag(&mut s, "a1", [57.07, 7.07], [50.0 + 200f64.sqrt(), 200f64.sqrt()]);
    assert!(pt(&s, "a1.center").dist(v(50.0, 0.0)) < 1e-9);
    assert!((snap(&s, "a1").1.unwrap_or(0.0) - 20.0).abs() < 1e-9, "{:?}", snap(&s, "a1"));
    assert!(pt(&s, "a1.start").dist(v(70.0, 0.0)) < 1e-9, "{:?}", pt(&s, "a1.start"));
}

#[test]
fn dragging_a_conic_reads_rho_off_the_cursor_and_its_apex_slides_like_any_point() {
    let mut s = new_sketch();
    run(&mut s, "sketch.conic", json!({"start": [0, 0], "end": [40, 0], "apex": [20, 20]}));
    let rho = |s: &Session| match sketch(s).curves[0].kind {
        solidcraft_sketch::CurveKind::Conic { rho, .. } => rho,
        ref k => panic!("not a conic: {k:?}"),
    };
    assert!((rho(&s) - 0.5).abs() < 1e-9, "default rho: {}", rho(&s));

    // Grabbed on the curve itself (its midpoint, at the default rho of 0.5): `rho` follows the
    // cursor, read off where it falls between the chord's midpoint (20, 0) and the apex (20,
    // 20). The three defining points stay exactly where they were.
    drag(&mut s, "n1", [20.0, 10.0], [20.0, 14.0]);
    assert!((rho(&s) - 0.7).abs() < 1e-6, "rho after drag: {}", rho(&s));
    assert!(pt(&s, "n1.start").dist(v(0.0, 0.0)) < 1e-9);
    assert!(pt(&s, "n1.end").dist(v(40.0, 0.0)) < 1e-9);
    assert!(pt(&s, "n1.apex").dist(v(20.0, 20.0)) < 1e-9);

    // Grabbed on the apex itself: it slides like any other point (not a `rho` edit), and `rho`
    // is unaffected.
    drag(&mut s, "n1.apex", [20.0, 20.0], [10.0, 30.0]);
    assert!(pt(&s, "n1.apex").dist(v(10.0, 30.0)) < 1e-9);
    assert!((rho(&s) - 0.7).abs() < 1e-6, "rho after apex drag: {}", rho(&s));
}

#[test]
fn constrained_geometry_resists_drags() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [40, 10]]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start", "l1.end"]}));
    let l0 = snap(&s, "l1");
    drag(&mut s, "l1", [25.0, 10.0], [25.0, 30.0]);
    assert!(same(&snap(&s, "l1"), &l0));
    // A horizontal line dragged at its end stays horizontal; its length dimension holds.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [40, 10]]}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    drag(&mut s, "l1.end", [40.0, 10.0], [45.0, 20.0]);
    let (a, b) = (pt(&s, "l1.start"), pt(&s, "l1.end"));
    assert!((a.y - b.y).abs() < 1e-6 && (a.dist(b) - 30.0).abs() < 1e-6, "{a:?} {b:?}");
}

/// How the arm's link lengths are set.
#[derive(Clone, Copy, Debug)]
enum Links {
    /// Length dimensions on both links.
    Dimensioned,
    /// Equal to fixed reference lines (no length dimension anywhere).
    EqualToFixed,
}

/// Two-link arm: shoulder `l1.start` fixed, |l1| = 30 and |l2| = 20, the hand `l2.end` dragged
/// in one `sketch.drag` call along a straight path that stays clear of the singular poses. The
/// elbow has two solutions for every hand position; a drag must keep the one it started on.
/// 1000 deterministic cases (fixed-seed LCG). Returns (elbow flips, drags that stopped short).
fn arm_drags(links: Links) -> (usize, usize) {
    use std::f64::consts::TAU;
    let (a, b) = (30.0, 20.0);
    let (lo, hi) = ((a - b) + 3.0, a + b - 3.0);
    let mut s = new_sketch();
    // Away from the sketch origin so nothing connects to it.
    let o = v(200.0, 100.0);
    run(&mut s, "sketch.line", json!({"points": [[o.x, o.y], [o.x + a, o.y], [o.x + a, o.y + b]], "ids": ["l1", "l2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    match links {
        Links::Dimensioned => {
            run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": a}));
            run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": b}));
        }
        Links::EqualToFixed => {
            run(&mut s, "sketch.line", json!({"points": [[o.x, o.y - 100.0], [o.x + a, o.y - 100.0]], "ids": ["r1"]}));
            run(&mut s, "sketch.line", json!({"points": [[o.x, o.y - 120.0], [o.x + b, o.y - 120.0]], "ids": ["r2"]}));
            run(&mut s, "sketch.constraint.fix", json!({"entities": ["r1", "r2"]}));
            run(&mut s, "sketch.constraint.equal", json!({"a": "r1", "b": "l1"}));
            run(&mut s, "sketch.constraint.equal", json!({"a": "r2", "b": "l2"}));
        }
    }
    let id = s.active_sketch.expect("sketch");
    let sk0 = sketch(&s);
    assert_eq!(sk0.constraints.iter().any(|c| c.kind.is_dimension()), matches!(links, Links::Dimensioned));
    let ix = |r: &str| sk0.resolve_point(r).unwrap_or_else(|| panic!("no point {r}"));
    let (p1, p2, p3) = (ix("l1.start"), ix("l1.end"), ix("l2.end"));
    let p2b = ix("l2.start");
    let side = |sk: &Sketch| {
        let (x1, x2, x3) = (sk.points[p1].pos, sk.points[p2].pos, sk.points[p3].pos);
        (x3 - x1).cross(x2 - x1).signum()
    };
    let mut seed = 11u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let (mut kept, mut flips, mut short) = (0, 0, 0);
    while kept < 1000 {
        let th1 = rnd() * TAU;
        let bend = (0.4 + rnd() * 2.2) * if rnd() < 0.5 { 1.0 } else { -1.0 };
        let v2 = o + v(a * th1.cos(), a * th1.sin());
        let v3 = v2 + v(b * (th1 + bend).cos(), b * (th1 + bend).sin());
        let (tr, tt) = (lo + rnd() * (hi - lo), rnd() * TAU);
        let to = o + v(tr * tt.cos(), tr * tt.sin());
        let clear = |p: solidcraft_geom::Vec2| (lo..=hi).contains(&p.dist(o));
        if !(0..=100).all(|i| clear(v3 + (to - v3) * (i as f64 / 100.0))) {
            continue;
        }
        kept += 1;
        {
            let sk = s.doc_mut().sketch_mut(id).expect("sketch");
            for (p, at) in [(p1, o), (p2, v2), (p2b, v2), (p3, v3)] {
                sk.points[p].pos = at;
            }
        }
        let before = side(&sketch(&s));
        drag(&mut s, "l2.end", [v3.x, v3.y], [to.x, to.y]);
        let sk = sketch(&s);
        if side(&sk) != before {
            flips += 1;
        }
        if sk.points[p3].pos.dist(to) > 1e-6 {
            short += 1;
        }
    }
    (flips, short)
}

#[test]
fn a_dragged_two_link_arm_keeps_its_elbow_side() {
    for links in [Links::Dimensioned, Links::EqualToFixed] {
        let (flips, short) = arm_drags(links);
        eprintln!("two-link arm ({links:?}), one-call drags: {flips}/1000 elbow flips, {short} stopped short");
        assert_eq!(flips, 0, "{links:?}: elbow flipped in {flips}/1000 drags");
        assert_eq!(short, 0, "{links:?}: {short}/1000 drags stopped short of the cursor");
    }
}

/// Each step of a drag is seeded on the positions from before it, so even a drag made in one
/// step (one held solve from the grab to the cursor) keeps the elbow of an arm whose links are
/// dimensioned or held equal to fixed lines (the seed reads both).
#[test]
fn a_single_step_arm_drag_keeps_its_elbow_side() {
    for links in [Links::Dimensioned, Links::EqualToFixed] {
        let (flips, short) = super::drag::step_cap::with(1, || arm_drags(links));
        eprintln!("two-link arm ({links:?}), one-step drags: {flips}/1000 elbow flips, {short} stopped short");
        assert_eq!(flips, 0, "{links:?}: elbow flipped in {flips}/1000 one-step drags");
        assert_eq!(short, 0, "{links:?}: {short}/1000 one-step drags stopped short of the cursor");
    }
}

/// Two-link arm (shoulder fixed, |l1| = 30, |l2| = 20) with a thin triangle (sides 3.9, 2, 2)
/// hanging off the hand: pinned there at one point, free to turn about it. The hand is dragged
/// in one `sketch.drag` call along a straight path clear of the arm's singular poses, `n`
/// deterministic cases with the triangle at random turns and on either side. Returns the drags
/// that turned the triangle over.
fn hanging_triangle_flips(n: usize) -> usize {
    use std::f64::consts::TAU;
    let (a, b) = (30.0, 20.0);
    let (lo, hi) = ((a - b) + 3.0, a + b - 3.0);
    let o = v(200.0, 100.0);
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[o.x, o.y], [o.x + a, o.y], [o.x + a, o.y + b]], "ids": ["l1", "l2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": a}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": b}));
    let hand = pt(&s, "l2.end");
    let (q, r) = (hand + v(3.9, 0.0), hand + v(1.95, (4.0f64 - 1.95 * 1.95).sqrt()));
    run(&mut s, "sketch.line", json!({"points": [[hand.x, hand.y], [q.x, q.y], [r.x, r.y], [hand.x, hand.y]], "ids": ["t1", "t2", "t3"]}));
    for (l, value) in [("t1", 3.9), ("t2", 2.0), ("t3", 2.0)] {
        run(&mut s, "sketch.dimension", json!({"entities": [l], "value": value}));
    }
    let id = s.active_sketch.expect("sketch");
    let sk0 = sketch(&s);
    let ix = |r: &str| sk0.resolve_point(r).unwrap_or_else(|| panic!("no point {r}"));
    let (p1, p2, p2b, p3) = (ix("l1.start"), ix("l1.end"), ix("l2.start"), ix("l2.end"));
    let (t1a, t1b, t2a, t2b, t3a, t3b) = (ix("t1.start"), ix("t1.end"), ix("t2.start"), ix("t2.end"), ix("t3.start"), ix("t3.end"));
    let side = |s: &Session| (pt(s, "t1.end") - pt(s, "l2.end")).cross(pt(s, "t2.end") - pt(s, "l2.end")).signum();
    let mut seed = 23u64;
    let mut rnd = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let (mut kept, mut flips) = (0, 0);
    while kept < n {
        let th1 = rnd() * TAU;
        let bend = (0.4 + rnd() * 2.2) * if rnd() < 0.5 { 1.0 } else { -1.0 };
        let v2 = o + v(a * th1.cos(), a * th1.sin());
        let v3 = v2 + v(b * (th1 + bend).cos(), b * (th1 + bend).sin());
        let (tr, tt) = (lo + rnd() * (hi - lo), rnd() * TAU);
        let to = o + v(tr * tt.cos(), tr * tt.sin());
        let clear = |p: solidcraft_geom::Vec2| (lo..=hi).contains(&p.dist(o));
        if !(0..=100).all(|i| clear(v3 + (to - v3) * (i as f64 / 100.0))) {
            continue;
        }
        kept += 1;
        let turn = rnd() * TAU;
        let u = solidcraft_geom::Vec2::from_angle(turn);
        let normal = u.perp() * if rnd() < 0.5 { 1.0 } else { -1.0 };
        let (tq, tr) = (v3 + u * 3.9, v3 + u * 1.95 + normal * (4.0f64 - 1.95 * 1.95).sqrt());
        {
            let sk = s.doc_mut().sketch_mut(id).expect("sketch");
            for (p, at) in [(p1, o), (p2, v2), (p2b, v2), (p3, v3), (t1a, v3), (t1b, tq), (t2a, tq), (t2b, tr), (t3a, tr), (t3b, v3)] {
                sk.points[p].pos = at;
            }
        }
        let before = side(&s);
        let r = run(&mut s, "sketch.drag", json!({"entity": "l2.end", "from": [v3.x, v3.y], "to": [to.x, to.y]}));
        assert_eq!(r["sketch"]["solved"], json!(true), "{r}");
        assert!(pt(&s, "l2.end").dist(to) < 1e-6, "{:?} vs {to:?}: {r}", pt(&s, "l2.end"));
        if side(&s) != before {
            flips += 1;
        }
    }
    flips
}

/// A rigid cluster pinned at a single point is placed by its reference pose (turned about the
/// pin, never reflected): a thin triangle hanging off a dragged arm's hand never turns over,
/// however few steps the drag takes.
#[test]
fn a_triangle_hanging_off_a_dragged_point_keeps_its_side() {
    let flips: Vec<(usize, usize)> =
        [1, 2, 4, usize::MAX].into_iter().map(|cap| (cap, super::drag::step_cap::with(cap, || hanging_triangle_flips(100)))).collect();
    eprintln!("thin triangle on a dragged hand, (step cap, turned over /100): {flips:?}");
    assert!(flips.iter().all(|(_, f)| *f == 0), "{flips:?}");
}

/// Two-link arm with both ends fixed: the elbow is pinned to one of two places. Its drags (one
/// step each) to `n` cursors across the shoulder-hand line all fail their held solve and leave
/// the elbow placed there; returns how many of the sketch's own solves that follow put it on
/// the other side. `move_point`: moved by `sketch.move_point` rather than `sketch.drag`.
fn pinned_elbow_flips(n: usize, move_point: bool) -> usize {
    let o = v(200.0, 100.0);
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[o.x, o.y], [o.x + 30.0, o.y + 10.0], [o.x + 40.0, o.y]], "ids": ["l1", "l2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start", "l2.end"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 20}));
    let start = s.doc.clone();
    let side = |s: &Session| (pt(s, "l2.end") - pt(s, "l1.start")).cross(pt(s, "l1.end") - pt(s, "l1.start")).signum();
    let before = side(&s);
    let mut flips = 0;
    for i in 0..n {
        s.doc = start.clone();
        let elbow = pt(&s, "l1.end");
        // Across the line, spread along it.
        let to = o + v(-10.0 + 60.0 * i as f64 / n as f64, -2.0 - 20.0 * ((i * 7) % n) as f64 / n as f64);
        let r = if move_point {
            run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [to.x, to.y]}))
        } else {
            run(&mut s, "sketch.drag", json!({"entity": "l1.end", "from": [elbow.x, elbow.y], "to": [to.x, to.y]}))
        };
        assert_eq!(r["sketch"]["solved"], json!(true), "{r}");
        if side(&s) != before {
            flips += 1;
        }
    }
    flips
}

/// A drag whose held solve fails leaves the step placed for the sketch's own solve, which keeps
/// the orientation of the last solved state: an elbow pinned by both ends of its arm, dragged
/// across the shoulder-hand line in one step, stays on its side.
#[test]
fn a_failed_drag_step_keeps_the_last_solved_orientation() {
    for move_point in [false, true] {
        let flips = super::drag::step_cap::with(1, || pinned_elbow_flips(50, move_point));
        eprintln!("pinned elbow, one-step moves across the line (move_point {move_point}): {flips}/50 flips");
        assert_eq!(flips, 0, "move_point {move_point}");
    }
}

/// After a failed step the drag follows as far as the geometry can, and `reached` says how far:
/// an arm's hand (reach 50) dragged in one step from 49 to 50.5 from its shoulder gets about
/// two thirds of the way.
#[test]
fn a_drag_follows_up_to_where_its_step_fails() {
    let o = v(200.0, 100.0);
    let mut s = new_sketch();
    // Elbow bent so the hand is 49 from the shoulder, along +x.
    let c = (30.0f64 * 30.0 + 49.0 * 49.0 - 20.0 * 20.0) / (2.0 * 30.0 * 49.0);
    let elbow = o + v(30.0 * c, 30.0 * (1.0 - c * c).sqrt());
    run(&mut s, "sketch.line", json!({"points": [[o.x, o.y], [elbow.x, elbow.y], [o.x + 49.0, o.y]], "ids": ["l1", "l2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 20}));
    assert_eq!(drag_steps(&s, "l2.end", [o.x + 49.0, o.y], [o.x + 50.5, o.y]), 1);
    let r = run(&mut s, "sketch.drag", json!({"entity": "l2.end", "from": [o.x + 49.0, o.y], "to": [o.x + 50.5, o.y]}));
    let reached = r["reached"].as_f64().unwrap_or(f64::NAN);
    eprintln!("arm hand past its reach in one step: {r}");
    assert!((1.0 / 1.5 - 1.0 / 16.0..=1.0 / 1.5).contains(&reached), "{r}");
    assert_eq!(r["sketch"]["solved"], json!(true), "{r}");
}

/// The test-only step cap limits the steps a drag takes, not the plan tests read, and is
/// restored when the capped code panics.
#[test]
fn the_step_cap_leaves_the_plan_and_is_restored() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[100, 0], [120, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 20}));
    let uncapped = super::drag::step_cap::get();
    let plan = drag_steps(&s, "l1.end", [120.0, 0.0], [100.0, 20.0]);
    assert!(plan > 1);
    assert_eq!(super::drag::step_cap::with(1, || drag_steps(&s, "l1.end", [120.0, 0.0], [100.0, 20.0])), plan);
    assert_eq!(super::drag::step_cap::get(), uncapped);
    assert!(std::panic::catch_unwind(|| super::drag::step_cap::with(1, || panic!("capped"))).is_err());
    assert_eq!(super::drag::step_cap::get(), uncapped);
}

/// Stored (counter-clockwise) sweep of an arc, degrees in [0, 360).
fn sweep_deg(s: &Session, arc: &str) -> f64 {
    let sk = sketch(s);
    let Some(solidcraft_sketch::CurveKind::Arc { c, a, b }) = sk.curve_index(arc).map(|i| sk.curves[i].kind.clone()) else { panic!("no arc {arc}") };
    let (c, a, b) = (sk.points[c].pos, sk.points[a].pos, sk.points[b].pos);
    ((b - c).angle() - (a - c).angle()).rem_euclid(std::f64::consts::TAU).to_degrees()
}

/// A free arc of radius 10 about a fixed centre (50, 50), from angle 0 sweeping `sweep`°.
fn free_arc(sweep: f64) -> Session {
    let mut s = new_sketch();
    run(&mut s, "sketch.arc.center_point", json!({"center": [50, 50], "start": [60, 50], "sweep": sweep, "id": "a1"}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["a1.center"]}));
    s
}

fn on_arc(deg: f64) -> [f64; 2] {
    let t = deg.to_radians();
    [50.0 + 10.0 * t.cos(), 50.0 + 10.0 * t.sin()]
}

/// An arc's end dragged through its start never wraps the sweep (15° → 0° → 345°, or
/// 330° → 360° → 30°): the arc stops short, in one call or in many.
#[test]
fn a_dragged_arc_end_never_wraps_through_its_start() {
    // One call, clockwise through the start: 90° → -60°.
    let mut s = free_arc(90.0);
    drag(&mut s, "a1.end", on_arc(90.0), on_arc(-60.0));
    let sw = sweep_deg(&s, "a1");
    eprintln!("free arc, CW one call 90° → -60°: sweep {sw:.1}°");
    assert!(sw > 0.0 && sw < 30.0, "CW one call: sweep {sw}°");
    // One call, counter-clockwise past a full turn: 330° → 390°.
    let mut s = free_arc(330.0);
    drag(&mut s, "a1.end", on_arc(330.0), on_arc(390.0));
    let sw = sweep_deg(&s, "a1");
    eprintln!("free arc, CCW one call 330° → 390°: sweep {sw:.1}°");
    assert!(sw > 330.0 && sw < 360.0, "CCW one call: sweep {sw}°");
    // 30° calls, clockwise: 90 → -60.
    let mut s = free_arc(90.0);
    let mut last = 90.0;
    let mut log = Vec::new();
    for deg in [60.0, 30.0, 0.0, -30.0, -60.0] {
        drag(&mut s, "a1.end", on_arc(last), on_arc(deg));
        last = deg;
        log.push(sweep_deg(&s, "a1"));
    }
    eprintln!("free arc, CW 30° calls: sweeps {log:.1?}");
    assert!(log.windows(2).all(|w| w[1] <= w[0] + 1e-9) && log.iter().all(|&x| x > 0.0), "CW calls: {log:?}");
    // 30° calls, counter-clockwise: 90 → 450.
    let mut s = free_arc(90.0);
    let mut last = 90.0;
    let mut log = Vec::new();
    for deg in (4..=15).map(|i| (i * 30) as f64) {
        drag(&mut s, "a1.end", on_arc(last), on_arc(deg));
        last = deg;
        log.push(sweep_deg(&s, "a1"));
    }
    eprintln!("free arc, CCW 30° calls: sweeps {log:.1?}");
    assert!(log.windows(2).all(|w| w[1] >= w[0] - 1e-9) && log.iter().all(|&x| x < 360.0), "CCW calls: {log:?}");
    assert!(log.last().is_some_and(|&x| x > 330.0), "CCW calls: {log:?}");
}

/// A fillet (radius 5, tangent to both legs) whose second leg is dragged round the corner
/// 90° → 174° → 14° → 90° in 2° calls stays inside the corner; with a 40 mm leg the leg turns
/// over near 14° (the tangent length passes 40) and comes back on the way up.
#[test]
fn a_dragged_fillet_stays_in_its_corner() {
    for (leg, strict) in [(80.0, true), (40.0, false)] {
        let mut s = new_sketch();
        let o = v(100.0, 100.0);
        run(&mut s, "sketch.line", json!({"points": [[o.x + 40.0, o.y], [o.x, o.y], [o.x, o.y + leg]], "ids": ["l1", "l2"]}));
        run(&mut s, "sketch.fillet", json!({"point": "l1.end", "radius": 5}));
        run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
        run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1", "mode": "horizontal"}));
        let sk0 = sketch(&s);
        let arc = sk0.curves.iter().position(|c| matches!(c.kind, solidcraft_sketch::CurveKind::Arc { .. })).expect("fillet arc");
        let arc_id = sk0.curves[arc].id.clone();
        let inside = |s: &Session| {
            let sk = sketch(s);
            let side = |p: solidcraft_geom::Vec2, a: solidcraft_geom::Vec2, b: solidcraft_geom::Vec2| (b - a).cross(p - a).signum();
            let c = sk.center(arc).expect("centre");
            let (p0, k1, k2, p3) = (pt(s, "l1.start"), pt(s, "l1.end"), pt(s, "l2.start"), pt(s, "l2.end"));
            let r = sk.radius(arc).unwrap_or(0.0);
            side(c, p0, k1) == side(p3, p0, k1) && side(c, k2, p3) == side(p0, k2, p3) && (r - 5.0).abs() < 1e-6
        };
        assert!(inside(&s));
        let mut path: Vec<f64> = (45..=87).map(|i| (i * 2) as f64).collect();
        path.extend((7..=87).rev().map(|i| (i * 2) as f64));
        path.extend((8..=45).map(|i| (i * 2) as f64));
        let corner = o;
        let at = |deg: f64| {
            let t = deg.to_radians();
            [corner.x + leg * t.cos(), corner.y + leg * t.sin()]
        };
        let mut bad = Vec::new();
        let mut last = at(90.0);
        for deg in path {
            let to = at(deg);
            drag(&mut s, "l2.end", last, to);
            last = to;
            let sw = sweep_deg(&s, &arc_id);
            if !(inside(&s) && sw < 180.0) {
                bad.push(deg);
            }
        }
        if strict {
            assert!(bad.is_empty(), "leg {leg}: bad states at {bad:?}");
        }
        assert!(inside(&s) && sweep_deg(&s, &arc_id) < 180.0, "leg {leg}: did not recover (bad at {bad:?})");
    }
}

/// Hostile `from` / `to` are refused and change nothing; a zero-length drag changes nothing.
#[test]
fn hostile_drags_change_nothing() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [40, 20]]}));
    let l0 = snap(&s, "l1");
    for bad in [json!(f64::NAN), json!(f64::INFINITY), json!(1e300), json!(-1e300), json!("x"), json!([1.0]), json!(null)] {
        for (from, to) in [(json!([10, 10]), json!([bad.clone(), 0])), (json!([bad.clone(), 0]), json!([10, 10]))] {
            for e in ["l1", "l1.end"] {
                let r = s.execute("sketch.drag", &json!({"entity": e, "from": from.clone(), "to": to.clone()}));
                assert!(r.is_err(), "{e} {from} {to}: {r:?}");
                assert!(same(&snap(&s, "l1"), &l0));
            }
        }
        assert!(s.execute("sketch.move_point", &json!({"point": "l1.end", "to": [bad.clone(), 0]})).is_err());
        assert!(same(&snap(&s, "l1"), &l0));
    }
    drag(&mut s, "l1", [20.0, 10.0], [20.0, 10.0]);
    drag(&mut s, "l1.end", [40.0, 20.0], [40.0, 20.0]);
    run(&mut s, "sketch.move_point", json!({"point": "l1.end", "to": [40, 20]}));
    assert!(same(&snap(&s, "l1"), &l0));
    // A huge (but finite, in-range) jump on a constrained group is capped and never panics.
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    assert_eq!(drag_steps(&s, "l1.end", [40.0, 20.0], [9.0e8, -9.0e8]), 64);
    drag(&mut s, "l1.end", [40.0, 20.0], [9.0e8, -9.0e8]);
    let (a, b) = (pt(&s, "l1.start"), pt(&s, "l1.end"));
    assert!(a.is_finite() && b.is_finite(), "{a:?} {b:?}");
}

/// The solves a drag of `entity` from `from` to `to` would take.
fn drag_steps(s: &Session, entity: &str, from: [f64; 2], to: [f64; 2]) -> usize {
    let sk = sketch(s);
    let e = super::ent(&sk, entity).unwrap_or_else(|| panic!("no entity {entity}"));
    super::drag::drag_steps(&sk, e, v(from[0], from[1]), v(to[0], to[1]))
}

/// Continuation costs nothing on a normal mouse frame: a short move next to the geometry is one
/// held solve, as it was before; only a long one is split.
#[test]
fn a_short_drag_is_a_single_solve() {
    let mut s = new_sketch();
    // Ten dimensioned 20 × 10 rectangles, free to move (clear of the origin).
    for i in 0..10 {
        let x = 100.0 + 30.0 * i as f64;
        let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [x, 0], "p1": [x + 20.0, 10]}));
        let c = curve_ids(&r);
        run(&mut s, "sketch.dimension", json!({"entities": [c[0]], "value": 20}));
        run(&mut s, "sketch.dimension", json!({"entities": [c[1]], "value": 10}));
    }
    assert_eq!(drag_steps(&s, "l1.end", [120.0, 0.0], [120.5, 0.3]), 1);
    assert_eq!(drag_steps(&s, "l1", [110.0, 0.0], [110.5, 0.5]), 1);
    drag(&mut s, "l1.end", [120.0, 0.0], [120.5, 0.3]);
    assert!(pt(&s, "l1.end").dist(v(120.5, 0.3)) < 1e-9);
    // A long move (more than an eighth of the 10 mm side) is split into steps.
    assert!(drag_steps(&s, "l1.end", [120.5, 0.3], [140.0, -20.0]) > 1);
    drag(&mut s, "l1.end", [120.5, 0.3], [140.0, -20.0]);
    assert!(pt(&s, "l1.end").dist(v(140.0, -20.0)) < 1e-9);
}

/// The drag group stops at fixed points, as the solve does: a tiny dimension on geometry that
/// only shares a fixed point with the dragged geometry does not shrink its steps.
#[test]
fn the_drag_step_ignores_geometry_beyond_a_fixed_point() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[100, 100], [120, 100]], "ids": ["la"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["la.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": 20}));
    let before = drag_steps(&s, "la.end", [120.0, 100.0], [120.0, 101.0]);
    run(&mut s, "sketch.line", json!({"points": [[100, 100], [100.01, 100]], "ids": ["lb"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["lb"], "value": 0.01}));
    assert_eq!(drag_steps(&s, "la.end", [120.0, 100.0], [120.0, 101.0]), before);
    assert_eq!(before, 1);
    // A free radius couples like a point: a line tangent to a circle whose centre is fixed
    // steps by the circle's radius (r 2: steps of 0.5 mm), its only link to the line.
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [100, 100], "radius": 2, "connect": false}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["c1.center"]}));
    run(&mut s, "sketch.line", json!({"points": [[90, 102], [110, 102]], "ids": ["lt"], "connect": false}));
    run(&mut s, "sketch.constraint.tangent", json!({"a": "c1", "b": "lt"}));
    assert_eq!(drag_steps(&s, "lt.end", [110.0, 102.0], [110.0, 105.0]), 6);
}

/// The step counts every size the constraints hold, offsets along an axis too (the solver holds
/// their size on either side), except a length between two points the drag holds: a dimensioned
/// line slid by its middle keeps its length whatever the step.
#[test]
fn the_drag_step_counts_every_held_size_but_the_dragged_length() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[100, 100], [100.5, 100]], "ids": ["la"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": 0.5}));
    let steps = drag_steps(&s, "la", [100.25, 100.0], [105.25, 100.0]);
    eprintln!("0.5 mm line slid 5 mm: {steps} steps");
    assert_eq!(steps, 1);
    // B 0.05 mm right of A (horizontal dimension); A dragged 1 mm right in one call: B stays
    // right of A instead of jumping to the other side.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[100, 100], [100, 110]], "ids": ["la"]}));
    run(&mut s, "sketch.line", json!({"points": [[100.05, 120], [110, 120]], "ids": ["lb"], "connect": false}));
    run(&mut s, "sketch.dimension", json!({"entities": ["la.start", "lb.start"], "type": "horizontal", "value": 0.05}));
    assert!(sketch(&s).constraints.iter().any(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::DistanceX { .. })));
    let steps = drag_steps(&s, "la.start", [100.0, 100.0], [101.0, 100.0]);
    drag(&mut s, "la.start", [100.0, 100.0], [101.0, 100.0]);
    let (a, b) = (pt(&s, "la.start"), pt(&s, "lb.start"));
    eprintln!("1 mm drag with a 0.05 mm DistanceX: {steps} steps; A {a:?} B {b:?}");
    assert!(steps > 1);
    assert!(a.dist(v(101.0, 100.0)) < 1e-9 && (b.x - a.x - 0.05).abs() < 1e-6, "B went to the other side: {a:?} {b:?}");
}

/// A circle shares only its centre with a dragged line's end (coincident): the solve couples
/// its centre, never its radius, so a tiny circle does not shrink the step.
#[test]
fn a_circle_centred_on_a_dragged_end_does_not_set_the_step() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[100, 100], [120, 100]], "ids": ["la"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["la.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["la"], "value": 20}));
    run(&mut s, "sketch.circle.center", json!({"center": [120, 100], "radius": 0.01}));
    assert!(sketch(&s).constraints.iter().any(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Coincident { .. })));
    let steps = drag_steps(&s, "la.end", [120.0, 100.0], [120.0, 101.0]);
    eprintln!("1 mm drag next to a 0.01 mm circle on the end: {steps} steps");
    assert_eq!(steps, 1);
}

/// A one-call drag whose straight path crosses where the geometry cannot reach (an arm's hand
/// through the hole round its shoulder) still lands on a cursor it can reach.
#[test]
fn a_drag_across_an_unreachable_middle_still_reaches_the_cursor() {
    let mut s = new_sketch();
    let o = v(200.0, 100.0);
    run(&mut s, "sketch.line", json!({"points": [[o.x, o.y], [o.x + 30.0, o.y], [o.x + 30.0, o.y + 20.0]], "ids": ["l1", "l2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 20}));
    // From (30, 20) to (-30, -20) about the shoulder: the path passes through it.
    let to = [o.x - 30.0, o.y - 20.0];
    let steps = drag_steps(&s, "l2.end", [o.x + 30.0, o.y + 20.0], to);
    let r = run(&mut s, "sketch.drag", json!({"entity": "l2.end", "from": [o.x + 30.0, o.y + 20.0], "to": to}));
    eprintln!("arm hand across the hole: {steps} steps, {r}");
    assert!(steps > 1);
    assert!(pt(&s, "l2.end").dist(v(to[0], to[1])) < 1e-6, "{:?}", pt(&s, "l2.end"));
    assert_eq!(r["reached"], json!(1.0));
}

/// Past the unreachable stretch round its shoulder, an arm's drag keeps stepping (no one jump to
/// the cursor), so a thin triangle hanging off the hand (it can turn over) keeps its side.
#[test]
fn a_drag_past_an_unreachable_stretch_keeps_stepping() {
    let mut s = new_sketch();
    let o = v(200.0, 100.0);
    let elbow = o + v(19.702247520273062, 22.62347105662377);
    let hand = o + v(39.654744430607906, 21.245840103473398);
    let to = o + v(-28.82686238241851, -31.24580379872583);
    run(&mut s, "sketch.line", json!({"points": [[o.x, o.y], [elbow.x, elbow.y], [hand.x, hand.y]], "ids": ["l1", "l2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 20}));
    // A triangle with sides 3.9, 2 and 2 on the hand.
    let hand = pt(&s, "l2.end");
    let (q, r) = (hand + v(3.9, 0.0), hand + v(1.95, (4.0f64 - 1.95 * 1.95).sqrt()));
    run(&mut s, "sketch.line", json!({"points": [[hand.x, hand.y], [q.x, q.y], [r.x, r.y], [hand.x, hand.y]], "ids": ["t1", "t2", "t3"]}));
    for (l, value) in [("t1", 3.9), ("t2", 2.0), ("t3", 2.0)] {
        run(&mut s, "sketch.dimension", json!({"entities": [l], "value": value}));
    }
    let side = |s: &Session| (pt(s, "t1.end") - pt(s, "l2.end")).cross(pt(s, "t2.end") - pt(s, "l2.end")).signum();
    let before = side(&s);
    let r = run(&mut s, "sketch.drag", json!({"entity": "l2.end", "from": [hand.x, hand.y], "to": [to.x, to.y]}));
    eprintln!("arm with a thin triangle past the hole: {r}, side {before} -> {}", side(&s));
    assert!(pt(&s, "l2.end").dist(to) < 1e-6, "{:?}", pt(&s, "l2.end"));
    assert_eq!(side(&s), before, "the triangle turned over");
    assert_eq!(r["reached"], json!(1.0));
}

/// The sweep guard stops an arc's end crossing its start, never mere closeness: an arc at
/// 359.99999° or 1e-7 rad still lets its own end and geometry joined to it be dragged.
#[test]
fn arcs_near_a_full_turn_or_nothing_can_still_be_dragged() {
    // (sweep, a nudge of the end towards the inside of the sweep that keeps it within 1e-6 rad
    // of the limit).
    for (sweep, nudge) in [(359.99999, -5e-7), (1e-7_f64.to_degrees(), 5e-7)] {
        let mut s = free_arc(sweep);
        // A dimensioned line hangs off the arc's end.
        let end = pt(&s, "a1.end");
        run(&mut s, "sketch.line", json!({"points": [[end.x, end.y], [end.x + 10.0, end.y + 10.0]], "ids": ["lc"], "connect": false}));
        run(&mut s, "sketch.constraint.coincident", json!({"a": "a1.end", "b": "lc.start"}));
        run(&mut s, "sketch.dimension", json!({"entities": ["lc"], "value": 200f64.sqrt()}));
        let sw0 = sweep_deg(&s, "a1");
        // The arc's own end.
        let t = (end - v(50.0, 50.0)).angle() + nudge;
        let to = [50.0 + 10.0 * t.cos(), 50.0 + 10.0 * t.sin()];
        drag(&mut s, "a1.end", [end.x, end.y], to);
        assert!(pt(&s, "a1.end").dist(v(to[0], to[1])) < 1e-9, "sweep {sweep}°: the arc's end did not move");
        // The joined line's far end, which pulls the arc's end along (inwards).
        let far = pt(&s, "lc.end");
        let target = v(far.x, far.y + 1e-5 * nudge.signum());
        drag(&mut s, "lc.end", [far.x, far.y], [target.x, target.y]);
        assert!(pt(&s, "lc.end").dist(target) < 1e-9, "sweep {sweep}°: the joined line did not move");
        assert!((sweep_deg(&s, "a1") - sw0).abs() < 1.0, "sweep {sweep}° became {}°", sweep_deg(&s, "a1"));
    }
}

#[test]
fn fully_constrained_feedback_is_per_entity() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [50, 35]}));
    run(&mut s, "sketch.circle.center", json!({"center": [80, 20], "radius": 8}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 25}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l1.start"], "type": "horizontal", "value": 10}));
    run(&mut s, "sketch.dimension", json!({"entities": ["origin", "l1.start"], "type": "vertical", "value": 10}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 3);
    for c in si["curves"].as_array().expect("curves") {
        assert_eq!(c["fully_constrained"], c["type"] == "line", "{c}");
    }
    // Colours follow: fully constrained lines in the fixed colour, the free circle in blue.
    let sk = sketch(&s);
    let st = s.model.state();
    let ss = st.sketch(s.active_sketch.expect("sketch")).expect("solved");
    let lines = crate::view::sketch_lines(&sk, &ss.plane, true, &ss.report.curve_determined);
    assert!(lines.iter().filter(|l| l.1 == crate::view::colors::SKETCH_FIXED).count() >= 4);
    assert!(lines.iter().any(|l| l.1 == crate::view::colors::SKETCH));
}

// ---------------------------------------------------------------------------------------------
// Selection inside sketches

#[test]
fn chain_follows_shared_and_coincident_ends() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.line", json!({"points": [[40, 0], [50, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[50.5, 0], [60, 5]]}));
    run(&mut s, "sketch.circle.center", json!({"center": [80, 0], "radius": 3}));
    let chain = |s: &mut Session, c: &str| -> Vec<String> {
        let v = run(s, "sketch.chain", json!({"curve": c}));
        v["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    };
    assert_eq!(chain(&mut s, "l3"), ["l3", "l1", "l2", "l4"]);
    assert_eq!(chain(&mut s, "l5"), ["l5"]);
    assert_eq!(chain(&mut s, "c1"), ["c1"]);
    // Joined by a coincident constraint, the two lines are one chain.
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l5.end", "b": "l6.start"}));
    assert_eq!(chain(&mut s, "l6"), ["l6", "l5"]);
    assert!(s.execute("sketch.chain", &json!({"curve": "nope"})).is_err());
}

#[test]
fn constraints_and_dimensions_can_be_selected_and_deleted_together() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0]], "connect": false}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
    let ids: Vec<String> = sketch(&s).constraints.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids.len(), 2);
    let items: Vec<Value> = ids.iter().map(|id| json!({"type": "sketch_constraint", "id": id})).collect();
    run(&mut s, "select.set", json!({ "items": items }));
    assert_eq!(s.selection.len(), 2);
    run(&mut s, "selection.delete", json!({"items": items}));
    assert!(sketch(&s).constraints.is_empty());
}

// ---------------------------------------------------------------------------------------------
// 3D sketch

#[test]
fn three_d_curves_need_the_option_and_move_joined() {
    let mut s = new_sketch();
    assert!(s.execute("sketch.line3d", &json!({"points": [[0, 0, 0], [0, 0, 10]]})).is_err());
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let r = run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 10], [10, 0, 10]]}));
    let ids = curve_ids(&r);
    assert_eq!(ids.len(), 2);
    run(&mut s, "sketch.point3d", json!({"point": [5, 5, 5]}));
    let sp = run(&mut s, "sketch.spline3d", json!({"points": [[10, 0, 10], [15, 5, 12], [20, 0, 20]]}));
    let sid = curve_ids(&sp)[0].clone();
    let sk = sketch(&s);
    let w = &sk.wires[sk.wire_index(&sid).expect("spline")];
    assert!(w.pts.len() > 10);
    assert!(w.pts.first().expect("p").dist(solidcraft_geom::Vec3::new(10.0, 0.0, 10.0)) < 1e-12);
    assert!(w.pts.last().expect("p").dist(solidcraft_geom::Vec3::new(20.0, 0.0, 20.0)) < 1e-12);
    // The shared corner moves for both lines (and the spline that starts there).
    let r = run(&mut s, "sketch.move3d", json!({"wire": ids[1], "index": 1, "by": [0, 0, 5]}));
    assert_eq!(r["moved"], 2);
    let r = run(&mut s, "sketch.move3d", json!({"wire": ids[0], "index": 1, "to": [0, 3, 12]}));
    assert_eq!(r["moved"], 2);
    let sk = sketch(&s);
    let fit = |id: &str| sk.wires[sk.wire_index(id).expect("w")].fit.clone();
    assert_eq!(fit(&ids[0])[1], fit(&ids[1])[0]);
    assert!(s.execute("sketch.move3d", &json!({"wire": "nope", "to": [0, 0, 0]})).is_err());
    assert!(s.execute("sketch.line3d", &json!({"points": [[0, 0, 0], [0, 0, 0]]})).is_err());
    assert!(s.execute("sketch.line3d", &json!({"points": [[0, 0, 0], [f64::NAN, 0, 0]]})).is_err());
    // Deleting works like any sketch entity.
    run(&mut s, "sketch.delete", json!({"entities": [sid]}));
    assert!(sketch(&s).wire_index(&curve_ids(&sp)[0]).is_none());
}

#[test]
fn pipe_and_sweep_follow_3d_sketch_curves() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let r = run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 30], [20, 0, 30]]}));
    let mut path = curve_ids(&r);
    let sp = run(&mut s, "sketch.spline3d", json!({"points": [[20, 0, 30], [35, 10, 35], [50, 0, 50]]}));
    path.extend(curve_ids(&sp));
    run(&mut s, "sketch.finish", json!({}));
    let p = run(&mut s, "solid.pipe", json!({"path_sketch": "Path", "path": path, "diameter": 4}));
    let si = run(&mut s, "document.inspect", json!({"measure": true}));
    let body = si["bodies"].as_array().and_then(|b| b.first()).cloned().unwrap_or_default();
    // The tube runs from the first line's start to the spline's end (radius 2 around them; the
    // end cap is square to the spline, so it reaches up to 2 past the end point).
    let (lo, hi) = (&body["bbox"]["min"], &body["bbox"]["max"]);
    let near = |v: &Value, x: f64| (v.as_f64().unwrap_or(f64::NAN) - x).abs() < 0.5;
    let within = |v: &Value, a: f64, b: f64| v.as_f64().is_some_and(|x| x > a - 0.1 && x < b + 0.1);
    assert!(near(&lo[0], -2.0) && near(&lo[2], 0.0) && within(&hi[0], 50.0, 52.0) && within(&hi[2], 50.0, 52.0), "{p} {body}");
    assert!(body["volume_mm3"].as_f64().unwrap_or(0.0) > 4.0 * std::f64::consts::PI * 50.0 * 0.95, "{p} {body}");
}

/// Between two points, or on a line, that don't line up with an axis, an `auto` dimension takes
/// its orientation from where its text goes relative to the box the two span: inside it the
/// distance between them, above or below it the horizontal distance, beside it the vertical
/// one, in its outer corners the distance again (#257, #7295).
#[test]
fn auto_dimension_orientation_follows_where_its_text_is_placed() {
    use solidcraft_sketch::ConstraintKind;
    let dimension = |start: [f64; 2], end: [f64; 2], entities: Value, extra: Value| {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points": [start, end], "connect": false}));
        let mut p = json!({"entities": entities});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        let r = run(&mut s, "sketch.dimension", p);
        let sk = sketch(&s);
        let kind = sk.constraints.iter().find(|c| c.kind.is_dimension()).map(|c| c.kind.clone()).expect("a dimension");
        (kind, r["measured"].as_f64().unwrap_or(f64::NAN), sk)
    };
    let points = json!(["l1.start", "l1.end"]);
    // A 30 × 40 line (50 long), and a shallow one, 50 × 10, whose box is a thin strip: above
    // the middle of it is horizontal, not the distance between its ends.
    let shallow = 2600_f64.sqrt();
    let cases: [([f64; 2], [f64; 2], [f64; 3], &[([f64; 2], &str)]); 2] = [
        (
            [0.0, 0.0],
            [30.0, 40.0],
            [30.0, 40.0, 50.0],
            &[
                ([15.0, 60.0], "horizontal"),
                ([15.0, 41.0], "horizontal"),
                ([15.0, -20.0], "horizontal"),
                ([-30.0, 5.0], "vertical"),
                ([55.0, 38.0], "vertical"),
                ([-20.0, 25.0], "vertical"),
                ([45.0, 10.0], "vertical"),
                ([20.0, 10.0], "aligned"),
                ([10.0, 30.0], "aligned"),
                ([-20.0, -20.0], "aligned"),
                ([40.0, 50.0], "aligned"),
            ],
        ),
        (
            [10.0, 10.0],
            [60.0, 0.0],
            [50.0, 10.0, shallow],
            &[
                ([35.0, 30.0], "horizontal"),
                ([35.0, -20.0], "horizontal"),
                ([12.0, 40.0], "horizontal"),
                ([80.0, 5.0], "vertical"),
                ([0.0, 3.0], "vertical"),
                ([35.0, 6.0], "aligned"),
                ([80.0, 40.0], "aligned"),
                ([0.0, -10.0], "aligned"),
            ],
        ),
    ];
    for (start, end, [dx, dy, len], placements) in cases {
        for entities in [points.clone(), json!(["l1"])] {
            for &(at, expected) in placements {
                let (kind, measured, _) = dimension(start, end, entities.clone(), json!({"text_at": at}));
                let (got, value) = match kind {
                    ConstraintKind::DistanceX { .. } => ("horizontal", dx),
                    ConstraintKind::DistanceY { .. } => ("vertical", dy),
                    ConstraintKind::Distance { .. } | ConstraintKind::Length { .. } => ("aligned", len),
                    other => panic!("{entities} at {at:?}: {other:?}"),
                };
                assert_eq!(got, expected, "{start:?}–{end:?}: {entities} with its text at {at:?}");
                assert!((measured - value).abs() < 1e-6, "{entities} at {at:?} measured {measured}");
            }
        }
    }
    for entities in [points.clone(), json!(["l1"])] {
        // No text, an explicit type, or two ends already on an axis keep today's dimension.
        let (kind, ..) = dimension([0.0, 0.0], [30.0, 40.0], entities.clone(), json!({}));
        assert!(matches!(kind, ConstraintKind::Distance { .. } | ConstraintKind::Length { .. }), "{kind:?}");
        let (kind, ..) = dimension([0.0, 0.0], [30.0, 40.0], entities.clone(), json!({"type": "distance", "text_at": [15, 60]}));
        assert!(matches!(kind, ConstraintKind::Distance { .. } | ConstraintKind::Length { .. }), "{kind:?}");
        let (kind, ..) = dimension([0.0, 0.0], [30.0, 40.0], entities.clone(), json!({"type": "vertical", "text_at": [15, 60]}));
        assert!(matches!(kind, ConstraintKind::DistanceY { .. }), "{kind:?}");
        let (kind, ..) = dimension([0.0, 0.0], [40.0, 0.0], json!(["l1.start", "l1.end"]), json!({"text_at": [60, 5]}));
        assert!(matches!(kind, ConstraintKind::Distance { .. }), "{kind:?}");
        let (kind, ..) = dimension([0.0, 0.0], [40.0, 0.0], json!(["l1.start", "l1.end"]), json!({"text_at": [20, 15]}));
        assert!(matches!(kind, ConstraintKind::Distance { .. }), "{kind:?}");
    }
    // The chosen dimension drives that coordinate: 24 across X, the 40 in Y kept.
    let (_, _, sk) = dimension([0.0, 0.0], [30.0, 40.0], points, json!({"text_at": [15, 60], "value": 24}));
    let end = sk.point(sk.resolve_point("l1.end").expect("end")).expect("end point");
    let start = sk.point(sk.resolve_point("l1.start").expect("start")).expect("start point");
    assert!(((end.x - start.x).abs() - 24.0).abs() < 1e-6 && ((end.y - start.y).abs() - 40.0).abs() < 1e-6, "{start:?} {end:?}");
}

/// A two-link arm (B, elbow E, hand H) hanging off a rigid triangle A B C that only the cluster
/// decomposition places: pinned at F by two of its corners and held at G by the third. Every
/// line is dimensioned. Without decomposition budget the elbow's seed has nothing to build on.
fn hung_arm() -> Session {
    let mut s = new_sketch();
    let o = v(200.0, 100.0);
    let at = |x: f64, y: f64| json!([o.x + x, o.y + y]);
    run(&mut s, "sketch.line", json!({"points": [at(0.0, 0.0), at(5.0, 15.0), at(20.0, 25.0), at(15.0, 8.0)], "ids": ["fa", "ab", "bc"]}));
    run(&mut s, "sketch.line", json!({"points": ["bc.end", "ab.start"], "ids": ["ca"]}));
    run(&mut s, "sketch.line", json!({"points": ["fa.start", "bc.end"], "ids": ["fc"]}));
    run(&mut s, "sketch.line", json!({"points": [at(40.0, 0.0), "ab.end"], "ids": ["gb"]}));
    run(&mut s, "sketch.line", json!({"points": ["ab.end", at(35.0, 40.0), at(50.0, 30.0)], "ids": ["be", "eh"]}));
    for id in ["fa", "ab", "bc", "ca", "fc", "gb", "be", "eh"] {
        run(&mut s, "sketch.dimension", json!({"entities": [id]}));
    }
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["fa.start", "gb.start"]}));
    s
}

/// The elbow's side of the shoulder–hand line.
fn hung_elbow_side(s: &Session) -> f64 {
    let b = pt(s, "ab.end");
    (pt(s, "eh.end") - b).cross(pt(s, "be.end") - b).signum()
}

/// The hand of `hung_arm` swung by `angle` about the shoulder.
fn hung_hand_target(s: &Session, angle: f64, scale: f64) -> solidcraft_geom::Vec2 {
    let (b, h) = (pt(s, "ab.end"), pt(s, "eh.end"));
    let r = h - b;
    let (c, si) = (angle.cos(), angle.sin());
    b + v(r.x * c - r.y * si, r.x * si + r.y * c) * scale
}

/// A live drag whose seed runs out of decomposition budget takes the rest of the way in short
/// steps (continuation keeps the elbow the seed no longer places).
#[test]
fn a_drag_out_of_decomposition_budget_takes_short_steps() {
    let start = hung_arm();
    let before = hung_elbow_side(&start);
    let h = pt(&start, "eh.end");
    let mut flips = 0;
    for k in [-3.0, -2.0, -1.0, 1.0, 2.0, 3.0] {
        let mut s = hung_arm();
        let to = hung_hand_target(&s, k * 0.7, 1.0);
        let planned = drag_steps(&s, "eh.end", [h.x, h.y], [to.x, to.y]);
        let r = super::drag::last_drag::with_frame_work(0, || {
            run(&mut s, "sketch.drag", json!({"entity": "eh.end", "from": [h.x, h.y], "to": [to.x, to.y], "live": true}))
        });
        let d = super::drag::last_drag::get();
        assert!(d.cut, "{d:?}");
        assert!(d.count > planned, "{planned} steps planned, {d:?}: the rest in short steps");
        assert!(pt(&s, "eh.end").dist(to) < 1e-6, "{r}");
        if hung_elbow_side(&s) != before {
            flips += 1;
        }
    }
    assert_eq!(flips, 0);
}

/// A live frame whose numerical solves have done their share takes no more held solves. Before,
/// a large sketch's frame could run 64 failing held solves of 150 ms each (10 s), where the
/// geometry couldn't follow. Progress so far stays and asks for another frame (`retry`).
#[test]
fn a_live_frame_stops_once_its_numerical_solves_are_spent() {
    let start = hung_arm();
    let h = pt(&start, "eh.end");
    let far = hung_hand_target(&start, 2.0, 1.5);
    let drag = |s: &mut Session| run(s, "sketch.drag", json!({"entity": "eh.end", "from": [h.x, h.y], "to": [far.x, far.y], "live": true}));
    let mut s = hung_arm();
    drag(&mut s);
    let whole = super::drag::last_drag::get();
    let mut s = hung_arm();
    let r = super::drag::last_drag::with_frame_numeric(1, || drag(&mut s));
    let short = super::drag::last_drag::get();
    assert!(short.numeric && short.count < whole.count, "{short:?} against {whole:?}");
    // What it reached stays, and asks for the next frame.
    let reached = r["reached"].as_f64().unwrap_or(0.0);
    assert!(reached > 0.0 && reached < 1.0 && r["retry"] == json!(true), "{r}");
}

/// A drag from a script (not live) past the arm's reach: the steps after a failed one are solved
/// at once and those that fail in a row are taken as tried. The drag ends where solving every
/// step one by one ends it, with the same held solves counted.
#[test]
fn a_scripted_drag_solves_the_steps_past_a_failure_at_once_and_ends_where_one_by_one_does() {
    let start = hung_arm();
    let h = pt(&start, "eh.end");
    let far = hung_hand_target(&start, 2.0, 1.5);
    let drag = |s: &mut Session| run(s, "sketch.drag", json!({"entity": "eh.end", "from": [h.x, h.y], "to": [far.x, far.y]}));
    let points = |s: &Session| s.doc.sketch(s.active_sketch.unwrap()).unwrap().points.iter().map(|p| (p.id.clone(), p.pos)).collect::<Vec<_>>();
    let mut one = hung_arm();
    let r1 = super::drag::last_drag::one_by_one(|| drag(&mut one));
    let solves1 = super::drag::last_drag::get();
    super::drag::last_drag::skipped();
    let mut ahead = hung_arm();
    let r2 = drag(&mut ahead);
    let solves2 = super::drag::last_drag::get();
    assert!(super::drag::last_drag::skipped() > 0, "steps were solved ahead");
    assert_eq!(r2, r1);
    assert_eq!(points(&ahead), points(&one));
    assert_eq!((solves2.count, solves2.work), (solves1.count, solves1.work));
}

/// All the held solves of one live frame, halvings included, share one decomposition budget:
/// together they do at most a frame's work, and one solve's more.
#[test]
fn a_live_drag_shares_one_decomposition_budget() {
    let mut s = hung_arm();
    let h = pt(&s, "eh.end");
    let near = hung_hand_target(&s, 0.01, 1.0);
    run(&mut s, "sketch.drag", json!({"entity": "eh.end", "from": [h.x, h.y], "to": [near.x, near.y], "live": true}));
    let one = super::drag::last_drag::get();
    assert!(one.count == 1 && one.work > 0 && !one.cut, "{one:?}");
    // Swung round and out past the arm's reach: many steps, then halvings.
    let mut s = hung_arm();
    let far = hung_hand_target(&s, 2.0, 1.5);
    let budget = 3 * one.work;
    super::drag::last_drag::with_frame_work(budget, || {
        run(&mut s, "sketch.drag", json!({"entity": "eh.end", "from": [h.x, h.y], "to": [far.x, far.y], "live": true}))
    });
    let d = super::drag::last_drag::get();
    assert!(d.count > 6, "{d:?}");
    assert!(d.work <= budget + one.work, "{d:?} over a budget of {budget}");
}

/// #1365: lines drawn by eye and a construction line through the origin, as in the report.
fn drawn_by_eye() -> Session {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[-56.7, 0], [56.7, 0]], "construction": true, "ids": ["hc"]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "origin", "b": "hc"}));
    run(&mut s, "sketch.line", json!({"points": [[0, -54], [0, 55.2]], "construction": true, "ids": ["vc"]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "origin", "b": "vc"}));
    run(
        &mut s,
        "sketch.line",
        json!({"points": [[0, 26], [29.6, 26], [29.6, 41.2], [44, 41.2], [44, 0]], "infer": true, "ids": ["a", "b", "c", "d"]}),
    );
    run(&mut s, "sketch.constraint.coincident", json!({"a": "a.start", "b": "vc"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "d.end", "b": "hc"}));
    s
}

#[test]
fn the_first_dimension_scales_a_sketch_drawn_by_eye() {
    let mut s = drawn_by_eye();
    let before = sketch(&s);
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["vc", "b"], "value": "50 mm", "text_at": [15, 47], "scale_sketch": true}));
    let f = 50.0 / 29.6;
    assert!((out["scaled"].as_f64().unwrap() - f).abs() < 1e-6, "{out}");
    assert!((out["measured"].as_f64().unwrap() - 50.0).abs() < 1e-6, "{out}");
    // Every point scaled about the origin: the shape and the construction lines keep their
    // directions (the vertical construction line stays vertical), only the size changes.
    let after = sketch(&s);
    for (p, q) in before.points.iter().zip(&after.points) {
        assert!(q.pos.dist(p.pos * f) < 1e-6, "{} {:?} -> {:?}", p.id, p.pos, q.pos);
    }
    // The second dimension sizes the sketch as usual: nothing scales again.
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["c"], "value": "30 mm", "scale_sketch": true}));
    assert!(out.get("scaled").is_none(), "{out}");
    assert!((pt(&s, "c.end").x - pt(&s, "c.start").x - 30.0).abs() < 1e-6);
    assert!((pt(&s, "b.start").x - pt(&s, "vc.start").x - 50.0).abs() < 1e-6);
    // The construction line can still be made vertical (it already is: redundant, not a conflict).
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "vc"}));
}

#[test]
fn the_first_dimension_does_not_scale_a_sketch_that_has_a_size() {
    // A sketch with projected or fixed geometry, or another length dimension, already has a
    // size: its first new dimension sizes what it measures as before.
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 40]}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": "l1.end"}));
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": "50 mm", "scale_sketch": true}));
    assert!(out.get("scaled").is_none(), "{out}");
    // An angle never scales.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0], [50, 20]]}));
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["l1", "l2"], "value": "60 deg", "scale_sketch": true}));
    assert!(out.get("scaled").is_none(), "{out}");
}

/// #3786: shapes that nothing links together are not one drawing: the first dimension sizes
/// the shape it measures and leaves the other alone.
#[test]
fn the_first_dimension_leaves_a_separate_shape_alone() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 10], "p1": [45, 20]}));
    let other = sketch(&s);
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "30 mm", "scale_sketch": true}));
    assert!(out.get("scaled").is_none(), "{out}");
    let after = sketch(&s);
    // The second rectangle (points 4..8, after the origin and the first rectangle's four) is untouched.
    for (p, q) in other.points.iter().zip(&after.points).skip(5) {
        assert!(q.pos.dist(p.pos) < 1e-6, "{} {:?} -> {:?}", p.id, p.pos, q.pos);
    }
}

/// #1365 still holds for one drawing: shapes joined by a constraint scale together.
#[test]
fn the_first_dimension_scales_shapes_joined_by_a_constraint() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 10], "p1": [45, 20]}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l1.start", "b": "l5.start"}));
    let out = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "40 mm", "scale_sketch": true}));
    assert!(out.get("scaled").is_some(), "{out}");
}

/// The cursor fallback shares the held solves' frame budget; a scripted drag can finish later.
#[test]
fn cursor_fallback_respects_an_exhausted_live_frame_budget() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [-10, 0]], "ids": ["crank"], "infer": false}));
    run(&mut s, "sketch.dimension", json!({"entities": ["crank"], "value": 10}));
    let target = v(-10.0, 16.0);
    let nearest = target * (10.0 / target.len());
    super::drag::last_drag::with_frame_work(0, || {
        run(&mut s, "sketch.drag", json!({"entity": "crank.end", "from": [-10, 0], "to": [target.x, target.y], "live": true}));
    });
    let at = pt(&s, "crank.end");
    assert!(at.dist(nearest) > 1.0, "an exhausted frame must not start unbudgeted cursor work: {at:?}");
    assert!((at.len() - 10.0).abs() < 1e-6);
    assert!(s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.ok());
    run(&mut s, "sketch.drag", json!({"entity": "crank.end", "from": [at.x, at.y], "to": [target.x, target.y], "live": false}));
    assert!(pt(&s, "crank.end").dist(nearest) < 1e-4, "scripted motion retains the full cursor objective");
}

fn four_bar_drag_sketch() -> Session {
    let mut s = new_sketch();
    let x = -10.0 + (1600.0 - 400.0 + 52.0 * 52.0) / 104.0;
    let y = -(1600.0_f64 - (x + 10.0) * (x + 10.0)).sqrt();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[-10,0],[x,y],[42,0]],"ids":["crank","coupler","rocker"],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":["crank.start","rocker.end"],"ids":["ground"],"infer":false}));
    for (line, len) in [("crank", 10), ("coupler", 40), ("rocker", 20), ("ground", 42)] {
        run(&mut s, "sketch.dimension", json!({"entities":[line],"value":len}));
    }
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line":"ground"}));
    s
}

#[test]
fn releasing_a_budget_limited_four_bar_drag_finishes_with_a_bounded_budget() {
    for (entity, fraction) in [("crank.end", 1.0), ("crank", 0.7)] {
        let mut s = four_bar_drag_sketch();
        let target = v(-10.0, 16.0) * fraction;
        let nearest = target * (10.0 * fraction / target.len());
        super::drag::last_drag::with_frame_work(0, || {
            run(&mut s, "sketch.drag", json!({"entity":entity,"from":[-10.0*fraction,0],"to":[target.x,target.y],"live":true}));
            let at = pt(&s, "crank.end") * fraction;
            assert!(at.dist(nearest) > 0.5);
        });
        // The release is a new frame, with the same normal frame budget, not an unbounded
        // final solve. The pointer has not moved since the budget-limited frame.
        let at = pt(&s, "crank.end") * fraction;
        run(&mut s, "sketch.drag", json!({"entity":entity,"from":[at.x,at.y],"to":[target.x,target.y],"live":true}));
        assert!((pt(&s, "crank.end") * fraction).dist(nearest) < 1e-4);
        run(&mut s, "sketch.drag_end", json!({}));
        assert!((pt(&s, "crank.end") * fraction).dist(nearest) < 1e-4);
    }
}

#[test]
fn budget_limited_cursor_progress_requests_a_frame_with_the_same_target() {
    let mut exercised = false;
    for budget in [2048, 4096, 8192, 16384, 32768] {
        let mut s = four_bar_drag_sketch();
        let target = v(-7.0, 11.2);
        let nearest = target * (7.0 / target.len());
        let r = super::drag::last_drag::with_frame_work(budget, || {
            run(&mut s, "sketch.drag", json!({"entity":"crank","from":[-7,0],"to":[target.x,target.y],"live":true}))
        });
        if r["retry"] != true {
            continue;
        }
        exercised = true;
        let at = pt(&s, "crank.end") * 0.7;
        assert!(at.dist(target) < v(-7.0, 0.0).dist(target));
        let r = run(&mut s, "sketch.drag", json!({"entity":"crank","from":[at.x,at.y],"to":[target.x,target.y],"live":true}));
        assert!((pt(&s, "crank.end") * 0.7).dist(nearest) < 1e-4);
        assert_eq!(r["retry"], false);
    }
    assert!(exercised, "exercise continuation after budget-limited progress");
}

// ---------------------------------------------------------------------------------------------
// Cancel Sketch (#3838)

fn sketch_below_a_solid() -> (Session, u64) {
    let mut s = Session::default();
    let id = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    (s, id)
}

#[test]
fn finishing_a_temporary_sketch_edit_at_the_undo_limit_keeps_the_original_design() {
    let (mut s, id) = sketch_below_a_solid();
    for i in 0..crate::MAX_UNDO {
        run(&mut s, "browser.visibility", json!({"items": ["Body1"], "visible": i % 2 != 0}));
    }
    assert_eq!(s.undo.len(), crate::MAX_UNDO);
    let before = s.doc.to_json();
    run(&mut s, "sketch.edit", json!({"sketch": id, "restore_marker": null}));
    for y in [20, 30] {
        run(&mut s, "sketch.line", json!({"points": [[0, y], [10, y]]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.undo.len() <= crate::MAX_UNDO);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.active_sketch, None);
    assert_eq!(s.model.state().bodies.len(), 1);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.marker, None);
    assert_eq!(s.doc.sketch(id).unwrap().curves.len(), 6);
}

#[test]
fn undoing_cancel_then_finishing_restores_the_temporary_sketch_marker() {
    let (mut s, id) = sketch_below_a_solid();
    run(&mut s, "sketch.edit", json!({"sketch": id, "restore_marker": null}));
    run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 20]]}));
    run(&mut s, "sketch.cancel", json!({}));
    assert_eq!(s.doc.marker, None);
    assert_eq!(s.doc.sketch(id).unwrap().curves.len(), 4);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.active_sketch, Some(id));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(s.doc.marker, None);
    assert_eq!(s.active_sketch, None);
    assert_eq!(s.doc.sketch(id).unwrap().curves.len(), 5);
    assert_eq!(s.model.state().bodies.len(), 1);
}

#[test]
fn cancel_removes_a_new_sketch_with_its_dimensions_and_undo_brings_it_back() {
    let mut s = Session::default();
    let before = s.doc.to_json();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 25}));
    let drawn = s.doc.to_json();
    let r = run(&mut s, "sketch.cancel", json!({}));
    assert_eq!(r["removed"], true);
    assert_eq!(s.active_sketch, None);
    assert_eq!(s.doc.to_json(), before, "the sketch and its dimension parameter are gone");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), drawn);
    assert!(s.active_sketch.is_some(), "undo goes back into the sketch");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn cancel_puts_an_edited_sketch_back_as_it_was_opened() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    let opened = s.doc.to_json();
    let volume = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    run(&mut s, "sketch.edit", json!({"sketch": "Base"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 30]]}));
    let r = run(&mut s, "sketch.cancel", json!({}));
    assert_eq!(r["removed"], false);
    assert_eq!(s.doc.to_json(), opened);
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "the solid is built from the sketch as it was");

    // Finishing keeps the changes: there is nothing left to cancel.
    run(&mut s, "sketch.edit", json!({"sketch": "Base"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(&mut s, "sketch.finish", json!({}));
    assert!((volume(&mut s) - 2000.0).abs() < 1e-6);
    assert!(s.execute("sketch.cancel", &json!({})).is_err(), "not in a sketch");
}

#[test]
fn cancel_refuses_once_undo_has_gone_back_past_where_the_sketch_was_opened() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 5]]}));
    // Back past the edit: the first line's step is undone too, still inside the sketch.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.active_sketch.is_some());
    let before = s.doc.to_json();
    let e = s.execute("sketch.cancel", &json!({})).unwrap_err().to_string();
    assert!(e.contains("Undo"), "{e}");
    assert_eq!(s.doc.to_json(), before, "nothing changed");
}

/// A line tangent to a free circle, one end dragged off the line: the other end stays where it
/// is and the circle takes the change (it moved the far end, by 0.4 mm towards the dragged one
/// for a 5 mm pull, though nothing needed it to).
#[test]
fn dragging_an_end_of_a_tangent_line_leaves_its_other_end_still() {
    let mut s = new_sketch();
    let c = run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"][0].as_str().unwrap().to_string();
    let l = run(&mut s, "sketch.line", json!({"points": [[-20, 10], [20, 10]], "infer": false, "connect": false}))["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "sketch.constraint.tangent", json!({"a": l, "b": c}));
    let at = |s: &Session, id: &str| {
        let sk = sketch(s);
        sk.resolve_point(id).and_then(|i| sk.point(i)).unwrap()
    };
    let start = at(&s, &format!("{l}.start"));
    drag(&mut s, &format!("{l}.end"), [20.0, 10.0], [5.0, 12.0]);
    assert!(at(&s, &format!("{l}.start")).dist(start) < 1e-7, "the far end moved: {:?}", at(&s, &format!("{l}.start")));
    assert!(
        at(&s, &format!("{l}.end")).dist(solidcraft_geom::Vec2::new(5.0, 12.0)) < 1e-6,
        "the end went where dragged: {:?}",
        at(&s, &format!("{l}.end"))
    );
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["failing"].as_array().is_some_and(Vec::is_empty), "still tangent: {si}");
}

// ---------------------------------------------------------------------------------------------
// A dimension without a value restates its measurement in the design's units (#3812): `1 in`
// for a 25.4 mm line in an inch design, so the value box opens in the units the text shows.

/// A sketch with a 25.4 mm line `l1` in a design in `units`.
fn units_line(units: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [25.4, 0]], "ids": ["l1"]}));
    run(&mut s, "document.units", json!({"units": units}));
    s
}

fn units_param(s: &Session, name: &str) -> (String, String) {
    let p = s.doc.param(name).unwrap();
    (p.expr.clone(), p.unit.clone())
}

fn units_line_end(s: &Session) -> f64 {
    let state = s.model.state();
    let sk = &state.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    sk.point(sk.resolve_point("l1.end").unwrap()).unwrap().x
}

#[test]
fn a_dimension_without_a_value_is_written_in_the_design_units() {
    for (units, expr) in [("in", "1 in"), ("mm", "25.4 mm"), ("cm", "2.54 cm"), ("ft", "0.083333333 ft"), ("m", "0.0254 m")] {
        let mut s = units_line(units);
        let r = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "scale_sketch": true}));
        let name = r["param"].as_str().unwrap();
        assert_eq!(units_param(&s, name), (expr.to_string(), units.to_string()), "{units}");
        assert!((units_line_end(&s) - 25.4).abs() < 1e-5, "{units}: the restated dimension moves nothing ({})", units_line_end(&s));
    }
}

#[test]
fn a_typed_bare_number_still_means_the_design_units() {
    let mut s = units_line("in");
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": "2"}));
    assert_eq!(units_param(&s, r["param"].as_str().unwrap()).0, "2 in");
    assert!((units_line_end(&s) - 50.8).abs() < 1e-6);
}

#[test]
fn a_driven_dimension_made_driving_and_autoconstrain_dimensions_use_the_design_units() {
    let mut s = units_line("in");
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "driven": true}));
    let state = s.model.state();
    let id = state.sketch(s.active_sketch.unwrap()).unwrap().sketch.constraints.iter().find(|c| c.driven).unwrap().id.clone();
    run(&mut s, "sketch.toggle_driven", json!({"constraint": id, "driven": false}));
    let made = s.doc.params.iter().find(|p| p.model).unwrap();
    assert_eq!((made.expr.as_str(), made.unit.as_str()), ("1 in", "in"));

    let mut s = units_line("in");
    run(&mut s, "sketch.auto_constrain", json!({}));
    let lengths: Vec<_> = s.doc.params.iter().filter(|p| p.model && p.unit != "deg").collect();
    assert!(!lengths.is_empty(), "AutoConstrain dimensioned the line");
    assert!(lengths.iter().all(|p| p.unit == "in" && p.expr.ends_with(" in")), "{lengths:?}");
}

/// #6070: changing a rectangle side's dimension keeps the side's first corner where it is, so
/// the side grows from where it was drawn, not from both ends. A centred rectangle (Symmetric
/// about the axis) still grows both ways, as it must.
#[test]
fn a_changed_length_dimension_keeps_its_first_point() {
    let at = |s: &Session, p: &str| {
        let sk = sketch(s);
        sk.point(sk.resolve_point(p).unwrap()).unwrap()
    };
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [50, 30]}));
    let bottom = r["sides"]["bottom"].as_str().unwrap().to_string();
    let right = r["sides"]["right"].as_str().unwrap().to_string();
    let width = run(&mut s, "sketch.dimension", json!({"entities": [bottom]}))["param"].as_str().unwrap().to_string();
    let height = run(&mut s, "sketch.dimension", json!({"entities": [right]}))["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": width, "expression": "60 mm"}));
    assert!(at(&s, "p1").dist(solidcraft_geom::Vec2::new(10.0, 10.0)) < 1e-6, "the first corner stays: {:?}", at(&s, "p1"));
    assert!(at(&s, "p2").dist(solidcraft_geom::Vec2::new(70.0, 10.0)) < 1e-6, "the far corner moves: {:?}", at(&s, "p2"));
    // Several at once: the right side grows up from its start, the bottom-right corner.
    run(&mut s, "parameters.change", json!({"changes": {height: "25 mm"}}));
    assert!(at(&s, "p2").dist(solidcraft_geom::Vec2::new(70.0, 10.0)) < 1e-6, "{:?}", at(&s, "p2"));
    assert!(at(&s, "p3").dist(solidcraft_geom::Vec2::new(70.0, 35.0)) < 1e-6, "{:?}", at(&s, "p3"));

    // Centred on the Y axis: holding a corner can't keep it symmetric, so both ends move.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-20, 0], "p1": [20, 10]}));
    let bottom = r["sides"]["bottom"].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "p1", "b": "p2", "line": "y"}));
    let width = run(&mut s, "sketch.dimension", json!({"entities": [bottom]}))["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": width, "expression": "60 mm"}));
    assert!((at(&s, "p1").x + 30.0).abs() < 1e-6 && (at(&s, "p2").x - 30.0).abs() < 1e-6, "{:?} {:?}", at(&s, "p1"), at(&s, "p2"));
}

/// #7912: a dimension can measure a model edge: the edge is projected into the sketch and the
/// dimension measures the distance from the sketch's point to it.
#[test]
fn a_dimension_measures_a_model_edge() {
    let mut s = Session::default();
    // A sheet-metal base flange: a 100 x 50 plate whose front edge runs along y = 0.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base", "body_name": "Flange"}));
    // A sketch with a point 25 above the flange's front edge.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Near"}));
    let point = run(&mut s, "sketch.point", json!({"point": [50, 25]}))["point"].as_str().unwrap().to_string();
    let out = run(&mut s, "sketch.dimension", json!({"entities": [point, {"edge": [50, 0, 0], "body": "Flange"}]}));
    assert!((out["measured"].as_f64().unwrap() - 25.0).abs() < 1e-6, "the distance to the edge: {out}");
    // The edge is projected into the sketch, and the dimension measures that projection.
    let sk = sketch(&s);
    assert_eq!(sk.links.len(), 1, "{:?}", sk.links);
}

/// #7912: a dimension refuses a long list of entities before projecting any of them, so one
/// request cannot make the engine resolve thousands of model edges.
#[test]
fn a_dimension_refuses_a_long_list_of_entities() {
    // The most entities one call may project (the engine's `MAX_PROJECTED_ENTITIES`).
    const MAX_PROJECTED: usize = 1_000;
    let mut s = Session::default();
    // A sheet-metal base flange: a 100 x 50 plate whose front edge runs along y = 0.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 50]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base", "body_name": "Flange"}));
    // A sketch with a point 25 above the flange's front edge.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Near"}));
    let point = run(&mut s, "sketch.point", json!({"point": [50, 25]}))["point"].as_str().unwrap().to_string();
    // More entities than a call may project, every one of them a model edge to project.
    let mut entities = vec![json!(point)];
    for _ in 0..MAX_PROJECTED {
        entities.push(json!({"edge": [50, 0, 0], "body": "Flange"}));
    }
    let before = serde_json::to_value(&s.doc).unwrap();
    let error = s.execute("sketch.dimension", &json!({"entities": entities})).unwrap_err();
    assert!(error.to_string().contains("at most"), "{error}");
    // The refused request leaves the design as it was.
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
}
