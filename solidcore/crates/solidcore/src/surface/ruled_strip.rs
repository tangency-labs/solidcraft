//! The numbers a ruled strip along a chain of edges is checked and refined by, for the law
//! extension (a strip leaving the edges at a constant angle to their surface) and the ruled
//! surface it is built as. Each strip has an edge (its base curve) and a far edge, with one
//! parameter between them, joined by straight rulings.
//!
//! - The angle: the law extension's angle to the surface, `±`[`ANGLE_LIMIT`] at most, as the
//!   angle from the surface's normal a ruled surface takes ([`ruling_angle`]).
//! - Folding: where the far edge runs against the edge (or stands still), the rulings between
//!   have crossed and the strip overlaps itself ([`folds`], [`strip_folds`] at
//!   [`FOLD_SAMPLES`] parameters).
//! - Closeness: a free far edge is fitted to its rulings' ends; it strays when it passes more
//!   than a tolerance ([`far_tolerance`]) from them at any of [`STRAY_SAMPLES`] parameters
//!   ([`strays`]; sampled only, no whole-span bound: see its notes). The edge's spans are then
//!   halved ([`crate::nurbs::knots::span_midpoints`]), at most [`MAX_HALVINGS`] times.
//! - A straight strip whose corners leave no length along its edge ([`cut_away`]).
//!
//! Not upstream: no counterpart in SMLib. Deviations: none. No recursion.

use crate::geom::vector3d::Vec3;

/// The largest law-extension angle (radians, either way) short of a right angle: at 90° the
/// strip would stand along the surface's normal, no longer an extension of it.
pub const ANGLE_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - 1e-6;

/// How near its rulings' ends a law strip's far edge passes, in parts of the body's size and the
/// strip's length together ([`far_tolerance`]).
pub const FAR_TOLERANCE: f64 = 1e-5;

/// Parameters along a strip, ends included, where [`strip_folds`] compares its far edge with
/// its edge.
pub const FOLD_SAMPLES: usize = 64;

/// Parameters along a free strip, ends included, where [`strays`] measures its far edge against
/// its rulings' ends.
pub const STRAY_SAMPLES: usize = 64;

/// Most rounds of halving a free strip's spans while its far edge strays.
pub const MAX_HALVINGS: usize = 6;

/// The angle from the surface's normal, turned away from the face, that a ruled surface takes
/// for a law extension at `angle` to the surface (0: tangent to it, positive toward the side the
/// face's normal points to): `π/2 − angle`. `None` for an angle that is not finite or lies past
/// `±`[`ANGLE_LIMIT`].
#[must_use]
pub fn ruling_angle(angle: f64) -> Option<f64> {
    (angle.is_finite() && angle.abs() <= ANGLE_LIMIT).then_some(std::f64::consts::FRAC_PI_2 - angle)
}

/// How near its rulings' ends the far edge of a law strip `length` long, on a body of `size`,
/// must pass: [`FAR_TOLERANCE`] of the two together.
#[must_use]
pub fn far_tolerance(size: f64, length: f64) -> f64 {
    FAR_TOLERANCE * (size + length)
}

/// Whether a strip folds where its edge runs along `along` and its far edge, at the same
/// parameter, along `far`: the far edge does not run forward with the edge (its tangent's
/// component along the edge's is not positive, past rounding), so the rulings there have
/// crossed. A tangent that is not a number folds.
#[must_use]
pub fn folds(along: Vec3, far: Vec3) -> bool {
    let ahead = far.dot(along).partial_cmp(&(1e-9 * along.length_squared()));
    ahead != Some(std::cmp::Ordering::Greater)
}

