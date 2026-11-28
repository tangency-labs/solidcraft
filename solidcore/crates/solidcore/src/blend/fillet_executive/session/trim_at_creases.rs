//! The cliff overflow trimmed at its creases on the live session (not upstream): the
//! fork's `SmFilletExecutive::TrimAtCreases` (`source/SMLib/src/SmFilletOverflow.cpp`,
//! tangency-labs/smlib#79 step 6 at 5e8c9c1: 99e57b8, 668fe9f, bacd0bb, 2d4b154, bc8c44d,
//! de5df14, 8832ead; WR-695, unconditional since `fillet_overflow_trim_at_creases` was
//! removed). The geometry steps
//! are in [`crate::blend::fillet_overflow`].
//!
//! | Rust                                | Fork                                 |
//! |-------------------------------------|--------------------------------------|
//! | `FilletSession::trim_at_creases`    | `SmFilletExecutive::TrimAtCreases`   |
//! | `is_tangent_edge` (private)         | `SmFilletSolver::IsTangentEdge` (bacd0bb) |
//! | `FilletSession::has_open_setback`   | `SmFilletExecutive::HasOpenSetBack` (ef57570) |
//! | `open_setback_springs` (private)    | `TrimAtCreases`' open setback corners' spring curves and hole edges (ef57570) |
//!
//! Deviations:
//! - **Bounded sheets before sewing.** Open ends are cut and capped before the first
//!   stitch, so independent sheets never sew their coincident continuations together.
//! - **The sheet leaves the session.** As in the removed `subtract_removal_region`, the
//!   sheet (the intermediate body) is taken out of the session first and edited on its own; the
//!   target is edited in place through the session's notifications, as the fork edits
//!   it, and put back as it was when the trim fails (the fork leaves whatever it reached).
//! - **Sets.** The fork's pointer-keyed `std::set` / `std::map` are `BTreeSet` /
//!   `BTreeMap` keyed by ids, for membership; every loop follows a vector in the bodies'
//!   own orders. The faces taken off are deleted in the order they were reached (the
//!   fork's in address order).
//! - **Fit sizes.** The three refusals step 5 sizes (`NoteFitOnFace` after the tangent
//!   edge and the extension, `NoteFitRadius` from `sm_CornerTakesPerRadius` after the
//!   whole edge; tangency-labs/smlib a3d6d81, 49f393d) note them (WR-696, #2278).
//! - **Section ends (not the fork's).** A round with WR-674's section ends is trimmed here
//!   (WR-706, #2223): each such end is cut by its section's plane and closed by the
//!   section's flat face, and a side of that face that lies along an edge of the body is
//!   sewn to the edge's piece the rail's imprint cuts off, not imprinted.
//! - **Creases along an edge (not the fork's).** (WR-717, #2385; once the flag
//!   `fillet_overflow_crease_along_edge`) A crease that runs along an edge of the face it
//!   crosses, within the trim's tolerance, is that edge: the sheet is split along it, and
//!   the edge stands as its imprint, so the material taken off stops there and the sheet
//!   is sewn to it. The fork drops such a crease as not inside the face, leaves the
//!   sheet's piece past the face, and refuses at the sewing (a disc 2 high rounded 2.001:
//!   the crease runs 2.5e-7 inside the bottom edge).
//! - **A strip below resolution (not the fork's, #2404).** A body edge that runs along the
//!   kept sheet, just off it, leaves a strip whose sides the trim's tolerances cannot tell
//!   apart: refused (`NoRoomReason::StripBelowResolution`), not left to fail at the sewing or
//!   to fall in two.
//! - **Open setbacks and each round's side (the fork's ef57570, ce748fa; smlib#125).** An
//!   open G1 setback corner's spring curves are imprinted with the sheet's boundary and its
//!   hole's `3 n` edges stay free; a concave round's sheet lies in the void it fills.
//! - **The Roll-over's certified creases (not the fork's, #10876).** In the blend corner's
//!   Roll-over second pass (`FilletSession::set_roll_over_pass`), a crease of an exact torus
//!   with a cliff face parallel to its axis (a plane, or a cylinder touching the convex
//!   round) is the certified graph over the face's profile
//!   ([`crate::blend::blend_corner::rollover::geometry::torus_creases`]), within the body's
//!   tolerance of the torus, not the general intersection.
//! - **Split imprint edges (not the fork's, #12342).** The fork takes the imprint as the
//!   edges `MergeCurvesOnSurface` names; a later imprint that splits one of them keeps only
//!   one piece named. Here every piece of a face's edges along one of its imprint curves is
//!   the imprint, whichever way the edge runs.
//! - **A shallow crossing ends on the meet.** A crease that meets a boundary edge at a
//!   shallow angle is classified short of that meet, by more than the trim's tolerance,
//!   and the two edges there do not sew. Each curve lies in a tube of that tolerance, so
//!   the meet can sit `2 tol / sin(theta)` past the classified end. The end moves onto
//!   the crossing the edge's curve makes with the face.

use std::collections::{BTreeMap, BTreeSet};

use super::cliff_overflow::SOLVER_EDGE;
use super::{invalid, FilletBody, FilletExecutive, FilletObject, FilletSession, SessionObjects};
use crate::blend::fillet_cliff_overflow::{
    cut_fillet_at_open_end, section_edge, section_end, section_end_face, Refusal, SectionEnd,
};
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::{
    FilletEdgeType, FilletErrorType, FilletGeomId, FilletSurfaceStorage,
};
use crate::blend::fillet_overflow::{
    creases, edge_curve, edge_on_surface, extend, in_material, make_extension_face, outward_plane,
    ExtendOutcome, Extension, Rail,
};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::blend::{BlendFailure, CliffInput, CreaseTrimStop, CrossingInput, OverflowInput};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::attribute::ids;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::ObjectRef;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::b_spline_curve::{BSplineCurve, ControlPointForm};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::iges::clamp_curve;
use crate::nurbs::knots::Side;
use crate::nurbs::nurbs_crv::is_clamped;
use crate::query::brep::BrepIdentity;
use crate::surface::plane::Plane;
use crate::surface::procedural_blend::NetScope;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::brep::p3::{self, delete_face, with_kernel_callbacks, DeleteFaceOptions};
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge::NotifyEnv;
use crate::topology::topology_traverser::Marks;
use crate::topology::{edge, edgeuse::p1 as eu1, face, vertex, Brep, BrepId, EdgeId, FaceId};

const OP: &str = "blend::fillet_executive::session::trim_at_creases";

/// A face or edge the trim reads lacks `input` ([`BlendFailure::OverflowInputMissing`], the
/// cause of the overflows' helpers, which read the same bodies): the faces and edges read are
/// the caller's body's and the sheet's alike, so the failure names no entity.
fn input_missing(input: OverflowInput) -> SolidError {
    SolidError::blend(OP, BlendFailure::OverflowInputMissing(input))
}

/// A step the executive's own construction guarantees, failed ([`BlendFailure::Defect`],
/// status `SM_ERR`), `why` saying why it is the kernel's fault.
fn defect(why: &'static str) -> SolidError {
    SolidError::blend(OP, BlendFailure::Defect { why })
}

