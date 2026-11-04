//! The blend corner's End (the fork's `SmBlendCornerEnd.cpp`): the End at a smooth vertex,
//! the End's edges, and the refusal of a rail that runs past its side edge.
//!
//! Upstream: none. The fork's `source/SMLib/src/SmBlendCornerEnd.cpp`,
//! `source/SMLib/inc/SmBlendCorner.h` (tangency-labs/smlib `41a39ec`, `54a4ee7`, `63c7962`, smlib#124).
//!
//! | Rust                              | Fork                                      |
//! |-----------------------------------|-------------------------------------------|
//! | [`end_at_smooth_vertex`]          | `SmBlendCorner::EndAtSmoothVertex`        |
//! | [`calc_end_edge_geom`]            | `SmBlendCorner::CalcEndEdgeGeom` (before the base `CalcCornerEdgeGeom`) |
//! | `calc_end_cap_edges` (private)    | `SmBlendCorner::CalcEndCapEdges`          |
//! | `ball_touching_at` (private)      | `SmBlendCorner::BallTouchingAt` (54a4ee7 splits it out of 41a39ec's `RadiusReachingSideEdgeEnd`, bit for bit) |
//! | `radius_reaching_side_edge_end` (private) | `SmBlendCorner::RadiusReachingSideEdgeEnd` |
//! | `refuse_end_past_side_edge` (private) | `SmBlendCorner::RefuseEndPastSideEdge` |
//! | [`describe_end_past_side_edge`]   | `SmBlendCorner::DescribeEndPastSideEdge`  |
//! | `refuse_end_overflowing_side_edge` (private), [`describe_end_overflowing_side_edge`] | `SmBlendCorner::RefuseEndOverflowingSideEdge` (54a4ee7, 63c7962) |
//! | [`side_edgeuse_across_seam_vertex`] | `sm_SideEdgeuseAcrossSeamVertex` (`SmBlendCorner.cpp`, 63c7962) |
//! | [`side_edge_chain_to`]            | `SmBlendCorner::SideEdgeChainTo` (63c7962) |
//! | [`fit_of_end_past_side_edges`]    | `SmBlendCorner::FitOfEndPastSideEdges`    |
//! | [`create_end_cap_patch`]          | `SmBlendCorner::CreateEndPatchOfEnd`'s `SM_BEND_AT_SMOOTH` branch |
//! | [`end_awaits_rollover`], [`end_can_await_rollover`], [`end_past_tangent_edge`], `end_rolls_over`, `ends_in_tangent_edge` (private) | none (#2406: the End's rail rolls over a tangent edge first) |
//! | [`place_rolled_rail_end`]         | none (#2406: a rail end not reached on the End's plane before the rollover) |
//! | `side_edge_in_cap` (private)      | none (#2406: a cap edge along a side edge in the cap's plane) |
//! | [`EndEdges`], `end_face_runs_on_past_side_edge`, `rail_face` (private) | none: an end face running on tangentially past the side edge is the global merge's, not refused |
//! | `run_on_leaves_the_part` (private) | none (#2656: a blend run on past a smooth vertex that runs out of the part ends on the end face, not capped) |
//!
//! Deviations: the fork's `NoteError(SM_FILERR_NO_ROOM, ...)` and `NoteFitRadius` before
//! `SM_ERR` are returned ([`EndRefusal`]) for the executive's caller to note, which then
//! returns `SM_ERR`. Solvers come through [`SweptSolvers`]. The bisection runs at most
//! 100 steps and the golden section 200, as the fork's; the side edge's chain at most
//! [`SIDE_EDGE_CHAIN_STEPS`] (the fork's 64) and `CalcRailIntEdgeuse`'s continuation along
//! it the same (the fork's recursion there has no bound). No recursion. Opt-in: `Kernel::fixes.fillet_blend_corner`
//! (WR-697).

