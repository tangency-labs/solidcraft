//! The cliff overflow trimmed at its creases on the live session (not upstream): the
//! fork's `SmFilletExecutive::TrimAtCreases` (`source/SMLib/src/SmFilletOverflow.cpp`,
//! tangency-labs/smlib#79 step 6 at 5e8c9c1: 99e57b8, 668fe9f, bacd0bb, 2d4b154, bc8c44d,
//! de5df14, 8832ead; `fixes.fillet_overflow_trim_at_creases`, WR-695). The geometry steps
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
//! - **The sheet leaves the session.** As in `subtract_removal_region`, the sheet (the
//!   intermediate body) is taken out of the session first and edited on its own; the
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
//!   with `fixes.fillet_overflow_trim_at_creases` (WR-706, #2223): each such end is
//!   cut by its section's plane and closed by the section's flat face, and a side of that
//!   face that lies along an edge of the body is sewn to the edge's piece the rail's
//!   imprint cuts off, not imprinted.
//! - **Creases along an edge (not the fork's).** With
//!   `fixes.fillet_overflow_crease_along_edge` (WR-717, #2385), a crease that runs along an
//!   edge of the face it crosses, within the trim's tolerance, is that edge: the sheet is
//!   split along it, and the edge stands as its imprint, so the material taken off stops
//!   there and the sheet is sewn to it. The fork drops such a crease as not inside the
//!   face, leaves the sheet's piece past the face, and refuses at the sewing (a disc 2 high
//!   rounded 2.001: the crease runs 2.5e-7 inside the bottom edge). Without it the caller subtracts the round, as before.
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

use std::collections::{BTreeMap, BTreeSet};

