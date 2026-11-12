//! A curve from two views: the 3D curves seen as one curve along one direction and as another
//! along a second, where the two curves' extrusions along their directions meet.
//!
//! Upstream: none.
//!
//! | Rust                    | Upstream        |
//! |-------------------------|-----------------|
//! | [`curves_from_views`]   | (not upstream)  |
//! | [`views_do_not_meet`]   | (not upstream)  |
//!
//! Each curve is extruded along its unit direction both ways from itself, far enough that no
//! meeting point is left out, and the two extrusions are intersected
//! ([`intersect_surfaces_within`]) at the kernel's default approximation tolerance
//! ([`approx_tol3d`] of [`USE_DEFAULT`]: half its zone tolerance). The intersector's own default,
//! a thousandth of the surfaces' size, is not taken: these sheets reach far past the curves, so
//! at model scale it is a thousand times coarser than the curves need.
//!
//! How far is a bound, not a search: a point `X` seen on both curves is
//! `X = p + s â = q + t b̂` with `p` on the first curve and `q` on the second. Its component
//! across `b̂` gives `s (â - (â·b̂) b̂) = (q - p) - ((q - p)·b̂) b̂`, so `|s| sin θ <= |q - p| <= D`,
//! with `θ` the angle between the directions and `D` the diagonal of the box around both
//! curves' control polygons (a NURBS curve with positive weights lies in the convex hull of its
//! control points, so in that box); `|t|` likewise. Each extrusion
//! reaches [`REACH_MARGIN`] times `D / sin θ` on either side, so the meeting curves end on the
//! curves' own ends, never on an extrusion's far edge.
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::curve::curve::Curve;
use crate::curve::from_parameters::b_spline_part;
use crate::error::intersect::IntersectFailure;
use crate::error::{Result, SolidError};
use crate::geom::tol::{approx_tol3d, USE_DEFAULT};
use crate::geom::vector3d::Vec3;
use crate::intersect::api_intersectors::{intersect_surfaces_within, NO_SURFACE_INTERSECTION};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_RAD;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::geom::MinMaxBox;
use crate::surface::surface::Surface;
use crate::sweep::api_surfaces::create_extrude;

const OP: &str = "intersect::views::curves_from_views";

/// The phrase of [`curves_from_views`]'s failure when the two views have no point in common
/// (read with [`views_do_not_meet`]).
pub const NO_MEETING: &str = "the views do not meet";

/// How many times the bound on a meeting point's distance from its curve each extrusion
/// reaches on either side of the curve: past the bound by as much again, so that no meeting
/// curve ends on an extrusion's far edge.
pub const REACH_MARGIN: f64 = 2.0;

/// The 3D curves seen as `a` along `a_dir` and as `b` along `b_dir`: where `a` extruded along
/// `a_dir` meets `b` extruded along `b_dir`, both ways from each curve (the module docs give
/// how far). Each curve is returned as a B-spline, as the intersector traces it to the kernel's
/// default approximation tolerance ([`approx_tol3d`] of [`USE_DEFAULT`]) of both extrusions: each
/// of its spans checked at interior samples against both (docs/ssi.md, §4.3), not certified.
/// The directions need not be
/// unit vectors, nor the curves planar: a curve that is not flat across its direction is seen
/// as its shadow along it.
///
/// # Errors
///
/// `InvalidInput` for a direction not finite or shorter than the kernel's zero, directions
/// within [`EFF_ZERO_RAD`] of parallel (either way), or curves whose box has no size or one
/// not finite; [`crate::SolidError::Failed`] with [`IntersectFailure::Failed`] of
/// [`NO_MEETING`] when the extrusions do not meet ([`views_do_not_meet`]); `Unsupported` for a
/// meeting curve with no B-spline form; those of the extrusions and of
/// [`intersect_surfaces_within`].
pub fn curves_from_views(
    k: &Kernel,
    a: &NurbCurve,
    a_dir: Vec3,
    b: &NurbCurve,
    b_dir: Vec3,
) -> Result<Vec<Curve>> {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if !finite(a_dir) || !finite(b_dir) {
        return Err(SolidError::invalid_input(OP, "a direction not finite"));
    }
    let (a_unit, b_unit) = (a_dir.unitize()?, b_dir.unitize()?);
    let sine = a_unit.cross(b_unit).length();
    if !(sine >= EFF_ZERO_RAD) {
        return Err(SolidError::invalid_input(OP, "the directions are parallel"));
    }
    let span = union(&a.bbox()?, &b.bbox()?).diagonal();
    if !(span.is_finite() && span > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the curves' box has no size or is not finite",
        ));
    }
    let reach = REACH_MARGIN * span / sine;
    if !reach.is_finite() {
        return Err(SolidError::invalid_input(
            OP,
            "the extrusions' reach is not finite",
        ));
    }
    let a_sweep = both_ways(a, a_unit, reach)?;
    let b_sweep = both_ways(b, b_unit, reach)?;
    let tol = approx_tol3d(k, USE_DEFAULT);
    let (curves, status) = intersect_surfaces_within(k, &a_sweep, &b_sweep, Some(tol));
    match status {
        Ok(()) => curves.iter().map(|c| spline(k, c)).collect(),
        Err(e)
            if curves.is_empty()
                && e.intersect_failure()
                    == Some(&IntersectFailure::Failed(NO_SURFACE_INTERSECTION)) =>
        {
            Err(SolidError::intersect_failed(OP, NO_MEETING))
        }
        Err(e) => Err(e),
    }
}

