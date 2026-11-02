//! The blend corner's Setback: where every edge at a vertex is blended, three or more, each
//! blend stops at its setback from the vertex and the hole they leave is filled by the
//! setback split ([`crate::blend::blend_setback`]), to G1, or to G2 for curvature-continuous
//! sections (or rounds eased into them).
//!
//! Upstream: none (pinned SMLib builds such a vertex as an `NxN` corner). The fork's
//! `source/SMLib/src/SmBlendCornerSetback.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib 61547af, 1506e2f, ac8b4f8, f1d81f4; smlib#104).
//!
//! The corner's topology in the executive's pseudo B-rep, for faces `F_i` at the vertex
//! (`F_i` between blended edges `i - 1` and `i`, `m_pAllEUs`'s order): on each face the rail
//! end of the blend before it (`M_i`, `SM_FV_MATE` of `S_(i-1)`), the fill's base point there
//! (`B_i`, `SM_FV_PRECOMPUTED`) and the rail end of the blend after it (`S_i`,
//! `SM_FV_SETBACK`: its section square to its edge at the setback); edges `M_i-B_i` and
//! `B_i-S_i`, the two halves of the face's spring curve (`SM_FE_PRECOMPUTED`, on `F_i`), and
//! `S_i-M_(i+1)`, blend i's end section (`SM_FE_CROSS_SECTION`). The rails and sections are
//! the executive's; the fill, its base points and its spring curves are built once the
//! blends' surfaces exist (and are eased, for an eased setback): the spring curves are the
//! setback patches' sides on the faces, the base points their inner corners. The fill's 2n
//! patches become faces of their own, bounded by their iso-curves, and are stitched to the
//! blends' faces and to each other with the rest.
//!
//! | Rust                         | Fork                                                  |
//! |------------------------------|-------------------------------------------------------|
//! | [`SetbackCorner`]            | `SmBlendCorner`'s `m_vSetBacks`, `m_lFillOrder`, `m_bConvexSetBack`, `m_bSetBackOverflows`, `m_pFill` |
//! | [`SetBackFill`], [`set_back_fill_of`] | `SmFilletExecutive::SetBackFill`, `GetSetBackFill` |
//! | [`SetbackRefusal`]           | `SetBackNotBuilt`, the corner's `NoteError` and `NoteFitSetBack` |
//! | [`classify_setback`]         | `SmBlendCorner::ClassifySetBack`                      |
//! | [`make_setback_topology`]    | `SmBlendCorner::MakeSetBackTopology`                  |
//! | [`build_setback_fill`]       | `SmBlendCorner::BuildSetBackFill`                     |
//! | [`create_setback_patches`]   | `SmBlendCorner::CreateSetBackPatches`                 |
//! | `away_from` (private)        | `sm_SbAwayFrom`                                       |
//! | `corner_planes` (private)    | `sm_SbCornerPlanes`                                   |
//! | `edge_blended` (private)     | `sm_SbEdgeBlended`                                    |
//! | `set_backs` (private)        | `sm_SbSetBacks`                                       |
//!
//! Deviations: the Setback's own members are kept in [`SetbackCorner`] on the corner
//! ([`FilletCorner::setback`]) from its creation (`SetFillOrder`) until its patches are
//! faces. The fork's corner notes its refusal on the executive itself; here it is returned
//! ([`SetbackRefusal`]) and the session notes it. The executive's set-back fills
//! (`m_vSetBackFills`) are the session's, passed in. Opt-in:
//! `Kernel::fixes.fillet_blend_corner` (WR-697). No recursion.
//!
//! [`FilletCorner::setback`]: crate::blend::fillet_corner::p3::FilletCorner::setback

use crate::blend::blend_corner::ball::{solver_of_edgeuse_or_mate, BallSolvers};
use crate::blend::blend_corner::swept::fmt_g;
use crate::blend::blend_corner::BlendCornerEvent;
use crate::blend::blend_setback::{
    certified_scale, containment, distances, fill_hole, Edge, Face, Fill, Round,
};
use crate::blend::fillet_corner::p1::{face_by_two_edgeuses, test_g1_continuity};
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner};
use crate::blend::fillet_corner::topology::new_corner_vertex;
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilStatus, FilletBrep, FilletCornerId, FilletEdgeType, FilletErrorType,
    FilletGeomId, FilletVertexType,
};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolverType, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::b_spline_surface::p2::IsoCurveOptions;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::Surface;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::setback::classify_setback";
const OP_TOPOLOGY: &str = "blend::blend_corner::setback::make_setback_topology";
const OP_FILL: &str = "blend::blend_corner::setback::build_setback_fill";
const OP_PATCHES: &str = "blend::blend_corner::setback::create_setback_patches";

/// The Setback's own members on a blend corner that is one.
#[derive(Debug, Clone)]
pub struct SetbackCorner {
    /// `m_vSetBacks`: each blended edge's setback, in the corner's `m_pAllEUs` order.
    pub setbacks: Vec<f64>,
    /// `m_lFillOrder`: the fill's order (1: G1, 2: G2).
    pub fill_order: usize,
    /// `m_bConvexSetBack`: whether the corner's edges are convex.
    pub convex: bool,
    /// `m_bSetBackOverflows`: the rule's setbacks do not fit, the blends overflow (refused
    /// if the corner is built).
    pub overflows: bool,
    /// `m_pFill`: the fill, from the edge geometry until the patches take it.
    pub fill: Option<Fill>,
}

impl Default for SetbackCorner {
    /// The constructor's: `m_lFillOrder(1)`, `m_bConvexSetBack(TRUE)`,
    /// `m_bSetBackOverflows(FALSE)`, no setbacks and no fill.
    fn default() -> Self {
        Self {
            setbacks: Vec::new(),
            fill_order: 1,
            convex: true,
            overflows: false,
            fill: None,
        }
    }
}

