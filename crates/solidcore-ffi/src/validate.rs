//! Input checks shared by the unit wrappers' `validate` functions.
//!
//! Each item is the most common form of a helper that unit wrappers used to copy;
//! a unit that needs a different check keeps its own definition.

use crate::FfiError;

/// `NL_NOZ`: the `z` value marking a 2D control point (`+DBL_MAX`).
pub(crate) const NOZ: f64 = f64::MAX;
/// `NL_NOW`: the `w` value marking a non-rational control point (`-DBL_MAX`).
pub(crate) const NOW: f64 = -f64::MAX;

/// An [`FfiError::InvalidArgument`] with message `msg`.
pub(crate) fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// The Euclidean point of a control point `x y z w` ([`NOZ`] -> `z = 0`,
/// [`NOW`] -> non-rational).
pub(crate) fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// Whether `sm_CreateBezSegments` (`SmNurbsCrv.cpp`) writes through its null
/// next-segment pointer `NQw` for a B-spline curve of degree `p` with `knots` (UB-286).
/// It does when the curve has exactly one non-zero span (`N_BasisGetSpanCount`: the
/// `i` in `p..m-p` with `U[i] != U[i+1]`), so `NQw` is never set, and a knot after
/// that span's end run differs from it (an end that is not clamped), so the segment
/// loop ends with `ie < m` and copies control points through `NQw`. The curve cache
/// every B-spline curve gets (`SmCurveCache`) then segfaults. The indices are
/// upstream's: the skip over a first knot's over-multiplicity, then the end run.
pub(crate) fn bez_segments_write_null(p: usize, knots: &[f64]) -> bool {
    let Some(m) = knots.len().checked_sub(1) else {
        return false;
    };
    let u = |i: usize| knots.get(i).copied();
    let nsp = (p..m.saturating_sub(p))
        .filter(|&i| u(i) != u(i + 1))
        .count();
    if nsp != 1 {
        return false;
    }
    let mut ie = p + 1;
    while ie < m && u(ie - 1) == u(ie) {
        ie += 1;
    }
    while ie < m && u(ie) == u(ie + 1) {
        ie += 1;
    }
    ie < m
}

/// Checks a clamped knot vector of degree `p`; returns its range.
pub(crate) fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UB-286: one span and an end that is not clamped (the curve of
    /// `topology.proto_topology_p2` fuzz00353), and the curves around it that
    /// `sm_CreateBezSegments` handles.
    #[test]
    fn bez_segments_write_null_only_for_one_unclamped_span() {
        let (a, b) = (-4.774869788871685, -3.774869788871685);
        let fuzz00353 = [a, a, a, a, a, b, b, b, b, -0.0];
        assert!(bez_segments_write_null(4, &fuzz00353));
        let clamped = [a, a, a, a, a, b, b, b, b, b];
        assert!(!bez_segments_write_null(4, &clamped));
        // degree 2, one span [0, 1], then a knot past it
        assert!(bez_segments_write_null(2, &[0.0, 0.0, 0.0, 1.0, 1.0, 2.0]));
        // the domain u2..u3 is one span here too, and 2, 3 run on past it
        assert!(bez_segments_write_null(2, &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0]));
        // two spans: the first segment sets NQw (unclamped and clamped ends)
        assert!(!bez_segments_write_null(
            2,
            &[0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 3.0]
        ));
        assert!(!bez_segments_write_null(
            2,
            &[0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0]
        ));
        // no span (the caller refuses it with SM_ERR first), and no knots
        assert!(!bez_segments_write_null(1, &[0.0, 0.0, 0.0, 0.0]));
        assert!(!bez_segments_write_null(1, &[]));
    }
}
