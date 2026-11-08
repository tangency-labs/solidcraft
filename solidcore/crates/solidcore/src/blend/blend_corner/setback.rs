//! The blend corner's Setback: where every edge at a vertex is blended, three or more, each
//! blend stops at its setback from the vertex and the hole they leave is filled by the
//! setback split ([`crate::blend::blend_setback`]), to G1, or to G2 for curvature-continuous
//! sections (or rounds eased into them).
//!
//! Upstream: none (pinned SMLib builds such a vertex as an `NxN` corner). The fork's
//! `source/SMLib/src/SmBlendCornerSetback.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib 61547af, 1506e2f, ac8b4f8, f1d81f4; smlib#104; open corners
//! ef57570, smlib#125).
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
//! | [`SetbackRefusal`]           | `SetBackNotBuilt`, the corner's `NoteError` and `NoteFitSetBack` (its `refusal` as data: solidcore's) |
//! | [`classify_setback`]         | `SmBlendCorner::ClassifySetBack`                      |
//! | [`make_setback_topology`]    | `SmBlendCorner::MakeSetBackTopology`                  |
//! | [`build_setback_fill`]       | `SmBlendCorner::BuildSetBackFill`                     |
//! | [`create_setback_patches`]   | `SmBlendCorner::CreateSetBackPatches`                 |
//! | `open_setback_springs` (private) | `SmBlendCorner::OpenSetBackSprings` (ef57570)     |
//! | `one_weight_polynomial` (private) | `sm_SbOneWeightPolynomial` (ef57570)             |
//! | [`FilletCorner::is_setback_open`] | `SmBlendCorner::IsSetBackOpen` (ef57570)         |
//! | `away_from` (private)        | `sm_SbAwayFrom`                                       |
//! | `corner_planes` (private)    | `sm_SbCornerPlanes`                                   |
//! | `edge_blended` (private)     | `sm_SbEdgeBlended`                                    |
//! | `set_backs` (private)        | `sm_SbSetBacks`                                       |
//! | [`SetbackSolvers`]           | the corner's `SmFilletVertex::CalcSetBackVert` calls (37c9e1d) |
//! | `quadric_face` (private)     | `sm_SbQuadricFace` (37c9e1d)                          |
//! | `corner_quadrics` (private)  | `sm_SbCornerPlanes`'s `pbCurved` (37c9e1d)            |
//! | `along_edge` (private)       | `sm_SbAlongEdge` (37c9e1d)                            |
//! | `curved_least` (private)     | `sm_SbCurvedLeast` (37c9e1d)                          |
//! | `gather_ends` (private)      | `BuildSetBackFill`'s `gather`                         |
//! | `grow_curved` (private)      | `BuildSetBackFill`'s growth beside curved faces (37c9e1d) |
//! | `springs_on_faces`, `springs_refusal` (private) | none (solidcore#2343 (d))          |
//!
//! Deviations: the Setback's own members are kept in [`SetbackCorner`] on the corner
//! ([`FilletCorner::setback`]) from its creation (`SetFillOrder`) until its patches are
//! faces. The fork's corner notes its refusal on the executive itself; here it is returned
//! ([`SetbackRefusal`]) and the session notes it. The executive's set-back fills
//! (`m_vSetBackFills`) are the session's, passed in (WR-697). No recursion. Beside curved faces
//! (solidcore#2343, built in solidcore from the fork's work in progress at 37c9e1d, no fork
//! commit; with `Kernel::fixes.fillet_setback_beside_curved_faces` too): the curved faces
//! are found apart from `corner_planes` (`corner_quadrics`); the
//! fork's `SBSCALE` environment override of the rule is not taken; the executive's
//! `CalcSetBackVert` is reached through [`SetbackSolvers`]; a failing call within a growth
//! trial is that call's error, where the fork's trial counts it as not certified. The
//! corner's side of each blend's end section (`gather_ends`) is the side its edge runs on to
//! the vertex there, from the edge's direction at the setback, not the end of its rails
//! nearer the vertex as the fork's `gather` takes it: a round on an arc, extended past a far
//! corner that does not blend, comes back round nearer the vertex than a long setback's end
//! section (solidcore#2835). Beside a curved face each spring curve is checked to stay inside
//! its trimmed face before the fill is made, and again at setbacks the growth finds; a
//! setback that takes one off its face is refused, naming the largest that keeps every one
//! on its face, or none where none down to the rails' crossing does or the growth's
//! setbacks take one off (solidcore#2343 (d); the fork's ruling checked the curve's parameter-plane
//! control triangle instead, and built no check).
//!
//! [`FilletCorner::setback`]: crate::blend::fillet_corner::p3::FilletCorner::setback

use std::sync::Arc;

use crate::blend::blend_corner::ball::{solver_of_edgeuse_or_mate, BallSolvers};
use crate::blend::blend_corner::on_face::{self, Clearance};
use crate::blend::blend_corner::swept::fmt_g;
use crate::blend::blend_corner::{BlendCorner, BlendCornerEvent};
use crate::blend::blend_refusal::BlendRefusal;
use crate::blend::blend_setback::{
    bisect_thousandths, certified_scale, containment, distances, fill_hole_counted,
    fill_hole_probe, spring_curve, Edge, Face, FaceKind, Fill, FillStats, Probe, Round,
    SpringCurve,
};
use crate::blend::fillet_corner::p1::{face_by_two_edgeuses, test_g1_continuity};
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner, FilletCornerKind};
use crate::blend::fillet_corner::topology::new_corner_vertex;
use crate::blend::fillet_exact::{BlendQuadric, QuadricKind};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilStatus, FilletBrep, FilletCornerId, FilletEdgeType, FilletErrorType,
    FilletGeomId, FilletVertexType,
};
use crate::blend::fillet_geom::p2::set_back_frame;
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolverType, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::vector3d::sqrt;
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p2::IsoCurveOptions;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::procedural_blend::corner::{CornerDefinition, SetbackFill};
use crate::surface::surface::Surface;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::face::p4 as face4;
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
    /// `m_vSetBackRoom` (the fork's 37c9e1d): each blended edge's room, its length less the
    /// far end's claim (what a setback grown beside a curved face is held within). Beside a
    /// curved face the length is the edge's arc length, as the setbacks are measured along
    /// it; between planes (straight edges) the distance between its ends.
    pub room: Vec<f64>,
    /// Not the fork (solidcore#2343 (d)): each blended edge's least setback, where the
    /// rails of the rounds beside it cross (`sm_SbSetBacks`'s `rvLeast`); a setback below it
    /// is too short.
    pub least: Vec<f64>,
    /// `m_pFill`: the fill, from the edge geometry until the patches take it.
    pub fill: Option<Fill>,
    /// `m_bSetBackOpen` (the fork's ef57570, smlib#125): an open G1 corner, its spring
    /// curves made in closed form ([`build_setback_fill`]), no fill and no patches.
    pub open: bool,
    /// Not the fork (#2488, with `fillet_round_as_definition`): the fill's definition, which
    /// its patches keep.
    pub definition: Option<Arc<SetbackFill>>,
}

impl Default for SetbackCorner {
    /// The constructor's: `m_lFillOrder(1)`, `m_bConvexSetBack(TRUE)`,
    /// `m_bSetBackOverflows(FALSE)`, `m_bSetBackOpen(FALSE)`, no setbacks and no fill.
    fn default() -> Self {
        Self {
            setbacks: Vec::new(),
            fill_order: 1,
            convex: true,
            overflows: false,
            room: Vec::new(),
            least: Vec::new(),
            fill: None,
            open: false,
            definition: None,
        }
    }
}

/// [`classify_setback`]'s answer: the corner's event and, for the Setback, its fill's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetbackClass {
    /// [`BlendCornerEvent::Setback`], or [`BlendCornerEvent::None`] where the corner is not one.
    pub event: BlendCornerEvent,
    /// The fill's order (1: G1, 2: G2).
    pub order: usize,
}

/// The blend corner's Setback of fill order `order` at `corner`'s vertex, on its edge uses
/// (`SmFilletCorner::Create`'s N x N case taking the Setback, smlib#104 61547af), its setback
/// and ease the ones set on `corner`.
#[must_use]
pub fn setback_corner_of(corner: &FilletCorner, order: usize) -> FilletCorner {
    let mut setback = FilletCorner::new(
        FilletCornerKind::NByTwo,
        corner.vertex,
        corner.solver_eus.clone(),
        corner.all_eus.clone(),
        corner.approx_tol_3d,
        corner.tangency_tol_radians,
    );
    setback.blend = Some(BlendCorner {
        event: BlendCornerEvent::Setback,
        turn: crate::blend::blend_corner::BlendTurn::Mitre,
        end: crate::blend::blend_corner::BlendEnd::OnEndFace,
    });
    setback.setback = Some(SetbackCorner::with_order(order));
    setback.set_back_dist = corner.set_back_dist;
    setback.ease_dist = corner.ease_dist;
    setback
}