impl SetbackCorner {
    /// The Setback of fill order `order` (`SetFillOrder`).
    #[must_use]
    pub fn with_order(order: usize) -> Self {
        Self {
            fill_order: order,
            ..Self::default()
        }
    }
}

/// `SmFilletExecutive::SetBackFill`: a corner asked to be set back, before the corners are
/// made (`SetSetBackFill`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetBackFill {
    /// `m_cpVertex`.
    pub vertex: VertexId,
    /// `m_dSetBack`: its setback along every edge there.
    pub setback: f64,
    /// `m_dEase`: the ease before it (0: none).
    pub ease: f64,
    /// `m_bOpen`: left open, for the caller to fill.
    pub open: bool,
}

/// `SmFilletExecutive::GetSetBackFill`: the first fill asked at `vertex`.
#[must_use]
pub fn set_back_fill_of(fills: &[SetBackFill], vertex: VertexId) -> Option<SetBackFill> {
    fills.iter().find(|f| f.vertex == vertex).copied()
}

/// A Setback the corner does not build, with what it notes at its vertex (the fork's
/// `SetBackNotBuilt`: `SM_FILERR_CORNER_UNSUPPORTED`; or `SM_FILERR_NO_ROOM`), and the
/// setback that builds (`NoteFitSetBack`) where it notes one.
#[derive(Debug, Clone, PartialEq)]
pub struct SetbackRefusal {
    /// The error noted.
    pub error: FilletErrorType,
    /// Its reason.
    pub comment: String,
    /// `NoteFitSetBack(dSetBack)`: `Some` where the corner notes one.
    pub fit_setback: Option<f64>,
}

impl SetbackRefusal {
    /// `SetBackNotBuilt(cpWhy)`.
    fn not_built(why: impl Into<String>) -> Self {
        Self {
            error: FilletErrorType::CornerUnsupported,
            comment: why.into(),
            fit_setback: None,
        }
    }

    /// `NoteError(.., SM_FILERR_NO_ROOM, cWhy)`.
    fn no_room(why: impl Into<String>, fit_setback: Option<f64>) -> Self {
        Self {
            error: FilletErrorType::NoRoom,
            comment: why.into(),
            fit_setback,
        }
    }
}

/// `smos_Min`.
fn smin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `smos_Max`.
fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `sm_SbAwayFrom`: the unit direction of `e` away from `vertex`, from its curve's end
/// tangent there.
fn away_from(k: &Kernel, b: &Brep, e: EdgeId, vertex: VertexId) -> Result<Vec3> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let curve = edge::curve(b, e)?.ok_or_else(nul)?;
    let ivl = edge::interval(b, e)?;
    let at_start = edge::start_vertex(b, e)? == Some(vertex);
    let pv = curve.evaluate(
        k,
        if at_start { ivl.min() } else { ivl.max() },
        1,
        true,
        true,
    )?;
    let d1 = pv.get(1).copied().ok_or_else(nul)?;
    let away = if at_start { d1 } else { -d1 };
    away.unitize()
}

/// What `sm_SbCornerPlanes` finds of a corner whose faces are all planes.
struct CornerPlanes {
    /// `rvFaces`: face i between edges `i - 1` and `i`.
    faces: Vec<FaceId>,
    /// `rvPlanes`: their planes through the vertex, normals out of the material.
    planes: Vec<Face>,
    /// `rvEdges`: edge i between faces i and `i + 1`.
    edges: Vec<Edge>,
}

/// `sm_SbCornerPlanes`: of a corner whose edges all have one convexity, the planes of its
/// faces with their normals out of the material, and its edges for [`distances`]; `None`
/// (`rbPlanes` FALSE) where a face is not a plane (or its side is undecided).
fn corner_planes(
    k: &Kernel,
    b: &Brep,
    vtx: VertexId,
    all_eus: &[EdgeuseId],
    solvers: &dyn BallSolvers,
    tol: f64,
    convex: bool,
) -> Result<Option<CornerPlanes>> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let n = all_eus.len();
    let mut away = Vec::with_capacity(n);
    for &eu in all_eus {
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        away.push(away_from(k, b, e, vtx)?);
    }
    let vpoint = vertex::point(b, vtx)?;
    let mut faces = Vec::with_capacity(n);
    let mut planes = Vec::with_capacity(n);
    for i in 0..n {
        let prev = all_eus.get((i + n - 1) % n).copied();
        let this = all_eus.get(i).copied();
        let Some(face) = face_by_two_edgeuses(b, prev, this, None)? else {
            return Ok(None);
        };
        let Some(surface) = face2::surface(b, face)? else {
            return Ok(None);
        };
        let planar = surface.is_planar(k, tol)?;
        if !planar.is_planar {
            return Ok(None);
        }
        let Ok(mut normal) = planar.normal.unitize() else {
            return Ok(None);
        };
        // the other edges lie on the material's side of a convex corner's face, off it at
        // a concave one's
        let mut others = Vec3::new(0.0, 0.0, 0.0);
        for (j, &aj) in away.iter().enumerate() {
            if j != i && j != (i + n - 1) % n {
                others += aj;
            }
        }
        let side = normal.dot(others);
        if side.abs() <= EFF_ZERO {
            return Ok(None);
        }
        if (side > 0.0) == convex {
            normal = -normal;
        }
        faces.push(face);
        planes.push(Face {
            point: vpoint,
            normal,
        });
    }
    let mut edges = Vec::with_capacity(n);
    for (i, &eu) in all_eus.iter().enumerate() {
        let id = solver_of_edgeuse_or_mate(b, eu, solvers)?.ok_or_else(nul)?;
        let fs = solvers.solver(id)?;
        let s0 = fs.surface(0)?.ok_or_else(nul)?;
        edges.push(Edge {
            away: away.get(i).copied().ok_or_else(nul)?,
            radius: s0.surface.offset_distance().abs(),
            faces: [i, (i + 1) % n],
        });
    }
    Ok(Some(CornerPlanes {
        faces,
        planes,
        edges,
    }))
}

