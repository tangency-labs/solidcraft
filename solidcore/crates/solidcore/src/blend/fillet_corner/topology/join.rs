//! Topology of the blend corner's Join: the blends end on one shared section.
//!
//! Upstream: none (pinned SMLib builds a G1 continuation with `SmFillet1x1Corner`,
//! `SmFillet2x2Corner` or `SmFilletNx2Corner`). The fork's
//! `source/SMLib/src/SmBlendCornerJoin.cpp` (tangency-labs/smlib `68e039c`, `543d25a`;
//! smlib#79 step 4d).
//!
//! | Rust                                  | Fork                               |
//! |---------------------------------------|------------------------------------|
//! | [`FilletCorner::make_join_topology`]  | `SmBlendCorner::MakeJoinTopology`  |
//! | `join_sides` (private)                | `sm_JoinSides`                     |
//! | [`JoinTopology`]                      | its `NoteError` + `SER(SM_ERR)` refusal |
//! | `unequal_blends` (private)            | not the fork's (#2905)             |
//! | `rolled_sides` (private)              | not the fork's (solidcore-native: rails rolled over a smooth far edge) |
//!
//! Deviations: the caller resolves `GetFilletSolverOfEdgeuse` of the blended edge uses
//! ([`JoinTopologyEnv::solver_eu_solvers`]) and notes the refusal's error
//! ([`JoinTopology::RefusedAcrossEdges`]); blends on unequal offsets are refused (not the
//! fork's, #2905: [`JoinTopology::RefusedUnequalOffsets`]). A missing solver the fork dereferences
//! through `NER` is `SM_ERR_NULL_POINTER`; other missing topology is an input error. The
//! walks round the vertex are bounded by its edge uses (WR-697). Not the fork's
//! (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): a blend whose rail rolls over
//! a smooth far edge of its face rolls on the face beyond, so where the one edge between the
//! blends on a side is that edge and the other blend rolls on that face, nothing stands between
//! them there (`rolled_sides`).

use super::{
    defect, find_side_edgeuse, missing_defect, need, new_corner_vertex, radial, shape_not_built,
    NO_SOLVER_OF_BLENDED_EU, NO_SOLVER_SURFACE,
};
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_executive::{p3::FitKind, session::SolverObject};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletEdgeType, FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::error::blend::CornerShape;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::det;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::brep::p7::LOOP_GAP_ZONES;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{Brep, EdgeuseId, FaceId};

const OP: &str = "blend::fillet_corner::topology::join";

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The fork's refusal comment (`SM_FILERR_NO_ROOM` with no size building it since
/// 41a39ec; `SM_FILERR_CORNER_UNSUPPORTED` with 543d25a's comment before).
pub const JOIN_ACROSS_EDGES: &str =
    "Fillet Error: blends continuing each other across more than one edge on a side have no section both reach: no size builds it";

/// The comment of [`JoinTopology::RefusedUnequalOffsets`] (not the fork's, #2905).
pub const JOIN_UNEQUAL_OFFSETS: &str =
    "Fillet Error: blends continuing each other at a smooth vertex have different offsets, so no section ends both: give them one offset";

/// The comment of the refusal [`FilletCorner::join_rail_misses_side_edge`] names (not the
/// fork's, #17224).
pub const JOIN_RAIL_MISSES_SIDE_EDGE: &str =
    "Fillet Error: blends continuing each other across a smooth edge between their faces meet only where their contact lines cross that edge, and at this size they never reach it";

/// The golden-section steps of [`largest_ball_on_side_edge`]: 0.618^80 is about 2e-17 of
/// the side edge's parameter range, below a double's resolution of it, so a further step
/// moves nothing.
const GOLDEN_STEPS: usize = 80;

/// Actual objects `SmBlendCorner::MakeJoinTopology` uses.
#[derive(Debug)]
pub struct JoinTopologyEnv<'a> {
    /// The executive's kernel.
    pub k: &'a Kernel,
    /// The target whose edge uses the corner stores.
    pub target: &'a mut Brep,
    /// The executive's actual pseudo-B-rep.
    pub pseudo: &'a mut FilletBrep,
    /// This corner's identity in the executive's corner store.
    pub corner: FilletCornerId,
    /// The executive's solver store.
    pub solvers: &'a [Option<SolverObject>],
    /// `GetFilletSolverOfEdgeuse` of each `m_pSolverEUs` entry, in order (`None`:
    /// `NULL`).
    pub solver_eu_solvers: Vec<Option<FilletSolverId>>,
}

/// How [`FilletCorner::make_join_topology`] ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JoinTopology {
    /// The Join's topology is built.
    Built,
    /// More than one edge stands between the blends on a side: the fork notes
    /// `SM_FILERR_NO_ROOM` at the vertex ([`JOIN_ACROSS_EDGES`]) and the fit 0 on the
    /// first blended edge (41a39ec: no size builds it), and returns `SM_ERR`; the caller
    /// does these.
    RefusedAcrossEdges,
    /// Two constant blends with definitions on, each on its own offsets, whose offsets on a
    /// side differ by more than a loop closes (`unequal_blends`): their rails miss the
    /// shared section, and their faces could not be made (not the fork's, #2905). The
    /// caller notes `SM_FILERR_NO_ROOM` at the vertex
    /// ([`JOIN_UNEQUAL_OFFSETS`]) and the fit `builds` on blend `blend`'s edge.
    RefusedUnequalOffsets {
        /// The blend with the larger offset or chord, 0 or 1 in the corner's order.
        blend: usize,
        /// The other blend's offset or chord length.
        builds: f64,
        /// Whether the fit sizes a radius or a chord.
        kind: FitKind,
    },
}

/// Why a side or blend index past the Join's two sides and two blends is the kernel's fault.
const SIDE_INDEX: &str = "a side or blend index of the Join is past its two sides and two \
     blends, though the sides are the first and the other and the blends one or two";

