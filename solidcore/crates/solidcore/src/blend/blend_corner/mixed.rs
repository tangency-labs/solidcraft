//! The blend corner's Mixed event: a convex blend B1 and a concave blend B2 whose edges
//! continue each other G1 at the vertex V, on a common face T (a plane), the third edge
//! at V, not blended, a straight tangent seam between their other faces F1 and F2 (the
//! tangent peg's front top edge and its concave arc, where the front face runs tangent
//! into the cylinder). A fourth edge at V, not the fork's (#2953): the seam's line runs on
//! past V as an edge between F2 and the face beyond the seam, tangent to F2 there (the
//! drafted peg's cone, split along its generator through the seam). V-Q then lies on that
//! edge, as P-V lies on the seam.
//!
//! Upstream: none (pinned SMLib builds this corner with `SmFillet3x2MixedConvexityCorner`).
//! The fork's `source/SMLib/src/SmBlendCornerMixed.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! and its dispatch in `SmBlendCorner.cpp` and `SmFilletCorner.cpp` (tangency-labs/smlib
//! d443f10 on dda881f, branch `wip/82-mixed-corner`: 6fd0411, 6e79de5, e1a2153; smlib#82).
//! From the fork's file head: on T the blends' rails cross at X. Each blend is trimmed
//! along the other's rail continued past X: B1 by the surface S2 swept by B2's rail on T
//! along T's normal, from X to A on its rail on F1, and B2 by S1, from X to B on its rail
//! on F2. On the wall F1 + F2 the corner runs along B1's rail from A to P, where it meets
//! the seam, up the seam's line past V to Q, where B2's rail on F2 crosses it, and along
//! B2's rail back to B. The corner splits along the arc X-V in T: the face L (X, A, P, V)
//! with B1 and the face U (X, V, Q, B) with B2, each a Coons patch in R^4, bicubically
//! blended for rounds ([`algebra`]).
//!
//! | Rust                      | Fork                                                  |
//! |---------------------------|-------------------------------------------------------|
//! | [`MixedCorner`]           | `SmBlendCorner`'s `m_apMixedSurface`, `m_vMixed3d`, `m_vMixedUV`, `m_vMixedOrients`, and `SmMixedRing` |
//! | [`classify_mixed`]        | `SmBlendCorner::ClassifyMixed`                        |
//! | `mixed_ring` (private)    | `sm_MixedRing`                                        |
//! | `leaving` (crate)         | `sm_MixedLeaving`                                     |
//! | `param_of` (private)      | `sm_MixedParam`                                       |
//! | `curve_surface` (private) | `sm_MixedCurveSurface`                                |
//! | `rail_wall` (private)     | `sm_MixedRailWall`                                    |
//! | `FilletCorner::make_mixed_topology` (`fillet_corner::topology`) | `SmBlendCorner::MakeMixedTopology` |
//! | [`calc_mixed_vert_geom`]  | `SmBlendCorner::CalcMixedVertGeom`                    |
//! | [`calc_mixed_edge_geom`]  | `SmBlendCorner::CalcMixedEdgeGeom` (before the base's `CalcCornerEdgeGeom`) |
//! | [`create_mixed_patches`]  | `SmBlendCorner::CreateMixedPatches`                   |
//!
//! Deviations: the ring (`sm_MixedRing`) is read once, by the topology, and kept in
//! [`MixedCorner`] with the blends' solvers' identities; the fork reads it again in
//! each step from the same unchanged ring. The two faces' surfaces and loops are kept in
//! [`MixedCorner`] from the edge geometry until the patches take them. `SER_MSG` texts
//! are in comments; their status is `SM_ERR` (WR-697, its 2026-11-02 Mixed addendum; `leaving` follows the fork's b785227, its
//! 2026-11-02 leaving-tangent addendum). No recursion here; [`algebra`]'s root search
//! recurses at most 61 deep.

pub mod algebra;

use algebra::{Crv, Side, TwistTerm, V4};

use crate::blend::blend_corner::swept::SweptSolvers;
use crate::blend::fillet_corner::p1::{face_by_two_edgeuses, test_g1_continuity};
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner};
use crate::blend::fillet_corner::topology::check_tangential_edgeuse_at_vertex;
use crate::blend::fillet_geom::p1::{FilStatus, FilletBrep, FilletGeomId, FilletSolverId};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::{edge_bounds_material_linked, is_concave_edge};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::{
    BlendFailure, CornerInput, CornerStep, FilletSetUp, MixedStep, SolverFailure,
};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::p2::IsoCurveOptions;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::crv_adv::p1::CurveEnd;
use crate::nurbs::knots::Side as KnotSide;
use crate::query::edge_turn::{leaving_tangent, Leaving};
use crate::surface::procedural_blend::corner::CornerDefinition;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::mixed::classify_mixed";
const OP_VERT: &str = "blend::blend_corner::mixed::calc_mixed_vert_geom";
const OP_EDGE: &str = "blend::blend_corner::mixed::calc_mixed_edge_geom";
const OP_PATCH: &str = "blend::blend_corner::mixed::create_mixed_patches";

/// `a[i]` of a pair: the first for `i == 0`, else the second.
fn of2<T>(a: &[T; 2], i: usize) -> &T {
    let [x, y] = a;
    if i == 0 {
        x
    } else {
        y
    }
}

/// `a[i]` of a pair, mutably: the first for `i == 0`, else the second.
fn of2m<T>(a: &mut [T; 2], i: usize) -> &mut T {
    let [x, y] = a;
    if i == 0 {
        x
    } else {
        y
    }
}

/// `a[i]` of the corner's eight sides: the last for `i >= 7` (every index the corner
/// uses is a constant under eight).
fn of8(a: &[Side; 8], i: usize) -> &Side {
    let [s0, s1, s2, s3, s4, s5, s6, s7] = a;
    match i {
        0 => s0,
        1 => s1,
        2 => s2,
        3 => s3,
        4 => s4,
        5 => s5,
        6 => s6,
        _ => s7,
    }
}

/// `a[i]` of the corner's eight sides, mutably: the last for `i >= 7`.
fn of8m(a: &mut [Side; 8], i: usize) -> &mut Side {
    let [s0, s1, s2, s3, s4, s5, s6, s7] = a;
    match i {
        0 => s0,
        1 => s1,
        2 => s2,
        3 => s3,
        4 => s4,
        5 => s5,
        6 => s6,
        _ => s7,
    }
}

/// A curve of the corner's algebra has no segment or control point.
const EMPTY_CURVE: &str = "a curve of the Mixed corner's algebra has no segment or no control point, though from_bspline, compose and piece make one segment of degree + 1 points at least";

/// A defect of `op`: a step the Mixed corner's own checks or constructions promise did
/// not hold (`why`).
#[track_caller]
fn defect(op: &'static str, why: &'static str) -> SolidError {
    SolidError::blend(op, BlendFailure::Defect { why })
}

