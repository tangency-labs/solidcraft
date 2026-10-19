//! `SmBrep` members of unit `topology.brep_p1`: the tolerance and size-estimate
//! accessors, the topology collections, the infinite region, the region voting, the
//! blend, projection and intersection entry points, and the file-static helpers of
//! `SmBrep.cpp` the unit owns.
//!
//! Upstream: `source/SMLib/inc/SmBrep.h`, `source/SMLib/src/SmBrep.cpp`.
//!
//! | Rust                                              | Upstream                                      |
//! |---------------------------------------------------|-----------------------------------------------|
//! | [`TYPE`], [`TYPE_STRING`], [`CLASS_STRING`], [`is_kind_of`] | `SmBrep::GetType`, `GetTypeString`, `GetClassString`, `IsKindOf` |
//! | [`Brep::tolerance`] (in `topology.rs`)            | `SmBrep::GetTolerance`                        |
//! | [`Brep::this_model_size_estimate`]                | `SmBrep::GetThisModelSizeEstimate`            |
//! | [`Brep::set_this_model_size_estimate`]            | `SmBrep::SetThisModelSizeEstimate`            |
//! | [`Brep::this_large_small_size_ratio`]             | `SmBrep::GetThisLargeSmallSizeRatio`          |
//! | [`Brep::set_this_large_small_size_ratio`]         | `SmBrep::SetThisLargeSmallSizeRatio`          |
//! | [`Brep::infinite_region`] (in `topology.rs`), [`Brep::set_infinite_region`] | `SmBrep::GetInfiniteRegion`, `SetInfiniteRegion` |
//! | [`Brep::editing_enabled`], [`Brep::set_editing_enabled`] (in `topology.rs`) | `SmBrep::m_bEditingEnabled` |
//! | [`Brep::face_list`], [`Brep::edge_list`], [`Brep::vertex_list`] (+ `set_*_list`) | `m_pFaceListHead`, `m_pEdgeListHead`, `m_pVertexListHead` (list order) |
//! | [`Brep::add_deleted_surface`], [`Brep::deleted_surfaces`] | `SmBrep::AddDeletedSurface`, `m_pDeletedSurfaces` |
//! | [`num_faces`]                                     | `SmBrep::GetNumFaces`                         |
//! | [`is_live_topology_member`]                       | `SmBrep::IsLiveTopologyMember`                |
//! | [`regions`], [`faces`], [`edges`], [`vertices`]   | `SmBrep::GetRegions`, `GetFaces`, `GetEdges`, `GetVertices` |
//! | [`shells`]                                        | `SmBrep::GetShells`                           |
//! | [`surfaces`], [`curves`]                          | `SmBrep::GetSurfaces`, `GetCurves`            |
//! | [`wire_edges`], [`topological_edges`], [`topological_vertices`] | `SmBrep::GetWireEdges`, `GetTopologicalEdges`, `GetTopologicalVertices` |
//! | [`is_degenerate`]                                 | `SmBrep::IsDegenerate`                        |
//! | [`reset_infinite_region`], [`SetRegionVoidType`]  | `SmBrep::ResetInfiniteRegion`, `SmSetRegionVoidType` |
//! | [`set_region_is_void_flags_for_nested_solids`]    | `SmBrep::SetRegionIsVoidFlagsForNestedSolids` |
//! | [`heal_brep`]                                     | `SmBrep::HealBrep`                            |
//! | [`brep_intersect`]                                | `SmBrep::BrepIntersect`                       |
//! | [`create_corner_blend`]                           | `SmBrep::CreateCornerBlend`                   |
//! | [`create_edge_edge_blend`]                        | `SmBrep::CreateEdgeEdgeBlend`                 |
//! | [`create_parallel_projection_curves`], [`ParallelProjectionOptions`], [`ParallelProjectionCurves`] | `SmBrep::CreateParallelProjectionCurves` |
//! | [`update_vertex_edge_intervals`]                  | `SmBrep::UpdateVertexEdgeIntervals`           |
//! | [`LockBrepBox`]                                   | `SmLockBrepBox` (constructor, destructor)     |
//! | [`check_uv_trim_curve`]                           | `sm_CheckUVTrimCurve`                         |
//! | [`swap_face_uv`]                                  | `sm_SwapFaceUV`                               |
//! | [`mark_loops_to_delete`]                          | `sm_MarkLoopsToDelete`                        |
//! | [`rm_and_del_old_loop_vertices`]                  | `sm_RmAndDel_OldLoopVertices`                 |
//! | [`find_sector_of_face`]                           | `sm_FindSectorOfFace`                         |
//! | [`vote_for_region`], [`find_highest_vote`]        | `sm_VoteForRegion`, `sm_FindHighestVote`      |
//! | [`redistribute_seam_uv_trim_curves`]              | `sm_RedistributeSeamUVTrimCurves`             |
//! | [`add_attributes`]                                | `sm_AddAttributes`                            |
//! | [`PendingTopologyDelete`]                         | `SmPendingTopologyDelete` (`Clear`)           |
//! | [`make_vertex_split_uv_trim_curve`]               | `sm_MakeVertexSplitUVTrimCurve`               |
//! | [`ObjectMap`], [`set_object_map`], [`get_object_map`] | `sm_SetObjectMap`, `sm_GetObjectMap` (the copy maps are `topology.proto_topology_p1`'s [`CopyBrepMap`]) |
//! | [`reverse_reflection_edgeuses`]                   | `sm_ReverseReflectionEdgeuses`                |
//! | [`swap_reflection_shells`]                        | `sm_SwapReflectionShells`                     |
//! | [`find_side_of_curve`]                            | `sm_FindSideOfCurve`                          |
//!
//! Deviations:
//! - Out-params become return values; `SmTArray` outputs are `Vec`s; pointers are
//!   typed ids and `None` is `NULL`. Where upstream dereferences a null pointer the
//!   port returns [`SolidError::InvalidInput`]; `SM_ERR` returns are
//!   [`SolidError::Upstream`] with that code.
//! - The region list of the B-rep (`SmOwningTopology::m_pList`) is the region arena in
//!   traversal order rotated to [`Brep::region_list_head`]; `ResetInfiniteRegion`
//!   only moves the head. Regions not on the list (`Region::in_brep` false) are not
//!   reported by [`regions`].
//! - [`surfaces`] / [`curves`]: upstream's `GetOwner() == pFace` / `pEdge` test compares
//!   the geometry's owner with the face's / edge's
//!   [`arena_identity`](crate::topology::brep_cache::arena_identity) (the kernel's
//!   identity of arena entities; a B-rep whose geometry carries no owner reports no
//!   surfaces or curves, as upstream for unowned geometry); the attribute filter
//!   ([`GeometryAttributeFilter`]) looks the geometry up under its own keys.
//! - `SetThisModelSizeEstimate` / `SetThisLargeSmallSizeRatio` also write the B-rep's
//!   context (`m_cpContext`): the kernel's [`Context`](crate::geom::context::Context).
//! - Marks (`sm_SwapFaceUV`) are caller-owned
//!   [`Marks`](crate::topology::topology_traverser::Marks) records, as the traverser.
//! - `sm_MarkLoopsToDelete` recurses once per nesting level of the loops and branches
//!   at every other loop of a face; the port stops with [`SolidError::InvalidInput`]
//!   past [`MAX_LOOP_NESTING`] levels and past [`MAX_LOOP_MARK_CALLS`] calls in all
//!   (upstream overflows its stack or hangs on a cyclic radial walk; real loop
//!   nestings are a handful).
//! - `sm_RmAndDel_OldLoopVertices` deletes two vertex uses: the port unlinks them from
//!   their vertex and resets their records (an arena record cannot be freed).
//! - `sm_SetObjectMap` / `sm_GetObjectMap` keep an [`EntityRef`] `BTreeMap`
//!   ([`ObjectMap`]) instead of the pointer hash map (plan hazard `ptr-hash`: upstream
//!   `SmMapPtrToPtr` iteration order is pointer order; only lookups are used here) and
//!   the ported [`CopyBrepMap`] for the optional copy maps.
//! - `SmPendingTopologyDelete`'s destructor work (detaching the pending object's
//!   attributes and deleting it) runs in `MakeTopologyFromData` (unit
//!   `topology.brep_p10`); this unit owns only `Clear`.
//! - `SmLockBrepBox` is a guard over `SmBrep::LockBoundingBox` / `UnLockBoundingBox`
//!   (unit `query.brep`).
//! - Statuses upstream does not check (`GetEnds`, point evaluations, `SetUVTrimCurve`,
//!   `Notify`, `ReverseParameterization`, `EditParameterization`) do not abort the port
//!   either: only [`SolidError::Unported`] propagates from them; a failed point
//!   evaluation leaves undefined coordinates (upstream leaves the `SmPoint3d` unset).
//! - `sm_MakeVertexSplitUVTrimCurve` clones the face surface for the lift of the split
//!   point (upstream borrows it); `sm_FindSectorOfFace` returns the region as `Option`
//!   (upstream hands back the shell's region pointer unchecked).
//! - Calls into units not ported yet return [`SolidError::Unported`] (every site is
//!   tagged `UNPORTED(<unit>)`, one stub per call site with the code after it ported):
//!   `SmFace::FaceIntersect` (`topology.face_p3`), `SmBrep::CalculateBoundingBox`, `LockBoundingBox`,
//!   `UnLockBoundingBox` (`query.brep`), `SmBSplineSurface::CreateCornerBlend` with a
//!   non-B-spline neighbour surface (`nurbs.b_spline_surface_n_lib_p2`, whose port takes
//!   B-spline neighbours only).
//! - `CreateEdgeEdgeBlend` takes the B-rep mutably (the edge use evaluations of
//!   `topology.edgeuse_p3` take it so); it ignores both `EvaluateBinormal` statuses as
//!   upstream and goes on with the outputs upstream sees after a failure (undefined, or
//!   zeroed point, tangent and normal).
//! - `GetShells` dedups with a shell mark record local to the call (upstream marks the
//!   `SmShell` objects); `SetRegionIsVoidFlagsForNestedSolids` bounds its shell queue at
//!   `4 * shells + 16` passes.

use std::collections::BTreeMap;

use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::p3::PointSolveOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::a_object::sag_is_kind_of;
use crate::geom::attribute::{AttributeHandle, AttributeStore, ObjectKey};
use crate::geom::context::{with_mark_lock, MarkType, ALL_MARKS};
use crate::geom::extent1d::{Extent1d, ExtentPointType};
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::tol;
use crate::heal::face_props::{HealerOpType, LoopObject};
use crate::heal::heal_data::p1::HealBrepTargets;
use crate::heal::heal_data::p2::HealData;
use crate::intersect::point_class::{ClassObject, ClassObjectKind};
use crate::kernel::Kernel;
use crate::math::{max2, Point2, Point3, Vec3, EFF_ZERO, EFF_ZERO_RAD, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::{BlendEndType, CurvatureType, DerivSurfDefinition, SurfaceKind};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::brep_cache::{arena_identity, CacheObject};
use crate::topology::edge::{self, AttributeFilter};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p2::BinormalEval;
use crate::topology::edgeuse::p3::{self as eu3, BinormalOptions};
use crate::topology::face::p1 as face1;
use crate::topology::face::p2 as face2;
use crate::topology::loopuse;
use crate::topology::proto_topology::p1::CopyBrepMap;
use crate::topology::topology::Topology;
use crate::topology::topology_traverser::{Entity, Marks};
use crate::topology::vertex;
use crate::topology::vertexuse;
use crate::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, LoopId, RegionId, ShellId, VertexId, Vertexuse, VertexuseId,
};

/// `SmBoolean` `TRUE` / `FALSE`.
const TRUE: i32 = 1;
const FALSE: i32 = 0;

const OP: &str = "topology::brep::p1";

/// `SmBrep::GetType`: `SmBrep_TYPE`.
pub const TYPE: i64 = 16011;
/// `SmBrep::GetTypeString`.
pub const TYPE_STRING: &str = "SmBrep_TYPE";
/// `SmBrep::GetClassString`.
pub const CLASS_STRING: &str = "SmBrep";

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i32 = 1007;

/// Deepest loop nesting `sm_MarkLoopsToDelete` follows before failing (upstream
/// recurses without bound).
pub const MAX_LOOP_NESTING: u64 = 256;

/// `SmBrep::IsKindOf`: `SmBrep_TYPE` or a base class type (`SmSAGObject` and up).
#[must_use]
pub const fn is_kind_of(t: i64) -> bool {
    t == TYPE || sag_is_kind_of(t)
}

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn status(code: i32) -> SolidError {
    SolidError::Upstream {
        operation: OP,
        code,
        entity: None,
    }
}

fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

// ---- SmBrep members (SmBrep.h inline functions) -----------------------------------------

impl Brep {
    /// `GetThisModelSizeEstimate`: the B-rep's model size estimate; with
    /// `chase_defaults` (upstream default `TRUE`) a stored [`tol::USE_DEFAULT`] is
    /// replaced by the context's estimate (`m_cpContext->GetThisModelSizeEstimate()`,
    /// the kernel context).
    #[must_use]
    pub fn this_model_size_estimate(&self, k: &Kernel, chase_defaults: bool) -> f64 {
        if !chase_defaults {
            self.model_size_estimate
        } else if self.model_size_estimate == tol::USE_DEFAULT {
            k.context.this_model_size_estimate(k)
        } else {
            self.model_size_estimate
        }
    }

    /// `GetThisLargeSmallSizeRatio`: as [`Brep::this_model_size_estimate`] for the
    /// large/small size ratio (the kernel's system ratio when the context has none).
    #[must_use]
    pub fn this_large_small_size_ratio(&self, k: &Kernel, chase_defaults: bool) -> f64 {
        if !chase_defaults {
            self.large_small_size_ratio
        } else if self.large_small_size_ratio == tol::USE_DEFAULT {
            // SmContext::GetThisLargeSmallSizeRatio
            let ratio = k.context.stored_large_small_size_ratio();
            if ratio == tol::USE_DEFAULT {
                k.tolerances.large_small_size_ratio
            } else {
                ratio
            }
        } else {
            self.large_small_size_ratio
        }
    }

    /// `SetThisModelSizeEstimate` (upstream default `SM_USE_DEFAULT`): stores the
    /// estimate, passes it to the B-rep's context (the kernel context) and sets the
    /// B-rep tolerance to [`tol::size_estimate_to_zone_tol_3d`] of it; returns that
    /// tolerance.
    pub fn set_this_model_size_estimate(
        &mut self,
        k: &mut Kernel,
        model_size_estimate: f64,
    ) -> f64 {
        self.model_size_estimate = model_size_estimate;
        k.context.set_this_model_size_estimate(model_size_estimate);
        self.tolerance = tol::size_estimate_to_zone_tol_3d(self.model_size_estimate);
        self.tolerance
    }

    /// `SetThisLargeSmallSizeRatio` (upstream default `SM_USE_DEFAULT`): stores the
    /// ratio and passes it to the B-rep's context (the kernel context).
    pub fn set_this_large_small_size_ratio(&mut self, k: &mut Kernel, large_small_size_ratio: f64) {
        self.large_small_size_ratio = large_small_size_ratio;
        k.context
            .set_this_large_small_size_ratio(large_small_size_ratio);
    }

    /// `SetInfiniteRegion`: makes `region` the infinite region (the raw member write;
    /// [`reset_infinite_region`] does the bookkeeping).
    pub fn set_infinite_region(&mut self, region: Option<RegionId>) {
        self.set_infinite_region_record(region);
    }

    /// `m_pFaceListHead`: the faces in list order.
    #[must_use]
    pub fn face_list(&self) -> &[FaceId] {
        &self.face_list
    }

