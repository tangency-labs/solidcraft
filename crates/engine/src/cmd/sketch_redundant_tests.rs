//! Redundant but consistent constraints (#687): rectangles of one size drawn around a shared
//! corner (a "clover"), or onto each other's sides, imply some of their constraints twice. Those
//! are kept (and reported as redundant) or not added at all; only constraints that conflict
//! with the sketch are refused.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn report(s: &Session) -> solidcraft_sketch::SolveReport {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.clone()
}

fn glyphs(s: &mut Session) -> Value {
    run(s, "sketch.glyphs", json!({}))
}

/// The four quadrants around `c`, each `w` square: bottom-left, bottom-right, top-right,
/// top-left (each next one shares a side with the one before; the last shares two).
fn quadrants(c: [f64; 2], w: f64) -> [([f64; 2], [f64; 2]); 4] {
    let [x, y] = c;
    [([x - w, y - w], [x, y]), ([x, y - w], [x + w, y]), ([x, y], [x + w, y + w]), ([x - w, y], [x, y + w])]
}

fn horizontal_vertical(sk: &solidcraft_sketch::Sketch) -> usize {
    sk.constraints
        .iter()
        .filter(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Horizontal { .. } | solidcraft_sketch::ConstraintKind::Vertical { .. }))
        .count()
}

/// The owner's steps: rectangles of one size (typed 10 × 10) drawn snapped onto a shared
/// corner, 2, 3 and 4 of them. Every one is accepted; a size the sketch already has becomes a
/// driven (reference) dimension, and the sketch ends fully constrained.
#[test]
fn a_clover_of_same_size_rectangles_drawn_by_snapping_is_accepted() {
    for n in 2..=4 {
        let mut s = new_sketch();
        for (i, (a, b)) in quadrants([0.0, 0.0], 10.0).iter().take(n).enumerate() {
            let r = s.execute(
                "sketch.draw.dimensioned",
                &json!({"command": "sketch.rectangle.two_point", "params": {"p0": a, "p1": b}, "dimensions": {"width": 10, "height": 10}}),
            );
            let r = r.unwrap_or_else(|e| panic!("rectangle {i} of {n}: {e}"));
            assert_eq!(r["sketch"]["solved"], true, "{r}");
        }
        let g = glyphs(&mut s);
        assert_eq!(g["fully_constrained"], true, "{n} rectangles: {g}");
        assert_eq!(g["conflicts"], json!([]), "{g}");
        let sk = sketch(&s);
        assert_eq!(sk.curves.len(), 4 * n);
        // Shared sides carry one horizontal/vertical, not two: 4 for the first rectangle, 3 for
        // each with one shared side, 2 for the last of four (two shared sides).
        let expected = [0, 4, 7, 10, 12][n];
        assert_eq!(horizontal_vertical(&sk), expected, "{n} rectangles");
        // Each later rectangle's size along a shared side is a reference dimension.
        let driven = sk.constraints.iter().filter(|c| c.driven).count();
        assert_eq!(driven, [0, 0, 1, 2, 4][n], "{n} rectangles");
        for (a, b) in quadrants([0.0, 0.0], 10.0).iter().take(n) {
            for corner in [Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1])] {
                assert!(sk.points.iter().any(|p| p.pos.dist(corner) < 1e-9), "{n} rectangles: corner {corner:?} moved");
            }
        }
    }
}

/// Undimensioned, the same clover is drawn without a single redundant constraint: shared
/// sides are not constrained twice, and the freedom left is the two column widths and the two
/// row heights.
#[test]
fn rectangles_sharing_sides_do_not_constrain_a_side_twice() {
    let mut s = new_sketch();
    for (a, b) in quadrants([50.0, 50.0], 10.0) {
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": a, "p1": b}));
    }
    let rep = report(&s);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.redundant, Vec::<String>::new());
    // Free: the shared corner (2), two column widths, two row heights.
    assert_eq!(rep.dof, 6);
    assert_eq!(horizontal_vertical(&sketch(&s)), 12);
}

/// Two rectangles on one side: the second only adds its width.
#[test]
fn shared_edge_rectangles() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [25, 10]}));
    // Three of its sides' directions and its two corners on the first one.
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(5), "{r}");
    let rep = report(&s);
    assert_eq!(rep.dof, 3, "the first one's width and height, the second one's width");
    assert!(rep.redundant.is_empty());
    // Sized and placed, all of it is fixed.
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 10}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 10}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l5"], "value": 15}));
    assert_eq!(glyphs(&mut s)["fully_constrained"], true);
}