/// `sm_SbEdgeBlended`: whether `e` is blended by the executive (any of its edge uses has a
/// solver).
fn edge_blended(b: &Brep, e: EdgeId, solvers: &dyn BallSolvers) -> Result<bool> {
    let mut eu = edge::primary_edgeuse(b, e)?;
    for _ in 0..2 {
        let Some(this) = eu else {
            break;
        };
        if solvers.solver_of_edgeuse(this)?.is_some() {
            return Ok(true);
        }
        if let Some(mate) = eu1::mate(b, this)? {
            if solvers.solver_of_edgeuse(mate)?.is_some() {
                return Ok(true);
            }
        }
        eu = eu1::radial(b, this)?;
    }
    Ok(false)
}

/// What `sm_SbSetBacks` works out.
struct SetBacks {
    /// `rvSetBacks`.
    setbacks: Vec<f64>,
    /// `rvLeast`.
    least: Vec<f64>,
    /// `rvRoom`.
    room: Vec<f64>,
    /// `rbFits`.
    fits: bool,
    /// `rbShort`.
    short: bool,
    /// `rdLargest`.
    largest: f64,
}

/// `sm_SbSetBacks`: the corner's setbacks (owner, 2026-11-01). Each edge's room: its
/// length, less the far end's claim: an asked-for setback with its ease there, or, where the
/// far end is itself a vertex whose three or more edges are all blended (a corner that may
/// set back too), half the edge. Asked for: the asked setback on every edge, refused (`fits`
/// false) unless it and its ease lie below every room, `largest` then the largest that fits
/// (the least room less the ease); and below an edge's contact-line minimum (`short`). Else
/// the rule's ([`distances`]), each shrunk to its room less a quarter of its radius where
/// longer, as long as that is at or above its minimum (`fits` false where not: the rounds
/// beside an edge cross past its room, an overflow). `Err(reason)` where [`distances`]
/// refuses.
#[allow(clippy::too_many_arguments)]
fn set_backs(
    b: &Brep,
    vtx: VertexId,
    all_eus: &[EdgeuseId],
    solvers: &dyn BallSolvers,
    fills: &[SetBackFill],
    planes: &[Face],
    edges: &[Edge],
    tol: f64,
) -> Result<core::result::Result<SetBacks, Option<&'static str>>> {
    let nul = || SolidError::null_pointer(OP_TOPOLOGY);
    let n = all_eus.len();
    let rule = match distances(planes, edges) {
        Ok(rule) => rule,
        Err(refusal) => return Ok(Err(refusal.reason)),
    };
    let mut setbacks = rule.setbacks;
    let least = rule.least;
    let asked = set_back_fill_of(fills, vtx);
    let mut room = vec![0.0; n];
    let mut fits = true;
    let mut short = false;
    let mut largest = BIG_DOUBLE;
    for (i, &eu) in all_eus.iter().enumerate() {
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        let start = edge::start_vertex(b, e)?.ok_or_else(nul)?;
        let end = edge::end_vertex(b, e)?.ok_or_else(nul)?;
        let length = vertex::point(b, start)?.distance(vertex::point(b, end)?);
        let far = edge::other_vertex(b, e, Some(vtx))?.filter(|&f| f != vtx);
        let mut claim = 0.0;
        if let Some(ff) = far.and_then(|f| set_back_fill_of(fills, f)) {
            claim = ff.setback + ff.ease;
        } else if let Some(far) = far.filter(|_| asked.is_none()) {
            let far_edges = vertex::edges(b, far, None)?;
            let mut all = far_edges.len() >= 3;
            for &fe in &far_edges {
                if !all {
                    break;
                }
                all = edge_blended(b, fe, solvers)?;
            }
            if all {
                claim = 0.5 * length;
            }
        }
        let r = length - claim;
        *room.get_mut(i).ok_or_else(nul)? = r;
        let least_i = least.get(i).copied().ok_or_else(nul)?;
        let radius_i = edges.get(i).ok_or_else(nul)?.radius;
        let sb = setbacks.get_mut(i).ok_or_else(nul)?;
        if let Some(a) = asked {
            largest = smin(largest, r - a.ease);
            if a.setback + a.ease >= r - tol {
                fits = false;
            }
            if a.setback < least_i - tol {
                short = true;
            }
            *sb = a.setback;
        } else {
            let fit = r - 0.25 * radius_i;
            if *sb > fit {
                *sb = fit;
            }
            if *sb < least_i {
                fits = false;
            }
        }
    }
    Ok(Ok(SetBacks {
        setbacks,
        least,
        room,
        fits,
        short,
        largest,
    }))
}

