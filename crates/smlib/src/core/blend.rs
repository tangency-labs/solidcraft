//! Fillets and chamfers on solidcore: the C API's composition round SMLib's fillet executive,
//! ported from the fork's `capi/src/smlib_ops.cpp` (`blend_call`, `round_edges`,
//! `chamfer_offsets`, `chamfer_edges`, `chamfer_radius`, `outer_faceuse`, `face_frame`,
//! `sides_offsets`, `blend_failure`, `setbacks_fit`, `overflow_first`, `Picked`,
//! `size_ok`, `points_ok`, `tolerance_ok`, `law_ok`, `law_pinches`, `law_along`, `finish`,
//! `smlib_fillet`, `smlib_chamfer`, `smlib_blend`, `smlib_blend_dropping`, `smlib_blend_setback`,
//! `smlib_blend_setback_open`, `smlib_fillet_variable`, `smlib_chamfer_sides`) and
//! `capi/src/smlib_pick.h` (`pick`, `pick_edges`, `pick_vertices`).
//!
//! As there, every operation works on a copy of its body (`SmApiBrepCopy`), picks edges and
//! corners by point, and drives the executive step by step (`SetFilletParameters`,
//! `CreateFilletCorners`, the corners' bevels and setbacks, `DoFilleting`, `StitchAndOrient`).
//!
//! What the capi asks of fork-only SMLib code that solidcore does not have is
//! [`Error::Unsupported`], refused where the capi would first reach it, so every error before
//! it is the C++ path's:
//! - a variable radius whose law has a corner (`SplitFilletFacesAtRadiusCorners`, fork 6eecf04; solidcore#430, PR #1403).
//!
//! solidcore's own refusals of code it has not ported (`SolidError::Unported`) are
//! [`Error::Unsupported`] too.

use std::collections::{HashMap, HashSet};
use std::sync::PoisonError;

use solidcore::EntityRef;
use solidcore::SolidError;
use solidcore::blend::fillet_executive::p1::{FilletLaw, FilletLawKind};
use solidcore::blend::fillet_executive::p3::NoRoomReason;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::{FilletCornerType, FilletErrorType};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{Attribute, AttributeHandlers, Behavior, ids as attribute_ids};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::edge_length;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::query::brep::evaluate_surface_normal;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p2 as brep_p2;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::{DeleteFaceOptions, delete_face};
use solidcore::topology::brep::p5::{StitchAndOrientOptions, stitch_and_orient};
use solidcore::topology::edgeuse::p1 as eu_p1;
use solidcore::topology::edgeuse::p3::{BinormalOptions, evaluate_binormal};
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::face::p4 as face_p4;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, VertexId, edge, vertex};

use super::{Body, answer, run};
use crate::{Blend, ChamferSecond, Error, Hole, HoleEdge, MAX_POINTS, Result};
use solidcore::topology::brep::p3::brep_copy;

/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

/// `SM_ERR_INVALID_INPUT`.
const INVALID_INPUT: i32 = 1007;

/// An SMLib status as the capi passes it on (`status`, smlib_shim.h): solidcore's upstream
/// status codes as they are; its refusals of code it has not ported as not supported.
pub(super) fn status(e: SolidError) -> Error {
    match e {
        SolidError::Upstream { code, .. } => Error::Status(code),
        SolidError::InvalidInput { .. } => Error::Status(INVALID_INPUT),
        e if e.is_unported() => Error::Unsupported,
        e => Error::Solidcore(e.to_string()),
    }
}

/// `size_ok`.
fn size_ok(x: f64) -> bool {
    x.is_finite() && x > 0.0 && x <= 1e6
}

/// `tolerance_ok`.
pub(super) fn tolerance_ok(t: f64) -> bool {
    t.is_finite() && t > 0.0 && t < 1e6
}

/// `points_ok`.
pub(super) fn points_ok(points: &[[f64; 3]], allow_none: bool) -> bool {
    points.len() <= MAX_POINTS && (allow_none || !points.is_empty()) && points.iter().flatten().all(|x| x.is_finite())
}

pub(super) fn p3(p: [f64; 3]) -> Point3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Why a pick failed: `SMLIB_ERR_PICK_NONE` or `SMLIB_ERR_PICK_AMBIGUOUS`, and the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Miss {
    None(usize),
    Ambiguous(usize),
}

/// `smlib_pick::pick`: the entity nearest each point, each once, with the index of the point that
/// first picked it. A point picks none when the nearest is farther than `tol`, and is ambiguous
/// when the second nearest is within `tol` too and nearer than the first by at most `model_tol`.
/// An entity whose distance query returns a status other than success is skipped, as the capi
/// skips it ([`answer`]); any other solidcore error fails the pick.
pub(super) fn pick<T: Copy + PartialEq>(
    all: &[T],
    points: &[[f64; 3]],
    tol: f64,
    model_tol: f64,
    mut dist: impl FnMut(T, Point3) -> solidcore::Result<Option<f64>>,
) -> solidcore::Result<std::result::Result<(Vec<T>, Vec<usize>), Miss>> {
    let (mut picked, mut first) = (Vec::new(), Vec::new());
    for (i, p) in points.iter().enumerate() {
        let p = p3(*p);
        let (mut best, mut d0, mut d1) = (None, f64::INFINITY, f64::INFINITY);
        for &x in all {
            let Some(d) = dist(x, p)? else { continue };
            if !d.is_finite() {
                continue;
            }
            if d < d0 {
                d1 = d0;
                d0 = d;
                best = Some(x);
            } else if d < d1 {
                d1 = d;
            }
        }
        let Some(best) = best.filter(|_| d0 <= tol) else {
            return Ok(Err(Miss::None(i)));
        };
        if d1 <= tol && d1 - d0 <= model_tol {
            return Ok(Err(Miss::Ambiguous(i)));
        }
        if !picked.contains(&best) {
            picked.push(best);
            first.push(i);
        }
    }
    Ok(Ok((picked, first)))
}

/// `pick_edges` (`edge_distance`: `SmApiEdgeClosestPoint`).
pub(super) fn pick_edges(
    k: &Kernel,
    b: &Brep,
    points: &[[f64; 3]],
    tol: f64,
) -> solidcore::Result<std::result::Result<(Vec<EdgeId>, Vec<usize>), Miss>> {
    let all = brep_p1::edges(b, None)?;
    pick(&all, points, tol, b.tolerance(), |e, p| Ok(answer(edge_closest_point(k, b, e, p))?.map(|c| c.closest.distance)))
}

/// `pick_vertices` (`vertex_distance`).
fn pick_vertices(b: &Brep, points: &[[f64; 3]], tol: f64) -> solidcore::Result<std::result::Result<(Vec<VertexId>, Vec<usize>), Miss>> {
    let all = brep_p1::vertices(b, None)?;
    pick(&all, points, tol, b.tolerance(), |v, p| Ok(Some((b.vertex(v)?.point - p).length())))
}

/// An edge pick's failure as `pick_error` (ops.rs) reads it.
pub(super) fn edge_miss(m: Miss) -> Error {
    match m {
        Miss::None(point) => Error::NoMatch { point },
        Miss::Ambiguous(point) => Error::Ambiguous { point },
    }
}

/// A corner pick's failure as `setback_error` (ops.rs) reads it: the capi writes the corner
/// point's index to `out_failed_corner` only, so the status comes back bare.
fn corner_miss(m: Miss) -> Error {
    match m {
        Miss::None(_) => Error::Status(crate::ffi::ops::ERR_PICK_NONE),
        Miss::Ambiguous(_) => Error::Status(crate::ffi::ops::ERR_PICK_AMBIGUOUS),
    }
}