impl FilletCorner {
    /// `SmBlendCorner::IsSetBackOpen` (the fork's ef57570): an open G1 setback corner, its
    /// hole bounded by its sections and spring curves, no patches.
    #[must_use]
    pub fn is_setback_open(&self) -> bool {
        self.blend.is_some_and(BlendCorner::is_setback)
            && self.setback.as_ref().is_some_and(|sb| sb.open)
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

/// Whether `corner` builds a setback it is given with `SetSetBackDist`: the blend corner's
/// Setback (its setbacks, [`SetbackCorner::setbacks`]), or an N x N corner of one convexity
/// that no blend corner took (`SmFilletConvexNxNCorner`'s setback corner, to which
/// `SmFilletConcaveNxNCorner` hands its own). The fork's dc757a6 deletes the N x N setback
/// paths; solidcore keeps them (WR-697 addendum `setback-asked-not-built-refused`), as the
/// Setback does not build every corner they do: a marked corner beside a curved face, and
/// one [`classify_setback`] does not take (variable rounds, mixed convexity, edges tangent at
/// the vertex). Three equal rounds of one convexity between planes, convex or concave, which
/// a set-back vertex keeps from the Ball, are the Setback once a setback is set on their
/// corner (solidcore#2933, classified when the corners are filleted: [`classify_setback`]'s
/// `marked`).
#[must_use]
pub fn builds_setback(corner: &FilletCorner) -> bool {
    match corner.blend {
        Some(blend) => blend.is_setback(),
        None => matches!(
            corner.kind,
            FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN
        ),
    }
}

/// Not the fork's (solidcore-native, WR-697 addendum `setback-asked-not-built-refused`; the
/// fork refuses this only in its capi): whether a setback set on the made `corner`
/// (`SetSetBackDist`, its [`FilletCorner::set_back_dist`]) would be dropped, the corner
/// building none ([`builds_setback`]). The session refuses it ([`SETBACK_NOT_BUILT`], `CornerUnsupported`) instead of building the corner
/// without it, and refuses a `SetSetBackFill` request at such a corner when the corners are
/// made. An N x N corner is never refused, as before the port: it builds the setback set on
/// it, and ignores a `SetSetBackFill` request (which only the Setback reads).
#[must_use]
pub fn asked_setback_not_built(corner: &FilletCorner) -> bool {
    corner.set_back_dist > 0.0 && !builds_setback(corner)
}

/// The refusal's comment for [`asked_setback_not_built`].
pub const SETBACK_NOT_BUILT: &str =
    "Fillet Error: a setback is asked for at a corner that builds none (neither the blend corner's Setback nor an N x N corner), so it would not be built";

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
    /// Not the fork's: the refusal as data, where it is one of a setback asked for that
    /// stops short of the rails' crossing or does not fit on the corner's edges
    /// ([`BlendRefusal::SetbackTooShort`], [`BlendRefusal::SetbackTooLong`]).
    pub refusal: Option<BlendRefusal>,
}

impl SetbackRefusal {
    /// `SetBackNotBuilt(cpWhy)`.
    fn not_built(why: impl Into<String>) -> Self {
        Self {
            error: FilletErrorType::CornerUnsupported,
            comment: why.into(),
            fit_setback: None,
            refusal: None,
        }
    }

    /// This refusal carrying `refusal` as data.
    const fn typed(mut self, refusal: BlendRefusal) -> Self {
        self.refusal = Some(refusal);
        self
    }

