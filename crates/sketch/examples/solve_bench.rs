//! Solve times of an n×n grid (origin corner fixed, every row and column edge horizontal or
//! vertical with a distance), started at the solution and ±1 mm off it, and of one held drag
//! frame on a strip of rectangles. Run with
//! `cargo run --release -p solidcraft-sketch --example solve_bench`; results go in docs/perf.md.

use std::time::Instant;

use solidcraft_geom::Vec2;
use solidcraft_sketch::{Analysis, Budget, ConstraintKind, Hold, PointIdx, Sketch, SolveReport, solve, solve_with};

/// A small deterministic generator for the start offsets.
struct Lcg(u64);

impl Lcg {
    /// In [-1, 1).
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }
}

fn grid(n: usize, jitter: f64) -> Result<Sketch, Box<dyn std::error::Error>> {
    let mut sk = Sketch::new();
    let mut rnd = Lcg(12345);
    let mut id = vec![vec![PointIdx::ORIGIN; n]; n];
    for (r, row) in id.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            let j = if r == 0 && c == 0 { Vec2::ZERO } else { Vec2::new(rnd.next(), rnd.next()) * jitter };
            *slot = sk.add_point(Vec2::new(c as f64 * 10.0, r as f64 * 10.0) + j, None)?;
        }
    }
    sk.add_constraint(ConstraintKind::Fix { p: id[0][0] }, None)?;
    for r in 0..n {
        for c in 0..n {
            for (dr, dc, horizontal) in [(0, 1, true), (1, 0, false)] {
                let (Some(&p), Some(&q)) = (id.get(r).and_then(|x| x.get(c)), id.get(r + dr).and_then(|x| x.get(c + dc))) else { continue };
                let l = sk.add_line_pts(p, q, None)?;
                sk.add_constraint(if horizontal { ConstraintKind::Horizontal { l } } else { ConstraintKind::Vertical { l } }, None)?;
                sk.add_constraint(ConstraintKind::Distance { p, q, value: 10.0 }, None)?;
            }
        }
    }
    Ok(sk)
}

/// `n` rectangles side by side sharing their vertical edges: horizontal and vertical sides, every
/// width dimensioned, the first height too. Returns the sketch and its first corner.
fn strip(n: usize) -> Result<(Sketch, PointIdx), Box<dyn std::error::Error>> {
    let mut sk = Sketch::new();
    let bottom: Vec<PointIdx> = (0..=n).map(|i| sk.add_point(Vec2::new(10.0 + i as f64 * 20.0, 10.0), None)).collect::<Result<_, _>>()?;
    let top: Vec<PointIdx> = (0..=n).map(|i| sk.add_point(Vec2::new(10.0 + i as f64 * 20.0, 25.0), None)).collect::<Result<_, _>>()?;
    for i in 0..=n {
        let l = sk.add_line_pts(bottom[i], top[i], None)?;
        sk.add_constraint(ConstraintKind::Vertical { l }, None)?;
        if i == 0 {
            sk.add_constraint(ConstraintKind::Length { l, value: 15.0 }, None)?;
        }
    }
    for i in 0..n {
        let b = sk.add_line_pts(bottom[i], bottom[i + 1], None)?;
        let t = sk.add_line_pts(top[i], top[i + 1], None)?;
        sk.add_constraint(ConstraintKind::Horizontal { l: b }, None)?;
        sk.add_constraint(ConstraintKind::Horizontal { l: t }, None)?;
        sk.add_constraint(ConstraintKind::Length { l: b, value: 20.0 }, None)?;
    }
    Ok((sk, bottom[0]))
}

/// Best of `reps` solves of a copy of `sk` (ms) and the last report.
fn time(sk: &Sketch, reps: usize, f: impl Fn(&mut Sketch) -> SolveReport) -> (f64, SolveReport) {
    let mut best = f64::MAX;
    let mut rep = None;
    for _ in 0..reps {
        let mut k = sk.clone();
        let t = Instant::now();
        let r = f(&mut k);
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
        rep = Some(r);
    }
    (best, rep.unwrap_or_else(|| solve(&mut sk.clone())))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("| grid | points | start | ms | status | dof | iterations |");
    println!("|---|---|---|---|---|---|---|");
    for n in [10usize, 15, 20] {
        for jitter in [0.0, 1.0] {
            let sk = grid(n, jitter)?;
            let (ms, rep) = time(&sk, 5, solve);
            let start = if jitter == 0.0 { "at solution" } else { "±1 mm" };
            println!("| {n}×{n} | {} | {start} | {ms:.2} | {:?} | {} | {} |", n * n, rep.status, rep.dof, rep.iterations);
        }
    }
    println!();
    println!("| strip | points | held drag frame ms | full solve ms |");
    println!("|---|---|---|---|");
    for n in [30usize, 100] {
        let (mut sk, corner) = strip(n)?;
        if let Some(p) = sk.points.get_mut(corner) {
            p.pos -= Vec2::new(0.5, 0.0);
        }
        let hold = Hold { points: vec![corner], radii: vec![] };
        let (frame, rep) = time(&sk, 5, |k| solve_with(k, &hold, None, Analysis::StatusOnly, Some(&mut Budget::frame())));
        let (full, _) = time(&sk, 5, solve);
        println!("| {n} rectangles | {} | {frame:.2} ({:?}) | {full:.2} |", sk.points.len(), rep.status);
    }
    Ok(())
}