/// Why a blend index past the Join's solvers and geometries is the kernel's fault.
const PER_BLEND: &str = "the Join has no solver or geometry for a blend, though the loop that \
     resolves its solvers takes one solver and one geometry for each of its one or two blends";

/// The live solver `id` (`NULL`: a reviewed `Defect`, `SM_ERR_NULL_POINTER`).
fn live(solvers: &[Option<SolverObject>], id: Option<FilletSolverId>) -> Result<&FilletSolver> {
    let id = id.ok_or_else(|| missing_defect(OP, NO_SOLVER_OF_BLENDED_EU))?;
    solvers
        .get(id.0 as usize)
        .and_then(Option::as_ref)
        .map(SolverObject::base)
        .ok_or_else(|| invalid("corner solver is not live"))
}

/// The four edge uses at an isolated tangent contact of a concave chamfer and a
/// sharp rim. The strips share their supports through the contact, but the
/// material beyond the rim is absent. Preserve that rim for the crease trim.
pub(crate) fn tangent_cliff_join(
    k: &Kernel,
    target: &mut Brep,
    vertex: crate::topology::VertexId,
    selected: &[EdgeuseId],
    ring: &[EdgeuseId],
    angle_tol_deg: f64,
) -> Result<Option<crate::blend::blend_corner::BlendCornerEvent>> {
    use crate::blend::fillet_corner::p1::test_g1_continuity;
    use crate::blend::fillet_solver::p5::{edge_bounds_material, is_concave_edge};
    use crate::surface::surface::SurfaceKind;
    use crate::topology::edge;

    // The corner stores one outgoing use per distinct edge. A closed spine
    // supplies both end tangents and may have its surface seam at the contact.
    let (first, second, closed) = match selected {
        [first, second] if ring.len() == 4 => (first, second, false),
        [edge] if (3..=4).contains(&ring.len()) => (edge, edge, true),
        _ => return Ok(None),
    };
    let (Some(a), Some(b)) = (eu1::edge(target, *first)?, eu1::edge(target, *second)?) else {
        return Ok(None);
    };
    if (closed && !edge::is_closed(target, a)?)
        || (!closed && a == b)
        || !edge_bounds_material(target, a)?
        || !is_concave_edge(k, target, a)?
        || (!closed && (!edge_bounds_material(target, b)? || !is_concave_edge(k, target, b)?))
        || !test_g1_continuity(k, target, Some(vertex), Some(a), Some(b), angle_tol_deg)?.0
    {
        return Ok(None);
    }
    if closed {
        let supports = faces_of(target, *first)?;
        for &eu in ring {
            let adjacent = faces_of(target, eu)?;
            if !adjacent
                .iter()
                .flatten()
                .any(|face| supports.contains(&Some(*face)))
            {
                return Ok(None);
            }
        }
    }
    let (sides, between) = join_sides(target, selected, ring)?;
    let same_support = |others: &[EdgeuseId], faces: [Option<FaceId>; 2]| -> Result<bool> {
        let [Some(f0), Some(f1)] = faces else {
            return Ok(false);
        };
        if f0 != f1 {
            return Ok(false);
        }
        match others {
            [] => Ok(true),
            [seam] if closed => Ok(faces_of(target, *seam)? == [Some(f0); 2]),
            _ => Ok(false),
        }
    };
    let (rim_uses, rim_support) = match &between {
        [before, after] if before.len() == 2 && same_support(after, sides[1])? => {
            (before, &sides[0])
        }
        [before, after] if after.len() == 2 && same_support(before, sides[0])? => {
            (after, &sides[1])
        }
        _ => return Ok(None),
    };
    let [left, right] = rim_uses.as_slice() else {
        return Ok(None);
    };
    let (Some(left), Some(right)) = (eu1::edge(target, *left)?, eu1::edge(target, *right)?) else {
        return Ok(None);
    };
    if left == right
        || is_concave_edge(k, target, left)?
        || is_concave_edge(k, target, right)?
        || !test_g1_continuity(
            k,
            target,
            Some(vertex),
            Some(left),
            Some(right),
            angle_tol_deg,
        )?
        .0
        || !test_g1_continuity(k, target, Some(vertex), Some(a), Some(left), angle_tol_deg)?.0
    {
        return Ok(None);
    }
    let [Some(f0), Some(f1)] = *rim_support else {
        return Ok(None);
    };
    let plane_normal = |f| -> Result<Option<crate::math::Vec3>> {
        Ok(
            match crate::topology::face::p2::surface(target, f)?
                .map(crate::surface::surface::Surface::kind)
            {
                Some(SurfaceKind::Plane(p)) => Some(p.evaluate_normal().unitize()?),
                _ => None,
            },
        )
    };
    let (Some(n0), Some(n1)) = (plane_normal(f0)?, plane_normal(f1)?) else {
        return Ok(None);
    };
    // Both planes pass through this topological vertex. Equal normals prove
    // that the second support continues the first, including split faces.
    if n0.cross(n1).length() > crate::math::EFF_ZERO {
        return Ok(None);
    }
    let left_faces = edge::faces(target, left, None)?;
    let right_faces = edge::faces(target, right, None)?;
    let mut common = left_faces
        .into_iter()
        .filter(|f| *f != f0 && *f != f1 && right_faces.contains(f));
    let Some(cliff) = common.next() else {
        return Ok(None);
    };
    let Some(n_cliff) = plane_normal(cliff)? else {
        return Ok(None);
    };
    if common.next().is_some() || n0.cross(n_cliff).length() <= crate::math::EFF_ZERO {
        return Ok(None);
    }
    Ok(Some(
        crate::blend::blend_corner::BlendCornerEvent::CliffJoin {
            rim: [left, right],
            cliff,
        },
    ))
}

