//! A procedural blend to its record and back (#2388): [`to_record`] writes the definition
//! and the net as plain data, [`from_record`] makes the blend from it exactly, and
//! [`rebuild`] also re-certifies the net against the definition
//! ([`ProceduralBlend::recertify`]), refusing a net that is off it. This is the record ↔
//! kernel conversion `docs/layers.md` gives `solidcore-ops`: it copies numbers and computes
//! nothing, and moves there when that crate exists.
//!
//! Not upstream. The fork carries a definition through its database only
//! (`SmProceduralBlend::WriteToDB`, [`ProceduralBlend::read_from_db`]).
//!
//! Deviations: none (no upstream counterpart). Recursion: nested supports are walked at
//! most [`BLEND_MAX_DEPTH`] deep, with an explicit depth refused past it.

use super::chamfer::{ChamferDefinition, ChamferSize};
use super::exact::{ExactClass, ExactRound};
use super::record::{
    BlendDefinitionRecord, ChamferRecord, ChamferSizeRecord, ClassInputsRecord, ExactClassRecord,
    ExactNetKindRecord, ExactNetRecord, ExactRoundRecord, LeaderMapRecord, NurbsCurveRecord,
    NurbsSurfaceRecord, ParameterMapRecord, ProceduralBlendRecord, RadiusRecord, SupportKindRecord,
    SupportRecord,
};
use super::{BlendDefinitionInput, BlendRef, ProceduralBlend, BLEND_MAX_DEPTH};
use crate::blend::fillet_exact::{classify_supports, BlendSpine, ClassInputs};
use crate::blend::fillet_section::section::GnSectionFsg;
use crate::blend::fillet_section::station::as_b_spline;
use crate::error::{EntityRef, Result, SolidError, SM_ERR_INVALID_INPUT};
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::Surface as NurbSurface;
use crate::surface::ball_patch::BallPatch;
use crate::surface::implicit::{AnalyticSupport, LeaderMap, SupportKind};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::FaceId;

const OP_TO: &str = "surface::procedural_blend::convert::to_record";
const OP_FROM: &str = "surface::procedural_blend::convert::from_record";
const OP_FACE: &str = "surface::procedural_blend::convert::rebuild_for_face";

/// The record of a procedural blend: its definition (supports by value, nested blends as
/// their own records) and its net, every number as the kernel stores it.
///
/// # Errors
///
/// `Unsupported` for a support that is not a NURBS surface (an offset, a curve-bounded or a
/// STEP surface: the definition's stations evaluate those by their own evaluators, which a
/// record does not carry); `SM_ERR` for a curve or surface without NURB data; those of
/// making an analytic support's NURBS form.
pub fn to_record(k: &Kernel, pb: &ProceduralBlend) -> Result<ProceduralBlendRecord> {
    to_record_at(k, pb, 1)
}

fn to_record_at(k: &Kernel, pb: &ProceduralBlend, depth: u32) -> Result<ProceduralBlendRecord> {
    if depth > BLEND_MAX_DEPTH {
        return Err(SolidError::invalid_input(
            OP_TO,
            "blends nest at most 3 definitions deep",
        ));
    }
    let d = pb.definition();
    let [s0, s1] = pb.supports();
    let supports = [support_record(k, s0, depth)?, support_record(k, s1, depth)?];
    let radius = match &d.law {
        Some(law) => RadiusRecord::Law(curve_record(law)?),
        None => RadiusRecord::Constant { offsets: d.offsets },
    };
    let [seed0, seed1] = &d.seeds;
    Ok(ProceduralBlendRecord {
        definition: BlendDefinitionRecord {
            supports,
            sides: d.sides,
            radius,
            order: d.order,
            thumbweight: d.thumbweight,
            leader: curve_record(&d.leader)?,
            seeds: [curve_record(seed0)?, curve_record(seed1)?],
            bound: d.bound,
            map: ParameterMapRecord {
                a: d.map_a,
                b: d.map_b,
                swapped: d.swapped,
            },
            chamfer: d.chamfer.map(chamfer_record),
        },
        net: surface_record(pb.net())?,
    })
}

