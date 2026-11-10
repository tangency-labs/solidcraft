//! Grid lines: as many in each knot span as its curvature needs for the tolerance.

use crate::surface::{norm, sub, NurbsSurface};
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
    let (p, k, n, m) = match dir {
        Dir::U => (s.u_degree, &s.u_knots, s.nu, s.nv),
        Dir::V => (s.v_degree, &s.v_knots, s.nv, s.nu),
    };
    let at = |i: usize, j: usize| match dir {
        Dir::U => i * s.nv + j,
        Dir::V => j * s.nv + i,
    };
    let knot = |i: usize| k.get(i).copied().ok_or(MeshError::Internal);
    let pole = |i: usize| s.poles.get(i).copied().ok_or(MeshError::Internal);
    // r[i]: the largest second-derivative control point i over every row.
    let mut r = vec![0.0_f64; n.saturating_sub(2)];
    if p >= 2 {
        for (i, ri) in r.iter_mut().enumerate() {
            let d1 = knot(i + p + 1)? - knot(i + 1)?;
            let d2 = knot(i + p + 2)? - knot(i + 2)?;
            let d3 = knot(i + p + 1)? - knot(i + 2)?;
            // A span of zero length is a kink, and kinks sit on knots, which are grid lines.
            if !(d1 > 0.0 && d2 > 0.0 && d3 > 0.0) {
                continue;
            }
            let pf = p as f64;
            for j in 0..m {
                let (a, b, c) = (pole(at(i, j))?, pole(at(i + 1, j))?, pole(at(i + 2, j))?);
                let [x0, y0, z0] = sub(b, a);
                let [x1, y1, z1] = sub(c, b);
                let q = |e0: f64, e1: f64| ((pf - 1.0) * (pf * e1 / d2 - pf * e0 / d1)) / d3;
                *ri = ri.max(norm([q(x0, x1), q(y0, y1), q(z0, z1)]));
            }
        }
    }
    let mut lines = Vec::new();
    let mut capped_spans = 0;
    for span in p..n {
        let (a, b) = (knot(span)?, knot(span + 1)?);
        if b <= a {
            continue;
        }
        // Span [k[span], k[span + 1]] uses second-derivative control points span - p ..= span - 2.
        let bound = if p >= 2 {
            r.get(span - p..=span - 2)
                .map_or(0.0, |w| w.iter().copied().fold(0.0, f64::max))
        } else {
            0.0
        };
        let for_curvature = ((b - a) * (bound / (8.0 * tol)).sqrt()).ceil();
        // A flat span needs no line for curvature, but a face still needs cells for its
        // boundary to cross, so no step is longer than max_step.
        let for_step = ((b - a) / max_step).ceil();
        let cap = max_per_span.max(1) as f64;
        if for_curvature > cap {
            capped_spans += 1;
        }
        let count = for_curvature.max(for_step).max(1.0).min(cap) as usize;
        lines.extend((0..count).map(|q| a + (b - a) * q as f64 / count as f64));
    }
    lines.push(knot(n)?);
    Ok(GridLines {
        lines,
        capped_spans,
    })
}