    /// `NoteError(.., SM_FILERR_NO_ROOM, cWhy)`.
    fn no_room(why: impl Into<String>, fit_setback: Option<f64>) -> Self {
        Self {
            error: FilletErrorType::NoRoom,
            comment: why.into(),
            fit_setback,
            refusal: None,
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
        planes.push(Face::plane(vpoint, normal));
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

/// `sm_SbQuadricFace` (the fork's 37c9e1d): a curved corner face as the fill sees it, its
/// natural quadric ([`BlendQuadric::from_surface`] at the middle of the face's uv domain);
/// `None` where it is no plane or natural quadric. Its side is settled by the caller
/// ([`corner_quadrics`]); its normal is left zero.
fn quadric_face(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    vpoint: Point3,
    tol: f64,
) -> Result<Option<Face>> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let surface = face2::surface(b, face)?.ok_or_else(nul)?;
    let q = BlendQuadric::from_surface(k, surface, face2::uv_domain(b, face)?.mid(), tol)?;
    let zero = Vec3::new(0.0, 0.0, 0.0);
    let found = match q.kind {
        QuadricKind::Plane => Face::plane(vpoint, zero),
        QuadricKind::Cylinder => Face {
            point: q.point,
            normal: zero,
            kind: FaceKind::Cylinder,
            axis: q.dir,
            radius: q.radius,
            ..Face::default()
        },
        // the apex, the axis into the nappe
        QuadricKind::Cone => Face {
            point: q.point,
            normal: zero,
            kind: FaceKind::Cone,
            axis: q.dir,
            radius: 0.0,
            slope: q.angle.tan(),
            ..Face::default()
        },
        QuadricKind::Sphere => Face {
            point: q.point,
            normal: zero,
            kind: FaceKind::Sphere,
            radius: q.radius,
            ..Face::default()
        },
        _ => return Ok(None),
    };
    Ok(Some(found))
}

/// `sm_SbCornerPlanes` with `pbCurved` (the fork's 37c9e1d): [`corner_planes`]'s, where a
/// face may also be a natural quadric ([`quadric_face`]): its normal out of the material at
/// the vertex, its side the material's (`+1` inside it, where that normal points away from
/// its axis or centre). `None` where a face is neither, a side is undecided, or no face is
/// curved (the corner is [`corner_planes`]'s then).
fn corner_quadrics(
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
    let mut curved = false;
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
        let mut quadric = None;
        let mut normal = if planar.is_planar {
            match planar.normal.unitize() {
                Ok(normal) => normal,
                Err(_) => return Ok(None),
            }
        } else {
            // a natural quadric: its normal at the vertex (its side settled below)
            let Some(found) = quadric_face(k, b, face, vpoint, tol)? else {
                return Ok(None);
            };
            let probe = Face { side: 1.0, ..found };
            let Ok(normal) = probe.distance(vpoint).1.unitize() else {
                return Ok(None);
            };
            quadric = Some(found);
            normal
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
        let flip = (side > 0.0) == convex;
        if flip {
            normal = -normal;
        }
        faces.push(face);
        planes.push(match quadric {
            // the material inside the quadric (+1) where the normal out of it points away
            // from its axis
            Some(found) => {
                curved = true;
                Face {
                    normal,
                    side: if flip { -1.0 } else { 1.0 },
                    ..found
                }
            }
            None => Face::plane(vpoint, normal),
        });
    }
    if !curved {
        return Ok(None);
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

/// The corner's faces as the fill sees them: [`corner_planes`]'s where every face is a
/// plane, else, with `fillet_setback_beside_curved_faces`, [`corner_quadrics`]'s (`true` with
/// them: the corner is beside a curved face).
#[allow(clippy::too_many_arguments)]
fn corner_faces(
    k: &Kernel,
    b: &Brep,
    vtx: VertexId,
    all_eus: &[EdgeuseId],
    solvers: &dyn BallSolvers,
    tol: f64,
    convex: bool,
) -> Result<Option<(CornerPlanes, bool)>> {
    if let Some(planes) = corner_planes(k, b, vtx, all_eus, solvers, tol, convex)? {
        return Ok(Some((planes, false)));
    }
    // beside a curved face only with its own flag: without it such a corner is the corner it
    // was before (#2343)
    if !k.fixes.fillet_setback_beside_curved_faces {
        return Ok(None);
    }
    Ok(corner_quadrics(k, b, vtx, all_eus, solvers, tol, convex)?.map(|q| (q, true)))
}

/// `sm_SbAlongEdge` (the fork's 37c9e1d): how far along `e` from `vertex` the plane square
/// to it holds `c` (Newton on the edge's parameter from the vertex, clamped to its interval,
/// at most 60 steps, to `1e-3` of `tol`), as the edge's arc length from the vertex.
fn along_edge(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    vertex: VertexId,
    c: Point3,
    tol: f64,
) -> Result<Option<f64>> {
    let nul = || SolidError::null_pointer(OP_TOPOLOGY);
    // the curve as its edge's (its length's cache at the edge's tolerance, as the fork's
    // pEdge->GetCurve() reads it through its owner)
    let curve = edge::evaluation_curve(b, e)?.ok_or_else(nul)?;
    let ivl = edge::interval(b, e)?;
    let from_start = edge::start_vertex(b, e)? == Some(vertex);
    let t0 = if from_start { ivl.min() } else { ivl.max() };
    let mut t = t0;
    for _ in 0..60 {
        let d = curve.evaluate(k, t, 2, true, true)?;
        let (Some(&d0), Some(&d1), Some(&d2)) = (d.first(), d.get(1), d.get(2)) else {
            return Err(nul());
        };
        let w = c - d0;
        let f = w.dot(d1);
        let df = -d1.dot(d1) + w.dot(d2);
        if df.is_nan() || df.abs() <= 0.0 {
            return Ok(None);
        }
        let step = f / df;
        t -= step;
        t = smax(ivl.min(), smin(ivl.max(), t));
        if step.abs() * d1.length() <= 1.0e-3 * tol {
            break;
        }
    }
    if (t - t0).abs() == 0.0 {
        return Ok(Some(0.0));
    }
    Ok(Some(curve.length(
        k,
        &Extent1d::from_min_max(smin(t, t0), smax(t, t0)),
        1.0e-3 * tol,
    )?))
}

/// The determinant of a 3 by 3 matrix, by its first row (the fork's own expansion).
fn det3(m: &[[f64; 3]; 3]) -> f64 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
}

/// `sm_SbCurvedLeast` (the fork's 37c9e1d): the least setbacks at a corner beside curved
/// faces. On each face i, where the rails of the two rounds beside it cross: the point P of
/// the face whose ball centres `P - s r n(P)` (s `+1` at a convex corner, `-1` at a concave
/// one) lie at r from each round's other face (Newton on the faces' closed-form distances,
/// their Jacobian by central differences, at most 60 steps, to `1e-13` of the corner's size:
/// its radii, and its distance from the origin, which the distances' rounding grows with);
/// as far along each round's edge as the plane square to it through that ball's centre
/// ([`along_edge`]). Each edge the farther of its two faces'. `None` where a crossing is not
/// found (no convergence, a singular Jacobian, an edge with no Newton step).
#[allow(clippy::too_many_arguments)]
fn curved_least(
    k: &Kernel,
    b: &Brep,
    vtx: VertexId,
    all_eus: &[EdgeuseId],
    faces: &[Face],
    radii: &[f64],
    convex: bool,
    tol: f64,
) -> Result<Option<Vec<f64>>> {
    let nul = || SolidError::null_pointer(OP_TOPOLOGY);
    let n = all_eus.len();
    let mut least = vec![0.0; n];
    let s = if convex { 1.0 } else { -1.0 };
    let vpoint = vertex::point(b, vtx)?;
    for i in 0..n {
        // the rounds beside face i: edge a (faces a, i), edge b (faces i, i + 1)
        let (ea, eb) = ((i + n - 1) % n, i);
        let fi = faces.get(i).ok_or_else(nul)?;
        let fa = faces.get(ea).ok_or_else(nul)?;
        let fb = faces.get((i + 1) % n).ok_or_else(nul)?;
        let ra = radii.get(ea).copied().ok_or_else(nul)?;
        let rb = radii.get(eb).copied().ok_or_else(nul)?;
        let residual = |p: Point3| -> ([f64; 3], Point3, Point3) {
            let (d, normal) = fi.distance(p);
            let ca = p - normal * (s * ra);
            let cb = p - normal * (s * rb);
            (
                [d, fa.distance(ca).0 + s * ra, fb.distance(cb).0 + s * rb],
                ca,
                cb,
            )
        };
        let mut p = vpoint;
        let scale = smax(1.0, smax(ra, rb));
        // the residuals are distances computed from world coordinates: their rounding grows
        // with the corner's distance from the origin, not with its radii alone
        let mut size = smax(scale, vpoint.length());
        for f in [fi, fa, fb] {
            size = smax(size, f.point.length());
        }
        let mut done = None;
        for _ in 0..60 {
            let (f, ca, cb) = residual(p);
            if f[0].abs() + f[1].abs() + f[2].abs() <= 1.0e-13 * size {
                done = Some((ca, cb));
                break;
            }
            // the Jacobian by central differences of the closed-form distances
            let mut jac = [[0.0; 3]; 3];
            let h = 1.0e-6 * scale;
            for kk in 0..3 {
                let mut dv = Vec3::new(0.0, 0.0, 0.0);
                match kk {
                    0 => dv.x = h,
                    1 => dv.y = h,
                    _ => dv.z = h,
                }
                let (fp, _, _) = residual(p + dv);
                let (fm, _, _) = residual(p - dv);
                for (row, (&a, &bm)) in jac.iter_mut().zip(fp.iter().zip(&fm)) {
                    if let Some(slot) = row.get_mut(kk) {
                        *slot = (a - bm) / (2.0 * h);
                    }
                }
            }
            let det = det3(&jac);
            if det.is_nan() || det.abs() <= EFF_ZERO {
                return Ok(None);
            }
            let mut x = [0.0; 3];
            for (kk, xk) in x.iter_mut().enumerate() {
                let mut m = jac;
                for (row, &fr) in m.iter_mut().zip(&f) {
                    if let Some(slot) = row.get_mut(kk) {
                        *slot = fr;
                    }
                }
                *xk = det3(&m) / det;
            }
            let [x0, x1, x2] = x;
            p -= Vec3::new(x0, x1, x2);
        }
        let Some((ca, cb)) = done else {
            return Ok(None);
        };
        let edge_a = eu1::edge(b, *all_eus.get(ea).ok_or_else(nul)?)?.ok_or_else(nul)?;
        let edge_b = eu1::edge(b, *all_eus.get(eb).ok_or_else(nul)?)?.ok_or_else(nul)?;
        let (Some(da), Some(db)) = (
            along_edge(k, b, edge_a, vtx, ca, tol)?,
            along_edge(k, b, edge_b, vtx, cb, tol)?,
        ) else {
            return Ok(None);
        };
        let la = least.get_mut(ea).ok_or_else(nul)?;
        *la = smax(*la, da);
        let lb = least.get_mut(eb).ok_or_else(nul)?;
        *lb = smax(*lb, db);
    }
    Ok(Some(least))
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
/// length (beside a curved face its arc length, as its setbacks are), less the far end's claim: an asked-for setback with its ease there, or, where the
/// far end is itself a vertex whose three or more edges are all blended (a corner that may
/// set back too), half the edge. Asked for: the asked setback on every edge, refused (`fits`
/// false) unless it and its ease lie below every room, `largest` then the largest that fits
/// (the least room less the ease); and below an edge's contact-line minimum (`short`). Else
/// the rule's ([`distances`]), each shrunk to its room less a quarter of its radius where
/// longer, as long as that is at or above its minimum (`fits` false where not: the rounds
/// beside an edge cross past its room, an overflow). `Err(reason)` where [`distances`]
/// refuses. Beside a curved face (`curved`; the fork's 37c9e1d): the least setbacks where
/// the rails cross on the faces as they are ([`curved_least`], refused where they cannot be
/// found), and the rule's `max(1.5 r, least + 0.25 r)` from them ([`distances`]'s rule).
#[allow(clippy::too_many_arguments)]
fn set_backs(
    k: &Kernel,
    b: &Brep,
    vtx: VertexId,
    all_eus: &[EdgeuseId],
    solvers: &dyn BallSolvers,
    fills: &[SetBackFill],
    planes: &[Face],
    edges: &[Edge],
    tol: f64,
    curved: bool,
    convex: bool,
) -> Result<core::result::Result<SetBacks, Option<&'static str>>> {
    let nul = || SolidError::null_pointer(OP_TOPOLOGY);
    let n = all_eus.len();
    let (mut setbacks, least) = if curved {
        let radii: Vec<f64> = edges.iter().map(|e| e.radius).collect();
        let Some(least) = curved_least(k, b, vtx, all_eus, planes, &radii, convex, tol)? else {
            return Ok(Err(Some(
                "Fillet Error: where the rails of the rounds at a setback corner cross on its curved faces cannot be found",
            )));
        };
        let setbacks = least
            .iter()
            .zip(&radii)
            .map(|(&l, &r)| smax(1.5 * r, l + 0.25 * r))
            .collect();
        (setbacks, least)
    } else {
        match distances(planes, edges) {
            Ok(rule) => (rule.setbacks, rule.least),
            Err(refusal) => return Ok(Err(refusal.reason)),
        }
    };
    let asked = set_back_fill_of(fills, vtx);
    let mut room = vec![0.0; n];
    let mut fits = true;
    let mut short = false;
    let mut largest = BIG_DOUBLE;
    for (i, &eu) in all_eus.iter().enumerate() {
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        let length = if curved {
            // beside a curved face the edges may be arcs: the setbacks are lengths along
            // them (along_edge, CalcSetBackVert), and so is the room they are held within
            let curve = edge::evaluation_curve(b, e)?.ok_or_else(nul)?;
            curve.length(k, &edge::interval(b, e)?, 1.0e-3 * tol)?
        } else {
            let start = edge::start_vertex(b, e)?.ok_or_else(nul)?;
            let end = edge::end_vertex(b, e)?.ok_or_else(nul)?;
            vertex::point(b, start)?.distance(vertex::point(b, end)?)
        };
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
/// every face there a plane, every edge a straight line (or, solidcore#2343 from the fork's
/// 37c9e1d, a face a natural quadric, any edges, every blend a constant circular round, the
/// fill G1), no two edges tangent, all of one
/// convexity, each blend a constant round of one radius (`SM_FSG_CIRCULAR`) or the
/// curvature-continuous section (`SM_FSG_BLEND_CURVE`), and either set back on request
/// (`fills`, `GetSetBackFill`), or G2 (curvature-continuous sections), or rounds of unequal
/// radii, which no one ball touches. Equal circular rounds stay where they were
/// ([`BlendCornerEvent::None`]). The fill's order with it: 2 for curvature-continuous
/// sections or a setback with an ease, else 1.
///
/// Not the fork (solidcore#2933): `marked` says the fill asked at the vertex is its mark's
/// (`AddSetBackVertex`, the distance and ease set on the made corner with `SetSetBackDist`
/// and `SetEaseDist`), which the session reads as one when the corners are filleted: three
/// equal rounds there are then the Setback with its certified fill, not the N x N corner's
/// `N_FillNSidedHole` patch. Beside a curved face such a corner stays the N x N corner, as
/// before.
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
    marked: bool,
) -> Result<SetbackClass> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let none = Ok(SetbackClass {
        event: BlendCornerEvent::None,
        order: 1,
    });
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

    // the edges: of one convexity, no two tangent at the vertex; straight between planes
    // (beside a curved face, any edge)
    let mut concave0 = false;
    let mut straight = true;
    for (i, &eu) in all_eus.iter().enumerate() {
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        let Some(curve) = edge::curve(b, e)? else {
            return none;
        };
        if curve.is_line(k, 10, tol)?.is_none() {
            straight = false;
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

    // the faces: planes, or natural quadrics (solidcore#2343, the fork's 37c9e1d)
    let Some((_, curved)) = corner_faces(k, b, vtx, all_eus, solvers, tol, !concave0)? else {
        return none;
    };
    if !curved && !straight {
        return none;
    }
    // beside a curved face only the G1 fill of constant circular rounds (smlib#85)
    if curved && (kind != FilletSurfaceGeneratorType::Circular || !one_kind) {
        return none;
    }
    // a mark's fill beside a curved face is not read (the N x N corner's, solidcore#2343)
    if curved && marked {
        return none;
    }

    let asked = set_back_fill_of(fills, vtx);
    if asked.is_none() && !one_kind {
        return none;
    }
    // a corner left open (`open`) is the Setback's hole: its fill is built uncertified and
    // left out by the caller (the fork's ef57570). Beside a curved face the open hole's
    // spring curves (parabolas tangent to straight rails on planes) are not defined: such a
    // corner keeps the patch its caller removes, as before.
    if curved && asked.is_some_and(|a| a.open) {
        return none;
    }
    let g2 = one_kind && kind == FilletSurfaceGeneratorType::BlendCurve;
    // three rounds of one radius meet a common ball (the Ball): not a setback unless asked.
    // Four or more meet one only where every face's offset passes through one point
    // (ClassifyBall, first); elsewhere they are set back (smlib#125 278bf77).
    if asked.is_none() && !g2 && max_r - min_r <= tol && n == 3 {
        return none;
    }
    // (setbacks that do not fit are the Setback's to refuse, or the overflow's:
    // MakeSetBackTopology)
    let order = if g2 || asked.is_some_and(|a| a.ease > 0.0) {
        2
    } else {
        1
    };
    if curved && order != 1 {
        return Ok(SetbackClass {
            event: BlendCornerEvent::None,
            order,
        });
    }
    Ok(SetbackClass {
        event: BlendCornerEvent::Setback,
        order,
    })
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
    let Some((planes, curved)) = corner_faces(k, target, vtx, &all, solvers, tol, sb.convex)?
    else {
        return Err(err()); // ClassifySetBack found them
    };

    // the setbacks. Where the rule's do not fit (the rounds beside an edge cross past its
    // room) the blends overflow their faces: the executive's overflow comes first, and the
    // corner, made meanwhile with each setback at half its room (only so that its vertices
    // can be placed), refuses in BuildSetBackFill if it is ever built.
    let found = set_backs(
        k,
        target,
        vtx,
        &all,
        solvers,
        fills,
        &planes.planes,
        &planes.edges,
        tol,
        curved,
        sb.convex,
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
            return Ok(Err(setback_too_long(
                a,
                vtx,
                largest,
                "does not fit on the corner's edges",
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
            ))
            .typed(BlendRefusal::SetbackTooShort {
                vertex: vtx,
                least: most,
            })));
        }
        corner.ease_dist = a.ease;
    }
    sb.overflows = !fits;
    sb.room.clone_from(&room);
    sb.least.clone_from(&least);
    // a G1 fill is taken only certified convex: the least scale of the setbacks at which it
    // is (CertifiedScale). The rule's grow to it, each held at its room less the rule's
    // quarter radius (the others growing on); refused, naming them, where none within the
    // rooms certifies. One asked for that is too short is refused, naming the setback that
    // builds (or that none does, where that one does not fit with its ease). Beside a curved
    // face they grow with the real rounds instead (build_setback_fill). A corner left open
    // is not filled: its fill is not certified, nor its setbacks grown (the fork's ef57570).
    if sb.fill_order == 1 && !sb.overflows && !curved && !asked.is_some_and(|a| a.open) {
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

/// What the Setback's fill reads of the executive's solvers ([`BallSolvers`]) and, to grow
/// its setbacks beside curved faces (solidcore#2343), the executive's placing of its setback
/// vertices again.
pub trait SetbackSolvers: BallSolvers {
    /// `SmFilletVertex::CalcSetBackVert` of `corner`'s setback vertex `fv`, at the setbacks
    /// `corner` holds ([`SetbackCorner::setbacks`]); its mate gets the matching point.
    ///
    /// # Errors
    /// Those of the executive's `CalcSetBackVert`.
    fn calc_set_back_vert(
        &mut self,
        k: &Kernel,
        target: &Brep,
        pseudo: &mut FilletBrep,
        corner: &FilletCorner,
        fv: VertexId,
    ) -> Result<()>;
}

/// Each blend's surface at its end section, and its rails' direction on it, the rail
/// parameter of its setback vertex and on which side the corner lies.
type Ends = (Vec<Surface>, Vec<(SurfParamType, f64, bool)>);

/// The blends at their end sections as `gather` (the fork's lambda in `BuildSetBackFill`)
/// finds them: each one's surface, and its rails' direction, the rail parameter of its
/// setback vertex and on which side the corner lies.
enum Gathered {
    /// The blends' ends.
    Ends(Vec<Surface>, Vec<(SurfParamType, f64, bool)>),
    /// Refused, with its reason (`pGatherWhy`).
    Refused(&'static str),
    /// A setback vertex not on its rail (`DropPoint` found none): the fork's `SM_ERR`, a
    /// trial not placed while the setbacks grow.
    Unplaced,
    /// A setback that, with its ease, reaches past where a far corner cuts a round's rail.
    PastRails {
        /// The least over the corner's edges of how far along its edge each round's rails
        /// reach from the vertex, less the fit margin: a setback that with its ease is no
        /// longer fits.
        reach: f64,
    },
}

/// The fork's `gather` in `BuildSetBackFill`: each blend at its end section (its surface,
/// its rails' direction on it, the rail parameter of its setback vertex, and on which side
/// the corner lies). Two rules differ from the fork's (solidcore#2835):
/// - the corner lies on the side of the end section that the blend's edge runs on to the
///   vertex (the rail's direction there against the edge's at the setback, in the frame
///   `CalcSetBackVert` places the setback vertex in, `set_back_frame`), where the fork took
///   the end of the rails nearer the vertex;
/// - a setback that with its ease reaches as far along its edge as a far corner cuts one of
///   its round's rails (the rail's far vertex), less the fit margin, is
///   [`Gathered::PastRails`].
#[allow(clippy::too_many_arguments)]
fn gather_ends(
    k: &Kernel,
    target: &Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn BallSolvers,
    faces: &[FaceId],
    tol: f64,
) -> Result<Gathered> {
    let nul = || SolidError::null_pointer(OP_FILL);
    let err = || SolidError::sm_err(OP_FILL);
    let vtx = corner.vertex.ok_or_else(nul)?;
    let n = faces.len();
    let mut surfaces: Vec<Surface> = Vec::with_capacity(n);
    let mut sides: Vec<(SurfParamType, f64, bool)> = Vec::with_capacity(n);
    let mut reach = BIG_DOUBLE;
    let mut past = false;
    for (i, &face) in faces.iter().enumerate() {
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
            return Ok(Gathered::Unplaced);
        }
        let end = dropped.parameter;
        let on_rail = rail_curve.evaluate_point(k, end)?;
        // the corner's side of the end section: the side the blend's edge runs on to the
        // vertex. Which end of the rails lies nearer the vertex does not tell it: a round on
        // an arc, extended past a far corner that does not blend, comes back round toward the
        // vertex, nearer it than the end section of a long setback.
        let rail_dir = rail_curve
            .evaluate(k, end, 1, true, false)?
            .get(1)
            .copied()
            .ok_or_else(nul)?;
        let e = eu1::edge(target, fs.edgeuse(0)?.ok_or_else(nul)?)?.ok_or_else(nul)?;
        let setback = corner.set_back_dist_along(target, e)?;
        let (_, edge_dir) = set_back_frame(k, target, e, Some(vtx), setback, OP_FILL)?;
        let from_start = edge::start_vertex(target, e)? == Some(vtx);
        let toward_vertex = if from_start { -edge_dir } else { edge_dir };
        let side = rail_dir.dot(toward_vertex);
        // Only the sign is read. A rail runs along its edge, and the end section is square
        // to the edge, so the product is about the rail's speed; exactly zero (a rail square
        // to its edge) or not finite is no round along the edge.
        if !side.is_finite() || side.abs() <= 0.0 {
            return Err(err());
        }
        // How far along the edge the round's rails reach from the vertex: each rail's far
        // vertex, where a far corner cuts the round, set square onto the edge (the plane
        // `CalcSetBackVert` sets the setback vertex in, there). The bound is open: a setback
        // at it leaves the round no length past its end section. So, as docs/generic-blend.md
        // §3.5 asks, the reach is the cut less the fit margin, a thousand of the edge's
        // tolerances, and the size named there builds (the room `set_backs` gives an edge is
        // closed instead, refused within `tol` of it). Lengths along the edge are measured to
        // a thousandth of `tol`, as `set_backs` measures the room and `along_edge` a setback.
        let edge_curve = edge::evaluation_curve(target, e)?.ok_or_else(nul)?;
        let edge_ivl = edge::interval(target, e)?;
        let margin = 1000.0 * edge::tolerance(target, e)?;
        let t0 = if from_start {
            edge_ivl.min()
        } else {
            edge_ivl.max()
        };
        for j in 0..2 {
            let Some(r) = pseudo.geom(fg)?.rail(j) else {
                continue;
            };
            let ends = [
                edge::start_vertex(pseudo.brep(), r)?,
                edge::end_vertex(pseudo.brep(), r)?,
            ];
            for far in ends.into_iter().flatten() {
                // not the corner's own vertices; and a far vertex not yet placed has no
                // point to measure, so a cut there is not seen here
                if corner.vertices.contains(&far) || !pseudo.fillet_vertex(far)?.is_processed() {
                    continue;
                }
                let foot = edge_curve.drop_point(
                    k,
                    &edge_ivl,
                    pseudo.vertex_point(far)?,
                    None,
                    tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                // A vertex with no foot on the edge, or whose foot is the vertex's own end (the
                // drop clamped there: it lies behind the corner, not along the edge), cuts no
                // part of the edge between the corner and its setback: not measured, as one not
                // yet placed. Exact: only a clamped foot is that end.
                if !foot.success || foot.parameter == t0 {
                    continue;
                }
                let t = foot.parameter;
                let length = edge_curve.length(
                    k,
                    &Extent1d::from_min_max(smin(t, t0), smax(t, t0)),
                    1.0e-3 * tol,
                )?;
                let fits = length - margin;
                reach = smin(reach, fits);
                if setback + corner.ease_dist > fits + tol {
                    past = true;
                }
            }
        }
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
            return Ok(Gathered::Refused(
                "Fillet Error: a setback corner needs each blend's surface to run along its rails",
            ));
        }
        surfaces.push(surface);
        sides.push((along, end, side > 0.0));
    }
    if past {
        return Ok(Gathered::PastRails { reach });
    }
    Ok(Gathered::Ends(surfaces, sides))
}

/// The fill's rounds from the blends' ends.
fn rounds_of<'s>(surfaces: &'s [Surface], sides: &[(SurfParamType, f64, bool)]) -> Vec<Round<'s>> {
    surfaces
        .iter()
        .zip(sides)
        .map(|(surface, &(along, end, corner_above))| Round {
            surface,
            along,
            end,
            corner_above,
        })
        .collect()
}

/// What the growth of a Setback's setbacks beside curved faces works on.
struct Growth<'a> {
    k: &'a Kernel,
    target: &'a Brep,
    planes: &'a CornerPlanes,
    tol: f64,
    /// The setbacks it grows from (the rule's, or the one asked for).
    base: Vec<f64>,
    /// The most each may grow to: its room less the rule's quarter radius (unbounded where
    /// asked for).
    most: Vec<f64>,
}

impl Growth<'_> {
    /// The fork's `place`: the setbacks at `scale` times the base (each held at its most),
    /// the setback vertices placed there (`CalcSetBackVert`), and the blends' ends gathered
    /// there; `None` where a vertex is not placed or the ends are not gathered.
    fn place<S: SetbackSolvers>(
        &self,
        pseudo: &mut FilletBrep,
        corner: &mut FilletCorner,
        solvers: &mut S,
        scale: f64,
    ) -> Result<Option<Ends>> {
        let nul = || SolidError::null_pointer(OP_FILL);
        let sb = corner.setback.as_mut().ok_or_else(nul)?;
        let mut dist: f64 = 0.0;
        for ((s, &base), &most) in sb.setbacks.iter_mut().zip(&self.base).zip(&self.most) {
            *s = smin(scale * base, most);
            dist = smax(dist, *s);
        }
        corner.set_back_dist = dist;
        for i in 0..self.base.len() {
            let s = *corner.vertices.get(3 * i + 2).ok_or_else(nul)?;
            solvers.calc_set_back_vert(self.k, self.target, pseudo, corner, s)?;
            if !pseudo.fillet_vertex(s)?.is_processed() {
                return Ok(None);
            }
        }
        match gather_ends(
            self.k,
            self.target,
            pseudo,
            corner,
            &*solvers,
            &self.planes.faces,
            self.tol,
        )? {
            Gathered::Ends(surfaces, sides) => Ok(Some((surfaces, sides))),
            Gathered::Refused(_) | Gathered::Unplaced | Gathered::PastRails { .. } => Ok(None),
        }
    }

    /// The fork's `trial`: the fill at `scale` times the base (`FillHoleProbe`, from the
    /// least degree up); `None` where its vertices are not placed or it is refused. A failing
    /// call within it is that call's error.
    fn trial<S: SetbackSolvers>(
        &self,
        pseudo: &mut FilletBrep,
        corner: &mut FilletCorner,
        solvers: &mut S,
        scale: f64,
    ) -> Result<Option<Fill>> {
        let Some((surfaces, sides)) = self.place(pseudo, corner, solvers, scale)? else {
            return Ok(None);
        };
        let rounds = rounds_of(&surfaces, &sides);
        // certified by more than its rounding, so that the setback the growth names builds
        // when asked for (#2243)
        let mut probe = Probe {
            fill: true,
            rounding_margin: true,
            ..Probe::default()
        };
        match fill_hole_probe(
            self.k,
            &self.planes.planes,
            &rounds,
            self.tol,
            &mut probe,
            &mut FillStats::default(),
        ) {
            Ok(fill) => Ok(Some(fill)),
            Err(refusal) if refusal.reason.is_some() => Ok(None),
            Err(refusal) => Err(refusal.error),
        }
    }
}

/// The fork's growth in `BuildSetBackFill` (37c9e1d; #85 C, the owner's section 28): beside
/// a curved face, the rule's setbacks, or the one asked for, grow to the least scale at which
/// the fill is certified, with the real rounds: each trial moves the setback vertices
/// ([`SetbackSolvers::calc_set_back_vert`] at the scaled setbacks, each held at its room less
/// a quarter radius) and fills the hole from the least degree up. Twice them first, then the
/// planar search's bisection on the grid of thousandths (`certified_scale`, the fork's
/// 20b1c01, where 37c9e1d bisected to `1e-2`): the smallest certifying scale whose neighbour
/// below fails, in at most 11 trials. A trial is certified only by more than its certificate's
/// rounding bound (`Probe::rounding_margin`, #2243), so that the corner built afresh there,
/// as the caller builds it or a caller asking that setback would, is certified too. None up
/// to twice them: refused, naming none. One asked for that certifies only grown: refused,
/// naming the setback that builds (or none, where that one does not fit with its ease).
/// Returns the blends' ends where the setbacks found put them.
fn grow_curved<S: SetbackSolvers>(
    growth: &Growth<'_>,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &mut S,
    asked: Option<SetBackFill>,
) -> Result<core::result::Result<Ends, SetbackRefusal>> {
    let err = || SolidError::sm_err(OP_FILL);
    if growth.trial(pseudo, corner, solvers, 2.0)?.is_none() {
        let why = if let Some(a) = asked {
            format!(
                "Fillet Error: no setback up to twice {} keeps the setback corner's fill no more concave than its faces",
                fmt_g(a.setback)
            )
        } else {
            "Fillet Error: no setbacks up to twice the rule's within the corner's edges keep its fill no more concave than its faces".to_owned()
        };
        return Ok(Err(SetbackRefusal::no_room(why, Some(0.0))));
    }
    // The planar search's (`certified_scale`, #2382, the fork's 20b1c01): the smallest
    // certifying scale on the grid of thousandths whose neighbour below fails, by bisection
    // from 1 (the fill at the setbacks themselves, refused) and 2.
    let hi = bisect_thousandths(1000, 2000, |i| {
        let at = growth.trial(pseudo, corner, solvers, f64::from(i) / 1000.0)?;
        Ok::<bool, SolidError>(at.is_some())
    })?;
    let hi = f64::from(hi) / 1000.0;
    let Some(ends) = growth.place(pseudo, corner, solvers, hi)? else {
        return Err(err());
    };
    // the setbacks the fill needs keep every spring curve on its face, or none is taken
    // (#2343 (d)): the growth finds no smaller one certified, and a spring curve off its face
    // at these is taken to stay off it at every larger one (as the growth takes
    // certification, checked on the cases, not proved)
    let springs = springs_on_faces(
        growth.k,
        growth.target,
        pseudo,
        corner,
        growth.planes,
        growth.tol,
    )?;
    if springs == Springs::Leave {
        return Ok(Err(SetbackRefusal::no_room(
            format!(
                "Fillet Error: the setback corner's fill stays no more concave than its faces only from {} times its setbacks, where a spring curve is not shown to stay on its face: no setback builds",
                fmt_g(hi)
            ),
            Some(0.0),
        )));
    }
    if let Some(a) = asked {
        let room = &corner.setback.as_ref().ok_or_else(err)?.room;
        let fits_room = room.iter().all(|&r| hi * a.setback + a.ease < r);
        let builds = if fits_room { hi * a.setback } else { 0.0 };
        let why = if fits_room {
            format!(
                "Fillet Error: a setback of {} is too short for the setback corner's fill to stay no more concave than its faces: a setback of {} builds it",
                fmt_g(a.setback),
                fmt_g(builds)
            )
        } else {
            format!(
                "Fillet Error: a setback of {} is too short for the setback corner's fill to stay no more concave than its faces, and the {} that would be does not fit on its edges",
                fmt_g(a.setback),
                fmt_g(hi * a.setback)
            )
        };
        return Ok(Err(SetbackRefusal::no_room(why, Some(builds))));
    }
    Ok(Ok(ends))
}

/// Where a Setback's spring curves stand on its faces ([`springs_on_faces`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Springs {
    /// Each proved to keep clear of every other edge of its face.
    Clear,
    /// One comes within its gap of another edge of its face, or is not shown not to.
    Leave,
    /// One is not made: [`spring_curve`] refuses the rails' ends (the fill, which makes it
    /// from the same ends, says why).
    Unmade,
}

/// The derivative along `v` of `face`'s unit normal field ([`Face::distance`]'s normal, out
/// of the material, at any point) at `x`: zero on a plane; a sphere's `side (v - (v . u) u) /
/// |x - c|` (u the unit direction from its centre); a cylinder's or cone's `side cos v_t /
/// rho`, `v_t` the part of `v` along the circle about the axis through `x`, rho that circle's
/// radius and `cos` that of the cone's half angle (1 on a cylinder). `None` on a quadric's
/// centre or axis.
fn normal_turn(face: &Face, x: Point3, v: Vec3) -> Option<Vec3> {
    let u = x - face.point;
    match face.kind {
        FaceKind::Plane => Some(Vec3::ZERO),
        FaceKind::Sphere => {
            let len = u.length();
            let d = u.unitize().ok()?;
            Some((v - d * v.dot(d)) * (face.side / len))
        }
        FaceKind::Cylinder | FaceKind::Cone => {
            let r = u - face.axis * u.dot(face.axis);
            let rho = r.length();
            let d = r.unitize().ok()?;
            let along = v - d * v.dot(d) - face.axis * v.dot(face.axis);
            let cos = 1.0 / sqrt(1.0 + face.slope * face.slope);
            Some(along * (face.side * cos / rho))
        }
    }
}

/// The unit direction of the rail on `face` of the round of radius `r` along `edge_id`, at
/// its end `p` on `face` whose other end is `q` on `other` (the round's two contact points in
/// one section), turned toward the corner's vertex (`sign` `+1` at a convex corner, `-1` at a
/// concave one). In closed form, not from the rail as traced, which runs only from the
/// setback the corner was made at outward: the ball's centre is `c = p - sign r n(p)` and
/// runs along `n(p) x m(q)` (n, m the faces' normals, constant along their normal lines), the
/// rail is `c + sign r n(c)`, so its direction is that of the centre's plus `sign r` times the
/// normal's turn along it ([`normal_turn`]). Toward the vertex: the side the edge runs on to
/// the vertex at its setback (as `gather_ends` takes the corner's side of an end section).
/// `None` where the faces' normals are parallel there, or a normal's turn is not defined.
#[allow(clippy::too_many_arguments)]
fn rail_toward_vertex(
    k: &Kernel,
    target: &Brep,
    corner: &FilletCorner,
    edge_id: EdgeId,
    face: &Face,
    p: Point3,
    other: &Face,
    q: Point3,
    r: f64,
    sign: f64,
) -> Result<Option<Vec3>> {
    let nul = || SolidError::null_pointer(OP_FILL);
    let vtx = corner.vertex.ok_or_else(nul)?;
    let (_, n) = face.distance(p);
    let (_, m) = other.distance(q);
    let centre = p - n * (sign * r);
    let along = n.cross(m);
    let Some(turn) = normal_turn(face, centre, along) else {
        return Ok(None);
    };
    let Ok(t) = (along + turn * (sign * r)).unitize() else {
        return Ok(None);
    };
    let setback = corner.set_back_dist_along(target, edge_id)?;
    let (_, edge_dir) = set_back_frame(k, target, edge_id, Some(vtx), setback, OP_FILL)?;
    let from_start = edge::start_vertex(target, edge_id)? == Some(vtx);
    let side = t.dot(if from_start { -edge_dir } else { edge_dir });
    if !side.is_finite() || side.abs() <= 0.0 {
        return Ok(None);
    }
    Ok(Some(if side > 0.0 { t } else { -t }))
}

/// The corner's spring curves, one per face, at the setbacks its vertices are placed at, as
/// the fill makes them ([`spring_curve`]): on face `F_i`, from `S_i`, the end of the blend
/// after `F_i`, leaving its rail toward the vertex, to `M_i`, the end of the blend before,
/// tangent to both rails ([`rail_toward_vertex`], each with the blend's other end, on its
/// other face, the mate of its end on `F_i`). [`Springs::Leave`] where a rail's direction is
/// not found (not shown to stay on its face); [`Springs::Unmade`] where [`spring_curve`]
/// refuses the rails' ends.
fn corner_springs(
    k: &Kernel,
    target: &Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    planes: &CornerPlanes,
) -> Result<core::result::Result<Vec<SpringCurve>, Springs>> {
    let nul = || SolidError::null_pointer(OP_FILL);
    let all = corner.all_eus.as_ref().ok_or_else(nul)?;
    let sign = if corner.setback.as_ref().ok_or_else(nul)?.convex {
        1.0
    } else {
        -1.0
    };
    let n = planes.faces.len();
    let point = |i: usize| -> Result<Point3> {
        pseudo.vertex_point(*corner.vertices.get(i).ok_or_else(nul)?)
    };
    let mut springs = Vec::new();
    for (f, plane) in planes.planes.iter().enumerate() {
        let before = (f + n - 1) % n;
        let mut ends = [(Point3::ZERO, None); 2];
        // S_f with its mate M_(f+1) on F_(f+1); M_f with its mate S_(f-1) on F_(f-1)
        for (slot, (blend, at, mate, other)) in ends.iter_mut().zip([
            (f, 3 * f + 2, 3 * ((f + 1) % n), (f + 1) % n),
            (before, 3 * f, 3 * before + 2, before),
        ]) {
            let e = eu1::edge(target, *all.get(blend).ok_or_else(nul)?)?.ok_or_else(nul)?;
            let p = point(at)?;
            let tangent = rail_toward_vertex(
                k,
                target,
                corner,
                e,
                plane,
                p,
                planes.planes.get(other).ok_or_else(nul)?,
                point(mate)?,
                planes.edges.get(blend).ok_or_else(nul)?.radius,
                sign,
            )?;
            *slot = (p, tangent);
        }
        let [(p, Some(tp)), (q, Some(tq))] = ends else {
            return Ok(Err(Springs::Leave));
        };
        let Ok(spring) = spring_curve(plane, p, tp, q, -tq) else {
            return Ok(Err(Springs::Unmade));
        };
        springs.push(spring);
    }
    Ok(Ok(springs))
}

/// solidcore#2343 (d), the owner's ruling B there: whether the corner's spring curves
/// ([`corner_springs`]) stay inside their trimmed faces. Each starts inside its face, so it
/// stays inside where it keeps clear of every edge of the face but the two blended edges at
/// the vertex ([`on_face::clear_of`]: by more than the corner's tolerance and the edge's). The
/// two blended edges are left out whole: between them the spring curve lies past the rails of
/// their rounds, which run inside the face along them. An edge without a curve, or whose
/// curve is not built on a B-spline, gives no hull to bound it by: the spring curve is not
/// shown clear of it.
fn springs_on_faces(
    k: &Kernel,
    target: &Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    planes: &CornerPlanes,
    tol: f64,
) -> Result<Springs> {
    let nul = || SolidError::null_pointer(OP_FILL);
    let vtx = corner.vertex.ok_or_else(nul)?;
    let springs = match corner_springs(k, target, pseudo, corner, planes)? {
        Ok(springs) => springs,
        Err(verdict) => return Ok(verdict),
    };
    for (&face, spring) in planes.faces.iter().zip(&springs) {
        let mut seen = Vec::new();
        for e in face4::edges(target, face, None, false, None)? {
            if seen.contains(&e) {
                continue;
            }
            seen.push(e);
            if edge::start_vertex(target, e)? == Some(vtx)
                || edge::end_vertex(target, e)? == Some(vtx)
            {
                continue;
            }
            let Some(curve) = edge::curve(target, e)? else {
                return Ok(Springs::Leave);
            };
            let Some(pieces) = on_face::bezier_pieces(k, curve)? else {
                return Ok(Springs::Leave);
            };
            let gap = tol + edge::tolerance(target, e)?;
            for half in [&spring.from_p.curve, &spring.from_q.curve] {
                if on_face::clear_of(k, half, &pieces, gap)? == Clearance::Near {
                    return Ok(Springs::Leave);
                }
            }
        }
    }
    Ok(Springs::Clear)
}

/// What a setback that takes a spring curve off its face does, for [`setback_too_long`].
const SPRING_LEAVES: &str =
    "takes a setback corner's spring curve off its face (it comes within the tolerances of another edge of the face, or is not shown not to)";

/// solidcore#2343 (d): where a spring curve leaves its face at the corner's setbacks
/// ([`springs_on_faces`]), the refusal naming the largest common scale of them on the grid of
/// thousandths at which every spring curve stays inside its face. It is found by the growth's
/// bisection ([`bisect_thousandths`], each trial placing the setback vertices again as the
/// growth does), between 1 and the least scale at which every setback reaches its edge's
/// least, where the rails cross; the search assumes a spring curve on its face at a scale
/// stays on it at every smaller one (the curve shrinks toward the vertex with its setbacks),
/// as the growth assumes certification monotone: checked on the cases, not proved. One asked
/// for is refused as [`BlendRefusal::SetbackTooLong`] naming that scale times it; the
/// rule's, naming those setbacks. None at or above the least: no setback builds. A trial
/// whose vertices are not placed counts as off its face, so the scale named is at worst
/// smaller than need be, never one off its face; whether the fill certifies there is the
/// fill's to say when that setback is asked. `None` where every spring curve stays on its
/// face, or one is not made (the fill says why).
#[allow(clippy::too_many_arguments)]
fn springs_refusal<S: SetbackSolvers>(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &mut S,
    planes: &CornerPlanes,
    fills: &[SetBackFill],
    tol: f64,
) -> Result<Option<SetbackRefusal>> {
    let err = || SolidError::sm_err(OP_FILL);
    let at_base = springs_on_faces(k, target, pseudo, corner, planes, tol)?;
    if at_base != Springs::Leave {
        return Ok(None);
    }
    let vtx = corner.vertex.ok_or_else(err)?;
    let asked = set_back_fill_of(fills, vtx);
    let sb = corner.setback.as_ref().ok_or_else(err)?;
    let base = sb.setbacks.clone();
    if sb.least.len() != base.len() {
        return Err(err());
    }
    let mut least_scale: f64 = 0.0;
    for (&l, &s) in sb.least.iter().zip(&base) {
        least_scale = smax(least_scale, l / s);
    }
    let growth = Growth {
        k,
        target,
        planes,
        tol,
        most: vec![BIG_DOUBLE; base.len()],
        base,
    };
    let mut leaves = |i: i32| -> Result<bool> {
        Ok(
            match growth.place(pseudo, corner, solvers, f64::from(i) / 1000.0)? {
                Some(_) => {
                    springs_on_faces(k, target, pseudo, corner, planes, tol)? != Springs::Clear
                }
                None => true,
            },
        )
    };
    let lo = (1..1000).find(|&i| f64::from(i) >= 1000.0 * least_scale);
    let fit = match lo {
        Some(lo) if !leaves(lo)? => Some(bisect_thousandths(lo, 1000, &mut leaves)? - 1),
        _ => None,
    };
    let Some(fit) = fit.map(|i| f64::from(i) / 1000.0) else {
        return Ok(Some(SetbackRefusal::no_room(
            "Fillet Error: no setback at or above where the rails cross keeps the setback corner's spring curves on their faces",
            Some(0.0),
        )));
    };
    if let Some(a) = asked {
        return Ok(Some(setback_too_long(
            a,
            vtx,
            fit * a.setback,
            SPRING_LEAVES,
        )));
    }
    let list: Vec<String> = growth.base.iter().map(|&s| fmt_g(fit * s)).collect();
    Ok(Some(SetbackRefusal::not_built(format!(
        "Fillet Error: the rule's setbacks take a setback corner's spring curve off its face (it comes within the tolerances of another edge of the face, or is not shown not to): setbacks of {} keep every spring curve on its face",
        list.join(", ")
    ))))
}

/// A setback asked at `vtx` that, with its ease, does not fit (`why`): refused, naming
/// `largest`, below which one fits ([`BlendRefusal::SetbackTooLong`]).
fn setback_too_long(a: SetBackFill, vtx: VertexId, largest: f64, why: &str) -> SetbackRefusal {
    SetbackRefusal::no_room(
        format!(
            "Fillet Error: a setback of {} (with its ease, {}) {why}: a setback below {} fits",
            fmt_g(a.setback),
            fmt_g(a.setback + a.ease),
            fmt_g(largest)
        ),
        None,
    )
    .typed(BlendRefusal::SetbackTooLong {
        vertex: vtx,
        largest,
    })
}

/// The refusal of a corner whose setbacks reach past where a far corner cuts a round's
/// rail, `reach` ([`Gathered::PastRails`]) along the corner's edges from the vertex
/// (solidcore#2835): one asked for names the largest that fits with its ease, `reach` less
/// the ease, which builds.
fn past_rails(asked: Option<SetBackFill>, vtx: VertexId, reach: f64) -> SetbackRefusal {
    let Some(a) = asked else {
        return SetbackRefusal::not_built(format!(
            "Fillet Error: the setback corner's setbacks reach past where a far corner cuts a round: a setback below {} fits",
            fmt_g(reach)
        ));
    };
    setback_too_long(
        a,
        vtx,
        reach - a.ease,
        "reaches past where a far corner cuts a round",
    )
}

/// `SmBlendCorner::BuildSetBackFill` (the fork's 61547af, 1506e2f): the Setback's fill
/// ([`fill_hole`](crate::blend::blend_setback::fill_hole)), from the blends' surfaces at their end sections, and from it the base
/// points and the spring curves' halves. `Ok(Err(refusal))` where the fill is refused (its
/// reason noted), where the setbacks reach past a far corner's cut on a round's rail
/// ([`past_rails`]), where it stands past a face ([`containment`], certified) farther than the
/// corner's tolerance, or where the rule's setbacks overflowed. Beside a curved face
/// (solidcore#2343, the fork's 37c9e1d) a fill refused as uncertified grows its setbacks
/// with the real rounds (`grow_curved`, placing the setback vertices again through
/// `solvers`), and one not certified on the material's side of its curved faces is refused.
/// `fills`: the executive's set-back fills (`GetSetBackFill`).
///
/// # Errors
///
/// `SM_ERR` where the corner's topology is not the Setback's, a setback vertex is not
/// processed or the fill does not match it; [`SolidError::null_pointer`] where the fork
/// dereferences missing geometry (`NER`); those of the queries.
#[allow(clippy::too_many_lines)] // the fork's one function
pub fn build_setback_fill<S: SetbackSolvers>(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &mut S,
    fills: &[SetBackFill],
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
    let Some((planes, curved)) = corner_faces(k, target, vtx, &all, &*solvers, tol, convex)? else {
        return Err(err());
    };

    // each blend at its end section
    let (mut surfaces, mut sides) =
        match gather_ends(k, target, pseudo, corner, &*solvers, &planes.faces, tol)? {
            Gathered::Ends(surfaces, sides) => (surfaces, sides),
            Gathered::Refused(why) => return Ok(Err(SetbackRefusal::not_built(why))),
            Gathered::Unplaced => return Err(err()),
            Gathered::PastRails { reach } => {
                return Ok(Err(past_rails(set_back_fill_of(fills, vtx), vtx, reach)))
            }
        };
    // A corner left open is the Setback's hole, not filled (the fork's ef57570). G1: its face
    // curves are the spring curves, in closed form whatever a fill's heights: no fill is made
    // (OpenSetBackSprings). G2: they come from the fill's least-energy interior, no closed
    // form: the order-2 fill is made, its sides taken, and its patches handed to the caller.
    // (classify_setback makes no open corner beside a curved face.)
    let open = set_back_fill_of(fills, vtx).is_some_and(|a| a.open);
    if open && order == 1 {
        return open_setback_springs(pseudo, corner, &planes.planes, &planes.edges);
    }
    // beside a curved face, the spring curves stay inside their trimmed faces (#2343 (d))
    if curved {
        if let Some(refusal) =
            springs_refusal(k, target, pseudo, corner, solvers, &planes, fills, tol)?
        {
            return Ok(Err(refusal));
        }
    }
    let mut stats = FillStats::default();
    let first = fill_hole_counted(
        k,
        &planes.planes,
        &rounds_of(&surfaces, &sides),
        order,
        tol,
        &mut stats,
    );
    let fill = match first {
        Ok(fill) => fill,
        Err(refusal) if !curved || !stats.uncertified => {
            return Ok(Err(SetbackRefusal::not_built(refusal.reason.unwrap_or(
                "Fillet Error: the setback corner's hole cannot be filled",
            ))))
        }
        Err(_) => {
            let asked = set_back_fill_of(fills, vtx);
            let sb = corner.setback.as_ref().ok_or_else(err)?;
            if sb.room.len() != n || sb.setbacks.len() != n {
                return Err(err());
            }
            let base = sb.setbacks.clone();
            let mut most = Vec::with_capacity(n);
            for ((&b, &r), e) in base.iter().zip(&sb.room).zip(&planes.edges) {
                most.push(if asked.is_some() {
                    BIG_DOUBLE
                } else {
                    smax(b, r - 0.25 * e.radius)
                });
            }
            let growth = Growth {
                k,
                target,
                planes: &planes,
                tol,
                base,
                most,
            };
            match grow_curved(&growth, pseudo, corner, solvers, asked)? {
                // the fill where the grown setbacks put the blends' ends, made as a corner
                // asked for those setbacks would make it
                Ok((grown_surfaces, grown_sides)) => {
                    surfaces = grown_surfaces;
                    sides = grown_sides;
                    match fill_hole_counted(
                        k,
                        &planes.planes,
                        &rounds_of(&surfaces, &sides),
                        order,
                        tol,
                        &mut stats,
                    ) {
                        Ok(fill) => fill,
                        Err(refusal) => {
                            return Ok(Err(SetbackRefusal::not_built(refusal.reason.unwrap_or(
                                "Fillet Error: the setback corner's hole cannot be filled",
                            ))))
                        }
                    }
                }
                Err(refusal) => return Ok(Err(refusal)),
            }
        }
    };
    let rounds = rounds_of(&surfaces, &sides);

    if fill.patches.len() != 2 * n || fill.face.len() != n {
        return Err(err());
    }
    // (an open corner keeps no fill: no containment, and no definition of its patches)
    let worst = if open {
        None
    } else {
        let worst = containment(k, &fill, &planes.planes, convex, tol)?;
        if worst == f64::INFINITY {
            return Ok(Err(SetbackRefusal::not_built(
                "Fillet Error: the setback corner's fill is not certified to stay on the material's side of its curved faces",
            )));
        }
        if worst > tol {
            return Ok(Err(SetbackRefusal::not_built(format!(
                "Fillet Error: the setback corner's fill stands {} past a face it meets",
                fmt_g(worst)
            ))));
        }
        Some(worst)
    };

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
            // a side whose weights are all one weight is the polynomial it is (the fill's
            // face curves on planar faces: one polynomial span, which a caller's polynomial
            // patch holds exactly; the fork's ef57570)
            one_weight_polynomial(&mut best)?;
            set_edge_curve(pseudo.brep_mut(), e, Some(Curve::from(best)))?;
        }
    }
    let definition = match worst {
        Some(worst) if k.fixes.fillet_round_as_definition => Some(keep_definition(
            k,
            &planes.planes,
            &rounds,
            order,
            tol,
            convex,
            worst,
            &fill,
        )?),
        _ => None,
    };
    if let Some(sb) = corner.setback.as_mut() {
        sb.fill = Some(fill);
        sb.definition = definition;
    }
    Ok(Ok(()))
}