    /// `m_pEdgeListHead`: the edges in list order.
    #[must_use]
    pub fn edge_list(&self) -> &[EdgeId] {
        &self.edge_list
    }

    /// `m_pVertexListHead`: the vertices in list order.
    #[must_use]
    pub fn vertex_list(&self) -> &[VertexId] {
        &self.vertex_list
    }

    /// Reorders the face list (an importer's friend write): `order` must hold every
    /// face record exactly once.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] otherwise.
    pub fn set_face_list(&mut self, order: Vec<FaceId>) -> Result<()> {
        check_order(&order, self.faces.len())?;
        self.face_list = order;
        Ok(())
    }

    /// Reorders the edge list (an importer's friend write): `order` must hold every
    /// edge record exactly once.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] otherwise.
    pub fn set_edge_list(&mut self, order: Vec<EdgeId>) -> Result<()> {
        check_order(&order, self.edges.len())?;
        self.edge_list = order;
        Ok(())
    }

    /// Reorders the vertex list (an importer's friend write): `order` must hold every
    /// vertex record exactly once.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] otherwise.
    pub fn set_vertex_list(&mut self, order: Vec<VertexId>) -> Result<()> {
        check_order(&order, self.vertices.len())?;
        self.vertex_list = order;
        Ok(())
    }

    /// The head of the B-rep's region list (`SmOwningTopology::m_pList`), `None` for
    /// the first region record.
    #[must_use]
    pub const fn region_list_head(&self) -> Option<RegionId> {
        self.region_list_head
    }

    /// Sets the head of the B-rep's region list (the friend write `m_pList = ..`).
    pub fn set_region_list_head(&mut self, region: Option<RegionId>) {
        self.region_list_head = region;
    }

    /// `AddDeletedSurface`: keeps a surface removed from the B-rep until the operation
    /// ends (the list is created on first use upstream).
    pub fn add_deleted_surface(&mut self, surface: Surface) {
        self.deleted_surfaces.push(surface);
    }

    /// `m_pDeletedSurfaces`: the surfaces kept by [`Brep::add_deleted_surface`], in
    /// order.
    #[must_use]
    pub fn deleted_surfaces(&self) -> &[Surface] {
        &self.deleted_surfaces
    }
}

/// Whether `order` is a permutation of the `count` arena ids.
fn check_order<I: crate::topology::ArenaId>(order: &[I], count: usize) -> Result<()> {
    let mut seen = vec![false; count];
    for id in order {
        match seen.get_mut(id.index() as usize) {
            Some(s) if !*s => *s = true,
            _ => return Err(null("list order is not a permutation of the records")),
        }
    }
    if order.len() != count {
        return Err(null("list order is not a permutation of the records"));
    }
    Ok(())
}

/// `GetNumFaces`: the face count (`m_pFaceListHead->GetSize()`).
#[must_use]
pub fn num_faces(b: &Brep) -> usize {
    b.face_list().len()
}

/// `IsLiveTopologyMember`: whether `target` is on the B-rep's face, edge or vertex
/// list (identity only; `None` is `FALSE`, as every other entity kind).
#[must_use]
pub fn is_live_topology_member(b: &Brep, target: Option<EntityRef>) -> bool {
    match target {
        Some(EntityRef::Face(f)) => b.face_list().contains(&f),
        Some(EntityRef::Edge(e)) => b.edge_list().contains(&e),
        Some(EntityRef::Vertex(v)) => b.vertex_list().contains(&v),
        _ => false,
    }
}

// ---- collections (SmOwningTopology::GetAll over the B-rep's lists) --------------------

fn filtered<I: Copy>(
    ids: impl Iterator<Item = I>,
    attribute: Option<&AttributeFilter<'_>>,
    entity: impl Fn(I) -> EntityRef,
) -> Vec<I> {
    match attribute {
        None => ids.collect(),
        Some(filter) => ids.filter(|&id| filter.matches(entity(id))).collect(),
    }
}

/// `GetRegions`: the B-rep's region list from its head ([`Brep::region_list_head`]),
/// those carrying `attribute` when given.
///
/// # Errors
///
/// [`SolidError::NotFound`] when the list head is a dangling id.
pub fn regions(b: &Brep, attribute: Option<&AttributeFilter<'_>>) -> Result<Vec<RegionId>> {
    let on_list: Vec<RegionId> = b
        .regions()
        .iter()
        .filter(|(_, r)| r.in_brep)
        .map(|(id, _)| id)
        .collect();
    let start = match b.region_list_head() {
        None => 0,
        Some(head) => {
            b.region(head)?;
            // (upstream's head is on the list by construction)
            on_list
                .iter()
                .position(|&r| r == head)
                .ok_or_else(|| null("GetRegions: region list head not on the list"))?
        }
    };
    let rotated = on_list
        .iter()
        .skip(start)
        .chain(on_list.iter().take(start))
        .copied();
    Ok(filtered(rotated, attribute, EntityRef::Region))
}

/// `GetFaces`: every face in list order, those carrying `attribute` when given.
///
/// # Errors
///
/// None (the signature matches the other collections).
pub fn faces(b: &Brep, attribute: Option<&AttributeFilter<'_>>) -> Result<Vec<FaceId>> {
    Ok(filtered(
        b.face_list().iter().copied(),
        attribute,
        EntityRef::Face,
    ))
}

/// `GetEdges`: every edge in list order, those carrying `attribute` when given.
///
/// # Errors
///
/// None (the signature matches the other collections).
pub fn edges(b: &Brep, attribute: Option<&AttributeFilter<'_>>) -> Result<Vec<EdgeId>> {
    Ok(filtered(
        b.edge_list().iter().copied(),
        attribute,
        EntityRef::Edge,
    ))
}

/// `GetVertices`: every vertex in list order, those carrying `attribute` when given.
///
/// # Errors
///
/// None (the signature matches the other collections).
pub fn vertices(b: &Brep, attribute: Option<&AttributeFilter<'_>>) -> Result<Vec<VertexId>> {
    Ok(filtered(
        b.vertex_list().iter().copied(),
        attribute,
        EntityRef::Vertex,
    ))
}

/// `GetShells`: the distinct shells of every region (`SmRegion::GetShells` under a new
/// mark, in region then shell order), those carrying `attribute` when given.
///
/// # Errors
///
/// [`SolidError::Unported`] from `SmRegion::GetShells` (unit `topology.shell`) for a
/// B-rep with regions; a B-rep without regions returns the empty list, as upstream
/// (the per-region loop never runs); the errors of the mark lock.
pub fn shells(
    k: &mut Kernel,
    b: &Brep,
    attribute: Option<&AttributeFilter<'_>>,
) -> Result<Vec<ShellId>> {
    // get regions
    let regions = regions(b, None)?;
    with_mark_lock(k, ALL_MARKS, |k, mark| -> Result<Vec<ShellId>> {
        let mut marked: BTreeMap<ShellId, Topology> = BTreeMap::new();
        let mut out = Vec::new();
        // for every region
        for r in regions {
            // get this region's shells
            for s in region_shells(b, r, attribute)? {
                // skip marked shells
                let rec = marked.entry(s).or_default();
                if rec.is_marked(&k.context, mark) {
                    continue;
                }
                // mark and add shell to output
                rec.mark(Some(&k.context), mark);
                out.push(s);
            }
        }
        Ok(out)
    })?
}

/// `GetWireEdges`: the wire edges of [`edges`] (`SmEdge::IsWire`).
///
/// # Errors
///
/// Those of [`edge::is_wire`].
pub fn wire_edges(b: &Brep, attribute: Option<&AttributeFilter<'_>>) -> Result<Vec<EdgeId>> {
    let mut out = Vec::new();
    // add wire edges to output array
    for e in edges(b, attribute)? {
        if edge::is_wire(b, e)? {
            out.push(e);
        }
    }
    Ok(out)
}

/// `GetTopologicalEdges`: the edges `SmEdge::IsTopological` (default gain) accepts:
/// those removable by merging their two faces. As upstream the coverage test may
/// expand a face's surface in place.
///
/// # Errors
///
/// Those of [`edge::is_topological`].
pub fn topological_edges(k: &Kernel, b: &mut Brep) -> Result<Vec<EdgeId>> {
    let mut out = Vec::new();
    for e in edges(b, None)? {
        if edge::is_topological(k, b, e, 1.0)?.is_topological {
            out.push(e);
        }
    }
    Ok(out)
}

/// `GetTopologicalVertices`: the vertices `SmVertex::IsTopologicalVertex` accepts:
/// those removable by merging their two edges.
///
/// # Errors
///
/// Those of [`vertex::is_topological_vertex`].
pub fn topological_vertices(k: &Kernel, b: &mut Brep) -> Result<Vec<VertexId>> {
    let mut out = Vec::new();
    for v in vertices(b, None)? {
        if vertex::is_topological_vertex(k, b, v)? {
            out.push(v);
        }
    }
    Ok(out)
}

/// The attribute filter of [`surfaces`] and [`curves`]: upstream tests the geometry's
/// own attributes (`pSurface->FindAttribute`, `pCurve->FindAttribute`), so the keys
/// name a face's surface and an edge's curve in `attributes`.
pub struct GeometryAttributeFilter<'a> {
    /// The attribute store.
    pub attributes: &'a AttributeStore,
    /// `*pOptAttributeId`.
    pub attribute_id: u64,
    /// The key of face `f`'s surface.
    pub surface_key_of: &'a dyn Fn(FaceId) -> ObjectKey,
    /// The key of edge `e`'s curve.
    pub curve_key_of: &'a dyn Fn(EdgeId) -> ObjectKey,
}

impl core::fmt::Debug for GeometryAttributeFilter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GeometryAttributeFilter")
            .field("attribute_id", &self.attribute_id)
            .finish_non_exhaustive()
    }
}

/// `GetSurfaces`: the surfaces owned by their faces (`pSurface->GetOwner() == pFace`,
/// the owner being the face's [`arena_identity`]), in face order, those carrying
/// `attribute` when given.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface (upstream dereferences
/// it).
pub fn surfaces<'b>(
    b: &'b Brep,
    attribute: Option<&GeometryAttributeFilter<'_>>,
) -> Result<Vec<&'b Surface>> {
    let mut out = Vec::new();
    // for every face
    for f in faces(b, None)? {
        let surface = b
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| null("GetSurfaces: face without surface"))?;
        // add all standalone face->Surfaces to the list
        if surface.owner() == Some(arena_identity(CacheObject::Face(f)))
            && attribute.is_none_or(|filter| {
                filter
                    .attributes
                    .find_attribute((filter.surface_key_of)(f), filter.attribute_id)
                    .is_some()
            })
        {
            out.push(surface);
        }
    }
    Ok(out)
}

/// `GetCurves`: the 3D curves owned by their edges (`pCurve->GetOwner() == pEdge`, the
/// owner being the edge's [`arena_identity`]), in edge order, those carrying
/// `attribute` when given.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge without curve (upstream dereferences it).
pub fn curves<'b>(
    b: &'b Brep,
    attribute: Option<&GeometryAttributeFilter<'_>>,
) -> Result<Vec<&'b Curve>> {
    let mut out = Vec::new();
    for e in edges(b, None)? {
        let curve = b
            .edge(e)?
            .curve
            .as_ref()
            .ok_or_else(|| null("GetCurves: edge without curve"))?;
        if curve.owner() == Some(arena_identity(CacheObject::Edge(e)))
            && attribute.is_none_or(|filter| {
                filter
                    .attributes
                    .find_attribute((filter.curve_key_of)(e), filter.attribute_id)
                    .is_some()
            })
        {
            out.push(curve);
        }
    }
    Ok(out)
}

/// `IsDegenerate`: whether every vertex lies within `tol_3d` of the first and the
/// B-rep's bounding box is point sized to `tol_3d`.
///
/// # Errors
///
/// [`SolidError::Unported`] when the vertex check passes: the bounding box is
/// `SmBrep::CalculateBoundingBox` (unit `query.brep`), then `SmExtent3d::IsPointSized`.
pub fn is_degenerate(k: &Kernel, b: &Brep, tol_3d: f64) -> Result<bool> {
    let _ = k;
    let tol_sq = tol_3d * tol_3d;
    let verts = vertices(b, None)?;

    // Do quick cheap check on Vertices
    if let Some(&first) = verts.first() {
        let p0 = b.vertex(first)?.point;
        for &v in verts.iter().skip(1) {
            if p0.distance_squared(b.vertex(v)?.point) > tol_sq {
                return Ok(false);
            }
        }
    }

    // If Brep might be degenerate - Do expensive full check (CalculateBoundingBox's
    // status is ignored: a failure leaves the default box)
    // CalculateBoundingBox(sBBox): header defaults bApprox = FALSE, bTight = FALSE,
    // bExpandByZoneTol3d = TRUE
    let bbox = ignored(brep_bounding_box(b, false, false, true))?.unwrap_or_default();
    Ok(bbox.is_point_sized(tol_3d))
}

// ---- infinite region ------------------------------------------------------------------

/// `SmSetRegionVoidType`: how `IsVoid` flags are set after the infinite region changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetRegionVoidType {
    /// `SM_VS_NESTEDSOLID` (the default): a nested solid model
    /// ([`set_region_is_void_flags_for_nested_solids`]).
    #[default]
    NestedSolid,
    /// `SM_VS_INFINITEREGION_ONLY`: only the new infinite region becomes void.
    InfiniteRegionOnly,
    /// `SM_VS_INFINITEREGION_SWAP`: the old infinite region takes the new one's flag,
    /// the new one becomes void.
    InfiniteRegionSwap,
}

/// `ResetInfiniteRegion`: makes `new_infinite_region` (one of the B-rep's regions) the
/// infinite region and the head of the region list, then sets the `IsVoid` flags as
/// `void_setting` asks.
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for no region (`NULL`) or one not on the B-rep's list;
/// [`SolidError::Unported`] for the flag update (`SmRegion::SetIsVoid` / `IsVoid`,
/// unit `topology.shell`), after the region and list head have been changed. The
/// nested-solid update's statuses are ignored as upstream (only `Unported` and the
/// null-dereference `InvalidInput` propagate).
pub fn reset_infinite_region(
    k: &mut Kernel,
    b: &mut Brep,
    new_infinite_region: Option<RegionId>,
    void_setting: SetRegionVoidType,
) -> Result<()> {
    // No work: Null input
    let Some(new_infinite_region) = new_infinite_region else {
        return Err(status(SM_ERR_INVALID_INPUT));
    };

    // If given region is not one our our regions, fail.
    if !regions(b, None)?.contains(&new_infinite_region) {
        return Err(status(SM_ERR_INVALID_INPUT));
    }

    // Reset the infinite region, and make sure that the first in list is infinite region.
    let old_infinite_region = b.infinite_region();
    b.set_infinite_region(Some(new_infinite_region));
    b.set_region_list_head(Some(new_infinite_region));

    // manage IsVoid region bits
    match void_setting {
        SetRegionVoidType::NestedSolid => {
            // status ignored upstream; the port's null-dereference marker still propagates
            match set_region_is_void_flags_for_nested_solids(k, b) {
                Err(e @ SolidError::InvalidInput { .. }) => return Err(e),
                r => ignore_status(r)?,
            }
        }
        SetRegionVoidType::InfiniteRegionOnly => {
            region_set_is_void(b, new_infinite_region, TRUE)?;
        }
        SetRegionVoidType::InfiniteRegionSwap => {
            let Some(old) = old_infinite_region else {
                // upstream dereferences the (null) old infinite region here
                return Err(null(
                    "ResetInfiniteRegion swap without an old infinite region",
                ));
            };
            let new_is_void = region_is_void(b, new_infinite_region)?;
            region_set_is_void(b, old, new_is_void)?;
            region_set_is_void(b, new_infinite_region, TRUE)?;
        }
    }
    Ok(())
}

