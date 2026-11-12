//! Checked-success bounds for full-circle cylinder evaluation from the literal
//! homogeneous polar chart and affine height map. The independent outward-rounded
//! enclosure checks the native converter/STEP result and rejects unsound seam snaps.
//! Unsupported charts retain their existing behavior and cannot obtain this cap.

use super::Cone;
use crate::blend::fillet_section::interval::{sin_cos_certified, Ivl};
use crate::curve::ellipse::Ellipse;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::knots::Side;
use crate::nurbs::math::{gamma, CPoint, UNIT};
use std::sync::{Arc, Mutex, PoisonError};

const OP: &str = "surface::cone::checked";
type PolarCoefficient = ([Ivl; 3], Ivl);
type Box3 = [Ivl; 3];
#[derive(Clone, Copy)]
struct PolarSpan {
    coefficients: [PolarCoefficient; 3],
    parameter: Ivl,
}
fn verify_point(reference: Box3, candidate: Point3, cap: f64) -> Result<()> {
    let [a, b, c] = boxed(candidate);
    let [x, y, z] = reference;
    let error = norm([a - x, b - y, c - z]).hi;
    if !error.is_finite() || error > cap {
        return Err(bad("cylinder point exceeds its checked runtime allowance"));
    }
    Ok(())
}
fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}
fn boxed(v: Vec3) -> Box3 {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}
fn add(a: Box3, b: Box3) -> Box3 {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x + u, y + v, z + w]
}
fn scale(a: Box3, b: Ivl) -> Box3 {
    a.map(|v| v * b)
}
fn square(a: Ivl) -> Ivl {
    let lo = Ivl::from(a.mig()) * Ivl::from(a.mig());
    let hi = Ivl::from(a.mag()) * Ivl::from(a.mag());
    Ivl::new(lo.lo.max(0.0), hi.hi)
}
fn norm(a: Box3) -> Ivl {
    let [x, y, z] = a.map(square);
    let sum = x + y + z;
    Ivl::new(sum.lo.max(0.0), sum.hi).sqrt()
}
fn finite(a: Box3) -> bool {
    a.iter().all(|x| x.lo.is_finite() && x.hi.is_finite())
}

/// Coefficients enclosing both the exact affine height map and every rounding
/// of `Cone::convert_uv_from_nurbs_to_step`'s native-height arithmetic. Coefficients
/// may vary within these intervals with the input; the enclosure is pointwise.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HeightMap {
    origin: Ivl,
    scale: Ivl,
    intermediates: [(Ivl, Ivl); 4],
}

impl HeightMap {
    pub(crate) fn new(k: &Kernel, cone: &Cone) -> Result<Self> {
        let rev = cone.rev();
        let step = rev.step_uv_domain().v_interval();
        let domain = rev.base().natural_uv_domain();
        let native = if rev.swap_uv() {
            domain.u_interval()
        } else {
            domain.v_interval()
        };
        let inside = cone.inside_out();
        // This is the existing converter's branch predicate, including its
        // established unbounded inside-out behavior; no new option is introduced.
        let linear = rev.is_bounded()?
            || (k.fixes.cone_unbounded_parameterized_as_nurb
                && inside
                && (native.min().to_bits() != (-step.max()).to_bits()
                    || native.max().to_bits() != (-step.min()).to_bits()));
        if !linear {
            return Ok(Self {
                origin: Ivl::from(0.0),
                scale: Ivl::from(if inside { -1.0 } else { 1.0 }),
                intermediates: [(Ivl::from(0.0), Ivl::from(if inside { -1.0 } else { 1.0 })); 4],
            });
        }
        let width = Ivl::from(native.max()) - Ivl::from(native.min());
        let step_width = Ivl::from(step.max()) - Ivl::from(step.min());
        if width.lo <= 0.0
            || !width.hi.is_finite()
            || !step_width.lo.is_finite()
            || !step_width.hi.is_finite()
        {
            return Err(bad(
                "cylinder height map has an unproved finite denominator",
            ));
        }
        let start = if inside { native.max() } else { native.min() };
        let denominator = if inside {
            Ivl::from(0.0) - width
        } else {
            width
        };
        // fl(z)=z(1+delta)+eta, |delta|<=UNIT, |eta|<=smallest_subnormal.
        // Lift each of the four native operations to affine interval coefficients;
        // this includes cancellation and underflow without a scale heuristic.
        let factor = Ivl::from(1.0) + Ivl::new(-UNIT, UNIT);
        let tiny = f64::from_bits(1);
        let round = |(a, b): (Ivl, Ivl)| (a * factor + Ivl::new(-tiny, tiny), b * factor);
        let subtract = round((Ivl::from(-start), Ivl::from(1.0)));
        let divide = round((subtract.0 / denominator, subtract.1 / denominator));
        let multiply = round((divide.0 * step_width, divide.1 * step_width));
        let add = round((multiply.0 + Ivl::from(step.min()), multiply.1));
        let map = Self {
            origin: add.0,
            scale: add.1,
            intermediates: [subtract, divide, multiply, add],
        };
        if ![map.origin.lo, map.origin.hi, map.scale.lo, map.scale.hi]
            .iter()
            .all(|v| v.is_finite())
            || map.scale.mig() <= 0.0
        {
            return Err(bad(
                "cylinder height coefficients are not proved finite and invertible",
            ));
        }
        Ok(map)
    }