/// A defect of `op` where something the kernel built is missing (`why`), with the
/// `SM_ERR_NULL_POINTER` status of the null pointer it replaces.
#[track_caller]
fn missing_defect(op: &'static str, why: &'static str) -> SolidError {
    Failure::new(op, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// Invalid input to `op`: the caller's corner, fillet B-rep or body is not what the corner's
/// construction reads (`input`), about `entity` where the check reads one, with the status
/// the site returned before ([`CornerInput::upstream_status`]). The entry points are public
/// and take a caller's corner.
#[track_caller]
fn bad_input(op: &'static str, input: CornerInput, entity: Option<EntityRef>) -> SolidError {
    corner_input(op, input, entity).into_error()
}

/// [`bad_input`] with the site's own `status` where its C++ reports the condition otherwise
/// than the kind's default ([`CornerInput::upstream_status`]): a failed state check,
/// `SE(SM_ERR)`, where the default is `SM_ERR_NULL_POINTER`.
#[track_caller]
fn bad_input_status(op: &'static str, input: CornerInput, status: i32) -> SolidError {
    corner_input(op, input, None)
        .with_status(status)
        .into_error()
}

/// The [`Failure`] of [`bad_input`], located at `entity` where there is one.
#[track_caller]
fn corner_input(op: &'static str, input: CornerInput, entity: Option<EntityRef>) -> Failure {
    let failure = Failure::new(op, BlendFailure::CornerInputInvalid { input, entity });
    match entity {
        Some(at) => failure.at(at),
        None => failure,
    }
}

/// A face of the caller's body that the corner reads, `face`, has no surface: the caller's
/// input, [`BlendFailure::FaceHasNoSurface`] at the face, status `SM_ERR_NULL_POINTER` (the
/// site's).
#[track_caller]
fn face_without_surface(op: &'static str, face: FaceId) -> SolidError {
    Failure::new(op, BlendFailure::FaceHasNoSurface)
        .with_status(SM_ERR_NULL_POINTER)
        .at(EntityRef::Face(face))
        .into_error()
}

/// An edge of the caller's body that the corner reads, `edge`, has no curve: the caller's
/// input, [`BlendFailure::EdgeWithoutCurve`] at the edge, status `SM_ERR_NULL_POINTER`.
#[track_caller]
fn edge_without_curve(op: &'static str, edge: EdgeId) -> SolidError {
    SolidError::blend_at(op, BlendFailure::EdgeWithoutCurve, EntityRef::Edge(edge))
}

/// An edge use of the caller's body or fillet B-rep, `edgeuse`, has no edge: the caller's
/// input, [`BlendFailure::EdgeUseWithoutEdge`] at the edge use, status
/// `SM_ERR_NULL_POINTER`.
#[track_caller]
fn edge_use_without_edge(op: &'static str, edgeuse: EdgeuseId) -> SolidError {
    SolidError::blend_at(
        op,
        BlendFailure::EdgeUseWithoutEdge,
        EntityRef::Edgeuse(edgeuse),
    )
}

/// Invalid input to `op`: the caller's fillet solver set-up lacks what the corner reads
/// (`cause`, [`BlendFailure::Solver`]), about `entity` where the check reads one; status
/// the cause's, `SM_ERR_NULL_POINTER`, the site's.
#[track_caller]
fn solver_input(op: &'static str, cause: SolverFailure, entity: Option<EntityRef>) -> SolidError {
    let failure = Failure::new(op, BlendFailure::Solver(cause));
    match entity {
        Some(at) => failure.at(at),
        None => failure,
    }
    .into_error()
}

/// Invalid input to `op`: a link of the caller's fillet structures that the corner reads is
/// not there (`part`), about `entity` where the check reads one; status
/// `SM_ERR_NULL_POINTER`, the site's.
#[track_caller]
fn bad_setup(op: &'static str, part: FilletSetUp, entity: Option<EntityRef>) -> SolidError {
    let failure = Failure::new(op, BlendFailure::FilletSetUpInvalid(part));
    match entity {
        Some(at) => failure.at(at),
        None => failure,
    }
    .into_error()
}

/// The corner has no construction at `step` (status `SM_ERR`).
#[track_caller]
fn corner_unbuildable(op: &'static str, step: CornerStep) -> SolidError {
    SolidError::blend(op, BlendFailure::CornerUnbuildable(step))
}

/// The Mixed corner's geometry has no construction at `step` (status `SM_ERR`).
#[track_caller]
fn unbuildable(op: &'static str, step: MixedStep) -> SolidError {
    SolidError::blend(op, BlendFailure::MixedCornerUnbuildable(step))
}

/// A curve or surface kind with no B-spline form at `step`, with the
/// `SM_ERR_NULL_POINTER` status of the null cast it replaces.
#[track_caller]
fn not_bspline(op: &'static str, step: MixedStep, at: EntityRef) -> SolidError {
    Failure::new(op, BlendFailure::MixedCornerUnbuildable(step))
        .with_status(SM_ERR_NULL_POINTER)
        .at(at)
        .into_error()
}

/// Blend `i`'s blended edge use at the corner: B1's (the convex one) for 0, B2's (the
/// concave one) otherwise; where a failure of that blend's geometry is reported.
fn blend_edgeuse(ring: &MixedRing, i: usize) -> EntityRef {
    EntityRef::Edgeuse(if i == 0 { ring.convex } else { ring.concave })
}

/// `SmMixedRing`: the Mixed corner's parts at its vertex, read off the ring: the convex
/// and the concave blended edge uses, the seam, the faces T, F1 (B1's other), F2 (B2's
/// other).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixedRing {
    /// `pConvex`.
    pub convex: EdgeuseId,
    /// `pConcave`.
    pub concave: EdgeuseId,
    /// The convex blend's edge (`pConvex`'s; not the fork's member, #19829).
    pub convex_edge: EdgeId,
    /// The concave blend's edge (`pConcave`'s; not the fork's member, #19829).
    pub concave_edge: EdgeId,
    /// `pSeam`.
    pub seam: EdgeuseId,
    /// Not the fork's (#2953): the fourth edge at V, the seam's line run on past V as an
    /// edge between F2 and the seam's other face; `None` at a vertex of three edges.
    pub beyond: Option<EdgeuseId>,
    /// `pT`.
    pub t: FaceId,
    /// `pF1`.
    pub f1: FaceId,
    /// `pF2`.
    pub f2: FaceId,
}

/// The Mixed corner's own members on its corner: the ring and its blends' solvers
/// (`SmMixedRing`'s `apFS`, B1 then B2), and, from the edge geometry until the patches
/// take them, the two faces' surfaces (`m_apMixedSurface`, L then U) and loops
/// (`m_vMixed3d`, `m_vMixedUV`, `m_vMixedOrients`).
#[derive(Debug, Clone)]
pub struct MixedCorner {
    /// The ring.
    pub ring: MixedRing,
    /// `apFS`: B1's and B2's solvers.
    pub solvers: [FilletSolverId; 2],
    /// `m_apMixedSurface`.
    pub surfaces: [Option<Surface>; 2],
    /// `m_vMixed3d`.
    pub curves_3d: [Vec<Curve>; 2],
    /// `m_vMixedUV`.
    pub uv_curves: [Vec<Curve>; 2],
    /// `m_vMixedOrients`.
    pub orients: [Vec<OrientType>; 2],
}

impl MixedCorner {
    /// A Mixed corner on `ring` with no surfaces yet.
    #[must_use]
    pub fn new(ring: MixedRing, solvers: [FilletSolverId; 2]) -> Self {
        Self {
            ring,
            solvers,
            surfaces: [None, None],
            curves_3d: [Vec::new(), Vec::new()],
            uv_curves: [Vec::new(), Vec::new()],
            orients: [Vec::new(), Vec::new()],
        }
    }

    /// `DeleteMixedSurfaces`.
    fn delete_surfaces(&mut self) {
        for (((c3, uv), orient), surface) in self
            .curves_3d
            .iter_mut()
            .zip(self.uv_curves.iter_mut())
            .zip(self.orients.iter_mut())
            .zip(self.surfaces.iter_mut())
        {
            c3.clear();
            uv.clear();
            orient.clear();
            *surface = None;
        }
    }
}

/// `sm_MixedRing`: the ring's parts, `None` where the vertex is not a Mixed one's shape
/// (not two solver edge uses of opposite convexity among three or four, a blended edge
/// that bounds no material as a sheet's does (not the fork's: its convexity test fails
/// there; #19829), no common face,
/// no other face of each blend at the seam, or a blend without a solver). `solver_of` is
/// `GetFilletSolverOfEdgeuse` (`None`: `NULL`); the blends' solvers are returned with
/// the ring (B1, B2), each of its edge use or else of its mate. Of four edges (not the
/// fork's, #2953) the two not blended are the seam, on B1's other face, and the edge
/// beyond it, on B2's, with one face between them.
pub(crate) fn mixed_ring<S: Copy>(
    k: &Kernel,
    b: &mut Brep,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    solver_of: &dyn Fn(EdgeuseId) -> Option<S>,
) -> Result<Option<(MixedRing, [S; 2])>> {
    let &[eu0, eu1_] = solver_eus else {
        return Ok(None);
    };
    let e0 = eu1::edge(b, eu0)?.ok_or_else(|| edge_use_without_edge(OP_CLASSIFY, eu0))?;
    let e1 = eu1::edge(b, eu1_)?.ok_or_else(|| edge_use_without_edge(OP_CLASSIFY, eu1_))?;
    // a sheet's edge bounds no material and is neither convex nor concave: no Mixed ring
    // (`is_concave_edge` refuses it; #19829). A loop use without a shell or region is
    // refused as `is_concave_edge` refused it; both edges are asked first, so a sheet
    // answer at one never hides a broken link at the other.
    let bounds = [
        edge_bounds_material_linked(b, e0)?,
        edge_bounds_material_linked(b, e1)?,
    ];
    if !bounds[0] || !bounds[1] {
        return Ok(None);
    }
    let concave = [is_concave_edge(k, b, e0)?, is_concave_edge(k, b, e1)?];
    if concave[0] == concave[1] {
        return Ok(None);
    }
    let convex_eu = if concave[0] { eu1_ } else { eu0 };
    let concave_eu = if concave[0] { eu0 } else { eu1_ };
    let edges = if concave[0] { [e1, e0] } else { [e0, e1] };
    let rest: Vec<EdgeuseId> = all_eus
        .iter()
        .copied()
        .filter(|&eu| eu != eu0 && eu != eu1_)
        .collect();
    let orders = match rest.as_slice() {
        [seam] if all_eus.len() == 3 => vec![(*seam, None)],
        [x, y] if all_eus.len() == 4 => vec![(*x, Some(*y)), (*y, Some(*x))],
        _ => return Ok(None),
    };
    for (seam, beyond) in orders {
        let found = ring_of(b, [convex_eu, concave_eu], edges, seam, beyond, solver_of)?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

/// The face of `eu`'s sector other than `not`, where the sector holds `not`.
fn other_face(b: &Brep, eu: EdgeuseId, not: FaceId) -> Result<Option<FaceId>> {
    let radial = eu1::radial(b, eu)?.ok_or_else(|| {
        bad_setup(
            OP_CLASSIFY,
            FilletSetUp::RadialEdgeUse,
            Some(EntityRef::Edgeuse(eu)),
        )
    })?;
    let faces = [eu1::face(b, eu)?, eu1::face(b, radial)?];
    Ok(match faces {
        [Some(f), Some(g)] if f == not => Some(g),
        [Some(f), Some(g)] if g == not => Some(f),
        _ => None,
    })
}

/// [`mixed_ring`]'s parts for one choice of the seam and the edge beyond it.
fn ring_of<S: Copy>(
    b: &Brep,
    [convex_eu, concave_eu]: [EdgeuseId; 2],
    [convex_edge, concave_edge]: [EdgeId; 2],
    seam: EdgeuseId,
    beyond: Option<EdgeuseId>,
    solver_of: &dyn Fn(EdgeuseId) -> Option<S>,
) -> Result<Option<(MixedRing, [S; 2])>> {
    let Some(t) = face_by_two_edgeuses(b, Some(convex_eu), Some(concave_eu), Some(seam))? else {
        return Ok(None);
    };
    let mut others = [None, None];
    let mut solvers = [None, None];
    for (ii, blend) in [convex_eu, concave_eu].into_iter().enumerate() {
        // B2's other face is at the edge beyond the seam, where there is one
        let side = if ii == 1 {
            beyond.unwrap_or(seam)
        } else {
            seam
        };
        let radial = eu1::radial(b, side)?;
        let side_faces = [
            eu1::face(b, side)?,
            match radial {
                Some(r) => eu1::face(b, r)?,
                None => None,
            },
        ];
        let mut other = None;
        for &sf in &side_faces {
            if sf.is_some()
                && sf != Some(t)
                && face_by_two_edgeuses(b, Some(blend), Some(side), None)? == sf
            {
                other = sf;
            }
        }
        let Some(other) = other else {
            return Ok(None);
        };
        *of2m(&mut others, ii) = Some(other);
        let mut fs = solver_of(blend);
        if fs.is_none() {
            if let Some(mate) = eu1::mate(b, blend)? {
                fs = solver_of(mate);
            }
        }
        let Some(fs) = fs else {
            return Ok(None);
        };
        *of2m(&mut solvers, ii) = Some(fs);
    }
    let ([Some(f1), Some(f2)], [Some(s0), Some(s1)]) = (others, solvers) else {
        return Ok(None);
    };
    if f1 == f2 {
        return Ok(None);
    }
    if let Some(beyond) = beyond {
        // one face between the seam and the edge beyond it, and it is not T
        let between = other_face(b, seam, f1)?;
        if between.is_none() || between == Some(t) || between != other_face(b, beyond, f2)? {
            return Ok(None);
        }
    }
    Ok(Some((
        MixedRing {
            convex: convex_eu,
            concave: concave_eu,
            convex_edge,
            concave_edge,
            seam,
            beyond,
            t,
            f1,
            f2,
        },
        [s0, s1],
    )))
}

/// `SmBlendCorner::ClassifyMixed`: whether the filleted `vertex` is a Mixed corner: three
/// edges, two blended of opposite convexity continuing each other G1 there (within
/// `tangency_tol_deg`), the third a straight tangent seam between their other faces, the
/// blends' common face a plane (each to the smaller of the blends' tolerances). A fourth
/// edge (not the fork's, #2953) is the seam's line run on past `vertex`, tangent there too.
///
/// "Not mixed" is a value, `Ok(false)`: every test of the shape that does not hold, of
/// the ring (a blended edge of a sheet, which bounds no material, included), the
/// continuity, the tangency, the planarity and the lines. An error is a failure, never
/// "not mixed": the caller passes it on.
///
/// # Errors
///
/// Those of the ring ([`BlendFailure::EdgeUseWithoutEdge`], status
/// `SM_ERR_NULL_POINTER`, at a blended edge use of the caller's body that has no edge;
/// the convexity and face lookups), of the continuity, planarity and line tests, and of
/// the leaving tangents of the seam and the edge beyond it (read only once both are
/// lines).
pub fn classify_mixed<'s>(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    solver_of: &dyn Fn(EdgeuseId) -> Option<&'s FilletSolver>,
    tangency_tol_deg: f64,
) -> Result<bool> {
    let Some((ring, [fs0, fs1])) = mixed_ring(k, b, solver_eus, all_eus, solver_of)? else {
        return Ok(false);
    };
    if !test_g1_continuity(
        k,
        b,
        Some(vertex),
        Some(ring.convex_edge),
        Some(ring.concave_edge),
        tangency_tol_deg,
    )?
    .0 || !check_tangential_edgeuse_at_vertex(k, b, ring.seam, vertex, tangency_tol_deg)?
    {
        return Ok(false);
    }
    let tol = min(fs0.this_approx_tol_3d(), fs1.this_approx_tol_3d());
    let planar = match b.face(ring.t)?.surface.as_ref() {
        Some(s) => s.is_planar(k, tol)?.is_planar,
        None => false,
    };
    if !planar {
        return Ok(false);
    }
    // the seam's edge, which its tangency test just read: without one, not tangential
    let Some(seam_e) = eu1::edge(b, ring.seam)? else {
        return Ok(false);
    };
    let line = match edge::curve(b, seam_e)? {
        Some(c) => c.is_line(k, 10, tol)?.is_some(),
        None => false,
    };
    if !line {
        return Ok(false);
    }
    let Some(beyond) = ring.beyond else {
        return Ok(true);
    };
    if !check_tangential_edgeuse_at_vertex(k, b, beyond, vertex, tangency_tol_deg)? {
        return Ok(false);
    }
    // the same for the edge beyond
    let Some(beyond_e) = eu1::edge(b, beyond)? else {
        return Ok(false);
    };
    let straight = match edge::curve(b, beyond_e)? {
        Some(c) => c.is_line(k, 10, tol)?.is_some(),
        None => false,
    };
    // an edge beyond that is not a line is not mixed, whatever its tangent: its tangent
    // is read only on a line, as the seam's is
    if !straight {
        return Ok(false);
    }
    let down = leaving(k, b, seam_e, vertex, OP_CLASSIFY)?;
    let up = leaving(k, b, beyond_e, vertex, OP_CLASSIFY)?;
    Ok(down.dot(up) < 0.0 && down.is_parallel_to(up, tangency_tol_deg))
}

/// `smos_Min`.
fn min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `smos_Max`.
fn max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `SM_CAST_PTR(SmBSplineCurve, ..)`: the B-spline of the kinds derived from
/// `SmBSplineCurve`.
fn bspline_curve(curve: &Curve) -> Option<&BSplineCurve> {
    match curve.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        _ => None,
    }
}

/// `SM_CAST_PTR(SmBSplineSurface, ..)`: the B-spline net of the kinds derived from
/// `SmBSplineSurface`.
fn bspline_surface(s: &Surface) -> Option<&BSplineSurface> {
    match s.kind() {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        SurfaceKind::Cone(c) => Some(c.rev().base()),
        SurfaceKind::Sphere(s) => Some(s.base().base()),
        SurfaceKind::Torus(t) => Some(t.base().base()),
        _ => None,
    }
}

/// The objects the Mixed corner's solves name (upstream's `SmObject*`): the curve and
/// the surface of one solve, with their natural intervals and domains.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MxObject {
    Curve(u8, Extent1d),
    Surface(Extent2d),
}

