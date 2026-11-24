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
use super::corner::{CornerDefinition, SetbackFill};
use super::exact::{ExactClass, ExactRound};
use super::record::{
    BezierCurveRecord, BlendDefinitionRecord, ChamferKindRecord, ChamferRecord, ChamferSizeRecord,
    ClassInputsRecord, CornerRecord, DraftRecord, EndCornerRecord, ExactClassRecord,
    ExactNetKindRecord, ExactNetRecord, ExactRoundDefinition, ExactRoundRecord, FaceDefinition,
    LeaderMapRecord, MixedCornerRecord, MixedSideRecord, NurbsCurveRecord, NurbsSurfaceRecord,
    ParameterMapRecord, PlaneRecord, ProceduralBlendRecord, QuadricKindRecord, QuadricRecord,
    RadiusRecord, RevolutionRecord, SetbackCornerRecord, SetbackRoundRecord, SupportKindRecord,
    SupportRecord, SweptCornerRecord,
};
use super::record::{EndOrderRecord, MotionRecord, SectionBlendRecord, SectionFrameRecord};
use super::{BlendDefinitionInput, BlendRef, ProceduralBlend, BLEND_MAX_DEPTH};
use crate::blend::blend_corner::mixed::algebra::{Crv, Side};
use crate::blend::blend_setback::{Face, FaceKind, Round};
use crate::blend::fillet_exact::{classify_supports, BlendSpine, ClassInputs};
use crate::blend::fillet_section::section::GnSectionFsg;
use crate::blend::fillet_section::station::as_b_spline;
use crate::error::surface::SurfaceFailure;
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::crv_basic::Curve as NurbCurve;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::Surface as NurbSurface;
use crate::surface::ball_patch::BallPatch;
use crate::surface::implicit::{AnalyticSupport, LeaderMap, SupportKind};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::FaceId;

const OP_TO: &str = "surface::procedural_blend::convert::to_record";
const OP_SUPPORT: &str = "surface::procedural_blend::convert::surface_of_support";
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
        None if d
            .chamfer
            .and_then(ChamferDefinition::constant_setback)
            .is_some() =>
        {
            RadiusRecord::EqualSetback
        }
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
/// solved on ([`as_b_spline`]: an analytic support is evaluated as its net), labelled with
/// its analytic kind where reading the record derives that surface back from the form
/// written ([`AnalyticSupport::derive`], then [`AnalyticSupport::build`], as
/// [`exact_from_record`], [`from_record`] and [`corner_from_record`] do), or where the
/// cone or cylinder the surface is places onto that form ([`proved_revolution`]). A circle
/// is its chord and its middle weight, so a span of a few degrees proves by evaluation. A
/// shorter arc fixes its centre only to about the radius over the square of the sweep. The
/// cone or cylinder, when [`proved_revolution`] places it, is written beside the form
/// ([`RevolutionRecord`]). A form that is neither that cone nor proved from its rows goes
/// unlabelled, as a face whose form does not prove its label does
/// (`face_surface::record_face`, owner 22254), and the record reads back.
fn support_record(k: &Kernel, s: &Surface, depth: u32) -> Result<SupportRecord> {
    nurbs_record(k, s, depth, true)
}

/// A corner's net or a draft's source or net: its record with its analytic label as it is,
/// proved or not, since the record requires the label (a swept turn's net is a torus, a
/// drafted plane a plane and a drafted cone a cone: [`CornerRecord::validate`],
/// [`DraftRecord::validate`]).
fn face_record(k: &Kernel, s: &Surface) -> Result<SupportRecord> {
    nurbs_record(k, s, 1, false)
}

/// [`support_record`] (`proved`: a label only where reading derives it back) or
/// [`face_record`].
fn nurbs_record(k: &Kernel, s: &Surface, depth: u32, proved: bool) -> Result<SupportRecord> {
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
    let form = surface_record(&b)?;
    // the reader's own steps on the form as it reads it
    let stored_form = surface_of(&form)?;
    let revolution = match (revolution_record(s), AnalyticSupport::of(s)) {
        (Some(rev), Some(AnalyticSupport::Cylinder { .. })) => {
            proved_revolution(k, &stored_form, SupportKindRecord::Cylinder, &rev)?.map(|_| rev)
        }
        (Some(rev), Some(AnalyticSupport::Cone { .. })) => {
            proved_revolution(k, &stored_form, SupportKindRecord::Cone, &rev)?.map(|_| rev)
        }
        _ => None,
    };
    let reads_back = |kind: SupportKind| -> Result<bool> {
        match labelled(k, kind, stored_form.copy()) {
            Ok(_) => Ok(true),
            Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
            Err(_) => Ok(false),
        }
    };
    // The label stands when the stored cone places on the form, or when the form proves it.
    let label = match AnalyticSupport::of(s).map(|a| a.kind()) {
        Some(kind) if !proved || revolution.is_some() || reads_back(kind)? => Some(kind),
        _ => None,
    };
    let kind = match label {
        None => SupportKindRecord::Free,
        Some(SupportKind::Plane) => SupportKindRecord::Plane,
        Some(SupportKind::Cylinder) => SupportKindRecord::Cylinder,
        Some(SupportKind::Cone) => SupportKindRecord::Cone,
        Some(SupportKind::Sphere) => SupportKindRecord::Sphere,
        Some(SupportKind::Torus) => SupportKindRecord::Torus,
    };
    Ok(SupportRecord::Nurbs {
        kind,
        form,
        revolution,
    })
}

/// The cone or cylinder `s` is, as stored. `None` for any other surface.
fn revolution_record(s: &Surface) -> Option<RevolutionRecord> {
    let (placement, radius, semi_angle_deg) = match AnalyticSupport::of(s)? {
        AnalyticSupport::Cylinder { placement, radius } => (placement, radius, 0.0),
        AnalyticSupport::Cone {
            placement,
            base_radius,
            semi_angle_deg,
        } => (placement, base_radius, semi_angle_deg),
        _ => return None,
    };
    let o = placement.origin();
    let x = placement.x_axis();
    let y = placement.y_axis();
    Some(RevolutionRecord {
        origin: [o.x, o.y, o.z],
        x_axis: [x.x, x.y, x.z],
        y_axis: [y.x, y.y, y.z],
        radius,
        semi_angle_deg,
    })
}

