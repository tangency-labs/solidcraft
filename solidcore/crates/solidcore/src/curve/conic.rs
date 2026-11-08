//! Simple rational parametric 2D conics (ellipse, hyperbola, parabola, line) in a
//! rotated and translated natural position, with implicit forms, conic/conic
//! intersection and point inversion (the Dupin indicatrix curves of the surface
//! intersector).
//!
//! Upstream: `source/SMLib/inc/SmConic.h`, `source/SMLib/src/SmConic.cpp`.
//!
//! | Rust                                   | Upstream                                       |
//! |----------------------------------------|------------------------------------------------|
//! | [`ConicType`]                          | `SmConicType`                                  |
//! | [`Conic`]                              | `SmConic`                                      |
//! | [`Conic::new`]                         | `SmConic(SmConicType, double, double)`         |
//! | [`Conic::empty`], `Default`            | `SmConic()`                                    |
//! | `Drop` (implicit)                      | `SmConic::~SmConic`                            |
//! | [`Conic::approx_eq`]                   | `SmConic::operator==`                          |
//! | [`Conic::calculate_continuities`]      | `SmConic::CalculateContinuities`               |
//! | [`Conic::natural_interval`]            | `SmConic::GetNaturalInterval`                  |
//! | [`Conic::knots`]                       | `SmConic::GetKnots`                            |
//! | [`Conic::evaluate`], [`Conic::evaluate_with_fixes`] | `SmConic::Evaluate`               |
//! | [`Conic::evaluate_point`]              | `SmConic::EvaluatePoint`                       |
//! | [`Conic::compute_implicit_equation`]   | `SmConic::ComputeImplicitEquation`             |
//! | [`Conic::intersect_implicit_conic`]    | `SmConic::IntersectImplicitConic`              |
//! | [`Conic::intersect_conic`], [`ConicIntersection`] | `SmConic::IntersectConic`           |
//! | [`Conic::invert_point_on_conic`]       | `SmConic::InvertPointOnConic`                  |
//! | [`Conic::transform_2d`]                | `SmConic::Transform2D`                         |
//! | [`Conic::read_from_db`]                | `SmConic::ReadFromDB`                          |
//! | [`Conic::TYPE`], [`Conic::type_code`], [`Conic::type_string`], [`Conic::class_string`] | `GetType`, `GetTypeString`, `GetClassString` (`SM_COMMON`) |
//! | [`Conic::is_kind_of`]                  | `SmConic::IsKindOf`                            |
//!
//! Deviations:
//! - **Curve base.** `SmConic` derives from `SmCurve` with image dimension 2
//!   (`SmCurve(2)`, unit `curve.curve_p5`: it only sets the dimension and sends the
//!   construction notification, which is not modelled); [`Conic::dim`] is that
//!   constant. `SmCurve::operator==` (same type, same dimension) always holds
//!   between two conics. As a [`crate::curve::curve::Curve`] kind the class gets
//!   the `SmCurve` base defaults.
//! - **Notifications.** `Transform2D` sends `SM_NO_PRE_EDIT` to `SmCurve::Notify`,
//!   which drops the curve's object cache; a [`Conic`] has no cache or context, so
//!   nothing is sent.
//! - Out-params become return values; `Evaluate`'s `bFromLeft` (only passed to a
//!   recursive call that cannot be reached) and `IntersectConic`'s two intervals
//!   (`SM_REF2`, unused) are dropped. `IntersectConic`'s `SmSolutionArray` (default
//!   solutions with `m_vStart = [t, t_other]` and the distance as solution value)
//!   becomes a `Vec` of [`ConicIntersection`]; `IntersectImplicitConic`'s
//!   multiplicity array (never written) is dropped; `InvertPointOnConic`'s success
//!   flag and parameter become an `Option`.
//! - **Uninitialised data.** By default (`WR-27`,
//!   `plan/findings/curve.conic--notes.md`), `Evaluate`'s zero-tangent fix reads
//!   the second-derivative slot of the caller's array *before* writing it; the
//!   port reads a fresh `SmVector3d` there ([`Vec3::UNDEF`]), whose length never
//!   exceeds the threshold, so the fix never fires. With the opt-in
//!   [`UpstreamFixes::conic_evaluate_uses_computed_second_deriv`],
//!   [`Conic::evaluate_with_fixes`] (and [`crate::curve::curve::Curve::evaluate`])
//!   uses the computed second derivative instead. `EvaluatePoint` of an
//!   [`ConicType::Unknown`] conic leaves `x`, `y` of the caller's point unwritten;
//!   the port starts from a fresh point ([`crate::math::UNDEF_DOUBLE`]).
//!   `IntersectImplicitConic` (and so `IntersectConic`) of an unknown conic solves a
//!   quartic of uninitialised coefficients; the port returns
//!   [`SolidError::InvalidInput`].
//! - **`ReadFromDB`** always builds a new curve (the "fill the curve passed in"
//!   branch and its `IsKindOf` check have no counterpart; the type, dimension,
//!   context and version arguments are unused upstream and dropped). A conic type
//!   code outside `0..=3` behaves as `SM_CT_UNKNOWN` in every method here and is
//!   stored as [`ConicType::Unknown`]. An ASCII type code whose extraction is
//!   skipped (upstream: uninitialised) and a short binary read are errors.
//! - `WriteToDB`, `GetMemoryUsed`, `GetRootCurve` and `Dump` are not part of this
//!   unit.

use crate::curve::curve::p2::{self as curve_p2, CONIC_TYPE};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::polynomial::{solve_quadratic, solve_quartic};
use crate::geom::tol::is_zero;
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::UpstreamFixes;
use crate::math::{EFF_ZERO, EFF_ZERO_SQRT, UNDEF_DOUBLE};
use crate::topology::database_io::{DatabaseReader, FileType, InputStream};

/// Operation path of [`Conic::intersect_implicit_conic`].
const OP_INTERSECT_IMPLICIT: &str = "curve::conic::Conic::intersect_implicit_conic";

/// Operation path of [`Conic::read_from_db`].
const OP_READ: &str = "curve::conic::Conic::read_from_db";

/// Operation path of [`Conic::evaluate`].
const OP_EVALUATE: &str = "curve::conic::Conic::evaluate";