impl SolutionObject for MxObject {
    fn kind(&self) -> ObjectKind {
        match self {
            Self::Curve(..) => ObjectKind::Curve,
            Self::Surface(_) => ObjectKind::Surface,
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Self::Curve(_, ivl) => Some(*ivl),
            Self::Surface(_) => None,
        }
    }
    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Self::Curve(..) => None,
            Self::Surface(d) => Some(*d),
        }
    }
}

/// `sm_MixedLeaving`: the unit tangent of an edge leaving a vertex.
///
/// An edge's start vertex is its curve's start (its interval's least parameter), so the
/// curve's derivative taken inside the edge points into it at its start and out of it at
/// its end, where it is turned. Inside the edge means one-sided: from the interval above
/// the start (`from_left` true) and from the interval below the end (`from_left` false);
/// [`edge::end_tangent`] takes both from above, which at the end reads past the edge. A
/// closed edge leaves its vertex at its start (fork b785227). The evaluation is
/// [`crate::query::edge_turn::leaving_derivative`]'s ([`crate::query::edge_turn::leaving_tangent`]);
/// this adds the closed edge's start and
/// this module's errors (a vertex the edge does not meet, no curve, a tangent of no length).
pub(crate) fn leaving(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    vertex: VertexId,
    op: &'static str,
) -> Result<Vec3> {
    let at_start = edge::start_vertex(b, e)? == Some(vertex);
    if !at_start && edge::end_vertex(b, e)? != Some(vertex) {
        return Err(bad_input(
            op,
            CornerInput::EdgeOffVertex,
            Some(EntityRef::Edge(e)),
        ));
    }
    // the one evaluation, inside the edge and turned away from the vertex
    let d = match leaving_tangent(k, b, e, at_start)? {
        Leaving::NoCurve => {
            return Err(edge_without_curve(op, e))
        }
        Leaving::NoTangent => {
            return Err(defect(
                op,
                "the curve's evaluation at the vertex returned no first derivative, though one was asked for",
            ))
        }
        Leaving::Tangent(d) => d,
    };
    d.unitize()
}

/// `sm_MixedParam`: the parameter of the point of `curve` nearest `pnt`.
fn param_of(k: &Kernel, curve: &Curve, pnt: Point3, tol: f64, op: &'static str) -> Result<f64> {
    let ivl = curve.natural_interval();
    let sols = curve.global_point_solve::<MxObject>(
        k,
        &ivl,
        SolverOperation::Minimize,
        pnt,
        tol,
        SolutionRequested::Single,
        &crate::curve::curve::p1::GlobalPointSolveOptions {
            handle: Some(MxObject::Curve(0, ivl)),
            analytic_handle: Some(MxObject::Curve(1, ivl)),
            ..Default::default()
        },
    )?;
    // the tree solve's own failures are dropped, so the solve can end empty
    let first = sols
        .get(0)
        .ok_or_else(|| unbuildable(op, MixedStep::NoClosestPointOnRail))?;
    first.start.param(0)
}

/// `sm_MixedCurveSurface`: the crossing of a curve with a surface nearest `near`.
fn curve_surface(
    k: &Kernel,
    curve: &Curve,
    surface: &Surface,
    near: Point3,
    tol: f64,
    op: &'static str,
) -> Result<Point3> {
    let dom = surface.natural_uv_domain_with_nurb(k)?;
    let ivl = curve.natural_interval();
    let sols = surface.global_curve_intersect::<MxObject>(
        k,
        [MxObject::Curve(0, ivl), MxObject::Surface(dom)],
        &dom,
        curve,
        &ivl,
        tol,
    )?;
    let mut best = BIG_DOUBLE;
    let mut out = None;
    for sol in sols.as_slice() {
        let pnt = curve.evaluate_point(k, sol.start.param(0)?)?;
        if pnt.distance(near) < best {
            best = pnt.distance(near);
            out = Some(pnt);
        }
    }
    // Mixed corner: a rail does not meet the other blend's rail surface
    out.ok_or_else(|| unbuildable(op, MixedStep::RailMissesRailWall))
}

/// `sm_MixedRailWall`: the surface swept by a rail on the plane T along T's normal,
/// reaching `reach` either side; the rail first continued past both ends as itself (its
/// end spans' own polynomials) by a quarter of `reach`.
fn rail_wall(
    rail: &Curve,
    normal: Vec3,
    reach: f64,
    op: &'static str,
    rail_at: RailAt,
    at: EntityRef,
) -> Result<Surface> {
    let b0 = bspline_curve(rail).ok_or_else(|| rail_not_bspline(op, rail_at, rail, at))?;
    let b1 = b0
        .create_extended_curve_by_distance(
            0.25 * reach,
            CurveEnd::Start,
            ContinuityType::CInfinity,
        )?
        .ok_or_else(|| {
            missing_defect(
                op,
                "extending the rail at its start returned no curve, though the extension by distance returns its curve whenever it succeeds",
            )
        })?;
    let mut bb = b1
        .create_extended_curve_by_distance(0.25 * reach, CurveEnd::End, ContinuityType::CInfinity)?
        .ok_or_else(|| {
            missing_defect(
                op,
                "extending the rail at its end returned no curve, though the extension by distance returns its curve whenever it succeeds",
            )
        })?;
    let shift = normal * (-reach);
    bb.transform(
        &Axis2Placement::new(
            Point3::new(shift.x, shift.y, shift.z),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ),
        None,
    )?;
    let wall = BSplineSurface::create_linear_sweep(&mut bb, normal * (2.0 * reach))?;
    Ok(Surface::from(wall))
}

/// The rail curves of B1 and B2 on T and on their walls (F1, F2), with their rails'
/// indices.
struct Rails {
    on_t: [Curve; 2],
    on_wall: [Curve; 2],
    /// Each rail on T's fillet geometry and side, where a failure of its curve is reported.
    t_at: [RailAt; 2],
    /// Each rail on its wall's fillet geometry and side.
    wall_at: [RailAt; 2],
}

