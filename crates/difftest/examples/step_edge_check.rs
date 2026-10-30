//! Diagnostics for the STEP mapping: for every edge of every body of the given files,
//! the distance from its curve at `t_start` / `t_end` to its start / end vertex (the
//! edge's own direction), and the edges whose ends miss their vertices.
//!
//! `cargo run --profile difftest -p difftest --example step_edge_check -- <file.stp>...`

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn main() {
    for file in std::env::args().skip(1) {
        let Some(model) = std::fs::read(&file)
            .ok()
            .and_then(|b| step_import::import(&b, &step_import::Limits::default()).ok())
        else {
            println!("{file}: unreadable");
            continue;
        };
        let (mut n, mut bad) = (0usize, 0usize);
        for (bi, body) in model.bodies.iter().enumerate() {
            for (ei, e) in body.edges.iter().enumerate() {
                n += 1;
                let (Some(p0), Some(p1)) = (
                    step_import::script::edge_point(e, 0.0),
                    step_import::script::edge_point(e, 1.0),
                ) else {
                    continue;
                };
                let vs = body.vertices.get(e.start).map(|v| v.point);
                let ve = body.vertices.get(e.end).map(|v| v.point);
                let (Some(vs), Some(ve)) = (vs, ve) else {
                    continue;
                };
                let gap = dist(p0, vs).max(dist(p1, ve));
                let scale = 1e-6
                    * (1.0
                        + vs.iter()
                            .chain(ve.iter())
                            .fold(0.0f64, |a, x| a.max(x.abs())));
                if gap > scale.max(1e-4) {
                    bad += 1;
                    if bad <= 5 {
                        println!("{file} body {bi} edge {ei}: gap {gap:.3e} t=[{}, {}] same_sense={} {:?}", e.t_start, e.t_end, e.same_sense, std::mem::discriminant(&e.curve));
                    }
                }
            }
        }
        println!("{file}: edges {n}, ends off their vertices {bad}");
    }
}
