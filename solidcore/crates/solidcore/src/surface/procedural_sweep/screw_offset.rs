//! Normal offsets of screw-swept straight generators, retained as exact definitions.
//!
//! Source: the normal-offset definition in Patrikalakis, Maekawa and Cho,
//! *Shape Interrogation for Computer Aided Design and Manufacturing*, §11.3.1,
//! <https://web.mit.edu/hyperbook/Patrikalakis-Maekawa-Cho/node224.html>.
//! The screw-equivariance and error estimates below are derived here, not sampled.
//! Root enclosure uses the Krawczyk inclusion/exclusion theorem, as surveyed by
//! S. M. Rump, *Verification methods: Rigorous results using floating-point
//! arithmetic*, Acta Numerica 19 (2010),
//! <https://www.tuhh.de/ti3/rump/intlab/ActaNumerica2010.pdf>.
//!
//! For `P(x)=a+x b`, `h=pitch/(2 pi)` and unit axis `k`, the surface's oriented
//! normal at turn zero is `n=V/|V|`, where
//! `V=b × (k × (P-o)+h k)=A+x B`, `B=b × (k × b)`.
//! Consequently `C=P+d n` generates the exact parallel surface under the same
//! screw motion. Its meridian section is **not** the planar offset of `P`:
//! with `phi=atan2(C_y,C_x)` it is `(hypot(C_x,C_y), C_z-h phi)`.
//! A section intersection therefore lifts to a helix, with the phase `phi`
//! retained when mapping back to the source wall's parameters.
//!
//! For certification write `B=alpha n+T`, `T` perpendicular to `n`. Then
//! `n''=(-|T|² n-2 alpha T)/|V|²`. With the constant `Q=A×B=V×B`,
//! `|n''|²=|Q|² (|Q|²+4(V·B)²)/|V|⁸`. The identity
//! `|Q|²+(V·B)²=|V|² |B|²` gives `|C''| <= 2 |d| |Q| |B|/|V|³`.
//! This retains the small normal variation when `V` and `B` nearly align.
//! Linear interpolation on an interval of width `w` errs by at most
//! `w² sup|C''|/8` (the Green-function bound for the linear interpolant).
//! Rounded interval arithmetic encloses the coefficients, the denominator and
//! the floating endpoint errors. No probe values certify an interval.
//!
//! Deviations: no upstream counterpart. This primitive covers straight generators
//! and constant radial laws; a varying law is not invariant under a screw motion.
//! Iterative subdivision has at most 16,384 leaves and depth 48; no recursion.

use std::f64::consts::TAU;

use crate::blend::fillet_section::interval::Ivl;
use crate::error::{Result, SolidError};
use crate::math::{Point2, Point3, Vec3};
use crate::surface::procedural_sweep::ScrewMotion;

const OP: &str = "surface::procedural_sweep::screw_offset";
const MAX_LEAVES: usize = 16_384;
const MAX_DEPTH: u32 = 48;
type Box3 = [Ivl; 3];

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn boxed(v: Vec3) -> Box3 {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

fn add(a: Box3, b: Box3) -> Box3 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + bx, ay + by, az + bz]
}

fn sub(a: Box3, b: Box3) -> Box3 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax - bx, ay - by, az - bz]
}

fn scale(a: Box3, b: Ivl) -> Box3 {
    a.map(|v| v * b)
}

fn cross(a: Box3, b: Box3) -> Box3 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn square(a: Ivl) -> Ivl {
    let lower = Ivl::from(a.mig()) * Ivl::from(a.mig());
    let upper = Ivl::from(a.mag()) * Ivl::from(a.mag());
    Ivl::new(lower.lo.max(0.0), upper.hi)
}

fn norm_squared(a: Box3) -> Ivl {
    let [x, y, z] = a.map(square);
    let sum = x + y + z;
    Ivl::new(sum.lo.max(0.0), sum.hi)
}

fn dot(a: Box3, b: Box3) -> Ivl {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    ax * bx + ay * by + az * bz
}

/// Monotonic endpoint enclosure using the parent's certified alternating series.
/// Two half-angle reductions put every argument in `[0, 2]` below `1/2`.
fn atan_positive(value: f64) -> Ivl {
    if !value.is_finite() || value < 0.0 {
        return Ivl::whole();
    }
    let invert = value > 2.0;
    let mut x = if invert {
        Ivl::from(1.0) / Ivl::from(value)
    } else {
        Ivl::from(value)
    };
    for _ in 0..2 {
        x = x / (Ivl::from(1.0) + (Ivl::from(1.0) + square(x)).sqrt());
    }
    let result = Ivl::new(
        super::atan_point(x.lo.max(0.0)).lo,
        super::atan_point(x.hi).hi,
    ) * Ivl::from(4.0);
    if invert {
        // Widen TAU as well: its rounded representation encloses the mathematical pi.
        Ivl::checked(TAU, TAU) / Ivl::from(4.0) - result
    } else {
        result
    }
}

pub(crate) fn atan_interval(value: Ivl) -> Ivl {
    let point = |x: f64| {
        if x < 0.0 {
            let positive = atan_positive(-x);
            Ivl::new(-positive.hi, -positive.lo)
        } else {
            atan_positive(x)
        }
    };
    Ivl::new(point(value.lo).lo, point(value.hi).hi)
}

/// The exact normal-offset generator of a screw-swept line, parameterised on `[0,1]`.
/// Positive distance follows `dS/dx × dS/dtheta`; face orientation is the caller's job.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrewOffsetSection {
    start: Point3,
    tangent: Vec3,
    origin: Point3,
    axis: Vec3,
    rise: f64,
    rise_box: Ivl,
    distance: f64,
    normal_start: Vec3,
    normal_rate: Vec3,
    start_box: Box3,
    tangent_box: Box3,
    normal_start_box: Box3,
    normal_rate_box: Box3,
}

/// A piecewise-linear section with a pointwise certificate against its exact generator.
#[derive(Clone, Debug)]
pub struct CertifiedSection {
    /// Strictly ascending parameters, including the requested interval's ends.
    pub parameters: Vec<f64>,
    /// The interpolant's control points, in the same order as `parameters`.
    pub points: Vec<Point3>,
    /// Upper bound of the distance from the interpolant to the mathematical definition
    /// at the same parameter, including rounding of these control points.
    pub bound: f64,
}

