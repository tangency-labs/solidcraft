//! The blend corner's End (the fork's `SmBlendCornerEnd.cpp`): the End at a smooth vertex,
//! the End's edges, and the refusal of a rail that runs past its side edge.
//!
//! Upstream: none. The fork's `source/SMLib/src/SmBlendCornerEnd.cpp`,
//! `source/SMLib/inc/SmBlendCorner.h` (tangency-labs/smlib `41a39ec`, `54a4ee7`, `63c7962`, smlib#124).
//!
//! | Rust                              | Fork                                      |
//! |-----------------------------------|-------------------------------------------|
//! | [`end_at_smooth_vertex`], [`continuing_edges`] | `SmBlendCorner::EndAtSmoothVertex` (its test, with every edge it finds: solidcore's) |
//! | [`calc_end_edge_geom`]            | `SmBlendCorner::CalcEndEdgeGeom` (before the base `CalcCornerEdgeGeom`) |
//! | `calc_end_cap_edges` (private)    | `SmBlendCorner::CalcEndCapEdges`          |
//! | `ball_touching_at` (private)      | `SmBlendCorner::BallTouchingAt` (54a4ee7 splits it out of 41a39ec's `RadiusReachingSideEdgeEnd`), G's surface continued exactly (#17780; not the fork's) |
//! | `radius_reaching_side_edge_end` (private) | `SmBlendCorner::RadiusReachingSideEdgeEnd` |
//! | `refuse_end_past_side_edge` (private) | `SmBlendCorner::RefuseEndPastSideEdge` |
//! | `blend_runs_out_past_vertex` (private) | none (a round run on past a smooth vertex keeps going past its side edge's end) |
//! | [`describe_end_past_side_edge`]   | `SmBlendCorner::DescribeEndPastSideEdge`  |
//! | `refuse_end_overflowing_side_edge` (private), [`describe_end_overflowing_side_edge`] | `SmBlendCorner::RefuseEndOverflowingSideEdge` (54a4ee7, 63c7962) |
//! | [`side_edgeuse_across_seam_vertex`] | `sm_SideEdgeuseAcrossSeamVertex` (`SmBlendCorner.cpp`, 63c7962) |
//! | [`side_edge_chain_to`]            | `SmBlendCorner::SideEdgeChainTo` (63c7962) |
//! | [`fit_of_end_past_side_edges`]    | `SmBlendCorner::FitOfEndPastSideEdges`    |
//! | [`create_end_cap_patch`]          | `SmBlendCorner::CreateEndPatchOfEnd`'s `SM_BEND_AT_SMOOTH` branch |
//! | [`end_awaits_rollover`], [`end_can_await_rollover`], [`end_past_tangent_edge`], `end_rolls_over`, `ends_in_tangent_edge` (private) | none (#2406: the End's rail rolls over a tangent edge first) |
//! | [`side_edgeuse_across_rolled_edge`] | none (solidcore-native tangent rollover: a rail rolled over a tangent edge the whole length ends on the next side edge) |
//! | `rail_rolls_on_past_side_edge` (private) | none (#15767: a rail past a side edge's end that rolls on over a smooth edge there) |
//! | `end_section_rolls_over` (private) | none (#15767: the roll-over lies wholly before the end face) |
//! | [`place_rolled_rail_end`]         | none (#2406: a rail end not reached on the End's plane before the rollover) |
//! | `side_edge_in_cap` (private)      | none (#2406: a cap edge along a side edge in the cap's plane) |
//! | [`EndEdges`], `end_face_runs_on_past_side_edge`, `rail_face` (private) | none: an end face running on tangentially past the side edge is the global merge's, not refused |
//! | `classify_rail_end` (private), `rail_end_on_surfaces` (crate) | none (an End's rail end off its side face at the face's zone solved on the faces, and classified there) |
//! | `run_on_leaves_the_part` (private) | none (#2656: a blend run on past a smooth vertex that runs out of the part ends on the end face, not capped) |
//! | `depth_under_edge_continued` (private) | none (a bend at the smooth vertex that takes the continuing edge to the far side of the blended edge continued: the part stands across the blend run on, which is capped) |
//! | `runs_on_across_seam` (private), [`RUNS_ON_ACROSS_SEAM`] | none (#2864: a round does not end at a seam's vertex; a variable round's End there is refused, every other blend runs on across it) |
//! | [`largest_ball_ending_on_end_face`], [`describe_end_passing_over_end_face`], `ball_touching_on` (crate) | none (SolidCraft #19454: an End on its end face whose contact line passes over that face, which falls away under the round, is refused naming the largest ball whose rail still ends on it; asked only after the End failed) |
//!
//! Deviations: the fork's `NoteError(SM_FILERR_NO_ROOM, ...)` and `NoteFitRadius` before
//! `SM_ERR` are returned ([`EndRefusal`]) for the executive's caller to note, which then
//! returns `SM_ERR`. Solvers come through [`SweptSolvers`]. The bisection runs at most
//! 100 steps and the golden section 200, as the fork's; the side edge's chain at most
//! [`SIDE_EDGE_CHAIN_STEPS`] (the fork's 64) and `CalcRailIntEdgeuse`'s continuation along
//! it the same (the fork's recursion there has no bound). No recursion (WR-697).

use super::swept::SweptSolvers;
use super::{
    bad_input, bad_input_with, bad_setup, edge_use_without_edge, edge_use_without_face,
    edge_without_curve, face_without_surface, missing_defect, solver_input, unbuildable,
    unbuildable_with,
};
use crate::blend::fillet_cliff_overflow::distance_to_surface;
use crate::blend::fillet_corner::p3::CornerPatchEnv;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_corner::topology::same_convexity_edges_round_vertex;
use crate::blend::fillet_corner::topology::{
    check_tangential_edgeuse_at_vertex, find_side_edgeuse, same_convexity_edges,
};
use crate::blend::fillet_geom::p1::{
    FilStatus, FilletBrep, FilletEdgeType, FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_geom::p2::{srf_srf_intersection, SrfSrfOptions};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolver, OffsetSurfaceSlot};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::error::blend::{
    BlendFailure, CornerInput, CornerShape, CornerStep, FilletSetUp, SolverFailure,
};
use crate::error::topology::TopologyFailure;
use crate::error::{EntityRef, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::assert_array::c_general;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::zone_tol3d;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, PI};
use crate::nurbs::knots::Side;
use crate::query::brep::{BrepFaceServices, FaceCaches};
use crate::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP: &str = "blend::blend_corner::end";

/// The fork's comment of a rail rolling over its face's tangent edge into an End at a
/// smooth vertex (`CalcEndCapEdges`).
pub const ROLLS_INTO_SMOOTH_END: &str = "Fillet Error: the round's contact line rolls over its face's tangent edge where the round ends at a smooth vertex (a rail rolling over into an end is not built): no radius builds it";

/// Not the fork's (tangency-labs/solidcore#2864): the comment of a round whose edge runs on
/// across a seam's vertex into the rest of the same curve, not blended.
pub const RUNS_ON_ACROSS_SEAM: &str = "Fillet Error: the round's edge runs on across a seam's vertex into the rest of the same curve between the same two faces, which is not blended, and a round does not end at a seam: blend that edge with it";

/// The fork's comment of an End at a smooth vertex whose cap face fails
/// (`CreateEndPatchOfEnd`).
pub const SMOOTH_END_CAP_FAILED: &str =
    "Fillet Corner Error: the cap of a round ending at a smooth vertex";

/// What [`calc_end_edge_geom`] made of an End's edges.
#[derive(Debug, Clone, PartialEq)]
pub enum EndEdges {
    /// The End's edges are built (or left open, noted), for `CalcCornerEdgeGeom`.
    Built,
    /// Not the fork's: the end rib was deleted where the end face runs on tangentially past
    /// the side edge's far end; the executive does a global merge (as WR-303's end rib).
    GlobalMerge,
    /// The End is refused.
    Refused(EndRefusal),
}

impl EndEdges {
    /// [`EndEdges::Refused`] for a refusal, else [`EndEdges::Built`].
    #[must_use]
    pub fn of(refusal: Option<EndRefusal>) -> Self {
        refusal.map_or(Self::Built, Self::Refused)
    }
}

/// The rail's face F: the fillet's face (of `fil_faces`) that the side edge (of
/// `side_faces`) bounds.
fn rail_face(fil_faces: [FaceId; 2], side_faces: [FaceId; 2]) -> FaceId {
    if side_faces.contains(&fil_faces[0]) {
        fil_faces[0]
    } else {
        fil_faces[1]
    }
}

/// An End's refusal: the fork's `NoteError(SM_FILERR_NO_ROOM, comment)` and
/// `NoteFitRadius(fit, edge)` before its `SM_ERR`.
#[derive(Debug, Clone, PartialEq)]
pub struct EndRefusal {
    /// The reason noted.
    pub comment: String,
    /// The largest radius that builds (0: none).
    pub fit: f64,
    /// The blended edge the fit names.
    pub edge: Option<EdgeId>,
}

/// The face of `eu`; [`edge_use_without_face`] without one.
fn face_of(target: &Brep, eu: EdgeuseId) -> Result<FaceId> {
    eu1::face(target, eu)?.ok_or_else(|| edge_use_without_face(OP, eu))
}

/// The radial edge use of `eu`; [`FilletSetUp::RadialEdgeUse`] at `eu` without one.
fn radial_of(target: &Brep, eu: EdgeuseId) -> Result<EdgeuseId> {
    eu1::radial(target, eu)?
        .ok_or_else(|| bad_setup(OP, FilletSetUp::RadialEdgeUse, Some(EntityRef::Edgeuse(eu))))
}

/// The face of the radial edge use of `eu`.
fn radial_face(target: &Brep, eu: EdgeuseId) -> Result<FaceId> {
    face_of(target, radial_of(target, eu)?)
}

/// The edge of `eu`; [`edge_use_without_edge`] without one.
fn edge_of(b: &Brep, eu: EdgeuseId) -> Result<EdgeId> {
    eu1::edge(b, eu)?.ok_or_else(|| edge_use_without_edge(OP, eu))
}

/// The curve of `e`; [`edge_without_curve`] without one.
fn curve_of(b: &Brep, e: EdgeId) -> Result<&Curve> {
    edge::curve(b, e)?.ok_or_else(|| edge_without_curve(OP, e))
}

/// The solver's edge use `index` (0 or 1); [`SolverFailure::SolverEdgeUseMissing`] without
/// it.
fn solver_edgeuse(fs: &FilletSolver, index: usize) -> Result<EdgeuseId> {
    fs.edgeuse(index)?
        .ok_or_else(|| solver_input(OP, SolverFailure::SolverEdgeUseMissing { index }, None))
}

/// The solver's offset surface `index` (0 or 1); [`FilletSetUp::SolverOffsetSurface`]
/// without it.
fn solver_surface(fs: &FilletSolver, index: usize) -> Result<&OffsetSurfaceSlot> {
    fs.surface(index)?
        .ok_or_else(|| bad_setup(OP, FilletSetUp::SolverOffsetSurface, None))
}

/// The corner's vertex, the caller's ([`BlendFailure::NoVertexGiven`] without one).
fn corner_vertex(corner: &FilletCorner) -> Result<VertexId> {
    corner
        .vertex
        .ok_or_else(|| SolidError::blend(OP, BlendFailure::NoVertexGiven))
}

/// The corner has no filleted (blended) edge use: the caller's input,
/// [`CornerInput::FilletedEdgeUsesNotGiven`].
fn filleted_eus_not_given(corner: &FilletCorner) -> SolidError {
    bad_input(
        OP,
        CornerInput::FilletedEdgeUsesNotGiven { kind: corner.kind },
        None,
    )
}

/// The fillet vertex `fv`, classified to an edge use, names no edge use:
/// [`FilletSetUp::PointClassEdgeUse`] at `fv`.
fn not_on_side_edge_use(fv: VertexId) -> SolidError {
    bad_setup(
        OP,
        FilletSetUp::PointClassEdgeUse,
        Some(EntityRef::Vertex(fv)),
    )
}

/// The caller's side edge `side_edge` has no vertex at its far end from the corner's vertex:
/// [`CornerInput::EdgeWithoutVertex`] at the edge.
fn edge_without_vertex(side_edge: EdgeId) -> SolidError {
    bad_input(
        OP,
        CornerInput::EdgeWithoutVertex,
        Some(EntityRef::Edge(side_edge)),
    )
}

/// The two vertices of the pseudo-B-rep's fillet edge `fe`; [`FilletSetUp::EdgeVertices`]
/// at `fe` without them.
fn edge_ends(pseudo: &FilletBrep, fe: EdgeId) -> Result<(VertexId, VertexId)> {
    let no_ends = || bad_setup(OP, FilletSetUp::EdgeVertices, Some(EntityRef::Edge(fe)));
    let start = edge::vertex(pseudo.brep(), fe)?.ok_or_else(no_ends)?;
    let end = edge::other_vertex(pseudo.brep(), fe, Some(start))?.ok_or_else(no_ends)?;
    Ok((start, end))
}

/// The End's first edge, its end rib; [`CornerInput::TopologyMismatch`] (status
/// `SM_ERR_NULL_POINTER`) without one.
fn first_edge(corner: &FilletCorner) -> Result<EdgeId> {
    corner
        .edges
        .first()
        .copied()
        .ok_or_else(|| bad_input_with(OP, CornerInput::TopologyMismatch, None, SM_ERR_NULL_POINTER))
}

/// The solver of the pseudo-B-rep's rib `rib`: of its primary edge use's fillet geometry
/// ([`FilletSetUp::PrimaryEdgeUse`], [`CornerInput::EdgeUseWithoutFilletGeometry`],
/// [`SolverFailure::FilletGeomWithoutSolver`] where a link is not there).
fn rib_solver(pseudo: &FilletBrep, rib: EdgeId) -> Result<FilletSolverId> {
    let eu = eu1::primary_edgeuse(pseudo.brep(), rib)?
        .ok_or_else(|| bad_setup(OP, FilletSetUp::PrimaryEdgeUse, Some(EntityRef::Edge(rib))))?;
    let geom = pseudo.fillet_edgeuse(eu)?.fillet_geom().ok_or_else(|| {
        bad_input(
            OP,
            CornerInput::EdgeUseWithoutFilletGeometry,
            Some(EntityRef::Edgeuse(eu)),
        )
    })?;
    pseudo.geom(geom)?.fillet_solver().ok_or_else(|| {
        solver_input(
            OP,
            SolverFailure::FilletGeomWithoutSolver { fillet_geom: geom },
            None,
        )
    })
}

/// The original face of the pseudo-B-rep's fillet edge `fe`; [`FilletSetUp::OriginalFace`]
/// at `fe` without one.
fn original_face(pseudo: &FilletBrep, fe: EdgeId) -> Result<FaceId> {
    pseudo
        .fillet_edge(fe)?
        .original_face()
        .ok_or_else(|| bad_setup(OP, FilletSetUp::OriginalFace, Some(EntityRef::Edge(fe))))
}

/// The fork's `SmBlendCorner::EndAtSmoothVertex` (41a39ec): whether the blended edge (of
/// `fil_eu`) runs on at `vertex`, tangentially, into an edge of its convexity that is
/// not blended ([`continuing_edges`], the first one found). `all_eus` are the edge uses
/// round the vertex.
///
/// # Errors
/// Those of [`continuing_edges`].
pub fn end_at_smooth_vertex(
    k: &Kernel,
    target: &mut Brep,
    vertex: VertexId,
    fil_eu: EdgeuseId,
    all_eus: &[EdgeuseId],
    tan_tol_deg: f64,
    earlier_pass_faces: &[FaceId],
) -> Result<bool> {
    let found = continuing_edges(
        k,
        target,
        vertex,
        fil_eu,
        all_eus,
        tan_tol_deg,
        earlier_pass_faces,
        true,
        true,
    )?;
    Ok(!found.is_empty())
}

