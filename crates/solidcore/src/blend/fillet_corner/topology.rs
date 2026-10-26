//! Topology at the end of one filleted edge.
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp` (44–119, 443–506, 5990–6443,
//! 6487–6859).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletCorner::make_nx1_corner_topology`] | `SmFilletNx1Corner::MakeCornerTopology` |
//! | [`FilletCorner::make_nx2_corner_topology`] | `SmFilletNx2Corner::MakeCornerTopology` |
//! | `find_side_edgeuse`, `find_closed_side_edgeuse` | `sm_FindSideEdgeuse`, `sm_FindClosedSideEdgeuse` |
//! | `same_convexity_edges` | `same_convexity_edges` |
//!
//! Deviations: the caller lends the actual target, pseudo-B-rep and solver.
//! Missing topology that native dereferences is an input error. Mixed-convexity
//! topology stops at the unported surface-extension-distance helper, after its
//! native vertex/rail prefix. The mixed-convexity 4x2 corner stops at the unported
//! `Make4x2ConcaveCornerTopology`. The Nx2 corner's two solvers are resolved by the
//! caller (upstream looks them up after the 4x2 checks; the lookup has no effects).
//! An unset `sm_TestG1Continuity` angle (upstream reads it uninitialised) is an input
//! error where the mixed-convexity tests read it. There is no recursion.

use crate::blend::fillet_corner::p1::{
    check_face_complimentary_corner_concavity, face_by_two_edgeuses, test_g1_continuity,
};
use crate::blend::fillet_corner::p3::{FilletCorner, FilletCornerKind};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletEdgeType, FilletVertex, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::topology::edge;
use crate::topology::edgeuse::{p1 as eu1, p2 as eu2};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, Vertex, VertexId};

const OP: &str = "blend::fillet_corner::topology::make_nx1_corner_topology";
const OP_NX2: &str = "blend::fillet_corner::topology::make_nx2_corner_topology";

/// Actual objects used by `SmFilletNx2Corner::MakeCornerTopology`.
#[derive(Debug)]
pub struct Nx2TopologyEnv<'a> {
    /// The executive's kernel.
    pub k: &'a Kernel,
    /// The target whose edge uses the corner stores.
    pub target: &'a mut Brep,
    /// The executive's actual pseudo-B-rep.
    pub pseudo: &'a mut FilletBrep,
    /// This corner's identity in the executive's corner store.
    pub corner: FilletCornerId,
    /// The loaded solvers of the corner's first and second filleted edge use
    /// (`GetFilletSolverOfEdgeuse`).
    pub solvers: [&'a FilletSolver; 2],
}

/// Actual objects used by `SmFilletNx1Corner::MakeCornerTopology`.
#[derive(Debug)]
pub struct Nx1TopologyEnv<'a> {
    /// The executive's kernel.
    pub k: &'a Kernel,
    /// The target whose edge uses the corner stores.
    pub target: &'a mut Brep,
    /// The executive's actual pseudo-B-rep.
    pub pseudo: &'a mut FilletBrep,
    /// This corner's identity in the executive's corner store.
    pub corner: FilletCornerId,
    /// The loaded solver of the corner's filleted edge use.
    pub solver: &'a FilletSolver,
    /// The executive's live global-merge switch.
    pub do_global_merge: &'a mut bool,
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn need<T>(value: Option<T>, reason: &'static str) -> Result<T> {
    value.ok_or_else(|| invalid(reason))
}

fn edge_of(target: &Brep, eu: EdgeuseId) -> Result<EdgeId> {
    need(eu1::edge(target, eu)?, "edge use without edge")
}

fn radial(target: &Brep, eu: EdgeuseId) -> Result<EdgeuseId> {
    need(eu1::radial(target, eu)?, "edge use without radial")
}

fn vertex_of(target: &Brep, eu: EdgeuseId) -> Result<Option<VertexId>> {
    let vu = need(eu1::vertexuse(target, eu)?, "edge use without vertex use")?;
    Ok(target.vertexuse(vu)?.vertex)
}

fn find_side_edgeuse(
    target: &Brep,
    filleted: EdgeuseId,
    side: EdgeuseId,
) -> Result<Option<EdgeuseId>> {
    let filleted_edge = eu1::edge(target, filleted)?;
    let cw = need(
        eu1::cw_edgeuse(target, side)?,
        "side edge use without CW neighbour",
    )?;
    if filleted_edge == eu1::edge(target, cw)? {
        return Ok(Some(side));
    }
    let side = radial(target, side)?;
    let ccw = need(
        eu1::ccw_edgeuse(target, side)?,
        "side radial without CCW neighbour",
    )?;
    Ok((filleted_edge == eu1::edge(target, ccw)?).then_some(side))
}

