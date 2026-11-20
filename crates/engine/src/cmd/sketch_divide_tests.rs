//! `sketch.point.divide` (#8969): points dividing sketch curves into equal lengths, held at their
//! shares of the length as the curves change.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::Vec2Geom as _;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> Sketch {
    s.doc.sketch(s.active_sketch.unwrap()).unwrap().clone()
}

fn pt(sk: &Sketch, r: &str) -> Vec2 {
    sk.point(sk.resolve_point(r).unwrap()).unwrap()
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn line_ends(sk: &Sketch, l: &str) -> (Vec2, Vec2) {
    let Some(CurveKind::Line { a, b }) = sk.curves.get(sk.curve_index(l).unwrap()).map(|c| &c.kind) else { panic!("{l} is not a line") };
    (sk.point(*a).unwrap(), sk.point(*b).unwrap())
}

/// Length along `poly` to the point of it nearest `q`, and how far `q` is from it.
fn station(poly: &[Vec2], q: Vec2) -> (f64, f64) {
    let mut best = (0.0, f64::INFINITY);
    let mut run_len = 0.0;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = b - a;
        let t = if d.len2() > 0.0 { ((q - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
        let dist = q.dist(a + d * t);
        if dist < best.1 {
            best = (run_len + d.len() * t, dist);
        }
        run_len += d.len();
    }
    best
}

#[test]
fn a_line_divided_in_four_keeps_its_quarters_when_it_grows() {
    let mut s = new_sketch();
    let l = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0]], "infer": false}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.point.divide", json!({"curve": l, "segments": 4}));
    let points = strs(&r["points"]);
    assert_eq!(points.len(), 3, "{r}");
    assert_eq!(strs(&r["created"]["points"]), points, "{r}");
    assert_eq!(strs(&r["constraints"]).len(), 3, "{r}");
    let sk = sketch(&s);
    for (k, id) in points.iter().enumerate() {
        let want = Vec2::new(25.0 * (k + 1) as f64, 0.0);
        assert!(pt(&sk, id).dist(want) < 1e-9, "{id}: {:?}", pt(&sk, id));
    }
    // The line itself is unchanged: still one line between the same two points.
    assert_eq!(sk.curves.len(), 1);
    assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::PointAlong { .. })).count(), 3);

    // Lengthen and turn the line: the points keep their quarters.
    run(&mut s, "sketch.dimension", json!({"entities": [l], "type": "length", "value": 200}));
    let (_, b) = line_ends(&sketch(&s), &l);
    run(&mut s, "sketch.drag", json!({"entity": format!("{l}.end"), "from": [b.x, b.y], "to": [b.x, b.y + 80.0]}));
    let sk = sketch(&s);
    let (a, b) = line_ends(&sk, &l);
    assert!((a.dist(b) - 200.0).abs() < 1e-6, "{a:?} {b:?}");
    assert!(b.y.abs() > 1.0, "the drag turned the line: {b:?}");
    for (k, id) in points.iter().enumerate() {
        let want = a + (b - a) * ((k + 1) as f64 / 4.0);
        assert!(pt(&sk, id).dist(want) < 1e-6, "{id}: {:?} want {want:?}", pt(&sk, id));
    }
}

#[test]
fn an_arc_is_divided_by_its_length_and_a_circle_all_the_way_round() {
    let mut s = new_sketch();
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [20, 0], "sweep": 90, "id": "arc"}));
    let r = run(&mut s, "sketch.point.divide", json!({"curve": "arc", "segments": 3}));
    let sk = sketch(&s);
    let points = strs(&r["points"]);
    assert_eq!(points.len(), 2, "{r}");
    for (k, id) in points.iter().enumerate() {
        let want = Vec2::from_angle(std::f64::consts::FRAC_PI_6 * (k + 1) as f64) * 20.0;
        assert!(pt(&sk, id).dist(want) < 1e-6, "{id}: {:?} want {want:?}", pt(&sk, id));
    }

    run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 10, "id": "c"}));
    let r = run(&mut s, "sketch.point.divide", json!({"curves": ["c"], "segments": 4}));
    let points = strs(&r["points"]);
    assert_eq!(points.len(), 4, "a closed curve gets as many points as pieces: {r}");
    run(&mut s, "sketch.dimension", json!({"entities": ["c"], "type": "diameter", "value": 30}));
    let sk = sketch(&s);
    let ci = sk.curve_index("c").unwrap();
    let CurveKind::Circle { c, .. } = sk.curves[ci].kind else { panic!() };
    let centre = sk.point(c).unwrap();
    for (k, id) in points.iter().enumerate() {
        let want = centre + Vec2::from_angle(std::f64::consts::FRAC_PI_2 * k as f64) * 15.0;
        assert!(pt(&sk, id).dist(want) < 1e-6, "{id}: {:?} want {want:?}", pt(&sk, id));
    }
}

