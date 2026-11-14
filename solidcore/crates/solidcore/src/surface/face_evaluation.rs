//! A face's surface evaluated in the face's own (u, v), the record's: its point, its unit normal,
//! and the point of it nearest a given point, found from a guess. The surface is the face's exact
//! one ([`FaceSurface`]: an analytic surface on its form, a blend by its definition, or its NURBS),
//! so a blend is evaluated on its definition S, not on its net. A round of the exact circle class
//! also has its torus read off its definition ([`circle_round_torus`]).
//!
//! The nearest point is a Newton iteration on the squared distance from the guess, each step the
//! Newton step where the distance's Hessian is positive and the Gauss-Newton step otherwise,
//! clamped to the face's parameter box, at most [`MAX_NEAREST_STEPS`] steps: it stops when a step
//! moves (u, v) by no more than their rounding, or once a step within the square root of the
//! rounding no longer halves the one before (Newton's quadratic steps have reached the noise of
//! the evaluations, which dither (u, v) in their last bits), and is refused (`NotConverged`) past
//! the cap.
//! Nothing is sampled, and the steps are a function of the inputs alone.
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_exact::{class_surface, classify_supports};
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::EvaluateFlags;
use crate::surface::face_surface::FaceSurface;
use crate::surface::implicit::{ClassSurface, Implicit};
use crate::surface::procedural_blend::exact::ExactClass;
use crate::surface::procedural_blend::BlendRef;

const OP_NORMAL: &str = "surface::face_evaluation::normal";
const OP_NEAREST: &str = "surface::face_evaluation::nearest";
const OP_TORUS: &str = "surface::face_evaluation::circle_round_torus";

/// The most Newton steps [`nearest`] takes.
pub const MAX_NEAREST_STEPS: u32 = 64;

/// The point of a face's surface nearest a given point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nearest {
    /// Its (u, v), the face's (the record's).
    pub uv: [f64; 2],
    /// The point.
    pub point: Point3,
    /// Its distance from the given point.
    pub distance: f64,
    /// The Newton steps taken.
    pub steps: u32,
}

/// The torus a round of the exact circle class is, from its definition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundTorus {
    /// Its centre.
    pub centre: Point3,
    /// Its unit axis.
    pub axis: Vec3,
    /// The major radius: the circle the ball's centre runs on.
    pub major: f64,
    /// The minor radius: the ball's.
    pub minor: f64,
}

/// The kernel's (u, v) of the face's: `[su u, sv v]` ([`FaceSurface::scale`]).
fn kernel_uv(f: &FaceSurface, uv: [f64; 2]) -> Point2 {
    Point2::new(f.scale[0] * uv[0], f.scale[1] * uv[1])
}

/// The partial `d[i][j]` of an evaluation.
fn partial(d: &[Vec<Vec3>], i: usize, j: usize, op: &'static str) -> Result<Vec3> {
    d.get(i)
        .and_then(|row| row.get(j))
        .copied()
        .ok_or_else(|| SolidError::invalid_input(op, "a short evaluation"))
}

/// The face's parameter box in its own (u, v), `[[u0, u1], [v0, v1]]`: the kernel's over the
/// scales.
#[must_use]
pub fn face_domain(f: &FaceSurface) -> [[f64; 2]; 2] {
    let d = &f.uv_domain;
    let side = |a: f64, b: f64, s: f64| {
        let (a, b) = (a / s, b / s);
        [a.min(b), a.max(b)]
    };
    [
        side(d.u_min(), d.u_max(), f.scale[0]),
        side(d.v_min(), d.v_max(), f.scale[1]),
    ]
}

/// The point of the face's surface at its (u, v).
///
/// # Errors
///
/// Those of the surface's evaluation.
pub fn point(k: &Kernel, f: &FaceSurface, uv: [f64; 2]) -> Result<Point3> {
    f.surface.evaluate_point(k, kernel_uv(f, uv))
}

/// The unit normal of the face's surface at its (u, v): `∂S/∂u × ∂S/∂v` in the face's parameters,
/// made unit.
///
/// # Errors
///
/// `Degenerate` where the partials are parallel (a pole, an apex); those of the evaluation.
pub fn normal(k: &Kernel, f: &FaceSurface, uv: [f64; 2]) -> Result<Vec3> {
    let d = f
        .surface
        .evaluate(k, kernel_uv(f, uv), 1, 1, EvaluateFlags::default())?;
    let n = partial(&d, 1, 0, OP_NORMAL)?.cross(partial(&d, 0, 1, OP_NORMAL)?);
    // the face's parameters are the kernel's over the scales: its normal turns with their sign
    let n = if f.scale[0] * f.scale[1] < 0.0 {
        n * -1.0
    } else {
        n
    };
    n.unitize().map_err(|_| SolidError::Degenerate {
        operation: OP_NORMAL,
        entity: None,
    })
}

