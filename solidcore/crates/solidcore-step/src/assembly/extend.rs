//! A spline patch continued past its knot range to a face's trims that run a little past it.
//!
//! A producer that holds a face on a patch bigger than the one it writes may leave the face's
//! boundary a little past the written patch's edge: its pcurves (and the vertices on them) end
//! outside the knot range, where the patch has no points. Where moving them onto the range would
//! move the surface by more than the edges' tolerance ([`super::onto_knots`] refuses), the patch is
//! continued instead: each end span's polynomial (in homogeneous coordinates, so a rational patch
//! keeps its shape) taken over a longer interval that reaches the trims. On its old range the
//! patch is the same surface, its parameterisation unchanged, so the pcurves stay as they are.
//!
//! The end span is made one Bézier segment (its inner knot raised to the degree, which leaves the
//! surface as it is), and that segment is subdivided by de Casteljau's algorithm at a parameter
//! past its end: the left piece is the segment over the longer interval. Only a short continuation
//! is made ([`MAX_EXTENSION`] of the end span), and none of a direction the patch closes in (its
//! two boundaries meet: a trim past that range wraps round instead), or where a weight of the
//! continued patch would not be positive.

use solidcore_record::brep::Surface as BSurface;

/// The farthest a patch is continued past an end, as a share of that end's span.
pub(crate) const MAX_EXTENSION: f64 = 0.05;

/// One direction's control lines: each line runs along that direction, one per index across.
pub(super) type Lines = Vec<Vec<[f64; 4]>>;

/// `surface` continued along u and v to reach `need` (`[[u_lo, u_hi], [v_lo, v_hi]]`, the box of
/// the face's pcurves), where that box runs past its knot range; `None` where it does not, where
/// the surface is no spline, or where a continuation is refused (too long, a direction the patch
/// closes in to within `tol`, a weight that would not be positive, or knots that do not fit).
pub(crate) fn extended(surface: &BSurface, need: [[f64; 2]; 2], tol: f64) -> Option<BSurface> {
    let (rational, u_knots, v_knots, grid) = grid(surface)?;
    let [need_u, need_v] = need;
    // Along u the lines are the grid's columns (one per v index); along v its rows.
    let mut columns = transpose(&grid)?;
    let u = extend_direction(u_knots, &mut columns, need_u, tol)?;
    let mut rows = transpose(&columns)?;
    let v = extend_direction(v_knots, &mut rows, need_v, tol)?;
    if u.is_none() && v.is_none() {
        return None;
    }
    let (u_knots, v_knots) = (u.unwrap_or_else(|| u_knots.clone()), v.unwrap_or_else(|| v_knots.clone()));
    from_grid(rational, u_knots, v_knots, rows)
}

