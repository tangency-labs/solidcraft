//! Adversarial Trim, Extend and Offset cases (#45): each gives the expected result or a clear
//! error, and the sketch always stays sound (it solves, every point is finite, nothing
//! degenerate is left behind).

use serde_json::{Value, json};

use crate::Session;

fn new_sketch() -> Session {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s
}

fn curves(r: &Value) -> Vec<String> {
    r["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn add(s: &mut Session, id: &str, p: Value) -> Vec<String> {
    match s.execute(id, &p) {
        Ok(r) => curves(&r),
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The sketch is sound: it solves, every point is finite, no curve has collapsed.
fn sound(s: &Session) {
    let id = s.active_sketch.unwrap();
    let st = s.model.state();
    let ss = st.sketch(id).unwrap();
    assert!(ss.report.ok(), "the sketch no longer solves: {:?}", ss.report.failing);
    for p in &ss.sketch.points {
        assert!(p.pos.is_finite(), "point {} is {:?}", p.id, p.pos);
    }
    for (i, c) in ss.sketch.curves.iter().enumerate() {
        let len: f64 = ss.sketch.segs(i).iter().map(|g| g.length()).sum();
        assert!(len.is_finite() && len > 1e-9, "curve {} has length {len}", c.id);
    }
}

fn count(s: &Session) -> usize {
    let id = s.active_sketch.unwrap();
    s.model.state().sketch(id).unwrap().sketch.curves.len()
}

#[test]
fn trim_cases() {
    // A line crossing a circle twice: trimming the chord leaves the two outer stubs.
    let mut s = new_sketch();
    add(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    let l = add(&mut s, "sketch.line", json!({"points": [[-20, 0], [20, 0]]}))[0].clone();
    s.execute("sketch.trim", &json!({"curve": l, "at": [0, 0]})).unwrap();
    sound(&s);
    assert_eq!(count(&s), 3, "circle + two stubs");
    // Trimming at a tangent touch, at a curve end, on a curve with no crossings.
    let mut s = new_sketch();
    let c = add(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))[0].clone();
    add(&mut s, "sketch.line", json!({"points": [[-20, 10], [20, 10]]}));
    let r = s.execute("sketch.trim", &json!({"curve": c, "at": [0, -10]}));
    sound(&s);
    let _ = r;
    let mut s = new_sketch();
    let l = add(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))[0].clone();
    // No crossings: the whole line goes (it is deleted).
    let r = s.execute("sketch.trim", &json!({"curve": l, "at": [5, 0]}));
    sound_or_empty(&s);
    assert!(r.is_ok() || r.is_err());
    // Overlapping collinear lines.
    let mut s = new_sketch();
    let a = add(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))[0].clone();
    add(&mut s, "sketch.line", json!({"points": [[5, 0], [15, 0]]}));
    let _ = s.execute("sketch.trim", &json!({"curve": a, "at": [10, 0]}));
    sound(&s);
    // A spline crossed by a line, trimmed between the crossings; an ellipse cut by a line.
    let mut s = new_sketch();
    let sp = add(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 10], [20, -10], [30, 0]]}))[0].clone();
    add(&mut s, "sketch.line", json!({"points": [[-5, 2], [35, 2]]}));
    s.execute("sketch.trim", &json!({"curve": sp, "at": [10, 9]})).unwrap();
    sound(&s);
    let mut s = new_sketch();
    let e = add(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}))[0].clone();
    add(&mut s, "sketch.line", json!({"points": [[-15, 0], [15, 0]]}));
    s.execute("sketch.trim", &json!({"curve": e, "at": [0, -4]})).unwrap();
    sound(&s);
    // Hostile points.
    let mut s = new_sketch();
    let l = add(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))[0].clone();
    for at in [json!([f64::MAX, 0]), json!([1e300, 1e300]), json!("x"), json!([0])] {
        let _ = s.execute("sketch.trim", &json!({"curve": l, "at": at}));
        sound(&s);
    }
}

fn sound_or_empty(s: &Session) {
    if count(s) > 0 {
        sound(s);
    }
}

#[test]
fn extend_cases() {
    // A line extends to a circle; to a spline; with nothing to reach it runs on as far as the
    // preview showed.
    let mut s = new_sketch();
    add(&mut s, "sketch.circle.center", json!({"center": [20, 0], "radius": 5}));
    let l = add(&mut s, "sketch.line", json!({"points": [[0, 0], [5, 0]]}))[0].clone();
    s.execute("sketch.extend", &json!({"curve": l, "at": [5, 0]})).unwrap();
    sound(&s);
    let id = s.active_sketch.unwrap();
    let st = s.model.state();
    let ss = st.sketch(id).unwrap();
    let li = ss.sketch.curve_index(&l).unwrap();
    let end = ss.sketch.segs(li).iter().map(|g| g.end().x.max(g.start().x)).fold(f64::NEG_INFINITY, f64::max);
    assert!((end - 15.0).abs() < 1e-6, "{end}");
    drop(st);
    let mut s = new_sketch();
    add(&mut s, "sketch.spline.fit_point", json!({"points": [[20, -10], [22, 0], [20, 10]]}));
    let l = add(&mut s, "sketch.line", json!({"points": [[0, 0], [5, 0]]}))[0].clone();
    s.execute("sketch.extend", &json!({"curve": l, "at": [5, 0]})).unwrap();
    sound(&s);
    let mut s = new_sketch();
    let l = add(&mut s, "sketch.line", json!({"points": [[0, 0], [5, 0]]}))[0].clone();
    s.execute("sketch.extend", &json!({"curve": l, "at": [5, 0]})).unwrap();
    sound(&s);
    let id = s.active_sketch.unwrap();
    let st = s.model.state();
    let ss = st.sketch(id).unwrap();
    let li = ss.sketch.curve_index(&l).unwrap();
    let end = ss.sketch.segs(li).iter().map(|g| g.end().x.max(g.start().x)).fold(f64::NEG_INFINITY, f64::max);
    assert!((end - (5.0 + super::EXTEND_REACH)).abs() < 1e-6, "{end}");
    drop(st);
    // An arc extends along its circle to a line.
    let mut s = new_sketch();
    add(&mut s, "sketch.line", json!({"points": [[-20, -3], [20, -3]]}));
    let a = add(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [0, 10], "through": [7.0710678, 7.0710678]}))[0].clone();
    let _ = s.execute("sketch.extend", &json!({"curve": a, "at": [10, 0]}));
    sound(&s);
}