/// An exact offset generator's screw-invariant coordinates in an axial half-plane.
#[derive(Clone, Copy, Debug)]
pub struct MeridianPoint {
    /// Radius and axial height after undoing the point's angular phase.
    pub point: Point2,
    /// Derivative of `point` with respect to the line parameter.
    pub tangent: Point2,
    /// Angular phase of the offset generator. Its screw orbit starts at this angle.
    pub phase: f64,
    /// Derivative of `phase` with respect to the line parameter.
    pub phase_rate: f64,
}

/// Interval enclosure of an axial section and its derivative for root isolation.
#[derive(Clone, Copy, Debug)]
pub struct MeridianBounds {
    /// All radii on the parameter interval.
    pub radius: Ivl,
    /// All phase-corrected axial heights on the interval.
    pub height: Ivl,
    /// Derivatives of radius with respect to the original line parameter.
    pub radius_rate: Ivl,
    /// Derivatives of phase-corrected height.
    pub height_rate: Ivl,
    /// The continuous angular phase on this open half-plane.
    pub phase: Ivl,
}

/// The same retained affine source over a whole interval of signed offsets.
/// Derivatives in `distance` hold the original source parameter fixed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeridianFamilyBounds {
    pub section: MeridianBounds,
    pub radius_distance: Ivl,
    pub height_distance: Ivl,
    /// Oriented offset area against the original unnormalized normal. A positive
    /// lower bound proves regularity and preservation of the original side.
    pub oriented_area: Ivl,
}

/// A proved intersection of two meridian sections, with enclosing source parameters.
#[derive(Clone, Copy, Debug)]
pub struct SectionIntersection {
    /// Parameter enclosure on the first source line.
    pub first: Ivl,
    /// Parameter enclosure on the second source line.
    pub second: Ivl,
    /// Enclosure of the intersection radius.
    pub radius: Ivl,
    /// Enclosure of the intersection height in the common meridian.
    pub height: Ivl,
}

/// The result of an interval intersection test; undecided never means no intersection.
#[derive(Clone, Copy, Debug)]
pub enum IntersectionIsolation {
    /// The supplied parameter rectangle contains no section intersection.
    NoRoot,
    /// Exactly one intersection lies in the supplied rectangle, enclosed here to the
    /// requested geometric tolerance.
    Unique(SectionIntersection),
    /// Existence/uniqueness or the requested enclosure could not be proved. A caller
    /// may split this remaining rectangle under its own bounded isolation policy.
    Undecided {
        /// Remaining parameter interval on the first section.
        first: Ivl,
        /// Remaining parameter interval on the second section.
        second: Ivl,
    },
}

fn overlap(a: Ivl, b: Ivl) -> Option<Ivl> {
    let lo = a.lo.max(b.lo);
    let hi = a.hi.min(b.hi);
    (lo <= hi).then_some(Ivl::new(lo, hi))
}

fn negative(a: Ivl) -> Ivl {
    Ivl::new(-a.hi, -a.lo)
}

impl ScrewOffsetSection {
    /// Offsets the line from `start` to `end` under `motion` by signed `distance`.
    /// A constant radial law is absorbed into the generator before taking its normal.
    ///
    /// # Errors
    /// Refuses nonfinite data, a zero-length generator, a varying radial law or
    /// arithmetic outside the finite range. Regularity is certified per interval by
    /// [`Self::approximate`]; individual evaluations refuse a singular normal.
    pub fn new(start: Point3, end: Point3, motion: &ScrewMotion, distance: f64) -> Result<Self> {
        if !finite(start) || !finite(end) || !distance.is_finite() {
            return Err(bad("a screw offset requires finite points and distance"));
        }
        let shift = if let Some(law) = motion.law() {
            let first = law
                .offsets()
                .first()
                .copied()
                .ok_or_else(|| bad("empty radial law"))?;
            if law.offsets().iter().any(|&value| value != first) {
                return Err(bad("a varying radial law has no invariant screw section"));
            }
            scale(boxed(law.direction()), Ivl::from(first))
        } else {
            boxed(Vec3::ZERO)
        };
        let start_box = add(boxed(start), shift);
        let tangent_box = sub(boxed(end), boxed(start));
        let [sx, sy, sz] = start_box;
        let start = Vec3::new(sx.mid(), sy.mid(), sz.mid());
        let [tx, ty, tz] = tangent_box;
        let tangent = Vec3::new(tx.mid(), ty.mid(), tz.mid());
        let axis = motion.axis();
        let origin = motion.origin();
        let rise = motion.pitch() / TAU;
        let rise_box = Ivl::from(motion.pitch()) / Ivl::checked(TAU, TAU);
        let axis_box = boxed(axis);
        let velocity_box = add(
            cross(axis_box, sub(start_box, boxed(origin))),
            scale(axis_box, Ivl::from(motion.pitch()) / Ivl::checked(TAU, TAU)),
        );
        let normal_start_box = cross(tangent_box, velocity_box);
        let normal_rate_box = cross(tangent_box, cross(axis_box, tangent_box));
        let normal_start = tangent.cross(axis.cross(start - origin) + axis * rise);
        let normal_rate = tangent.cross(axis.cross(tangent));
        if !finite(start)
            || !finite(tangent)
            || tangent.length_squared() <= 0.0
            || !finite(normal_start)
            || !finite(normal_rate)
            || !norm_squared(normal_start_box).hi.is_finite()
            || !norm_squared(normal_rate_box).hi.is_finite()
        {
            return Err(bad(
                "the screw generator is degenerate or outside the finite range",
            ));
        }
        Ok(Self {
            start,
            tangent,
            origin,
            axis,
            rise,
            rise_box,
            distance,
            normal_start,
            normal_rate,
            start_box,
            tangent_box,
            normal_start_box,
            normal_rate_box,
        })
    }

    /// The exact generator and its first derivative at `parameter`.
    ///
    /// # Errors
    /// Refuses a nonfinite parameter, singular normal or nonfinite result. Parameters
    /// outside `[0,1]` are allowed for continued support sections, with the same checks.
    pub fn evaluate(&self, parameter: f64) -> Result<(Point3, Vec3)> {
        if !parameter.is_finite() {
            return Err(bad("a screw section parameter must be finite"));
        }
        let v = self.normal_start + self.normal_rate * parameter;
        let length = v.length();
        if !length.is_finite() || length <= 0.0 {
            return Err(bad("the screw section has a singular normal"));
        }
        let normal = v / length;
        let normal_derivative = (self.normal_rate - normal * normal.dot(self.normal_rate)) / length;
        let point = self.start + self.tangent * parameter + normal * self.distance;
        let tangent = self.tangent + normal_derivative * self.distance;
        if !finite(point) || !finite(tangent) {
            return Err(bad(
                "the screw offset evaluation is outside the finite range",
            ));
        }
        Ok((point, tangent))
    }