/// `SetRegionIsVoidFlagsForNestedSolids`: with the infinite region first on the
/// region list, marks it void and alternates the flag down every level of nested
/// shells (each nested region's outer shell becomes its list head).
///
/// # Errors
///
/// `SM_ERR` when the infinite region is not identified or not the list head;
/// [`SolidError::Unported`] from the region and shell calls (`SmRegion::SetIsVoid`,
/// `IsVoid`, `GetShells`, `SetList`, `SmShell::GetMatedShells`: their data
/// is unit `topology.shell`'s); [`SolidError::InvalidInput`] where upstream
/// dereferences null, and past `4 * shells + 16` queue passes.
pub fn set_region_is_void_flags_for_nested_solids(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    // check state - look for a properly labeled infinite region
    let Some(infinite) = b.infinite_region() else {
        return Err(sm_err());
    };
    if regions(b, None)?.first() != Some(&infinite) {
        return Err(sm_err());
    }

    // mark the infinite region as void
    region_set_is_void(b, infinite, TRUE)?;

    // get the infinite region's shells
    let mut queue = region_shells(b, infinite, None)?;

    // for every shell in the queue (bounded: a well-formed nesting visits each shell
    // once)
    let bound = b.shells().len().saturating_mul(4).saturating_add(16);
    let mut passes = 0usize;
    while !queue.is_empty() {
        passes += 1;
        if passes > bound {
            return Err(null(
                "SetRegionIsVoidFlagsForNestedSolids: shell nesting does not terminate",
            ));
        }
        let shell = queue.remove(0);
        let region = shell_region(b, shell)?
            .ok_or_else(|| null("SetRegionIsVoidFlagsForNestedSolids: shell without region"))?;

        // nested regions get the opposite flag (`!pRegion->IsVoid()` on the raw SmBoolean:
        // UNSURE (2) gives FALSE)
        let nested_is_void = i32::from(region_is_void(b, region)? == FALSE);

        // the shells mated to this one (increments an unused mark value)
        // (status ignored upstream: a failed call is read as no mated shells)
        for mated in ignored(shell_mated_shells(k, b, shell))?.unwrap_or_default() {
            let nested = shell_region(b, mated)?.ok_or_else(|| {
                null("SetRegionIsVoidFlagsForNestedSolids: mated shell without region")
            })?;
            region_set_is_void(b, nested, nested_is_void)?;

            // the mated shell is the nested region's outer shell
            region_set_list(b, nested, mated)?;

            // add all inner shells to the queue (the 1st shell is the outer shell)
            let mut nested_shells = region_shells(b, nested, None)?;
            // RemoveAt(0) of an empty array leaves it empty in the release build
            // (SM_ASSERT_BREAK is a no-op)
            if !nested_shells.is_empty() {
                nested_shells.remove(0);
            }
            queue.extend(nested_shells);
        }
    }
    Ok(())
}

// ---- region votes (sm_VoteForRegion, sm_FindHighestVote) -------------------------------

/// `sm_VoteForRegion`: adds a vote for `new_region` (appending it with one vote when
/// it is not listed yet). A null region (`None`, the result of a failed
/// `sm_FindSectorOfFace`) is a region of its own, as upstream.
pub fn vote_for_region(
    regions: &mut Vec<Option<RegionId>>,
    votes: &mut Vec<u64>,
    new_region: Option<RegionId>,
) {
    for (i, r) in regions.iter().enumerate() {
        if *r == new_region {
            if let Some(v) = votes.get_mut(i) {
                *v = v.saturating_add(1);
            }
            return;
        }
    }
    regions.push(new_region);
    votes.push(1);
}

/// `sm_FindHighestVote`: the first region with the most votes; `None` for no regions
/// and when the null region wins (upstream returns `NULL` in both cases).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] when `votes` is shorter than `regions` (upstream reads
/// past the array).
pub fn find_highest_vote(regions: &[Option<RegionId>], votes: &[u64]) -> Result<Option<RegionId>> {
    if votes.len() < regions.len() {
        return Err(null("sm_FindHighestVote: fewer votes than regions"));
    }
    let mut highest = 0usize;
    for i in 1..regions.len() {
        if votes.get(highest) < votes.get(i) {
            highest = i;
        }
    }
    Ok(regions.get(highest).copied().flatten())
}

// ---- copy maps (sm_SetObjectMap, sm_GetObjectMap) ---------------------------------------

/// The `SmMapPtrToPtr<SmTopology, SmTopology>` of the B-rep copy: original object to
/// its copy (`None` is a null copy).
pub type ObjectMap = BTreeMap<EntityRef, Option<EntityRef>>;

/// `sm_SetObjectMap`: records `orig -> copy`, and in the optional `SmCopyBrepMap`s
/// ([`CopyBrepMap`], unit `topology.proto_topology_p1`) `copy -> orig` and `orig ->
/// copy`. A null copy is recorded only in `map`: upstream passes it to
/// `SmCopyBrepMap::SetAt`, which dereferences it (UB-98).
pub fn set_object_map(
    orig: EntityRef,
    copy: Option<EntityRef>,
    map: &mut ObjectMap,
    copy_brep_map: Option<&mut CopyBrepMap<EntityRef>>,
    reverse_map: Option<&mut CopyBrepMap<EntityRef>>,
) {
    // Use a map object track relationships.
    map.insert(orig, copy);
    if let (Some(m), Some(c)) = (copy_brep_map, copy) {
        m.set_at(c, orig);
    }
    if let (Some(m), Some(c)) = (reverse_map, copy) {
        m.set_at(orig, c);
    }
}

/// `sm_GetObjectMap`: the copy recorded for `from` (`None` when none, or a null copy).
#[must_use]
pub fn get_object_map(from: EntityRef, map: &ObjectMap) -> Option<EntityRef> {
    map.get(&from).copied().flatten()
}

// ---- attributes (sm_AddAttributes, SmPendingTopologyDelete) -----------------------------

/// `sm_AddAttributes`: adds the attributes `attributes[i]` for every index `i` of
/// `indexed_attributes` to the object `obj`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an index past `attributes` (upstream reads past the
/// array); those of [`AttributeStore::add_attribute`].
pub fn add_attributes(
    store: &mut AttributeStore,
    obj: ObjectKey,
    indexed_attributes: &[usize],
    attributes: &[AttributeHandle],
) -> Result<()> {
    // for every Attribute index
    for &index in indexed_attributes {
        // get attribute
        let attr = attributes
            .get(index)
            .copied()
            .ok_or_else(|| null("sm_AddAttributes: attribute index out of range"))?;
        // add attribute to object
        store.add_attribute(obj, attr)?;
    }
    Ok(())
}

/// `SmPendingTopologyDelete`: the topology object `MakeTopologyFromData` must delete
/// (after detaching its borrowed attributes) if it exits early; [`Self::clear`]
/// releases it once it is linked into the B-rep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingTopologyDelete {
    pending: Option<EntityRef>,
}

impl PendingTopologyDelete {
    /// `SmPendingTopologyDelete(pTopology)`.
    #[must_use]
    pub const fn new(topology: Option<EntityRef>) -> Self {
        Self { pending: topology }
    }

    /// `Clear`: the object is linked in; nothing is deleted.
    pub fn clear(&mut self) {
        self.pending = None;
    }

    /// The object still pending deletion (`m_pTopology`), read by the destructor
    /// (unit `topology.brep_p10`).
    #[must_use]
    pub const fn pending(&self) -> Option<EntityRef> {
        self.pending
    }
}

// ---- reflection helpers (sm_ReverseReflectionEdgeuses, sm_SwapReflectionShells) ---------

/// `sm_ReverseReflectionEdgeuses`: swaps the members of every mated pair of the edge's
/// edge use list (`[a0 a1 b0 b1 ..]` becomes `[a1 a0 b1 b0 ..]`).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an odd
/// edge use count (upstream reads past the array).
pub fn reverse_reflection_edgeuses(b: &mut Brep, e: EdgeId) -> Result<()> {
    let uses = b.edge(e)?.edgeuses.clone();
    if uses.len() % 2 != 0 {
        return Err(null("sm_ReverseReflectionEdgeuses: odd edge use count"));
    }
    let mut relinked = Vec::with_capacity(uses.len());
    for pair in uses.chunks_exact(2) {
        if let [first, second] = *pair {
            relinked.push(second);
            relinked.push(first);
        }
    }
    b.edge_mut(e)?.edgeuses = relinked;
    Ok(())
}

/// `sm_SwapReflectionShells`: when the face's two face uses sit in different shells,
/// exchanges the shells they belong to.
///
/// Upstream removes each face use from its shell's list and appends it to the other's
/// (`Remove` / `PostInsert`): the membership is each face use's `shell`, swapped here.
/// The order of a shell's face use list is `SmShell`'s (unit `topology.shell`) and is
/// not modelled yet. `Face::shell` and `Shell::faces` have no upstream counterpart
/// (`SmFace` holds no shell; the arena builders fill them, the import leaves
/// `Shell::faces` empty): they follow the first face use, so the face moves to the
/// other shell's list.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// upstream dereferences null (a face without two face uses, a face use without
/// shell).
pub fn swap_reflection_shells(b: &mut Brep, f: FaceId) -> Result<()> {
    let (first, second) = face2::faceuses(b, f)?;
    let first = first.ok_or_else(|| null("sm_SwapReflectionShells: face without face use"))?;
    let second = second.ok_or_else(|| null("sm_SwapReflectionShells: face use without mate"))?;
    let a = eu1::faceuse_shell(b, first)?
        .ok_or_else(|| null("sm_SwapReflectionShells: face use without shell"))?;
    let bb = eu1::faceuse_shell(b, second)?
        .ok_or_else(|| null("sm_SwapReflectionShells: mate without shell"))?;
    if a != bb {
        b.faceuse_mut(first)?.shell = Some(bb);
        b.faceuse_mut(second)?.shell = Some(a);
        // the face record follows its first face use: move it to the end of the other
        // shell's face list (PostInsert appends)
        b.shell_mut(a)?.faces.retain(|&x| x != f);
        b.shell_mut(bb)?.faces.retain(|&x| x != f);
        b.shell_mut(bb)?.faces.push(f);
        b.face_mut(f)?.shell = bb;
    }
    Ok(())
}

// ---- loop vertices (sm_RmAndDel_OldLoopVertices) ----------------------------------------

/// `SmTopology::GetNext` / `GetLast` of a vertex use within its vertex's circular
/// vertex use list.
fn vertexuse_neighbour(b: &Brep, vu: VertexuseId, next: bool) -> Result<Option<VertexuseId>> {
    let Some(v) = b.vertexuse(vu)?.vertex else {
        return Ok(None);
    };
    let list = &b.vertex(v)?.vertexuses;
    let Some(i) = list.iter().position(|&x| x == vu) else {
        return Ok(None);
    };
    let n = list.len();
    let j = if next { (i + 1) % n } else { (i + n - 1) % n };
    Ok(list.get(j).copied())
}

/// The loop of a vertex use's loop use (`GetLoopuse()->GetLoop()`).
fn vertexuse_loop(b: &Brep, vu: VertexuseId) -> Result<Option<LoopId>> {
    let lu = vertexuse::loopuse(b, vu)?
        .ok_or_else(|| null("sm_RmAndDel_OldLoopVertices: loop vertex use without loop use"))?;
    loopuse::loop_(b, lu)
}

/// `sm_RmAndDel_OldLoopVertices`: when `vu` is a loop vertex use, finds its partner
/// (the next, else the previous, loop vertex use on the same loop in the vertex's
/// list), removes both from the vertex and deletes them (their records are reset).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; `SM_ERR_NULL_POINTER` for a vertex use
/// without list neighbour (`NER`); [`SolidError::InvalidInput`] where upstream
/// dereferences null (no loop use, no vertex); `SM_ERR` when no partner is found.
pub fn rm_and_del_old_loop_vertices(b: &mut Brep, vu: VertexuseId) -> Result<()> {
    // when pVertexuse is a LoopVertexuse
    if !vertexuse::is_loop_vertexuse(b, vu)? {
        return Ok(());
    }
    let this_loop = vertexuse_loop(b, vu)?;
    let partner = |b: &Brep, other: VertexuseId| -> Result<bool> {
        Ok(vertexuse::is_loop_vertexuse(b, other)? && vertexuse_loop(b, other)? == this_loop)
    };

    let other = vertexuse_neighbour(b, vu, true)?.ok_or_else(|| SolidError::null_pointer(OP))?; // NER

    // When OtherVU is a LoopVertexuse attached to the same Loop as input VU
    let del = if partner(b, other)? {
        other
    } else {
        // try the VU in the other direction of the Vertex's doubly linked Vertexuse list
        let other =
            vertexuse_neighbour(b, vu, false)?.ok_or_else(|| SolidError::null_pointer(OP))?; // NER
        if partner(b, other)? {
            other
        } else {
            // Unable to find the partner of this loop vertexuse
            return Err(sm_err());
        }
    };

    // remove the Tgt LoopVertexuse and its mated pair from their Vertex
    let v = b
        .vertexuse(vu)?
        .vertex
        .ok_or_else(|| null("sm_RmAndDel_OldLoopVertices: vertex use without vertex"))?;
    b.vertex_mut(v)?.vertexuses.retain(|&x| x != vu && x != del);

    // delete the LoopVertexuses
    *b.vertexuse_mut(vu)? = Vertexuse::default();
    *b.vertexuse_mut(del)? = Vertexuse::default();
    Ok(())
}

// ---- loop marking (sm_MarkLoopsToDelete) ------------------------------------------------

/// `sm_MarkLoopsToDelete`: given a 'big' face made from a surface's natural trim and
/// the inner topology to keep, appends to `loops_to_delete` the loops whose nesting
/// level (`rec_level`, even levels) marks them for deletion, following each other loop
/// of the face across its first edge's radial neighbour.
///
/// # Errors
///
/// `SM_ERR` for an edge that is a wire, lamina or not manifold; [`SolidError::NotFound`]
/// for a dangling id; [`SolidError::InvalidInput`] where upstream dereferences null or
/// reads an empty edge list, past [`MAX_LOOP_NESTING`] levels and past
/// [`MAX_LOOP_MARK_CALLS`] calls in all (the breadth bound).
pub fn mark_loops_to_delete(
    b: &Brep,
    loops_to_delete: &mut Vec<LoopId>,
    loop_: LoopId,
    rec_level: u64,
) -> Result<()> {
    let mut calls = 0u64;
    mark_loops_to_delete_bounded(b, loops_to_delete, loop_, rec_level, &mut calls)
}

/// The bound on the total number of `sm_MarkLoopsToDelete` calls of one marking: the
/// recursion is bounded in depth by [`MAX_LOOP_NESTING`] but branches at every other
/// loop of a face, so a radial chain that cycles through faces with three or more
/// loops would otherwise branch without bound (upstream hangs there).
pub const MAX_LOOP_MARK_CALLS: u64 = 1 << 20;