/// The edges of `all_eus` (edge uses of the edges round `vertex`) that the edge of `fil_eu`
/// runs on into at `vertex`, tangentially, each of its convexity (the fork's
/// `EndAtSmoothVertex`'s test, 41a39ec; `first`: only the first found). Such an edge runs
/// on along one of the edge's faces and leaves the vertex opposite to it, within
/// `tan_tol_deg` (two edges leaving it the same way meet in a cusp, a knife edge's tip,
/// which nothing continues).
///
/// An edge whose faces meet tangentially (a flat side running onto a round end) has no
/// convexity and is no such edge: the End on the end face takes the vertex (the fork's
/// a763ac1, smlib#130). Nor is an edge of a face in `earlier_pass_faces`, the faces an
/// earlier pass of the same blend made (`cpExec->IsEarlierPassFace`, the fork's 9822462):
/// the two rounds meet as one blend's corner, never with an End and its cap between them.
///
/// Not the fork's (tangency-labs/solidcore#2656): nor is an edge into which the blend, run on
/// past the vertex, runs out of the part (`run_on_leaves_the_part`: the edge turns into
/// their shared face away from the blended edge continued, as an earlier round's end arc
/// does). There the End on the end face runs the blend on, as before smlib#124, instead of
/// capping it with a face square to the blends at the vertex. `run_out` asks for that test:
/// the End's question; without it, the edges the blended edge continues into tangentially,
/// whether or not a blend run on would stay in the part (a tangent chain's).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for an edge use without face, radial or edge; those of the
/// convexity test.
#[allow(clippy::too_many_arguments)] // the fork's test, with what it is asked for
pub fn continuing_edges(
    k: &Kernel,
    target: &mut Brep,
    vertex: VertexId,
    fil_eu: EdgeuseId,
    all_eus: &[EdgeuseId],
    tan_tol_deg: f64,
    earlier_pass_faces: &[FaceId],
    first: bool,
    run_out: bool,
) -> Result<Vec<EdgeId>> {
    let mut found = Vec::new();
    let fil_edge = edge_of(target, fil_eu)?;
    let fil_faces = [face_of(target, fil_eu)?, radial_face(target, fil_eu)?];
    for &eu in all_eus {
        let other = edge_of(target, eu)?;
        if other == fil_edge || found.contains(&other) {
            continue;
        }
        // an edge whose faces meet tangentially (a flat side running onto a round end) has
        // no convexity: it is no edge of a tangent chain, and the End on the end face takes
        // the vertex (the fork's a763ac1)
        if check_tangential_edgeuse_at_vertex(k, target, eu, vertex, tan_tol_deg)? {
            continue;
        }
        // the edge running on along one of the blended edge's faces
        let faces = [face_of(target, eu)?, radial_face(target, eu)?];
        // an edge of a round an earlier pass of this blend made (the rounds that overflow
        // go first) is the other round of one blend: the two meet as the corner of both,
        // as when they go together, never with an End and its cap between them (the
        // fork's 9822462)
        if faces.iter().any(|f| earlier_pass_faces.contains(f)) {
            continue;
        }
        let along_its_face = faces.iter().any(|f| fil_faces.contains(f));
        // the other edge leaves the vertex opposite to the blended edge (it runs on;
        // leaving the same way, the two edges meet in a cusp, a knife edge's tip, which
        // nothing continues)
        let mut leave = [Vec3::ZERO, Vec3::ZERO];
        let mut leaves = true;
        for (ee, e) in [fil_edge, other].into_iter().enumerate() {
            if !leaves {
                break;
            }
            if edge::start_vertex(target, e)? == edge::end_vertex(target, e)? {
                leaves = false;
                continue;
            }
            let at_start = edge::start_vertex(target, e)? == Some(vertex);
            let Ok(tangent) = edge::end_tangent(k, target, e, at_start) else {
                leaves = false;
                continue;
            };
            let Ok(mut tangent) = tangent.unitize() else {
                leaves = false;
                continue;
            };
            if !at_start {
                tangent = -tangent;
            }
            if let Some(slot) = leave.get_mut(ee) {
                *slot = tangent;
            }
        }
        let [leave0, leave1] = leave;
        if along_its_face
            && leaves
            && leave0.dot(leave1) < -(tan_tol_deg * (PI / 180.0)).cos()
            && same_convexity_edges_round_vertex(k, target, fil_eu, other)?
            && !(run_out && run_on_leaves_the_part(k, target, vertex, fil_eu, eu, leave0)?)
        {
            found.push(other);
            if first {
                break;
            }
        }
    }
    Ok(found)
}

/// Whether the blend of the edge of `fil_eu`, run on past `vertex` along the edge of
/// `other_eu` that continues it there tangentially, runs out of the part, so the blend ends
/// there by running on (the End on the end face) and needs no cap (tangency-labs/solidcore#2656;
/// not the fork's).
///
/// The two edges E (blended) and E' (continuing) share a face A, and leave the vertex V
/// opposite to each other, within the corner's tangency tolerance of one tangent. The blend
/// run on past V keeps its section, the
/// corner between A and E's other face continued; the part there is bounded on A by E'.
/// Where E' turns into A away from E continued (towards the blend's side of E: the rail on A
/// lies that way), the part falls away from the blend run on, which takes off (or, concave,
/// fills) what is left of its corner and runs out of the part: an edge running on into an
/// earlier round's end arc, or into a round between two faces of a corner. Otherwise the
/// blend run on would go on along E', and the End at the smooth vertex caps it: E' straight
/// past a curved E (a rounded corner arc running on into a straight side,
/// tangency-labs/solidcore#2176), E' along E continued, or one face round V (#453's rim).
///
/// The two curves are compared in A's binormal `d` into A at V (unit, square to the edge),
/// each as its height `c1 x + c2 x^2 + c3 x^3` along `d` over the distance `x` past V along
/// E's tangent, from its derivatives at V (E's run on past V; E's `c1` is zero, `d` being
/// square to it), over the shorter of the two edges' chords from V, against A's zone
/// tolerance.
///
/// First the bend. E' leaving V on the far side of E continued from the blend (its `c1`
/// below E's) runs under E continued until their curvatures bring it back, if they do
/// ([`depth_under_edge_continued`]). Where that is deeper than the tolerance, the part
/// stands across the blend run on from V on, which cannot run out through it, whatever E'
/// does farther along, and the cap stays: an arc meeting a circle a few degrees off
/// tangent, where the circle crosses the arc continued a second time past V. A bend the
/// other way, or one that takes E' no deeper than the tolerance, decides nothing.
///
/// Then the turn, at the chord's end. Where the second orders' difference
/// there exceeds both third orders, it decides: E' turns away where its height exceeds E's
/// continued. Otherwise the curvatures nearly agree (an edge of a curvature-continuous round,
/// which leaves its face with little curvature and turns as it goes), and E' turns away only
/// where its own third order does: E's says how E turns into itself, away from V, not past it.
///
/// Where an edge's curve cannot give the derivatives at V (a tangent field, a composite, a
/// short evaluation), the run-on is undecided and the cap stays, as in the other undecided
/// cases (tangency-labs/solidcore#2679).
///
/// # Errors
/// [`SolidError::Cancelled`] when the kernel's token is cancelled; `SM_ERR_NULL_POINTER`
/// for an edge use without face, radial or edge, or an edge without curve; those of the
/// binormal and the curve evaluations (their cancels and unported calls included).
fn run_on_leaves_the_part(
    k: &Kernel,
    target: &mut Brep,
    vertex: VertexId,
    fil_eu: EdgeuseId,
    other_eu: EdgeuseId,
    leave: Vec3,
) -> Result<bool> {
    use crate::curve::curve::CurveKind;
    use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
    k.check_cancelled(OP)?;
    let fil_radial = radial_of(target, fil_eu)?;
    let other_radial = radial_of(target, other_eu)?;
    let fil_faces = [face_of(target, fil_eu)?, face_of(target, fil_radial)?];
    let other_faces = [face_of(target, other_eu)?, face_of(target, other_radial)?];
    // one face round the vertex: the blend run on would follow the edge
    if fil_faces.iter().all(|f| other_faces.contains(f)) {
        return Ok(false);
    }
    // E's use on the shared face A
    let eu_a = if other_faces.contains(&fil_faces[0]) {
        fil_eu
    } else if other_faces.contains(&fil_faces[1]) {
        fil_radial
    } else {
        return Ok(false);
    };
    let fil_edge = edge_of(target, fil_eu)?;
    let other_edge = edge_of(target, other_eu)?;
    let at_start = |target: &Brep, e: EdgeId| -> Result<bool> {
        Ok(edge::start_vertex(target, e)? == Some(vertex))
    };
    let param_at = |target: &Brep, e: EdgeId| -> Result<f64> {
        let ivl = edge::interval(target, e)?;
        Ok(if at_start(target, e)? {
            ivl.min()
        } else {
            ivl.max()
        })
    };
    // A curve that cannot give the derivatives the comparison needs leaves the run-on
    // undecided, and the cap stays (tangency-labs/solidcore#2679): a tangent field, whose
    // values are cross tangents rather than points and which differentiates only once
    // (`SmTangentField::Evaluate`'s SM_ERR past the first derivative), and a composite,
    // which evaluates only through its segments (`Curve::evaluate`'s SM_ERR). Every other
    // failure of the evaluations below (a cancel, an unported call) is the caller's.
    for e in [fil_edge, other_edge] {
        let c = curve_of(target, e)?;
        if matches!(
            c.kind(),
            CurveKind::TangentField(_) | CurveKind::Composite(_)
        ) {
            return Ok(false);
        }
    }
    let options = BinormalOptions::default();
    let t_fil = param_at(target, fil_edge)?;
    let on_a = evaluate_binormal(k, target, eu_a, t_fil, true, &options)?;
    let Ok(d) = on_a.binormal.unitize() else {
        return Ok(false);
    };
    // past V, along the continuing edge
    let along = -leave;
    // (c1, c2, c3) of an edge's curve at V, its parameter turned to run past V along `along`:
    // the height h over the distance x, both functions of the parameter, differentiated
    // by x
    let heights = |target: &Brep, e: EdgeId| -> Result<Option<(f64, f64, f64)>> {
        let start = at_start(target, e)?;
        let c = curve_of(target, e)?;
        let ds = c.evaluate(k, param_at(target, e)?, 3, !start, true)?;
        // a kind that returns fewer derivatives than asked: undecided, the cap stays
        let (Some(&d1), Some(&d2), Some(&d3)) = (ds.get(1), ds.get(2), ds.get(3)) else {
            return Ok(None);
        };
        let (mut d1, mut d3) = (d1, d3);
        if d1.dot(along) < 0.0 {
            d1 = -d1;
            d3 = -d3;
        }
        let x1 = d1.dot(along);
        if x1 <= 0.0 {
            return Ok(None);
        }
        let (x2, x3) = (d2.dot(along), d3.dot(along));
        let (h1, h2, h3) = (d1.dot(d), d2.dot(d), d3.dot(d));
        let c1 = h1 / x1;
        // h'' x' - h' x'', the second derivative by x times x'^3
        let turn = h2 * x1 - h1 * x2;
        let c2 = turn / (2.0 * x1 * x1 * x1);
        let c3 = ((h3 * x1 - h1 * x3) * x1 - 3.0 * x2 * turn) / (6.0 * x1 * x1 * x1 * x1 * x1);
        Ok(Some((c1, c2, c3)))
    };
    let (Some((e1, e2, e3)), Some((o1, o2, o3))) =
        (heights(target, fil_edge)?, heights(target, other_edge)?)
    else {
        return Ok(false);
    };
    // over the shorter of the two edges' chords from V
    let at = vertex::point(target, vertex)?;
    let mut span = BIG_DOUBLE;
    for e in [fil_edge, other_edge] {
        if let Some(far) = edge::other_vertex(target, e, Some(vertex))? {
            span = span.min(vertex::point(target, far)?.distance(at));
        }
    }
    if !(span > 0.0 && span < BIG_DOUBLE) {
        return Ok(false);
    }
    let tol = zone_tol3d(k, target.face(face_of(target, eu_a)?)?.tolerance);
    // the bend: the part stands across the blend run on from V on
    if depth_under_edge_continued(o1 - e1, o2 - e2, span) > tol {
        return Ok(false);
    }
    let second = (o2 - e2) * span * span;
    if second.abs() > (o3.abs() + e3.abs()) * span * span * span {
        return Ok(second > tol);
    }
    Ok(o3 * span * span * span > tol)
}

/// How deep the bend at V takes the continuing edge E' under the blended edge E continued,
/// on the far side of it from the blend ([`run_on_leaves_the_part`]): E' stands
/// `first x + second x^2` over E continued at the distance `x` past V, to second order
/// (`first` the bend's slope, `second` the difference of the two edges' second orders), over
/// `0 < x <= span`.
///
/// 0 where E' leaves V on the blend's side or along E continued (`first >= 0`). Else, where
/// the curvatures bring E' back (`second > 0`), its depth where it turns back, at
/// `x = -first / (2 second)`: `first^2 / (4 second)`, or its depth at `span` where it turns
/// back past it. Where they never do (`second <= 0`), the bend's own share at `span`,
/// `-first span`: the second order's share there is the turn's to judge, as without a bend.
fn depth_under_edge_continued(first: f64, second: f64, span: f64) -> f64 {
    if first >= 0.0 {
        return 0.0;
    }
    if second > 0.0 {
        let x = (-first / (2.0 * second)).min(span);
        -first * x - second * x * x
    } else {
        -first * span
    }
}

/// The fork's `SmBlendCorner::BallTouchingAt` (54a4ee7; 41a39ec's
/// `RadiusReachingSideEdgeEnd` body): the ball touching the fillet's side face F at its
/// point `p` (F the solver's face that the side edge of `side_eu` bounds) and the fillet's
/// other face G (`other_face`): its radius rho, the root of `dist(p + rho n_F, G) - rho`
/// (its centre `p` offset along F's normal the way the solver offsets F), by bisection on
/// `(0, r]`, r the fillet's radius. Returns `(on_g, within, rho)`: `on_g` where `p` lies on
/// G (rho 0); `within` where the root lies below r (else rho is r).
///
/// Not the fork's (#17780): G is G's surface continued as far as it continues exactly
/// ([`distance_to_surface`]: a plane whole, a cylinder, cone, sphere or torus continued as
/// itself), the surface the solver offsets, not G's surface over its face's own domain. The
/// ball touching F at P, past the corner's vertex, touches G's surface beyond the face's
/// end: dropped on the face's own domain, the centre's foot stopped at the domain's side
/// (the vertex), so the ball came out larger than the round's and a rail that does run past
/// P was found not to.
///
/// # Errors
/// `SM_ERR` when no solver face is F, or a point does not drop on F or G;
/// `SM_ERR_NULL_POINTER` without solver, offset surface or G's surface; those of the drops
/// and the normal.
fn ball_touching_at(
    k: &Kernel,
    target: &Brep,
    solvers: &dyn SweptSolvers,
    fil_eu: EdgeuseId,
    side_eu: EdgeuseId,
    other_face: FaceId,
    p: Point3,
) -> Result<(bool, bool, f64)> {
    let fs = solvers.solver_of_edgeuse(fil_eu)?;
    ball_touching_with(k, target, fs, side_eu, other_face, p, None)
}

/// [`ball_touching_at`] for the solver `fs`: the ball touching F (the solver's face that
/// the side edge of `side_eu` bounds) at its point `p` and the solver's other face G
/// (`other_face`), its radius looked for on `(0, cap]`, `cap` the solver's radius where
/// `None`. Returns `(on_g, within, rho)` as [`ball_touching_at`] does, `rho` being `cap`
/// where the root lies past it.
///
/// # Errors
/// As [`ball_touching_at`].
pub(crate) fn ball_touching_with(
    k: &Kernel,
    target: &Brep,
    fs: &FilletSolver,
    side_eu: EdgeuseId,
    other_face: FaceId,
    p: Point3,
    cap: Option<f64>,
) -> Result<(bool, bool, f64)> {
    let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
    let mut f_index = 2;
    for ii in 0..2 {
        let eu = solver_edgeuse(fs, ii)?;
        let face = face_of(target, eu)?;
        if face != other_face && side_faces.contains(&face) {
            f_index = ii;
        }
    }
    if f_index > 1 {
        return Err(unbuildable(OP, CornerStep::SideEdgeOffSolverFaces));
    }
    ball_touching_on(k, target, fs, f_index, other_face, p, cap)
}

