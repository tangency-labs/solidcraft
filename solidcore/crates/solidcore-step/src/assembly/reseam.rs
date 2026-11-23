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
/// `None` where the surface is no spline, does not close exactly along `d` (each control line's
/// two ends the same point and weight: a patch whose ends meet within the tolerance has had them
/// made one when its face was added, `topology::closed_within`), has knots that are not clamped,
/// or `at` is not inside its range. The result is the same surface: nothing is moved.
pub(crate) fn reseamed(surface: &BSurface, d: usize, at: f64) -> Option<BSurface> {
    let (rational, u_knots, v_knots, grid) = extend::grid(surface)?;
    match d {
        // Along u the lines are the grid's columns (one per v index); along v its rows.
        0 => {
            let mut columns = extend::transpose(&grid)?;
            let u = reseam_direction(u_knots, &mut columns, at)?;
            extend::from_grid(rational, u, v_knots.clone(), extend::transpose(&columns)?)
        }
        1 => {
            let mut rows = grid;
            let v = reseam_direction(v_knots, &mut rows, at)?;
            extend::from_grid(rational, u_knots.clone(), v, rows)
        }
        _ => None,
    }
}

/// The middle of the widest gap that `spans` (intervals along a closed direction over `[a, b]`,
/// each taken round its period: a pcurve's control hull there, which encloses it) leave, inside
/// the range; `None` where there are none, where they cover a whole period, or where that middle
/// is the range's own end (the seam already lies in the gap). The gap is proved: no point of a
/// curve enclosed by a span lies in it.
pub(crate) fn gap_middle_of(spans: &[[f64; 2]], [a, b]: [f64; 2]) -> Option<f64> {
    let period = b - a;
    if !(period > 0.0 && period.is_finite()) {
        return None;
    }
    let mut on: Vec<[f64; 2]> = Vec::new();
    for &[lo, hi] in spans {
        if !(lo.is_finite() && hi.is_finite()) {
            continue;
        }
        let width = (hi - lo).max(0.0);
        if width >= period {
            return None;
        }
        let s = a + (lo.min(hi) - a).rem_euclid(period);
        let e = s + width;
        if e <= b {
            on.push([s, e]);
        } else {
            on.push([s, b]);
            on.push([a, e - period]);
        }
    }
    on.sort_by(|x, y| x[0].total_cmp(&y[0]));
    // The covered parts, merged where they meet or overlap.
    let mut merged: Vec<[f64; 2]> = Vec::new();
    for x in on {
        match merged.last_mut() {
            Some(m) if x[0] <= m[1] => m[1] = m[1].max(x[1]),
            _ => merged.push(x),
        }
    }
    let (first, last) = (merged.first()?[0], merged.last()?[1]);
    // The gap across the range's ends first, then each between neighbours.
    let mut best = (first + period - last, last);
    for w in merged.windows(2) {
        match w {
            [x, y] if y[0] - x[1] > best.0 => best = (y[0] - x[1], x[1]),
            _ => {}
        }
    }
    if !(best.0 > 0.0) {
        return None;
    }
    let middle = a + (best.1 + 0.5 * best.0 - a).rem_euclid(period);
    let margin = 1e-6 * period;
    (middle > a + margin && middle < b - margin).then_some(middle)
}

/// Clamped `knots` and closed `lines` started at `at`: the new knots, the lines rewritten.
fn reseam_direction(knots: &[f64], lines: &mut Lines, at: f64) -> Option<Vec<f64>> {
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
    // Closed exactly: each line's two ends, which become one interior control point, are one.
    if !lines.iter().all(|l| l.first() == l.last()) {
        return None;
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
        // The old seam, one point of the line now: its two ends are the same point.
        let joint = *line.first()?;
        let mut out: Vec<[f64; 4]> = line.get(m..)?.to_vec();
        if let Some(x) = out.last_mut() {
            *x = joint;
        }
        out.extend_from_slice(line.get(1..=m)?);
        *line = out;
    }
    (new_knots.len() == lines.first()?.len() + p + 1).then_some(new_knots)
}
