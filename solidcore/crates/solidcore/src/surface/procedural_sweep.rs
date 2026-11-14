//! A swept wall stored as its definition (#2572, not upstream): the profile curve carried
//! by a screw motion about an axis, in closed form, beside its net B, the rational
//! B-spline wall [`crate::sweep::screw`] builds, and the net's certified distance from it,
//! `|B - S| <= bound`. The contract is the procedural blends'
//! ([`crate::surface::procedural_blend`]): a caller whose [`NetScope`] asks for a tolerance
//! at or above the bound reads the net, a tighter one reads S.
//!
//! **The definition.** At the face's `(u, v)` (through its map, which the net's reversals,
//! reparametrisations and swaps move), `w` is the profile's parameter and `t` the net's
//! parameter across the turns, whose knots are the wall's stations `t_0 < .. < t_n`
//! (radians of turn). S is the profile point `P(w)` turned by `theta(t)` about the axis
//! through `o` along the unit `k`, lifted `pitch theta / (2 pi)` along it, and first moved
//! `delta(theta)` along the unit radial direction `e` (the radial law, piecewise linear
//! in theta, held beyond its ends):
//!
//! `S(w, t) = o + R_k(theta) (P(w) - o + e delta(theta)) + k pitch theta / (2 pi)`.
//!
//! On the span `[t_i, t_i+1]`, with `s = 2 (t - t_i) / (t_i+1 - t_i) - 1` and
//! `tau = tan(h / 2)` (`h` the spans' half angle), `theta = mid_i + 2 atan(tau s)`: the
//! angle the net's rational quadratic arc turns a point to at the same parameter, so S
//! and B are compared point for point.
//!
//! **The bound.** The net is the profile's NURBS across, and across the turns each control
//! point's path: an exact rational quadratic arc (weights `1, cos h, 1`), its lift and
//! offset the same rational quadratic of their values at the span's ends and middle.
//! The screw motion at a fixed angle is affine in the point, so `B - S` is the same
//! for every profile point: per span, along `k` the lift's error `lambda E_l(x)`
//! (`lambda = |pitch| / (2 pi)`, `x = tau s`), and square to it the offset's
//! `(D / 2) E_o(x)` (`D` the law's change across the span). With `K = 2 (1 + tau^2)
//! atan(tau) / tau`,
//!
//! - `E_l(x) = K x / (1 + x^2) - 2 atan x`, odd, nought at `x = 0, +-tau`, its extreme at
//!   `x*^2 = (K - 2) / (K + 2)`: `max |E_l| = E_l(x*)` ([`lift_error_bound`]);
//! - `E_o(x) = (x (1 - tau^2) / tau - A(x) (1 - x^2), tau + x^2 / tau - 2 x A(x)) / (1 + x^2)`,
//!   `A(x) = atan x / atan tau`, its largest length by interval branch and bound over
//!   `[0, tau]` ([`offset_error_bound`]).
//!
//! Both are enclosed in rounded interval arithmetic ([`Ivl`], `atan` by its alternating
//! series with its remainder), never sampled. The bound is
//! `sqrt((lambda E_l*)^2 + (D / 2 E_o*)^2)` plus a floor ([`ProceduralSweep::floor`]), a
//! forward error bound of computing the net and of evaluating B and S.
//!
//! **Two motions, one kind (slice 2).** A wall swept along a general path ([`path`],
//! [`PathMotion`]) is the same definition with another motion: the profile carried along the
//! path in the path's rotation-minimizing frame, in closed form, the net
//! across a map from its parameter to the path's, and the bound an enclosure of the net
//! against it (the module's docs state the frame rule and the enclosure).
//!
//! The definition keeps no analytic kind of its own: the face's surface stays a B-spline,
//! so every match on its kind still sees the wall; the definition rides on the surface
//! ([`crate::surface::surface::Surface::procedural_sweep`]) as an exact round's does. A
//! copy or a piece of the net carries it (a piece keeps its parameters); an extension, a
//! mirror, an offset and any other new surface keep none.
//!
//! Deviations: none (no upstream counterpart). No recursion.

// The range tests are NaN-safe negations (`!(x > y)`), as `sweep::screw`'s.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::f64::consts::TAU;

use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::recertify::transform_rounding;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::math::gamma;
use crate::nurbs::srf_adv::p6::SweepTrace;
use crate::surface::procedural_blend::{BlendRef, NetScope};

pub mod path;

pub use path::PathMotion;

const OP_CREATE: &str = "surface::procedural_sweep::ProceduralSweep::create";
const OP_MOTION: &str = "surface::procedural_sweep::ScrewMotion::new";
const OP_LAW: &str = "surface::procedural_sweep::RadialLaw::new";
const OP_EVAL: &str = "surface::procedural_sweep::ProceduralSweep::evaluate_definition";

/// The widest half angle of a span of a screw sweep's net: its spans turn at most an
/// eighth of a turn ([`crate::sweep::screw`]), so `tau <= tan(pi / 16)`.
pub const MAX_HALF_ANGLE: f64 = TAU / 16.0;

/// The most terms of a series taken ([`alternating`]): for `|x| <= 1/2` the remainder of
/// `atan`'s is then below `2^-129 / 129`.
const SERIES_TERMS: u32 = 64;

/// The relative excess over the largest offset error at which the branch and bound stops.
const SUP_REL: f64 = 1e-3;

/// The most boxes the branch and bound splits.
const SUP_STEPS: usize = 4096;

/// A radial law: the offset `delta(theta)` along the unit `direction`, square to the
/// axis, piecewise linear through `(angles, offsets)` (radians of turn, ascending) and
/// held at its first and last offsets beyond them.
#[derive(Debug, Clone, PartialEq)]
pub struct RadialLaw {
    direction: Vec3,
    angles: Vec<f64>,
    offsets: Vec<f64>,
}

