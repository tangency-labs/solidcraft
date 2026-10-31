#![allow(clippy::unwrap_used, clippy::panic)]
//! #2424: curves start at the ends of an associated offset spline, and follow them.
use serde_json::{Value, json};
use solidcraft_doc::Document;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}
fn solved(s: &Session, id: u64) -> Sketch {
    s.model.state().sketch(id).unwrap().sketch.clone()
}
/// An open fit-point spline `s1` and its associated offset; returns the offset's id.
fn offset_spline(s: &mut Session) -> (u64, String) {
    run(s, "sketch.create", json!({"plane":"XY"}));
    let sketch = s.active_sketch.unwrap();
    run(s, "sketch.spline.fit_point", json!({"points":[[0,0],[10,5],[20,0],[30,5]],"ids":["s1"]}));
    let result = run(s, "sketch.offset", json!({"curves":["s1"],"distance":"2 mm","chain":true}));
    (sketch, result["curves"][0].as_str().unwrap().into())
}
fn coincident(sk: &Sketch, a: usize, b: usize) -> bool {
    sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Coincident { p, q } if (p, q) == (a, b) || (p, q) == (b, a)))
}

#[test]
fn lines_drawn_from_offset_spline_ends_get_their_own_coincident_points() {
    let mut s = Session::default();
    let (id, output) = offset_spline(&mut s);
    // Close the shape between the spline and its offset at both ends.
    run(&mut s, "sketch.line", json!({"points":["s1.start", format!("{output}.start")],"ids":["l1"]}));
    run(&mut s, "sketch.line", json!({"points":[format!("{output}.end"), "s1.end"],"ids":["l2"]}));
    let sk = solved(&s, id);
    sk.validate_freeform_offsets().unwrap();
    let o = sk.curve_index(&output).unwrap();
    let (start, end) = sk.curves[o].kind.ends().unwrap();
    let (l1, l2) = (sk.curve_index("l1").unwrap(), sk.curve_index("l2").unwrap());
    let (CurveKind::Line { b: l1_end, .. }, CurveKind::Line { a: l2_start, .. }) = (&sk.curves[l1].kind, &sk.curves[l2].kind) else {
        panic!("lines")
    };
    assert!(*l1_end != start && *l2_start != end, "the offset keeps sole ownership of its nodes");
    assert!(coincident(&sk, start, *l1_end) && coincident(&sk, end, *l2_start));
    assert!(sk.points[*l1_end].pos.dist(sk.points[start].pos) < 1e-9);

    // Moving the source moves the offset, and the lines follow its ends.
    run(&mut s, "sketch.move", json!({"entities":["s1"],"translate":[-3.0,2.0]}));
    let moved = solved(&s, id);
    moved.validate_freeform_offsets().unwrap();
    assert!(moved.points[start].pos.dist(sk.points[start].pos) > 1.0, "the offset followed its source");
    assert!(moved.points[*l1_end].pos.dist(moved.points[start].pos) < 1e-6);
    assert!(moved.points[*l2_start].pos.dist(moved.points[end].pos) < 1e-6);

    // The design saves and loads with the connection.
    let doc = Document::from_json(&s.doc.to_json()).unwrap();
    let loaded = doc.sketch(id).unwrap();
    loaded.validate_freeform_offsets().unwrap();
    assert!(coincident(loaded, start, *l1_end));
}

#[test]
fn an_arc_from_an_offset_end_is_drawn_and_undone_atomically() {
    let mut s = Session::default();
    let (id, output) = offset_spline(&mut s);
    let before = s.doc.to_json();
    run(&mut s, "sketch.arc.three_point", json!({"start":format!("{output}.end"),"through":[33,3],"end":"s1.end"}));
    let sk = solved(&s, id);
    sk.validate_freeform_offsets().unwrap();
    let end = sk.curves[sk.curve_index(&output).unwrap()].kind.ends().unwrap().1;
    assert_eq!(sk.curves.iter().filter(|c| c.kind.uses(end)).count(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn an_open_offset_spline_shows_only_its_ends() {
    let mut s = Session::default();
    let (id, output) = offset_spline(&mut s);
    let sk = solved(&s, id);
    let o = sk.curve_index(&output).unwrap();
    let (start, end) = sk.curves[o].kind.ends().unwrap();
    let nodes = sk.curves[o].kind.point_ids();
    assert!(nodes.len() > 2);
    for i in nodes {
        assert_eq!(crate::view::sketch_point_visible(&sk, i), i == start || i == end, "node {i}");
    }
}