/// Two separately drawn 10 × 10 squares snapped corner to corner: the second snap is implied by
/// the first and the equal sizes. It is kept, reported as redundant along with what implies
/// it, and the degrees of freedom and the fully constrained state are unchanged.
#[test]
fn snapping_a_second_corner_of_same_size_rectangles_is_redundant_not_refused() {
    let mut s = new_sketch();
    for (a, b) in [([0.0, 0.0], [10.0, 10.0]), ([20.0, 0.0], [30.0, 10.0])] {
        run(
            &mut s,
            "sketch.draw.dimensioned",
            json!({"command": "sketch.rectangle.two_point", "params": {"p0": a, "p1": b}, "dimensions": {"width": 10, "height": 10}}),
        );
    }
    // The second square's bottom-left corner onto the first one's bottom-right.
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l5.start", "b": "l1.end"}));
    let dof = report(&s).dof;
    assert_eq!(dof, 0, "the first square is on the origin, the second one placed by its corner");
    // Its top-left corner is already on the first one's top-right.
    let r = run(&mut s, "sketch.constraint.coincident", json!({"a": "l7.end", "b": "l2.end"}));
    assert_eq!(r["redundant"], true, "{r}");
    assert!(!r["determined_by"].as_array().is_none_or(Vec::is_empty), "{r}");
    let rep = report(&s);
    assert_eq!((rep.ok(), rep.dof), (true, 0));
    assert_eq!(rep.redundant, vec![r["constraint"].as_str().unwrap().to_string()]);
    let g = glyphs(&mut s);
    assert_eq!(g["fully_constrained"], true);
    let flagged: Vec<&Value> = g["glyphs"].as_array().unwrap().iter().filter(|x| x["redundant"] == true).collect();
    assert_eq!(flagged.len(), 1, "{g}");
    // Drawing-time inferences ask for redundant ones to be left out.
    let e = s.execute("sketch.constraint.horizontal_vertical", &json!({"line": "l8", "if_redundant": "refuse"})).unwrap_err().to_string();
    assert!(e.contains("already determined by"), "{e}");
}

/// A size the sketch already has, dimensioned again: refused as already determined (naming
/// what determines it), or placed as a driven dimension; an equal constraint the sketch already
/// implies is kept as redundant.
#[test]
fn redundant_equal_dimensions() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [20, 10]}));
    let height = run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 10}));
    let k = sketch(&s).constraints.iter().find(|c| c.param.as_deref() == height["param"].as_str()).map(|c| c.id.clone()).unwrap();
    // The second one's right side is as tall as the shared side.
    let e = s.execute("sketch.dimension", &json!({"entities": ["l6"], "value": 10})).unwrap_err().to_string();
    assert!(e.contains("already determined by") && e.contains(&k), "{e}");
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["l6"], "value": 10, "if_redundant": "driven"}));
    assert_eq!((d["driven"].clone(), d["redundant"].clone()), (json!(true), json!(true)), "{d}");
    // Equal widths, twice: the second is implied by the first.
    run(&mut s, "sketch.constraint.equal", json!({"a": "l1", "b": "l5"}));
    let before = report(&s).dof;
    let r = run(&mut s, "sketch.constraint.equal", json!({"a": "l3", "b": "l7"}));
    assert_eq!(r["redundant"], true, "{r}");
    assert_eq!(report(&s).dof, before);
    // Its driving value changes still solve with the redundant constraints present.
    let pname = height["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": pname, "expression": "14 mm"}));
    let sk = sketch(&s);
    assert!(report(&s).ok());
    let len = |id: &str| {
        let c = sk.curve_index(id).unwrap();
        let solidcraft_sketch::CurveKind::Line { a, b } = sk.curves[c].kind else { panic!() };
        sk.points[a].pos.dist(sk.points[b].pos)
    };
    assert!((len("l6") - 14.0).abs() < 1e-7 && (len("l4") - 14.0).abs() < 1e-7);
}

/// A constraint that does not hold where the others put the geometry still conflicts.
#[test]
fn a_conflicting_constraint_is_still_refused() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [20, 10]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 10}));
    let before = sketch(&s).constraints.len();
    // The second one's right side cannot be 12 while the shared side is 10.
    let e = s.execute("sketch.dimension", &json!({"entities": ["l6"], "value": 12})).unwrap_err().to_string();
    assert!(e.contains("over-constrain") || e.contains("conflicts"), "{e}");
    let e = s.execute("sketch.dimension", &json!({"entities": ["l6"], "value": 12, "if_redundant": "driven"})).unwrap_err().to_string();
    assert!(e.contains("over-constrain") || e.contains("conflicts"), "{e}");
    // A vertical side cannot be horizontal.
    let e = s.execute("sketch.constraint.horizontal_vertical", &json!({"line": "l2", "mode": "horizontal"})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
    assert_eq!(sketch(&s).constraints.len(), before);
    assert!(report(&s).redundant.is_empty());
}