impl RadialLaw {
    /// A law through `angles` and `offsets` along `direction` (made unit).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a direction not finite or of no length, no angles, angles and
    /// offsets of different counts, a number not finite or angles not strictly ascending.
    pub fn new(direction: Vec3, angles: Vec<f64>, offsets: Vec<f64>) -> Result<Self> {
        let n = direction.length();
        if !(n.is_finite() && n > 0.0) {
            return Err(SolidError::invalid_input(
                OP_LAW,
                "a law's direction has a finite, non-zero length",
            ));
        }
        if angles.is_empty() || angles.len() != offsets.len() {
            return Err(SolidError::invalid_input(
                OP_LAW,
                "a law has an offset at each of its angles, at least one",
            ));
        }
        let finite = angles.iter().chain(&offsets).all(|x| x.is_finite());
        let ascending = angles.windows(2).all(|w| matches!(w, [a, b] if a < b));
        if !(finite && ascending) {
            return Err(SolidError::invalid_input(
                OP_LAW,
                "a law's angles ascend strictly and its numbers are finite",
            ));
        }
        Ok(Self {
            direction: direction * (1.0 / n),
            angles,
            offsets,
        })
    }

    /// The unit direction the offset moves the profile along.
    #[must_use]
    pub const fn direction(&self) -> Vec3 {
        self.direction
    }

    /// The angles, radians of turn.
    #[must_use]
    pub fn angles(&self) -> &[f64] {
        &self.angles
    }

    /// The offsets at the angles.
    #[must_use]
    pub fn offsets(&self) -> &[f64] {
        &self.offsets
    }

    /// The offset at angle `a`.
    #[must_use]
    pub fn offset(&self, a: f64) -> f64 {
        let (Some(&a0), Some(&o0)) = (self.angles.first(), self.offsets.first()) else {
            return 0.0;
        };
        if !(a > a0) {
            return o0;
        }
        for (wa, wo) in self.angles.windows(2).zip(self.offsets.windows(2)) {
            if let (&[a_prev, a_i], &[o_prev, o_i]) = (wa, wo) {
                if a <= a_i {
                    return o_prev + (o_i - o_prev) * (a - a_prev) / (a_i - a_prev);
                }
            }
        }
        self.offsets.last().copied().unwrap_or(0.0)
    }

    /// The slope `d delta / d theta` on `[from, to]`, where the law is linear (its angles
    /// bound the walls' pieces): the slope of its segment holding the middle, 0 where it
    /// is held.
    fn slope_over(&self, from: f64, to: f64) -> f64 {
        let mid = 0.5 * (from + to);
        for (wa, wo) in self.angles.windows(2).zip(self.offsets.windows(2)) {
            if let (&[a0, a1], &[o0, o1]) = (wa, wo) {
                if a0 <= mid && mid <= a1 {
                    return (o1 - o0) / (a1 - a0);
                }
            }
        }
        0.0
    }

    /// An upper bound of `|d delta / d theta|` over the segments meeting `(from, to)`.
    fn largest_slope_over(&self, from: f64, to: f64) -> f64 {
        let mut most = 0.0_f64;
        for (wa, wo) in self.angles.windows(2).zip(self.offsets.windows(2)) {
            if let (&[a0, a1], &[o0, o1]) = (wa, wo) {
                if a0 < to && a1 > from {
                    let slope = (Ivl::from(o1) - Ivl::from(o0)) / (Ivl::from(a1) - Ivl::from(a0));
                    most = most.max(slope.mag());
                }
            }
        }
        most
    }

    /// The largest offset's size.
    fn largest_offset(&self) -> f64 {
        self.offsets.iter().fold(0.0_f64, |m, o| m.max(o.abs()))
    }
}

/// A screw motion: about the axis through `origin` along the unit `axis`, rising `pitch`
/// per turn (signed), with an optional radial law.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrewMotion {
    origin: Point3,
    axis: Vec3,
    pitch: f64,
    law: Option<RadialLaw>,
}

impl ScrewMotion {
    /// The motion about `axis` (made unit) through `origin`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an origin or pitch not finite, a pitch of 0 or an axis not
    /// finite or of no length.
    pub fn new(origin: Point3, axis: Vec3, pitch: f64, law: Option<RadialLaw>) -> Result<Self> {
        let finite = origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite();
        let n = axis.length();
        if !(finite && pitch.is_finite() && pitch != 0.0 && n.is_finite() && n > 0.0) {
            return Err(SolidError::invalid_input(
                OP_MOTION,
                "a screw motion has a finite origin, a finite non-zero pitch and an axis",
            ));
        }
        Ok(Self {
            origin,
            axis: axis * (1.0 / n),
            pitch,
            law,
        })
    }

    /// A point of the axis.
    #[must_use]
    pub const fn origin(&self) -> Point3 {
        self.origin
    }

    /// The unit axis.
    #[must_use]
    pub const fn axis(&self) -> Vec3 {
        self.axis
    }

    /// The rise per turn.
    #[must_use]
    pub const fn pitch(&self) -> f64 {
        self.pitch
    }

    /// The radial law, if any.
    #[must_use]
    pub const fn law(&self) -> Option<&RadialLaw> {
        self.law.as_ref()
    }

    /// Rodrigues: `v` turned right-handed about the axis by `a`.
    fn turn(&self, v: Vec3, a: f64) -> Vec3 {
        let k = self.axis;
        v * a.cos() + k.cross(v) * a.sin() + k * (k.dot(v) * (1.0 - a.cos()))
    }

    /// The lift at angle `a`.
    fn lift(&self, a: f64) -> f64 {
        self.pitch * a / TAU
    }

    /// The radial move at angle `a` and its rate in `a`.
    fn shift(&self, a: f64, slope: f64) -> (Vec3, Vec3) {
        match &self.law {
            Some(l) => (l.direction * l.offset(a), l.direction * slope),
            None => (Vec3::ZERO, Vec3::ZERO),
        }
    }
}

/// How the face's `(u, v)` give the definition's `(w, t)`: `(p, q)` is `(u, v)`, or
/// `(v, u)` when swapped, and `w = a[0] + b[0] p`, `t = a[1] + b[1] q`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SweepMap {
    a: [f64; 2],
    b: [f64; 2],
    swapped: bool,
}

impl Default for SweepMap {
    fn default() -> Self {
        Self {
            a: [0.0, 0.0],
            b: [1.0, 1.0],
            swapped: false,
        }
    }
}