use super::{invalid, FilletBody, FilletObject, FilletSession, SessionObjects};
use crate::blend::fillet_cliff_overflow::{
    cut_fillet_at_open_end, section_edge, section_end, section_end_face, Refusal, SectionEnd,
};
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_executive::p3::NoRoomReason;
use crate::blend::fillet_geom::p1::{FilletEdgeType, FilletErrorType, FilletGeomId};
use crate::blend::fillet_overflow::{
    creases, edge_curve, edge_on_surface, extend, in_material, make_extension_face,
    pieces_along_edges, pieces_in_face, ExtendOutcome, Extension, Rail,
};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::ids;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::b_spline_curve::ControlPointForm;
use crate::nurbs::knots::Side;
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
fn is_tangent_edge(k: &Kernel, b: &mut Brep, e: EdgeId, tangency_tol_rad: f64) -> Result<bool> {
    if !edge::is_manifold(b, e)? {
        return Ok(false);
    }
    Ok(
        crate::blend::fillet_solver::p1::test_manifold_tangent_rollover(k, b, e, tangency_tol_rad)?
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

impl FilletSession<'_> {
    /// `SmFilletExecutive::HasOpenSetBack` (the fork's ef57570): whether a corner is an
    /// open G1 setback (its hole made by the trim's imprint, no patches).
    pub(super) fn has_open_setback(&self) -> bool {
        self.executive.corners.iter().any(|c| {
            matches!(c, FilletObject::Corner(id)
                if self.objects.corners.get(id.0 as usize).is_some_and(FilletCorner::is_setback_open))
        })
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
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?
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
            .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
                .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?;
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
                    .ok_or_else(|| SolidError::blend_missing(OP, "the solver's offset surface"))?;
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
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge's start vertex"))?,
            edge::end_vertex(sheet, e)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge's end vertex"))?,
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
                    .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
    /// Each variable round on planes must hold its law; each open end is cut by its end
    /// face's plane ([`cut_fillet_at_open_end`]). Each sheet face is sectioned by every
    /// face of the target near it whose surface holds none of its boundary ([`creases`]); a
    /// crease is kept where it lies inside the face it is on and the sheet face
    /// ([`pieces_in_face`]), and the sheet face is split along all of them at once; a
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
    /// The refusals as `SM_ERR`; those of the steps.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub(super) fn trim_at_creases(&mut self, k: &mut Kernel) -> Result<()> {
        crate::profile::profile_scope!(k, "blend::session::trim_at_creases");
        if self.objects.intermediate.is_none() {
            return Err(SolidError::blend_missing(OP, "the intermediate body"));
        }
        let solvers = self.executive.solvers.clone();
        for &solver in &solvers {
            if !self.holds_its_law(k, solver)? {
                self.note_refusal(refusal(
                    FilletErrorType::NoRoom,
                    Some(NoRoomReason::CliffLawNotHeld),
                    "Fillet Error: a variable round traced on its faces' continuations does not hold its radius law",
                ));
                return Err(SolidError::blend_failed(
                    OP,
                    "a variable round does not hold its radius law on its faces' continuations",
                ));
            }
        }

        // What the trim reads of the solvers, before the sheet leaves the session.
        let mut faces_of: BTreeMap<FilletObject, Vec<(FilletGeomId, Option<FaceId>)>> =
            BTreeMap::new();
        let mut approx = Vec::new();
        let mut tangency = 0.0;
        let mut blended_edges: Vec<EdgeId> = Vec::new();
        for &solver in &solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let base = self.objects.solver(id)?.base();
            approx.push(base.approx_tol_3d);
            tangency = smos_max(tangency, base.tangency_tolerance());
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's edge use"))?;
            blended_edges.push(
                eu1::edge(self.objects.target, eu)?
                    .ok_or_else(|| SolidError::blend_missing(OP, "the edge use's edge"))?,
            );
            faces_of.insert(solver, self.fillet_faces(solver)?);
        }
        let open_ends = self.executive.open_ends.clone();
        // WR-706: a section end (WR-674) is trimmed here only with the crease trim
        let section_trim = k.fixes.fillet_overflow_trim_at_creases;
        if !section_trim && open_ends.iter().any(|end| end.section) {
            return Err(invalid("a section end is not trimmed at its creases"));
        }

        let mut sheet = self
            .objects
            .intermediate
            .take()
            .ok_or_else(|| SolidError::blend_missing(OP, "the intermediate body"))?;

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
            let base = self.objects.solver(id)?.base();
            let solver_tol = base.approx_tol_3d;
            let target = &*self.objects.target;
            let eu = base
                .edgeuse(0)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's edge use"))?;
            let fil_edge = eu1::edge(target, eu)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge use's edge"))?;
            let edge_mid = edge::curve(target, fil_edge)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?
                .evaluate_point(k, edge::interval(target, fil_edge)?.evaluate(0.5))?;
            let vertex_point = vertex::point(target, v)?;
            let end_surface = if end.section {
                // WR-706: a section end (WR-674) is cut by the section through the vertex,
                // normal to the filleted edge
                let section = section_end(k, target, fil_edge, v)?.ok_or_else(|| {
                    SolidError::blend_failed(OP, "no section through the end vertex")
                })?;
                section_ends.push(section);
                section_solvers.push(end.solver);
                Surface::from(Plane::from_normal(vertex_point, section.normal))
            } else {
                face::p2::surface(target, end_face)?
                    .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?
                    .clone_for_evaluation()?
            };
            let entry = faces_of
                .get_mut(&end.solver)
                .ok_or_else(|| SolidError::blend_missing(OP, "the solver's fillet faces"))?;
            let mut faces: Vec<Option<FaceId>> = entry.iter().map(|&(_, f)| f).collect();
            let cut = cut_fillet_at_open_end(
                k,
                &mut sheet,
                &mut faces,
                &end_surface,
                vertex_point,
                edge_mid,
                solver_tol,
                &mut note,
            );
            for (slot, f) in entry.iter_mut().zip(faces) {
                slot.1 = f;
            }
            // not the fork's (#2401, WR-696): an open end the end face does not cut is an
            // overflow's, absent below the size at which the end's blend stays on its
            // faces: that size (open)
            if note.is_some() && cut.is_err() {
                self.executive
                    .note_fit_on_its_faces(k, &mut self.objects, end.solver)?;
            }
            self.noted(note.take(), cut)?;
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
                    self.objects.intermediate = Some(sheet);
                    self.note_refusal(refusal(
                        FilletErrorType::NoRoom,
                        Some(NoRoomReason::TangentContinuationEndUnsupported),
                        "Fillet Error: a round ending where its edge runs on tangentially has no single section there",
                    ));
                    return Err(SolidError::blend_failed(OP, "a round that ends where its edge runs on tangentially has no single section there"));
                };
                section_end_face(k, &mut sheet, end, section, 10.0 * tol)?;
            }
        }

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
            tangency,
            &section_ends,
            &mut note,
        );
        if let Some(refused) = note.take() {
            self.note_refusal(refused);
        }
        if trimmed.is_err() {
            self.objects.target.restore(kept);
        }
        trimmed
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
        tangency: f64,
        section_ends: &[SectionEnd],
        note: &mut Option<Refusal>,
    ) -> Result<()> {
        let refuse = |note: &mut Option<Refusal>, r: Refusal| -> Result<()> {
            *note = Some(r);
            Err(SolidError::blend_failed(
                OP,
                "the trim at the creases is refused",
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
            return refuse(
                note,
                refusal(
                    FilletErrorType::NoRoom,
                    Some(NoRoomReason::StripBelowResolution),
                    "Fillet Error: a blend past its faces would leave a strip of the body along an edge thinner than the part resolves: a smaller blend keeps it",
                ),
            );
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
        let mut kept_rounds: BTreeSet<usize> = BTreeSet::new();

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
        // WR-717: the body's edges those creases stand for
        let mut crease_edges: Vec<EdgeId> = Vec::new();
        for s in crate::topology::brep::p1::faces(&sheet, None)? {
            let mut sheet_box =
                crate::query::face::p1::calculate_bounding_box(k, &sheet, s, false)?;
            sheet_box.expand_absolute(on_tol);
            let sheet_edges = face::p4::edges(&sheet, s, None, false, None)?;
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
                    if holds
                        && k.fixes.fillet_overflow_crease_along_edge
                        && edge_on_surface(k, &sheet, e, self.objects.target, f, tol)?
                    {
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
                let skip = if k.fixes.fillet_overflow_crease_along_edge {
                    held.contains(&f)
                } else {
                    supports.contains(&f)
                };
                if bx.are_disjoint(&sheet_box, on_tol) || skip {
                    continue;
                }
                // fitted to the body's own accuracy, so that creases meeting at a corner of
                // the sheet (on two faces past a cliff) meet there within it; exact
                // sections stay exact
                // Not upstream (#10876): the Roll-over's torus against a cliff face parallel
                // to its axis, its creases as certified graphs over the face's profile
                let certified = if self.executive.roll_over_pass {
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
                    None => creases(k, &mut sheet, s, self.objects.target, f, body_tol)?,
                };
                // each crease where it lies inside both faces
                let mut in_face = Vec::new();
                for c in &found {
                    pieces_in_face(k, c, self.objects.target, f, tol, &mut in_face)?;
                }
                // Not the fork's (WR-717, #2385): a crease that runs along an edge of the
                // face, within tolerance, is that edge. The sheet is split along it as along
                // any crease, and the edge stands as its imprint (below).
                if k.fixes.fillet_overflow_crease_along_edge {
                    let first = in_face.len();
                    for c in &found {
                        pieces_along_edges(k, c, self.objects.target, f, tol, &mut in_face)?;
                    }
                    for c in in_face.get(first..).unwrap_or_default() {
                        edge_creases.push(c.copy(k)?);
                        face_creases.push((f, c.copy(k)?));
                    }
                }
                let before = pieces.len();
                for c in &in_face {
                    pieces_in_face(k, c, &mut sheet, s, tol, &mut pieces)?;
                }
                if pieces.len() > before {
                    // A face past a cliff meets the face the rail leaves (one the blend
                    // rolls on) at a sharp edge. One that runs on from it tangentially (a
                    // round already on the face) is no cliff: the rail would roll over
                    // onto it, and the ball does not fit between.
                    for e in face::p4::edges(self.objects.target, f, None, false, None)? {
                        for g in edge::faces(self.objects.target, e, None)? {
                            if g != f
                                && supports.contains(&g)
                                && rolled_on.contains(&g)
                                && is_tangent_edge(k, self.objects.target, e, tangency)?
                            {
                                // the fork's NoteFitOnFace(g) (49f393d, WR-696)
                                let face =
                                    FilletObject::Topology(FilletBody::Target, EntityRef::Face(g));
                                self.executive
                                    .note_fit_on_face(k, &mut self.objects, face)?;
                                return refuse(
                                    note,
                                    refusal(
                                        FilletErrorType::NoRoom,
                                        Some(NoRoomReason::CreaseAcrossTangentEdge),
                                        "Fillet Error: a rail leaves the face it rolls on across a tangent edge, onto a face it would cut (the blend is larger than the face allows)",
                                    ),
                                );
                            }
                        }
                    }
                    crossed += 1;
                    only = Some(
                        face::p2::surface(self.objects.target, f)?
                            .ok_or_else(|| SolidError::blend_missing(OP, "the face's surface"))?
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
                    return refuse(
                        note,
                        refusal(
                            FilletErrorType::BadMerge,
                            None,
                            "Fillet Error: a blend past its faces lies on the body's boundary away from its creases",
                        ),
                    );
                }
                let mut piece_adds = any_adds;
                if let Some(&i) = round_of.get(&s) {
                    piece_adds = adds.get(i).copied().unwrap_or(false);
                } else if any_adds && any_takes {
                    return refuse(
                        note,
                        refusal(
                            FilletErrorType::CornerUnsupported,
                            None,
                            "Fillet Error: a corner's face between blends that add and blends that take off material, past their faces or at an open setback, is not built",
                        ),
                    );
                }
                if if piece_adds { inside } else { !inside } {
                    with_kernel_callbacks(k, &mut sheet, |k, b, env| delete(k, b, env, p))?;
                } else if let Some(&i) = round_of.get(&s) {
                    kept_rounds.insert(i);
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
                return refuse(
                    note,
                    refusal(FilletErrorType::NoRoom, Some(reason), comment),
                );
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
        let mut extensions: Vec<Extension> = Vec::new();
        let mut along: Vec<Curve> = Vec::new();
        for &f in &target_faces {
            let mut rails = Vec::new();
            for &e in &lamina_edges {
                if edge_on_surface(k, &sheet, e, self.objects.target, f, on_tol)? {
                    let start = edge::start_vertex(&sheet, e)?
                        .ok_or_else(|| SolidError::blend_missing(OP, "the edge's start vertex"))?;
                    let end = edge::end_vertex(&sheet, e)?
                        .ok_or_else(|| SolidError::blend_missing(OP, "the edge's end vertex"))?;
                    rails.push(Rail {
                        curve: edge_curve(k, &sheet, e)?,
                        start,
                        end,
                        start_point: vertex::point(&sheet, start)?,
                        end_point: vertex::point(&sheet, end)?,
                    });
                }
            }
            if rails.is_empty() {
                continue;
            }
            let outcome = extend(k, self.objects.target, f, &rails, blended_edges, tol)?;
            match outcome {
                ExtendOutcome::Refused(why) => {
                    // the fork's NoteFitOnFace(f) (49f393d, WR-696)
                    let face = FilletObject::Topology(FilletBody::Target, EntityRef::Face(f));
                    self.executive
                        .note_fit_on_face(k, &mut self.objects, face)?;
                    return refuse(
                        note,
                        refusal(
                            FilletErrorType::NoRoom,
                            Some(NoRoomReason::RailRunsPastFaceIntoMaterial),
                            why,
                        ),
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
            let (_, on_edges) =
                self.with_target_env(k, |k, b, env, _| merge_curves(k, b, env, f, tol, on, None))?;
            imprinted.extend(on_edges);
        }
        // WR-706: the pieces of the body's edges a section's side lies along, cut off at
        // the rails' ends by the imprint
        for &(a, b) in &seam_sides {
            let target = &*self.objects.target;
            let mut found = false;
            for e in crate::topology::brep::p1::edges(target, None)? {
                let (sv, ev) = (
                    edge::start_vertex(target, e)?
                        .ok_or_else(|| SolidError::blend_missing(OP, "the edge's start vertex"))?,
                    edge::end_vertex(target, e)?
                        .ok_or_else(|| SolidError::blend_missing(OP, "the edge's end vertex"))?,
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
                    refusal(
                        FilletErrorType::BadMerge,
                        None,
                        "Fillet Error: a round's section end lies along an edge of the body that its rail does not cut",
                    ),
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
                    .ok_or_else(|| SolidError::blend_missing(OP, "the edge's start vertex"))?,
                edge::end_vertex(target, e)?
                    .ok_or_else(|| SolidError::blend_missing(OP, "the edge's end vertex"))?,
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
            let curve = edge::curve(target, e)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
            let curve = edge::curve(&sheet, e)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
                        .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
            let curve = edge::curve(target, e)?
                .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
                refusal(
                    FilletErrorType::BadMerge,
                    None,
                    "Fillet Error: a blend past its faces takes off nothing round its blended edges",
                ),
            );
        }
        if gone.len() >= crate::topology::brep::p1::num_faces(target) {
            return refuse(
                note,
                refusal(
                    FilletErrorType::BadMerge,
                    None,
                    "Fillet Error: a blend past its faces reaches every face of the body (its creases and rails enclose no part of it)",
                ),
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
                    refusal(
                        FilletErrorType::BadMerge,
                        None,
                        "Fillet Error: a body edge moved onto a blend's crease leaves geometry the trim does not take off",
                    ),
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
                            edge::start_vertex(&sheet, e)?.ok_or_else(|| {
                                SolidError::blend_missing(OP, "the edge's start vertex")
                            })?,
                            edge::end_vertex(&sheet, e)?.ok_or_else(|| {
                                SolidError::blend_missing(OP, "the edge's end vertex")
                            })?,
                        );
                        if !edge::is_lamina(&sheet, e)?
                            || vertex::point(&sheet, sv)?.distance(p) <= 10.0 * tol
                            || vertex::point(&sheet, ev)?.distance(p) <= 10.0 * tol
                        {
                            continue;
                        }
                        let curve = edge::curve(&sheet, e)?
                            .ok_or_else(|| SolidError::blend_missing(OP, "the edge's curve"))?;
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
                crate::topology::brep::p4::stitch_faces_with_context(
                    k,
                    b,
                    sew_tol,
                    Some(&listed),
                    false,
                    Some(identity),
                    Some(&mut edits),
                )?
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
                return refuse(
                    note,
                    refusal(
                        FilletErrorType::BadMerge,
                        None,
                        "Fillet Error: a blend past its faces does not sew into the body along its creases and rails",
                    ),
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
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as NlCurve;
    use crate::nurbs::math::{CPoint, NOW};

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
}
