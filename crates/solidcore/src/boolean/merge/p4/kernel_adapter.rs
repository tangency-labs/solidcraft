//! Concrete arena adapter for the manifold Boolean's shared merge stages.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/src/SmTopologyIntersector.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`KernelManifoldIntersector`] | `SmMerge::m_vTI` and shared merge-stage access |
//! | [`KernelManifoldIntersector::delete_faces_in`] | `SmTopologyIntersector(ctx, brep, brep, ..).DeleteFaces` over one B-rep |
//!
//! Deviations: the existing typed `Relations` is the sole mate store. Object
//! keys encode a caller-reserved 27-bit namespace, operand, class and arena id;
//! reserve a different namespace for every pair sharing an attribute store.
//! Source-list notification handles resolve back into that same attribute store.
//! Surface identity follows the canonical B-rep cache convention: its owner,
//! or its containing face when the owned surface has no explicit owner.
//! No geometry, relationship algorithm or input-dependent recursion is added.

use super::arena_pair::{self, ArenaPair, H};
use super::{
    manifold_boolean, BooleanKernels, FixedGaps, MadeFaceuse, ManifoldIntersector, ManifoldState,
    MergeEntity,
};
use crate::boolean::merge::p1::{self, MergeState};
use crate::boolean::merge::p2::{self, MergeContext, TopologyCopy};
use crate::boolean::merge::p5;
use crate::boolean::topology_intersector::p1::TopologyIntersector;
use crate::boolean::topology_intersector::p1::{set_subset_objects, Relations};
use crate::error::{Result, SolidError};
use crate::geom::a_object::{BooleanOperation, REGION_TYPE};
use crate::geom::attribute::{ids, AttributeStore, ObjectKey};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::{Context, MarkLock, MarkType};
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::topology::brep::p1 as bp;
use crate::topology::brep_cache::{arena_identity, CacheObject};
use crate::topology::shell::{RegionLookup, RegionRef};
use crate::topology::topology::Topology;
use crate::topology::{Brep, BrepId, EdgeId, FaceId, FaceuseId, RegionId, VertexId};
use crate::EntityRef;
use std::collections::{BTreeMap, BTreeSet};

const OP: &str = "boolean::merge::p4::kernel_adapter";

fn key(namespace: u32, other: bool, kind: u64, index: u32) -> ObjectKey {
    ObjectKey(
        (u64::from(namespace) << 37) | (u64::from(other) << 36) | (kind << 32) | u64::from(index),
    )
}

fn entity_key(namespace: u32, other: bool, entity: MergeEntity) -> ObjectKey {
    let (kind, index) = match entity {
        MergeEntity::Region(id) => (1, id.index()),
        MergeEntity::Face(id) => (2, id.index()),
        MergeEntity::Edge(id) => (3, id.index()),
        MergeEntity::Vertex(id) => (4, id.index()),
    };
    key(namespace, other, kind, index)
}

// Cache construction takes only &Kernel; preserve the caller's settings and
// RNG state for that read-only work rather than evaluating under defaults.
fn cache_kernel(k: &Kernel, context: &Context) -> Kernel {
    Kernel {
        tolerances: k.tolerances,
        rng: k.rng.clone(),
        caches: crate::kernel::Caches::default(),
        context: context.metadata_copy(),
    }
}

fn cache_kernels(k: &Kernel, other: Option<&Kernel>, context: &Context) -> [Kernel; 2] {
    [
        cache_kernel(k, context),
        other.map_or_else(|| cache_kernel(k, context), |k| cache_kernel(k, &k.context)),
    ]
}

// Every topology class has its own slot in the caller-reserved namespace.
pub(super) fn notification_key(namespace: u32, other: bool, entity: EntityRef) -> ObjectRef {
    let (kind, type_code, index) = match entity {
        EntityRef::Region(v) => (1, 16012, v.index()),
        EntityRef::Face(v) => (2, 16002, v.index()),
        EntityRef::Edge(v) => (3, 16016, v.index()),
        EntityRef::Vertex(v) => (4, 16021, v.index()),
        EntityRef::Brep(v) => (0, 16011, v.index()),
        EntityRef::Shell(v) => (7, 16013, v.index()),
        EntityRef::Loop(v) => (8, 16003, v.index()),
        EntityRef::Faceuse(v) => (9, 16022, v.index()),
        EntityRef::Loopuse(v) => (10, 16004, v.index()),
        EntityRef::Edgeuse(v) => (11, 16005, v.index()),
        EntityRef::Vertexuse(v) => (12, 16006, v.index()),
    };
    ObjectRef {
        key: key(namespace, other, kind, index),
        type_code,
    }
}

/// The kernel adapter for the ported merge and topology-intersector stages.
/// The canonical merge state owns the single shared face/edge/vertex mate store;
/// notifications use `records` and the attributes supplied to each operation.
/// Missing dependency calls retain the trait's owner-tagged defaults.
#[derive(Debug)]
pub struct KernelManifoldIntersector {
    /// Canonical options, tag and relationships, shared with the ported stages.
    pub merge: MergeState<Relations>,
    /// Topology notification state for both operands, under [`Self::object_key`].
    pub records: BTreeMap<ObjectKey, Topology>,
    /// `m_bTopologyDeleted`; dependency editing stages update this same flag.
    pub topology_deleted: bool,
    intersector: TopologyIntersector<H>,
    curves: Vec<crate::curve::curve::Curve>,
    extra_curves: Vec<crate::curve::curve::Curve>,
    namespace: u32,
    subset: Option<BTreeSet<ObjectRef>>,
}

impl KernelManifoldIntersector {
    /// Create an adapter with default merge options and no relationships.
    /// The caller reserves `namespace` exclusively among all object keys in the
    /// supplied attribute/notification stores; its complete block is `2^37` keys.
    ///
    /// # Errors
    /// A namespace exceeding 27 bits.
    pub fn new(namespace: u32) -> Result<Self> {
        if namespace > 0x07ff_ffff {
            return Err(SolidError::invalid_input(
                OP,
                "object namespace exceeds 27 bits",
            ));
        }
        Ok(Self {
            merge: MergeState::new(Relations::default()),
            records: BTreeMap::new(),
            topology_deleted: false,
            namespace,
            subset: None,
            intersector: TopologyIntersector::new(
                Some(arena_pair::brep_h(0)),
                Some(arena_pair::brep_h(1)),
                0.0,
                0.0,
            ),
            curves: Vec::new(),
            extra_curves: Vec::new(),
        })
    }