fn mark_loops_to_delete_bounded(
    b: &Brep,
    loops_to_delete: &mut Vec<LoopId>,
    loop_: LoopId,
    rec_level: u64,
    calls: &mut u64,
) -> Result<()> {
    if rec_level > MAX_LOOP_NESTING {
        return Err(null("sm_MarkLoopsToDelete: loop nesting too deep"));
    }
    *calls += 1;
    if *calls > MAX_LOOP_MARK_CALLS {
        return Err(null("sm_MarkLoopsToDelete: too many loop visits"));
    }
    let face = loopuse::loop_face(b, loop_)?
        .ok_or_else(|| null("sm_MarkLoopsToDelete: loop without face"))?;
    let even_level = rec_level % 2 == 0;

    for l in face2::loops(b, face, false, None)? {
        if l == loop_ {
            continue;
        }
        let edges = loopuse::loop_edges(b, l)?;
        let e = edges
            .first()
            .copied()
            .ok_or_else(|| null("sm_MarkLoopsToDelete: loop without edges"))?
            .ok_or_else(|| null("sm_MarkLoopsToDelete: edge use without edge"))?;
        if edge::is_wire(b, e)? {
            return Err(sm_err());
        }
        if edge::is_lamina(b, e)? {
            return Err(sm_err());
        }
        if !edge::is_manifold(b, e)? {
            return Err(sm_err());
        }

        let eu = edge::primary_edgeuse(b, e)?
            .ok_or_else(|| null("sm_MarkLoopsToDelete: edge without edge use"))?; // may not belong to pF
        let eu_rad = eu1::radial(b, eu)?
            .ok_or_else(|| null("sm_MarkLoopsToDelete: edge use without radial"))?;
        let loop_of = |eu: EdgeuseId| -> Result<LoopId> {
            let lu = eu1::loopuse(b, eu)?
                .ok_or_else(|| null("sm_MarkLoopsToDelete: edge use without loop use"))?;
            loopuse::loop_(b, lu)?
                .ok_or_else(|| null("sm_MarkLoopsToDelete: loop use without loop"))
        };
        let mut rad_l = loop_of(eu_rad)?;
        if loopuse::loop_face(b, rad_l)? == Some(face) {
            rad_l = loop_of(eu)?;
        }
        mark_loops_to_delete_bounded(b, loops_to_delete, rad_l, rec_level + 1, calls)?;
        // recursion level
    }

    if even_level {
        loops_to_delete.push(loop_);
    }
    Ok(())
}

// ---- face UV swap (sm_SwapFaceUV) --------------------------------------------------------

/// `sm_SwapFaceUV`: flips the face's normal (`SmFace::SwapUV`) unless its `mark` is
/// set, and sets the mark. `SwapUV`'s failing status is ignored (upstream's release
/// `SE`), so the face is marked whatever the swap did.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling face; [`SolidError::Unported`] from
/// `SwapUV`'s unported callees; [`SolidError::InvalidInput`] where `SwapUV`
/// dereferences null upstream (a face without surface or face use).
pub fn swap_face_uv(
    k: &Kernel,
    b: &mut Brep,
    marks: &mut Marks,
    face: FaceId,
    mark: MarkType,
) -> Result<()> {
    b.face(face)?;
    // skip faces already marked
    if marks.is_marked(k, Entity::Face(face), mark) {
        return Ok(());
    }
    // swap and mark the face (SE: the status is ignored). The port's InvalidInput marks
    // a null pointer upstream's SwapUV dereferences (undefined there, not a status): it
    // propagates, as this unit's own null dereferences.
    match face1::swap_uv(k, b, face) {
        // face_p1's own InvalidInput is its null-dereference marker; a callee's
        // InvalidInput is that callee's SM_ERR_INVALID_INPUT status, ignored as SE
        Err(SolidError::InvalidInput {
            operation: "topology::face::p1",
            ..
        }) => {
            return Err(SolidError::invalid_input(
                OP,
                "sm_SwapFaceUV: SmFace::SwapUV dereferences a null pointer",
            ));
        }
        r => ignore_status(r)?,
    }
    marks.mark(k, Entity::Face(face), mark);
    Ok(())
}

// ---- UV trim curve check (sm_CheckUVTrimCurve) -----------------------------------------

/// The vertex of an edge use's vertex use (`GetVertexuse()->GetVertex()`).
fn start_vertex(b: &Brep, eu: EdgeuseId) -> Result<VertexId> {
    let vu = eu1::vertexuse(b, eu)?.ok_or_else(|| null("edge use without vertex use"))?;
    b.vertexuse(vu)?
        .vertex
        .ok_or_else(|| null("vertex use without vertex"))
}

/// `sm_CheckUVTrimCurve`: checks the edge use's own UV trim curve (not its mate's):
/// at both ends the surface point of the trim curve must be within the face tolerance
/// plus the vertex tolerance of the vertex, and within the face tolerance plus the
/// edge tolerance of the edge curve's end. Nothing is checked without a trim curve.
///
/// # Errors
///
/// `SM_ERR` when a distance is too large; [`SolidError::InvalidInput`] where upstream
/// dereferences null (no edge, curve, mate, vertex use, vertex, face or surface);
/// those of the evaluations.
pub fn check_uv_trim_curve(k: &Kernel, b: &Brep, eu: EdgeuseId) -> Result<()> {
    let _ = k;
    let Some(trim_crv) = eu1::uv_trim_curve_pointer(b, eu)? else {
        return Ok(());
    };

    // 'Start' will be the start of pTrimCrv, same as start of Edge and Edge curve.
    let reverse = eu1::orientation(b, eu)? == OrientType::Opposite;
    let mate = eu1::mate(b, eu)?;

    // Trim curve should start and end right at Edge ends. (GetEnds and the point
    // evaluations below are not status-checked upstream: a failure leaves the points
    // unset; the port uses undefined coordinates there.)
    let (start_uv, end_uv) =
        ignored(trim_crv.ends())?.map_or((Point3::UNDEF, Point3::UNDEF), |e| (e.start, e.end));

    let e = eu1::edge(b, eu)?.ok_or_else(|| null("sm_CheckUVTrimCurve: edge use without edge"))?;
    let edge_ivl = edge::interval(b, e)?;
    let curve = b
        .edge(e)?
        .curve
        .as_ref()
        .ok_or_else(|| null("sm_CheckUVTrimCurve: edge without curve"))?;
    let edge_3d_start =
        ignored(curve.evaluate_point(edge_ivl.evaluate(0.0)))?.unwrap_or(Point3::UNDEF);

    let mate_vertex = |b: &Brep| -> Result<VertexId> {
        start_vertex(
            b,
            mate.ok_or_else(|| null("sm_CheckUVTrimCurve: edge use without mate"))?,
        )
    };
    let start_vtx = if reverse {
        mate_vertex(b)?
    } else {
        start_vertex(b, eu)?
    };
    let vrtx_3d_start = vertex::point(b, start_vtx)?;

    let face =
        eu1::face(b, eu)?.ok_or_else(|| null("sm_CheckUVTrimCurve: edge use without face"))?;
    let surf = face2::surface(b, face)?
        .ok_or_else(|| null("sm_CheckUVTrimCurve: face without surface"))?;
    let face_3d_start =
        ignored(surf.evaluate_point(Point2::new(start_uv.x, start_uv.y)))?.unwrap_or(Point3::UNDEF);

    let face_tol = face2::tolerance(b, face)?;
    let edge_tol = edge::tolerance(b, e)?;
    let mut vrtx_tol = vertex::tolerance(b, start_vtx)?;

    let dist_face_vrtx_start = face_3d_start.distance(vrtx_3d_start);
    if dist_face_vrtx_start > face_tol + vrtx_tol {
        return Err(sm_err());
    }
    let dist_face_edge_start = face_3d_start.distance(edge_3d_start);
    if dist_face_edge_start > face_tol + edge_tol {
        return Err(sm_err());
    }

    // Now the other end:
    let edge_3d_end =
        ignored(curve.evaluate_point(edge_ivl.evaluate(1.0)))?.unwrap_or(Point3::UNDEF);
    let end_vtx = if reverse {
        start_vertex(b, eu)?
    } else {
        mate_vertex(b)?
    };
    let vrtx_3d_end = vertex::point(b, end_vtx)?;
    let face_3d_end =
        ignored(surf.evaluate_point(Point2::new(end_uv.x, end_uv.y)))?.unwrap_or(Point3::UNDEF);
    vrtx_tol = vertex::tolerance(b, end_vtx)?; // have to update this to End vtx.

    let dist_face_vrtx_end = face_3d_end.distance(vrtx_3d_end);
    if dist_face_vrtx_end > face_tol + vrtx_tol {
        return Err(sm_err());
    }
    let dist_face_edge_end = face_3d_end.distance(edge_3d_end);
    if dist_face_edge_end > face_tol + edge_tol {
        return Err(sm_err());
    }
    Ok(())
}

// ---- bounding box lock (SmLockBrepBox) --------------------------------------------------

/// `SmLockBrepBox`: locks the B-rep's bounding box for its lifetime
/// (`SmBrep::LockBoundingBox` on construction, `UnLockBoundingBox` on drop).
#[derive(Debug)]
pub struct LockBrepBox<'b> {
    brep: &'b mut Brep,
}

impl<'b> LockBrepBox<'b> {
    /// `SmLockBrepBox(pBrep)`.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: `SmBrep::LockBoundingBox` is unit `query.brep`.
    pub fn new(k: &Kernel, brep: &'b mut Brep) -> Result<Self> {
        let _ = k;
        // UNPORTED(query.brep): SmBrep::LockBoundingBox
        let _unlocked = Self { brep };
        Err(SolidError::unported(
            "SmBrep::LockBoundingBox",
            "query.brep",
        ))
    }

    /// The locked B-rep.
    #[must_use]
    pub fn brep(&mut self) -> &mut Brep {
        self.brep
    }
}

impl Drop for LockBrepBox<'_> {
    fn drop(&mut self) {
        // UNPORTED(query.brep): SmBrep::UnLockBoundingBox (nothing to unlock until
        // `new` can lock)
    }
}

// ---- calls into unit topology.edgeuse_p3 ------------------------------------------------

/// `SmEdgeuse::NormalizedEvaluate(t, bParameterSpaceEval, rPoint, pOptFirstDeriv)`
/// (`topology::edgeuse::p3::normalized_evaluate`; header default
/// `bOKToMakeUVTrimCurve = TRUE`): `rPoint`. `want_first_deriv` is
/// `pOptFirstDeriv != NULL` (the derivative itself is unused by every caller here).
fn normalized_evaluate(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    t: f64,
    parameter_space_eval: bool,
    want_first_deriv: bool,
) -> Result<Point3> {
    Ok(eu3::normalized_evaluate(k, b, eu, t, parameter_space_eval, want_first_deriv, true)?.point)
}

/// `SmRegion::GetShells(rShells, pOptAttributeId)` (`Region::get_shells`, unit
/// `topology.shell`): the region's shell list, those carrying `attribute` when given.
fn region_shells(
    b: &Brep,
    region: RegionId,
    attribute: Option<&AttributeFilter<'_>>,
) -> Result<Vec<ShellId>> {
    b.region(region)?
        .get_shells(attribute.map(|f| f.attribute_id), |shell, _| {
            Ok(attribute.is_some_and(|f| f.matches(EntityRef::Shell(shell))))
        })
}

/// `SmRegion::SetIsVoid(flag)` (`Region::set_is_void`): the raw `SmBoolean` flag.
fn region_set_is_void(b: &mut Brep, region: RegionId, is_void: i32) -> Result<()> {
    b.region_mut(region)?.set_is_void(is_void);
    Ok(())
}

/// `SmRegion::IsVoid()` (`Region::is_void`): the raw `SmBoolean` flag (2 is `UNSURE`).
fn region_is_void(b: &Brep, region: RegionId) -> Result<i32> {
    Ok(b.region(region)?.is_void())
}

/// `SmRegion::SetList(shell)` (`SmOwningTopology::SetList`, a header inline): makes
/// `shell` the head of the region's circular shell list (a rotation of the typed list).
fn region_set_list(b: &mut Brep, region: RegionId, shell: ShellId) -> Result<()> {
    let shells = &mut b.region_mut(region)?.shells;
    let at = shells
        .iter()
        .position(|&s| s == shell)
        .ok_or_else(|| null("SmRegion::SetList: the shell is not on the region's list"))?;
    shells.rotate_left(at);
    Ok(())
}

/// `SmShell::GetRegion()`: the shell's `m_pListOwner` (co-ported as `Shell::region`).
fn shell_region(b: &Brep, shell: ShellId) -> Result<Option<RegionId>> {
    Ok(b.shell(shell)?.region)
}

/// `SmShell::GetMatedShells(rShells)` (`Shell::mated_shells`, unit `topology.shell`:
/// claims a mark).
fn shell_mated_shells(k: &mut Kernel, b: &Brep, shell: ShellId) -> Result<Vec<ShellId>> {
    b.shell(shell)?.mated_shells(k, b)
}

/// `SmBrep::CalculateBoundingBox(rBox, bApprox, bTight, bExpandByZoneTol3d)` (unit
/// `query.brep`).
fn brep_bounding_box(
    b: &Brep,
    approx: bool,
    tight: bool,
    expand_by_zone_tol_3d: bool,
) -> Result<Extent3d> {
    let _ = (b, approx, tight, expand_by_zone_tol_3d);
    // UNPORTED(query.brep): SmBrep::CalculateBoundingBox
    Err(SolidError::unported(
        "SmBrep::CalculateBoundingBox",
        "query.brep",
    ))
}

/// `SmEdgeuse::GetOrCreateUVTrimCurve(rpUVTrimCurve, pdOptDistToSurf, pdOptDeviation,
/// bUVSpaceOkay)` (`topology::edgeuse::p3::get_or_create_uv_trim_curve`; header defaults
/// `NULL`, `NULL`, `TRUE`): the returned status, `None` for `SM_SUCCESS`. The curve
/// upstream hands back in `rpUVTrimCurve` (also alongside a failure status) is the edge
/// use's or its mate's attached one afterwards, [`eu1::uv_trim_curve`]. `want_dist` /
/// `want_deviation` are `pdOpt... != NULL`. `Err` only where upstream has no status
/// (`Unported`, dangling ids, null dereferences).
fn get_or_create_uv_trim_curve(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    want_dist: bool,
    want_deviation: bool,
    uv_space_okay: bool,
) -> Result<Option<SolidError>> {
    Ok(
        eu3::get_or_create_uv_trim_curve(k, b, eu, want_dist, want_deviation, uv_space_okay)?
            .status
            .err(),
    )
}

/// `SmEdgeuse::EvaluateBinormal(dParameter, bUse3DOnly, rBinormalPoint, rBinormal,
/// pOptEdgeuseTangent, pOptFaceuseNormal)` (`topology::edgeuse::p3::evaluate_binormal`;
/// header defaults `pOptUVParameter = pOptUVDir = NULL`).
fn evaluate_binormal(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    use_3d_only: bool,
) -> Result<BinormalEval> {
    eu3::evaluate_binormal(
        k,
        b,
        eu,
        parameter,
        use_3d_only,
        &BinormalOptions::default(),
    )
}