    pub(crate) fn range(self, parameter: Ivl) -> Result<Ivl> {
        let value = self.origin + self.scale * parameter;
        let finite_intermediates = self.intermediates.into_iter().all(|(a, b)| {
            let range = a + b * parameter;
            range.lo.is_finite() && range.hi.is_finite()
        });
        if !finite_intermediates
            || parameter.lo > parameter.hi
            || ![parameter.lo, parameter.hi, value.lo, value.hi]
                .iter()
                .all(|v| v.is_finite())
        {
            return Err(bad("cylinder height range is not finite"));
        }
        Ok(value)
    }
}

/// Project raw homogeneous polar controls into the coordinates used by ellipse
/// STEP inversion. Division by the two positive canonical radii leaves atan2
/// unchanged. No Euclidean control-point reconstruction or sampling is used.
pub(crate) fn polar_coefficients(
    polar: &Ellipse,
    controls: &[CPoint],
) -> Result<[PolarCoefficient; 3]> {
    let [p0, p1, p2] = controls else {
        return Err(bad("polar certificate needs a quadratic span"));
    };
    let frame = polar.canonical();
    if !frame.radius_x.is_finite()
        || frame.radius_x <= 0.0
        || !frame.radius_y.is_finite()
        || frame.radius_y <= 0.0
    {
        return Err(bad("polar certificate needs finite positive radii"));
    }
    let rational = polar.base().is_rational()?;
    let origin = frame.position.origin();
    let x = frame.position.x_axis();
    let y = frame.position.y_axis();
    Ok([p0, p1, p2].map(|p| {
        let w = Ivl::from(if rational { p.w } else { 1.0 });
        let r = [
            Ivl::from(p.x) - Ivl::from(origin.x) * w,
            Ivl::from(p.y) - Ivl::from(origin.y) * w,
            Ivl::from(p.z) - Ivl::from(origin.z) * w,
        ];
        let [rx, ry, rz] = r;
        let dot = |v: Vec3| rx * Ivl::from(v.x) + ry * Ivl::from(v.y) + rz * Ivl::from(v.z);
        (
            [
                dot(x) / Ivl::from(frame.radius_x),
                dot(y) / Ivl::from(frame.radius_y),
                Ivl::from(0.0),
            ],
            w,
        )
    }))
}

/// The bits of every surface value the checked cylinder data is derived from (the
/// frame, the STEP and NURB domains, the height map's branch, the polar chart's frame,
/// degree, knots and controls); see [`Cone::checked_memo`].
const KEY_SCALARS: usize = 36;

/// A cylinder point that passed its check: the bits of its parameters and candidate, and
/// the two out-of-bounds settings.
type PassedPoint = ([u64; 5], [bool; 2]);

/// Slots of the passed cylinder points ([`MemoSlots::runtime_passed`]).
const PASSED_SLOTS: usize = 512;