/// A missing thing the executive built ([`BlendFailure::Defect`] keeping the upstream `NER`
/// status `SM_ERR_NULL_POINTER`), `why` saying why it is always there.
fn null_defect(why: &'static str) -> SolidError {
    Failure::new(OP, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// A trim refused for a blend too large, `reason` ([`BlendFailure::NoRoom`]), as noted.
fn no_room(reason: NoRoomReason) -> SolidError {
    SolidError::blend(OP, BlendFailure::NoRoom(reason))
}

/// Why a section end's solver still rolls on an edge use with an edge when the trim reads it.
const SECTION_END_SOLVER: &str = "a section end's solver has no edge use, or its edge use no \
     edge: the bound of the open ends built and capped this end from the same solver's edge \
     before sewing";

/// Why the session's intermediate body is there when the trim runs.
const INTERMEDIATE: &str = "the session has no intermediate body when the blend is trimmed at \
     its creases: the trim runs on the fillet body the executive has just made (its open ends \
     bounded), before anything else takes it";

/// Why each solver's rolling offset surface is there.
const SOLVER_OFFSET: &str = "a solver that rolls a ball has no offset surface: the solver \
     set-up gives each rail the offset of its face's surface before the solver is loaded";

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `SmFilletSolver::IsTangentEdge` (the fork's bacd0bb): whether the two faces at the
/// manifold edge `e` meet tangentially there (`sm_TestManifoldTangentRollover`).
fn is_tangent_edge(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    parameter: f64,
    tangency_tol_rad: f64,
) -> Result<bool> {
    if !edge::is_manifold(b, e)? {
        return Ok(false);
    }
    Ok(
        crate::blend::fillet_solver::p1::test_manifold_tangent_rollover_at(
            k,
            b,
            e,
            parameter,
            tangency_tol_rad,
        )?
        .0,
    )
}

/// The key of entity `e` of `b` in a notification (its issued identity, or the default
/// scheme's).
fn notify_key(b: &Brep, e: EntityRef) -> ObjectRef {
    match b.notify_serial() {
        Some(s) => p3::topology_ref(s, e),
        None => crate::topology::loopuse::notify_ref(e),
    }
}

/// `MergeCurvesOnSurface(*face->GetSurface(), tol, curves, NULL, rNewFaces, rEdges, FALSE,
/// intersection)` on `b`, notified through `env`: the new faces and the curves' edges.
fn merge_curves(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    f: FaceId,
    tol: f64,
    mut curves: Vec<Curve>,
    intersection: Option<Surface>,
) -> Result<(Vec<FaceId>, Vec<EdgeId>)> {
    let key_of = env.key_of;
    let curve_ref = |e: EdgeId, kind: i64| ObjectRef {
        key: key_of(EntityRef::Edge(e)).key,
        type_code: kind,
    };
    match intersection {
        Some(surface) => crate::topology::brep::p10::merge_curves_on_surface_through_surface(
            k,
            b,
            env,
            &curve_ref,
            f,
            tol,
            &mut curves,
            None,
            Some(surface),
        ),
        None => crate::topology::brep::p10::merge_curves_on_surface(
            k, b, env, &curve_ref, f, tol, &curves, None, false,
        ),
    }
}

/// `DeleteFace(f, TRUE)` on `b`, notified through `env`.
fn delete(k: &mut Kernel, b: &mut Brep, env: &mut NotifyEnv<'_>, f: FaceId) -> Result<()> {
    let mut marks = Marks::default();
    delete_face(
        k,
        b,
        env,
        &mut marks,
        f,
        DeleteFaceOptions {
            delete_edges_vertices: true,
            ..DeleteFaceOptions::default()
        },
    )
    .map(drop)
}

/// The drop of `point` onto `curve` over `interval`: `Some(parameter, distance)` when it
/// succeeds.
fn drop_on(
    k: &Kernel,
    curve: &Curve,
    interval: &crate::geom::extent1d::Extent1d,
    point: Point3,
) -> Result<Option<(f64, f64)>> {
    let d = curve.drop_point(
        k,
        interval,
        point,
        None,
        BIG_DOUBLE,
        None,
        SolverOperation::Minimize,
    )?;
    Ok(d.success.then_some((d.parameter, d.distance)))
}

/// The point where `crease` meets `other`, solved from `start` by Gauss-Newton on the
/// pair (the parameters `(t, u)` minimising `|crease(t) - other(u)|`, each kept within its
/// curve's natural interval: the crease's piece and the edge's trimmed curve, neither run
/// on past its ends), which converges quadratically wherever the curves cross, however
/// shallow the crossing. Accepted only on the solved residual: the point on the crease
/// when the two points lie within `tol * 1e-6` of each other (iterated on while that
/// still halves the residual). `None` when a drop or an evaluation fails, the curves are
/// parallel there to rounding (the system is singular), or no step within 32 reaches the
/// residual (curves that pass apart, or a meet past either curve's end).
fn crease_meets(
    k: &Kernel,
    crease: &Curve,
    other: &Curve,
    start: Point3,
    tol: f64,
) -> Result<Option<Point3>> {
    let (ci, oi) = (crease.natural_interval(), other.natural_interval());
    let (Some((mut t, _)), Some((mut u, _))) = (
        drop_on(k, crease, &ci, start)?,
        drop_on(k, other, &oi, start)?,
    ) else {
        return Ok(None);
    };
    let accept = tol * 1e-6;
    let mut found: Option<(f64, Point3)> = None;
    for _ in 0..32 {
        let dc = crease.evaluate(k, t, 1, false, false)?;
        let d_o = other.evaluate(k, u, 1, false, false)?;
        let (Some(&p), Some(&a), Some(&q), Some(&b)) =
            (dc.first(), dc.get(1), d_o.first(), d_o.get(1))
        else {
            return Ok(None);
        };
        let r = p - q;
        let gap = r.length();
        // NaN fails every comparison and so is never accepted
        if gap <= accept {
            match found {
                Some((best, _)) if gap > 0.5 * best => break,
                _ => found = Some((gap, p)),
            }
            if gap == 0.0 {
                break;
            }
        }
        // the least-squares step of r + a dt - b du = 0
        let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
        let (ar, br) = (a.dot(r), b.dot(r));
        let det = ab * ab - aa * bb;
        let solvable = det.abs() > 64.0 * f64::EPSILON * aa * bb;
        if !solvable {
            break;
        }
        let dt = (ar * bb - ab * br) / det;
        let du = (ab * ar - aa * br) / det;
        t = smos_min(smos_max(t + dt, ci.min()), ci.max());
        u = smos_min(smos_max(u + du, oi.min()), oi.max());
    }
    Ok(found.map(|(_, p)| p))
}

/// The piece of `curve` that runs from `from` to `to` (each dropped onto it), reversed
/// when they run against its parameter. `None` when either does not drop onto it, or
/// both drop to one parameter (no piece between them).
fn piece_between(k: &Kernel, curve: &Curve, from: Point3, to: Point3) -> Result<Option<Curve>> {
    let natural = curve.natural_interval();
    let (Some((t0, _)), Some((t1, _))) = (
        drop_on(k, curve, &natural, from)?,
        drop_on(k, curve, &natural, to)?,
    ) else {
        return Ok(None);
    };
    let distinct = t0 < t1 || t1 < t0;
    if !distinct {
        return Ok(None);
    }
    let mut piece = curve.copy(k)?;
    let (lo, hi) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
    let mut ivl = crate::geom::extent1d::Extent1d::from_min_max(lo, hi);
    if ivl != piece.natural_interval() {
        piece.trim(
            k,
            &mut ivl,
            crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
        )?;
    }
    if t0 > t1 {
        let old = piece.natural_interval();
        piece.reverse_parameterization(k, &old)?;
    }
    Ok(Some(piece))
}

/// `piece` as the edge's new 3D curve with its UV trims cleared (`SmBrep::ReplaceCurve`'s
/// edit on the notification environment).
fn replace_edge_curve(
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    e: EdgeId,
    piece: Curve,
) -> Result<()> {
    edge::replace_3d_curve(b, e, Some(piece), env)?;
    for eu in edge::edgeuses(b, e, None, None)? {
        let _ = eu1::set_uv_trim_curve(b, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None)?;
    }
    Ok(())
}

/// The Euclidean control points of a B-spline kind (a line's or a conic's own
/// `SmBSplineCurve` included) whose weights are all positive, with the largest coordinate
/// among them: `None` for another kind, a base without its NURB, or a weight that is not
/// positive (NaN included). Such a curve lies in their convex hull.
fn hull_points(curve: &Curve) -> Option<(Vec<Point3>, f64)> {
    let spline = match curve.kind() {
        CurveKind::BSpline(c) => c,
        CurveKind::Line(c) => c.base(),
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Parabola(c) => c.base(),
        CurveKind::Hyperbola(c) => c.base(),
        _ => return None,
    };
    let n = spline.number_control_points().ok()?;
    let mut points = Vec::new();
    let mut scale = 0.0_f64;
    for i in 0..n {
        let (p, w) = spline
            .control_point(ControlPointForm::EuclideanRational, i)
            .ok()?;
        let positive = w > 0.0;
        if !positive && w != crate::nurbs::math::NOW {
            return None;
        }
        scale = smos_max(scale, smos_max(p.x.abs(), smos_max(p.y.abs(), p.z.abs())));
        points.push(p);
    }
    Some((points, scale))
}

/// The largest coordinate of `p`.
fn coordinate_scale(p: Point3) -> f64 {
    smos_max(p.x.abs(), smos_max(p.y.abs(), p.z.abs()))
}

/// An enclosed bound on the distance from every point of `crease` to the line through
/// `a` and `b` (`None` when it cannot be bounded: see [`hull_points`], or `a` and `b` one
/// point). A B-spline with positive weights lies in its control points' convex hull, and
/// the distance to a line is convex, so its greatest value over the curve is at most the
/// greatest over the control points. Each distance is computed in the line's own frame
/// and counted up for its rounding by `64 eps s (1 + D / L)`, with `s` the largest
/// coordinate, `L = |b - a|` and `D` the largest `|p - a|`: `b - a` and `p - a` take one
/// rounding per coordinate on `s`, so the unit direction is off by about `eps s / L`,
/// which moves the perpendicular of a point `D` away by `eps s D / L`; `along` and
/// `d - dir along` take about eight roundings on `D` (at most `2 sqrt 3 s`), and a
/// rational point one more for its weight. 64 of each covers them with room to spare.
fn distance_to_line_bound(crease: &Curve, a: Point3, b: Point3) -> Option<f64> {
    let (points, hull_scale) = hull_points(crease)?;
    let axis = b - a;
    let len = axis.length();
    let has_length = len > 0.0;
    if !has_length {
        return None;
    }
    let dir = axis / len;
    let scale = smos_max(
        hull_scale,
        smos_max(coordinate_scale(a), coordinate_scale(b)),
    );
    let (mut bound, mut reach) = (0.0_f64, 0.0_f64);
    for p in points {
        let d = p - a;
        let along = d.dot(dir);
        let off = (d - dir * along).length();
        if !off.is_finite() {
            return None;
        }
        bound = smos_max(bound, off);
        reach = smos_max(reach, d.length());
    }
    Some(bound + 64.0 * f64::EPSILON * scale * (1.0 + reach / len))
}

/// An enclosed bound on the distance from every point of `crease` to the plane through
/// `at` with unit normal `normal` (`None` when it cannot be bounded, see [`hull_points`]):
/// the distance to a plane is convex, so the control points' greatest bounds the curve's;
/// each is counted up by `64 eps` of the largest coordinate for its rounding (one per
/// coordinate of `p - at` and three for the dot product, on that size).
fn distance_to_plane_bound(crease: &Curve, at: Point3, normal: Vec3) -> Option<f64> {
    let (points, hull_scale) = hull_points(crease)?;
    let scale = smos_max(hull_scale, coordinate_scale(at));
    let mut bound = 0.0_f64;
    for p in points {
        let off = (p - at).dot(normal).abs();
        if !off.is_finite() {
            return None;
        }
        bound = smos_max(bound, off);
    }
    Some(bound + 64.0 * f64::EPSILON * scale)
}

/// Whether `curve` is straight: a line, or a B-spline whose control points all lie on the
/// line through its ends to rounding. Its ends are returned, with the enclosed bound on
/// its distance from the line through them (0 for a line).
fn straight_ends(k: &Kernel, curve: &Curve) -> Result<Option<(Point3, Point3, f64)>> {
    let ivl = curve.natural_interval();
    let (a, b) = (
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    );
    let deviation = match curve.kind() {
        CurveKind::Line(_) => Some(0.0),
        CurveKind::BSpline(_) => {
            let scale = smos_max(
                smos_max(a.x.abs(), smos_max(a.y.abs(), a.z.abs())),
                smos_max(b.x.abs(), smos_max(b.y.abs(), b.z.abs())),
            );
            distance_to_line_bound(curve, a, b).filter(|&d| d <= 128.0 * f64::EPSILON * scale)
        }
        _ => None,
    };
    Ok(deviation.map(|d| (a, b, d)))
}

/// What a move of an edge onto its crease leaves to the trim: geometry that no longer
/// meets where it did, which the faces the trim deletes must take away (else the trim
/// is refused, never accommodated by a larger tolerance).
#[derive(Debug, Default)]
struct MovedEdges {
    /// The edges moved onto their creases.
    moved: BTreeSet<EdgeId>,
    /// The moved vertices: an edge through one whose curve misses its new point is off it.
    vertices: Vec<crate::topology::VertexId>,
    /// The crease pieces the edges took.
    pieces: Vec<Curve>,
}

/// WR-717 (#17633): moves the body edge `e` of face `f`, which `crease` runs along, onto
/// the crease, so that the sheet's boundary is imprinted and sewn where it is instead of
/// being drawn to the edge. Each end vertex goes to where the crease meets one of the
/// vertex's other edges on `f`, within that edge's curve (so the edge keeps its geometry
/// and is only trimmed to it); the edge takes the crease's piece between them. Edges at a
/// moved vertex whose curve passes through its new point are trimmed to it; the others,
/// and the moved edge against its other faces, are recorded in `record` for the trim to
/// take away. Nothing moves (the imprint then draws the section to the edge, as before)
/// when: the edge is not straight, or the crease's piece is not enclosed within `tol` of
/// its line (a bound over the whole piece, [`distance_to_line_bound`]); either end has no
/// meet within `on_tol`; or a move would leave an edge already moved off its new vertex.
#[allow(clippy::too_many_arguments)]
fn move_edge_onto_crease(
    k: &Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    f: FaceId,
    e: EdgeId,
    crease: &Curve,
    tol: f64,
    on_tol: f64,
    record: &mut MovedEdges,
) -> Result<bool> {
    let (Some(sv), Some(ev)) = edge::start_end_vertices(b, e)? else {
        return Ok(false);
    };
    if sv == ev {
        return Ok(false);
    }
    let Some((ea, eb, straightness)) = straight_ends(k, &edge_curve(k, b, e)?)? else {
        return Ok(false);
    };
    // the trim checks the moved edge's faces against their planes (below, after the
    // faces past the creases are deleted): a crease on a curved face is not moved
    let planar = match face::p2::surface(b, f)? {
        Some(surface) => crate::blend::fillet_overflow::plane_of(k, surface)?.is_some(),
        None => false,
    };
    if !planar {
        return Ok(false);
    }
    let mut ends = Vec::with_capacity(2);
    for v in [sv, ev] {
        let p = vertex::point(b, v)?;
        let mut q = None;
        for e2 in vertex::edges(b, v, Some(f))? {
            if e2 == e {
                continue;
            }
            let other = edge_curve(k, b, e2)?;
            q = crease_meets(k, crease, &other, p, tol)?;
            if q.is_some() {
                break;
            }
        }
        let Some(q) = q else {
            return Ok(false);
        };
        let near = q.distance(p) <= on_tol;
        if !near {
            return Ok(false);
        }
        ends.push((v, p, q));
    }
    if ends.iter().all(|&(_, p, q)| p.distance(q) <= tol * 1e-6) {
        return Ok(false);
    }
    let (Some(&(_, _, qs)), Some(&(_, _, qe))) = (ends.first(), ends.get(1)) else {
        return Ok(false);
    };
    let Some(piece) = piece_between(k, crease, qs, qe)? else {
        return Ok(false);
    };
    // the crease's piece within `tol` of the edge over its whole length (its distance to the
    // edge's line, and the edge's own from that line)
    if !matches!(distance_to_line_bound(&piece, ea, eb), Some(d) if d + straightness <= tol) {
        return Ok(false);
    }
    // what each moved vertex does to the edges through it, decided before anything moves
    let mut trims: Vec<(EdgeId, Curve, Point3, Point3)> = Vec::new();
    for &(v, _, q) in &ends {
        for e2 in vertex::edges(b, v, None)? {
            if e2 == e {
                continue;
            }
            let curve = edge_curve(k, b, e2)?;
            let through = matches!(
                drop_on(k, &curve, &curve.natural_interval(), q)?,
                Some((_, dist)) if dist <= tol * 1e-6
            );
            if !through {
                if record.moved.contains(&e2) {
                    // an edge already on its crease would be left off its own vertex
                    return Ok(false);
                }
                // left for the trim to take off (checked once it has)
                continue;
            }
            let (Some(a), Some(z)) = edge::start_end_vertices(b, e2)? else {
                continue;
            };
            if a == z || trims.iter().any(|(t, _, _, _)| *t == e2) {
                // a closed edge through the vertex: its curve already runs through it; an
                // edge between both moved vertices is trimmed once
                continue;
            }
            let at = |x: crate::topology::VertexId| -> Result<Point3> {
                Ok(ends
                    .iter()
                    .find(|&&(w, _, _)| w == x)
                    .map_or(vertex::point(b, x)?, |&(_, _, q)| q))
            };
            trims.push((e2, curve, at(a)?, at(z)?));
        }
    }
    let mut pieces = Vec::new();
    for (e2, curve, pa, pz) in trims {
        let Some(p) = piece_between(k, &curve, pa, pz)? else {
            return Ok(false);
        };
        pieces.push((e2, p));
    }
    // the move
    record.pieces.push(piece.copy(k)?);
    replace_edge_curve(b, env, e, piece)?;
    for &(v, _, q) in &ends {
        let this = (env.key_of)(EntityRef::Vertex(v));
        let key_of = env.key_of;
        let vu_ref = |vu: crate::topology::VertexuseId| key_of(EntityRef::Vertexuse(vu));
        vertex::set_point(
            b,
            v,
            q,
            &mut *env.records,
            &mut *env.attributes,
            env.context.as_deref_mut(),
            this,
            &vu_ref,
        )?;
    }
    for (e2, p) in pieces {
        replace_edge_curve(b, env, e2, p)?;
    }
    record.vertices.extend(ends.iter().map(|&(v, _, _)| v));
    record.moved.insert(e);
    Ok(true)
}

/// #17633: whether the trim kept geometry that an edge's move onto its crease left off
/// (checked once the faces past the creases are deleted; a `true` is refused). Checked on
/// the geometry, not on ids, so an edge or face the imprint split is still found: an edge
/// still at a moved vertex that does not run through it (within `tol * 1e-6`), or an edge
/// along a crease piece (picked by its midpoint) that does not lie, by the enclosed bound
/// of its control points, on the plane of every face it bounds (a face that is not planar
/// fails).
fn move_leftovers_kept(k: &Kernel, target: &Brep, moves: &MovedEdges, tol: f64) -> Result<bool> {
    let on = tol * 1e-6;
    for &v in &moves.vertices {
        if target.vertex(v).is_err() {
            continue;
        }
        let p = vertex::point(target, v)?;
        for e in vertex::edges(target, v, None)? {
            let curve = edge_curve(k, target, e)?;
            let through = matches!(
                drop_on(k, &curve, &curve.natural_interval(), p)?,
                Some((_, dist)) if dist <= on
            );
            if !through {
                return Ok(true);
            }
        }
    }
    if moves.pieces.is_empty() {
        return Ok(false);
    }
    for e in crate::topology::brep::p1::edges(target, None)? {
        let curve = edge_curve(k, target, e)?;
        let mid = curve.evaluate_point(k, curve.natural_interval().evaluate(0.5))?;
        // picks the edges along a piece (the acceptance below is a bound)
        let mut along = false;
        for piece in &moves.pieces {
            along = matches!(
                drop_on(k, piece, &piece.natural_interval(), mid)?,
                Some((_, dist)) if dist <= on
            );
            if along {
                break;
            }
        }
        if !along {
            continue;
        }
        for g in edge::faces(target, e, None)? {
            let plane = match face::p2::surface(target, g)? {
                Some(surface) => crate::blend::fillet_overflow::plane_of(k, surface)?,
                None => None,
            };
            let on_plane = plane.is_some_and(|(at, normal)| {
                matches!(distance_to_plane_bound(&curve, at, normal), Some(d) if d <= on)
            });
            if !on_plane {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The fraction of the body's tolerance the creases and the open ends' sections are
/// fitted to (#17673). Each becomes an edge between the sheet and a planar face of the
/// body, whose area (and the body's volume) follows the edge's curve while the sheet's
/// side follows its surface: a curve fitted to the body's tolerance (1e-5) lies up to
/// ~3e-6 off the sheet, which moves a box's variable round 1.3e-7 of its volume. A
/// thousandth keeps them within ~1e-8, the sheet surface's own accuracy.
const SECTION_FIT: f64 = 1e-3;

/// The point where `curve` crosses face `f` of `b`'s surface, solved from `start` by
/// Newton's method on the curve's parameter: each step zeroes the distance to the
/// surface's tangent plane at the foot of the current point (the plane itself for a
/// planar face), so it converges quadratically wherever the curve crosses the surface,
/// however shallow the crossing. Accepted only on the solved residual: the point on
/// `curve` when its distance along the normal to the tangent plane at its foot is within
/// `tol * 1e-6`, or within 64 eps of its own coordinates where those ulps are larger
/// (iterated on while that still halves the residual). `None` when the face has no
/// surface, a drop or an evaluation fails, the curve runs along the tangent plane to
/// rounding, or no step within 32 reaches the residual (a crossing past the curve's end,
/// or none): no crossing, so the caller leaves the piece where it ends.
fn crosses_surface(
    k: &Kernel,
    curve: &Curve,
    b: &Brep,
    f: FaceId,
    start: Point3,
    tol: f64,
) -> Result<Option<Point3>> {
    let Some(surface) = face::p2::surface(b, f)? else {
        return Ok(None);
    };
    let plane = crate::blend::fillet_overflow::plane_of(k, surface)?;
    let domain = if plane.is_none() {
        Some(surface.natural_uv_domain_with_nurb(k)?)
    } else {
        None
    };
    let interval = curve.natural_interval();
    let Some((mut t, _)) = drop_on(k, curve, &interval, start)? else {
        return Ok(None);
    };
    let mut found: Option<(f64, Point3)> = None;
    for _ in 0..32 {
        let d = curve.evaluate(k, t, 1, false, false)?;
        let (Some(&p), Some(&dp)) = (d.first(), d.get(1)) else {
            return Ok(None);
        };
        // the foot of `p` on the surface and the unit normal there
        let (foot, normal) = match (plane, &domain) {
            (Some((at, n)), _) => (p - n * (p - at).dot(n), n),
            (None, Some(domain)) => {
                let dropped =
                    surface.drop_point::<NoObject>(k, p, domain, &DropPointOptions::default())?;
                if !dropped.success {
                    return Ok(None);
                }
                let e = surface.evaluate(k, dropped.result_uv, 1, 1, EvaluateFlags::default())?;
                let (Some(&foot), Some(&su), Some(&sv)) = (
                    e.first().and_then(|r| r.first()),
                    e.get(1).and_then(|r| r.first()),
                    e.first().and_then(|r| r.get(1)),
                ) else {
                    return Ok(None);
                };
                let Ok(n) = su.cross(sv).unitize() else {
                    return Ok(None);
                };
                (foot, n)
            }
            (None, None) => return Ok(None),
        };
        // the residual along the normal (a drop's tangential error is not the crossing's);
        // accepted at `tol * 1e-6`, or at the rounding of `p`'s own coordinates where
        // those are so large that their ulps exceed it
        let gap = normal.dot(p - foot).abs();
        let accept = smos_max(
            tol * 1e-6,
            64.0 * f64::EPSILON * smos_max(p.x.abs(), smos_max(p.y.abs(), p.z.abs())),
        );
        // NaN fails every comparison and so is never accepted
        if gap <= accept {
            match found {
                Some((best, _)) if gap > 0.5 * best => break,
                _ => found = Some((gap, p)),
            }
            if gap == 0.0 {
                break;
            }
        }
        let slope = normal.dot(dp);
        let solvable = slope.abs() > 64.0 * f64::EPSILON * dp.length();
        if !solvable {
            break;
        }
        t = smos_min(
            smos_max(t - normal.dot(p - foot) / slope, interval.min()),
            interval.max(),
        );
    }
    Ok(found.map(|(_, p)| p))
}

/// The refusal of a crease piece (its kind `kind`) whose first end with a meet (the start
/// at `start.0` where it has one, else the end at `end.0`) has no construction to put it on
/// its meet: a curve kind with no control polygon, or a B-spline not clamped there, whose
/// end is not its classified end (#17673; never a piece left short of its meet).
fn piece_end_unbuildable(
    kind: &'static str,
    start: (f64, Option<Point3>),
    end: (f64, Option<Point3>),
) -> SolidError {
    let (at_end, t) = match start.1 {
        Some(_) => (false, start.0),
        None => (true, end.0),
    };
    SolidError::blend(
        OP,
        BlendFailure::CreasePieceEndUnbuildable { kind, at_end, t },
    )
}

/// `crease` with each B-spline clamped at both ends: an unclamped one as the same curve
/// over its domain ([`clamped_spline`]), whose natural ends are its real ends; any other
/// as it is. The trim classifies, meets and ends its pieces on this curve, so an end it
/// records is the piece's end (#17673 review: an unclamped crease's natural end is its end
/// control point, off the curve, and the kernel cannot evaluate it inside its domain).
fn crease_as_clamped(crease: &Curve) -> Result<std::borrow::Cow<'_, Curve>> {
    Ok(match crease.kind() {
        CurveKind::BSpline(spline) if !spline_clamped(spline)? => {
            std::borrow::Cow::Owned(Curve::from(clamped_spline(spline)?))
        }
        _ => std::borrow::Cow::Borrowed(crease),
    })
}

/// An invariant of the crease piece's end construction that did not hold (a B-spline
/// with too few knots or control points, or one not clamped where its end is moved):
/// `Unsupported`, never a piece left short of its meet.
const fn piece_end_unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// Whether `spline`'s knots are clamped at both ends (the first and the last `p + 1`
/// equal), so that each end of the curve is its end control point.
fn spline_clamped(spline: &BSplineCurve) -> Result<bool> {
    let nurb = spline.nurb_ref()?;
    Ok(is_clamped(nurb.knots(), nurb.degree()))
}

/// `spline` as a B-spline clamped at both ends: a clamped one as it is
/// ([`BSplineCurve::clone_exact`]). An unclamped one (its first or last `p + 1` knots not
/// all equal) is a curve over its domain `[U_p, U_{m-p}]` only: the kernel's B-spline is
/// clamped by contract, its canonical constructor
/// ([`BSplineCurve::create_canonical`]) reads unclamped knots so, and its evaluation
/// gives the end control point at a natural end `U0` or `Um` but the end span's
/// polynomial just inside it, two different points. Such a curve is clamped to its
/// domain by knot insertion ([`clamp_curve`], `N_IgesClampCrv`, as the canonical
/// constructor does), which leaves every point of the domain where it is, so its ends
/// are the domain's end points and its end control points.
fn clamped_spline(spline: &BSplineCurve) -> Result<BSplineCurve> {
    if spline_clamped(spline)? {
        return Ok(spline.clone_exact());
    }
    let nurb = spline.nurb_ref()?;
    let p = nurb.degree();
    let u = nurb.knots();
    let at = |i: Option<usize>| {
        i.and_then(|i| u.get(i))
            .copied()
            .ok_or_else(|| piece_end_unsupported("a crease piece's B-spline with too few knots"))
    };
    let domain = [at(Some(p))?, at(u.len().checked_sub(p + 1))?];
    let mut clamped = nurb.clone();
    clamp_curve(&mut clamped, domain)?;
    Ok(BSplineCurve::from_gw_nurb(spline.dim(), &clamped))
}

/// `spline`'s end (`at_end`: the last; else the first) put at `q` by moving its end
/// control point, with its weight. A clamped end is that control point, whose rational
/// basis function lies in `[0, 1]` and vanishes past the end span, so the end lands on
/// `q` and no point of the curve moves farther than `|q - end|`, and only the end span
/// moves. `Unsupported` for a B-spline not clamped at both ends (never one from
/// [`clamped_spline`]).
fn move_spline_end(spline: &mut BSplineCurve, at_end: bool, q: Point3) -> Result<()> {
    if !spline_clamped(spline)? {
        return Err(piece_end_unsupported(
            "a crease piece's B-spline end that is not its control point",
        ));
    }
    let idx = if at_end {
        spline
            .number_control_points()?
            .checked_sub(1)
            .ok_or_else(|| {
                piece_end_unsupported("a crease piece's B-spline with no control point")
            })?
    } else {
        0
    };
    let (_, w) = spline.control_point(ControlPointForm::EuclideanRational, idx)?;
    spline.set_control_point(ControlPointForm::EuclideanRational, idx, q, w)?;
    Ok(())
}

/// #17673: [`crate::blend::fillet_overflow::pieces_in_face`] for a crease on target face
/// `f`, each piece run on to the crease's own end (start or end) when every run between
/// the piece and that end is classified along an edge of the face (within `tol` of it).
/// A crease of a sheet tangent to the face beside `f` (a round rolling on it) meets `f`'s
/// edge there at a shallow angle: it comes within `tol` of that edge a run before its
/// end, which lies on the edge (the sheet's rail end). Clipped where it comes within
/// `tol`, the piece stops short of the corner by that run, and the sheet's boundary no
/// longer closes there.
///
/// The run-on is not limited to that approach: any stretch from a piece to the crease's
/// end that lies along the face's edges is taken into the piece, whether it is a shallow
/// approach or a crease that runs along an edge to its end. Either way the sheet is split
/// along the same stretch as the piece and its along-edge runs together (WR-717), now as
/// one curve. A stretch along an edge followed by another run inside the face, or a
/// crease that ends off the face's boundary, is not run on: those pieces are clipped as
/// before.
///
/// The crease's along-edge runs ([`crate::blend::fillet_overflow::pieces_along_edges`],
/// from the same classification) are appended to `along`, each marked whether a piece was
/// run on over it, so that the caller splits the sheet along each stretch once.
fn pieces_in_face_to_ends(
    k: &Kernel,
    crease: &Curve,
    b: &mut Brep,
    f: FaceId,
    tol: f64,
    pieces: &mut Vec<Curve>,
    along: &mut Vec<(Curve, bool)>,
) -> Result<Vec<(EdgeId, f64)>> {
    let natural = crease.natural_interval();
    let runs = face::p2::classify_free_curve_on_face(k, b, f, crease, natural, tol, true)?;
    // which runs a piece was run on over
    let mut taken = vec![false; runs.len()];
    for (i, run) in runs.iter().enumerate() {
        if run.mid != PointClassType::Face {
            continue;
        }
        let (mut lo, mut hi) = (run.interval.min(), run.interval.max());
        let (mut on_before, mut on_after) = (false, false);
        // the runs before it, all along an edge, reaching the crease's start
        let before = runs.get(..i).unwrap_or_default();
        if !before.is_empty()
            && before.iter().all(|r| r.mid == PointClassType::Edge)
            && before
                .first()
                .is_some_and(|r| r.interval.min() <= natural.min())
        {
            lo = natural.min();
            on_before = true;
        }
        let after = runs.get(i + 1..).unwrap_or_default();
        if !after.is_empty()
            && after.iter().all(|r| r.mid == PointClassType::Edge)
            && after
                .last()
                .is_some_and(|r| r.interval.max() >= natural.max())
        {
            hi = natural.max();
            on_after = true;
        }
        let a = crease.evaluate_point(k, lo)?;
        let z = crease.evaluate_point(k, hi)?;
        if a.distance(z) <= tol && !crease.is_closed(k, &natural, tol)? {
            continue;
        }
        let mut piece = crease.copy(k)?;
        let ivl = crate::geom::extent1d::Extent1d::from_min_max(lo, hi);
        if ivl != piece.natural_interval() {
            let mut trim = ivl;
            piece.trim(
                k,
                &mut trim,
                crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
            )?;
        }
        pieces.push(piece);
        if on_before {
            taken.iter_mut().take(i).for_each(|t| *t = true);
        }
        if on_after {
            taken.iter_mut().skip(i + 1).for_each(|t| *t = true);
        }
    }
    // `pieces_along_edges`, step for step, on the same runs
    for (run, &taken) in runs.iter().zip(&taken) {
        if run.mid != PointClassType::Edge {
            continue;
        }
        let ivl = run.interval;
        let a = crease.evaluate_point(k, ivl.min())?;
        let z = crease.evaluate_point(k, ivl.max())?;
        if a.distance(z) <= tol && !crease.is_closed(k, &natural, tol)? {
            continue;
        }
        let mut piece = crease.copy(k)?;
        if ivl != piece.natural_interval() {
            let mut trim = ivl;
            piece.trim(
                k,
                &mut trim,
                crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
            )?;
        }
        along.push((piece, taken));
    }
    let mut events: Vec<(EdgeId, f64)> = runs
        .iter()
        .flat_map(|run| {
            [
                run.start.1.zip(run.start_edge_param),
                run.end.1.zip(run.end_edge_param),
            ]
            .into_iter()
            .flatten()
        })
        .collect();
    // A run end classified at a vertex names no edge, yet the crease leaves the face there
    // across each of the face's own edges that meet it: each is an event, at the parameter
    // of the vertex on it. Edges at the vertex that bound other faces only (a seam of the
    // face next door, between two uses of that one face) separate no face from this one,
    // so the crease crosses none of them out of this face (#2385's disc: a crease along
    // the bottom edge through the vertex at the foot of the side face's seam).
    for v in runs
        .iter()
        .flat_map(|run| [run.start_vertex, run.end_vertex])
        .flatten()
    {
        for e in vertex::edges(b, v, Some(f))? {
            let at_start = edge::start_vertex(b, e)? == Some(v);
            let range = edge::interval(b, e)?;
            let parameter = if at_start { range.min() } else { range.max() };
            if !events.contains(&(e, parameter)) {
                events.push((e, parameter));
            }
        }
    }
    Ok(events)
}

/// Whether `q`, the crossing of `edge_curve` with the face, is the meet this crease stops
/// short of. The classification ends the piece where it is still `tol` from the edge, and
/// a shallow crossing (angle `theta` between the two tangents) is up to `2 tol / sin(theta)`
/// further on: each curve lies in a tube of radius `tol`, and those tubes meet that far
/// along the edge. The closest point of the crease to `q` is this end, so the curve does
/// not already pass the crossing, and the gap is shorter than the piece, so the meet is
/// not the crossing at the piece's other end.
#[allow(clippy::too_many_arguments)]
fn short_of_shallow_crossing(
    k: &Kernel,
    crease: &Curve,
    edge_curve: &Curve,
    t: f64,
    end: Point3,
    q: Point3,
    tq: f64,
    tol: f64,
    other: Point3,
) -> Result<bool> {
    let foot = crease.evaluate_point(k, tq)?;
    if foot.distance(end) > tol {
        return Ok(false);
    }
    let gap = q.distance(end);
    if gap <= tol || gap >= end.distance(other) {
        return Ok(false);
    }
    let c_der = crease.evaluate(k, t, 1, false, false)?;
    let Some(c_tan) = c_der.get(1).copied() else {
        return Ok(false);
    };
    let Some((et, _)) = drop_on(k, edge_curve, &edge_curve.natural_interval(), end)? else {
        return Ok(false);
    };
    let e_der = edge_curve.evaluate(k, et, 1, false, false)?;
    let Some(e_tan) = e_der.get(1).copied() else {
        return Ok(false);
    };
    let (c_len, e_len) = (c_tan.length(), e_tan.length());
    if c_len <= 0.0 || e_len <= 0.0 {
        return Ok(false);
    }
    let sin_theta = c_tan.cross(e_tan).length() / (c_len * e_len);
    Ok(sin_theta > 0.0 && gap * sin_theta <= 2.0 * tol)
}

/// #17673: the pieces of the crease `crease` (on face `f` of `target`) that lie inside the
/// sheet face `s` (its runs classified in the face, as [`crate::blend::fillet_overflow::pieces_in_face`]), each end that
/// the classification puts on a boundary edge of the sheet face (one no other sheet face
/// shares) ended where that edge's curve crosses `f`'s surface ([`crosses_surface`]):
/// the merge then joins the two at a point both curves pass through, and the edge is split
/// on its own curve (it was split where the piece's end dropped onto it, up to `tol` off,
/// and kept its curve short of its new vertex).
///
/// The meet is taken when it lies on the crease (within `tol`) at a parameter between the
/// end and the far end of the runs beyond it that the classification puts along that same
/// edge, `tol` past those, or within `tol` of the end. Where the crease runs into the edge
/// at a shallow angle (a sheet tangent to the face the edge lies on), the classification
/// ends the piece where the crease comes within `tol` of the edge, short of their meet by
/// up to the length of that run. The piece is trimmed to the meet's parameter and ends
/// exactly on the meet ([`piece_ending_at`], by a construction chosen from its geometry;
/// a typed refusal for a curve kind it has none for). An end with no such meet is left as
/// classified. Where the two meets come out of order (each past the other), the piece
/// keeps both its classified ends (as before #17673).
fn pieces_ending_at_meets(
    k: &Kernel,
    crease: &Curve,
    sheet: &mut Brep,
    s: FaceId,
    target: &Brep,
    f: FaceId,
    tol: f64,
    pieces: &mut Vec<Curve>,
) -> Result<()> {
    // classified, met and ended on the crease clamped, so every recorded end is its piece's
    let clamped = crease_as_clamped(crease)?;
    let crease: &Curve = &clamped;
    let natural = crease.natural_interval();
    let runs = face::p2::classify_free_curve_on_face(k, sheet, s, crease, natural, tol, true)?;
    for (i, run) in runs.iter().enumerate() {
        if run.mid != PointClassType::Face {
            continue;
        }
        let ivl = run.interval;
        let a = crease.evaluate_point(k, ivl.min())?;
        let z = crease.evaluate_point(k, ivl.max())?;
        if a.distance(z) <= tol && !crease.is_closed(k, &natural, tol)? {
            continue;
        }
        // each end: its parameter, the boundary edge it lies on, and the parameter the
        // runs beyond it along that edge reach
        let mut ends: [(f64, Option<Point3>); 2] = [(ivl.min(), None), (ivl.max(), None)];
        for (side, slot) in ends.iter_mut().enumerate() {
            let (t, (_, on_edge)) = if side == 0 {
                (ivl.min(), run.start)
            } else {
                (ivl.max(), run.end)
            };
            let Some(e) = on_edge else {
                continue;
            };
            if !edge::is_lamina(sheet, e)? {
                continue;
            }
            // the runs beyond this end along the same edge
            let mut reach = t;
            let mut j = i;
            loop {
                let next = if side == 0 {
                    j.checked_sub(1)
                } else {
                    Some(j + 1)
                };
                let Some(n) = next else { break };
                let Some(r) = runs.get(n) else { break };
                let along = r.mid == PointClassType::Edge
                    && (if side == 0 { r.end.1 } else { r.start.1 }) == Some(e);
                if !along {
                    break;
                }
                reach = if side == 0 {
                    r.interval.min()
                } else {
                    r.interval.max()
                };
                j = n;
            }
            let end = crease.evaluate_point(k, t)?;
            let edge_curve = edge_curve(k, sheet, e)?;
            let Some(q) = crosses_surface(k, &edge_curve, target, f, end, tol)? else {
                continue;
            };
            let Some((tq, d)) = drop_on(k, crease, &natural, q)? else {
                continue;
            };
            let on_crease = d <= tol;
            let (lo, hi) = if t <= reach { (t, reach) } else { (reach, t) };
            let within = (tq >= lo && tq <= hi) || q.distance(end) <= tol || {
                let past = crease.evaluate_point(k, tq)?;
                let at_reach = crease.evaluate_point(k, reach)?;
                past.distance(at_reach) <= tol
            };
            if on_crease && within {
                *slot = (tq, Some(q));
            } else if short_of_shallow_crossing(
                k,
                crease,
                &edge_curve,
                t,
                end,
                q,
                tq,
                tol,
                crease.evaluate_point(k, if side == 0 { ivl.max() } else { ivl.min() })?,
            )? {
                // the classified parameter: the curve's domain stops at this end, and
                // moving that control point lands it on the crossing
                *slot = (t, Some(q));
            }
        }
        let [start, end] = ends;
        let classified = ((ivl.min(), None), (ivl.max(), None));
        // two meets out of order (each end's meet past the other's): the ends as
        // classified
        let (start, end) = if start.0 < end.0 {
            (start, end)
        } else {
            classified
        };
        pieces.push(piece_ending_at(k, crease, start, end, tol)?);
    }
    Ok(())
}

/// `crease` trimmed to `start.0 .. end.0`, each end that has a meet ending exactly on it,
/// by a construction chosen from the piece's geometry:
/// - a line: the line between its two ends, each meet in place of its end (a line still,
///   every point within `|q - end|` of where it was); `Degenerate` where the two are within
///   `tol` of each other;
/// - a B-spline, or a conic (an ellipse or circle, hyperbola or parabola, taken as its
///   exact rational B-spline, the kernel's own NURB of it), clamped at both ends: each
///   end's control point moved onto its meet ([`move_spline_end`]). One not clamped (its
///   natural end a control point off the curve, not the end the caller classified) is
///   refused ([`BlendFailure::CreasePieceEndUnbuildable`]); the trim's creases are clamped
///   before they are classified ([`crease_as_clamped`]);
/// - any other kind (a curve with no control polygon):
///   [`BlendFailure::CreasePieceEndUnbuildable`], naming the kind and the first end with a
///   meet, never a piece left ending short of its meet.
///
/// Both meets are applied, or none and the error: a meet taken at one end is never
/// dropped because of the other. A piece with no meet is the trimmed crease as it is.
fn piece_ending_at(
    k: &Kernel,
    crease: &Curve,
    start: (f64, Option<Point3>),
    end: (f64, Option<Point3>),
    tol: f64,
) -> Result<Curve> {
    let mut piece = crease_piece(k, crease, start.0, end.0)?;
    if start.1.is_none() && end.1.is_none() {
        return Ok(piece);
    }
    match piece.kind() {
        CurveKind::Line(_) => {
            let ivl = piece.natural_interval();
            let a = match start.1 {
                Some(q) => q,
                None => piece.evaluate_point(k, ivl.min())?,
            };
            let z = match end.1 {
                Some(q) => q,
                None => piece.evaluate_point(k, ivl.max())?,
            };
            // the meets within `tol` of each other: no line between them
            if a.distance(z) <= tol {
                return Err(SolidError::Degenerate {
                    operation: OP,
                    entity: None,
                });
            }
            let mut line = Line::from_points(a, z, piece.dim());
            line.make_nurb()?;
            Ok(Curve::from(line))
        }
        CurveKind::BSpline(_)
        | CurveKind::Ellipse(_)
        | CurveKind::Hyperbola(_)
        | CurveKind::Parabola(_) => {
            // a B-spline is edited in place; a conic is replaced by its exact NURB. Either
            // must be clamped at both ends: an unclamped one's ends are not its classified
            // ends ([`crease_as_clamped`] clamps the trim's creases first)
            let replacement = {
                let base = piece.b_spline_base().ok_or_else(|| {
                    piece_end_unsupported("a crease piece with no B-spline of its own")
                })?;
                if !spline_clamped(base)? {
                    let kind = match piece.kind() {
                        CurveKind::BSpline(_) => "unclamped B-spline",
                        _ => "conic with an unclamped NURB",
                    };
                    return Err(piece_end_unbuildable(kind, start, end));
                }
                match piece.kind() {
                    CurveKind::BSpline(_) => None,
                    _ => Some(base.clone_exact()),
                }
            };
            if let Some(nurb) = replacement {
                piece = Curve::from(nurb);
            }
            let CurveKind::BSpline(spline) = piece.kind_mut() else {
                return Err(piece_end_unsupported(
                    "a crease piece with no B-spline of its own",
                ));
            };
            if let Some(q) = start.1 {
                move_spline_end(spline, false, q)?;
            }
            if let Some(q) = end.1 {
                move_spline_end(spline, true, q)?;
            }
            Ok(piece)
        }
        CurveKind::Hermite(_)
        | CurveKind::Projected(_)
        | CurveKind::Iso(_)
        | CurveKind::Offset(_)
        | CurveKind::Conic(_)
        | CurveKind::CrvInVolume(_)
        | CurveKind::CrvOnSurf(_)
        | CurveKind::TangentField(_)
        | CurveKind::Composite(_) => Err(piece_end_unbuildable(piece.kind().name(), start, end)),
    }
}

/// `crease` trimmed to `t0 .. t1`.
fn crease_piece(k: &Kernel, crease: &Curve, t0: f64, t1: f64) -> Result<Curve> {
    let mut piece = crease.copy(k)?;
    let trim_to = crate::geom::extent1d::Extent1d::from_min_max(t0, t1);
    if trim_to != piece.natural_interval() {
        let mut trim = trim_to;
        piece.trim(
            k,
            &mut trim,
            crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
        )?;
    }
    Ok(piece)
}

/// The refusal of a crease on the target face `f` whose rail leaves a face it rolls on (one
/// of `rolled_supports`) across a tangent edge, with that face; `None` when no such edge is
/// crossed. A face past a cliff meets the face the rail leaves at a sharp edge; one that
/// runs on from it tangentially (a round already on the face) is no cliff: the rail would
/// roll over onto it, and the ball does not fit between.
///
/// The edges the crease crosses (`seam_events`, each with the parameter it crosses at,
/// each edge of `f` at a vertex it crosses through included) are judged where it crosses them,
/// against the round's section budget `tangent_budget` (a spline seam's tangency varies
/// along it). The face's other edges are judged as ever: at a fixed point of the edge,
/// against the solvers' tangency tolerance `face_tangency`.
fn crease_across_tangent_edge(
    k: &Kernel,
    target: &mut Brep,
    f: FaceId,
    seam_events: &[(EdgeId, f64)],
    rolled_supports: &BTreeSet<FaceId>,
    tangent_budget: f64,
    face_tangency: f64,
) -> Result<Option<(FaceId, Refusal)>> {
    let mut judged: Vec<(EdgeId, f64, f64)> = seam_events
        .iter()
        .map(|&(e, parameter)| (e, parameter, tangent_budget))
        .collect();
    for e in face::p4::edges(target, f, None, false, None)? {
        if !seam_events.iter().any(|&(crossed, _)| crossed == e) {
            let parameter = edge::interval(target, e)?.evaluate(0.34);
            judged.push((e, parameter, face_tangency));
        }
    }
    for (e, parameter, budget) in judged {
        for g in edge::faces(target, e, None)? {
            if g != f
                && rolled_supports.contains(&g)
                && is_tangent_edge(k, target, e, parameter, budget)?
            {
                return Ok(Some((
                    g,
                    refusal(
                        FilletErrorType::NoRoom,
                        Some(NoRoomReason::CreaseAcrossTangentEdge),
                        "Fillet Error: a rail leaves the face it rolls on across a tangent edge, onto a face it would cut (the blend is larger than the face allows)",
                    ),
                )));
            }
        }
    }
    Ok(None)
}

/// A refusal with its fillet error and typed reason.
const fn refusal(
    code: FilletErrorType,
    reason: Option<NoRoomReason>,
    comment: &'static str,
) -> Refusal {
    Refusal {
        code,
        reason,
        comment,
    }
}

impl SessionObjects<'_> {
    /// Whether the intermediate sheet has an open G1 setback hole.
    fn has_open_setback(&self, executive: &FilletExecutive<FilletObject>) -> bool {
        executive.corners.iter().any(|c| {
            matches!(c, FilletObject::Corner(id)
                if self.corners.get(id.0 as usize).is_some_and(FilletCorner::is_setback_open))
        })
    }

    /// Bound every open-ended sheet before sewing: sheets on the same continuation
    /// may overlap away from their own end planes, where they must never share topology.
    /// The retained faces remain the owning faces of their fillet geometries.
    #[allow(clippy::too_many_lines)] // the existing end cuts and section caps form one stage
    pub(super) fn bound_open_fillet_ends(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        if !executive.cliff_overflow
            && !executive.open_ends.iter().any(|end| end.section)
            && !self.has_open_setback(executive)
        {
            return Ok(());
        }
        let solvers = executive.solvers.clone();
        for &solver in &solvers {
            if !self.holds_its_law(k, solver)? {
                executive.note_trim_refusal(refusal(
                    FilletErrorType::NoRoom,
                    Some(NoRoomReason::CliffLawNotHeld),
                    "Fillet Error: a variable round traced on its faces' continuations does not hold its radius law",
                ));
                return Err(no_room(NoRoomReason::CliffLawNotHeld));
            }
        }

        let mut faces_of = BTreeMap::new();
        let mut approx = Vec::new();
        for &solver in &solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            approx.push(self.solver(id)?.base().approx_tol_3d);
            faces_of.insert(solver, self.fillet_faces(solver)?);
        }
        let open_ends = executive.open_ends.clone();
        let body = self.intermediate_identity(EntityRef::Brep(BrepId::from_index(0)))?;
        let mut sheet = self.intermediate.take().ok_or_else(|| {
            defect(
                "the session has no fillet body when its open ends are bounded: the \
                     executive bounds the open ends of the fillet body it has just made",
            )
        })?;
        let bounded = (|| {
            // the ends the corners left uncut, cut off by their end faces' surfaces continued
            // (an end's event: the end face cuts the blend whatever lies past it, a lip over a
            // wall's top)
            let mut note = None;
            let mut section_ends: Vec<SectionEnd> = Vec::new();
            // each section end's solver, whose round it ends (#2314)
            let mut section_solvers: Vec<FilletObject> = Vec::new();
            for end in &open_ends {
                let FilletObject::Solver(id) = end.solver else {
                    return Err(invalid("open end solver is not a solver"));
                };
                let (
                    FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v)),
                    FilletObject::Topology(FilletBody::Target, EntityRef::Face(end_face)),
                ) = (end.vertex, end.end_face)
                else {
                    return Err(invalid("open end is not target topology"));
                };
                let base = self.solver(id)?.base();
                let solver_tol = base.approx_tol_3d;
                let target = &*self.target;
                let eu = base.edgeuse(0)?.ok_or_else(|| null_defect(SOLVER_EDGE))?;
                let fil_edge = eu1::edge(target, eu)?.ok_or_else(|| null_defect(SOLVER_EDGE))?;
                let edge_mid = edge::curve(target, fil_edge)?
                    .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?
                    .evaluate_point(k, edge::interval(target, fil_edge)?.evaluate(0.5))?;
                let vertex_point = vertex::point(target, v)?;
                let end_surface = if end.section {
                    // WR-706: a section end (WR-674) is cut by the section through the vertex,
                    // normal to the filleted edge
                    let section = section_end(k, target, fil_edge, v)?.ok_or_else(|| {
                        SolidError::blend(
                            OP,
                            BlendFailure::CreaseTrimNotBuilt(
                                CreaseTrimStop::NoSectionThroughEndVertex,
                            ),
                        )
                    })?;
                    section_ends.push(section);
                    section_solvers.push(end.solver);
                    Surface::from(Plane::from_normal(vertex_point, section.normal))
                } else {
                    face::p2::surface(target, end_face)?
                        .ok_or_else(|| input_missing(OverflowInput::FaceSurface))?
                        .clone_for_evaluation()?
                };
                let entry = faces_of.get_mut(&end.solver).ok_or_else(|| {
                    null_defect(
                        "an open end's solver has no fillet faces: each open end is noted \
                             for a solver of the executive, and every solver's faces are read \
                             before the ends are bounded",
                    )
                })?;
                let mut faces: Vec<Option<FaceId>> = entry.iter().map(|&(_, f)| f).collect();
                let cut = cut_fillet_at_open_end(
                    k,
                    &mut sheet,
                    &mut faces,
                    &end_surface,
                    vertex_point,
                    edge_mid,
                    solver_tol,
                    smos_min(solver_tol, self.target.tolerance()) * SECTION_FIT,
                    &mut note,
                );
                for (slot, f) in entry.iter_mut().zip(faces) {
                    if slot.1 != f {
                        self.pseudo.geom_mut(slot.0)?.fillet_surface =
                            f.map(|face| FilletSurfaceStorage::ManagedFace { body, face });
                        slot.1 = f;
                    }
                }
                // not the fork's (#2401, WR-696): an open end the end face does not cut is an
                // overflow's, absent below the size at which the end's blend stays on its
                // faces: that size (open)
                if note.is_some() && cut.is_err() {
                    executive.note_fit_on_its_faces(k, self, end.solver)?;
                }
                if let Some(refused) = note.take() {
                    executive.note_trim_refusal(refused);
                }
                cut?;
            }

            // WR-706: each section end closed by its flat face, from the round's section to the
            // vertex (WR-674's, as the subtraction closes it)
            if !section_ends.is_empty() {
                let mut tol = 0.0;
                for &a in &approx {
                    tol = smos_max(tol, a);
                }
                for (end, solver) in section_ends.iter().zip(&section_solvers) {
                    // the end's own round's faces, as the cut left them
                    let faces: Vec<FaceId> = faces_of
                        .get(solver)
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter_map(|&(_, f)| f)
                        .collect();
                    let Some(section) = section_edge(k, &sheet, end, &faces, 10.0 * tol)? else {
                        executive.note_trim_refusal(refusal(
                        FilletErrorType::NoRoom,
                        Some(NoRoomReason::TangentContinuationEndUnsupported),
                        "Fillet Error: a round ending where its edge runs on tangentially has no single section there",
                    ));
                        return Err(no_room(NoRoomReason::TangentContinuationEndUnsupported));
                    };
                    section_end_face(k, &mut sheet, end, section, 10.0 * tol)?;
                }
            }

            Ok(())
        })();
        // Standalone cuts return geometry under the body's naming scheme. The live
        // session stitches and shrinks under its issued identities, including the
        // retained faces recorded by ManagedFace above. Restore those identities on
        // failure too, preserving the boundary construction's original error.
        let rebound = (|| {
            let serial = sheet
                .notify_serial()
                .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
            let naming = sheet.naming();
            p3::rebind_geometry_owners(
                &mut sheet,
                |e| crate::topology::scheme_owner_identity(naming, e),
                |e| p3::topology_ref(serial, e),
            )
        })();
        self.intermediate = Some(sheet);
        bounded.and(rebound)
    }
}

impl FilletSession<'_> {
    /// `SmFilletExecutive::HasOpenSetBack` (the fork's ef57570): whether a corner is an
    /// open G1 setback (its hole made by the trim's imprint, no patches).
    pub(super) fn has_open_setback(&self) -> bool {
        self.objects.has_open_setback(&self.executive)
    }

    /// The open G1 setback corners' spring curves (`SM_FE_PRECOMPUTED` edges with a
    /// curve), each with the face it lies on, and the edges their holes leave free: `3 n`
    /// for each (its `n` sections and `2 n` spring halves).
    fn open_setback_springs(&self) -> Result<(Vec<(FaceId, Curve)>, u32)> {
        let mut springs = Vec::new();
        let mut free = 0_u32;
        for c in &self.executive.corners {
            let FilletObject::Corner(id) = *c else {
                continue;
            };
            let Some(corner) = self
                .objects
                .corners
                .get(id.0 as usize)
                .filter(|corner| corner.is_setback_open())
            else {
                continue;
            };
            for &e in &corner.edges {
                let fe = self.objects.pseudo.fillet_edge(e)?;
                let Some(face) = fe.original_face() else {
                    continue;
                };
                if fe.fillet_edge_type() != FilletEdgeType::Precomputed {
                    continue;
                }
                if let Some(curve) = edge::curve(self.objects.pseudo.brep(), e)? {
                    springs.push((face, curve.clone()));
                }
            }
            if let Some(all) = corner.all_eus.as_ref() {
                let n = u32::try_from(all.len()).map_err(|_| invalid("corner too large"))?;
                free = free.saturating_add(n.saturating_mul(3));
            }
        }
        Ok((springs, free))
    }

    /// The fork's fit for a blend whose corners take the whole of its edge (a3d6d81): for a
    /// constant ball, the edge's length over what its corners take per unit radius at both
    /// ends (`sm_CornerTakesPerRadius`, over the solvers of the same radius), less the fit
    /// margin, noted where some corner takes any. Returns the refusal's reason: corners that
    /// take the edge, or, where no corner takes any of it, an overflow that keeps nothing.
    fn note_fit_of_corners_taking_the_edge(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        solvers: &[FilletObject],
    ) -> Result<NoRoomReason> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self.objects.solver(id)?;
        let Some(eu) = current.base().edgeuse(0)? else {
            return Ok(NoRoomReason::CornersTakeWholeEdge);
        };
        let Some(e) = eu1::edge(self.objects.target, eu)? else {
            return Ok(NoRoomReason::CornersTakeWholeEdge);
        };
        let (rolls, on_edge) = super::ball_regular::rolls_a_ball(current);
        if !rolls || on_edge {
            return Ok(NoRoomReason::CornersTakeWholeEdge);
        }
        let offset = |o: &super::SolverObject| -> Result<f64> {
            Ok(o.base()
                .surface(0)?
                .ok_or_else(|| null_defect(SOLVER_OFFSET))?
                .surface
                .offset_distance())
        };
        let own = offset(current)?;
        let mut blended = BTreeSet::new();
        for &other in solvers {
            let FilletObject::Solver(oid) = other else {
                return Err(invalid("object is not a solver"));
            };
            let o = self.objects.solver(oid)?;
            let (r, law) = super::ball_regular::rolls_a_ball(o);
            if r && !law && (offset(o)? - own).abs() <= crate::math::EFF_ZERO * (1.0 + own.abs()) {
                if let Some(oeu) = o.base().edgeuse(0)? {
                    if let Some(oe) = eu1::edge(self.objects.target, oeu)? {
                        blended.insert(oe);
                    }
                }
            }
        }
        let target: &Brep = self.objects.target;
        let v0 = edge::vertex(target, e)?;
        let v1 = edge::other_vertex(target, e, v0)?;
        let mut per = [0.0_f64; 2];
        for (slot, v) in per.iter_mut().zip([v0, v1]) {
            if let Some(v) = v {
                *slot = super::fit_size::corner_takes_per_radius(k, target, v, e, &blended)?;
            }
        }
        let curve = edge::evaluation_curve(target, e)?
            .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
        let length = curve.length(k, &edge::interval(target, e)?, crate::math::EFF_ZERO)?;
        if per[0] + per[1] > 0.0 {
            let margin = 1000.0 * edge::tolerance(target, e)?;
            let fit = smos_max(0.0, length / (per[0] + per[1]) - margin);
            self.executive.note_fit_radius(
                fit,
                Some(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edge(e),
                )),
                crate::blend::fillet_executive::FitKind::Radius,
            );
            Ok(NoRoomReason::CornersTakeWholeEdge)
        } else {
            // not the fork's (#15250): a knife edge's equal chamfer between a plane and a
            // cylinder tangent to it keeps nothing past the distance its chord's sweep takes
            // the whole body at, computed from the geometry; open, so less the fit margin.
            // The radius that fits is its distance (`FilletSession::chamfer_fit`). Named
            // only below the size asked: a chamfer that keeps nothing short of that limit
            // fails for another cause, and keeps the overflow's own bound.
            let margin = 1000.0 * edge::tolerance(target, e)?;
            let knife = if self.objects.knife_chamfers.contains(&e) {
                super::fit_size::knife_chamfer_keeps_nothing(k, target, e)?
                    .map(|limit| smos_max(0.0, limit - margin))
                    .filter(|&fit| fit < own.abs())
            } else {
                None
            };
            if let Some(fit) = knife {
                self.executive.note_fit_radius(
                    fit,
                    Some(FilletObject::Topology(
                        FilletBody::Target,
                        EntityRef::Edge(e),
                    )),
                    crate::blend::fillet_executive::FitKind::Radius,
                );
            } else {
                // not the fork's (#2401): no corner takes any of the edge (its ends are open
                // ends the round runs out to), so the trim took it all: the overflow's own
                // bound, where its rails stay on their faces
                self.executive
                    .note_fit_on_its_faces(k, &mut self.objects, solver)?;
            }
            Ok(NoRoomReason::OverflowKeepsNothing)
        }
    }

    /// Not the fork's (#2404): whether an edge of the target other than the blended ones runs
    /// along the sheet just off it, on the side the sheet keeps: at its quarter points off
    /// the sheet by more than the body's tolerance (on the sheet, it is a crease) and less
    /// than the trim's `tol` and the body's together, a sheet vertex's and a body vertex's
    /// tolerances, across which the strip's two sides are one. An edge on the side of the
    /// blended edges goes with the material they take off (a crease that stands for it,
    /// WR-717), and leaves no strip.
    fn strip_below_resolution(
        &self,
        k: &Kernel,
        sheet: &Brep,
        blended: &[EdgeId],
        tol: f64,
    ) -> Result<bool> {
        let target = &*self.objects.target;
        let body_tol = target.tolerance();
        // answered to the trim's tolerance: a reopened round's net holds it (#2510)
        let _scope = NetScope::within(k, tol);
        let sheet_faces = crate::topology::brep::p1::faces(sheet, None)?;
        let mut taken = Vec::new();
        for &e in blended {
            let curve = edge_curve(k, target, e)?;
            taken.push(curve.evaluate_point(k, curve.natural_interval().evaluate(0.5))?);
        }
        // the signed drop of `p` on face `s`: its gap, and its side of the surface
        let drop_on_face = |s: FaceId, p: Point3| -> Result<Option<(f64, f64)>> {
            let surface = face::p2::surface(sheet, s)?
                .ok_or_else(|| input_missing(OverflowInput::FaceSurface))?;
            let domain = face::p2::uv_domain(sheet, s)?;
            let dropped =
                surface.drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())?;
            if !dropped.success {
                return Ok(None);
            }
            let normal = surface.evaluate_normal(k, dropped.result_uv, Side::Left, Side::Left)?;
            let foot = surface.evaluate_point(k, dropped.result_uv)?;
            Ok(Some((dropped.gap, (p - foot).dot(normal))))
        };
        for e in crate::topology::brep::p1::edges(target, None)? {
            if blended.contains(&e) {
                continue;
            }
            let Some(curve) = edge::curve(target, e)? else {
                continue;
            };
            let ivl = edge::interval(target, e)?;
            let mut along = true;
            for quarter in [0.25, 0.5, 0.75] {
                let p = curve.evaluate_point(k, ivl.evaluate(quarter))?;
                let mut nearest: Option<(FaceId, f64, f64)> = None;
                for &s in &sheet_faces {
                    if let Some((gap, side)) = drop_on_face(s, p)? {
                        if nearest.is_none_or(|(_, best, _)| gap < best) {
                            nearest = Some((s, gap, side));
                        }
                    }
                }
                let Some((s, gap, side)) = nearest else {
                    along = false;
                    break;
                };
                if gap <= body_tol || gap >= tol + body_tol {
                    along = false;
                    break;
                }
                let mut toward = None;
                let mut closest = f64::INFINITY;
                for &m in &taken {
                    let d = m.distance(p);
                    if d < closest {
                        closest = d;
                        toward = Some(m);
                    }
                }
                let Some(m) = toward else {
                    along = false;
                    break;
                };
                let kept = drop_on_face(s, m)?.is_some_and(|(_, other)| other * side < 0.0);
                if !kept {
                    along = false;
                    break;
                }
            }
            if along {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The fit of [`Self::strip_below_resolution`]'s refusal: the least constant ball of the
    /// blends, less the fit margin of a blended edge (a thousand of its tolerances). None
    /// where no constant ball rolls.
    fn note_fit_of_the_strip(
        &mut self,
        solvers: &[FilletObject],
        blended: &[EdgeId],
    ) -> Result<()> {
        let mut size = f64::INFINITY;
        for &solver in solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let object = self.objects.solver(id)?;
            let (rolls, on_law) = super::ball_regular::rolls_a_ball(object);
            if rolls && !on_law {
                let surface = object
                    .base()
                    .surface(0)?
                    .ok_or_else(|| null_defect(SOLVER_OFFSET))?;
                size = smos_min(size, surface.surface.offset_distance().abs());
            }
        }
        let Some(&e) = blended.first() else {
            return Ok(());
        };
        if size.is_finite() {
            let margin = 1000.0 * edge::tolerance(self.objects.target, e)?;
            self.executive.note_fit_radius(
                smos_max(0.0, size - margin),
                Some(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edge(e),
                )),
                crate::blend::fillet_executive::FitKind::Radius,
            );
        }
        Ok(())
    }

    /// Runs `edit` on the target through the session's notifications (its records and
    /// attribute store, under the target's identities), with the identity its queries
    /// resolve the target's records by, as the session's own stitch of the target does.
    fn with_target_env<R>(
        &mut self,
        k: &mut Kernel,
        edit: impl FnOnce(&mut Kernel, &mut Brep, &mut NotifyEnv<'_>, BrepIdentity<'_>) -> Result<R>,
    ) -> Result<R> {
        with_target_env_of(&mut self.objects, k, edit)
    }

    /// WR-706: whether the lamina edge `e` of `sheet` is a side of a section's flat face
    /// (`section_ends`) that runs from the end's vertex along an edge of the target at that
    /// vertex (its samples on the section's plane and its middle on the edge, within ten
    /// times `tol`): its two end points if so.
    fn seam_side(
        &self,
        k: &Kernel,
        sheet: &Brep,
        e: EdgeId,
        section_ends: &[SectionEnd],
        tol: f64,
    ) -> Result<Option<(Point3, Point3)>> {
        let on_tol = 10.0 * tol;
        let (sv, ev) = (
            edge::start_vertex(sheet, e)?
                .ok_or_else(|| input_missing(OverflowInput::EdgeStartVertex))?,
            edge::end_vertex(sheet, e)?
                .ok_or_else(|| input_missing(OverflowInput::EdgeEndVertex))?,
        );
        let (a, b) = (vertex::point(sheet, sv)?, vertex::point(sheet, ev)?);
        let curve = edge_curve(k, sheet, e)?;
        let ivl = curve.natural_interval();
        let mid = curve.evaluate_point(k, ivl.evaluate(0.5))?;
        let target = &*self.objects.target;
        for end in section_ends {
            if a.distance(end.at) > on_tol && b.distance(end.at) > on_tol {
                continue;
            }
            let on_plane = |p: Point3| (p - end.at).dot(end.normal).abs() <= on_tol;
            if !(on_plane(a) && on_plane(b) && on_plane(mid)) {
                continue;
            }
            for te in crate::topology::brep::p1::edges(target, None)? {
                if edge::start_vertex(target, te)? != Some(end.vertex)
                    && edge::end_vertex(target, te)? != Some(end.vertex)
                {
                    continue;
                }
                let t_curve = edge::curve(target, te)?
                    .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
                if let Some((_, dist)) = drop_on(k, t_curve, &edge::interval(target, te)?, mid)? {
                    if dist <= on_tol {
                        return Ok(Some((a, b)));
                    }
                }
            }
        }
        Ok(None)
    }

    /// The fork's `SmFilletExecutive::TrimAtCreases` (WR-695): in a cliff overflow, trim
    /// the blends along their creases with the faces beyond the cliffs and sew them into
    /// the target: the blends built on their faces' continuations, the sharp edges moved
    /// to where they meet the body.
    ///
    /// Each variable round on planes holds its law and each open end is already bounded
    /// by its end plane before sewing ([`SessionObjects::bound_open_fillet_ends`]).
    /// Each sheet face is sectioned by every
    /// face of the target near it whose surface holds none of its boundary ([`creases`]); a
    /// crease is kept where it lies inside the face it is on and the sheet face
    /// ([`crate::blend::fillet_overflow::pieces_in_face`]), and the sheet face is split along all of them at once; a
    /// piece whose inner point lies inside the target is kept, the others go. A support
    /// face whose kept rail runs past its boundary into the material is carried on along
    /// its plane ([`extend`]). The kept pieces' lamina edges are imprinted on the target
    /// faces they lie on; the target faces reached from the blended edges without crossing
    /// an imprinted edge (or a piece of an edge an extension runs along) are deleted; the
    /// sheet is merged in, the extensions made, all stitched, the sewn edges held to the
    /// sheet's accuracy, the body oriented, and the edges between sewn faces on one surface
    /// removed.
    ///
    /// Refused (`NoRoom`): a law not held; a crease on a face that runs on tangentially
    /// from a face the sheet rolls on; a blend that keeps no face of its edge; a rail into
    /// the material that [`extend`] does not build. `BadMerge`: a piece on the body's
    /// boundary, nothing or everything taken off, a sheet that does not sew into a
    /// manifold solid. Each refusal is noted in the error info.
    ///
    /// # Errors
    /// The refusals as `SM_ERR`: a blend too large as [`BlendFailure::NoRoom`] with the
    /// reason noted, any other as a [`BlendFailure::CreaseTrimNotBuilt`] naming why
    /// ([`CreaseTrimStop`]); a face or edge read without its geometry,
    /// [`BlendFailure::OverflowInputMissing`]; those of the steps.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub(super) fn trim_at_creases(&mut self, k: &mut Kernel) -> Result<()> {
        crate::profile::profile_scope!(k, "blend::session::trim_at_creases");
        if self.objects.intermediate.is_none() {
            return Err(null_defect(INTERMEDIATE));
        }
        let solvers = self.executive.solvers.clone();
        // What the trim reads of the solvers, before the sheet leaves the session.
        let mut faces_of: BTreeMap<FilletObject, Vec<(FilletGeomId, Option<FaceId>)>> =
            BTreeMap::new();
        let mut approx = Vec::new();
        let mut tangency = Vec::new();
        let mut face_tangency = 0.0;
        let mut blended_edges: Vec<EdgeId> = Vec::new();
        for &solver in &solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let active_solver = self.objects.solver(id)?;
            let base = active_solver.base();
            approx.push(base.approx_tol_3d);
            let budget = if let Some(offsets) = active_solver.constant_offsets()? {
                base.constant_offset_rollover_tolerance(k, self.objects.target, offsets)
            } else {
                base.tangency_tolerance()
            };
            tangency.push(budget);
            face_tangency = smos_max(face_tangency, base.tangency_tolerance());
            let eu = base.edgeuse(0)?.ok_or_else(|| null_defect(SOLVER_EDGE))?;
            blended_edges
                .push(eu1::edge(self.objects.target, eu)?.ok_or_else(|| null_defect(SOLVER_EDGE))?);
            faces_of.insert(solver, self.objects.fillet_faces(solver)?);
        }

        let sheet = self
            .objects
            .intermediate
            .take()
            .ok_or_else(|| null_defect(INTERMEDIATE))?;

        // These ends were already built and capped before sewing. Losing their solver
        // topology or section here is a stage defect, not a geometric refusal.
        let mut section_ends = Vec::new();
        for end in self.executive.open_ends.iter().filter(|end| end.section) {
            let FilletObject::Solver(id) = end.solver else {
                return Err(invalid("open end solver is not a solver"));
            };
            let FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v)) = end.vertex
            else {
                return Err(invalid("open end is not a target vertex"));
            };
            let eu = self
                .objects
                .solver(id)?
                .base()
                .edgeuse(0)?
                .ok_or_else(|| defect(SECTION_END_SOLVER))?;
            let edge =
                eu1::edge(self.objects.target, eu)?.ok_or_else(|| defect(SECTION_END_SOLVER))?;
            section_ends.push(
                section_end(k, self.objects.target, edge, v)?.ok_or_else(|| {
                    defect(
                        "a section end has no section through its vertex: the bound of the open \
                     ends found that section on the same edge and vertex of the unchanged \
                     target before sewing",
                    )
                })?,
            );
        }
        let mut note = None;
        let mut site = None;

        // the target is trimmed in place; a failed trim puts it back as it was, from a
        // snapshot that keeps its geometry's owners (#2715: a clone's surfaces have none)
        let kept = self.objects.target.snapshot()?;
        let trimmed = self.trim_sheet_into(
            k,
            sheet,
            &solvers,
            &faces_of,
            &blended_edges,
            &approx,
            &tangency,
            face_tangency,
            &section_ends,
            &mut note,
            &mut site,
        );
        if let Some(refused) = note.take() {
            self.note_refusal(refused);
            if let Some((edge, at)) = site {
                self.executive.error.edge = Some(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edge(edge),
                ));
                self.executive.error.location = Some(at);
            }
        }
        if trimmed.is_err() {
            self.objects.target.restore(kept);
        }
        trimmed
    }

    /// The two creases that touch at the seam of a closed strip end on different
    /// parameter sides. Cut it at an interior section first so the following
    /// face splits retain both arcs of the closed contact rail at the seam.
    fn divide_closed_cliff_strips(
        &mut self,
        k: &mut Kernel,
        sheet: &mut Brep,
        solvers: &[FilletObject],
        round_of: &mut BTreeMap<FaceId, usize>,
        tol: f64,
    ) -> Result<()> {
        if self.executive.continued_cliffs.is_empty() {
            return Ok(());
        }
        for (face, index) in round_of.clone() {
            let Some(solver) = solvers.get(index) else {
                continue;
            };
            if !self.executive.continued_cliffs.contains_key(solver) {
                continue;
            }
            let domain = face::p2::uv_domain(sheet, face)?;
            let surface = face::p2::surface(sheet, face)?.ok_or_else(|| {
                SolidError::blend(
                    OP,
                    BlendFailure::CliffInputMissing {
                        face,
                        input: CliffInput::Surface,
                    },
                )
            })?;
            if !surface.is_periodic(k, &domain, SurfParamType::U)? {
                continue;
            }
            let section = surface
                .create_iso_parametric_curve(
                    k,
                    SurfParamType::U,
                    domain.evaluate(0.5, 0.5).x,
                    tol,
                    Some(&domain),
                    false,
                )?
                .curve;
            let (extra, _) = with_kernel_callbacks(k, sheet, |k, b, env| {
                merge_curves(k, b, env, face, tol, vec![section], None)
            })?;
            if extra.is_empty() {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::ClosedCliffWouldNotSplit { face },
                ));
            }
            self.copy_divided_blend_face_mark(sheet, face, &extra)?;
            round_of.extend(extra.into_iter().map(|part| (part, index)));
        }
        Ok(())
    }

    /// The fork's `TrimAtCreases` from its tolerances on: `sheet` (the open ends cut)
    /// trimmed and sewn into the target. A refusal is written to `note`.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one fork routine
    fn trim_sheet_into(
        &mut self,
        k: &mut Kernel,
        mut sheet: Brep,
        solvers: &[FilletObject],
        faces_of: &BTreeMap<FilletObject, Vec<(FilletGeomId, Option<FaceId>)>>,
        blended_edges: &[EdgeId],
        approx: &[f64],
        tangency: &[f64],
        face_tangency: f64,
        section_ends: &[SectionEnd],
        note: &mut Option<Refusal>,
        site: &mut Option<(EdgeId, Point3)>,
    ) -> Result<()> {
        // a refusal for a blend too large, noted with its reason and failing with it
        let refuse_no_room =
            |note: &mut Option<Refusal>, reason: NoRoomReason, comment| -> Result<()> {
                *note = Some(refusal(FilletErrorType::NoRoom, Some(reason), comment));
                Err(no_room(reason))
            };
        // a refusal with no reason a size decides, noted under `code` and failing with `stop`
        let refuse = |note: &mut Option<Refusal>,
                      code: FilletErrorType,
                      stop: CreaseTrimStop,
                      comment|
         -> Result<()> {
            *note = Some(refusal(code, None, comment));
            Err(SolidError::blend(
                OP,
                BlendFailure::CreaseTrimNotBuilt(stop),
            ))
        };
        let mut tol = sheet.tolerance();
        for &a in approx {
            tol = smos_max(tol, a);
        }
        let on_tol = 10.0 * tol;

        // Not the fork's (#2404): the strip a blend leaves on an edge of the body, thinner than
        // the part resolves, has no topology to build
        if self.strip_below_resolution(k, &sheet, blended_edges, tol)? {
            self.note_fit_of_the_strip(solvers, blended_edges)?;
            return refuse_no_room(note, NoRoomReason::StripBelowResolution, "Fillet Error: a blend past its faces would leave a strip of the body along an edge thinner than the part resolves: a smaller blend keeps it");
        }
        // the blended edges' curves, to find their pieces after the imprint
        let mut blended_curves = Vec::new();
        for &e in blended_edges {
            blended_curves.push(edge_curve(k, self.objects.target, e)?);
        }

        let target_faces = crate::topology::brep::p1::faces(self.objects.target, None)?;
        // the faces the blends roll on: those of the blended edges (not the end faces they
        // end on)
        let mut rolled_on: BTreeSet<FaceId> = BTreeSet::new();
        for &e in blended_edges {
            rolled_on.extend(edge::faces(self.objects.target, e, None)?);
        }
        let mut target_boxes = Vec::new();
        for &f in &target_faces {
            target_boxes.push(crate::query::face::p1::calculate_bounding_box(
                k,
                &*self.objects.target,
                f,
                false,
            )?);
        }

        // each blended edge's own fillet faces (not its corners')
        let mut round_of: BTreeMap<FaceId, usize> = BTreeMap::new();
        for (i, solver) in solvers.iter().enumerate() {
            for &(_, f) in faces_of.get(solver).map_or(&[][..], Vec::as_slice) {
                if let Some(f) = f {
                    round_of.insert(f, i);
                }
            }
        }
        self.divide_closed_cliff_strips(k, &mut sheet, solvers, &mut round_of, tol)?;
        let mut kept_rounds: BTreeSet<usize> = BTreeSet::new();
        // #18646: each kept piece of a round's face, with its round
        let mut piece_round: BTreeMap<FaceId, usize> = BTreeMap::new();

        // each round's side of the body (the fork's ce748fa): a
        // convex blend's sheet lies in the material it takes off, a concave blend's (an open
        // setback corner's concave rounds) in the void it fills. A sheet face of no round (a
        // corner's) takes the blends' one side.
        let mut adds = vec![false; solvers.len()];
        let (mut any_adds, mut any_takes) = (false, false);
        for (a, &e) in adds.iter_mut().zip(blended_edges) {
            *a = is_concave_edge(k, self.objects.target, e)?;
            if *a {
                any_adds = true;
            } else {
                any_takes = true;
            }
        }
        let (open_springs, open_free) = self.open_setback_springs()?;

        // 1, 2: each sheet face split along its creases, the pieces outside the body
        // (beyond the cliffs, or past an end) deleted, whether a crease split it or not
        let body_tol = self.objects.target.tolerance();
        // WR-717: the creases that run along an edge of the face they cross
        let mut edge_creases: Vec<Curve> = Vec::new();
        // #17633: each such crease with the face it crosses
        let mut face_creases: Vec<(FaceId, Curve)> = Vec::new();
        let mut continued_walls: BTreeMap<FaceId, BTreeSet<EdgeId>> = BTreeMap::new();
        // WR-717: the body's edges those creases stand for
        let mut crease_edges: Vec<EdgeId> = Vec::new();
        for s in crate::topology::brep::p1::faces(&sheet, None)? {
            // Each round uses its own section budget; a shared corner must meet all.
            let tangent_budget = if let Some(&i) = round_of.get(&s) {
                tangency.get(i).copied()
            } else {
                tangency.iter().copied().reduce(f64::min)
            }
            .ok_or_else(|| {
                SolidError::blend_crossing_input_missing(OP, CrossingInput::SheetFaceTangencyBudget)
            })?;
            let mut sheet_box =
                crate::query::face::p1::calculate_bounding_box(k, &sheet, s, false)?;
            sheet_box.expand_absolute(on_tol);
            let sheet_edges = face::p4::edges(&sheet, s, None, false, None)?;
            let mut cliffs = BTreeMap::new();
            if let Some(crossings) = round_of
                .get(&s)
                .and_then(|&i| solvers.get(i))
                .and_then(|solver| self.executive.continued_cliffs.get(solver))
            {
                for &(rim, wall) in crossings {
                    continued_walls.entry(wall).or_default().insert(rim);
                    if let std::collections::btree_map::Entry::Vacant(entry) = cliffs.entry(wall) {
                        entry.insert(outward_plane(k, self.objects.target, wall)?);
                    }
                }
            }
            let mut pieces: Vec<Curve> = Vec::new();
            let mut only: Option<Surface> = None;
            let mut crossed = 0_usize;
            // the faces the sheet face rolls on or ends on: each holds one of its boundary
            // edges
            let mut supports: BTreeSet<FaceId> = BTreeSet::new();
            // WR-717: the supports that hold one of the edges to the trim's own tolerance
            let mut held: BTreeSet<FaceId> = BTreeSet::new();
            for (&f, bx) in target_faces.iter().zip(&target_boxes) {
                if bx.are_disjoint(&sheet_box, on_tol) {
                    continue;
                }
                let mut holds = false;
                for &e in &sheet_edges {
                    if holds {
                        break;
                    }
                    if !edge::is_lamina(&sheet, e)? {
                        continue;
                    }
                    holds = edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)?;
                    if holds && edge_on_surface(k, &sheet, e, self.objects.target, f, tol)? {
                        held.insert(f);
                    }
                }
                if holds {
                    supports.insert(f);
                }
            }
            for (&f, bx) in target_faces.iter().zip(&target_boxes) {
                // WR-717: a face that holds a rail only to the looser `on_tol` (a rail that
                // runs past it by less than that, beside the edge where the sheet crosses
                // it) is still looked at for creases
                let continues = cliffs.contains_key(&f);
                if !continues && (bx.are_disjoint(&sheet_box, on_tol) || held.contains(&f)) {
                    continue;
                }
                // fitted to a thousandth of the body's accuracy (#17673, `SECTION_FIT`), so
                // that creases meeting at a corner of the sheet (on two faces past a cliff)
                // meet there well within it; exact sections stay exact
                // Where a roll-over or continued cliff meets a torus parallel to its axis,
                // use the certified creases over the face's profile (#10876).
                let certified = if self.executive.roll_over_pass || continues {
                    match (
                        face::p2::surface(&sheet, s)?,
                        face::p2::surface(self.objects.target, f)?,
                    ) {
                        (Some(torus), Some(cliff)) => {
                            crate::blend::blend_corner::rollover::geometry::torus_creases(
                                k, torus, cliff, tol, body_tol,
                            )?
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                let found = match certified {
                    Some(found) => found,
                    None => creases(
                        k,
                        &mut sheet,
                        s,
                        self.objects.target,
                        f,
                        body_tol * SECTION_FIT,
                    )?,
                };
                // each crease where it lies inside both faces
                let mut in_face = Vec::new();
                let mut along = Vec::new();
                let mut seam_events = Vec::new();
                if continues {
                    in_face = found;
                } else {
                    for c in &found {
                        seam_events.extend(pieces_in_face_to_ends(
                            k,
                            c,
                            self.objects.target,
                            f,
                            tol,
                            &mut in_face,
                            &mut along,
                        )?);
                    }
                }
                // Not the fork's (WR-717, #2385): a crease that runs along an edge of the
                // face, within tolerance, is that edge. The sheet is split along it as along
                // any crease, and the edge stands as its imprint (below). #17673: a run a
                // piece was run on over is split along once, in that piece; it still stands
                // for its edge.
                for (c, taken) in along {
                    edge_creases.push(c.copy(k)?);
                    face_creases.push((f, c.copy(k)?));
                    if !taken {
                        in_face.push(c);
                    }
                }
                let before = pieces.len();
                for c in &in_face {
                    // #17673: each piece that ends on the sheet face's boundary ends where
                    // that edge crosses the face
                    pieces_ending_at_meets(
                        k,
                        c,
                        &mut sheet,
                        s,
                        self.objects.target,
                        f,
                        tol,
                        &mut pieces,
                    )?;
                }
                if pieces.len() > before {
                    // A face past a cliff meets the face the rail leaves (one the blend
                    // rolls on) at a sharp edge. One that runs on from it tangentially (a
                    // round already on the face) is no cliff: the rail would roll over
                    // onto it, and the ball does not fit between.
                    let rolled_supports: BTreeSet<FaceId> =
                        supports.intersection(&rolled_on).copied().collect();
                    if let Some((g, r)) = crease_across_tangent_edge(
                        k,
                        self.objects.target,
                        f,
                        &seam_events,
                        &rolled_supports,
                        tangent_budget,
                        face_tangency,
                    )? {
                        // the fork's NoteFitOnFace(g) (49f393d, WR-696)
                        let face = FilletObject::Topology(FilletBody::Target, EntityRef::Face(g));
                        self.executive
                            .note_fit_on_face(k, &mut self.objects, face)?;
                        return refuse_no_room(
                            note,
                            NoRoomReason::CreaseAcrossTangentEdge,
                            r.comment,
                        );
                    }
                    crossed += 1;
                    only = Some(
                        face::p2::surface(self.objects.target, f)?
                            .ok_or_else(|| input_missing(OverflowInput::FaceSurface))?
                            .clone_for_evaluation()?,
                    );
                }
            }
            let mut new_faces = Vec::new();
            if !pieces.is_empty() {
                let intersection = if crossed == 1 { only } else { None };
                new_faces = with_kernel_callbacks(k, &mut sheet, |k, b, env| {
                    merge_curves(k, b, env, s, tol, pieces, intersection)
                })?
                .0;
            }
            self.copy_divided_blend_face_mark(&sheet, s, &new_faces)?;
            // keep the pieces inside the target
            let mut all = vec![s];
            all.extend(new_faces);
            for p in all {
                // a piece keeps the domain of the face it was split from: its own, from its
                // trims
                let domain = face::p1::calculate_uv_domain_from_uv_trim_curves(
                    k, &mut sheet, p, true, false, false,
                )?
                .domain;
                face::p2::set_uv_domain(&mut sheet, p, &domain)?;
                let inner = face::p2::calculate_an_internal_point(k, &mut sheet, p)?;
                let (inside, on_boundary) = in_material(k, self.objects.target, inner)?;
                if on_boundary {
                    if let Some(&i) = round_of.get(&s) {
                        if let (Some(&edge_id), Some(curve)) =
                            (blended_edges.get(i), blended_curves.get(i))
                        {
                            if let Ok(mid) =
                                curve.evaluate_point(k, curve.natural_interval().evaluate(0.5))
                            {
                                *site = Some((edge_id, mid));
                            }
                        }
                    }
                    return refuse(
                        note,
                        FilletErrorType::BadMerge,
                        CreaseTrimStop::PieceOnBoundaryAwayFromCreases,
                        "Fillet Error: a blend past its faces lies on the body's boundary away from its creases",
                    );
                }
                let mut piece_adds = any_adds;
                if let Some(&i) = round_of.get(&s) {
                    piece_adds = adds.get(i).copied().unwrap_or(false);
                } else if any_adds && any_takes {
                    return refuse(
                        note,
                        FilletErrorType::CornerUnsupported,
                        CreaseTrimStop::CornerBetweenAddingAndTaking,
                        "Fillet Error: a corner's face between blends that add and blends that take off material, past their faces or at an open setback, is not built",
                    );
                }
                let beyond_cliff = cliffs
                    .values()
                    .any(|&(at, outward)| (inner - at).dot(outward) > tol);
                if beyond_cliff || if piece_adds { inside } else { !inside } {
                    with_kernel_callbacks(k, &mut sheet, |k, b, env| delete(k, b, env, p))?;
                } else if let Some(&i) = round_of.get(&s) {
                    kept_rounds.insert(i);
                    piece_round.insert(p, i);
                }
            }
        }

        // A blended edge whose blend keeps no face in the body has no length left: its
        // corners take the whole of it (a 20 cube's corner rounded R20: the corner's ball
        // reaches the far faces, the rounds lie past them). Larger than the faces allow,
        // so a fit can reduce it.
        for i in 0..solvers.len() {
            let has = round_of.values().any(|&r| r == i);
            if has && !kept_rounds.contains(&i) {
                // the largest radius that keeps a length of the edge: its corners take
                // r <u, e> at each end (sm_CornerTakesPerRadius), the edge's length L in all
                // at r = L / (s0 + s1); open, so less the fit margin (the fork's a3d6d81,
                // WR-696)
                let reason = match solvers.get(i) {
                    Some(&solver) => {
                        self.note_fit_of_corners_taking_the_edge(k, solver, solvers)?
                    }
                    None => NoRoomReason::CornersTakeWholeEdge,
                };
                let comment = if reason == NoRoomReason::OverflowKeepsNothing {
                    "Fillet Error: a blend past its faces keeps no length of its edge: no corner takes any of it, the trim takes the whole of it (the blend is larger than the faces allow)"
                } else {
                    "Fillet Error: a blend past its faces keeps no length of its edge: its corners take the whole of it (the blend is larger than the faces allow)"
                };
                return refuse_no_room(note, reason, comment);
            }
        }

        // the support faces continued to the kept rails that run past them into the
        // material
        let sheet_edges = crate::topology::brep::p1::edges(&sheet, None)?;
        let mut lamina_edges = Vec::new();
        for &e in &sheet_edges {
            if edge::is_lamina(&sheet, e)? {
                lamina_edges.push(e);
            }
        }
        // WR-706: a side of a section's flat face that runs from the end's vertex along an
        // edge of the body (the seam where the corner's cylinder runs on tangentially into
        // the flat side) is sewn to that edge's piece, not imprinted on its faces
        let mut seam_sides: Vec<(Point3, Point3)> = Vec::new();
        if !section_ends.is_empty() {
            let mut kept_lamina = Vec::new();
            for &e in &lamina_edges {
                let side = self.seam_side(k, &sheet, e, section_ends, tol)?;
                match side {
                    Some(ends) => seam_sides.push(ends),
                    None => kept_lamina.push(e),
                }
            }
            lamina_edges = kept_lamina;
        }
        // #18646: a rail on a face of its own blended edge extends only that edge's faces.
        // Matching another face's surface does not make that face a support of this round,
        // whether the faces share a surface or merely meet along the rail. Creases and
        // section ends with no own-face match keep all the faces they lie on.
        let mut on_faces: Vec<Vec<FaceId>> = Vec::new();
        for &e in &lamina_edges {
            let mut on = Vec::new();
            for &f in &target_faces {
                if edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)? {
                    on.push(f);
                }
            }
            let mut own: Vec<FaceId> = Vec::new();
            for s in edge::faces(&sheet, e, None)? {
                let round = piece_round.get(&s).or_else(|| round_of.get(&s));
                if let Some(&be) = round.and_then(|&i| blended_edges.get(i)) {
                    own.extend(edge::faces(self.objects.target, be, None)?);
                }
            }
            if on.iter().any(|f| own.contains(f)) {
                on.retain(|f| own.contains(f));
            }
            on_faces.push(on);
        }
        let mut extensions: Vec<Extension> = Vec::new();
        let mut along: Vec<Curve> = Vec::new();
        for &f in &target_faces {
            let mut rails = Vec::new();
            let mut cliff_rails = Vec::new();
            let rim = continued_walls.get(&f);
            for (&e, on) in lamina_edges.iter().zip(&on_faces) {
                if on.contains(&f) {
                    let start = edge::start_vertex(&sheet, e)?
                        .ok_or_else(|| input_missing(OverflowInput::EdgeStartVertex))?;
                    let end = edge::end_vertex(&sheet, e)?
                        .ok_or_else(|| input_missing(OverflowInput::EdgeEndVertex))?;
                    let rail = Rail {
                        curve: edge_curve(k, &sheet, e)?,
                        start,
                        end,
                        start_point: vertex::point(&sheet, start)?,
                        end_point: vertex::point(&sheet, end)?,
                    };
                    // A shared target face can carry an ordinary rail from a
                    // different span. Only this span's crease extends into void.
                    let fills_void = rim.is_some()
                        && edge::faces(&sheet, e, None)?
                            .iter()
                            .filter_map(|face| piece_round.get(face))
                            .filter_map(|&i| solvers.get(i))
                            .filter_map(|solver| self.executive.continued_cliffs.get(solver))
                            .flatten()
                            .any(|&(_, wall)| wall == f);
                    if fills_void {
                        cliff_rails.push(rail);
                    } else {
                        rails.push(rail);
                    }
                }
            }
            for (rails, rim) in [(&rails, None), (&cliff_rails, rim)] {
                if rails.is_empty() {
                    continue;
                }
                match extend(k, self.objects.target, f, rails, blended_edges, tol, rim)? {
                    ExtendOutcome::Refused(why) => {
                        // the fork's NoteFitOnFace(f) (49f393d, WR-696)
                        let face = FilletObject::Topology(FilletBody::Target, EntityRef::Face(f));
                        self.executive
                            .note_fit_on_face(k, &mut self.objects, face)?;
                        return refuse_no_room(
                            note,
                            NoRoomReason::RailRunsPastFaceIntoMaterial,
                            why,
                        );
                    }
                    ExtendOutcome::Extended(made) => {
                        for ext in &made {
                            for &e in &ext.along {
                                along.push(edge_curve(k, self.objects.target, e)?);
                            }
                        }
                        extensions.extend(made);
                    }
                }
            }
        }

        // #17633 (WR-717): each body edge a crease runs along moves onto the crease
        // first, so that the sheet's boundary is imprinted on its faces and sewn where it
        // lies, not drawn the crease's gap to the edge (a section's end on an end face
        // drawn to the old vertex skews the end face's outline by that gap)
        // (each edge once: a second crease along it, on its other face, is left to the
        // imprint)
        let mut moves = MovedEdges::default();
        for (f, c) in &face_creases {
            for e in face::p4::edges(self.objects.target, *f, None, false, None)? {
                if moves.moved.contains(&e) {
                    continue;
                }
                // a candidate: the edge's ends and midpoint within the trim's tolerance of
                // the crease (the move itself is decided on a bound over the whole edge)
                let curve = edge_curve(k, self.objects.target, e)?;
                let ivl = edge::interval(self.objects.target, e)?;
                let mut along = true;
                for at in [0.0, 0.5, 1.0] {
                    let p = curve.evaluate_point(k, ivl.evaluate(at))?;
                    along = matches!(
                        drop_on(k, c, &c.natural_interval(), p)?,
                        Some((_, dist)) if dist <= tol
                    );
                    if !along {
                        break;
                    }
                }
                if !along {
                    continue;
                }
                self.with_target_env(k, |k, b, env, _| {
                    move_edge_onto_crease(k, b, env, *f, e, c, tol, on_tol, &mut moves)
                })?;
            }
        }

        // 4: the kept sheet's boundary imprinted on the target faces it lies on
        let mut imprinted: BTreeSet<EdgeId> = BTreeSet::new();
        // each face's imprint curves, with the faces they split it into
        let mut imprints: Vec<(Vec<FaceId>, Vec<Curve>)> = Vec::new();
        for &f in &target_faces {
            let mut on = Vec::new();
            for &e in &lamina_edges {
                if edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)? {
                    on.push(edge_curve(k, &sheet, e)?);
                }
            }
            // an extension's runs from a chain's end to its foot lie on the walls beside it;
            // with the sheet's section there they fence what the blend takes off that wall
            // (else the section ends inside the wall and the material taken off reaches
            // every face)
            for ext in &extensions {
                for run in &ext.foot_runs {
                    if crate::blend::fillet_overflow::curve_on_surface(
                        k,
                        run,
                        self.objects.target,
                        f,
                        on_tol,
                    )? {
                        on.push(run.copy(k)?);
                    }
                }
            }
            // an open G1 setback corner's spring curves on this face: with the rails they
            // bound the corner's hole on it (the fork's ef57570)
            for (sf, curve) in &open_springs {
                if *sf == f {
                    on.push(curve.clone());
                }
            }
            if on.is_empty() {
                continue;
            }
            let curves = on.clone();
            let (new_faces, on_edges) =
                self.with_target_env(k, |k, b, env, _| merge_curves(k, b, env, f, tol, on, None))?;
            imprinted.extend(on_edges);
            imprints.push(([vec![f], new_faces].concat(), curves));
        }
        // not the fork's (#12342): an edge a curve classified to can be split by a later
        // imprint, on its face or on the face beyond, and only one of its pieces keeps its
        // edge; which one follows the edge's direction. Every piece of a face's edges along
        // one of its imprint curves is that imprint, so the trim does not depend on which
        // way the body's edges run.
        for (faces, curves) in &imprints {
            for &g in faces {
                for e in face::p4::edges(self.objects.target, g, None, false, None)? {
                    if imprinted.contains(&e) {
                        continue;
                    }
                    let target = &*self.objects.target;
                    let curve = edge::curve(target, e)?
                        .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
                    let mid = curve.evaluate_point(k, edge::interval(target, e)?.evaluate(0.5))?;
                    for c in curves {
                        if matches!(
                            drop_on(k, c, &c.natural_interval(), mid)?,
                            Some((_, dist)) if dist <= tol
                        ) {
                            imprinted.insert(e);
                            break;
                        }
                    }
                }
            }
        }
        // WR-706: the pieces of the body's edges a section's side lies along, cut off at
        // the rails' ends by the imprint
        for &(a, b) in &seam_sides {
            let target = &*self.objects.target;
            let mut found = false;
            for e in crate::topology::brep::p1::edges(target, None)? {
                let (sv, ev) = (
                    edge::start_vertex(target, e)?
                        .ok_or_else(|| input_missing(OverflowInput::EdgeStartVertex))?,
                    edge::end_vertex(target, e)?
                        .ok_or_else(|| input_missing(OverflowInput::EdgeEndVertex))?,
                );
                let (s_pt, e_pt) = (vertex::point(target, sv)?, vertex::point(target, ev)?);
                if (s_pt.distance(a) <= on_tol && e_pt.distance(b) <= on_tol)
                    || (s_pt.distance(b) <= on_tol && e_pt.distance(a) <= on_tol)
                {
                    imprinted.insert(e);
                    found = true;
                }
            }
            if !found {
                return refuse(
                    note,
                    FilletErrorType::BadMerge,
                    CreaseTrimStop::SectionEndAlongUncutEdge,
                    "Fillet Error: a round's section end lies along an edge of the body that its rail does not cut",
                );
            }
        }

        // the faces round the blended edges, up to the imprint and the extensions' edges
        // along the body: the material the blends take off
        let target = &*self.objects.target;
        let target_edges = crate::topology::brep::p1::edges(target, None)?;
        for &e in &target_edges {
            let (sv, ev) = (
                edge::start_vertex(target, e)?
                    .ok_or_else(|| input_missing(OverflowInput::EdgeStartVertex))?,
                edge::end_vertex(target, e)?
                    .ok_or_else(|| input_missing(OverflowInput::EdgeEndVertex))?,
            );
            let (s_pt, e_pt) = (vertex::point(target, sv)?, vertex::point(target, ev)?);
            // a piece of an edge an extension runs along, cut at its crossing by the imprint
            for ext in &extensions {
                for &(a, b) in &ext.along_pieces {
                    if (s_pt.distance(a) <= 10.0 * tol && e_pt.distance(b) <= 10.0 * tol)
                        || (s_pt.distance(b) <= 10.0 * tol && e_pt.distance(a) <= 10.0 * tol)
                    {
                        imprinted.insert(e);
                    }
                }
            }
            if along.is_empty() && edge_creases.is_empty() {
                continue;
            }
            let curve =
                edge::curve(target, e)?.ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
            let mid = curve.evaluate_point(k, edge::interval(target, e)?.evaluate(0.5))?;
            for c in &along {
                if let Some((_, dist)) = drop_on(k, c, &c.natural_interval(), mid)? {
                    if dist <= tol {
                        imprinted.insert(e);
                        break;
                    }
                }
            }
            // WR-717: an edge a crease runs along is that crease's imprint
            for c in &edge_creases {
                if let Some((_, dist)) = drop_on(k, c, &c.natural_interval(), mid)? {
                    if dist <= tol {
                        imprinted.insert(e);
                        crease_edges.push(e);
                        break;
                    }
                }
            }
        }
        // WR-717: a sheet edge that lies along an edge a crease stands for (the sheet not
        // split there, the crease within tolerance of its own rail too) is sewn to it across
        // the gap between them: at twice that gap, as the stitch glues within half its
        // tolerance, and never past `on_tol`, the distance `edge_on_surface` already took
        // the sheet edge to be on the face at
        let mut sew_tol = tol;
        for &e in &lamina_edges {
            let curve =
                edge::curve(&sheet, e)?.ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
            let ivl = edge::interval(&sheet, e)?;
            let mut worst = 0.0_f64;
            let mut along_one = !crease_edges.is_empty();
            let mut i = 0_u32;
            while i <= 4 && along_one {
                let p = curve.evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0))?;
                i += 1;
                let mut best = f64::INFINITY;
                for &ce in &crease_edges {
                    let c = edge::curve(target, ce)?
                        .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
                    if let Some((_, dist)) = drop_on(k, c, &edge::interval(target, ce)?, p)? {
                        best = smos_min(best, dist);
                    }
                }
                along_one = best <= on_tol;
                worst = smos_max(worst, best);
            }
            if along_one {
                sew_tol = smos_max(sew_tol, smos_min(on_tol, 2.0 * worst));
            }
        }
        let mut gone: BTreeSet<FaceId> = BTreeSet::new();
        let mut gone_order: Vec<FaceId> = Vec::new();
        let mut front: Vec<FaceId> = Vec::new();
        for &e in &target_edges {
            let curve =
                edge::curve(target, e)?.ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
            let mid = curve.evaluate_point(k, edge::interval(target, e)?.evaluate(0.5))?;
            let mut on_blended = false;
            for c in &blended_curves {
                if let Some((_, dist)) = drop_on(k, c, &c.natural_interval(), mid)? {
                    if dist <= tol {
                        on_blended = true;
                        break;
                    }
                }
            }
            if !on_blended || imprinted.contains(&e) {
                continue;
            }
            for g in edge::faces(target, e, None)? {
                if gone.insert(g) {
                    gone_order.push(g);
                    front.push(g);
                }
            }
        }
        while let Some(f) = front.pop() {
            for e in face::p4::edges(target, f, None, false, None)? {
                if imprinted.contains(&e) {
                    continue;
                }
                for g in edge::faces(target, e, None)? {
                    if gone.insert(g) {
                        gone_order.push(g);
                        front.push(g);
                    }
                }
            }
        }
        if gone.is_empty() {
            return refuse(
                note,
                FilletErrorType::BadMerge,
                CreaseTrimStop::TakesOffNothing,
                "Fillet Error: a blend past its faces takes off nothing round its blended edges",
            );
        }
        if gone.len() >= crate::topology::brep::p1::num_faces(target) {
            return refuse(
                note,
                FilletErrorType::BadMerge,
                CreaseTrimStop::ReachesEveryFace,
                "Fillet Error: a blend past its faces reaches every face of the body (its creases and rails enclose no part of it)",
            );
        }
        self.with_target_env(k, |k, b, env, _| {
            for f in gone_order {
                delete(k, b, env, f)?;
            }
            Ok(())
        })?;
        // #17633: what an edge's move onto its crease left off its old geometry is taken off
        // with the faces past the creases; anything left of it is refused, not kept
        // within a larger tolerance ([`move_leftovers_kept`])
        {
            let kept_off = move_leftovers_kept(k, self.objects.target, &moves, tol)?;
            if kept_off {
                return refuse(
                    note,
                    FilletErrorType::BadMerge,
                    CreaseTrimStop::MovedEdgeLeavesGeometry,
                    "Fillet Error: a body edge moved onto a blend's crease leaves geometry the trim does not take off",
                );
            }
        }

        // the fillet edges' attribute stays with the executive's own B-reps
        if let (Some(serial), Some(FilletObject::Attribute(handle))) =
            (sheet.notify_serial(), self.executive.edge_attribute)
        {
            for e in crate::topology::brep::p1::edges(&sheet, None)? {
                let key = p3::topology_ref(serial, EntityRef::Edge(e)).key;
                if self
                    .objects
                    .attributes
                    .find_attribute(key, ids::ALL_FILLET_EDGES)
                    == Some(handle)
                {
                    self.objects
                        .attributes
                        .remove_attribute(key, handle, false)?;
                }
            }
        }

        // The sheet's boundary is cut where an extension's loop meets it: a rib that passes
        // under a lip runs on the end face, under the lip (the extension's edge) and on the
        // end face again, three edges of the body for one of the sheet's.
        for ext in &extensions {
            for c in &ext.lp {
                let ivl = c.curve.natural_interval();
                let ends = [
                    c.curve.evaluate_point(k, ivl.min())?,
                    c.curve.evaluate_point(k, ivl.max())?,
                ];
                for p in ends {
                    let mut found = None;
                    for e in crate::topology::brep::p1::edges(&sheet, None)? {
                        if found.is_some() {
                            break;
                        }
                        let (sv, ev) = (
                            edge::start_vertex(&sheet, e)?
                                .ok_or_else(|| input_missing(OverflowInput::EdgeStartVertex))?,
                            edge::end_vertex(&sheet, e)?
                                .ok_or_else(|| input_missing(OverflowInput::EdgeEndVertex))?,
                        );
                        if !edge::is_lamina(&sheet, e)?
                            || vertex::point(&sheet, sv)?.distance(p) <= 10.0 * tol
                            || vertex::point(&sheet, ev)?.distance(p) <= 10.0 * tol
                        {
                            continue;
                        }
                        let curve = edge::curve(&sheet, e)?
                            .ok_or_else(|| input_missing(OverflowInput::EdgeCurve))?;
                        if let Some((t, dist)) = drop_on(k, curve, &edge::interval(&sheet, e)?, p)?
                        {
                            if dist <= 10.0 * tol {
                                found = Some((e, t));
                            }
                        }
                    }
                    if let Some((e, t)) = found {
                        with_kernel_callbacks(k, &mut sheet, |k, b, env| {
                            let key_of = env.key_of;
                            let curve_ref = |e: EdgeId, kind: i64| ObjectRef {
                                key: key_of(EntityRef::Edge(e)).key,
                                type_code: kind,
                            };
                            crate::topology::brep::p9::make_vertex_split_edge(
                                k, b, e, t, env, &curve_ref,
                            )
                        })?;
                    }
                }
            }
        }

        // the sheet sewn in, and the body oriented. The sheet is merged in as the fillet's
        // normal path merges it (SmBrep::MergeBrep, surfaces kept as they are): its edges
        // keep their own tolerances and the body its own.
        let roll_over_pass = self.executive.roll_over_pass;
        self.with_target_env(k, |k, b, env, identity| {
            let before: BTreeSet<FaceId> =
                crate::topology::brep::p1::faces(b, None)?.into_iter().collect();
            let source_key = |e: EntityRef| notify_key(&sheet, e);
            p3::merge_brep(k, env, b, &sheet, &source_key, None, None, false)?;
            let mut copies: Vec<FaceId> = crate::topology::brep::p1::faces(b, None)?
                .into_iter()
                .filter(|f| !before.contains(f))
                .collect();
            for ext in extensions {
                copies.push(make_extension_face(k, b, env, ext, tol)?);
            }
            let listed: Vec<Option<FaceId>> = copies.iter().copied().map(Some).collect();
            let stitched = {
                let mut edits = crate::heal::stitch::p1::BorrowedStitchEdits {
                    env: &mut *env,
                    curve_identity: crate::topology::brep::p9::CurveIdentity::Actual(
                        &crate::topology::brep::p9::issue_curve_identity,
                    ),
                };
                // A Roll-over second pass whose End cap spans two cliff faces across their
                // tangent seam without a seam-cap split (`rollover::seam_end`) must not have
                // its single cap edge split across the seam vertex during stitching; keep
                // upstream candidate-list semantics there while enabling per-entity candidate
                // splitting for ordinary crease trims.
                if roll_over_pass {
                    crate::topology::brep::p4::stitch_faces_with_context(
                        k,
                        b,
                        sew_tol,
                        Some(&listed),
                        false,
                        Some(identity),
                        Some(&mut edits),
                    )?
                } else {
                    crate::topology::brep::p4::stitch_faces_splitting_candidates_with_context(
                        k,
                        b,
                        sew_tol,
                        Some(&listed),
                        false,
                        Some(identity),
                        Some(&mut edits),
                    )?
                }
            };
            // the edges the sheet is sewn along hold it to its own accuracy
            let sewn_tol = smos_max(
                sew_tol,
                smos_max(
                    stitched.max_vertex_edge_gap,
                    stitched.max_edge_face_trim_curve_gap,
                ),
            );
            for &f in &copies {
                for e in face::p4::edges(b, f, None, false, None)? {
                    edge::set_tolerance(b, e, sewn_tol, true, true)?;
                }
            }
            // an open G1 setback corner's hole: its n sections and 2 n spring halves stay
            // free (the fork's ef57570)
            let mut maybe_open = false;
            if stitched.lamina_edges == open_free {
                let mut marks = Marks::default();
                let oriented =
                    p3::orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)?;
                maybe_open = oriented.maybe_not_closed_solid;
            }
            if stitched.lamina_edges != open_free
                || (open_free == 0
                    && (maybe_open || !crate::topology::brep::p2::is_manifold_solid(b)?))
            {
                if let Ok(found) =
                    failing_blended_edge(k, b, &copies, blended_edges, &blended_curves)
                {
                    *site = found;
                }
                return refuse(
                    note,
                    FilletErrorType::BadMerge,
                    CreaseTrimStop::NotSewn,
                    "Fillet Error: a blend past its faces does not sew into the body along its creases and rails",
                );
            }

            // Faces sewn on one surface are one face, as a boolean's post-process leaves
            // them (only the edges it made are looked at, SmMerge::BooleanPostProcess): a
            // corner's end face on the end wall's plane carried on over a rim is that wall,
            // past the edges the blend sewed in. Every other edge and vertex of the body
            // stays.
            let mut sewn_edges: BTreeSet<EdgeId> = BTreeSet::new();
            let mut sewn_vertices = BTreeSet::new();
            for &f in &copies {
                for e in face::p4::edges(b, f, None, false, None)? {
                    sewn_edges.insert(e);
                    if let Some(v) = edge::start_vertex(b, e)? {
                        sewn_vertices.insert(v);
                    }
                    if let Some(v) = edge::end_vertex(b, e)? {
                        sewn_vertices.insert(v);
                    }
                }
            }
            let keep_edges: Vec<EdgeId> = crate::topology::brep::p1::edges(b, None)?
                .into_iter()
                .filter(|e| !sewn_edges.contains(e))
                .collect();
            let keep_vertices: Vec<_> = crate::topology::brep::p1::vertices(b, None)?
                .into_iter()
                .filter(|v| !sewn_vertices.contains(v))
                .collect();
            crate::topology::brep::p5::remove_topological_with_notifications(
                k,
                b,
                Some(&keep_edges),
                Some(&keep_vertices),
                Some(env),
            )
        })
    }
}