/// A rail's fillet geometry and side ([`SolverFailure::RailNotBSpline`]).
#[derive(Clone, Copy)]
struct RailAt {
    fillet_geom: FilletGeomId,
    side: usize,
}

/// Rail `at` has no B-spline form for the corner's exact algebra: the caller's fillet
/// geometry ([`SolverFailure::RailNotBSpline`], with the curve's kind), at blend edge use
/// `entity`; status `SM_ERR_NULL_POINTER`, the site's.
#[track_caller]
fn rail_not_bspline(op: &'static str, at: RailAt, rail: &Curve, entity: EntityRef) -> SolidError {
    solver_input(
        op,
        SolverFailure::RailNotBSpline {
            fillet_geom: at.fillet_geom,
            side: at.side,
            kind: rail.type_string(),
        },
        Some(entity),
    )
}

fn rails(
    target: &Brep,
    pseudo: &FilletBrep,
    mixed: &MixedCorner,
    solvers: [&FilletSolver; 2],
    op: &'static str,
) -> Result<Rails> {
    // a blend's rails on T and on its wall
    let pair = |fs: &FilletSolver, wall: FaceId| -> Result<([Curve; 2], [RailAt; 2])> {
        let g = fs.first_fillet_geom()?;
        let geom = pseudo.geom(g)?;
        let it = fs.find_index_of_rail_on_face(target, Some(mixed.ring.t))?;
        let iw = fs.find_index_of_rail_on_face(target, Some(wall))?;
        let rail_curve =
            |idx| -> Result<Curve> {
                let rail = geom.rail(idx).ok_or_else(|| {
                    solver_input(
                        op,
                        SolverFailure::FilletGeomWithoutRail {
                            fillet_geom: g,
                            side: idx,
                        },
                        None,
                    )
                })?;
                pseudo.brep().edge(rail)?.curve.clone().ok_or_else(|| {
                    bad_setup(op, FilletSetUp::RailCurve, Some(EntityRef::Edge(rail)))
                })
            };
        let at = |side| RailAt {
            fillet_geom: g,
            side,
        };
        Ok(([rail_curve(it)?, rail_curve(iw)?], [at(it), at(iw)]))
    };
    let ([t0, w0], [t0_at, w0_at]) = pair(solvers[0], mixed.ring.f1)?;
    let ([t1, w1], [t1_at, w1_at]) = pair(solvers[1], mixed.ring.f2)?;
    Ok(Rails {
        on_t: [t0, t1],
        on_wall: [w0, w1],
        t_at: [t0_at, t1_at],
        wall_at: [w0_at, w1_at],
    })
}

/// T's plane (`s`, T's surface): its unit normal.
fn plane_normal(k: &Kernel, s: &Surface, tol: f64, op: &'static str) -> Result<Vec3> {
    let planar = s.is_planar(k, tol)?;
    if !planar.is_planar {
        return Err(corner_unbuildable(op, CornerStep::CommonFaceNotPlanar));
    }
    planar.normal.unitize()
}

/// `SmBlendCorner::CalcMixedVertGeom` (the fork's 6fd0411): the Mixed corner's vertices
/// once both blends are traced (X, the rails' crossing, and V by their own kinds, from
/// the base's vertex geometry): A and B, where each blend's rail on the wall meets the
/// other's rail on T swept along T's normal; P, where B1's rail on F1 meets the seam's
/// line on the seam's side of V; Q, where B2's rail on F2 crosses it past V. Nothing
/// while a blend is not traced (the corner's second pass places them).
///
/// # Errors
///
/// `SM_ERR` for a corner that is not a Mixed one of six vertices and seven edges, or
/// with X or V not processed, T not planar, or a crossing not found;
/// [`SolidError::null_pointer`] for missing rails or curves; those of the solves.
pub fn calc_mixed_vert_geom(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn SweptSolvers,
) -> Result<()> {
    let mixed = corner
        .mixed
        .as_ref()
        // the site reports a missing Mixed event as a failed check, `SM_ERR`
        .ok_or_else(|| bad_input_status(OP_VERT, CornerInput::EventNotSet, crate::error::SM_ERR))?;
    let &[x, a, p, v, q, bv] = corner.vertices.as_slice() else {
        return Err(bad_input(OP_VERT, CornerInput::TopologyMismatch, None));
    };
    if corner.edges.len() != 7 {
        return Err(bad_input(OP_VERT, CornerInput::TopologyMismatch, None));
    }
    let fs = [
        solvers.solver(mixed.solvers[0])?,
        solvers.solver(mixed.solvers[1])?,
    ];
    for s in fs {
        let g = s.first_fillet_geom()?;
        if pseudo.geom(g)?.fillet_surface(None)?.is_none() {
            return Ok(()); // not traced yet: the corner's second pass places them
        }
    }
    if !pseudo.fillet_vertex(x)?.is_processed() || !pseudo.fillet_vertex(v)?.is_processed() {
        return Err(bad_input(OP_VERT, CornerInput::VertexNotProcessed, None));
    }
    let tol = corner.approx_tol_3d;
    let t_surface = target
        .face(mixed.ring.t)?
        .surface
        .as_ref()
        .ok_or_else(|| face_without_surface(OP_VERT, mixed.ring.t))?;
    let normal = plane_normal(k, t_surface, tol, OP_VERT)?;
    let sv = pseudo.vertex_point(v)?;
    let sx = pseudo.vertex_point(x)?;
    let rails = rails(target, pseudo, mixed, fs, OP_VERT)?;
    let reach = 4.0 * (sv.distance(sx) + 1.0);

    // A and B: each wall rail meets the other blend's rail on T swept along T's normal
    for ii in 0..2 {
        let wall = rail_wall(
            of2(&rails.on_t, 1 - ii),
            normal,
            reach,
            OP_VERT,
            *of2(&rails.t_at, 1 - ii),
            blend_edgeuse(&mixed.ring, 1 - ii),
        )?;
        let pnt = curve_surface(k, of2(&rails.on_wall, ii), &wall, sx, tol, OP_VERT)?;
        let fv = if ii == 0 { a } else { bv };
        pseudo.set_vertex_point(fv, pnt)?;
        pseudo
            .fillet_vertex_mut(fv)?
            .set_status(FilStatus::Processed);
    }

    // P: B1's wall rail crosses the seam; Q: B2's wall rail crosses the seam's line past V
    let seam_e = eu1::edge(target, mixed.ring.seam)?
        .ok_or_else(|| edge_use_without_edge(OP_VERT, mixed.ring.seam))?;
    let vertex = corner
        .vertex
        .ok_or_else(|| bad_setup(OP_VERT, FilletSetUp::CornerVertex, None))?;
    let along = leaving(k, target, seam_e, vertex, OP_VERT)?;
    let line = Curve::from(BSplineCurve::create_line_segment(
        3,
        sv + along * reach,
        sv - along * reach,
    )?);
    for ii in 0..2 {
        let wall_rail = of2(&rails.on_wall, ii);
        let (ivl0, ivl1) = (wall_rail.natural_interval(), line.natural_interval());
        let sols = wall_rail.global_curve_intersect::<MxObject>(
            k,
            &ivl0,
            &line,
            &ivl1,
            tol,
            &crate::curve::curve::p8::CurveIntersectOptions {
                skip_global_coincidence_check: false,
                handles: [
                    Some(MxObject::Curve(0, ivl0)),
                    Some(MxObject::Curve(1, ivl1)),
                ],
                analytic_handles: [
                    Some(MxObject::Curve(2, ivl0)),
                    Some(MxObject::Curve(3, ivl1)),
                ],
            },
        )?;
        let mut best = BIG_DOUBLE;
        let mut best_pnt = Point3::ZERO;
        for sol in sols.as_slice() {
            let pnt = line.evaluate_point(k, sol.start.param(1)?)?;
            // P on the seam's side of V (along it), Q past V
            if (ii == 0) != ((pnt - sv).dot(along) > 0.0) {
                continue;
            }
            if pnt.distance(sv) < best {
                best = pnt.distance(sv);
                best_pnt = pnt;
            }
        }
        if best == BIG_DOUBLE {
            // Mixed corner: a wall rail does not cross the seam's line
            return Err(unbuildable(OP_VERT, MixedStep::WallRailMissesSeamLine));
        }
        let fv = if ii == 0 { p } else { q };
        pseudo.set_vertex_point(fv, best_pnt)?;
        pseudo
            .fillet_vertex_mut(fv)?
            .set_status(FilStatus::Processed);
    }
    Ok(())
}

/// A side's unit tangent direction at its start (`t - p t_w`, `p` of weight 1).
fn start_dir(c: &Crv) -> Result<Vec3> {
    let t = algebra::crv_deriv_at(c, 0.0, 1)?;
    let p = algebra::eval(c, 0.0)?;
    Ok(Vec3::new(
        t[0] - p[0] * t[3],
        t[1] - p[1] * t[3],
        t[2] - p[2] * t[3],
    ))
}

/// The normal of face `face`'s surface at its point nearest `pnt` (`GlobalPointSolve` at
/// `SmZoneTol3d(tol)`, its fast drop's solutions tested on the face as the face-owned
/// surface's trimmed cache does; `EvaluateNormal(uv, TRUE, TRUE)`).
fn normal_at(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    pnt: Point3,
    tol: f64,
    op: &'static str,
) -> Result<Vec3> {
    let surface = b
        .face(face)?
        .surface
        .clone()
        .ok_or_else(|| face_without_surface(op, face))?;
    let dom = surface.natural_uv_domain_with_nurb(k)?;
    let mut test_on: Option<bool> = None;
    let mut classify = |k: &Kernel, uv: Point2, src_tol: f64| -> Result<bool> {
        crate::intersect::point_class::live_face_fast_drop_test(
            k,
            b,
            face,
            &mut test_on,
            uv,
            src_tol,
        )
    };
    let sols = surface.global_point_solve_face_tested::<MxObject>(
        k,
        Some(MxObject::Surface(dom)),
        &dom,
        SolverOperation::Minimize,
        pnt,
        crate::geom::tol::zone_tol3d(k, tol),
        None,
        SolutionRequested::Single,
        Some(&mut classify),
    )?;
    let sol = sols
        .get(0)
        .ok_or_else(|| unbuildable(op, MixedStep::NoFootOnWallFace))?;
    surface.evaluate_normal(
        k,
        Point2::new(sol.start.param(0)?, sol.start.param(1)?),
        KnotSide::Left,
        KnotSide::Left,
    )
}