/// Whether a strip folds anywhere at [`FOLD_SAMPLES`] parameters `f` from 0 to 1, `at(f)`
/// giving its edge's and its far edge's tangents there ([`folds`]). `None` where `at` cannot
/// evaluate one.
pub fn strip_folds(mut at: impl FnMut(f64) -> Option<(Vec3, Vec3)>) -> Option<bool> {
    for i in 0..=FOLD_SAMPLES {
        let (along, far) = at(i as f64 / FOLD_SAMPLES as f64)?;
        if folds(along, far) {
            return Some(true);
        }
    }
    Some(false)
}

/// Whether a far edge strays more than `tol` from its rulings' ends at any of [`STRAY_SAMPLES`]
/// parameters `f` from 0 to 1, `at(f)` giving the far edge's point and the ruling's end there.
/// `None` where `at` cannot evaluate one, where a distance is not a number, or for a tolerance
/// that is not a finite, non-negative number: nothing is accepted that was not measured.
///
/// What `Some(false)` guarantees, and what it does not (a known limit, not a certificate):
/// - Guaranteed: at each of the `STRAY_SAMPLES + 1` parameters `f = i / STRAY_SAMPLES`
///   (`i = 0 ..= 64`) the far edge lies within `tol` of the ruling's end, every distance
///   measured finite. The law extension passes `tol` = [`far_tolerance`]
///   (`FAR_TOLERANCE · (size + length)`).
/// - Only sampled: between two of those parameters nothing is measured, so a far edge that
///   leaves `tol` and comes back between samples is not caught. This is no whole-span
///   enclosure (control-point, Bernstein or interval bound) of the deviation.
/// - Why: `at` gives points only, and the rulings' ends are the edge pushed along a direction
///   taken from its face's normal (an offset-like curve with no spline form here). Bounding the
///   gap between samples needs a bound on how fast those ends move, that is on the face's
///   curvature along the edge, which this function is not given. A hull bound on the fitted far
///   edge alone would not bound the difference.
/// - Exact strips (on planes, cylinders, cones) are built in closed form and never checked here.
pub fn strays(tol: f64, mut at: impl FnMut(f64) -> Option<(Vec3, Vec3)>) -> Option<bool> {
    if !(tol.is_finite() && tol >= 0.0) {
        return None;
    }
    for i in 0..=STRAY_SAMPLES {
        let (far, end) = at(i as f64 / STRAY_SAMPLES as f64)?;
        let off = (far - end).length();
        if !off.is_finite() {
            return None;
        }
        if off > tol {
            return Some(true);
        }
    }
    Some(false)
}

/// The next, shorter length tried for a strip up to a target while it folds: half of `length`.
#[must_use]
pub fn halved_length(length: f64) -> f64 {
    0.5 * length
}

/// The length halfway between a strip `lo` long that the target missed and one `hi` long that
/// folded, tried next; halved first, so lengths near the largest finite number do not overflow.
#[must_use]
pub fn length_between(lo: f64, hi: f64) -> f64 {
    0.5 * lo + 0.5 * hi
}

