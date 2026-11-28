//! A screw-swept wall's definition as plain data, both ways (not upstream): read off a face's
//! surface for its record ([`face_sweep`]), and given back to the face of a body built from a
//! record as the wall's label ([`adopt_sweeps`], [`ProceduralSweep::is_label`]): the kernel goes
//! on working on the net, exactly as it did on a wall read back as a plain B-spline, and writes
//! the definition back with it. Its numbers are the kernel's stored
//! values, bit for bit: the profile's control points as stored (weighted when rational) and its
//! full knot vector, so a wall written, read back and adopted is the wall it was written from.
//! `solidcore-convert` copies these fields to and from `solidcore_record::brep::sweep`.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::{Motion, ProceduralSweep, RadialLaw, ScrewMotion, SweepMap};
use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::surface::procedural_blend::convert::{curve_of, curve_record};
use crate::surface::procedural_blend::record::NurbsCurveRecord;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::{Brep, FaceId};

const OP_ADOPT: &str = "surface::procedural_sweep::record::adopt_sweeps";

/// A radial law's numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct LawRecord {
    /// The unit direction, square to the axis.
    pub direction: [f64; 3],
    /// The angles, radians of turn, strictly ascending.
    pub angles: Vec<f64>,
    /// The offset at each angle.
    pub offsets: Vec<f64>,
}

/// A screw-swept wall's definition: [`ProceduralSweep`] with a screw motion, field for field.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepRecord {
    /// The profile, as stored.
    pub profile: NurbsCurveRecord,
    /// A point of the axis.
    pub origin: [f64; 3],
    /// The unit axis.
    pub axis: [f64; 3],
    /// The rise per turn, signed (positive is right-handed about the axis).
    pub pitch: f64,
    /// The radial law, if any.
    pub law: Option<LawRecord>,
    /// The net's stations across the turns, radians.
    pub stations: Vec<f64>,
    /// Each span's half angle.
    pub halves: Vec<f64>,
    /// The map's terms `(a, b, swapped)` ([`ProceduralSweep::definition_map`]).
    pub map: ([f64; 2], [f64; 2], bool),
    /// The bound the kernel held.
    pub bound: f64,
}

