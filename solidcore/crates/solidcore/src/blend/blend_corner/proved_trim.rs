//! Trims on rounds kept as their definitions, proved against their edges (#23151).
//!
//! Upstream: none (solidcore-native; the fork joins the two vertices of a cross section on
//! each round with `sm_CreateUVCurve`, and takes a mitre's trims from the rounds' traced
//! intersection).
//!
//! | Rust                 | Upstream |
//! |----------------------|----------|
//! | [`proved_trim_gap`]  | none     |
//! | [`held_in_domain`]   | none     |
//! | [`lifted_trim`]      | none     |
//!
//! An edge on a round is a curve in space and a trim, a curve in the round's parameters.
//! The round's face is cut where the trim runs, so the trim must map onto the edge: the
//! distance between the round at the trim and the edge, at one parameter, over the whole
//! edge, within the corner's tolerance. A check at sample points proves nothing of the
//! stretch between them (`docs/definitions-and-certificates.md` §4): the distance is
//! enclosed, by [`certified_lift`], which takes a surface that is its net. A round kept as
//! its definition S is not; its net B is proved within the definition's bound of S at
//! every parameter, so the distance is enclosed on B and the bound added.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::foot_on_surface;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::p2::ControlPointForm;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::surface::surface::exact_pieces::certified_lift;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "blend::blend_corner::proved_trim::lifted_trim";

/// The most intervals a lifted trim is cut into.
const MOST: usize = 256;

