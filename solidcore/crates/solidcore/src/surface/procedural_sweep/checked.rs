//! Checked-success evaluation of the retained screw definition.
//!
//! Candidates retain the existing scalar formulas. An interval enclosure verifies
//! every returned transcendental value, and an end-to-end point check enforces a
//! uniform model-space cap. The cap is an accuracy requirement, not an assumed
//! libm error estimate: every successful call proves it, otherwise it refuses.

use super::{Ivl, ProceduralSweep, TAU};
use crate::blend::fillet_section::interval::{sin_cos_certified, tan_certified};
use crate::error::{Result, SolidError};
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::math::gamma;

const OP: &str = "surface::procedural_sweep::checked";
// A fixed requested accuracy, verified at each call. It is never enlarged after
// a failed check and is included explicitly in the model-space certificate.
const ACCURACY_UNITS: usize = 256;
type Box3 = [Ivl; 3];

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}
fn accuracy() -> Ivl {
    let g = gamma(ACCURACY_UNITS);
    Ivl::checked(g, g)
}
fn scalar_cap(scale: f64) -> f64 {
    (accuracy() * (Ivl::from(1.0) + Ivl::from(scale.abs()))).hi
}

fn verify(candidate: f64, enclosure: Ivl, scale: f64) -> Result<f64> {
    let error = (Ivl::from(candidate) - enclosure).mag();
    if !candidate.is_finite()
        || !enclosure.lo.is_finite()
        || !enclosure.hi.is_finite()
        || !error.is_finite()
        || error > scalar_cap(scale)
    {
        return Err(bad(
            "scalar evaluation is not enclosed within its accuracy requirement",
        ));
    }
    Ok(candidate)
}

fn atan_enclosure(x: Ivl) -> Ivl {
    let point = |v: f64| {
        if v < 0.0 {
            let a = super::atan_point(-v);
            Ivl::new(-a.hi, -a.lo)
        } else {
            super::atan_point(v)
        }
    };
    Ivl::new(point(x.lo).lo, point(x.hi).hi)
}

pub(super) fn tan(x: f64) -> Result<f64> {
    verify(x.tan(), tan_certified(Ivl::from(x)), x)
}
fn atan(x: f64) -> Result<f64> {
    verify(x.atan(), atan_enclosure(Ivl::from(x)), x)
}

pub(super) fn powi(x: f64, n: usize) -> Result<f64> {
    if n > 4 {
        return Err(bad("checked screw powers are bounded at four"));
    }
    let mut enclosure = Ivl::from(1.0);
    for _ in 0..n {
        enclosure = enclosure * Ivl::from(x);
    }
    let candidate = x.powi(n as i32);
    verify(candidate, enclosure, enclosure.mag())
}

pub(super) struct Rotation {
    sine: f64,
    cosine: f64,
}
impl Rotation {
    pub(super) fn new(theta: f64) -> Result<Self> {
        let (s, c) = sin_cos_certified(Ivl::from(theta));
        Ok(Self {
            sine: verify(theta.sin(), s, theta)?,
            cosine: verify(theta.cos(), c, theta)?,
        })
    }
    pub(super) fn turn(&self, k: Vec3, v: Vec3) -> Vec3 {
        v * self.cosine + k.cross(v) * self.sine + k * (k.dot(v) * (1.0 - self.cosine))
    }
}

pub(super) struct Turn {
    pub theta: f64,
    pub rate: f64,
    pub tau: f64,
}

fn boxed(p: Vec3) -> Box3 {
    [Ivl::from(p.x), Ivl::from(p.y), Ivl::from(p.z)]
}
fn add(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x + u, y + v, z + w]
}
fn sub(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x - u, y - v, z - w]
}
fn scale(a: Box3, b: Ivl) -> Box3 {
    a.map(|x| x * b)
}
fn dot(a: Box3, b: Box3) -> Ivl {
    let [x, y, z] = a;
    let [u, v, w] = b;
    x * u + y * v + z * w
}
fn cross(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [y * w - z * v, z * u - x * w, x * v - y * u]
}
fn magnitude(a: Box3) -> Ivl {
    let [x, y, z] = a.map(|v| Ivl::from(v.mag()));
    (x * x + y * y + z * z).sqrt()
}
fn finite_box(a: Box3) -> bool {
    a.iter().all(|v| v.lo.is_finite() && v.hi.is_finite())
}