/// `SmBlendCorner::ClassifySetBack` (the fork's 61547af): whether the corner of a filleted
/// vertex where every edge is blended (`solver_eus`, `all_eus`, three or more) is a Setback:
/// every face there a plane, every edge a straight line, no two edges tangent, all of one
/// convexity, each blend a constant round of one radius (`SM_FSG_CIRCULAR`) or the
/// curvature-continuous section (`SM_FSG_BLEND_CURVE`), and either set back on request
/// (`fills`, `GetSetBackFill`), or G2 (curvature-continuous sections), or rounds of unequal
/// radii, which no one ball touches. Equal circular rounds stay where they were
/// ([`BlendCornerEvent::None`]). The fill's order with it: 2 for curvature-continuous
/// sections or a setback with an ease, else 1.
///
/// # Errors
///
/// [`SolidError::null_pointer`] where a ring edge use has no solver or one no offset surface
/// (`NER`); those of the queries.
#[allow(clippy::too_many_arguments)]
pub fn classify_setback(
    k: &Kernel,
    b: &mut Brep,
    vtx: VertexId,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    solvers: &dyn BallSolvers,
    tangency_tol_deg: f64,
    fills: &[SetBackFill],
) -> Result<(BlendCornerEvent, usize)> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let none = Ok((BlendCornerEvent::None, 1));
    let n = all_eus.len();
    if n < 3 || solver_eus.len() != n {
        return none;
    }

    // the blends: constant rounds of one radius across, or curvature-continuous sections
    let mut tol = BIG_DOUBLE;
    let mut min_r = BIG_DOUBLE;
    let mut max_r = 0.0;
    let mut kind = FilletSurfaceGeneratorType::Unknown;
    let mut one_kind = true;
    for (i, &eu) in all_eus.iter().enumerate() {
        let id = solver_of_edgeuse_or_mate(b, eu, solvers)?.ok_or_else(nul)?;
        let fs = solvers.solver(id)?;
        let this = fs
            .fsg
            .as_ref()
            .ok_or_else(nul)?
            .fillet_surface_generator_type();
        if (this != FilletSurfaceGeneratorType::Circular
            && this != FilletSurfaceGeneratorType::BlendCurve)
            || solvers.solver_type(id)? != FilletSolverType::ConstRadius
            || solvers.offset_radii_can_change(id)?
        {
            return none;
        }
        if i > 0 && this != kind {
            one_kind = false;
        }
        kind = this;
        let r0 = fs
            .surface(0)?
            .ok_or_else(nul)?
            .surface
            .offset_distance()
            .abs();
        let r1 = fs
            .surface(1)?
            .ok_or_else(nul)?
            .surface
            .offset_distance()
            .abs();
        tol = smin(tol, fs.approx_tol_3d);
        if (r0 - r1).abs() > fs.approx_tol_3d {
            return none;
        }
        min_r = smin(min_r, r0);
        max_r = smax(max_r, r0);
    }

    // the edges: straight, of one convexity, no two tangent at the vertex
    let mut concave0 = false;
    for (i, &eu) in all_eus.iter().enumerate() {
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        let Some(curve) = edge::curve(b, e)? else {
            return none;
        };
        if curve.is_line(k, 10, tol)?.is_none() {
            return none;
        }
        let concave = is_concave_edge(k, b, e)?;
        if i == 0 {
            concave0 = concave;
        } else if concave != concave0 {
            return none;
        }
        for &other in all_eus.iter().skip(i + 1) {
            let oe = eu1::edge(b, other)?;
            if test_g1_continuity(k, b, Some(vtx), Some(e), oe, tangency_tol_deg)?.0 {
                return none;
            }
        }
    }

    // the faces: planes
    if corner_planes(k, b, vtx, all_eus, solvers, tol, !concave0)?.is_none() {
        return none;
    }

    let asked = set_back_fill_of(fills, vtx);
    if asked.is_some_and(|a| a.open) || (asked.is_none() && !one_kind) {
        return none; // a corner left open keeps the patch its caller removes
    }
    let g2 = one_kind && kind == FilletSurfaceGeneratorType::BlendCurve;
    // rounds of one radius meet a common ball where three faces meet: not a setback unless
    // asked
    if asked.is_none() && !g2 && max_r - min_r <= tol {
        return none;
    }
    // (setbacks that do not fit are the Setback's to refuse, or the overflow's:
    // MakeSetBackTopology)
    let order = if g2 || asked.is_some_and(|a| a.ease > 0.0) {
        2
    } else {
        1
    };
    Ok((BlendCornerEvent::Setback, order))
}

/// What the Setback's topology operates on besides the corner itself.
pub struct SetbackTopologyEnv<'a> {
    /// The executive's kernel.
    pub k: &'a Kernel,
    /// The target whose edge uses the corner stores.
    pub target: &'a mut Brep,
    /// The executive's pseudo-B-rep.
    pub pseudo: &'a mut FilletBrep,
    /// This corner's identity in the executive's corner store.
    pub corner: FilletCornerId,
    /// The executive's solvers.
    pub solvers: &'a dyn BallSolvers,
    /// The executive's set-back fills (`m_vSetBackFills`).
    pub fills: &'a [SetBackFill],
}

impl std::fmt::Debug for SetbackTopologyEnv<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetbackTopologyEnv")
            .field("corner", &self.corner)
            .finish_non_exhaustive()
    }
}

/// The corner's lookup for the pseudo B-rep's rail edge uses.
fn lookup(corner: FilletCornerId, this: &FilletCorner) -> CornerLookup<'_> {
    CornerLookup {
        this: Some((Some(corner), this)),
        ..Default::default()
    }
}