/// How a sweep carries its profile: one definition kind, two motions (#2572).
#[derive(Debug, Clone)]
pub enum Motion {
    /// A screw motion about an axis (slice 1, [`crate::sweep::screw`]).
    Screw(ScrewMotion),
    /// The profile along a general path in a frame (slice 2, [`path`]).
    Path(PathMotion),
}

/// A swept wall by its definition (module docs).
#[derive(Debug, Clone)]
pub struct ProceduralSweep {
    profile: BSplineCurve,
    motion: Motion,
    stations: Vec<f64>,
    half: f64,
    tau: f64,
    bound: f64,
    map: SweepMap,
}

/// `sum_{n >= 0} (-1)^n c(n) p0 r^n` enclosed, for `0 <= p0`, `0 <= r` and terms that fall
/// in size from the first (an alternating series): its partial sum until a term is below
/// `2^-64` of the sum or after [`SERIES_TERMS`] terms, widened by the first term left out,
/// which bounds the rest. `p0` and `r` are intervals, so the sum holds for every series
/// they hold.
fn alternating(p0: Ivl, r: Ivl, c: impl Fn(u32) -> Ivl) -> Ivl {
    let mut power = p0;
    let mut sum = Ivl::from(0.0);
    let mut n = 0;
    while n < SERIES_TERMS {
        let term = c(n) * power;
        sum = if n % 2 == 0 { sum + term } else { sum - term };
        power = power * r;
        n += 1;
        if (c(n) * power).mag() <= sum.mig() * f64::powi(2.0, -64) {
            break;
        }
    }
    let rest = (c(n) * power).mag();
    sum + Ivl::new(-rest, rest)
}

/// `1 / m` enclosed.
fn reciprocal(m: u32) -> Ivl {
    Ivl::from(1.0) / Ivl::from(f64::from(m))
}

/// `atan(x)` enclosed for `0 <= x <= 1/2`: `sum (-1)^n x^(2n+1) / (2n+1)` (the whole line
/// outside).
fn atan_point(x: f64) -> Ivl {
    if !(0.0..=0.5).contains(&x) {
        return Ivl::whole();
    }
    let xi = Ivl::from(x);
    alternating(xi, xi * xi, |n| reciprocal(2 * n + 1))
}

/// `atan` over `x`, `0 <= x <= 1/2` (it rises).
fn atan_ivl(x: Ivl) -> Ivl {
    Ivl::new(atan_point(x.lo).lo, atan_point(x.hi).hi)
}

/// `max |E_l|` over a span of `tau = tan(h / 2)`, `0 < tau <= 1/2`, enclosed from above
/// (module docs): in radians of turn, how far the net's rational quadratic lift leads or
/// lags the true one. `E_l` is a difference of numbers far larger than it, so both it and
/// `K - 2` are taken as their own series, which do not cancel:
///
/// - `K - 2 = 4 sum_{n >= 1} (-1)^(n-1) tau^(2n) / (4 n^2 - 1)` (from `(1 + tau^2) atan(tau)
///   / tau`);
/// - `E_l(x) = (K - 2) x / (1 + x^2) + 2 sum_{i >= 1} (-1)^i 2i / (2i + 1) x^(2i+1)` (from
///   `2 x / (1 + x^2) - 2 atan x`), its terms falling for `x <= 1/2`.
///
/// Both are alternating with falling terms, so [`alternating`] encloses them; `E_l` is
/// taken over an interval holding `x*`.
fn lift_error_bound(tau: f64) -> f64 {
    if !(tau > 0.0 && tau <= 0.5) {
        return f64::INFINITY;
    }
    let one = Ivl::from(1.0);
    let four = Ivl::from(4.0);
    let t2 = Ivl::from(tau) * Ivl::from(tau);
    let kappa = four * alternating(t2, t2, |n| reciprocal((2 * n + 1) * (2 * n + 3)));
    let x2 = kappa / (four + kappa);
    let x = x2.sqrt();
    let x3 = x * x2;
    let series = alternating(x3, x2, |n| {
        Ivl::from(f64::from(2 * n + 2)) * reciprocal(2 * n + 3)
    });
    let e = kappa * x / (one + x2) - Ivl::from(2.0) * series;
    e.mag()
}

/// `|E_o|^2` over `x` (module docs).
fn offset_error_sq(tau: f64, x: Ivl) -> Ivl {
    let one = Ivl::from(1.0);
    let two = Ivl::from(2.0);
    let t = Ivl::from(tau);
    let a = atan_ivl(x) / atan_point(tau);
    let x2 = x * x;
    let den = one + x2;
    let ex = (x * (one - t * t) / t - a * (one - x2)) / den;
    let ey = (t + x2 / t - two * x * a) / den;
    ex * ex + ey * ey
}

/// The largest `|E_o|` over a span of `tau`, enclosed from above by interval branch and
/// bound over `[0, tau]` (`|E_o|` is even): the box of the largest upper end is split
/// until it is within [`SUP_REL`] of a value the function takes, or after [`SUP_STEPS`]
/// splits; the answer is that box's upper end, which no box exceeds.
fn offset_error_bound(tau: f64) -> f64 {
    let f = |lo: f64, hi: f64| offset_error_sq(tau, Ivl::new(lo, hi));
    let mut boxes: Vec<(f64, f64, f64)> = vec![(f(0.0, tau).hi, 0.0, tau)];
    let mut lower = f(0.0, 0.0).lo.max(f(tau, tau).lo);
    for _ in 0..SUP_STEPS {
        let Some((at, &(up, lo, hi))) = boxes
            .iter()
            .enumerate()
            .max_by(|x, y| x.1 .0.total_cmp(&y.1 .0))
        else {
            break;
        };
        let mid = 0.5 * (lo + hi);
        let open = up > lower + SUP_REL * lower.abs();
        if !(open && lo < mid && mid < hi) {
            return Ivl::new(0.0, up).sqrt().hi;
        }
        boxes.swap_remove(at);
        lower = lower.max(f(mid, mid).lo);
        boxes.push((f(lo, mid).hi, lo, mid));
        boxes.push((f(mid, hi).hi, mid, hi));
    }
    let up = boxes.iter().fold(0.0_f64, |m, b| m.max(b.0));
    Ivl::new(0.0, up).sqrt().hi
}

