//! Typed access to the topology services called by the sweep algorithm.
//!
//! Upstream: `source/SMLib/src/SmTopologySweep.cpp` (calls to other units).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`SweepAccess`] | the B-rep/context/topology services used by `SmTopologySweep` |
//!
//! Deviations: a statically dispatched access interface keeps B-rep and context
//! identities explicit. Defaults below defer only the named unavailable callee;
//! the sweep's remaining control flow is implemented by the caller of each service.

use super::{SweepEntity, TopologySweep};
use crate::blend::hcr::{Hcr, HcrOptions};
use crate::curve::curve::Curve;
use crate::error::EntityRef;
use crate::error::{Result, SolidError};
use crate::geom::attribute::TagData;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::Surface;
use crate::topology::edge::NotifyEnv;
use crate::topology::{Brep, BrepId, EdgeId, EdgeuseId, FaceId, RegionId, ShellId, VertexId};

/// A region and its owning B-rep.
pub type RegionRef = (BrepId, RegionId);
/// A shell and its owning B-rep.
pub type ShellRef = (BrepId, ShellId);
/// A face and its owning B-rep.
pub type FaceRef = (BrepId, FaceId);
/// An edge and its owning B-rep.
pub type EdgeRef = (BrepId, EdgeId);
/// A vertex and its owning B-rep.
pub type VertexRef = (BrepId, VertexId);
/// An edge use and its owning B-rep.
pub type EdgeuseRef = (BrepId, EdgeuseId);

/// Owned inputs to `SmBrep::MakeFaceWithCurves`.
#[derive(Debug)]
pub struct FaceInput {
    /// Target region.
    pub region: RegionRef,
    /// Edge count of each non-vertex loop.
    pub loop_counts: Vec<usize>,
    /// Trimming curves in loop order.
    pub curves: Vec<Curve>,
    /// Optional UV trimming curves.
    pub uv_curves: Option<Vec<Curve>>,
    /// Orientations in loop order.
    pub orientations: Vec<OrientType>,
    /// Locations of vertex loops.
    pub loop_points: Vec<Point3>,
    /// Surface being bounded.
    pub surface: Surface,
    /// Domain of that surface.
    pub domain: Extent2d,
    /// Face/surface orientation.
    pub orientation: OrientType,
}

/// Outputs of `MakeFaceWithCurves`, retaining each input curve's new owner.
#[derive(Debug)]
pub struct FaceBuild {
    /// Created face.
    pub face: FaceRef,
    /// Owner of each input curve, in input order; null curves have no owner.
    pub curve_edges: Vec<Option<EdgeRef>>,
}

/// Identifies a caller-owned `SmMerge` port instance for its scoped notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepMergeId(pub u32);

/// Hidden-curve-removal scene. The access implementation retains the HCR session
/// until `delete_brep(brep)`, so silhouettes are queried after copying the scene.
#[derive(Debug)]
pub struct SweepScene {
    /// HCR scene B-rep, owned by this scoped session.
    pub brep: BrepId,
}

