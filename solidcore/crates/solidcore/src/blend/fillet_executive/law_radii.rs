//! A radius law from its control radii: the B-spline law (`SmBSplineFilletLaw`) whose curve
//! has the radius as its x and its own parameter as its y, the control points at the
//! Greville abscissae, so its y runs along the edge as the parameter does.
//!
//! Not upstream: the fork's C API builds the same curve for its variable fillet
//! (`law_along`). No recursion.

use super::p1::{FilletLaw, FilletLawKind};
use crate::nurbs::b_spline_curve::p1::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::crv_nurb::greville_abscissa;
use crate::{Result, SolidError, Vec3};

const OP: &str = "blend::fillet_executive::FilletLaw::of_radii";

impl FilletLaw {
    /// The law of a clamped B-spline radius of `degree` over `knots` (from 0 to 1, `n +
    /// degree + 1` of them) with the control radii `radii` (`n` of them): its curve's control
    /// points are `(r_i, g_i, 0)`, `g_i` the Greville abscissa of control point i. `reversed`
    /// runs it from the edge's end: the knots mirrored about 1/2 and the radii taken from
    /// the last. The law's edge map is the whole curve, 0 to 1.
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] for knots and radii whose counts do not make a B-spline of
    /// `degree`; those of the Greville abscissae and the curve's construction.
    pub fn of_radii(degree: usize, knots: &[f64], radii: &[f64], reversed: bool) -> Result<Self> {
        let (nk, nr) = (knots.len(), radii.len());
        if degree == 0
            || nr <= degree
            || degree.checked_add(nr).and_then(|m| m.checked_add(1)) != Some(nk)
        {
            return Err(SolidError::invalid_input(
                OP,
                "a law has degree + 1 to n control radii and n + degree + 1 knots",
            ));
        }
        let kv: Vec<f64> = if reversed {
            knots.iter().rev().map(|&u| 1.0 - u).collect()
        } else {
            knots.to_vec()
        };
        let rs: Vec<f64> = if reversed {
            radii.iter().rev().copied().collect()
        } else {
            radii.to_vec()
        };
        let mut cps = Vec::new();
        for (i, &r) in rs.iter().enumerate() {
            cps.push(Vec3::new(r, greville_abscissa(&kv, degree, i)?, 0.0));
        }
        let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
        for &x in &kv {
            match (unique.last(), mults.last_mut()) {
                (Some(&u), Some(m)) if u == x => *m += 1,
                _ => {
                    unique.push(x);
                    mults.push(1);
                }
            }
        }
        let data = CanonicalData {
            dim: 3,
            degree,
            control_points: &cps,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &mults,
            knots: &unique,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        };
        let law_curve = BSplineCurve::create_canonical(&data)?;
        Ok(FilletLaw::new(FilletLawKind::BSpline {
            law_curve,
            extended_law: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent1d::Extent1d;

    /// A straight law's radius runs from its first radius to its last, reversed from the
    /// last; a bent one has its corner at the inner knot, mirrored when reversed.
    #[test]
    fn a_law_of_radii_runs_along_its_curve() {
        let law = FilletLaw::of_radii(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0], false).unwrap();
        let c = law.law_curve().unwrap();
        let (lo, hi) = (
            c.evaluate(0.0, 0, false, false).unwrap(),
            c.evaluate(1.0, 0, false, false).unwrap(),
        );
        assert_eq!((lo[0].x, hi[0].x), (1.0, 2.0));
        assert_eq!((lo[0].y, hi[0].y), (0.0, 1.0));
        let back = FilletLaw::of_radii(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0], true).unwrap();
        assert_eq!(
            back.law_curve()
                .unwrap()
                .evaluate(0.0, 0, false, false)
                .unwrap()[0]
                .x,
            2.0
        );

        let edge = Extent1d::from_raw(0.0, 10.0);
        let bent =
            FilletLaw::of_radii(1, &[0.0, 0.0, 0.25, 1.0, 1.0], &[1.0, 2.0, 1.0], false).unwrap();
        assert_eq!(bent.corner_parameters(&edge, false).unwrap(), vec![2.5]);
        let bent =
            FilletLaw::of_radii(1, &[0.0, 0.0, 0.25, 1.0, 1.0], &[1.0, 2.0, 1.0], true).unwrap();
        assert_eq!(bent.corner_parameters(&edge, false).unwrap(), vec![7.5]);
        let straight =
            FilletLaw::of_radii(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 1.5, 2.0], false).unwrap();
        assert!(straight.corner_parameters(&edge, false).unwrap().is_empty());
        assert!(FilletLaw::of_radii(1, &[0.0, 0.0, 1.0], &[1.0, 2.0], false).is_err());
    }
}