/// `SmBlendCorner::MakeSetBackTopology` (the fork's 61547af, 1506e2f, f1d81f4): the
/// Setback's setbacks and topology (module docs). The setbacks are those asked for, each at
/// least where the rails beside its edge cross it, or else the owner's rule, grown for a G1
/// fill to the least scale that certifies it convex (within each edge's room); each with
/// its ease must fit on its edge. `Ok(Err(refusal))` where the corner refuses, with what it
/// notes.
///
/// # Errors
///
/// `SM_ERR` for a corner that is not a Setback of three or more ring edge uses;
/// [`SolidError::null_pointer`] where the fork dereferences missing topology (`NER`); those
/// of the pseudo B-rep's construction.
#[allow(clippy::too_many_lines)] // the fork's one function
pub fn make_setback_topology(
    corner: &mut FilletCorner,
    env: SetbackTopologyEnv<'_>,
) -> Result<core::result::Result<(), SetbackRefusal>> {
    let SetbackTopologyEnv {
        k,
        target,
        pseudo,
        corner: corner_id,
        solvers,
        fills,
    } = env;
    let nul = || SolidError::null_pointer(OP_TOPOLOGY);
    let err = || SolidError::sm_err(OP_TOPOLOGY);
    let all = corner.all_eus.clone().ok_or_else(nul)?;
    let solver_eus = corner.solver_eus.clone().ok_or_else(nul)?;
    let n = all.len();
    if n < 3 || solver_eus.len() != n {
        return Err(err());
    }
    let vtx = corner.vertex.ok_or_else(nul)?;
    let tol = corner.approx_tol_3d;
    let mut sb = corner.setback.clone().ok_or_else(err)?;

    let first = eu1::edge(target, *all.first().ok_or_else(nul)?)?.ok_or_else(nul)?;
    let concave = is_concave_edge(k, target, first)?;
    sb.convex = !concave;
    let Some(planes) = corner_planes(k, target, vtx, &all, solvers, tol, sb.convex)? else {
        return Err(err()); // ClassifySetBack found them
    };

    // the setbacks. Where the rule's do not fit (the rounds beside an edge cross past its
    // room) the blends overflow their faces: the executive's overflow comes first, and the
    // corner, made meanwhile with each setback at half its room (only so that its vertices
    // can be placed), refuses in BuildSetBackFill if it is ever built.
    let found = set_backs(
        target,
        vtx,
        &all,
        solvers,
        fills,
        &planes.planes,
        &planes.edges,
        tol,
    )?;
    let SetBacks {
        setbacks: mut sbs,
        least,
        room,
        fits,
        short,
        largest,
    } = match found {
        Ok(found) => found,
        Err(why) => {
            return Ok(Err(SetbackRefusal::not_built(why.unwrap_or(
                "Fillet Error: the corner's setbacks cannot be found",
            ))))
        }
    };
    let asked = set_back_fill_of(fills, vtx);
    if let Some(a) = asked {
        if !fits {
            return Ok(Err(SetbackRefusal::no_room(
                format!(
                    "Fillet Error: a setback of {} (with its ease, {}) does not fit on the corner's edges: a setback below {} fits",
                    fmt_g(a.setback),
                    fmt_g(a.setback + a.ease),
                    fmt_g(largest)
                ),
                None,
            )));
        }
        if short {
            let mut most = 0.0;
            for &l in &least {
                most = smax(most, l);
            }
            return Ok(Err(SetbackRefusal::not_built(format!(
                "Fillet Error: a setback of {} stops short of where the rails of the rounds beside an edge cross it, {} from the vertex",
                fmt_g(a.setback),
                fmt_g(most)
            ))));
        }
        corner.ease_dist = a.ease;
    }
    sb.overflows = !fits;
    // a G1 fill is taken only certified convex: the least scale of the setbacks at which it
    // is (CertifiedScale). The rule's grow to it, each held at its room less the rule's
    // quarter radius (the others growing on); refused, naming them, where none within the
    // rooms certifies. One asked for that is too short is refused, naming the setback that
    // builds (or that none does, where that one does not fit with its ease).
    if sb.fill_order == 1 && !sb.overflows {
        let mut most = vec![0.0; n];
        for (i, m) in most.iter_mut().enumerate() {
            *m = if asked.is_some() {
                BIG_DOUBLE
            } else {
                smax(
                    sbs.get(i).copied().ok_or_else(nul)?,
                    room.get(i).copied().ok_or_else(nul)?
                        - 0.25 * planes.edges.get(i).ok_or_else(nul)?.radius,
                )
            };
        }
        let scale = match certified_scale(k, &planes.planes, &planes.edges, &sbs, Some(&most)) {
            Ok(scale) => scale,
            Err(refusal) => {
                return Ok(Err(SetbackRefusal::not_built(refusal.reason.unwrap_or(
                    "Fillet Error: the setback corner's convex fill cannot be sized",
                ))))
            }
        };
        if scale.scale == 0.0 {
            let why = if let Some(a) = asked {
                format!(
                    "Fillet Error: no setback up to twice {} keeps the setback corner's fill convex",
                    fmt_g(a.setback)
                )
            } else {
                let list: Vec<String> = most.iter().map(|&m| fmt_g(m)).collect();
                format!(
                    "Fillet Error: no setbacks within the corner's edges keep its fill convex (the most that fit: {})",
                    list.join(", ")
                )
            };
            return Ok(Err(SetbackRefusal::no_room(why, Some(0.0))));
        }
        if let Some(a) = asked.filter(|_| scale.scale > 1.0) {
            let mut fits_room = true;
            for &r in &room {
                if scale.scale * a.setback + corner.ease_dist >= r {
                    fits_room = false;
                }
            }
            let builds = if fits_room {
                scale.scale * a.setback
            } else {
                0.0
            };
            let why = if fits_room {
                format!(
                    "Fillet Error: a setback of {} is too short for the setback corner's fill to stay convex: a setback of {} builds it",
                    fmt_g(a.setback),
                    fmt_g(builds)
                )
            } else {
                format!(
                    "Fillet Error: a setback of {} is too short for the setback corner's fill to stay convex, and the {} that would be does not fit on its edges",
                    fmt_g(a.setback),
                    fmt_g(scale.scale * a.setback)
                )
            };
            return Ok(Err(SetbackRefusal::no_room(why, Some(builds))));
        }
        for (s, &m) in sbs.iter_mut().zip(&most) {
            *s = smin(scale.scale * *s, m);
        }
    }
    if sb.overflows {
        for (s, &r) in sbs.iter_mut().zip(&room) {
            *s = 0.5 * r;
        }
    }
    corner.set_back_dist = 0.0;
    for &s in &sbs {
        corner.set_back_dist = smax(corner.set_back_dist, s);
    }
    sb.setbacks = sbs;
    corner.setback = Some(sb);

    // the vertices: per face, M (the blend before's rail end), B (the base point), S (the
    // blend after's)
    let mut solver_ids = Vec::with_capacity(n);
    let mut geoms: Vec<FilletGeomId> = Vec::with_capacity(n);
    for &eu in &all {
        let id = solver_of_edgeuse_or_mate(target, eu, solvers)?.ok_or_else(nul)?;
        geoms.push(solvers.solver(id)?.first_fillet_geom()?);
        solver_ids.push(id);
    }
    for i in 0..n {
        let face = *planes.faces.get(i).ok_or_else(nul)?;
        let p = (i + n - 1) % n;
        let mut fvs = [None; 3];
        for (slot, vtype) in fvs.iter_mut().zip([
            FilletVertexType::Mate,
            FilletVertexType::Precomputed,
            FilletVertexType::Setback,
        ]) {
            let fv = new_corner_vertex(k, pseudo, corner_id)?;
            {
                let v = pseudo.fillet_vertex_mut(fv)?;
                v.set_point_class(PointClassType::Face, Some(EntityRef::Face(face)));
                v.set_fillet_vertex_type(vtype);
            }
            corner.vertices.push(fv);
            *slot = Some(fv);
        }
        let [fv_m, _, fv_s] = fvs;
        for (round, fv) in [(p, fv_m), (i, fv_s)] {
            let id = *solver_ids.get(round).ok_or_else(nul)?;
            let fs = solvers.solver(id)?;
            let g = *geoms.get(round).ok_or_else(nul)?;
            let rail = fs.find_index_of_rail_on_face(target, Some(face))?;
            pseudo.make_rail_edgeuse_on(
                g,
                rail,
                fv,
                OrientType::Unknown,
                None,
                lookup(corner_id, corner),
                fs,
                target,
            )?;
        }
    }

    // the edges: per face, M-B and B-S on it, then the blend after's end section S-M
    for i in 0..n {
        let m = *corner.vertices.get(3 * i).ok_or_else(nul)?;
        let b = *corner.vertices.get(3 * i + 1).ok_or_else(nul)?;
        let s = *corner.vertices.get(3 * i + 2).ok_or_else(nul)?;
        let next_m = *corner.vertices.get(3 * ((i + 1) % n)).ok_or_else(nul)?;
        for (kk, (from, to)) in [(m, b), (b, s), (s, next_m)].into_iter().enumerate() {
            let e = pseudo.make_fillet_edge(Some(from), Some(to), Some(corner_id))?;
            if kk < 2 {
                let fe = pseudo.fillet_edge_mut(e)?;
                fe.set_fillet_edge_type(FilletEdgeType::Precomputed);
                fe.set_original_face(planes.faces.get(i).copied());
            } else {
                pseudo
                    .fillet_edge_mut(e)?
                    .set_fillet_edge_type(FilletEdgeType::CrossSection);
                let primary = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(nul)?;
                pseudo.set_edgeuse_fillet_geom(primary, geoms.get(i).copied())?;
                pseudo.fillet_vertex_mut(s)?.set_mate(0, Some(next_m));
                pseudo.fillet_vertex_mut(next_m)?.set_mate(0, Some(s));
            }
            corner.edges.push(e);
        }
    }

    // each blend ends on its own section; the hole is the corner's
    corner.blending = true;
    corner.bevel = false;
    Ok(Ok(()))
}

