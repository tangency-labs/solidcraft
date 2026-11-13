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
//!
//! Deviations: the caller resolves `GetFilletSolverOfEdgeuse` of the blended edge uses
//! ([`JoinTopologyEnv::solver_eu_solvers`]) and notes the refusal's error
//! ([`JoinTopology::RefusedAcrossEdges`]); blends on unequal offsets are refused (not the
//! fork's, #2905: [`JoinTopology::RefusedUnequalOffsets`]). A missing solver the fork dereferences
//! through `NER` is `SM_ERR_NULL_POINTER`; other missing topology is an input error. The
//! walks round the vertex are bounded by its edge uses (WR-697).

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
/// law's rails are the law's to meet), without `fixes.fillet_round_as_definition` (a blend
/// not by its definition bends its rails to the section), or one closed blend.
fn unequal_blends(
    k: &Kernel,
    target: &Brep,
    solvers: &[Option<SolverObject>],
    ids: &[Option<FilletSolverId>],
    faces: [Option<FaceId>; 2],
) -> Result<Option<(usize, f64)>> {
    if !k.fixes.fillet_round_as_definition {
        return Ok(None);
    }
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
        let (side, between) = join_sides(target, &solver_eus, &all_eus)?;
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
            unequal_blends(k, target, solvers, &solver_eu_solvers, first_side)?
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
}