/// The slot of a passed point, from its bits alone (a multiplicative mix, deterministic).
fn passed_slot((point, bounds): &PassedPoint) -> usize {
    let h = point
        .iter()
        .chain(&[u64::from(bounds[0]) | (u64::from(bounds[1]) << 1)])
        .fold(0x243f_6a88_85a3_08d3_u64, |h, &b| {
            (h ^ b).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29)
        });
    usize::try_from(h >> 32).map_or(0, |slot| slot & (PASSED_SLOTS - 1))
}

/// The STEP angles whose certified sine and cosine [`CheckedMemo`] keeps.
const ANGLES: usize = 4;

/// What the checked cylinder evaluation derives once rather than at every point:
/// - what [`Cone::verify_cylinder_runtime_point`] derives from the surface alone (the
///   runtime allowance, the height map, the polar chart's knots and every span's polar
///   coefficients), kept with the exact bits it was derived from and used only while the
///   surface still has those bits, so a hit returns what a fresh derivation would. Only a
///   successful derivation is kept; any other case derives afresh every time;
/// - the certified sine and cosine of the last few STEP angles
///   [`Cone::verify_cylinder_step_point`] took, by the angle's bits (a function of the
///   angle alone: a solve's grid evaluates one angle at many heights);
/// - the last point each of the two checks passed, with every bit its verdict is a
///   function of (a point's value and its derivatives are evaluated, and checked, one
///   after the other at the same parameters).
///
/// Boxed, so a cone stays the size it was.
#[derive(Default)]
pub(crate) struct CheckedMemo(Box<Mutex<MemoSlots>>);

#[derive(Clone, Default)]
struct MemoSlots {
    data: Option<Arc<CheckedData>>,
    angles: [Option<(u64, Ivl, Ivl)>; ANGLES],
    next_angle: usize,
    /// The data, the NURB parameters and the point, and the two out-of-bounds settings
    /// of the last point [`Cone::verify_cylinder_runtime_point`] passed.
    /// The points [`Cone::verify_cylinder_runtime_point`] passed with the checked data
    /// they were checked against, one per slot ([`passed_slot`]).
    runtime_passed: Option<(Arc<CheckedData>, Vec<Option<PassedPoint>>)>,
    /// The frame, the STEP parameters and the point of the last point
    /// [`Cone::verify_cylinder_step_point`] passed.
    step_passed: Option<[u64; 15]>,
}

impl CheckedMemo {
    fn slots(&self) -> std::sync::MutexGuard<'_, MemoSlots> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// [`sin_cos_certified`] of `degrees` in radians, enclosed.
    fn step_sin_cos(&self, degrees: f64) -> (Ivl, Ivl) {
        let bits = degrees.to_bits();
        let mut slots = self.slots();
        if let Some((_, sin, cos)) = slots.angles.iter().flatten().find(|(b, ..)| *b == bits) {
            return (*sin, *cos);
        }
        let theta = Ivl::from(degrees) * Ivl::checked(std::f64::consts::PI, std::f64::consts::PI)
            / Ivl::from(180.0);
        let (sin, cos) = sin_cos_certified(theta);
        let next = slots.next_angle;
        if let Some(slot) = slots.angles.get_mut(next) {
            *slot = Some((bits, sin, cos));
        }
        slots.next_angle = if next + 1 < ANGLES { next + 1 } else { 0 };
        (sin, cos)
    }
}

impl Clone for CheckedMemo {
    /// A copy keeps the derived data and angles; its passed points start empty (a copy
    /// meets few of them, and which are kept never changes an answer), so copying a
    /// cylinder never copies the table.
    fn clone(&self) -> Self {
        let slots = self.slots();
        Self(Box::new(Mutex::new(MemoSlots {
            data: slots.data.clone(),
            angles: slots.angles,
            next_angle: slots.next_angle,
            runtime_passed: None,
            step_passed: slots.step_passed,
        })))
    }
}

impl std::fmt::Debug for CheckedMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CheckedMemo")
    }
}

struct CheckedData {
    scalars: [u64; KEY_SCALARS],
    degree: usize,
    knot_bits: Vec<u64>,
    control_bits: Vec<[u64; 4]>,
    cap: f64,
    height: HeightMap,
    knots: Vec<f64>,
    coefficients: Vec<[PolarCoefficient; 3]>,
}