/// `SmBlendCorner::OpenSetBackSprings` (the fork's ef57570): an open G1 setback corner's
/// face curves, no fill. On face `f`, `P` is the rail end of the blend before it (`M_f`),
/// `Q` the blend after's (`S_f`), each rail a line parallel to its edge, `X` where they
/// cross (least squares). The parabola `(P, X, Q)`, a quadratic Bezier, is tangent to the
/// rails at `P` and `Q`; halved at `t = 1/2` it is `(P, (P + X) / 2, B)` and
/// `(B, (X + Q) / 2, Q)`, `B = (P + 2 X + Q) / 4` the base point. Each half is certified on
/// the face's plane (its control points) and tangent to its rail at its rail end (its first
/// leg along the rail), else refused.
fn open_setback_springs(
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    planes: &[Face],
    edges: &[Edge],
) -> Result<core::result::Result<(), SetbackRefusal>> {
    let nul = || SolidError::null_pointer(OP_FILL);
    let err = || SolidError::sm_err(OP_FILL);
    let n = corner.all_eus.as_ref().ok_or_else(nul)?.len();
    if corner.vertices.len() != 3 * n
        || corner.edges.len() != 3 * n
        || planes.len() != n
        || edges.len() != n
    {
        return Err(err());
    }
    let tol = corner.approx_tol_3d;
    for f in 0..n {
        let fv_m = *corner.vertices.get(3 * f).ok_or_else(nul)?;
        let fv_b = *corner.vertices.get(3 * f + 1).ok_or_else(nul)?;
        let fv_s = *corner.vertices.get(3 * f + 2).ok_or_else(nul)?;
        if !pseudo.fillet_vertex(fv_m)?.is_processed()
            || !pseudo.fillet_vertex(fv_s)?.is_processed()
        {
            return Err(err());
        }
        let p = pseudo.vertex_point(fv_m)?;
        let q = pseudo.vertex_point(fv_s)?;
        let dp = edges.get((f + n - 1) % n).ok_or_else(nul)?.away;
        let dq = edges.get(f).ok_or_else(nul)?.away;
        // X = P + a dP = Q + b dQ, by least squares on (dP, -dQ)
        let qp = q - p;
        let d11 = dp.dot(dp);
        let d12 = -dp.dot(dq);
        let d22 = dq.dot(dq);
        let r1 = dp.dot(qp);
        let r2 = -dq.dot(qp);
        let det = d11 * d22 - d12 * d12;
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's `!( |det| > zero )`: NaN fails
        if !(det.abs() > EFF_ZERO) {
            return Err(err()); // rails along one line: ClassifySetBack takes no two edges G1
        }
        let a = (r1 * d22 - r2 * d12) / det;
        let x = p + dp * a;
        let base = (p + x * 2.0 + q) * 0.25;
        pseudo.set_vertex_point(fv_b, base)?;
        pseudo
            .fillet_vertex_mut(fv_b)?
            .set_status(FilStatus::Processed);
        let plane = planes.get(f).ok_or_else(nul)?;
        for kk in 0..2 {
            let e = *corner.edges.get(3 * f + kk).ok_or_else(nul)?;
            // M-B: P to B; B-S: B to Q
            let points = if kk == 0 {
                [p, (p + x) * 0.5, base]
            } else {
                [base, (x + q) * 0.5, q]
            };
            let half = BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })?;
            // certified: on the face's plane, tangent to its rail at its rail end
            let mut off: f64 = 0.0;
            for &pt in &points {
                off = smax(off, plane.normal.dot(pt - plane.point).abs());
            }
            let [p0, p1, p2] = points;
            let (leg, rail) = if kk == 0 {
                (p1 - p0, dp)
            } else {
                (p2 - p1, dq)
            };
            let leg_len = leg.length();
            let turn = if leg_len > 0.0 {
                leg.cross(rail).length() / (leg_len * rail.length())
            } else {
                1.0
            };
            if off > tol || turn > EFF_ZERO_SQRT {
                return Ok(Err(SetbackRefusal::not_built(format!(
                    "Fillet Error: an open setback corner's face curve leaves its face (by {}) or its rail's tangent (by {})",
                    fmt_g(off),
                    fmt_g(turn)
                ))));
            }
            set_edge_curve(pseudo.brep_mut(), e, Some(Curve::from(half)))?;
        }
    }
    if let Some(sb) = corner.setback.as_mut() {
        sb.open = true;
    }
    Ok(Ok(()))
}