/// [`ball_touching_with`] where F is the solver's side `f_index` (0 or 1): the ball touching
/// that side's surface at its point `p` and the solver's other face G (`other_face`).
///
/// # Errors
/// As [`ball_touching_at`], but for the side, which is given.
pub(crate) fn ball_touching_on(
    k: &Kernel,
    target: &Brep,
    fs: &FilletSolver,
    f_index: usize,
    other_face: FaceId,
    p: Point3,
    cap: Option<f64>,
) -> Result<(bool, bool, f64)> {
    let offset_f = &solver_surface(fs, f_index)?.surface;
    let f = offset_f.base_surface();
    let radius = cap.unwrap_or_else(|| offset_f.offset_distance().abs());
    let sign = if offset_f.offset_distance() < 0.0 {
        -1.0
    } else {
        1.0
    };
    let dropped = f.drop_point::<NoObject>(
        k,
        p,
        &f.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    if !dropped.success {
        return Err(unbuildable(OP, CornerStep::EndPointOffBaseSurface));
    }
    let normal = f.evaluate_normal(k, dropped.result_uv, Side::Left, Side::Left)?;
    let distance = |rho: f64| -> Result<f64> {
        let centre = p + normal * (sign * rho);
        let gap = distance_to_surface(k, target, other_face, centre)?;
        if gap >= BIG_DOUBLE {
            return Err(unbuildable(OP, CornerStep::EndCentreOffOtherFace));
        }
        Ok(gap - rho)
    };
    let mut lo = 0.0;
    let mut hi = radius;
    let at_lo = distance(lo)?;
    let at_hi = distance(hi)?;
    let on_g = at_lo <= 0.0;
    let within = at_hi < 0.0;
    let mut rho = if on_g { 0.0 } else { radius };
    if on_g || !within {
        return Ok((on_g, within, rho));
    }
    let mut it = 0;
    while it < 100 && hi - lo > EFF_ZERO * (1.0 + radius) {
        let mid = 0.5 * (lo + hi);
        if distance(mid)? > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
        it += 1;
    }
    rho = 0.5 * (lo + hi);
    Ok((on_g, within, rho))
}

/// The fork's `SmBlendCorner::RadiusReachingSideEdgeEnd` (41a39ec, 54a4ee7): whether the
/// rail at the fillet's radius runs past the far end P of the side edge of `side_eu` (away
/// from the corner's vertex), and the radius at which the rail reaches P (0 when not).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without side edge or far vertex; those of `ball_touching_at`.
#[allow(clippy::too_many_arguments)]
fn radius_reaching_side_edge_end(
    k: &Kernel,
    target: &Brep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    fil_eu: EdgeuseId,
    side_eu: EdgeuseId,
    other_face: FaceId,
) -> Result<(bool, f64)> {
    let side_edge = edge_of(target, side_eu)?;
    let far = if edge::start_vertex(target, side_edge)? == corner.vertex {
        edge::end_vertex(target, side_edge)?
    } else {
        edge::start_vertex(target, side_edge)?
    }
    .ok_or_else(|| edge_without_vertex(side_edge))?;
    let (on, within, fit) = ball_touching_at(
        k,
        target,
        solvers,
        fil_eu,
        side_eu,
        other_face,
        vertex::point(target, far)?,
    )?;
    let past = !on && within;
    Ok((past, if past { fit } else { 0.0 }))
}

/// Not the fork's (#2364 review, SolidCraft #11360's `a_round_ending_in_an_earlier_round`,
/// WR-697): whether the End's face G (the side edge's face that
/// is not the rail's face F) runs on tangentially past the side edge's far end P, so the rail
/// running past P stays on F and ends on G's tangent continuation. That holds where P has
/// exactly three edges: the side edge (F and G), an edge E2 that bounds F and a face H, and
/// an edge E3 between G and H whose faces meet tangentially at P. There the End takes no
/// single end face. The round meets G and H, which are one smooth surface, and the global
/// merge cuts it there exactly. Rounding an edge into an earlier round does this where the
/// earlier round runs tangentially onto the top face: before the blend corner's End the
/// global merge built it (WR-303) to the volume of rounding both edges together.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without side edge, far vertex or edge use; those of the topology
/// walks and the tangency test.
fn end_face_runs_on_past_side_edge(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
    side_eu: EdgeuseId,
    rail_face: FaceId,
) -> Result<bool> {
    let side_edge = edge_of(target, side_eu)?;
    let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
    if !side_faces.contains(&rail_face) {
        return Ok(false);
    }
    let end_face = if side_faces[0] == rail_face {
        side_faces[1]
    } else {
        side_faces[0]
    };
    if end_face == rail_face {
        return Ok(false);
    }
    let far = if edge::start_vertex(target, side_edge)? == corner.vertex {
        edge::end_vertex(target, side_edge)?
    } else {
        edge::start_vertex(target, side_edge)?
    }
    .ok_or_else(|| edge_without_vertex(side_edge))?;
    let edges = vertex::edges(target, far, None)?;
    if edges.len() != 3 || !edges.contains(&side_edge) {
        return Ok(false);
    }
    let mut on_rail_face = None;
    let mut on_end_face = None;
    for &e in &edges {
        if e == side_edge {
            continue;
        }
        let faces = edge::faces(target, e, None)?;
        if faces.len() != 2 {
            return Ok(false);
        }
        let other = |f: FaceId| faces.iter().copied().find(|&x| x != f);
        if faces.contains(&rail_face) && !faces.contains(&end_face) {
            on_rail_face = other(rail_face).map(|h| (e, h));
        } else if faces.contains(&end_face) && !faces.contains(&rail_face) {
            on_end_face = other(end_face).map(|h| (e, h));
        }
    }
    let (Some((_, h)), Some((e3, h3))) = (on_rail_face, on_end_face) else {
        return Ok(false);
    };
    if h != h3 || h == end_face || h == rail_face {
        return Ok(false);
    }
    let eu = eu1::primary_edgeuse(target, e3)?
        .ok_or_else(|| SolidError::topology(OP, TopologyFailure::EdgeWithoutEdgeuses))?;
    check_tangential_edgeuse_at_vertex(k, target, eu, far, rad_to_deg(corner.tangency_tol_radians))
}

/// Not the fork's (#15767): whether the rail's face F (`rail_face`) runs on tangentially past
/// the side edge's far end P onto a face X, so that a rail past P rolls over that tangent
/// edge onto X (the solver's `ProcessRollOver`, after the corners' edges) and ends on the
/// edge between X and the end face ([`side_edgeuse_across_rolled_edge`]), not past the side
/// edge. The rail continued on F's surface past P is what the corner sees before the roll-over.
/// `false` where the walk fails: the End is then refused as on main ([`proven_or_main`]).
///
/// # Errors
/// Those `SolidError::is_unported_or_cancelled` passes on (cancellation, unported callees,
/// unavailable face trims, a spent certificate budget) only.
fn rail_rolls_on_past_side_edge(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
    side_eu: EdgeuseId,
    rail_face: FaceId,
) -> Result<bool> {
    proven_or_main(rail_rolls_on_past_side_edge_walk(
        k, target, corner, side_eu, rail_face,
    ))
}

/// Not the fork's (#15767): an unproven roll-over test as main's path. Each of
/// [`rail_rolls_on_past_side_edge`] and [`end_section_rolls_over`] gates a branch main did
/// not have, taken in place of main's refusal of an End whose rail runs past its side edge;
/// a failure inside the test proves no roll-over, so it is `false` and the End is refused as
/// on main, not ended in a bare error main never reached. Errors that
/// `SolidError::is_unported_or_cancelled` passes on still propagate.
///
/// # Errors
/// `r`'s error where `SolidError::is_unported_or_cancelled` passes it on.
fn proven_or_main(r: Result<bool>) -> Result<bool> {
    match r {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(false),
        ok => ok,
    }
}

/// The walk of [`rail_rolls_on_past_side_edge`].
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without side edge or far vertex; those of the continuation.
fn rail_rolls_on_past_side_edge_walk(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
    side_eu: EdgeuseId,
    rail_face: FaceId,
) -> Result<bool> {
    let side_edge = edge_of(target, side_eu)?;
    let far = if edge::start_vertex(target, side_edge)? == corner.vertex {
        edge::end_vertex(target, side_edge)?
    } else {
        edge::start_vertex(target, side_edge)?
    }
    .ok_or_else(|| edge_without_vertex(side_edge))?;
    let Some(beyond) = side_edgeuse_across_rolled_edge(
        k,
        target,
        side_eu,
        far,
        None,
        corner.tangency_tol_radians,
    )?
    else {
        return Ok(false);
    };
    // the continuation lies on the end face: the face rolled over is F's neighbour
    Ok(eu1::face(target, beyond)? != Some(rail_face))
}

/// Not the fork's (#15767): whether the cross-section of the round at its other rail's end
/// (`g_end`, the rail end on the fillet's other face G at the end face) has already rolled
/// over the tangent edge the rail on F (`rail_face`) runs on past: the ball of the round's
/// radius touching G at that point (its centre offset along G's normal as the solver offsets
/// G) touches F's surface off the face F. Only then does the whole roll-over lie before the
/// end face, so the rolled rail ends on the edge beyond (`rail_rolls_on_past_side_edge`) and
/// the fillet's pieces each keep four sides. Where that ball still touches F inside the
/// face, the section through the roll-over point square to the edge meets G past the end
/// face, the seam between the round's pieces crosses the end face and the piece rolled over
/// is three-sided, which the corner's pieces do not represent: the End is then refused as
/// one whose rail runs past its side edge.
///
/// `true` only where the roll-over is proven: G's rail end is solved, the solver's two
/// surfaces are G's and F's, both drops succeed and the foot lies off F. Anything that
/// proves nothing (an unsolved rail end, a solver whose faces are not G and F, a drop that
/// fails, or a failure of the test itself, [`proven_or_main`]) is `false`, and the End is
/// refused as on main, naming the radius at which the rail reaches the side edge's end.
///
/// `g_end` is `None` where G's rail end missed its own side edge too (a second miss on a
/// non-cliff End: its rail end was found on the side edge's line continued past the edge's
/// end, or not at all). That rail end is not a point of the round at the end face, so the
/// section through it proves nothing, and nothing proves the second miss rolls over either:
/// `false`, before any lookup, and the End is refused as before #20757.
///
/// # Errors
/// Those `SolidError::is_unported_or_cancelled` passes on (cancellation, unported callees,
/// unavailable face trims, a spent certificate budget) only.
#[allow(clippy::too_many_arguments)]
fn end_section_rolls_over(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    solvers: &dyn SweptSolvers,
    fil_eu: EdgeuseId,
    other_face: FaceId,
    rail_face: FaceId,
    g_end: Option<VertexId>,
) -> Result<bool> {
    let Some(g_end) = g_end else {
        return Ok(false);
    };
    proven_or_main(end_section_rolled_over_at(
        k, target, pseudo, solvers, fil_eu, other_face, rail_face, g_end,
    ))
}

/// The test of [`end_section_rolls_over`], `false` where it proves nothing.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without solver surface; those of the drops and the classification.
#[allow(clippy::too_many_arguments)]
fn end_section_rolled_over_at(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    solvers: &dyn SweptSolvers,
    fil_eu: EdgeuseId,
    other_face: FaceId,
    rail_face: FaceId,
    g_end: VertexId,
) -> Result<bool> {
    if !pseudo.fillet_vertex(g_end)?.is_processed() {
        return Ok(false);
    }
    let fs = solvers.solver_of_edgeuse(fil_eu)?;
    let mut g_index = 2;
    let mut f_index = 2;
    for ii in 0..2 {
        let face = face_of(target, solver_edgeuse(fs, ii)?)?;
        if face == other_face {
            g_index = ii;
        } else if face == rail_face {
            f_index = ii;
        }
    }
    if g_index > 1 || f_index > 1 {
        return Ok(false);
    }
    let offset_g = &solver_surface(fs, g_index)?.surface;
    let g = offset_g.base_surface();
    let sign = if offset_g.offset_distance() < 0.0 {
        -1.0
    } else {
        1.0
    };
    let p = pseudo.vertex_point(g_end)?;
    let on_g = g.drop_point::<NoObject>(
        k,
        p,
        &g.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    if !on_g.success {
        return Ok(false);
    }
    let normal = g.evaluate_normal(k, on_g.result_uv, Side::Left, Side::Left)?;
    let centre = p + normal * (sign * offset_g.offset_distance().abs());
    let f = solver_surface(fs, f_index)?.surface.base_surface();
    let on_f = f.drop_point::<NoObject>(
        k,
        centre,
        &f.natural_uv_domain_with_nurb(k)?,
        &DropPointOptions::default(),
    )?;
    if !on_f.success {
        return Ok(false);
    }
    let foot = f.evaluate_point(k, on_f.result_uv)?;
    let zone = zone_tol3d(k, target.face(rail_face)?.tolerance);
    let mut class = PointClassification::new(zone);
    point_3d_classify(
        k,
        target,
        rail_face,
        foot,
        zone,
        true,
        &mut class,
        Point3dClassifyOptions::default(),
    )?;
    Ok(class.point_class() != PointClassType::Face)
}

/// Not the fork's (SolidCraft's `smooth_edges_and_unfinished_tangent_chains_are_refused_saying_why`,
/// after #2656): whether this End is one where the blended edge runs on tangentially at the
/// vertex into an edge not blended, and the blend run on runs out of the part
/// ([`continuing_edges`] with and without that test). Such a round has no end rib to stop at:
/// a rail past its side edge's far end goes on until the round leaves the material, and the
/// global merge cuts it there, as it does where the rail stays on the side edge.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without the corner's blended edge use, edge uses or vertex; those of
/// [`continuing_edges`].
fn blend_runs_out_past_vertex(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
) -> Result<bool> {
    let fil_eu = corner
        .solver_eus
        .as_deref()
        .and_then(<[EdgeuseId]>::first)
        .copied()
        .ok_or_else(|| filleted_eus_not_given(corner))?;
    let all = corner
        .all_eus
        .as_ref()
        .ok_or_else(|| bad_input(OP, CornerInput::RingNotSet, None))?;
    let vertex = corner_vertex(corner)?;
    let tol = rad_to_deg(corner.tangency_tol_radians);
    let continues = continuing_edges(k, target, vertex, fil_eu, all, tol, &[], true, false)?;
    if continues.is_empty() {
        return Ok(false);
    }
    let capped = continuing_edges(k, target, vertex, fil_eu, all, tol, &[], true, true)?;
    Ok(capped.is_empty())
}

/// The fork's `SmBlendCorner::DescribeEndPastSideEdge` (41a39ec): the reason of a refusal
/// of an End whose rail runs past its side edge's far end, naming `fit`.
#[must_use]
pub fn describe_end_past_side_edge(fit: f64) -> String {
    format!(
        "Fillet Error: the round's contact line runs past the end of its side edge at the corner (onto the faces beyond): a radius below {} fits",
        c_general(fit, 0, 6)
    )
}

/// The most steps [`side_edge_chain_to`] walks and the most edges a rail's end is looked
/// for along (the fork's `SideEdgeChainTo` and `CalcEdgeOnEdge` bound, 64).
pub const SIDE_EDGE_CHAIN_STEPS: usize = 64;

/// The fork's `sm_SideEdgeuseAcrossSeamVertex` (`SmBlendCorner.cpp`, 63c7962): the side
/// edge of `eu` (on the rail's face F, its radial on the end face G) continued across its
/// end `at`, where it is split only by a seam: when `at`'s only other edges are N, which
/// continues the side edge G1 at `at` (within `ang_tol_deg`) between the same two faces,
/// and a seam of F or G (both its sides on that one face), N's edge use on `eu`'s faceuse.
/// `None` otherwise, and for a closed side edge or an `at` that is not its end.
///
/// # Errors
/// Those of the topology walks, the seam test and the tangent test.
pub fn side_edgeuse_across_seam_vertex(
    k: &Kernel,
    target: &mut Brep,
    eu: EdgeuseId,
    at: VertexId,
    ang_tol_deg: f64,
) -> Result<Option<EdgeuseId>> {
    let Some(radial) = eu1::radial(target, eu)? else {
        return Ok(None);
    };
    let Some(e) = eu1::edge(target, eu)? else {
        return Ok(None);
    };
    if edge::is_closed(target, e)?
        || (edge::start_vertex(target, e)? != Some(at) && edge::end_vertex(target, e)? != Some(at))
    {
        return Ok(None);
    }
    let (Some(f), Some(g)) = (eu1::face(target, eu)?, eu1::face(target, radial)?) else {
        return Ok(None);
    };
    if f == g {
        return Ok(None);
    }
    let edges = vertex::edges(target, at, None)?;
    if edges.len() != 3 {
        return Ok(None);
    }
    let mut next_edge = None;
    let mut seam = None;
    for &x in &edges {
        if x == e {
            continue;
        }
        let faces = edge::faces(target, x, None)?;
        let mut one_face = faces.first().is_some_and(|&f0| f0 == f || f0 == g);
        for &fk in faces.iter().skip(1) {
            one_face = one_face && Some(&fk) == faces.first();
        }
        if one_face && edge::is_seam(k, target, x)? {
            seam = Some(x);
        } else {
            next_edge = Some(x);
        }
    }
    let (Some(n), Some(_)) = (next_edge, seam) else {
        return Ok(None);
    };
    if edge::is_closed(target, n)? {
        return Ok(None);
    }
    let Ok(uses) = edge::edgeuses(target, n, None, None) else {
        return Ok(None);
    };
    let faceuse = eu1::faceuse(target, eu)?;
    let mut next = None;
    for &x in &uses {
        if next.is_some() {
            break;
        }
        if eu1::faceuse(target, x)? == faceuse {
            if let Some(r) = eu1::radial(target, x)? {
                if eu1::face(target, r)? == Some(g) {
                    next = Some(x);
                }
            }
        }
    }
    let Some(next) = next else {
        return Ok(None);
    };
    let (g1, _) = crate::blend::fillet_corner::p1::test_g1_continuity(
        k,
        target,
        Some(at),
        Some(e),
        Some(n),
        ang_tol_deg,
    )?;
    Ok(g1.then_some(next))
}

/// Not the fork's (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): the side edge of `eu` continued across
/// its end `at` where a smooth (tangent) edge T leaves it: `eu`'s edge E lies between a face F
/// and the end face P, T between F and a face X it meets tangentially, and the third edge N at
/// `at` between X and P. A rail rolled over T onto X the whole length ends on N, not on F's
/// side edge, as a rail continues across a seam's vertex ([`side_edgeuse_across_seam_vertex`]).
/// Given `rolled`, X must be that face. Returns N's edge use on P (the face E and N share).
/// `None` otherwise, and for a closed edge or an `at` that is not E's end.
///
/// # Errors
/// Those of the topology walks and the tangency test.
pub fn side_edgeuse_across_rolled_edge(
    k: &Kernel,
    target: &mut Brep,
    eu: EdgeuseId,
    at: VertexId,
    rolled: Option<FaceId>,
    tangency_tol_rad: f64,
) -> Result<Option<EdgeuseId>> {
    let Some(e) = eu1::edge(target, eu)? else {
        return Ok(None);
    };
    if edge::is_closed(target, e)?
        || (edge::start_vertex(target, e)? != Some(at) && edge::end_vertex(target, e)? != Some(at))
    {
        return Ok(None);
    }
    let e_faces = edge::faces(target, e, None)?;
    let edges = vertex::edges(target, at, None)?;
    if edges.len() != 3 || e_faces.len() != 2 {
        return Ok(None);
    }
    let others: Vec<EdgeId> = edges.iter().copied().filter(|&x| x != e).collect();
    // the vertex's three edges hold E, so two others are left unless the body's vertex
    // does not list E among its edges
    let &[a, b] = others.as_slice() else {
        return Err(bad_input_with(
            OP,
            CornerInput::TopologyMismatch,
            Some(EntityRef::Vertex(at)),
            SM_ERR_NULL_POINTER,
        ));
    };
    for (t, n) in [(a, b), (b, a)] {
        let t_faces = edge::faces(target, t, None)?;
        let n_faces = edge::faces(target, n, None)?;
        // T: F (a face of E) and X; N: X and P (E's other face)
        let Some(&f) = t_faces.iter().find(|f| e_faces.contains(f)) else {
            continue;
        };
        let Some(&x) = t_faces.iter().find(|&&g| g != f) else {
            continue;
        };
        let Some(&p) = e_faces.iter().find(|&&g| g != f) else {
            continue;
        };
        if !n_faces.contains(&x)
            || !n_faces.contains(&p)
            || x == p
            || rolled.is_some_and(|r| r != x)
            || edge::is_closed(target, n)?
        {
            continue;
        }
        let (tangent, _) = crate::blend::fillet_solver::p1::test_manifold_tangent_rollover(
            k,
            target,
            t,
            tangency_tol_rad,
        )?;
        if !tangent {
            continue;
        }
        for y in edge::edgeuses(target, n, None, None)? {
            if eu1::face(target, y)? == Some(p) {
                return Ok(Some(y));
            }
        }
    }
    Ok(None)
}

/// The fork's `SmBlendCorner::SideEdgeChainTo` (63c7962): the side edge at the corner's
/// vertex continued across seams' vertices ([`side_edgeuse_across_seam_vertex`]) up to
/// `last`, the side edge use the rail's end was last looked for on: its edge uses in order
/// from the vertex (`last` alone when it meets the vertex), walked back from `last`, each
/// step across the end of the edge that continues one not yet taken.
///
/// # Errors
/// `SM_ERR` when a step finds no continuation, or past [`SIDE_EDGE_CHAIN_STEPS`] steps;
/// `SM_ERR_NULL_POINTER` without an edge; those of the continuation.
pub fn side_edge_chain_to(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
    last: EdgeuseId,
) -> Result<Vec<EdgeuseId>> {
    let tol_deg = rad_to_deg(corner.tangency_tol_radians);
    let mut back = vec![last];
    let mut curr = last;
    for _ in 0..SIDE_EDGE_CHAIN_STEPS {
        let e = edge_of(target, curr)?;
        let ends = [edge::start_vertex(target, e)?, edge::end_vertex(target, e)?];
        if ends.contains(&corner.vertex) {
            back.reverse();
            return Ok(back);
        }
        let mut prev = None;
        for end in ends {
            if prev.is_some() {
                break;
            }
            let Some(end) = end else {
                continue;
            };
            let Some(x) = side_edgeuse_across_seam_vertex(k, target, curr, end, tol_deg)? else {
                continue;
            };
            let x_edge = eu1::edge(target, x)?;
            let mut taken = false;
            for &b in &back {
                taken = taken || eu1::edge(target, b)? == x_edge;
            }
            if !taken {
                prev = Some(x);
            }
        }
        let Some(prev) = prev else {
            return Err(unbuildable(OP, CornerStep::EndSideEdgeChainBroken));
        };
        back.push(prev);
        curr = prev;
    }
    Err(SolidError::blend(
        OP,
        BlendFailure::SideEdgeChainPastBound {
            steps: SIDE_EDGE_CHAIN_STEPS,
        },
    ))
}

/// The fork's comment of a concave round's mixed End whose rail passes over the side
/// edge of the other convexity (`RefuseEndOverflowingSideEdge`, 54a4ee7), naming `fit`.
#[must_use]
pub fn describe_end_overflowing_side_edge(fit: f64) -> String {
    format!(
        "Fillet Error: the round's contact line passes over its side edge where the round ends on a face (the round would run on past that face, adding material over it): a radius below {} fits",
        c_general(fit, 0, 6)
    )
}

/// The fork's `SmBlendCorner::RefuseEndOverflowingSideEdge` (54a4ee7, 63c7962): a concave
/// round's End of mixed convexity whose rail misses the side edge E of the other convexity
/// (the rail passes over it) is refused, naming the largest radius whose rail still ends
/// on E: the most, over E's pieces `side_eus` (E from the vertex on, continued across
/// seams' vertices), of the radius of the ball touching F on E that also touches G
/// (`ball_touching_at`), found by golden section over each piece's interval (at most 200
/// steps), less the part's own tolerance (the bound is open).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without a piece, its edge or curve; those of the evaluations and
/// `ball_touching_at`.
#[allow(clippy::too_many_arguments)]
fn refuse_end_overflowing_side_edge(
    k: &Kernel,
    target: &Brep,
    solvers: &dyn SweptSolvers,
    fil_eu: EdgeuseId,
    side_eus: &[EdgeuseId],
    other_face: FaceId,
) -> Result<EndRefusal> {
    let mut most: f64 = 0.0;
    for &side_eu in side_eus {
        let side_edge = edge_of(target, side_eu)?;
        let curve = curve_of(target, side_edge)?;
        let ivl = edge::interval(target, side_edge)?;
        let rho = |t: f64| -> Result<f64> {
            let p = curve.evaluate_point(k, t)?;
            let (_, _, rho) = ball_touching_at(k, target, solvers, fil_eu, side_eu, other_face, p)?;
            Ok(rho)
        };
        let gold = 0.5 * (5.0_f64.sqrt() - 1.0);
        let mut a = ivl.min();
        let mut b = ivl.max();
        let mut c = b - gold * (b - a);
        let mut d = a + gold * (b - a);
        let mut at_c = rho(c)?;
        let mut at_d = rho(d)?;
        let mut it = 0;
        while it < 200 && b - a > EFF_ZERO * (1.0 + ivl.length()) {
            if at_c >= at_d {
                b = d;
                d = c;
                at_d = at_c;
                c = b - gold * (b - a);
                at_c = rho(c)?;
            } else {
                a = c;
                c = d;
                at_c = at_d;
                d = a + gold * (b - a);
                at_d = rho(d)?;
            }
            it += 1;
        }
        let piece_most = rho(0.5 * (a + b))?;
        most = most.max(piece_most);
    }
    // the margin under the open bound: the part's own tolerance (the input's)
    let fit = (most - target.tolerance()).max(0.0);
    Ok(EndRefusal {
        comment: describe_end_overflowing_side_edge(fit),
        fit,
        edge: eu1::edge(target, fil_eu)?,
    })
}

/// The comment of an End on its end face whose contact line passes over that face
/// ([`largest_ball_ending_on_end_face`]; not the fork's), naming `fit`.
#[must_use]
pub fn describe_end_passing_over_end_face(fit: f64) -> String {
    format!(
        "Fillet Error: the round's contact line passes over the face the round ends on (that face falls away under the round, which would run on past it, adding material over it): a radius below {} fits",
        c_general(fit, 0, 6)
    )
}

/// The ball touching the rail's face at a point of an end face's crossing with it, and the
/// round's other face, as [`largest_ball_ending_on_end_face`] judges it.
enum CrossingBall {
    /// Its radius, below the round's and the margin.
    Radius(f64),
    /// As large as the round and the margin, or larger: the round's rail reaches the crossing.
    Reaches,
    /// The point lies on the other face: the crossing is back on the blended edge.
    OnOtherFace,
    /// Not solved.
    Unknown,
}

/// The most steps [`largest_ball_ending_on_end_face`] walks along the end face's surface on
/// a rail's face: a forty-eighth of the blended edge's length each, so twice that length.
const END_FACE_WALK_STEPS: usize = 96;

/// The most steps of each solve of [`largest_ball_ending_on_end_face`]: its point on two
/// surfaces (a Newton step on their tangent planes each), the bisection to the rail's
/// face's boundary, and the golden section about its largest ball (0.618^60 is 3e-13 of
/// two steps of the walk).
const END_FACE_SOLVE_STEPS: usize = 60;

/// Not the fork's (SolidCraft #19454): why an End on its end face E was not built, asked
/// only after its vertices or its edges failed. Its rail on a face F of the blended edge
/// ends where it crosses E's surface, continued or as it is
/// (`FilletBrep::calc_rail_int_end_surface`). The ball of the round that touches F there
/// touches F on the curve C in which E's surface crosses F, which leaves the corner's vertex
/// along the blended edge's side: the side edge between F and E run on past the vertex. So
/// the rail of a round of radius r ends on E's surface only where a ball touching F on C,
/// and the round's other face G, has radius r. Where E falls away under the round (a ridge
/// between G and E, E the shallower), those balls stay small: the largest of them, over C
/// as far as it lies on F, is the largest round whose rail still ends on E. A larger round
/// passes over E whole, and no section of E ends it.
///
/// C is walked from the vertex in steps of a forty-eighth of the blended edge's length, at
/// most [`END_FACE_WALK_STEPS`], each point solved on both surfaces. A rail is judged only
/// where the walk comes to C's end on F: C leaves F across F's rim (the last point on F found
/// by bisection, and classified on the rim) or comes back onto the blended edge. Where the
/// ball at a point is the round's (its rail does end on E), the steps run out, or a solve
/// fails inside F, the walk has not seen all of C and its largest ball so far is no bound:
/// nothing is judged of that rail. The largest ball walked is refined by golden section over
/// the two steps about it, on points of F only. Between the points walked the ball is not
/// enclosed: a larger ball between two of them, away from the largest walked, is not seen,
/// which can only make the size named smaller than the largest that ends on E.
///
/// With `missed`, only a rail is judged for which the corner's vertices' own search, run
/// again here (the rail curve's crossings with that surface, `rail_crossing_nearest`), finds
/// no end on E's surface: that the round's rail misses E is then that search's finding, not
/// this walk's, and not a status another failure may have left.
///
/// `Some(largest)` of a constant round where a rail's largest ball is smaller than the round
/// and `margin` together (a round within `margin` of that ball ends on E only where its rail
/// does, so it is judged with the larger ones): the least over the rails judged. `None` where
/// nothing is judged: not an End on its end face, capped at a smooth vertex, a solver whose
/// offsets change along the blend or differ, or no rail judged whose balls all stay below
/// that.
///
/// Since it is asked only on the failure path, an End that builds is never refused by it.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without the solver's edge uses, surfaces or the faces' surfaces;
/// those of the topology walks, the classification and the evaluations. A solve that fails
/// ends its rail's walk.
#[allow(clippy::too_many_lines)] // one judgement: the lookups, the walk, the two refinements
pub fn largest_ball_ending_on_end_face(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    extended_surfaces: &std::collections::BTreeMap<FaceId, crate::surface::surface::Surface>,
    margin: f64,
    missed: bool,
) -> Result<Option<f64>> {
    if !corner
        .blend
        .is_some_and(super::BlendCorner::is_end_on_end_face)
        || corner.end_cap.is_some()
    {
        return Ok(None);
    }
    let (Some(vertex), Some(&fil_eu)) = (
        corner.filleted_vertex(),
        corner.solver_eus.as_deref().and_then(<[_]>::first),
    ) else {
        return Ok(None);
    };
    let Some(fil_edge) = eu1::edge(target, fil_eu)? else {
        return Ok(None);
    };
    let Some(edge_curve) = edge::curve(target, fil_edge)?.cloned() else {
        return Ok(None);
    };
    let start = vertex::point(target, vertex)?;
    // the blended edge: its length (a polygon of sixteen chords) and its way from the vertex
    let ivl = edge::interval(target, fil_edge)?;
    let mut length = 0.0;
    let mut previous = edge_curve.evaluate_point(k, ivl.min())?;
    let first = previous;
    for i in 1..=16 {
        let at = edge_curve.evaluate_point(k, ivl.evaluate(f64::from(i) / 16.0))?;
        length += at.distance(previous);
        previous = at;
    }
    if length <= 0.0 {
        return Ok(None);
    }
    let from_start = first.distance(start) <= previous.distance(start);
    let inward = edge_curve.evaluate_point(
        k,
        ivl.evaluate(if from_start { 1.0e-3 } else { 1.0 - 1.0e-3 }),
    )? - start;
    let step = length / 48.0;
    let tol = 1.0e-3 * target.tolerance();

    let mut fit: Option<f64> = None;
    for &fv in &corner.vertices {
        if pseudo.fillet_vertex(fv)?.fillet_vertex_type() != FilletVertexType::RailXExtendedEdgeuse
        {
            continue;
        }
        // the end rib at this rail end and its rail, its geometry and solver
        let mut rib = None;
        let mut rail = None;
        for e in vertex::edges(pseudo.brep(), fv, None)? {
            let edge_type = pseudo.fillet_edge(e)?.fillet_edge_type();
            if edge_type == FilletEdgeType::FilletXSideFace && rib.is_none() {
                rib = Some(e);
            } else if edge_type == FilletEdgeType::Rail && rail.is_none() {
                rail = Some(e);
            }
        }
        let Some(rib) = rib else {
            continue;
        };
        let geom = match eu1::primary_edgeuse(pseudo.brep(), rib)? {
            Some(eu) => pseudo.fillet_edgeuse(eu)?.fillet_geom(),
            None => None,
        };
        let Some(geom) = geom else {
            continue;
        };
        let Some(solver) = pseudo.geom(geom)?.fillet_solver() else {
            continue;
        };
        // a round: one fixed offset on both sides
        let Some(offsets) = solvers.constant_offsets(solver)? else {
            continue;
        };
        let side = pseudo.rail_index_of_vertex(geom, Some(fv))?;
        let other = usize::from(side == 0);
        // the rail index is 0 or 1
        let [o0, o1] = offsets;
        let (radius, other_radius) = if side == 0 { (o0, o1) } else { (o1, o0) };
        let (radius, other_radius) = (radius.abs(), other_radius.abs());
        if radius <= 0.0 || (radius - other_radius).abs() > EFF_ZERO * (1.0 + radius) {
            continue;
        }
        let fs = solvers.solver(solver)?;
        let f_face = face_of(target, solver_edgeuse(fs, side)?)?;
        let g_face = face_of(target, solver_edgeuse(fs, other)?)?;
        let Some(end_face) = pseudo.fillet_edge(rib)?.original_face() else {
            continue;
        };
        if end_face == f_face || end_face == g_face {
            continue;
        }
        let f = solver_surface(fs, side)?
            .surface
            .base_surface()
            .clone_for_evaluation()?;
        // E's surface as the rail's end is looked for on it: continued, or as it is
        let end_surface = match extended_surfaces.get(&end_face) {
            Some(extended) => extended.clone_for_evaluation()?,
            None => target
                .face(end_face)?
                .surface
                .as_ref()
                .ok_or_else(|| face_without_surface(OP, end_face))?
                .clone_for_evaluation()?,
        };
        if missed {
            // the corner's vertices' own search, run again: the rail curve's crossings with
            // E's surface. A rail it finds an end for on E is not judged, nor one without a
            // curve to search
            let Some(rail_curve) = rail.and_then(|e| pseudo.brep().edge(e).ok()?.curve.clone())
            else {
                continue;
            };
            let found = crate::blend::fillet_geom::p4::rail_crossing_nearest(
                k,
                &rail_curve,
                &end_surface,
                start,
                fs.this_approx_tol_3d(),
            )?;
            if found.is_some() {
                continue;
            }
        }
        let f_domain = f.natural_uv_domain_with_nurb(k)?;
        let e_domain = end_surface.natural_uv_domain_with_nurb(k)?;
        let zone = zone_tol3d(k, target.face(f_face)?.tolerance);

        // the point of C nearest `guess`, on F's surface, and C's direction there
        let on_both = |guess: Point3| -> Result<Option<(Point3, Vec3)>> {
            let mut p = guess;
            for _ in 0..END_FACE_SOLVE_STEPS {
                let on_f =
                    f.drop_point::<NoObject>(k, p, &f_domain, &DropPointOptions::default())?;
                let on_e = end_surface.drop_point::<NoObject>(
                    k,
                    p,
                    &e_domain,
                    &DropPointOptions::default(),
                )?;
                if !on_f.success || !on_e.success {
                    return Ok(None);
                }
                let a = f.evaluate_point(k, on_f.result_uv)?;
                let b = end_surface.evaluate_point(k, on_e.result_uv)?;
                let n_f = f.evaluate_normal(k, on_f.result_uv, Side::Left, Side::Left)?;
                let n_e = end_surface.evaluate_normal(k, on_e.result_uv, Side::Left, Side::Left)?;
                let along = n_f.cross(n_e);
                if a.distance(p) <= tol && b.distance(p) <= tol {
                    return Ok(Some((a, along)));
                }
                // the point of both tangent planes nearest p
                let c = n_f.dot(n_e);
                let det = 1.0 - c * c;
                if det <= EFF_ZERO {
                    return Ok(None);
                }
                let to_f = (a - p).dot(n_f);
                let to_e = (b - p).dot(n_e);
                p = p + n_f * ((to_f - c * to_e) / det) + n_e * ((to_e - c * to_f) / det);
            }
            Ok(None)
        };
        // where a point of F's surface lies on the face F: `None` off it, `Some(true)` on
        // its rim (an edge or a vertex of it), `Some(false)` inside it
        let on_f_face = |target: &mut Brep, p: Point3| -> Result<Option<bool>> {
            let mut class = PointClassification::new(zone);
            point_3d_classify(
                k,
                target,
                f_face,
                p,
                zone,
                true,
                &mut class,
                Point3dClassifyOptions::default(),
            )?;
            Ok(match class.point_class() {
                PointClassType::Face => Some(false),
                PointClassType::Edge | PointClassType::Edgeuse | PointClassType::Vertex => {
                    Some(true)
                }
                _ => None,
            })
        };
        // the ball touching F at a point of C and G, judged up to the round's and the margin
        let most_judged = radius + margin;
        let ball = |target: &Brep, p: Point3| -> Result<CrossingBall> {
            match ball_touching_on(k, target, fs, side, g_face, p, Some(most_judged)) {
                Ok((true, _, _)) => Ok(CrossingBall::OnOtherFace),
                Ok((false, false, _)) => Ok(CrossingBall::Reaches),
                Ok((false, true, rho)) => Ok(CrossingBall::Radius(rho)),
                Err(e) if e.is_unported_or_cancelled() => Err(e),
                Err(_) => Ok(CrossingBall::Unknown),
            }
        };

        let Some((mut p, along)) = on_both(start)? else {
            continue;
        };
        let Some((_, mut d)) = along.length_and_unit() else {
            continue;
        };
        if d.dot(inward) < 0.0 {
            d *= -1.0;
        }
        let mut walked = vec![(p, 0.0_f64)];
        // how the walk stopped: the round's rail reaches C; or C came to its end on F
        let mut reached = false;
        let mut ended = false;
        // the step that left F, or that no point of C was solved at
        let mut left: Option<Point3> = None;
        for _ in 0..END_FACE_WALK_STEPS {
            let ahead = p + d * step;
            let Some((q, along)) = on_both(ahead)? else {
                left = Some(ahead);
                break;
            };
            if on_f_face(target, q)?.is_none() {
                left = Some(q);
                break;
            }
            match ball(target, q)? {
                CrossingBall::Reaches => {
                    reached = true;
                    break;
                }
                CrossingBall::OnOtherFace => {
                    ended = true;
                    break;
                }
                CrossingBall::Unknown => break,
                CrossingBall::Radius(rho) => walked.push((q, rho)),
            }
            let Some((_, mut u)) = along.length_and_unit() else {
                break;
            };
            if u.dot(d) < 0.0 {
                u *= -1.0;
            }
            p = q;
            d = u;
        }
        if reached {
            continue;
        }
        // the last point of C on F before the step past it. C ends there only where that
        // point lies on F's rim: a step that no point was solved at inside F shows nothing
        if let Some(off) = left {
            let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
            let mut last = None;
            for _ in 0..END_FACE_SOLVE_STEPS {
                let mid = 0.5 * (lo + hi);
                let on = match on_both(p + (off - p) * mid)? {
                    Some((q, _)) => on_f_face(target, q)?.map(|rim| (q, rim)),
                    None => None,
                };
                if on.is_some() {
                    lo = mid;
                    last = on;
                } else {
                    hi = mid;
                }
            }
            match last {
                Some((q, rim)) => match ball(target, q)? {
                    CrossingBall::Reaches => continue,
                    CrossingBall::Radius(rho) => {
                        walked.push((q, rho));
                        ended = rim;
                    }
                    CrossingBall::OnOtherFace => ended = true,
                    CrossingBall::Unknown => {}
                },
                // no point of C past the last one walked: it is C's end where it is on the rim
                None => ended = on_f_face(target, p)? == Some(true),
            }
        }
        // a walk that ran out of steps, or stopped inside F, has not seen all of C: its
        // largest ball so far bounds nothing
        if !ended || walked.len() < 2 {
            continue;
        }
        let mut at = 0;
        let mut most = 0.0;
        for (i, &(_, rho)) in walked.iter().enumerate() {
            if rho > most {
                most = rho;
                at = i;
            }
        }
        // an inner largest ball: golden section over the two chords about it
        if let (Some(&(before, _)), Some(&(here, _)), Some(&(after, _))) = (
            at.checked_sub(1).and_then(|i| walked.get(i)),
            walked.get(at),
            walked.get(at + 1),
        ) {
            // (a probe off F, or with no ball solved, ends the search and adds nothing; a
            // probe whose ball is the round's is kept whatever the other probe gives)
            let probe_reaches = std::cell::Cell::new(false);
            let rho_at = |target: &mut Brep, x: f64| -> Result<Option<f64>> {
                let guess = if x < 0.0 {
                    here + (before - here) * -x
                } else {
                    here + (after - here) * x
                };
                let Some((q, _)) = on_both(guess)? else {
                    return Ok(None);
                };
                if on_f_face(target, q)?.is_none() {
                    return Ok(None);
                }
                Ok(match ball(target, q)? {
                    CrossingBall::Reaches => {
                        probe_reaches.set(true);
                        Some(most_judged)
                    }
                    CrossingBall::Radius(rho) => Some(rho),
                    CrossingBall::OnOtherFace | CrossingBall::Unknown => None,
                })
            };
            let gold = 0.5 * (5.0_f64.sqrt() - 1.0);
            let (mut a, mut b) = (-1.0_f64, 1.0_f64);
            let mut c = b - gold * (b - a);
            let mut e = a + gold * (b - a);
            let (mut at_c, mut at_e) = (rho_at(target, c)?, rho_at(target, e)?);
            for _ in 0..END_FACE_SOLVE_STEPS {
                let (Some(rho_c), Some(rho_e)) = (at_c, at_e) else {
                    break;
                };
                most = most.max(rho_c).max(rho_e);
                if rho_c >= rho_e {
                    b = e;
                    e = c;
                    at_e = at_c;
                    c = b - gold * (b - a);
                    at_c = rho_at(target, c)?;
                } else {
                    a = c;
                    c = e;
                    at_c = at_e;
                    e = a + gold * (b - a);
                    at_e = rho_at(target, e)?;
                }
            }
            // the round's rail reaches C at a probe: nothing is judged of this rail
            if probe_reaches.get() {
                continue;
            }
        }
        if most >= most_judged {
            continue;
        }
        fit = Some(fit.map_or(most, |least| least.min(most)));
    }
    Ok(fit)
}

/// The fork's `SmBlendCorner::RefuseEndPastSideEdge` (41a39ec): an End whose rail runs
/// past its side edge's far end P, refused naming the radius at which the rail reaches P.
///
/// # Errors
/// `SM_ERR` when the rail does not run past P; those of `radius_reaching_side_edge_end`.
#[allow(clippy::too_many_arguments)]
fn refuse_end_past_side_edge(
    k: &Kernel,
    target: &Brep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    fil_eu: EdgeuseId,
    side_eu: EdgeuseId,
    other_face: FaceId,
) -> Result<EndRefusal> {
    let (past, fit) =
        radius_reaching_side_edge_end(k, target, corner, solvers, fil_eu, side_eu, other_face)?;
    if !past {
        return Err(unbuildable(OP, CornerStep::EndRailNotPastSideEdge));
    }
    Ok(EndRefusal {
        comment: describe_end_past_side_edge(fit),
        fit,
        edge: eu1::edge(target, fil_eu)?,
    })
}

/// The fork's `SmBlendCorner::FitOfEndPastSideEdges` (41a39ec): before the corner is
/// built, at an End on its end face of a constant round (`const_radius`: the solver's
/// type is `SM_FS_CONST_RADIUS` or `SM_FS_CONST_RADIUS_ASSISTED`), the least radius at
/// which a rail reaches its side edge's far end, over the side edges the rail at the
/// round's radius runs past; with the blended edge. Side edges of the other convexity or
/// tangent at the vertex, and a closed side edge, are not sized.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without solver, side edge use or other face; those of the
/// convexity, tangency and ball computations.
pub fn fit_of_end_past_side_edges(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    const_radius: impl FnOnce() -> Result<bool>,
) -> Result<Option<(f64, EdgeId)>> {
    let on_end_face = corner
        .blend
        .is_some_and(super::BlendCorner::is_end_on_end_face);
    let (Some(solver_eus), Some(all_eus)) =
        (corner.solver_eus.as_deref(), corner.all_eus.as_deref())
    else {
        return Ok(None);
    };
    let (true, &[fil_eu]) = (on_end_face, solver_eus) else {
        return Ok(None);
    };
    solvers.solver_of_edgeuse(fil_eu)?;
    if !const_radius()? {
        return Ok(None);
    }
    let Some(fil) = all_eus.iter().position(|&eu| eu == fil_eu) else {
        return Ok(None);
    };
    // the ring from the blended edge use: its neighbours, of at least three
    let mut ring = all_eus.to_vec();
    ring.rotate_left(fil);
    let &[_, next, .., prev] = ring.as_slice() else {
        return Ok(None);
    };
    let eus = [prev, next];
    if eus[0] == eus[1] {
        return Ok(None);
    }
    let vertex = corner_vertex(corner)?;
    let tan_tol_deg = rad_to_deg(corner.tangency_tol_radians);
    for &eu in &eus {
        if !same_convexity_edges(k, target, fil_eu, eu, vertex)?
            || check_tangential_edgeuse_at_vertex(k, target, eu, vertex, tan_tol_deg)?
        {
            return Ok(None);
        }
    }
    let mut found: Option<(f64, EdgeId)> = None;
    for &eu in &eus {
        let side_eu = find_side_edgeuse(target, fil_eu, eu)?.ok_or_else(|| {
            SolidError::blend(
                OP,
                BlendFailure::CornerShapeNotBuilt(CornerShape::RingEdgesShareNoFace),
            )
        })?;
        let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
        let mut other_face = face_of(target, fil_eu)?;
        if side_faces.contains(&other_face) {
            other_face = radial_face(target, fil_eu)?;
        }
        let (past, fit) =
            radius_reaching_side_edge_end(k, target, corner, solvers, fil_eu, side_eu, other_face)?;
        // Not the fork's: past an end face that runs on tangentially the global merge cuts
        // the round (`end_face_runs_on_past_side_edge`), so the rail's run past is no reason
        // to refuse.
        let fil_faces = [face_of(target, fil_eu)?, radial_face(target, fil_eu)?];
        let rail = rail_face(fil_faces, side_faces);
        if past && end_face_runs_on_past_side_edge(k, target, corner, side_eu, rail)? {
            continue;
        }
        // Not the fork's (#15767): past a tangent edge of the rail's face the rail rolls on
        // onto the face beyond (`rail_rolls_on_past_side_edge`)
        if past && rail_rolls_on_past_side_edge(k, target, corner, side_eu, rail)? {
            continue;
        }
        if past && found.is_none_or(|(least, _)| fit < least) {
            found = Some((fit, edge_of(target, fil_eu)?));
        }
    }
    Ok(found)
}

/// Not the fork's (#2406): the edge of side face `face` the rail's end (`rail_end`,
/// classified `on` the face) lies on, when that edge runs through the corner's vertex
/// and lies on the cap's plane `cap` from the rail's end to the vertex (three points
/// between them within the corner's approximation tolerance): the cap's edge on the face
/// is then that edge.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without vertex or curve; those of the drops and evaluations.
fn side_edge_in_cap(
    k: &Kernel,
    target: &Brep,
    corner: &FilletCorner,
    face: FaceId,
    on: &PointClassification<EntityRef>,
    rail_end: Point3,
    cap: &crate::surface::surface::Surface,
) -> Result<Option<EdgeId>> {
    let side = match (on.point_class(), on.object()) {
        (PointClassType::Edge, Some(EntityRef::Edge(e))) => e,
        (PointClassType::Edge | PointClassType::Edgeuse, Some(EntityRef::Edgeuse(eu))) => {
            edge_of(target, eu)?
        }
        _ => return Ok(None),
    };
    let vertex = corner_vertex(corner)?;
    let at_start = edge::start_vertex(target, side)? == Some(vertex);
    if !at_start && edge::end_vertex(target, side)? != Some(vertex) {
        return Ok(None);
    }
    if !edge::faces(target, side, None)?.contains(&face) {
        return Ok(None);
    }
    let curve = curve_of(target, side)?;
    let interval = edge::interval(target, side)?;
    let dropped = curve.drop_point(
        k,
        &interval,
        rail_end,
        None,
        BIG_DOUBLE,
        None,
        SolverOperation::Minimize,
    )?;
    if !dropped.success || dropped.distance > corner.approx_tol_3d {
        return Ok(None);
    }
    let at_vertex = if at_start {
        interval.min()
    } else {
        interval.max()
    };
    let domain = cap.natural_uv_domain_with_nurb(k)?;
    for s in [0.25, 0.5, 0.75] {
        let t = dropped.parameter + (at_vertex - dropped.parameter) * s;
        let p = curve.evaluate_point(k, t)?;
        let foot = cap.drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())?;
        if !foot.success || foot.gap > corner.approx_tol_3d {
            return Ok(None);
        }
    }
    Ok(Some(side))
}

/// Newton steps of [`rail_end_on_surfaces`].
const RAIL_END_STEPS: usize = 32;

/// Not the fork's: the End's rail end on F solved on the faces themselves. The point p of
/// `f` (F's surface) on the End's plane (through `origin`, square to `normal`) where the
/// ball touches F: its centre c = p + `f_offset` `N_F(p)` lies `g_offset` from `g` (the
/// other face's surface) along G's normal, (c - q) . `N_G(q)` = `g_offset` with q c's foot on
/// G.
/// Both offsets are signed, as the solver offsets its surfaces (along the base surface's
/// normal), so the root with the centre on G's other side does not solve this system.
///
/// Newton's method on p's (u, v) from the foot p0 of `start` (the rail curve's crossing with
/// the plane, which approximates p), at most [`RAIL_END_STEPS`] steps, its Jacobian J by
/// forward differences. p is accepted where
/// - both residuals are at most `tol` = min(1e-9 (1 + |`f_offset`| + |`g_offset`|) + 64 eps
///   (|`origin`| + |`start`| + |`f_offset`| + |`g_offset`|), `zone` / 1000): the first term
///   is the solve's target, independent of where the part lies; the second the rounding of
///   the residuals' own arithmetic (eps the machine epsilon); the cap keeps `tol` a
///   thousandth of the zone the point is classified at (`zone`, F's), wherever the part
///   lies and however large the ball. Where rounding keeps the residuals above the cap the
///   solve finds nothing;
/// - the Newton correction from p, J^-1 r mapped onto `f`, is at most `tol` long. To first
///   order that correction is p's distance from the root, however ill-conditioned J is, so
///   a small residual alone (a near-singular J) does not accept p;
/// - p lies at most twice the first correction (plus `tol`) from p0, and at most
///   |`f_offset`| from `start`: Newton's steps from a start that converges shrink
///   quadratically, so their sum is close to the first; a root farther away is another
///   ball's contact, not this rail's.
///
/// `None` (the rail end left as it is) where a step leaves `f`'s domain, a centre does not
/// drop onto `g`, the Jacobian is singular, or the solve settles nowhere acceptable within its
/// steps.
///
/// # Errors
/// Those of the evaluations and drops; [`SolidError::Cancelled`] when the kernel is
/// cancelled.
#[allow(clippy::too_many_arguments)] // the two faces' offsets, the plane and the zone
pub(crate) fn rail_end_on_surfaces(
    k: &Kernel,
    f: &crate::surface::surface::Surface,
    f_offset: f64,
    g: &crate::surface::surface::Surface,
    g_offset: f64,
    origin: Point3,
    normal: Vec3,
    start: Point3,
    zone: f64,
) -> Result<Option<Point3>> {
    let f_domain = f.natural_uv_domain_with_nurb(k)?;
    let g_domain = g.natural_uv_domain_with_nurb(k)?;
    let foot = f.drop_point::<NoObject>(k, start, &f_domain, &DropPointOptions::default())?;
    if !foot.success {
        return Ok(None);
    }
    let offsets = f_offset.abs() + g_offset.abs();
    let tol = (1e-9 * (1.0 + offsets)
        + 64.0 * f64::EPSILON * (origin.length() + start.length() + offsets))
        .min(zone / 1000.0);
    let in_domain = |uv: crate::math::Point2| {
        uv.x >= f_domain.min.x
            && uv.x <= f_domain.max.x
            && uv.y >= f_domain.min.y
            && uv.y <= f_domain.max.y
    };
    // the plane's and the ball's equations at (u, v), and the point
    let residual = |uv: crate::math::Point2| -> Result<Option<(f64, f64, Point3)>> {
        if !in_domain(uv) {
            return Ok(None);
        }
        let p = f.evaluate_point(k, uv)?;
        let n = f.evaluate_normal(k, uv, Side::Left, Side::Left)?;
        let centre = p + n * f_offset;
        let at = g.drop_point::<NoObject>(k, centre, &g_domain, &DropPointOptions::default())?;
        if !at.success {
            return Ok(None);
        }
        let q = g.evaluate_point(k, at.result_uv)?;
        let n_g = g.evaluate_normal(k, at.result_uv, Side::Left, Side::Left)?;
        Ok(Some((
            (p - origin).dot(normal),
            (centre - q).dot(n_g) - g_offset,
            p,
        )))
    };
    let hu = 1e-7 * (f_domain.max.x - f_domain.min.x);
    let hv = 1e-7 * (f_domain.max.y - f_domain.min.y);
    let mut uv = foot.result_uv;
    let p0 = f.evaluate_point(k, uv)?;
    let mut first = None;
    for _ in 0..RAIL_END_STEPS {
        k.check_cancelled(OP)?;
        let Some((r1, r2, p)) = residual(uv)? else {
            return Ok(None);
        };
        let (Some((u1, u2, _)), Some((v1, v2, _))) = (
            residual(crate::math::Point2::new(uv.x + hu, uv.y))?,
            residual(crate::math::Point2::new(uv.x, uv.y + hv))?,
        ) else {
            return Ok(None);
        };
        let (a, b) = ((u1 - r1) / hu, (v1 - r1) / hv);
        let (c, d) = ((u2 - r2) / hu, (v2 - r2) / hv);
        let det = a * d - b * c;
        // a singular (or NaN) Jacobian has no step
        if det.abs().partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Ok(None);
        }
        let next = crate::math::Point2::new(
            uv.x - (d * r1 - b * r2) / det,
            uv.y - (a * r2 - c * r1) / det,
        );
        if !in_domain(next) {
            return Ok(None);
        }
        let step = f.evaluate_point(k, next)?.distance(p);
        let first = *first.get_or_insert(step);
        if r1.abs() <= tol && r2.abs() <= tol && step <= tol {
            let near = p.distance(p0) <= 2.0 * first + tol && p.distance(start) <= f_offset.abs();
            return Ok(near.then_some(p));
        }
        uv = next;
    }
    Ok(None)
}