    /// Exact generator derivatives through order four, for normal and curvature queries.
    ///
    /// # Errors
    /// Those of [`Self::evaluate`], an order above four or nonfinite derivative arithmetic.
    pub fn derivatives(&self, parameter: f64, order: usize) -> Result<Vec<Vec3>> {
        if order > 4 {
            return Err(bad("screw section derivatives are bounded at order four"));
        }
        let (point, tangent) = self.evaluate(parameter)?;
        let mut result = vec![point];
        if order == 0 {
            return Ok(result);
        }
        result.push(tangent);
        if order == 1 {
            return Ok(result);
        }
        let v = self.normal_start + self.normal_rate * parameter;
        let g = 1.0 / v.length();
        let q = v.dot(self.normal_rate);
        let b2 = self.normal_rate.length_squared();
        let g3 = g * g * g;
        let g5 = g3 * g * g;
        let first = -q * g3;
        let second = 3.0 * q * q * g5 - b2 * g3;
        result.push((v * second + self.normal_rate * (2.0 * first)) * self.distance);
        if order >= 3 {
            let third = 9.0 * q * b2 * g5 - 15.0 * q * q * q * g5 * g * g;
            result.push((v * third + self.normal_rate * (3.0 * second)) * self.distance);
            if order == 4 {
                let fourth = 9.0 * b2 * b2 * g5 - 90.0 * q * q * b2 * g5 * g * g
                    + 105.0 * q * q * q * q * g5 * g * g * g * g;
                result.push((v * fourth + self.normal_rate * (4.0 * third)) * self.distance);
            }
        }
        if result.iter().any(|&v| !finite(v)) {
            return Err(bad("screw section derivatives overflowed"));
        }
        Ok(result)
    }

    /// Reduces a generator point to the half-plane spanned by the axis and `radial`.
    /// The radial reference is projected perpendicular to the axis and made unit.
    ///
    /// # Errors
    /// Those of [`Self::evaluate`], or a degenerate radial reference, or a point outside
    /// its open half-plane (where a continuous phase branch must be chosen by a caller).
    pub fn meridian(&self, parameter: f64, radial: Vec3) -> Result<MeridianPoint> {
        let axis = self.axis / self.axis.length();
        let reference = radial - axis * radial.dot(axis);
        let length = reference.length();
        if !length.is_finite() || length <= 0.0 {
            return Err(bad("a screw section needs a radial reference off the axis"));
        }
        let e = reference / length;
        let f = axis.cross(e);
        let (point, tangent) = self.evaluate(parameter)?;
        let p = point - self.origin;
        let x = p.dot(e);
        let y = p.dot(f);
        let dx = tangent.dot(e);
        let dy = tangent.dot(f);
        let radius = x.hypot(y);
        if !radius.is_finite() || x <= 0.0 || radius <= 0.0 {
            return Err(bad("the offset leaves the chosen open axial half-plane"));
        }
        let phase = y.atan2(x);
        let phase_rate = (x * dy - y * dx) / (radius * radius);
        let result = MeridianPoint {
            point: Point2::new(radius, p.dot(axis) - self.rise * phase),
            tangent: Point2::new(
                (x * dx + y * dy) / radius,
                tangent.dot(axis) - self.rise * phase_rate,
            ),
            phase,
            phase_rate,
        };
        if !result.point.x.is_finite()
            || !result.point.y.is_finite()
            || !result.tangent.x.is_finite()
            || !result.tangent.y.is_finite()
            || !phase_rate.is_finite()
        {
            return Err(bad("the meridian section is outside the finite range"));
        }
        Ok(result)
    }

    /// Encloses the meridian section and its derivative on `[from,to]`, including
    /// continued support parameters outside `[0,1]`. This is suitable for exclusion,
    /// interval-Newton and Krawczyk intersection tests; a sampled box is never used.
    ///
    /// # Errors
    /// Refuses nonfinite/reversed intervals, a radial reference on the axis, or an
    /// interval whose normal regularity or open-half-plane membership is not proved.
    /// A caller may subdivide an undecided interval within its own fixed budget.
    pub fn meridian_bounds(&self, from: f64, to: f64, radial: Vec3) -> Result<MeridianBounds> {
        Ok(self
            .meridian_family_bounds(from, to, radial, Ivl::from(self.distance))?
            .section)
    }

    pub(crate) fn meridian_family_bounds(
        &self,
        from: f64,
        to: f64,
        radial: Vec3,
        distance: Ivl,
    ) -> Result<MeridianFamilyBounds> {
        self.meridian_family_chart_bounds(from, to, radial, distance)?
            .ok_or_else(|| bad("the section interval does not prove a regular open meridian chart"))
    }