/// A spline surface's knots and its control grid in homogeneous coordinates (`[i][j]`, i along
/// u), and whether it is rational; `None` for another surface.
pub(super) fn grid(surface: &BSurface) -> Option<(bool, &Vec<f64>, &Vec<f64>, Lines)> {
    Some(match surface {
        BSurface::Spline { u_knots, v_knots, points } => {
            (false, u_knots, v_knots, points.iter().map(|row| row.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect::<Lines>())
        }
        BSurface::RationalSpline { u_knots, v_knots, homogeneous } => (true, u_knots, v_knots, homogeneous.clone()),
        _ => return None,
    })
}

/// The spline surface of [`grid`]'s parts; `None` where a weight is not positive or a coordinate
/// not finite.
pub(super) fn from_grid(rational: bool, u_knots: Vec<f64>, v_knots: Vec<f64>, rows: Lines) -> Option<BSurface> {
    if rows.iter().flatten().any(|h| !(h[3] > 0.0 && h.iter().all(|x| x.is_finite()))) {
        return None;
    }
    Some(if rational {
        BSurface::RationalSpline { u_knots, v_knots, homogeneous: rows }
    } else {
        let points = rows.iter().map(|row| row.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect()).collect();
        BSurface::Spline { u_knots, v_knots, points }
    })
}

/// `grid[i][j]` as `[j][i]`; `None` for a ragged or empty grid.
pub(super) fn transpose(grid: &Lines) -> Option<Lines> {
    let width = grid.first()?.len();
    if width == 0 || grid.iter().any(|r| r.len() != width) {
        return None;
    }
    Some((0..width).map(|j| grid.iter().filter_map(|r| r.get(j).copied()).collect()).collect())
}

/// The lines continued along their direction to reach `[lo, hi]`: `Some(Some(knots))` with the
/// new knots, `Some(None)` where the range already holds it, `None` where refused.
fn extend_direction(knots: &[f64], lines: &mut Lines, [lo, hi]: [f64; 2], tol: f64) -> Option<Option<Vec<f64>>> {
    let n = lines.first()?.len();
    let p = knots.len().checked_sub(n)?.checked_sub(1)?;
    let (&start, &end) = (knots.get(p)?, knots.get(n)?);
    let (low, high) = (lo < start, hi > end);
    if !(low || high) {
        return Some(None);
    }
    if !(lo.is_finite() && hi.is_finite()) || closes(lines, tol)? {
        return None;
    }
    // Each end's limit is a share of its end span on the patch as given: continuing one end first
    // lengthens a single span, which must not let the other end reach farther.
    let (high_span, low_span) = (end - *knots.get(n - 1)?, *knots.get(p + 1)? - start);
    if (high && !(hi - end <= MAX_EXTENSION * high_span)) || (low && !(start - lo <= MAX_EXTENSION * low_span)) {
        return None;
    }
    let mut knots = knots.to_vec();
    if high {
        knots = extend_high(&knots, lines, hi)?;
    }
    if low {
        // The low end is the high end of the reversed direction.
        let mut mirrored: Vec<f64> = knots.iter().rev().map(|t| -t).collect();
        lines.iter_mut().for_each(|l| l.reverse());
        mirrored = extend_high(&mirrored, lines, -lo)?;
        lines.iter_mut().for_each(|l| l.reverse());
        knots = mirrored.iter().rev().map(|t| -t).collect();
    }
    Some(Some(knots))
}

/// Whether the patch closes in the lines' direction: its two boundary curves (each line's first
/// and last control points) are one curve to within `tol`. They are where each pair of
/// corresponding control points is within `tol` and the pairs' weights are in one ratio (to
/// rounding): each boundary point is then the same convex combination of its curve's points, so
/// the two curves lie within `tol` of each other everywhere. Boundaries whose weights are not
/// in one ratio are not taken to close, however near their control points.
pub(super) fn closes(lines: &Lines, tol: f64) -> Option<bool> {
    let euclid = |h: &[f64; 4]| [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
    let mut ratio: Option<f64> = None;
    for l in lines {
        let (s, e) = (l.first()?, l.last()?);
        let (a, b) = (euclid(s), euclid(e));
        let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        if !(d <= tol) {
            return Some(false);
        }
        let r = e[3] / s[3];
        match ratio {
            None => ratio = Some(r),
            Some(r0) if (r - r0).abs() <= 1e-12 * r0.abs().max(r.abs()) => {}
            Some(_) => return Some(false),
        }
    }
    Some(true)
}

/// Clamped `knots` and `lines` continued past their high end to `to`: the end span made one Bézier
/// segment, then taken over `[its start, to]`. `None` where the knots are not clamped there, the end
/// span is empty, or `to` is more than [`MAX_EXTENSION`] of the span past it.
fn extend_high(knots: &[f64], lines: &mut Lines, to: f64) -> Option<Vec<f64>> {
    let n = lines.first()?.len();
    let p = knots.len().checked_sub(n)?.checked_sub(1)?;
    if p == 0 {
        return None;
    }
    let end = *knots.get(n)?;
    // Clamped at the end: the last p + 1 knots are its end.
    if knots.get(n..)?.iter().any(|t| *t != end) {
        return None;
    }
    let first = *knots.get(n - 1)?;
    let span = end - first;
    if !(span > 0.0 && to > end && to - end <= MAX_EXTENSION * span) {
        return None;
    }
    // The end span's inner knot raised to the degree (Boehm's insertion, one at a time). A span
    // that starts at a clamped start is one segment already (its knot there `p + 1` times); one
    // that starts at an unclamped start is not, and is raised as an inner knot is.
    let mut knots = knots.to_vec();
    let r = knots.iter().filter(|t| **t == first).count();
    for _ in r..p {
        let k = knots.iter().rposition(|t| *t == first)?;
        let r = knots.iter().filter(|t| **t == first).count();
        for line in lines.iter_mut() {
            *line = inserted(&knots, line, p, k, r, first)?;
        }
        knots = knots.iter().take(k + 1).chain(std::iter::once(&first)).chain(knots.iter().skip(k + 1)).copied().collect();
    }
    // De Casteljau at s past 1: the left piece's polygon is each level's first point.
    let s = (to - first) / span;
    for line in lines.iter_mut() {
        let at = line.len().checked_sub(p + 1)?;
        let mut b: Vec<[f64; 4]> = line.get(at..)?.to_vec();
        let mut left = Vec::new();
        left.push(*b.first()?);
        for level in 1..=p {
            for i in 0..=(p - level) {
                let (x, y) = (*b.get(i)?, *b.get(i + 1)?);
                *b.get_mut(i)? = lerp(x, y, s);
            }
            left.push(*b.first()?);
        }
        line.truncate(at);
        line.extend(left);
    }
    let count = knots.len();
    knots.get_mut(count.checked_sub(p + 1)?..)?.iter_mut().for_each(|t| *t = to);
    Some(knots)
}

/// `line` with knot `t` inserted once (Boehm), `k` the last index of `t` in `knots` and `r` its
/// multiplicity there.
pub(super) fn inserted(knots: &[f64], line: &[[f64; 4]], p: usize, k: usize, r: usize, t: f64) -> Option<Vec<[f64; 4]>> {
    let n = line.len();
    let mut out = Vec::new();
    for i in 0..=n {
        let q = if i + p <= k {
            *line.get(i)?
        } else if i + r > k {
            *line.get(i.checked_sub(1)?)?
        } else {
            let (a, b) = (*knots.get(i)?, *knots.get(i + p)?);
            if !(b > a) {
                return None;
            }
            let alpha = (t - a) / (b - a);
            let (x, y) = (*line.get(i.checked_sub(1)?)?, *line.get(i)?);
            lerp(x, y, alpha)
        };
        out.push(q);
    }
    Some(out)
}

/// `(1 − s) x + s y`, coordinate by coordinate.
fn lerp(x: [f64; 4], y: [f64; 4], s: f64) -> [f64; 4] {
    let mut out = x;
    for (o, b) in out.iter_mut().zip(y) {
        *o = (1.0 - s) * *o + s * b;
    }
    out
}