impl ProceduralSweep {
    /// The definition of the wall `sweep::screw` builds from `profile` over `stations`
    /// (the net's knots across the turns, radians, ascending) with spans of half angle
    /// `half`, under `motion`; its bound certified ([`ProceduralSweep::certify`]).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a profile not of dimension 3 or with numbers not finite or
    /// weights not positive, fewer than two stations, stations not finite or not strictly
    /// ascending, a half angle out of `(0, MAX_HALF_ANGLE]`, or a bound not certified
    /// finite; those of reading the profile.
    pub(crate) fn create(
        profile: &BSplineCurve,
        motion: ScrewMotion,
        stations: &[f64],
        half: f64,
    ) -> Result<Self> {
        let c = profile.canonical()?;
        let finite = c
            .control_points
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
        let weights = c.weights.iter().all(|&w| w.is_finite() && w > 0.0);
        if c.dim != 3 || !finite || !weights {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a profile is a 3D curve of finite control points and positive weights",
            ));
        }
        let ascending = stations.len() >= 2
            && stations.iter().all(|t| t.is_finite())
            && stations.windows(2).all(|w| matches!(w, [a, b] if a < b));
        if !ascending {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a wall's stations are at least two finite angles, strictly ascending",
            ));
        }
        if !(half > 0.0 && half <= MAX_HALF_ANGLE) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a span turns more than nothing and at most an eighth of a turn",
            ));
        }
        let mut profile = profile.copy();
        profile.set_out_of_bounds_enabled(false);
        let mut sweep = Self {
            profile,
            motion: Motion::Screw(motion),
            stations: stations.to_vec(),
            half,
            tau: (0.5 * half).tan(),
            bound: 0.0,
            map: SweepMap::default(),
        };
        sweep.bound = sweep.certify()?;
        Ok(sweep)
    }

    /// The profile curve.
    #[must_use]
    pub const fn profile(&self) -> &BSplineCurve {
        &self.profile
    }

    /// The motion.
    #[must_use]
    pub const fn motion(&self) -> &Motion {
        &self.motion
    }

    /// The screw motion, for a screw-swept wall.
    #[must_use]
    pub const fn screw(&self) -> Option<&ScrewMotion> {
        match &self.motion {
            Motion::Screw(m) => Some(m),
            Motion::Path(_) => None,
        }
    }

    /// The path motion, for a wall swept along a general path.
    #[must_use]
    pub const fn path(&self) -> Option<&PathMotion> {
        match &self.motion {
            Motion::Path(m) => Some(m),
            Motion::Screw(_) => None,
        }
    }

    /// The net's stations across the turns, radians: the face's turn range is their
    /// first to their last.
    #[must_use]
    pub fn stations(&self) -> &[f64] {
        &self.stations
    }

    /// The spans' half angle.
    #[must_use]
    pub const fn half_angle(&self) -> f64 {
        self.half
    }

    /// The net's certified distance from the definition.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        self.bound
    }

    /// Whether the net answers an evaluation on `k`: the innermost [`NetScope`] asks for
    /// a tolerance at or above the bound (the blends' rule,
    /// [`crate::surface::procedural_blend::ProceduralBlend::net_answers`]).
    #[must_use]
    pub fn net_answers(&self, k: &Kernel) -> bool {
        NetScope::tolerance(k) >= self.bound
    }

    /// The definition's `(w, t)` at the face's `(u, v)`.
    #[must_use]
    pub fn definition_parameters(&self, uv: Point2) -> Point2 {
        let m = &self.map;
        let (p, q) = if m.swapped {
            (uv.y, uv.x)
        } else {
            (uv.x, uv.y)
        };
        Point2::new(m.a[0] + m.b[0] * p, m.a[1] + m.b[1] * q)
    }

    /// The map's terms: `(w, t) = (a[0] + b[0] p, a[1] + b[1] q)`, `(p, q)` the face's
    /// `(u, v)`, or `(v, u)` when swapped (the blends' `GetDefinitionMap`).
    #[must_use]
    pub const fn definition_map(&self) -> ([f64; 2], [f64; 2], bool) {
        (self.map.a, self.map.b, self.map.swapped)
    }

    /// The angle `theta(t)` the net's arcs turn to at its parameter `t` across the turns
    /// (clamped to the stations), and `d theta / dt`.
    #[must_use]
    pub fn turn_at(&self, t: f64) -> (f64, f64) {
        if !matches!(self.motion, Motion::Screw(_)) {
            return (t, 1.0);
        }
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return (t, 1.0);
        };
        let t = t.clamp(first, last);
        let spans = self.stations.len().saturating_sub(1);
        let i = self
            .stations
            .partition_point(|&x| x <= t)
            .saturating_sub(1)
            .min(spans.saturating_sub(1));
        let (Some(&t0), Some(&t1)) = (self.stations.get(i), self.stations.get(i + 1)) else {
            return (t, 1.0);
        };
        let width = t1 - t0;
        let s = 2.0 * (t - t0) / width - 1.0;
        let x = self.tau * s;
        let theta = 0.5 * (t0 + t1) + 2.0 * x.atan();
        let rate = 2.0 * self.tau / (1.0 + x * x) * (2.0 / width);
        (theta, rate)
    }

    /// The definition's partials at the face's `uv`, `d[i][j] = d^(i+j) S / du^i dv^j`,
    /// `i <= highest_u_deriv`, `j <= highest_v_deriv`, in closed form; `uv` clamped to the
    /// definition's domain.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an order above 1; those of evaluating the profile.
    pub fn evaluate_definition(
        &self,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
    ) -> Result<Vec<Vec<Vec3>>> {
        self.evaluate_definition_side(uv, highest_u_deriv, highest_v_deriv, false)
    }

    /// [`ProceduralSweep::evaluate_definition`] at a joint of the path (a node of the map):
    /// the segment that starts there, or the one that ends there when `left`, as a face that
    /// ends at the joint reads it ([`SweepRef`] asks for it at the face's end). A screw has
    /// no joint.
    ///
    /// # Errors
    ///
    /// Those of [`ProceduralSweep::evaluate_definition`].
    pub fn evaluate_definition_side(
        &self,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
        left: bool,
    ) -> Result<Vec<Vec<Vec3>>> {
        if highest_u_deriv > 1 || highest_v_deriv > 1 {
            return Err(SolidError::invalid_input(
                OP_EVAL,
                "a sweep's definition gives partials to the first order in each parameter",
            ));
        }
        let wt = self.definition_parameters(uv);
        let first = highest_u_deriv + highest_v_deriv > 0;
        let (s, s_w, s_t, s_wt) = match &self.motion {
            Motion::Screw(m) => self.screw_partials(m, wt, first)?,
            Motion::Path(m) => self.path_partials(m, wt, first, left)?,
        };
        Ok(self.assemble(s, s_w, s_t, s_wt, highest_u_deriv, highest_v_deriv))
    }

    /// The screw definition's `S`, `S_w`, `S_t` and `S_wt` at `(w, t)`.
    fn screw_partials(
        &self,
        m: &ScrewMotion,
        wt: Point2,
        first: bool,
    ) -> Result<(Vec3, Vec3, Vec3, Vec3)> {
        let p = self
            .profile
            .evaluate(wt.x, usize::from(first), false, false)?;
        let point = p.first().copied().unwrap_or(Vec3::ZERO);
        let tangent = p.get(1).copied().unwrap_or(Vec3::ZERO);
        let (theta, rate) = self.turn_at(wt.y);
        let (first_t, last_t) = (
            self.stations.first().copied().unwrap_or(0.0),
            self.stations.last().copied().unwrap_or(0.0),
        );
        let slope = m
            .law
            .as_ref()
            .map_or(0.0, |l| l.slope_over(first_t, last_t));
        let (shift, shift_rate) = m.shift(theta, slope);
        let arm = m.turn(point - m.origin + shift, theta);
        let s = m.origin + arm + m.axis * m.lift(theta);
        // dS/dw, dS/dt and d2S/dw dt
        let s_w = m.turn(tangent, theta);
        let s_t = (m.axis.cross(arm) + m.turn(shift_rate, theta) + m.axis * (m.pitch / TAU)) * rate;
        let s_wt = m.axis.cross(s_w) * rate;
        Ok((s, s_w, s_t, s_wt))
    }

    /// The path definition's `S`, `S_w`, `S_t` and `S_wt` at `(w, v)`, `v` the net's
    /// parameter across the sweep ([`PathMotion::path_parameter`]).
    fn path_partials(
        &self,
        m: &PathMotion,
        wt: Point2,
        first: bool,
        left: bool,
    ) -> Result<(Vec3, Vec3, Vec3, Vec3)> {
        let p = self
            .profile
            .evaluate(wt.x, usize::from(first), false, false)?;
        let point = p.first().copied().unwrap_or(Vec3::ZERO);
        let tangent = p.get(1).copied().unwrap_or(Vec3::ZERO);
        let (t, slope) = m.path_parameter_side(wt.y, left);
        let frame = m.frame_side(t, left)?;
        let s = frame.place(point);
        let s_w = frame.turn(tangent);
        let s_t = frame.rate(point) * slope;
        let s_wt = frame.turn_rate(tangent) * slope;
        Ok((s, s_w, s_t, s_wt))
    }

    /// The partials of the face at its `(u, v)` from those at the definition's `(w, t)`.
    fn assemble(
        &self,
        s: Vec3,
        s_w: Vec3,
        s_t: Vec3,
        s_wt: Vec3,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
    ) -> Vec<Vec<Vec3>> {
        let [b0, b1] = self.map.b;
        let (du, dv) = if self.map.swapped {
            (s_t * b1, s_w * b0)
        } else {
            (s_w * b0, s_t * b1)
        };
        let mixed = s_wt * (b0 * b1);
        let mut d = vec![vec![Vec3::ZERO; highest_v_deriv + 1]; highest_u_deriv + 1];
        for (i, row) in d.iter_mut().enumerate() {
            for (j, slot) in row.iter_mut().enumerate() {
                *slot = match (i, j) {
                    (0, 0) => s,
                    (1, 0) => du,
                    (0, 1) => dv,
                    _ => mixed,
                };
            }
        }
        d
    }

    /// The definition's point at the face's `uv`.
    ///
    /// # Errors
    ///
    /// Those of [`ProceduralSweep::evaluate_definition`].
    pub fn evaluate_definition_point(&self, uv: Point2) -> Result<Point3> {
        let d = self.evaluate_definition(uv, 0, 0)?;
        Ok(d.first()
            .and_then(|r| r.first())
            .copied()
            .unwrap_or(Vec3::ZERO))
    }

    /// The bound certified from the definition (module docs): the lift's and the
    /// offset's errors enclosed, and the floor. A definition made by
    /// [`crate::sweep::screw`] stores it as its bound; a motion adds its rounding to the
    /// stored bound ([`ProceduralSweep::transform`]).
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the enclosure is not finite; those of reading the profile.
    pub fn certify(&self) -> Result<f64> {
        let Motion::Screw(m) = &self.motion else {
            return Ok(self.bound);
        };
        let lam = Ivl::from(m.pitch.abs()) / Ivl::from(TAU);
        let lift = lam * Ivl::from(lift_error_bound(self.tau));
        let change = self.largest_change_per_span();
        let offset = if change > 0.0 {
            Ivl::from(0.5 * change) * Ivl::from(offset_error_bound(self.tau))
        } else {
            Ivl::from(0.0)
        };
        let exact = (lift * lift + offset * offset).sqrt().hi;
        let bound = round_up(exact + self.floor()?);
        if bound.is_finite() && bound > 0.0 {
            Ok(bound)
        } else {
            Err(SolidError::invalid_input(
                OP_CREATE,
                "the net's distance from the definition is not certified finite",
            ))
        }
    }

    /// An upper bound of the law's change across one span, `|D|`.
    fn largest_change_per_span(&self) -> f64 {
        let Motion::Screw(m) = &self.motion else {
            return 0.0;
        };
        let Some(law) = m.law.as_ref() else {
            return 0.0;
        };
        let (Some(&first), Some(&last)) = (self.stations.first(), self.stations.last()) else {
            return 0.0;
        };
        let width = self.stations.windows(2).fold(0.0_f64, |m, w| match w {
            [a, b] => m.max((Ivl::from(*b) - Ivl::from(*a)).hi),
            _ => m,
        });
        (Ivl::from(law.largest_slope_over(first, last)) * Ivl::from(width)).hi
    }

    /// The floor of the bound: a forward error bound of how the net is computed and B
    /// and S are evaluated, `G gamma_K (A + (R + lambda) Theta + R)`:
    ///
    /// - A bounds the coordinates of the net's points, `|o| + (max |P_l - o| + delta*) /
    ///   cos h + lambda Theta` (a mid-span control point is moved out by `1 / cos h`);
    ///   each coordinate is the sum of a few products of numbers of at most that size;
    /// - `(R + lambda) Theta`: an angle of at most `Theta` (the stations' largest) is
    ///   rounded relative to its size, which turns a point at reach `R` (the profile's
    ///   largest from the axis, plus the law's largest offset) and lifts it by `lambda`
    ///   times that; `R` again for the arcs' weight `cos h` and `tau`, each a rounding;
    /// - `K = 2 (p + 3) + 24` operations: the turn (Rodrigues, ten with its sine and
    ///   cosine, each within an ulp), the mid point's scale and weight (two), the sums
    ///   with the origin and the lift (three), the lift and the stations' angles (six),
    ///   `cos h`, `tan(h / 2)` and `atan` (three), and evaluating the two rational
    ///   factors of degrees p and 2 in homogeneous coordinates (`2 (p + 3)`);
    /// - G, the weights' ratio, the profile's over `cos h`: a homogeneous error becomes a
    ///   Euclidean one divided by the weight.
    ///
    /// # Errors
    ///
    /// Those of reading the profile.
    pub fn floor(&self) -> Result<f64> {
        let c = self.profile.canonical()?;
        let Motion::Screw(m) = &self.motion else {
            return self.path_floor();
        };
        let (w_lo, w_hi) = if c.weights.is_empty() {
            (1.0, 1.0)
        } else {
            c.weights
                .iter()
                .fold((f64::INFINITY, 0.0_f64), |(lo, hi), &w| {
                    (lo.min(w), hi.max(w))
                })
        };
        let cos_h = self.half.cos();
        let mut dist = 0.0_f64;
        let mut reach = 0.0_f64;
        for &p in &c.control_points {
            let w = p - m.origin;
            dist = dist.max(w.length());
            reach = reach.max((w - m.axis * w.dot(m.axis)).length());
        }
        let delta = m.law.as_ref().map_or(0.0, RadialLaw::largest_offset);
        let theta = self.stations.iter().fold(0.0_f64, |t, &x| t.max(x.abs()));
        let lam = m.pitch.abs() / TAU;
        let a = m.origin.length() + (dist + delta) / cos_h + lam * theta;
        let r = reach + delta;
        let g = (w_hi / w_lo) / cos_h;
        let floor = g * gamma(2 * (c.degree + 3) + 24) * (a + (r + lam) * theta + r);
        if floor.is_finite() {
            Ok(floor)
        } else {
            Err(SolidError::invalid_input(
                OP_CREATE,
                "the floor of the bound is not finite",
            ))
        }
    }

    /// The net's parameter in `u` (or v) became `w'` with `w = alpha + beta w'` (a
    /// reversal or reparametrisation of the net): the map follows.
    pub(crate) fn reparametrize_map(&mut self, u: bool, alpha: f64, beta: f64) {
        // u drives w unless swapped
        let idx = usize::from(u == self.map.swapped);
        if let (Some(a), Some(&b)) = (self.map.a.get_mut(idx), self.map.b.get(idx)) {
            *a += b * alpha;
        }
        if let Some(b) = self.map.b.get_mut(idx) {
            *b *= beta;
        }
    }

    /// The net's u and v swapped: the map follows.
    pub(crate) fn swap_map(&mut self) {
        self.map.swapped = !self.map.swapped;
    }

    /// Whether the definition follows a motion of its net: a rigid motion with a scale
    /// that is the same number on every axis (anything else leaves no screw motion, and
    /// the net moves without its definition).
    #[must_use]
    pub fn follows(scale: Option<Vec3>) -> bool {
        scale.is_none_or(|s| s.x == s.y && s.x == s.z && s.x > 0.0 && s.x.is_finite())
    }

    /// Moves the definition with its net by `rotate_n_move` after the scale `scale`
    /// (one that [`ProceduralSweep::follows`]): the profile, the axis, its origin and the
    /// law's direction move, the pitch and the offsets scale. The bound scales and takes
    /// in how far the placement is from a rotation: with `eta` the largest of
    /// `| |x|^2 - 1 |`, `| |y|^2 - 1 |` and `|x . y|`, the motion is a rotation plus at
    /// most `3 eta` in norm, which moves each of the profile, the origin, the axis (made
    /// unit: twice) and the law's direction by that times their lever, the definition's
    /// reach from the origin. [`ProceduralSweep::add_motion_rounding`] adds the net's own.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a scale it does not follow or a motion that leaves no axis; those
    /// of the profile's transform.
    pub(crate) fn transform(
        &mut self,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
    ) -> Result<()> {
        if !Self::follows(scale) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a sweep's definition moves rigidly or by a uniform scale",
            ));
        }
        let factor = scale.map_or(1.0, |s| s.x);
        let (x, y) = (rotate_n_move.x_axis(), rotate_n_move.y_axis());
        let eta = (x.dot(x) - 1.0)
            .abs()
            .max((y.dot(y) - 1.0).abs())
            .max(x.dot(y).abs());
        let reach = self.reach()?;
        let m = match &mut self.motion {
            Motion::Screw(m) => {
                self.profile.transform(rotate_n_move, scale)?;
                m
            }
            Motion::Path(p) => {
                p.transform(rotate_n_move, scale)?;
                self.profile.transform(&Axis2Placement::IDENTITY, scale)?;
                self.bound = round_up(
                    self.bound * factor * (1.0 + 3.0 * eta)
                        + 12.0 * eta * (reach + self.bound) * factor,
                );
                return Ok(());
            }
        };
        let origin = rotate_n_move.transform_point(m.origin * factor);
        let axis = rotate_n_move.transform_vector(m.axis);
        let law = match m.law.take() {
            Some(l) => Some(RadialLaw::new(
                rotate_n_move.transform_vector(l.direction),
                l.angles,
                l.offsets.iter().map(|o| o * factor).collect(),
            )?),
            None => None,
        };
        *m = ScrewMotion::new(origin, axis, m.pitch * factor, law)?;
        self.bound = round_up(
            self.bound * factor * (1.0 + 3.0 * eta) + 12.0 * eta * (reach + self.bound) * factor,
        );
        Ok(())
    }

    /// Adds the rounding of moving the net `net` (already moved) to the bound, as a
    /// moved blend's ([`crate::blend::fillet_section::recertify`]).
    ///
    /// # Errors
    ///
    /// Those of reading the net.
    pub(crate) fn add_motion_rounding(&mut self, net: &BSplineSurface) -> Result<()> {
        self.bound = round_up(self.bound + transform_rounding(net)?);
        Ok(())
    }

    /// How far the definition reaches from the axis's origin: `|P - o| + delta* + lambda
    /// Theta`.
    fn reach(&self) -> Result<f64> {
        let c = self.profile.canonical()?;
        let Motion::Screw(m) = &self.motion else {
            return self.path_reach();
        };
        let dist = c
            .control_points
            .iter()
            .fold(0.0_f64, |d, &p| d.max((p - m.origin).length()));
        let delta = m.law.as_ref().map_or(0.0, RadialLaw::largest_offset);
        let theta = self.stations.iter().fold(0.0_f64, |t, &x| t.max(x.abs()));
        Ok(dist + delta + m.pitch.abs() / TAU * theta)
    }
}