    /// The same enclosure oracle, with an explicit unresolved result for a
    /// normal or chart interval containing a singularity. Certificate callers
    /// may subdivide that parameter interval; malformed inputs remain errors.
    pub(crate) fn meridian_family_chart_bounds(
        &self,
        from: f64,
        to: f64,
        radial: Vec3,
        distance: Ivl,
    ) -> Result<Option<MeridianFamilyBounds>> {
        if !from.is_finite() || !to.is_finite() || from > to {
            return Err(bad("a meridian interval must be finite and ascending"));
        }
        if !distance.lo.is_finite() || !distance.hi.is_finite() || distance.lo > distance.hi {
            return Err(bad("a meridian offset family must be finite and ascending"));
        }
        let (e, f, axis) = self.meridian_frame(radial)?;
        let parameter = Ivl::new(from, to);
        let v = self.normal_box(parameter);
        let length = norm_squared(v).sqrt();
        if !length.lo.is_finite() || length.lo <= 0.0 {
            return Ok(None);
        }
        let inverse = Ivl::from(1.0) / length;
        let normal = scale(v, inverse);
        let normal_derivative = scale(
            sub(
                self.normal_rate_box,
                scale(normal, dot(normal, self.normal_rate_box)),
            ),
            inverse,
        );
        let point = sub(
            add(
                add(self.start_box, scale(self.tangent_box, parameter)),
                scale(normal, distance),
            ),
            boxed(self.origin),
        );
        let tangent = add(self.tangent_box, scale(normal_derivative, distance));
        let x = dot(point, e);
        let y = dot(point, f);
        if x.lo <= 0.0 {
            return Ok(None);
        }
        let dx = dot(tangent, e);
        let dy = dot(tangent, f);
        let radius_squared = square(x) + square(y);
        let radius = radius_squared.sqrt();
        let phase = atan_interval(y / x);
        let phase_rate = (x * dy - y * dx) / radius_squared;
        let result = MeridianBounds {
            radius,
            height: dot(point, axis) - self.rise_box * phase,
            radius_rate: (x * dx + y * dy) / radius,
            height_rate: dot(tangent, axis) - self.rise_box * phase_rate,
            phase,
        };
        if [
            result.radius,
            result.height,
            result.radius_rate,
            result.height_rate,
            result.phase,
        ]
        .iter()
        .any(|value| !value.lo.is_finite() || !value.hi.is_finite())
        {
            return Err(bad("the meridian interval is outside the finite range"));
        }
        let nx = dot(normal, e);
        let ny = dot(normal, f);
        let radius_distance = (x * nx + y * ny) / radius;
        let height_distance =
            dot(normal, axis) - self.rise_box * (x * ny - y * nx) / radius_squared;
        let velocity = add(
            cross(boxed(self.axis), point),
            scale(boxed(self.axis), self.rise_box),
        );
        let oriented_area = dot(cross(tangent, velocity), v);
        let result = MeridianFamilyBounds {
            section: result,
            radius_distance,
            height_distance,
            oriented_area,
        };
        if [
            result.radius_distance,
            result.height_distance,
            result.oriented_area,
        ]
        .iter()
        .any(|v| !v.lo.is_finite() || !v.hi.is_finite())
        {
            return Err(bad(
                "the meridian family derivative is outside the finite range",
            ));
        }
        Ok(Some(result))
    }

    /// Isolates an intersection with `other` in two source parameter intervals using
    /// the interval Krawczyk operator, for at most 48 contractions. This operates on
    /// screw-invariant sections; both callers must use the same axis, pitch and axial
    /// origin convention. A coincident or tangent pair is undecided, never a fake root.
    ///
    /// Exclusion follows from `K(X) ∩ X` being empty. Existence and uniqueness require
    /// `K(X)` strictly inside `X` and a contraction in the scaled box norm. These
    /// statements use interval values and derivatives over the entire rectangle.
    /// Endpoint/midpoint evaluations merely choose a preconditioner.
    ///
    /// # Errors
    /// Refuses invalid parameter intervals/tolerance or inconsistent screw motions.
    /// Unproved normal/half-plane regularity is reported as `Undecided`.
    pub fn isolate_intersection(
        &self,
        other: &Self,
        radial: Vec3,
        first: Ivl,
        second: Ivl,
        tolerance: f64,
    ) -> Result<IntersectionIsolation> {
        self.isolate_intersection_shifted(other, radial, first, second, Ivl::from(0.0), tolerance)
    }

    /// As [`Self::isolate_intersection`], with the second section translated axially
    /// by `height_shift`. For screw orbits use `height_shift=n*pitch`: meridian height
    /// is periodic modulo pitch. With this convention the second wall's angle is the
    /// first wall's angle plus `phase_first-phase_second+2*pi*n`.
    ///
    /// `height_shift` is an interval so multiplying the integer winding by pitch can
    /// be enclosed; a successful proof applies to every shift in that enclosure.
    ///
    /// # Errors
    /// Those of [`Self::isolate_intersection`], or a nonfinite/reversed shift interval.
    pub fn isolate_intersection_shifted(
        &self,
        other: &Self,
        radial: Vec3,
        first: Ivl,
        second: Ivl,
        height_shift: Ivl,
        tolerance: f64,
    ) -> Result<IntersectionIsolation> {
        if !tolerance.is_finite()
            || tolerance <= 0.0
            || !height_shift.lo.is_finite()
            || !height_shift.hi.is_finite()
            || height_shift.lo > height_shift.hi
            || [first, second]
                .iter()
                .any(|x| !x.lo.is_finite() || !x.hi.is_finite() || x.lo >= x.hi)
            || self.origin != other.origin
            || self.axis != other.axis
            || self.rise != other.rise
        {
            return Err(bad(
                "section isolation needs finite intervals and one common screw motion",
            ));
        }
        let mut first = first;
        let mut second = second;
        let mut unique = false;
        for _ in 0..48 {
            let undecided = IntersectionIsolation::Undecided { first, second };
            let (Ok(a), Ok(b)) = (
                self.meridian_bounds(first.lo, first.hi, radial),
                other.meridian_bounds(second.lo, second.hi, radial),
            ) else {
                return Ok(undecided);
            };
            let (Some(radius), Some(height)) = (
                overlap(a.radius, b.radius),
                overlap(a.height, b.height + height_shift),
            ) else {
                return Ok(IntersectionIsolation::NoRoot);
            };
            if unique
                && [a.radius, a.height, b.radius, b.height]
                    .iter()
                    .all(|value| (Ivl::from(value.hi) - Ivl::from(value.lo)).hi <= tolerance)
            {
                return Ok(IntersectionIsolation::Unique(SectionIntersection {
                    first,
                    second,
                    radius,
                    height,
                }));
            }
            let x = first.mid();
            let y = second.mid();
            let (Ok(am), Ok(bm)) = (
                self.meridian_bounds(x, x, radial),
                other.meridian_bounds(y, y, radial),
            ) else {
                return Ok(undecided);
            };
            let j00 = a.radius_rate;
            let j01 = negative(b.radius_rate);
            let j10 = a.height_rate;
            let j11 = negative(b.height_rate);
            let determinant = j00.mid() * j11.mid() - j01.mid() * j10.mid();
            if !determinant.is_finite() || determinant == 0.0 {
                return Ok(undecided);
            }
            // C is any nonsingular point matrix. It need not be an exact inverse.
            let c00 = Ivl::from(j11.mid() / determinant);
            let c01 = Ivl::from(-j01.mid() / determinant);
            let c10 = Ivl::from(-j10.mid() / determinant);
            let c11 = Ivl::from(j00.mid() / determinant);
            let cdet = c00 * c11 - c01 * c10;
            if !cdet.lo.is_finite() || !cdet.hi.is_finite() || (cdet.lo <= 0.0 && cdet.hi >= 0.0) {
                return Ok(undecided);
            }
            let one = Ivl::from(1.0);
            let r00 = one - c00 * j00 - c01 * j10;
            let r01 = negative(c00 * j01 + c01 * j11);
            let r10 = negative(c10 * j00 + c11 * j10);
            let r11 = one - c10 * j01 - c11 * j11;
            let f0 = am.radius - bm.radius;
            let f1 = am.height - bm.height - height_shift;
            let dx = first - Ivl::from(x);
            let dy = second - Ivl::from(y);
            let k0 = Ivl::from(x) - c00 * f0 - c01 * f1 + r00 * dx + r01 * dy;
            let k1 = Ivl::from(y) - c10 * f0 - c11 * f1 + r10 * dx + r11 * dy;
            let (Some(next_first), Some(next_second)) = (overlap(first, k0), overlap(second, k1))
            else {
                return Ok(IntersectionIsolation::NoRoot);
            };
            // The weighted infinity norm uses each box's half-width. Intervals also
            // enclose this test so a rounded value just below one cannot prove it.
            let wx = Ivl::from(dx.mag());
            let wy = Ivl::from(dy.mag());
            let row0 = Ivl::from(r00.mag()) + Ivl::from(r01.mag()) * wy / wx;
            let row1 = Ivl::from(r11.mag()) + Ivl::from(r10.mag()) * wx / wy;
            unique |= k0.lo > first.lo
                && k0.hi < first.hi
                && k1.lo > second.lo
                && k1.hi < second.hi
                && row0.hi < 1.0
                && row1.hi < 1.0;
            if next_first == first && next_second == second {
                return Ok(undecided);
            }
            first = next_first;
            second = next_second;
        }
        Ok(IntersectionIsolation::Undecided { first, second })
    }