/// A support's record: a nested blend's own record, or the NURBS form its stations are
/// solved on ([`as_b_spline`]: an analytic support is evaluated as its net).
fn support_record(k: &Kernel, s: &Surface, depth: u32) -> Result<SupportRecord> {
    if let SurfaceKind::ProceduralBlend(inner) = s.kind() {
        return Ok(SupportRecord::Blend(Box::new(to_record_at(
            k,
            inner,
            depth + 1,
        )?)));
    }
    let Some(b) = as_b_spline(k, s)? else {
        return Err(SolidError::Unsupported {
            operation: OP_TO,
            reason: "a support that is not a NURBS surface has no record",
        });
    };
    let kind = match AnalyticSupport::of(s).map(|a| a.kind()) {
        None => SupportKindRecord::Free,
        Some(SupportKind::Plane) => SupportKindRecord::Plane,
        Some(SupportKind::Cylinder) => SupportKindRecord::Cylinder,
        Some(SupportKind::Cone) => SupportKindRecord::Cone,
        Some(SupportKind::Sphere) => SupportKindRecord::Sphere,
        Some(SupportKind::Torus) => SupportKindRecord::Torus,
    };
    Ok(SupportRecord::Nurbs {
        kind,
        form: surface_record(&b)?,
    })
}

/// The kernel's kind a record's label names.
const fn kind_of(k: SupportKindRecord) -> Option<SupportKind> {
    match k {
        SupportKindRecord::Free => None,
        SupportKindRecord::Plane => Some(SupportKind::Plane),
        SupportKindRecord::Cylinder => Some(SupportKind::Cylinder),
        SupportKindRecord::Cone => Some(SupportKind::Cone),
        SupportKindRecord::Sphere => Some(SupportKind::Sphere),
        SupportKindRecord::Torus => Some(SupportKind::Torus),
    }
}

fn to_u32(x: usize) -> Result<u32> {
    u32::try_from(x).map_err(|_| SolidError::invalid_input(OP_TO, "a count past u32"))
}

/// A NURBS curve's stored numbers.
fn curve_record(c: &BSplineCurve) -> Result<NurbsCurveRecord> {
    let nurb = c.nurb.as_ref().ok_or_else(|| SolidError::sm_err(OP_TO))?;
    let rational = nurb.is_rational();
    let points_3d = nurb.is_3d();
    let cpts = nurb.cpts();
    Ok(NurbsCurveRecord {
        dim: u32::try_from(c.dim).map_err(|_| SolidError::sm_err(OP_TO))?,
        points_3d,
        degree: to_u32(nurb.degree())?,
        knots: nurb.knots().to_vec(),
        points: cpts
            .iter()
            .map(|p| [p.x, p.y, if points_3d { p.z } else { 0.0 }])
            .collect(),
        weights: rational.then(|| cpts.iter().map(|p| p.w).collect()),
    })
}

/// A NURBS surface's stored numbers.
fn surface_record(s: &BSplineSurface) -> Result<NurbsSurfaceRecord> {
    let nurb = s.gw_nurb().ok_or_else(|| SolidError::sm_err(OP_TO))?;
    let rational = nurb.is_rational();
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for row in nurb.rows() {
        for p in row {
            points.push([p.x, p.y, p.z]);
            weights.push(p.w);
        }
    }
    Ok(NurbsSurfaceRecord {
        u_degree: to_u32(nurb.p())?,
        v_degree: to_u32(nurb.q())?,
        u_count: to_u32(nurb.n().saturating_add(1))?,
        v_count: to_u32(nurb.m().saturating_add(1))?,
        u_knots: nurb.u_knots().to_vec(),
        v_knots: nurb.v_knots().to_vec(),
        points,
        weights: rational.then_some(weights),
    })
}

/// The blend a record holds, exactly: its supports, curves and net as their stored
/// numbers, the definition made by [`ProceduralBlend::create`] (which refuses an order,
/// side, radius, thumbweight or bound out of range) with the record's map. The net is
/// taken as it is, not checked against the definition: [`rebuild`] does that.
///
/// # Errors
///
/// `InvalidInput` for a record its structural check refuses
/// ([`ProceduralBlendRecord::validate`]), a degree past the kernel's, knots that decrease,
/// a weight that is not positive, or a singular map; those of
/// [`ProceduralBlend::create`].
pub fn from_record(k: &Kernel, rec: &ProceduralBlendRecord) -> Result<ProceduralBlend> {
    rec.validate()
        .map_err(|e| SolidError::invalid_input(OP_FROM, e.reason))?;
    from_record_at(k, rec, 1)
}