/// `EvaluateBinormal(dParameter, TRUE, rPos, rBinormal, &rTangent, &rNormal)` with its
/// status ignored, as `CreateEdgeEdgeBlend` calls it: the outputs upstream goes on with.
/// The out-params start as `SM_UNDEF_DOUBLE` (`SmVector3d`'s default). `EvaluateBinormal`
/// writes them all only on success; its one status before its "Init outputs" step (not
/// a loop edge use) leaves them undefined, every later one leaves the point, tangent and
/// normal zeroed and the binormal undefined. `Err` for [`SolidError::Unported`] and for
/// `topology.edgeuse_p3`'s own [`SolidError::InvalidInput`] (an upstream null dereference).
fn evaluate_binormal_status_ignored(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
) -> Result<BinormalEval> {
    let undefined = BinormalEval {
        point: Vec3::UNDEF,
        binormal: Vec3::UNDEF,
        tangent: Vec3::UNDEF,
        face_normal: Vec3::UNDEF,
        uv: Point2::UNDEF,
    };
    if !eu1::is_loop_edgeuse(b, eu)? {
        return Ok(undefined); // SER(SM_ERR_INVALID_INPUT) before the outputs are set
    }
    match evaluate_binormal(k, b, eu, parameter, true) {
        Ok(eval) => Ok(eval),
        Err(e) if e.is_unported() => Err(e),
        Err(
            e @ SolidError::InvalidInput {
                operation: "topology::edgeuse::p3",
                ..
            },
        ) => Err(e),
        // a status after "Init outputs": point, tangent and normal set to zero
        Err(_) => Ok(BinormalEval {
            point: Vec3::new(0.0, 0.0, 0.0),
            tangent: Vec3::new(0.0, 0.0, 0.0),
            face_normal: Vec3::new(0.0, 0.0, 0.0),
            ..undefined
        }),
    }
}

// ---- curve side (sm_FindSideOfCurve) ----------------------------------------------------

/// `sm_FindSideOfCurve`: projects `point` onto `curve` along `projection_direction`
/// and tells whether the point lies on the right-hand side of the curve for an observer
/// on the positive side of the projection plane walking along the curve tangent.
///
/// # Errors
///
/// `SM_ERR` when the projection does not give exactly one solution, the point is on
/// the curve (within `tol`), the tangent is along the projection direction, or the
/// projected vector and tangent are parallel (upstream also leaves `UNSURE`); those of
/// the solve and evaluation.
pub fn find_side_of_curve(
    k: &Kernel,
    curve: &Curve,
    point: Point3,
    projection_direction: Vec3,
    tol: f64,
) -> Result<bool> {
    // Project Point onto Curve along the Projection direction
    let vectors = [projection_direction];
    let solutions = curve.global_point_solve::<NoObject>(
        k,
        &curve.natural_interval(),
        SolverOperation::ProjectedMinimize,
        point,
        EFF_ZERO_SQRT,
        SolutionRequested::Single,
        &GlobalPointSolveOptions {
            target_distance: None,
            opt_vectors: Some(&vectors),
            handle: None,
            analytic_handle: None,
        },
    )?;

    // Determine if point is on left or right side of the curve relative to the projection plane.
    if solutions.len() != 1 {
        return Err(sm_err());
    }
    let sol = solutions.get(0).ok_or_else(sm_err)?;

    let pv = curve.evaluate(sol.start.param(0)?, 1, true, true)?;
    let (p0, p1) = match pv.as_slice() {
        [p0, p1, ..] => (*p0, *p1),
        _ => return Err(null("sm_FindSideOfCurve: evaluation without tangent")),
    };
    let mut vec = point - p0;

    // Project vectors into projection plane
    vec = projection_direction.cross(vec).cross(projection_direction);

    // point is on the curve
    if vec.length() < tol {
        return Err(sm_err());
    }

    vec = vec.unitize_or_z();
    let mut tan = projection_direction.cross(p1).cross(projection_direction);

    // projection direction is the same as the curve tangent or curve tangent is zero
    if tan.length() < tol {
        return Err(sm_err());
    }
    tan = tan.unitize_or_z();

    // projected vec and curve tangent are parallel
    let side = tan.cross(vec);
    if side.length() < EFF_ZERO {
        return Err(sm_err());
    }

    // set output
    Ok(side.dot(projection_direction) <= 0.0)
}

// ---- face sectors (sm_FindSectorOfFace) -------------------------------------------------

fn face_surface(b: &Brep, face: FaceId) -> Result<&Surface> {
    face2::surface(b, face)?.ok_or_else(|| null("face without surface"))
}

/// `sm_FindSectorOfFace`: the region on the side of `face` a test point (known to be
/// near an intersection with the face) lies in, and its distance from the face. With
/// `uv_guess` (a point on the face) the side is taken from `test_dir` unless it is
/// tangent to the face, where (as without a guess) the test point is dropped to the
/// surface and the side taken from the drop vector.
///
/// # Errors
///
/// `SM_ERR` when the global drop finds nothing or the test point is on the surface;
/// [`SolidError::InvalidInput`] where upstream dereferences null (no surface, face
/// use, mate or shell; a shell without region returns `None`, as upstream); those of
/// the surface evaluations and solves. Deviation: without a guess upstream turns the
/// surface cache's point test off (`SmTemporaryChangeValue` on `m_bPointTestEnabled`)
/// around `GlobalPointSolve`; the Rust cache has no such switch, so for a face-owned
/// surface the solve defers on `SmFace::PointClassify` (unit `query.face_p2`, see
/// `surface::surface::p4`) where upstream keeps every solution.
pub fn find_sector_of_face(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    uv_guess: Option<Point2>,
    test_point: Point3,
    test_dir: Vec3,
) -> Result<(f64, Option<RegionId>)> {
    let surface = face_surface(b, face)?;
    let eval = |uv: Point2| -> Result<(Point3, Vec3)> {
        // found Surface Point, Found Surface Point Normal
        let p = surface.evaluate_point(uv)?;
        let n = surface.evaluate_normal(uv, Side::Left, Side::Left)?;
        Ok((p, n))
    };
    let uv_of = |start: &crate::geom::solution_array::SolutionEnd| -> Result<Point2> {
        Ok(Point2::new(start.param(0)?, start.param(1)?))
    };

    // Get SurfacePoint, SurfaceNormal, rdDistance, and sTestVec
    let (surf_pnt, norm, test_vec) = if let Some(guess) = uv_guess {
        {
            // assume that the given cpUVGuess is on the face as expected
            let (surf_pnt, norm) = eval(guess)?;

            // Can't classify a point on the surface - that point won't be in a region sector
            // (upstream passes SM_EFF_ZERO_RAD to the degree tolerance of IsPerpendicularTo)
            if norm.is_perpendicular_to(test_dir, EFF_ZERO_RAD) {
                // try old method of dropping a point to the surface

                // with cpUVGuess do a cheap local solve
                let solve = surface.local_point_solve::<NoObject>(
                    k,
                    &face2::uv_domain(b, face)?,
                    SolverOperation::Minimize,
                    test_point,
                    guess,
                )?;
                let uv_found = if solve.found {
                    uv_of(&solve.solution.start)?
                } else {
                    guess
                };
                let (surf_pnt, norm) = eval(uv_found)?;
                (surf_pnt, norm, test_point - surf_pnt)
            } else {
                (surf_pnt, norm, test_dir)
            }
        }
    } else {
        {
            // without cpUVGuess do a more expensive global solve (point testing off:
            // every point solution is kept without classifying it against the trims)
            let solutions = surface.global_point_solve::<NoObject>(
                k,
                Some(NoObject),
                &face2::uv_domain(b, face)?,
                SolverOperation::Minimize,
                test_point,
                EFF_ZERO,
                None,
                SolutionRequested::All,
            )?;
            let Some(first) = solutions.get(0) else {
                return Err(sm_err());
            };
            let uv_found = uv_of(&first.start)?;
            let (surf_pnt, norm) = eval(uv_found)?;
            (surf_pnt, norm, test_point - surf_pnt)
        }
    };

    // size of sTestVec (From sSurfPnt to crTestPoint or crTestDir)
    let distance = test_vec.length();

    // Can't classify a point on the surface - that point won't be in a region sector
    if distance < EFF_ZERO {
        return Err(sm_err());
    }

    // classify testPoint based on angle between FacePointNormal and TestVec
    let mut fu = face2::upward_faceuse(b, face)?
        .ok_or_else(|| null("sm_FindSectorOfFace: face without face use"))?;
    // (upstream: `if (dot > 0.0) up else mate`, so a NaN dot product takes the mate)
    let up = norm.dot(test_vec) > 0.0;
    if !up {
        fu = eu1::faceuse_mate(b, fu)?
            .ok_or_else(|| null("sm_FindSectorOfFace: face use without mate"))?;
    }
    let shell = eu1::faceuse_shell(b, fu)?
        .ok_or_else(|| null("sm_FindSectorOfFace: face use without shell"))?;
    let region = eu1::shell_region(b, shell)?;
    let _ = surf_pnt;
    Ok((distance, region))
}

// ---- vertex edge intervals (SmBrep::UpdateVertexEdgeIntervals) ---------------------------

/// `UpdateVertexEdgeIntervals`: moves the end of every edge at `vertex` to the drop of
/// the vertex point onto the edge curve when that is closer than the current end
/// (notifying the edge of the pre-edit and rebuilding the UV trim curves of its edge
/// use pairs), then sets the vertex tolerance to twice the largest remaining gap.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge without curve (upstream dereferences it);
/// those of the evaluations, drops and notifications.
pub fn update_vertex_edge_intervals(
    k: &Kernel,
    b: &mut Brep,
    env: &mut edge::NotifyEnv<'_>,
    vertex: VertexId,
) -> Result<()> {
    let mut max_dist = 0.0;
    let brep_ref = ObjectRef {
        type_code: TYPE,
        key: b.key(),
    };

    // for every vertex edge
    for e in vertex::edges(b, vertex, None)? {
        // locals
        let curve = b
            .edge(e)?
            .curve
            .clone()
            .ok_or_else(|| null("UpdateVertexEdgeIntervals: edge without curve"))?;
        let vertex_point = vertex::point(b, vertex)?;
        let vertex_tol = vertex::tolerance(b, vertex)?;
        let interval = edge::interval(b, e)?;
        let mut edited_edge = false;

        let drop = |t: f64| -> Result<crate::curve::curve::p6::DroppedPoint> {
            // drop vertex point to curve (cheap LocalPointSolve with the guess)
            curve.drop_point(
                k,
                &curve.natural_interval(),
                vertex_point,
                None,
                vertex_tol,
                Some(t),
                SolverOperation::Minimize,
            )
        };
        let mut orig_dist;
        if edge::start_vertex(b, e)? == Some(vertex) {
            // eval edge's start point
            let min_t = interval.min();
            let pnt = curve.evaluate_point(min_t)?;

            // get edgeStart to vertex distance
            orig_dist = pnt.distance(vertex_point);

            let d = drop(min_t)?;

            // when drop point is better choice than current edgeStart point
            if d.distance < orig_dist && d.parameter < interval.max() - EFF_ZERO {
                // reset the curve interval
                let edge_ref = (env.key_of)(EntityRef::Edge(e));
                ignored(edge::notify(
                    b,
                    e,
                    env,
                    NotifyOperation::PreEdit,
                    [Some(edge_ref), Some(brep_ref), None],
                ))?; // (status not checked upstream)
                edited_edge = true;
                let ivl = Extent1d::from_min_max(d.parameter, interval.max());
                edge::set_interval(b, e, &ivl)?;
                orig_dist = d.distance;
            }
        } else {
            // (SM_ASSERT: the edge's other vertex is the target vertex)
            // eval EdgeEnd point
            let max_t = interval.max();
            let pnt = curve.evaluate_point(max_t)?;

            // get EdgeEnd to Vertex distance
            orig_dist = pnt.distance(vertex_point);

            let d = drop(max_t)?;

            // when drop point is better than current edgeEnd
            if d.distance < orig_dist && d.parameter > interval.min() + EFF_ZERO {
                // retset the edge's interval
                let edge_ref = (env.key_of)(EntityRef::Edge(e));
                ignored(edge::notify(
                    b,
                    e,
                    env,
                    NotifyOperation::PreEdit,
                    [Some(edge_ref), Some(brep_ref), None],
                ))?; // (status not checked upstream)
                edited_edge = true;
                let ivl = Extent1d::from_min_max(interval.min(), d.parameter);
                edge::set_interval(b, e, &ivl)?;
                orig_dist = d.distance;
            }
        }

        // when edge's interval was modified
        if edited_edge {
            // for every edgeuse mated pair
            let eus = b.edge(e)?.edgeuses.clone();
            for &eu in eus.iter().step_by(2) {
                // for edges with UVTrimCurves - rebuild UVTrimCurve
                if eu1::uv_trim_curve(b, eu)?.0.is_some() {
                    // TRUE = delete existing UVTrimCurve (statuses not checked upstream)
                    ignored(eu1::set_uv_trim_curve(
                        b,
                        eu,
                        eu1::UvTrimCurveArg::Null,
                        0.0,
                        true,
                        None,
                    ))?;
                    let _ignored_status =
                        get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
                }
            }
        }

        max_dist = max2(max_dist, orig_dist);
    }

    vertex::set_tolerance(b, vertex, max_dist * 2.0, true, true)?;
    Ok(())
}

// ---- seam UV trim curves (sm_RedistributeSeamUVTrimCurves) -------------------------------

