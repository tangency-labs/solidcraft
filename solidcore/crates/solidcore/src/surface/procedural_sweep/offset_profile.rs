//! The source and certified polyline net of a screw wall's exact normal-offset section.
//! The source parameter map survives support extensions; fitting changes only the net.
//!
//! Deviations: no upstream counterpart. No recursion; subdivision is bounded by
//! [`super::screw_offset::ScrewOffsetSection::approximate_over`].

use crate::blend::fillet_section::interval::Ivl;
use crate::error::{Result, SolidError};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

use super::screw_offset::ScrewOffsetSection;
use super::ScrewMotion;

const OP: &str = "surface::procedural_sweep::offset_profile";

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The exact source behind a screw offset's fitted profile.
#[derive(Clone, Debug)]
pub(crate) struct OffsetProfile {
    /// The original affine generator, retaining its original parameter interval.
    pub(crate) source: BSplineCurve,
    /// Signed offset along the original generator's screw-surface normal.
    pub(crate) distance: f64,
    /// Certified pointwise error of the profile approximant against the source offset.
    pub(crate) bound: f64,
}

/// Recognises the exact affine class, rather than inferring a line from samples.
pub(crate) fn affine_source(profile: &BSplineCurve) -> Result<Option<([Point3; 2], [f64; 2])>> {
    let c = profile.canonical()?;
    let ([start, end], [lo, hi]) = (c.control_points.as_slice(), c.knots.as_slice()) else {
        return Ok(None);
    };
    if c.dim != 3
        || c.degree != 1
        || c.knot_multiplicities.as_slice() != [2, 2]
        || !lo.is_finite()
        || !hi.is_finite()
        || lo >= hi
    {
        return Ok(None);
    }
    let equal_weights = c.weights.is_empty()
        || matches!(c.weights.as_slice(), [a, b] if a == b && a.is_finite() && *a > 0.0);
    if !equal_weights {
        return Ok(None);
    }
    Ok(Some(([*start, *end], [*lo, *hi])))
}

impl OffsetProfile {
    /// Prepares the exact section and its unchanged source parameter interval.
    pub(crate) fn section(&self, motion: &ScrewMotion) -> Result<(ScrewOffsetSection, [f64; 2])> {
        let Some(([start, end], domain)) = affine_source(&self.source)? else {
            return Err(bad("a screw normal offset needs an affine profile source"));
        };
        Ok((
            ScrewOffsetSection::new(start, end, motion, self.distance)?,
            domain,
        ))
    }

    /// Evaluates the exact offset source at its original profile parameter, including
    /// continuation beyond that source's original ends.
    pub(crate) fn evaluate(&self, motion: &ScrewMotion, w: f64) -> Result<(Point3, Vec3)> {
        let (section, range) = self.section(motion)?;
        Self::evaluate_on(&section, range, w)
    }

    /// [`Self::evaluate`] on the section and source interval [`Self::section`] gives.
    pub(crate) fn evaluate_on(
        section: &ScrewOffsetSection,
        [lo, hi]: [f64; 2],
        w: f64,
    ) -> Result<(Point3, Vec3)> {
        let (point, tangent) = section.evaluate((w - lo) / (hi - lo))?;
        Ok((point, tangent / (hi - lo)))
    }

    pub(crate) fn derivatives(
        &self,
        motion: &ScrewMotion,
        w: f64,
        order: usize,
    ) -> Result<Vec<Vec3>> {
        let (section, range) = self.section(motion)?;
        Self::derivatives_on(&section, range, w, order)
    }

    /// [`Self::derivatives`] on the section and source interval [`Self::section`] gives.
    pub(crate) fn derivatives_on(
        section: &ScrewOffsetSection,
        [lo, hi]: [f64; 2],
        w: f64,
        order: usize,
    ) -> Result<Vec<Vec3>> {
        let width = hi - lo;
        let mut derivatives = section.derivatives((w - lo) / width, order)?;
        let mut factor = 1.0;
        for value in &mut derivatives {
            *value *= factor;
            factor /= width;
        }
        Ok(derivatives)
    }