/// [`from_record`] and the net re-certified against the definition
/// ([`ProceduralBlend::recertify`]): the blend a record holds, refused unless its net is
/// proved within the record's bound of its definition.
///
/// # Errors
///
/// Those of [`from_record`] and [`ProceduralBlend::recertify`].
pub fn rebuild(k: &Kernel, rec: &ProceduralBlendRecord) -> Result<ProceduralBlend> {
    let pb = from_record(k, rec)?;
    pb.recertify(k)?;
    Ok(pb)
}

/// [`rebuild`] for the face `face` of a body: a refusal names the face
/// (`SM_ERR_INVALID_INPUT` with [`EntityRef::Face`]), as a body read back from its record
/// reports it; a cancel passes through as it is.
///
/// # Errors
///
/// Those of [`rebuild`], naming `face`.
pub fn rebuild_for_face(
    k: &Kernel,
    face: FaceId,
    rec: &ProceduralBlendRecord,
) -> Result<ProceduralBlend> {
    rebuild(k, rec).map_err(|e| {
        // (a cancel ends the operation; it is no refusal of the face)
        if e.is_cancelled() {
            return e;
        }
        SolidError::Upstream {
            operation: OP_FACE,
            code: SM_ERR_INVALID_INPUT,
            entity: Some(EntityRef::Face(face)),
        }
    })
}

fn from_record_at(k: &Kernel, rec: &ProceduralBlendRecord, depth: u32) -> Result<ProceduralBlend> {
    if depth > BLEND_MAX_DEPTH {
        return Err(SolidError::invalid_input(
            OP_FROM,
            "blends nest at most 3 definitions deep",
        ));
    }
    let d = &rec.definition;
    let mut supports = Vec::new();
    for (i, s) in d.supports.iter().enumerate() {
        supports.push(match s {
            SupportRecord::Nurbs { kind, form } => {
                let form = surface_of(form)?;
                match kind_of(*kind) {
                    None => Surface::from(form),
                    // the analytic surface derived from the form, the form checked on it
                    Some(kind) => AnalyticSupport::derive(k, kind, &form)
                        .and_then(|a| a.build(k, form))
                        .map_err(|e| match e {
                            SolidError::InvalidInput { .. } => SolidError::invalid_input(
                                OP_FROM,
                                if i == 0 {
                                    "support 1's NURBS form is not of its kind"
                                } else {
                                    "support 2's NURBS form is not of its kind"
                                },
                            ),
                            other => other,
                        })?,
                }
            }
            SupportRecord::Blend(b) => Surface::from(from_record_at(k, b, depth + 1)?),
        });
    }
    let (law, offsets) = match &d.radius {
        RadiusRecord::Constant { offsets } => (None, *offsets),
        RadiusRecord::Law(law) => (Some(curve_of(law)?), [0.0, 0.0]),
    };
    let chamfer = d.chamfer.as_ref().map(chamfer_of).transpose()?;
    let leader = curve_of(&d.leader)?;
    let [seed0, seed1] = &d.seeds;
    let seeds = [curve_of(seed0)?, curve_of(seed1)?];
    let net = surface_of(&rec.net)?;
    let (Some(s0), Some(s1)) = (supports.first(), supports.get(1)) else {
        return Err(SolidError::sm_err(OP_FROM));
    };
    if !(d.map.b[0] != 0.0 && d.map.b[1] != 0.0) {
        return Err(SolidError::invalid_input(
            OP_FROM,
            "its parametrization is singular",
        ));
    }
    let input = BlendDefinitionInput {
        supports: [s0, s1],
        sides: d.sides,
        offsets,
        law: law.as_ref(),
        order: d.order,
        thumbweight: d.thumbweight,
        leader: &leader,
        seeds: [&seeds[0], &seeds[1]],
        bound: d.bound,
        chamfer,
    };
    let mut pb = ProceduralBlend::create(k, &net, &input, |n, w| {
        GnSectionFsg::is_valid(n as usize, w)
    })?;
    pb.def.map_a = d.map.a;
    pb.def.map_b = d.map.b;
    pb.def.swapped = d.map.swapped;
    Ok(pb)
}