/// Dragging the shared corner of a clover moves all four rectangles with it, redundant
/// constraints and all.
#[test]
fn dragging_the_shared_corner_moves_every_rectangle() {
    let mut s = new_sketch();
    for (a, b) in quadrants([50.0, 50.0], 10.0) {
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": a, "p1": b}));
    }
    // A redundant equal and a redundant vertical, so the drag has some to carry.
    run(&mut s, "sketch.constraint.equal", json!({"a": "l1", "b": "l13"}));
    let r = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l4"}));
    assert_eq!(r["redundant"], true, "{r}");
    assert!(!report(&s).redundant.is_empty());
    let centre = Vec2::new(50.0, 50.0);
    let id = sketch(&s).points.iter().find(|p| p.pos.dist(centre) < 1e-9).map(|p| p.id.clone()).unwrap();
    run(&mut s, "sketch.drag", json!({"entity": id, "from": [50, 50], "to": [56, 47]}));
    let sk = sketch(&s);
    let rep = report(&s);
    assert!(rep.ok(), "{rep:?}");
    let to = Vec2::new(56.0, 47.0);
    // Each rectangle still has a corner on the moved one and is still a rectangle.
    let near: usize = sk.points.iter().filter(|p| p.pos.dist(to) < 1e-6).count();
    assert!(near >= 4, "{near} corners at the dragged spot");
    for l in &sk.curves {
        let solidcraft_sketch::CurveKind::Line { a, b } = l.kind else { continue };
        let d = sk.points[b].pos - sk.points[a].pos;
        assert!(d.x.abs() < 1e-7 || d.y.abs() < 1e-7, "{} is no longer axis-aligned: {d:?}", l.id);
    }
    assert!(sk.points.iter().all(|p| p.pos.dist(centre) > 1e-6), "nothing stayed behind at the old corner");
}

/// A horizontal on a line that already is (it was drawn so, inferred) is redundant, not an
/// error; the degrees of freedom do not change, and the sketch still turns fully constrained.
#[test]
fn an_explicit_redundant_constraint_keeps_the_dof_and_full_constraint() {
    let mut s = new_sketch();
    let l = run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "infer": true}))["curves"][0].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": l}));
    assert_eq!(r["redundant"], true, "{r}");
    // Its start is on the origin (snapped): only its length is free.
    assert_eq!(report(&s).dof, 1);
    run(&mut s, "sketch.dimension", json!({"entities": [l], "value": 10}));
    let g = glyphs(&mut s);
    assert_eq!((g["dof"].clone(), g["fully_constrained"].clone()), (json!(0), json!(true)), "{g}");
    assert!(report(&s).point_determined.iter().all(|d| *d));
}

/// An arc slot is drawn with nothing redundant, as a straight slot is: its caps are tangent to
/// both sides, which already makes them equal (#8355). Its six degrees of freedom are the
/// centre, radius, start, sweep and width, and the two caps stay equal when the width changes.
#[test]
fn an_arc_slot_is_drawn_without_a_redundant_constraint() {
    for (cmd, p) in [
        ("sketch.slot.arc_center", json!({"center": [30, 30], "start": [50, 30], "end": [30, 50], "width": 6})),
        ("sketch.slot.arc_three_point", json!({"start": [30, 30], "through": [40, 35], "end": [50, 30], "width": 3})),
    ] {
        let mut s = new_sketch();
        let out = run(&mut s, cmd, p);
        let rep = report(&s);
        assert!(rep.ok(), "{cmd}: {rep:?}");
        assert_eq!((rep.dof, rep.redundant.clone()), (6, Vec::<String>::new()), "{cmd}: {rep:?}");
        let id = |i: usize| out["curves"][i].as_str().unwrap().to_string();
        let (end_cap, start_cap) = (id(1), id(3));
        let r = run(&mut s, "sketch.dimension", json!({"entities": [end_cap], "type": "diameter", "value": 8}));
        assert_ne!(r["redundant"], true, "{cmd}: {r}");
        let sk = sketch(&s);
        let radius = |c: &str| match sk.curves[sk.curve_index(c).unwrap()].kind {
            solidcraft_sketch::CurveKind::Arc { c, a, .. } => sk.points[c].pos.dist(sk.points[a].pos),
            _ => panic!("{c} is a cap arc"),
        };
        assert!(
            (radius(&end_cap) - 4.0).abs() < 1e-7 && (radius(&start_cap) - 4.0).abs() < 1e-7,
            "{cmd}: caps {} and {}",
            radius(&end_cap),
            radius(&start_cap)
        );
        let rep = report(&s);
        assert_eq!((rep.dof, rep.redundant), (5, Vec::<String>::new()), "{cmd}");
    }
}
