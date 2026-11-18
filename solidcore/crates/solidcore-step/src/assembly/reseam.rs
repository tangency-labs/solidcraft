//! A closed spline patch with its seam moved off a face that lies across it.
//!
//! A patch whose two boundaries along a direction meet (a tube or a ring written as a clamped
//! B-spline, its first and last control lines the same) has a seam there, where its parameter
//! starts and ends. A face on it that lies across that seam, without a seam edge, has no one set
//! of pcurves on the patch: its loop would jump from one end of the range to the other. The patch
//! is made to start elsewhere instead, inside the widest gap the face's boundary leaves round it:
//! that parameter's knot raised to the degree (which leaves the surface as it is), the patch split
//! there, and its two pieces joined the other way round at the old seam. It is the same surface,
//! one period further on past the new start, so the face lies on it once.

use solidcore_record::brep::Surface as BSurface;

use super::extend::{self, Lines};

/// `surface` with its seam along `d` (0 for u, 1 for v) moved to `at`, inside its knot range;
/// `None` where the surface is no spline, does not close along `d` to within `tol`, has knots
/// that are not clamped or a weight that differs across the seam, or `at` is not inside its range.
pub(crate) fn reseamed(surface: &BSurface, d: usize, at: f64, tol: f64) -> Option<BSurface> {
    let (rational, u_knots, v_knots, grid) = extend::grid(surface)?;
    match d {
        // Along u the lines are the grid's columns (one per v index); along v its rows.
        0 => {
            let mut columns = extend::transpose(&grid)?;
            let u = reseam_direction(u_knots, &mut columns, at, tol)?;
            extend::from_grid(rational, u, v_knots.clone(), extend::transpose(&columns)?)
        }
        1 => {
            let mut rows = grid;
            let v = reseam_direction(v_knots, &mut rows, at, tol)?;
            extend::from_grid(rational, u_knots.clone(), v, rows)
        }
        _ => None,
    }
}

/// The middle of the widest gap that `values` (parameters along a closed direction over
/// `[a, b]`, taken round its period) leave, inside the range; `None` where there are none, or
/// where that middle is the range's own end (the seam already lies in the gap).
pub(crate) fn gap_middle(values: &[f64], [a, b]: [f64; 2]) -> Option<f64> {
    let period = b - a;
    if !(period > 0.0 && period.is_finite()) {
        return None;
    }
    let mut on: Vec<f64> = values.iter().filter(|x| x.is_finite()).map(|x| a + (x - a).rem_euclid(period)).collect();
    on.sort_by(f64::total_cmp);
    let (first, last) = (*on.first()?, *on.last()?);
    // The gap across the range's ends first, then each between neighbours.
    let mut best = (first + period - last, last);
    for w in on.windows(2) {
        match w {
            [x, y] if y - x > best.0 => best = (y - x, *x),
            _ => {}
        }
    }
    let middle = a + (best.1 + 0.5 * best.0 - a).rem_euclid(period);
    let margin = 1e-6 * period;
    (middle > a + margin && middle < b - margin).then_some(middle)
}

/// Clamped `knots` and closed `lines` started at `at`: the new knots, the lines rewritten.
fn reseam_direction(knots: &[f64], lines: &mut Lines, at: f64, tol: f64) -> Option<Vec<f64>> {
    let n = lines.first()?.len();
    let p = knots.len().checked_sub(n)?.checked_sub(1)?;
    if p == 0 {
        return None;
    }
    let (a, b) = (*knots.get(p)?, *knots.get(n)?);
    if !(a < at && at < b) {
        return None;
    }
    if knots.get(..=p)?.iter().any(|t| *t != a) || knots.get(n..)?.iter().any(|t| *t != b) {
        return None;
    }
    if !extend::closes(lines, tol)? {
        return None;
    }
    // The two ends of each line, which become one interior control point, carry one weight.
    for l in lines.iter() {
        let (s, e) = (l.first()?, l.last()?);
        if (s[3] - e[3]).abs() > 1e-12 * s[3].abs().max(e[3].abs()) {
            return None;
        }
    }
    // `at` raised to the degree (Boehm's insertion, one at a time).
    let mut knots = knots.to_vec();
    let given = knots.iter().filter(|t| **t == at).count();
    if given > p {
        return None;
    }
    for _ in given..p {
        let k = knots.iter().rposition(|t| *t <= at)?;
        let r = knots.iter().filter(|t| **t == at).count();
        for line in lines.iter_mut() {
            *line = extend::inserted(&knots, line, p, k, r, at)?;
        }
        knots = knots.iter().take(k + 1).chain(std::iter::once(&at)).chain(knots.iter().skip(k + 1)).copied().collect();
    }
    // The control point the surface passes through at `at` (its knot `p` times from `s` on).
    let s = knots.iter().position(|t| *t == at)?;
    let m = s.checked_sub(1)?;
    let count = knots.len();
    let period = b - a;
    let inner_left = knots.get(p + 1..s)?;
    let inner_right = knots.get(s + p..count.checked_sub(p + 1)?)?;
    let new_knots: Vec<f64> = std::iter::repeat_n(at, p + 1)
        .chain(inner_right.iter().copied())
        .chain(std::iter::repeat_n(b, p))
        .chain(inner_left.iter().map(|t| t + period))
        .chain(std::iter::repeat_n(at + period, p + 1))
        .collect();
    for line in lines.iter_mut() {
        let (first, last) = (*line.first()?, *line.last()?);
        // The old seam, one point of the line now: its two ends met to within `tol`.
        let mut joint = first;
        for (j, x) in joint.iter_mut().zip(last) {
            *j = 0.5 * (*j + x);
        }
        let mut out: Vec<[f64; 4]> = line.get(m..)?.to_vec();
        if let Some(x) = out.last_mut() {
            *x = joint;
        }
        out.extend_from_slice(line.get(1..=m)?);
        *line = out;
    }
    (new_knots.len() == lines.first()?.len() + p + 1).then_some(new_knots)
}