    fn normal_box(&self, parameter: Ivl) -> Box3 {
        add(
            self.normal_start_box,
            scale(self.normal_rate_box, parameter),
        )
    }

    /// An orthonormal meridian frame about the mathematical unit direction of
    /// the stored axis. Enclose every projection/normalization operation before
    /// using its result as a coefficient in a phase or section certificate.
    pub(crate) fn meridian_frame(&self, radial: Vec3) -> Result<(Box3, Box3, Box3)> {
        let k = boxed(self.axis);
        let axis_length = norm_squared(k).sqrt();
        if axis_length.lo <= 0.0 || !axis_length.hi.is_finite() {
            return Err(bad("a meridian frame needs a nonzero finite axis"));
        }
        let axis = scale(k, Ivl::from(1.0) / axis_length);
        let radial = boxed(radial);
        let reference = sub(radial, scale(axis, dot(radial, axis)));
        let length = norm_squared(reference).sqrt();
        if length.lo <= 0.0 || !length.hi.is_finite() {
            return Err(bad("a meridian reference cannot be proved off the axis"));
        }
        let e = scale(reference, Ivl::from(1.0) / length);
        Ok((e, cross(axis, e), axis))
    }

    /// At distance exactly zero the section is the source line itself, so its
    /// normal is never normalised: a line whose normal vanishes somewhere (a
    /// skew line, one crossing the axis) still has an exact enclosure.
    fn point_box(&self, t: Ivl) -> Box3 {
        let line = add(self.start_box, scale(self.tangent_box, t));
        if self.distance == 0.0 {
            return line;
        }
        let normal = self.normal_box(t);
        let inverse_length = Ivl::from(1.0) / norm_squared(normal).sqrt();
        add(
            line,
            scale(normal, inverse_length * Ivl::from(self.distance)),
        )
    }

    /// Exact retained source enclosure for checked definition evaluation.
    pub(crate) fn point_enclosure(&self, parameter: Ivl) -> Result<[Ivl; 3]> {
        if !parameter.lo.is_finite() || !parameter.hi.is_finite() || parameter.lo > parameter.hi {
            return Err(bad("source enclosure needs finite ordered parameters"));
        }
        let point = self.point_box(parameter);
        if point.iter().any(|v| !v.lo.is_finite() || !v.hi.is_finite()) {
            return Err(bad("source point has no finite regular enclosure"));
        }
        Ok(point)
    }

    /// Upper speeds in source parameter and physical angle, over all turns.
    /// Triangle bounds for Rodrigues and its derivative remain valid for the
    /// rounded stored axis. These bounds convert pcurve coordinate certificates
    /// to model units; singular or unbounded source intervals are refused.
    /// At distance exactly zero the section's tangent is the source tangent and
    /// the normal is not needed, so a vanishing normal is not refused.
    pub(crate) fn speed_upper_bounds(&self, parameter: Ivl) -> Result<[f64; 2]> {
        let tangent = if self.distance == 0.0 {
            self.tangent_box
        } else {
            let v = self.normal_box(parameter);
            let length = norm_squared(v).sqrt();
            if !length.lo.is_finite() || length.lo <= 0.0 {
                return Err(bad("pcurve range has no regular normal enclosure"));
            }
            let inverse = Ivl::from(1.0) / length;
            let normal = scale(v, inverse);
            let dn = scale(
                sub(
                    self.normal_rate_box,
                    scale(normal, dot(normal, self.normal_rate_box)),
                ),
                inverse,
            );
            add(self.tangent_box, scale(dn, Ivl::from(self.distance)))
        };
        let k = boxed(self.axis);
        let q = norm_squared(k);
        let rotation = Ivl::from(1.0) + q.sqrt() + Ivl::from(2.0) * q;
        let profile = (rotation * norm_squared(tangent).sqrt()).hi;
        let r = sub(self.point_box(parameter), boxed(self.origin));
        let turn = (norm_squared(cross(k, r)).sqrt()
            + norm_squared(sub(scale(k, dot(k, r)), r)).sqrt()
            + Ivl::from(self.rise_box.mag()) * q.sqrt())
        .hi;
        if !profile.is_finite() || !turn.is_finite() {
            return Err(bad("pcurve derivative enclosure is not finite"));
        }
        Ok([profile, turn])
    }