    /// `SmTopologyIntersector(ctx, pBrep, pBrep, SM_EFF_ZERO, SM_EFF_ZERO).DeleteFaces(faces)`
    /// over one kernel B-rep, as the offset executive's lamina clean-up uses it
    /// (`SmOffsetExecutive.cpp:3749-3752`): a fresh intersector whose two operands
    /// are the same B-rep deletes `faces` through this crate's arena access.
    /// Notifications of the deleted topology go to `records` and `attributes` under
    /// `namespace` (27 bits, distinct from any other pair sharing the store).
    ///
    /// # Errors
    ///
    /// Those of [`TopologyIntersector::delete_faces`] and an over-wide namespace.
    pub fn delete_faces_in(
        kernel: &mut Kernel,
        brep: &mut Brep,
        faces: &[FaceId],
        records: &mut BTreeMap<ObjectKey, Topology>,
        attributes: &mut AttributeStore,
        namespace: u32,
    ) -> Result<()> {
        if namespace > 0x07ff_ffff {
            return Err(SolidError::invalid_input(
                OP,
                "object namespace exceeds 27 bits",
            ));
        }
        let mut intersector: TopologyIntersector<H> = TopologyIntersector::new(
            Some(arena_pair::brep_h(0)),
            Some(arena_pair::brep_h(0)),
            crate::math::EFF_ZERO,
            crate::math::EFF_ZERO,
        );
        let handles: Vec<H> = faces
            .iter()
            .map(|&face| arena_pair::encode(EntityRef::Face(face), false))
            .collect();
        let mut curves = Vec::new();
        let mut extra_curves = Vec::new();
        Context::with_notification_context(kernel, |k, context| {
            let mut pair = ArenaPair::new(
                brep,
                None,
                &mut curves,
                &mut extra_curves,
                records,
                attributes,
                cache_kernels(k, None, context),
                context,
                None,
                namespace,
            );
            intersector.delete_faces(&mut pair, &handles, None)
        })
    }

    /// Set the same explicit intersection tolerances supplied to `SmMerge`.
    /// Leaving them at zero retains the context-only intersector defaults;
    /// automatic tolerance selection belongs to the merge constructor's owner.
    pub fn set_intersection_tolerances(&mut self, distance: f64, angle_radians: f64) {
        self.intersector.set_this_approx_tol_3d(distance);
        self.intersector.set_this_ang_tol_rad(angle_radians);
    }

    /// Set `m_vTI.m_lIntersectLaminaEdges` (0 none, 1 both, 2 only the B-rep's,
    /// 3 only the other's), as `NonManifoldBoolean` and `PiecewiseMerge` do.
    pub fn set_intersect_lamina_edges(&mut self, mode: u64) {
        self.intersector.intersect_lamina_edges = mode;
    }

    /// `m_vTI.m_lIntersectLaminaEdges`.
    #[must_use]
    pub fn intersect_lamina_edges(&self) -> u64 {
        self.intersector.intersect_lamina_edges
    }

    /// Run the orchestration with the options held in the canonical `MergeState`.
    pub fn manifold_boolean(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Option<Brep>,
        other: &mut Option<Brep>,
        state: &mut ManifoldState,
        attributes: &mut AttributeStore,
        manifold: bool,
        operation: BooleanOperation,
        lock: Option<&mut MarkLock>,
    ) -> Result<bool> {
        let options = self.merge.options;
        manifold_boolean(
            kernels, primary, other, self, state, attributes, options, manifold, operation, lock,
        )
    }

    /// The geometry identity used by the canonical B-rep cache, including shared
    /// owner identities. This deliberately does not compare surface values.
    pub fn surface_identity(other: &Brep, face: FaceId) -> Result<ObjectRef> {
        let surface = other
            .face(face)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        Ok(surface
            .owner()
            .unwrap_or_else(|| arena_identity(CacheObject::Face(face))))
    }

    /// Replace the partial-merge surface subset; an empty input clears it, as
    /// `SetSubset`. Obtain handles with [`Self::surface_identity`].
    pub fn set_surface_subset(&mut self, surfaces: &[ObjectRef]) {
        set_subset_objects(&mut self.subset, surfaces);
    }

    /// Resolve a propagation callback's source-list handle to the current merge
    /// attribute's actual list. No list contents or region flags are duplicated.
    pub fn source_regions<'a>(
        &self,
        attributes: &'a AttributeStore,
        handle: ObjectRef,
    ) -> Result<&'a [ObjectKey]> {
        let kind = (handle.key.0 >> 32) & 15;
        if handle.type_code != 0
            || handle.key.0 >> 37 != u64::from(self.namespace)
            || (handle.key.0 >> 36) & 1 != 0
            || !(kind == 5 || kind == 6)
        {
            return Err(SolidError::invalid_input(OP, "not a source-list handle"));
        }
        let region = RegionId::from_index(handle.key.0 as u32);
        let owner = self.object_key(false, MergeEntity::Region(region));
        let data = attributes
            .find_attribute(owner, ids::OTHER_REGION_ID)
            .and_then(|h| attributes.get(h))
            .and_then(|a| a.merge_region_data())
            .ok_or_else(|| {
                SolidError::invalid_input(OP, "source-list attribute no longer exists")
            })?;
        Ok(if kind == 5 {
            &data.this_regions
        } else {
            &data.other_regions
        })
    }
}

struct Regions<'a> {
    namespace: u32,
    primary: &'a Brep,
    other: &'a Brep,
}

impl RegionLookup for Regions<'_> {
    fn resolve_region(&self, object: ObjectKey) -> Result<RegionRef<'_>> {
        if object.0 >> 37 != u64::from(self.namespace) || (object.0 >> 32) & 15 != 1 {
            return Err(SolidError::invalid_input(
                OP,
                "not an operand region identity",
            ));
        }
        let other = (object.0 >> 36) & 1 != 0;
        let brep = if other { self.other } else { self.primary };
        let region_id = RegionId::from_index(object.0 as u32);
        brep.region(region_id)?;
        Ok(RegionRef {
            brep,
            brep_id: BrepId::from_index(self.namespace * 2 + u32::from(other)),
            region_id,
        })
    }
}

