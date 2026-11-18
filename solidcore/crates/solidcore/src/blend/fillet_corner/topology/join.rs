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

use super::{find_side_edgeuse, need, new_corner_vertex, radial};
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_executive::session::SolverObject;
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletEdgeType, FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
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
        /// The blend with the larger offset, 0 or 1 in the corner's order.
        blend: usize,
        /// The other blend's offset there, which `blend` builds at.
        builds: f64,
    },
}

/// The live solver `id` (`NULL`: `SM_ERR_NULL_POINTER`).
fn live(solvers: &[Option<SolverObject>], id: Option<FilletSolverId>) -> Result<&FilletSolver> {
    let id = id.ok_or_else(|| SolidError::blend_missing(OP, "the blend's solver"))?;
    solvers
        .get(id.0 as usize)
        .and_then(Option::as_ref)
        .map(SolverObject::base)
        .ok_or_else(|| invalid("corner solver is not live"))
}

/// Whether two blends at a Join are constant and differ: for each of the two faces of side
/// `faces` first, its offset, and where the blends' offsets on a side differ by more than a
/// loop's gap closes ([`LOOP_GAP_ZONES`] of the body's tolerance), the blend with the larger
/// and the other's offset. Each blend's rails are on its own offsets, which
/// `SM_FE_CROSS_SECTION` cannot end both of. `None` for anything else: other solvers (a
/// law's rails are the law's to meet), or one closed blend.
fn unequal_blends(
    target: &Brep,
    solvers: &[Option<SolverObject>],
    ids: &[Option<FilletSolverId>],
    faces: [Option<FaceId>; 2],
) -> Result<Option<(usize, f64)>> {
    let mut offsets = Vec::new();
    for (&id, face) in ids.iter().zip(faces) {
        let object = id
            .and_then(|id| solvers.get(id.0 as usize))
            .and_then(Option::as_ref);
        let Some(SolverObject::ConstantRadius(object)) = object else {
            return Ok(None);
        };
        let solver = &object.solver;
        let rail = solver.find_index_of_rail_on_face(target, face)?;
        let offset = |r: usize| -> Result<f64> {
            let slot = solver.surface(r)?;
            let slot = slot.ok_or_else(|| SolidError::blend_missing(OP, "the solver's surface"))?;
            Ok(slot.surface.surface.offset_distance().abs())
        };
        offsets.push([offset(rail)?, offset(1 - rail.min(1))?]);
    }
    let [a, b] = offsets.as_slice() else {
        return Ok(None);
    };
    let gap = LOOP_GAP_ZONES * target.tolerance();
    for (&x, &y) in a.iter().zip(b) {
        if (x - y).abs() > gap {
            return Ok(Some(if x > y { (0, y) } else { (1, x) }));
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
                return Err(SolidError::blend_failed(
                    OP,
                    "a ring edge is on neither face of the closed blend",
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
        return Err(SolidError::blend_failed(
            OP,
            "a blended edge use is not in the corner's ring",
        ));
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
        return Err(SolidError::blend_failed(
            OP,
            "neither side has an edge between the blends",
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
        let known_face =
            face_by_two_edgeuses(target, Some(eu), Some(nb), None)?.ok_or_else(|| {
                SolidError::blend_missing(OP, "the face a blend shares with its neighbour")
            })?;
        let faces = faces_of(target, eu)?;
        if Some(known_face) != faces[0] && Some(known_face) != faces[1] {
            return Err(SolidError::blend_failed(
                OP,
                "the face shared with the neighbour is not one of the blend's faces",
            ));
        }
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
/// `SM_ERR_NULL_POINTER` without a solver; those of the topology queries.
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
    /// along it, which the classification checks, or a face's seam runs from the vertex)
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
    /// `SM_ERR` for other than one or two blended edge uses, an edge use not in the ring,
    /// a ring edge on neither face of a closed blend, a face not shared, or a rail index
    /// past 1; `SM_ERR_NULL_POINTER` without a solver, a shared face or a side edge use;
    /// invalid topology; those of the rail lookups and pseudo-topology operations.
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
            return Err(SolidError::blend_failed(
                OP,
                "the corner has other than one or two blended edge uses",
            ));
        }
        let (side, mut between) = join_sides(target, &solver_eus, &all_eus)?;
        let rolled = rolled_sides(target, solvers, &solver_eu_solvers, &side, &mut between)?;
        // the section's ends: on a side with an edge between the blends (where they change
        // face, or cross a face's seam) where the rails cross it, else (the first side)
        // through the vertex
        if between.iter().any(|b| b.len() > 1) {
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
        if let Some((blend, builds)) =
            unequal_blends(target, solvers, &solver_eu_solvers, first_side)?
        {
            return Ok(JoinTopology::RefusedUnequalOffsets { blend, builds });
        }
        // the section's ends
        let mut ends = [None, None];
        for (ee, &s) in [first, other].iter().enumerate() {
            let fv = new_corner_vertex(k, pseudo, corner)?;
            if on_edge.get(s).copied().unwrap_or(false) {
                let side_eu = find_side_edgeuse(
                    target,
                    *solver_eus.first().ok_or_else(|| {
                        SolidError::blend_failed(OP, "the corner has no blended edge use")
                    })?,
                    *between.get(s).and_then(|b| b.first()).ok_or_else(|| {
                        SolidError::blend_failed(OP, "the side has no edge between the blends")
                    })?,
                )?
                .ok_or_else(|| SolidError::blend_missing(OP, "the side edge use"))?;
                let v = pseudo.fillet_vertex_mut(fv)?;
                v.set_point_class(PointClassType::Edgeuse, Some(EntityRef::Edgeuse(side_eu)));
                v.set_fillet_vertex_type(FilletVertexType::RailXEdgeuse);
            } else {
                let face = side
                    .get(s)
                    .and_then(|f| f.first())
                    .copied()
                    .ok_or_else(|| SolidError::blend_failed(OP, "the side has no face"))?;
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
            return Err(SolidError::blend_failed(
                OP,
                "the section's ends were not made",
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
        let first_geom = *geoms
            .first()
            .ok_or_else(|| SolidError::blend_failed(OP, "the corner has no blend geometry"))?;
        let last_geom = *geoms
            .last()
            .ok_or_else(|| SolidError::blend_failed(OP, "the corner has no blend geometry"))?;
        pseudo.set_edgeuse_fillet_geom(prim, Some(first_geom))?;
        pseudo.set_edgeuse_fillet_geom(mate, Some(last_geom))?;
        self.edges.push(section);

        // the rails' ends
        for ii in 0..blends {
            let solver = *fs
                .get(ii)
                .ok_or_else(|| SolidError::blend_failed(OP, "the blend has no solver"))?;
            let geom = *geoms
                .get(ii)
                .ok_or_else(|| SolidError::blend_failed(OP, "the blend has no geometry"))?;
            let face = side
                .get(first)
                .and_then(|f| f.get(ii))
                .copied()
                .ok_or_else(|| {
                    SolidError::blend_failed(OP, "the blend has no face on the first side")
                })?;
            let rail = solver.find_index_of_rail_on_face(target, face)?;
            if rail > 1 {
                return Err(SolidError::blend_failed(
                    OP,
                    "the blend has no rail on its face on the first side",
                ));
            }
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