impl ProceduralSweep {
    /// The profile's reach: its largest control point from the frame's origin.
    fn profile_reach(&self) -> Result<f64> {
        let c = self.profile.canonical()?;
        Ok(c.control_points
            .iter()
            .fold(0.0_f64, |m, q| m.max(q.length())))
    }

    /// How far a path sweep reaches from the origin: the path's and the profile's.
    fn path_reach(&self) -> Result<f64> {
        let Motion::Path(p) = &self.motion else {
            return Ok(0.0);
        };
        Ok(p.reach()? + self.profile_reach()?)
    }

    /// The floor of a path sweep's bound (included in it), [`path::rounding_floor`] at the
    /// definition's own sizes (the bound's takes the net's control points in as well).
    fn path_floor(&self) -> Result<f64> {
        let Motion::Path(p) = &self.motion else {
            return Ok(0.0);
        };
        Ok(path::rounding_floor(p.reach()?, self.profile_reach()?))
    }

    /// The definition of the wall `sweep_scale` fitted: `profile` carried along `path` in the
    /// path's rotation-minimizing frame (started from the normal `trace` holds), its bound
    /// certified against `net` ([`PathMotion::create`]).
    ///
    /// # Errors
    ///
    /// Those of [`PathMotion::create`]: a net that is not the profile swept along the path.
    /// The definition of a path sweep and the net that is it exactly ([`PathMotion::create_exact`]),
    /// for a path of lines and circular arcs.
    ///
    /// # Errors
    ///
    /// Those of [`PathMotion::create_exact`].
    pub(crate) fn create_path_exact(
        profile: &BSplineCurve,
        path: &BSplineCurve,
        start_normal: Vec3,
    ) -> Result<(Self, BSplineSurface)> {
        let (motion, net, bound) = PathMotion::create_exact(profile, path, start_normal)?;
        let mut profile = profile.copy();
        profile.set_out_of_bounds_enabled(false);
        let stations = motion.nodes().iter().map(|n| n.0).collect();
        let sweep = Self {
            profile,
            motion: Motion::Path(motion),
            stations,
            half: 0.0,
            tau: 0.0,
            bound,
            map: SweepMap {
                swapped: true,
                ..SweepMap::default()
            },
        };
        Ok((sweep, net))
    }