/// A corner set back: its vertex and the setback along each of its edges (the capi's
/// `SetBack`; the entry points take no ease, which is 0).
struct SetBack {
    vertex: VertexId,
    distance: f64,
}

/// `setbacks_fit`: each setback shorter than every edge at its corner, and where an edge's
/// other end is a setback corner too, the two together shorter than the edge.
fn setbacks_fit(k: &Kernel, b: &Brep, setbacks: &[SetBack]) -> solidcore::Result<bool> {
    for sb in setbacks {
        for e in vertex::edges(b, sb.vertex, None)? {
            let Some(length) = answer(edge_length(k, b, e, 1e-6))? else {
                return Ok(false);
            };
            let far = edge::other_vertex(b, e, Some(sb.vertex))?;
            let mut other = 0.0;
            for o in setbacks {
                if Some(o.vertex) == far && far != Some(sb.vertex) {
                    other = o.distance;
                }
            }
            if sb.distance + other >= length {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The midpoint parameter of `e` (`GetInterval().Evaluate(0.5)`).
fn mid(b: &Brep, e: EdgeId) -> solidcore::Result<f64> {
    Ok(b.edge(e)?.interval.evaluate(0.5))
}

/// `chamfer_radius`: the radius of the ball whose tangent points on the two faces of `e` lie
/// `distance` back from the edge at its midpoint, `distance * tan(a / 2)` with `a` the angle
/// between the directions into the faces; at a cusp (a knife edge) `distance` itself, the ball
/// whose contacts the chamfer joins. `None` where the capi's returns false.
fn chamfer_radius(k: &Kernel, b: &mut Brep, e: EdgeId, distance: f64) -> solidcore::Result<Option<f64>> {
    let Some(u0) = edge::blend_edgeuse(k, b, e)? else { return Ok(None) };
    let Some(u1) = eu_p1::radial(b, u0)? else { return Ok(None) };
    let t = mid(b, e)?;
    let options = BinormalOptions::default();
    let Some(e0) = answer(evaluate_binormal(k, b, u0, t, false, &options))? else { return Ok(None) };
    let Some(e1) = answer(evaluate_binormal(k, b, u1, t, false, &options))? else { return Ok(None) };
    // Unitize's failure (a zero vector) is the capi's `return false`.
    let (Ok(n0), Ok(b0), Ok(b1)) = (e0.face_normal.unitize(), e0.binormal.unitize(), e1.binormal.unitize()) else { return Ok(None) };
    let c = b0.dot(b1).clamp(-1.0, 1.0);
    // At a cusp (a knife edge: the faces tangent within the half degree `GetBlendEdgeuse` calls
    // tangent, folded back on each other, the binormals the same way) a is 0 and no ball lies
    // `distance` back along both: the chamfer is the chord between the contacts of the ball of
    // radius `distance` (fork e96898d).
    let cusp = c > 0.0 && b1.dot(n0).abs() < (0.5 * std::f64::consts::PI / 180.0).sin();
    let radius = if cusp { distance } else { distance * ((1.0 - c) / (1.0 + c)).sqrt() };
    Ok((radius.is_finite() && radius > 0.0).then_some(radius))
}

/// What `round_edges` makes: the capi's kinds less the chamfer, which `chamfer_offsets` makes.
#[derive(Clone, Copy, PartialEq)]
enum Section<'a> {
    Round,
    Chord,
    /// A chamfer, with each edge's offset of its second surface (`chamfer_offsets`' `second`;
    /// the sizes are the first's).
    Chamfer(&'a [f64]),
}

/// The faces of `b`, for `made_faces`: a face whose id is new after the operation was made by
/// it. (The capi marks faces with an attribute SMLib copies through every split; here a face
/// split by the operation counts as made, which only matters to a blend that made no blend
/// face but split one, never seen.)
fn face_set(b: &Brep) -> solidcore::Result<HashSet<FaceId>> {
    Ok(brep_p1::faces(b, None)?.into_iter().collect())
}

/// `blend_failure`: a blend the executive found larger than its faces allow
/// (`SM_FILERR_NO_ROOM`) is [`Error::NoRoom`]; a filleted vertex no corner type handles
/// (`SM_FILERR_CORNER_UNSUPPORTED`, which solidcore notes under
/// `fixes.fillet_vertex_error_diagnostics`) is [`Error::CornerUnsupported`]; any other failure
/// its status. `corner` says `CreateFilletCorners` noted such a vertex: the corner it could not
/// make is why a later step fails, so that note stands for it whatever the later step noted
/// (as solidcore's own `FilletSession::do_fillet` keeps it).
///
/// Except a blend whose rail leaves its face refused as [`NoRoomReason::RailLeavesFace`]: that
/// is solidcore without `fixes.fillet_cliff_overflow`, refusing every such blend where the fork
/// decides between building it over the cliff (`SmFilletCliffOverflow.cpp`, fork 8ba4667) and
/// refusing it. Its answer is the fork's only by chance, so it is [`Error::Unsupported`]. With
/// the cliff overflow on, solidcore makes the fork's choice, and its refusals (each with its own
/// reason) are [`Error::NoRoom`] like the fork's.
fn blend_failure(s: &FilletSession<'_>, e: SolidError, corner: bool) -> Error {
    let info = s.error_info();
    if e.is_unported() {
        status(e)
    } else if corner || info.code == FilletErrorType::CornerUnsupported as i32 {
        Error::CornerUnsupported
    } else if info.code != FilletErrorType::NoRoom as i32 {
        status(e)
    } else if info.no_room_reason() == Some(NoRoomReason::RailLeavesFace) {
        Error::Unsupported
    } else {
        Error::NoRoom
    }
}

/// `round_edges`' `overflow_first` argument: with it the session stops, nothing built, when blends
/// of several edges overflow their faces across cliffs (`SetOverflowFirst`) or (`classify_only`)
/// always (`SetClassifyOnly`), and `stopped` gets the edges whose blends overflow
/// (`GetOverflowEdges`; the capi's status `OVERFLOW_FIRST`, the call's failure then meaning only that)
/// and `mitred` the edges of each two rounds mitred at a face's concave corner, two by two
/// (`GetMitredEdges`), when the caller asks.
struct OverflowFirst<'a> {
    classify_only: bool,
    stopped: &'a mut Option<Vec<EdgeId>>,
    mitred: Option<&'a mut Vec<EdgeId>>,
}

/// `round_edges` and `chamfer_offsets` (with equal offsets): `edges` of `b` blended in place,
/// edge i at `sizes[i]`, the executive driven as `SmApiFilletEdgesPerEdge` drives it with the
/// corners' setbacks (`SmFilletCorner::SetSetBackDist`) set between making the corners and
/// filleting, or (a chamfer) every corner bevelled; then `StitchAndOrient`. A blend that made
/// no face is [`Error::NoEffect`]. A setback corner that is not SMLib's same-convexity N x N
/// corner is refused with its index in `setbacks` (`Err(Ok(j))`).
///
/// With `patches`, the session marks the set-back corners' patches (`SetMarkCornerPatches`, fork
/// 60af574), and after the stitch `patches` gets each corner's, in `setbacks`' order: the faces
/// marked with its index, in the body's face order. A corner whose patch has no face is
/// [`Error::Unsupported`].
fn blend_edges(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    sizes: &[f64],
    section: Section<'_>,
    law: Option<FilletLaw>,
    setbacks: &[SetBack],
    patches: Option<&mut Vec<Vec<FaceId>>>,
) -> solidcore::Result<std::result::Result<(), std::result::Result<usize, Error>>> {
    blend_edges_stopping(k, b, edges, sizes, section, law, setbacks, patches, None)
}

/// [`blend_edges`], with `round_edges`' `overflow_first` argument.
#[allow(clippy::too_many_arguments)] // the capi's round_edges
fn blend_edges_stopping(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    sizes: &[f64],
    section: Section<'_>,
    law: Option<FilletLaw>,
    setbacks: &[SetBack],
    patches: Option<&mut Vec<Vec<FaceId>>>,
    overflow_first: Option<OverflowFirst<'_>>,
) -> solidcore::Result<std::result::Result<(), std::result::Result<usize, Error>>> {
    let before = face_set(b)?;
    let done = {
        let mut s = FilletSession::new(k, b)?;
        // `SetFilletParameters(b, es, sections, solvers, values, laws, -1, -1, -1, -1,
        // continuity 1, thumbweight 1)`: the session sets the executive's global merge first, as
        // `SmApiFilletEdges` does, and `SetFilletParameters` resets it, as in the capi.
        let prepared = match (section, law) {
            (Section::Round, None) => s.prepare_per_edge_constant_radius(k, edges, sizes, &[FilletSurfaceGeneratorType::Circular], 1, 1.0),
            // The size is the start radius (`smlib_fillet_variable`).
            (Section::Round, Some(law)) => match sizes.first() {
                Some(&size) => s.prepare_variable_radius_edges(k, edges, size, law, 1, 1),
                None => return Ok(Err(Err(Error::Argument))),
            },
            (Section::Chord, _) => s.prepare_per_edge_constant_distance(k, edges, sizes, &[FilletSurfaceGeneratorType::Circular], 1, 1.0),
            (Section::Chamfer(second), _) => chamfer_parameters(k, &mut s, edges, sizes, second),
        };
        let mut step = prepared.and_then(|()| s.create_corners(k, None)).map(|()| Ok(Vec::new()));
        // A vertex `CreateFilletCorners` made no corner for, noted before a later step notes its
        // own failure over it ([`blend_failure`]).
        let corner = step.is_ok() && s.error_info().code == FilletErrorType::CornerUnsupported as i32;
        s.set_overflow_first(overflow_first.is_some());
        s.set_classify_only(overflow_first.as_ref().is_some_and(|o| o.classify_only));
        if let Ok(Ok(_)) = step {
            step = corners(&mut s, section, setbacks);
        }
        s.set_mark_corner_patches(patches.is_some());
        match step {
            // The marks go with the session: read them before it drops.
            Ok(Ok(set_back)) => match s.do_filleting(k, None) {
                Ok(()) => s.corner_patch_faces().map(|marked| Ok((set_back, marked))),
                Err(e) => {
                    if let Some(o) = overflow_first
                        && s.error_info().code == FilletErrorType::OverflowFirst as i32
                    {
                        *o.stopped = Some(s.overflow_edges());
                        if let Some(m) = o.mitred {
                            *m = s.mitred_edges();
                        }
                    }
                    Ok(Err(Err(blend_failure(&s, e, corner))))
                }
            },
            Ok(Err(refusal)) => Ok(Err(refusal)),
            Err(e) => Ok(Err(Err(blend_failure(&s, e, corner)))),
        }
    };
    let done: solidcore::Result<std::result::Result<(Vec<usize>, Vec<(usize, FaceId)>), std::result::Result<usize, Error>>> = done;
    let (set_back, marked) = match done? {
        Ok(d) => d,
        Err(refusal) => {
            b.refresh_cached_topology();
            return Ok(Err(refusal));
        }
    };
    let stitched = stitch_and_orient(k, b, &StitchAndOrientOptions::default()).map_err(status);
    // The port's cached topology, true again for what reads the body next (as solidcore's own
    // fillet entry points do).
    b.refresh_cached_topology();
    if let Err(e) = stitched {
        return Ok(Err(Err(e)));
    }
    let made = face_set(b)?.difference(&before).count();
    if made == 0 {
        return Ok(Err(Err(Error::NoEffect)));
    }
    if let Some(patches) = patches {
        // The stitch keeps the faces filleting made: a corner's patch is the faces marked with
        // its index.
        let live = brep_p1::faces(b, None)?;
        let marked: HashMap<FaceId, usize> = marked.into_iter().map(|(i, f)| (f, i)).collect();
        for &index in &set_back {
            let patch: Vec<FaceId> = live.iter().copied().filter(|f| marked.get(f) == Some(&index)).collect();
            if patch.is_empty() {
                return Ok(Err(Err(Error::Unsupported)));
            }
            patches.push(patch);
        }
        // `RemoveAttributeFromTopology(SM_AI_FILLET_CORNER_FACE)`: the marks are the session's,
        // and none is left on the body's own faces.
        for f in live {
            let at = EntityRef::Face(f);
            let marks: Vec<_> =
                b.entity_attributes(at).into_iter().filter(|(_, a)| a.attribute_id() == attribute_ids::FILLET_CORNER_FACE).map(|(h, _)| h).collect();
            for h in marks {
                b.remove_attribute(at, h)?;
            }
        }
    }
    Ok(Ok(()))
}

/// `chamfer_offsets`' `SetFilletParameters(b, es, kinds, solvers, first, NULL, -1, -1, -1, -1,
/// 1, 1.0, &second)` with every kind `SM_FSG_LINEAR` and every solver `SM_FS_CONST_RADIUS`, as the
/// fork's `SetFilletParameters` runs it (its `pOptSecondRadii`, fork f653ac9).
fn chamfer_parameters(k: &mut Kernel, s: &mut FilletSession<'_>, edges: &[EdgeId], first: &[f64], second: &[f64]) -> solidcore::Result<()> {
    if edges.is_empty() {
        return Ok(());
    }
    s.prepare_per_edge_constant_radius_with_second(k, edges, first, second, &[FilletSurfaceGeneratorType::Linear], 1, 1.0)
}

/// The corners' part of `round_edges` / `chamfer_offsets`: a chamfer's corners bevelled; each
/// setback set on its vertex's corner (`GetFilletCornerOfVertex`), which must be an
/// `SM_FCR_N_x_N_CONVEX` corner. Each set-back corner's index in the session's corners, in
/// `setbacks`' order.
fn corners(
    s: &mut FilletSession<'_>,
    section: Section<'_>,
    setbacks: &[SetBack],
) -> solidcore::Result<std::result::Result<Vec<usize>, std::result::Result<usize, Error>>> {
    let ids = s.fillet_corners()?;
    if matches!(section, Section::Chamfer(_)) {
        for id in ids {
            s.set_corner_bevel(id, true)?;
        }
        return Ok(Ok(Vec::new()));
    }
    let mut set_back = Vec::with_capacity(setbacks.len());
    for (j, sb) in setbacks.iter().enumerate() {
        let mut found = None;
        for (i, &id) in ids.iter().enumerate() {
            if s.corner(id)?.vertex == Some(sb.vertex) {
                found = Some((i, id));
                break;
            }
        }
        let Some((i, id)) = found else {
            return Ok(Err(Err(Error::Argument)));
        };
        if s.corner(id)?.corner_type() != FilletCornerType::NByNConvex {
            return Ok(Err(Ok(j)));
        }
        s.set_corner_setback(id, sb.distance)?;
        set_back.push(i);
    }
    Ok(Ok(set_back))
}

/// `finish`: the result must have faces, and a solid input must give a closed manifold solid.
pub(super) fn finish(b: Brep, solid_in: bool) -> solidcore::Result<Out<Brep>> {
    if b.face_list().is_empty() {
        return Ok(Err(Error::Empty));
    }
    if solid_in && !brep_p2::is_manifold_solid(&b)? {
        return Ok(Err(Error::NotSolid));
    }
    Ok(Ok(b))
}

/// Runs `f` on a copy of `body` (`brep_copy`) under the kernel's lock.
pub(super) fn on_copy(call: &'static str, body: &Body, f: impl FnOnce(&mut Kernel, Brep) -> solidcore::Result<Out<Brep>>) -> Result<Body> {
    if body.0.is_poisoned() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    run(call, |k| {
        let g = body.0.lock().unwrap_or_else(PoisonError::into_inner);
        super::dump::input(k, "brep", &g);
        let c = brep_copy(k, &g)?;
        drop(g);
        f(k, c)
    })?
    .map(Body::new)
}

/// What [`blend`] does with the picked corners' patches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Corners {
    /// Filled, as SMLib builds them (`smlib_blend`, `smlib_blend_setback`).
    Filled,
    /// Removed, the holes listed (`smlib_blend_setback_open`).
    Open,
}

/// `Picked::ID`: above SMLib's own ids (`SM_AI_USER_1 + 9`).
const PICKED_ID: u64 = attribute_ids::USER_1 + 9;

/// The capi's `Picked`: which picked edge an edge stands for, put on the picked edges before an
/// overflowing blend goes first, handed on through every split and copy (`SM_AB_COPY`), so
/// afterwards the edges carrying it are what the overflow left of that picked edge (none: it
/// removed the edge whole).
#[derive(Debug, Clone, PartialEq)]
struct Picked(usize);

impl AttributeHandlers for Picked {}

/// The attribute `Picked` on `e`, with its handle.
fn picked_on(b: &Brep, e: EdgeId) -> Option<(solidcore::geom::attribute::AttributeHandle, usize)> {
    let (h, a) = b.entity_attributes(EntityRef::Edge(e)).into_iter().find(|(_, a)| a.attribute_id() == PICKED_ID)?;
    a.application_value::<Picked>().map(|p| (h, p.0))
}

/// Whether the rounds that overflow go first ([`overflow_first`]): on since solidcore#1980, which
/// keeps the attributes of the target edges the subtraction replaces, so a picked edge the overflow
/// only shortened still reads as picked.
const OVERFLOW_GOES_FIRST: bool = true;

/// `overflow_first`: the rounds of [`blend_call`] when blends of several edges overflow their
/// faces (`round_edges` stopped in the copy `w` of `body`, `picked` its picked edges, `each` their
/// sizes). The blends that overflow on their own (each edge's alone, classified in `w`, which
/// nothing was built in) go first: on a fresh copy of `body`, the picked edges tagged with their
/// places ([`Picked`]), those are rounded, and each other picked edge is then what is left of it,
/// the edges carrying its tag, rounded at its size. A picked edge with nothing left is gone: its
/// points are flagged in `dropped`. Two rounds mitred at a face's concave corner (`mitred`, the edges
/// of each such pair two by two, as the stopped call found them in `w`) meet in their mitre, each run
/// on past the corner: one goes first with the other, so a round mitred to one that overflows goes
/// first too (fork 9ba2b12). Where none overflows on its own, or all go first, the edges are rounded
/// together as before. The result is the new body. Unlike the capi, the tags come off it
/// (nothing reads them after the call, and a later call's copy would carry them on).
#[allow(clippy::too_many_arguments)] // the capi's overflow_first
fn overflow_first(
    k: &mut Kernel,
    body: &Body,
    points: &[[f64; 3]],
    tolerance: f64,
    section: Section<'_>,
    picked: &[EdgeId],
    mitred: &[EdgeId],
    each: &[f64],
    w: &mut Brep,
    dropped: &mut [bool],
) -> solidcore::Result<Out<Brep>> {
    /// A failure of `round_edges` on the fresh copy, which has no setback corners.
    fn failed(e: std::result::Result<usize, Error>) -> Error {
        e.err().unwrap_or(Error::Unsupported)
    }
    let mut fresh = {
        let g = body.0.lock().unwrap_or_else(PoisonError::into_inner);
        brep_copy(k, &g)?
    };
    let again = match pick_edges(k, &fresh, points, tolerance)? {
        Ok((again, _)) if again.len() == picked.len() => again,
        Ok(_) => return Ok(Err(Error::NoMatch { point: 0 })),
        Err(m) => return Ok(Err(edge_miss(m))),
    };
    let mut goes_first = vec![false; picked.len()];
    for (i, ((&e, &to), &size)) in picked.iter().zip(&again).zip(each).enumerate() {
        let mut stopped = None;
        let alone = OverflowFirst { classify_only: true, stopped: &mut stopped, mitred: None };
        match blend_edges_stopping(k, w, &[e], &[size], section, None, &[], None, Some(alone))? {
            Err(Err(why)) if stopped.is_none() => return Ok(Err(why)),
            Err(Ok(_)) => return Ok(Err(Error::Unsupported)),
            Ok(()) => return Ok(Err(Error::Status(1001))),
            Err(Err(_)) => {}
        }
        fresh.add_attribute(EntityRef::Edge(to), Attribute::application(PICKED_ID, Behavior::Copy, Picked(i)))?;
        if let Some(flag) = goes_first.get_mut(i) {
            *flag = stopped.is_some_and(|over| !over.is_empty());
        }
    }
    // A round mitred to one that goes first goes with it, through every mitre.
    let place = |e: &EdgeId| picked.iter().position(|p| p == e);
    let pairs: Vec<(usize, usize)> = mitred.as_chunks::<2>().0.iter().filter_map(|[a, b]| Some((place(a)?, place(b)?))).collect();
    let mut grew = true;
    while grew {
        grew = false;
        for &(a, b) in &pairs {
            if goes_first.get(a) != goes_first.get(b) {
                for i in [a, b] {
                    if let Some(flag) = goes_first.get_mut(i) {
                        *flag = true;
                    }
                }
                grew = true;
            }
        }
    }
    let (mut lead, mut lead_sizes, mut rest) = (Vec::new(), Vec::new(), Vec::new());
    for (i, ((&to, &size), &first)) in again.iter().zip(each).zip(&goes_first).enumerate() {
        if first {
            lead.push(to);
            lead_sizes.push(size);
        } else {
            rest.push(i);
        }
    }
    if lead.is_empty() || rest.is_empty() {
        return Ok(match blend_edges(k, &mut fresh, &again, each, section, None, &[], None)? {
            Ok(()) => untagged(fresh),
            Err(e) => Err(failed(e)),
        });
    }
    if let Err(e) = blend_edges(k, &mut fresh, &lead, &lead_sizes, section, None, &[], None)? {
        return Ok(Err(failed(e)));
    }
    let edges = brep_p1::edges(&fresh, None)?;
    let (mut rest_edges, mut rest_sizes) = (Vec::new(), Vec::new());
    for i in rest {
        let mut left = false;
        for &e in &edges {
            if picked_on(&fresh, e).is_some_and(|(_, p)| p == i) {
                rest_edges.push(e);
                rest_sizes.push(each.get(i).copied().unwrap_or(0.0));
                left = true;
            }
        }
        if !left {
            // Every point that picked edge i (in `w`, which nothing was built in).
            for (j, p) in points.iter().enumerate() {
                if let (Ok((one, _)), Some(flag)) = (pick_edges(k, w, std::slice::from_ref(p), tolerance)?, dropped.get_mut(j))
                    && one.as_slice() == picked.get(i..=i).unwrap_or_default()
                {
                    *flag = true;
                }
            }
        }
    }
    if !rest_edges.is_empty()
        && let Err(e) = blend_edges(k, &mut fresh, &rest_edges, &rest_sizes, section, None, &[], None)?
    {
        return Ok(Err(failed(e)));
    }
    Ok(untagged(fresh))
}

/// `b` with [`Picked`] off its edges.
fn untagged(mut b: Brep) -> Out<Brep> {
    let edges = brep_p1::edges(&b, None).map_err(status)?;
    for e in edges {
        if let Some((h, _)) = picked_on(&b, e) {
            b.remove_attribute(EntityRef::Edge(e), h).map_err(status)?;
        }
    }
    Ok(b)
}

/// Why [`Blend::Curvature`] is refused. The capi's curvature section is the Gⁿ generator's order 2
/// at the equal-area thumbweight (`SMLIB_BLEND_THUMBWEIGHT`, fork 6148773), which solidcore does
/// not have yet; the blend-curve quintic it does have takes off another area, so it is not built
/// in its place. The refusal comes off when the solidcore pin carries #2260 and 6148773 and its
/// `fillet_gn_section_generator` flag is on.
pub(crate) const CURVATURE_SECTION: &str = "a curvature-continuous fillet on solidcore: the curvature section's equal-area Gⁿ \
generator is not in solidcore yet (solidcore#2250, port in progress as #2260)";

/// [`blend_holes`] without its holes.
#[allow(clippy::too_many_arguments)] // the capi's blend_call
pub(crate) fn blend(
    body: &Body,
    kind: Blend,
    points: &[[f64; 3]],
    sizes: &[f64],
    tolerance: f64,
    corners: &[[f64; 3]],
    setbacks: &[f64],
    open: Corners,
) -> Result<Body> {
    blend_holes(body, kind, points, sizes, tolerance, corners, setbacks, open).map(|(b, _)| b)
}

/// `smlib_blend_dropping`: [`blend`] with one size per point, no corners, and per point whether it
/// picked an edge that a larger round, overflowing its faces, removed whole.
pub(crate) fn blend_dropping(body: &Body, kind: Blend, points: &[[f64; 3]], sizes: &[f64], tolerance: f64) -> Result<(Body, Vec<bool>)> {
    let mut dropped = vec![false; points.len()];
    let (b, _) = blend_call(body, kind, points, sizes, tolerance, &[], &[], Corners::Filled, &mut dropped)?;
    Ok((b, dropped))
}

/// `blend_call` (as `smlib_fillet`, `smlib_chamfer`, `smlib_blend`, `smlib_blend_setback` and
/// `smlib_blend_setback_open` call it): `sizes` one per point, or one for all; `corners` and
/// `setbacks` the corner setbacks. With [`Corners::Open`], the holes, one per point of
/// `corners` (none otherwise).
#[allow(clippy::too_many_arguments)] // the capi's blend_call
pub(crate) fn blend_holes(
    body: &Body,
    kind: Blend,
    points: &[[f64; 3]],
    sizes: &[f64],
    tolerance: f64,
    corners: &[[f64; 3]],
    setbacks: &[f64],
    open: Corners,
) -> Result<(Body, Vec<Hole>)> {
    blend_call(body, kind, points, sizes, tolerance, corners, setbacks, open, &mut vec![false; points.len()])
}

/// `blend_call`: [`blend_holes`], with per point in `dropped` whether it picked an edge an
/// overflowing round removed whole ([`overflow_first`]).
#[allow(clippy::too_many_arguments)] // the capi's blend_call
fn blend_call(
    body: &Body,
    kind: Blend,
    points: &[[f64; 3]],
    sizes: &[f64],
    tolerance: f64,
    corners: &[[f64; 3]],
    setbacks: &[f64],
    open: Corners,
    dropped: &mut [bool],
) -> Result<(Body, Vec<Hole>)> {
    let one_size = sizes.len() == 1;
    if !points_ok(points, false)
        || !tolerance_ok(tolerance)
        || !(one_size || sizes.len() == points.len())
        || !points_ok(corners, true)
        || setbacks.len() != corners.len()
    {
        return Err(Error::Argument);
    }
    if !sizes.iter().all(|&s| size_ok(s)) || !setbacks.iter().all(|&s| size_ok(s)) {
        return Err(Error::Argument);
    }
    // A chamfer's corners are bevelled, which builds no setback.
    if !corners.is_empty() && kind == Blend::Chamfer {
        return Err(Error::Unsupported);
    }
    let mut holes = Vec::new();
    let made = on_copy("blend_call", body, |k, mut w| {
        let (picked, first) = match pick_edges(k, &w, points, tolerance)? {
            Ok(p) => p,
            Err(m) => return Ok(Err(edge_miss(m))),
        };
        let each: Vec<f64> = first.iter().map(|&i| sizes.get(if one_size { 0 } else { i }).copied().unwrap_or(0.0)).collect();
        // Each corner point picks a vertex; a vertex picked twice takes the first point's setback.
        let (at, first_corner) = match pick_vertices(&w, corners, tolerance)? {
            Ok(p) => p,
            Err(m) => return Ok(Err(corner_miss(m))),
        };
        let back: Vec<SetBack> =
            at.iter().zip(&first_corner).map(|(&vertex, &j)| SetBack { vertex, distance: setbacks.get(j).copied().unwrap_or(0.0) }).collect();
        if !setbacks_fit(k, &w, &back)? {
            return Ok(Err(Error::NoRoom));
        }
        let solid_in = brep_p2::is_manifold_solid(&w)?;
        let section = match kind {
            Blend::Round => Section::Round,
            Blend::Curvature => return Ok(Err(Error::Unported(CURVATURE_SECTION))),
            Blend::Chord => Section::Chord,
            // `chamfer_edges`: each edge's offsets from its setback at its midpoint.
            Blend::Chamfer => {
                let mut radii = Vec::with_capacity(picked.len());
                for (&e, &d) in picked.iter().zip(&each) {
                    match chamfer_radius(k, &mut w, e, d)? {
                        Some(r) => radii.push(r),
                        None => return Ok(Err(Error::Status(INVALID_INPUT))),
                    }
                }
                return Ok(match blend_edges(k, &mut w, &picked, &radii, Section::Chamfer(&radii), None, &[], None)? {
                    Ok(()) => finish(w, solid_in)?,
                    Err(Ok(_)) => Err(Error::Unsupported),
                    Err(Err(e)) => Err(e),
                });
            }
        };
        // The corners' places, read now: filleting deletes the vertices.
        let at_points = at.iter().map(|&v| vertex::point(&w, v)).collect::<solidcore::Result<Vec<Point3>>>()?;
        let mut patches = Vec::new();
        // An overflow goes first (rounds without corner setbacks): when some of the rounds
        // overflow their faces across cliffs and others fit, the overflowing ones are built
        // first, on a fresh copy, and the others then round what is left of their edges. An edge
        // the overflow removes whole is gone (`dropped`); one it cuts keeps the rest.
        let may_go_first = OVERFLOW_GOES_FIRST && back.is_empty() && open == Corners::Filled && picked.len() > 1;
        let (mut stopped, mut mitred) = (None, Vec::new());
        let first = may_go_first.then_some(OverflowFirst { classify_only: false, stopped: &mut stopped, mitred: Some(&mut mitred) });
        let done = blend_edges_stopping(k, &mut w, &picked, &each, section, None, &back, (open == Corners::Open).then_some(&mut patches), first)?;
        if stopped.is_some() {
            return Ok(match overflow_first(k, body, points, tolerance, section, &picked, &mitred, &each, &mut w, dropped)? {
                Ok(fresh) => finish(fresh, solid_in)?,
                Err(e) => Err(e),
            });
        }
        match done {
            Ok(()) => {}
            // A corner SMLib does not set back: the first of the caller's points that picked it.
            Err(Ok(j)) => return Ok(Err(first_corner.get(j).map_or(Error::Unsupported, |&corner| Error::CornerRefused { corner }))),
            Err(Err(e)) => return Ok(Err(e)),
        }
        if open == Corners::Filled {
            return finish(w, solid_in);
        }
        // Each corner opened: its patch removed, the edges round its hole in the result's
        // orders, once per corner point (a vertex picked twice gives its hole twice).
        if !brep_p2::is_manifold_solid(&w)? {
            return Ok(Err(Error::NotSolid));
        }
        let rims = match open_corners(k, &mut w, &patches)? {
            Ok(r) => r,
            Err(e) => return Ok(Err(e)),
        };
        let edge_index: HashMap<EdgeId, usize> = brep_p1::edges(&w, None)?.into_iter().enumerate().map(|(i, e)| (e, i)).collect();
        let face_index: HashMap<FaceId, usize> = brep_p1::faces(&w, None)?.into_iter().enumerate().map(|(i, f)| (f, i)).collect();
        for c in corners {
            let p = Point3::new(c[0], c[1], c[2]);
            let mut which = None;
            let mut best = f64::INFINITY;
            for (j, q) in at_points.iter().enumerate() {
                let d = (*q - p).length();
                if d < best {
                    best = d;
                    which = Some(j);
                }
            }
            let Some(rim) = which.and_then(|j| rims.get(j)) else { return Ok(Err(Error::Argument)) };
            let mut edges = Vec::with_capacity(rim.len());
            for h in rim {
                let (Some(&edge), Some(&face)) = (edge_index.get(&h.edge), face_index.get(&h.face)) else { return Ok(Err(Error::Unsupported)) };
                edges.push(HoleEdge { edge, face, forward: h.forward });
            }
            holes.push(Hole { edges });
        }
        finish(w, false)
    })?;
    Ok((made, holes))
}

/// One edge round an opened corner's hole (`HoleEdge`): the face beside it, and whether that
/// face's boundary, run counter-clockwise about its outward normal (the solid's), runs the edge
/// from its start vertex to its end.
#[derive(Clone, Copy)]
struct RimEdge {
    edge: EdgeId,
    face: FaceId,
    forward: bool,
}

/// `open_corners`: the faces of each patch in `patches` removed from the solid `b`, with the edges
/// and vertices only they used, and the edges round each patch's hole, as the faces beside it run
/// them, each edge's start the previous one's end. Read on the solid before anything is removed,
/// where each face's outward side is known. Each edge round a hole must lie between a face of its
/// patch and one other face, outside every patch, and the edges must close one loop (a patch that
/// is a disc), else [`Error::Unsupported`].
fn open_corners(k: &mut Kernel, b: &mut Brep, patches: &[Vec<FaceId>]) -> solidcore::Result<Out<Vec<Vec<RimEdge>>>> {
    let any: HashSet<FaceId> = patches.iter().flatten().copied().collect();
    let mut holes = Vec::with_capacity(patches.len());
    for patch in patches {
        let mine: HashSet<FaceId> = patch.iter().copied().collect();
        let mut seen = HashSet::new();
        let mut rim: Vec<RimEdge> = Vec::new();
        for &f in patch {
            for e in face_p4::edges(b, f, None, false, None)? {
                if !seen.insert(e) {
                    continue;
                }
                let beside = edge::faces(b, e, None)?;
                let mut outside = beside.iter().copied().filter(|g| !mine.contains(g));
                let (Some(other), more) = (outside.next(), outside.next()) else { continue };
                if more.is_some() || beside.len() != 2 || any.contains(&other) {
                    return Ok(Err(Error::Unsupported));
                }
                let Some(fu) = outer_faceuse(b, other)? else { return Ok(Err(Error::Unsupported)) };
                let mut used = None;
                for u in edge::edgeuses(b, e, None, None)? {
                    if eu_p1::faceuse(b, u)? == Some(fu) {
                        used = Some(u);
                    }
                }
                let Some(u) = used else { return Ok(Err(Error::Unsupported)) };
                rim.push(RimEdge { edge: e, face: other, forward: b.edgeuse(u)?.orientation == OrientType::Same });
            }
        }
        let ends = |h: &RimEdge| -> solidcore::Result<(Option<VertexId>, Option<VertexId>)> {
            let (s, t) = edge::start_end_vertices(b, h.edge)?;
            Ok(if h.forward { (s, t) } else { (t, s) })
        };
        let Some(&front) = rim.first() else { return Ok(Err(Error::Unsupported)) };
        let start = ends(&front)?.0;
        let mut used = vec![false; rim.len()];
        if let Some(u) = used.first_mut() {
            *u = true;
        }
        let mut hole = vec![front];
        let mut end = ends(&front)?.1;
        // Each turn takes a rim edge not yet in the hole, so it ends within the rim's length.
        while end != start {
            let mut next = None;
            for (i, h) in rim.iter().enumerate() {
                if !used.get(i).copied().unwrap_or(true) && ends(h)?.0 == end {
                    if next.is_some() {
                        return Ok(Err(Error::Unsupported));
                    }
                    next = Some((i, *h));
                }
            }
            let Some((i, h)) = next else { return Ok(Err(Error::Unsupported)) };
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            hole.push(h);
            end = ends(&h)?.1;
        }
        if hole.len() != rim.len() {
            return Ok(Err(Error::Unsupported));
        }
        holes.push(hole);
    }
    // `DeleteFace(f, TRUE)` with the B-rep open for editing, each face once, in the patches'
    // order.
    let mut order = Vec::with_capacity(any.len());
    let mut taken = HashSet::new();
    for &f in patches.iter().flatten() {
        if taken.insert(f) {
            order.push(f);
        }
    }
    let editing = b.editing_enabled();
    b.set_editing_enabled(true);
    let mut deleted = Ok(());
    {
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        for f in order {
            let options = DeleteFaceOptions { delete_edges_vertices: true, ..DeleteFaceOptions::default() };
            if let Err(e) = delete_face(k, b, &mut env, &mut marks, f, options) {
                deleted = Err(e);
                break;
            }
        }
    }
    b.set_editing_enabled(editing);
    b.refresh_cached_topology();
    Ok(match deleted {
        Ok(()) => Ok(holes),
        Err(e) => Err(status(e)),
    })
}

/// The faceuse of `f` on the outside of its body (`outer_faceuse`): the one bounding the
/// infinite region (for a sheet, the upward one).
fn outer_faceuse(b: &Brep, f: FaceId) -> solidcore::Result<Option<FaceuseId>> {
    let Some(fu) = face_p2::upward_faceuse(b, f)? else { return Ok(None) };
    let shell = b.faceuse(fu)?.shell;
    let region = match shell {
        Some(sh) => b.shell(sh)?.region,
        None => None,
    };
    if shell.is_none() || region != b.infinite_region() { Ok(b.faceuse(fu)?.mate) } else { Ok(Some(fu)) }
}

/// `face_frame`: the outward unit normal, at the edge's midpoint, of the face the edge use `u`
/// bounds, and the unit direction into that face square to the edge.
fn face_frame(k: &Kernel, b: &mut Brep, u: EdgeuseId) -> solidcore::Result<Option<(Vec3, Vec3)>> {
    let Some(e) = b.edgeuse(u)?.edge else { return Ok(None) };
    let fu_of_lu = match eu_p1::loopuse(b, u)? {
        Some(lu) => b.loopuse(lu)?.faceuse,
        None => None,
    };
    let f = match fu_of_lu {
        Some(fu) => b.faceuse(fu)?.face,
        None => None,
    };
    let Some(f) = f else { return Ok(None) };
    let Some(fu) = outer_faceuse(b, f)? else { return Ok(None) };
    if b.face(f)?.surface.is_none() {
        return Ok(None);
    }
    let t = mid(b, e)?;
    let Some(at) = answer(evaluate_binormal(k, b, u, t, false, &BinormalOptions::default()))? else { return Ok(None) };
    let Some(normal) = answer(evaluate_surface_normal(k, b.face(f)?.surface.as_ref(), at.uv))? else { return Ok(None) };
    let len = normal.length();
    if !(len > 0.0 && len.is_finite()) {
        return Ok(None);
    }
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same { 1.0 } else { -1.0 };
    Ok(Some((normal * (sign / len), at.binormal)))
}

/// `sides_offsets`: the surface offsets of `e` (its blend edge use's surface first) at which
/// `chamfer_offsets` cuts a chamfer `d1` back along the face whose outward normal at the edge's
/// midpoint leans most along `up`, and `second` back along the other (or at the angle `second`
/// to the first face). With b0, b1 the directions into the faces and θ the angle between them,
/// setbacks s0, s1 lie at offsets r0 = (s1 - s0 cos θ) / sin θ and r1 = (s0 - s1 cos θ) / sin θ;
/// an angle φ to the first face gives s1 = s0 sin φ / sin(θ + φ).
fn sides_offsets(k: &Kernel, b: &mut Brep, e: EdgeId, d1: f64, second: ChamferSecond, up: Vec3) -> solidcore::Result<Option<(f64, f64)>> {
    let Some(u0) = edge::blend_edgeuse(k, b, e)? else { return Ok(None) };
    let Some(u1) = eu_p1::radial(b, u0)? else { return Ok(None) };
    let (Some((n0, b0)), Some((n1, b1))) = (face_frame(k, b, u0)?, face_frame(k, b, u1)?) else { return Ok(None) };
    let c = b0.dot(b1).clamp(-1.0, 1.0);
    let sn = (1.0 - c * c).max(0.0).sqrt();
    if !(sn > 1e-6) {
        return Ok(None);
    }
    let mut s0 = d1;
    let mut s1 = match second {
        ChamferSecond::Distance(d) => d,
        ChamferSecond::Angle(phi) => {
            let d = (c.acos() + phi).sin();
            if !(d > 1e-9) {
                return Ok(None);
            }
            d1 * phi.sin() / d
        }
    };
    if n1.dot(up) > n0.dot(up) {
        std::mem::swap(&mut s0, &mut s1);
    }
    let (r0, r1) = ((s1 - s0 * c) / sn, (s0 - s1 * c) / sn);
    Ok((r0.is_finite() && r1.is_finite() && r0 > 0.0 && r1 > 0.0).then_some((r0, r1)))
}

/// `smlib_chamfer_sides`: the edges nearest `points` chamfered `distance` back along the face
/// whose outward normal leans most along `first`, and by `second` on the other, with bevelled
/// corners.
pub(crate) fn chamfer_sides(body: &Body, points: &[[f64; 3]], tolerance: f64, distance: f64, second: ChamferSecond, first: [f64; 3]) -> Result<Body> {
    let second_ok = match second {
        ChamferSecond::Angle(a) => a.is_finite() && a > 1e-6 && a < std::f64::consts::PI - 1e-6,
        ChamferSecond::Distance(d) => size_ok(d),
    };
    if !points_ok(points, false) || !tolerance_ok(tolerance) || !size_ok(distance) || !second_ok || !first.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let up = p3(first);
    if !(up.length() > 0.0) {
        return Err(Error::Argument);
    }
    on_copy("smlib_chamfer_sides", body, |k, mut w| {
        let picked = match pick_edges(k, &w, points, tolerance)? {
            Ok((p, _)) => p,
            Err(m) => return Ok(Err(edge_miss(m))),
        };
        let solid_in = brep_p2::is_manifold_solid(&w)?;
        let (mut r0, mut r1) = (Vec::with_capacity(picked.len()), Vec::with_capacity(picked.len()));
        for &e in &picked {
            match sides_offsets(k, &mut w, e, distance, second, up)? {
                Some((a, b)) => {
                    r0.push(a);
                    r1.push(b);
                }
                None => return Ok(Err(Error::Status(INVALID_INPUT))),
            }
        }
        Ok(match blend_edges(k, &mut w, &picked, &r0, Section::Chamfer(&r1), None, &[], None)? {
            Ok(()) => finish(w, solid_in)?,
            Err(Ok(_)) => Err(Error::Unsupported),
            Err(Err(e)) => Err(e),
        })
    })
}

/// `law_ok`: a clamped B-spline of `degree` 1 to 3 from 0 to 1, 2 to 100 control radii, each
/// finite, at least 0 and at most 1e6.
fn law_ok(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    let n = radii.len();
    if !(1..=3).contains(&degree) || !(2..=100).contains(&n) || n <= degree || knots.len() != n + degree + 1 {
        return false;
    }
    if !knots.iter().chain(radii).all(|x| x.is_finite()) {
        return false;
    }
    for (i, &k) in knots.iter().enumerate() {
        let (low_end, high_end) = (i <= degree, i >= n);
        if (low_end && k != 0.0) || (high_end && k != 1.0) || (i > 0 && knots.get(i - 1).is_some_and(|&p| k < p)) {
            return false;
        }
    }
    radii.iter().all(|&r| (0.0..=1e6).contains(&r))
}

/// `law_pinches`: whether the law's radius is zero somewhere inside (0, 1).
fn law_pinches(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    let n = radii.len() as i64;
    let p = degree as i64;
    let all_zero = |from: i64, to: i64| -> bool {
        let lo = from.max(0);
        let mut j = lo;
        while j <= to && j < n {
            if radii.get(j as usize).is_some_and(|&r| r != 0.0) {
                return false;
            }
            j += 1;
        }
        from <= to
    };
    let kn = |i: i64| knots.get(i as usize).copied().unwrap_or(f64::NAN);
    let n_knots = n + p + 1;
    let mut i = p;
    while i + 1 < n_knots && i < n {
        if kn(i) < kn(i + 1) && all_zero(i - p, i) {
            return true;
        }
        i += 1;
    }
    let mut s = p + 1;
    while s < n {
        let mut m = 1;
        while s + m < n && kn(s + m) == kn(s) {
            m += 1;
        }
        if all_zero(s + m - 1 - p, s - 1) {
            return true;
        }
        s += m;
    }
    false
}

/// Whether the law has a corner inside (0, 1) (`SmBSplineFilletLaw::GetCornerParameters`, fork
/// 6eecf04): an interior knot where the radius' slope from the left and from the right differ by
/// more than `SM_EFF_ZERO * (1 + |left| + |right|)`. The slope is the derivative B-spline's,
/// `Q_i = p (r_{i+1} - r_i) / (u_{i+p+1} - u_{i+1})`; it jumps only at a knot repeated `m >= p`
/// times, from `Q_{s-2}` to `Q_{s+m-1-p}` (`s` the knot's first index).
fn law_has_corner(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    const EFF_ZERO: f64 = 1e-12;
    let p = degree;
    let q = |i: usize| -> Option<f64> {
        let (r0, r1) = (radii.get(i)?, radii.get(i + 1)?);
        let span = knots.get(i + p + 1)? - knots.get(i + 1)?;
        Some(p as f64 * (r1 - r0) / span)
    };
    let n = radii.len();
    let mut s = p + 1;
    while s < n {
        let Some(&u) = knots.get(s) else { break };
        let mut m = 1;
        while knots.get(s + m) == Some(&u) {
            m += 1;
        }
        if m >= p && u > 0.0 && u < 1.0 {
            let (Some(left), Some(right)) = (s.checked_sub(2).and_then(q), q(s + m - 1 - p)) else {
                return true;
            };
            if (left - right).abs() > EFF_ZERO * (1.0 + left.abs() + right.abs()) {
                return true;
            }
        }
        s += m;
    }
    false
}

/// `law_along`: the radius law along `e`, the B-spline with the radius as its x and its own
/// parameter as its y (control points at the Greville abscissae), reversed when `start` is
/// nearer the end of the edge's interval than its start. `None` where the capi's is null.
fn law_along(k: &Kernel, b: &Brep, e: EdgeId, start: [f64; 3], degree: usize, knots: &[f64], radii: &[f64]) -> solidcore::Result<Option<FilletLaw>> {
    let edge = b.edge(e)?;
    let Some(curve) = edge.curve.as_ref() else { return Ok(None) };
    let ivl = edge.interval;
    let (Some(lo), Some(hi)) = (answer(curve.evaluate_point(k, ivl.min()))?, answer(curve.evaluate_point(k, ivl.max()))?) else {
        return Ok(None);
    };
    let at = p3(start);
    let reverse = (hi - at).length() < (lo - at).length();
    let (nk, nr) = (knots.len(), radii.len());
    let kv: Vec<f64> = (0..nk).map(|i| if reverse { 1.0 - knots[nk - 1 - i] } else { knots[i] }).collect();
    let mut cps = Vec::with_capacity(nr);
    for i in 0..nr {
        let g: f64 = (1..=degree).filter_map(|j| kv.get(i + j)).sum();
        let r = if reverse { radii[nr - 1 - i] } else { radii[i] };
        cps.push(Vec3::new(r, g / degree as f64, 0.0));
    }
    let (mut unique, mut mults): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for &x in &kv {
        match (unique.last(), mults.last_mut()) {
            (Some(&u), Some(m)) if u == x => *m += 1,
            _ => {
                unique.push(x);
                mults.push(1);
            }
        }
    }
    let data = CanonicalData {
        dim: 3,
        degree,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    };
    let Some(c) = answer(BSplineCurve::create_canonical(&data))? else { return Ok(None) };
    // The law's map is the whole curve, 0 to 1 (`FilletLaw::new`'s).
    Ok(Some(FilletLaw::new(FilletLawKind::BSpline { law_curve: c, extended_law: None })))
}

/// `smlib_fillet_variable`: the one edge nearest `point` rounded with the radius law of
/// `degree`, `knots` and `radii`, 0 at the end nearest `start`.
pub(crate) fn fillet_variable(
    body: &Body,
    point: [f64; 3],
    tolerance: f64,
    start: [f64; 3],
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<Body> {
    if !points_ok(&[point], false) || !start.iter().all(|x| x.is_finite()) || !tolerance_ok(tolerance) || !law_ok(degree, knots, radii) {
        return Err(Error::Argument);
    }
    if law_pinches(degree, knots, radii) {
        return Err(Error::Unsupported);
    }
    on_copy("smlib_fillet_variable", body, |k, mut w| {
        let picked = match pick_edges(k, &w, &[point], tolerance)? {
            Ok((p, _)) => p,
            Err(m) => return Ok(Err(edge_miss(m))),
        };
        let [e] = picked[..] else { return Ok(Err(Error::NoMatch { point: 0 })) };
        let Some(law) = law_along(k, &w, e, start, degree, knots, radii)? else {
            return Ok(Err(Error::Status(INVALID_INPUT)));
        };
        // A law with a corner splits the round's face there (`SplitFilletFacesAtRadiusCorners`,
        // fork 6eecf04), which solidcore does not have.
        if law_has_corner(degree, knots, radii) {
            return Ok(Err(Error::Unsupported));
        }
        // The round is the union of its balls (smlib#78, solidcore#2262 with
        // `fillet_gn_section_generator`): each ball touches it along the circle shifted
        // -r r'/|c'| from its great circle. A law whose balls fold over one another is refused
        // (`NoRoomReason::BallsFold`), [`Error::NoRoom`].
        let solid_in = brep_p2::is_manifold_solid(&w)?;
        let (r0, r1) = (radii.first().copied().unwrap_or(0.0), radii.last().copied().unwrap_or(0.0));
        Ok(match blend_edges(k, &mut w, &[e], &[r0.max(r1)], Section::Round, Some(law), &[], None)? {
            Ok(()) => finish(w, solid_in)?,
            Err(Ok(_)) => Err(Error::Unsupported),
            Err(Err(e)) => Err(e),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laws_are_checked_as_the_capi_checks_them() {
        assert!(law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]));
        assert!(!law_ok(1, &[0.0, 0.0, 1.0], &[1.0, 2.0]));
        assert!(!law_ok(1, &[0.0, 0.5, 1.0, 1.0], &[1.0, 2.0]));
        assert!(!law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[-1.0, 2.0]));
        assert!(!law_ok(4, &[0.0; 7], &[1.0, 2.0]));
        assert!(law_pinches(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 0.0, 1.0]));
        assert!(!law_pinches(1, &[0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]));
        // A straight law has no corner; a bent one does at its inner knot; a cubic none.
        assert!(!law_has_corner(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]));
        assert!(!law_has_corner(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 1.5, 2.0]));
        assert!(law_has_corner(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 2.0, 1.0]));
        assert!(!law_has_corner(3, &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0], &[1.0, 2.0, 1.0, 2.0, 1.0]));
    }

    #[test]
    fn picks_follow_the_capis_rule() {
        let all = [0.0f64, 1.0, 3.0];
        let d = |x: f64, p: Point3| Ok(Some((x - p.x).abs()));
        assert_eq!(pick(&all, &[[0.9, 0.0, 0.0]], 0.5, 1e-6, d).unwrap(), Ok((vec![1.0], vec![0])));
        assert_eq!(pick(&all, &[[2.0, 0.0, 0.0]], 0.5, 1e-6, d).unwrap(), Err(Miss::None(0)));
        assert_eq!(pick(&all, &[[1.0, 0.0, 0.0], [0.5, 0.0, 0.0]], 1.0, 1e-6, d).unwrap(), Err(Miss::Ambiguous(1)));
        // The same entity twice is picked once, by its first point.
        assert_eq!(pick(&all, &[[3.1, 0.0, 0.0], [2.9, 0.0, 0.0]], 0.5, 1e-6, d).unwrap(), Ok((vec![3.0], vec![0])));
    }
}