impl CheckedData {
    fn derived_from(
        &self,
        scalars: &[u64; KEY_SCALARS],
        degree: usize,
        knots: &[f64],
        controls: &[CPoint],
    ) -> bool {
        self.scalars == *scalars
            && self.degree == degree
            && self.knot_bits.len() == knots.len()
            && self
                .knot_bits
                .iter()
                .zip(knots)
                .all(|(a, b)| *a == b.to_bits())
            && self.control_bits.len() == controls.len()
            && self
                .control_bits
                .iter()
                .zip(controls)
                .all(|(a, b)| *a == control_bits(b))
    }
}

fn control_bits(p: &CPoint) -> [u64; 4] {
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), p.w.to_bits()]
}

/// The span of the canonical polar `knots` holding `u` (on `side`, clamped to the
/// knots unless the chart evaluates out of bounds) and `u`'s parameter in it.
fn polar_span_at(polar: &Ellipse, knots: &[f64], u: f64, side: Side) -> Result<(usize, Ivl)> {
    let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
        return Err(bad("missing polar interval"));
    };
    if !lo.is_finite() || !hi.is_finite() || lo >= hi {
        return Err(bad("invalid polar interval"));
    }
    // Matches the B evaluator's outer-domain rule. Endpoint/seam replacement
    // is not assumed harmless: the final candidate must pass the point cap.
    let u = if polar.base().is_out_of_bounds_enabled() {
        u
    } else {
        crate::math::clamp_between(u, lo, hi)
    };
    let index = knots
        .partition_point(|&x| if side == Side::Right { x < u } else { x <= u })
        .saturating_sub(1)
        .min(knots.len().saturating_sub(2));
    let (Some(&a), Some(&b)) = (knots.get(index), knots.get(index + 1)) else {
        return Err(bad("missing polar span"));
    };
    if !a.is_finite() || !b.is_finite() || a >= b {
        return Err(bad("invalid polar span"));
    }
    let width = Ivl::from(b) - Ivl::from(a);
    Ok((index, (Ivl::from(u) - Ivl::from(a)) / width))
}

impl Cone {
    /// The polar chart of a full-circle cylinder: the cases
    /// [`Self::cylinder_runtime_allowance`] checks before it reads the chart's
    /// knots (`None` there is no allowance).
    fn full_circle_polar(&self) -> Option<&Ellipse> {
        let frame = self.canonical();
        let angular = self.rev().step_uv_domain().u_interval();
        let polar = self.rev().polar_curve()?;
        let polar_domain = polar.canonical().domain;
        // Full-circle chart: periodic choices have identical geometric meaning.
        // Partial arcs need a separate proof of their angular endpoint clamp.
        if frame.semi_angle_deg != 0.0
            || angular.min() != 0.0
            || angular.max() != 360.0
            || polar_domain.min() != 0.0
            || polar_domain.max() != 360.0
            || !frame.radius.is_finite()
            || frame.radius <= 0.0
        {
            return None;
        }
        Some(polar)
    }