/// Nearest blended edge to an unstitched lamina edge after a failed crease-trim stitch,
/// preferring lamina edges on `copies` (the merged sheet and extension faces). Returns the
/// target edge id and its curve midpoint so the caller can report which edge failed to sew.
fn failing_blended_edge(
    k: &Kernel,
    b: &Brep,
    copies: &[FaceId],
    blended_edges: &[EdgeId],
    blended_curves: &[Curve],
) -> Result<Option<(EdgeId, Point3)>> {
    let mut probe: Option<Point3> = None;
    'copies: for &f in copies {
        if b.face(f).is_err() {
            continue;
        }
        for e in face::p4::edges(b, f, None, false, None)? {
            if !edge::is_lamina(b, e)? {
                continue;
            }
            if let Some(curve) = edge::curve(b, e)? {
                probe = Some(curve.evaluate_point(k, edge::interval(b, e)?.evaluate(0.5))?);
                break 'copies;
            }
        }
    }
    if probe.is_none() {
        for e in crate::topology::brep::p1::edges(b, None)? {
            if !edge::is_lamina(b, e)? {
                continue;
            }
            if let Some(curve) = edge::curve(b, e)? {
                probe = Some(curve.evaluate_point(k, edge::interval(b, e)?.evaluate(0.5))?);
                break;
            }
        }
    }
    let Some(p) = probe else {
        return Ok(None);
    };
    let mut best: Option<(f64, EdgeId, Point3)> = None;
    for (&edge_id, curve) in blended_edges.iter().zip(blended_curves) {
        let ivl = curve.natural_interval();
        if let Some((_, dist)) = drop_on(k, curve, &ivl, p)? {
            if best.is_none_or(|(d, _, _)| dist < d) {
                let mid = curve.evaluate_point(k, ivl.evaluate(0.5))?;
                best = Some((dist, edge_id, mid));
            }
        }
    }
    Ok(best.map(|(_, edge_id, mid)| (edge_id, mid)))
}

