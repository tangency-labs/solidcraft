//! Topology at the end of one filleted edge.
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp` (44–119, 443–506, 5990–6443).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletCorner::make_nx1_corner_topology`] | `SmFilletNx1Corner::MakeCornerTopology` |
//! | `find_side_edgeuse`, `find_closed_side_edgeuse` | `sm_FindSideEdgeuse`, `sm_FindClosedSideEdgeuse` |
//! | `same_convexity_edges` | `same_convexity_edges` |
//!
//! Deviations: the caller lends the actual target, pseudo-B-rep and solver.
//! Missing topology that native dereferences is an input error. Mixed-convexity
//! topology stops at the unported surface-extension-distance helper, after its
//! native vertex/rail prefix. There is no recursion.

use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::{FilletCorner, FilletCornerKind};
use crate::blend::fillet_geom::p1::{
    FilletBrep, FilletCornerId, FilletEdgeType, FilletVertex, FilletVertexType,
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
    let vertex = pseudo.brep_mut().add_vertex_record(Vertex {
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
                crate::nurbs::b_spline_surface_n_lib::p2::OrientType::Same,
                None,
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
