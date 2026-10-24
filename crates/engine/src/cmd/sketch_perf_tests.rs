//! Timings for large sketches (run with `cargo test -p solidcraft-engine --release sketch_perf -- --ignored --nocapture`;
//! numbers are recorded in docs/perf.md).

use std::time::Instant;

use serde_json::json;

use crate::Session;
use solidcraft_sketch::{CurveIdx, PointIdx};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// A sketch of `n` constrained rectangles (4n lines, each dimensioned) plus `n` circles inside
/// them, built straight into the document.
fn big(n: usize) -> Session {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    let id = s.active_sketch.unwrap();
    let mut doc = (*s.doc).clone();
    let sk = doc.sketch_mut(id).unwrap();
    use solidcraft_geom::Vec2;
    use solidcraft_sketch::ConstraintKind::*;
    let side = (n as f64).sqrt().ceil() as usize;
    for k in 0..n {
        let (x, y) = ((k % side) as f64 * 30.0, (k / side) as f64 * 30.0);
        let p: Vec<PointIdx> = [(0.0, 0.0), (20.0, 0.3), (20.2, 15.0), (-0.1, 15.1)]
            .iter()
            .map(|(dx, dy)| sk.add_point(Vec2::new(x + dx, y + dy), None).unwrap())
            .collect();
        let l: Vec<CurveIdx> = (0..4).map(|i| sk.add_line_pts(p[i], p[(i + 1) % 4], None).unwrap()).collect();
        sk.add_constraint(Horizontal { l: l[0] }, None).unwrap();
        sk.add_constraint(Horizontal { l: l[2] }, None).unwrap();
        sk.add_constraint(Vertical { l: l[1] }, None).unwrap();
        sk.add_constraint(Vertical { l: l[3] }, None).unwrap();
        sk.add_constraint(Length { l: l[0], value: 20.0 }, None).unwrap();
        sk.add_constraint(Length { l: l[1], value: 15.0 }, None).unwrap();
        sk.add_circle(Vec2::new(x + 10.0, y + 7.5), 3.0, None, None).unwrap();
    }
    *s.doc_mut() = doc;
    s.refresh();
    s
}

#[test]
#[ignore]
fn sketch_perf() {
    for n in [250usize, 500, 1000] {
        let t = Instant::now();
        let mut s = big(n);
        let build = ms(t);
        let id = s.active_sketch.unwrap();
        let sk = s.doc.sketch(id).unwrap().clone();
        let entities = sk.curves.len() + sk.points.len();
        let t = Instant::now();
        let mut k = sk.clone();
        let rep = solidcraft_sketch::solve(&mut k);
        let solve = ms(t);
        let t = Instant::now();
        let prof = solidcraft_sketch::find_profiles(&k);
        let profiles = ms(t);
        let ss = s.model.state().sketch(id).unwrap().clone();
        let t = Instant::now();
        let lines = crate::view::sketch_lines(&ss.sketch, &ss.plane, true, &ss.report.curve_determined);
        let draw = ms(t);
        let t = Instant::now();
        s.execute("sketch.snap", &json!({"at": [10.0, 0.2], "from": [0, 0], "radius": 1})).unwrap();
        let snap = ms(t);
        let t = Instant::now();
        s.execute("sketch.line", &json!({"points": [[-50, -50], [-40, -45]]})).unwrap();
        let add = ms(t);
        let t = Instant::now();
        s.execute("sketch.move_point", &json!({"point": "p1", "to": [1, 1]})).unwrap();
        let drag = ms(t);
        eprintln!(
            "PERF n={n} curves={} points={} entities={entities} constraints={} dof={} profiles={}: build {build:.0} ms, solve {solve:.0} ms, profiles {profiles:.0} ms, draw lists {draw:.1} ms ({} polylines), snap {snap:.1} ms, add a line {add:.0} ms, drag a point {drag:.0} ms",
            sk.curves.len(),
            sk.points.len(),
            sk.constraints.len(),
            rep.dof,
            prof.len(),
            lines.len()
        );
    }
}