/// [`FilletSession`]'s target edit through its notifications, on its objects alone (for
/// the session's other steps, which hold other fields of the session meanwhile).
pub(super) fn with_target_env_of<R>(
    objects: &mut SessionObjects<'_>,
    k: &mut Kernel,
    edit: impl FnOnce(&mut Kernel, &mut Brep, &mut NotifyEnv<'_>, BrepIdentity<'_>) -> Result<R>,
) -> Result<R> {
    let serial = objects.target_serial;
    let key_of = |entity| p3::topology_ref(serial, entity);
    let keys = |object| match object {
        CacheObject::Brep => key_of(EntityRef::Brep(BrepId::from_index(0))),
        CacheObject::Other(owner) => owner,
        object => crate::topology::brep::p2::caller_cache_key(&key_of, object),
    };
    let owner_of = |_: ObjectRef| None;
    let identity = BrepIdentity {
        brep_id: BrepId::from_index(
            u32::try_from(serial)
                .map_err(|_| invalid("target body identity exceeds query domain"))?,
        ),
        keys: &keys,
        owner_of: &owner_of,
    };
    let SessionObjects {
        target,
        notification_records,
        attributes,
        ..
    } = objects;
    if k.context.issued_brep_serial(target.notify_identity_slot()) != Some(serial) {
        return Err(invalid(
            "target body belongs to another notification context",
        ));
    }
    crate::geom::context::Context::with_notification_context(k, |k, context| {
        p3::seed_records(notification_records, target, &key_of);
        let mut env = NotifyEnv {
            records: notification_records,
            attributes,
            context: Some(context),
            key_of: &key_of,
        };
        edit(k, target, &mut env, identity)
    })?
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::nurbs::crv_basic::Curve as NlCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// The trim tolerance the end-construction tests pass.
    const TOL: f64 = 1e-6;

    /// The control points of `c` (a B-spline), each with its weight.
    fn control_polygon(c: &BSplineCurve) -> Vec<(Point3, f64)> {
        (0..c.number_control_points().unwrap())
            .map(|i| {
                c.control_point(ControlPointForm::EuclideanRational, i)
                    .unwrap()
            })
            .collect()
    }

    /// A certificate that `piece` is `before` with only its end control points moved: the
    /// same degree and knots, every control point and weight bit-identical but the first
    /// (where `q0` is given) and the last (where `q1` is), each of which is its meet (moved
    /// by `q - old`), its weight unchanged. A moved point is stored homogeneous (`q w`) and
    /// read back divided by `w`: equal to `q` but for that product's and quotient's
    /// rounding, 2 ulps of each coordinate (exact where `w = 1`).
    fn assert_only_ends_moved(
        before: &BSplineCurve,
        piece: &Curve,
        q0: Option<Point3>,
        q1: Option<Point3>,
    ) {
        let CurveKind::BSpline(after) = piece.kind() else {
            panic!("a B-spline piece: {:?}", piece.kind());
        };
        let (nb, na) = (before.nurb_ref().unwrap(), after.nurb_ref().unwrap());
        assert_eq!(na.degree(), nb.degree(), "the degree");
        assert_eq!(na.knots(), nb.knots(), "the knots");
        let (pb, pa) = (control_polygon(before), control_polygon(after));
        assert_eq!(pa.len(), pb.len(), "the control points' count");
        let last = pb.len() - 1;
        for (i, (&(b, wb), &(a, wa))) in pb.iter().zip(&pa).enumerate() {
            assert_eq!(wa, wb, "control point {i}'s weight");
            let want = match (i, q0, q1) {
                (0, Some(q), _) => q,
                (i, _, Some(q)) if i == last => q,
                _ => b,
            };
            if want == b {
                assert_eq!(a, b, "control point {i} unmoved");
            } else {
                for (x, y) in [(a.x, want.x), (a.y, want.y), (a.z, want.z)] {
                    let ulps = 2.0 * f64::EPSILON * y.abs().max(f64::MIN_POSITIVE);
                    assert!(
                        (x - y).abs() <= ulps,
                        "control point {i}: {a:?} on its meet {want:?}"
                    );
                }
                if wa == 1.0 {
                    assert_eq!(a, want, "control point {i}: exact at weight 1");
                }
            }
        }
    }

    /// A cubic S through (0, 0, 0) and (3, 0, 0) that crosses its chord at its middle:
    /// its ends and midpoint lie on the line, its quarter points `0.2887 h` off it.
    fn s_curve(h: f64) -> Curve {
        Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(1.0, h, 0.0, NOW),
                    CPoint::new(2.0, -h, 0.0, NOW),
                    CPoint::new(3.0, 0.0, 0.0, NOW),
                ],
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        ))
    }

    /// The bound the move's gate compares with `tol` (`distance_to_line_bound`, plus
    /// `straight_ends` for the edge) is over the whole curve, not samples: a crease that
    /// meets the edge's line at its ends and middle but bows off it between is not within
    /// `tol`, and is not taken as a straight edge.
    #[test]
    fn a_crease_on_the_line_at_three_points_but_bowed_between_is_not_moved_onto_it() {
        let k = Kernel::default();
        let (h, tol) = (2e-4, 4e-5);
        let c = s_curve(h);
        let (a, b) = (Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0));
        for t in [0.0, 0.5, 1.0] {
            let p = c.evaluate_point(&k, t).unwrap();
            assert!(p.y.abs() <= 1e-15, "t {t}: {p:?} on the line");
        }
        // the true deviation, at t = 1/2 -+ sqrt(3)/6: h / (2 sqrt 3) = 0.2887 h
        let worst = (0..=1000)
            .map(|i| c.evaluate_point(&k, f64::from(i) / 1000.0).unwrap().y.abs())
            .fold(0.0_f64, f64::max);
        assert!(worst > tol, "the S bows {worst} off the line, past tol");
        let bound = distance_to_line_bound(&c, a, b).unwrap();
        assert!(
            bound >= worst,
            "bound {bound} encloses the deviation {worst}"
        );
        assert!(bound > tol, "the gate refuses: bound {bound} > tol {tol}");
        // and the S is not taken as a straight edge
        assert!(straight_ends(&k, &c).unwrap().is_none());
    }

    /// A crease that crosses the top of a box at a corner vertex ends its run at that vertex,
    /// which names no edge. The seam events still carry each edge of the top that meets the
    /// vertex, at the vertex's parameter on it, so the tangent-edge refusal sees the
    /// crossing; the vertical edge there bounds the two side faces only and is no event.
    #[test]
    fn a_crease_through_a_vertex_gives_a_seam_event_for_each_face_edge_there() {
        use crate::primitive::api_primitives::create_box;
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
        let corner = Point3::new(40.0, 20.0, 10.0);
        let v = b
            .vertices()
            .ids()
            .find(|&v| b.vertex(v).unwrap().point.distance(corner) < 1e-9)
            .unwrap();
        // the top: the face at the corner whose two edges there both lie at z = 10
        let top = edge_faces_at(&b, v)
            .into_iter()
            .find(|&f| {
                let es = vertex::edges(&b, v, Some(f)).unwrap();
                es.len() == 2
                    && es.iter().all(|&e| {
                        let other = edge::other_vertex(&b, e, Some(v)).unwrap().unwrap();
                        (b.vertex(other).unwrap().point.z - 10.0).abs() < 1e-9
                    })
            })
            .unwrap();
        // in the plane of the top, from inside it through the corner and on past it
        let crease: Curve = Line::from_points(
            Point3::new(30.0, 10.0, 10.0),
            Point3::new(50.0, 30.0, 10.0),
            3,
        )
        .into();
        let (mut in_face, mut along) = (Vec::new(), Vec::new());
        let events =
            pieces_in_face_to_ends(&k, &crease, &mut b, top, TOL, &mut in_face, &mut along)
                .unwrap();
        let of_top = vertex::edges(&b, v, Some(top)).unwrap();
        for e in vertex::edges(&b, v, None).unwrap() {
            let range = edge::interval(&b, e).unwrap();
            let at = if edge::start_vertex(&b, e).unwrap() == Some(v) {
                range.min()
            } else {
                range.max()
            };
            assert_eq!(
                events.contains(&(e, at)),
                of_top.contains(&e),
                "edge {e:?} at the corner, parameter {at}, is a seam event exactly when it \
                 bounds the top: {events:?}"
            );
        }
    }

    /// The faces the edges at `v` lie on.
    fn edge_faces_at(b: &Brep, v: crate::topology::VertexId) -> Vec<FaceId> {
        let mut out = Vec::new();
        for e in vertex::edges(b, v, None).unwrap() {
            for f in edge::faces(b, e, None).unwrap() {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// #17016, #2385: a crease that leaves the top of a cylinder through the vertex where its
    /// seam starts ends its run at that vertex, which names no edge. The seam, an edge of
    /// the side face only (tangent to itself all along), touches the top at that vertex
    /// alone and separates no face from the top: the crease leaves the top there across the
    /// top's own circle, which is sharp, so it is not refused. (Judging the seam refused
    /// #2385's disc, whose crease runs along the bottom edge through the seam's foot.)
    #[test]
    fn a_crease_through_a_seam_vertex_crosses_only_the_face_edges_there() {
        use crate::primitive::api_primitives::create_cylinder;
        let mut k = Kernel::default();
        let mut b = create_cylinder(&mut k, Point3::ZERO, 10.0, 20.0).unwrap();
        // the vertex of the top circle: the end of the seam at z = 20
        let v = b
            .vertices()
            .ids()
            .find(|&v| (b.vertex(v).unwrap().point.z - 20.0).abs() < 1e-9)
            .unwrap();
        let at = b.vertex(v).unwrap().point;
        let (mut seam, mut circle) = (None, None);
        for e in vertex::edges(&b, v, None).unwrap() {
            if edge::other_vertex(&b, e, Some(v)).unwrap() == Some(v) {
                circle = Some(e);
            } else {
                seam = Some(e);
            }
        }
        let (seam, circle) = (seam.unwrap(), circle.unwrap());
        let side = edge::faces(&b, seam, None).unwrap();
        assert!(side.iter().all(|&g| g == side[0]), "the seam has one face");
        let side = side[0];
        let top = edge::faces(&b, circle, None)
            .unwrap()
            .into_iter()
            .find(|&g| g != side)
            .unwrap();
        // in the plane of the top, from its centre out through the vertex and on past it
        let centre = Point3::new(0.0, 0.0, 20.0);
        let crease: Curve = Line::from_points(centre, centre + (at - centre) * 2.0, 3).into();
        let (mut in_face, mut along) = (Vec::new(), Vec::new());
        let events =
            pieces_in_face_to_ends(&k, &crease, &mut b, top, TOL, &mut in_face, &mut along)
                .unwrap();
        // the circle is an event at the vertex's parameter on it; the seam is none
        let range = edge::interval(&b, circle).unwrap();
        assert!(events.contains(&(circle, range.min())), "{events:?}");
        assert!(events.iter().all(|&(e, _)| e != seam), "{events:?}");
        // the fillet's default tangency, 10 degrees (`edge_refusals::DEFAULT_TANGENCY`): the
        // circle is sharp at 90 degrees, so any budget below that decides the same
        let tangency = 10.0 * (crate::math::PI / 180.0);
        let rolled_supports = BTreeSet::from([side]);
        let refused = crease_across_tangent_edge(
            &k,
            &mut b,
            top,
            &events,
            &rolled_supports,
            tangency,
            tangency,
        )
        .unwrap();
        assert!(refused.is_none(), "the circle at the vertex is sharp");
    }

    /// The leftover check refuses what a move left off and passes a body that holds
    /// together: the trim's `BadMerge` refusal (#17704) is this `true`.
    #[test]
    fn a_move_s_leftovers_are_found_on_the_geometry() {
        use crate::primitive::api_primitives::{create_box, create_cylinder};
        let mut k = Kernel::default();
        let tol = 4e-5;

        // a box's vertex: every edge at it runs through it
        let mut b = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
        let v = b.vertices().ids().next().unwrap();
        let mut moves = MovedEdges {
            vertices: vec![v],
            ..MovedEdges::default()
        };
        assert!(!move_leftovers_kept(&k, &b, &moves, tol).unwrap());
        // the vertex moved 1e-6 off its edges (far past `tol * 1e-6`): refused
        b.vertex_mut(v).unwrap().point.x += 1e-6;
        assert!(move_leftovers_kept(&k, &b, &moves, tol).unwrap());

        // a box's edge taken as a crease piece: it lies on both its planes
        let b = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
        let e = b.edges().ids().next().unwrap();
        moves = MovedEdges {
            pieces: vec![edge_curve(&k, &b, e).unwrap()],
            ..MovedEdges::default()
        };
        assert!(!move_leftovers_kept(&k, &b, &moves, tol).unwrap());

        // a cylinder's rim taken as a crease piece: the rim bounds the curved side, whose
        // distance no plane bound encloses: refused
        let c = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
        let rim = c
            .edges()
            .ids()
            .find(|&e| {
                edge::faces(&c, e, None).unwrap().iter().any(|&g| {
                    let surface = face::p2::surface(&c, g).unwrap().unwrap();
                    crate::blend::fillet_overflow::plane_of(&k, surface)
                        .unwrap()
                        .is_none()
                }) && edge::faces(&c, e, None).unwrap().iter().any(|&g| {
                    let surface = face::p2::surface(&c, g).unwrap().unwrap();
                    crate::blend::fillet_overflow::plane_of(&k, surface)
                        .unwrap()
                        .is_some()
                })
            })
            .expect("a rim between a cap and the side");
        let moves = MovedEdges {
            pieces: vec![edge_curve(&k, &c, rim).unwrap()],
            ..MovedEdges::default()
        };
        assert!(move_leftovers_kept(&k, &c, &moves, tol).unwrap());
    }

    /// A polynomial B-spline of degree 2 through `pts` over `knots` (`pts.len() + 3`).
    fn quadratic(pts: &[(f64, f64, f64)], knots: Vec<f64>) -> Curve {
        Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                pts.iter()
                    .map(|&(x, y, z)| CPoint::new(x, y, z, NOW))
                    .collect(),
                2,
                knots,
            )
            .unwrap(),
            3,
            false,
        ))
    }

    /// The ends of `c` over its natural interval.
    fn ends(k: &Kernel, c: &Curve) -> (Point3, Point3) {
        let ivl = c.natural_interval();
        (
            c.evaluate_point(k, ivl.min()).unwrap(),
            c.evaluate_point(k, ivl.max()).unwrap(),
        )
    }

    /// #17673 review: a line crease ends exactly on both its meets, as a line. Before, a
    /// piece that was no B-spline kept both its classified ends, short of the meets.
    #[test]
    fn a_line_crease_piece_ends_on_both_its_meets() {
        let k = Kernel::default();
        let mut line = Line::from_points(Point3::ZERO, Point3::new(10.0, 0.0, 0.0), 3);
        line.make_nurb().unwrap();
        let crease = Curve::from(line);
        let ivl = crease.natural_interval();
        let (q0, q1) = (
            Point3::new(-2e-6, 3e-7, 0.0),
            Point3::new(10.0 + 4e-6, -1e-7, 0.0),
        );
        let piece = piece_ending_at(
            &k,
            &crease,
            (ivl.min(), Some(q0)),
            (ivl.max(), Some(q1)),
            TOL,
        )
        .unwrap();
        assert!(matches!(piece.kind(), CurveKind::Line(_)), "still a line");
        let (a, z) = ends(&k, &piece);
        assert!(a.distance(q0) <= 1e-15, "start {a:?} on its meet {q0:?}");
        assert!(z.distance(q1) <= 1e-14, "end {z:?} on its meet {q1:?}");
        // one meet only: the other end stays where it was
        let piece =
            piece_ending_at(&k, &crease, (ivl.min(), None), (ivl.max(), Some(q1)), TOL).unwrap();
        let (a, z) = ends(&k, &piece);
        assert_eq!(a, Point3::ZERO);
        assert!(z.distance(q1) <= 1e-14, "end {z:?} on its meet {q1:?}");
    }

    /// #17673 review: a circular arc's end is put on its meet through the arc's exact
    /// rational B-spline: the end lands on the meet, the start stays, and no point moves
    /// farther than the meet is from the end (the moved control point's rational basis
    /// function is at most 1); the arc away from its last span is the circle still.
    #[test]
    fn a_circle_crease_piece_ends_on_its_meet() {
        let k = Kernel::default();
        let (r, centre) = (5.0, Point3::new(1.0, 2.0, 3.0));
        let place = crate::geom::axis2_placement::Axis2Placement::new(centre, Vec3::X, Vec3::Y);
        let domain = crate::geom::extent1d::Extent1d::from_min_max(0.0, 270.0);
        let circle = crate::curve::ellipse::Ellipse::create_canonical_circle(
            &k,
            &place,
            r,
            Some(&domain),
            None,
        )
        .unwrap();
        let crease = Curve::from(circle);
        let ivl = crease.natural_interval();
        let (a0, z0) = ends(&k, &crease);
        let q = z0 + Vec3::new(3e-6, -2e-6, 1e-6);
        let piece =
            piece_ending_at(&k, &crease, (ivl.min(), None), (ivl.max(), Some(q)), TOL).unwrap();
        assert!(matches!(piece.kind(), CurveKind::BSpline(_)));
        // certificate: the circle's own exact NURB, only its last control point moved
        let before = clamped_spline(crease.b_spline_base().unwrap()).unwrap();
        assert_only_ends_moved(&before, &piece, None, Some(q));
        let (a, z) = ends(&k, &piece);
        assert!(a.distance(a0) <= 1e-14, "start {a:?} stays at {a0:?}");
        assert!(z.distance(q) <= 1e-14, "end {z:?} on its meet {q:?}");
        let moved = q.distance(z0);
        let pivl = piece.natural_interval();
        // where the last span starts: the last knot below the end
        let CurveKind::BSpline(spline) = piece.kind() else {
            panic!("a B-spline");
        };
        let last_span = *spline
            .nurb_ref()
            .unwrap()
            .knots()
            .iter()
            .rev()
            .find(|&&u| u < pivl.max())
            .unwrap();
        assert!(last_span > pivl.min(), "an arc of more than one span");
        for i in 0..=400 {
            let t = pivl.min() + (pivl.max() - pivl.min()) * f64::from(i) / 400.0;
            let p = piece.evaluate_point(&k, t).unwrap();
            let off = ((p - centre).length() - r).abs();
            assert!(off <= moved + 1e-12, "t {t}: {off} off the circle");
            if t <= last_span {
                assert!(
                    off <= 1e-12,
                    "t {t}: {off} off the circle before the last span"
                );
            }
        }
    }

    /// The largest distance between `c`'s end (`at_end`: the last; else the first) and its
    /// points within `1e-9` of its parameter range of it: a curve that runs on to its end
    /// point, not one whose end is a lone control point off its last span.
    fn end_gap(k: &Kernel, c: &Curve, at_end: bool) -> f64 {
        let ivl = c.natural_interval();
        let (t, inward) = if at_end {
            (ivl.max(), -1.0)
        } else {
            (ivl.min(), 1.0)
        };
        let end = c.evaluate_point(k, t).unwrap();
        (1..=10)
            .map(|i| {
                let s = t + inward * f64::from(i) * 1e-10 * (ivl.max() - ivl.min());
                c.evaluate_point(k, s).unwrap().distance(end)
            })
            .fold(0.0, f64::max)
    }

    /// #17673 review: a B-spline unclamped at both ends is a curve over its domain
    /// `[U_p, U_{m-p}]` (the kernel's evaluation gives the end control point at `U0` and
    /// `Um`, off the end spans, and fails inside the domain). The trim clamps it first
    /// ([`crease_as_clamped`]) and ends its pieces on the clamped curve: they end on both
    /// their meets and run on to them. The unclamped curve itself is refused, naming its
    /// first met end, never ended away from its classified end.
    #[test]
    fn an_unclamped_crease_is_clamped_first_and_its_piece_ends_on_its_meets() {
        let k = Kernel::default();
        let crease = quadratic(
            &[
                (0.0, 0.0, 0.0),
                (1.0, 2.0, 0.0),
                (3.0, 2.0, 1.0),
                (4.0, 0.0, 1.0),
            ],
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        );
        let ivl = crease.natural_interval();
        let CurveKind::BSpline(spline) = crease.kind() else {
            panic!("a B-spline");
        };
        assert!(!spline_clamped(spline).unwrap());
        // the clamped curve is the curve over its domain [U_p, U_{m-p}] = [2, 4]
        let clamped = crease_as_clamped(&crease).unwrap().into_owned();
        let CurveKind::BSpline(c) = clamped.kind() else {
            panic!("a B-spline");
        };
        assert!(spline_clamped(c).unwrap());
        let civl = clamped.natural_interval();
        assert_eq!((civl.min(), civl.max()), (2.0, 4.0));
        // the uniform quadratic's closed forms (the kernel cannot evaluate the unclamped
        // crease inside its domain): (P0+P1)/2, (P0+6P1+P2)/8, (P1+P2)/2, (P1+6P2+P3)/8,
        // (P2+P3)/2 at 2, 2.5, 3, 3.5, 4
        let pts = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(3.0, 2.0, 1.0),
            Point3::new(4.0, 0.0, 1.0),
        ];
        let half = |i: usize| (pts[i] + pts[i + 1]) * 0.5;
        let mid = |i: usize| (pts[i] + pts[i + 1] * 6.0 + pts[i + 2]) * 0.125;
        for (t, want) in [
            (2.0, half(0)),
            (2.5, mid(0)),
            (3.0, half(1)),
            (3.5, mid(1)),
            (4.0, half(2)),
        ] {
            let got = clamped.evaluate_point(&k, t).unwrap();
            assert!(
                got.distance(want) <= 1e-14,
                "t {t}: {got:?} against {want:?}"
            );
        }
        // meets near the domain's ends, classified on the clamped curve
        let (d0, d1) = (Vec3::new(2e-6, 1e-6, 0.0), Vec3::new(-1e-6, 0.0, 3e-6));
        let (q0, q1) = (half(0) + d0, half(2) + d1);
        let piece = piece_ending_at(
            &k,
            &clamped,
            (civl.min(), Some(q0)),
            (civl.max(), Some(q1)),
            TOL,
        )
        .unwrap();
        // certificate: the clamped curve, only its two end control points moved
        assert_only_ends_moved(c, &piece, Some(q0), Some(q1));
        let (a, z) = ends(&k, &piece);
        assert!(a.distance(q0) <= 1e-14, "start {a:?} on its meet {q0:?}");
        assert!(z.distance(q1) <= 1e-14, "end {z:?} on its meet {q1:?}");
        for at_end in [false, true] {
            let gap = end_gap(&k, &piece, at_end);
            assert!(gap <= 1e-8, "end {at_end}: the curve {gap} off its end");
        }
        // no point moves farther than the two meets' moves together
        for i in 0..=60 {
            let t = 2.0 + 2.0 * f64::from(i) / 60.0;
            let moved = piece
                .evaluate_point(&k, t)
                .unwrap()
                .distance(clamped.evaluate_point(&k, t).unwrap());
            assert!(
                moved <= d0.length() + d1.length() + 1e-14,
                "t {t}: moved {moved}"
            );
        }
        // the unclamped curve itself: refused at its start, never ended off its
        // classified ends
        let err = piece_ending_at(
            &k,
            &crease,
            (ivl.min(), Some(q0)),
            (ivl.max(), Some(q1)),
            TOL,
        )
        .expect_err("an unclamped piece's ends are not its classified ends");
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CreasePieceEndUnbuildable {
                kind: "unclamped B-spline",
                at_end: false,
                t: ivl.min(),
            }),
            "{err:?}"
        );
    }

    /// #17673 review (review comment 4587418682): a B-spline clamped at its start but not at
    /// its end, its start's meet alone. Clamped first, the end the trim records is the
    /// clamped curve's end, and the piece's un-met end is that end bit for bit, not the
    /// original end control point (4, 0, 1), 1.118 away. Both meets together are kept
    /// too; the unclamped curve itself is refused, naming its start.
    #[test]
    fn a_meet_at_one_end_of_a_crease_unclamped_at_the_other_keeps_the_recorded_end() {
        let k = Kernel::default();
        let crease = quadratic(
            &[
                (0.0, 0.0, 0.0),
                (1.0, 2.0, 0.0),
                (3.0, 2.0, 1.0),
                (4.0, 0.0, 1.0),
            ],
            vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0],
        );
        let ivl = crease.natural_interval();
        let original_end = crease.evaluate_point(&k, ivl.max()).unwrap();
        assert_eq!(
            original_end,
            Point3::new(4.0, 0.0, 1.0),
            "the end control point"
        );
        // clamped first: the curve over its domain [U_2, U_4] = [0, 2]
        let clamped = crease_as_clamped(&crease).unwrap().into_owned();
        let civl = clamped.natural_interval();
        assert_eq!((civl.min(), civl.max()), (0.0, 2.0));
        let CurveKind::BSpline(before) = clamped.kind() else {
            panic!("a B-spline");
        };
        // the ends the trim records: the clamped curve's, evaluated
        let (a0, z0) = ends(&k, &clamped);
        // closed form: the uniform quadratic's end (P2+P3)/2
        assert!(z0.distance(Point3::new(3.5, 1.0, 1.0)) <= 1e-14, "{z0:?}");
        assert!(
            z0.distance(original_end) > 1.0,
            "the clamp moved the natural end"
        );
        let (q0, q1) = (
            a0 + Vec3::new(0.0, 2e-6, 1e-6),
            z0 + Vec3::new(1e-6, 1e-6, 0.0),
        );
        // the start's meet alone: the piece's end is the recorded end, bit for bit
        let piece = piece_ending_at(
            &k,
            &clamped,
            (civl.min(), Some(q0)),
            (civl.max(), None),
            TOL,
        )
        .unwrap();
        assert_only_ends_moved(before, &piece, Some(q0), None);
        let (a, z) = ends(&k, &piece);
        assert_eq!(a, q0, "the start's meet");
        assert_eq!(z, z0, "the un-met end is the recorded (clamped) end");
        assert_ne!(z, original_end);
        assert!(end_gap(&k, &piece, true) <= 1e-8);
        // both meets: both kept
        let piece = piece_ending_at(
            &k,
            &clamped,
            (civl.min(), Some(q0)),
            (civl.max(), Some(q1)),
            TOL,
        )
        .unwrap();
        assert_only_ends_moved(before, &piece, Some(q0), Some(q1));
        let (a, z) = ends(&k, &piece);
        assert_eq!(a, q0, "the start's meet is kept");
        assert!(z.distance(q1) <= 1e-14, "end {z:?} on its meet {q1:?}");
        for at_end in [false, true] {
            let gap = end_gap(&k, &piece, at_end);
            assert!(gap <= 1e-8, "end {at_end}: the curve {gap} off its end");
        }
        // the unclamped curve itself: refused, naming its met start, never a piece whose
        // un-met end is not the one recorded
        let err = piece_ending_at(&k, &crease, (ivl.min(), Some(q0)), (ivl.max(), None), TOL)
            .expect_err("an unclamped piece's end is not its classified end");
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CreasePieceEndUnbuildable {
                kind: "unclamped B-spline",
                at_end: false,
                t: ivl.min(),
            }),
            "{err:?}"
        );
    }

    /// #17673 review: a crease piece of a curve kind with no control polygon (an offset
    /// curve) is refused with a typed error where it has a meet, never left short of it;
    /// with none it is the crease as it is.
    #[test]
    fn a_crease_piece_with_no_control_polygon_is_refused_at_a_meet() {
        let k = Kernel::default();
        let base = s_curve(0.5);
        let interval = base.natural_interval();
        let offset =
            crate::curve::offset_curve::OffsetCurve::new(3, base, interval, Vec3::Z, 0.25, true)
                .unwrap();
        let crease = Curve::from(offset);
        let ivl = crease.natural_interval();
        let (_, z0) = ends(&k, &crease);
        let q = z0 + Vec3::new(1e-6, 0.0, 0.0);
        let err = piece_ending_at(&k, &crease, (ivl.min(), None), (ivl.max(), Some(q)), TOL)
            .expect_err("no construction for its end");
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CreasePieceEndUnbuildable {
                kind: "offset",
                at_end: true,
                t: ivl.max(),
            }),
            "{err:?}"
        );
        assert_eq!(err.kind(), crate::error::ErrorKind::Unsupported);
        // the first end with a meet is named: the start, where both have one
        let q0 = ends(&k, &crease).0 + Vec3::new(0.0, 1e-6, 0.0);
        let err = piece_ending_at(
            &k,
            &crease,
            (ivl.min(), Some(q0)),
            (ivl.max(), Some(q)),
            TOL,
        )
        .expect_err("no construction for its ends");
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CreasePieceEndUnbuildable {
                kind: "offset",
                at_end: false,
                t: ivl.min(),
            }),
            "{err:?}"
        );
        let piece =
            piece_ending_at(&k, &crease, (ivl.min(), None), (ivl.max(), None), TOL).unwrap();
        assert!(matches!(piece.kind(), CurveKind::Offset(_)));
    }

    /// #17673 review: a trimmed circular arc's piece is the trimmed arc's own exact NURB
    /// (the conic's base follows `crease_piece`'s trim), only its end control point moved:
    /// its start is the circle's point at the trim's start, the unmoved base ends on the
    /// circle's point at the trim's end, and the piece ends on its meet.
    #[test]
    fn a_trimmed_circle_crease_piece_is_its_trimmed_arc_with_its_end_on_its_meet() {
        let k = Kernel::default();
        let (r, centre) = (5.0, Point3::new(1.0, 2.0, 3.0));
        let place = crate::geom::axis2_placement::Axis2Placement::new(centre, Vec3::X, Vec3::Y);
        let domain = crate::geom::extent1d::Extent1d::from_min_max(0.0, 270.0);
        let circle = crate::curve::ellipse::Ellipse::create_canonical_circle(
            &k,
            &place,
            r,
            Some(&domain),
            None,
        )
        .unwrap();
        let crease = Curve::from(circle);
        // 30 and 200 degrees along the 270-degree arc, in its natural parameter
        let ivl = crease.natural_interval();
        let at = |deg: f64| ivl.min() + (ivl.max() - ivl.min()) * deg / 270.0;
        let (t0, t1) = (at(30.0), at(200.0));
        let (p0, p1) = (
            crease.evaluate_point(&k, t0).unwrap(),
            crease.evaluate_point(&k, t1).unwrap(),
        );
        // the trimmed arc's exact NURB runs from the circle's point at t0 to its point at t1
        let trimmed = crease_piece(&k, &crease, t0, t1).unwrap();
        assert!(matches!(trimmed.kind(), CurveKind::Ellipse(_)));
        let base = clamped_spline(trimmed.b_spline_base().unwrap()).unwrap();
        let base_curve = Curve::from(base.clone_exact());
        let (b0, b1) = ends(&k, &base_curve);
        assert!(b0.distance(p0) <= 1e-12, "base start {b0:?} at {p0:?}");
        assert!(b1.distance(p1) <= 1e-12, "base end {b1:?} at {p1:?}");
        // and it is on the circle throughout: the base is the trimmed arc, not the 270 one
        let bivl = base_curve.natural_interval();
        for i in 0..=100 {
            let t = bivl.min() + (bivl.max() - bivl.min()) * f64::from(i) / 100.0;
            let p = base_curve.evaluate_point(&k, t).unwrap();
            let off = ((p - centre).length() - r).abs();
            assert!(off <= 1e-12, "t {t}: {off} off the circle");
        }
        let q = p1 + Vec3::new(-2e-6, 1e-6, 3e-6);
        let piece = piece_ending_at(&k, &crease, (t0, None), (t1, Some(q)), TOL).unwrap();
        assert_only_ends_moved(&base, &piece, None, Some(q));
        let (a, z) = ends(&k, &piece);
        assert!(a.distance(p0) <= 1e-12, "start {a:?} at the trim's {p0:?}");
        assert!(z.distance(q) <= 1e-14, "end {z:?} on its meet {q:?}");
    }

    /// #17673 review: a line piece whose two meets are within the trim tolerance of each
    /// other is refused as degenerate, never built as a degenerate line.
    #[test]
    fn a_line_crease_piece_whose_meets_coincide_is_refused() {
        let k = Kernel::default();
        let mut line = Line::from_points(Point3::ZERO, Point3::new(10.0, 0.0, 0.0), 3);
        line.make_nurb().unwrap();
        let crease = Curve::from(line);
        let ivl = crease.natural_interval();
        let q = Point3::new(5.0, 0.0, 0.0);
        let err = piece_ending_at(
            &k,
            &crease,
            (ivl.min(), Some(q)),
            (ivl.max(), Some(q + Vec3::new(TOL * 0.5, 0.0, 0.0))),
            TOL,
        )
        .expect_err("no line between coinciding meets");
        assert!(
            matches!(
                err,
                SolidError::Degenerate {
                    operation: OP,
                    entity: None
                }
            ),
            "{err:?}"
        );
    }
}