    pub(crate) fn create_path(
        profile: &BSplineCurve,
        path: &BSplineCurve,
        trace: &SweepTrace,
        net: &BSplineSurface,
    ) -> Result<Self> {
        let (motion, bound, swapped) = PathMotion::create(profile, path, trace, net)?;
        let mut profile = profile.copy();
        profile.set_out_of_bounds_enabled(false);
        let stations = motion.nodes().iter().map(|n| n.0).collect();
        Ok(Self {
            profile,
            motion: Motion::Path(motion),
            stations,
            half: 0.0,
            tau: 0.0,
            bound,
            map: SweepMap {
                swapped,
                ..SweepMap::default()
            },
        })
    }
}

/// A swept face by its definition: the definition and the net it rides on.
#[derive(Debug, Clone, Copy)]
pub struct SweepRef<'a> {
    pub(crate) sweep: &'a ProceduralSweep,
    pub(crate) net: &'a BSplineSurface,
}

impl<'a> SweepRef<'a> {
    /// Whether a path definition read at `uv` takes the segment that ends at a joint: the face's
    /// own side at an end of its parameter across the sweep is inward, and the map takes the
    /// face's parameter to the definition's with the sign of its slope `b`, so the face's far end
    /// is the definition's earlier side when `b > 0` and its later side when `b < 0`, and the
    /// face's near end the reverse.
    fn joint_side_is_left(&self, uv: Point2) -> bool {
        let dom = self.net.natural_uv_domain();
        let (x, iv) = if self.sweep.map.swapped {
            (uv.x, dom.u_interval())
        } else {
            (uv.y, dom.v_interval())
        };
        let tol = 1e-12 * (1.0 + iv.max().abs().max(iv.min().abs()));
        let far = x >= iv.max() - tol && x > iv.min();
        let near = x <= iv.min() + tol && x < iv.max();
        let rising = self.sweep.map.b[1] > 0.0;
        (far && rising) || (near && !rising)
    }

