//! An n-sided patch bounded by curves meeting end to end round one loop, certified: the kernel's
//! fill (`N_FillNSidedHole`, [`BSplineSurface::create_n_sided_patch`]) splits each boundary
//! curve at its middle and fills the hole with N four-sided patches round a centre, each bounded
//! by halves of two boundary curves and two inner curves from their middles to the centre.
//!
//! Upstream: none (not upstream; solidcore-native). The fill is upstream's; the certificate is
//! this module's.
//!
//! | Rust          | Upstream |
//! |---------------|----------|
//! | [`n_sided`]   | none     |
//!
//! What is certified, and what is not:
//! - **The boundary**: each half of each boundary curve against the patch side it is (the side
//!   whose certified distance to it is least, either way round, both on `[0, 1]`):
//!   [`certified_distance`], plus the rounding of extracting the side and splitting the curve by
//!   knot insertion (`γ_(12 (d + 1))` of twice the largest coordinate each, a coordinate error
//!   `e` moving a point by at most `2 e`), counted in the loop's own frame (minus its first
//!   control point, each moved coordinate rounding once). The largest is returned.
//! - **The inner joins (G0)**: every patch side that is no boundary half against the nearest
//!   side of another patch, certified the same way; the largest must be within the tolerance.
//! - **Tangency across the inner curves (G1)** is the fill's aim, within its angle tolerance,
//!   and is **not** certified here.
//!
//! Deviations: none (no upstream counterpart). Non-rational curves only (a curve whose weights
//! are all one value is its polynomial form). No recursion.

use crate::error::{Result, SolidError};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_deviation::certified_distance;
use crate::nurbs::geom::Interval;
use crate::nurbs::gordon::{largest, moved, moved_surface, nurb_of, within};
use crate::nurbs::math::gamma;
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_tool::p1::extract_iso_crv;

const OP: &str = "nurbs::n_sided::n_sided";

/// The fewest curves of an n-sided loop.
pub const MIN_SIDES: usize = 3;
/// The most curves of an n-sided loop.
pub const MAX_SIDES: usize = 64;

/// The angle (degrees) within which the fill matches the cross-boundary derivatives it builds
/// across its inner curves: the fill's own tests' value.
const ANGLE_DEG: f64 = 1.0;

/// The N patches of an n-sided fill and its certificates.
#[derive(Debug, Clone)]
pub struct NSided {
    /// The patches, one per boundary curve.
    pub patches: Vec<BSplineSurface>,
    /// The largest certified distance between a boundary curve's half and its patch side.
    pub bound: f64,
    /// The largest certified distance between the two patch sides of an inner join.
    pub joins: f64,
}

/// Why a loop has no certified n-sided patch here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NSidedRefusal {
    /// A curve is rational (its weights not all one value): the fill takes polynomial curves.
    Rational,
    /// A boundary curve is not proved within the tolerance of the patches: the bound.
    PastTolerance {
        /// The largest certified distance.
        bound: f64,
    },
    /// Two patches are not proved to meet within the tolerance along an inner curve: the
    /// largest certified gap.
    Joins {
        /// The largest certified gap between two patches' sides.
        gap: f64,
    },
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `c` on `[0, 1]`, as it runs or reversed.
fn on_unit(c: &Curve, reversed: bool) -> Result<Curve> {
    let mut c = c.clone();
    if reversed {
        c.reverse()?;
    }
    c.reparam_to_interval(Interval::new(0.0, 1.0))?;
    Ok(c)
}

/// The four sides of patch `s`, each on `[0, 1]`.
fn sides(s: &Surface) -> Result<[Curve; 4]> {
    let b = s.parameter_bounds();
    Ok([
        on_unit(&extract_iso_crv(s, b.vb, Direction::U)?, false)?,
        on_unit(&extract_iso_crv(s, b.vt, Direction::U)?, false)?,
        on_unit(&extract_iso_crv(s, b.ul, Direction::V)?, false)?,
        on_unit(&extract_iso_crv(s, b.ur, Direction::V)?, false)?,
    ])
}