/// `degree` as a kernel degree: 1 to `NL_MAXDEG` (the NURBS evaluators size their stack
/// arrays by it, as the database readers check).
fn degree_of(degree: u32) -> Result<usize> {
    let p = degree as usize;
    if !(1..=MAX_DEGREE).contains(&p) {
        return Err(SolidError::invalid_input(
            OP_FROM,
            "a degree is 1 to the kernel's highest",
        ));
    }
    Ok(p)
}

fn non_decreasing(knots: &[f64]) -> Result<()> {
    if knots.windows(2).all(|w| matches!(w, [a, b] if a <= b)) {
        Ok(())
    } else {
        Err(SolidError::invalid_input(
            OP_FROM,
            "a knot vector decreases",
        ))
    }
}

/// The stored point `i` of a record (weighted when rational).
fn stored_point(
    points: &[[f64; 3]],
    weights: Option<&Vec<f64>>,
    i: usize,
    z: bool,
) -> Result<CPoint> {
    let [x, y, pz] = *points.get(i).ok_or_else(|| SolidError::sm_err(OP_FROM))?;
    let w = match weights {
        Some(ws) => {
            let w = *ws.get(i).ok_or_else(|| SolidError::sm_err(OP_FROM))?;
            if w.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
                return Err(SolidError::invalid_input(
                    OP_FROM,
                    "a weight is not positive",
                ));
            }
            w
        }
        None => NOW,
    };
    Ok(CPoint::new(x, y, if z { pz } else { NOZ }, w))
}

/// The curve a record holds, its NURB as stored (no knot fix-up).
fn curve_of(r: &NurbsCurveRecord) -> Result<BSplineCurve> {
    let p = degree_of(r.degree)?;
    non_decreasing(&r.knots)?;
    let mut pw = Vec::new();
    for i in 0..r.points.len() {
        pw.push(stored_point(&r.points, r.weights.as_ref(), i, r.points_3d)?);
    }
    let nurb = NurbCurve::new(pw, p, r.knots.clone())?;
    let dim = u64::from(r.dim);
    Ok(BSplineCurve {
        nurb: Some(nurb),
        ..BSplineCurve::new(dim)
    })
}

/// The surface a record holds, its NURB as stored (no knot fix-up).
fn surface_of(r: &NurbsSurfaceRecord) -> Result<BSplineSurface> {
    let p = degree_of(r.u_degree)?;
    let q = degree_of(r.v_degree)?;
    non_decreasing(&r.u_knots)?;
    non_decreasing(&r.v_knots)?;
    let (nu, nv) = (r.u_count as usize, r.v_count as usize);
    let mut net = Vec::new();
    for i in 0..nu {
        let mut row = Vec::new();
        for j in 0..nv {
            let at = i
                .checked_mul(nv)
                .and_then(|x| x.checked_add(j))
                .ok_or_else(|| SolidError::sm_err(OP_FROM))?;
            row.push(stored_point(&r.points, r.weights.as_ref(), at, true)?);
        }
        net.push(row);
    }
    let nurb = NurbSurface::new(net, p, q, r.u_knots.clone(), r.v_knots.clone())?;
    Ok(BSplineSurface {
        nurb: Some(nurb).into(),
        ..BSplineSurface::new()
    })
}

const OP_EXACT_TO: &str = "surface::procedural_blend::convert::exact_to_record";
const OP_EXACT_FROM: &str = "surface::procedural_blend::convert::exact_from_record";