/// Whether two blends at a Join are constant and differ: for each of the two faces of side
/// `faces` first, its offset, and where the blends' offsets on a side differ by more than a
/// loop's gap closes ([`LOOP_GAP_ZONES`] of the body's tolerance), the blend with the larger
/// and the other's offset. At a cliff join, constant-distance blends compare their chord
/// lengths by the same rule. Each blend's rails are on its own offsets, which
/// `SM_FE_CROSS_SECTION` cannot end both of. `None` for anything else: other solvers (a
/// law's rails are the law's to meet), or one closed blend.
fn unequal_blends(
    target: &Brep,
    solvers: &[Option<SolverObject>],
    ids: &[Option<FilletSolverId>],
    faces: [Option<FaceId>; 2],
    cliff_join: bool,
) -> Result<Option<(usize, f64, FitKind)>> {
    let [Some(first), Some(second)] = ids else {
        return Ok(None);
    };
    let object = |id: FilletSolverId| solvers.get(id.0 as usize).and_then(Option::as_ref);
    let (first, second, kind) = match (object(*first), object(*second)) {
        (Some(SolverObject::ConstantRadius(a)), Some(SolverObject::ConstantRadius(b))) => {
            let offsets = |solver: &FilletSolver, face| -> Result<[f64; 2]> {
                let rail = solver.find_index_of_rail_on_face(target, face)?;
                let offset = |index| {
                    solver
                        .surface(index)?
                        .map(|slot| slot.surface.surface.offset_distance().abs())
                        .ok_or_else(|| missing_defect(OP, NO_SOLVER_SURFACE))
                };
                Ok([offset(rail)?, offset(1 - rail.min(1))?])
            };
            let [first_face, second_face] = faces;
            (
                offsets(&a.solver, first_face)?,
                offsets(&b.solver, second_face)?,
                FitKind::Radius,
            )
        }
        (Some(SolverObject::ConstantDistance(a)), Some(SolverObject::ConstantDistance(b)))
            if cliff_join
                && a.solver.station_setback().is_none()
                && b.solver.station_setback().is_none() =>
        {
            ([a.distance(); 2], [b.distance(); 2], FitKind::Chord)
        }
        _ => return Ok(None),
    };
    let gap = LOOP_GAP_ZONES * target.tolerance();
    for (x, y) in first.into_iter().zip(second) {
        if (x - y).abs() > gap {
            return Ok(Some(if x > y { (0, y, kind) } else { (1, x, kind) }));
        }
    }
    Ok(None)
}

fn lookup(corner: FilletCornerId, this: &FilletCorner) -> CornerLookup<'_> {
    CornerLookup {
        this: Some((Some(corner), this)),
        ..Default::default()
    }
}

/// `(face, radial face)` of `eu`.
fn faces_of(target: &Brep, eu: EdgeuseId) -> Result<[Option<FaceId>; 2]> {
    Ok([
        eu1::face(target, eu)?,
        eu1::face(target, radial(target, eu)?)?,
    ])
}

/// `sm_JoinSides`: `[side][blend]` the blend's face on each side, and `[side]` the edge
/// uses between the blends there. Going round the vertex from the first blended edge use
/// to the second, the first side's faces lie between them, the second side's from the
/// second back to the first. Each blend's face on a side is the one it shares with its
/// neighbour on that side: the same face for both blends when nothing stands between
/// them there. With two edges in all, or one closed edge, both blends roll on the first
/// edge use's face (first side) and its radial's (second side).
type JoinSides = ([[Option<FaceId>; 2]; 2], [Vec<EdgeuseId>; 2]);

fn join_sides(target: &Brep, solver_eus: &[EdgeuseId], all_eus: &[EdgeuseId]) -> Result<JoinSides> {
    let eu1_ = *solver_eus
        .first()
        .ok_or_else(|| invalid("corner without blended edge use"))?;
    let [f0, f1] = faces_of(target, eu1_)?;
    let mut side = [[f0, f0], [f1, f1]];
    let mut between: [Vec<EdgeuseId>; 2] = [Vec::new(), Vec::new()];
    let edge1 = eu1::edge(target, eu1_)?;
    if solver_eus.len() == 1 {
        // one closed edge meeting itself: the other edges lie on its faces' sides by
        // their faces
        for &eu in all_eus {
            if eu1::edge(target, eu)? == edge1 {
                continue;
            }
            let faces = faces_of(target, eu)?;
            if faces.contains(&f0) {
                between[0].push(eu);
            } else if faces.contains(&f1) {
                between[1].push(eu);
            } else {
                return Err(shape_not_built(
                    OP,
                    CornerShape::RingEdgeOffClosedBlendFaces,
                ));
            }
        }
        return Ok((side, between));
    }
    if all_eus.len() <= 2 {
        return Ok((side, between));
    }
    let eu2_ = *solver_eus
        .get(1)
        .ok_or_else(|| invalid("corner without second blended edge use"))?;
    let edge2 = eu1::edge(target, eu2_)?;
    let total = all_eus.len();
    let (mut idx1, mut idx2) = (None, None);
    for (ii, &eu) in all_eus.iter().enumerate() {
        let e = eu1::edge(target, eu)?;
        if e == edge1 {
            idx1 = Some(ii);
        }
        if e == edge2 {
            idx2 = Some(ii);
        }
    }
    let (Some(idx1), Some(idx2)) = (idx1, idx2) else {
        return Err(shape_not_built(OP, CornerShape::BlendedEdgeOffRing));
    };
    let at = |kk: usize| {
        all_eus
            .get(kk)
            .copied()
            .ok_or_else(|| invalid("edge use index past the ring"))
    };
    // (ULONG kk = (lIdx1 + 1) % lTotal; kk != lIdx2; kk = (kk + 1) % lTotal): at most
    // lTotal steps
    let mut kk = (idx1 + 1) % total;
    while kk != idx2 {
        between[0].push(at(kk)?);
        kk = (kk + 1) % total;
    }
    let mut kk = (idx2 + 1) % total;
    while kk != idx1 {
        between[1].push(at(kk)?);
        kk = (kk + 1) % total;
    }
    // a side with edges between the blends: each blend's face is the one it shares with
    // its neighbour there; its face on the other side is its other face
    let blended = [eu1_, eu2_];
    let known = usize::from(between[0].is_empty());
    let other = 1 - known;
    let (Some(&first_known), Some(&last_known)) = (
        between.get(known).and_then(|b| b.first()),
        between.get(known).and_then(|b| b.last()),
    ) else {
        return Err(defect(
            OP,
            "neither side of the Join has an edge between the blends, though its ring has more \
             than two edges and the walks from each blend to the other take every edge but \
             theirs",
        ));
    };
    let neighbour = if known == 0 {
        [first_known, last_known]
    } else {
        [last_known, first_known]
    };
    for bb in 0..2 {
        let eu = *blended
            .get(bb)
            .ok_or_else(|| invalid("blend index past 1"))?;
        let nb = *neighbour
            .get(bb)
            .ok_or_else(|| invalid("blend index past 1"))?;
        // the neighbour is next to the blend in the ring
        let known_face = face_by_two_edgeuses(target, Some(eu), Some(nb), None)?
            .ok_or_else(|| shape_not_built(OP, CornerShape::RingEdgesShareNoFace))?;
        // one of the blend's faces: the shared face is found among them
        let faces = faces_of(target, eu)?;
        let other_face = if Some(known_face) == faces[0] {
            faces[1]
        } else {
            faces[0]
        };
        if let Some(slot) = side.get_mut(known).and_then(|s| s.get_mut(bb)) {
            *slot = Some(known_face);
        }
        if let Some(slot) = side.get_mut(other).and_then(|s| s.get_mut(bb)) {
            *slot = other_face;
        }
    }
    Ok((side, between))
}