/// `sm_RedistributeSeamUVTrimCurves`: after a new seam edge (`uv_trim_curve` given,
/// `seam_edge` a seam direction) has been connected to its vertices, checks that the
/// UV trim curves on either side of the seam continue their predecessors' side and
/// swaps them between the two mated pairs when not. Returns whether they were swapped.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] where upstream reads past the edge use list (fewer
/// than four edge uses) or dereferences null; those of the UV trim curve creation,
/// the singularity test and the trim curve swap.
pub fn redistribute_seam_uv_trim_curves(
    k: &Kernel,
    b: &mut Brep,
    new_edge: EdgeId,
    uv_trim_curve: Option<&Curve>,
    seam_edge: SurfParamType,
    srf_domain: &Extent2d,
) -> Result<bool> {
    // when NewEdge has UVTrimCurves and NewEdge is a SeamEdge - see if UVTrimCurves on either
    // side of seam are properly distributed
    if uv_trim_curve.is_none() || seam_edge == SurfParamType::Neither {
        return Ok(false);
    }
    let eus = b.edge(new_edge)?.edgeuses.clone(); // SM_ASSERT(4 edge uses)
                                                  // sEdgeuse[0] = Starts At StartVertex OneFace, sEdgeuse[3] = Starts At EndVertex OtherFace
    let this_eu = *eus
        .first()
        .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: no edge uses"))?;
    let other_eu = *eus
        .get(3)
        .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: fewer than 4 edge uses"))?;
    let prev_this = eu1::cw_edgeuse(b, this_eu)?
        .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: edge use without CW"))?;
    let prev_other = eu1::cw_edgeuse(b, other_eu)?
        .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: edge use without CW"))?;

    // GWC: Check if we need to make sure the PrevEdgeuses have UVTrimCurves for this to work?
    // (the statuses and curves are only SM_ASSERTed upstream)
    let _this_status = get_or_create_uv_trim_curve(k, b, prev_this, false, false, true)?;
    let _other_status = get_or_create_uv_trim_curve(k, b, prev_other, false, false, true)?;

    // Find the UV point for the two new starting Vertexuses (statuses not checked
    // upstream: a failed evaluation leaves the point unset; undefined here; the first
    // derivatives are requested but unused upstream)
    // NewE Start UV values from UVTrimCurves
    let this_point_uv =
        ignored(normalized_evaluate(k, b, this_eu, 0.0, true, true))?.unwrap_or(Point3::UNDEF);
    let other_point_uv =
        ignored(normalized_evaluate(k, b, other_eu, 0.0, true, true))?.unwrap_or(Point3::UNDEF);
    // Prev Start UV values from PrevUVtrimCurves
    let prev_this_point_uv =
        ignored(normalized_evaluate(k, b, prev_this, 1.0, true, true))?.unwrap_or(Point3::UNDEF);
    let prev_other_point_uv =
        ignored(normalized_evaluate(k, b, prev_other, 1.0, true, true))?.unwrap_or(Point3::UNDEF);

    // make sure sThisPointUV == sPrevThisPointUV, sOtherPointUV == sPrevOtherPointUV are
    // the same to tol by switching the UVTrimCurves if needed. Expect all 4 PointUVs to
    // lie on one side or the other of the Seam.
    // pick large bndry search tolerance - just need to know which side of the seam the
    // UVPoints lie.
    let tol_u = srf_domain.x_length() / 4.0;
    let tol_v = srf_domain.y_length() / 4.0;
    // switch to SmPoint2d objs - yuk
    let this_point_2d = Point2::new(this_point_uv.x, this_point_uv.y);
    let prev_this_point_2d = Point2::new(prev_this_point_uv.x, prev_this_point_uv.y);
    let other_point_2d = Point2::new(other_point_uv.x, other_point_uv.y);
    let prev_other_point_2d = Point2::new(prev_other_point_uv.x, prev_other_point_uv.y);
    // classify the 4 UVPoints
    let (this_u, this_v) = srf_domain.classify_point(this_point_2d, tol_u, Some(tol_v));
    let (prev_this_u, prev_this_v) =
        srf_domain.classify_point(prev_this_point_2d, tol_u, Some(tol_v));
    let (_other_u, _other_v) = srf_domain.classify_point(other_point_2d, tol_u, Some(tol_v)); // (debug asserts only)
    let (prev_other_u, prev_other_v) =
        srf_domain.classify_point(prev_other_point_2d, tol_u, Some(tol_v));
    let this_seam_edge = match seam_edge {
        SurfParamType::UMin | SurfParamType::UMax => SurfParamType::U,
        SurfParamType::VMin | SurfParamType::VMax => SurfParamType::V,
        other => other,
    };
    let face = eu1::face(b, this_eu)?
        .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: edge use without face"))?;
    let surface = face2::surface(b, face)?
        .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: face without surface"))?;
    // (IsSingularity is a boolean: a failing evaluation reads TRUE upstream)
    let this_pole = ignored(surface.is_singularity(k, this_point_2d, EFF_ZERO, false))?
        .is_none_or(|sg| sg.singular);
    let other_pole = ignored(surface.is_singularity(k, other_point_2d, EFF_ZERO, false))?
        .is_none_or(|sg| sg.singular);

    // decide if we have to swap UV curves
    let mut swap = (this_seam_edge == SurfParamType::U
        && this_u != prev_this_u
        && this_u == prev_other_u)
        || (this_seam_edge == SurfParamType::V && this_v != prev_this_v && this_v == prev_other_v);

    // A pole maps to an entire UV boundary. Its neighboring trim endpoint cannot select
    // the seam side. As in CreateUVTrimCurve, compare the inward binormal with the
    // cross-boundary derivative away from the poles.
    if this_pole || other_pole {
        let parameter = edge::interval(b, new_edge)?.evaluate(0.48765); // perturb off midpoint for evaluation
        let this_uv_trim_curve = eu1::uv_trim_curve(b, this_eu)?.0.ok_or_else(|| {
            null("sm_RedistributeSeamUVTrimCurves: edge use without UV trim curve")
        })?;
        // (the three statuses are tested, not propagated)
        let uv = ignored(this_uv_trim_curve.evaluate_point(parameter))?;
        let binormal = match uv {
            Some(_) => ignored(evaluate_binormal(k, b, this_eu, parameter, true))?,
            None => None,
        };
        if let (Some(uv), Some(BinormalEval { binormal, .. })) = (uv, binormal) {
            // (the face surface fetched above; re-borrowed past the binormal evaluation)
            let surface = face2::surface(b, face)?
                .ok_or_else(|| null("sm_RedistributeSeamUVTrimCurves: face without surface"))?;
            if let Some((_surface_point, du, dv)) =
                ignored(surface.evaluate_1st_derivatives(Point2::new(uv.x, uv.y), true, true))?
            {
                let across = if this_seam_edge == SurfParamType::U {
                    du
                } else {
                    dv
                };
                let side = if this_seam_edge == SurfParamType::U {
                    this_u
                } else {
                    this_v
                };
                let inward = across.dot(binormal);
                // The derivative points into the domain at its minimum and out at its
                // maximum. The edgeuse's binormal must point inside.
                swap = (side == ExtentPointType::Start && inward < 0.0)
                    || (side == ExtentPointType::End && inward > 0.0);
            }
        }
    }

    // When we need to swap the UVCurves to get the connected UVPoints on the same side
    // of the Seam
    if swap {
        // note: SetUVTrimCurve sets EU->m_pUVTrimCurve and m_pUVTrimCurve->Owner
        // (`set_member_uv_trim_curve` takes the curve by value: a mated pair without
        // one, which upstream would clear with NULL, is reported as a null dereference)
        let this_uv_trim_curve = eu1::uv_trim_curve(b, this_eu)?.0.cloned().ok_or_else(|| {
            null("sm_RedistributeSeamUVTrimCurves: edge use without UV trim curve")
        })?;
        let other_uv_trim_curve = eu1::uv_trim_curve(b, other_eu)?.0.cloned().ok_or_else(|| {
            null("sm_RedistributeSeamUVTrimCurves: edge use without UV trim curve")
        })?;
        let this_drop_curve_fail = eu1::drop_curve_fail(b, this_eu)?.0.cloned();
        let other_drop_curve_fail = eu1::drop_curve_fail(b, other_eu)?.0.cloned();
        // connect ThisEdgeuse MatedPair set with OtherUVTrimCurve
        eu1::set_member_uv_trim_curve(b, this_eu, other_uv_trim_curve, other_drop_curve_fail)?;
        // connect OtherEdgeuse MatedPair set with ThisUVTrimCurve
        eu1::set_member_uv_trim_curve(b, other_eu, this_uv_trim_curve, this_drop_curve_fail)?;
        // inform the public
        return Ok(true);
    }

    // all done
    Ok(false)
}

// ---- vertex split UV trim curves (sm_MakeVertexSplitUVTrimCurve) -------------------------

/// The `SmBSplineCurve` part of a B-spline-derived curve (`SM_CAST_PTR(SmBSplineCurve,
/// ..)`), `None` for other kinds.
fn bspline_part(curve: &Curve) -> Option<BSplineCurve> {
    match curve.kind() {
        CurveKind::BSpline(c) => Some(c.clone()),
        CurveKind::Line(c) => Some(c.base.clone()),
        CurveKind::Ellipse(c) => Some(c.base().clone()),
        CurveKind::Hyperbola(c) => Some(c.base().clone()),
        CurveKind::Parabola(c) => Some(c.base().clone()),
        _ => None,
    }
}

/// The neighbour surface `SmBSplineSurface::CreateCornerBlend` gets: the
/// `SmBSplineSurface` part of a surface (`SmPlane` and `SmSurfOfRevolution` are B-splines
/// upstream).
fn bspline_surface(surface: &Surface) -> Result<&BSplineSurface> {
    match surface.kind() {
        SurfaceKind::BSpline(s) => Ok(s),
        SurfaceKind::Plane(p) => Ok(p.base()),
        SurfaceKind::SurfOfRevolution(s) => Ok(s.base()),
        // Upstream passes any SmSurface*; the port of SmBSplineSurface::CreateCornerBlend
        // takes B-spline neighbours only (its Deviations): the other kinds are that
        // partial unit's own unported part.
        // UNPORTED(nurbs.b_spline_surface_n_lib_p2): SmBSplineSurface::CreateCornerBlend with a non-B-spline neighbour surface
        _ => Err(SolidError::unported(
            "SmBSplineSurface::CreateCornerBlend (non-B-spline neighbour SmSurface)",
            "nurbs.b_spline_surface_n_lib_p2",
        )),
    }
}

/// `sm_MakeVertexSplitUVTrimCurve` (a helper of `SmBrep::MakeVertexSplitEdge`): splits
/// the UV trim curve of `eu_orig` at `edge_split_parameter` into the parts of the
/// original and the new edge use (`eu_new`), reparameterised to the two halves of the
/// edge interval; the trim curve is deleted instead when the split is within
/// `scaled_zero` of its ends or its 3D image is farther than a hundredth of the edge
/// tolerance from `split_pnt` (unless `for_tesselation` re-projects the split point and
/// finds it). `orient` says which half the original edge use keeps.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` for an edge use without face or a face without surface
/// (`NER`); [`SolidError::InvalidInput`] for an edge use without edge; those of the
/// copy, evaluation, solve, trim and reparameterisation; [`SolidError::Unported`] for a
/// face surface that is not a B-spline or plane.
#[allow(clippy::too_many_arguments)]
pub fn make_vertex_split_uv_trim_curve(
    k: &Kernel,
    b: &mut Brep,
    eu_orig: EdgeuseId,
    eu_new: EdgeuseId,
    split_pnt: Point3,
    edge_split_parameter: f64,
    scaled_zero: f64,
    orient: OrientType,
    for_tesselation: bool,
) -> Result<()> {
    let Some(uv_trim_curve) = eu1::uv_trim_curve_pointer(b, eu_orig)?.cloned() else {
        return Ok(());
    };
    // when edgeuse has a UVTrimCurve

    // get UVTrimCurve interval
    let mut ivl1 = uv_trim_curve.natural_interval();

    // when SplitParameter is near UVTrimCurve->Ends
    if edge_split_parameter < ivl1.min() + scaled_zero
        || edge_split_parameter > ivl1.max() - scaled_zero
    {
        // (SetUVTrimCurve statuses are not checked upstream)
        ignored(eu1::set_uv_trim_curve(
            b,
            eu_orig,
            eu1::UvTrimCurveArg::Null,
            0.0,
            true,
            None,
        ))?;
        return Ok(());
    }
    // split param is far from UVTrimCurve->Ends

    // copy UVTrimCurve
    let mut new_uv_curve = uv_trim_curve.copy(k)?;

    // make a 3dCurve from the UVTrimCurve (SmCrvOnSurf(*pNewUVCurve, *pSurface)): its
    // point evaluation is the UV curve point lifted by the face surface; the Rust
    // SmCrvOnSurf is built over a B-spline surface only, so the lift goes through the
    // face surface itself and the local solve of the tessellation branch needs the
    // B-spline one (DISPATCH GAP as curve.crv_on_surf elsewhere)
    let edge_to_split = eu1::edge(b, eu_orig)?
        .ok_or_else(|| null("sm_MakeVertexSplitUVTrimCurve: edge use without edge"))?;
    let face = eu1::face(b, eu_orig)?.ok_or_else(|| SolidError::null_pointer(OP))?; // NER
    let surface = face2::surface(b, face)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone(); // NER
    let lift = |t: f64| -> Result<Point3> {
        let p = new_uv_curve.evaluate_point(t)?;
        surface.evaluate_point(Point2::new(p.x, p.y))
    };

    // when 3D distance from splitPoint to UVTrimCurvePoint > tol/100 (was "tol/30" [B606])
    let mut edge_split_parameter_uv = edge_split_parameter;
    let p3d = lift(edge_split_parameter_uv)?;

    if p3d.distance(split_pnt) > edge::tolerance(b, edge_to_split)? / 100.0 {
        let mut reuse_trim_curve = false;
        if for_tesselation {
            let SurfaceKind::BSpline(bs) = surface.kind() else {
                // the port of SmCrvOnSurf takes a B-spline surface only: that partial
                // unit's own unported part
                // UNPORTED(curve.crv_on_surf): SmCrvOnSurf::SmCrvOnSurf over a non-B-spline SmSurface
                return Err(SolidError::unported(
                    "SmSurface (non-B-spline) in SmCrvOnSurf",
                    "curve.crv_on_surf",
                ));
            };
            let uv_3d_curve = Curve::from(CrvOnSurf::new(
                k,
                new_uv_curve.clone(),
                bs.clone(),
                None,
                0,
            )?);
            // get best UVTrimCurve param possible by projecting splitPoint to 3d UVTrimCurve
            let (found, sol) = uv_3d_curve.local_point_solve::<NoObject>(
                &new_uv_curve.natural_interval(),
                SolverOperation::Minimize,
                split_pnt,
                &PointSolveOptions {
                    distance_tolerance: None,
                    target_distance: None,
                    vectors: None,
                },
                edge_split_parameter,
            )?;

            // If we have a solution and it is central to the interval, use this trim curve
            if found
                && edge_split_parameter_uv > ivl1.min() + scaled_zero
                && edge_split_parameter_uv < ivl1.max() - scaled_zero
            {
                edge_split_parameter_uv = sol.start.param(0)?;
                reuse_trim_curve = true;
            }
        }

        if !reuse_trim_curve {
            ignored(eu1::set_uv_trim_curve(
                b,
                eu_orig,
                eu1::UvTrimCurveArg::Null,
                0.0,
                true,
                None,
            ))?;
            return Ok(());
        }
    }

    // set up intervals for 3D and UV curve split segments
    //    sIvl1,   sIvl2   = UVTrimCurve [start,SplitParamUV], [SplitParamUV, end]
    //    sIvl13D, sIvl23D = 3DCurve     [start,SplitParam],   [SplitParam,   end]
    let mut ivl2 = ivl1;
    // (the SetMinMax statuses are not checked upstream)
    ignored(ivl1.set_min_max(ivl1.min(), edge_split_parameter_uv))?;
    let ivl1_3d = Extent1d::from_min_max(ivl1.min(), edge_split_parameter);
    ignored(ivl2.set_min_max(edge_split_parameter_uv, ivl2.max()))?;
    let ivl2_3d = Extent1d::from_min_max(edge_split_parameter, ivl2.max());

    // trim and bind UVTrimCurves to Edgeuses (the attached curve is edited in place)
    {
        let attached = b
            .edgeuse_mut(eu_orig)?
            .uv_trim_curve
            .as_mut()
            .ok_or_else(|| null("sm_MakeVertexSplitUVTrimCurve: trim curve vanished"))?;
        if orient == OrientType::Same {
            attached.trim(k, &mut ivl1, TrimMode::Notify)?;
            attached.edit_parameterization(k, &ivl1_3d)?;
            new_uv_curve.trim(k, &mut ivl2, TrimMode::Notify)?;
            new_uv_curve.edit_parameterization(k, &ivl2_3d)?;
        } else {
            attached.trim(k, &mut ivl2, TrimMode::Notify)?;
            attached.edit_parameterization(k, &ivl2_3d)?;
            new_uv_curve.trim(k, &mut ivl1, TrimMode::Notify)?;
            new_uv_curve.edit_parameterization(k, &ivl1_3d)?;
        }
    }

    ignored(eu1::set_uv_trim_curve(
        b,
        eu_new,
        eu1::UvTrimCurveArg::New(Box::new(new_uv_curve)),
        0.0,
        false,
        None,
    ))?;
    Ok(())
}

// ---- B-rep intersection (SmBrep::BrepIntersect) -----------------------------------------

/// `BrepIntersect`: intersects every face of this B-rep with every face of `other`
/// (`SmFace::FaceIntersect`) and returns the 3D curves (duplicates removed) and, when
/// `want_points`, the singular points. Wire edges and shell vertices are not handled.
///
/// # Errors
///
/// [`SolidError::Unported`] from `SmFace::FaceIntersect` (unit `topology.face_p3`, a
/// justified stub: the two faces are in different B-reps) when
/// both B-reps have faces (upstream's `NER` on the faces never fires from arenas);
/// those of the duplicate removal.
pub fn brep_intersect(
    k: &Kernel,
    b: &Brep,
    other: &Brep,
    approx_tol_3d: f64,
    crv_angle_tolerance_deg: f64,
    want_points: bool,
) -> Result<(Vec<Curve>, Option<Vec<Point3>>)> {
    // init output
    let mut curves_3d: Vec<Option<Curve>> = Vec::new();
    let mut points: Option<Vec<Point3>> = want_points.then(Vec::new);

    // get all thisBrep and otherBrep Faces
    let this_faces = faces(b, None)?;
    let other_faces = faces(other, None)?;

    // for every thisBrep Face
    for &this_face in &this_faces {
        // (NER: arena ids are never null)
        // for every otherBrep Face
        for &other_face in &other_faces {
            // intersect the faces
            let (mut curves, mut pts) = face_intersect(
                k,
                b,
                this_face,
                other,
                other_face,
                approx_tol_3d,
                crv_angle_tolerance_deg,
            )?;

            // accumulate the outputs
            curves_3d.append(&mut curves);
            if let Some(points) = points.as_mut() {
                points.append(&mut pts);
            }
        }
    }

    crate::curve::curve::p5::remove_duplicate_curves(&mut curves_3d)?;
    Ok((curves_3d.into_iter().flatten().collect(), points))
}

