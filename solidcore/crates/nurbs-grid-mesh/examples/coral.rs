//! Mesh one dense NURBS surface and measure it.
//!
//!     cargo run --release --example coral -- bench/coral-s204.json [tolerance=0.312] [out.obj]
//!
//! The JSON holds `uDegree`, `vDegree`, `nu`, `nv`, flat `uKnots` and `vKnots`, `poles` flat at
//! `(i * nv + j) * 3`, and optionally `weights`. The face is the whole parameter domain, its
//! boundary sampled at the midpoints of the grid's own cells (each chord within tolerance). Times
//! cover the boundary and the mesh, not reading the file. Then the same surface with five round
//! holes, to time the cut cells, and every triangle of both measured against the surface.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::f64::consts::PI;
use std::fmt::Write as _;
use std::time::Instant;

use nurbs_grid_mesh::{
    deviation, grid_lines, mesh_trimmed, Dir, LoopPoint, MeshOptions, NurbsSurface, TrimmedMesh,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: coral <surface.json> [tolerance] [out.obj]");
    let tol: f64 = args.get(2).map_or(0.312, |t| t.parse().expect("tolerance"));
    let text = std::fs::read_to_string(path).expect("read surface");
    let int = |k: &str| number(&text, k).expect(k) as usize;
    let (p, q, nu, nv) = (int("uDegree"), int("vDegree"), int("nu"), int("nv"));
    let flat = array(&text, "poles").expect("poles");
    let poles: Vec<[f64; 3]> = flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let weights = array(&text, "weights");

    let t0 = Instant::now();
    let s = NurbsSurface::new(
        p,
        q,
        array(&text, "uKnots").unwrap(),
        array(&text, "vKnots").unwrap(),
        nu,
        nv,
        poles,
        weights.as_deref(),
    )
    .expect("surface");
    println!(
        "surface: degree {p} x {q}, poles {nu} x {nv}, rational {}; built in {:.1} ms",
        s.is_rational(),
        ms(t0)
    );

    let opts = MeshOptions::new(tol);
    let mut whole = None;
    for run in 1..=3 {
        let t = Instant::now();
        let outer = boundary(&s, tol);
        let m = mesh_trimmed(&s, &[outer], false, &opts).expect("mesh");
        report(&format!("whole face, run {run}"), ms(t), &m);
        whole = Some(m);
    }
    let whole = whole.unwrap();

    let [[u0, u1], [v0, v1]] = s.domain();
    let t = Instant::now();
    let mut loops = vec![boundary(&s, tol)];
    for (cu, cv) in [
        (0.25, 0.25),
        (0.75, 0.25),
        (0.5, 0.5),
        (0.25, 0.75),
        (0.75, 0.75),
    ] {
        let hole: Vec<LoopPoint> = (0..400)
            .map(|k| {
                let a = 2.0 * PI * f64::from(k) / 400.0;
                let (u, v) = (
                    u0 + (u1 - u0) * (cu + 0.1 * a.cos()),
                    v0 + (v1 - v0) * (cv + 0.1 * a.sin()),
                );
                LoopPoint {
                    u,
                    v,
                    p: s.evaluate(u, v).unwrap().point,
                }
            })
            .collect();
        loops.push(hole);
    }
    let holed = mesh_trimmed(&s, &loops, false, &opts).expect("mesh with holes");
    report("five holes", ms(t), &holed);

    for (name, m) in [("whole face", &whole), ("five holes", &holed)] {
        let t = Instant::now();
        let d = deviation(&s, m, tol).expect("deviation");
        println!(
            "{name}: deviation over all {} triangles (7 points each): max {:.4}, mean {:.4}; {} over {tol} ({:.3}%); {:.1} s, not timed above",
            d.triangles,
            d.max,
            d.mean,
            d.over,
            100.0 * d.over as f64 / d.triangles as f64,
            ms(t) / 1000.0
        );
    }

    if let Some(out) = args.get(3) {
        std::fs::write(out, obj(&whole)).expect("write obj");
        println!("wrote {out}");
    }
}

/// The domain's rectangle, anticlockwise, sampled at its corners and at the midpoints of the
/// grid's cells along each side, so each chord spans one cell.
fn boundary(s: &NurbsSurface, tol: f64) -> Vec<LoopPoint> {
    let [[u0, u1], [v0, v1]] = s.domain();
    let mids = |dir| {
        let l = grid_lines(s, dir, tol, f64::INFINITY, 64).unwrap().lines;
        l.windows(2)
            .map(|w| (w[0] + w[1]) / 2.0)
            .collect::<Vec<_>>()
    };
    let (mu, mv) = (mids(Dir::U), mids(Dir::V));
    let mut uv = vec![(u0, v0)];
    uv.extend(mu.iter().map(|&u| (u, v0)));
    uv.push((u1, v0));
    uv.extend(mv.iter().map(|&v| (u1, v)));
    uv.push((u1, v1));
    uv.extend(mu.iter().rev().map(|&u| (u, v1)));
    uv.push((u0, v1));
    uv.extend(mv.iter().rev().map(|&v| (u0, v)));
    uv.into_iter()
        .map(|(u, v)| LoopPoint {
            u,
            v,
            p: s.evaluate(u, v).unwrap().point,
        })
        .collect()
}

fn report(name: &str, ms: f64, m: &TrimmedMesh) {
    println!(
        "{name}: {ms:.1} ms, {} triangles, {} vertices, grid {} x {} lines, capped spans {}",
        m.triangles.len(),
        m.positions.len(),
        m.grid[0],
        m.grid[1],
        m.capped_spans
    );
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn obj(m: &TrimmedMesh) -> String {
    let mut s = String::new();
    for [x, y, z] in &m.positions {
        writeln!(s, "v {x} {y} {z}").unwrap();
    }
    for [a, b, c] in &m.triangles {
        writeln!(s, "f {} {} {}", a + 1, b + 1, c + 1).unwrap();
    }
    s
}

/// The text after `"key":`, or None when the key is absent.
fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = text.get(at..)?.trim_start();
    rest.strip_prefix(':').map(str::trim_start)
}

fn number(text: &str, key: &str) -> Option<f64> {
    let v = value(text, key)?;
    let end = v.find([',', '}', ']', '\n']).unwrap_or(v.len());
    v[..end].trim().parse().ok()
}

fn array(text: &str, key: &str) -> Option<Vec<f64>> {
    let v = value(text, key)?.strip_prefix('[')?;
    let end = v.find(']')?;
    Some(
        v[..end]
            .split(',')
            .map(|x| x.trim().parse().expect("number"))
            .collect(),
    )
}