macro_rules! deferred {
    ($(#[$doc:meta])* fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $out:ty; $(,)? $unit:literal, $symbol:literal $(,)?) => {
        $(#[$doc])*
        fn $name(&mut self, $($arg: $ty),*) -> Result<$out> {
            $(let _ = $arg;)*
            Err(SolidError::Unported { upstream: $symbol, unit: $unit })
        }
    };
}

/// B-rep resolution and external services for a topology sweep.
///
/// Implementations must resolve contexts and attribute keys using the caller's
/// object identities. Available defaults call the existing ports; unavailable
/// dependencies return explicit `Unported` errors without reproducing open PRs.
pub trait SweepAccess {
    /// Resolve a B-rep without changing its topology.
    fn brep(&self, id: BrepId) -> Result<&Brep>;
    /// Resolve a mutable B-rep.
    fn brep_mut(&mut self, id: BrepId) -> Result<&mut Brep>;
    /// Context of a B-rep, including its tolerances and caches.
    fn kernel(&self, id: BrepId) -> Result<&Kernel>;
    /// Mutable context; B-reps sharing a context must resolve to the same kernel.
    fn kernel_mut(&mut self, id: BrepId) -> Result<&mut Kernel>;
    /// Borrow a B-rep with its actual caller context and face identity resolver.
    /// The resolver also covers future face ids allocated by the operation.
    /// `NotifyEnv` borrows persistent notification records and attributes. Context
    /// notifications use `with_brep_notifications` to avoid aliasing the kernel.
    fn with_brep_mut<T>(
        &mut self,
        brep: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &dyn Fn(FaceId) -> ObjectRef,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T>;
    /// Borrow distinct source and destination B-reps, using the destination's
    /// actual mutable context, face identity resolver and persistent notification
    /// records/attributes. The engine only copies between
    /// distinct B-reps; implementations must reject aliasing rather than fabricate
    /// a second mutable reference. Face identities must agree with `NotifyEnv::key_of`.
    fn with_breps_mut<T>(
        &mut self,
        source: BrepId,
        destination: BrepId,
        body: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(FaceId) -> ObjectRef,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T>;
    /// Borrow topology with its persistent records, attributes, entity resolver
    /// and the owning kernel's mutable notification context.
    fn with_brep_notifications<T>(
        &mut self,
        brep: BrepId,
        body: impl FnOnce(&mut Brep, &mut NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T>;
    /// Retain the entire HCR session until `delete_brep`, resolving its actual
    /// scene through `brep`, `brep_mut` and the borrowing callbacks.
    fn retain_hcr(&mut self, context_of: BrepId, hcr: Hcr) -> Result<BrepId>;
    /// Resolve the retained session belonging to this scene.
    fn hcr_session(&self, scene: BrepId) -> Result<&Hcr>;
    /// Allocate an empty B-rep in `context_of`'s context.
    fn new_brep(&mut self, context_of: BrepId) -> Result<BrepId>;
    /// Release live temporary topology and geometry, including on error.
    /// For an HCR scene, drop the retained session and unregister its scene id.
    /// A successful `union` already consumes its source, so it is never released twice.
    fn delete_brep(&mut self, id: BrepId);
    /// Find a tag by resolved object identity.
    fn tag(&self, entity: SweepEntity) -> Result<Option<TagData>>;
    /// Attach a tag using `SmAObject::AddAttribute` semantics.
    fn add_tag(&mut self, entity: SweepEntity, tag: TagData) -> Result<()>;
    /// Find the tag on an edge's original curve (null curve is an error).
    fn curve_tag(&self, edge: EdgeRef) -> Result<Option<TagData>>;
    /// Read/write the upstream editing flag. Return its previous value.
    fn set_editing(&mut self, brep: BrepId, editing: bool) -> Result<bool>;

    /// Release a scoped merge object and its notification registrations.
    fn delete_merge(&mut self, merge: SweepMergeId);

    // UNPORTED(boolean.merge_p2): SmMerge::SmMerge
    deferred!(
        /// Construct the merge notifier with both operands and caller context.
        fn new_merge(
            context_of: BrepId,
            destination: BrepId,
            source: BrepId,
            tolerance: f64,
            angle: f64,
        ) -> SweepMergeId;,
        "boolean.merge_p2",
        "SmMerge::SmMerge"
    );

    /// Keep a merge notifier alive for the complete upstream scope, releasing it
    /// after success or an early error.
    fn with_merge<T>(
        &mut self,
        context_of: BrepId,
        destination: BrepId,
        source: BrepId,
        tolerance: f64,
        angle: f64,
        body: impl FnOnce(&mut Self, SweepMergeId) -> Result<T>,
    ) -> Result<T>
    where
        Self: Sized,
    {
        let merge = self.new_merge(context_of, destination, source, tolerance, angle)?;
        let result = body(self, merge);
        self.delete_merge(merge);
        result
    }

    /// `SmBrep::GetInfiniteRegion`; a null region retains the upstream NER error.
    fn infinite_region(&mut self, brep: BrepId) -> Result<RegionId> {
        self.brep(brep)?
            .infinite_region()
            .ok_or_else(|| SolidError::null_pointer(super::OP))
    }
    /// `SmFace::GetEdges`, using the existing faithful co-port.
    fn face_edges(&mut self, face: FaceRef) -> Result<Vec<EdgeId>> {
        crate::topology::topology_traverser::face_edges(self.brep(face.0)?, face.1)
    }
    /// `SmFace::GetVertices`, using the existing faithful co-port.
    fn face_vertices(&mut self, face: FaceRef) -> Result<Vec<VertexId>> {
        crate::query::face::p2::face_vertices(self.brep(face.0)?, face.1, false, None)
    }
    /// Acquire the caller context's unlocked mark and retain its lock until release.
    fn lock_mark(&mut self, brep: BrepId, mask: i32) -> Result<crate::geom::context::MarkType>;
    /// Release that mark lock (also on an early return).
    fn unlock_mark(&mut self, brep: BrepId, mark: crate::geom::context::MarkType) -> Result<()>;
    /// Query the persistent topology mark record using the owning context.
    fn is_marked(&self, entity: SweepEntity, mark: crate::geom::context::MarkType) -> Result<bool>;
    /// Set the persistent topology mark record using the owning context.
    fn mark(&mut self, entity: SweepEntity, mark: crate::geom::context::MarkType) -> Result<()>;
    /// `SmTopologyTraverser::CollectFaces` with the current lock and mark records.
    fn collect_faces(
        &mut self,
        face: FaceRef,
        mark: crate::geom::context::MarkType,
        faces: &mut Vec<FaceId>,
    ) -> Result<()>;

    /// `SmBrep::MakeShellVertex`, including its owning notification context.
    fn make_shell_vertex(
        &mut self,
        region: RegionRef,
        point: Point3,
    ) -> Result<(ShellRef, VertexRef)> {
        self.with_brep_notifications(region.0, |brep, env| {
            let (shell, vertex) =
                crate::topology::brep::p9::make_shell_vertex(brep, Some(region.1), point, env)?;
            Ok(((region.0, shell), (region.0, vertex)))
        })
    }
    // UNPORTED(topology.brep_p3): SmBrep::MakeWireEdgeVertex
    deferred!(
        /// `SmBrep::MakeWireEdgeVertex`.
        fn make_wire_edge_vertex(
            region: RegionRef,
            start: VertexRef,
            curve: Curve,
            interval: Extent1d,
            orientation: OrientType,
            end: Point3,
        ) -> (EdgeRef, VertexRef);,
        "topology.brep_p3",
        "SmBrep::MakeWireEdgeVertex"
    );
    // UNPORTED(topology.brep_p3): SmBrep::MakeWireEdge
    deferred!(
        /// `SmBrep::MakeWireEdge`.
        fn make_wire_edge(
            region: RegionRef,
            start: VertexRef,
            end: VertexRef,
            curve: Curve,
            interval: Extent1d,
            orientation: OrientType,
        ) -> EdgeRef;,
        "topology.brep_p3",
        "SmBrep::MakeWireEdge"
    );
    /// `SmBrep::MakeFaceWithCurves`, retaining owners of the original curve slots.
    fn make_face(&mut self, input: FaceInput) -> Result<FaceBuild> {
        let FaceInput {
            region,
            loop_counts,
            curves,
            uv_curves,
            orientations,
            loop_points,
            surface,
            domain,
            orientation,
        } = input;
        self.with_brep_mut(region.0, |kernel, brep, face_objects, env| {
            let mut owners = Vec::new();
            let built = crate::topology::brep::p7::make_face_with_curve_owners(
                kernel,
                brep,
                Some(region.1),
                crate::topology::brep::p7::FaceCurvesInput {
                    loops: loop_counts,
                    curves_3d: Some(curves.into_iter().map(Some).collect()),
                    uv_curves,
                    orientations,
                    loop_points,
                },
                surface,
                &domain,
                orientation,
                None,
                face_objects,
                &mut owners,
                env,
            )?;
            Ok(FaceBuild {
                face: (
                    region.0,
                    built
                        .face
                        .ok_or_else(|| SolidError::null_pointer(super::OP))?,
                ),
                curve_edges: owners
                    .into_iter()
                    .map(|edge| edge.map(|id| (region.0, id)))
                    .collect(),
            })
        })
    }
    /// `SmFace::SetTolerance` with upstream's default flags and double conversion.
    fn set_face_tolerance(&mut self, face: FaceRef, tolerance: f64) -> Result<()> {
        let tolerance = crate::geom::tol::zone_tol3d(self.kernel(face.0)?, tolerance);
        crate::topology::face::p3::set_tolerance(
            self.brep_mut(face.0)?,
            face.1,
            tolerance,
            true,
            true,
        )
    }
    /// `SmBrep::MakeVertexLoop`, with upstream's default opposite orientation.
    fn make_vertex_loop(&mut self, face: FaceRef, point: Point3) -> Result<VertexRef> {
        self.with_brep_notifications(face.0, |brep, env| {
            let (_, vertex) = crate::topology::brep::p9::make_vertex_loop(
                brep,
                face.1,
                point,
                OrientType::Opposite,
                env,
            )?;
            Ok((face.0, vertex))
        })
    }
    /// `SmBrep::MakeEdgeInFace` (the new loop and face outputs are unused).
    fn make_edge_in_face(
        &mut self,
        face: FaceRef,
        start: VertexRef,
        end: VertexRef,
        curve: Curve,
        uv_curve: Option<Curve>,
        interval: Extent1d,
        orientation: OrientType,
        tolerance: f64,
    ) -> Result<EdgeRef> {
        let editing = self.brep_mut(face.0)?.editing_enabled();
        if editing && (start.0 != face.0 || end.0 != face.0) {
            return Err(SolidError::invalid_input(
                super::OP,
                "edge vertices belong to a different B-rep",
            ));
        }
        let mut output = crate::topology::brep::p5::EdgeInFace::default();
        self.with_brep_mut(face.0, |k, brep, _face_ref, env| {
            crate::topology::brep::p5::make_edge_in_face_with_uv_curve(
                k,
                brep,
                editing,
                Some(face.1),
                Some(start.1),
                Some(end.1),
                curve,
                uv_curve,
                interval,
                orientation,
                tolerance,
                &mut output,
                Some(env),
            )
        })?;
        Ok((
            face.0,
            output
                .edge
                .ok_or_else(|| SolidError::null_pointer(super::OP))?,
        ))
    }
    /// `SmBrep::GetFaces`, preserving the owning list's order.
    fn faces(&mut self, brep: BrepId) -> Result<Vec<FaceId>> {
        crate::topology::brep::p1::faces(self.brep(brep)?, None)
    }
    /// `SmRegion::GetShells` without an attribute filter.
    fn region_shells(&mut self, region: RegionRef) -> Result<Vec<ShellId>> {
        self.brep(region.0)?
            .region(region.1)?
            .get_shells(None, |_, _| Ok(true))
    }
    /// Faces of the shell's face uses, retaining duplicates and list order.
    fn shell_faceuse_faces(&mut self, shell: ShellRef) -> Result<Vec<FaceId>> {
        let brep = self.brep(shell.0)?;
        brep.shell(shell.1)?
            .get_faceuses()?
            .into_iter()
            .map(|id| {
                brep.faceuse(id)?
                    .face
                    .ok_or_else(|| SolidError::null_pointer(super::OP))
            })
            .collect()
    }
    /// `SmBrep::RemoveFaces` using the actual editing flag and context marks.
    fn remove_faces(&mut self, brep: BrepId, faces: &[FaceId]) -> Result<()> {
        self.with_brep_mut(brep, |kernel, brep, _, _| {
            crate::topology::brep::p5::remove_faces(kernel, brep, brep.editing_enabled(), faces)
        })
    }
    /// HCR constructor, silhouette mode, visibility and scene ownership.
    fn hcr(
        &mut self,
        brep: BrepId,
        inverse: Axis2Placement,
        tolerance: f64,
        angle: f64,
        seams: bool,
        smooth: bool,
        silhouettes: bool,
    ) -> Result<SweepScene> {
        // The scene copy notifies through an arena-only store with the source in a
        // key namespace of its own, as boolean::merge::p3's list-operand copies.
        fn source_ref(e: EntityRef) -> ObjectRef {
            let r = crate::topology::loopuse::notify_ref(e);
            ObjectRef {
                type_code: r.type_code,
                key: crate::geom::attribute::ObjectKey(r.key.0 | 1 << 40),
            }
        }
        let opts = HcrOptions {
            approx_tol_3d: tolerance,
            ang_tol_rad: angle,
            display_seams: seams,
            display_smooth_edges: smooth,
        };
        let mut hcr = self.with_brep_mut(brep, |kernel, model, _, _| {
            let mut state = crate::topology::brep::p2::ArenaNotifyState::default();
            Hcr::with_transform(
                kernel,
                &mut state.env(None),
                model,
                &source_ref,
                &inverse,
                opts,
            )
        })?;
        hcr.set_hcr_for_silhouettes(silhouettes);
        crate::error::ignore_status(hcr.compute_global_visibility(self.kernel_mut(brep)?))?;
        Ok(SweepScene {
            brep: self.retain_hcr(brep, hcr)?,
        })
    }
    /// Query the retained HCR session at the original upstream call site.
    fn silhouette_edges(&mut self, scene: BrepId) -> Result<Vec<EdgeId>> {
        Ok(self.hcr_session(scene)?.silhouette_edges())
    }
    /// `SmBrep::GetWireEdges`, preserving the owning list's order.
    fn wire_edges(&mut self, brep: BrepId) -> Result<Vec<EdgeId>> {
        crate::topology::brep::p1::wire_edges(self.brep(brep)?, None)
    }
    /// `SmBrep::GetShellVertices` in the owning B-rep's vertex-list order.
    fn shell_vertices(&mut self, brep: BrepId) -> Result<Vec<VertexId>> {
        crate::topology::brep::p2::shell_vertices(self.brep(brep)?, None)
    }
    /// `SmBrep::CopyEdges`, using the destination's context.
    fn copy_edges(
        &mut self,
        source: BrepId,
        edges: &[EdgeId],
        destination: BrepId,
    ) -> Result<Vec<EdgeId>> {
        self.with_breps_mut(source, destination, |kernel, source, destination, _, _| {
            crate::topology::brep::p6::copy_edges(kernel, source, edges, destination)
        })
    }
    // UNPORTED(topology.brep_p3): SmBrep::CopyVertices
    deferred!(
        /// `SmBrep::CopyVertices` with its output list supplied.
        fn copy_vertices(
            source: BrepId,
            vertices: &[VertexId],
            destination: BrepId,
        ) -> Vec<VertexId>;,
        "topology.brep_p3",
        "SmBrep::CopyVertices"
    );
    /// `SmBrep::RemoveAllTopology`, using its actual mutable caller context.
    fn remove_all_topology(&mut self, brep: BrepId) -> Result<()> {
        self.with_brep_mut(brep, |kernel, brep, _, _| {
            crate::topology::brep::p10::remove_all_topology(kernel, brep)
        })
    }
    /// `SmBrep::CopyFaces` with the scoped merge notifier's edge mates.
    fn copy_faces(
        &mut self,
        source: BrepId,
        faces: &[FaceId],
        destination: BrepId,
        use_merge: bool,
        merge: SweepMergeId,
        edge_mates: &mut BTreeEdgeMap,
    ) -> Result<()> {
        let status = self.with_breps_mut(
            source,
            destination,
            |kernel, source, destination, face_objects, env| {
                // Keep resolver errors outside the native copy status: a
                // declined borrow must not become a deferred notifier lookup.
                Ok(crate::topology::brep::p7::copy_faces(
                    kernel,
                    source,
                    faces,
                    destination,
                    use_merge,
                    face_objects,
                    env,
                )
                .and_then(|(_, all_ok)| {
                    if all_ok {
                        Ok(())
                    } else {
                        Err(SolidError::sm_err(super::OP))
                    }
                }))
            },
        )?;
        if status.as_ref().is_err_and(|error| {
            error.is_unported()
                || matches!(
                    error,
                    SolidError::InvalidInput { .. } | SolidError::NotFound { .. }
                )
        }) {
            return status;
        }
        // Keep notifications from completed copies even when a later copy fails.
        self.merge_edge_mates(merge, edge_mates)?;
        status
    }

    // UNPORTED(boolean.merge_p2): SmMerge::SmMerge (copy-notification registration)
    deferred!(
        /// Resolve edge mates recorded by the live merge notifier after copying.
        /// Override with `new_merge` once its constructor installs copy registrations;
        /// the intersector's `GetThisMate` map lookup itself is already ported.
        fn merge_edge_mates(merge: SweepMergeId, edge_mates: &mut BTreeEdgeMap) -> ();,
        "boolean.merge_p2",
        "SmMerge::SmMerge"
    );
    /// `SmBrep::Transform` with default scale and tolerance-scaling arguments.
    fn transform(&mut self, brep: BrepId, frame: Axis2Placement) -> Result<()> {
        self.with_brep_mut(brep, |kernel, brep, _, _| {
            crate::topology::brep::p5::transform(kernel, brep, &frame, None, true)
        })
    }
    // UNPORTED(query.brep): SmBrep::CalculateBoundingBox
    deferred!(
        /// `SmBrep::CalculateBoundingBox`.
        fn bounding_box(brep: BrepId, output: &mut Extent3d) -> ();,
        "query.brep",
        "SmBrep::CalculateBoundingBox"
    );
    /// `SmEdgeuse::EvaluateBinormal`. Upstream writes these two outputs only after
    /// all fallible evaluations succeed (the optional UV vector is not requested).
    fn binormal(
        &mut self,
        edgeuse: EdgeuseRef,
        parameter: f64,
        use_3d_only: bool,
        point: &mut Point3,
        binormal: &mut Vec3,
    ) -> Result<()> {
        self.with_brep_mut(edgeuse.0, |kernel, brep, _, _| {
            let evaluated = crate::topology::edgeuse::p3::evaluate_binormal(
                kernel,
                brep,
                edgeuse.1,
                parameter,
                use_3d_only,
                &crate::topology::edgeuse::p3::BinormalOptions::default(),
            )?;
            *point = evaluated.point;
            *binormal = evaluated.binormal;
            Ok(())
        })
    }
    /// `SmFace::HasLaminaEdge`, using the existing traversal co-port.
    fn has_lamina_edge(&mut self, face: FaceRef) -> Result<bool> {
        crate::topology::topology_traverser::has_lamina_edge(self.brep(face.0)?, face.1)
    }
    // UNPORTED(topology.brep_p3): SmBrep::MergeBrep
    deferred!(
        /// `SmBrep::MergeBrep`.
        fn merge_brep(destination: BrepId, source: BrepId) -> ();,
        "topology.brep_p3",
        "SmBrep::MergeBrep"
    );
    // UNPORTED(boolean.merge_p3): SmMerge::PiecewiseMerge
    deferred!(
        /// `SmMerge::PiecewiseMerge(NULL, TRUE, TRUE, ...)`.
        fn piecewise_merge(
            merge: SweepMergeId,
            use_existing_topology: bool,
            create_new_breps_for_faces: bool,
        ) -> BrepId;,
        "boolean.merge_p3",
        "SmMerge::PiecewiseMerge"
    );
    /// Sweep callback, default targets and caller-selected manifold mode.
    fn stitch(
        &mut self,
        brep: BrepId,
        sweep: &mut TopologySweep,
        tolerance: f64,
        making_manifold_solid: bool,
    ) -> Result<()> {
        self.with_brep_mut(brep, |kernel, model, _, _| {
            let mut callback = SweepStitchCallback { brep, sweep };
            let mut stitch =
                crate::heal::stitch::p1::Stitch::new(&mut callback, tolerance, true, true);
            stitch.making_manifold_solid = making_manifold_solid;
            stitch
                .do_stitching(
                    kernel,
                    model,
                    None,
                    None,
                    crate::heal::stitch::p2::MinGapRequest::default(),
                )
                .map(|_| ())
        })
    }
    // UNPORTED(topology.brep_p3): SmBrep::OrientTrimmedSurfaces
    deferred!(
        /// `SmBrep::OrientTrimmedSurfaces`.
        fn orient(brep: BrepId, outward: bool, maybe_open: bool, unify_only: bool) -> ();,
        "topology.brep_p3",
        "SmBrep::OrientTrimmedSurfaces"
    );
    // UNPORTED(boolean.merge_p4): SmMerge::ManifoldBoolean
    deferred!(
        /// `SmMerge::ManifoldBoolean(SM_BO_UNION, ...)`. Success consumes and
        /// unregisters the source B-rep; an error leaves it owned by the caller.
        fn union(merge: SweepMergeId) -> BrepId;,
        "boolean.merge_p4",
        "SmMerge::ManifoldBoolean"
    );
}

/// Correspondence from source to copied edges, filled by the merge notifier.
pub type BTreeEdgeMap = std::collections::BTreeMap<EdgeId, EdgeId>;

pub(crate) struct SweepStitchCallback<'a> {
    pub(crate) brep: BrepId,
    pub(crate) sweep: &'a mut TopologySweep,
}

impl SweepStitchCallback<'_> {
    fn entity(&self, entity: EntityRef) -> Result<SweepEntity> {
        match entity {
            EntityRef::Vertex(vertex) => Ok(SweepEntity::Vertex(self.brep, vertex)),
            EntityRef::Edge(edge) => Ok(SweepEntity::Edge(self.brep, edge)),
            EntityRef::Face(face) => Ok(SweepEntity::Face(self.brep, face)),
            EntityRef::Region(region) => Ok(SweepEntity::Region(self.brep, region)),
            _ => Err(SolidError::invalid_input(
                "sweep::topology_sweep::stitch",
                "unsupported stitch callback entity",
            )),
        }
    }
}

impl crate::heal::stitch::p1::StitchCallback for SweepStitchCallback<'_> {
    fn select_topology_to_keep(
        &mut self,
        _kernel: &Kernel,
        _model: &Brep,
        first: EntityRef,
        second: EntityRef,
        dist_between: f64,
    ) -> Result<crate::heal::stitch::p1::KeepChoice> {
        let first = self.entity(first)?;
        let second = self.entity(second)?;
        let mut choice = crate::heal::stitch::p1::KeepChoice {
            max_distance: 0.0,
            keep_first: true,
        };
        self.sweep.select_topology_to_keep(
            Some(first),
            Some(second),
            dist_between,
            &mut choice.max_distance,
            &mut choice.keep_first,
        )?;
        Ok(choice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heal::stitch::p1::StitchCallback;
    use crate::sweep::topology_sweep::TopologySweepOptions;

    #[test]
    fn stitch_callback_preserves_brep_namespace_and_map_side_effects() {
        let brep = BrepId::from_index(7);
        let other = BrepId::from_index(2);
        let original = VertexId::from_index(3);
        let image = VertexId::from_index(4);
        let mut sweep = TopologySweep::default();
        let from = SweepEntity::Vertex(brep, original);
        let to = SweepEntity::Vertex(brep, image);
        let unrelated = SweepEntity::Vertex(other, original);
        sweep.set_same(Some(from), Some(to));
        sweep.set_same(Some(unrelated), Some(unrelated));
        let mut callback = SweepStitchCallback {
            brep,
            sweep: &mut sweep,
        };
        let result = callback
            .select_topology_to_keep(
                &Kernel::default(),
                &Brep::new(),
                EntityRef::Vertex(original),
                EntityRef::Vertex(image),
                0.375,
            )
            .unwrap();
        assert!(result.keep_first);
        assert_eq!(result.max_distance, 0.0);
        assert_eq!(sweep.get_to_same(Some(from)), Some(from));
        assert_eq!(sweep.get_from_same(Some(to)), None);
        assert_eq!(sweep.get_to_same(Some(unrelated)), Some(unrelated));
    }
    #[test]
    fn stitch_callback_preserves_owner_and_updates_sweep_maps() {
        let owner = BrepId::from_index(7);
        let kernel = Kernel::default();
        let model = Brep::new();
        for (first, second, original, copy) in [
            (
                EntityRef::Vertex(VertexId::from_index(1)),
                EntityRef::Vertex(VertexId::from_index(2)),
                SweepEntity::Vertex(owner, VertexId::from_index(1)),
                SweepEntity::Vertex(owner, VertexId::from_index(2)),
            ),
            (
                EntityRef::Edge(EdgeId::from_index(1)),
                EntityRef::Edge(EdgeId::from_index(2)),
                SweepEntity::Edge(owner, EdgeId::from_index(1)),
                SweepEntity::Edge(owner, EdgeId::from_index(2)),
            ),
        ] {
            let mut sweep = TopologySweep::new(TopologySweepOptions::default());
            sweep.set_same(Some(original), Some(copy));
            let mut callback = SweepStitchCallback {
                brep: owner,
                sweep: &mut sweep,
            };
            let choice = callback
                .select_topology_to_keep(&kernel, &model, second, first, 3.5)
                .unwrap();
            assert!(!choice.keep_first);
            assert_eq!(choice.max_distance, 0.0);
            assert_eq!(sweep.get_to_same(Some(original)), Some(original));
            assert_eq!(sweep.get_from_same(Some(original)), Some(original));
            assert_eq!(sweep.get_from_same(Some(copy)), None);
        }
    }

    #[test]
    fn stitch_callback_propagates_conflicting_sweep_originals() {
        let owner = BrepId::from_index(7);
        let first = EdgeId::from_index(1);
        let second = EdgeId::from_index(2);
        let mut sweep = TopologySweep::new(TopologySweepOptions::default());
        for edge in [first, second] {
            sweep.set_same(
                Some(SweepEntity::Edge(owner, edge)),
                Some(SweepEntity::Edge(owner, EdgeId::from_index(3))),
            );
        }
        let mut callback = SweepStitchCallback {
            brep: owner,
            sweep: &mut sweep,
        };
        assert!(callback
            .select_topology_to_keep(
                &Kernel::default(),
                &Brep::new(),
                EntityRef::Edge(first),
                EntityRef::Edge(second),
                3.5,
            )
            .is_err());
    }
}