/// Not the fork's (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): on each side
/// (`side`, `sm_JoinSides`'s faces), where the one edge between the two blends there is a
/// smooth far edge of one blend's face that its rail rolls over onto the face beyond, and the
/// other blend's face there is that face, the face both blends roll on: nothing stands between
/// them there, so that side's edge is taken out of `between` and the section's end there lies
/// on that face, through the vertex. `None` on every other side.
///
/// # Errors
/// A reviewed `Defect` (`SM_ERR_NULL_POINTER`) without a solver surface; those of the
/// topology queries.
fn rolled_sides(
    target: &Brep,
    solvers: &[Option<SolverObject>],
    ids: &[Option<FilletSolverId>],
    side: &[[Option<FaceId>; 2]; 2],
    between: &mut [Vec<EdgeuseId>; 2],
) -> Result<[Option<FaceId>; 2]> {
    let mut out = [None, None];
    if ids.len() != 2 {
        return Ok(out);
    }
    for ((slot, own), between) in out.iter_mut().zip(side).zip(between.iter_mut()) {
        let [only] = between.as_slice() else {
            continue;
        };
        let edge = eu1::edge(target, *only)?
            .ok_or_else(|| invalid("edge use between the blends without edge"))?;
        let bounds = crate::topology::edge::faces(target, edge, None)?;
        for bb in 0..2 {
            let (Some(&Some(face)), Some(&other), Some(&id)) =
                (own.get(bb), own.get(1 - bb), ids.get(bb))
            else {
                continue;
            };
            let solver = live(solvers, id)?;
            if solver.rolled_faces.iter().all(Option::is_none) {
                continue;
            }
            let rail = solver.find_index_of_rail_on_face(target, Some(face))?;
            let Some(onto) = solver.rolled_faces.get(rail).copied().flatten() else {
                continue;
            };
            if other == Some(onto) && bounds.contains(&face) && bounds.contains(&onto) {
                *slot = Some(onto);
                break;
            }
        }
        if slot.is_some() {
            between.clear();
        }
    }
    Ok(out)
}