/// `SmFace::FaceIntersect(crContext, pOtherFace, dApproxTol3d, dCrvAngTolDeg, r3DCurves,
/// NULL, NULL, &sPoints)`: the 3D intersection curves and single-point intersections of
/// two faces.
#[allow(clippy::too_many_arguments)]
fn face_intersect(
    k: &Kernel,
    b: &Brep,
    this_face: FaceId,
    other: &Brep,
    other_face: FaceId,
    approx_tol_3d: f64,
    crv_angle_tolerance_deg: f64,
) -> Result<(Vec<Option<Curve>>, Vec<Point3>)> {
    let _ = (
        k,
        b,
        this_face,
        other,
        other_face,
        approx_tol_3d,
        crv_angle_tolerance_deg,
    );
    // UNPORTED(topology.face_p3): SmFace::FaceIntersect -- justified: findings.md topology.face_p3 "Stubs left for this unit"
    // (the two faces belong to different B-reps; the ported routine takes both from one)
    Err(SolidError::unported(
        "SmFace::FaceIntersect",
        "topology.face_p3",
    ))
}

// ---- corner blends (SmBrep::CreateCornerBlend) ------------------------------------------

/// `CreateCornerBlend`: a corner blend patch over a geometrically connected (unordered)
/// set of edge uses: orders them end to end (growing the match tolerance up to a
/// hundredfold), takes trimmed copies of their curves and, where tangent to the face is
/// wanted (`tangent_to_face`, default all; never for wire edges), their UV trim curves
/// and surfaces, pads two- and three-sided inputs with point curves, and passes the
/// arrays to `SmBSplineSurface::CreateCornerBlend`.
///
/// # Errors
///
/// `SM_ERR` for fewer than two edge uses or a gap in the chain; those of the edge use
/// evaluations; [`SolidError::InvalidInput`]
/// where upstream dereferences null (an opposite edge use without mate, an edge
/// without curve, a tangent edge use without face) or reads past
/// `tangent_to_face`; those of the copies, trims and the blend.
pub fn create_corner_blend(
    k: &Kernel,
    b: &mut Brep,
    edgeuses: &[EdgeuseId],
    tangent_to_face: Option<&[bool]>,
    approx_tol_3d: f64,
    tangency_tol_radians: f64,
) -> Result<Vec<BSplineSurface>> {
    // Need at least two edges to blend
    if edgeuses.len() < 2 {
        return Err(sm_err());
    }
    // locals
    let mut work_eus: Vec<EdgeuseId> = edgeuses.to_vec();
    let mut ordered_eus: Vec<EdgeuseId> = Vec::new();
    let mut eu_orients: Vec<OrientType> = Vec::new();

    // set sWorkTan: Copied from optional input else set to TRUE for all edgeuses
    let mut work_tan: Vec<bool> = match tangent_to_face {
        Some(t) => t.to_vec(),
        None => vec![true; edgeuses.len()],
    };

    // set sWorkEUS = list of all input edgeuses running in the SM_OT_SAME direction
    for i in 0..work_eus.len() {
        let eu = work_eus.get(i).copied().ok_or_else(sm_err)?;
        let eu = if eu1::orientation(b, eu)? == OrientType::Opposite {
            eu1::mate(b, eu)?
                .ok_or_else(|| null("CreateCornerBlend: opposite edge use without mate"))?
        } else {
            eu
        };
        if let Some(slot) = work_eus.get_mut(i) {
            *slot = eu;
        }
        let e =
            eu1::edge(b, eu)?.ok_or_else(|| null("CreateCornerBlend: edge use without edge"))?;
        if edge::is_wire(b, e)? {
            *work_tan
                .get_mut(i)
                .ok_or_else(|| null("CreateCornerBlend: tangent flags shorter than edge uses"))? =
                false;
        }
    }

    // locals for upcoming SmSurface::CreateCornerBlend call
    let mut tangencies: Vec<bool> = vec![*work_tan
        .first()
        .ok_or_else(|| null("CreateCornerBlend: tangent flags shorter than edge uses"))?];
    ordered_eus.push(work_eus.remove(0));
    let first_eu = *ordered_eus.first().ok_or_else(sm_err)?;
    eu_orients.push(eu1::orientation(b, first_eu)?);
    work_tan.remove(0);

    let mut tol_scale = 1.0;

    // While there are sWorkEUS - find the next one sharing an endPoint with the last
    while !work_eus.is_empty() {
        let mut found = false;

        // for every sWorkEUS edgeuse - look for one sharing a common endPoint with the last
        for i in 0..work_eus.len() {
            let eu = *work_eus.get(i).ok_or_else(sm_err)?;
            let tan = *work_tan
                .get(i)
                .ok_or_else(|| null("CreateCornerBlend: tangent flags shorter than edge uses"))?;
            let last_eu = *ordered_eus.last().ok_or_else(sm_err)?;
            let param = if eu_orients.last() == Some(&OrientType::Same) {
                1.0
            } else {
                0.0
            };

            // lastEdgeuse endpoint (3d Eval)
            let end_point = normalized_evaluate(k, b, last_eu, param, false, false)?;

            // TestEdgeuse StartPoint
            let test_pt = normalized_evaluate(k, b, eu, 0.0, false, false)?;

            // when LastEdge EndPoint coincides with TestEdge Start
            if test_pt.distance(end_point) < approx_tol_3d * tol_scale {
                // insert this edge into local inputs in SAME orientation
                tangencies.push(tan);
                ordered_eus.push(eu);
                eu_orients.push(OrientType::Same);
                work_eus.remove(i);
                work_tan.remove(i);
                found = true;
                tol_scale = 1.0;
                break; // go find next connected edgeuse
            }

            // TestEdgeuse EndPoint
            let test_pt = normalized_evaluate(k, b, eu, 1.0, false, false)?;

            // When LastEdge EndPoint coincides with TestEdge End
            if test_pt.distance(end_point) < approx_tol_3d * tol_scale {
                // insert this edge into local inputs in OPPOSITE orientation
                tangencies.push(tan);
                ordered_eus.push(eu);
                eu_orients.push(OrientType::Opposite);
                work_eus.remove(i);
                work_tan.remove(i);
                found = true;
                tol_scale = 1.0;
                break; // go find next connected edgeuse
            }
        }

        // arrive here when none of the remaining edges connect to the growing chain within current tol

        // exit error: A gap in the input edgeuses
        if !found && tol_scale > 100.0 {
            return Err(sm_err());
        }

        // arrive here to try again - increase check tolerance and try searching the edgeuses again.
        if !found {
            tol_scale *= 2.5;
        }
    }

    // SmSurface::CreateCornerBlend locals
    let mut ordered_uv: Vec<Option<BSplineCurve>> = Vec::new();
    let mut ordered_3d: Vec<BSplineCurve> = Vec::new();
    let mut ordered_surfs: Vec<Option<BSplineSurface>> = Vec::new();

    // for every ordered Edgeuse - load a trimmed copy of Edgeuse->Edge->Curve to sOrdered3D
    //   and when tangency to adjacent surfaces are needed build sOrderedUV and sOrderedSurf arrays
    for (i, &eu) in ordered_eus.iter().enumerate() {
        let e =
            eu1::edge(b, eu)?.ok_or_else(|| null("CreateCornerBlend: edge use without edge"))?;
        let curve = b
            .edge(e)?
            .curve
            .as_ref()
            .ok_or_else(|| null("CreateCornerBlend: edge without curve"))?;

        // copy Edgeuse->Edge->Curve, trim the copy (may snap sIvl by tol to existing knots)
        let mut copy = curve.copy(k)?;
        let mut e_ivl = edge::interval(b, e)?;
        copy.trim(k, &mut e_ivl, TrimMode::Notify)?;

        // build the sOrdered3D array
        ordered_3d.push(
            bspline_part(&copy)
                .ok_or_else(|| null("CreateCornerBlend: curve without NURBS data"))?,
        );

        // When Needed, put in UVTrimCurve and Surface in sOrderedUV and sOrderedSurfs arrays
        let tangent = tangencies.get(i).copied().unwrap_or(true);
        if tangent {
            // get Edgeuse->UVTrimCurve (status ignored upstream; SM_ASSERT(pUVCurve != NULL)
            // is a release no-op and the copy dereferences it), copy UVTrimCurve
            // (the status is ignored: upstream copies whatever curve it returns)
            let _ignored_status = get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
            let uv_curve = eu1::uv_trim_curve(b, eu)?.0.ok_or_else(|| {
                null("CreateCornerBlend: GetOrCreateUVTrimCurve left no UV trim curve")
            })?;
            let uv_copy = uv_curve.copy(k)?;
            let Some(uv_bspline) = bspline_part(&uv_copy) else {
                return Err(sm_err());
            };
            ordered_uv.push(Some(uv_bspline));
            let face = eu1::face(b, eu)?.ok_or_else(|| SolidError::null_pointer(OP))?; // NER
            ordered_surfs.push(
                face2::surface(b, face)?
                    .map(|s| bspline_surface(s).cloned())
                    .transpose()?,
            );
        } else {
            ordered_uv.push(None);
            ordered_surfs.push(None);
        }
    }

    // ensure input arrays have at least 4 entries by inserting degenerate 'point' curves as needed
    // assume the input always starts with at least 2 curves
    for ii in 0..2usize {
        // skip inputs that already have 4 or more curve
        if ordered_3d.len() >= 4 {
            break;
        }

        // grab the second curve - pass 1: use curve end, pass 2: use curve start
        let curve_3d = ordered_3d.get(1).ok_or_else(sm_err)?;
        let mut param = if eu_orients.get(1) == Some(&OrientType::Same) {
            1.0
        } else {
            0.0
        };
        if ii == 2 {
            param = 1.0 - param; // Beginning of curve second time (never: ii < 2, as upstream)
        }
        // (upstream fetches the natural interval here and leaves it unused: the
        // evaluation below takes the raw 0/1 parameter)

        // Curve EndPoint
        let pnt = curve_3d.evaluate_point(param)?;

        // create a degenerate 'point' curve at the second curve's endPoint (parameterized from 0 to 1)
        let top_curve = BSplineCurve::create_point_curve(pnt)?;

        // add it to the input arrays at the proper location
        ordered_3d.insert(2 - ii, top_curve);
        eu_orients.insert(2 - ii, OrientType::Same);
        ordered_uv.insert(2 - ii, None);
        ordered_surfs.insert(2 - ii, None);
    }

    // move the last entry of every input array to the 1st position - GWC:Why?
    ordered_3d.rotate_right(1);
    eu_orients.rotate_right(1);
    ordered_uv.rotate_right(1);
    ordered_surfs.rotate_right(1);

    // pass the call along (FALSE = bicubic patch through curves and crossTangents)
    let surfs: Vec<Option<&BSplineSurface>> = ordered_surfs.iter().map(Option::as_ref).collect();
    BSplineSurface::create_corner_blend(
        &ordered_3d,
        &eu_orients,
        &ordered_uv,
        &surfs,
        approx_tol_3d,
        tangency_tol_radians,
        false,
    )
}

// ---- edge-to-edge blends (SmBrep::CreateEdgeEdgeBlend) ----------------------------------