/// `SmBlendCorner::CalcMixedEdgeGeom` (the fork's 6fd0411, 6e79de5, e1a2153): the Mixed
/// corner's edges and its two faces' surfaces, L (X, A, P, V) and U (X, V, Q, B), before
/// the base's `CalcCornerEdgeGeom` (the caller's).
///
/// The trimlines: each blend meets the other's rail wall in a certified intersection;
/// its uv curve on the blend, held at X and at its rail end exactly on the rails' iso
/// lines, is the trimline's definition, and the edge's curve is the blend over it
/// ([`algebra::compose`]). The rail pieces A-P and Q-B are the rails' own, the lines P-V
/// and V-Q the seam's, the inflection V-X the conic in T leaving X along an asymptotic
/// direction of the corner's shape operator and arriving at V along the edges. Each face
/// is the Coons patch of its sides ([`algebra::coons`]; chamfers' [`algebra::coons_chamfer`],
/// G0), every ribbon G1 to its neighbour by its field.
///
/// # Errors
///
/// `SM_ERR` for a corner that is not a Mixed one or whose vertices are not all processed,
/// and where the construction's own checks fail (the fork's `SER_MSG`s); those of the
/// intersections, solves and curve and surface constructions.
pub fn calc_mixed_edge_geom(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &dyn SweptSolvers,
) -> Result<()> {
    let Some(mixed) = corner.mixed.as_mut() else {
        // the site reports a missing Mixed event as a failed check, `SM_ERR`
        return Err(bad_input_status(
            OP_EDGE,
            CornerInput::EventNotSet,
            crate::error::SM_ERR,
        ));
    };
    let ring = mixed.ring;
    for &v in &corner.vertices {
        if !pseudo.fillet_vertex(v)?.is_processed() {
            return Err(bad_input(
                OP_EDGE,
                CornerInput::VertexNotProcessed,
                Some(EntityRef::Vertex(v)),
            ));
        }
    }
    let &[x, a, p, v, q, bv] = corner.vertices.as_slice() else {
        return Err(bad_input(OP_EDGE, CornerInput::TopologyMismatch, None));
    };
    let fs = [
        solvers.solver(mixed.solvers[0])?,
        solvers.solver(mixed.solvers[1])?,
    ];
    let geoms = [fs[0].first_fillet_geom()?, fs[1].first_fillet_geom()?];
    let tol = corner.approx_tol_3d;
    // chamfers meet their faces G0: the corner's faces are then the chamfer's Coons
    // patches of their sides
    let mut chamfer = false;
    for s in fs {
        if s.fillet_surface_generator()
            .ok_or_else(|| bad_setup(OP_EDGE, FilletSetUp::SurfaceGenerator, None))?
            .fillet_surface_generator_type()
            == FilletSurfaceGeneratorType::Linear
        {
            chamfer = true;
        }
    }
    let common = target
        .face(ring.t)?
        .surface
        .clone()
        .ok_or_else(|| face_without_surface(OP_EDGE, ring.t))?;
    let normal = plane_normal(k, &common, tol, OP_EDGE)?;
    let sx = pseudo.vertex_point(x)?;
    let sv = pseudo.vertex_point(v)?;
    let size = 1.0 + sx.max_dimension() + sv.distance(sx);
    let reach = 4.0 * (sv.distance(sx) + 1.0);
    let rails = rails(target, pseudo, mixed, fs, OP_EDGE)?;
    // the blends' surfaces, traced before the corner's edges
    let round_of = |g: FilletGeomId| -> Result<Surface> {
        Ok(pseudo
            .geom(g)?
            .fillet_surface(None)?
            .ok_or_else(|| {
                solver_input(
                    OP_EDGE,
                    SolverFailure::FilletGeomWithoutSurface { fillet_geom: g },
                    None,
                )
            })?
            .clone())
    };
    let rounds = [round_of(geoms[0])?, round_of(geoms[1])?];

    // the edges' directions at V, the seam's, the concave edge's axis (Omega through O)
    let vertex = corner
        .vertex
        .ok_or_else(|| bad_setup(OP_EDGE, FilletSetUp::CornerVertex, None))?;
    let convex_e = eu1::edge(target, ring.convex)?
        .ok_or_else(|| edge_use_without_edge(OP_EDGE, ring.convex))?;
    let seam_e =
        eu1::edge(target, ring.seam)?.ok_or_else(|| edge_use_without_edge(OP_EDGE, ring.seam))?;
    let concave_e = eu1::edge(target, ring.concave)?
        .ok_or_else(|| edge_use_without_edge(OP_EDGE, ring.concave))?;
    // along the convex edge into V, on along the concave one
    let chain = -leaving(k, target, convex_e, vertex, OP_EDGE)?;
    let seam_dir = leaving(k, target, seam_e, vertex, OP_EDGE)?;
    let convex_dir = chain;
    let omega = normal;
    let o = {
        // the concave edge's circle: its centre from three of its points
        let arc = edge::curve(target, concave_e)?
            .ok_or_else(|| edge_without_curve(OP_EDGE, concave_e))?;
        let ivl = edge::interval(target, concave_e)?;
        let p0 = arc.evaluate_point(k, ivl.evaluate(0.1))?;
        let p1 = arc.evaluate_point(k, ivl.evaluate(0.5))?;
        let p2 = arc.evaluate_point(k, ivl.evaluate(0.9))?;
        let va = p1 - p0;
        let vb = p2 - p0;
        let n = va.cross(vb);
        let dn = n.dot(n);
        if dn <= EFF_ZERO {
            return Err(unbuildable(OP_EDGE, MixedStep::ConcaveEdgeStraight));
        }
        // the circumcentre: p0 + (|a|^2 (b x n) + |b|^2 (n x a)) / (2 |n|^2)
        p0 + (vb.cross(n) * va.dot(va) + n.cross(va) * vb.dot(vb)) * (0.5 / dn)
    };

    // the outward side of T at X: B1 (convex) lies below it
    let mut out_sign = 1.0;

    // the trimlines: uv on B_i from X to its rail end, then B_i over it
    let mut trim: [Crv; 2] = [Crv::default(), Crv::default()];
    let mut trim_t: [Crv; 2] = [Crv::default(), Crv::default()];
    let mut pcs: [Crv; 2] = [Crv::default(), Crv::default()];
    for ii in 0..2 {
        let wall = rail_wall(
            of2(&rails.on_t, 1 - ii),
            normal,
            reach,
            OP_EDGE,
            *of2(&rails.t_at, 1 - ii),
            blend_edgeuse(&ring, 1 - ii),
        )?;
        let to = if ii == 0 { a } else { bv };
        let s_to = pseudo.vertex_point(to)?;
        let mut sdir = s_to - sx;
        let round = of2(&rounds, ii);
        let curves = crate::blend::fillet_geom::p2::srf_srf_intersection(
            k,
            Some(sx),
            Some(s_to),
            Some(&mut sdir),
            None,
            round,
            &wall,
            tol,
            of2(&fs, ii).this_ang_tol_rad(),
            crate::blend::fillet_geom::p2::SrfSrfOptions::default(),
        )?;
        let uv1 = curves.uv_curve1.ok_or_else(|| {
            missing_defect(
                OP_EDGE,
                "the trimline's intersection returned no parameter curve on the blend, though srf_srf_intersection drops the curve onto both surfaces where the intersector gave none",
            )
        })?;
        let round_b = bspline_surface(round).ok_or_else(|| {
            not_bspline(
                OP_EDGE,
                MixedStep::BlendSurfaceNotBSpline,
                blend_edgeuse(&ring, ii),
            )
        })?;
        let mut pc = algebra::from_bspline(bspline_curve(&uv1).ok_or_else(|| {
            not_bspline(
                OP_EDGE,
                MixedStep::TrimlineNotBSpline,
                blend_edgeuse(&ring, ii),
            )
        })?)?;
        // which end is at X
        let end0 = {
            let q0 = algebra::eval(
                &pc,
                *pc.brk.first().ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))?,
            )?;
            round.evaluate_point(k, Point2::new(q0[0] / q0[3], q0[1] / q0[3]))?
        };
        pc = algebra::unit(&pc, end0.distance(sx) > end0.distance(s_to))?;
        // its ends on the rails' iso lines: the rails are the round's v = min, max lines,
        // along u
        let dom = round.natural_uv_domain_with_nurb(k)?;
        for e in 0..2 {
            let rail = if e == 0 {
                of2(&rails.on_t, ii)
            } else {
                of2(&rails.on_wall, ii)
            };
            let at = if e == 0 { sx } else { s_to };
            let dt = param_of(k, rail, at, tol, OP_EDGE)?;
            let sr = rail.evaluate_point(k, dt)?;
            let lo = round.evaluate_point(k, Point2::new(dt, dom.min().y))?;
            let hi = round.evaluate_point(k, Point2::new(dt, dom.max().y))?;
            let iso = if lo.distance(sr) <= hi.distance(sr) {
                dom.min().y
            } else {
                dom.max().y
            };
            let slot: &mut V4 = if e == 0 {
                pc.seg
                    .first_mut()
                    .and_then(|s| s.first_mut())
                    .ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))?
            } else {
                pc.seg
                    .last_mut()
                    .and_then(|s| s.last_mut())
                    .ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))?
            };
            *slot = [dt, iso, 0.0, 1.0];
        }
        let srf = algebra::srf_from_bspline(round_b)?;
        // pc's pieces within the round's spans (where it crosses them)
        {
            let cut = algebra::compose(&srf, &pc)?;
            pc = algebra::refine(&pc, &cut.brk)?;
        }
        // the trimline and the round's cross partial along it (the ribbon's field), at
        // one degree, made C1 in R^4 together (with pc) across the round's knots
        let mut c = algebra::compose(&srf, &pc)?;
        let mut ct = algebra::compose(&algebra::srf_deriv_v(&srf)?, &pc)?;
        ct = algebra::elevate(&ct, c.deg().ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))?)?;
        algebra::c1(&mut c, &mut [&mut pc, &mut ct])?;
        *of2m(&mut trim, ii) = algebra::unit_weights(&c)?;
        *of2m(&mut trim_t, ii) = ct;
        *of2m(&mut pcs, ii) = pc;
        if ii == 0 {
            let m = algebra::eval(&trim[0], 0.5)?;
            out_sign = if (algebra::project(m) - sx).dot(normal) < 0.0 {
                1.0
            } else {
                -1.0
            };
        }
    }
    let s_out = normal * out_sign;
    // the rail ends exactly where the trimlines end
    pseudo.set_vertex_point(a, algebra::project(algebra::eval(&trim[0], 1.0)?))?;
    pseudo.set_vertex_point(bv, algebra::project(algebra::eval(&trim[1], 1.0)?))?;
    let sa = pseudo.vertex_point(a)?;
    let sb = pseudo.vertex_point(bv)?;

    // the rail pieces A-P (B1's), B-Q (B2's), the lines V-P, V-Q, and the arc X-V
    let (ap, bq) = {
        let r0 = algebra::from_bspline(bspline_curve(&rails.on_wall[0]).ok_or_else(|| {
            rail_not_bspline(
                OP_EDGE,
                rails.wall_at[0],
                &rails.on_wall[0],
                blend_edgeuse(&ring, 0),
            )
        })?)?;
        let r1 = algebra::from_bspline(bspline_curve(&rails.on_wall[1]).ok_or_else(|| {
            rail_not_bspline(
                OP_EDGE,
                rails.wall_at[1],
                &rails.on_wall[1],
                blend_edgeuse(&ring, 1),
            )
        })?)?;
        let da = param_of(k, &rails.on_wall[0], sa, tol, OP_EDGE)?;
        let dp = param_of(k, &rails.on_wall[0], pseudo.vertex_point(p)?, tol, OP_EDGE)?;
        let db = param_of(k, &rails.on_wall[1], sb, tol, OP_EDGE)?;
        let dq = param_of(k, &rails.on_wall[1], pseudo.vertex_point(q)?, tol, OP_EDGE)?;
        // the rail pieces made C1 in R^4 where they cross their round's knots
        let mut piece_ap = algebra::piece(&r0, da, dp)?;
        let mut piece_bq = algebra::piece(&r1, db, dq)?;
        algebra::c1(&mut piece_ap, &mut [])?;
        algebra::c1(&mut piece_bq, &mut [])?;
        let mut ap = algebra::unit_weights(&piece_ap)?;
        let mut bq = algebra::unit_weights(&piece_bq)?;
        // their ends exactly the vertices (A and B the trimlines' ends)
        *ap.seg
            .first_mut()
            .and_then(|s| s.first_mut())
            .ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))? = algebra::point(sa, 1.0);
        *bq.seg
            .first_mut()
            .and_then(|s| s.first_mut())
            .ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))? = algebra::point(sb, 1.0);
        let end_p = *ap
            .seg
            .last()
            .and_then(|s| s.last())
            .ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))?;
        let end_q = *bq
            .seg
            .last()
            .and_then(|s| s.last())
            .ok_or_else(|| defect(OP_EDGE, EMPTY_CURVE))?;
        pseudo.set_vertex_point(p, algebra::project(end_p))?;
        pseudo.set_vertex_point(q, algebra::project(end_q))?;
        (ap, bq)
    };
    let sp = pseudo.vertex_point(p)?;
    let sq = pseudo.vertex_point(q)?;
    let line = |from: Point3, to: Point3| Crv {
        brk: vec![0.0, 1.0],
        seg: vec![vec![algebra::point(from, 1.0), algebra::point(to, 1.0)]],
    };
    let vp = line(sv, sp);
    let vq = line(sv, sq);
    let (xv, xvq) = {
        // V-X leaves X along an asymptotic direction of the corner's shape operator there
        // and arrives at V along the edges: the conic in T with those end tangents (its
        // middle control point where they meet), weighted as a circle's arc of that turn.
        // Chamfers (G0) take the wedge's bisector.
        let mut es = [Vec3::default(); 2];
        let mut ks = [0.0; 2];
        for ii in 0..2 {
            let c = algebra::eval(of2(&trim, ii), 0.0)?;
            let c1 = algebra::crv_deriv_at(of2(&trim, ii), 0.0, 1)?;
            let c2 = algebra::crv_deriv_at(of2(&trim, ii), 0.0, 2)?;
            let pp = Vec3::new(c[0] / c[3], c[1] / c[3], c[2] / c[3]);
            let p1 = Vec3::new(
                (c1[0] - pp.x * c1[3]) / c[3],
                (c1[1] - pp.y * c1[3]) / c[3],
                (c1[2] - pp.z * c1[3]) / c[3],
            );
            let p2 = Vec3::new(
                (c2[0] - 2.0 * p1.x * c1[3] - pp.x * c2[3]) / c[3],
                (c2[1] - 2.0 * p1.y * c1[3] - pp.y * c2[3]) / c[3],
                (c2[2] - 2.0 * p1.z * c1[3] - pp.z * c2[3]) / c[3],
            );
            let speed2 = p1.dot(p1);
            if speed2 <= EFF_ZERO {
                return Err(unbuildable(OP_EDGE, MixedStep::TrimlineStalls));
            }
            *of2m(&mut ks, ii) = p2.dot(s_out) / speed2;
            *of2m(&mut es, ii) = (p1 - s_out * p1.dot(s_out)).unitize()?;
        }
        let mut dir_x = es[0] + es[1];
        if !chamfer {
            // e2 = lambda e1 + mu D (in T)
            let d = (convex_dir - s_out * convex_dir.dot(s_out)).unitize()?;
            let d11 = 1.0;
            let d12 = es[0].dot(d);
            let d22 = 1.0;
            let r1 = es[1].dot(es[0]);
            let r2 = es[1].dot(d);
            let det = d11 * d22 - d12 * d12;
            if det.abs() <= EFF_ZERO {
                return Err(unbuildable(OP_EDGE, MixedStep::TrimlineAlongConvexEdge));
            }
            let lambda = (r1 * d22 - r2 * d12) / det;
            let ii11 = ks[0];
            let ii22 = ks[1];
            let ii12 = lambda * ks[0];
            // II22 k^2 + 2 II12 k + II11 = 0, k > 0: a = e1 + k e2
            let disc = ii12 * ii12 - ii11 * ii22;
            let mut k_best = -1.0;
            if disc >= 0.0 && ii22.abs() > EFF_ZERO {
                for sgn in [-1.0, 1.0] {
                    let kk = (-ii12 + sgn * disc.sqrt()) / ii22;
                    if kk > 0.0 && (k_best < 0.0 || kk < k_best) {
                        k_best = kk;
                    }
                }
            }
            if k_best <= 0.0 {
                // Mixed corner: the corner's shape at the rails' crossing has no
                // asymptotic direction between the trimlines
                return Err(unbuildable(OP_EDGE, MixedStep::NoAsymptoticDirection));
            }
            dir_x = es[0] + es[1] * k_best;
        }
        let dir_x = dir_x.unitize()?;
        let dir_v = (chain - s_out * chain.dot(s_out)).unitize()?; // arriving at V
                                                                   // X + s a = V - t c
        let d = sv - sx;
        let a11 = dir_x.dot(dir_x);
        let a12 = dir_x.dot(dir_v);
        let a22 = dir_v.dot(dir_v);
        let b1 = dir_x.dot(d);
        let b2 = dir_v.dot(d);
        let det = a11 * a22 - a12 * a12;
        if det.abs() <= EFF_ZERO {
            return Err(unbuildable(OP_EDGE, MixedStep::InflectionTangentsParallel));
        }
        let s = (b1 * a22 - b2 * a12) / det;
        let t = (a11 * b2 - a12 * b1) / det;
        if s.is_nan() || s <= 0.0 || t.is_nan() || t <= 0.0 {
            // Mixed corner: the inflection's end tangents do not meet between X and V
            return Err(unbuildable(OP_EDGE, MixedStep::InflectionTangentsMiss));
        }
        let si = sx + dir_x * s;
        let w = (0.5 * max(-1.0, min(1.0, dir_x.dot(dir_v))).acos()).cos();
        let xvq = Crv {
            brk: vec![0.0, 1.0],
            seg: vec![vec![
                algebra::point(sx, 1.0),
                algebra::point(si, w),
                algebra::point(sv, 1.0),
            ]],
        };
        (algebra::unit_weights(&xvq)?, xvq)
    };

    // the transversal fields
    let circumferential = move |c: V4| {
        let r = Vec3::new(c[0] - c[3] * o.x, c[1] - c[3] * o.y, c[2] - c[3] * o.z);
        algebra::dir(omega.cross(r))
    };
    let (ma, mb, rho) = {
        // F2's meridian at Q, as a Omega + b rho^
        let mut rad = (sq - o) - omega * (sq - o).dot(omega);
        let rho = rad.length();
        rad = rad.unitize()?;
        let n = normal_at(k, target, ring.f2, sq, tol, OP_EDGE)?;
        let meridian = n.cross(omega.cross(rad)).unitize()?;
        (meridian.dot(omega), meridian.dot(rad), rho)
    };
    let meridian = move |c: V4| {
        let mut r = Vec3::new(c[0] - c[3] * o.x, c[1] - c[3] * o.y, c[2] - c[3] * o.z);
        r = r - omega * r.dot(omega);
        algebra::dir(omega * (ma * c[3]) + r * (mb / rho))
    };
    let f1n = normal_at(k, target, ring.f1, sa, tol, OP_EDGE)?;

    // the sides of L (X: (0,0), A: (1,0), P: (1,1), V: (0,1)) and U (X: (0,0), V: (1,0),
    // Q: (1,1), B: (0,1)): L = sides 0..4, U = sides 4..8 (c0, c1, d0, d1)
    let mut sides: [Side; 8] = Default::default();
    let set = |sides: &mut [Side; 8], i: usize, c: Crv, dir: Option<Crv>| {
        let s = of8m(sides, i);
        s.c = c;
        if let Some(d) = dir {
            s.dir = d;
        } else {
            s.shared = true;
        }
    };
    set(
        &mut sides,
        0,
        trim[0].clone(),
        Some(algebra::constant(algebra::dir(convex_dir))),
    );
    set(
        &mut sides,
        1,
        vp,
        Some(algebra::constant(algebra::dir(chain))),
    );
    set(&mut sides, 2, xv.clone(), None);
    set(&mut sides, 3, ap.clone(), Some(Crv::default()));
    set(&mut sides, 4, xv, None);
    set(&mut sides, 5, bq.clone(), Some(algebra::map(&bq, meridian)));
    set(
        &mut sides,
        6,
        trim[1].clone(),
        Some(algebra::map(&trim[1], circumferential)),
    );
    set(
        &mut sides,
        7,
        vq,
        Some(algebra::constant(algebra::dir(chain))),
    );
    // the cross fields where a side is only C1 (the trimlines' fits, the wall rail across
    // the round's knot): the round's cross partial along its trimline, the wall's turn
    // along B-Q
    for (i, t) in [
        (0, trim_t[0].clone()),
        (6, trim_t[1].clone()),
        (5, algebra::map(&bq, circumferential)),
    ] {
        let s = of8m(&mut sides, i);
        s.t = t;
        s.has_t = true;
    }
    {
        // A-P's direction (its tangent at A)
        let d = start_dir(&ap)?;
        of8m(&mut sides, 3).dir = algebra::constant(algebra::dir(f1n.cross(d)));
    }
    // M along V-X, pointing into U: A + k B with A = d (n_T x T~), d the curve's distance
    // from the wall's plane at V, T~ its homogeneous tangent; B = t w^2 s, s the seam's
    // line at V turned to T's outward side; only k, the rate at X, is left: solved with
    // the twists
    let (m_a, m_b) = {
        // the wall's plane at V: its normal, into the material
        let mut nw = normal.cross(chain);
        if nw.dot(sx - sv) < 0.0 {
            nw = -nw;
        }
        let nw = nw.unitize()?;
        let qc = &xvq;
        let dist = algebra::map(qc, |q| {
            [
                nw.x * (q[0] - q[3] * sv.x)
                    + nw.y * (q[1] - q[3] * sv.y)
                    + nw.z * (q[2] - q[3] * sv.z),
                0.0,
                0.0,
                0.0,
            ]
        });
        let w = algebra::map(qc, |q| [q[3], 0.0, 0.0, 0.0]);
        let dq = algebra::deriv(qc)?;
        let dw = algebra::map(&dq, |q| [q[3], 0.0, 0.0, 0.0]);
        let tt = algebra::sum(
            &algebra::times(&w, &dq)?,
            &algebra::times(&dw, qc)?,
            1.0,
            -1.0,
        )?;
        let nt = algebra::map(&tt, |q| {
            let v = normal.cross(Vec3::new(q[0], q[1], q[2]));
            [v.x, v.y, v.z, 0.0]
        });
        let mut m_a = algebra::times(&dist, &nt)?;
        let lin = Crv {
            brk: vec![0.0, 1.0],
            seg: vec![vec![[0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]],
        };
        let sc = algebra::times(&lin, &algebra::times(&w, &w)?)?;
        let up = seam_dir * if seam_dir.dot(s_out) < 0.0 { -1.0 } else { 1.0 };
        let m_b = algebra::map(&sc, |q| [q[0] * up.x, q[0] * up.y, q[0] * up.z, 0.0]);
        // A into U at X (towards U's trimline)
        let e2 = start_dir(&trim[1])?;
        let a0 = algebra::eval(&m_a, 0.0)?;
        if Vec3::new(a0[0], a0[1], a0[2]).dot(e2) < 0.0 {
            m_a = algebra::map(&m_a, |q| algebra::mul(q, -1.0));
        }
        for i in [2, 4] {
            let s = of8m(&mut sides, i);
            s.dir_val[0] = algebra::eval(&m_a, 0.0)?;
            s.dir_der[0] = algebra::crv_deriv_at(&m_a, 0.0, 1)?;
            s.dir_k[0] = algebra::crv_deriv_at(&m_b, 0.0, 1)?;
            s.dir_val[1] = algebra::eval(&m_b, 1.0)?; // gamma at V is taken per unit of k
            s.dir_der[1] = algebra::crv_deriv_at(&m_b, 1.0, 1)?;
            s.dir_k[1] = V4::default();
        }
        (m_a, m_b)
    };
    for s in &mut sides {
        if !s.shared {
            algebra::side_ends(s)?;
        }
    }

    let shared_dir = if chamfer {
        algebra::sum(&m_a, &m_b, 1.0, 1.0)?
    } else {
        // first order: each ribbon's end = the adjacent side's tangent there
        let tangent_at = |sides: &[Side; 8], i: usize, e: f64| -> Result<V4> {
            algebra::crv_deriv_at(&of8(sides, i).c, e, 1)
        };
        for f in 0..2 {
            let o = 4 * f;
            for (s, e, from, at) in [
                (0, 0, 2, 0.0),
                (0, 1, 3, 0.0),
                (1, 0, 2, 1.0),
                (1, 1, 3, 1.0),
                (2, 0, 0, 0.0),
                (2, 1, 1, 0.0),
                (3, 0, 0, 1.0),
                (3, 1, 1, 1.0),
            ] {
                let target_v = tangent_at(&sides, o + from, at)?;
                algebra::first_order(of8m(&mut sides, o + s), e, target_v, size)?;
            }
        }
        // twists: at X and V both faces at once, with k
        let term = |side: usize, e: usize, sign: f64| TwistTerm { side, e, sign };
        let k_rate = algebra::twists(
            &mut sides,
            &[
                [term(0, 0, 1.0), term(2, 0, -1.0)],
                [term(4, 0, 1.0), term(6, 0, -1.0)],
                [term(1, 0, 1.0), term(2, 1, -1.0)],
                [term(4, 1, 1.0), term(7, 0, -1.0)],
            ],
            size,
            true,
        )?;
        algebra::twists(
            &mut sides,
            &[[term(0, 1, 1.0), term(3, 0, -1.0)]],
            size,
            false,
        )?;
        algebra::twists(
            &mut sides,
            &[[term(1, 1, 1.0), term(3, 1, -1.0)]],
            size,
            false,
        )?;
        algebra::twists(
            &mut sides,
            &[[term(5, 0, 1.0), term(6, 1, -1.0)]],
            size,
            false,
        )?;
        algebra::twists(
            &mut sides,
            &[[term(5, 1, 1.0), term(7, 1, -1.0)]],
            size,
            false,
        )?;
        if k_rate.is_nan() || k_rate <= 0.0 {
            // Mixed corner: the faces' tangent plane would turn the wrong way along the
            // inflection
            return Err(unbuildable(OP_EDGE, MixedStep::TangentPlaneTurns));
        }
        for i in [2, 4] {
            let s = of8m(&mut sides, i);
            s.val[1][2] /= k_rate;
            s.der[1][2] /= k_rate;
        }
        algebra::sum(&m_a, &m_b, 1.0, k_rate)?
    };
    for i in [2, 4] {
        of8m(&mut sides, i).dir = shared_dir.clone();
    }

    // the faces, each turned so its normal leaves the material (out of T at X)
    let state = mixed;
    state.delete_surfaces();
    let mut side_curves: [[Option<BSplineCurve>; 4]; 2] = Default::default();
    let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    for f in 0..2 {
        let o = 4 * f;
        let s = |i: usize| of8(&sides, o + i);
        let mut srf = if chamfer {
            algebra::coons_chamfer(s(0), s(1), s(2), s(3), tol)?
        } else {
            algebra::coons(s(0), s(1), s(2), s(3))?
        };
        // its normal at X: S_u x S_v = c0' x d0'
        let p0 = algebra::project(algebra::eval(&s(0).c, 0.0)?);
        let dir_of = |c: &Crv| -> Result<Vec3> {
            let t = algebra::crv_deriv_at(c, 0.0, 1)?;
            let pp = algebra::eval(c, 0.0)?;
            Ok(Vec3::new(
                (t[0] - p0.x * t[3]) / pp[3],
                (t[1] - p0.y * t[3]) / pp[3],
                (t[2] - p0.z * t[3]) / pp[3],
            ))
        };
        let turn = dir_of(&s(0).c)?.cross(dir_of(&s(2).c)?).dot(s_out) < 0.0;
        if turn {
            srf = algebra::srf_transpose(&srf)?;
        }
        let patch = algebra::srf_to_bspline(&srf)?;
        // its loop: v = 0 (SAME), u = 1 (SAME), v = 1 (OPPOSITE), u = 0 (OPPOSITE), each the
        // patch's iso
        let dirs = [
            SurfParamType::V,
            SurfParamType::U,
            SurfParamType::V,
            SurfParamType::U,
        ];
        let ats = [0.0, 1.0, 1.0, 0.0];
        let orients = [
            OrientType::Same,
            OrientType::Same,
            OrientType::Opposite,
            OrientType::Opposite,
        ];
        let froms = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.0, 0.0)];
        let tos = [(1.0, 0.0), (1.0, 1.0), (1.0, 1.0), (0.0, 1.0)];
        // which logical side each loop curve is: untransposed v=0 c0, u=1 d1, v=1 c1,
        // u=0 d0
        let which = if turn {
            [2usize, 1, 3, 0]
        } else {
            [0usize, 3, 1, 2]
        };
        for ((((&dir, &at), (&from, &to)), side_i), &orient) in dirs
            .iter()
            .zip(&ats)
            .zip(froms.iter().zip(&tos))
            .zip(which)
            .zip(&orients)
        {
            let iso = patch
                .create_iso_parametric_curve(
                    k,
                    dir,
                    at,
                    IsoCurveOptions {
                        domain: Some(dom),
                        uv_iso_curve: false,
                    },
                )?
                .curve;
            let uv = BSplineCurve::create_line_segment(
                2,
                Point3::new(from.0, from.1, 0.0),
                Point3::new(to.0, to.1, 0.0),
            )?;
            // `which` is a turn of the four sides: every slot is filled once
            if let Some(slot) = of2m(&mut side_curves, f).get_mut(side_i) {
                *slot = Some(iso.clone());
            }
            of2m(&mut state.curves_3d, f).push(Curve::from(iso));
            of2m(&mut state.uv_curves, f).push(Curve::from(uv));
            of2m(&mut state.orients, f).push(orient);
        }
        let mut patch_surface = Surface::from(patch);
        // (#2488) the face keeps its rule: its sides from the trimmed blends and the
        // conic in T; L's trimline is its side c0 on B1 and the conic its d0, U's the
        // conic its c0 and the trimline its d0 on B2
        let local: [Side; 4] = [s(0).clone(), s(1).clone(), s(2).clone(), s(3).clone()];
        let (trim_side, conic_side) = if f == 0 { (0, 2) } else { (2, 0) };
        let definition = CornerDefinition::mixed(
            k,
            [&rounds[0], &rounds[1]],
            &common,
            local,
            (chamfer, turn),
            (f, trim_side, of2(&pcs, f).clone()),
            conic_side,
            tol,
        )?;
        definition.recertify(k, &patch_surface)?;
        patch_surface.set_corner_definition(definition);
        *of2m(&mut state.surfaces, f) = Some(patch_surface);
    }

    // the edges' curves, from the faces' isos: X-A L c0, A-P L d1, P-V L c1 reversed,
    // V-X L d0 reversed, V-Q U d1, Q-B U c1 reversed, X-B U d0
    let faces = [0usize, 0, 0, 0, 1, 1, 1];
    let which = [0usize, 3, 1, 2, 3, 1, 2];
    let rev = [false, false, true, true, false, true, false];
    let edges: [EdgeId; 7] = corner
        .edges
        .as_slice()
        .try_into()
        .map_err(|_| bad_input(OP_EDGE, CornerInput::TopologyMismatch, None))?;
    for (((&face, &side), &reversed), &e) in faces.iter().zip(&which).zip(&rev).zip(&edges) {
        let iso = of2(&side_curves, face)
            .get(side)
            .cloned()
            .flatten()
            .ok_or_else(|| {
                missing_defect(
                    OP_EDGE,
                    "a Mixed face has no iso curve for an edge's side, though every face makes its four sides' isos",
                )
            })?;
        let mut curve = Curve::from(iso);
        if reversed {
            let ivl = curve.natural_interval();
            curve.reverse_parameterization(k, &ivl)?;
        }
        set_edge_curve(pseudo.brep_mut(), e, Some(curve))?;
    }
    // the trimlines' uv curves on the blends (X-A on B1, X-B on B2, both from X)
    for ii in 0..2 {
        let uv = algebra::to_uv(of2(&pcs, ii))?;
        let e = if ii == 0 { edges[0] } else { edges[6] };
        let prim = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(|| {
            bad_setup(
                OP_EDGE,
                FilletSetUp::PrimaryEdgeUse,
                Some(EntityRef::Edge(e)),
            )
        })?;
        pseudo.set_edgeuse_uv_curve(prim, Some(Curve::from(uv)), true, false)?;
        pseudo
            .geom_mut(*of2(&geoms, ii))?
            .add_side_fillet_edgeuse(prim);
    }
    Ok(())
}