/// Not the fork's: an End's rail end `point` classified on its side face `face` (F), at F's
/// zone. The rail end is the rail curve's crossing with the End's plane, and the rail is the
/// trace's Hermite approximation of the contact line: its span is accepted by a check at
/// sample points, so the rail end carries no bound and can lie off F's surface by more than
/// F's zone while the ball does touch F there. A rail end off F at F's zone (unknown) is
/// therefore solved on the faces ([`rail_end_on_surfaces`]: the point of F's surface on the
/// plane where the ball touches F, offset from F by the solver's distance on the rail's side
/// and from G, the solver's other side, by its distance there) and that point is classified
/// at F's zone instead. It lies on F's surface, so its class is F's trim boundary's: on F
/// where the ball touches F inside it, unknown where it touches F's surface past its tangent
/// edge (the rail rolled over) or past F's surface. The ball's offsets are the solver's radii
/// times its orientations (signed, as the solver offsets its surfaces), so only a
/// constant-radius solver is solved: one whose offsets change along the blend (constant
/// distance, variable radius) leaves the rail end unknown, as does a solve that finds no
/// point. Unknown is refused, or awaits the rollover, as before.
/// A rail end on F at F's zone is classified as before.
///
/// # Errors
/// Those of the classification (a face without surface is refused there); `SM_ERR_NULL_POINTER`
/// for a corner without cap or rib edge, a solver without the other side's surface or a face
/// without surface, and those of the corner's lookups (the rail end at neither of its
/// geometry's rails); an unported or cancelled solve. A failure of the solve's own
/// evaluations and drops keeps the rail end unknown.
fn classify_rail_end(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    face: FaceId,
    rail_end: VertexId,
) -> Result<PointClassification<EntityRef>> {
    let point = pseudo.vertex_point(rail_end)?;
    let zone = zone_tol3d(k, target.face(face)?.tolerance);
    let mut on = PointClassification::new(zone);
    point_3d_classify(
        k,
        target,
        face,
        point,
        zone,
        true,
        &mut on,
        Point3dClassifyOptions::default(),
    )?;
    if on.point_class() != PointClassType::Unknown {
        return Ok(on);
    }
    // the corner's data the solve reads. A missing piece is the caller's corner input, and
    // its error propagates as `calc_end_cap_edges`' does; a corner without a constant-radius
    // solver has nothing to solve with
    let cap = corner
        .end_cap
        .as_ref()
        .ok_or_else(|| bad_input(OP, CornerInput::EventNotSet, None))?;
    let crate::surface::surface::p2::SurfaceKind::Plane(plane) = cap.kind() else {
        return Ok(on);
    };
    let rib = first_edge(corner)?;
    let geom = match eu1::primary_edgeuse(pseudo.brep(), rib)? {
        Some(eu) => pseudo.fillet_edgeuse(eu)?.fillet_geom(),
        None => None,
    };
    let Some(geom) = geom else {
        return Ok(on);
    };
    let Some(solver) = pseudo.geom(geom)?.fillet_solver() else {
        return Ok(on);
    };
    // the offsets are the solver's radii times its orientations, its fixed data: the offset
    // surfaces' own distances are working state its solves overwrite. A solver whose
    // offsets change along the blend (constant distance, variable radius) is not solved
    let Some(offsets) = solvers.constant_offsets(solver)? else {
        return Ok(on);
    };
    // the rail end's rail is the solver's side on F; the other side is G
    let side = pseudo.rail_index_of_vertex(geom, Some(rail_end))?;
    let other = usize::from(side == 0);
    // the rail index is 0 or 1
    let [o0, o1] = offsets;
    let (f_distance, g_offset) = if side == 0 { (o0, o1) } else { (o1, o0) };
    let f_distance = f_distance.abs();
    let fs = solvers.solver(solver)?;
    let g = solver_surface(fs, other)?.surface.base_surface();
    // F is solved on the face's own surface (after a rollover the rail lies on the face
    // rolled onto, which need not be one of the solver's faces), offset by the solver's
    // distance on its side, towards the centre that lies the solver's signed offset from G
    let f = crate::topology::face::p2::surface(target, face)?
        .ok_or_else(|| face_without_surface(OP, face))?
        .clone_for_evaluation()?;
    // the solve's own evaluations and drops: one that fails proves nothing about the
    // point, so the rail end keeps its class (unknown, the conservative side)
    let solved = || -> Result<Option<Point3>> {
        let f_domain = f.natural_uv_domain_with_nurb(k)?;
        let g_domain = g.natural_uv_domain_with_nurb(k)?;
        let foot = f.drop_point::<NoObject>(k, point, &f_domain, &DropPointOptions::default())?;
        if !foot.success {
            return Ok(None);
        }
        let at = f.evaluate_point(k, foot.result_uv)?;
        let n = f.evaluate_normal(k, foot.result_uv, Side::Left, Side::Left)?;
        let mut sign = None;
        let mut best = BIG_DOUBLE;
        for s in [1.0, -1.0] {
            let centre = at + n * (s * f_distance);
            let on_g =
                g.drop_point::<NoObject>(k, centre, &g_domain, &DropPointOptions::default())?;
            if !on_g.success {
                continue;
            }
            let q = g.evaluate_point(k, on_g.result_uv)?;
            let n_g = g.evaluate_normal(k, on_g.result_uv, Side::Left, Side::Left)?;
            let miss = ((centre - q).dot(n_g) - g_offset).abs();
            if miss < best {
                best = miss;
                sign = Some(s);
            }
        }
        let Some(sign) = sign else {
            return Ok(None);
        };
        rail_end_on_surfaces(
            k,
            &f,
            sign * f_distance,
            g,
            g_offset,
            plane.is_planar().0,
            plane.evaluate_normal(),
            point,
            zone,
        )
    };
    let at = match solved() {
        Ok(Some(at)) => at,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Ok(None) | Err(_) => return Ok(on),
    };
    let mut on_solved = PointClassification::new(zone);
    point_3d_classify(
        k,
        target,
        face,
        at,
        zone,
        true,
        &mut on_solved,
        Point3dClassifyOptions::default(),
    )?;
    Ok(on_solved)
}