fn find_closed_side_edgeuse(
    target: &Brep,
    solver: &FilletSolver,
    side: EdgeuseId,
    rail: usize,
) -> Result<EdgeuseId> {
    let filleted = need(solver.edgeuse(0)?, "solver without first edge use")?;
    let same_vertex = vertex_of(target, side)? == vertex_of(target, filleted)?;
    let neighbour = if same_vertex {
        need(
            eu1::cw_edgeuse(target, filleted)?,
            "filleted edge use without CW neighbour",
        )?
    } else {
        need(
            eu1::ccw_edgeuse(target, filleted)?,
            "filleted edge use without CCW neighbour",
        )?
    };
    if (rail == 0) == same_vertex {
        need(eu1::mate(target, neighbour)?, "side edge use without mate")
    } else {
        Ok(neighbour)
    }
}

fn same_convexity_edges(
    k: &Kernel,
    target: &mut Brep,
    first: EdgeuseId,
    second: EdgeuseId,
    vertex: VertexId,
) -> Result<bool> {
    let concave = if eu2::is_convex_radial_sector(k, target, first, 5, false, 0.0)? {
        if edge::is_manifold(target, edge_of(target, first)?)? {
            let mate = need(eu1::mate(target, first)?, "filleted edge use without mate")?;
            Some(radial(target, mate)?)
        } else {
            None
        }
    } else {
        Some(first)
    };
    // Native SER(SM_ERR) returns a nonzero status through this SmBoolean.
    let Some(concave) = concave else {
        return Ok(true);
    };
    let second_edge = eu1::edge(target, second)?;
    let mut side = need(
        eu1::corner_mate_edgeuse(target, concave, Some(vertex))?,
        "concave edge use without corner mate",
    )?;
    if eu1::edge(target, side)? != second_edge {
        side = need(
            eu1::corner_mate_edgeuse(target, radial(target, concave)?, Some(vertex))?,
            "concave radial without corner mate",
        )?;
    }
    Ok(!eu2::is_convex_radial_sector(
        k, target, side, 5, false, 0.0,
    )?)
}

fn is_fillet_side_face(target: &Brep, filleted: EdgeuseId, face: Option<FaceId>) -> Result<bool> {
    if face == eu1::face(target, filleted)? {
        return Ok(true);
    }
    Ok(face == eu1::face(target, radial(target, filleted)?)?)
}

fn new_corner_vertex(
    k: &Kernel,
    pseudo: &mut FilletBrep,
    corner: FilletCornerId,
) -> Result<VertexId> {
    let vertex = pseudo.brep_mut().add_unlinked_vertex_record(Vertex {
        point: Point3::ZERO,
        tolerance: 0.0,
        ..Vertex::default()
    })?;
    pseudo.insert_fillet_vertex(vertex, FilletVertex::new(k, Some(corner)));
    Ok(vertex)
}