/// Not the fork's (tangency-labs/solidcraft#21940): on the closed blend's side whose one edge
/// between its ends is a seam of a sphere face F there (both its edge uses on F) that runs
/// from the corner's `vertex` into F and ends at a vertex no other edge reaches (the sphere's
/// pole, as on a sphere turned so that its pole lies near the rim), where the blend's other
/// face G is a plane and every ball touching F along the seam and touching G is smaller than
/// the round: the seam lies inside the band the round takes off F, the round's contact line
/// never crosses it, and nothing stands between the blend's ends there. The seam is taken out
/// of `between`, so the section's end on that side lies on F, through the vertex, as where no
/// edge runs from it.
///
/// Decided exactly: the seam is the arc of a great circle of the sphere from the vertex to the
/// pole, and the ball touching the sphere at a point and the plane grows with the point's
/// distance h from the plane (`h r / (r - h)`, for a nub as for a socket: the ball lies outside
/// the sphere in both);
/// so the largest ball along the seam is the one at the arc's point farthest from the plane
/// ([`seam_point_farthest_from_plane`], in closed form), and `ball_touching_with` there decides.
/// A seam that runs over the sphere's top and back down (a sphere turned so that its seam leaves
/// the rim across the face) reaches the contact line there and stays a side edge. Other surfaces
/// are left as they were.
///
/// Returns the face whose seam was taken out (the corner's `seam_short_of_rails`).
///
/// # Errors
/// Those of the topology queries, the seam test, the solver lookup and `ball_touching_with`.
fn seams_short_of_rails(
    k: &Kernel,
    target: &mut Brep,
    solvers: &[Option<SolverObject>],
    ids: &[Option<FilletSolverId>],
    blended: EdgeuseId,
    vertex: Option<crate::topology::VertexId>,
    side: &[[Option<FaceId>; 2]; 2],
    between: &mut [Vec<EdgeuseId>; 2],
) -> Result<Option<FaceId>> {
    use crate::surface::surface::p2::SurfaceKind;
    // one closed blend meeting itself
    let (1, Some(&Some(id)), Some(vertex)) = (ids.len(), ids.first(), vertex) else {
        return Ok(None);
    };
    for s in 0..2 {
        let Some(&[only]) = between.get(s).map(Vec::as_slice) else {
            continue;
        };
        let (Some(&Some(face)), Some(&Some(other_face))) = (
            side.get(s).and_then(|f| f.first()),
            side.get(1 - s).and_then(|f| f.first()),
        ) else {
            continue;
        };
        if faces_of(target, only)? != [Some(face); 2] {
            continue;
        }
        let (Some(SurfaceKind::Sphere(sphere)), Some(SurfaceKind::Plane(plane))) = (
            crate::topology::face::p2::surface(target, face)?
                .map(crate::surface::surface::Surface::kind),
            crate::topology::face::p2::surface(target, other_face)?
                .map(crate::surface::surface::Surface::kind),
        ) else {
            continue;
        };
        let centre = sphere.base().position().origin();
        let radius = sphere.radius;
        let plane_origin = plane.position().origin();
        let plane_normal = plane.evaluate_normal();
        let Some(edge) = eu1::edge(target, only)? else {
            continue;
        };
        if !crate::topology::edge::is_seam(k, target, edge)? {
            continue;
        }
        let Some(far) = crate::topology::edge::other_vertex(target, edge, Some(vertex))? else {
            continue;
        };
        if far == vertex || crate::topology::vertex::edges(target, far, None)? != [edge] {
            continue;
        }
        let Some(side_eu) = find_side_edgeuse(target, blended, only)? else {
            continue;
        };
        let Some(at) = seam_point_farthest_from_plane(
            centre,
            radius,
            crate::topology::vertex::point(target, vertex)?,
            crate::topology::vertex::point(target, far)?,
            plane_origin,
            plane_normal,
        ) else {
            continue;
        };
        let (on_g, within, _) = crate::blend::blend_corner::end::ball_touching_with(
            k,
            target,
            live(solvers, Some(id))?,
            side_eu,
            other_face,
            at,
            None,
        )?;
        if within && !on_g {
            if let Some(b) = between.get_mut(s) {
                b.clear();
                return Ok(Some(face));
            }
        }
    }
    Ok(None)
}

/// Not the fork's (tangency-labs/solidcraft#21940): the point of the arc of the great circle of
/// the sphere about `centre` of radius `radius` from its point `from` to its point `to` (less
/// than half a turn: a seam from a rim's vertex to a pole) farthest from the plane through
/// `plane_origin` square to `plane_normal`. On the arc `p(t) = c + r (cos t e1 + sin t e2)`,
/// `t` in `[0, phi]`, the distance `n . (p - o)` is `a + r (A cos t + B sin t)`, whose extrema
/// lie at `atan2(B, A)` and half a turn from it; the farthest of those inside the arc and the
/// arc's ends is taken. `None` where the arc is not defined (`to` on the line of `centre` and
/// `from`).
fn seam_point_farthest_from_plane(
    centre: crate::math::Point3,
    radius: f64,
    from: crate::math::Point3,
    to: crate::math::Point3,
    plane_origin: crate::math::Point3,
    plane_normal: crate::math::Vec3,
) -> Option<crate::math::Point3> {
    let e1 = (from - centre).unitize().ok()?;
    let w = to - centre;
    let e2 = (w - e1 * w.dot(e1)).unitize().ok()?;
    let phi = det::atan2(w.dot(e2), w.dot(e1));
    let (a, b) = (plane_normal.dot(e1), plane_normal.dot(e2));
    let point = |t: f64| centre + (e1 * det::cos(t) + e2 * det::sin(t)) * radius;
    let distance = |t: f64| plane_normal.dot(point(t) - plane_origin).abs();
    let peak = det::atan2(b, a);
    let mut best = (0.0, distance(0.0));
    for t in [phi, peak, peak + crate::math::PI, peak - crate::math::PI] {
        if (0.0..=phi).contains(&t) && distance(t) > best.1 {
            best = (t, distance(t));
        }
    }
    Some(point(best.0))
}

/// Not the fork's (#17224): where the Join's section ends on a side edge `side_eu`
/// (`SM_FV_RAIL_X_EDGEUSE`), whether the constant round `fs` has a ball of its radius
/// touching its face F there (F the round's face the side edge bounds, G `other_face` its
/// other face). The round's contact line on F crosses the side edge exactly where the ball
/// touching F at the edge's point and touching G (`ball_touching_with`, its radius rho) is
/// the round's own; where rho stays below the round's radius along the whole side edge the
/// contact line never reaches it. `None` where some point of the side edge has rho at the
/// round's radius (or the point is not on G's side of F, so no smaller ball touches it),
/// else `Some(rho_max)`, the largest radius whose contact line reaches the edge.
///
/// rho is 0 at the corner's vertex (on both faces) and is maximized along the side edge by
/// golden section over its parameter range ([`GOLDEN_STEPS`]); a point where the ball of the
/// round's radius already reaches G answers at once, as the rounds that cross their side edge
/// do at their first step. The search assumes one peak of rho along the edge, which nothing
/// derives, so it never decides whether a round builds: it is asked only after the section's
/// end on the side edge was looked for and not found ([`FilletCorner::join_rail_misses_side_edge`]),
/// and then only names why and the size that fits. A side edge without its curve is `None`:
/// nothing judged, the failure stands.
///
/// # Errors
/// Those of `ball_touching_with`.
fn largest_ball_on_side_edge(
    k: &Kernel,
    target: &Brep,
    fs: &FilletSolver,
    side_eu: EdgeuseId,
    other_face: FaceId,
) -> Result<Option<f64>> {
    let Some(e) = eu1::edge(target, side_eu)? else {
        return Ok(None);
    };
    let Some(curve) = crate::topology::edge::evaluation_curve(target, e)? else {
        return Ok(None);
    };
    let ivl = crate::topology::edge::interval(target, e)?;
    // rho at t, or None where the round's own ball touches F there
    let rho = |t: f64| -> Result<Option<f64>> {
        let p = curve.evaluate_point(k, t)?;
        let (on_g, within, rho) = crate::blend::blend_corner::end::ball_touching_with(
            k, target, fs, side_eu, other_face, p, None,
        )?;
        Ok(if on_g {
            Some(0.0)
        } else if within {
            Some(rho)
        } else {
            None
        })
    };
    let golden = 0.5 * (f64::sqrt(5.0) - 1.0);
    let (mut a, mut b) = (ivl.min(), ivl.max());
    let mut x1 = b - golden * (b - a);
    let mut x2 = a + golden * (b - a);
    let (Some(mut f1), Some(mut f2)) = (rho(x1)?, rho(x2)?) else {
        return Ok(None);
    };
    for _ in 0..GOLDEN_STEPS {
        if f1 > f2 {
            b = x2;
            x2 = x1;
            f2 = f1;
            x1 = b - golden * (b - a);
            let Some(f) = rho(x1)? else {
                return Ok(None);
            };
            f1 = f;
        } else {
            a = x1;
            x1 = x2;
            f1 = f2;
            x2 = a + golden * (b - a);
            let Some(f) = rho(x2)? else {
                return Ok(None);
            };
            f2 = f;
        }
    }
    Ok(Some(f1.max(f2)))
}