    /// The definition.
    #[must_use]
    pub const fn definition(&self) -> &'a ProceduralSweep {
        self.sweep
    }

    /// The net.
    #[must_use]
    pub const fn net(&self) -> &'a BSplineSurface {
        self.net
    }

    /// The net's certified distance from the definition.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        self.sweep.bound
    }

    /// The point at `uv`: the net's where it answers the [`NetScope`] on `k`
    /// ([`ProceduralSweep::net_answers`]), the definition's elsewhere.
    ///
    /// # Errors
    ///
    /// Those of the net's and the definition's evaluations.
    pub fn evaluate_point(&self, k: &Kernel, uv: Point2) -> Result<Point3> {
        if self.sweep.net_answers(k) {
            return self.net.evaluate_point(uv);
        }
        self.sweep.evaluate_definition_point(uv)
    }

    /// The partials at `uv` to the first order in each parameter, the net's or the
    /// definition's as [`SweepRef::evaluate_point`] chooses.
    ///
    /// # Errors
    ///
    /// Those of the net's and the definition's evaluations.
    pub fn evaluate(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
    ) -> Result<Vec<Vec<Vec3>>> {
        if self.sweep.net_answers(k) {
            return self.net.evaluate(
                uv,
                highest_u_deriv,
                highest_v_deriv,
                EvaluateFlags::default(),
            );
        }
        self.sweep.evaluate_definition_side(
            uv,
            highest_u_deriv,
            highest_v_deriv,
            self.joint_side_is_left(uv),
        )
    }
}