    /// The checked data of this surface, derived once for its current value (#13891:
    /// every cylinder evaluation verifies its point, and deriving the allowance, the
    /// height map and the polar span anew each time made a long intersection several
    /// times slower). `None` when the surface has no polar chart, its generator has no
    /// bounds to read, or the derivation does not succeed: the caller then derives
    /// afresh, as before, with the same results and errors.
    fn checked_memo(&self, k: &Kernel, polar: &Ellipse) -> Option<Arc<CheckedData>> {
        let bounded = self.rev().is_bounded().ok()?;
        let degree = polar.base().degree().ok()?;
        let knots = polar.base().knot_values().ok()?;
        let controls = polar.base().control_points().ok()?;
        let frame = self.canonical();
        let polar_frame = polar.canonical();
        let step = self.rev().step_uv_domain();
        let native = self.rev().base().natural_uv_domain();
        let placement = |a: &crate::geom::axis2_placement::Axis2Placement| {
            let (o, x, y) = (a.origin(), a.x_axis(), a.y_axis());
            [o.x, o.y, o.z, x.x, x.y, x.z, y.x, y.y, y.z].map(f64::to_bits)
        };
        let parts = placement(&frame.origin)
            .into_iter()
            .chain([frame.radius, frame.semi_angle_deg].map(f64::to_bits))
            .chain([
                u64::from(frame.swap_uv),
                u64::from(frame.inside_out),
                u64::from(bounded),
                u64::from(k.fixes.cone_unbounded_parameterized_as_nurb),
            ])
            .chain([step.min().x, step.min().y, step.max().x, step.max().y].map(f64::to_bits))
            .chain(
                [
                    native.min().x,
                    native.min().y,
                    native.max().x,
                    native.max().y,
                ]
                .map(f64::to_bits),
            )
            .chain(placement(&polar_frame.position))
            .chain(
                [
                    polar_frame.radius_x,
                    polar_frame.radius_y,
                    polar_frame.domain.min(),
                    polar_frame.domain.max(),
                ]
                .map(f64::to_bits),
            );
        let mut scalars = [0_u64; KEY_SCALARS];
        let mut filled = 0;
        for (slot, bits) in scalars.iter_mut().zip(parts) {
            *slot = bits;
            filled += 1;
        }
        if filled != KEY_SCALARS {
            return None;
        }
        let mut slots = self.checked.slots();
        if let Some(data) = slots.data.as_ref() {
            if data.derived_from(&scalars, degree, knots, controls) {
                return Some(Arc::clone(data));
            }
        }
        let cap = self.cylinder_runtime_allowance(k).ok()??;
        let height = HeightMap::new(k, self).ok()?;
        let canonical = polar.base().canonical().ok()?;
        let spans = canonical.knots.len().checked_sub(1)?;
        let coefficients = (0..spans)
            .map(|index| {
                controls
                    .get(2 * index..2 * index + 3)
                    .ok_or_else(|| bad("missing polar controls"))
                    .and_then(|raw| polar_coefficients(polar, raw))
            })
            .collect::<Result<Vec<_>>>()
            .ok()?;
        let data = Arc::new(CheckedData {
            scalars,
            degree,
            knot_bits: knots.iter().map(|x| x.to_bits()).collect(),
            control_bits: controls.iter().map(control_bits).collect(),
            cap,
            height,
            knots: canonical.knots,
            coefficients,
        });
        slots.data = Some(Arc::clone(&data));
        Some(data)
    }

    /// A uniform accuracy requirement for a successful native cylinder point.
    /// This is enforced on every returned point, never assumed of scalar libm.
    /// The exact normalized polar curve is the reference; the metadata's distance
    /// from an ideal rational arc is a separate construction term.
    pub(crate) fn cylinder_runtime_allowance(&self, k: &Kernel) -> Result<Option<f64>> {
        let frame = self.canonical();
        let Some(polar) = self.full_circle_polar() else {
            return Ok(None);
        };
        let canonical = polar.base().canonical()?;
        if canonical.degree != 2
            || canonical.knots.len() < 2
            || canonical.knots.len() > 24_001
            || canonical.control_points.len() != 2 * canonical.knots.len() - 1
            || canonical.knot_multiplicities.len() != canonical.knots.len()
            || canonical
                .knot_multiplicities
                .iter()
                .enumerate()
                .any(|(i, &m)| {
                    m != if i == 0 || i + 1 == canonical.knots.len() {
                        3
                    } else {
                        2
                    }
                })
        {
            return Ok(None);
        }
        let domain = self.rev().base().natural_uv_domain();
        let native = if frame.swap_uv {
            domain.u_interval()
        } else {
            domain.v_interval()
        };
        let height = HeightMap::new(k, self)?.range(Ivl::new(native.min(), native.max()))?;
        let size = Ivl::from(1.0)
            + norm(boxed(frame.origin.origin()))
            + Ivl::from(frame.radius)
                * (norm(boxed(frame.origin.x_axis())) + norm(boxed(frame.origin.y_axis())))
            + norm(boxed(frame.origin.z_axis())) * (Ivl::from(1.0) + Ivl::from(height.mag()));
        let g = gamma(256);
        let cap = (Ivl::checked(g, g) * size).hi;
        if !cap.is_finite() || cap <= 0.0 {
            return Err(bad("cylinder runtime accuracy requirement is not finite"));
        }
        Ok(Some(cap))
    }