#[test]
fn a_spline_is_divided_into_equal_lengths_along_it() {
    let mut s = new_sketch();
    // Fit points bunched at one end: equal parameter steps would not be equal lengths.
    let sp = strs(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [2, 3], [4, 4], [40, 10], [80, -10]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.point.divide", json!({"curve": sp, "segments": 5}));
    let points = strs(&r["points"]);
    assert_eq!(points.len(), 4, "{r}");
    let check = |sk: &Sketch| {
        let poly = sk.polyline(sk.curve_index(&sp).unwrap());
        let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
        for (k, id) in points.iter().enumerate() {
            let (along, off) = station(&poly, pt(sk, id));
            assert!(off < 0.05, "{id} is {off} off the spline");
            let want = total * (k + 1) as f64 / 5.0;
            assert!((along - want).abs() < 0.005 * total, "{id} at {along} of {total}, want {want}");
        }
    };
    check(&sketch(&s));
    // Move a fit point: the points slide to the new equal lengths.
    run(&mut s, "sketch.drag", json!({"entity": format!("{sp}.end"), "from": [80, -10], "to": [60, 30]}));
    let sk = sketch(&s);
    assert!(pt(&sk, &format!("{sp}.end")).dist(Vec2::new(60.0, 30.0)) < 1e-6);
    check(&sk);
}

#[test]
fn breaking_and_trimming_a_divided_spline_use_only_the_picked_piece() {
    for command in ["sketch.break", "sketch.trim"] {
        let mut s = new_sketch();
        let input = json!({"points": [[0, 0], [10, 8], [20, 0], [30, 8], [40, 0]], "connect": false});
        let spline = strs(&run(&mut s, "sketch.spline.fit_point", input)["curves"])[0].clone();
        let marks = strs(&run(&mut s, "sketch.point.divide", json!({"curve": spline, "segments": 4}))["points"]);
        let before = sketch(&s);
        let ci = before.curve_index(&spline).unwrap();
        let pick = solidcraft_sketch::point_along(&before, ci, 0.36).unwrap();
        run(&mut s, "sketch.point", json!({"point": [pick.x, pick.y], "id": "free", "connect": false}));
        let before = sketch(&s);
        let preview = crate::cmd::trim_piece(&before, &spline, pick).unwrap();
        assert!(preview.first().unwrap().dist(pt(&before, &marks[0])) < 1e-6);
        assert!(preview.last().unwrap().dist(pt(&before, &marks[1])) < 1e-6);

        let r = run(&mut s, command, json!({"curve": spline, "at": [pick.x, pick.y]}));
        let pieces = strs(if command == "sketch.break" { &r["curves"] } else { &r["result"]["pieces"] });
        assert_eq!(pieces.len(), if command == "sketch.break" { 3 } else { 2 }, "{r}");
        let sk = sketch(&s);
        for mark in &marks[..2] {
            let p = sk.resolve_point(mark).unwrap();
            let ends = pieces.iter().filter_map(|id| sk.curves[sk.curve_index(id).unwrap()].kind.ends());
            assert_eq!(ends.filter(|(a, b)| *a == p || *b == p).count(), if command == "sketch.break" { 2 } else { 1 });
            assert!(pt(&sk, mark).dist(pt(&before, mark)) < 1e-6);
        }
        let last = sk.resolve_point(&marks[2]).unwrap();
        assert!(
            sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { p, c } if p == last && pieces.contains(&sk.curves[c].id)))
        );
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(sketch(&s), before);
    }
}

#[test]
fn a_division_point_and_a_spline_crossing_bound_the_same_piece() {
    let mut s = new_sketch();
    let input = json!({"points": [[0, 0], [4, 12], [8, 12], [12, 0]], "connect": false});
    let spline = strs(&run(&mut s, "sketch.spline.control_point", input)["curves"])[0].clone();
    let middle = strs(&run(&mut s, "sketch.point.divide", json!({"curve": spline, "segments": 2}))["points"])[0].clone();
    // The vertical separation is 16(t-1/4)(t-3/4); they meet at x=3 and x=9.
    let y = 29.0 / 3.0;
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0, 3], [4, y], [8, y], [12, 3]], "connect": false}));
    let pick = Vec2::new(4.5, 36.0 * 0.375 * 0.625);
    let preview = crate::cmd::trim_piece(&sketch(&s), &spline, pick).unwrap();
    assert!(preview.first().unwrap().dist(Vec2::new(3.0, 6.75)) < 1e-6);
    assert!(preview.last().unwrap().dist(pt(&sketch(&s), &middle)) < 1e-6);
    let r = run(&mut s, "sketch.break", json!({"curve": spline, "at": [pick.x, pick.y]}));
    let pieces = strs(&r["curves"]);
    assert_eq!(pieces.len(), 3);
    let sk = sketch(&s);
    let (a, b) = sk.curves[sk.curve_index(&pieces[1]).unwrap()].kind.ends().unwrap();
    assert!(sk.point(a).unwrap().dist(Vec2::new(3.0, 6.75)) < 1e-6);
    assert_eq!(b, sk.resolve_point(&middle).unwrap());
}