    /// Proves that the offset has not passed a focal singularity on this support.
    /// An affine generator makes a ruled surface with nonpositive Gaussian
    /// curvature. Its oriented parallel-area factor is a concave quadratic in the
    /// distance, positive at zero; positivity at `distance` therefore also proves
    /// positivity at every intervening offset.
    ///
    /// # Errors
    /// Refuses a fold, a singular source or an interval not proved regular within
    /// the same fixed subdivision budget used by section approximation.
    pub fn certify_regular(&self, from: f64, to: f64) -> Result<()> {
        if !from.is_finite() || !to.is_finite() || from >= to {
            return Err(bad("regularity needs a finite ascending source interval"));
        }
        let mut pending = vec![(from, to, 0_u32)];
        let mut splits = 0_usize;
        while let Some((from, to, depth)) = pending.pop() {
            let parameter = Ivl::new(from, to);
            let v = self.normal_box(parameter);
            let length = norm_squared(v).sqrt();
            let inverse = Ivl::from(1.0) / length;
            let n = scale(v, inverse);
            let dn = scale(
                sub(self.normal_rate_box, scale(n, dot(n, self.normal_rate_box))),
                inverse,
            );
            let tangent = add(self.tangent_box, scale(dn, Ivl::from(self.distance)));
            let velocity = add(
                cross(
                    boxed(self.axis),
                    sub(self.point_box(parameter), boxed(self.origin)),
                ),
                scale(boxed(self.axis), self.rise_box),
            );
            let area = dot(cross(tangent, velocity), v);
            if length.lo > 0.0 && area.lo > 0.0 && area.hi.is_finite() {
                continue;
            }
            if area.hi <= 0.0 {
                return Err(bad(
                    "the requested screw offset reaches or passes a focal fold",
                ));
            }
            if depth >= MAX_DEPTH || splits >= MAX_LEAVES - 1 {
                return Err(bad(
                    "screw offset regularity is not proved within its subdivision budget",
                ));
            }
            let mid = from + (to - from) * 0.5;
            if mid <= from || mid >= to {
                return Err(bad("screw offset regularity cannot subdivide further"));
            }
            pending.push((mid, to, depth + 1));
            pending.push((from, mid, depth + 1));
            splits += 1;
        }
        Ok(())
    }

    fn endpoint_error(&self, parameter: Ivl, point: Point3) -> f64 {
        norm_squared(sub(self.point_box(parameter), boxed(point)))
            .sqrt()
            .hi
    }

    /// Encloses the error of a chord between two numerically evaluated endpoints.
    fn chord_bound(&self, from: f64, to: f64, a: Point3, b: Point3) -> f64 {
        self.chord_bound_enclosed(Ivl::from(from), Ivl::from(to), a, b)
    }

    /// Certifies a chord whose exact end parameters are enclosed, including the
    /// arithmetic of mapping a stored profile knot back to its source parameter.
    pub(crate) fn chord_bound_enclosed(&self, from: Ivl, to: Ivl, a: Point3, b: Point3) -> f64 {
        let lower = norm_squared(self.normal_box(Ivl::new(from.lo, to.hi))).lo;
        if lower <= 0.0 {
            return f64::INFINITY;
        }
        // V=A+xB, so V×B=A×B is constant over the whole source interval.
        // The header's norm identity bounds the exact second derivative, not
        // sampled derivatives; all coefficient and denominator rounding is enclosed.
        let cross_length = norm_squared(cross(self.normal_start_box, self.normal_rate_box)).sqrt();
        let rate_length = norm_squared(self.normal_rate_box).sqrt();
        let length_squared = Ivl::from(lower);
        let curvature =
            Ivl::from(2.0) * Ivl::from(self.distance.abs()) * cross_length * rate_length
                / (length_squared * length_squared.sqrt());
        let width = to - from;
        let interpolation = curvature * width * width / Ivl::from(8.0);
        let endpoints = self.endpoint_error(from, a).max(self.endpoint_error(to, b));
        (interpolation + Ivl::from(endpoints)).hi
    }

    /// Makes a piecewise-linear generator with a certified error at most `tolerance`.
    /// The certificate compares the mathematical interpolant with the mathematical
    /// normal offset; the screw-net builder must add its own evaluation/rounding bound.
    ///
    /// # Errors
    /// Refuses a nonpositive/nonfinite tolerance, a singular generator, or a requested
    /// tolerance not established within 16,384 leaves and subdivision depth 48.
    pub fn approximate(&self, tolerance: f64) -> Result<CertifiedSection> {
        self.approximate_over(0.0, 1.0, tolerance)
    }