/// Not the fork's (#17224): the end of the edge of `eu` other than `vertex` where it lies on
/// the face `face` (a vertex of one of that face's edges), else `None`.
///
/// # Errors
/// Those of the topology queries.
fn far_end_on_face(
    target: &Brep,
    eu: EdgeuseId,
    vertex: Option<crate::topology::VertexId>,
    face: FaceId,
) -> Result<Option<crate::topology::VertexId>> {
    let Some(e) = eu1::edge(target, eu)? else {
        return Ok(None);
    };
    let Some(far) = crate::topology::edge::other_vertex(target, e, vertex)? else {
        return Ok(None);
    };
    if Some(far) == vertex {
        return Ok(None);
    }
    for g in crate::topology::face::p4::edges(target, face, None, false, None)? {
        if crate::topology::edge::start_vertex(target, g)? == Some(far)
            || crate::topology::edge::end_vertex(target, g)? == Some(far)
        {
            return Ok(Some(far));
        }
    }
    Ok(None)
}

impl FilletCorner {
    /// `SmBlendCorner::MakeJoinTopology`: the blends end on one shared section, one
    /// `SM_FE_CROSS_SECTION` corner edge with the first blend on its primary edge use and
    /// the second (the one closed blend, when one closed edge meets itself) on the mate.
    /// Each blend's rail on a side ends at the section's end on that side. Where nothing
    /// stands between the blends the section is the one through the vertex: its end on
    /// the first side `SM_FV_JOIN_SECTION`, on the other its mate (`SM_FV_MATE`). Where
    /// one edge stands between them on a side (their faces change there and are tangent
    /// along it, which the classification checks, or a face's seam runs from the vertex and
    /// the rails cross it: not one that ends inside the band, `seams_short_of_rails`)
    /// its end there is where the rails cross that edge (`SM_FV_RAIL_X_EDGEUSE`), where
    /// the first blend's ball is the second's. Its mate is on the other side, or
    /// with such an edge on both sides where the rails cross the other one too. More
    /// than one edge on a side is refused ([`JoinTopology::RefusedAcrossEdges`]). The
    /// corner is blending and tangent (`m_bG1Case`), with no patch of its own.
    ///
    /// Allocations and corner-list insertions occur in the fork's order and remain
    /// visible if a later operation fails.
    ///
    /// # Errors
    /// [`CornerShapeNotBuilt`](crate::error::blend::BlendFailure::CornerShapeNotBuilt)
    /// for a blended edge use not in the ring, a ring edge on neither face of a closed
    /// blend, neighbouring ring edges that share no face or a side edge not beside a blend
    /// (`SM_ERR_NULL_POINTER` for the last two, `SM_ERR` otherwise); reviewed `Defect`s for other than one or two blended edge uses, sides
    /// with no edge between the blends, indices past the sides and blends, ends not made
    /// (`SM_ERR`) and a missing solver (`SM_ERR_NULL_POINTER`); invalid topology; those of
    /// the rail lookups and pseudo-topology operations.
    #[allow(clippy::too_many_lines)] // one fork routine
    pub fn make_join_topology(&mut self, env: JoinTopologyEnv<'_>) -> Result<JoinTopology> {
        let JoinTopologyEnv {
            k,
            target,
            pseudo,
            corner,
            solvers,
            solver_eu_solvers,
        } = env;
        let solver_eus = need(self.solver_eus.clone(), "corner without blended edge uses")?;
        let all_eus = need(self.all_eus.clone(), "corner without outward edge uses")?;
        let blends = solver_eus.len();
        if blends != 1 && blends != 2 {
            return Err(defect(
                OP,
                "the Join has other than one or two blended edge uses, though the corner's \
                 create makes a Join only of one closed blended edge or two blended edges",
            ));
        }
        let (side, mut between) = join_sides(target, &solver_eus, &all_eus)?;
        let rolled = rolled_sides(target, solvers, &solver_eu_solvers, &side, &mut between)?;
        self.seam_short_of_rails = seams_short_of_rails(
            k,
            target,
            solvers,
            &solver_eu_solvers,
            *solver_eus.first().ok_or_else(|| defect(OP, PER_BLEND))?,
            self.vertex,
            &side,
            &mut between,
        )?;
        let cliff_join = self
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_cliff_join);
        // the section's ends: on a side with an edge between the blends (where they change
        // face, or cross a face's seam) where the rails cross it, else (the first side)
        // through the vertex
        if cliff_join {
            // The two rim segments meet here; neither bounds the shared section
            // on the continued support. The face beyond trims the sheet later.
            for edges in &mut between {
                edges.clear();
            }
        } else if between.iter().any(|edges| edges.len() > 1) {
            return Ok(JoinTopology::RefusedAcrossEdges);
        }
        let on_edge = [between[0].len() == 1, between[1].len() == 1];
        // lFirst = (abOnEdge[0] || !abOnEdge[1]) ? 0 : 1
        let first = usize::from(!on_edge[0] && on_edge[1]);
        let other = 1 - first;