/// A face by its definition, whatever made it: a blend ([`BlendRef`]) or a sweep
/// ([`SweepRef`]) ([`crate::surface::surface::Surface::procedural_definition`]).
#[derive(Debug, Clone, Copy)]
pub enum DefinitionRef<'a> {
    /// A blend's.
    Blend(BlendRef<'a>),
    /// A sweep's.
    Sweep(SweepRef<'a>),
}

impl<'a> DefinitionRef<'a> {
    /// The net's certified distance from the definition.
    #[must_use]
    pub fn bound(&self) -> f64 {
        match self {
            Self::Blend(b) => b.bound(),
            Self::Sweep(s) => s.bound(),
        }
    }

    /// The sweep, for a sweep's face.
    #[must_use]
    pub const fn sweep(&self) -> Option<SweepRef<'a>> {
        match *self {
            Self::Sweep(s) => Some(s),
            Self::Blend(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The series enclosure holds the library's atan, and is tight.
    #[test]
    fn atan_is_enclosed() {
        for x in [0.0, 1e-9, 0.05, 0.0985, 0.19891, 0.5] {
            let e = atan_point(x);
            let y = f64::atan(x);
            assert!(e.lo <= y && y <= e.hi, "{x}: {e:?} misses {y}");
            assert!(e.hi - e.lo <= 1e-13 * y + 1e-290, "{x}: {e:?} is loose");
        }
        assert_eq!(atan_point(0.6), Ivl::whole());
        assert_eq!(atan_point(-0.1), Ivl::whole());
    }

    /// The lift's error enclosed at its extreme: no sampled value exceeds it, and the
    /// densest samples come within a part in a million of it (it is no looser).
    #[test]
    fn the_lift_error_bound_is_its_maximum() {
        for half in [MAX_HALF_ANGLE, 0.1, 1e-3] {
            let tau = (0.5 * half).tan();
            let k = 2.0 * (1.0 + tau * tau) * tau.atan() / tau;
            let e = |x: f64| k * x / (1.0 + x * x) - 2.0 * x.atan();
            let bound = lift_error_bound(tau);
            let mut most = 0.0_f64;
            for i in 0..=20_000 {
                let x = tau * f64::from(i) / 20_000.0;
                most = most.max(e(x).abs());
            }
            assert!(most <= bound, "{half}: sampled {most} past {bound}");
            assert!(
                most >= bound * (1.0 - 1e-6),
                "{half}: {bound} loose, {most}"
            );
        }
    }

    /// The offset's error enclosed: past every sampled value, within its stopping margin.
    #[test]
    fn the_offset_error_bound_holds_and_is_tight() {
        for half in [MAX_HALF_ANGLE, 0.05] {
            let tau = (0.5 * half).tan();
            let bound = offset_error_bound(tau);
            let mut most = 0.0_f64;
            for i in 0..=20_000 {
                let x = Ivl::from(tau * f64::from(i) / 20_000.0);
                most = most.max(offset_error_sq(tau, x).mid().sqrt());
            }
            assert!(most <= bound, "{half}: sampled {most} past {bound}");
            assert!(
                most >= bound * (1.0 - 2.0 * SUP_REL),
                "{half}: {bound} loose, {most}"
            );
        }
    }

    #[test]
    fn hostile_definitions_are_refused() {
        let line = BSplineCurve::create_line_segment(
            3,
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 1.0),
        )
        .unwrap();
        let z = Vec3::new(0.0, 0.0, 1.0);
        let motion = ScrewMotion::new(Point3::ZERO, z, 1.5, None).unwrap();
        let ok = ProceduralSweep::create(&line, motion.clone(), &[0.0, 0.5, 1.0], 0.25);
        assert!(ok.is_ok_and(|s| s.bound() > 0.0));
        for (stations, half) in [
            (vec![0.0], 0.25),
            (vec![0.0, 0.0], 0.25),
            (vec![1.0, 0.0], 0.25),
            (vec![0.0, f64::NAN], 0.25),
            (vec![0.0, 1.0], 0.0),
            (vec![0.0, 1.0], 1.0),
            (vec![0.0, 1.0], f64::NAN),
        ] {
            assert!(ProceduralSweep::create(&line, motion.clone(), &stations, half).is_err());
        }
        assert!(ScrewMotion::new(Point3::ZERO, Vec3::ZERO, 1.0, None).is_err());
        assert!(ScrewMotion::new(Point3::ZERO, z, 0.0, None).is_err());
        assert!(ScrewMotion::new(Point3::ZERO, z, f64::INFINITY, None).is_err());
        assert!(RadialLaw::new(Vec3::X, vec![], vec![]).is_err());
        assert!(RadialLaw::new(Vec3::X, vec![1.0, 0.0], vec![0.0, 0.0]).is_err());
        assert!(RadialLaw::new(Vec3::X, vec![0.0], vec![f64::NAN]).is_err());
        assert!(RadialLaw::new(Vec3::ZERO, vec![0.0], vec![0.0]).is_err());
    }
}