    /// As [`Self::approximate`], on the continued generator interval `[from,to]`.
    /// Parameters retain the original source-line map, even beyond its original ends.
    ///
    /// # Errors
    /// Those of [`Self::approximate`], or a nonfinite/reversed/zero-length interval.
    pub fn approximate_over(&self, from: f64, to: f64, tolerance: f64) -> Result<CertifiedSection> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(bad("a section tolerance must be finite and positive"));
        }
        if !from.is_finite() || !to.is_finite() || from >= to {
            return Err(bad(
                "a section approximation interval must be finite and ascending",
            ));
        }
        let first = self.evaluate(from)?.0;
        let last = self.evaluate(to)?.0;
        let mut pending = vec![(from, to, first, last, 0_u32)];
        let mut result = CertifiedSection {
            parameters: vec![from],
            points: vec![first],
            bound: 0.0,
        };
        let mut splits = 0_usize;
        while let Some((from, to, a, b, depth)) = pending.pop() {
            let bound = self.chord_bound(from, to, a, b);
            if bound.is_finite() && bound <= tolerance {
                result.parameters.push(to);
                result.points.push(b);
                result.bound = result.bound.max(bound);
                continue;
            }
            if depth >= MAX_DEPTH || splits >= MAX_LEAVES - 1 {
                return Err(bad(
                    "the screw section error could not be certified within its budget",
                ));
            }
            let mid = from + (to - from) * 0.5;
            if mid <= from || mid >= to {
                return Err(bad(
                    "the screw section cannot subdivide its parameter further",
                ));
            }
            let point = self.evaluate(mid)?.0;
            // LIFO, left before right, so accepted endpoints are strictly ascending.
            pending.push((mid, to, point, b, depth + 1));
            pending.push((from, mid, a, point, depth + 1));
            splits += 1;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::procedural_sweep::RadialLaw;

    fn motion() -> ScrewMotion {
        ScrewMotion::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 2.5, None).unwrap()
    }

    #[test]
    fn a_flank_moves_along_the_three_dimensional_normal() {
        let start = Vec3::new(8.6, 0.0, -0.3);
        let end = Vec3::new(10.0, 0.0, 0.5);
        for distance in [-0.1, 0.1] {
            let section = ScrewOffsetSection::new(start, end, &motion(), distance).unwrap();
            for x in [0.0, 0.137, 0.5, 0.831, 1.0] {
                let source = start + (end - start) * x;
                let velocity = Vec3::new(-source.y, source.x, 2.5 / TAU);
                let normal = (end - start).cross(velocity).unitize().unwrap();
                let (point, tangent) = section.evaluate(x).unwrap();
                assert!((point - source - normal * distance).length() < 1e-14);
                assert!(point.y.abs() > 1e-3);
                assert!(tangent.dot(normal).abs() < 1e-13);
            }
        }
    }

    /// A skew line whose screw velocity is parallel to it at its midpoint has a
    /// vanishing surface normal there. At distance zero the section is the line
    /// itself: its point and speed enclosures never normalise that normal, so
    /// they exist and contain the exact line; any nonzero offset still refuses.
    #[test]
    fn a_zero_offset_encloses_a_line_whose_normal_vanishes() {
        let rise = 2.5 / TAU;
        let slope = rise / 8.0;
        let start = Vec3::new(8.0, -1.0, -slope);
        let end = Vec3::new(8.0, 1.0, slope);
        let section = ScrewOffsetSection::new(start, end, &motion(), 0.0).unwrap();
        assert!(norm_squared(section.normal_box(Ivl::new(0.0, 1.0))).lo <= 0.0);
        let whole = Ivl::new(0.0, 1.0);
        let point = section.point_enclosure(whole).unwrap();
        for x in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let exact = start + (end - start) * x;
            for (enclosure, value) in point.iter().zip([exact.x, exact.y, exact.z]) {
                assert!(enclosure.lo <= value && value <= enclosure.hi);
            }
        }
        let [profile, turn] = section.speed_upper_bounds(whole).unwrap();
        assert!(profile >= (end - start).length());
        assert!(turn >= Vec3::new(1.0, 8.0, rise).length());
        let offset = ScrewOffsetSection::new(start, end, &motion(), 0.1).unwrap();
        assert!(offset.speed_upper_bounds(whole).is_err());
        assert!(offset.point_enclosure(whole).is_err());
    }

    #[test]
    fn affine_normal_curvature_agrees_with_its_constant_cross_product_identity() {
        let motion = ScrewMotion::new(
            Vec3::new(-1.0, 0.3, 0.2),
            Vec3::new(0.3, 0.1, 1.0),
            2.5,
            None,
        )
        .unwrap();
        for distance in [-0.1, 0.1] {
            let section = ScrewOffsetSection::new(
                Vec3::new(2.0, 1.0, 0.3),
                Vec3::new(3.0, 1.3, 0.8),
                &motion,
                distance,
            )
            .unwrap();
            let a = section.normal_start;
            let b = section.normal_rate;
            let q2 = a.cross(b).length_squared();
            for x in [-0.2, 0.0, 0.173, 0.5, 1.0, 1.2] {
                let v = a + b * x;
                let length2 = v.length_squared();
                let vb = v.dot(b);
                let expected =
                    distance.abs() * (q2 * (q2 + 4.0 * vb * vb)).sqrt() / (length2 * length2);
                let second = section.derivatives(x, 2).unwrap()[2].length();
                assert!((second - expected).abs() <= 2e-13 * expected);
            }
        }
    }

    #[test]
    fn literal_m20_profiles_fit_the_existing_display_budget() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::surface::procedural_sweep::offset_profile::OffsetProfile;

        let motion = ScrewMotion::new(Vec3::new(-1.0, -64.0, 0.0), Vec3::Z, 2.5, None).unwrap();
        let flanks = [
            [
                Vec3::new(7.646835306586814, -64.0, -2.8125),
                Vec3::new(9.125, -64.0, -3.6659187836487037),
            ],
            [
                Vec3::new(9.125, -64.0, -1.3340812163512963),
                Vec3::new(7.646835306586814, -64.0, -2.1875),
            ],
        ];
        // The actual retained turn structure with three independently certified
        // subarcs has 3,361 controls. The existing evaluator budget permits at
        // most 77 profile controls (78 would exceed it by fourteen).
        let angular_controls = 3361;
        let display_limit = crate::eval::spline::MAX_EVAL_POINTS;
        for [start, end] in flanks {
            let source = BSplineCurve::create_line_segment(3, start, end).unwrap();
            for distance in [-0.1, 0.1] {
                for (domain, epsilon) in [
                    ([0.0, 1.0], 1.0000000000000002e-6),
                    ([-0.1270970031047413, 1.127097003104742], 5e-6),
                ] {
                    let budget = (Ivl::from(epsilon) * Ivl::from(0.25)).lo;
                    let (profile, exact) =
                        OffsetProfile::build(&source, &motion, distance, domain, budget).unwrap();
                    let count = profile.control_points().unwrap().len();
                    eprintln!(
                        "M20_PROFILE distance={distance} domain={domain:?} budget={budget:.17e} bound={:.17e} rows={count} tensor_controls={}",
                        exact.bound,
                        count * angular_controls
                    );
                    assert!(exact.bound <= budget);
                    assert!(
                        count * angular_controls <= display_limit,
                        "rows={count}, limit={display_limit}"
                    );
                    assert_eq!(exact.certify(&profile, &motion).unwrap(), exact.bound);
                    assert_eq!(
                        exact.source.canonical().unwrap().control_points,
                        [start, end]
                    );
                    assert_eq!(
                        [
                            profile.natural_interval().min(),
                            profile.natural_interval().max()
                        ],
                        domain
                    );
                }
            }
        }
    }

    #[test]
    fn the_meridian_phase_recovers_the_original_generator() {
        let section = ScrewOffsetSection::new(
            Vec3::new(8.6, 0.0, -0.3),
            Vec3::new(10.0, 0.0, 0.5),
            &motion(),
            0.1,
        )
        .unwrap();
        for t in [0.0, 0.231, 0.719, 1.0] {
            let m = section.meridian(t, Vec3::new(1.0, 0.0, 0.0)).unwrap();
            let reconstructed = Vec3::new(
                m.point.x * m.phase.cos(),
                m.point.x * m.phase.sin(),
                m.point.y + (2.5 / TAU) * m.phase,
            );
            assert!((reconstructed - section.evaluate(t).unwrap().0).length() < 1e-13);
        }
    }

    #[test]
    fn the_certificate_covers_interior_parameters_off_the_interpolation_net() {
        let section = ScrewOffsetSection::new(
            Vec3::new(8.6, 0.0, -0.3),
            Vec3::new(10.0, 0.0, 0.5),
            &motion(),
            -0.1,
        )
        .unwrap();
        let net = section.approximate(1e-7).unwrap();
        assert!(net.bound <= 1e-7);
        assert!(net.parameters.len() > 2);
        for (parameters, points) in net.parameters.windows(2).zip(net.points.windows(2)) {
            let [a, b] = parameters else { unreachable!() };
            let [pa, pb] = points else { unreachable!() };
            for fraction in [0.113, 0.381, 0.719, 0.937] {
                let parameter = a + (b - a) * fraction;
                let interpolated = *pa + (*pb - *pa) * fraction;
                let actual = section.evaluate(parameter).unwrap().0;
                assert!((actual - interpolated).length() <= net.bound);
            }
        }
    }

    #[test]
    fn singular_and_noninvariant_inputs_are_refused() {
        assert!(ScrewOffsetSection::new(Vec3::ZERO, Vec3::ZERO, &motion(), 0.1).is_err());
        let law = RadialLaw::new(Vec3::new(1.0, 0.0, 0.0), vec![0.0, 1.0], vec![0.0, 1.0]).unwrap();
        let varying =
            ScrewMotion::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 2.5, Some(law)).unwrap();
        assert!(ScrewOffsetSection::new(
            Vec3::new(8.6, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.5),
            &varying,
            0.1
        )
        .is_err());
        let axial =
            ScrewOffsetSection::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), &motion(), 0.1).unwrap();
        assert!(axial.evaluate(0.5).is_err());
        assert!(axial.approximate(1e-6).is_err());
    }

    #[test]
    fn meridian_phase_encloses_oblique_reference_normalization() {
        let motion = ScrewMotion::new(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 2.5, None).unwrap();
        let section = ScrewOffsetSection::new(
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 2.0),
            &motion,
            0.0,
        )
        .unwrap();
        let radial = motion.axis() * 1e6 + Vec3::X;
        let bounds = section.meridian_bounds(0.371, 0.371, radial).unwrap();
        // Independent 90-digit Decimal calculation from the exact binary64 input
        // axis/radial/parameter: normalize k, project r-k(k.r), normalize e, then
        // atan((P.(k cross e))/(P.e)). The atan alternating series through 299th
        // order has remainder below 1e-390 here. This point is off the source net.
        let phase = Ivl::new(-0.049_793_840_382_288_50, -0.049_793_840_382_288_48);
        assert!(
            bounds.phase.lo <= phase.lo && bounds.phase.hi >= phase.hi,
            "phase {:?} misses independent witness {phase:?}",
            bounds.phase
        );
    }

    #[test]
    fn meridian_frame_encloses_unit_orthogonal_directions() {
        for axis in [Vec3::Z, Vec3::new(1.0, 2.0, 3.0)] {
            let motion = ScrewMotion::new(Vec3::ZERO, axis, 2.5, None).unwrap();
            let section = ScrewOffsetSection::new(
                Vec3::new(8.0, 0.0, 0.0),
                Vec3::new(10.0, 0.0, 2.0),
                &motion,
                0.0,
            )
            .unwrap();
            for radial in [Vec3::X, motion.axis() * 1e6 + Vec3::X] {
                let (e, f, k) = section.meridian_frame(radial).unwrap();
                for direction in [e, f, k] {
                    let squared = norm_squared(direction);
                    assert!(squared.lo <= 1.0 && squared.hi >= 1.0, "{squared:?}");
                }
                for product in [dot(e, f), dot(e, k), dot(f, k)] {
                    assert!(product.lo <= 0.0 && product.hi >= 0.0, "{product:?}");
                }
            }
        }
    }

    #[test]
    fn interval_boxes_enclose_points_and_derivatives() {
        let section = ScrewOffsetSection::new(
            Vec3::new(8.6, 0.0, -0.3),
            Vec3::new(10.0, 0.0, 0.5),
            &motion(),
            -0.1,
        )
        .unwrap();
        let radial = Vec3::new(1.0, 0.0, 0.0);
        let bounds = section.meridian_bounds(-0.1, 1.1, radial).unwrap();
        for t in [-0.1, 0.0, 0.371, 0.819, 1.1] {
            let point = section.meridian(t, radial).unwrap();
            for (interval, value) in [
                (bounds.radius, point.point.x),
                (bounds.height, point.point.y),
                (bounds.radius_rate, point.tangent.x),
                (bounds.height_rate, point.tangent.y),
                (bounds.phase, point.phase),
            ] {
                assert!(interval.lo <= value && value <= interval.hi);
            }
        }
        let net = section.approximate_over(-0.1, 1.1, 1e-6).unwrap();
        assert_eq!(net.parameters.first(), Some(&-0.1));
        assert_eq!(net.parameters.last(), Some(&1.1));
    }

    #[test]
    fn krawczyk_distinguishes_a_unique_root_no_root_and_coincidence() {
        let a = ScrewOffsetSection::new(
            Vec3::new(8.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 2.0),
            &motion(),
            0.0,
        )
        .unwrap();
        let b = ScrewOffsetSection::new(
            Vec3::new(8.0, 0.0, 2.0),
            Vec3::new(10.0, 0.0, 0.0),
            &motion(),
            0.0,
        )
        .unwrap();
        let radial = Vec3::new(1.0, 0.0, 0.0);
        let domain = Ivl::new(0.0, 1.0);
        let root = a
            .isolate_intersection(&b, radial, domain, domain, 1e-8)
            .unwrap();
        let IntersectionIsolation::Unique(root) = root else {
            panic!("expected a proved unique root, got {root:?}");
        };
        assert!(root.first.lo <= 0.5 && root.first.hi >= 0.5);
        assert!(root.second.lo <= 0.5 && root.second.hi >= 0.5);
        assert!(root.radius.lo <= 9.0 && root.radius.hi >= 9.0);
        assert!(root.height.lo <= 1.0 && root.height.hi >= 1.0);
        assert!(matches!(
            a.isolate_intersection(&b, radial, Ivl::new(0.0, 0.1), domain, 1e-8)
                .unwrap(),
            IntersectionIsolation::NoRoot
        ));
        assert!(matches!(
            a.isolate_intersection(&a, radial, domain, domain, 1e-8)
                .unwrap(),
            IntersectionIsolation::Undecided { .. }
        ));
    }
}