        let mut fs: Vec<&FilletSolver> = Vec::new();
        let mut geoms = Vec::new();
        for ii in 0..blends {
            let solver = live(solvers, solver_eu_solvers.get(ii).copied().flatten())?;
            geoms.push(solver.first_fillet_geom()?);
            fs.push(solver);
        }
        let first_side = side.get(first).copied().unwrap_or_default();
        if let Some((blend, builds, kind)) =
            unequal_blends(target, solvers, &solver_eu_solvers, first_side, cliff_join)?
        {
            return Ok(JoinTopology::RefusedUnequalOffsets {
                blend,
                builds,
                kind,
            });
        }
        // the section's ends
        let mut ends = [None, None];
        for (ee, &s) in [first, other].iter().enumerate() {
            let fv = new_corner_vertex(k, pseudo, corner)?;
            if on_edge.get(s).copied().unwrap_or(false) {
                let side_eu = find_side_edgeuse(
                    target,
                    *solver_eus.first().ok_or_else(|| {
                        defect(
                            OP,
                            "the Join has no blended edge use, though it was checked above to \
                             have one or two",
                        )
                    })?,
                    *between.get(s).and_then(|b| b.first()).ok_or_else(|| {
                        defect(
                            OP,
                            "the side of the Join on an edge has no edge between the blends, \
                             though it is on an edge where it has exactly one",
                        )
                    })?,
                )?
                .ok_or_else(|| shape_not_built(OP, CornerShape::SideEdgeNotBesideBlend))?;
                let v = pseudo.fillet_vertex_mut(fv)?;
                v.set_point_class(PointClassType::Edgeuse, Some(EntityRef::Edgeuse(side_eu)));
                v.set_fillet_vertex_type(FilletVertexType::RailXEdgeuse);
            } else {
                let face = side
                    .get(s)
                    .and_then(|f| f.first())
                    .copied()
                    .ok_or_else(|| defect(OP, SIDE_INDEX))?;
                // (solidcore-native) the face both blends roll on there, beyond a rolled-over edge
                let face = rolled.get(s).copied().flatten().or(face);
                let v = pseudo.fillet_vertex_mut(fv)?;
                v.set_point_class(PointClassType::Face, face.map(EntityRef::Face));
                v.set_fillet_vertex_type(if ee == 0 {
                    FilletVertexType::JoinSection
                } else {
                    FilletVertexType::Mate
                });
            }
            self.vertices.push(fv);
            if let Some(slot) = ends.get_mut(ee) {
                *slot = Some(fv);
            }
        }
        let [Some(first_fv), Some(other_fv)] = ends else {
            return Err(defect(
                OP,
                "the Join's section ends were not made, though the loop above makes one for \
                 each of its two sides",
            ));
        };
        if !on_edge.get(other).copied().unwrap_or(false) {
            pseudo
                .fillet_vertex_mut(other_fv)?
                .set_mate(0, Some(first_fv));
            pseudo
                .fillet_vertex_mut(first_fv)?
                .set_mate(0, Some(other_fv));
        }

        // the shared section
        let section = pseudo.make_fillet_edge(Some(first_fv), Some(other_fv), Some(corner))?;
        pseudo
            .fillet_edge_mut(section)?
            .set_fillet_edge_type(FilletEdgeType::CrossSection);
        let prim = need(
            eu1::primary_edgeuse(pseudo.brep(), section)?,
            "fillet edge without primary edge use",
        )?;
        let mate = need(
            eu1::mate(pseudo.brep(), prim)?,
            "fillet edge use without mate",
        )?;
        let first_geom = *geoms.first().ok_or_else(|| defect(OP, PER_BLEND))?;
        let last_geom = *geoms.last().ok_or_else(|| defect(OP, PER_BLEND))?;
        pseudo.set_edgeuse_fillet_geom(prim, Some(first_geom))?;
        pseudo.set_edgeuse_fillet_geom(mate, Some(last_geom))?;
        self.edges.push(section);

        // the rails' ends
        for ii in 0..blends {
            let solver = *fs.get(ii).ok_or_else(|| defect(OP, PER_BLEND))?;
            let geom = *geoms.get(ii).ok_or_else(|| defect(OP, PER_BLEND))?;
            let face = side
                .get(first)
                .and_then(|f| f.get(ii))
                .copied()
                .ok_or_else(|| defect(OP, SIDE_INDEX))?;
            // 0 or 1: the rail whose first edge use is on the face, else the other
            let rail = solver.find_index_of_rail_on_face(target, face)?;
            let other_rail = 1 - rail;
            let ends: &[(usize, _, OrientType)] = if blends == 1 {
                &[
                    (rail, first_fv, OrientType::Same),
                    (rail, first_fv, OrientType::Opposite),
                    (other_rail, other_fv, OrientType::Same),
                    (other_rail, other_fv, OrientType::Opposite),
                ]
            } else {
                &[
                    (rail, first_fv, OrientType::Unknown),
                    (other_rail, other_fv, OrientType::Unknown),
                ]
            };
            for &(r, fv, orient) in ends {
                pseudo.make_rail_edgeuse_on(
                    geom,
                    r,
                    Some(fv),
                    orient,
                    None,
                    lookup(corner, self),
                    solver,
                    target,
                )?;
            }
        }