/// The fork's `SmBlendCorner::CalcEndCapEdges` (41a39ec): an End at a smooth vertex's
/// cap edges on the side faces, each the face's intersection with the cap's plane
/// between the rail's end and the vertex. A rail ending off its side face (rolling over
/// the face's tangent edge into the End) is refused, no radius building it.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without cap, edge, face, vertex, solver or curve; `SM_ERR` for an
/// unprocessed vertex; those of the classification and the intersection.
fn calc_end_cap_edges(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
) -> Result<Option<EndRefusal>> {
    let cap = corner
        .end_cap
        .as_ref()
        .ok_or_else(|| bad_input(OP, CornerInput::EventNotSet, None))?;
    for &fe in corner.edges.iter().skip(1) {
        if pseudo.fillet_edge(fe)?.fillet_edge_type() != FilletEdgeType::Precomputed
            || edge::curve(pseudo.brep(), fe)?.is_some()
        {
            continue;
        }
        let face = original_face(pseudo, fe)?;
        let (start, end) = edge_ends(pseudo, fe)?;
        if !pseudo.fillet_vertex(start)?.is_processed()
            || !pseudo.fillet_vertex(end)?.is_processed()
        {
            return Err(bad_input(
                OP,
                CornerInput::VertexNotProcessed,
                Some(EntityRef::Edge(fe)),
            ));
        }
        // The rail's end must lie on the side face (a side face ending tangentially at the
        // vertex, as a round at its contact line, can have the rail roll over onto the face
        // beyond).
        let rail_end =
            if pseudo.fillet_vertex(start)?.fillet_vertex_type() == FilletVertexType::OnVertex {
                end
            } else {
                start
            };
        let on = classify_rail_end(k, target, pseudo, corner, solvers, face, rail_end)?;
        if on.point_class() == PointClassType::Unknown {
            // The rail ends off its side face, past the face's tangent edge at the vertex:
            // it rolls over into the End, which is not built. That edge runs through the
            // vertex, on the cap's plane, so the ball touching the side face on it at the
            // plane is the vertex itself: no radius keeps the rail on its face up to the
            // plane.
            let rib = first_edge(corner)?;
            let fs = solvers.solver(rib_solver(pseudo, rib)?)?;
            let blended = solver_edgeuse(fs, 0)?;
            return Ok(Some(EndRefusal {
                comment: ROLLS_INTO_SMOOTH_END.to_string(),
                fit: 0.0,
                edge: eu1::edge(target, blended)?,
            }));
        }
        // Not the fork's (#2406): the rail's end on the side face's own edge at the
        // vertex, that edge lying on the cap's plane up to the vertex (the ball touching
        // the face on its boundary there): the cap's edge on the face is that edge's piece
        // (`SM_FE_ON_EDGE`, as an End's edges along its side edges), not a second curve
        // laid on it.
        if let Some(side) = side_edge_in_cap(
            k,
            target,
            corner,
            face,
            &on,
            pseudo.vertex_point(rail_end)?,
            cap,
        )? {
            let cap_edge = pseudo.fillet_edge_mut(fe)?;
            cap_edge.set_fillet_edge_type(FilletEdgeType::OnEdge);
            cap_edge.set_original_edge(Some(side));
            continue;
        }
        let start_pt = pseudo.vertex_point(start)?;
        let end_pt = pseudo.vertex_point(end)?;
        let mut dir = end_pt - start_pt;
        let surface = crate::topology::face::p2::surface(target, face)?
            .ok_or_else(|| face_without_surface(OP, face))?
            .clone_for_evaluation()?;
        let curves = srf_srf_intersection(
            k,
            Some(start_pt),
            Some(end_pt),
            Some(&mut dir),
            None,
            &surface,
            cap,
            corner.approx_tol_3d,
            corner.tangency_tol_radians,
            SrfSrfOptions::default(),
        )?;
        let curve = curves.curve_3d.ok_or_else(|| {
            unbuildable_with(OP, CornerStep::EndSideFaceMissesCap, SM_ERR_NULL_POINTER)
        })?;
        set_edge_curve(pseudo.brep_mut(), fe, Some(curve))?;
    }
    Ok(None)
}