/// The kind of a [`Conic`] (`SmConicType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConicType {
    /// `SM_CT_ELLIPSE`: `x = a (1 - t²) / (1 + t²)`, `y = b 2t / (1 + t²)`.
    Ellipse = 0,
    /// `SM_CT_HYPERBOLA`: `x = a (1 + t²) / (1 - t²)`, `y = b 2t / (1 - t²)`.
    Hyperbola = 1,
    /// `SM_CT_PARABOLA`: `x = a t²`, `y = 2 a t`.
    Parabola = 2,
    /// `SM_CT_LINE`: `x = t`, `y = a`.
    Line = 3,
    /// `SM_CT_UNKNOWN`.
    #[default]
    Unknown = 4,
}

impl ConicType {
    /// The upstream enum value.
    #[must_use]
    pub const fn code(self) -> i64 {
        self as i64
    }

    /// The type of upstream value `code`; values outside `0..=3` behave as
    /// `SM_CT_UNKNOWN` and map to [`ConicType::Unknown`].
    #[must_use]
    pub const fn from_code(code: i64) -> Self {
        match code {
            0 => Self::Ellipse,
            1 => Self::Hyperbola,
            2 => Self::Parabola,
            3 => Self::Line,
            _ => Self::Unknown,
        }
    }
}

/// One intersection of [`Conic::intersect_conic`]: an `SmSolution` with
/// `m_vStart = [param, other_param]` and solution value `distance`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConicIntersection {
    /// Parameter on the conic intersected (`m_vStart[0]`).
    pub param: f64,
    /// Parameter on the other conic (`m_vStart[1]`).
    pub other_param: f64,
    /// Distance between the two points (`m_vStart.m_dSolutionValue`).
    pub distance: f64,
}

/// A 2D conic (`SmConic`) in its natural position (see [`ConicType`]), rotated
/// by `-theta` and translated by `origin`. Parameterised over the whole real line;
/// see [`Conic::natural_interval`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conic {
    /// `m_eConicType`.
    kind: ConicType,
    /// `m_dA`.
    a: f64,
    /// `m_dB`.
    b: f64,
    /// `m_vOrigin`.
    origin: Point2,
    /// `m_dThetaRad`.
    theta_rad: f64,
    /// `m_dSinTheta`.
    sin_theta: f64,
    /// `m_dCosTheta`.
    cos_theta: f64,
}

/// `SmConic()`: [`Conic::empty`].
impl Default for Conic {
    fn default() -> Self {
        Self::empty()
    }
}

impl Conic {
    /// `SmConic_TYPE` (`CURV_BASE_TYPE + 20`).
    pub const TYPE: i64 = CONIC_TYPE;

    /// A conic of `conic_type` with constants `a`, `b`, unrotated at the origin.
    #[must_use]
    pub const fn new(conic_type: ConicType, a: f64, b: f64) -> Self {
        Self {
            kind: conic_type,
            a,
            b,
            origin: Vec2::new(0.0, 0.0),
            theta_rad: 0.0,
            sin_theta: 0.0,
            cos_theta: 1.0,
        }
    }

