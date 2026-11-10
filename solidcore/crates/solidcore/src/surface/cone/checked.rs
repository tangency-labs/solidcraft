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

impl Cone {
    /// A uniform accuracy requirement for a successful native cylinder point.
    /// This is enforced on every returned point, never assumed of scalar libm.
    /// The exact normalized polar curve is the reference; the metadata's distance
    /// from an ideal rational arc is a separate construction term.
    pub(crate) fn cylinder_runtime_allowance(&self, k: &Kernel) -> Result<Option<f64>> {
        let frame = self.canonical();
        let angular = self.rev().step_uv_domain().u_interval();
        let Some(polar) = self.rev().polar_curve() else {
            return Ok(None);
        };
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
            return Ok(None);
        }
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

    fn cylinder_polar_span(&self, u: f64, side: Side) -> Result<PolarSpan> {
        let polar = self
            .rev()
            .polar_curve()
            .ok_or_else(|| bad("missing cylinder polar curve"))?;
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
        let (Some(&lo), Some(&hi)) = (canonical.knots.first(), canonical.knots.last()) else {
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
        let index = canonical
            .knots
            .partition_point(|&x| if side == Side::Right { x < u } else { x <= u })
            .saturating_sub(1)
            .min(canonical.knots.len() - 2);
        let (Some(&a), Some(&b)) = (canonical.knots.get(index), canonical.knots.get(index + 1))
        else {
            return Err(bad("missing polar span"));
        };
        if !a.is_finite() || !b.is_finite() || a >= b {
            return Err(bad("invalid polar span"));
        }
        let raw = controls
            .get(2 * index..2 * index + 3)
            .ok_or_else(|| bad("missing polar controls"))?;
        let width = Ivl::from(b) - Ivl::from(a);
        Ok(PolarSpan {
            coefficients: polar_coefficients(polar, raw)?,
            parameter: (Ivl::from(u) - Ivl::from(a)) / width,
        })
    }

    fn cylinder_point_enclosure(&self, k: &Kernel, uv: Point2) -> Result<Box3> {
        if !uv.x.is_finite() || !uv.y.is_finite() {
            return Err(bad("cylinder parameters must be finite"));
        }
        let frame = self.canonical();
        let (u, v) = if frame.swap_uv {
            (uv.y, uv.x)
        } else {
            (uv.x, uv.y)
        };
        let span = self.cylinder_polar_span(u, Side::Left)?;
        self.cylinder_span_enclosure(k, span, Ivl::from(v))
    }

    fn cylinder_span_enclosure(&self, k: &Kernel, span: PolarSpan, v: Ivl) -> Result<Box3> {
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
        let mut height = HeightMap::new(k, self)?.range(v)?;
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
        let Some(cap) = self.cylinder_runtime_allowance(k)? else {
            return Ok(());
        };
        let reference = self.cylinder_point_enclosure(k, uv)?;
        verify_point(reference, candidate, cap)
    }

    pub(crate) fn verify_cylinder_step_point(&self, uv: Point2, candidate: Point3) -> Result<()> {
        let frame = self.canonical();
        if frame.semi_angle_deg != 0.0 || frame.radius <= 0.0 {
            return Ok(());
        }
        let theta = Ivl::from(uv.x) * Ivl::checked(std::f64::consts::PI, std::f64::consts::PI)
            / Ivl::from(180.0);
        let (sin, cos) = sin_cos_certified(theta);
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
        Ok(())
    }
}