    fn cylinder_polar_span(
        &self,
        u: f64,
        side: Side,
        memo: Option<&CheckedData>,
    ) -> Result<PolarSpan> {
        let polar = self
            .rev()
            .polar_curve()
            .ok_or_else(|| bad("missing cylinder polar curve"))?;
        if let Some(memo) = memo {
            let (index, parameter) = polar_span_at(polar, &memo.knots, u, side)?;
            return Ok(PolarSpan {
                coefficients: *memo
                    .coefficients
                    .get(index)
                    .ok_or_else(|| bad("missing polar controls"))?,
                parameter,
            });
        }
        let canonical = polar.base().canonical()?;
        let controls = polar.base().control_points()?;
        if canonical.degree != 2
            || canonical.knots.len() < 2
            || canonical.knots.len() > 24_001
            || controls.len() != 2 * canonical.knots.len() - 1
            || canonical
                .knot_multiplicities
                .iter()
                .enumerate()
                .any(|(i, &m)| {
                    m != if i == 0 || i + 1 == canonical.knots.len() {
                        3
                    } else {
                        2
                    }
                })
        {
            return Err(bad("checked cylinder needs quadratic Bezier polar spans"));
        }
        let (index, parameter) = polar_span_at(polar, &canonical.knots, u, side)?;
        let raw = controls
            .get(2 * index..2 * index + 3)
            .ok_or_else(|| bad("missing polar controls"))?;
        Ok(PolarSpan {
            coefficients: polar_coefficients(polar, raw)?,
            parameter,
        })
    }

    fn cylinder_point_enclosure(
        &self,
        k: &Kernel,
        uv: Point2,
        memo: Option<&CheckedData>,
    ) -> Result<Box3> {
        if !uv.x.is_finite() || !uv.y.is_finite() {
            return Err(bad("cylinder parameters must be finite"));
        }
        let frame = self.canonical();
        let (u, v) = if frame.swap_uv {
            (uv.y, uv.x)
        } else {
            (uv.x, uv.y)
        };
        let span = self.cylinder_polar_span(u, Side::Left, memo)?;
        self.cylinder_span_enclosure(k, span, Ivl::from(v), memo)
    }

    fn cylinder_span_enclosure(
        &self,
        k: &Kernel,
        span: PolarSpan,
        v: Ivl,
        memo: Option<&CheckedData>,
    ) -> Result<Box3> {
        let frame = self.canonical();
        let PolarSpan {
            coefficients: coeff,
            parameter: t,
            ..
        } = span;
        let r = Ivl::from(1.0) - t;
        let basis = [r * r, Ivl::from(2.0) * r * t, t * t];
        let mut h = [Ivl::from(0.0); 3];
        let mut weight = Ivl::from(0.0);
        for ((point, w), basis) in coeff.into_iter().zip(basis) {
            h = add(h, scale(point, basis));
            weight = weight + w * basis;
        }
        if weight.lo <= 0.0 {
            return Err(bad("polar denominator is not proved positive"));
        }
        let [x, y, _] = scale(h, Ivl::from(1.0) / weight);
        let length = (square(x) + square(y)).sqrt();
        if length.lo <= 0.0 || !length.hi.is_finite() {
            return Err(bad("polar projection is not proved away from the origin"));
        }
        let map = match memo {
            Some(memo) => memo.height,
            None => HeightMap::new(k, self)?,
        };
        let mut height = map.range(v)?;
        if !self.rev().is_out_of_bounds_enabled() {
            let domain = self.rev().step_uv_domain().v_interval();
            if !domain.min().is_finite() || !domain.max().is_finite() || domain.min() > domain.max()
            {
                return Err(bad("invalid cylinder height domain"));
            }
            height = Ivl::new(
                crate::math::clamp_between(height.lo, domain.min(), domain.max()),
                crate::math::clamp_between(height.hi, domain.min(), domain.max()),
            );
        }
        let radial = add(
            scale(boxed(frame.origin.x_axis()), x / length),
            scale(boxed(frame.origin.y_axis()), y / length),
        );
        let result = add(
            add(
                boxed(frame.origin.origin()),
                scale(radial, Ivl::from(frame.radius)),
            ),
            scale(boxed(frame.origin.z_axis()), height),
        );
        if !finite(result) {
            return Err(bad("cylinder point has no finite enclosure"));
        }
        Ok(result)
    }

