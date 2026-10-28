//! Grid lines: as many in each knot span as its curvature needs for the tolerance.

use crate::surface::NurbsSurface;
use crate::MeshError;

/// A parameter direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Along u: lines of constant u.
    U,
    /// Along v: lines of constant v.
    V,
}

/// Grid lines in one direction.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLines {
    /// Parameter values, increasing, from the domain's start to its end.
    pub lines: Vec<f64>,
    /// Knot spans whose curvature asked for more than `max_per_span` lines. Their chords may
    /// sag past the tolerance.
    pub capped_spans: usize,
}

/// Parameter values along `dir`, as many in each knot span as its curvature needs for chords
/// within `tol`, no two further apart than `max_step`, and at most `max_per_span` per span.
///
/// The bound: the second derivative of a B-spline is the B-spline of its second-derivative
/// control points, so on one span |C''| is at most the largest of the `p - 1` that span uses,
/// and a chord of parameter length h sags at most h^2 |C''| / 8. Every iso-curve in `dir` is a
/// convex combination of the rows of poles, so the largest over all rows bounds them all.
///
/// For a rational surface the same bound is taken over the Cartesian poles. That is an estimate,
/// not a bound: weights far from 1 can bend the surface more than its poles say. Measure such a
/// mesh with [`deviation`](crate::deviation).
pub fn grid_lines(
    s: &NurbsSurface,
    dir: Dir,
    tol: f64,
    max_step: f64,
    max_per_span: usize,
) -> Result<GridLines, MeshError> {
    let (n, k) = match dir {
        Dir::U => (s.nu, &s.u_knots),
        Dir::V => (s.nv, &s.v_knots),
    };
    let p = match dir {
        Dir::U => s.u_degree,
        Dir::V => s.v_degree,
    };
    let knot = |i: usize| k.get(i).copied().ok_or(MeshError::Internal);
    let mut lines = Vec::new();
    let mut capped_spans = 0;
    for span in p..n {
        let (a, b) = (knot(span)?, knot(span + 1)?);
        if b <= a {
            continue;
        }
        let mut span_lines = vec![a];
        let mut segments = 0usize;
        let cap = max_per_span.max(1);
        let mut capped = false;
        push_span_lines(
            s,
            dir,
            a,
            b,
            tol,
            max_step,
            cap,
            &mut span_lines,
            &mut segments,
            &mut capped,
        )?;
        if capped {
            capped_spans += 1;
        }
        lines.extend(span_lines);
    }
    lines.push(knot(n)?);
    lines.dedup_by(|a, b| (*a - *b).abs() <= 1.0e-12 * a.abs().max(b.abs()).max(1.0));
    Ok(GridLines {
        lines,
        capped_spans,
    })
}

#[allow(clippy::too_many_arguments)]
fn push_span_lines(
    s: &NurbsSurface,
    dir: Dir,
    lo: f64,
    hi: f64,
    tol: f64,
    max_step: f64,
    cap: usize,
    lines: &mut Vec<f64>,
    segments: &mut usize,
    capped: &mut bool,
) -> Result<(), MeshError> {
    if *segments >= cap.saturating_sub(1) {
        *capped = true;
        lines.push(hi);
        *segments += 1;
        return Ok(());
    }
    let step_too_long = hi - lo > max_step;
    let sag = sampled_sag(s, dir, lo, hi)?;
    if (sag > tol || step_too_long) && hi - lo > 1.0e-12 {
        let mid = (lo + hi) * 0.5;
        push_span_lines(s, dir, lo, mid, tol, max_step, cap, lines, segments, capped)?;
        push_span_lines(s, dir, mid, hi, tol, max_step, cap, lines, segments, capped)?;
    } else {
        lines.push(hi);
        *segments += 1;
    }
    Ok(())
}

fn sampled_sag(s: &NurbsSurface, dir: Dir, lo: f64, hi: f64) -> Result<f64, MeshError> {
    let mid = (lo + hi) * 0.5;
    let [[u0, u1], [v0, v1]] = s.domain();
    let mut sag = 0.0f64;
    for step in 0..=4 {
        let other = (step as f64) / 4.0;
        let (a, m, b) = match dir {
            Dir::U => {
                let v = v0 + (v1 - v0) * other;
                (
                    s.evaluate(lo, v)?.point,
                    s.evaluate(mid, v)?.point,
                    s.evaluate(hi, v)?.point,
                )
            }
            Dir::V => {
                let u = u0 + (u1 - u0) * other;
                (
                    s.evaluate(u, lo)?.point,
                    s.evaluate(u, mid)?.point,
                    s.evaluate(u, hi)?.point,
                )
            }
        };
        let chord = [
            (a[0] + b[0]) * 0.5 - m[0],
            (a[1] + b[1]) * 0.5 - m[1],
            (a[2] + b[2]) * 0.5 - m[2],
        ];
        sag = sag.max((chord[0] * chord[0] + chord[1] * chord[1] + chord[2] * chord[2]).sqrt());
    }
    Ok(sag)
}