#[test]
fn a_closed_spline_keeps_the_piece_across_its_parameter_seam() {
    let mut s = new_sketch();
    let params = json!({"points": [[10, 0], [0, 8], [-10, 0], [0, -8]], "periodic": true, "connect": false});
    let id = strs(&run(&mut s, "sketch.spline.fit_point", params)["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&id).unwrap();
    let mark = |s: &mut Session, fraction| {
        let q = solidcraft_sketch::point_along(&sk, ci, fraction).unwrap();
        run(s, "sketch.point", json!({"point": format!("on:{id}:{},{}", q.x, q.y)}))["point"].as_str().unwrap().to_owned()
    };
    let near_start = mark(&mut s, 0.125);
    let other = mark(&mut s, 0.5);
    let near_end = mark(&mut s, 0.875);
    let seam = solidcraft_sketch::point_along(&sk, ci, 0.0).unwrap();
    let sk = sketch(&s);
    let preview = crate::cmd::trim_piece(&sk, &id, seam).unwrap();
    assert!(preview.first().unwrap().dist(pt(&sk, &near_end)) < 1e-6);
    assert!(preview.last().unwrap().dist(pt(&sk, &near_start)) < 1e-6);
    let r = run(&mut s, "sketch.break", json!({"curve": id, "at": [seam.x, seam.y]}));
    let parts = strs(&r["curves"]);
    assert_eq!(parts.len(), 2, "{r}");
    let sk = sketch(&s);
    let other = sk.resolve_point(&other).unwrap();
    assert!(sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { p, c } if p == other && parts.contains(&sk.curves[c].id))));
}

#[test]
fn several_curves_divide_in_one_undo_step() {
    let mut s = new_sketch();
    let a = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "infer": false}))["curves"])[0].clone();
    let b = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [30, 20]], "infer": false}))["curves"])[0].clone();
    let before = sketch(&s).points.len();
    let r = run(&mut s, "sketch.point.divide", json!({"curves": [a, b], "segments": 10}));
    assert_eq!(strs(&r["points"]).len(), 18, "{r}");
    assert_eq!(sketch(&s).points.len(), before + 18);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sketch(&s).points.len(), before);
}

#[test]
fn breaking_a_divided_line_leaves_its_points_on_it() {
    let mut s = new_sketch();
    let l = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0]], "infer": false}))["curves"])[0].clone();
    let points = strs(&run(&mut s, "sketch.point.divide", json!({"curve": l, "segments": 4}))["points"]);
    run(&mut s, "sketch.break", json!({"curve": l, "points": [[60, 0]]}));
    let sk = sketch(&s);
    assert!(sk.constraints.iter().all(|c| !matches!(c.kind, ConstraintKind::PointAlong { .. })), "the shares of the old length no longer hold");
    for (k, id) in points.iter().enumerate() {
        assert!(pt(&sk, id).dist(Vec2::new(25.0 * (k + 1) as f64, 0.0)) < 1e-6, "{id} moved: {:?}", pt(&sk, id));
    }
}

#[test]
fn hostile_divide_parameters_are_refused() {
    let mut s = new_sketch();
    let l = strs(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0]], "infer": false}))["curves"])[0].clone();
    let before = sketch(&s);
    for p in [
        json!({"curve": l, "segments": 1}),
        json!({"curve": l, "segments": 0}),
        json!({"curve": l, "segments": -3}),
        json!({"curve": l, "segments": 2.5}),
        json!({"curve": l, "segments": 1e300}),
        json!({"curve": l, "segments": 501}),
        json!({"curve": l, "segments": "four"}),
        json!({"curve": l}),
        json!({"segments": 4}),
        json!({"curve": "nope", "segments": 4}),
        json!({"curve": 7, "segments": 4}),
        json!({"curves": [l, 3], "segments": 4}),
        json!({"curves": vec![l.clone(); 201], "segments": 4}),
    ] {
        assert!(s.execute("sketch.point.divide", &p).is_err(), "{p}");
        assert_eq!(sketch(&s), before, "{p} changed the sketch");
    }
    // Five hundred segments on each of five curves is more points than one command adds.
    let many: Vec<String> = (0..5)
        .map(|i| strs(&run(&mut s, "sketch.line", json!({"points": [[0, i * 5 + 10], [50, i * 5 + 10]], "infer": false}))["curves"])[0].clone())
        .collect();
    assert!(s.execute("sketch.point.divide", &json!({"curves": many, "segments": 500})).is_err());
}

#[test]
fn a_share_out_of_range_is_not_a_constraint() {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    let l = sk.add_curve(CurveKind::Line { a, b }, None).unwrap();
    let p = sk.add_point(Vec2::new(5.0, 0.0), None).unwrap();
    for at in [f64::NAN, f64::INFINITY, -0.1, 1.5] {
        assert!(sk.add_constraint(ConstraintKind::PointAlong { p, c: l, at }, None).is_err(), "{at}");
    }
    assert!(sk.add_constraint(ConstraintKind::PointAlong { p: a, c: l, at: 0.5 }, None).is_err(), "a line's own end");
    assert!(sk.add_constraint(ConstraintKind::PointAlong { p, c: l, at: 0.5 }, None).is_ok());
}