/// `SmBlendCorner::BuildSetBackFill` (the fork's 61547af, 1506e2f): the Setback's fill
/// ([`fill_hole`]), from the blends' surfaces at their end sections, and from it the base
/// points and the spring curves' halves. `Ok(Err(refusal))` where the fill is refused (its
/// reason noted), where it stands past a face ([`containment`], certified) farther than the
/// corner's tolerance, or where the rule's setbacks overflowed.
///
/// # Errors
///
/// `SM_ERR` where the corner's topology is not the Setback's, a setback vertex is not
/// processed or the fill does not match it; [`SolidError::null_pointer`] where the fork
/// dereferences missing geometry (`NER`); those of the queries.
#[allow(clippy::too_many_lines)] // the fork's one function
pub fn build_setback_fill(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &dyn BallSolvers,
) -> Result<core::result::Result<(), SetbackRefusal>> {
    let nul = || SolidError::null_pointer(OP_FILL);
    let err = || SolidError::sm_err(OP_FILL);
    let all = corner.all_eus.clone().ok_or_else(nul)?;
    let sb = corner.setback.as_ref().ok_or_else(err)?;
    if sb.overflows {
        return Ok(Err(SetbackRefusal::no_room(
            "Fillet Error: the rounds beside an edge at a setback corner cross past the room on that edge: the blends are larger than their faces allow",
            None,
        )));
    }
    let n = all.len();
    if corner.vertices.len() != 3 * n || corner.edges.len() != 3 * n {
        return Err(err());
    }
    let vtx = corner.vertex.ok_or_else(nul)?;
    let tol = corner.approx_tol_3d;
    let (convex, order) = (sb.convex, sb.fill_order);
    let Some(planes) = corner_planes(k, target, vtx, &all, solvers, tol, convex)? else {
        return Err(err());
    };
    let vpoint = vertex::point(target, vtx)?;

    // each blend at its end section: its surface, its rails' direction on it, the rail
    // parameter of its setback vertex, and on which side the corner lies
    let mut surfaces: Vec<Surface> = Vec::with_capacity(n);
    let mut sides: Vec<(SurfParamType, f64, bool)> = Vec::with_capacity(n);
    for i in 0..n {
        let s = *corner.vertices.get(3 * i + 2).ok_or_else(nul)?;
        if !pseudo.fillet_vertex(s)?.is_processed() {
            return Err(err());
        }
        let fv_geoms = pseudo.vertex_fillet_geoms(s)?;
        if fv_geoms.len() != 1 {
            return Err(err());
        }
        let fg = *fv_geoms.first().ok_or_else(err)?;
        let solver_id = pseudo.geom(fg)?.solver.ok_or_else(nul)?;
        let fs = solvers.solver(solver_id)?;
        // SM_CAST_PTR(SmBSplineSurface, pFG->GetFilletSurface()) (NER): an exact round's
        // cylinder, cone or torus is one, evaluated as its own kind
        let surface = match pseudo.geom(fg)?.fillet_surface(None)? {
            Some(s) if crate::topology::brep::p3::with_nurb(k, s)?.is_some() => s.clone(),
            _ => return Err(nul()),
        };
        let face = *planes.faces.get(i).ok_or_else(nul)?;
        let rail_index = fs.find_index_of_rail_on_face(target, Some(face))?;
        if rail_index > 1 {
            return Err(err());
        }
        let rail = pseudo.geom(fg)?.rail(rail_index).ok_or_else(nul)?;
        let rail_curve = edge::curve(pseudo.brep(), rail)?.ok_or_else(nul)?.clone();
        let rail_ivl = rail_curve.natural_interval();
        let dropped = rail_curve.drop_point(
            k,
            &rail_ivl,
            pseudo.vertex_point(s)?,
            None,
            tol,
            None,
            SolverOperation::Minimize,
        )?;
        if !dropped.success {
            return Err(err());
        }
        let end = dropped.parameter;
        let on_rail = rail_curve.evaluate_point(k, end)?;
        let lo = rail_curve.evaluate_point(k, rail_ivl.min())?;
        let hi = rail_curve.evaluate_point(k, rail_ivl.max())?;
        // the rails' direction: the one over the rails' interval whose iso-curve at the end
        // section passes through the rail there
        let dom = surface.natural_uv_domain_with_nurb(k)?;
        let param_tol = 1.0e-9 * (1.0 + rail_ivl.length());
        let mut along = SurfParamType::Unknown;
        for dir in 0..2 {
            if along != SurfParamType::Unknown {
                break;
            }
            let (al, ac) = if dir == 0 {
                (dom.u_interval(), dom.v_interval())
            } else {
                (dom.v_interval(), dom.u_interval())
            };
            if (al.min() - rail_ivl.min()).abs() > param_tol
                || (al.max() - rail_ivl.max()).abs() > param_tol
            {
                continue;
            }
            let uv = |a: f64| {
                if dir == 0 {
                    Point2::new(end, a)
                } else {
                    Point2::new(a, end)
                }
            };
            let pa = surface.evaluate_point(k, uv(ac.min()))?;
            let pb = surface.evaluate_point(k, uv(ac.max()))?;
            if smin(pa.distance(on_rail), pb.distance(on_rail)) <= tol {
                along = if dir == 0 {
                    SurfParamType::U
                } else {
                    SurfParamType::V
                };
            }
        }
        if along == SurfParamType::Unknown {
            return Ok(Err(SetbackRefusal::not_built(
                "Fillet Error: a setback corner needs each blend's surface to run along its rails",
            )));
        }
        surfaces.push(surface);
        sides.push((along, end, hi.distance(vpoint) < lo.distance(vpoint)));
    }
    let rounds: Vec<Round<'_>> = surfaces
        .iter()
        .zip(&sides)
        .map(|(surface, &(along, end, corner_above))| Round {
            surface,
            along,
            end,
            corner_above,
        })
        .collect();

    let fill = match fill_hole(k, &planes.planes, &rounds, order, tol) {
        Ok(fill) => fill,
        Err(refusal) => {
            return Ok(Err(SetbackRefusal::not_built(refusal.reason.unwrap_or(
                "Fillet Error: the setback corner's hole cannot be filled",
            ))))
        }
    };
    if fill.patches.len() != 2 * n || fill.face.len() != n {
        return Err(err());
    }
    let worst = containment(&fill, &planes.planes, convex)?;
    if worst > tol {
        return Ok(Err(SetbackRefusal::not_built(format!(
            "Fillet Error: the setback corner's fill stands {} past a face it meets",
            fmt_g(worst)
        ))));
    }

    // the base points: interior part q's corner (1, 1) on its face
    for q in 0..n {
        let f = *fill.face.get(q).ok_or_else(nul)?;
        if f >= n {
            return Err(err());
        }
        let part = fill.patches.get(n + q).ok_or_else(nul)?;
        let base = part.evaluate_point(part.natural_uv_domain().max())?;
        let bv = *corner.vertices.get(3 * f + 1).ok_or_else(nul)?;
        pseudo.set_vertex_point(bv, base)?;
        pseudo
            .fillet_vertex_mut(bv)?
            .set_status(FilStatus::Processed);
    }

    // the spring curves' halves: of the setback patches' sides on the faces (v = 0, 1, u
    // from the blend's end to the base point), the one from that rail end to that base point
    for f in 0..n {
        for kk in 0..2 {
            let e = *corner.edges.get(3 * f + kk).ok_or_else(nul)?;
            let rail_end = *corner
                .vertices
                .get(3 * f + if kk == 0 { 0 } else { 2 })
                .ok_or_else(nul)?;
            let base = *corner.vertices.get(3 * f + 1).ok_or_else(nul)?;
            if !pseudo.fillet_vertex(rail_end)?.is_processed() {
                return Err(err());
            }
            let rail_pt = pseudo.vertex_point(rail_end)?;
            let base_pt = pseudo.vertex_point(base)?;
            let mut best = None;
            let mut best_off = BIG_DOUBLE;
            for m in 0..n {
                for ee in 0..2 {
                    if fill.face.get((m + ee) % n).copied() != Some(f) {
                        continue;
                    }
                    let patch = fill.patches.get(m).ok_or_else(nul)?;
                    let dom = patch.natural_uv_domain();
                    let v = if ee == 0 { dom.min().y } else { dom.max().y };
                    let at_end = patch.evaluate_point(Point2::new(dom.min().x, v))?;
                    let inner = patch.evaluate_point(Point2::new(dom.max().x, v))?;
                    let off = smax(at_end.distance(rail_pt), inner.distance(base_pt));
                    if off >= best_off {
                        continue;
                    }
                    let iso = patch.create_iso_parametric_curve(
                        k,
                        SurfParamType::V,
                        v,
                        IsoCurveOptions {
                            domain: Some(dom),
                            uv_iso_curve: false,
                        },
                    )?;
                    best = Some(iso.curve);
                    best_off = off;
                }
            }
            let mut best = best.ok_or_else(nul)?;
            if best_off > tol {
                // the fill's spring side ends where the blend's section meets the face
                return Err(err());
            }
            if kk == 1 {
                // B-S: from the base point to the rail end
                let ivl = best.natural_interval();
                best.reverse_parameterization(&ivl)?;
            }
            set_edge_curve(pseudo.brep_mut(), e, Some(Curve::from(best)))?;
        }
    }
    if let Some(sb) = corner.setback.as_mut() {
        sb.fill = Some(fill);
    }
    Ok(Ok(()))
}