/// The point of the face's surface nearest `p`, from the guess `guess` (the face's (u, v)), within
/// the face's parameter box (module docs).
///
/// # Errors
///
/// `NotConverged` past [`MAX_NEAREST_STEPS`]; `InvalidInput` for a point or guess not finite; those
/// of the evaluations.
pub fn nearest(k: &Kernel, f: &FaceSurface, p: Point3, guess: [f64; 2]) -> Result<Nearest> {
    let finite = [p.x, p.y, p.z, guess[0], guess[1]]
        .iter()
        .all(|x| x.is_finite());
    if !finite {
        return Err(SolidError::invalid_input(
            OP_NEAREST,
            "a point or guess not finite",
        ));
    }
    let flags = EvaluateFlags::default();
    let mut uv = f.uv_domain.clamp_point(kernel_uv(f, guess));
    // the last step's size, relative to (u, v)
    let mut last = f64::INFINITY;
    for step in 1..=MAX_NEAREST_STEPS {
        let d = f.surface.evaluate(k, uv, 2, 2, flags)?;
        let (s, su, sv) = (
            partial(&d, 0, 0, OP_NEAREST)?,
            partial(&d, 1, 0, OP_NEAREST)?,
            partial(&d, 0, 1, OP_NEAREST)?,
        );
        let (suu, suv, svv) = (
            partial(&d, 2, 0, OP_NEAREST)?,
            partial(&d, 1, 1, OP_NEAREST)?,
            partial(&d, 0, 2, OP_NEAREST)?,
        );
        let r = s - p;
        let (fu, fv) = (r.dot(su), r.dot(sv));
        let (guu, guv, gvv) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (a, b, c) = (guu + r.dot(suu), guv + r.dot(suv), gvv + r.dot(svv));
        let det = a * c - b * b;
        // Newton where the Hessian is positive, else Gauss-Newton (the first fundamental form)
        let (a, b, c, det) = if det > 0.0 && a > 0.0 && det.is_finite() {
            (a, b, c, det)
        } else {
            (guu, guv, gvv, guu * gvv - guv * guv)
        };
        let usable = det > 0.0 && det.is_finite();
        if !usable {
            return Err(SolidError::Degenerate {
                operation: OP_NEAREST,
                entity: None,
            });
        }
        let du = -(c * fu - b * fv) / det;
        let dv = -(a * fv - b * fu) / det;
        let next = f.uv_domain.clamp_point(Point2::new(uv.x + du, uv.y + dv));
        let size = ((next.x - uv.x).abs() / (1.0 + uv.x.abs()))
            .max((next.y - uv.y).abs() / (1.0 + uv.y.abs()));
        let settled =
            size <= 4.0 * f64::EPSILON || (size <= f64::EPSILON.sqrt() && size >= 0.5 * last);
        last = size;
        uv = next;
        if settled {
            let point = f.surface.evaluate_point(k, uv)?;
            return Ok(Nearest {
                uv: [uv.x / f.scale[0], uv.y / f.scale[1]],
                point,
                distance: (point - p).length(),
                steps: step,
            });
        }
    }
    Err(SolidError::NotConverged {
        operation: OP_NEAREST,
        iterations: MAX_NEAREST_STEPS,
    })
}

/// The torus a round of the exact circle class is (a plane square to a cylinder's axis, coaxial
/// cones, a sphere and a coaxial cone): the class's torus its supports give, with the net certified
/// on it within the tolerance the class was read in (`ExactRound::supports_distance`). `None` for
/// any other surface.
///
/// # Errors
///
/// `InvalidInput` for a circle-class round whose supports give no torus; those of classifying the
/// supports and of the certificate.
pub fn circle_round_torus(k: &Kernel, f: &FaceSurface) -> Result<Option<RoundTorus>> {
    let Some(BlendRef::Exact { round, net }) = f.surface.procedural_blend() else {
        return Ok(None);
    };
    if round.class() != ExactClass::Circle {
        return Ok(None);
    }
    let (Some(a), Some(b)) = (round.support(0), round.support(1)) else {
        return Err(SolidError::invalid_input(
            OP_TORUS,
            "an exact round without its two supports",
        ));
    };
    let blend = classify_supports(k, [a, b], round.inputs())?;
    let Some(ClassSurface::Implicit(Implicit::Torus {
        centre,
        axis,
        major,
        minor,
    })) = class_surface(&blend, round.leader(), round.leader_map())
    else {
        return Err(SolidError::invalid_input(
            OP_TORUS,
            "a round of the circle class whose supports give no torus",
        ));
    };
    round.supports_distance(k, net)?;
    Ok(Some(RoundTorus {
        centre,
        axis,
        major,
        minor,
    }))
}