        self.blending = true;
        self.g1_case = true;
        Ok(JoinTopology::Built)
    }

    /// Not the fork's (#17224): why this Join's section was not built where its end lies on
    /// a side edge (`SM_FV_RAIL_X_EDGEUSE`), asked only after its vertex geometry failed. A
    /// side whose one edge between the blends runs from the corner's vertex to a vertex of
    /// the blends' other face, where the first blend is a constant round: the ball touching
    /// that edge shrinks to nothing at both its ends (on both faces), and past them the
    /// edge's line leaves its face across the other face, so the round's contact line
    /// crosses the edge between its ends or nowhere. Where the largest ball touching the edge
    /// (`largest_ball_on_side_edge`) is smaller than the round, the contact line never
    /// reaches it: `Some` of that radius, the smaller where both sides are judged (the size
    /// both build at). An edge running on to another face (a rim's seam down a wall) is not
    /// judged: its contact line may cross the edge's line past its far end
    /// (`calc_rail_int_edgeuse`'s run-on), as a rim round past its wall does; nor one split
    /// at its far end only by a seam, which the rail's search continues across
    /// (`side_edgeuse_across_seam_vertex`). `None` where nothing is judged or no side refuses:
    /// the failure stands as it is.
    ///
    /// Since it is asked only on the failure path, a Join that builds is never refused by it.
    ///
    /// # Errors
    /// Those of the topology queries and of `largest_ball_on_side_edge`.
    pub fn join_rail_misses_side_edge(
        &self,
        k: &Kernel,
        target: &mut Brep,
        solvers: &[Option<SolverObject>],
        solver_eu_solvers: &[Option<FilletSolverId>],
    ) -> Result<Option<f64>> {
        let (Some(solver_eus), Some(all_eus)) =
            (self.solver_eus.as_deref(), self.all_eus.as_deref())
        else {
            return Ok(None);
        };
        let (2, Some(&Some(id)), Some(&eu1_)) = (
            solver_eus.len(),
            solver_eu_solvers.first(),
            solver_eus.first(),
        ) else {
            return Ok(None);
        };
        let Some(SolverObject::ConstantRadius(object)) =
            solvers.get(id.0 as usize).and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let (side, mut between) = join_sides(target, solver_eus, all_eus)?;
        rolled_sides(target, solvers, solver_eu_solvers, &side, &mut between)?;
        if between.iter().any(|b| b.len() > 1) {
            return Ok(None);
        }
        let mut fit: Option<f64> = None;
        for s in 0..2 {
            let Some(&[only]) = between.get(s).map(Vec::as_slice) else {
                continue;
            };
            let Some(other_face) = side.get(1 - s).and_then(|f| f.first()).copied().flatten()
            else {
                continue;
            };
            let Some(far) = far_end_on_face(target, only, self.vertex, other_face)? else {
                continue;
            };
            let Some(side_eu) = find_side_edgeuse(target, eu1_, only)? else {
                continue;
            };
            if crate::blend::blend_corner::end::side_edgeuse_across_seam_vertex(
                k,
                target,
                side_eu,
                far,
                crate::math::rad_to_deg(self.tangency_tol_radians),
            )?
            .is_some()
            {
                continue;
            }
            if let Some(builds) =
                largest_ball_on_side_edge(k, target, &object.solver, side_eu, other_face)?
            {
                fit = Some(fit.map_or(builds, |f| f.min(builds)));
            }
        }
        Ok(fit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #21940: the point of the seam arc farthest from the plane, for a sphere of radius 1
    /// about the origin and the plane y = 0. Turned 0.1 about x, the seam from the rim's vertex
    /// (1, 0, 0) to the pole (0, -sin 0.1, cos 0.1) rises all the way: the pole is the farthest.
    /// An arc from (1, 0, 0) to (-cos 0.1, -sin 0.1, 0), as a seam running over the sphere's
    /// top, passes (0, -1, 0), the farthest, inside the arc, at distance 1.
    #[test]
    fn the_seam_s_farthest_point_from_the_plane_is_in_closed_form() {
        use crate::math::{Point3, Vec3};
        let (c, n, o) = (
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        );
        let a = 0.1_f64;
        let pole = Point3::new(0.0, -det::sin(a), det::cos(a));
        let p = seam_point_farthest_from_plane(c, 1.0, Point3::new(1.0, 0.0, 0.0), pole, o, n)
            .expect("the arc is defined");
        assert!(p.distance(pole) < 1e-12, "{p:?}");
        // a seam over the top: from (1, 0, 0) through (0, -1, 0)... to a pole near (-1, 0, 0)
        let far = Point3::new(-det::cos(a), -det::sin(a), 0.0);
        let p = seam_point_farthest_from_plane(c, 1.0, Point3::new(1.0, 0.0, 0.0), far, o, n)
            .expect("the arc is defined");
        assert!((n.dot(p - o).abs() - 1.0).abs() < 1e-12, "{p:?}");
        assert!(p.distance(Point3::new(0.0, -1.0, 0.0)) < 1e-12, "{p:?}");
    }
    use crate::blend::fillet_standard_solver::p1::ConstantDistanceFs;

    #[test]
    fn cliff_join_requires_equal_chord_lengths() -> Result<()> {
        let target = Brep::default();
        let ids = [Some(FilletSolverId(0)), Some(FilletSolverId(1))];
        let chord = |distance| {
            Some(SolverObject::ConstantDistance(Box::new(
                ConstantDistanceFs {
                    solver: FilletSolver::new(1e-7, 1e-6, 1e-6),
                    distance,
                },
            )))
        };
        let equal = [chord(1.0), chord(1.0)];
        assert_eq!(
            unequal_blends(&target, &equal, &ids, [None; 2], true)?,
            None
        );
        let unequal = [chord(1.0), chord(2.0)];
        assert_eq!(
            unequal_blends(&target, &unequal, &ids, [None; 2], true)?,
            Some((1, 1.0, FitKind::Chord))
        );
        assert_eq!(
            unequal_blends(&target, &unequal, &ids, [None; 2], false)?,
            None
        );
        Ok(())
    }
}