/// The record of an exact round (#2485): its definition and its net, the exact surface,
/// as its analytic kind's label and its NURBS form, every number as the kernel stores it.
///
/// # Errors
///
/// `InvalidInput` for a surface that is not an exact round; `Unsupported` for a net of a
/// kind with no label; those of [`to_record`]'s supports and forms.
pub fn exact_to_record(k: &Kernel, surface: &Surface) -> Result<ExactRoundRecord> {
    let Some(BlendRef::Exact { round, net }) = surface.procedural_blend() else {
        return Err(SolidError::invalid_input(
            OP_EXACT_TO,
            "the surface is not an exact round",
        ));
    };
    let [s0, s1] = &round.supports;
    let supports = [support_record(k, s0, 1)?, support_record(k, s1, 1)?];
    let kind = match net.kind() {
        SurfaceKind::Plane(_) => ExactNetKindRecord::Plane,
        SurfaceKind::Cone(c) if c.cylinder => ExactNetKindRecord::Cylinder,
        SurfaceKind::Cone(_) => ExactNetKindRecord::Cone,
        SurfaceKind::Sphere(_) => ExactNetKindRecord::Sphere,
        SurfaceKind::Torus(_) => ExactNetKindRecord::Torus,
        SurfaceKind::SurfOfRevolution(_) => ExactNetKindRecord::Revolution,
        SurfaceKind::BallPatch(_) => ExactNetKindRecord::BallPatch,
        SurfaceKind::BSpline(_) => ExactNetKindRecord::BSpline,
        _ => {
            return Err(SolidError::Unsupported {
                operation: OP_EXACT_TO,
                reason: "an exact net of this kind has no label",
            })
        }
    };
    let Some(form) = as_b_spline(k, net)? else {
        return Err(SolidError::sm_err(OP_EXACT_TO));
    };
    let i = round.inputs;
    Ok(ExactRoundRecord {
        class: match round.class {
            ExactClass::Line => ExactClassRecord::Line,
            ExactClass::Circle => ExactClassRecord::Circle,
            ExactClass::Point => ExactClassRecord::Point,
            ExactClass::Chord => ExactClassRecord::Chord,
            ExactClass::Conic => ExactClassRecord::Conic,
            ExactClass::ConicPair => ExactClassRecord::ConicPair,
            ExactClass::NodalQuartic => ExactClassRecord::NodalQuartic,
        },
        supports,
        radius: round.radius,
        order: round.order,
        leader: round.leader.as_ref().map(curve_record).transpose()?,
        inputs: ClassInputsRecord {
            uv: [[i.uv[0].x, i.uv[0].y], [i.uv[1].x, i.uv[1].y]],
            offsets: i.offsets,
            centre: [i.centre.x, i.centre.y, i.centre.z],
            tol: i.tol,
            ball_on_collapsed: i.ball_on_collapsed,
            radii_can_change: i.radii_can_change,
        },
        chamfer: round.chamfer.map(chamfer_record),
        leader_map: LeaderMapRecord {
            a: round.map.a,
            b: round.map.b,
            along_v: round.map.along_v,
        },
        net: ExactNetRecord {
            kind,
            form: surface_record(&form)?,
        },
    })
}