/// Whether `e` is [`curves_from_views`]'s refusal of two views that do not meet.
#[must_use]
pub fn views_do_not_meet(e: &SolidError) -> bool {
    e.intersect_failure() == Some(&IntersectFailure::Failed(NO_MEETING))
}

/// The smallest box holding both boxes.
fn union(a: &MinMaxBox, b: &MinMaxBox) -> MinMaxBox {
    MinMaxBox::new(
        a.xl.min(b.xl),
        a.xr.max(b.xr),
        a.yb.min(b.yb),
        a.yt.max(b.yt),
        a.zn.min(b.zn),
        a.zf.max(b.zf),
    )
}

/// `curve` extruded along the unit vector `unit` from `-reach` to `reach`: a copy moved back by
/// `reach`, swept forward by twice it ([`create_extrude`]: linear in u, the curve's parameter
/// in v).
fn both_ways(curve: &NurbCurve, unit: Vec3, reach: f64) -> Result<Surface> {
    let mut start = curve.clone();
    start.translate(unit * -reach);
    let mut start = BSplineCurve::from_nurb(start, 3, false);
    Ok(Surface::from(create_extrude(
        &mut start,
        unit * (2.0 * reach),
    )?))
}

/// The meeting curve `c` as a B-spline ([`b_spline_part`]: a traced curve is one already, a
/// line made of its two points).
fn spline(k: &Kernel, c: &Curve) -> Result<Curve> {
    let base = b_spline_part(k, c)?.ok_or(SolidError::Unsupported {
        operation: OP,
        reason: "a meeting curve with no B-spline form",
    })?;
    Ok(Curve::from(base.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::knots::Side;
    use crate::nurbs::math::{CPoint, NOW};

    /// The cubic Bézier curve on `points`.
    fn bezier(points: [[f64; 3]; 4]) -> NurbCurve {
        let pw = points
            .iter()
            .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
            .collect();
        NurbCurve::new(pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap()
    }

    /// The side view, z = f(x) in the plane y = 0, its control points evenly spaced in x so
    /// that x = 9 t; its slope |dz/dx| is at most 1 (that of its control polygon).
    fn side() -> NurbCurve {
        bezier([
            [0.0, 0.0, 0.0],
            [3.0, 0.0, 2.0],
            [6.0, 0.0, -1.0],
            [9.0, 0.0, 1.0],
        ])
    }

    /// The plan view, y = g(x) in the plane z = 0, x = -3 + 15 t (wider than the side view, so
    /// the curve seen in both ends on the side view's ends); |dy/dx| at most 0.6.
    fn plan(shift: f64) -> NurbCurve {
        bezier([
            [shift - 3.0, 1.0, 0.0],
            [shift + 2.0, 4.0, 0.0],
            [shift + 7.0, 2.0, 0.0],
            [shift + 12.0, 5.0, 0.0],
        ])
    }

    #[test]
    fn a_side_and_a_plan_view_meet_in_the_curve_seen_in_both() {
        let k = Kernel::default();
        let tol = approx_tol3d(&k, USE_DEFAULT);
        let (a, b) = (side(), plan(0.0));
        let curves = curves_from_views(
            &k,
            &a,
            Vec3::new(0.0, 1.0, 0.0),
            &b,
            Vec3::new(0.0, 0.0, 3.0),
        )
        .unwrap();
        assert!(!curves.is_empty());
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in &curves {
            let iv = c.natural_interval();
            for i in 0..=64 {
                let s = iv.min() + (iv.max() - iv.min()) * f64::from(i) / 64.0;
                let x = c.evaluate_point(&k, s).unwrap();
                assert!(
                    (-tol..=9.0 + tol).contains(&x.x),
                    "past the side view: {x:?}"
                );
                (lo, hi) = (lo.min(x.x), hi.max(x.x));
                // On the side view's extrusion along y: its height there is the side view's.
                // A gap measured along z is at most the distance times sqrt(1 + slope²),
                // below 1 + slope, the slope at most 1.
                let on_a = a.eval((x.x / 9.0).clamp(0.0, 1.0), Side::Left).unwrap();
                assert!((on_a.z - x.z).abs() <= 2.0 * tol, "{x:?} against {on_a:?}");
                // On the plan view's extrusion along z, the slope at most 0.6.
                let on_b = b.eval((x.x + 3.0) / 15.0, Side::Left).unwrap();
                assert!((on_b.y - x.y).abs() <= 1.6 * tol, "{x:?} against {on_b:?}");
            }
        }
        // The whole of the side view's run is seen.
        assert!(lo <= tol && hi >= 9.0 - tol, "from {lo} to {hi}");
    }

    /// A side view in y = 0 (seen along y) and a plan view in z = 0 (seen along z) at model
    /// scale, about 170 mm across: quadratic Bézier curves, both increasing in x.
    const SIDE_Q: [[f64; 3]; 3] = [
        [-92.06693847750134, 0.0, -39.76423812596792],
        [0.0, 0.0, 18.451591211372346],
        [73.55298836314594, 0.0, 15.024361734430357],
    ];
    const PLAN_Q: [[f64; 3]; 3] = [
        [-16.758544054811637, -82.27159357644084, 0.0],
        [42.459806193714414, -83.42331426400659, 0.0],
        [76.37870944288886, -34.86770213654586, 0.0],
    ];

    /// The quadratic Bézier curve on `q`.
    fn quadratic(q: [[f64; 3]; 3]) -> NurbCurve {
        let pw = q
            .iter()
            .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
            .collect();
        NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
    }

    /// The point of the quadratic Bézier curve `q` at `t`, in Bernstein's form (none of the
    /// kernel's evaluators).
    fn bernstein(q: &[[f64; 3]; 3], t: f64) -> [f64; 3] {
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
        [0, 1, 2].map(|i| b0 * q[0][i] + b1 * q[1][i] + b2 * q[2][i])
    }

    /// Its derivative at `t`.
    fn bernstein_d(q: &[[f64; 3]; 3], t: f64) -> [f64; 3] {
        [0, 1, 2].map(|i| 2.0 * (1.0 - t) * (q[1][i] - q[0][i]) + 2.0 * t * (q[2][i] - q[1][i]))
    }

    /// The parameter of `q` (increasing in x) where its x is `x`, by bisection on [0, 1].
    fn at_x(q: &[[f64; 3]; 3], x: f64) -> f64 {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..100 {
            let m = 0.5 * (lo + hi);
            if bernstein(q, m)[0] < x {
                lo = m;
            } else {
                hi = m;
            }
        }
        0.5 * (lo + hi)
    }

    /// The point at `t` of the B-spline with the full knot vector `knots` and the homogeneous
    /// control points `pw`, by de Boor's algorithm (none of the kernel's evaluators).
    fn de_boor(knots: &[f64], pw: &[[f64; 4]], t: f64) -> [f64; 3] {
        let n = pw.len();
        let p = knots.len() - n - 1;
        let k = (p..n).rev().find(|&k| knots[k] <= t).unwrap_or(p);
        let mut d: Vec<[f64; 4]> = (0..=p).map(|j| pw[j + k - p]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let (lo, hi) = (knots[j + k - p], knots[j + 1 + k - r]);
                let alpha = (t - lo) / (hi - lo);
                d[j] = [0, 1, 2, 3].map(|i| (1.0 - alpha) * d[j - 1][i] + alpha * d[j][i]);
            }
        }
        let h = d[p];
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    /// The full knot vector and homogeneous control points of a meeting curve (a B-spline).
    fn net(c: &Curve) -> (Vec<f64>, Vec<[f64; 4]>) {
        let crate::curve::curve::CurveKind::BSpline(b) = c.kind() else {
            panic!("a B-spline: {c:?}")
        };
        let can = b.canonical().unwrap();
        let knots = can
            .knots
            .iter()
            .zip(&can.knot_multiplicities)
            .flat_map(|(k, m)| std::iter::repeat_n(*k, *m))
            .collect();
        let pw = can
            .control_points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let w = can.weights.get(i).copied().unwrap_or(1.0);
                [p.x * w, p.y * w, p.z * w, w]
            })
            .collect();
        (knots, pw)
    }

    #[test]
    fn views_at_model_scale_meet_within_the_tolerance() {
        // The intersector's own default tolerance, a thousandth of these sheets' size, left
        // this curve 5.75e-3 off its views; it is traced to the kernel's tolerance.
        let k = Kernel::default();
        let tol = approx_tol3d(&k, USE_DEFAULT);
        let (a, b) = (quadratic(SIDE_Q), quadratic(PLAN_Q));
        let curves = curves_from_views(&k, &a, Vec3::Y, &b, Vec3::Z).unwrap();
        assert!(!curves.is_empty());
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in &curves {
            let (knots, pw) = net(c);
            let p = knots.len() - pw.len() - 1;
            let (t0, t1) = (knots[p], knots[pw.len()]);
            for i in 0..=512 {
                let x = de_boor(&knots, &pw, t0 + (t1 - t0) * f64::from(i) / 512.0);
                (lo, hi) = (lo.min(x[0]), hi.max(x[0]));
                // A gap along the view's other axis is at most the distance times
                // sqrt(1 + slope²), below 1 + slope; twice the tolerance, as the intersector
                // checks its spans at interior samples.
                let s = at_x(&SIDE_Q, x[0]);
                let (q, d) = (bernstein(&SIDE_Q, s), bernstein_d(&SIDE_Q, s));
                let allowed = 2.0 * tol * (1.0 + (d[2] / d[0]).abs());
                assert!(
                    (q[2] - x[2]).abs() <= allowed,
                    "off the side view: {x:?} against {q:?}"
                );
                let s = at_x(&PLAN_Q, x[0]);
                let (q, d) = (bernstein(&PLAN_Q, s), bernstein_d(&PLAN_Q, s));
                let allowed = 2.0 * tol * (1.0 + (d[1] / d[0]).abs());
                assert!(
                    (q[1] - x[1]).abs() <= allowed,
                    "off the plan view: {x:?} against {q:?}"
                );
            }
        }
        // The overlap of the two runs is seen: from the plan view's start to the side view's end.
        assert!(
            (lo - PLAN_Q[0][0]).abs() <= 2.0 * tol && (hi - SIDE_Q[2][0]).abs() <= 2.0 * tol,
            "from {lo} to {hi}"
        );
    }

    #[test]
    fn parallel_or_zero_directions_are_refused() {
        let k = Kernel::default();
        let (a, b) = (side(), plan(0.0));
        let y = Vec3::new(0.0, 1.0, 0.0);
        for (da, db) in [
            (y, Vec3::new(0.0, -2.0, 0.0)),
            (y, y),
            (Vec3::ZERO, Vec3::Z),
            (y, Vec3::new(f64::NAN, 0.0, 1.0)),
        ] {
            let e = curves_from_views(&k, &a, da, &b, db).unwrap_err();
            assert!(matches!(e, SolidError::InvalidInput { .. }), "{e:?}");
            assert!(!views_do_not_meet(&e));
        }
    }

    #[test]
    fn views_whose_runs_do_not_overlap_do_not_meet() {
        let k = Kernel::default();
        // The plan view moved to x in [20, 35], past the side view's [0, 9].
        let e = curves_from_views(&k, &side(), Vec3::Y, &plan(23.0), Vec3::Z).unwrap_err();
        assert!(views_do_not_meet(&e), "{e:?}");
    }
}