/// Whether the blended edge's face `face` (F) ends at `vertex` in an edge T tangent to
/// the face beyond it: F's one edge at the vertex other than the blended edge (of
/// `fil_eu`), tested as the rail's rollover tests its side edges
/// (`sm_TestManifoldTangentRollover` within the solver's `tangency_tol_rad`).
///
/// # Errors
/// Those of the topology queries and the tangency test.
fn ends_in_tangent_edge(
    k: &Kernel,
    target: &mut Brep,
    fil_eu: EdgeuseId,
    face: FaceId,
    vertex: VertexId,
    tangency_tol_rad: f64,
) -> Result<bool> {
    let fil_edge = edge_of(target, fil_eu)?;
    if face_of(target, fil_eu)? != face && radial_face(target, fil_eu)? != face {
        return Ok(false);
    }
    let mut others = vertex::edges(target, vertex, Some(face))?
        .into_iter()
        .filter(|&e| e != fil_edge);
    let (Some(side), None) = (others.next(), others.next()) else {
        return Ok(false);
    };
    Ok(
        crate::blend::fillet_solver::p1::test_manifold_tangent_rollover(
            k,
            target,
            side,
            tangency_tol_rad,
        )?
        .0,
    )
}

/// Not the fork's (#2406): whether an End at a smooth
/// vertex waits for its rails' rollover. A rail that ends on the cap's plane off its side
/// face F, F being a face of the blended edge whose edge T at the vertex is tangent to the
/// face G beyond, has left F across T: the ball rolls over T onto G before the plane. The
/// rails' rollover (`TestRollOver`, run after the corners' edges) splits the blend where
/// the rail crosses T, traces the rest on G, ends both rails on the plane again and puts
/// the cap's edge there on G: the End's edges are built then, in the adjusted corner's
/// pass, and not now (on F's surface, which does not run on past T). Only while the rails
/// are still to be rolled over (`rolls_over`: the executive classifies them, not in a cliff
/// overflow); a rail ending off a face of any other kind is refused as the fork refuses it
/// ([`ROLLS_INTO_SMOOTH_END`]).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without cap, edge, face, vertex, solver or blended edge use;
/// those of the classification and the tangency test.
pub fn end_awaits_rollover(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    rolls_over: bool,
) -> Result<bool> {
    end_rolls_over(k, target, pseudo, corner, solvers, rolls_over, true)
}

/// Not the fork's (#2406): whether an End at a smooth vertex
/// can wait for its rails' rollover, before its rail ends are found
/// ([`end_awaits_rollover`] without its rail end off the side face): F, a face of the
/// blended edge that a cap edge lies on, has its other edge at the vertex tangent to the
/// face beyond. Then a rail that does not reach the End's plane on F's surface run on
/// past that edge is given its traced end for now ([`place_rolled_rail_end`]): the rails'
/// rollover traces it again on the face beyond, up to the plane.
///
/// # Errors
/// As [`end_awaits_rollover`].
pub fn end_can_await_rollover(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    rolls_over: bool,
) -> Result<bool> {
    end_rolls_over(k, target, pseudo, corner, solvers, rolls_over, false)
}