impl ProceduralSweep {
    fn checked_span(&self, t: f64, ending: bool) -> Result<(f64, f64, f64)> {
        if !t.is_finite() {
            return Err(bad("screw parameter must be finite"));
        }
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(bad("missing screw stations"));
        };
        if !first.is_finite() || !last.is_finite() || first >= last {
            return Err(bad("invalid screw station bounds"));
        }
        let t = crate::math::clamp_between(t, first, last);
        let index = self
            .stations
            .partition_point(|&s| if ending { s < t } else { s <= t })
            .saturating_sub(1)
            .min(self.stations.len().saturating_sub(2));
        let (Some(&a), Some(&b)) = (self.stations.get(index), self.stations.get(index + 1)) else {
            return Err(bad("missing screw span"));
        };
        Ok((t, a, b))
    }

    fn turn_enclosure(&self, t: f64, ending: bool) -> Result<(Ivl, Ivl)> {
        let (t, a, b) = self.checked_span(t, ending)?;
        let width = Ivl::from(b) - Ivl::from(a);
        let tau = tan_certified(width / Ivl::from(4.0));
        let x = tau * (Ivl::from(2.0) * (Ivl::from(t) - Ivl::from(a)) / width - Ivl::from(1.0));
        let theta =
            (Ivl::from(a) + Ivl::from(b)) / Ivl::from(2.0) + Ivl::from(2.0) * atan_enclosure(x);
        let rate = Ivl::from(4.0) * tau / (width * (Ivl::from(1.0) + x * x));
        Ok((theta, rate))
    }

    pub(super) fn checked_turn_at_side(&self, t: f64, ending: bool) -> Result<Turn> {
        let (t, a, b) = self.checked_span(t, ending)?;
        let width = b - a;
        let tau = tan(0.25 * width)?;
        let x = tau * (2.0 * (t - a) / width - 1.0);
        let theta = 0.5 * (a + b) + 2.0 * atan(x)?;
        let rate = 2.0 * tau / (1.0 + x * x) * (2.0 / width);
        let (theta_box, rate_box) = self.turn_enclosure(t, ending)?;
        Ok(Turn {
            theta: verify(theta, theta_box, a.abs().max(b.abs()))?,
            rate: verify(rate, rate_box, rate_box.mag())?,
            tau,
        })
    }

    /// Monotone mathematical turn range, including exact station endpoints.
    pub(crate) fn mathematical_turn_range(&self, range: Ivl) -> Result<Ivl> {
        if !range.lo.is_finite() || !range.hi.is_finite() || range.lo > range.hi {
            return Err(bad("invalid mathematical turn range"));
        }
        let endpoint = |value: f64, ending: bool| -> Result<Ivl> {
            let (t, a, b) = self.checked_span(value, ending)?;
            if t == a || t == b {
                return Ok(Ivl::from(t));
            }
            Ok(self.turn_enclosure(t, ending)?.0)
        };
        let lo = endpoint(range.lo, false)?;
        let hi = endpoint(range.hi, true)?;
        let result = Ivl::new(lo.lo, hi.hi);
        if !result.lo.is_finite() || !result.hi.is_finite() {
            return Err(bad("turn range could not be enclosed"));
        }
        Ok(result)
    }

    pub(super) fn checked_source_class(&self) -> Result<bool> {
        // canonical() dehomogenizes rational controls with rounded division.
        // Until those quotient intervals are retained, they cannot define the
        // independent exact source used by this runtime certificate.
        let source = self
            .offset
            .as_ref()
            .map_or(&self.profile, |offset| &offset.source);
        Ok(!source.is_rational()?)
    }

    /// Uniform cap for every successful retained affine screw point evaluation
    /// at the same face parameters. Includes source evaluation, signed/swapped
    /// map arithmetic, raw-axis Rodrigues motion and lift. This is enforced by
    /// an end-to-end interval check; no legacy floor or libm accuracy is assumed.
    /// Exact-real net/profile approximation and unit-axis reference differences
    /// are separate certificate terms and are not included here.
    pub(crate) fn runtime_evaluation_allowance(&self) -> Result<f64> {
        if let Some(&cap) = self.runtime_allowance.get() {
            return Ok(cap);
        }
        let cap = self.compute_runtime_evaluation_allowance()?;
        // Concurrent immutable readers may prove the same cap independently;
        // either identical successful proof is valid. No failed proof is cached.
        let _ = self.runtime_allowance.set(cap);
        Ok(cap)
    }

    /// The runtime evaluation allowance when this definition's class has one:
    /// `Ok(None)` for the classes the checked evaluator does not cover (rational
    /// source controls; no invariant affine screw section, such as a varying
    /// radial law on a tapered thread or a path sweep), and the allowance's own
    /// error for a covered class whose proof fails (a nonfinite cap, missing
    /// stations, an enclosure that cannot be formed).
    ///
    /// # Errors
    /// Those of inspecting the class and of [`Self::runtime_evaluation_allowance`].
    pub(crate) fn runtime_evaluation_allowance_if_proved(&self) -> Result<Option<f64>> {
        if !self.checked_source_class()? || self.screw_offset_section()?.is_none() {
            return Ok(None);
        }
        self.runtime_evaluation_allowance().map(Some)
    }

    /// Definition-field editors must call this before changing S or its map.
    /// Stored records contain no runtime cache and always prove their own cap.
    pub(super) fn invalidate_runtime_allowance(&mut self) {
        let _ = self.runtime_allowance.take();
    }

    fn compute_runtime_evaluation_allowance(&self) -> Result<f64> {
        if !self.checked_source_class()? {
            return Err(bad(
                "checked point allowance needs exact nonrational source controls",
            ));
        }
        let Some((section, [lo, hi])) = self.screw_offset_section()? else {
            return Err(bad(
                "checked point allowance needs an invariant affine source",
            ));
        };
        let motion = self.screw().ok_or_else(|| bad("missing screw motion"))?;
        let domain = self.profile.natural_interval();
        let width = Ivl::from(hi) - Ivl::from(lo);
        let source = (Ivl::new(domain.min(), domain.max()) - Ivl::from(lo)) / width;
        let point = section.point_enclosure(source)?;
        let [profile_speed, turn_speed] = section.speed_upper_bounds(source)?;
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return Err(bad("missing screw stations"));
        };
        let map_size = |range: Ivl, a: f64, b: f64| -> Result<Ivl> {
            if !a.is_finite() || !b.is_finite() || b == 0.0 {
                return Err(bad("invalid screw parameter map"));
            }
            let native = (range - Ivl::from(a)) / Ivl::from(b);
            Ok(Ivl::from(a.abs()) + Ivl::from(b.abs()) * Ivl::from(native.mag()))
        };
        let [a0, a1] = self.map.a;
        let [b0, b1] = self.map.b;
        let mw = map_size(Ivl::new(domain.min(), domain.max()), a0, b0)?;
        let mt = map_size(Ivl::new(first, last), a1, b1)?;
        let mut rate = 0.0_f64;
        for pair in self.stations.windows(2) {
            let [a, b] = *pair else { continue };
            let w = Ivl::from(b) - Ivl::from(a);
            rate = rate.max((Ivl::from(4.0) * tan_certified(w / Ivl::from(4.0)) / w).hi);
        }
        let theta = Ivl::from(first.abs().max(last.abs()));
        let k = magnitude(boxed(motion.axis()));
        let arm = magnitude(sub(point, boxed(motion.origin())));
        let angular = arm * (Ivl::from(1.0) + k + k * k)
            + Ivl::from(motion.pitch().abs()) / Ivl::checked(TAU, TAU) * k;
        let size = Ivl::from(1.0)
            + magnitude(point)
            + magnitude(boxed(motion.origin()))
            + angular * (Ivl::from(1.0) + theta)
            + Ivl::from(profile_speed) / width * (Ivl::from(1.0) + mw)
            + Ivl::from(turn_speed) * Ivl::from(rate) * (Ivl::from(1.0) + mt);
        let cap = (accuracy() * size).hi;
        if !cap.is_finite() || cap <= 0.0 {
            return Err(bad("checked screw allowance is not finite"));
        }
        Ok(cap)
    }

    /// Encloses every successful definition evaluation in a whole UV rectangle.
    /// Unlike an active-control hull, the source and turn intervals contract with
    /// the requested patch. The uniform runtime cap encloses actual S evaluations.
    pub(crate) fn definition_patch_enclosure(
        &self,
        uv: crate::geom::extent2d::Extent2d,
    ) -> Result<Option<crate::geom::extent3d::Extent3d>> {
        let Some(point) = self.checked_patch_enclosure([
            Ivl::new(uv.min().x, uv.max().x),
            Ivl::new(uv.min().y, uv.max().y),
        ])?
        else {
            return Ok(None);
        };
        let cap = self.runtime_evaluation_allowance()?;
        let [x, y, z] = point.map(|coordinate| coordinate + Ivl::new(-cap, cap));
        Ok(Some(crate::geom::extent3d::Extent3d::from_bounds(
            x.lo, y.lo, z.lo, x.hi, y.hi, z.hi,
        )))
    }

    fn checked_point_enclosure(&self, uv: Point2) -> Result<Option<Box3>> {
        self.checked_patch_enclosure([Ivl::from(uv.x), Ivl::from(uv.y)])
    }

    fn checked_patch_enclosure(&self, [u, v]: [Ivl; 2]) -> Result<Option<Box3>> {
        if !self.checked_source_class()? {
            return Ok(None);
        }
        let Some((section, [lo, hi])) = self.screw_offset_section()? else {
            return Ok(None);
        };
        if ![u.lo, u.hi, v.lo, v.hi].iter().all(|x| x.is_finite()) || u.lo > u.hi || v.lo > v.hi {
            return Err(bad("face parameters must be finite"));
        }
        let motion = self.screw().ok_or_else(|| bad("missing screw motion"))?;
        let (p, q) = if self.map.swapped { (v, u) } else { (u, v) };
        let [a0, a1] = self.map.a;
        let [b0, b1] = self.map.b;
        let w = Ivl::from(a0) + Ivl::from(b0) * p;
        let t = Ivl::from(a1) + Ivl::from(b1) * q;
        let domain = self.profile.natural_interval();
        if !domain.min().is_finite() || !domain.max().is_finite() || domain.min() >= domain.max() {
            return Err(bad("invalid screw profile bounds"));
        }
        let w = Ivl::new(
            crate::math::clamp_between(w.lo, domain.min(), domain.max()),
            crate::math::clamp_between(w.hi, domain.min(), domain.max()),
        );
        let point =
            section.point_enclosure((w - Ivl::from(lo)) / (Ivl::from(hi) - Ivl::from(lo)))?;
        // theta(t) is monotone and continuous across all retained arc joints.
        // Endpoint enclosures therefore cover even a rounded map crossing a knot.
        let theta = self.mathematical_turn_range(t)?;
        // A whole patch can cross many quadrants. A width at most one radian
        // leaves the nearest-quadrant remainder inside [-1.5,1.5]; wider
        // patches use the mathematical global range, not a failed local proof.
        let (s, c) = if (Ivl::from(theta.hi) - Ivl::from(theta.lo)).hi > 1.0 {
            (Ivl::new(-1.0, 1.0), Ivl::new(-1.0, 1.0))
        } else {
            sin_cos_certified(theta)
        };
        let k = boxed(motion.axis());
        let origin = boxed(motion.origin());
        let r = sub(point, origin);
        let rotated = add(
            add(scale(r, c), scale(cross(k, r), s)),
            scale(k, dot(k, r) * (Ivl::from(1.0) - c)),
        );
        let result = add(
            add(origin, rotated),
            scale(
                k,
                Ivl::from(motion.pitch()) * theta / Ivl::checked(TAU, TAU),
            ),
        );
        if !finite_box(result) {
            return Err(bad("screw point cannot be enclosed"));
        }
        Ok(Some(result))
    }

    pub(super) fn verify_runtime_point(&self, uv: Point2, candidate: Point3) -> Result<()> {
        let Some(enclosure) = self.checked_point_enclosure(uv)? else {
            return Ok(());
        };
        let error = magnitude(sub(boxed(candidate), enclosure)).hi;
        if !error.is_finite() || error > self.runtime_evaluation_allowance()? {
            return Err(bad("screw point exceeds its checked runtime allowance"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidates_are_verified_not_replaced() {
        let x = 0.371_f64;
        assert_eq!(tan(x).unwrap(), x.tan());
        assert_eq!(atan(x).unwrap(), x.atan());
        let r = Rotation::new(x).unwrap();
        assert_eq!(r.sine, x.sin());
        assert_eq!(r.cosine, x.cos());
        let (s, _) = sin_cos_certified(Ivl::from(x));
        assert!(verify(0.5, s, x).is_err());
        assert!(verify(f64::NAN, s, x).is_err());
        assert!(Rotation::new(3e15).is_err());
    }

    /// A class without a checked evaluator (rational controls, a varying radial
    /// law) has no allowance, and says so with `None`; a covered class whose
    /// proof fails is an error, never mistaken for a class refusal.
    #[test]
    fn allowance_if_proved_separates_class_refusals_from_failed_proofs() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::CPoint;
        use crate::surface::procedural_sweep::{RadialLaw, ScrewMotion};

        let (start, end) = (Vec3::new(8.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 1.0));
        let stations = [0.0, 0.3, 0.6];
        let line = BSplineCurve::create_line_segment(3, start, end).unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let plain = ProceduralSweep::create(&line, motion.clone(), &stations, 0.15).unwrap();
        assert_eq!(
            plain.runtime_evaluation_allowance_if_proved().unwrap(),
            Some(plain.compute_runtime_evaluation_allowance().unwrap())
        );

        // the same line, both controls homogeneous with weight 2
        let pw = vec![
            CPoint::new(16.0, 0.0, 0.0, 2.0),
            CPoint::new(20.0, 0.0, 2.0, 2.0),
        ];
        let rational = BSplineCurve::from_nurb(
            NlCurve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap(),
            3,
            false,
        );
        assert!(rational.is_rational().unwrap());
        let weighted = ProceduralSweep::create(&rational, motion, &stations, 0.15).unwrap();
        assert_eq!(
            weighted.runtime_evaluation_allowance_if_proved().unwrap(),
            None
        );

        let law = RadialLaw::new(Vec3::X, vec![0.0, 0.6], vec![0.0, 0.1]).unwrap();
        let tapered = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, Some(law)).unwrap();
        let tapered = ProceduralSweep::create(&line, tapered, &stations, 0.15).unwrap();
        assert_eq!(
            tapered.runtime_evaluation_allowance_if_proved().unwrap(),
            None
        );

        let mut unstationed = plain.clone();
        unstationed.invalidate_runtime_allowance();
        unstationed.stations.clear();
        assert!(unstationed
            .runtime_evaluation_allowance_if_proved()
            .is_err());
    }

    #[test]
    fn point_checks_reuse_the_uniform_station_proof() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::ScrewMotion;

        let profile = BSplineCurve::create_line_segment(
            3,
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 1.0),
        )
        .unwrap();
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::Z, 2.5, None).unwrap();
        let stations = (0..=560).map(|i| f64::from(i) * 0.1).collect::<Vec<_>>();
        let definition = ProceduralSweep::create(&profile, motion, &stations, 0.05).unwrap();
        let proved = definition.compute_runtime_evaluation_allowance().unwrap();
        let first = Point2::new(0.37, 0.0);
        let point = definition.evaluate_definition(first, 0, 0).unwrap()[0][0];
        definition.verify_runtime_point(first, point).unwrap();
        // The first point check proves the uniform station cap once and keeps it.
        assert_eq!(definition.runtime_allowance.get().copied(), Some(proved));
        for i in 0..16 {
            let uv = Point2::new(0.37, f64::from(i) * 0.1);
            let point = definition.evaluate_definition(uv, 0, 0).unwrap()[0][0];
            definition.verify_runtime_point(uv, point).unwrap();
            assert!(definition
                .verify_runtime_point(uv, point + Vec3::X)
                .is_err());
        }
        assert_eq!(definition.runtime_allowance.get().copied(), Some(proved));

        let original = definition.runtime_evaluation_allowance().unwrap();
        let mut changed = definition.clone();
        assert_eq!(changed.runtime_allowance.get().copied(), Some(original));
        assert_eq!(changed.runtime_evaluation_allowance().unwrap(), original);
        changed.reparametrize_map(true, 1e8, -2.0);
        assert!(changed.runtime_allowance.get().is_none());
        let mapped = changed.runtime_evaluation_allowance().unwrap();
        assert!(mapped > original);
        assert_eq!(
            mapped,
            changed.compute_runtime_evaluation_allowance().unwrap()
        );
        changed.swap_map();
        assert!(changed.runtime_allowance.get().is_none());
        assert_eq!(changed.runtime_evaluation_allowance().unwrap(), mapped);

        let mut moved = definition.clone();
        moved
            .transform(
                &crate::geom::axis2_placement::Axis2Placement::IDENTITY,
                Some(Vec3::new(2.0, 2.0, 2.0)),
            )
            .unwrap();
        assert!(moved.runtime_allowance.get().is_none());
        let scaled = moved.runtime_evaluation_allowance().unwrap();
        assert!(scaled > original);
        assert_eq!(
            scaled,
            moved.compute_runtime_evaluation_allowance().unwrap()
        );
        assert_eq!(definition.runtime_evaluation_allowance().unwrap(), original);
    }
}