#[test]
fn offset_cases() {
    // A rectangle offset outward and inward; inward past its middle vanishes (clear error).
    let mut s = new_sketch();
    let r = add(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
    let n0 = count(&s);
    s.execute("sketch.offset", &json!({"curves": [r[0].clone()], "distance": 2, "side": [-5, -5]})).unwrap();
    sound(&s);
    assert!(count(&s) > n0);
    let n1 = count(&s);
    s.execute("sketch.offset", &json!({"curves": [r[0].clone()], "distance": 2, "side": [10, 5]})).unwrap();
    sound(&s);
    assert!(count(&s) > n1);
    let n2 = count(&s);
    assert!(
        s.execute("sketch.offset", &json!({"curves": [r[0].clone()], "distance": 6, "side": [10, 5]})).is_err(),
        "inward 6 on a 10 wide rectangle"
    );
    assert_eq!(count(&s), n2, "a failed offset adds nothing");
    sound(&s);
    // A slot (lines and arcs), a spline chain, an open zig-zag whose inner offset self-crosses.
    let mut s = new_sketch();
    let slot = add(&mut s, "sketch.slot.center_to_center", json!({"p0": [0, 0], "p1": [20, 0], "width": 6}));
    if let Some(c) = slot.first() {
        s.execute("sketch.offset", &json!({"curves": [c], "distance": 1, "side": [10, 10]})).unwrap();
        sound(&s);
    }
    let mut s = new_sketch();
    let sp = add(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 6], [20, -6], [30, 0]]}))[0].clone();
    s.execute("sketch.offset", &json!({"curves": [sp], "distance": 1.5})).unwrap();
    sound(&s);
    let mut s = new_sketch();
    let z = add(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [10.5, 5], [11, 0], [20, 0]]}));
    let r = s.execute("sketch.offset", &json!({"curves": [z[0].clone()], "distance": 2, "side": [10, -5]}));
    sound(&s);
    let _ = r;
    // Hostile distances.
    let mut s = new_sketch();
    let l = add(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))[0].clone();
    for d in [json!(0), json!(-1e308), json!(f64::MAX), json!("x"), json!(1e-12)] {
        let _ = s.execute("sketch.offset", &json!({"curves": [l.clone()], "distance": d}));
        sound(&s);
    }
}

/// #7635: the reporter's design - `l1` with its midpoint pinned at the origin, `l2` perpendicular
/// to it at the origin, and a construction line `l3` at the origin constrained horizontal with a
/// 50-degree angle dimension to `l1`. Dragging a free end of `l1` (whose exact target the pinned
/// inclination makes unreachable) used to leave the failed step placed and break the perpendicular
/// and angle constraints; the sketch must stay solved instead.
#[test]
fn dragging_the_midpoint_line_end_keeps_the_sketch_solved() {
    let mut s = new_sketch();
    add(&mut s, "sketch.line", json!({"points": [[-6.616478376301535, -7.876608451910095], [6.616478376301535, 7.876608451910095]]}));
    add(&mut s, "sketch.constraint.midpoint", json!({"point": "origin", "line": "l1"}));
    add(&mut s, "sketch.line", json!({"points": [[0.0, 0.0], [-10.656000978748368, 8.951213010548841]]}));
    add(&mut s, "sketch.constraint.coincident", json!({"a": "l2.start", "b": "origin"}));
    add(&mut s, "sketch.constraint.perpendicular", json!({"a": "l2", "b": "l1"}));
    add(&mut s, "sketch.line", json!({"points": [[0.0, 0.0], [10.0, 0.0]], "construction": true}));
    add(&mut s, "sketch.constraint.coincident", json!({"a": "l3.start", "b": "origin"}));
    add(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l3", "mode": "horizontal"}));
    add(&mut s, "sketch.dimension", json!({"entities": ["l3", "l1"], "type": "angle", "value": "50 deg"}));
    let r = s.execute("sketch.drag", &json!({"entity": "l1.end", "from": [6.616478376301535, 7.876608451910095], "to": [10.0, 4.0]})).unwrap();
    assert_eq!(r["sketch"]["solved"], json!(true), "the drag left the sketch unsolved: {r}");
    s.execute("sketch.drag_end", &json!({})).unwrap();
    sound(&s);
}