/// `SmBlendCorner::CreateSetBackPatches` (the fork's 61547af): the fill's patches as faces
/// of the fillet B-rep, each bounded by its iso-curves: a setback patch by its end section
/// (`u = 0`), its sides on the faces (`v = 0, 1`) and the two halves of its inner curve
/// (`u = 1`, split at `v = 1/2`, where the spokes meet it); an interior part by its four
/// sides. Each surface joins `corner.surfaces` and its face `corner.surface_faces` (the
/// corner's patch faces for the stitch).
///
/// # Errors
///
/// [`SolidError::null_pointer`] without the fill or the fillet B-rep (`NER`); `SM_ERR`
/// where the fill has not `2 n` patches; those of the curves' and faces' construction.
pub fn create_setback_patches(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_PATCHES);
    let n = corner.all_eus.as_ref().ok_or_else(nul)?.len();
    let fill = corner
        .setback
        .as_mut()
        .and_then(|sb| sb.fill.take())
        .ok_or_else(nul)?;
    if fill.patches.len() != 2 * n {
        return Err(SolidError::sm_err(OP_PATCHES));
    }
    let fillet = env.fillet.as_mut().ok_or_else(nul)?;
    for (p, patch) in fill.patches.into_iter().enumerate() {
        let dom = patch.natural_uv_domain();
        let (u0, u1, v0, v1) = (dom.min().x, dom.max().x, dom.min().y, dom.max().y);
        let vm = 0.5 * (v0 + v1);
        // the loop counterclockwise in (u, v): each side its iso-curve over its natural
        // direction (the constant parameter, its value, the other's interval) and whether
        // the loop runs it backwards
        let mut sides: Vec<(SurfParamType, f64, f64, f64, bool)> =
            vec![(SurfParamType::V, v0, u0, u1, false)];
        if p < n {
            sides.push((SurfParamType::U, u1, v0, vm, false));
            sides.push((SurfParamType::U, u1, vm, v1, false));
        } else {
            sides.push((SurfParamType::U, u1, v0, v1, false));
        }
        sides.push((SurfParamType::V, v1, u0, u1, true));
        sides.push((SurfParamType::U, u0, v0, v1, true));
        let mut curves_3d = Vec::with_capacity(sides.len());
        let mut uv_curves = Vec::with_capacity(sides.len());
        let mut orients = Vec::with_capacity(sides.len());
        for &(iso, at, from, to, back) in &sides {
            let mut sub = Extent2d::new();
            if iso == SurfParamType::V {
                sub.set_min_max(Point2::new(from, at), Point2::new(to, at))?;
            } else {
                sub.set_min_max(Point2::new(at, from), Point2::new(at, to))?;
            }
            let curve = patch
                .create_iso_parametric_curve(
                    env.k,
                    iso,
                    at,
                    IsoCurveOptions {
                        domain: Some(sub),
                        uv_iso_curve: false,
                    },
                )?
                .curve;
            curves_3d.push(Some(Curve::from(curve)));
            let line = Line::create_line_segment(
                2,
                Point3::new(sub.min().x, sub.min().y, 0.0),
                Point3::new(sub.max().x, sub.max().y, 0.0),
                None,
            )?;
            uv_curves.push(Curve::from(line));
            orients.push(if back {
                OrientType::Opposite
            } else {
                OrientType::Same
            });
        }
        let input = FaceCurvesInput {
            loops: vec![curves_3d.len()],
            curves_3d: Some(curves_3d),
            uv_curves: Some(uv_curves),
            orientations: orients,
            loop_points: Vec::new(),
        };
        let surface = Surface::from(patch);
        let region = fillet.brep.infinite_region();
        let key_of = fillet.notify.key_of;
        let face_ref = |f| key_of(EntityRef::Face(f));
        let made = make_face_with_curves(
            env.k,
            fillet.brep,
            region,
            input,
            surface.clone(),
            &dom,
            OrientType::Same,
            None,
            &face_ref,
            &mut fillet.notify,
        )?;
        let face = made.face.ok_or_else(nul)?;
        // the corner's patch faces (SmFilletCorner::GetPatchFaces, the stitch)
        corner.surfaces.push(surface);
        corner.surface_faces.push(face);
    }
    Ok(())
}