/// `CreateEdgeEdgeBlend`: a ruled (`blend_flag` 0) or G1/G2/G3 blend (`1`/`2`/`3`)
/// surface between the trimmed NURBS curves of two edges. `curves_dir_flag`: `0`
/// guesses the pairing from the rail distances, `1` starts both at their curve starts,
/// `2` pairs start with end. `opt_ds_def` (its curvature type is overwritten from
/// `blend_flag`) tunes a blend.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` for a null edge use (`NER`); `SM_ERR_INVALID_INPUT` for an
/// edge use without edge, an edge without B-spline curve or a face without surface;
/// [`SolidError::InvalidInput`] for a blend edge use without face (upstream
/// dereferences it); those of the knot dampening and the ruled surface (the binormal
/// statuses are ignored as upstream).
#[allow(clippy::too_many_arguments)]
pub fn create_edge_edge_blend(
    k: &Kernel,
    b: &mut Brep,
    blend_flag: u64,
    eu_1: Option<EdgeuseId>,
    eu_2: Option<EdgeuseId>,
    curves_dir_flag: u64,
    opt_ds_def: Option<&mut DerivSurfDefinition<'_>>,
) -> Result<Surface> {
    let eu_1 = eu_1.ok_or_else(|| SolidError::null_pointer(OP))?; // NER
    let eu_2 = eu_2.ok_or_else(|| SolidError::null_pointer(OP))?; // NER

    // locals
    let (Some(edge1), Some(edge2)) = (eu1::edge(b, eu_1)?, eu1::edge(b, eu_2)?) else {
        return Err(status(SM_ERR_INVALID_INPUT)); // Could not fetch Edge from Edgeuse
    };

    // edge locals
    let ivl1 = edge::interval(b, edge1)?;
    let mut ivl2 = edge::interval(b, edge2)?;
    let rail1 = edge::create_trimmed_nurbs_curve(k, b, edge1)?;
    let rail2 = edge::create_trimmed_nurbs_curve(k, b, edge2)?;
    let tolerance = max2(edge::tolerance(b, edge1)?, edge::tolerance(b, edge2)?);
    let (Some(rail1), Some(rail2)) = (rail1, rail2) else {
        return Err(status(SM_ERR_INVALID_INPUT)); // Could not build BSplineCurves from Edges
    };
    let (Some(mut rail1), Some(mut rail2)) = (bspline_part(&rail1), bspline_part(&rail2)) else {
        return Err(status(SM_ERR_INVALID_INPUT));
    };

    // to preserve previous behavior - use backward compatible style for ruled surfaces
    if blend_flag == 0 {
        // insert knots when needed to make knot spacing more regular
        if rail1.degree()? >= 3 {
            rail1.dampen_knot_spacing(2.0, 3)?;
        }
        if rail2.degree()? >= 3 {
            rail2.dampen_knot_spacing(2.0, 3)?;
        }

        // create the ruled surface - passes call to NLib N_CreateRuledSrf() which makes
        // curves compatible (knot tol = .001)
        let new_surface = BSplineSurface::create_ruled_surface(&rail1, &rail2, SurfParamType::U)?;
        return Ok(Surface::from(new_surface));
    }

    // arrive here to build a blend surface between two edges

    // make sure the input is self-consistent: m_eCurvatureType matches lBlendFlag value
    let mut ds_def = opt_ds_def;
    if let Some(d) = ds_def.as_deref_mut() {
        d.curvature_type = match blend_flag {
            1 => CurvatureType::NoCurvature,
            3 => CurvatureType::G3FromSurface,
            _ => CurvatureType::G2FromSurface,
        };
    }

    // Guess curve directions if necessary
    let same_dir_curves = match curves_dir_flag {
        1 => true,
        2 => false,
        _ => {
            // Get positions along railcurves in order of parameterization
            let num_loc = 11usize;
            let mut rail1_pos = Vec::with_capacity(num_loc);
            let mut rail2_pos = Vec::with_capacity(num_loc);
            for ii in 0..num_loc {
                // (evaluation statuses are not checked upstream: a failure leaves the
                // sample unset; undefined coordinates here)
                let t = ii as f64 / (num_loc - 1) as f64;
                rail1_pos.push(
                    ignored(rail1.evaluate_point(ivl1.evaluate(t)))?.unwrap_or(Point3::UNDEF),
                );
                rail2_pos.push(
                    ignored(rail2.evaluate_point(ivl2.evaluate(t)))?.unwrap_or(Point3::UNDEF),
                );
            }

            // Check under which orientation the distance between rail curves is less
            let mut same_direction = 0.0;
            let mut opp_direction = 0.0;
            for ((p1, p2), p2r) in rail1_pos.iter().zip(&rail2_pos).zip(rail2_pos.iter().rev()) {
                same_direction += p1.distance(*p2);
                opp_direction += p1.distance(*p2r);
            }
            same_direction <= opp_direction
        }
    };

    // face locals
    let face1 =
        eu1::face(b, eu_1)?.ok_or_else(|| null("CreateEdgeEdgeBlend: edge use without face"))?;
    let face2_ =
        eu1::face(b, eu_2)?.ok_or_else(|| null("CreateEdgeEdgeBlend: edge use without face"))?;
    let surfaces_present = (
        face2::surface(b, face1)?.is_some(),
        face2::surface(b, face2_)?.is_some(),
    );
    if !(surfaces_present.0 && surfaces_present.1) {
        return Err(status(SM_ERR_INVALID_INPUT)); // Could not fetch Surface from Edgeuse
    }

    // set the bIntoFace values for curve/Surface pairs
    // (both statuses ignored upstream: a failure leaves defined UNDEF / zero outputs)
    let BinormalEval {
        binormal: edge1_binormal,
        tangent: mut edge1_tangent,
        face_normal: mut face1_normal,
        ..
    } = evaluate_binormal_status_ignored(k, b, eu_1, ivl1.evaluate(0.5))?;
    let BinormalEval {
        binormal: edge2_binormal,
        tangent: mut edge2_tangent,
        face_normal: mut face2_normal,
        ..
    } = evaluate_binormal_status_ignored(k, b, eu_2, ivl2.evaluate(0.5))?;
    if eu1::orientation(b, eu_1)? == OrientType::Opposite {
        edge1_tangent = -edge1_tangent;
    }
    if eu1::orientation(b, eu_2)? == OrientType::Opposite {
        edge2_tangent = -edge2_tangent;
    }
    let fu_orient = |eu: EdgeuseId| -> Result<OrientType> {
        let fu = eu1::faceuse(b, eu)?
            .ok_or_else(|| null("CreateEdgeEdgeBlend: edge use without face use"))?;
        eu1::faceuse_orientation(b, fu)
    };
    if fu_orient(eu_1)? == OrientType::Opposite {
        face1_normal = -face1_normal;
    }
    if fu_orient(eu_2)? == OrientType::Opposite {
        face2_normal = -face2_normal;
    }

    // set edge1 blend direction to run out of the face, edge2 blend direction to run into the face
    let curve1_left = face1_normal.cross(edge1_tangent);
    let curve2_left = face2_normal.cross(edge2_tangent);
    let mut off_to_the_right = edge1_binormal.dot(curve1_left) >= 0.0;
    let mut in_from_the_left = edge2_binormal.dot(curve2_left) < 0.0;

    // make sure two curve intervals run in the same direction and share a common parameterization
    if !same_dir_curves {
        // (status not checked upstream; the interval is left as is on failure)
        ivl2 = ignored(rail2.reverse_parameterization(&ivl2))?.unwrap_or(ivl2);
        in_from_the_left = !in_from_the_left;
    }

    let rescale = |rail2: &mut BSplineCurve, ivl2: &mut Extent1d| -> Result<()> {
        if !ivl1.are_equal(ivl2, 0.0) {
            // rescale Curve2 (the offset expression keeps upstream's operator precedence)
            let scale = (ivl1.min() - ivl1.max()) / (ivl2.min() - ivl2.max());
            let offset =
                (ivl2.min() * ivl1.max()) - (ivl1.min() * ivl2.max()) / (ivl2.min() - ivl2.max());

            let nat = rail2.natural_interval();
            let mut new_ivl = nat;
            // (neither status is checked upstream)
            ignored(new_ivl.set_min_max(nat.min() * scale + offset, nat.max() * scale + offset))?;
            ignored(rail2.edit_parameterization(&new_ivl))?;
            *ivl2 = ivl1;
        }
        Ok(())
    };
    rescale(&mut rail2, &mut ivl2)?;

    // make blends either G* or D*
    if let Some(d) = ds_def.as_deref() {
        match d.blend_end_type {
            BlendEndType::NoCusp => {}
            BlendEndType::FirstCusp => off_to_the_right = !off_to_the_right,
            BlendEndType::SecondCusp => in_from_the_left = !in_from_the_left,
            BlendEndType::BothCusp => {
                off_to_the_right = !off_to_the_right;
                in_from_the_left = !in_from_the_left;
            }
        }
    }

    // make sure two curve intervals run in the same direction and share a common
    // parameterization (upstream repeats both steps here)
    if !same_dir_curves {
        ivl2 = ignored(rail2.reverse_parameterization(&ivl2))?.unwrap_or(ivl2);
    }
    rescale(&mut rail2, &mut ivl2)?;

    // build blend surface
    let curve1 = Curve::from(rail1);
    let curve2 = Curve::from(rail2);
    // (the face surfaces fetched above; re-borrowed past the edge use evaluations)
    let surface1 = face2::surface(b, face1)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let surface2 = face2::surface(b, face2_)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    surface1.create_blend_surface(
        k,
        &curve1,
        &ivl1,
        off_to_the_right,
        surface2,
        &curve2,
        &ivl2,
        in_from_the_left,
        tolerance,
        ds_def.as_deref(),
    )
}

// ---- parallel projections (SmBrep::CreateParallelProjectionCurves) ----------------------

/// The optional inputs of [`create_parallel_projection_curves`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ParallelProjectionOptions<'f> {
    /// `cpdOptProjectionDistance`: how far to project (through the surface when `None`).
    pub projection_distance: Option<f64>,
    /// `cpdOpt3DApproximationTol`: the 3D approximation tolerance (also the curve
    /// tolerance of the trimming).
    pub approx_tol_3d: Option<f64>,
    /// `cpdOptAngleTolRadians`.
    pub angle_tol_radians: Option<f64>,
    /// `pOptSubsetFaces`: only these faces (default every face of the B-rep).
    pub subset_faces: Option<&'f [FaceId]>,
    /// `pOpt3DProjectedCurves` wanted.
    pub want_3d: bool,
    /// `pOptUVProjectedCurves` wanted.
    pub want_uv: bool,
    /// `pOptFaceOfUVCurves` wanted.
    pub want_faces: bool,
}

/// The outputs of [`create_parallel_projection_curves`].
#[derive(Debug, Clone, Default)]
pub struct ParallelProjectionCurves {
    /// `pOpt3DProjectedCurves`: the 3D projected curves trimmed to the faces.
    pub curves_3d: Option<Vec<Curve>>,
    /// `pOptUVProjectedCurves`: the UV projected curves trimmed to the faces (`None`
    /// entries as `SmFace::CreateCurvesByTrimming` leaves them).
    pub uv_curves: Option<Vec<Option<Curve>>>,
    /// `pOptFaceOfUVCurves`: the face of each UV curve.
    pub faces_of_uv_curves: Option<Vec<FaceId>>,
}

/// `CreateParallelProjectionCurves`: projects `curve` along `projection_vector` onto
/// every face (or `subset_faces`) of the B-rep (`SmSurface::CreateParallelProjectionCurves`
/// over the face's UV domain, then `SmFace::CreateCurvesByTrimming`), collecting the
/// outputs asked for in `opts`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface (upstream dereferences
/// it); those of the projection and the trimming (upstream deletes the curves made so
/// far on failure; here they are dropped with the error).
pub fn create_parallel_projection_curves(
    k: &Kernel,
    b: &Brep,
    curve: &BSplineCurve,
    projection_vector: Vec3,
    opts: &ParallelProjectionOptions<'_>,
) -> Result<ParallelProjectionCurves> {
    let mut out = ParallelProjectionCurves {
        curves_3d: opts.want_3d.then(Vec::new),
        uv_curves: opts.want_uv.then(Vec::new),
        faces_of_uv_curves: opts.want_faces.then(Vec::new),
    };

    let face_list: Vec<FaceId> = match opts.subset_faces {
        Some(s) => s.to_vec(),
        None => faces(b, None)?,
    };

    for face in face_list {
        let uv_domain = face2::uv_domain(b, face)?;
        let surface = face_surface(b, face)?;
        let (curves_3d, uv_curves) = surface.create_parallel_projection_curves(
            k,
            &uv_domain,
            curve,
            projection_vector,
            opts.projection_distance,
            opts.approx_tol_3d,
            opts.angle_tol_radians,
        )?;
        let curves_3d: Vec<Option<Curve>> = curves_3d.into_iter().map(Some).collect();

        let trimmed = face2::create_curves_by_trimming(
            k,
            b,
            face,
            opts.approx_tol_3d,
            Some(&curves_3d),
            Some(&uv_curves),
            true,
            true,
        )?;
        let trim_3d = trimmed.curves_3d.unwrap_or_default();
        let trim_uv = trimmed.uv_curves.unwrap_or_default();

        if let Some(fs) = out.faces_of_uv_curves.as_mut() {
            for _ in 0..trim_uv.len() {
                fs.push(face);
            }
        }
        if let Some(c) = out.curves_3d.as_mut() {
            c.extend(trim_3d);
        }
        if let Some(c) = out.uv_curves.as_mut() {
            c.extend(trim_uv);
        }
    } // for each Face

    Ok(out)
}

// ---- healing (SmBrep::HealBrep) ---------------------------------------------------------

/// The loop handle of the healer `SmBrep::HealBrep` creates locally (`SmHealData
/// sHealData`): a loop id, as the healer's classification objects are loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealLoopHandle(pub LoopId);

impl ClassObject for HealLoopHandle {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Loop
    }
}

impl LoopObject for HealLoopHandle {
    fn from_loop(l: LoopId) -> Self {
        Self(l)
    }
}

/// `HealBrep`: runs the healer sequence (`SmHealData::HealBrep`, on a fresh
/// [`HealData`]) up to and including `healer_op` on the given vertices, edges and
/// faces (default: all), optionally making the UV trim curves. Upstream returns
/// `SM_SUCCESS` whatever the healer reports: only [`SolidError::Unported`] (from the
/// healer's own stubs, `heal.heal_data_p3` and later) propagates.
///
/// # Errors
///
/// [`SolidError::Unported`] from the healer.
pub fn heal_brep(
    k: &mut Kernel,
    b: &mut Brep,
    healer_op: HealerOpType,
    opt_vertices: Option<&[VertexId]>,
    opt_edges: Option<&[EdgeId]>,
    opt_faces: Option<&[FaceId]>,
    make_uv_trim_curves: bool,
) -> Result<()> {
    let mut heal_data = HealData::<HealLoopHandle>::default();
    // pass the call along (status ignored upstream)
    ignored(heal_data.heal_brep(
        k,
        Some(b),
        healer_op,
        HealBrepTargets {
            vertices: opt_vertices,
            edges: opt_edges,
            faces: opt_faces,
        },
        make_uv_trim_curves,
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::a_object;
    use crate::topology::Region;

    #[test]
    fn type_queries() {
        assert!(is_kind_of(TYPE));
        assert!(is_kind_of(a_object::SAG_TYPE));
        assert!(is_kind_of(a_object::TYPE));
        assert!(!is_kind_of(16012));
        assert_eq!(CLASS_STRING, "SmBrep");
    }

    #[test]
    fn region_list_rotates_to_head() {
        let mut b = Brep::new();
        let r: Vec<RegionId> = (0..3)
            .map(|_| {
                b.add_region_record(Region {
                    in_brep: true,
                    ..Region::default()
                })
                .unwrap()
            })
            .collect();
        let off = b
            .add_region_record(Region {
                in_brep: false,
                ..Region::default()
            })
            .unwrap();
        assert_eq!(regions(&b, None).unwrap(), r);
        b.set_region_list_head(Some(r[1]));
        assert_eq!(regions(&b, None).unwrap(), vec![r[1], r[2], r[0]]);
        assert!(!regions(&b, None).unwrap().contains(&off));
        assert!(!is_live_topology_member(&b, None));
    }

    #[test]
    fn votes() {
        let mut rs = Vec::new();
        let mut vs = Vec::new();
        let (a, c) = (Some(RegionId::from_index(0)), Some(RegionId::from_index(1)));
        assert_eq!(find_highest_vote(&rs, &vs).unwrap(), None);
        vote_for_region(&mut rs, &mut vs, a);
        vote_for_region(&mut rs, &mut vs, c);
        vote_for_region(&mut rs, &mut vs, c);
        assert_eq!((rs.clone(), vs.clone()), (vec![a, c], vec![1, 2]));
        assert_eq!(find_highest_vote(&rs, &vs).unwrap(), c);
        vote_for_region(&mut rs, &mut vs, a);
        // ties keep the first
        assert_eq!(find_highest_vote(&rs, &vs).unwrap(), a);
        // the null region is a region of its own; a null winner reads None, as upstream
        vote_for_region(&mut rs, &mut vs, None);
        vote_for_region(&mut rs, &mut vs, None);
        vote_for_region(&mut rs, &mut vs, None);
        assert_eq!((rs.clone(), vs.clone()), (vec![a, c, None], vec![2, 2, 3]));
        assert_eq!(find_highest_vote(&rs, &vs).unwrap(), None);
        assert!(find_highest_vote(&rs, &[1]).is_err());
    }

    #[test]
    fn size_estimates_follow_the_context() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        assert_eq!(b.this_model_size_estimate(&k, false), tol::USE_DEFAULT);
        assert_eq!(
            b.this_model_size_estimate(&k, true),
            tol::MODEL_SIZE_ESTIMATE
        );
        assert_eq!(b.set_this_model_size_estimate(&mut k, 1000.0), 1.0e-4);
        assert_eq!(b.tolerance(), 1.0e-4);
        assert_eq!(k.context.stored_model_size_estimate(), 1000.0);
        b.set_this_large_small_size_ratio(&mut k, 7.0);
        assert_eq!(b.this_large_small_size_ratio(&k, false), 7.0);
        assert_eq!(k.context.stored_large_small_size_ratio(), 7.0);
    }

    #[test]
    fn object_maps() {
        let (a, c) = (
            EntityRef::Face(FaceId::from_index(0)),
            EntityRef::Face(FaceId::from_index(1)),
        );
        let mut map = ObjectMap::new();
        let mut copy = CopyBrepMap::new();
        let mut rev = CopyBrepMap::new();
        set_object_map(a, Some(c), &mut map, Some(&mut copy), Some(&mut rev));
        set_object_map(c, None, &mut map, Some(&mut copy), None);
        assert_eq!(get_object_map(a, &map), Some(c));
        assert_eq!(get_object_map(c, &map), None);
        assert_eq!(copy.get_at(Some(c)).unwrap(), Some(a));
        assert_eq!(rev.get_at(Some(a)).unwrap(), Some(c));
        assert_eq!(copy.len(), 1);
    }
}