fn arr(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// The definition of the wall `surface` is the net of, live or as its label, when it is a
/// screw-swept wall's ([`Surface::kept_sweep`] with a screw motion); `None` for any other surface
/// (a path sweep keeps its walls as their nets in a record).
///
/// # Errors
///
/// Those of reading the profile.
pub fn face_sweep(k: &Kernel, surface: &Surface) -> Result<Option<SweepRecord>> {
    let Some(d) = surface.kept_sweep() else {
        return Ok(None);
    };
    // A label is checked against its net before the record names the net by it: an edit of
    // the net in place that kept the label (a span added at an end) leaves the net alone.
    if d.is_label() && !agrees(k, surface, d).unwrap_or(false) {
        return Ok(None);
    }
    let Motion::Screw(m) = d.motion() else {
        return Ok(None);
    };
    Ok(Some(SweepRecord {
        profile: curve_record(d.profile())?,
        origin: arr(m.origin()),
        axis: [m.axis().x, m.axis().y, m.axis().z],
        pitch: m.pitch(),
        law: m.law().map(|l| LawRecord {
            direction: [l.direction().x, l.direction().y, l.direction().z],
            angles: l.angles().to_vec(),
            offsets: l.offsets().to_vec(),
        }),
        stations: d.stations().to_vec(),
        halves: d.half_angles().to_vec(),
        map: d.definition_map(),
        bound: d.bound(),
    }))
}

/// The definition a record holds, rebuilt as a wall's label ([`ProceduralSweep::is_label`]): the
/// same checks as [`ProceduralSweep::create`]'s, its bound the record's (a label's is never read
/// as one, so it is not proved again).
///
/// # Errors
///
/// `InvalidInput` for a map whose terms are not finite or whose scales are nought, or a bound
/// that is not finite and nonnegative; those of rebuilding the profile, the motion and the law,
/// and of [`ProceduralSweep::create`]'s checks.
pub fn sweep_of(r: &SweepRecord) -> Result<ProceduralSweep> {
    let law = match &r.law {
        Some(l) => Some(RadialLaw::new(
            Vec3::new(l.direction[0], l.direction[1], l.direction[2]),
            l.angles.clone(),
            l.offsets.clone(),
        )?),
        None => None,
    };
    let motion = ScrewMotion::new(
        Point3::new(r.origin[0], r.origin[1], r.origin[2]),
        Vec3::new(r.axis[0], r.axis[1], r.axis[2]),
        r.pitch,
        law,
    )?;
    let (a, b, swapped) = r.map;
    let finite = a.iter().chain(&b).all(|x| x.is_finite()) && b.iter().all(|&x| x != 0.0);
    if !finite {
        return Err(SolidError::invalid_input(
            OP_ADOPT,
            "a sweep's map has finite terms and non-zero scales",
        ));
    }
    if !(r.bound.is_finite() && r.bound >= 0.0) {
        return Err(SolidError::invalid_input(
            OP_ADOPT,
            "a sweep's bound is finite and nonnegative",
        ));
    }
    let mut sweep =
        ProceduralSweep::checked(&curve_of(&r.profile)?, motion, &r.stations, &r.halves)?;
    sweep.map = SweepMap { a, b, swapped };
    sweep.bound = r.bound;
    sweep.label = true;
    Ok(sweep)
}

/// Whether the net `net` answers to `sweep` where both are known to meet: at the corners and the
/// middle of the net's domain, the net and the definition are within the bound (with the
/// coordinates' rounding). A net of another wall fails it; a check, not a proof, that keeps a
/// record's definition from riding on a net it does not describe.
fn agrees(k: &Kernel, net: &Surface, sweep: &ProceduralSweep) -> Result<bool> {
    let SurfaceKind::BSpline(spline) = net.kind() else {
        return Ok(false);
    };
    let d = spline.natural_uv_domain();
    let (u, v) = (d.u_interval(), d.v_interval());
    let mid = |i: crate::geom::extent1d::Extent1d| 0.5 * (i.min() + i.max());
    for uv in [
        Point2::new(u.min(), v.min()),
        Point2::new(u.max(), v.min()),
        Point2::new(u.min(), v.max()),
        Point2::new(u.max(), v.max()),
        Point2::new(mid(u), mid(v)),
    ] {
        let b = net.evaluate_point(k, uv)?;
        let s = sweep.evaluate_definition_point(uv)?;
        let scale = 1.0 + b.x.abs().max(b.y.abs()).max(b.z.abs());
        if !((b - s).length() <= sweep.bound + 1e-12 * scale) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Give the faces `faces` (each by its id in `b`) the definitions their record holds: each
/// face's surface, a B-spline net, carries its wall's definition again as its label
/// ([`sweep_of`]), which no evaluation reads. A face
/// whose surface is not a plain B-spline, or whose net does not agree with the definition (a
/// record whose definition belongs to another net), keeps its net alone: the face is valid as
/// it is, and is never refused.
///
/// # Errors
///
/// `InvalidInput` for a face `b` does not have.
pub fn adopt_sweeps(k: &Kernel, b: &mut Brep, faces: &[(FaceId, SweepRecord)]) -> Result<()> {
    for (face, record) in faces {
        let face = *face;
        // A definition that does not rebuild, or a net it does not describe, leaves the face on
        // its net, which is the face's geometry either way.
        let Ok(sweep) = sweep_of(record) else {
            continue;
        };
        let takes = match b.face(face)?.surface.as_ref() {
            Some(surface) => {
                matches!(surface.kind(), SurfaceKind::BSpline(_))
                    && surface.procedural_definition().is_none()
                    && surface.kept_sweep().is_none()
                    && agrees(k, surface, &sweep).unwrap_or(false)
            }
            None => false,
        };
        // The label goes on the face's own surface, in place: the net, its caches and its
        // trims stay exactly as they are (a copy of the surface would refit its knots).
        if takes {
            if let Some(surface) = b.face_mut(face)?.surface.as_mut() {
                surface.set_sweep_definition(sweep);
            }
        }
    }
    Ok(())
}