/// A proved bound on how far the trim `uv` of `surface` stands from its edge `curve`: for
/// every `t` of `ivl`, the interval the two share, `surface(uv(t))` is within the returned distance of
/// `curve(t)`. Enclosed, with its rounding ([`certified_lift`]). A round kept as its
/// definition is read on its net, and the net's proved distance from the definition is
/// added (module docs). `None` where the distance is not proved within `tol`.
///
/// # Errors
///
/// Those of the enclosure other than its refusal (`Unsupported`: a distance not proved
/// within `tol`, or a curve kind it does not take), which is `None` here.
pub(crate) fn proved_trim_gap(
    k: &Kernel,
    surface: &Surface,
    uv: &Curve,
    curve: &Curve,
    ivl: Extent1d,
    tol: f64,
) -> Result<Option<f64>> {
    let (net, bound) = match surface.kind() {
        SurfaceKind::ProceduralBlend(blend) => {
            (Some(Surface::from(blend.net().copy())), blend.bound())
        }
        _ => (None, 0.0),
    };
    // the two roundings of the budget and of the sum
    let room = (tol - bound) * (1.0 - 4.0 * f64::EPSILON);
    if room.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Ok(None);
    }
    match certified_lift(k, net.as_ref().unwrap_or(surface), uv, curve, ivl, room) {
        Ok(gap) => Ok(Some((gap + bound) * (1.0 + 4.0 * f64::EPSILON))),
        Err(SolidError::Unsupported { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Holds the fitted trim `uv` inside `domain`: every control point outside is put on the
/// side it is past, and the curve, which has no weights, then lies in the box its control
/// points lie in. A trim that touches a side of the domain along the side has a fit
/// whose control points stand past it by the fit's error, and one that ends on a side by
/// its rounding. The trim is proved against its edge after this ([`proved_trim_gap`]), so
/// a move that takes it off the edge is not accepted.
///
/// # Errors
///
/// Those of reading and writing the control points.
pub(crate) fn held_in_domain(uv: &mut BSplineCurve, domain: &Extent2d) -> Result<()> {
    let (lo, hi) = (domain.min(), domain.max());
    let (points, _) = uv.control_polygon()?;
    for (index, point) in points.into_iter().enumerate() {
        let held = Point3::new(
            point.x.max(lo.x).min(hi.x),
            point.y.max(lo.y).min(hi.y),
            0.0,
        );
        if held != point {
            uv.set_control_point(ControlPointForm::NonRational, index, held, 1.0)?;
        }
    }
    Ok(())
}

/// The trim of the edge `curve` on the round `surface` it runs across: the curve through
/// the feet of the edge's points on the round, at the edge's own parameters, cut finer
/// until its distance from the edge is proved within `tol` ([`proved_trim_gap`]). `None`
/// where a point of the edge has no foot on the round or the distance is not proved at
/// [`MOST`] intervals. The trim is held in the round's domain ([`held_in_domain`]).
///
/// # Errors
///
/// Those of the evaluations, the drop, the fit and the enclosure; cancellation.
pub(crate) fn lifted_trim(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    tol: f64,
) -> Result<Option<Curve>> {
    let ivl = curve.natural_interval();
    let (t0, t1) = (ivl.min(), ivl.max());
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let from = curve.evaluate_point(k, t0)?;
    let dropped = surface.drop_point::<NoObject>(k, from, &domain, &DropPointOptions::default())?;
    if !dropped.success || dropped.gap > tol {
        return Ok(None);
    }
    let mut count = 8;
    loop {
        k.check_cancelled(OP)?;
        let mut params = Vec::new();
        let mut feet = Vec::new();
        let mut guess = dropped.result_uv;
        for ii in 0..=count {
            let t = if ii == count {
                t1
            } else {
                t0 + (t1 - t0) * (ii as f64 / count as f64)
            };
            let Some(foot) = foot_on_surface(k, surface, curve.evaluate_point(k, t)?, guess)?
            else {
                return Ok(None);
            };
            guess = foot;
            params.push(t);
            feet.push(Point3::new(foot.x, foot.y, 0.0));
        }
        let mut fitted = BSplineCurve::create_interpolating_curve_with_params(
            ParamMethod::ChordLength,
            2,
            3,
            &feet,
            &[],
            None,
            false,
            Some(&params),
        )?;
        held_in_domain(&mut fitted, &domain)?;
        let uv = Curve::from(fitted);
        if proved_trim_gap(k, surface, &uv, curve, ivl, tol)?.is_some() {
            return Ok(Some(uv));
        }
        if count >= MOST {
            return Ok(None);
        }
        count *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::crv_basic::Curve as NlCurve;
    use crate::nurbs::math::CPoint;
    use crate::nurbs::srf_basic::Surface as NlSurface;
    use crate::surface::plane::Plane;

    fn spline(points: &[[f64; 3]], dim: u64) -> BSplineCurve {
        let degree = points.len() - 1;
        let pw = points
            .iter()
            .map(|p| CPoint::new(p[0], p[1], p[2], 1.0))
            .collect();
        let mut knots = vec![0.0; degree + 1];
        knots.extend(vec![1.0; degree + 1]);
        BSplineCurve::from_nurb(NlCurve::new(pw, degree, knots).unwrap(), dim, false)
    }

    /// `S(u, v) = (u, v, u^2 / 2)` over the unit square.
    fn trough() -> Surface {
        let rows = [[0.0, 0.0], [0.5, 0.0], [1.0, 0.5]]
            .iter()
            .map(|&[x, z]| vec![CPoint::new(x, 0.0, z, 1.0), CPoint::new(x, 1.0, z, 1.0)])
            .collect();
        let net = NlSurface::new(
            rows,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_nurb(net, false))
    }

    /// The edge `(t, 1/2, 3 z t (1 - t)^2)` stands `4 z / 9` over the plane's trim
    /// `(t, 1/2)` at `t = 1/3`, and less at both ends and the middle. The gap proved is
    /// that largest distance, not a sample's; past the tolerance it is not proved.
    #[test]
    fn a_trim_s_gap_is_its_largest_distance_and_is_not_proved_past_the_tolerance() {
        let k = Kernel::default();
        let tol = 1e-5;
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let plane = Surface::from(Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap());
        let trim = Curve::from(Line::from_points(
            Point3::new(0.0, 0.5, 0.0),
            Point3::new(1.0, 0.5, 0.0),
            2,
        ));
        let edge = |peak: f64| {
            let z = 2.25 * peak;
            Curve::from(spline(
                &[
                    [0.0, 0.5, 0.0],
                    [1.0 / 3.0, 0.5, z],
                    [2.0 / 3.0, 0.5, 0.0],
                    [1.0, 0.5, 0.0],
                ],
                3,
            ))
        };
        let gap = proved_trim_gap(&k, &plane, &trim, &edge(0.9 * tol), ivl, tol)
            .unwrap()
            .unwrap();
        assert!((0.9 * tol..=tol).contains(&gap), "{gap}");
        let over = edge(1.1 * tol);
        for t in [0.0, 0.5, 1.0] {
            assert!(over.evaluate_point(&k, t).unwrap().z < tol);
        }
        assert_eq!(
            proved_trim_gap(&k, &plane, &trim, &over, ivl, tol).unwrap(),
            None
        );
    }

    /// A control point past a side of the domain is put on the side; the others stay.
    #[test]
    fn a_fitted_trim_is_held_in_the_domain_by_its_control_points() {
        let mut trim = spline(
            &[
                [0.0, 1.0, 0.0],
                [-1e-6, 0.7, 0.0],
                [0.2, 0.3, 0.0],
                [0.4, -1e-9, 0.0],
            ],
            2,
        );
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        held_in_domain(&mut trim, &domain).unwrap();
        let (points, _) = trim.control_polygon().unwrap();
        assert_eq!(
            points,
            vec![
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(0.0, 0.7, 0.0),
                Point3::new(0.2, 0.3, 0.0),
                Point3::new(0.4, 0.0, 0.0),
            ]
        );
    }

    /// The curve `S(t, 0.2 + 0.6 t)` runs across the trough from one side of it to the
    /// other, off every iso line. Its trim is `(t, 0.2 + 0.6 t)`, and is proved on it; the
    /// same curve stood 1e-3 over the trough has no trim within 1e-6.
    #[test]
    fn a_curve_across_a_surface_lifts_to_a_trim_proved_on_it() {
        let k = Kernel::default();
        let tol = 1e-6;
        let surface = trough();
        let across = |lift: f64| {
            Curve::from(spline(
                &[[0.0, 0.2, lift], [0.5, 0.5, lift], [1.0, 0.8, 0.5 + lift]],
                3,
            ))
        };
        let edge = across(0.0);
        let trim = lifted_trim(&k, &surface, &edge, tol).unwrap().unwrap();
        for t in [0.0, 0.25, 0.5, 1.0] {
            let uv = trim.evaluate_point(&k, t).unwrap();
            assert!((uv.x - t).abs() < 1e-9 && (uv.y - (0.2 + 0.6 * t)).abs() < 1e-9);
        }
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let gap = proved_trim_gap(&k, &surface, &trim, &edge, ivl, tol).unwrap();
        assert!(gap.is_some_and(|g| g <= tol), "{gap:?}");
        assert!(lifted_trim(&k, &surface, &across(1e-3), tol)
            .unwrap()
            .is_none());
    }
}