impl ManifoldIntersector for KernelManifoldIntersector {
    fn transform_brep(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        brep: &mut Brep,
        other: bool,
        attributes: &mut AttributeStore,
        placement: &Axis2Placement,
        scale: Option<Vec3>,
        scale_tolerance: bool,
    ) -> Result<()> {
        let namespace = self.namespace;
        let key_of = |e| notification_key(namespace, other, e);
        let k = if other {
            kernels.other.as_deref_mut().unwrap_or(kernels.primary)
        } else {
            &mut *kernels.primary
        };
        Context::with_notification_context(k, |k, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: &mut self.records,
                attributes,
                context: Some(context),
                key_of: &key_of,
            };
            crate::topology::brep::p5::transform_with_notifications(
                k,
                brep,
                placement,
                scale,
                scale_tolerance,
                Some(&mut env),
            )
        })
    }

    fn intersect_insert_relate(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
    ) -> Result<()> {
        let mut subset = Vec::new();
        if let Some(selected) = &self.subset {
            for f in bp::faces(other, None)? {
                if selected.contains(&Self::surface_identity(other, f)?) {
                    subset.push(arena_pair::surface_handle(true, f));
                }
            }
            if subset.is_empty() {
                subset.push(arena_pair::unmatched_surface());
            }
        }
        self.intersector.set_subset(&subset);
        self.intersector.topology_deleted = self.topology_deleted;
        Context::with_notification_context(kernels.primary, |k, context| {
            let mut pair = ArenaPair::new(
                primary,
                Some(other),
                &mut self.curves,
                &mut self.extra_curves,
                &mut self.records,
                attributes,
                cache_kernels(k, kernels.other.as_deref(), context),
                context,
                kernels.other.as_deref_mut().map(|k| &mut k.context),
                self.namespace,
            );
            let result = self.merge.intersector.with_intersector(
                &mut self.intersector,
                arena_pair::encode,
                arena_pair::decode,
                |ti| ti.intersect_insert_relate(k, &mut pair, arena_pair::TI_HANDLE),
            );
            self.topology_deleted = self.intersector.topology_deleted;
            // Deleted curve handles no longer appear in manager/table state. Storage
            // remains stable so surviving handles keep their identities.
            result.map(|_| ())
        })
    }

    fn remove_relationship(&mut self, face: FaceId, other_face: FaceId) -> Result<()> {
        self.merge.intersector.with_intersector(
            &mut self.intersector,
            arena_pair::encode,
            arena_pair::decode,
            |ti| {
                ti.remove_relationship(
                    Some(arena_pair::encode(EntityRef::Face(face), false)),
                    Some(arena_pair::encode(EntityRef::Face(other_face), true)),
                )
            },
        )
    }

    fn fix_gaps(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
        single_vertices: &[VertexId],
    ) -> Result<FixedGaps> {
        let vertices: Vec<_> = single_vertices
            .iter()
            .map(|&v| arena_pair::encode(EntityRef::Vertex(v), false))
            .collect();
        Context::with_notification_context(kernels.primary, |k, context| {
            let mut pair = ArenaPair::new(
                primary,
                Some(other),
                &mut self.curves,
                &mut self.extra_curves,
                &mut self.records,
                attributes,
                cache_kernels(k, kernels.other.as_deref(), context),
                context,
                kernels.other.as_deref_mut().map(|k| &mut k.context),
                self.namespace,
            );
            self.merge
                .intersector
                .with_intersector(
                    &mut self.intersector,
                    arena_pair::encode,
                    arena_pair::decode,
                    |ti| ti.fix_gaps(k, &mut pair, &vertices),
                )
                .and_then(|out| {
                    Ok(FixedGaps {
                        modified_topology: out.modified_topology,
                        deleted_vertices: out
                            .deleted_vertices
                            .into_iter()
                            .map(arena_pair::deleted_vertex)
                            .collect::<Result<_>>()?,
                        deleted_edges: out
                            .deleted_edges
                            .into_iter()
                            .flatten()
                            .map(arena_pair::deleted_edge)
                            .collect::<Result<_>>()?,
                    })
                })
        })
    }

    fn delete_faces(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        attributes: &mut AttributeStore,
        faces: &[FaceId],
        keep_wires: Option<&[EdgeId]>,
    ) -> Result<()> {
        let faces: Vec<_> = faces
            .iter()
            .map(|&f| arena_pair::encode(EntityRef::Face(f), false))
            .collect();
        let keep = keep_wires.map(|edges| {
            edges
                .iter()
                .map(|&e| arena_pair::encode(EntityRef::Edge(e), false))
                .collect::<Vec<_>>()
        });
        Context::with_notification_context(kernels.primary, |k, context| {
            let mut pair = ArenaPair::new(
                primary,
                None,
                &mut self.curves,
                &mut self.extra_curves,
                &mut self.records,
                attributes,
                cache_kernels(k, kernels.other.as_deref(), context),
                context,
                None,
                self.namespace,
            );
            self.merge.intersector.with_intersector(
                &mut self.intersector,
                arena_pair::encode,
                arena_pair::decode,
                |ti| ti.delete_faces(&mut pair, &faces, keep.as_deref()),
            )
        })
    }

    fn copy_component_faces(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        source: &mut Brep,
        other: bool,
        faces: &[FaceId],
        destination: &mut Brep,
        attributes: &mut AttributeStore,
    ) -> Result<()> {
        let namespace = self.namespace;
        let face_key = |face| notification_key(namespace, other, EntityRef::Face(face));
        let key_of = |e| notification_key(namespace, other, e);
        let k = if other {
            kernels.other.as_deref_mut().unwrap_or(kernels.primary)
        } else {
            &mut *kernels.primary
        };
        Context::with_notification_context(k, |k, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: &mut self.records,
                attributes,
                context: Some(context),
                key_of: &key_of,
            };
            // Native defaults: use_merge=TRUE, no supplied merge object.
            crate::topology::brep::p7::copy_faces(
                k,
                source,
                faces,
                destination,
                true,
                &face_key,
                &mut env,
            )
            .map(|_| ())
        })
    }

    fn mapped_edges(&self) -> Result<Vec<EdgeId>> {
        Ok(self.merge.intersector.edges.mapped_primary())
    }

    fn mapped_vertices(&self) -> Result<Vec<VertexId>> {
        Ok(self.merge.intersector.vertices.mapped_primary())
    }

    fn common_edges(&self, primary: &Brep, _other: &Brep) -> Result<(Vec<EdgeId>, Vec<EdgeId>)> {
        Ok(self
            .merge
            .intersector
            .edges
            .common(bp::edges(primary, None)?))
    }

    fn common_vertices(
        &self,
        primary: &Brep,
        _other: &Brep,
    ) -> Result<(Vec<VertexId>, Vec<VertexId>)> {
        Ok(self
            .merge
            .intersector
            .vertices
            .common(bp::vertices(primary, None)?))
    }

    fn edge_mate(&self, edge: EdgeId) -> Option<EdgeId> {
        self.merge.intersector.edges.other(edge)
    }

    fn face_mate(&self, face: FaceId) -> Option<FaceId> {
        self.merge.intersector.faces.other(face)
    }

    fn relate_faces(
        &mut self,
        _primary: &mut Brep,
        face: FaceId,
        _other: &mut Brep,
        other_face: FaceId,
    ) -> Result<()> {
        // The face branch of Relate only records the pair; tolerance updates are
        // restricted to vertices/edges. MateMap::record performs the same checks.
        self.merge.intersector.faces.record(face, other_face)
    }

    fn fix_face_coincidence(
        &mut self,
        primary: &mut Brep,
        other: &mut Brep,
        face: FaceId,
        other_face: FaceId,
    ) -> Result<(Vec<FaceId>, Vec<FaceId>, bool)> {
        let mates = &self.merge.intersector.edges;
        p1::fix_found_face_coincidence(
            primary,
            other,
            face,
            other_face,
            mates.primary_to_other(),
            mates.other_to_primary(),
        )
    }

    fn process_wires(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        state: &mut ManifoldState,
        attributes: &mut AttributeStore,
        operation: BooleanOperation,
        primary_mark: MarkType,
        other_mark: MarkType,
        keep: &mut Vec<EdgeId>,
    ) -> Result<()> {
        let namespace = self.namespace;
        let source_key = |id| ObjectRef {
            key: entity_key(namespace, true, MergeEntity::Vertex(id)),
            type_code: crate::topology::vertex::TYPE,
        };
        let destination_key = |e| notification_key(namespace, false, e);
        let edge_source = |e| notification_key(namespace, true, EntityRef::Edge(e));
        let edge_destination = |e| notification_key(namespace, false, EntityRef::Edge(e));
        let face_source = |f| notification_key(namespace, true, EntityRef::Face(f));
        let face_destination = |f| notification_key(namespace, false, EntityRef::Face(f));
        let mut copy = TopologyCopy {
            vertices: p1::VertexCopy {
                records: &mut self.records,
                attributes,
                source_key: &source_key,
                destination_key: &destination_key,
            },
            edge_source_key: &edge_source,
            edge_destination_key: &edge_destination,
            face_source_key: &face_source,
            face_destination_key: &face_destination,
        };
        p5::process_wires_and_shell_vertices_with_kernels(
            kernels.primary,
            kernels.other.as_deref_mut(),
            primary,
            other,
            &mut state.primary_marks,
            &mut state.other_marks,
            &mut self.merge.intersector,
            &mut copy,
            operation,
            primary_mark,
            other_mark,
            keep,
        )
    }

    fn make_faceuse(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
        faceuse: FaceuseId,
        original_region: Option<RegionId>,
    ) -> Result<MadeFaceuse> {
        let namespace = self.namespace;
        let vertex_source = |id| ObjectRef {
            key: entity_key(namespace, true, MergeEntity::Vertex(id)),
            type_code: crate::topology::vertex::TYPE,
        };
        let destination = |e| notification_key(namespace, false, e);
        let edge_source = |e| notification_key(namespace, true, EntityRef::Edge(e));
        let edge_destination = |e| notification_key(namespace, false, EntityRef::Edge(e));
        let face_source = |f| notification_key(namespace, true, EntityRef::Face(f));
        let face_destination = |f| notification_key(namespace, false, EntityRef::Face(f));
        let mut copy = TopologyCopy {
            vertices: p1::VertexCopy {
                records: &mut self.records,
                attributes,
                source_key: &vertex_source,
                destination_key: &destination,
            },
            edge_source_key: &edge_source,
            edge_destination_key: &edge_destination,
            face_source_key: &face_source,
            face_destination_key: &face_destination,
        };
        // New topology is made with the primary context; copies notify through the
        // other operand's (or the shared) context.
        let ctx = match kernels.other.as_deref_mut() {
            Some(o) => MergeContext::Split(kernels.primary, Some(&mut o.context)),
            None => MergeContext::Shared(kernels.primary),
        };
        let mut faces = Vec::new();
        let made = p2::make_faceuse_with(
            ctx,
            primary,
            other,
            faceuse,
            original_region,
            &mut self.merge.intersector,
            &mut copy,
            Some(&mut faces),
        )?;
        Ok(MadeFaceuse {
            old_region: made.old_region,
            new_region: made.new_region,
            faces,
        })
    }

    fn find_region_of_other_face(
        &mut self,
        k: &Kernel,
        primary: &mut Brep,
        other: &mut Brep,
        faceuse: FaceuseId,
    ) -> Result<Option<RegionId>> {
        p1::find_brep_region_of_other_face(
            k,
            primary,
            other,
            faceuse,
            self.merge.intersector.edges.other_to_primary(),
        )
    }

    fn propagate_regions(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &Brep,
        other: &Brep,
        attributes: &mut AttributeStore,
        operation: BooleanOperation,
    ) -> Result<()> {
        let mut regions = Vec::new();
        for id in bp::regions(primary, None)? {
            regions.push(p1::RegionPropagation {
                region: ObjectRef {
                    key: self.object_key(false, MergeEntity::Region(id)),
                    type_code: REGION_TYPE,
                },
                is_void: primary.region(id)?.is_void() == 1,
                source_lists: [5, 6].map(|kind| ObjectRef {
                    key: key(self.namespace, false, kind, id.index()),
                    type_code: 0,
                }),
            });
        }
        let lookup = Regions {
            namespace: self.namespace,
            primary,
            other,
        };
        p1::propagate_other_src_region_attribs(
            &regions,
            attributes,
            Some(&mut kernels.primary.context),
            operation,
            Some(&lookup),
        )
    }

    fn post_process(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        state: &ManifoldState,
        check_all_edges: bool,
    ) -> Result<()> {
        // Move, rather than clone, the canonical relationships through the existing
        // cleanup API, restoring them on both success and failure.
        let cleanup = p1::CleanupRelations {
            edges: state.intersection_edges.clone(),
            vertices: state.intersection_vertices.clone(),
            mates: std::mem::take(&mut self.merge.intersector),
        };
        let result = p1::boolean_post_process(kernels.primary, primary, &cleanup, check_all_edges);
        self.merge.intersector = cleanup.mates;
        result
    }

    fn object_key(&self, other: bool, entity: MergeEntity) -> ObjectKey {
        entity_key(self.namespace, other, entity)
    }

    fn excluded_surface(&self, other: &Brep, face: FaceId) -> Result<bool> {
        match &self.subset {
            Some(subset) => Ok(!subset.contains(&Self::surface_identity(other, face)?)),
            None => Ok(false),
        }
    }

    fn clear_topology_deleted(&mut self) {
        self.topology_deleted = false;
        self.intersector.topology_deleted = false;
    }

    fn topology_deleted(&self) -> bool {
        self.topology_deleted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::attribute::{Attribute, Behavior};
    use crate::geom::context::NotifyCallback;
    use crate::geom::object::NotifyOperation;
    use crate::math::Vec3;
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::topology::topology_traverser::Entity;
    use crate::topology::{
        Edge, Edgeuse, EdgeuseId, EdgeuseOwner, Faceuse, Loopuse, LoopuseStart, Region, Vertexuse,
        VertexuseId, VertexuseOwner,
    };
    use std::sync::{Arc, Mutex};

    fn face(b: &mut Brep) -> FaceId {
        let shell = b.add_shell().unwrap();
        b.add_face(shell).unwrap()
    }

    #[test]
    fn canonical_relationships_drive_lists_and_face_helpers() {
        let mut adapter = KernelManifoldIntersector::new(7).unwrap();
        let mut a = Brep::new();
        let mut b = Brep::new();
        let v0 = a.add_vertex(Vec3::X).unwrap();
        let v1 = a.add_vertex(Vec3::Y).unwrap();
        let e0 = a.add_edge(v0, v1).unwrap();
        let e1 = a.add_edge(v1, v0).unwrap();
        adapter
            .merge
            .intersector
            .vertices
            .record(v1, VertexId::from_index(0))
            .unwrap();
        adapter
            .merge
            .intersector
            .vertices
            .record(v0, VertexId::from_index(1))
            .unwrap();
        adapter
            .merge
            .intersector
            .edges
            .record(e1, EdgeId::from_index(0))
            .unwrap();
        adapter
            .merge
            .intersector
            .edges
            .record(e0, EdgeId::from_index(1))
            .unwrap();
        assert_eq!(adapter.mapped_edges().unwrap(), [e1, e0]);
        assert_eq!(adapter.mapped_vertices().unwrap(), [v1, v0]);
        assert_eq!(
            adapter.common_edges(&a, &b).unwrap(),
            (
                vec![e0, e1],
                vec![EdgeId::from_index(1), EdgeId::from_index(0)]
            )
        );
        assert_eq!(adapter.common_vertices(&a, &b).unwrap().0, [v0, v1]);
        assert_eq!(adapter.edge_mate(e0), Some(EdgeId::from_index(1)));
        let f = face(&mut a);
        let of = face(&mut b);
        adapter.relate_faces(&mut a, f, &mut b, of).unwrap();
        assert_eq!(adapter.face_mate(f), Some(of));
        assert!(adapter
            .relate_faces(&mut a, f, &mut b, FaceId::from_index(8))
            .is_err());
        assert_eq!(adapter.face_mate(f), Some(of));
        assert_eq!(
            adapter.fix_face_coincidence(&mut a, &mut b, f, of).unwrap(),
            (vec![f], vec![of], false)
        );
        let r = a
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        assert_eq!(
            adapter
                .find_region_of_other_face(
                    &Kernel::default(),
                    &mut a,
                    &mut b,
                    FaceuseId::from_index(u32::MAX)
                )
                .unwrap(),
            Some(r)
        );
    }

    // Use both ends of an edge as a closed boundary, so the face helper really
    // visits a non-empty loop. The degenerate face needs no surface evaluation.
    fn face_on_wire(b: &mut Brep, edge: EdgeId) -> (FaceId, FaceuseId) {
        let f = face(b);
        let fu = b
            .add_faceuse_record(Faceuse {
                face: Some(f),
                shell: Some(b.face(f).unwrap().shell),
                ..Faceuse::default()
            })
            .unwrap();
        b.face_mut(f).unwrap().faceuse = Some(fu);
        let lu = b
            .add_loopuse_record(Loopuse {
                faceuse: Some(fu),
                ..Loopuse::default()
            })
            .unwrap();
        b.faceuse_mut(fu).unwrap().loopuses.push(lu);
        let uses = b.edge(edge).unwrap().edgeuses.clone();
        for (&eu, &next) in uses.iter().zip(uses.iter().cycle().skip(1)) {
            let data = b.edgeuse_mut(eu).unwrap();
            data.owner = EdgeuseOwner::Loopuse(Some(lu));
            data.ccw = Some(next);
            data.cw = Some(next);
        }
        b.loopuse_mut(lu).unwrap().start = LoopuseStart::Edgeuse(uses.first().copied());
        (f, fu)
    }

    #[test]
    fn coincidence_uses_each_operand_edge_map_direction() {
        let mut adapter = KernelManifoldIntersector::new(0).unwrap();
        let mut a = Brep::new();
        let mut b = Brep::new();
        let edge = wire(&mut a);
        wire(&mut b); // Offset the other arena's boundary id: 0 -> 1.
        let other_edge = wire(&mut b);
        assert_ne!(edge, other_edge);
        adapter
            .merge
            .intersector
            .edges
            .record(edge, other_edge)
            .unwrap();
        let (f, _) = face_on_wire(&mut a, edge);
        let (of, _) = face_on_wire(&mut b, other_edge);
        assert_eq!(
            adapter.fix_face_coincidence(&mut a, &mut b, f, of).unwrap(),
            (vec![f], vec![of], false)
        );
    }

    #[test]
    fn region_lookup_uses_other_to_primary_edge_mates() {
        let mut adapter = KernelManifoldIntersector::new(0).unwrap();
        let mut a = Brep::new();
        let mut b = Brep::new();
        let r = a
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        a.add_region_record(Region {
            in_brep: true,
            ..Region::default()
        })
        .unwrap();
        let edge = wire(&mut a);
        let eu = *a.edge(edge).unwrap().edgeuses.first().unwrap();
        let shell = crate::topology::edgeuse::p1::shell(&a, eu)
            .unwrap()
            .unwrap();
        a.shell_mut(shell).unwrap().region = Some(r);
        wire(&mut b); // Disjoint lookup keys between the two map directions.
        let other_edge = wire(&mut b);
        let (_, other_fu) = face_on_wire(&mut b, other_edge);
        adapter
            .merge
            .intersector
            .edges
            .record(edge, other_edge)
            .unwrap();
        assert_eq!(
            adapter
                .find_region_of_other_face(&Kernel::default(), &mut a, &mut b, other_fu)
                .unwrap(),
            Some(r)
        );
    }

    #[test]
    fn cleanup_restores_the_same_mates_on_both_result_exits() {
        let mut adapter = KernelManifoldIntersector::new(0).unwrap();
        let v = VertexId::from_index(u32::MAX);
        adapter.merge.intersector.vertices.record(v, v).unwrap();
        adapter
            .merge
            .intersector
            .faces
            .record(FaceId::from_index(0), FaceId::from_index(2))
            .unwrap();
        let mut primary = Brep::new();
        let mut k = Kernel::default();
        let mut kernels = BooleanKernels {
            primary: &mut k,
            other: None,
        };
        let mut state = ManifoldState::default();
        adapter
            .post_process(&mut kernels, &mut primary, &state, false)
            .unwrap();
        state.intersection_vertices.push(v);
        assert!(adapter
            .post_process(&mut kernels, &mut primary, &state, false)
            .is_err());
        assert_eq!(adapter.merge.intersector.vertices.other(v), Some(v));
        assert_eq!(
            adapter.face_mate(FaceId::from_index(0)),
            Some(FaceId::from_index(2))
        );
    }

    type Notice = (NotifyOperation, ObjectRef, [Option<ObjectRef>; 3]);
    #[derive(Debug)]
    struct Notices(Arc<Mutex<Vec<Notice>>>);
    impl NotifyCallback for Notices {
        fn execute(
            &mut self,
            op: NotifyOperation,
            caller: ObjectRef,
            d1: Option<ObjectRef>,
            d2: Option<ObjectRef>,
            d3: Option<ObjectRef>,
        ) -> Result<()> {
            self.0.lock().unwrap().push((op, caller, [d1, d2, d3]));
            Ok(())
        }
    }

    #[test]
    fn region_propagation_uses_live_regions_attributes_and_primary_callback() {
        let mut adapter = KernelManifoldIntersector::new(10).unwrap();
        let mut a = Brep::new();
        let mut b = Brep::new();
        let r = a
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        let o = b
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        a.region_mut(r).unwrap().set_is_void(0);
        b.region_mut(o).unwrap().set_is_void(0);
        let rk = adapter.object_key(false, MergeEntity::Region(r));
        let ok = adapter.object_key(true, MergeEntity::Region(o));
        let lookup = Regions {
            namespace: 10,
            primary: &a,
            other: &b,
        };
        assert_ne!(
            lookup.resolve_region(rk).unwrap().brep_id,
            lookup.resolve_region(ok).unwrap().brep_id
        );
        assert!(lookup.resolve_region(ObjectKey(0)).is_err());
        let mut attrs = AttributeStore::new();
        let h = attrs.insert(Attribute::merge_region(rk, 0, ok, 0)).unwrap();
        attrs.add_attribute(rk, h).unwrap();
        let color_id = 81;
        let color = attrs
            .insert(Attribute::color(color_id, Vec3::Y, Behavior::Copy))
            .unwrap();
        attrs.add_attribute(ok, color).unwrap();
        for (kind, expected) in [(5, rk), (6, ok)] {
            assert_eq!(
                adapter
                    .source_regions(
                        &attrs,
                        ObjectRef {
                            key: key(10, false, kind, r.index()),
                            type_code: 0
                        }
                    )
                    .unwrap(),
                [expected]
            );
        }
        let mut k = Kernel::default();
        let mut other_k = Kernel::default();
        let primary_notices = Arc::new(Mutex::new(Vec::new()));
        let other_notices = Arc::new(Mutex::new(Vec::new()));
        k.context
            .set_user_notify_callback(Some(Box::new(Notices(primary_notices.clone()))));
        other_k
            .context
            .set_user_notify_callback(Some(Box::new(Notices(other_notices.clone()))));
        let mut kernels = BooleanKernels {
            primary: &mut k,
            other: Some(&mut other_k),
        };
        adapter
            .propagate_regions(&mut kernels, &a, &b, &mut attrs, BooleanOperation::Merge)
            .unwrap();
        assert!(attrs.find_attribute(rk, ids::OTHER_REGION_ID).is_none());
        assert!(attrs.find_attribute(rk, color_id).is_some());
        assert!(attrs.find_attribute(ok, color_id).is_some());
        let notices = primary_notices.lock().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].0, NotifyOperation::RegPropagation);
        assert_eq!(notices[0].1.key, rk);
        assert_eq!(notices[0].2[0].unwrap().key, key(10, false, 5, r.index()));
        assert_eq!(notices[0].2[1].unwrap().key, key(10, false, 6, r.index()));
        assert_eq!(notices[0].2[2].unwrap().key, rk);
        assert!(other_notices.lock().unwrap().is_empty());
        drop(notices);
        // Upstream compares IsVoid() to TRUE, rather than treating every nonzero
        // state as void. Unknown must still propagate and issue its notification.
        a.region_mut(r).unwrap().set_is_void(-1);
        let merge = attrs.insert(Attribute::merge_region(rk, 0, ok, 0)).unwrap();
        attrs.add_attribute(rk, merge).unwrap();
        adapter
            .propagate_regions(&mut kernels, &a, &b, &mut attrs, BooleanOperation::Merge)
            .unwrap();
        assert!(attrs.find_attribute(rk, color_id).is_some());
        assert_eq!(primary_notices.lock().unwrap().len(), 2);
    }

    #[test]
    fn newly_partial_intersector_stages_restore_the_live_mates_on_error() {
        let mut adapter = KernelManifoldIntersector::new(3).unwrap();
        adapter.set_intersection_tolerances(1e-9, 0.3);
        let mut a = Brep::default();
        let mut b = Brep::default();
        let f = face(&mut a);
        let of = face(&mut b);
        let v = a.add_vertex(Vec3::ZERO).unwrap();
        let ov = b.add_vertex(Vec3::X).unwrap();
        adapter.relate_faces(&mut a, f, &mut b, of).unwrap();
        adapter.merge.intersector.vertices.record(v, ov).unwrap();
        assert!(adapter
            .remove_relationship(f, FaceId::from_index(of.index() + 1))
            .is_err());
        assert_eq!(adapter.face_mate(f), Some(of));
        adapter.remove_relationship(f, of).unwrap();
        assert_eq!(adapter.face_mate(f), None);
        assert_eq!(adapter.merge.intersector.faces.primary(of), None);
        assert_eq!(adapter.merge.intersector.vertices.primary(ov), Some(v));
        let mut k = Kernel::default();
        let mut attrs = AttributeStore::default();
        let mut kernels = BooleanKernels {
            primary: &mut k,
            other: None,
        };
        let out = adapter
            .fix_gaps(&mut kernels, &mut a, &mut b, &mut attrs, &[v])
            .unwrap();
        assert!(!out.modified_topology);
        adapter
            .delete_faces(&mut kernels, &mut a, &mut attrs, &[], Some(&[]))
            .unwrap();
        // (SmBrep::GetFaces is wired since #274, so the walk now reaches the test's
        // surface-less faces, which upstream dereferences)
        let err = adapter
            .intersect_insert_relate(&mut kernels, &mut a, &mut b, &mut attrs)
            .unwrap_err();
        assert!(
            matches!(
                err,
                SolidError::InvalidInput {
                    operation: "boolean::topology_intersector::p2::intersect_topology",
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(adapter.intersector.proto_topology_manager().is_some());
        assert_eq!(adapter.merge.intersector.vertices.other(v), Some(ov));
        assert_eq!(adapter.merge.intersector.vertices.primary(ov), Some(v));
        assert_eq!(adapter.mapped_vertices().unwrap(), [v]);
    }

    #[test]
    fn notification_scope_preserves_marks_and_both_operand_callbacks_on_error() {
        use crate::boolean::topology_intersector::p1::IntersectorBrep;
        let mut k = Kernel::default();
        let mut other_k = Kernel::default();
        k.context.set_doing_boolean(true);
        let mut lock = MarkLock::new(
            std::slice::from_mut(&mut k.context),
            0,
            crate::geom::context::ALL_MARKS,
        )
        .unwrap();
        let mark = lock.mark_type();
        let primary_events = Arc::new(Mutex::new(Vec::new()));
        let other_events = Arc::new(Mutex::new(Vec::new()));
        k.context
            .set_user_notify_callback(Some(Box::new(Notices(primary_events.clone()))));
        other_k
            .context
            .set_user_notify_callback(Some(Box::new(Notices(other_events.clone()))));
        let mut a = Brep::default();
        let mut b = Brep::default();
        let v = a.add_vertex(Vec3::ZERO).unwrap();
        let ov = b.add_vertex(Vec3::X).unwrap();
        let mut records = BTreeMap::new();
        records.insert(
            notification_key(4, false, EntityRef::Vertex(v)).key,
            Topology::new(),
        );
        records.insert(
            notification_key(4, true, EntityRef::Vertex(ov)).key,
            Topology::new(),
        );
        let mut attrs = AttributeStore::default();
        let mut curves = Vec::new();
        let mut extra = Vec::new();
        let current_mark = k.context.current_mark(mark);
        let error: Result<()> = Context::with_notification_context(&mut k, |k, context| {
            assert!(k.context.is_mark_locked(mark));
            assert_eq!(k.context.current_mark(mark), current_mark);
            assert!(context.doing_boolean());
            let settings = cache_kernels(k, Some(&other_k), context);
            let mut pair = ArenaPair::new(
                &mut a,
                Some(&mut b),
                &mut curves,
                &mut extra,
                &mut records,
                &mut attrs,
                settings,
                context,
                Some(&mut other_k.context),
                4,
            );
            pair.set_vertex_point(k, arena_pair::encode(EntityRef::Vertex(v), false), Vec3::Y)?;
            pair.set_vertex_point(k, arena_pair::encode(EntityRef::Vertex(ov), true), Vec3::Z)?;
            Err(SolidError::sm_err(OP))
        });
        assert!(error.is_err());
        assert!(k.context.user_notify_callback().is_some());
        assert!(other_k.context.user_notify_callback().is_some());
        assert!(k.context.is_mark_locked(mark));
        assert_eq!(a.vertex(v).unwrap().point, Vec3::Y);
        assert_eq!(b.vertex(ov).unwrap().point, Vec3::Z);
        assert_eq!(primary_events.lock().unwrap().len(), 1);
        assert_eq!(other_events.lock().unwrap().len(), 1);
        assert_eq!(
            primary_events.lock().unwrap()[0].1.key,
            notification_key(4, false, EntityRef::Vertex(v)).key
        );
        assert_eq!(
            other_events.lock().unwrap()[0].1.key,
            notification_key(4, true, EntityRef::Vertex(ov)).key
        );
        lock.release(std::slice::from_mut(&mut k.context)).unwrap();
    }

    #[test]
    fn transform_delegates_identity_and_reports_the_actual_boundary() {
        let mut adapter = KernelManifoldIntersector::new(5).unwrap();
        let mut k = Kernel::default();
        let mut b = Brep::default();
        let mut attrs = AttributeStore::default();
        let mut kernels = BooleanKernels {
            primary: &mut k,
            other: None,
        };
        adapter
            .transform_brep(
                &mut kernels,
                &mut b,
                false,
                &mut attrs,
                &Axis2Placement::default(),
                None,
                true,
            )
            .unwrap();
        let mut placement = Axis2Placement::default();
        placement.translate(Vec3::X);
        // SmBrep::Notify is topology.brep_p2's (#149) and now runs through the adapter's
        // environment; the vertex walk (GetVertices) is wired, so the transform then
        // stops at its curve walk
        let r = adapter.transform_brep(
            &mut kernels,
            &mut b,
            false,
            &mut attrs,
            &placement,
            None,
            true,
        );
        assert!(
            matches!(
                r,
                Err(SolidError::Unported {
                    unit: "topology.brep_p1",
                    upstream: "SmBrep::GetCurves"
                })
            ),
            "transform: {r:?}"
        );
        let id = 17;
        let entities = [
            EntityRef::Region(RegionId::from_index(id)),
            EntityRef::Face(FaceId::from_index(id)),
            EntityRef::Edge(EdgeId::from_index(id)),
            EntityRef::Vertex(VertexId::from_index(id)),
            EntityRef::Edgeuse(EdgeuseId::from_index(id)),
            EntityRef::Vertexuse(VertexuseId::from_index(id)),
        ];
        let keys: BTreeSet<_> = entities
            .into_iter()
            .flat_map(|e| {
                [
                    notification_key(5, false, e).key,
                    notification_key(5, true, e).key,
                ]
            })
            .collect();
        assert_eq!(keys.len(), 12);
    }

    fn wire(b: &mut Brep) -> EdgeId {
        let shell = b.add_shell().unwrap();
        let v = b.add_vertex(Vec3::ZERO).unwrap();
        let w = b.add_vertex(Vec3::X).unwrap();
        let e = b.add_edge_record(Edge::new([v, w])).unwrap();
        for (vertex, orientation) in [(v, OrientType::Same), (w, OrientType::Opposite)] {
            let eu = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(e),
                    owner: EdgeuseOwner::Shell(Some(shell)),
                    orientation,
                    ..Edgeuse::default()
                })
                .unwrap();
            let vu = b
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(vertex),
                    owner: VertexuseOwner::Edgeuse(Some(eu)),
                    ..Vertexuse::default()
                })
                .unwrap();
            b.edgeuse_mut(eu).unwrap().vertexuse = Some(vu);
            b.edge_mut(e).unwrap().edgeuses.push(eu);
            b.vertex_mut(vertex).unwrap().vertexuses.push(vu);
        }
        let data = b.edge_mut(e).unwrap();
        data.curve = Some(crate::curve::line::Line::from_points(Vec3::ZERO, Vec3::X, 3).into());
        data.interval = crate::geom::extent1d::Extent1d::from_raw(0.0, 1.0);
        e
    }

    #[test]
    fn wire_stage_uses_other_context_marks_and_resets_output() {
        let mut adapter = KernelManifoldIntersector::new(0).unwrap();
        let mut a = Brep::new();
        let mut b = Brep::new();
        let ae = wire(&mut a);
        let be = wire(&mut b);
        adapter.merge.intersector.edges.record(ae, be).unwrap();
        let mut k = Kernel::default();
        let mut other_k = Kernel::default();
        let lock = MarkLock::new(std::slice::from_mut(&mut other_k.context), 0, 1).unwrap();
        let mark = lock.mark_type();
        assert_ne!(
            k.context.current_mark(mark),
            other_k.context.current_mark(mark)
        );
        let mut state = ManifoldState::default();
        state.other_marks.mark(&other_k, Entity::Edge(be), mark);
        for v in b.vertices().ids() {
            state.other_marks.mark(&other_k, Entity::Vertex(v), mark);
        }
        let mut kernels = BooleanKernels {
            primary: &mut k,
            other: Some(&mut other_k),
        };
        let mut keep = vec![EdgeId::from_index(123)];
        adapter
            .process_wires(
                &mut kernels,
                &mut a,
                &mut b,
                &mut state,
                &mut AttributeStore::new(),
                BooleanOperation::Merge,
                mark,
                mark,
                &mut keep,
            )
            .unwrap();
        assert_eq!(keep, [ae]);
    }

    #[test]
    fn surface_subset_uses_shared_owner_identity_and_can_be_cleared() {
        let mut adapter = KernelManifoldIntersector::new(0).unwrap();
        let mut b = Brep::new();
        let f0 = face(&mut b);
        let f1 = face(&mut b);
        let f2 = face(&mut b);
        let mut surface: crate::surface::surface::Surface =
            crate::surface::plane::p1::Plane::from_normal(Vec3::ZERO, Vec3::Z).into();
        *surface.owner_mut() = Some(arena_identity(CacheObject::Face(f0)));
        b.face_mut(f0).unwrap().surface = Some(surface.clone());
        b.face_mut(f1).unwrap().surface = Some(surface);
        b.face_mut(f2).unwrap().surface =
            Some(crate::surface::plane::p1::Plane::from_normal(Vec3::ZERO, Vec3::Z).into());
        adapter.set_surface_subset(&[KernelManifoldIntersector::surface_identity(&b, f0).unwrap()]);
        assert!(!adapter.excluded_surface(&b, f0).unwrap());
        assert!(!adapter.excluded_surface(&b, f1).unwrap());
        assert!(adapter.excluded_surface(&b, f2).unwrap());
        adapter.set_surface_subset(&[]);
        assert!(!adapter.excluded_surface(&b, f2).unwrap());
        adapter.topology_deleted = true;
        assert!(adapter.topology_deleted());
        adapter.clear_topology_deleted();
        assert!(!adapter.topology_deleted());
    }

    #[test]
    fn notification_key_blocks_never_alias_across_operands_or_classes() {
        let a = KernelManifoldIntersector::new(0).unwrap();
        let b = KernelManifoldIntersector::new(0x07ff_ffff).unwrap();
        assert!(KernelManifoldIntersector::new(0x0800_0000).is_err());
        let mut keys = BTreeSet::new();
        for adapter in [&a, &b] {
            for other in [false, true] {
                for i in [0, u32::MAX] {
                    for entity in [
                        MergeEntity::Vertex(VertexId::from_index(i)),
                        MergeEntity::Edge(EdgeId::from_index(i)),
                        MergeEntity::Face(FaceId::from_index(i)),
                        MergeEntity::Region(RegionId::from_index(i)),
                    ] {
                        assert!(keys.insert(adapter.object_key(other, entity)));
                    }
                }
            }
        }
    }
}