    /// The empty conic built for reading (`SmConic()`): unknown type, every value
    /// uninitialised ([`UNDEF_DOUBLE`]).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            kind: ConicType::Unknown,
            a: UNDEF_DOUBLE,
            b: UNDEF_DOUBLE,
            origin: Vec2::UNDEF,
            theta_rad: UNDEF_DOUBLE,
            sin_theta: UNDEF_DOUBLE,
            cos_theta: UNDEF_DOUBLE,
        }
    }

    /// The conic kind.
    #[must_use]
    pub const fn conic_type(&self) -> ConicType {
        self.kind
    }

    /// Constant `a` of the conic equation.
    #[must_use]
    pub const fn a(&self) -> f64 {
        self.a
    }

    /// Constant `b` of the conic equation.
    #[must_use]
    pub const fn b(&self) -> f64 {
        self.b
    }

    /// Origin of the displaced conic.
    #[must_use]
    pub const fn origin(&self) -> Point2 {
        self.origin
    }

    /// Rotation angle (radians) of the displaced conic.
    #[must_use]
    pub const fn theta_rad(&self) -> f64 {
        self.theta_rad
    }

    /// Cached sine and cosine of the rotation angle.
    #[must_use]
    pub const fn sin_cos_theta(&self) -> (f64, f64) {
        (self.sin_theta, self.cos_theta)
    }

    /// Image-space dimension (`GetDim`): always 2.
    #[must_use]
    pub const fn dim(&self) -> u64 {
        2
    }

    /// Upstream's tolerant equality (`operator==`): the same object, or the same
    /// kind with `a`, `b` and the rotation angle equal under `SM_IS_ZERO` and equal
    /// origins.
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        // low work
        if core::ptr::eq(self, other) {
            return true;
        }

        // first check the base (SmCurve::operator==: same type, same dimension 2)

        // check equivalence of these objects
        self.kind == other.kind
            && is_zero(self.a - other.a)
            && is_zero(self.b - other.b)
            && self.origin.approx_eq(other.origin)
            && is_zero(self.theta_rad - other.theta_rad)
    }

    /// Minimum continuity of the curve and the continuity at each of its two
    /// unique knots (`CalculateContinuities`): `CInfinity` and `[Discontinuous;
    /// 2]`.
    #[must_use]
    pub fn calculate_continuities(&self) -> (ContinuityType, Vec<ContinuityType>) {
        (
            ContinuityType::CInfinity,
            vec![ContinuityType::Discontinuous, ContinuityType::Discontinuous],
        )
    }

    /// The natural interval (`GetNaturalInterval`): `[-999, 999]` for parabolas and
    /// lines, `[-1, 3]` for ellipses (`(1, 3]` is the left half), and
    /// `[-1 + EFF_ZERO_SQRT, 1 - EFF_ZERO_SQRT]` otherwise.
    #[must_use]
    pub fn natural_interval(&self) -> Extent1d {
        if self.kind == ConicType::Parabola || self.kind == ConicType::Line {
            Extent1d::from_min_max(-999.0, 999.0)
        } else if self.kind == ConicType::Ellipse {
            Extent1d::from_min_max(-1.0, 3.0)
        } else {
            Extent1d::from_min_max(-1.0 + EFF_ZERO_SQRT, 1.0 - EFF_ZERO_SQRT)
        }
    }

    /// The unique knots of the equivalent cubic knot vector and their
    /// multiplicities (`GetKnots`): the natural interval ends, each of multiplicity
    /// 3, keeping only those inside `interval` when one is given.
    #[must_use]
    pub fn knots(&self, interval: Option<&Extent1d>) -> (Vec<f64>, Vec<u64>) {
        let mut knots = Vec::new();
        let mut mults = Vec::new();
        let ivl = self.natural_interval();

        // interval start
        if interval.is_none_or(|i| i.contains_value(ivl.min(), 0.0)) {
            knots.push(ivl.min());
            mults.push(3);
        }

        // interval end
        if interval.is_none_or(|i| i.contains_value(ivl.max(), 0.0)) {
            knots.push(ivl.max());
            mults.push(3);
        }
        (knots, mults)
    }

    /// Position and the first `num_derivs` derivatives at `param` (`Evaluate`), with
    /// default pinned SMLib behaviour ([`UpstreamFixes::default`]).
    ///
    /// With `non_zero_tangents` and two derivatives, upstream's zero-tangent fix
    /// reads the not yet written second-derivative slot (`WR-27`; see
    /// [`Self::evaluate_with_fixes`] for the opt-in fix).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `num_derivs > 2` (upstream also fills the first three
    /// vectors).
    pub fn evaluate(
        &self,
        param: f64,
        num_derivs: usize,
        non_zero_tangents: bool,
    ) -> Result<Vec<Vec3>> {
        self.evaluate_with_fixes(
            param,
            num_derivs,
            non_zero_tangents,
            &UpstreamFixes::default(),
        )
    }

    /// Position and the first `num_derivs` derivatives at `param` (`Evaluate`),
    /// honouring [`UpstreamFixes::conic_evaluate_uses_computed_second_deriv`] (`WR-27`).
    ///
    /// When `fixes.conic_evaluate_uses_computed_second_deriv` is `false` (default),
    /// the `non_zero_tangents` fallback for `num_derivs == 2` reads the unwritten
    /// slot `out[2]` ([`Vec3::UNDEF`]), matching pinned v1.0.1 C++ on a fresh buffer.
    /// When `true`, it uses the computed second derivative `sec` to orient and scale
    /// a near-zero first derivative (`|tan| < 1.1 * scaled_zero`) whenever
    /// `|sec| > 1.1 * scaled_zero` (negating above the midpoint of
    /// [`Self::natural_interval`]).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `num_derivs > 2` (upstream also fills the first three
    /// vectors).
    pub fn evaluate_with_fixes(
        &self,
        param: f64,
        num_derivs: usize,
        non_zero_tangents: bool,
        fixes: &UpstreamFixes,
    ) -> Result<Vec<Vec3>> {
        // At most three slots: `num_derivs > 2` is `SM_ERR` below, so a huge caller
        // order must not size the allocation.
        let mut out = vec![Vec3::UNDEF; num_derivs.min(2) + 1];
        let pos = self.evaluate_point(param);
        if let Some(o) = out.get_mut(0) {
            *o = pos;
        }
        let mut t_sq = param * param;
        let mut t = param;

        if num_derivs == 0 {
            return Ok(out);
        }

        let mut tan = Vec3::new(0.0, 0.0, 0.0);
        let mut sec = Vec3::new(0.0, 0.0, 0.0);

        match self.kind {
            ConicType::Ellipse => {
                let mut sign = 1.0;
                if param > 1.0 {
                    t = param - 2.0;
                    t_sq = t * t;
                    sign = -1.0;
                }
                let denom = 1.0 + 2.0 * t_sq + t_sq * t_sq;
                tan.x = sign * self.a * (-4.0 * t) / denom;
                tan.y = sign * self.b * 2.0 * (1.0 - t_sq) / denom;
                let denom_sq = denom * denom;
                let denom_prim = 4.0 * t + 4.0 * t * t_sq;
                if num_derivs > 1 {
                    sec.x = sign * self.a * (-4.0 * denom + 4.0 * t * denom_prim) / denom_sq;
                    sec.y = sign * self.b * (-4.0 * t * denom - (2.0 - 2.0 * t_sq) * denom_prim)
                        / denom_sq;
                }
            }
            ConicType::Hyperbola => {
                let denom = 1.0 - 2.0 * t_sq + t_sq * t_sq;
                tan.x = self.a * (4.0 * t) / denom;
                tan.y = self.b * 2.0 * (1.0 + t_sq) / denom;
                let denom_sq = denom * denom;
                let denom_prim = -4.0 * t + 4.0 * t * t_sq;
                if num_derivs > 1 {
                    sec.x = self.a * (4.0 * denom - 4.0 * t * denom_prim) / denom_sq;
                    sec.y = self.b * (4.0 * t * denom - (2.0 + 2.0 * t_sq) * denom_prim) / denom_sq;
                }
            }
            ConicType::Parabola => {
                tan.x = 2.0 * self.a * t;
                tan.y = 2.0 * self.a;
                if num_derivs > 1 {
                    sec.x = 2.0 * self.a;
                    sec.y = 0.0;
                }
            }
            ConicType::Line => {
                tan.x = 1.0;
                tan.y = 0.0;
                if num_derivs > 1 {
                    sec.x = 0.0;
                    sec.y = 0.0;
                }
            }
            ConicType::Unknown => {}
        }

        if self.theta_rad != 0.0 {
            tan = Vec3::new(
                tan.x * self.cos_theta + tan.y * self.sin_theta,
                tan.y * self.cos_theta - tan.x * self.sin_theta,
                0.0,
            );
            if num_derivs > 1 {
                sec = Vec3::new(
                    sec.x * self.cos_theta + sec.y * self.sin_theta,
                    sec.y * self.cos_theta - sec.x * self.sin_theta,
                    0.0,
                );
            }
        }

        if let Some(o) = out.get_mut(1) {
            *o = tan;
        }

        if num_derivs == 1 {
            return Ok(out);
        }

        // When asked, try to find a direction for zero-length first derivatives
        // based on the 2nd derivative value
        // (This is the same trick as is used for surface evaluations.)
        if non_zero_tangents {
            let scaled_zero = EFF_ZERO * (1.0 + pos.max_dimension());
            let new_len = 1.1 * scaled_zero;

            // see if the 1st order derivative is smaller than new_len (this gives a
            // continuous modified function)
            if tan.length_squared() < new_len * new_len {
                // num_derivs >= 2 here: upstream's recursion for fewer derivatives
                // cannot be reached. Fix the zero 1st derivative function here; by
                // default (WR-27) the 2nd derivative slot still holds the caller's
                // fresh value (`Vec3::UNDEF`), whereas the opt-in fix uses `sec`.
                let second = if fixes.conic_evaluate_uses_computed_second_deriv {
                    sec
                } else {
                    out.get(2).copied().unwrap_or(Vec3::UNDEF)
                };
                let len = second.length();
                if len > new_len {
                    let mut fixed = (new_len / len) * second;

                    // If we're at the 'top' of the domain, i.e., the 'good'
                    // parameter is entering a singularity instead of leaving it,
                    // then the degenerate derivative is shrinking, so its change
                    // (the 2nd derivative) is in the opposite direction of the
                    // derivative.
                    if param > self.natural_interval().mid() {
                        fixed *= -1.0;
                    }
                    if let Some(o) = out.get_mut(1) {
                        *o = fixed;
                    }
                }
            }
        }

        if let Some(o) = out.get_mut(2) {
            *o = sec;
        }

        if num_derivs == 2 {
            return Ok(out);
        }

        Err(SolidError::sm_err(OP_EVALUATE))
    }

    /// The point at `param` (`EvaluatePoint`); `z` is 0.
    #[must_use]
    pub fn evaluate_point(&self, param: f64) -> Point3 {
        let mut p = Vec3::new(UNDEF_DOUBLE, UNDEF_DOUBLE, 0.0);
        let mut t_sq = param * param;
        let mut t = param;

        match self.kind {
            ConicType::Ellipse => {
                let mut sign = 1.0;
                if param > 1.0 {
                    t = param - 2.0;
                    t_sq = t * t;
                    sign = -1.0;
                }
                let denom = 1.0 + t_sq;
                p.x = sign * self.a * (1.0 - t_sq) / denom;
                p.y = sign * self.b * (2.0 * t) / denom;
            }
            ConicType::Hyperbola => {
                let denom = 1.0 - t_sq;
                p.x = self.a * (1.0 + t_sq) / denom;
                p.y = self.b * (2.0 * t) / denom;
            }
            ConicType::Parabola => {
                p.x = self.a * t_sq;
                p.y = 2.0 * self.a * t;
            }
            ConicType::Line => {
                p.x = t;
                p.y = self.a;
            }
            ConicType::Unknown => {}
        }

        if self.theta_rad != 0.0 {
            p = Vec3::new(
                p.x * self.cos_theta + p.y * self.sin_theta,
                p.y * self.cos_theta - p.x * self.sin_theta,
                0.0,
            );
        }
        if self.origin.x != 0.0 || self.origin.y != 0.0 {
            p += Vec3::from(self.origin);
        }
        p
    }

    /// The implicit equation `A x² + B xy + C y² + D x + E y + F = 0` of the
    /// displaced conic, as `[A, B, C, D, E, F]` (`ComputeImplicitEquation`).
    #[must_use]
    pub fn compute_implicit_equation(&self) -> [f64; 6] {
        let (mut a, mut b, mut c, mut d, mut e, mut f) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

        match self.kind {
            ConicType::Ellipse => {
                a = 1.0 / (self.a * self.a);
                b = 0.0;
                c = 1.0 / (self.b * self.b);
                d = 0.0;
                e = 0.0;
                f = -1.0;
            }
            ConicType::Hyperbola => {
                a = 1.0 / (self.a * self.a);
                b = 0.0;
                c = -1.0 / (self.b * self.b);
                d = 0.0;
                e = 0.0;
                f = -1.0;
            }
            ConicType::Parabola => {
                a = 0.0;
                b = 0.0;
                c = 1.0;
                d = -4.0 * self.a;
                e = 0.0;
                f = 0.0;
            }
            ConicType::Line => {
                a = 0.0;
                b = 0.0;
                c = 0.0;
                d = 0.0;
                e = -1.0 / self.a;
                f = 1.0;
            }
            ConicType::Unknown => {}
        }

        // Now do rotation - plug counter clockwise rotation into
        // implicit equation.
        //
        // x' = x*Cos + y*Sin
        // y' = -x*Sin + Y*Cos
        //   2           2
        // Ax  + Bxy + Cy  + Dx + Ey + F = 0
        let sin = (-self.theta_rad).sin();
        let cos = (-self.theta_rad).cos();
        let sin_sq = sin * sin;
        let cos_sq = cos * cos;
        let sin_cos = sin * cos;

        let a2 = a * cos_sq - b * sin_cos + c * sin_sq;
        let b2 = 2.0 * a * sin_cos + b * cos_sq - b * sin_sq - 2.0 * c * sin_cos;
        let c2 = a * sin_sq + b * sin_cos + c * cos_sq;
        let d2 = d * cos - e * sin;
        let e2 = d * sin + e * cos;
        let f2 = f;

        // Here is where we do tranlation.
        let c1 = -self.origin.x;
        let cc2 = -self.origin.y;

        [
            a2,
            b2,
            c2,
            2.0 * a2 * c1 + b2 * cc2 + d2,
            b2 * c1 + 2.0 * c2 * cc2 + e2,
            a2 * c1 * c1 + b2 * c1 * cc2 + c2 * cc2 * cc2 + d2 * c1 + e2 * cc2 + f2,
        ]
    }

    /// Parameters of this conic (taken in natural position: rotation and origin
    /// are ignored) where it meets the implicit conic `coeff` (`[A, B, C, D, E,
    /// F]`, see [`Conic::compute_implicit_equation`]) (`IntersectImplicitConic`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an unknown conic (upstream solves a quartic
    /// of uninitialised coefficients).
    pub fn intersect_implicit_conic(&self, coeff: &[f64; 6]) -> Result<Vec<f64>> {
        let [ca, cb, cc, cd, ce, cf] = *coeff; // x^2, xy, y^2, x, y, constant
        if self.kind == ConicType::Unknown {
            return Err(SolidError::InvalidInput {
                operation: OP_INTERSECT_IMPLICIT,
                reason: "unknown conic type",
            });
        }
        let mut results = Vec::new();

        // Run through two times.  First for the right hand side and then
        // for the left hand side of the Ellipse.
        for side in 0..=1u32 {
            // Only do it two times for ellipse.
            if side == 1 && self.kind != ConicType::Ellipse {
                break;
            }
            // Plug the Implicit equ. into the Parametric and solve for
            // values of T.
            let sign = if side == 1 { -1.0 } else { 1.0 };
            let a = self.a * sign;
            let a_sq = a * a;
            let b = self.b * sign;
            let b_sq = b * b;
            let q: [f64; 5] = match self.kind {
                ConicType::Ellipse => [
                    ca * a_sq + cd * a + cf,
                    2.0 * (cb * a * b + ce * b),
                    2.0 * (-ca * a_sq + 2.0 * cc * b_sq + cf),
                    2.0 * (-cb * a * b + ce * b),
                    ca * a_sq - cd * a + cf,
                ],
                ConicType::Hyperbola => [
                    ca * a_sq + cd * a + cf,
                    2.0 * (cb * a * b + ce * b),
                    2.0 * (ca * a_sq + 2.0 * cc * b_sq - cf),
                    2.0 * (cb * a * b - ce * b),
                    ca * a_sq - cd * a + cf,
                ],
                ConicType::Parabola => [
                    ca * a_sq,
                    2.0 * (cb * a_sq),
                    4.0 * cc * a_sq + cd * a,
                    2.0 * ce * a,
                    cf,
                ],
                ConicType::Line | ConicType::Unknown => {
                    [cc * a_sq + ce * a + cf, cb * a + cd, ca, 0.0, 0.0]
                }
            };
            let sols = solve_quartic(&q, EFF_ZERO);
            for &sol in sols.iter() {
                let mut s = sol;
                if self.kind == ConicType::Ellipse {
                    if s.abs() <= 1.0 + EFF_ZERO {
                        if sign < 0.0 {
                            if s.abs() < 1.0 {
                                s += 2.0;
                                results.push(s);
                            }
                        } else {
                            results.push(s);
                        }
                    }
                } else if self.kind == ConicType::Hyperbola {
                    // Make case where hyperbola's intersect at their axes
                    // at infinity work.
                    let crv_ivl = self.natural_interval();
                    if s.abs() < 1.0 + EFF_ZERO_SQRT {
                        s = crv_ivl.clamp_value(s);
                    }
                    if crv_ivl.contains_value(s, 0.0) {
                        results.push(s);
                    }
                } else {
                    let crv_ivl = self.natural_interval();
                    if crv_ivl.contains_value(s, 0.0) {
                        results.push(s);
                    }
                }
            }
        } // For each side of the ellipse.

        Ok(results)
    }

    /// Intersections of this conic with `other` by direct analytic methods
    /// (`IntersectConic`): the other conic is moved into this conic's natural
    /// frame, intersected implicitly, and each point is inverted on `other`;
    /// points farther than `distance_tol` from their inversion are dropped. Only
    /// actual intersections are found, not near misses.
    ///
    /// # Errors
    ///
    /// Those of [`Conic::intersect_implicit_conic`].
    pub fn intersect_conic(
        &self,
        other: &Self,
        distance_tol: f64,
    ) -> Result<Vec<ConicIntersection>> {
        let mut solutions = Vec::new();
        let mut tmp = Self::new(other.kind, other.a, other.b);
        tmp.transform_2d(-other.theta_rad, other.origin);
        tmp.transform_2d(0.0, -self.origin);
        tmp.transform_2d(self.theta_rad, Vec2::new(0.0, 0.0));

        let coeff = tmp.compute_implicit_equation();
        let tsect_params = self.intersect_implicit_conic(&coeff)?;
        // Need to invert points and then determine multiplicities by
        // comparing tangent vectors.
        for &param in &tsect_params {
            let pnt = self.evaluate_point(param);
            let Some(param2) = other.invert_point_on_conic(pnt) else {
                continue;
            };
            let pnt2 = other.evaluate_point(param2);
            let dist = pnt2.distance(pnt);
            if dist > distance_tol {
                continue;
            }
            solutions.push(ConicIntersection {
                param,
                other_param: param2,
                distance: dist,
            });
        }

        Ok(solutions)
    }

    /// The parameter of `point`, a point known to lie on the conic
    /// (`InvertPointOnConic`); `None` when no parameter evaluates within
    /// `EFF_ZERO_SQRT * (1 + max |coordinate|)` of the point.
    #[must_use]
    pub fn invert_point_on_conic(&self, point: Point3) -> Option<f64> {
        // First move the point into the local parameter space of the
        // conic.
        let mut param = 0.0;
        let mut param2 = 0.0;

        let tmp = point - Vec3::from(self.origin);
        let cos = (-self.theta_rad).cos();
        let sin = (-self.theta_rad).sin();
        let local = Vec2::new(tmp.x * cos + tmp.y * sin, tmp.y * cos - tmp.x * sin);

        match self.kind {
            ConicType::Hyperbola => {
                let q = [-local.y, 2.0 * self.b, local.y];
                let sol = solve_quadratic(&q, EFF_ZERO);
                let sol = sol.as_slice();
                let (Some(&s0), s1) = (sol.first(), sol.get(1).copied()) else {
                    return None;
                };
                let ivl = self.natural_interval();
                if s0.abs() <= 1.0 + 0.0001 {
                    param = ivl.clamp_value(s0);
                    if let Some(s1) = s1 {
                        if s1.abs() <= 1.0 + 0.0001 {
                            param2 = ivl.clamp_value(s1);
                        }
                    }
                } else {
                    let s1 = s1.filter(|s| s.abs() <= 1.0 + 0.0001)?;
                    param = ivl.clamp_value(s1);
                }
            }
            ConicType::Ellipse => {
                let q = [local.y, -2.0 * self.b, local.y];
                let sol = solve_quadratic(&q, EFF_ZERO);
                let sol = sol.as_slice();
                let (Some(&s0), s1) = (sol.first(), sol.get(1).copied()) else {
                    return None;
                };
                if s0.abs() <= 1.0 + EFF_ZERO {
                    param = s0;
                } else {
                    param = s1.filter(|s| s.abs() <= 1.0 + EFF_ZERO)?;
                }
                // If point is on left side flip the parameter.
                if local.x < 0.0 {
                    param = 2.0 - param;
                }
            }
            ConicType::Parabola => {
                param = local.y / (2.0 * self.a);
            }
            ConicType::Line => {
                param = local.x;
            }
            ConicType::Unknown => {}
        }

        let mut test_pnt = self.evaluate_point(param);
        let mut diff = test_pnt.distance(point);

        let scale = 1.0 + test_pnt.max_dimension();
        if diff > EFF_ZERO_SQRT * scale {
            if self.kind == ConicType::Hyperbola {
                param = param2;
                test_pnt = self.evaluate_point(param);
                diff = test_pnt.distance(point);
                if diff > EFF_ZERO_SQRT * scale {
                    return None;
                }
            } else {
                return None;
            }
        }
        Some(param)
    }

    /// Rotates the conic by `ccw_rotation_rad` (counter-clockwise) and then
    /// translates it by `translation` (`Transform2D`).
    pub fn transform_2d(&mut self, ccw_rotation_rad: f64, translation: Point2) {
        self.theta_rad -= ccw_rotation_rad;
        let cos = (-ccw_rotation_rad).cos();
        let sin = (-ccw_rotation_rad).sin();
        let tmp = Vec2::new(
            self.origin.x * cos + self.origin.y * sin,
            self.origin.y * cos - self.origin.x * sin,
        );
        self.origin = tmp + translation;
        self.cos_theta = self.theta_rad.cos();
        self.sin_theta = self.theta_rad.sin();
    }

    /// Reads a conic written by `WriteToDB` (`ReadFromDB`): the type code, `a`,
    /// `b`, the origin and the rotation angle (one line each in ASCII files, the
    /// origin's two values on one line).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the ASCII type code cannot be extracted at
    /// all; a failed binary read.
    pub fn read_from_db(db: &mut DatabaseReader) -> Result<Self> {
        // init output object
        let mut conic = Self::empty();

        // locals
        let conic_type: Option<u64>;
        let (mut a, mut b, mut orig_x, mut orig_y, mut theta_rad) = (0.0, 0.0, 0.0, 0.0, 0.0);

        if db.file_type() == FileType::Ascii {
            conic_type = db.in_stream_mut().and_then(InputStream::extract_ulong);
            db.go_to_next_line();
            for vals in [
                &mut [&mut a][..],
                &mut [&mut b][..],
                &mut [&mut orig_x, &mut orig_y][..],
                &mut [&mut theta_rad][..],
            ] {
                if let Some(s) = db.in_stream_mut() {
                    for v in vals.iter_mut() {
                        if let Some(x) = s.extract_double() {
                            **v = x;
                        }
                    }
                }
                db.go_to_next_line();
            }
        } else {
            conic_type = Some(db.read_ulong()?);
            a = db.read_double()?;
            b = db.read_double()?;
            orig_x = db.read_double()?;
            orig_y = db.read_double()?;
            theta_rad = db.read_double()?;
        }
        let Some(code) = conic_type else {
            return Err(SolidError::InvalidInput {
                operation: OP_READ,
                reason: "conic type not read",
            });
        };

        // load obj
        conic.kind = ConicType::from_code(i64::try_from(code).unwrap_or(i64::MAX));
        conic.a = a;
        conic.b = b;
        conic.origin = Vec2::new(orig_x, orig_y);
        conic.theta_rad = theta_rad;
        conic.sin_theta = theta_rad.sin();
        conic.cos_theta = theta_rad.cos();
        Ok(conic)
    }

    /// `SmConic_TYPE` (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        Self::TYPE
    }

    /// `"SmConic_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        "SmConic_TYPE"
    }

    /// `"SmConic"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        "SmConic"
    }

    /// True for `SmConic_TYPE` and its base types (`IsKindOf`).
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        t == Self::TYPE || curve_p2::is_kind_of(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::{Curve, CurveEvaluate};
    use crate::geom::tol::ZONE_TOL_3D;
    use crate::kernel::Kernel;
    use crate::math::PI;

    #[test]
    fn natural_shapes() {
        let e = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        assert_eq!(e.evaluate_point(0.0), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(e.evaluate_point(1.0), Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(e.evaluate_point(2.0), Vec3::new(-2.0, 0.0, 0.0));
        let h = Conic::new(ConicType::Hyperbola, 2.0, 1.0);
        assert_eq!(h.evaluate_point(0.0), Vec3::new(2.0, 0.0, 0.0));
        let p = Conic::new(ConicType::Parabola, 1.0, 0.0);
        assert_eq!(p.evaluate_point(2.0), Vec3::new(4.0, 4.0, 0.0));
        let l = Conic::new(ConicType::Line, 0.5, 0.0);
        assert_eq!(l.evaluate_point(3.0), Vec3::new(3.0, 0.5, 0.0));
        assert_eq!(e.dim(), 2);
    }

    #[test]
    fn evaluate_derivatives_and_errors() {
        let e = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        let v = e.evaluate(0.0, 2, true).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[1], Vec3::new(0.0, 2.0, 0.0));
        assert!(e.evaluate(0.0, 3, false).is_err());
        let p = e.evaluate(0.3, 0, false).unwrap();
        assert_eq!(p[0], e.evaluate_point(0.3));
    }

    #[test]
    fn implicit_equation_vanishes_on_curve() {
        let mut c = Conic::new(ConicType::Hyperbola, 1.5, 0.7);
        c.transform_2d(0.4, Vec2::new(0.3, -0.2));
        let k = c.compute_implicit_equation();
        for t in [-0.5, 0.0, 0.3, 0.8] {
            let p = c.evaluate_point(t);
            let f = k[0] * p.x * p.x
                + k[1] * p.x * p.y
                + k[2] * p.y * p.y
                + k[3] * p.x
                + k[4] * p.y
                + k[5];
            assert!(f.abs() < 1e-9, "{t}: {f}");
        }
    }

    #[test]
    fn invert_round_trip() {
        let mut c = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        c.transform_2d(30.0 * PI / 180.0, Vec2::new(-0.3, -0.4));
        for t in [-0.7, 0.2, 0.9, 1.5, 2.5] {
            let p = c.evaluate_point(t);
            let u = c.invert_point_on_conic(p).unwrap();
            assert!((u - t).abs() < 1e-9, "{t} -> {u}");
        }
        assert!(c
            .invert_point_on_conic(Vec3::new(10.0, 10.0, 0.0))
            .is_none());
    }

    #[test]
    fn upstream_ellipse_pair_has_four_intersections() {
        // tests/prog_test/src/smCurveTest.cpp: ellipse (2,1) vs ellipse (1,2)
        let a = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        let b = Conic::new(ConicType::Ellipse, 1.0, 2.0);
        let s = a.intersect_conic(&b, ZONE_TOL_3D).unwrap();
        assert_eq!(s.len(), 4);
        let unknown = Conic::empty();
        assert!(unknown.intersect_conic(&b, ZONE_TOL_3D).is_err());
    }

    #[test]
    fn knots_kind_eq() {
        let c = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        assert_eq!(c.knots(None), (vec![-1.0, 3.0], vec![3, 3]));
        let ivl = Extent1d::from_min_max(0.0, 5.0);
        assert_eq!(c.knots(Some(&ivl)), (vec![3.0], vec![3]));
        assert!(c.is_kind_of(Conic::TYPE));
        assert!(c.is_kind_of(14001));
        assert!(!c.is_kind_of(1));
        let mut d = c;
        assert!(c.approx_eq(&d));
        d.transform_2d(0.1, Vec2::new(0.0, 0.0));
        assert!(!c.approx_eq(&d));
    }

    #[test]
    fn read_ascii_and_binary() {
        let text = "1 SmConic conic type\n2.5 a\n0.5 b\n1 2 origin\n0.25 theta\n";
        let mut db = DatabaseReader::new();
        db.open_for_read(text.as_bytes().to_vec(), FileType::Ascii);
        let c = Conic::read_from_db(&mut db).unwrap();
        assert_eq!(c.conic_type(), ConicType::Hyperbola);
        assert_eq!(c.origin(), Vec2::new(1.0, 2.0));
        assert_eq!(c.sin_cos_theta(), (0.25f64.sin(), 0.25f64.cos()));

        let mut bytes = 3u64.to_ne_bytes().to_vec();
        for v in [0.5, 0.0, 0.0, 0.0, 0.0f64] {
            bytes.extend(v.to_ne_bytes());
        }
        let mut db = DatabaseReader::new();
        db.open_for_read(bytes, FileType::Binary);
        let c = Conic::read_from_db(&mut db).unwrap();
        assert_eq!(c.conic_type(), ConicType::Line);
        assert_eq!(c.a(), 0.5);
    }

    #[test]
    fn wr27_flag_lookup_and_toggle() {
        let name = "conic_evaluate_uses_computed_second_deriv";
        assert!(UpstreamFixes::NAMES.contains(&name));

        let mut k = Kernel::default();
        assert!(!k.fixes.conic_evaluate_uses_computed_second_deriv);
        assert_eq!(k.fixes.flag_mut(name).copied(), Some(false));

        *k.fixes.flag_mut(name).unwrap() = true;
        assert!(k.fixes.conic_evaluate_uses_computed_second_deriv);
        assert_eq!(k.fixes.flag_mut(name).copied(), Some(true));

        *k.fixes.flag_mut(name).unwrap() = false;
        assert_eq!(k.fixes, UpstreamFixes::default());
    }

    #[test]
    fn wr27_zero_and_tiny_tangent_ellipse_uses_computed_second_deriv() {
        let fixes_off = UpstreamFixes::default();
        let fixes_on = UpstreamFixes {
            conic_evaluate_uses_computed_second_deriv: true,
            ..Default::default()
        };

        // Flat horizontal ellipse (a = 2, b = 0): natural interval [-1, 3], mid = 1.
        // At param = 0 (< mid): pos = (2, 0, 0), tan = (0, 0, 0), sec = (-8, 0, 0).
        let flat = Conic::new(ConicType::Ellipse, 2.0, 0.0);
        let new_len = 1.1 * (EFF_ZERO * (1.0 + 2.0));

        let v_def = flat.evaluate_with_fixes(0.0, 2, true, &fixes_off).unwrap();
        assert_eq!(v_def, flat.evaluate(0.0, 2, true).unwrap());
        assert_eq!(v_def[0], Vec3::new(2.0, 0.0, 0.0));
        // Default reads out[2] = Vec3::UNDEF (length ~2.14e-61 <= new_len), leaving tan zero.
        assert_eq!(v_def[1], Vec3::ZERO);
        assert_eq!(v_def[2], Vec3::new(-8.0, 0.0, 0.0));

        let v_fix = flat.evaluate_with_fixes(0.0, 2, true, &fixes_on).unwrap();
        let expected_0 = (new_len / v_fix[2].length()) * v_fix[2];
        assert_eq!(v_fix[0], Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(v_fix[1], expected_0);
        assert!(v_fix[1].x < 0.0 && v_fix[1].y == 0.0 && v_fix[1].z == 0.0);
        assert!((v_fix[1].length() - new_len).abs() <= 1.0e-27);
        assert_eq!(v_fix[2], Vec3::new(-8.0, 0.0, 0.0));

        // At param = 2 (> mid = 1): pos = (-2, 0, 0), tan = 0, sec = (+8, 0, 0);
        // entering upper domain half negates the fallback tangent so it points along -X.
        let v_def_top = flat.evaluate_with_fixes(2.0, 2, true, &fixes_off).unwrap();
        assert_eq!(v_def_top[1], Vec3::ZERO);
        assert_eq!(v_def_top[2], Vec3::new(8.0, 0.0, 0.0));

        let v_fix_top = flat.evaluate_with_fixes(2.0, 2, true, &fixes_on).unwrap();
        let mut expected_2 = (new_len / v_fix_top[2].length()) * v_fix_top[2];
        expected_2 *= -1.0;
        assert_eq!(v_fix_top[0], Vec3::new(-2.0, 0.0, 0.0));
        assert_eq!(v_fix_top[1], expected_2);
        assert!(v_fix_top[1].x < 0.0);
        assert_eq!(v_fix_top[2], Vec3::new(8.0, 0.0, 0.0));

        // Tiny non-zero tangent (|tan| = 2e-15 < new_len = 3.3e-12) also triggers the fallback.
        let tiny = Conic::new(ConicType::Ellipse, 2.0, 1.0e-15);
        let t_def = tiny.evaluate_with_fixes(0.0, 2, true, &fixes_off).unwrap();
        assert_eq!(t_def[1], Vec3::new(0.0, 2.0e-15, 0.0));
        assert!(t_def[1].length() < new_len);

        let t_fix = tiny.evaluate_with_fixes(0.0, 2, true, &fixes_on).unwrap();
        assert_eq!(t_fix[1], expected_0);
        // With non_zero_tangents = false, exact tangent is preserved even under opt-in.
        let t_exact = tiny.evaluate_with_fixes(0.0, 2, false, &fixes_on).unwrap();
        assert_eq!(t_exact[1], Vec3::new(0.0, 2.0e-15, 0.0));

        // Vertical degenerate ellipse (a = 0, b = 3) at -1 (< mid), 1 (== mid), and 3 (> mid).
        let vert = Conic::new(ConicType::Ellipse, 0.0, 3.0);
        let v_len = 1.1 * (EFF_ZERO * (1.0 + 3.0));
        let vm1 = vert.evaluate_with_fixes(-1.0, 2, true, &fixes_on).unwrap();
        assert!(vm1[1].y > 0.0 && (vm1[1].length() - v_len).abs() <= 1.0e-27);
        let vp1 = vert.evaluate_with_fixes(1.0, 2, true, &fixes_on).unwrap();
        assert!(vp1[1].y < 0.0 && (vp1[1].length() - v_len).abs() <= 1.0e-27);
        let vp3 = vert.evaluate_with_fixes(3.0, 2, true, &fixes_on).unwrap();
        assert!(vp3[1].y < 0.0 && (vp3[1].length() - v_len).abs() <= 1.0e-27);
    }

    #[test]
    fn wr27_hyperbola_transformed_conic_and_curve_dispatch() {
        let k_def = Kernel::default();
        let mut k_fix = Kernel::default();
        k_fix.fixes.conic_evaluate_uses_computed_second_deriv = true;

        // Degenerate hyperbola (a = 2, b = 0), natural interval mid = 0.
        let hyp = Conic::new(ConicType::Hyperbola, 2.0, 0.0);
        let h_len = 1.1 * (EFF_ZERO * (1.0 + 2.0));
        let h0_def = hyp.evaluate_with_fixes(0.0, 2, true, &k_def.fixes).unwrap();
        assert_eq!(h0_def[1], Vec3::ZERO);
        let h0_fix = hyp.evaluate_with_fixes(0.0, 2, true, &k_fix.fixes).unwrap();
        assert!(h0_fix[1].x > 0.0 && (h0_fix[1].length() - h_len).abs() <= 1.0e-27);

        // Above mid (param = 1e-15 > 0): |tan| = 8e-15 < 3.3e-12, negated to -X.
        let hp_fix = hyp
            .evaluate_with_fixes(1.0e-15, 2, true, &k_fix.fixes)
            .unwrap();
        assert!(hp_fix[1].x < 0.0);

        // Rotated & translated degenerate ellipse dispatched via Curve and CurveEvaluate.
        let mut rot = Conic::new(ConicType::Ellipse, 2.0, 0.0);
        rot.transform_2d(PI / 3.0, Vec2::new(10.0, -4.0));
        let crv = Curve::from(rot);
        let pos = rot.evaluate_point(0.0);
        let r_len = 1.1 * (EFF_ZERO * (1.0 + pos.max_dimension()));

        let c_def = crv.evaluate(&k_def, 0.0, 2, false, true).unwrap();
        assert_eq!(c_def[1], Vec3::ZERO);

        let c_fix = crv.evaluate(&k_fix, 0.0, 2, false, true).unwrap();
        let direct_fix = rot.evaluate_with_fixes(0.0, 2, true, &k_fix.fixes).unwrap();
        let trait_fix = CurveEvaluate::evaluate(&rot, &k_fix, 0.0, 2, true, true).unwrap();
        assert_eq!(c_fix, direct_fix);
        assert_eq!(trait_fix, direct_fix);
        assert!((c_fix[1].length() - r_len).abs() <= 1.0e-25);
        assert!(c_fix[1].dot(c_fix[2]) > 0.0);

        let mut buf = [Vec3::new(99.0, -77.0, 55.0); 3];
        crv.evaluate_into(&k_def, 0.0, false, true, &mut buf)
            .unwrap();
        assert_eq!(buf.as_slice(), c_def.as_slice());

        buf = [Vec3::new(99.0, -77.0, 55.0); 3];
        crv.evaluate_into(&k_fix, 0.0, false, true, &mut buf)
            .unwrap();
        assert_eq!(buf.as_slice(), c_fix.as_slice());

        // Totally collapsed conic (sec == 0): len > new_len is false in both modes (no NaN).
        let zero_conic = Conic::new(ConicType::Ellipse, 0.0, 0.0);
        assert_eq!(
            zero_conic
                .evaluate_with_fixes(0.0, 2, true, &k_def.fixes)
                .unwrap(),
            zero_conic
                .evaluate_with_fixes(0.0, 2, true, &k_fix.fixes)
                .unwrap()
        );
    }

    #[test]
    fn wr27_non_degenerate_and_other_deriv_orders_identical() {
        let fixes_off = UpstreamFixes::default();
        let fixes_on = UpstreamFixes {
            conic_evaluate_uses_computed_second_deriv: true,
            ..Default::default()
        };

        let mut ellipse = Conic::new(ConicType::Ellipse, 2.0, 1.0);
        ellipse.transform_2d(0.35, Vec2::new(1.5, -0.5));
        let mut hyperbola = Conic::new(ConicType::Hyperbola, 1.5, 0.7);
        hyperbola.transform_2d(-0.2, Vec2::new(-0.4, 2.1));
        let mut parabola = Conic::new(ConicType::Parabola, 1.2, 0.0);
        parabola.transform_2d(0.6, Vec2::new(0.2, 0.8));
        let mut line = Conic::new(ConicType::Line, 0.5, 0.0);
        line.transform_2d(-0.9, Vec2::new(-3.0, 1.0));

        for conic in [ellipse, hyperbola, parabola, line] {
            for param in [-0.6, 0.0, 0.4, 0.85] {
                for nzt in [false, true] {
                    for d in [0, 1, 2] {
                        assert_eq!(
                            conic
                                .evaluate_with_fixes(param, d, nzt, &fixes_off)
                                .unwrap(),
                            conic.evaluate_with_fixes(param, d, nzt, &fixes_on).unwrap(),
                        );
                    }
                    assert!(conic
                        .evaluate_with_fixes(param, 3, nzt, &fixes_off)
                        .is_err());
                    assert!(conic.evaluate_with_fixes(param, 3, nzt, &fixes_on).is_err());
                }
            }
        }

        // Even on a zero-tangent conic, derivative orders 0, 1, and 3 behave identically.
        let degen = Conic::new(ConicType::Ellipse, 2.0, 0.0);
        for param in [0.0, 2.0] {
            for d in [0, 1] {
                assert_eq!(
                    degen
                        .evaluate_with_fixes(param, d, true, &fixes_off)
                        .unwrap(),
                    degen
                        .evaluate_with_fixes(param, d, true, &fixes_on)
                        .unwrap(),
                );
            }
            let err_off = degen
                .evaluate_with_fixes(param, 3, true, &fixes_off)
                .unwrap_err();
            let err_on = degen
                .evaluate_with_fixes(param, 3, true, &fixes_on)
                .unwrap_err();
            assert_eq!(format!("{err_off:?}"), format!("{err_on:?}"));
        }
    }
}