/// The least certified distance from `c` (on `[0, 1]`) to `side` either way round.
fn nearest(c: &Curve, side: &Curve) -> Result<f64> {
    let forward = certified_distance(c, side)?;
    let backward = certified_distance(c, &on_unit(side, true)?)?;
    Ok(forward.min(backward))
}

/// The n-sided fill of `curves` (each running on from the one before, the last back to the
/// first), its boundary and inner joins certified within `tol` (module docs).
///
/// # Errors
///
/// `InvalidInput` for fewer than [`MIN_SIDES`] or more than [`MAX_SIDES`] curves, a curve
/// without its NURB or `tol` not finite and positive; those of the fill (curves that do not
/// meet within its model tolerance, ...), the extraction, the split and the certificates.
pub fn n_sided(
    curves: &[BSplineCurve],
    tol: f64,
) -> Result<std::result::Result<NSided, NSidedRefusal>> {
    if !(MIN_SIDES..=MAX_SIDES).contains(&curves.len()) {
        return Err(bad("fewer than three or more than 64 curves"));
    }
    if !(tol.is_finite() && tol > 0.0) {
        return Err(bad("a tolerance not finite and positive"));
    }
    let nurbs: Vec<Curve> = curves.iter().map(nurb_of).collect::<Result<_>>()?;
    if nurbs.iter().any(Curve::is_rational) {
        return Ok(Err(NSidedRefusal::Rational));
    }

    // the fill, on copies of the polynomial curves (it changes its inputs)
    let mut bounding: Vec<BSplineCurve> = nurbs
        .iter()
        .map(|c| BSplineCurve::from_gw_nurb(3, c))
        .collect();
    let mut across: Vec<Option<BSplineCurve>> = bounding.iter().map(|_| None).collect();
    let patches =
        BSplineSurface::create_n_sided_patch(&mut bounding, &mut across, ANGLE_DEG, None)?;

    // the loop's own frame
    let origin = nurbs
        .first()
        .and_then(|c| c.cpts().first().copied())
        .ok_or_else(|| bad("a curve without points"))?;
    let curves_local: Vec<Curve> = nurbs
        .iter()
        .map(|c| moved(c, origin))
        .collect::<Result<_>>()?;
    let mut local: Vec<Surface> = Vec::new();
    for p in &patches {
        let s = p
            .nurb
            .as_ref()
            .ok_or_else(|| bad("a patch without its NURB"))?;
        local.push(moved_surface(s, origin)?);
    }
    let size = largest(
        curves_local
            .iter()
            .flat_map(Curve::cpts)
            .chain(local.iter().flat_map(Surface::rows).flatten()),
    );
    let degree = curves_local
        .iter()
        .map(Curve::degree)
        .chain(local.iter().map(|s| {
            let (p, q) = s.degrees();
            p.max(q)
        }))
        .max()
        .unwrap_or(0);
    // splitting a curve and extracting a side (knot insertion each), and the two moves
    let rounding = 2.0 * (4.0 * gamma(12 * (degree + 1)) * size) + 2.0 * (2.0 * gamma(1) * size);

    let all_sides: Vec<[Curve; 4]> = local.iter().map(sides).collect::<Result<_>>()?;
    // which patch sides are boundary halves
    let mut boundary_side = vec![[false; 4]; all_sides.len()];
    let mut bound = 0.0_f64;
    for c in &curves_local {
        let (a, b) = c.param_bounds();
        let (first, second) = c.split(0.5 * (a + b))?;
        for half in [first, second] {
            let half = on_unit(&half, false)?;
            let mut best: Option<(f64, usize, usize)> = None;
            for (i, four) in all_sides.iter().enumerate() {
                for (j, side) in four.iter().enumerate() {
                    let d = nearest(&half, side)?;
                    if best.is_none_or(|(b, _, _)| d < b) {
                        best = Some((d, i, j));
                    }
                }
            }
            let Some((d, i, j)) = best else {
                return Err(bad("a fill without patches"));
            };
            if let Some(flag) = boundary_side.get_mut(i).and_then(|f| f.get_mut(j)) {
                *flag = true;
            }
            bound = bound.max((d + rounding) * (1.0 + gamma(3)));
        }
    }
    if !within(bound, tol) {
        return Ok(Err(NSidedRefusal::PastTolerance { bound }));
    }

    // the inner joins: each side that is no boundary half against the nearest side of another
    // patch
    let mut joins = 0.0_f64;
    for (i, four) in all_sides.iter().enumerate() {
        for (j, side) in four.iter().enumerate() {
            if boundary_side
                .get(i)
                .and_then(|f| f.get(j))
                .copied()
                .unwrap_or(true)
            {
                continue;
            }
            let mut best = f64::INFINITY;
            for (other, theirs) in all_sides.iter().enumerate() {
                if other == i {
                    continue;
                }
                for s in theirs {
                    best = best.min(nearest(side, s)?);
                }
            }
            joins = joins.max((best + rounding) * (1.0 + gamma(3)));
        }
    }
    if !within(joins, tol) {
        return Ok(Err(NSidedRefusal::Joins { gap: joins }));
    }
    Ok(Ok(NSided {
        patches,
        bound,
        joins,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::nurbs::math::CPoint;

    /// The polynomial curve of degree `p` on `points` with clamped uniform knots.
    fn spline(points: &[[f64; 3]], p: usize) -> BSplineCurve {
        let n = points.len();
        let mut knots = vec![0.0; p + 1];
        for i in 1..n - p {
            knots.push(i as f64 / (n - p) as f64);
        }
        knots.extend(vec![1.0; p + 1]);
        let pw = points
            .iter()
            .map(|q| CPoint::from_point(Vec3::new(q[0], q[1], q[2])))
            .collect();
        BSplineCurve::from_gw_nurb(3, &Curve::new(pw, p, knots).unwrap())
    }

    /// The corners of a regular pentagon of radius 10 in z = 0.
    fn pentagon() -> Vec<[f64; 3]> {
        (0..5)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 5.0;
                [10.0 * a.cos(), 10.0 * a.sin(), 0.0]
            })
            .collect()
    }

    #[test]
    fn a_pentagon_of_lines_is_filled_flat() {
        let p = pentagon();
        let lines: Vec<BSplineCurve> = (0..5).map(|i| spline(&[p[i], p[(i + 1) % 5]], 1)).collect();
        let f = n_sided(&lines, 1e-6).unwrap().unwrap();
        assert_eq!(f.patches.len(), 5);
        assert!(
            f.bound <= 1e-9 && f.joins <= 1e-9,
            "{} {}",
            f.bound,
            f.joins
        );
        // planar data, planar patches: every control point in z = 0
        for s in &f.patches {
            for c in s.nurb.as_ref().unwrap().rows().flatten() {
                assert!(c.z.abs() < 1e-9, "{c:?}");
            }
        }
    }

    #[test]
    fn rational_curves_are_refused() {
        let p = pentagon();
        let mut curves: Vec<BSplineCurve> =
            (0..5).map(|i| spline(&[p[i], p[(i + 1) % 5]], 1)).collect();
        let pw = vec![
            CPoint::new(p[0][0], p[0][1], 0.0, 1.0),
            CPoint::new(2.0 * p[1][0], 2.0 * p[1][1], 0.0, 2.0),
        ];
        curves[0] =
            BSplineCurve::from_gw_nurb(3, &Curve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap());
        assert_eq!(
            n_sided(&curves, 1e-6).unwrap().unwrap_err(),
            NSidedRefusal::Rational
        );
    }
}