/// Whether a straight strip's far edge, from corner to corner (`step`), leaves no more than
/// `eps` along its edge's direction `direction`: the corners have cut the strip away.
#[must_use]
pub fn cut_away(step: Vec3, direction: Vec3, eps: f64) -> bool {
    step.dot(direction) <= eps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_6};

    #[test]
    fn the_ruling_angle_is_measured_from_the_normal() {
        assert_eq!(ruling_angle(0.0), Some(FRAC_PI_2));
        assert_eq!(ruling_angle(FRAC_PI_6), Some(FRAC_PI_2 - FRAC_PI_6));
        assert_eq!(ruling_angle(-FRAC_PI_3), Some(FRAC_PI_2 + FRAC_PI_3));
        assert!(ruling_angle(ANGLE_LIMIT).is_some());
        for a in [FRAC_PI_2, -FRAC_PI_2, 2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(ruling_angle(a), None, "{a}");
        }
    }

    #[test]
    fn the_far_tolerance_grows_with_size_and_length() {
        assert_eq!(far_tolerance(10.0, 5.0), FAR_TOLERANCE * 15.0);
        assert!((far_tolerance(10.0, 5.0) - 1.5e-4).abs() < 1e-18);
    }

    #[test]
    fn a_far_edge_running_against_its_edge_folds() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        assert!(!folds(x, x));
        assert!(!folds(x, Vec3::new(0.5, 3.0, 0.0)));
        assert!(folds(x, Vec3::new(-0.5, 3.0, 0.0)));
        assert!(folds(x, Vec3::new(0.0, 0.0, 0.0)));
        assert!(folds(x, Vec3::new(f64::NAN, 0.0, 0.0)));
    }

    /// A quarter arc of radius 10 (tangent 10 (−sin φ, cos φ) per unit of φ = f π/2), its rulings
    /// leaning in 60° over a length `l`: the far edge is the arc of radius 10 − l sin 60°, which
    /// runs backward past l = 10 / sin 60° = 11.547 (its rulings cross on the axis).
    #[test]
    fn an_arc_leaning_in_folds_past_its_axis() {
        let strip = |l: f64| {
            let r = 10.0 - l * FRAC_PI_3.sin();
            strip_folds(|f| {
                let phi = f * FRAC_PI_2;
                let t = Vec3::new(-phi.sin(), phi.cos(), 0.0);
                Some((t * 10.0, t * r))
            })
        };
        assert_eq!(strip(5.0), Some(false));
        assert_eq!(strip(11.5), Some(false));
        assert_eq!(strip(11.6), Some(true));
        assert_eq!(strip(15.0), Some(true));
        assert_eq!(strip_folds(|_| None), None);
    }

    /// A far edge 1e-4 off its rulings' ends in the middle only (f = 1/2): it strays past 5e-5,
    /// not past 2e-4.
    #[test]
    fn a_far_edge_strays_where_it_passes_beyond_the_tolerance() {
        let at = |f: f64| {
            let off = if f == 0.5 { 1e-4 } else { 0.0 };
            Some((Vec3::new(f, off, 0.0), Vec3::new(f, 0.0, 0.0)))
        };
        assert_eq!(strays(5e-5, at), Some(true));
        assert_eq!(strays(2e-4, at), Some(false));
        assert_eq!(strays(1.0, |_| None), None);
    }

    /// A distance that is not a number (a NaN point), or a tolerance that is not a finite,
    /// non-negative number, is no measurement: `None`, never an accepted far edge. A tolerance of
    /// 0 still accepts a far edge exactly on its rulings' ends.
    #[test]
    fn a_far_edge_not_measured_is_not_accepted() {
        let on = |f: f64| Some((Vec3::new(f, 0.0, 0.0), Vec3::new(f, 0.0, 0.0)));
        let nan = |f: f64| Some((Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(f, 0.0, 0.0)));
        assert_eq!(strays(1.0, nan), None);
        for tol in [f64::NAN, f64::INFINITY, -1.0] {
            assert_eq!(strays(tol, on), None, "{tol}");
        }
        assert_eq!(strays(0.0, on), Some(false));
    }

    /// Halved: 2000 → 1000; between 7.8125 and 15.625: 11.71875 (all exact in binary); between
    /// 1e308 and 1.6e308, whose sum overflows: 1.3e308.
    #[test]
    fn lengths_are_halved_and_bisected() {
        assert_eq!(halved_length(2000.0), 1000.0);
        assert_eq!(length_between(7.8125, 15.625), 11.71875);
        assert!((length_between(1e308, 1.6e308) / 1.3e308 - 1.0).abs() < 1e-15);
    }

    #[test]
    fn corners_meeting_past_each_other_cut_a_strip_away() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        assert!(!cut_away(Vec3::new(2.0, 1.0, 0.0), x, 1e-9));
        assert!(cut_away(Vec3::new(0.0, 1.0, 0.0), x, 1e-9));
        assert!(cut_away(Vec3::new(-2.0, 0.0, 0.0), x, 1e-9));
    }
}