/// `form` as the cone or cylinder `revolution` names, when the form lies on it and those
/// numbers place a cone on the form. `None` when the numbers are not a placement or the
/// form does not lie on that cone. The caller then derives the label from the form.
fn proved_revolution(
    k: &Kernel,
    form: &BSplineSurface,
    kind: SupportKindRecord,
    revolution: &RevolutionRecord,
) -> Result<Option<Surface>> {
    let xyz = |p: [f64; 3]| Point3::new(p[0], p[1], p[2]);
    let origin = xyz(revolution.origin);
    let x_axis = xyz(revolution.x_axis);
    let y_axis = xyz(revolution.y_axis);
    let finite = [origin, x_axis, y_axis]
        .iter()
        .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
        && revolution.radius.is_finite()
        && revolution.semi_angle_deg.is_finite();
    if !finite {
        return Ok(None);
    }
    let placement = Axis2Placement::from_stored(origin, x_axis, y_axis);
    let cylinder = revolution.semi_angle_deg == 0.0;
    let support = match (kind, cylinder) {
        (SupportKindRecord::Cylinder, true) => AnalyticSupport::Cylinder {
            placement,
            radius: revolution.radius,
        },
        (SupportKindRecord::Cone, false) => AnalyticSupport::Cone {
            placement,
            base_radius: revolution.radius,
            semi_angle_deg: revolution.semi_angle_deg,
        },
        _ => return Ok(None),
    };
    // The stored axis, radius and angle, once the form lies on them.
    match support.place_on(k, form) {
        Ok(surface) => Ok(Some(surface)),
        Err(e) if e.is_cancelled() || e.is_not_certified() => Err(e),
        Err(SolidError::InvalidInput { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A NURBS support read back. A stored cone or cylinder places on the form, or the read
/// is refused. A support with no stored cone is the surface derived from its label.
fn surface_of_support(
    k: &Kernel,
    kind: SupportKindRecord,
    form: BSplineSurface,
    revolution: Option<&RevolutionRecord>,
) -> Result<Surface> {
    if let Some(revolution) = revolution {
        return proved_revolution(k, &form, kind, revolution)?.ok_or_else(|| {
            SolidError::invalid_input(
                OP_SUPPORT,
                "a support's cone does not place on its NURBS form",
            )
        });
    }
    match kind_of(kind) {
        None => Ok(Surface::from(form)),
        Some(kind) => labelled(k, kind, form),
    }
}

/// A labelled form's analytic surface, as every reader of a record makes it and
/// [`support_record`] proves it before it writes the label: derived from the form
/// ([`AnalyticSupport::derive`]) and proved its parametrisation ([`AnalyticSupport::build`]).
fn labelled(k: &Kernel, kind: SupportKind, form: BSplineSurface) -> Result<Surface> {
    AnalyticSupport::derive(k, kind, &form).and_then(|a| a.build(k, form))
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
    let nurb = c
        .nurb
        .as_ref()
        .ok_or_else(|| SolidError::surface(OP_TO, SurfaceFailure::CurveWithoutNurb))?;
    let rational = nurb.is_rational();
    let points_3d = nurb.is_3d();
    let cpts = nurb.cpts();
    Ok(NurbsCurveRecord {
        dim: u32::try_from(c.dim)
            .map_err(|_| SolidError::surface(OP_TO, SurfaceFailure::CurveDimensionTooLarge))?,
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
    let nurb = s.gw_nurb().ok_or_else(|| {
        Failure::new(OP_TO, SurfaceFailure::NoNurb)
            .with_status(SM_ERR)
            .into_error()
    })?;
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
        // (a cancel, or a certificate's budget spent, ends the operation; it is no refusal of the face)
        if e.is_cancelled() || e.is_not_certified() {
            return e;
        }
        Failure::new(OP_FACE, SurfaceFailure::RecordDoesNotRebuild)
            .at(EntityRef::Face(face))
            .caused_by(e)
            .into_error()
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
    // a chamfer of setbacks, before any of its supports is made (a corner's or a support's
    // nested blend comes here too)
    if matches!(d.radius, RadiusRecord::Edge) || d.chamfer.is_some_and(|c| c.is_setbacks()) {
        return Err(setbacks_not_built());
    }
    let support_of = |i: usize, s: &SupportRecord| -> Result<Surface> {
        Ok(match s {
            SupportRecord::Nurbs {
                kind,
                form,
                revolution,
            } => surface_of_support(k, *kind, surface_of(form)?, revolution.as_ref()).map_err(
                |e| match e {
                    SolidError::InvalidInput { .. } => SolidError::invalid_input(
                        OP_FROM,
                        if i == 0 {
                            "support 1's NURBS form is not of its kind"
                        } else {
                            "support 2's NURBS form is not of its kind"
                        },
                    ),
                    other => other,
                },
            )?,
            SupportRecord::Blend(b) => Surface::from(from_record_at(k, b, depth + 1)?),
        })
    };
    let [r0, r1] = &d.supports;
    let supports = [support_of(0, r0)?, support_of(1, r1)?];
    let mut chamfer = d.chamfer.as_ref().map(chamfer_of).transpose()?;
    let (law, offsets) = match &d.radius {
        RadiusRecord::Constant { offsets } => (None, *offsets),
        RadiusRecord::Law(law) => (Some(curve_of(law)?), [0.0, 0.0]),
        RadiusRecord::Edge => return Err(setbacks_not_built()),
        RadiusRecord::EqualSetback => {
            let sizes = chamfer
                .ok_or_else(|| {
                    SolidError::invalid_input(OP_FROM, "a local wedge needs the chamfer's setbacks")
                })?
                .at_every_station()?;
            chamfer = Some(sizes);
            (None, sizes.offsets()?)
        }
    };
    let leader = curve_of(&d.leader)?;
    let [seed0, seed1] = &d.seeds;
    let seeds = [curve_of(seed0)?, curve_of(seed1)?];
    let net = surface_of(&rec.net)?;
    let [s0, s1] = &supports;
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
    let [x, y, pz] = *points.get(i).ok_or_else(|| {
        SolidError::surface(
            OP_FROM,
            SurfaceFailure::Defect {
                why: "a stored point past the record's points was asked for, though its readers ask only below its point count (a surface's u_count * v_count), which the record's check or the kernel's own writing of it guarantees",
            },
        )
    })?;
    let w = match weights {
        Some(ws) => {
            let w = *ws.get(i).ok_or_else(|| {
                SolidError::surface(
                    OP_FROM,
                    SurfaceFailure::Defect {
                        why: "a stored weight past the record's weights was asked for, though a rational record has a weight per point, which its check or the kernel's own writing of it guarantees",
                    },
                )
            })?;
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
                .ok_or_else(|| {
                    SolidError::surface(
                        OP_FROM,
                        SurfaceFailure::Defect {
                            why: "a stored point's index i * v_count + j overflows, though it is below u_count * v_count, the point count, which the record's check or the kernel's own writing of it guarantees",
                        },
                    )
                })?;
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
/// kind with no label and for a chord with a sagitta (a weld bead's, #15129, which no record
/// holds yet); those of [`to_record`]'s supports and forms.
pub fn exact_to_record(k: &Kernel, surface: &Surface) -> Result<ExactRoundRecord> {
    let Some(BlendRef::Exact { round, net }) = surface.procedural_blend() else {
        return Err(SolidError::invalid_input(
            OP_EXACT_TO,
            "the surface is not an exact round",
        ));
    };
    // a weld bead's chord with a sagitta (#15129): no record holds a sagitta yet, and one read
    // without it would be the flat chord, so it is refused rather than dropped
    if round.sagitta != 0.0 {
        return Err(SolidError::Unsupported {
            operation: OP_EXACT_TO,
            reason: "a chord with a sagitta (a weld bead's) has no record yet",
        });
    }
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
        return Err(SolidError::surface(
            OP_EXACT_TO,
            SurfaceFailure::Defect {
                why: "the exact net has no B-spline form, though it is of a kind with a label (a plane, cone, sphere, torus, surface of revolution, ball patch or B-spline), which as_b_spline always gives one",
            },
        ));
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
    let support_of = |s: &SupportRecord| -> Result<Surface> {
        Ok(match s {
            SupportRecord::Nurbs {
                kind,
                form,
                revolution,
            } => surface_of_support(k, *kind, surface_of(form)?, revolution.as_ref())?,
            SupportRecord::Blend(_) => {
                return Err(SolidError::invalid_input(
                    OP_EXACT_FROM,
                    "an exact round's supports are faces",
                ))
            }
        })
    };
    let [r0, r1] = &rec.supports;
    let supports = [support_of(r0)?, support_of(r1)?];
    let form = surface_of(&rec.net.form)?;
    let analytic = |kind: SupportKind, form: BSplineSurface| labelled(k, kind, form);
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
    let [s0, s1] = &supports;
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
            return Err(SolidError::surface(
                OP_EXACT_FROM,
                SurfaceFailure::Defect {
                    why: "a ball patch's exact net is not a B-spline surface, though it was just made as the surface of its B-spline form",
                },
            ));
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

const OP_ADOPT: &str = "surface::procedural_blend::convert::adopt_definitions";

/// The definition a face's surface carries, without its net, when a body's record keeps it as
/// the face's blend: a procedural blend's, or a round of an exact class's whose net has no
/// analytic label of its own (a surface of revolution, or the rational classes' B-spline). A
/// round whose net is an analytic surface (a plane, cylinder, cone, sphere or torus) or a ball
/// patch keeps that label in the record, and its definition is not written; nor is a definition
/// whose supports a record cannot hold (`Unsupported`, [`to_record`]), which then goes as its
/// net. `None` for any other surface.
///
/// # Errors
///
/// Those of [`to_record`] and [`exact_to_record`] other than `Unsupported`.
pub fn face_definition(k: &Kernel, surface: &Surface) -> Result<Option<FaceDefinition>> {
    let unrecorded = |e: SolidError| match e {
        SolidError::Unsupported { .. } => Ok(None),
        other => Err(other),
    };
    if let SurfaceKind::ProceduralBlend(pb) = surface.kind() {
        return match to_record(k, pb) {
            Ok(rec) => Ok(Some(FaceDefinition::Procedural(Box::new(rec.definition)))),
            Err(e) => unrecorded(e),
        };
    }
    let Some(BlendRef::Exact { net, .. }) = surface.procedural_blend() else {
        return Ok(None);
    };
    if !matches!(
        net.kind(),
        SurfaceKind::SurfOfRevolution(_) | SurfaceKind::BSpline(_)
    ) {
        return Ok(None);
    }
    let rec = match exact_to_record(k, surface) {
        Ok(rec) => rec,
        Err(e) => return unrecorded(e),
    };
    Ok(Some(FaceDefinition::ExactRound(Box::new(
        ExactRoundDefinition {
            class: rec.class,
            supports: rec.supports,
            radius: rec.radius,
            order: rec.order,
            leader: rec.leader,
            inputs: rec.inputs,
            chamfer: rec.chamfer,
            leader_map: rec.leader_map,
            net_kind: rec.net.kind,
        },
    ))))
}

/// The surface a face's definition makes on its net `net` (the face's surface as built from its
/// record, a NURBS surface): the definition made from the record exactly ([`from_record`],
/// [`exact_from_record`]) and the net certified against it again. `Ok(Err(eps))`, with the bound
/// the definition states, where the net is not certified: the definition and its net disagree.
///
/// # Errors
///
/// `InvalidInput` for a net that is not a NURBS surface; those of the rebuilds; a cancel as it is.
pub fn definition_surface(
    k: &Kernel,
    definition: &FaceDefinition,
    net: &Surface,
) -> Result<std::result::Result<Surface, f64>> {
    let Some(form) = as_b_spline(k, net)? else {
        return Err(SolidError::invalid_input(
            OP_ADOPT,
            "a blend face's net is not a NURBS surface",
        ));
    };
    let net = surface_record(&form)?;
    // (a cancel, or a certificate's work budget spent, ends the operation; any other failure
    // of the certificate refuses the net)
    let certified = |r: Result<f64>, eps: f64| match r {
        Ok(_) => Ok(Ok(())),
        Err(e) if e.is_cancelled() || e.is_not_certified() => Err(e),
        Err(_) => Ok(Err(eps)),
    };
    Ok(match definition {
        FaceDefinition::Procedural(d) => {
            let record = ProceduralBlendRecord {
                definition: (**d).clone(),
                net,
            };
            let pb = from_record(k, &record)?;
            certified(recertify_remembered(k, &pb, &record), d.bound)?.map(|()| Surface::from(pb))
        }
        FaceDefinition::ExactRound(d) => {
            let rec = ExactRoundRecord {
                class: d.class,
                supports: d.supports.clone(),
                radius: d.radius,
                order: d.order,
                leader: d.leader.clone(),
                inputs: d.inputs,
                chamfer: d.chamfer,
                leader_map: d.leader_map,
                net: ExactNetRecord {
                    kind: d.net_kind,
                    form: net,
                },
            };
            let made = exact_from_record(k, &rec)?;
            let proved = match made.procedural_blend() {
                Some(blend) => certified(blend.recertify(k), 0.0)?,
                None => Ok(()),
            };
            proved.map(|()| made)
        }
    })
}

/// [`RecertifyMemo`]'s cap on the bytes its keys hold, the loop area memo's 8 MiB: past it the
/// memo starts again empty. A round's key is about 45 KB (its definition's supports and its net
/// as text), so the cap holds about 180 of them.
const RECERTIFY_MEMO_BYTES: usize = 8 << 20;

/// The memo of the procedural blends a kernel recertified as it gave a record's faces their
/// definitions back (not upstream): the bound [`ProceduralBlend::recertify`] proved for each,
/// by every bit of the blend's record (its definition and its net, by their debug text,
/// which writes each double so that it reads back exactly) and of the kernel's tolerances
/// and fixes. The certificate reads nothing else, so a hit returns what recertifying the
/// blend again returns. A body that goes through one boolean after another comes back from
/// each as a record, and its blends unchanged by the boolean are certified once, not again
/// at every import. Only a certificate that finished is kept, not a refusal or a cancel.
/// [`Kernel::memoize_curve_caches`] turns it off, with the curve cache memo.
#[derive(Debug, Default)]
pub struct RecertifyMemo {
    bounds: std::collections::HashMap<Box<str>, f64>,
    bytes: usize,
}

impl RecertifyMemo {
    /// The blends the memo keeps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bounds.len()
    }

    /// Whether it keeps none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bounds.is_empty()
    }

    /// Keeps `bound` for `key`, its keys holding at most `cap` bytes: past the cap the memo
    /// starts again empty. A key larger than the cap on its own is not kept, and the memo is
    /// left as it was, since emptying it would not make room for that key.
    fn remember(&mut self, key: String, bound: f64, cap: usize) {
        if key.len() > cap {
            return;
        }
        if self.bytes + key.len() > cap {
            self.bounds.clear();
            self.bytes = 0;
        }
        // (a boxed key holds its length exactly, not a grown string's spare room)
        self.bytes += key.len();
        self.bounds.insert(key.into_boxed_str(), bound);
    }
}

/// `pb`'s [`ProceduralBlend::recertify`], from the kernel's [`RecertifyMemo`] when it
/// recertified the same `record` (the one `pb` was made from) before.
fn recertify_remembered(
    k: &Kernel,
    pb: &ProceduralBlend,
    record: &ProceduralBlendRecord,
) -> Result<f64> {
    use std::fmt::Write as _;
    if !k.memoize_curve_caches {
        k.work_counters.add_blend_recertified();
        return pb.recertify(k);
    }
    let mut key = format!("{:?}", k.tolerances);
    for (name, on) in k.fixes.flags() {
        key.push_str(if on { "+" } else { "-" });
        key.push_str(name);
    }
    // (writing to a `String` never fails)
    let _ = write!(key, "\n{record:?}");
    if let Some(&hit) = k.recertify_memo.borrow().bounds.get(key.as_str()) {
        return Ok(hit);
    }
    k.work_counters.add_blend_recertified();
    let bound = pb.recertify(k)?;
    k.recertify_memo
        .borrow_mut()
        .remember(key, bound, RECERTIFY_MEMO_BYTES);
    Ok(bound)
}

/// Give each face of `faces` its definition back: its net is the face's surface as it was
/// built (a NURBS surface), the definition is made from the record exactly ([`from_record`],
/// [`exact_from_record`]) and the net certified against it again, and the face's surface becomes
/// the blend (`SetSurface`; `key_of` names a face as the body's notifications do). A net its
/// definition does not certify refuses the face ([`SolidError::DefinitionNotCertified`]): a
/// record's blend is never an operand until it is certified (docs/definitions-and-certificates.md
/// §7 inv. 8).
///
/// # Errors
///
/// [`SolidError::DefinitionNotCertified`] naming the first face refused; `InvalidInput` for a
/// face without a NURBS surface; those of the rebuilds and of `SetSurface`. A cancel passes
/// through as it is.
pub fn adopt_definitions(
    k: &Kernel,
    b: &mut crate::topology::Brep,
    key_of: &dyn Fn(EntityRef) -> crate::geom::object::ObjectRef,
    faces: &[(FaceId, FaceDefinition)],
) -> Result<()> {
    for (f, definition) in faces {
        let face = *f;
        let made = {
            let surface =
                b.face(face)?.surface.as_ref().ok_or_else(|| {
                    SolidError::invalid_input(OP_ADOPT, "a face without a surface")
                })?;
            definition_surface(k, definition, surface)?
        };
        let made = made.map_err(|eps| SolidError::DefinitionNotCertified {
            operation: OP_ADOPT,
            face: EntityRef::Face(face),
            eps,
        })?;
        crate::topology::face::p1::set_surface(
            k,
            b,
            face,
            key_of(EntityRef::Face(face)),
            made,
            true,
        )?;
    }
    Ok(())
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
        kind: ChamferKindRecord::Offsets,
    }
}

/// The refusal of a chamfer whose setbacks hold at every station
/// ([`ChamferKindRecord::Setbacks`], [`RadiusRecord::Edge`]): its record is defined, its
/// construction and certificate not yet, and reading it as offsets would build another body.
fn setbacks_not_built() -> SolidError {
    SolidError::Unsupported {
        operation: OP_CHAMFER_OF,
        reason: "a chamfer whose setbacks hold at every station is not built yet",
    }
}

const OP_CHAMFER_OF: &str = "surface::procedural_blend::convert::chamfer_of";

/// The chamfer's sizes a record holds, checked ([`ChamferDefinition::new`]); a chamfer of
/// setbacks is refused ([`setbacks_not_built`]).
///
/// # Errors
///
/// `Unsupported` for a chamfer of setbacks; those of [`ChamferDefinition::new`].
pub(crate) fn chamfer_of(c: &ChamferRecord) -> Result<ChamferDefinition> {
    if c.is_setbacks() {
        return Err(setbacks_not_built());
    }
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

const OP_CORNER_TO: &str = "surface::procedural_blend::convert::corner_to_record";
const OP_CORNER_FROM: &str = "surface::procedural_blend::convert::corner_from_record";

/// The record of a blend corner's face (#2488): its definition and its net (its analytic
/// kind's label and its NURBS form), every number as the kernel stores it.
///
/// # Errors
///
/// `InvalidInput` for a surface that is not a corner's face; those of [`to_record`]'s
/// supports and forms.
pub fn corner_to_record(k: &Kernel, surface: &Surface) -> Result<CornerRecord> {
    let Some(BlendRef::Corner { corner, net }) = surface.procedural_blend() else {
        return Err(SolidError::invalid_input(
            OP_CORNER_TO,
            "the surface is not a corner's face",
        ));
    };
    let xyz = |p: Point3| [p.x, p.y, p.z];
    let face_net = || face_record(k, net);
    Ok(match corner {
        // the Ball's patch is recorded as its B-rep face, its sphere the face's `BallPatch`
        // label over its net, and read back certified by solidcore-convert's `ball_patch`
        CornerDefinition::Ball(_) => {
            return Err(SolidError::invalid_input(
                OP_CORNER_TO,
                "a ball's patch is recorded as its B-rep face, not as a corner record",
            ))
        }
        CornerDefinition::Swept(d) => CornerRecord::Swept(SweptCornerRecord {
            common: support_record(k, &d.common, 1)?,
            axis_point: xyz(d.axis_point),
            axis: xyz(d.axis),
            centres: [xyz(d.centres[0]), xyz(d.centres[1])],
            radius: d.radius,
            tol: d.tol,
            net: face_net()?,
        }),
        CornerDefinition::End(d) => CornerRecord::End(EndCornerRecord {
            support: support_record(k, &d.support, 1)?,
            boundary: d.boundary.iter().map(curve_record).collect::<Result<_>>()?,
            net: face_net()?,
        }),
        CornerDefinition::Setback(d) => {
            let fill = &*d.fill;
            let Some(form) = as_b_spline(k, surface)? else {
                return Err(SolidError::surface(
                    OP_CORNER_TO,
                    SurfaceFailure::NoBSplineForm,
                ));
            };
            CornerRecord::Setback(SetbackCornerRecord {
                faces: fill
                    .faces
                    .iter()
                    .map(|f| PlaneRecord {
                        point: xyz(f.point),
                        normal: xyz(f.normal),
                        quadric: quadric_record(f),
                    })
                    .collect(),
                rounds: fill
                    .rounds
                    .iter()
                    .map(|r| {
                        Ok(SetbackRoundRecord {
                            surface: support_record(k, &r.surface, 1)?,
                            along_u: r.along == SurfParamType::U,
                            end: r.end,
                            corner_above: r.corner_above,
                        })
                    })
                    .collect::<Result<_>>()?,
                order: to_u32(fill.order)?,
                tol: fill.tol,
                convex: fill.convex,
                containment: fill.containment,
                certified: fill.certified,
                patch: to_u32(d.patch)?,
                net: surface_record(&form)?,
            })
        }
        CornerDefinition::Mixed(d) => {
            let Some(form) = as_b_spline(k, surface)? else {
                return Err(SolidError::surface(
                    OP_CORNER_TO,
                    SurfaceFailure::NoBSplineForm,
                ));
            };
            let [b1, b2] = &d.blends;
            CornerRecord::Mixed(Box::new(MixedCornerRecord {
                blends: [support_record(k, b1, 1)?, support_record(k, b2, 1)?],
                common: support_record(k, &d.common, 1)?,
                sides: d.sides.iter().map(side_record).collect(),
                bilinear: d.bilinear,
                transposed: d.transposed,
                blend: to_u32(d.blend)?,
                trim_side: to_u32(d.trim_side)?,
                trim_uv: bezier_record(&d.trim_uv),
                conic_side: to_u32(d.conic_side)?,
                tol: d.tol,
                net: surface_record(&form)?,
            }))
        }
    })
}

/// A corner's face or support a record holds: its analytic surface derived from its NURBS
/// form by its label (exact to rounding, the form as stored), or the form.
fn corner_surface_of(k: &Kernel, s: &SupportRecord) -> Result<Surface> {
    match s {
        SupportRecord::Nurbs {
            kind,
            form,
            revolution,
        } => surface_of_support(k, *kind, surface_of(form)?, revolution.as_ref()),
        SupportRecord::Blend(_) => Err(SolidError::invalid_input(
            OP_CORNER_FROM,
            "a corner's face is a NURBS form",
        )),
    }
}

/// The corner's face a record holds: its net and supports derived from their forms by
/// their labels, its definition as stored. The net is not checked against the definition:
/// [`corner_rebuild`] does that.
///
/// # Errors
///
/// `InvalidInput` for a record its structural check refuses ([`CornerRecord::validate`])
/// or a form not of its label's kind; those of making the supports, curves and net.
pub fn corner_from_record(k: &Kernel, rec: &CornerRecord) -> Result<Surface> {
    rec.validate()
        .map_err(|e| SolidError::invalid_input(OP_CORNER_FROM, e.reason))?;
    let p = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    let (net, definition) = match rec {
        CornerRecord::Swept(r) => (
            &r.net,
            CornerDefinition::swept(
                k,
                &corner_surface_of(k, &r.common)?,
                p(r.axis_point),
                p(r.axis),
                [p(r.centres[0]), p(r.centres[1])],
                r.radius,
                r.tol,
            )?,
        ),
        CornerRecord::End(r) => (
            &r.net,
            CornerDefinition::end(
                k,
                &corner_surface_of(k, &r.support)?,
                r.boundary.iter().map(curve_of).collect::<Result<_>>()?,
            )?,
        ),
        CornerRecord::Setback(r) => {
            let mut surfaces = Vec::with_capacity(r.rounds.len());
            for round in &r.rounds {
                surfaces.push(match &round.surface {
                    SupportRecord::Blend(b) => Surface::from(from_record_at(k, b, 1)?),
                    other @ SupportRecord::Nurbs { .. } => corner_surface_of(k, other)?,
                });
            }
            let rounds: Vec<Round<'_>> = r
                .rounds
                .iter()
                .zip(&surfaces)
                .map(|(round, surface)| Round {
                    surface,
                    along: if round.along_u {
                        SurfParamType::U
                    } else {
                        SurfParamType::V
                    },
                    end: round.end,
                    corner_above: round.corner_above,
                })
                .collect();
            let faces: Vec<Face> = r.faces.iter().map(setback_face_of).collect();
            let fill = SetbackFill::create(
                k,
                &faces,
                &rounds,
                r.order as usize,
                r.tol,
                r.convex,
                r.containment,
                r.certified,
            )?;
            let mut net = Surface::from(surface_of(&r.net)?);
            net.set_corner_definition(CornerDefinition::setback(
                std::sync::Arc::new(fill),
                r.patch as usize,
            )?);
            return Ok(net);
        }
        CornerRecord::Mixed(r) => {
            let blend = |b: &SupportRecord| match b {
                SupportRecord::Blend(b) => Ok(Surface::from(from_record_at(k, b, 1)?)),
                other @ SupportRecord::Nurbs { .. } => corner_surface_of(k, other),
            };
            let blends = [blend(&r.blends[0])?, blend(&r.blends[1])?];
            let sides: Vec<Side> = r.sides.iter().map(side_of).collect();
            let Ok(sides) = <[Side; 4]>::try_from(sides) else {
                return Err(SolidError::invalid_input(
                    OP_CORNER_FROM,
                    "a mixed face has four sides",
                ));
            };
            let mut net = Surface::from(surface_of(&r.net)?);
            net.set_corner_definition(CornerDefinition::mixed(
                k,
                [&blends[0], &blends[1]],
                &corner_surface_of(k, &r.common)?,
                sides,
                (r.bilinear, r.transposed),
                (
                    r.blend as usize,
                    r.trim_side as usize,
                    bezier_of(&r.trim_uv),
                ),
                r.conic_side as usize,
                r.tol,
            )?);
            return Ok(net);
        }
    };
    let mut net = corner_surface_of(k, net)?;
    net.set_corner_definition(definition);
    Ok(net)
}

/// [`corner_from_record`] and the net re-certified against the definition
/// ([`CornerDefinition::recertify`]), refused unless it is.
///
/// # Errors
///
/// Those of [`corner_from_record`] and [`CornerDefinition::recertify`].
pub fn corner_rebuild(k: &Kernel, rec: &CornerRecord) -> Result<Surface> {
    let net = corner_from_record(k, rec)?;
    if let Some(blend) = net.procedural_blend() {
        blend.recertify(k)?;
    }
    Ok(net)
}

/// A piecewise Bezier curve's record.
/// A Setback face's quadric as a record (#2343); `None` for a plane.
fn quadric_record(f: &Face) -> Option<QuadricRecord> {
    let kind = match f.kind {
        FaceKind::Plane => return None,
        FaceKind::Cylinder => QuadricKindRecord::Cylinder,
        FaceKind::Cone => QuadricKindRecord::Cone,
        FaceKind::Sphere => QuadricKindRecord::Sphere,
    };
    Some(QuadricRecord {
        kind,
        axis: [f.axis.x, f.axis.y, f.axis.z],
        radius: f.radius,
        slope: f.slope,
        side: f.side,
    })
}

/// A Setback face from its record: its plane, or the quadric it is (#2343).
fn setback_face_of(f: &PlaneRecord) -> Face {
    let p = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    let plane = Face::plane(p(f.point), p(f.normal));
    match &f.quadric {
        None => plane,
        Some(q) => Face {
            kind: match q.kind {
                QuadricKindRecord::Cylinder => FaceKind::Cylinder,
                QuadricKindRecord::Cone => FaceKind::Cone,
                QuadricKindRecord::Sphere => FaceKind::Sphere,
            },
            axis: p(q.axis),
            radius: q.radius,
            slope: q.slope,
            side: q.side,
            ..plane
        },
    }
}

fn bezier_record(c: &Crv) -> BezierCurveRecord {
    BezierCurveRecord {
        brk: c.brk.clone(),
        seg: c.seg.clone(),
    }
}

/// The piecewise Bezier curve a record holds.
fn bezier_of(r: &BezierCurveRecord) -> Crv {
    Crv {
        brk: r.brk.clone(),
        seg: r.seg.clone(),
    }
}

/// A Mixed side's record.
fn side_record(s: &Side) -> MixedSideRecord {
    MixedSideRecord {
        c: bezier_record(&s.c),
        dir: bezier_record(&s.dir),
        shared: s.shared,
        val: s.val,
        der: s.der,
        dir_val: s.dir_val,
        dir_der: s.dir_der,
        dir_k: s.dir_k,
        t: bezier_record(&s.t),
        has_t: s.has_t,
    }
}

/// The Mixed side a record holds.
fn side_of(r: &MixedSideRecord) -> Side {
    Side {
        c: bezier_of(&r.c),
        dir: bezier_of(&r.dir),
        shared: r.shared,
        val: r.val,
        der: r.der,
        dir_val: r.dir_val,
        dir_der: r.dir_der,
        dir_k: r.dir_k,
        t: bezier_of(&r.t),
        has_t: r.has_t,
    }
}

const OP_DRAFT_TO: &str = "surface::procedural_blend::convert::draft_to_record";

/// The record of a drafted face's surface (`offset::draft`): its definition and its net (its
/// analytic kind's label and its NURBS form), every number as the kernel stores it.
///
/// # Errors
///
/// `InvalidInput` for a surface that is not a drafted face's; those of the forms.
pub fn draft_to_record(k: &Kernel, surface: &Surface) -> Result<DraftRecord> {
    let Some(BlendRef::Draft { draft, net }) = surface.procedural_blend() else {
        return Err(SolidError::invalid_input(
            OP_DRAFT_TO,
            "the surface is not a drafted face's",
        ));
    };
    let xyz = |p: Point3| [p.x, p.y, p.z];
    let d = draft.draft();
    Ok(DraftRecord {
        source: face_record(k, draft.source())?,
        outward: draft.outward(),
        neutral_origin: xyz(d.neutral_origin()),
        neutral_normal: xyz(d.neutral_normal()),
        pull: xyz(d.pull()),
        angle: d.angle(),
        net: face_record(k, net)?,
    })
}

/// The drafted face's surface a record holds, its definition kept on it, not yet
/// re-certified: [`draft_rebuild`] does that.
///
/// # Errors
///
/// `InvalidInput` for a record its structural check refuses ([`DraftRecord::validate`]) or
/// a form not of its label's kind; those of making the surfaces.
pub fn draft_from_record(k: &Kernel, rec: &DraftRecord) -> Result<Surface> {
    rec.validate().map_err(|e| {
        SolidError::invalid_input(
            "surface::procedural_blend::convert::draft_from_record",
            e.reason,
        )
    })?;
    let p = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    let draft = crate::offset::draft::Draft::from_parts(
        p(rec.neutral_origin),
        p(rec.neutral_normal),
        p(rec.pull),
        rec.angle,
    )?;
    let source = corner_surface_of(k, &rec.source)?;
    let definition = crate::offset::draft::DraftDefinition::create(k, &source, rec.outward, draft)?;
    let mut net = corner_surface_of(k, &rec.net)?;
    net.set_draft_definition(definition);
    Ok(net)
}

/// [`draft_from_record`] and the net re-certified against the definition
/// ([`crate::offset::draft::DraftDefinition::recertify`]), refused unless it is.
///
/// # Errors
///
/// Those of [`draft_from_record`] and of the re-certification.
pub fn draft_rebuild(k: &Kernel, rec: &DraftRecord) -> Result<Surface> {
    let net = draft_from_record(k, rec)?;
    if let Some(d) = net.procedural_blend() {
        d.recertify(k)?;
    }
    Ok(net)
}

const OP_SECTION_TO: &str = "surface::procedural_blend::convert::section_blend_to_record";
const OP_SECTION_FROM: &str = "surface::procedural_blend::convert::section_blend_from_record";

/// The record of a multi-section blend's surface (`sweep::section_blend`, #3140): its
/// definition and its net, every number as the kernel stores it.
///
/// # Errors
///
/// `InvalidInput` for a surface that is not a section blend's, or a net that is not a
/// B-spline surface; those of the form.
pub fn section_blend_to_record(surface: &Surface) -> Result<SectionBlendRecord> {
    use crate::sweep::section_blend::EndOrder;
    let Some(BlendRef::SectionBlend { blend, net }) = surface.procedural_blend() else {
        return Err(SolidError::invalid_input(
            OP_SECTION_TO,
            "the surface is not a section blend's",
        ));
    };
    let SurfaceKind::BSpline(form) = net.kind() else {
        return Err(SolidError::invalid_input(
            OP_SECTION_TO,
            "a section blend's net is a B-spline surface",
        ));
    };
    let xyz = |p: Point3| [p.x, p.y, p.z];
    let order = |o: EndOrder| match o {
        EndOrder::G0 => EndOrderRecord::G0,
        EndOrder::G1 => EndOrderRecord::G1,
        EndOrder::G2 => EndOrderRecord::G2,
        EndOrder::G3 => EndOrderRecord::G3,
    };
    let [o0, o1] = blend.orders();
    Ok(SectionBlendRecord {
        sections: blend
            .sections()
            .iter()
            .map(|section| {
                section
                    .iter()
                    .map(|f| SectionFrameRecord {
                        p: xyz(f.p),
                        leave: xyz(f.leave),
                        bend: xyz(f.bend),
                        bend_change: xyz(f.bend_change),
                    })
                    .collect()
            })
            .collect(),
        orders: [order(o0), order(o1)],
        crown: blend.crown(),
        motions: blend
            .motions()
            .iter()
            .map(|m| MotionRecord {
                origin: xyz(m.rotate_n_move.origin()),
                x_axis: xyz(m.rotate_n_move.x_axis()),
                y_axis: xyz(m.rotate_n_move.y_axis()),
                scale: m.scale.map(xyz),
            })
            .collect(),
        net: surface_record(form)?,
    })
}

/// The multi-section blend's surface a record holds, its definition kept on it, not yet
/// re-certified: [`section_blend_rebuild`] does that.
///
/// # Errors
///
/// `InvalidInput` for a record its structural check refuses
/// ([`SectionBlendRecord::validate`]); those of making the net.
pub fn section_blend_from_record(rec: &SectionBlendRecord) -> Result<Surface> {
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::sweep::section_blend::{EndOrder, Motion, SectionBlendDefinition, SectionFrame};
    rec.validate()
        .map_err(|e| SolidError::invalid_input(OP_SECTION_FROM, e.reason))?;
    let p = |a: [f64; 3]| Point3::new(a[0], a[1], a[2]);
    let order = |o: EndOrderRecord| match o {
        EndOrderRecord::G0 => EndOrder::G0,
        EndOrderRecord::G1 => EndOrder::G1,
        EndOrderRecord::G2 => EndOrder::G2,
        EndOrderRecord::G3 => EndOrder::G3,
    };
    let sections = rec
        .sections
        .iter()
        .map(|section| {
            section
                .iter()
                .map(|f| SectionFrame {
                    p: p(f.p),
                    leave: p(f.leave),
                    bend: p(f.bend),
                    bend_change: p(f.bend_change),
                })
                .collect()
        })
        .collect();
    let motions = rec
        .motions
        .iter()
        .map(|m| Motion {
            rotate_n_move: Axis2Placement::from_stored(p(m.origin), p(m.x_axis), p(m.y_axis)),
            scale: m.scale.map(p),
        })
        .collect();
    let [o0, o1] = rec.orders;
    let definition =
        SectionBlendDefinition::from_parts(sections, [order(o0), order(o1)], rec.crown, motions);
    let mut net = Surface::from(surface_of(&rec.net)?);
    net.set_section_blend_definition(definition);
    Ok(net)
}

/// [`section_blend_from_record`] and the net re-certified against the definition
/// ([`crate::sweep::section_blend::SectionBlendDefinition::recertify`]): refused unless the
/// definition rebuilds it exactly.
///
/// # Errors
///
/// Those of [`section_blend_from_record`] and of the re-certification.
pub fn section_blend_rebuild(rec: &SectionBlendRecord) -> Result<Surface> {
    let net = section_blend_from_record(rec)?;
    if let Some(BlendRef::SectionBlend { blend, net: n }) = net.procedural_blend() {
        blend.recertify(n)?;
    }
    Ok(net)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-degree cone reads back as the axis, radius and semi-angle it was built with.
    /// A placement the patch does not lie on contributes no surface.
    #[test]
    fn a_one_degree_cone_keeps_the_axis_it_was_built_with() {
        use crate::surface::cone::Cone;
        use crate::surface::surface::Surface;
        let k = Kernel::default();
        let origin = Point3::new(1000.0, 500.0, 40.0);
        let mut cone = Cone::new_bounded(
            origin,
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            28.5,
            33.0,
            0.0,
            1.0,
            8.0,
            false,
            false,
            false,
        );
        cone.make_nurb(&k).unwrap();
        let built = cone.rev.position.origin();
        let want_radius = cone.base_radius;
        let want_angle = cone.semi_angle_deg;
        let want_x = cone.rev.position.x_axis();
        let surface = Surface::from(cone);
        let rec = nurbs_record(&k, &surface, 1, true).unwrap();
        let SupportRecord::Nurbs {
            kind,
            form,
            revolution: Some(revolution),
        } = rec
        else {
            panic!("the cone was not stored");
        };
        assert_eq!(kind, SupportKindRecord::Cone);
        let back = surface_of_support(
            &k,
            SupportKindRecord::Cone,
            surface_of(&form).unwrap(),
            Some(&revolution),
        )
        .unwrap();
        let read = AnalyticSupport::of(&back).unwrap();
        let AnalyticSupport::Cone {
            placement,
            base_radius,
            semi_angle_deg,
        } = read
        else {
            panic!("the support did not read back as a cone");
        };
        assert!(
            (placement.origin() - built).length() < 1e-9,
            "origin moved from {built:?} to {:?}",
            placement.origin()
        );
        assert!(
            (placement.x_axis() - want_x).length() < 1e-9,
            "x axis moved to {:?}",
            placement.x_axis()
        );
        assert!((base_radius - want_radius).abs() < 1e-9);
        assert!((semi_angle_deg - want_angle).abs() < 1e-9);
        let nurbs = surface_of(&form).unwrap();
        let mut forged = revolution;
        forged.origin[0] += 1.0;
        let refused = proved_revolution(&k, &nurbs, SupportKindRecord::Cone, &forged).unwrap();
        assert!(
            refused.is_none(),
            "a centre the patch does not lie on was used"
        );

        // A one-degree cylinder: semi-angle 0, the same placement and radius.
        let mut cylinder = Cone::new_cylinder_bounded(
            origin,
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            28.5,
            0.0,
            1.0,
            8.0,
            false,
            false,
            false,
        );
        cylinder.make_nurb(&k).unwrap();
        let cyl_origin = cylinder.rev.position.origin();
        let cyl = Surface::from(cylinder);
        let rec = nurbs_record(&k, &cyl, 1, true).unwrap();
        let SupportRecord::Nurbs {
            kind,
            form,
            revolution: Some(revolution),
        } = rec
        else {
            panic!("the cylinder was not stored");
        };
        assert_eq!(kind, SupportKindRecord::Cylinder);
        assert_eq!(revolution.semi_angle_deg, 0.0);
        let back = surface_of_support(
            &k,
            SupportKindRecord::Cylinder,
            surface_of(&form).unwrap(),
            Some(&revolution),
        )
        .unwrap();
        let AnalyticSupport::Cylinder { placement, radius } = AnalyticSupport::of(&back).unwrap()
        else {
            panic!("the support did not read back as a cylinder");
        };
        assert!((placement.origin() - cyl_origin).length() < 1e-9);
        assert!((radius - 28.5).abs() < 1e-9);
    }

    /// The memo keeps keys up to its cap, starts again empty when the next key would pass it,
    /// and keeps no key larger than the cap on its own, so no such allocation outlives the call.
    #[test]
    fn the_recertify_memo_keeps_no_key_larger_than_its_cap() {
        let mut memo = RecertifyMemo::default();
        memo.remember("a".repeat(6), 1.0, 10);
        memo.remember("b".repeat(4), 2.0, 10);
        assert_eq!(
            (memo.len(), memo.bytes),
            (2, 10),
            "two keys fill the cap exactly"
        );
        memo.remember("c".repeat(3), 3.0, 10);
        assert_eq!(
            (memo.len(), memo.bytes),
            (1, 3),
            "a key past the cap starts the memo again"
        );
        assert_eq!(memo.bounds.get("ccc"), Some(&3.0));
        memo.remember("d".repeat(11), 4.0, 10);
        assert_eq!(
            (memo.len(), memo.bytes),
            (1, 3),
            "a key larger than the cap is not kept"
        );
        assert_eq!(memo.bounds.get("d".repeat(11).as_str()), None);
    }

    /// The record conversion's named causes keep the status of the phrase causes they replaced
    /// (a site whose status differs from the cause's own keeps it through `with_status`), with
    /// the kind their condition is: the caller's blend, curve or surface, or a surface kind the
    /// conversion has no code for.
    #[test]
    fn the_record_conversion_causes_keep_the_status_they_replaced() {
        use crate::error::surface::SurfaceFailure;
        use crate::error::ErrorKind;
        let no_nurb = Failure::new(OP_TO, SurfaceFailure::NoNurb)
            .with_status(SM_ERR)
            .into_error();
        let samples = [
            (
                no_nurb,
                SurfaceFailure::NoNurb,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SolidError::surface(OP_TO, SurfaceFailure::CurveWithoutNurb),
                SurfaceFailure::CurveWithoutNurb,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SolidError::surface(OP_TO, SurfaceFailure::CurveDimensionTooLarge),
                SurfaceFailure::CurveDimensionTooLarge,
                SM_ERR,
                ErrorKind::InvalidInput,
            ),
            (
                SolidError::surface(OP_CORNER_TO, SurfaceFailure::NoBSplineForm),
                SurfaceFailure::NoBSplineForm,
                SM_ERR,
                ErrorKind::Unsupported,
            ),
        ];
        for (typed, failure, code, kind) in samples {
            assert_eq!(typed.sm_status(), Some(code), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), kind, "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert!(!typed.is_defect(), "{typed:?}");
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
    }

    /// The rebuild refusal keeps the face and status the bare status named, and the rebuild's
    /// own error as its cause.
    #[test]
    fn the_rebuild_refusal_keeps_its_face_and_cause() {
        use crate::error::surface::SurfaceFailure;
        let face = EntityRef::Face(FaceId::from_index(3));
        let why = SolidError::invalid_input(OP_FROM, "its parametrization is singular");
        let typed = Failure::new(OP_FACE, SurfaceFailure::RecordDoesNotRebuild)
            .at(face)
            .caused_by(why.clone())
            .into_error();
        let before = SolidError::Upstream {
            operation: OP_FACE,
            code: crate::error::SM_ERR_INVALID_INPUT,
            entity: Some(face),
        };
        assert_eq!(typed.sm_status(), before.sm_status());
        assert_eq!(typed.upstream_status(), before.upstream_status());
        assert_eq!(typed.kind(), before.kind());
        assert_eq!(typed.operation(), before.operation());
        assert_eq!(typed.entity(), Some(face));
        assert_eq!(typed.is_status_answer(), before.is_status_answer());
        assert_eq!(typed.cause(), Some(&why));
    }
}