    pub(crate) fn verify_cylinder_runtime_point(
        &self,
        k: &Kernel,
        uv: Point2,
        candidate: Point3,
    ) -> Result<()> {
        // (no allowance without a full-circle chart)
        let Some(polar) = self.full_circle_polar() else {
            return Ok(());
        };
        if let Some(memo) = self.checked_memo(k, polar) {
            // (the enclosure is a function of the data, the parameters and the two
            // out-of-bounds settings alone)
            let point = [uv.x, uv.y, candidate.x, candidate.y, candidate.z].map(f64::to_bits);
            let bounds = [
                self.rev().is_out_of_bounds_enabled(),
                polar.base().is_out_of_bounds_enabled(),
            ];
            let passed_point = (point, bounds);
            let slot = passed_slot(&passed_point);
            let passed = |slots: &MemoSlots| {
                slots.runtime_passed.as_ref().is_some_and(|(data, table)| {
                    Arc::ptr_eq(data, &memo)
                        && table.get(slot).copied().flatten() == Some(passed_point)
                })
            };
            if passed(&self.checked.slots()) {
                return Ok(());
            }
            let reference = self.cylinder_point_enclosure(k, uv, Some(&memo))?;
            verify_point(reference, candidate, memo.cap)?;
            let mut slots = self.checked.slots();
            // a table checked against other data is replaced, never mixed
            let fresh = !slots
                .runtime_passed
                .as_ref()
                .is_some_and(|(data, _)| Arc::ptr_eq(data, &memo));
            if fresh {
                slots.runtime_passed = Some((Arc::clone(&memo), vec![None; PASSED_SLOTS]));
            }
            if let Some(entry) = slots
                .runtime_passed
                .as_mut()
                .and_then(|(_, table)| table.get_mut(slot))
            {
                *entry = Some(passed_point);
            }
            return Ok(());
        }
        let Some(cap) = self.cylinder_runtime_allowance(k)? else {
            return Ok(());
        };
        let reference = self.cylinder_point_enclosure(k, uv, None)?;
        verify_point(reference, candidate, cap)
    }

    pub(crate) fn verify_cylinder_step_point(&self, uv: Point2, candidate: Point3) -> Result<()> {
        let frame = self.canonical();
        if frame.semi_angle_deg != 0.0 || frame.radius <= 0.0 {
            return Ok(());
        }
        // (the verdict is a function of the frame, the parameters and the point alone)
        let (o, x, y) = (
            frame.origin.origin(),
            frame.origin.x_axis(),
            frame.origin.y_axis(),
        );
        let key = [
            o.x,
            o.y,
            o.z,
            x.x,
            x.y,
            x.z,
            y.x,
            y.y,
            y.z,
            frame.radius,
            uv.x,
            uv.y,
            candidate.x,
            candidate.y,
            candidate.z,
        ]
        .map(f64::to_bits);
        if self.checked.slots().step_passed == Some(key) {
            return Ok(());
        }
        let (sin, cos) = self.checked.step_sin_cos(uv.x);
        let radial = add(
            scale(boxed(frame.origin.x_axis()), cos),
            scale(boxed(frame.origin.y_axis()), sin),
        );
        let reference = add(
            add(
                boxed(frame.origin.origin()),
                scale(radial, Ivl::from(frame.radius)),
            ),
            scale(boxed(frame.origin.z_axis()), Ivl::from(uv.y)),
        );
        let [a, b, c] = boxed(candidate);
        let [x, y, z] = reference;
        let error = norm([a - x, b - y, c - z]).hi;
        let size = Ivl::from(1.0)
            + norm(boxed(frame.origin.origin()))
            + Ivl::from(frame.radius)
                * (norm(boxed(frame.origin.x_axis())) + norm(boxed(frame.origin.y_axis())))
            + Ivl::from(uv.y.abs()) * norm(boxed(frame.origin.z_axis()));
        let g = gamma(256);
        let cap = (Ivl::checked(g, g) * size).hi;
        if !finite(reference) || !error.is_finite() || !cap.is_finite() || error > cap {
            return Err(bad(
                "cylinder STEP point exceeds its checked runtime allowance",
            ));
        }
        self.checked.slots().step_passed = Some(key);
        Ok(())
    }
}