impl FilletCorner {
    /// Construct the rails and end rib of an actual `NByOne` corner.
    ///
    /// Allocations and corner-list insertions occur in native order and remain
    /// visible if a later operation fails. Vertex geometry is computed later.
    ///
    /// # Errors
    /// Invalid topology or a different corner kind; those of the actual solver,
    /// convexity and pseudo-topology operations. Mixed-convexity extension remains
    /// `Unported` after creating its native rail/vertex prefix.
    pub fn make_nx1_corner_topology(&mut self, env: Nx1TopologyEnv<'_>) -> Result<()> {
        let Nx1TopologyEnv {
            k,
            target,
            pseudo,
            corner,
            solver,
            do_global_merge,
        } = env;
        if self.kind != FilletCornerKind::NByOne {
            return Err(invalid("requires an NByOne corner"));
        }
        let filleted = need(
            self.solver_eus
                .as_deref()
                .and_then(|eus| eus.first())
                .copied(),
            "corner without filleted edge use",
        )?;
        let geometry = solver.first_fillet_geom()?;
        let all = need(self.all_eus.as_deref(), "corner without outward edge uses")?;
        let total = all.len();
        let index = need(
            all.iter().position(|&eu| eu == filleted),
            "filleted edge use not in corner",
        )?;
        let mut sides = [
            *need(
                all.get((index + total - 1) % total),
                "preceding side edge use",
            )?,
            *need(all.get((index + 1) % total), "following side edge use")?,
        ];
        let [side0, side1] = sides;
        let closed_side = side0 == side1;
        let vertex = need(self.vertex, "corner without vertex")?;
        let same = [
            same_convexity_edges(k, target, filleted, side0, vertex)?,
            same_convexity_edges(k, target, filleted, side1, vertex)?,
        ];
        let [same0, same1] = same;
        let mixed = !same0 || !same1;
        let mut end_face = None;

        for rail in 0..2 {
            let side = *need(sides.get(rail), "side index")?;
            let fv = new_corner_vertex(k, pseudo, corner)?;
            self.vertices.push(fv);
            let (side_use, solver_rail) = if closed_side {
                (find_closed_side_edgeuse(target, solver, side, rail)?, rail)
            } else {
                let found = need(
                    find_side_edgeuse(target, filleted, side)?,
                    "no side edge use intersects rail",
                )?;
                (
                    found,
                    solver.find_index_of_rail_x_side_edgeuse(target, found)?,
                )
            };
            pseudo
                .fillet_vertex_mut(fv)?
                .set_point_class(PointClassType::Edgeuse, Some(EntityRef::Edgeuse(side_use)));
            pseudo.make_rail_edgeuse_on(
                geometry,
                solver_rail,
                Some(fv),
                crate::nurbs::b_spline_surface_n_lib::p2::OrientType::Unknown,
                None,
                CornerLookup {
                    this: Some((Some(corner), self)),
                    ..Default::default()
                },
                solver,
                target,
            )?;

            let face = eu1::face(target, side)?;
            if mixed && *need(same.get(rail), "convexity index")? {
                pseudo
                    .fillet_vertex_mut(fv)?
                    .set_fillet_vertex_type(FilletVertexType::RailXExtendedEdgeuse);
                let other = *need(sides.get(1 - rail), "other side index")?;
                end_face = face_by_two_edgeuses(target, Some(side), Some(other), None)?;
                if !is_fillet_side_face(target, filleted, face)? {
                    *need(sides.get_mut(rail), "side index")? = radial(target, side)?;
                }
            } else {
                pseudo
                    .fillet_vertex_mut(fv)?
                    .set_fillet_vertex_type(FilletVertexType::RailXEdgeuse);
                if is_fillet_side_face(target, filleted, face)? {
                    *need(sides.get_mut(rail), "side index")? = radial(target, side)?;
                }
            }
        }
        if !mixed && total > 3 {
            *do_global_merge = true;
            return Ok(());
        }
        if mixed {
            // UNPORTED(blend.fillet_corner_p1): sm_FindSurfaceExtensionDistAtCorner
            return Err(SolidError::unported(
                "sm_FindSurfaceExtensionDistAtCorner",
                "blend.fillet_corner_p1",
            ));
        }

        let start = self.vertices.first().copied();
        let end = self.vertices.get(1).copied();
        let rib = pseudo.make_fillet_edge(start, end, Some(corner))?;
        pseudo
            .fillet_edge_mut(rib)?
            .set_fillet_edge_type(FilletEdgeType::FilletXSideFace);
        let primary = need(
            eu1::primary_edgeuse(pseudo.brep(), rib)?,
            "end rib without primary edge use",
        )?;
        pseudo.set_edgeuse_fillet_geom(primary, Some(geometry))?;
        self.edges.push(rib);
        if end_face.is_none() {
            let [first, second] = sides;
            end_face = face_by_two_edgeuses(target, Some(first), Some(second), None)?;
            if end_face.is_none() {
                return Err(SolidError::sm_err(OP));
            }
        }
        pseudo.fillet_edge_mut(rib)?.set_original_face(end_face);
        Ok(())
    }
}