use super::swept::SweptSolvers;
use crate::blend::fillet_corner::p3::CornerPatchEnv;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_corner::topology::same_convexity_edges_round_vertex;
use crate::blend::fillet_corner::topology::{
    check_tangential_edgeuse_at_vertex, find_side_edgeuse, same_convexity_edges,
};
use crate::blend::fillet_geom::p1::{FilStatus, FilletBrep, FilletEdgeType, FilletVertexType};
use crate::blend::fillet_geom::p2::{srf_srf_intersection, SrfSrfOptions};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::error::{EntityRef, Result, SolidError};
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

fn nul() -> SolidError {
    SolidError::null_pointer(OP)
}

fn face_of(target: &Brep, eu: EdgeuseId) -> Result<FaceId> {
    eu1::face(target, eu)?.ok_or_else(nul)
}

fn radial_face(target: &Brep, eu: EdgeuseId) -> Result<FaceId> {
    face_of(target, eu1::radial(target, eu)?.ok_or_else(nul)?)
}

/// The fork's `SmBlendCorner::EndAtSmoothVertex` (41a39ec): whether the blended edge (of
/// `fil_eu`) runs on at `vertex`, tangentially, into an edge of its convexity that is
/// not blended. The other edge must run on along one of the blended edge's faces and
/// leave the vertex opposite to it, within `tan_tol_deg` (two edges leaving it the same
/// way meet in a cusp, a knife edge's tip, which nothing continues). `all_eus` are the
/// edge uses round the vertex.
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
/// capping it with a face square to the blends at the vertex.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for an edge use without face, radial or edge; those of the
/// convexity test.
pub fn end_at_smooth_vertex(
    k: &Kernel,
    target: &mut Brep,
    vertex: VertexId,
    fil_eu: EdgeuseId,
    all_eus: &[EdgeuseId],
    tan_tol_deg: f64,
    earlier_pass_faces: &[FaceId],
) -> Result<bool> {
    let fil_edge = eu1::edge(target, fil_eu)?.ok_or_else(nul)?;
    let fil_faces = [face_of(target, fil_eu)?, radial_face(target, fil_eu)?];
    for &eu in all_eus {
        let other = eu1::edge(target, eu)?.ok_or_else(nul)?;
        if other == fil_edge {
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
            && !run_on_leaves_the_part(k, target, vertex, fil_eu, eu, leave0)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the blend of the edge of `fil_eu`, run on past `vertex` along the edge of
/// `other_eu` that continues it there tangentially, runs out of the part, so the blend ends
/// there by running on (the End on the end face) and needs no cap (tangency-labs/solidcore#2656;
/// not the fork's).
///
/// The two edges E (blended) and E' (continuing) share a face A, and leave the vertex V
/// opposite to each other with one tangent. The blend run on past V keeps its section, the
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
/// each as its height `c2 x^2 + c3 x^3` along `d` over the distance `x` past V along the
/// tangent, from its derivatives at V (E's run on past V), at `x` the shorter of the two
/// edges' chords from V, against A's zone tolerance. Where the second orders' difference
/// there exceeds both third orders, it decides: E' turns away where its height exceeds E's
/// continued. Otherwise the curvatures nearly agree (an edge of a curvature-continuous round,
/// which leaves its face with little curvature and turns as it goes), and E' turns away only
/// where its own third order does: E's says how E turns into itself, away from V, not past it.
///
/// # Errors
/// `SM_ERR_NULL_POINTER` for an edge use without face, radial or edge, or an edge without
/// curve; those of the binormal and the curve evaluations.
fn run_on_leaves_the_part(
    k: &Kernel,
    target: &mut Brep,
    vertex: VertexId,
    fil_eu: EdgeuseId,
    other_eu: EdgeuseId,
    leave: Vec3,
) -> Result<bool> {
    use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
    let fil_radial = eu1::radial(target, fil_eu)?.ok_or_else(nul)?;
    let other_radial = eu1::radial(target, other_eu)?.ok_or_else(nul)?;
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
    let fil_edge = eu1::edge(target, fil_eu)?.ok_or_else(nul)?;
    let other_edge = eu1::edge(target, other_eu)?.ok_or_else(nul)?;
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
    let options = BinormalOptions::default();
    let t_fil = param_at(target, fil_edge)?;
    let Ok(d) = evaluate_binormal(k, target, eu_a, t_fil, true, &options)?
        .binormal
        .unitize()
    else {
        return Ok(false);
    };
    // past V, along the continuing edge
    let along = -leave;
    // (c2, c3) of an edge's curve at V, its parameter turned to run past V along `along`
    let heights = |target: &Brep, e: EdgeId| -> Result<Option<(f64, f64)>> {
        let start = at_start(target, e)?;
        let c = edge::curve(target, e)?.ok_or_else(nul)?;
        let ds = c.evaluate(k, param_at(target, e)?, 3, !start, true)?;
        let der = |i: usize| ds.get(i).copied().ok_or_else(nul);
        let (mut d1, d2, mut d3) = (der(1)?, der(2)?, der(3)?);
        if d1.dot(along) < 0.0 {
            d1 = -d1;
            d3 = -d3;
        }
        let x1 = d1.dot(along);
        if x1 <= 0.0 {
            return Ok(None);
        }
        let (x2, h2, h3) = (d2.dot(along), d2.dot(d), d3.dot(d));
        let c2 = h2 / (2.0 * x1 * x1);
        let c3 = h3 / (6.0 * x1 * x1 * x1) - h2 * x2 / (2.0 * x1 * x1 * x1 * x1);
        Ok(Some((c2, c3)))
    };
    let (Some((e2, e3)), Some((o2, o3))) =
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
    let second = (o2 - e2) * span * span;
    if second.abs() > (o3.abs() + e3.abs()) * span * span * span {
        return Ok(second > tol);
    }
    Ok(o3 * span * span * span > tol)
}

/// The fork's `SmBlendCorner::BallTouchingAt` (54a4ee7; 41a39ec's
/// `RadiusReachingSideEdgeEnd` body): the ball touching the fillet's side face F at its
/// point `p` (F the solver's face that the side edge of `side_eu` bounds) and the fillet's
/// other face G (`other_face`): its radius rho, the root of `dist(p + rho n_F, G) - rho`
/// (its centre `p` offset along F's normal the way the solver offsets F), by bisection on
/// `(0, r]`, r the fillet's radius. Returns `(on_g, within, rho)`: `on_g` where `p` lies on
/// G (rho 0); `within` where the root lies below r (else rho is r).
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
    let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
    let mut f_index = 2;
    for ii in 0..2 {
        let eu = fs.edgeuse(ii)?.ok_or_else(nul)?;
        let face = face_of(target, eu)?;
        if face != other_face && side_faces.contains(&face) {
            f_index = ii;
        }
    }
    if f_index > 1 {
        return Err(SolidError::sm_err(OP));
    }
    let offset_f = &fs.surface(f_index)?.ok_or_else(nul)?.surface;
    let f = offset_f.base_surface();
    let g = crate::topology::face::p2::surface(target, other_face)?.ok_or_else(nul)?;
    let radius = offset_f.offset_distance().abs();
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
        return Err(SolidError::sm_err(OP));
    }
    let normal = f.evaluate_normal(k, dropped.result_uv, Side::Left, Side::Left)?;
    let g_domain = g.natural_uv_domain_with_nurb(k)?;
    let distance = |rho: f64| -> Result<f64> {
        let centre = p + normal * (sign * rho);
        let at = g.drop_point::<NoObject>(k, centre, &g_domain, &DropPointOptions::default())?;
        if !at.success {
            return Err(SolidError::sm_err(OP));
        }
        Ok(at.gap - rho)
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
    let side_edge = eu1::edge(target, side_eu)?.ok_or_else(nul)?;
    let far = if edge::start_vertex(target, side_edge)? == corner.vertex {
        edge::end_vertex(target, side_edge)?
    } else {
        edge::start_vertex(target, side_edge)?
    }
    .ok_or_else(nul)?;
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
/// opt-in `fixes.fillet_blend_corner`): whether the End's face G (the side edge's face that
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
    let side_edge = eu1::edge(target, side_eu)?.ok_or_else(nul)?;
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
    .ok_or_else(nul)?;
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
    let eu = eu1::primary_edgeuse(target, e3)?.ok_or_else(nul)?;
    check_tangential_edgeuse_at_vertex(k, target, eu, far, rad_to_deg(corner.tangency_tol_radians))
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
        let e = eu1::edge(target, curr)?.ok_or_else(nul)?;
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
            return Err(SolidError::sm_err(OP));
        };
        back.push(prev);
        curr = prev;
    }
    Err(SolidError::sm_err(OP))
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
        let side_edge = eu1::edge(target, side_eu)?.ok_or_else(nul)?;
        let curve = edge::curve(target, side_edge)?.ok_or_else(nul)?;
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
        return Err(SolidError::sm_err(OP));
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
    if !on_end_face || solver_eus.len() != 1 {
        return Ok(None);
    }
    let fil_eu = *solver_eus.first().ok_or_else(nul)?;
    solvers.solver_of_edgeuse(fil_eu)?;
    if !const_radius()? {
        return Ok(None);
    }
    let total = all_eus.len();
    let Some(fil) = all_eus.iter().position(|&eu| eu == fil_eu) else {
        return Ok(None);
    };
    if total < 3 {
        return Ok(None);
    }
    let eus = [
        *all_eus.get((fil + total - 1) % total).ok_or_else(nul)?,
        *all_eus.get((fil + 1) % total).ok_or_else(nul)?,
    ];
    if eus[0] == eus[1] {
        return Ok(None);
    }
    let vertex = corner.vertex.ok_or_else(nul)?;
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
        let side_eu = find_side_edgeuse(target, fil_eu, eu)?.ok_or_else(nul)?;
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
        if past && found.is_none_or(|(least, _)| fit < least) {
            found = Some((fit, eu1::edge(target, fil_eu)?.ok_or_else(nul)?));
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
            eu1::edge(target, eu)?.ok_or_else(nul)?
        }
        _ => return Ok(None),
    };
    let vertex = corner.vertex.ok_or_else(nul)?;
    let at_start = edge::start_vertex(target, side)? == Some(vertex);
    if !at_start && edge::end_vertex(target, side)? != Some(vertex) {
        return Ok(None);
    }
    if !edge::faces(target, side, None)?.contains(&face) {
        return Ok(None);
    }
    let curve = edge::curve(target, side)?.ok_or_else(nul)?;
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
    let cap = corner.end_cap.as_ref().ok_or_else(nul)?;
    for &fe in corner.edges.iter().skip(1) {
        if pseudo.fillet_edge(fe)?.fillet_edge_type() != FilletEdgeType::Precomputed
            || edge::curve(pseudo.brep(), fe)?.is_some()
        {
            continue;
        }
        let face = pseudo.fillet_edge(fe)?.original_face().ok_or_else(nul)?;
        let start = edge::vertex(pseudo.brep(), fe)?.ok_or_else(nul)?;
        let end = edge::other_vertex(pseudo.brep(), fe, Some(start))?.ok_or_else(nul)?;
        if !pseudo.fillet_vertex(start)?.is_processed()
            || !pseudo.fillet_vertex(end)?.is_processed()
        {
            return Err(SolidError::sm_err(OP));
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
        let zone = zone_tol3d(k, target.face(face)?.tolerance);
        let mut on = PointClassification::new(zone);
        point_3d_classify(
            k,
            target,
            face,
            pseudo.vertex_point(rail_end)?,
            zone,
            true,
            &mut on,
            Point3dClassifyOptions::default(),
        )?;
        if on.point_class() == PointClassType::Unknown {
            // The rail ends off its side face, past the face's tangent edge at the vertex:
            // it rolls over into the End, which is not built. That edge runs through the
            // vertex, on the cap's plane, so the ball touching the side face on it at the
            // plane is the vertex itself: no radius keeps the rail on its face up to the
            // plane.
            let rib = *corner.edges.first().ok_or_else(nul)?;
            let rib_eu = eu1::primary_edgeuse(pseudo.brep(), rib)?;
            let geom = match rib_eu {
                Some(eu) => pseudo.fillet_edgeuse(eu)?.fillet_geom(),
                None => None,
            };
            let solver = match geom {
                Some(g) => pseudo.geom(g)?.fillet_solver(),
                None => None,
            }
            .ok_or_else(nul)?;
            let fs = solvers.solver(solver)?;
            let blended = fs.edgeuse(0)?.ok_or_else(nul)?;
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
            .ok_or_else(nul)?
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
        let curve = curves.curve_3d.ok_or_else(nul)?;
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
    let fil_edge = eu1::edge(target, fil_eu)?.ok_or_else(nul)?;
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

/// Not the fork's (#2406; `fixes.fillet_blend_corner`): whether an End at a smooth
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

/// Not the fork's (#2406; `fixes.fillet_blend_corner`): whether an End at a smooth vertex
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

/// Not the fork's (#2406; `fixes.fillet_blend_corner`): whether an End at a smooth vertex
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
    let vertex = corner.vertex.ok_or_else(nul)?;
    for &fe in corner.edges.iter().skip(1) {
        if pseudo.fillet_edge(fe)?.fillet_edge_type() != FilletEdgeType::Precomputed
            || edge::curve(pseudo.brep(), fe)?.is_some()
        {
            continue;
        }
        let face = pseudo.fillet_edge(fe)?.original_face().ok_or_else(nul)?;
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
    let at = vertex::point(target, corner.filleted_vertex().ok_or_else(nul)?)?;
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
    let vertex = corner.vertex.ok_or_else(nul)?;
    for &fe in corner.edges.iter().skip(1) {
        if pseudo.fillet_edge(fe)?.fillet_edge_type() != FilletEdgeType::Precomputed
            || edge::curve(pseudo.brep(), fe)?.is_some()
        {
            continue;
        }
        let face = pseudo.fillet_edge(fe)?.original_face().ok_or_else(nul)?;
        let start = edge::vertex(pseudo.brep(), fe)?.ok_or_else(nul)?;
        let end = edge::other_vertex(pseudo.brep(), fe, Some(start))?.ok_or_else(nul)?;
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
            let zone = zone_tol3d(k, target.face(face)?.tolerance);
            let mut on = PointClassification::new(zone);
            point_3d_classify(
                k,
                target,
                face,
                pseudo.vertex_point(rail_end)?,
                zone,
                true,
                &mut on,
                Point3dClassifyOptions::default(),
            )?;
            if on.point_class() != PointClassType::Unknown {
                continue;
            }
        }
        let rib = *corner.edges.first().ok_or_else(nul)?;
        let geom = match eu1::primary_edgeuse(pseudo.brep(), rib)? {
            Some(eu) => pseudo.fillet_edgeuse(eu)?.fillet_geom(),
            None => None,
        };
        let solver = match geom {
            Some(g) => pseudo.geom(g)?.fillet_solver(),
            None => None,
        }
        .ok_or_else(nul)?;
        let fs = solvers.solver(solver)?;
        let fil_eu = fs.edgeuse(0)?.ok_or_else(nul)?;
        if ends_in_tangent_edge(k, target, fil_eu, face, vertex, fs.tangency_tolerance)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The fork's `SmBlendCorner::CalcEndEdgeGeom` (41a39ec) before its closing
/// `SmFilletCorner::CalcCornerEdgeGeom`, which the caller runs: an End at a smooth vertex
/// computes its cap's edges ([`calc_end_cap_edges`]); an End's end rib (or, on more than
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
            .ok_or_else(nul)?;
        let fil_edge = eu1::edge(target, fil_eu)?.ok_or_else(nul)?;
        let concave = is_concave_edge(k, target, fil_edge)?;
        let vertex = corner.vertex.ok_or_else(nul)?;
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
                return Err(nul());
            };
            // the side edge from the vertex, continued across seams' vertices up to where
            // the rail's end was last looked for (CalcRailIntEdgeuse; 63c7962)
            let chain = side_edge_chain_to(k, target, corner, side_eu)?;
            let side_eu = *chain.first().ok_or_else(nul)?;
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
    if !(on_end_faces || rib_is_side) {
        return Ok(EndEdges::Built);
    }
    let start_fv = if on_end_faces {
        corner.vertices.first().copied()
    } else {
        edge::vertex(pseudo.brep(), end_rib.ok_or_else(nul)?)?
    }
    .ok_or_else(nul)?;
    let end_fv = if on_end_faces {
        corner.vertices.get(1).copied()
    } else {
        edge::other_vertex(pseudo.brep(), end_rib.ok_or_else(nul)?, Some(start_fv))?
    }
    .ok_or_else(nul)?;
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
        let rib = end_rib.ok_or_else(nul)?;
        let rib_eu = eu1::primary_edgeuse(pseudo.brep(), rib)?;
        let rib_geom = match rib_eu {
            Some(eu) => pseudo.fillet_edgeuse(eu)?.fillet_geom(),
            None => None,
        };
        Some(
            match rib_geom {
                Some(g) => pseudo.geom(g)?.fillet_surface(None)?.cloned(),
                None => None,
            }
            .ok_or_else(nul)?,
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
            let side_eu = match record.point_class_object() {
                Some(EntityRef::Edgeuse(eu)) => Some(eu),
                _ => None,
            };
            let side_edge = match side_eu {
                Some(eu) => eu1::edge(target, eu)?,
                None => None,
            }
            .ok_or_else(nul)?;
            let curve = edge::curve(target, side_edge)?.ok_or_else(nul)?;
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
    let record = pseudo.fillet_vertex(miss_fv)?;
    if record.point_class() != PointClassType::Edgeuse {
        return Ok(EndEdges::Built);
    }
    // The side edge the rail missed, and the fillet's side face it does not bound.
    let Some(EntityRef::Edgeuse(side_eu)) = record.point_class_object() else {
        return Err(nul());
    };
    let fil_eu = corner
        .solver_eus
        .as_ref()
        .and_then(|eus| eus.first().copied())
        .ok_or_else(nul)?;
    let side_faces = [face_of(target, side_eu)?, radial_face(target, side_eu)?];
    let fil_faces = [face_of(target, fil_eu)?, radial_face(target, fil_eu)?];
    let mut other_face = fil_faces[0];
    if side_faces.contains(&other_face) {
        other_face = fil_faces[1];
    }
    let surface = crate::topology::face::p2::surface(target, other_face)?
        .ok_or_else(nul)?
        .clone_for_evaluation()?;
    let side_edge = eu1::edge(target, side_eu)?.ok_or_else(nul)?;
    let curve = edge::curve(target, side_edge)?
        .ok_or_else(nul)?
        .clone_for_evaluation()?;
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
            return Err(SolidError::sm_err(OP));
        }
        solvers.solver_of_edgeuse(fil_eu)?;
        let vertex = corner.vertex.ok_or_else(nul)?;
        note_open_end(fil_eu, vertex, end_face, false)?;
        let rib = end_rib.ok_or_else(nul)?;
        pseudo.delete_wire_fillet_edge(k, rib)?;
        corner.edges.clear();
    } else if !runs_on
        && !both_miss
        && end_face_runs_on_past_side_edge(
            k,
            target,
            corner,
            side_eu,
            rail_face(fil_faces, side_faces),
        )?
    {
        // Not the fork's: the end face runs on tangentially past the side edge's far end
        // (`end_face_runs_on_past_side_edge`): the end rib is deleted and the global merge
        // cuts the round where it meets the end face and its continuation, as WR-303's did.
        let rib = end_rib.ok_or_else(nul)?;
        pseudo.delete_wire_fillet_edge(k, rib)?;
        corner.edges.clear();
        return Ok(EndEdges::GlobalMerge);
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
        .ok_or_else(nul)?
        .copy_and_add_analytics(env.k)?;
    corner.surfaces.push(cap);
    match corner.make_face_brep_with(env) {
        Ok(()) => Ok(true),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(false),
    }
}
