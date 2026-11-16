//! Scaling probe for #13844: times `create_linear_sweep` of one planar profile against its
//! side-face count. The profile is a regular N-gon of line segments, or (`holes`) a square
//! outline with N/4 square holes; both make one body of about N side faces.
//! Run: `cargo run --release -p solidcore --example linear_sweep_scaling -- [holes] [all] 250 1000 4000`
//! (`all`: every `UpstreamFixes` flag on, as SolidCraft runs).
use solidcore::curve::curve::Curve;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::{Result, Vec3};
use std::f64::consts::TAU;
use std::time::Instant;

fn line(a: (f64, f64), b: (f64, f64)) -> Result<Curve> {
    Ok(Curve::from(create_line_segment(
        Vec3::new(a.0, a.1, 0.0),
        Vec3::new(b.0, b.1, 0.0),
    )?))
}

fn polygon(points: &[(f64, f64)]) -> Result<Vec<Curve>> {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| line(a, b))
        .collect()
}

/// A regular `n`-gon of circumradius 30.
fn ngon(n: usize) -> Result<Vec<Curve>> {
    let points: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let t = TAU * i as f64 / n as f64;
            (30.0 * t.cos(), 30.0 * t.sin())
        })
        .collect();
    polygon(&points)
}

/// A square outline with `holes` square holes on a grid, each wound the other way.
fn holed_square(holes: usize) -> Result<Vec<Curve>> {
    let side = (holes as f64).sqrt().ceil().max(1.0) as usize;
    let size = 2.0 * side as f64 + 1.0;
    let mut curves = polygon(&[(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)])?;
    for h in 0..holes {
        let (x, y) = (1.0 + 2.0 * (h % side) as f64, 1.0 + 2.0 * (h / side) as f64);
        curves.extend(polygon(&[
            (x, y),
            (x, y + 1.0),
            (x + 1.0, y + 1.0),
            (x + 1.0, y),
        ])?);
    }
    Ok(curves)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let holes = args.iter().any(|a| a == "holes");
    let all = args.iter().any(|a| a == "all");
    let sizes: Vec<usize> = args.iter().filter_map(|a| a.parse().ok()).collect();
    for n in sizes {
        let curves = if holes {
            holed_square(n / 4)?
        } else {
            ngon(n)?
        };
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let mut k = Kernel::new(&KernelOptions::default());
        if all {
            k.fixes = UpstreamFixes::all();
        }
        let start = Instant::now();
        let b = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 0.035, true)?;
        let ms = start.elapsed().as_secs_f64() * 1e3;
        println!(
            "edges {:>6}  faces {:>6}  sweep {:>10.1} ms",
            curves.len(),
            b.faces().len(),
            ms
        );
    }
    Ok(())
}