/// `SmBlendCorner::CreateMixedPatches` (the fork's 6fd0411): the Mixed corner's two
/// faces, each its patch bounded by its natural loop (`MakeFaceWithCurves` in the fillet
/// B-rep's infinite region, its natural UV domain, `SM_OT_SAME`); the surfaces join
/// `corner.surfaces` and the faces `corner.surface_faces`.
///
/// # Errors
///
/// [`SolidError::null_pointer`] without a face's surface, the fillet B-rep or a new face;
/// those of the face construction.
pub fn create_mixed_patches(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<()> {
    for f in 0..2 {
        let mixed = corner
            .mixed
            .as_mut()
            .ok_or_else(|| bad_input(OP_PATCH, CornerInput::EventNotSet, None))?;
        let surface = of2m(&mut mixed.surfaces, f)
            .take()
            .ok_or_else(|| bad_input(OP_PATCH, CornerInput::FacesNotBuilt, None))?;
        let curves_3d = std::mem::take(of2m(&mut mixed.curves_3d, f));
        let uv_curves = std::mem::take(of2m(&mut mixed.uv_curves, f));
        let orients = of2(&mixed.orients, f).clone();
        let fillet = env
            .fillet
            .as_mut()
            .ok_or_else(|| bad_input(OP_PATCH, CornerInput::FilletBrepNotGiven, None))?;
        let input = FaceCurvesInput {
            loops: vec![curves_3d.len()],
            curves_3d: Some(curves_3d.into_iter().map(Some).collect()),
            uv_curves: Some(uv_curves),
            orientations: orients,
            loop_points: Vec::new(),
        };
        let uv_domain = surface.natural_uv_domain_with_nurb(env.k)?;
        let region = fillet.brep.infinite_region();
        let key_of = fillet.notify.key_of;
        let face_ref = |face| key_of(EntityRef::Face(face));
        let made = make_face_with_curves(
            env.k,
            fillet.brep,
            region,
            input,
            surface.clone(),
            &uv_domain,
            OrientType::Same,
            None,
            &face_ref,
            &mut fillet.notify,
        )?;
        let face = made.face.ok_or_else(|| {
            missing_defect(
                OP_PATCH,
                "making a Mixed face returned no face, though the face construction succeeded",
            )
        })?;
        corner.surfaces.push(surface);
        corner.surface_faces.push(face);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::nurbs_crv::allocate_nurb_curve;

    /// Solvers the Mixed entry points below never reach: they refuse the corner first.
    struct UnreachedSolvers;

    impl SweptSolvers for UnreachedSolvers {
        fn solver_of_edgeuse(&self, _: EdgeuseId) -> Result<&FilletSolver> {
            Err(SolidError::null_pointer("test"))
        }
        fn offset_radii_can_change(&self, _: EdgeuseId) -> Result<bool> {
            Err(SolidError::null_pointer("test"))
        }
        fn solver(&self, _: FilletSolverId) -> Result<&FilletSolver> {
            Err(SolidError::null_pointer("test"))
        }
        fn constant_offsets(&self, _: FilletSolverId) -> Result<Option<[f64; 2]>> {
            Err(SolidError::null_pointer("test"))
        }
    }

    /// `err` is the caller's corner without its Mixed event, with the `SM_ERR` status the
    /// site had before (a failed check, not the kind's `SM_ERR_NULL_POINTER` default;
    /// #20725's third review).
    fn assert_event_not_set_with_sm_err(err: &SolidError) {
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CornerInputInvalid {
                input: CornerInput::EventNotSet,
                entity: None,
            }),
            "{err:?}"
        );
        assert_eq!(
            crate::error::upstream_status(err),
            Some(crate::error::SM_ERR)
        );
    }

    #[test]
    fn a_corner_without_its_mixed_event_keeps_the_vertex_sites_status() {
        use crate::blend::fillet_corner::p3::FilletCornerKind;
        let corner = FilletCorner::new(
            FilletCornerKind::ThreeByTwoMixed,
            None,
            None,
            None,
            1e-6,
            1e-3,
        );
        let k = Kernel::default();
        let err = calc_mixed_vert_geom(
            &k,
            &Brep::new(),
            &mut FilletBrep::new(),
            &corner,
            &UnreachedSolvers,
        )
        .unwrap_err();
        assert_event_not_set_with_sm_err(&err);
    }

    #[test]
    fn a_corner_without_its_mixed_event_keeps_the_edge_sites_status() {
        use crate::blend::fillet_corner::p3::FilletCornerKind;
        let mut corner = FilletCorner::new(
            FilletCornerKind::ThreeByTwoMixed,
            None,
            None,
            None,
            1e-6,
            1e-3,
        );
        let k = Kernel::default();
        let err = calc_mixed_edge_geom(
            &k,
            &mut Brep::new(),
            &mut FilletBrep::new(),
            &mut corner,
            &UnreachedSolvers,
        )
        .unwrap_err();
        assert_event_not_set_with_sm_err(&err);
    }

    /// A unit box's first edge, its curve replaced by the polyline from its start
    /// vertex P0 through its end vertex P1 on to P1 + n, n square to the edge, with the
    /// edge on the first half: the curve turns a right angle at the edge's end vertex.
    /// Also a vertex off the edge.
    fn kinked_edge() -> (Brep, EdgeId, VertexId, VertexId, VertexId) {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, 1.0, 1.0, 1.0)
            .unwrap();
        let e = b.edges().ids().next().unwrap();
        let (v0, v1) = edge::start_end_vertices(&b, e).unwrap();
        let (v0, v1) = (v0.unwrap(), v1.unwrap());
        let off = b.vertices().ids().find(|&v| v != v0 && v != v1).unwrap();
        let (p0, p1) = (b.vertex(v0).unwrap().point, b.vertex(v1).unwrap().point);
        let along = p1 - p0;
        let n = if along.cross(Vec3::Z).length() > 0.5 {
            along.cross(Vec3::Z)
        } else {
            along.cross(Vec3::X)
        };
        let mut cur = allocate_nurb_curve(2, 1, 4).unwrap();
        for (d, s) in cur.cpts_mut().iter_mut().zip([p0, p1, p1 + n]) {
            d.x = s.x;
            d.y = s.y;
            d.z = s.z;
            d.w = -f64::MAX;
        }
        cur.knots_mut().copy_from_slice(&[0.0, 0.0, 0.5, 1.0, 1.0]);
        let rec = b.edge_mut(e).unwrap();
        rec.curve = Some(Curve::from(BSplineCurve::from_nurb(cur, 3, false)));
        rec.interval = Extent1d::from_min_max(0.0, 0.5);
        (b, e, v0, v1, off)
    }

    /// Fork b785227: the tangent leaving the end vertex is the edge's own, taken from
    /// below its end and turned: from P1 to P0 here. Before it, the curve's tangent from above (the
    /// next segment's, along n) was kept, since it is square to the way to the 1e-3 sample.
    #[test]
    fn leaving_end_vertex_takes_the_edge_sense_from_inside() {
        let k = Kernel::default();
        let (b, e, v0, v1, off) = kinked_edge();
        let (p0, p1) = (b.vertex(v0).unwrap().point, b.vertex(v1).unwrap().point);
        let at_end = leaving(&k, &b, e, v1, "test").unwrap();
        assert_eq!(at_end, (p0 - p1).unitize().unwrap());
        let at_start = leaving(&k, &b, e, v0, "test").unwrap();
        assert_eq!(at_start, (p1 - p0).unitize().unwrap());
        // an edge that does not meet the vertex is an error
        assert!(leaving(&k, &b, e, off, "test").is_err());
    }

    /// An edge that does not meet the vertex is invalid caller input (`leaving`
    /// is reached from the public entry points with a caller's corner), with the `SM_ERR`
    /// status the bare status had.
    #[test]
    fn an_edge_off_the_vertex_is_invalid_input() {
        let k = Kernel::default();
        let (b, e, _, _, off) = kinked_edge();
        let err = leaving(&k, &b, e, off, OP_VERT).unwrap_err();
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CornerInputInvalid {
                input: CornerInput::EdgeOffVertex,
                entity: Some(EntityRef::Edge(e)),
            }),
            "{err:?}"
        );
        assert!(!err.is_defect(), "{err:?}");
        assert_eq!(err.operation(), OP_VERT);
        assert_eq!(
            crate::error::upstream_status(&err),
            Some(crate::error::SM_ERR)
        );
    }
}