impl FilletCorner {
    /// `SmFilletNx2Corner::MakeCornerTopology`: the fillet vertices where the rails of
    /// the corner's two fillets cross its side edges, the face vertex (rail/rail
    /// intersection of a bevel, mate of a tangent corner) or the two mates of a
    /// mixed-convexity 3x2 corner, their rail edge uses and the corner's cross edge(s).
    ///
    /// Allocations and corner-list insertions occur in native order and remain visible
    /// if a later operation fails. Vertex geometry is computed later.
    ///
    /// # Errors
    /// `SM_ERR` for more than four edge uses around the corner, filleted edge uses not
    /// among them or adjacent in a 4x2 corner, or other than two vertices before the
    /// cross edge; [`SolidError::InvalidInput`] for a different corner kind or missing
    /// topology upstream dereferences; those of the convexity, continuity, solver and
    /// pseudo-topology operations. A mixed-convexity 4x2 corner is `Unported`.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn make_nx2_corner_topology(&mut self, env: Nx2TopologyEnv<'_>) -> Result<()> {
        fn lookup(corner: FilletCornerId, this: &FilletCorner) -> CornerLookup<'_> {
            CornerLookup {
                this: Some((Some(corner), this)),
                ..Default::default()
            }
        }
        let Nx2TopologyEnv {
            k,
            target,
            pseudo,
            corner,
            solvers: [fil_solver1, fil_solver2],
        } = env;
        let invalid = |reason: &'static str| SolidError::invalid_input(OP_NX2, reason);
        if self.kind != FilletCornerKind::NByTwo {
            return Err(invalid("requires an NByTwo corner"));
        }
        let all = self
            .all_eus
            .clone()
            .ok_or_else(|| invalid("corner without outward edge uses"))?;
        let total = all.len();

        // no work - unhandled case - Currently we handle only 3x2 and 4x2
        if total > 4 {
            return Err(SolidError::sm_err(OP_NX2));
        }

        // Nx2 corner, get the two filleted edgeuses from the base brep
        let (base_eu1, base_eu2) = match self.solver_eus.as_deref() {
            Some([first, second, ..]) => (*first, *second),
            _ => return Err(invalid("corner without two filleted edge uses")),
        };
        let vertex = self
            .vertex
            .ok_or_else(|| invalid("corner without vertex"))?;

        // for 4 EU case
        if total == 4 {
            // check state: to be filleted edgeuses are on list of outward edgeuses
            let (Some(found1), Some(found2)) = (
                all.iter().position(|&eu| eu == base_eu1),
                all.iter().position(|&eu| eu == base_eu2),
            ) else {
                return Err(SolidError::sm_err(OP_NX2));
            };
            // the two filleted edgeuses must not be adjacent to each other
            if (found1 + found2) % 2 != 0 {
                return Err(SolidError::sm_err(OP_NX2));
            }
            // Check the non-filleted (side) edges for mixed convexity
            for &side in &all {
                if side == base_eu1 || side == base_eu2 {
                    continue;
                }
                if check_face_complimentary_corner_concavity(
                    k,
                    target,
                    vertex,
                    base_eu1,
                    base_eu2,
                    Some(side),
                )? {
                    // UNPORTED(blend.fillet_executive_p1): SmFilletNx2Corner::Make4x2ConcaveCornerTopology
                    return Err(SolidError::unported(
                        "SmFilletNx2Corner::Make4x2ConcaveCornerTopology",
                        "blend.fillet_executive_p1",
                    ));
                }
            }
            self.bevel = true;
        }

        // Determine if two edges are tangent at corner vertex.
        let tangency_tol_deg = crate::math::rad_to_deg(self.tangency_tol_radians);
        let (is_tangent_case, angle_deg) = test_g1_continuity(
            k,
            target,
            Some(vertex),
            Some(edge_of(target, base_eu1)?),
            Some(edge_of(target, base_eu2)?),
            tangency_tol_deg,
        )?;
        // Store this value for later use.
        self.g1_case = is_tangent_case;

        let mixed_convexity =
            check_face_complimentary_corner_concavity(k, target, vertex, base_eu1, base_eu2, None)?;
        let angle = || angle_deg.ok_or_else(|| invalid("edge angle at the corner is unset"));

        // Get Solvers and FilletGeoms for the two filleted edges at this vertex
        let geom1 = fil_solver1.first_fillet_geom()?;
        let geom2 = fil_solver2.first_fillet_geom()?;
        let unknown = crate::nurbs::b_spline_surface_n_lib::p2::OrientType::Unknown;

        // For the non-filleted edge(s), we expect the rails to hit it (them).
        // Create a (fillet) vertex there, along with two rail edgeuses.
        for &curr in &all {
            if curr == base_eu1 || curr == base_eu2 {
                continue;
            }
            // This edgeuse is in between two filletted edgeuses
            let side = find_side_edgeuse(target, base_eu1, curr)?;
            let fv = new_corner_vertex(k, pseudo, corner)?;
            {
                let vertex = pseudo.fillet_vertex_mut(fv)?;
                vertex.set_point_class(PointClassType::Edgeuse, side.map(EntityRef::Edgeuse));
                vertex.set_fillet_vertex_type(FilletVertexType::RailXEdgeuse);
            }
            self.vertices.push(fv);
            let side = side.ok_or_else(|| invalid("no side edge use intersects rail"))?;
            let rail = fil_solver1.find_index_of_rail_x_side_edgeuse(target, side)?;
            let other_rail =
                fil_solver2.find_index_of_rail_x_side_edgeuse(target, radial(target, side)?)?;
            pseudo.make_rail_edgeuse_on(
                geom1,
                rail,
                Some(fv),
                unknown,
                None,
                lookup(corner, self),
                fil_solver1,
                target,
            )?;
            pseudo.make_rail_edgeuse_on(
                geom2,
                other_rail,
                Some(fv),
                unknown,
                None,
                lookup(corner, self),
                fil_solver2,
                target,
            )?;
        }

        // Create the topology for the fillet corner.
        if total == 3 {
            // Find the side EU -- the un-filleted one.
            let side = all
                .iter()
                .copied()
                .find(|&eu| eu != base_eu1 && eu != base_eu2);
            let common_face = face_by_two_edgeuses(target, Some(base_eu1), Some(base_eu2), side)?;
            let rail = fil_solver1.find_index_of_rail_on_face(target, common_face)?;
            let other_rail = fil_solver2.find_index_of_rail_on_face(target, common_face)?;
            let side_fv = *self
                .vertices
                .first()
                .ok_or_else(|| invalid("no side fillet vertex"))?;
            let face_fv = new_corner_vertex(k, pseudo, corner)?;
            pseudo
                .fillet_vertex_mut(face_fv)?
                .set_point_class(PointClassType::Face, common_face.map(EntityRef::Face));
            self.vertices.push(face_fv);

            // mixed convexity case
            if mixed_convexity && !self.bevel && angle()? > 5.0 {
                // Mixed-convexity 3x2 corner, will create a 3-sided corner face.
                self.blending = true;
                {
                    let fv = pseudo.fillet_vertex_mut(face_fv)?;
                    fv.set_fillet_vertex_type(FilletVertexType::Mate);
                    fv.set_mate(0, Some(side_fv));
                }
                pseudo
                    .fillet_vertex_mut(side_fv)?
                    .set_mate(0, Some(face_fv));
                pseudo.make_rail_edgeuse_on(
                    geom1,
                    rail,
                    Some(face_fv),
                    unknown,
                    None,
                    lookup(corner, self),
                    fil_solver1,
                    target,
                )?;

                let face_fv2 = new_corner_vertex(k, pseudo, corner)?;
                self.vertices.push(face_fv2);
                {
                    let fv = pseudo.fillet_vertex_mut(face_fv2)?;
                    fv.set_point_class(PointClassType::Face, common_face.map(EntityRef::Face));
                    fv.set_fillet_vertex_type(FilletVertexType::Mate);
                    fv.set_mate(0, Some(side_fv));
                }
                pseudo
                    .fillet_vertex_mut(side_fv)?
                    .set_mate(1, Some(face_fv2));
                pseudo.make_rail_edgeuse_on(
                    geom2,
                    other_rail,
                    Some(face_fv2),
                    unknown,
                    None,
                    lookup(corner, self),
                    fil_solver2,
                    target,
                )?;

                // a straight-across rib to end filletGeom1
                let rib = pseudo.make_fillet_edge(Some(side_fv), Some(face_fv), Some(corner))?;
                pseudo
                    .fillet_edge_mut(rib)?
                    .set_fillet_edge_type(FilletEdgeType::CrossSection);
                let primary = need(
                    eu1::primary_edgeuse(pseudo.brep(), rib)?,
                    "cross edge without primary edge use",
                )?;
                pseudo.set_edgeuse_fillet_geom(primary, Some(fil_solver1.first_fillet_geom()?))?;
                self.edges.push(rib);

                // the 3-sided face's third edge, inside the common face
                let rib = pseudo.make_fillet_edge(Some(face_fv), Some(face_fv2), Some(corner))?;
                {
                    let edge = pseudo.fillet_edge_mut(rib)?;
                    edge.set_fillet_edge_type(FilletEdgeType::RailRailInterpolation);
                    edge.set_original_face(common_face);
                }
                self.edges.push(rib);

                // a straight-across rib to end filletGeom2
                let rib = pseudo.make_fillet_edge(Some(face_fv2), Some(side_fv), Some(corner))?;
                pseudo
                    .fillet_edge_mut(rib)?
                    .set_fillet_edge_type(FilletEdgeType::CrossSection);
                let primary = need(
                    eu1::primary_edgeuse(pseudo.brep(), rib)?,
                    "cross edge without primary edge use",
                )?;
                pseudo.set_edgeuse_fillet_geom(primary, Some(fil_solver2.first_fillet_geom()?))?;
                self.edges.push(rib);

                // all done
                return Ok(());
            }

            if is_tangent_case || (mixed_convexity && angle()? <= 5.0) {
                self.blending = true;
                // The new face-vertex will just be straight across from the
                // rail-side edge intersection vertex.
                {
                    let fv = pseudo.fillet_vertex_mut(face_fv)?;
                    fv.set_fillet_vertex_type(FilletVertexType::Mate);
                    fv.set_mate(0, Some(side_fv));
                }
                pseudo
                    .fillet_vertex_mut(side_fv)?
                    .set_mate(0, Some(face_fv));
            } else {
                // The new face vertex will be the intersection of the rails.
                pseudo
                    .fillet_vertex_mut(face_fv)?
                    .set_fillet_vertex_type(FilletVertexType::RailXRail);
                self.bevel = true;
            }

            // Setup rails of fillet
            pseudo.make_rail_edgeuse_on(
                geom1,
                rail,
                Some(face_fv),
                unknown,
                None,
                lookup(corner, self),
                fil_solver1,
                target,
            )?;
            pseudo.make_rail_edgeuse_on(
                geom2,
                other_rail,
                Some(face_fv),
                unknown,
                None,
                lookup(corner, self),
                fil_solver2,
                target,
            )?;
        } else if total == 4 {
            if is_tangent_case || (mixed_convexity && angle()? <= 5.0) {
                // Two filleted edgeuses are tangent to each other
                self.blending = true;
                self.bevel = false;
            } else {
                // Regular BEVEL corner
                self.bevel = true;
                self.blending = false;
            }
        }

        // At this point, we have only one fillet (cross) edge at this corner.
        if self.vertices.len() != 2 {
            return Err(SolidError::sm_err(OP_NX2));
        }
        let cross = pseudo.make_fillet_edge(
            self.vertices.first().copied(),
            self.vertices.get(1).copied(),
            Some(corner),
        )?;
        let primary = need(
            eu1::primary_edgeuse(pseudo.brep(), cross)?,
            "cross edge without primary edge use",
        )?;
        let mate = need(
            eu1::mate(pseudo.brep(), primary)?,
            "cross edge without mate edge use",
        )?;
        pseudo.set_edgeuse_fillet_geom(primary, Some(geom1))?;
        pseudo.set_edgeuse_fillet_geom(mate, Some(geom2))?;
        let edge_type = if is_tangent_case || self.blending {
            FilletEdgeType::CrossSection
        } else {
            FilletEdgeType::FilletXFillet
        };
        pseudo
            .fillet_edge_mut(cross)?
            .set_fillet_edge_type(edge_type);
        self.edges.push(cross);

        // all done
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: FilletCornerKind, all: usize) -> Result<()> {
        let k = Kernel::default();
        let mut target = Brep::new();
        let mut pseudo = FilletBrep::new();
        let solver = FilletSolver::new(1e-5, 1e-3, 0.01);
        let mut corner = FilletCorner::new(kind, None, None, None, 1e-3, 0.1);
        let eus = (0..all)
            .map(|_| {
                target
                    .add_edgeuse_record(crate::topology::Edgeuse::default())
                    .unwrap()
            })
            .collect();
        corner.all_eus = Some(eus);
        corner.make_nx2_corner_topology(Nx2TopologyEnv {
            k: &k,
            target: &mut target,
            pseudo: &mut pseudo,
            corner: FilletCornerId(0),
            solvers: [&solver, &solver],
        })
    }

    #[test]
    fn nx2_topology_rejects_other_kinds_and_more_than_four_edge_uses() {
        assert!(matches!(
            run(FilletCornerKind::NByOne, 3),
            Err(SolidError::InvalidInput { .. })
        ));
        let err = run(FilletCornerKind::NByTwo, 5).unwrap_err();
        assert!(!matches!(err, SolidError::InvalidInput { .. }), "{err:?}");
        assert!(matches!(
            run(FilletCornerKind::NByTwo, 3),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