/// The exact round a record holds: its supports and net derived from their forms by their
/// labels (the analytic surfaces exact to rounding, their NURBS forms as stored), its
/// definition as stored. The net is not checked against the definition:
/// [`exact_rebuild`] does that.
///
/// # Errors
///
/// `InvalidInput` for a record its structural check refuses
/// ([`ExactRoundRecord::validate`]) or a form not of its label's kind; those of making the
/// supports, the leader and the net.
pub fn exact_from_record(k: &Kernel, rec: &ExactRoundRecord) -> Result<Surface> {
    rec.validate()
        .map_err(|e| SolidError::invalid_input(OP_EXACT_FROM, e.reason))?;
    let mut supports = Vec::new();
    for s in &rec.supports {
        supports.push(match s {
            SupportRecord::Nurbs { kind, form } => {
                let form = surface_of(form)?;
                match kind_of(*kind) {
                    None => Surface::from(form),
                    Some(kind) => {
                        AnalyticSupport::derive(k, kind, &form).and_then(|a| a.build(k, form))?
                    }
                }
            }
            SupportRecord::Blend(_) => {
                return Err(SolidError::invalid_input(
                    OP_EXACT_FROM,
                    "an exact round's supports are faces",
                ))
            }
        });
    }
    let form = surface_of(&rec.net.form)?;
    let analytic = |kind: SupportKind, form: BSplineSurface| {
        AnalyticSupport::derive(k, kind, &form).and_then(|a| a.build(k, form))
    };
    let mut net = match rec.net.kind {
        ExactNetKindRecord::Plane => analytic(SupportKind::Plane, form)?,
        ExactNetKindRecord::Cylinder => analytic(SupportKind::Cylinder, form)?,
        ExactNetKindRecord::Cone => analytic(SupportKind::Cone, form)?,
        ExactNetKindRecord::Sphere => analytic(SupportKind::Sphere, form)?,
        ExactNetKindRecord::Torus => analytic(SupportKind::Torus, form)?,
        ExactNetKindRecord::Revolution => AnalyticSupport::derive_revolution(k, &form)?,
        // (its sphere is the point class's ball, read from the supports below)
        ExactNetKindRecord::BallPatch | ExactNetKindRecord::BSpline => Surface::from(form),
    };
    let i = &rec.inputs;
    let inputs = ClassInputs {
        uv: [
            Point2::new(i.uv[0][0], i.uv[0][1]),
            Point2::new(i.uv[1][0], i.uv[1][1]),
        ],
        offsets: i.offsets,
        centre: Point3::new(i.centre[0], i.centre[1], i.centre[2]),
        tol: i.tol,
        ball_on_collapsed: i.ball_on_collapsed,
        radii_can_change: i.radii_can_change,
    };
    let class = match rec.class {
        ExactClassRecord::Line => ExactClass::Line,
        ExactClassRecord::Circle => ExactClass::Circle,
        ExactClassRecord::Point => ExactClass::Point,
        ExactClassRecord::Chord => ExactClass::Chord,
        ExactClassRecord::Conic => ExactClass::Conic,
        ExactClassRecord::ConicPair => ExactClass::ConicPair,
        ExactClassRecord::NodalQuartic => ExactClass::NodalQuartic,
    };
    let leader = rec.leader.as_ref().map(curve_of).transpose()?;
    let (Some(s0), Some(s1)) = (supports.first(), supports.get(1)) else {
        return Err(SolidError::sm_err(OP_EXACT_FROM));
    };
    if rec.net.kind == ExactNetKindRecord::BallPatch {
        // a ball patch's sphere: the ball of its point class, from its supports
        let blend = classify_supports(k, [s0, s1], &inputs)?;
        if class != ExactClass::Point || blend.spine != BlendSpine::Point {
            return Err(SolidError::invalid_input(
                OP_EXACT_FROM,
                "a ball patch is the net of a point class",
            ));
        }
        let SurfaceKind::BSpline(form) = net.kind() else {
            return Err(SolidError::sm_err(OP_EXACT_FROM));
        };
        net = Surface::from(BallPatch::create(form, blend.point, blend.radius)?);
    }
    let mut round = match (class, &rec.chamfer) {
        (ExactClass::Chord, Some(c)) => {
            ExactRound::chord(k, [s0, s1], chamfer_of(c)?, leader.as_ref(), inputs)?
        }
        _ => ExactRound::create(k, class, [s0, s1], rec.radius, leader.as_ref(), inputs)?,
    };
    round.map = LeaderMap {
        a: rec.leader_map.a,
        b: rec.leader_map.b,
        along_v: rec.leader_map.along_v,
    };
    net.set_exact_round(Some(round));
    Ok(net)
}

/// [`exact_from_record`] and the net re-certified against the definition
/// ([`ExactRound::recertify`]: the class read again from the supports, the net on its
/// surface), refused unless it is.
///
/// # Errors
///
/// Those of [`exact_from_record`] and [`ExactRound::recertify`].
pub fn exact_rebuild(k: &Kernel, rec: &ExactRoundRecord) -> Result<Surface> {
    let net = exact_from_record(k, rec)?;
    if let Some(blend) = net.procedural_blend() {
        blend.recertify(k)?;
    }
    Ok(net)
}

/// A chamfer's sizes as their record (#2486).
fn chamfer_record(c: ChamferDefinition) -> ChamferRecord {
    ChamferRecord {
        size: match c.size {
            ChamferSize::Distances(x) => ChamferSizeRecord::Distances(x),
            ChamferSize::DistanceAngle { distance, angle } => {
                ChamferSizeRecord::DistanceAngle { distance, angle }
            }
            ChamferSize::Chord(l) => ChamferSizeRecord::Chord(l),
        },
        wedge: c.wedge,
    }
}

/// The chamfer's sizes a record holds, checked ([`ChamferDefinition::new`]).
fn chamfer_of(c: &ChamferRecord) -> Result<ChamferDefinition> {
    ChamferDefinition::new(
        match c.size {
            ChamferSizeRecord::Distances(x) => ChamferSize::Distances(x),
            ChamferSizeRecord::DistanceAngle { distance, angle } => {
                ChamferSize::DistanceAngle { distance, angle }
            }
            ChamferSizeRecord::Chord(l) => ChamferSize::Chord(l),
        },
        c.wedge,
    )
}