/// Not the fork's (#2406): whether an End at a smooth vertex
/// has a cap edge on a face F of the blended edge (of `fil_eu`) whose other edge at the
/// vertex is tangent to the face beyond (`sm_TestManifoldTangentRollover` within
/// `tangency_tol_rad`): the ball can roll over that edge before the End's plane
/// ([`end_can_await_rollover`] as the trace sees it, before the rails' classification).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without the corner's vertex or a cap edge's face; those of the
/// topology queries and the tangency test.
pub fn end_past_tangent_edge(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    fil_eu: EdgeuseId,
    tangency_tol_rad: f64,
) -> Result<bool> {
    if corner.blend.map(|b| b.end).unwrap_or_default() != super::BlendEnd::AtSmooth {
        return Ok(false);
    }
    let vertex = corner_vertex(corner)?;
    for &fe in corner.edges.iter().skip(1) {
        if pseudo.fillet_edge(fe)?.fillet_edge_type() != FilletEdgeType::Precomputed
            || edge::curve(pseudo.brep(), fe)?.is_some()
        {
            continue;
        }
        let face = original_face(pseudo, fe)?;
        if ends_in_tangent_edge(k, target, fil_eu, face, vertex, tangency_tol_rad)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Not the fork's (#2406): an End's rail end `fv` that its rail does not reach on the
/// End's plane, at an End that can wait for the rollover ([`end_can_await_rollover`]):
/// the rail's traced end nearer the corner's vertex, for now. The rails' rollover traces
/// the rail again on the face beyond and finds the rail end on the plane then
/// (`Recovery::end_vertex`). False, and nothing set, for a vertex that is not an End's rail
/// end left by a failed crossing (`SM_FIL_SURF_INT_FAILURE`) or whose rail is not traced.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without the corner's vertex; those of the topology queries and
/// evaluations.
pub fn place_rolled_rail_end(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    fv: VertexId,
) -> Result<bool> {
    let record = pseudo.fillet_vertex(fv)?;
    if record.fillet_vertex_type() != FilletVertexType::RailXExtendedEdgeuse
        || record.status() != FilStatus::SurfIntFailure
    {
        return Ok(false);
    }
    let mut rail = None;
    for e in vertex::edges(pseudo.brep(), fv, None)? {
        if pseudo.fillet_edge(e)?.fillet_edge_type() == FilletEdgeType::Rail {
            rail = Some(e);
            break;
        }
    }
    let Some(rail) = rail else {
        return Ok(false);
    };
    let Some(curve) = edge::curve(pseudo.brep(), rail)?.cloned() else {
        return Ok(false);
    };
    let at = vertex::point(target, corner_vertex(corner)?)?;
    let interval = curve.natural_interval();
    let first = curve.evaluate_point(k, interval.min())?;
    let last = curve.evaluate_point(k, interval.max())?;
    let end = if first.distance(at) <= last.distance(at) {
        first
    } else {
        last
    };
    crate::blend::fillet_geom::p4::set_vertex_point(pseudo.brep_mut(), fv, end)?;
    pseudo
        .fillet_vertex_mut(fv)?
        .set_status(FilStatus::Processed);
    Ok(true)
}

fn end_rolls_over(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
    rolls_over: bool,
    rail_end_off_face: bool,
) -> Result<bool> {
    if !rolls_over || corner.blend.map(|b| b.end).unwrap_or_default() != super::BlendEnd::AtSmooth {
        return Ok(false);
    }
    let vertex = corner_vertex(corner)?;
    for &fe in corner.edges.iter().skip(1) {
        if pseudo.fillet_edge(fe)?.fillet_edge_type() != FilletEdgeType::Precomputed
            || edge::curve(pseudo.brep(), fe)?.is_some()
        {
            continue;
        }
        let face = original_face(pseudo, fe)?;
        let (start, end) = edge_ends(pseudo, fe)?;
        let rail_end =
            if pseudo.fillet_vertex(start)?.fillet_vertex_type() == FilletVertexType::OnVertex {
                end
            } else {
                start
            };
        if rail_end_off_face {
            if !pseudo.fillet_vertex(rail_end)?.is_processed() {
                continue;
            }
            let on = classify_rail_end(k, target, pseudo, corner, solvers, face, rail_end)?;
            if on.point_class() != PointClassType::Unknown {
                continue;
            }
        }
        let rib = first_edge(corner)?;
        let fs = solvers.solver(rib_solver(pseudo, rib)?)?;
        let fil_eu = solver_edgeuse(fs, 0)?;
        if ends_in_tangent_edge(k, target, fil_eu, face, vertex, fs.tangency_tolerance)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Not the fork's (tangency-labs/solidcore#2864): an End at a smooth vertex whose blended edge
/// runs on there across a seam's vertex into the rest of its own curve, between the same two
/// faces ([`side_edgeuse_across_seam_vertex`]'s narrow rule, the side edge's in 63c7962). A
/// seam is the surface's parameterisation, not a feature: the round runs on there or not at
/// all, and never ends in a step. A blend of constant size runs on across it (its edge's
/// continuation is blended with it, `FilletParametersAccess::parameters_runs_on_across_seams`),
/// so this End is met only by a variable round, whose law runs along its own edge and gives
/// no radius past it: refused, no radius building it.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without the corner's blended edge use or vertex; those of the seam
/// and G1 tests.
fn runs_on_across_seam(
    k: &Kernel,
    target: &mut Brep,
    corner: &FilletCorner,
) -> Result<Option<EndRefusal>> {
    let fil_eu = corner
        .solver_eus
        .as_ref()
        .and_then(|eus| eus.first().copied())
        .ok_or_else(|| filleted_eus_not_given(corner))?;
    let vertex = corner_vertex(corner)?;
    let tol_deg = rad_to_deg(corner.tangency_tol_radians);
    if side_edgeuse_across_seam_vertex(k, target, fil_eu, vertex, tol_deg)?.is_none() {
        return Ok(None);
    }
    Ok(Some(EndRefusal {
        comment: RUNS_ON_ACROSS_SEAM.to_string(),
        fit: 0.0,
        edge: eu1::edge(target, fil_eu)?,
    }))
}

/// The fork's `SmBlendCorner::CalcEndEdgeGeom` (41a39ec) before its closing
/// `SmFilletCorner::CalcCornerEdgeGeom`, which the caller runs: an End at a smooth vertex
/// computes its cap's edges ([`calc_end_cap_edges`]), unless its edge runs on there across a
/// seam's vertex (refused, `runs_on_across_seam`; not the fork's); an End's end rib (or, on more than
/// one end face, its two rail ends) whose rail misses its side edge — its end found on the
/// side edge's line past the edge's end, or, in a cliff overflow (`cliff_overflow`), off
/// the fillet — is, in a cliff overflow, left open (`note_open_end`, the end rib deleted),
/// else refused naming the radius at which the rail reaches the side edge's far end
/// ([`EndRefusal`]).
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for missing vertices, edge uses, faces, surfaces or curves;
/// `SM_ERR` for an open end whose end face is a face of the blended edge, or a refusal
/// whose rail does not run past its side edge; those of the drops, the coincidence test,
/// the deletion and the cap's edges.
#[allow(clippy::too_many_lines)] // one fork routine
pub fn calc_end_edge_geom(
    k: &mut Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &dyn SweptSolvers,
    cliff_overflow: bool,
    note_open_end: &mut dyn FnMut(EdgeuseId, VertexId, FaceId, bool) -> Result<()>,
) -> Result<EndEdges> {
    let end = corner.blend.map(|b| b.end).unwrap_or_default();
    if end == super::BlendEnd::AtSmooth {
        // a round never ends at a seam's vertex (#2864): where the edge it runs on into is
        // its own curve continued across one, the End is refused
        if let Some(refusal) = runs_on_across_seam(k, target, corner)? {
            return Ok(EndEdges::Refused(refusal));
        }
        // the cap's edges on the side faces (they need only the vertices), then the end rib
        return Ok(EndEdges::of(calc_end_cap_edges(
            k, target, pseudo, corner, solvers,
        )?));
    }
    // A concave round's End of mixed convexity (its end rib, then the cap's two edges): a
    // rail that misses the side edge of the other convexity passes over it, and the round
    // would run on past the end face, adding material over it: refused, naming the largest
    // radius whose rail still ends on that edge (54a4ee7, RefuseEndOverflowingSideEdge).
    let first_is_side = match corner.edges.first() {
        Some(&e) => pseudo.fillet_edge(e)?.fillet_edge_type() == FilletEdgeType::FilletXSideFace,
        None => false,
    };
    if end == super::BlendEnd::OnEndFace
        && corner.edges.len() == 3
        && first_is_side
        && corner.vertices.len() >= 2
    {
        let fil_eu = corner
            .solver_eus
            .as_ref()
            .and_then(|eus| eus.first().copied())
            .ok_or_else(|| filleted_eus_not_given(corner))?;
        let fil_edge = edge_of(target, fil_eu)?;
        let concave = is_concave_edge(k, target, fil_edge)?;
        let vertex = corner_vertex(corner)?;
        for kk in 0..2 {
            if !concave {
                break;
            }
            let Some(&fv) = corner.vertices.get(kk) else {
                continue;
            };
            let record = pseudo.fillet_vertex(fv)?;
            if record.status() != FilStatus::NoIntRailEu
                || record.point_class() != PointClassType::Edgeuse
            {
                continue;
            }
            let Some(EntityRef::Edgeuse(side_eu)) = record.point_class_object() else {
                return Err(not_on_side_edge_use(fv));
            };
            // the side edge from the vertex, continued across seams' vertices up to where
            // the rail's end was last looked for (CalcRailIntEdgeuse; 63c7962)
            let chain = side_edge_chain_to(k, target, corner, side_eu)?;
            let side_eu = *chain.first().ok_or_else(|| {
                missing_defect(
                    OP,
                    "the side edge's chain, which holds at least the edge use it is walked back from, is empty",
                )
            })?;
            if same_convexity_edges(k, target, fil_eu, side_eu, vertex)? {
                continue;
            }
            let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
            let mut other_face = face_of(target, fil_eu)?;
            if side_faces.contains(&other_face) {
                other_face = radial_face(target, fil_eu)?;
            }
            return refuse_end_overflowing_side_edge(
                k, target, solvers, fil_eu, &chain, other_face,
            )
            .map(EndEdges::Refused);
        }
    }
    let end_rib = if corner.edges.len() == 1 {
        corner.edges.first().copied()
    } else {
        None
    };
    // An end on more than one end face (MakeEndOnEndFace: a corner of more than three
    // edges of one convexity) has its two rail ends and no end rib: the global merge cuts
    // the fillet off where it meets those faces, which holds only where both rails reach
    // their side edges.
    let on_end_faces =
        end == super::BlendEnd::OnEndFace && corner.edges.is_empty() && corner.vertices.len() == 2;
    let rib_is_side = match end_rib {
        Some(rib) => pseudo.fillet_edge(rib)?.fillet_edge_type() == FilletEdgeType::FilletXSideFace,
        None => false,
    };
    // on end faces, the two rail ends; else the end rib's ends, a side edge's rib
    let (start_fv, end_fv) = match (on_end_faces, corner.vertices.as_slice(), end_rib) {
        (true, &[start, end], _) => (start, end),
        (false, _, Some(rib)) if rib_is_side => edge_ends(pseudo, rib)?,
        _ => return Ok(EndEdges::Built),
    };
    // on end faces there is no end rib, which the open end and the merge delete
    let rib_of = |end_rib: Option<EdgeId>| {
        end_rib.ok_or_else(|| {
            bad_input_with(OP, CornerInput::TopologyMismatch, None, SM_ERR_NULL_POINTER)
        })
    };
    let missed = |miss: VertexId, other: VertexId| -> Result<bool> {
        Ok(
            pseudo.fillet_vertex(miss)?.status() == FilStatus::NoIntRailEu
                && pseudo.fillet_vertex(other)?.is_processed(),
        )
    };
    let mut miss_fv = if missed(start_fv, end_fv)? {
        Some(start_fv)
    } else if missed(end_fv, start_fv)? {
        Some(end_fv)
    } else {
        None
    };
    let mut misses = [
        pseudo.fillet_vertex(start_fv)?.status() == FilStatus::NoIntRailEu,
        pseudo.fillet_vertex(end_fv)?.status() == FilStatus::NoIntRailEu,
    ];
    // A rail's end found on the side edge's line continued past the edge's end
    // (SmFilletSolver::RailEdgeuseIntersect continues a straight side edge) misses the side
    // edge itself, however close past the end. In a cliff overflow a rail can also miss its
    // side edge with the intersection clamped to the side edge's far vertex, off the
    // fillet; and both rails can miss there.
    let fillet_surface = if cliff_overflow {
        let rib = rib_of(end_rib)?;
        let rib_eu = eu1::primary_edgeuse(pseudo.brep(), rib)?.ok_or_else(|| {
            bad_setup(OP, FilletSetUp::PrimaryEdgeUse, Some(EntityRef::Edge(rib)))
        })?;
        let rib_geom = pseudo
            .fillet_edgeuse(rib_eu)?
            .fillet_geom()
            .ok_or_else(|| {
                bad_input(
                    OP,
                    CornerInput::EdgeUseWithoutFilletGeometry,
                    Some(EntityRef::Edgeuse(rib_eu)),
                )
            })?;
        Some(
            pseudo
                .geom(rib_geom)?
                .fillet_surface(None)?
                .cloned()
                .ok_or_else(|| {
                    solver_input(
                        OP,
                        SolverFailure::FilletGeomWithoutSurface {
                            fillet_geom: rib_geom,
                        },
                        None,
                    )
                })?,
        )
    } else {
        None
    };
    for (miss, fv) in misses.iter_mut().zip([start_fv, end_fv]) {
        if *miss || !pseudo.fillet_vertex(fv)?.is_processed() {
            continue;
        }
        let point = pseudo.vertex_point(fv)?;
        let mut off = false;
        if let Some(surface) = fillet_surface.as_ref() {
            let dropped = surface.drop_point::<NoObject>(
                k,
                point,
                &surface.natural_uv_domain_with_nurb(k)?,
                &DropPointOptions::default(),
            )?;
            off = !dropped.success || dropped.gap > 10.0 * corner.approx_tol_3d;
        }
        let record = pseudo.fillet_vertex(fv)?;
        if !off && record.point_class() == PointClassType::Edgeuse {
            let Some(EntityRef::Edgeuse(side_eu)) = record.point_class_object() else {
                return Err(not_on_side_edge_use(fv));
            };
            let side_edge = edge_of(target, side_eu)?;
            let curve = curve_of(target, side_edge)?;
            let on_side = curve.drop_point(
                k,
                &edge::interval(target, side_edge)?,
                point,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )?;
            off = !on_side.success || on_side.distance > corner.approx_tol_3d;
        }
        if off {
            *miss = true;
            if cliff_overflow {
                // its rail is not trimmed there
                pseudo
                    .fillet_vertex_mut(fv)?
                    .set_status(FilStatus::NoIntRailEu);
            }
        }
    }
    if (cliff_overflow || miss_fv.is_none()) && (misses[0] || misses[1]) {
        miss_fv = Some(if misses[0] { start_fv } else { end_fv });
    }
    let both_miss = cliff_overflow && misses[0] && misses[1];
    let Some(miss_fv) = miss_fv else {
        return Ok(EndEdges::Built);
    };
    let other_end = other_rail_end(miss_fv, start_fv, end_fv, misses);
    let record = pseudo.fillet_vertex(miss_fv)?;
    if record.point_class() != PointClassType::Edgeuse {
        return Ok(EndEdges::Built);
    }
    // The side edge the rail missed, and the fillet's side face it does not bound.
    let Some(EntityRef::Edgeuse(side_eu)) = record.point_class_object() else {
        return Err(not_on_side_edge_use(miss_fv));
    };
    let fil_eu = corner
        .solver_eus
        .as_ref()
        .and_then(|eus| eus.first().copied())
        .ok_or_else(|| filleted_eus_not_given(corner))?;
    let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
    let fil_faces = [face_of(target, fil_eu)?, radial_face(target, fil_eu)?];
    let mut other_face = fil_faces[0];
    if side_faces.contains(&other_face) {
        other_face = fil_faces[1];
    }
    let surface = crate::topology::face::p2::surface(target, other_face)?
        .ok_or_else(|| face_without_surface(OP, other_face))?
        .clone_for_evaluation()?;
    let side_edge = edge_of(target, side_eu)?;
    let curve = curve_of(target, side_edge)?.clone_for_evaluation()?;
    let interval = edge::interval(target, side_edge)?;
    // the face's own surface: its trimmed-surface services come from the B-rep
    let mut services = BrepFaceServices::with_caches(target, FaceCaches::new());
    let runs_on = surface
        .simple_coincidence_checker_with_context(
            k,
            &surface.natural_uv_domain_with_nurb(k)?,
            &curve,
            &interval,
            corner.approx_tol_3d,
            false,
            [
                Some(CacheObject::Curve(side_edge)),
                Some(CacheObject::Surface(other_face)),
            ],
            None,
            &mut services,
        )?
        .is_some();
    if !runs_on && cliff_overflow {
        // The end face: the side edge's face that is neither of the fillet's side faces.
        let [first, second] = side_faces;
        let end_face = if first == other_face || fil_faces.contains(&first) {
            second
        } else {
            first
        };
        if fil_faces.contains(&end_face) {
            return Err(SolidError::blend(
                OP,
                BlendFailure::CornerShapeNotBuilt(CornerShape::EndFaceIsBlendFace),
            ));
        }
        solvers.solver_of_edgeuse(fil_eu)?;
        let vertex = corner_vertex(corner)?;
        note_open_end(fil_eu, vertex, end_face, false)?;
        let rib = rib_of(end_rib)?;
        pseudo.delete_wire_fillet_edge(k, rib)?;
        corner.edges.clear();
    } else if !runs_on
        && !both_miss
        && (blend_runs_out_past_vertex(k, target, corner)?
            || end_face_runs_on_past_side_edge(
                k,
                target,
                corner,
                side_eu,
                rail_face(fil_faces, side_faces),
            )?)
    {
        // Not the fork's: the end face runs on tangentially past the side edge's far end
        // (`end_face_runs_on_past_side_edge`): the end rib is deleted and the global merge
        // cuts the round where it meets the end face and its continuation, as WR-303's did.
        let rib = rib_of(end_rib)?;
        pseudo.delete_wire_fillet_edge(k, rib)?;
        corner.edges.clear();
        return Ok(EndEdges::GlobalMerge);
    } else if !runs_on
        && !both_miss
        && rail_rolls_on_past_side_edge(
            k,
            target,
            corner,
            side_eu,
            rail_face(fil_faces, side_faces),
        )?
        && end_section_rolls_over(
            k,
            target,
            pseudo,
            solvers,
            fil_eu,
            other_face,
            rail_face(fil_faces, side_faces),
            other_end,
        )?
    {
        // Not the fork's (#15767): the rail runs past P because the rail's face runs on
        // tangentially there; it rolls over that edge onto the face beyond and ends on its
        // edge with the end face, which `ProcessRollOver` solves after the corners' edges
        // (the TangentRollover corner vertex, recomputed there)
        return Ok(EndEdges::Built);
    } else if !runs_on && !both_miss {
        // The rail runs past its side edge's far end P onto the faces beyond. Refused,
        // naming the radius at which the rail reaches P: the ball touching the rail's face
        // F at P (its centre P offset along F's normal as the solver offsets F) that also
        // touches the fillet's other face G, the root of dist(centre(rho), G) = rho on
        // (0, r].
        return refuse_end_past_side_edge(k, target, corner, solvers, fil_eu, side_eu, other_face)
            .map(EndEdges::Refused);
    }
    Ok(EndEdges::Built)
}

/// The other rail's end for `calc_end_edge_geom`'s roll-over test: of the End's rail ends
/// `start_fv` and `end_fv` (`misses[0]` and `misses[1]` say whether each missed its side
/// edge), the one that is not `miss_fv`, or `None` where it missed its side edge too (a
/// second miss, which only a cliff overflow's `both_miss` handles): the roll-over is proven
/// for `miss_fv`'s rail only.
fn other_rail_end(
    miss_fv: VertexId,
    start_fv: VertexId,
    end_fv: VertexId,
    misses: [bool; 2],
) -> Option<VertexId> {
    if miss_fv == start_fv {
        (!misses[1]).then_some(end_fv)
    } else {
        (!misses[0]).then_some(start_fv)
    }
}

/// The fork's `SmBlendCorner::CreateEndPatchOfEnd`'s branch for an End at a smooth vertex
/// (41a39ec): its cap (a copy of the plane with its analytics) is the corner's surface,
/// and its face is made (`MakeFaceBrep`). Returns `false` where the face fails, for the
/// caller to note `SM_FILERR_VERTEX_PROBLEM` ([`SMOOTH_END_CAP_FAILED`]) and return
/// `SM_ERR`.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` without cap; those of the copy.
pub fn create_end_cap_patch(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<bool> {
    let cap = corner
        .end_cap
        .as_ref()
        .ok_or_else(|| bad_input(OP, CornerInput::EventNotSet, None))?
        .copy_and_add_analytics(env.k)?;
    corner.surfaces.push(cap);
    match corner.make_face_brep_with(env) {
        Ok(()) => Ok(true),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::cone::p1::Cone;
    use crate::surface::plane::p1::Plane;
    use crate::surface::surface::Surface;

    /// A real rollover just past the face: the ball touches F's surface 1e-5 rad (3e-5)
    /// past its tangent edge T, more than F's zone (1e-5) past it, and the rail end the trace
    /// gives sits on T's near side, 2e-5 off F's surface (within the blend's approximation
    /// tolerance, 4e-5, which a foot taken to that tolerance would accept as on F). Solved on
    /// the faces, the rail end is the closed-form contact, past T, so it classifies off F.
    ///
    /// F: the cylinder of radius 3 about the z axis, trimmed at T, the line at 60 degrees;
    /// the ball (radius 1) touches it from inside, its centre at radius 2, and touches G, the
    /// plane x = a, from the side x < a: 2 cos(theta) = a - 1. The End's plane is z = 0.
    #[test]
    fn a_rail_end_rolled_just_past_its_face_is_solved_past_it() {
        let k = Kernel::default();
        let f = Surface::from(Cone::new_cylinder_bounded(
            Point3::new(0.0, 0.0, -10.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            3.0,
            0.0,
            360.0,
            20.0,
            false,
            false,
            false,
        ));
        let t = 60.0_f64.to_radians();
        let theta = t + 1e-5;
        let a = 2.0 * theta.cos() + 1.0;
        let g = Surface::from(Plane::from_normal(
            Point3::new(a, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
        ));
        let want = Point3::new(3.0 * theta.cos(), 3.0 * theta.sin(), 0.0);
        // the centre lies inside F: against F's normal where it points out
        let domain = f.natural_uv_domain_with_nurb(&k).unwrap();
        let foot = f
            .drop_point::<NoObject>(&k, want, &domain, &DropPointOptions::default())
            .unwrap();
        let n = f
            .evaluate_normal(&k, foot.result_uv, Side::Left, Side::Left)
            .unwrap();
        let f_offset = if n.dot(want) > 0.0 { -1.0 } else { 1.0 };
        // the trace's rail end: on T's near side, 2e-5 off F's surface
        let near = t - 0.5e-5;
        let start = Point3::new(3.0 * near.cos(), 3.0 * near.sin(), 0.0) * (1.0 + 2e-5 / 3.0);
        assert!(start.distance(want) < 4e-5 * 2.0);
        // a near miss: the rail end's foot on F lies inside T by more than F's zone, so a
        // foot taken at the rail's tolerance would call it on F
        let zone = 1e-5;
        let inside = 3.0 * (t - start.y.atan2(start.x));
        assert!(inside > zone, "{inside}");
        // the centre lies on G's negative side (x < a): G's signed offset is -1
        let solve = |g_offset: f64| {
            rail_end_on_surfaces(
                &k,
                &f,
                f_offset,
                &g,
                g_offset,
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                start,
                zone,
            )
            .unwrap()
        };
        let got = solve(-1.0).unwrap();
        assert!(got.distance(want) <= 1e-9, "{got:?} against {want:?}");
        // past T, by more than F's zone, along F
        let past = 3.0 * (got.y.atan2(got.x) - t);
        assert!(past > 2e-5, "{past}");
        // the offset is signed: a ball on G's other side (x > a) would need its centre at
        // radius 2 with x = a + 1 > 2, which no point of F gives, so nothing is solved
        assert_eq!(solve(1.0), None);
    }

    /// A circle of radius R meeting, at the angle `b` off tangent, the arc of radius `rho`
    /// continued, on the far side of it from the blend, and curving back across it (their
    /// centres on opposite sides): over the arc's tangent it stands `-tan(b) x` to first order
    /// and `x^2 / (2 R cos^3 b)` to second, the arc `-x^2 / (2 rho)`.
    fn circle_over_arc(big: f64, rho: f64, b: f64) -> (f64, f64) {
        (
            -b.tan(),
            1.0 / (2.0 * big * b.cos().powi(3)) + 1.0 / (2.0 * rho),
        )
    }

    #[test]
    fn the_bend_takes_the_continuing_edge_under_the_edge_continued_until_it_turns_back() {
        // it turns back at x = 0.1 / (2 * 0.02) = 2.5, 0.1 * 2.5 - 0.02 * 2.5^2 deep
        assert!((depth_under_edge_continued(-0.1, 0.02, 40.0) - 0.125).abs() < 1e-15);
        // past the span's end: its depth there
        let at_end = 0.1 * 2.0 - 0.02 * 2.0 * 2.0;
        assert!((depth_under_edge_continued(-0.1, 0.02, 2.0) - at_end).abs() < 1e-15);
        // never: the bend's own share at the span's end, whatever the second order
        assert!((depth_under_edge_continued(-0.1, 0.0, 40.0) - 4.0).abs() < 1e-15);
        assert!((depth_under_edge_continued(-0.1, -0.02, 40.0) - 4.0).abs() < 1e-15);
        // circles of R 55 and R 49 meeting 6 degrees off tangent: first^2 / (4 second), which
        // is, to second order, how thick the two discs' overlap is: the radii less the
        // distance between the centres
        let b = 6.0_f64.to_radians();
        let (first, second) = circle_over_arc(55.0, 49.0, b);
        let depth = depth_under_edge_continued(first, second, 33.9);
        assert!((depth - first * first / (4.0 * second)).abs() < 1e-15);
        let apart = (55.0_f64 * 55.0 + 49.0 * 49.0 + 2.0 * 55.0 * 49.0 * b.cos()).sqrt();
        assert!((depth - (55.0 + 49.0 - apart)).abs() < 2e-4, "{depth}");
    }

    #[test]
    fn a_bend_towards_the_blend_or_none_takes_the_continuing_edge_nowhere_under() {
        for second in [-0.02, 0.0, 0.02] {
            assert_eq!(depth_under_edge_continued(0.0, second, 40.0), 0.0);
            assert_eq!(depth_under_edge_continued(0.1, second, 40.0), 0.0);
        }
        // an undefined slope decides nothing: no depth exceeds a tolerance
        let undefined = depth_under_edge_continued(f64::NAN, 0.02, 40.0);
        assert!(undefined.is_nan() || undefined == 0.0);
    }

    /// The bend whose depth is the tolerance, on the same circles: `tan(b) = 2 sqrt(second
    /// tol)` with `second` at that bend (two steps of the fixed point; it barely moves).
    /// Just inside it the depth is under the tolerance, just past it over.
    #[test]
    fn the_depth_crosses_the_tolerance_at_the_bend_that_reaches_it() {
        let tol = 1e-5;
        let mut b = 0.0_f64;
        for _ in 0..4 {
            let (_, second) = circle_over_arc(55.0, 49.0, b);
            b = (2.0 * (second * tol).sqrt()).atan();
        }
        // about 0.05 degrees
        assert!((b.to_degrees() - 0.0503).abs() < 1e-3, "{}", b.to_degrees());
        let depth = |b: f64| {
            let (first, second) = circle_over_arc(55.0, 49.0, b);
            depth_under_edge_continued(first, second, 33.9)
        };
        assert!(depth(b * (1.0 - 1e-6)) < tol);
        assert!(depth(b * (1.0 + 1e-6)) > tol);
    }

    /// Solvers that every lookup fails on with `err`: the roll-over tests below must decide
    /// without them, or fall back to main's refusal when a lookup fails.
    struct FailingSolvers(fn() -> SolidError);

    impl SweptSolvers for FailingSolvers {
        fn solver_of_edgeuse(&self, _: EdgeuseId) -> Result<&FilletSolver> {
            Err((self.0)())
        }
        fn offset_radii_can_change(&self, _: EdgeuseId) -> Result<bool> {
            Err((self.0)())
        }
        fn solver(&self, _: FilletSolverId) -> Result<&FilletSolver> {
            Err((self.0)())
        }
        fn constant_offsets(&self, _: FilletSolverId) -> Result<Option<[f64; 2]>> {
            Err((self.0)())
        }
    }

    /// `end_section_rolls_over` for the other rail's end `g_end` of `pseudo`, on an empty
    /// target with ids the test never reaches.
    fn rolls_over(
        pseudo: &FilletBrep,
        g_end: Option<VertexId>,
        solvers: &FailingSolvers,
    ) -> Result<bool> {
        let k = Kernel::default();
        let mut target = Brep::new();
        end_section_rolls_over(
            &k,
            &mut target,
            pseudo,
            solvers,
            EdgeuseId::from_index(0),
            FaceId::from_index(0),
            FaceId::from_index(1),
            g_end,
        )
    }

    /// A pseudo-B-rep with one fillet vertex of status `status`.
    fn pseudo_with_vertex(status: FilStatus) -> (FilletBrep, VertexId) {
        let k = Kernel::default();
        let mut pseudo = FilletBrep::new();
        let v = pseudo
            .brep_mut()
            .add_vertex_record(crate::topology::Vertex::default())
            .unwrap();
        let mut fv = crate::blend::fillet_geom::p1::FilletVertex::new(&k, None);
        fv.set_status(status);
        pseudo.insert_fillet_vertex(v, fv);
        (pseudo, v)
    }

    fn lookup_failed() -> SolidError {
        SolidError::blend(
            OP,
            BlendFailure::FilletSetUpInvalid(FilletSetUp::SolverExecutive),
        )
    }

    fn cancelled() -> SolidError {
        SolidError::Cancelled { operation: OP }
    }

    /// #20757's review: a rail end on G the solver has not solved yet proves no roll-over
    /// past the end face. `end_section_rolls_over` is then `false`, so `calc_end_edges`
    /// takes main's branch, `refuse_end_past_side_edge` (the refusal naming the radius at
    /// which the rail reaches its side edge's end), not `EndEdges::Built`. It decides before
    /// any solver lookup (every lookup here fails).
    #[test]
    fn an_unsolved_other_rail_end_proves_no_roll_over_and_keeps_main_s_refusal() {
        let (pseudo, g_end) = pseudo_with_vertex(FilStatus::Unprocessed);
        assert!(!rolls_over(&pseudo, Some(g_end), &FailingSolvers(lookup_failed)).unwrap());
    }

    /// #21099: a failure inside the roll-over test (here the solver lookup of a solved rail
    /// end, or a vertex the pseudo-B-rep does not know) proves nothing; the End is refused as
    /// on main, not ended in an error main never reached. Cancellation still propagates.
    #[test]
    fn a_failed_roll_over_test_keeps_main_s_refusal_but_passes_cancellation_on() {
        let (pseudo, g_end) = pseudo_with_vertex(FilStatus::Processed);
        assert!(!rolls_over(&pseudo, Some(g_end), &FailingSolvers(lookup_failed)).unwrap());
        assert!(!rolls_over(
            &pseudo,
            Some(VertexId::from_index(7)),
            &FailingSolvers(lookup_failed)
        )
        .unwrap());
        assert!(matches!(
            rolls_over(&pseudo, Some(g_end), &FailingSolvers(cancelled)),
            Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ));
    }

    /// #20757's review (CodeRabbit): on a non-cliff End both rails can miss their side
    /// edges. The roll-over is proven for one miss only; the other rail's end lies past its
    /// own side edge, so the section through it proves nothing. `end_section_rolls_over` is
    /// then `false` before any lookup, and `calc_end_edges` refuses the End as before #20757
    /// (`refuse_end_past_side_edge`), not `EndEdges::Built`. The same solved rail end, not
    /// missed, does reach the section test (its cancelled lookup propagates).
    #[test]
    fn a_second_side_edge_miss_proves_no_roll_over_and_keeps_main_s_refusal() {
        let (pseudo, g_end) = pseudo_with_vertex(FilStatus::Processed);
        assert!(!rolls_over(&pseudo, None, &FailingSolvers(cancelled)).unwrap());
        assert!(matches!(
            rolls_over(&pseudo, Some(g_end), &FailingSolvers(cancelled)),
            Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ));
    }

    /// The other rail's end `calc_end_edge_geom` hands `end_section_rolls_over`: the rail end
    /// that is not the miss, or `None` where it missed its side edge too, from either end.
    #[test]
    fn the_other_rail_end_is_none_where_it_missed_its_side_edge_too() {
        let start = VertexId::from_index(3);
        let end = VertexId::from_index(5);
        assert_eq!(other_rail_end(start, start, end, [true, true]), None);
        assert_eq!(other_rail_end(start, start, end, [true, false]), Some(end));
        assert_eq!(other_rail_end(end, start, end, [true, true]), None);
        assert_eq!(other_rail_end(end, start, end, [false, true]), Some(start));
    }
}