    /// Constructs the fitted profile over `domain` without changing the exact source.
    /// Parameter-map rounding is included by re-certifying the actual stored knots.
    pub(crate) fn build(
        source: &BSplineCurve,
        motion: &ScrewMotion,
        distance: f64,
        domain: [f64; 2],
        tolerance: f64,
    ) -> Result<(BSplineCurve, Self)> {
        let mut definition = Self {
            source: source.copy(),
            distance,
            bound: 0.0,
        };
        let (section, [source_lo, source_hi]) = definition.section(motion)?;
        let [lo, hi] = domain;
        if !lo.is_finite() || !hi.is_finite() || lo >= hi {
            return Err(bad(
                "an offset profile requires a finite ascending support interval",
            ));
        }
        let scale = source_hi - source_lo;
        // Half the budget is reserved for the stored-knot and endpoint arithmetic.
        let approximation = section.approximate_over(
            (lo - source_lo) / scale,
            (hi - source_lo) / scale,
            tolerance * 0.5,
        )?;
        let mut knots: Vec<f64> = approximation
            .parameters
            .iter()
            .map(|&x| source_lo + scale * x)
            .collect();
        if let Some(first) = knots.first_mut() {
            *first = lo;
        }
        if let Some(last) = knots.last_mut() {
            *last = hi;
        }
        let last = knots.len().saturating_sub(1);
        let multiplicities: Vec<usize> = (0..knots.len())
            .map(|i| if i == 0 || i == last { 2 } else { 1 })
            .collect();
        let profile = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 1,
            control_points: &approximation.points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &multiplicities,
            knots: &knots,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })?;
        definition.bound = definition.certify(&profile, motion)?;
        if !definition.bound.is_finite() || definition.bound > tolerance {
            return Err(bad(
                "the stored offset profile does not meet its certified tolerance",
            ));
        }
        Ok((profile, definition))
    }

    /// Re-certifies an actual piecewise-linear profile, ignoring any claimed bound.
    pub(crate) fn certify(&self, profile: &BSplineCurve, motion: &ScrewMotion) -> Result<f64> {
        let (section, [lo, hi]) = self.section(motion)?;
        let c = profile.canonical()?;
        if c.dim != 3
            || c.degree != 1
            || c.control_points.len() < 2
            || c.knots.len() != c.control_points.len()
            || c.knot_multiplicities.len() != c.knots.len()
            || (!c.weights.is_empty() && c.weights.iter().any(|&w| w != 1.0))
        {
            return Err(bad(
                "an offset profile net must be a polynomial piecewise line",
            ));
        }
        let last = c.knots.len().saturating_sub(1);
        if c.knot_multiplicities
            .iter()
            .enumerate()
            .any(|(i, &m)| m != if i == 0 || i == last { 2 } else { 1 })
        {
            return Err(bad(
                "an offset profile has incompatible knot multiplicities",
            ));
        }
        let source_width = Ivl::from(hi) - Ivl::from(lo);
        let mut bound = 0.0_f64;
        for (knots, points) in c.knots.windows(2).zip(c.control_points.windows(2)) {
            let ([a, b], [pa, pb]) = (knots, points) else {
                continue;
            };
            if !a.is_finite() || !b.is_finite() || a >= b {
                return Err(bad("the offset profile knots are not finite and ascending"));
            }
            let from = (Ivl::from(*a) - Ivl::from(lo)) / source_width;
            let to = (Ivl::from(*b) - Ivl::from(lo)) / source_width;
            section.certify_regular(from.lo, to.hi)?;
            bound = bound.max(section.chord_bound_enclosed(from, to, *pa, *pb));
        }
        if !bound.is_finite() {
            return Err(bad(
                "the profile's exact section error is not certified finite",
            ));
        }
        Ok(bound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> BSplineCurve {
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 1,
            control_points: &[Vec3::new(8.6, 0.0, -0.3), Vec3::new(10.0, 0.0, 0.5)],
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[2, 2],
            knots: &[2.0, 4.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap()
    }

    fn motion() -> ScrewMotion {
        ScrewMotion::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 2.5, None).unwrap()
    }

    #[test]
    fn extended_net_preserves_the_original_source_parameter_map() {
        let (net, definition) =
            OffsetProfile::build(&source(), &motion(), 0.1, [1.0, 5.0], 1e-5).unwrap();
        assert!(definition.bound <= 1e-5);
        assert_eq!(
            affine_source(&definition.source).unwrap().unwrap().1,
            [2.0, 4.0]
        );
        for w in [1.0, 1.371, 2.719, 3.237, 4.813, 5.0] {
            let (exact, _) = definition.evaluate(&motion(), w).unwrap();
            let fitted = net.evaluate(w, 0, false, false).unwrap()[0];
            assert!((exact - fitted).length() <= definition.bound);
        }
    }

    #[test]
    fn re_certification_measures_the_actual_profile() {
        let (mut net, mut definition) =
            OffsetProfile::build(&source(), &motion(), -0.1, [2.0, 4.0], 1e-5).unwrap();
        let mut canonical = net.canonical().unwrap();
        canonical.control_points[0].y += 0.01;
        net.set_canonical(&CanonicalData {
            dim: canonical.dim,
            degree: canonical.degree,
            control_points: &canonical.control_points,
            form: canonical.form,
            knot_multiplicities: &canonical.knot_multiplicities,
            knots: &canonical.knots,
            knot_type: canonical.knot_type,
            weights: None,
            trim: None,
        })
        .unwrap();
        definition.bound = 0.0;
        assert!(definition.certify(&net, &motion()).unwrap() >= 0.0099);
    }
}