/// `sm_SbOneWeightPolynomial` (the fork's ef57570): a curve whose weights are all one
/// weight is the polynomial it is, stored so (its points unchanged), as one polynomial span
/// a caller's polynomial patch holds exactly.
fn one_weight_polynomial(curve: &mut BSplineCurve) -> Result<()> {
    if !curve.is_rational()? {
        return Ok(());
    }
    let points = curve.control_points()?;
    let Some(w0) = points.first().map(|c| c.w) else {
        return Err(SolidError::null_pointer(OP_FILL));
    };
    if points.iter().skip(1).any(|c| c.w != w0) {
        return Ok(());
    }
    curve.make_non_rational()
}

/// Not the fork (#2488, with `fillet_round_as_definition`): the fill's definition
/// ([`SetbackFill`]: the faces' planes, the rounds at their end sections, the rule's order
/// and tolerance, the corner's convexity and its containment), certified at once: the rule
/// run again from the definition makes this fill, number for number, with its
/// certificates ([`SetbackFill::refill`]). Beside a curved face (#2343) its faces are the
/// quadrics they are, and after a growth its rounds are where the grown setbacks put them.
///
/// # Errors
///
/// `SM_ERR` where the rule run again does not make this fill; those of the definition and
/// of the rule.
#[allow(clippy::too_many_arguments)]
fn keep_definition(
    k: &Kernel,
    faces: &[Face],
    rounds: &[Round<'_>],
    order: usize,
    tol: f64,
    convex: bool,
    worst: f64,
    fill: &Fill,
) -> Result<Arc<SetbackFill>> {
    let definition = SetbackFill::create(k, faces, rounds, order, tol, convex, worst)?;
    if !definition.is_fill(&definition.refill(k)?, &fill.patches) {
        return Err(SolidError::sm_err(OP_FILL));
    }
    Ok(Arc::new(definition))
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
    if corner.setback.as_ref().is_some_and(|sb| sb.open) {
        return Ok(()); // an open G1 corner: its hole is left (OpenSetBackSprings)
    }
    let n = corner.all_eus.as_ref().ok_or_else(nul)?.len();
    let fill = corner
        .setback
        .as_mut()
        .and_then(|sb| sb.fill.take())
        .ok_or_else(nul)?;
    if fill.patches.len() != 2 * n {
        return Err(SolidError::sm_err(OP_PATCHES));
    }
    let definition = corner.setback.as_ref().and_then(|sb| sb.definition.clone());
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
            let mut curve = patch
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
            one_weight_polynomial(&mut curve)?;
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
        let mut surface = Surface::from(patch);
        if let Some(d) = &definition {
            surface.set_corner_definition(CornerDefinition::setback(Arc::clone(d), p)?);
        }
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
