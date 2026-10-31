//! Defeaturing: separating a feature (a connected set of faces bounded by closed loops
//! of manifold edges) from a B-rep and filling the holes left in the base and in the
//! feature with patches (planar caps or trimmed extensions of the surrounding surfaces).
//!
//! Upstream: `source/SMLib/src/SmFeatureExecutive.cpp`,
//! `source/SMLib/inc/SmFeatureExecutive.h`; co-ported `SmTopologyTraverser::FixEdgeChainList`
//! (`SmTopologyTraverser.cpp`) and `SmEdge::GetEdgeNumberInBrep` (`SmEdge.cpp`).
//!
//! | Rust                                         | Upstream                                         |
//! |----------------------------------------------|--------------------------------------------------|
//! | [`RebuildBrepType`]                          | `SmRebuildBrepType`                              |
//! | [`FeatureLoop`]                              | `SmFeatureLoop`                                  |
//! | [`FeatureLoop::new`]                         | `SmFeatureLoop::SmFeatureLoop`                   |
//! | [`FeatureLoop::unique_patches`]              | `SmFeatureLoop::GetUniquePatches`                |
//! | [`FeatureLoop::index_in_feature`]            | `SmFeatureLoop::GetIndexInFeature`               |
//! | [`Feature`]                                  | `SmFeature`                                      |
//! | [`Feature::set_base_ext_dist`] (and the other setters) | `SmFeature::SetBaseExtDist`, `SetFeatureExtDist`, `SetBaseRBType`, `SetFeatureRBType` (both overloads each) |
//! | `collect_faces` (private)                    | `SmFeature::CollectFaces`                        |
//! | `collect_feature_loop_edges` (private)       | `SmFeature::CollectFeatureLoopEdges`             |
//! | `partition_feature` (private)                | `SmFeature::PartitionFeature`                    |
//! | `build_patches` (private)                    | `SmFeature::BuildPatches`                        |
//! | `build_patch` (private)                      | `SmFeature::BuildPatch`                          |
//! | `apply_patches` (private)                    | `SmFeature::ApplyPatches`                        |
//! | `restore_feature` (private)                  | `SmFeature::RestoreFeature`                      |
//! | `join_patches` (private)                     | `SmFeature::JoinPatches`                         |
//! | [`FeatureExecutive`]                         | `SmFeatureExecutive`                             |
//! | [`FeatureExecutive::new`]                    | `SmFeatureExecutive::SmFeatureExecutive`         |
//! | [`FeatureExecutive::add_feature_with_loops`] | `SmFeatureExecutive::AddFeature(SmFace*, SmTArray<SmFeatureLoop*>&)` |
//! | [`FeatureExecutive::add_feature_with_edges`] | `SmFeatureExecutive::AddFeature(SmFace*, SmTArray<SmEdge*>&, ..)` |
//! | [`FeatureExecutive::add_feature`]            | `SmFeatureExecutive::AddFeature(SmTArray<SmFace*>&, ..)` |
//! | `add_feature_internal` (private)             | `SmFeatureExecutive::AddFeatureInternal`         |
//! | [`FeatureExecutive::do_defeature`]           | `SmFeatureExecutive::DoDefeature`                |
//! | [`FeatureExecutive::restore_features`]       | `SmFeatureExecutive::RestoreFeatures`            |
//! | [`defeature_and_rebuild_with_edges`], [`defeature_and_rebuild`] | `SmFeatureExecutive::DefeatureAndRebuild` (both overloads) |
//! | [`FeatureExecutive::feature_breps`], [`FeatureExecutive::features`], [`FeatureExecutive::base_brep`] | `GetFeatureBreps`, `GetFeatures`, `GetBaseBrep` |
//! | [`FeatureExecutive::set_base_ext_dist`] (and the other setters) | `SmFeatureExecutive::SetBaseExtDist`, `SetFeatureExtDist`, `SetBaseRBType`, `SetFeatureRBType` |
//! | `validate_patch` (private)                   | `SmFeatureExecutive::ValidatePatch`              |
//! | `rebuild_brep` (private)                     | `SmFeatureExecutive::RebuildBrep`                |
//! | `planar_cap_rebuild` (private)               | `SmFeatureExecutive::PlanarCapRebuild`           |
//! | `ext_surface_rebuild` (private)              | `SmFeatureExecutive::ExtSurfaceRebuild`          |
//! | `trim_patches` (private)                     | `sm_TrimPatches`                                 |
//! | [`fix_edge_chain_list`]                      | `SmTopologyTraverser::FixEdgeChainList` (CO-PORTED) |
//! | [`edge_number_in_brep`]                      | `SmEdge::GetEdgeNumberInBrep` (CO-PORTED)        |
//!
//! Deviations:
//! - **Object model.** Upstream links loops, features and the executive by back
//!   pointers and owns the patch B-reps through raw pointers (a loop's base and feature
//!   patch may be the same object). Here the executive borrows the base B-rep
//!   (`m_pBaseBrep`, which upstream never deletes), owns its [`Feature`]s, each owning
//!   its [`FeatureLoop`]s and its feature B-rep, and keeps every patch B-rep in one
//!   list the loops index ([`PatchId`]); two loops sharing a patch hold the same id.
//!   [`FeatureExecutive::feature_breps`] lends the feature B-reps (upstream hands their
//!   pointers to the caller, who deletes them); [`FeatureExecutive::into_feature_breps`]
//!   gives them away.
//! - **Not ported.** `Draw` and `Dump` (display and debug output), the `SM_DEBUG_CODE`
//!   blocks, `NSidedPatchRebuild` (upstream "does not work"; its only call is commented
//!   out) and `RebuildBrepAll` (`#if 0`).
//! - **Statuses.** Upstream logs and ignores many statuses (`SE`, ignored calls): those
//!   stay ignored, through [`ignore_status`]. A status upstream returns becomes the
//!   `Err` of the Rust call; `SM_ERR_ASSERT_FAILURE` (`AERN`) is
//!   [`SolidError::Upstream`] with that code. `AddFeature` returning `NULL` is `None`.
//! - **Marks.** The executive locks and releases a mark in its constructor, as
//!   upstream's local `SmNewMarkAndLock` (so the mark is not held afterwards); the
//!   marks set on the base B-rep's objects persist in the executive's [`Marks`] store,
//!   as upstream's live on the objects for the next `AddFeature`.
//! - **Curve equality.** `*pBaseCurve == *pFeatCurve` is [`Curve::approx_eq`] (the
//!   virtual `operator==`).
//! - **`RestoreFeatures`.** Upstream removes `m_sFeatures[ii]` while iterating a copy by
//!   the same index, so after the first restored feature it drops the wrong entry and
//!   keeps a deleted feature in its list (use after free,
//!   UB-277). The port removes each
//!   restored feature itself; for at most one restored feature (the only case without
//!   undefined behaviour upstream) the two agree.
//! - **`ValidatePatch`'s intersection test.** Upstream compares
//!   `sPtClass.GetClassType()` (the static class id) with `SM_PC_FACE`, so the test never
//!   rejects a patch (WR-640); the port keeps that: it runs the
//!   face intersections (their status and edits count) and the classifications, and
//!   accepts.
//! - **Notifications.** B-rep edits run on arena-only notification records
//!   ([`crate::topology::brep::p2::ArenaNotifyState`]; a copy's records cover both
//!   B-reps), as upstream's objects notify their context. `PartitionFeature`'s and
//!   `RestoreFeature`'s `CopyFaces` run on a
//!   [`crate::topology::brep::p2::CopyNotifyState`] holding both B-reps' own attributes,
//!   so each attribute reaches the copies as its behaviour says (`SM_NO_COPY`), as
//!   upstream's live on the objects. The patch copies (`ExtSurfaceRebuild`,
//!   `ApplyPatches`) do not carry attributes yet: upstream's there depend on the merges
//!   that still differ (plan/findings/blend.feature_executive--ext-surface-patch-faces-differ.md).
//! - No recursion.

use crate::boolean::merge::p3::BooleanDriver;
use crate::curve::curve::Curve;
use crate::error::{ignore_status, EntityRef, Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::geom::context::{with_mark_lock, MarkType, ALL_MARKS};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::{self, SmallTopoType};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::topology::brep::p1 as bp1;
use crate::topology::face::p3::{FaceBreps, FaceIntersectOptions, IntersectObjects};
use crate::topology::topology_traverser::{self as traverser, Entity, Marks};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, LoopId, VertexId};

const OP: &str = "blend::feature_executive";

/// `SM_ERR_ASSERT_FAILURE`.
const SM_ERR_ASSERT_FAILURE: i32 = 1005;

/// `SM_ERR_ASSERT_FAILURE` (`AERN`).
fn assert_failure(reason: &'static str) -> SolidError {
    // (upstream logs `reason`)
    let _ = reason;
    SolidError::Upstream {
        code: SM_ERR_ASSERT_FAILURE,
        operation: OP,
        entity: None,
    }
}

fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

/// How the void a feature leaves is filled (`SmRebuildBrepType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RebuildBrepType {
    /// `SM_RB_NONE`: leave the hole (an empty patch).
    None = 0,
    /// `SM_RB_PLANAR_CAP`: a planar face on the loop edges when they are planar.
    PlanarCap = 1,
    /// `SM_RB_EXTEND_SURFACE`: G1 extensions of the surfaces along the loop edges.
    #[default]
    ExtendSurface = 2,
    /// `SM_RB_FROM_OTHER_BREP`: the extended-surface patch of the other B-rep (the
    /// feature's from the base or the base's from the feature).
    FromOtherBrep = 3,
}

/// Index of a patch B-rep in the executive's patch list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatchId(pub usize);

/// A closed loop of edges between a feature and its base (`SmFeatureLoop`).
#[derive(Debug, Clone)]
pub struct FeatureLoop {
    /// `m_sBaseEdges`: the loop's edges in the base B-rep.
    pub base_edges: Vec<EdgeId>,
    /// `m_sFeatEdges`: the matching edges in the feature B-rep.
    pub feat_edges: Vec<EdgeId>,
    /// `m_sBSourceFaces`: faces used to construct the most recent patch (upstream
    /// never fills it: `ExtSurfaceRebuild`'s output is `NotUsed`).
    pub b_source_faces: Vec<FaceId>,
    /// `m_eBaseRBType`.
    pub base_rb_type: RebuildBrepType,
    /// `m_eFeatRBType`.
    pub feat_rb_type: RebuildBrepType,
    /// `m_pBasePatch`.
    pub base_patch: Option<PatchId>,
    /// `m_pFeatPatch`.
    pub feat_patch: Option<PatchId>,
    /// `m_bUseBPatch`.
    pub use_b_patch: bool,
    /// `m_bUseFPatch`.
    pub use_f_patch: bool,
    /// `m_bTriedExtSurf`.
    pub tried_ext_surf: bool,
    /// `m_dBaseExtDist` (negative: estimated from the loop's size).
    pub base_ext_dist: f64,
    /// `m_dFeatExtDist`.
    pub feat_ext_dist: f64,
}

impl FeatureLoop {
    /// `SmFeatureLoop(rBaseEdges, eBaseRBType, eFeatRBType)` (also its private copy
    /// constructor, which copies these three only).
    #[must_use]
    pub fn new(
        base_edges: Vec<EdgeId>,
        base_rb_type: RebuildBrepType,
        feat_rb_type: RebuildBrepType,
    ) -> Self {
        Self {
            base_edges,
            feat_edges: Vec::new(),
            b_source_faces: Vec::new(),
            base_rb_type,
            feat_rb_type,
            base_patch: None,
            feat_patch: None,
            use_b_patch: true,
            use_f_patch: true,
            tried_ext_surf: false,
            base_ext_dist: -1.,
            feat_ext_dist: -1.,
        }
    }

    /// `GetUniquePatches`: the base and feature patches, once when they are the same
    /// (upstream then adds it even when it is `NULL`).
    #[must_use]
    pub fn unique_patches(&self) -> Vec<Option<PatchId>> {
        // If the same patch was used for both, return once
        if self.base_patch == self.feat_patch {
            return vec![self.base_patch];
        }
        // Else return the individual patches
        let mut out = Vec::new();
        if self.base_patch.is_some() {
            out.push(self.base_patch);
        }
        if self.feat_patch.is_some() {
            out.push(self.feat_patch);
        }
        out
    }

    /// `GetIndexInFeature`: this loop's index in `feature`, 0 when it is not there.
    #[must_use]
    pub fn index_in_feature(&self, feature: &Feature, this: usize) -> usize {
        if this < feature.loops.len() {
            this
        } else {
            0
        }
    }
}

/// A feature: connected faces of a B-rep bounded by feature loops (`SmFeature`).
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // upstream's SmFeature flags
pub struct Feature {
    /// `m_sFeatureLoops`.
    pub loops: Vec<FeatureLoop>,
    /// `m_sFaces`: the feature's faces in the base B-rep, and after partitioning the
    /// faces of the feature B-rep.
    pub faces: Vec<FaceId>,
    /// `m_pFBrep`: the feature B-rep made by partitioning.
    pub fbrep: Option<Brep>,
    /// `m_bIsPartitioned`.
    pub is_partitioned: bool,
    /// `m_bDefeatureReady`.
    pub defeature_ready: bool,
    /// `m_bBaseRebuilt`.
    pub base_rebuilt: bool,
    /// `m_bFeatRebuilt`.
    pub feat_rebuilt: bool,
    /// `m_bBaseWasApplied`.
    pub base_was_applied: bool,
    /// `m_bFeatWasApplied`.
    pub feat_was_applied: bool,
    /// `m_bCanRestore`.
    pub can_restore: bool,
    /// `m_bJoinedPatches`.
    pub joined_patches: bool,
    /// `m_bUseBPatchForF`.
    pub use_b_patch_for_f: bool,
}

impl Feature {
    /// `SetBaseExtDist(double)`: every loop's base extension distance.
    pub fn set_base_ext_dist(&mut self, ext_distance: f64) {
        for l in &mut self.loops {
            l.base_ext_dist = ext_distance;
        }
    }

    /// `SetBaseExtDist(SmTArray<double>)`: one distance per loop.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` without one distance per loop.
    pub fn set_base_ext_dists(&mut self, ext_distances: &[f64]) -> Result<()> {
        if ext_distances.len() != self.loops.len() {
            return Err(SolidError::invalid_input(
                OP,
                "Need a 1-1 correspondance for FeatureLoops and dExtDistances",
            ));
        }
        for (l, &d) in self.loops.iter_mut().zip(ext_distances) {
            l.base_ext_dist = d;
        }
        Ok(())
    }

    /// `SetFeatureExtDist(double)`: every loop's feature extension distance.
    pub fn set_feature_ext_dist(&mut self, ext_distance: f64) {
        for l in &mut self.loops {
            l.feat_ext_dist = ext_distance;
        }
    }

    /// `SetFeatureExtDist(SmTArray<double>)`: one distance per loop.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` without one distance per loop.
    pub fn set_feature_ext_dists(&mut self, ext_distances: &[f64]) -> Result<()> {
        if ext_distances.len() != self.loops.len() {
            return Err(SolidError::invalid_input(
                OP,
                "Need a 1-1 correspondance for FeatureLoops and dExtDistances",
            ));
        }
        for (l, &d) in self.loops.iter_mut().zip(ext_distances) {
            l.feat_ext_dist = d;
        }
        Ok(())
    }

    /// `SetBaseRBType(SmRebuildBrepType)`: every loop's base rebuild type.
    pub fn set_base_rb_type(&mut self, rb_type: RebuildBrepType) {
        for l in &mut self.loops {
            l.base_rb_type = rb_type;
        }
    }

    /// `SetBaseRBType(SmTArray<SmRebuildBrepType>)`: one type per loop.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` without one type per loop.
    pub fn set_base_rb_types(&mut self, rb_types: &[RebuildBrepType]) -> Result<()> {
        if rb_types.len() != self.loops.len() {
            return Err(SolidError::invalid_input(
                OP,
                "Need a 1-1 correspondance for FeatureLoops and sRBTypes",
            ));
        }
        for (l, &t) in self.loops.iter_mut().zip(rb_types) {
            l.base_rb_type = t;
        }
        Ok(())
    }

    /// `SetFeatureRBType(SmRebuildBrepType)`: every loop's feature rebuild type.
    pub fn set_feature_rb_type(&mut self, rb_type: RebuildBrepType) {
        for l in &mut self.loops {
            l.feat_rb_type = rb_type;
        }
    }

    /// `SetFeatureRBType(SmTArray<SmRebuildBrepType>)`: one type per loop.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` without one type per loop.
    pub fn set_feature_rb_types(&mut self, rb_types: &[RebuildBrepType]) -> Result<()> {
        if rb_types.len() != self.loops.len() {
            return Err(SolidError::invalid_input(
                OP,
                "Need a 1-1 correspondance for FeatureLoops and sRBTypes",
            ));
        }
        for (l, &t) in self.loops.iter_mut().zip(rb_types) {
            l.feat_rb_type = t;
        }
        Ok(())
    }
}

/// The defeaturing executive over a base B-rep (`SmFeatureExecutive`).
#[derive(Debug)]
pub struct FeatureExecutive<'a> {
    /// `m_pBaseBrep`: the original B-rep, which becomes the base.
    base: &'a mut Brep,
    /// `m_sFeatures`.
    features: Vec<Feature>,
    /// `m_eMarkType`.
    mark: MarkType,
    /// The marks upstream keeps on the base B-rep's objects.
    marks: Marks,
    /// The patch B-reps the loops index (upstream's `new SmBrep` patches).
    patches: Vec<Option<Brep>>,
}

impl<'a> FeatureExecutive<'a> {
    /// `SmFeatureExecutive(crContext, rOriginalBrep)`: takes an unlocked mark (advanced,
    /// locked and released again by the constructor's local lock).
    ///
    /// # Errors
    ///
    /// Those of the mark lock.
    pub fn new(k: &mut Kernel, original: &'a mut Brep) -> Result<Self> {
        // increment and lock an unlocked mark
        let mark = with_mark_lock(k, ALL_MARKS, |_, m| m)?;
        Ok(Self {
            base: original,
            features: Vec::new(),
            mark,
            marks: Marks::default(),
            patches: Vec::new(),
        })
    }

    /// `GetBaseBrep`.
    #[must_use]
    pub fn base_brep(&self) -> &Brep {
        self.base
    }

    /// `GetFeatures`.
    #[must_use]
    pub fn features(&self) -> &[Feature] {
        &self.features
    }

    /// `GetFeatures`, mutably (for the per-feature setters).
    pub fn features_mut(&mut self) -> &mut [Feature] {
        &mut self.features
    }

    /// `GetFeatureBreps`: each feature's B-rep (`None` before partitioning).
    #[must_use]
    pub fn feature_breps(&self) -> Vec<Option<&Brep>> {
        self.features.iter().map(|f| f.fbrep.as_ref()).collect()
    }

    /// The feature B-reps, given to the caller (upstream's caller deletes them).
    #[must_use]
    pub fn into_feature_breps(self) -> Vec<Option<Brep>> {
        self.features.into_iter().map(|f| f.fbrep).collect()
    }

    /// A patch B-rep of the executive.
    #[must_use]
    pub fn patch(&self, id: PatchId) -> Option<&Brep> {
        self.patches.get(id.0).and_then(Option::as_ref)
    }

    /// `SetBaseExtDist`: every feature's loops.
    pub fn set_base_ext_dist(&mut self, ext_distance: f64) {
        for f in &mut self.features {
            f.set_base_ext_dist(ext_distance);
        }
    }

    /// `SetFeatureExtDist`: every feature's loops.
    pub fn set_feature_ext_dist(&mut self, ext_distance: f64) {
        for f in &mut self.features {
            f.set_feature_ext_dist(ext_distance);
        }
    }

    /// `SetBaseRBType`: every feature's loops.
    pub fn set_base_rb_type(&mut self, rb_type: RebuildBrepType) {
        for f in &mut self.features {
            f.set_base_rb_type(rb_type);
        }
    }

    /// `SetFeatureRBType`: every feature's loops.
    pub fn set_feature_rb_type(&mut self, rb_type: RebuildBrepType) {
        for f in &mut self.features {
            f.set_feature_rb_type(rb_type);
        }
    }

    /// `AddFeatureInternal`: a feature of `loops` collected from `face`; `None` when the
    /// loops do not partition the B-rep.
    fn add_feature_internal(
        &mut self,
        k: &mut Kernel,
        face: FaceId,
        loops: Vec<FeatureLoop>,
    ) -> Result<Option<usize>> {
        // Create the feature and add the loops
        let mut feature = Feature {
            loops,
            ..Feature::default()
        };
        let stat = collect_faces(k, self.base, &mut self.marks, self.mark, &mut feature, face);
        match stat {
            Ok(()) => {}
            Err(e) if e.is_unported() => return Err(e),
            // SE_MSG(SM_ERR, "Feature edges do not partition the brep")
            Err(_) => return Ok(None),
        }
        self.features.push(feature);
        Ok(Some(self.features.len() - 1))
    }

    /// `AddFeature(pFace, rFLoops)`: a feature bounded by caller-made loops (copied, as
    /// upstream's copy constructor: only their edges and rebuild types); the feature's
    /// index, `None` when the loops do not partition the B-rep.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] and dangling ids.
    pub fn add_feature_with_loops(
        &mut self,
        k: &mut Kernel,
        face: FaceId,
        loops: &[FeatureLoop],
    ) -> Result<Option<usize>> {
        let copies = loops
            .iter()
            .map(|l| FeatureLoop::new(l.base_edges.clone(), l.base_rb_type, l.feat_rb_type))
            .collect();
        self.add_feature_internal(k, face, copies)
    }

    /// `AddFeature(pFace, rLoopEdges, eOptBaseRBType, eOptFeatureRBType)`: the loop
    /// edges are sorted into chains, one loop each (all of them one loop when they
    /// cannot be sorted).
    ///
    /// # Errors
    ///
    /// As [`FeatureExecutive::add_feature_with_loops`].
    pub fn add_feature_with_edges(
        &mut self,
        k: &mut Kernel,
        face: FaceId,
        loop_edges: &[EdgeId],
        base_rb_type: RebuildBrepType,
        feature_rb_type: RebuildBrepType,
    ) -> Result<Option<usize>> {
        let mut floops = Vec::new();
        // Use topology traverser to sort edges into loops
        let mut sorted = loop_edges.to_vec();
        let mut chain_start = Vec::new();
        let stat = fix_edge_chain_list(self.base, &mut sorted, &mut chain_start);
        match stat {
            Ok(()) => {
                let n_chains = chain_start.len();
                for ii in 0..n_chains {
                    let end = if ii + 1 == n_chains {
                        sorted.len()
                    } else {
                        *chain_start.get(ii + 1).ok_or_else(sm_err)?
                    };
                    let start = *chain_start.get(ii).ok_or_else(sm_err)?;
                    let edges = sorted.get(start..end).ok_or_else(sm_err)?.to_vec();
                    floops.push(FeatureLoop::new(edges, base_rb_type, feature_rb_type));
                }
            }
            Err(e) if e.is_unported() => return Err(e),
            Err(_) => {
                // Treat edge loops as a single SmFeatureLoop
                floops.push(FeatureLoop::new(
                    loop_edges.to_vec(),
                    base_rb_type,
                    feature_rb_type,
                ));
            }
        }
        // Pass the call along and return
        self.add_feature_with_loops(k, face, &floops)
    }

    /// `AddFeature(sFaces, eOptBaseRBType, eOptFeatureRBType)`: a feature of contiguous
    /// faces, bounded by their manifold edges with one face in the feature; `None` for
    /// no faces or when the boundary does not partition the B-rep.
    ///
    /// # Errors
    ///
    /// As [`FeatureExecutive::add_feature_with_loops`].
    pub fn add_feature(
        &mut self,
        k: &mut Kernel,
        faces: &[FaceId],
        base_rb_type: RebuildBrepType,
        feature_rb_type: RebuildBrepType,
    ) -> Result<Option<usize>> {
        let Some(&face) = faces.first() else {
            // SE_MSG(SM_ERR, "Adding a feature requires at least one face")
            return Ok(None);
        };
        // Get all the edges from sFaces
        let mut feature_edges: Vec<EdgeId> = Vec::new();
        for &f in faces {
            for e in face_edges(self.base, f, false)? {
                if !feature_edges.contains(&e) {
                    feature_edges.push(e);
                }
            }
        }
        // Determine which feature edges are on the boundary (will make SmFeatureLoops)
        let mut boundary = Vec::new();
        for &e in &feature_edges {
            let edge_faces = edge::faces(self.base, e, None)?;
            let common = edge_faces.iter().filter(|f| faces.contains(f)).count();
            // Each boundary edge must be manifold, with one face in and one face out
            if common == 1 && edge_faces.len() == 2 {
                boundary.push(e);
            }
        }
        // Use first face, since known to exist
        self.add_feature_with_edges(k, face, &boundary, base_rb_type, feature_rb_type)
    }

    /// `DoDefeature`: partitions each feature, builds its patches and applies them
    /// (their statuses ignored); the base B-rep is edited in place.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from a callee; dangling ids.
    pub fn do_defeature(&mut self, k: &mut Kernel) -> Result<()> {
        for i in 0..self.features.len() {
            let Some(feature) = self.features.get_mut(i) else {
                continue;
            };
            keep_unported(partition_feature(k, self.base, feature))?;
            keep_unported(build_patches(k, self.base, feature, &mut self.patches))?;
            keep_unported(apply_patches(k, self.base, feature, &mut self.patches))?;
        }
        Ok(())
    }

    /// `RestoreFeatures`: puts back the features that were partitioned but not
    /// rebuilt, and drops them (see the module docs for upstream's index slip).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from a callee.
    pub fn restore_features(&mut self, k: &mut Kernel) -> Result<()> {
        let mut ii = 0;
        while ii < self.features.len() {
            let Some(feature) = self.features.get_mut(ii) else {
                break;
            };
            match restore_feature(k, self.base, feature) {
                Ok(()) => {
                    self.features.remove(ii);
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => ii += 1,
            }
        }
        Ok(())
    }
}

/// `DefeatureAndRebuild(crContext, prOriginalBrep, rFeatureFace, rLoopEdges,
/// prFeatureBrep, eRBBaseType, eRBFeatureType)`: one feature of `face` bounded by
/// `loop_edges` is defeatured from `original`; its feature B-rep.
///
/// # Errors
///
/// `SM_ERR_ASSERT_FAILURE` when no feature is made; those of
/// [`FeatureExecutive::do_defeature`].
pub fn defeature_and_rebuild_with_edges(
    k: &mut Kernel,
    original: &mut Brep,
    face: FaceId,
    loop_edges: &[EdgeId],
    base_rb_type: RebuildBrepType,
    feature_rb_type: RebuildBrepType,
) -> Result<Option<Brep>> {
    let mut ex = FeatureExecutive::new(k, original)?;
    let made = ex.add_feature_with_edges(k, face, loop_edges, base_rb_type, feature_rb_type)?;
    if made.is_none() {
        return Err(assert_failure("Feature not built"));
    }
    ex.do_defeature(k)?;
    Ok(ex.into_feature_breps().into_iter().next().flatten())
}

/// `DefeatureAndRebuild(crContext, prOriginalBrep, rFeatureFaces, prFeatureBrep,
/// eRBBaseType, eRBFeatureType)`: one feature of the contiguous `faces` is defeatured
/// from `original`; its feature B-rep.
///
/// # Errors
///
/// As [`defeature_and_rebuild_with_edges`].
pub fn defeature_and_rebuild(
    k: &mut Kernel,
    original: &mut Brep,
    faces: &[FaceId],
    base_rb_type: RebuildBrepType,
    feature_rb_type: RebuildBrepType,
) -> Result<Option<Brep>> {
    let mut ex = FeatureExecutive::new(k, original)?;
    let made = ex.add_feature(k, faces, base_rb_type, feature_rb_type)?;
    if made.is_none() {
        return Err(assert_failure("Feature not built"));
    }
    ex.do_defeature(k)?;
    Ok(ex.into_feature_breps().into_iter().next().flatten())
}

/// A status upstream ignores, but an unported callee still surfaces.
fn keep_unported(r: Result<()>) -> Result<()> {
    match r {
        Err(e) if e.is_unported() => Err(e),
        _ => Ok(()),
    }
}

// ---- SmFeature -----------------------------------------------------------------------

/// `SmFeature::CollectFaces`: marks the loop edges, collects the faces connected to
/// `face` across unmarked edges, and checks that every loop edge has exactly two faces,
/// one of them in the feature.
fn collect_faces(
    k: &Kernel,
    base: &Brep,
    marks: &mut Marks,
    mark: MarkType,
    feature: &mut Feature,
    face: FaceId,
) -> Result<()> {
    // Mark the loop edges
    for l in &feature.loops {
        for &e in &l.base_edges {
            marks.mark(k, Entity::Edge(e), mark);
        }
    }
    // Collect all of the faces that make up the feature
    let mut faces = Vec::new();
    traverser::collect_faces(k, base, marks, Some(face), mark, &mut faces, None, None)?;
    feature.faces = faces;

    // Check that the loops provided bisect the BRep.
    for l in &feature.loops {
        for &e in &l.base_edges {
            let edge_faces = edge::faces(base, e, None)?;
            let [f0, f1] = edge_faces.as_slice() else {
                return Err(assert_failure("All loop edges must have exactly 2 faces"));
            };
            let m0 = marks.is_marked(k, Entity::Face(*f0), mark);
            let m1 = marks.is_marked(k, Entity::Face(*f1), mark);
            let is_bisection = (m0 && !m1) || (!m0 && m1);
            if !is_bisection {
                return Err(assert_failure("Loop does not bisect the brep."));
            }
        }
    }
    feature.defeature_ready = true;
    Ok(())
}

/// `SmFeature::CollectFeatureLoopEdges`: for each base loop edge, the first unused
/// feature B-rep edge whose curve equals its curve.
fn collect_feature_loop_edges(k: &Kernel, base: &Brep, feature: &mut Feature) -> Result<()> {
    let fbrep = feature.fbrep.as_ref().ok_or_else(sm_err)?;
    let mut feature_edges = bp1::edges(fbrep, None)?;
    for l in &mut feature.loops {
        for &be in &l.base_edges {
            let base_curve = base.edge(be)?.curve.as_ref();
            let mut found = None;
            for (kk, &fe) in feature_edges.iter().enumerate() {
                let feat_curve = fbrep.edge(fe)?.curve.as_ref();
                // *pBaseCurve == *pFeatCurve (upstream dereferences both)
                let (Some(bc), Some(fc)) = (base_curve, feat_curve) else {
                    return Err(SolidError::null_pointer(OP));
                };
                if bc.approx_eq(k, fc)? {
                    found = Some(kk);
                    break;
                }
            }
            if let Some(kk) = found {
                l.feat_edges.push(feature_edges.remove(kk));
            }
        }
        if l.feat_edges.len() != l.base_edges.len() {
            return Err(assert_failure("CollectFeatureLoopEdges"));
        }
    }
    Ok(())
}

/// `SmFeature::PartitionFeature`: copies the feature's faces into a new feature B-rep,
/// finds its loop edges there and removes the faces from the base.
fn partition_feature(k: &mut Kernel, base: &mut Brep, feature: &mut Feature) -> Result<()> {
    // No work, already partitioned
    if feature.is_partitioned {
        return Ok(());
    }
    if !feature.defeature_ready {
        return Err(assert_failure("Cannot partition Feature from Base"));
    }
    let fbrep = feature.fbrep.insert(new_brep(k)?);
    let save_base = base.editing_enabled();
    let save_feat = fbrep.editing_enabled();
    base.set_editing_enabled(true);
    fbrep.set_editing_enabled(true);
    let r = (|| -> Result<()> {
        // Copy faces from BaseBrep to FeatureBrep
        let fbrep = feature.fbrep.as_mut().ok_or_else(sm_err)?;
        let (_, copied) = copy_faces_carrying(k, base, &feature.faces, fbrep, true)?;
        if !copied {
            return Err(sm_err());
        }
        // Collect the loop edges on the Feature Brep
        keep_unported(collect_feature_loop_edges(k, base, feature))?;
        // Remove the faces from BaseBrep
        remove_faces(k, base, &feature.faces)?;
        Ok(())
    })();
    base.set_editing_enabled(save_base);
    if let Some(fb) = feature.fbrep.as_mut() {
        fb.set_editing_enabled(save_feat);
    }
    r?;
    // Update rFeature Faces
    let fbrep = feature.fbrep.as_ref().ok_or_else(sm_err)?;
    feature.faces = bp1::faces(fbrep, None)?;
    feature.is_partitioned = true;
    feature.can_restore = true;
    Ok(())
}

/// `SmFeature::BuildPatches`: a base patch for every loop that has none (joined when
/// some fail and there are several loops), then the feature patches: the joined base
/// patch when every loop rebuilds the feature from the base, else one per loop.
fn build_patches(
    k: &mut Kernel,
    base: &mut Brep,
    feature: &mut Feature,
    patches: &mut Vec<Option<Brep>>,
) -> Result<()> {
    feature.base_rebuilt = true;
    feature.feat_rebuilt = true;

    // Feature must have been partitioned
    if !feature.is_partitioned {
        return Err(assert_failure(
            "Trying to build patch for feature that wasn't partitioned.",
        ));
    }
    let n_loops = feature.loops.len();

    // Rebuild for the base
    for ii in 0..n_loops {
        let has_patch = feature
            .loops
            .get(ii)
            .ok_or_else(sm_err)?
            .base_patch
            .is_some();
        // Skip loops that already have patches
        let stat = if has_patch {
            Ok(())
        } else {
            build_patch(k, base, feature, patches, true, ii)
        };
        keep_unported(stat.clone())?;
        let ok = stat.is_ok();
        let l = feature.loops.get_mut(ii).ok_or_else(sm_err)?;
        if ok {
            l.use_b_patch = true;
        } else {
            feature.base_rebuilt = false;
            l.use_b_patch = false;
        }
    }

    // Try joining loop patches (fillets run through here, since 2 loops make 1 patch)
    if !feature.base_rebuilt && n_loops > 1 {
        keep_unported(join_patches(k, base, feature, patches, true))?;
    }

    // Check if we are using the base patch for the feature
    feature.use_b_patch_for_f = true;
    for l in &feature.loops {
        if !feature.use_b_patch_for_f {
            break;
        }
        if l.feat_rb_type != RebuildBrepType::FromOtherBrep {
            feature.use_b_patch_for_f = false;
        }
    }

    if feature.joined_patches && feature.use_b_patch_for_f {
        // Get all edges from feature
        let mut edges = Vec::new();
        for l in &feature.loops {
            edges.extend_from_slice(&l.feat_edges);
        }
        let first = feature.loops.first().ok_or_else(sm_err)?;
        let base_patch = first.base_patch;
        // Use merged patch if it will work for feature
        let fbrep = feature.fbrep.as_mut().ok_or_else(sm_err)?;
        let curves = edge_curves(fbrep, &edges)?;
        let pid = base_patch.ok_or_else(|| SolidError::null_pointer(OP))?;
        let mut patch = take_patch(patches, pid)?;
        let r = validate_patch(k, fbrep, &mut patch, &curves);
        put_patch(patches, pid, patch)?;
        match r {
            Ok(()) => {
                if let Some(first) = feature.loops.first_mut() {
                    first.feat_patch = first.base_patch;
                }
            }
            Err(e) if e.is_unported() => return Err(e),
            // SE_MSG(SM_ERR, "Patch does not fill Feature void manifold-ly")
            Err(_) => {}
        }
    } else {
        // Rebuild for the feature
        for ii in 0..n_loops {
            let has_patch = feature
                .loops
                .get(ii)
                .ok_or_else(sm_err)?
                .feat_patch
                .is_some();
            let stat = if has_patch {
                Ok(())
            } else {
                build_patch(k, base, feature, patches, false, ii)
            };
            keep_unported(stat.clone())?;
            let ok = stat.is_ok();
            let l = feature.loops.get_mut(ii).ok_or_else(sm_err)?;
            if ok {
                l.use_f_patch = true;
            } else {
                feature.feat_rebuilt = false;
                l.use_f_patch = false;
            }
        }
        if !feature.feat_rebuilt && n_loops > 1 {
            keep_unported(join_patches(k, base, feature, patches, false))?;
        }
    }
    Ok(())
}

/// Which of the base and feature B-reps a call works on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Base,
    Feature,
}

/// The B-rep of `side`.
fn side_brep<'b>(base: &'b Brep, feature: &'b Feature, side: Side) -> Result<&'b Brep> {
    match side {
        Side::Base => Ok(base),
        Side::Feature => feature
            .fbrep
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP)),
    }
}

/// The B-rep of `side`, mutably.
fn side_brep_mut<'b>(
    base: &'b mut Brep,
    feature: &'b mut Feature,
    side: Side,
) -> Result<&'b mut Brep> {
    match side {
        Side::Base => Ok(base),
        Side::Feature => feature
            .fbrep
            .as_mut()
            .ok_or_else(|| SolidError::null_pointer(OP)),
    }
}

/// `SmFeature::BuildPatch`: a patch for loop `li` of the base (`base_brep`) or of the
/// feature B-rep: an empty one for [`RebuildBrepType::None`], else the cached patch of
/// the source B-rep (itself, or a copy when the base patch is not reused for the
/// feature), else a new one ([`rebuild_brep`]); it is saved, then validated against the
/// target (the validation's status is returned).
fn build_patch(
    k: &mut Kernel,
    base: &mut Brep,
    feature: &mut Feature,
    patches: &mut Vec<Option<Brep>>,
    base_brep: bool,
    li: usize,
) -> Result<()> {
    let l = feature.loops.get(li).ok_or_else(sm_err)?;
    let base_edges = l.base_edges.clone();
    let feature_edges = l.feat_edges.clone();
    let base_ext_dist = l.base_ext_dist;
    let feature_ext_dist = l.feat_ext_dist;

    // set the rebuild method
    let rebuild_type = if base_brep {
        l.base_rb_type
    } else {
        l.feat_rb_type
    };

    // Confirm we have bisected the brep
    if !feature.is_partitioned {
        return Err(assert_failure(
            "Cannot rebuild the brep before partitioning",
        ));
    }

    // Get the pointer to the brep we are operating on
    let target = if base_brep { Side::Base } else { Side::Feature };
    let base_is_source = base_brep ^ (rebuild_type == RebuildBrepType::FromOtherBrep);
    let source = if base_is_source {
        Side::Base
    } else {
        Side::Feature
    };
    let ext_dist = if base_is_source {
        base_ext_dist
    } else {
        feature_ext_dist
    };
    let op_edges = if base_is_source {
        base_edges
    } else {
        feature_edges
    };

    // No work
    if rebuild_type == RebuildBrepType::None {
        // Save empty patch
        let patch = new_brep(k)?;
        let pid = add_patch(patches, patch);
        let l = feature.loops.get_mut(li).ok_or_else(sm_err)?;
        if base_brep {
            l.base_patch = Some(pid);
        } else {
            l.feat_patch = Some(pid);
        }
        return Ok(());
    }

    // Translate from the LoopEdges to the edges in the SourceBrep
    let edges = {
        let src = side_brep(base, feature, source)?;
        let mut ids = Vec::new();
        for &e in &op_edges {
            ids.push(edge_number_in_brep(src, e)?);
        }
        let all = bp1::edges(src, None)?;
        let mut edges = Vec::new();
        for i in ids {
            edges.push(*all.get(i).ok_or_else(|| SolidError::null_pointer(OP))?);
        }
        edges
    };

    // Have we already built and cached this patch?
    let l = feature.loops.get(li).ok_or_else(sm_err)?;
    let ext_rebuild = rebuild_type == RebuildBrepType::FromOtherBrep
        || rebuild_type == RebuildBrepType::ExtendSurface;
    let use_b_patch = base_is_source && l.base_patch.is_some() && ext_rebuild;
    let use_f_patch = !base_is_source && l.feat_patch.is_some() && ext_rebuild;
    let cached = if use_b_patch {
        l.base_patch
    } else {
        l.feat_patch
    };

    let pid = if (use_b_patch || use_f_patch) && feature.use_b_patch_for_f {
        // If the patch we want has already been built.
        cached.ok_or_else(sm_err)?
    } else if use_b_patch || use_f_patch {
        let to_copy = cached.ok_or_else(sm_err)?;
        let copy = copy_patch(k, patches, to_copy)?;
        add_patch(patches, copy)
    } else if ext_rebuild && l.tried_ext_surf {
        // We already tried this and it didn't work.
        // SER_MSG(SM_ERR, "Trying to rebuild with extended surfaces, known to fail. Skipping")
        return Err(sm_err());
    } else {
        // Attempt to build the patch
        if let Some(l) = feature.loops.get_mut(li) {
            l.tried_ext_surf |= ext_rebuild;
        }
        let tol = tol::brep_zone_tol_3d(
            k,
            side_brep(base, feature, target)?,
            SmallTopoType::default(),
        );
        let mut patch = new_brep(k)?;
        crate::topology::brep::p2::set_tolerance(k, &mut patch, tol, true, true)?;
        let save_patch = patch.editing_enabled();
        patch.set_editing_enabled(true);
        let src = side_brep_mut(base, feature, source)?;
        let r = rebuild_brep(k, &mut patch, &edges, rebuild_type, ext_dist, src);
        patch.set_editing_enabled(save_patch);
        // SER_MSG(.., "Could not build patch for void"): the patch is deleted
        r?;
        add_patch(patches, patch)
    };

    // Check that the patch successfully fills the void
    let stat = {
        let curves = edge_curves(side_brep(base, feature, source)?, &edges)?;
        let mut patch = take_patch(patches, pid)?;
        let tgt = side_brep_mut(base, feature, target)?;
        let r = validate_patch(k, tgt, &mut patch, &curves);
        put_patch(patches, pid, patch)?;
        r
    };
    keep_unported(stat.clone())?;

    // Save patch
    let l = feature.loops.get_mut(li).ok_or_else(sm_err)?;
    if base_brep {
        l.base_patch = Some(pid);
    } else {
        l.feat_patch = Some(pid);
    }
    stat
}

/// `SmFeature::ApplyPatches`: copies the faces of every usable patch into the base
/// (when rebuilt and not yet applied) and stitches it, then likewise for the feature
/// B-rep.
fn apply_patches(
    k: &mut Kernel,
    base: &mut Brep,
    feature: &mut Feature,
    patches: &mut [Option<Brep>],
) -> Result<()> {
    if feature.base_rebuilt && !feature.base_was_applied {
        for l in &feature.loops {
            let Some(pid) = l.base_patch else {
                continue;
            };
            if !l.use_b_patch {
                continue;
            }
            let patch = patches
                .get_mut(pid.0)
                .and_then(Option::as_mut)
                .ok_or_else(sm_err)?;
            let faces = bp1::faces(patch, None)?;
            if !faces.is_empty() {
                keep_unported(copy_faces(k, patch, &faces, base, true).map(drop))?;
            }
        }
        ignore_status(stitch_and_orient(k, base))?;
        feature.can_restore = false;
        feature.base_was_applied = true;
    }

    if feature.feat_rebuilt && !feature.feat_was_applied {
        let fbrep = feature
            .fbrep
            .as_mut()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        for l in &feature.loops {
            let Some(pid) = l.feat_patch else {
                continue;
            };
            if !l.use_f_patch {
                continue;
            }
            let patch = patches
                .get_mut(pid.0)
                .and_then(Option::as_mut)
                .ok_or_else(sm_err)?;
            let faces = bp1::faces(patch, None)?;
            if !faces.is_empty() {
                keep_unported(copy_faces(k, patch, &faces, fbrep, true).map(drop))?;
            }
        }
        ignore_status(stitch_and_orient(k, fbrep))?;
        feature.can_restore = false;
        feature.feat_was_applied = true;
    }
    Ok(())
}

/// `SmFeature::RestoreFeature`: copies the feature B-rep's faces back into the base,
/// stitches it and deletes the feature B-rep; `SM_ERR` unless restorable.
fn restore_feature(k: &mut Kernel, base: &mut Brep, feature: &mut Feature) -> Result<()> {
    if feature.can_restore {
        let mut fbrep = feature
            .fbrep
            .take()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let faces = bp1::faces(&fbrep, None)?;
        keep_unported(copy_faces_carrying(k, &mut fbrep, &faces, base, true).map(drop))?;
        ignore_status(stitch_and_orient(k, base))?;
        feature.is_partitioned = false;
        feature.defeature_ready = false;
        feature.can_restore = false;
        feature.base_was_applied = true;
        feature.feat_was_applied = true;
        return Ok(());
    }
    Err(sm_err())
}

/// `SmFeature::JoinPatches`: piecewise-merges the loops' patches (of the base or of the
/// feature) into one, trims it, and when it validates makes it the first loop's patch.
fn join_patches(
    k: &mut Kernel,
    base: &mut Brep,
    feature: &mut Feature,
    patches: &mut Vec<Option<Brep>>,
    base_loops: bool,
) -> Result<()> {
    let mut loop_edges: Vec<EdgeId> = Vec::new();
    let mut faces: Vec<FaceId> = Vec::new();
    let mut patch = new_brep(k)?;

    for l in &mut feature.loops {
        let this_patch = if base_loops {
            l.base_patch
        } else {
            l.feat_patch
        };
        if base_loops {
            l.use_b_patch = false;
        } else {
            l.use_f_patch = false;
        }
        // Skip missing patches
        let Some(tid) = this_patch else {
            continue;
        };
        let this = take_patch(patches, tid)?;
        let tol = tol::brep_zone_tol_3d(k, &this, SmallTopoType::default());

        // Merge the invalid patches together
        let (merged, this, stat) = piecewise_merge(k, patch, this, tol)?;
        patch = merged;
        put_patch(patches, tid, this)?;
        if stat.is_err() {
            continue;
        }
        // Save edges
        let edges = if base_loops {
            &l.base_edges
        } else {
            &l.feat_edges
        };
        loop_edges.extend_from_slice(edges);
        for &f in &l.b_source_faces {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }

    // Get the pointer to the brep we are operating on
    let target = if base_loops {
        Side::Base
    } else {
        Side::Feature
    };
    let curves = edge_curves(side_brep(base, feature, target)?, &loop_edges)?;
    {
        let tgt = side_brep_mut(base, feature, target)?;
        keep_unported(trim_patches(k, &mut patch, &curves, tgt, &faces))?;
    }

    // Check that the patch successfully fills the void, save if good result
    let stat = {
        let tgt = side_brep_mut(base, feature, target)?;
        validate_patch(k, tgt, &mut patch, &curves)
    };
    keep_unported(stat.clone())?;
    if stat.is_ok() {
        let pid = add_patch(patches, patch);
        let first = feature.loops.first_mut().ok_or_else(sm_err)?;
        // the replaced patch is deleted (a shared one stays with its other user here)
        if base_loops {
            first.base_patch = Some(pid);
            feature.base_rebuilt = true;
            first.use_b_patch = true;
        } else {
            first.feat_patch = Some(pid);
            feature.feat_rebuilt = true;
            first.use_f_patch = true;
        }
        feature.joined_patches = true;
    }
    stat
}

// ---- SmFeatureExecutive helpers -------------------------------------------------------

/// A loop edge's curve and interval (what `ValidatePatch` and `sm_TrimPatches` read).
type LoopCurve = (Curve, Extent1d);

fn edge_curves(b: &Brep, edges: &[EdgeId]) -> Result<Vec<LoopCurve>> {
    let mut out = Vec::new();
    for &e in edges {
        let r = b.edge(e)?;
        let c = r
            .curve
            .clone()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        out.push((c, r.interval));
    }
    Ok(out)
}

/// Whether the mid point of `e`'s curve lies on one of the loop curves
/// (`DropPoint(.., SM_SO_INTERSECT)` within `tol`).
fn on_loop_curve(k: &Kernel, b: &Brep, e: EdgeId, curves: &[LoopCurve], tol: f64) -> Result<bool> {
    let r = b.edge(e)?;
    let curve = r
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let crv_pt = curve.evaluate_point(k, r.interval.mid())?;
    for (orig, ivl) in curves {
        let drop = orig.drop_point(k, ivl, crv_pt, None, tol, None, SolverOperation::Intersect)?;
        if drop.success {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `SmFeatureExecutive::ValidatePatch`: the patch has faces, its only lamina edges lie
/// on the loop curves, and no face intersection with the target lies inside a target
/// face.
fn validate_patch(
    k: &mut Kernel,
    target: &mut Brep,
    patch: &mut Brep,
    loop_curves: &[LoopCurve],
) -> Result<()> {
    let patch_faces = bp1::faces(patch, None)?;
    let tol = tol::brep_zone_tol_3d(k, target, SmallTopoType::default());

    // Quality test 0: Existence check
    let mut good_patch = !patch_faces.is_empty();
    if !good_patch {
        return Err(assert_failure(
            "No Surfaces available to connect Loop Edges",
        ));
    }

    // Quality test 1: Check that each face is only lamina along rLoopEdges
    for &pf in &patch_faces {
        if !good_patch {
            break;
        }
        for e in face_edges(patch, pf, false)? {
            if crate::topology::edgeuse::p1::edge_is_lamina(patch, e)?
                && !on_loop_curve(k, patch, e, loop_curves, tol)?
            {
                good_patch = false;
                break;
            }
        }
    }
    if !good_patch {
        return Err(assert_failure(
            "Patch has unexpected lamina edges. Could be too small or trimmed incorrectly.",
        ));
    }

    // Get bounding boxes of Faces in sFaces
    let faces = bp1::faces(target, None)?;
    let mut boxes = Vec::new();
    for &f in &faces {
        boxes.push(face_box(k, target, f));
    }

    // Quality test 2: Check that the patch isn't intersecting pBrep internally
    for &pf in &patch_faces {
        let patch_box = face_box(k, patch, pf);
        for (jj, &f) in faces.iter().enumerate() {
            // Quick check via bounding boxes
            let face_bbox = boxes.get(jj).copied().unwrap_or_else(Extent3d::new);
            if patch_box.are_disjoint(&face_bbox, 0.0) {
                continue;
            }
            // intersect the faces (Method shouldn't force ApproxTol3d)
            let options = FaceIntersectOptions {
                want_this_classes: true,
                want_cc_curves: true,
                ..FaceIntersectOptions::default()
            };
            let approx = tol::approx_tol3d(k, tol);
            let out = crate::topology::face::p3::face_intersect_breps(
                k,
                FaceBreps::Two(patch, target),
                pf,
                f,
                approx,
                20.0,
                &options,
            )?;
            let cc_curves = out.cc_curves_3d.unwrap_or_default();
            let uv1 = out.cc_this_uv_curves.unwrap_or_default();
            let geometry = IntersectObjects {
                b: patch,
                other: target,
                side: false,
                curves_3d: &cc_curves,
                uv1: &uv1,
                uv2: &[],
            };
            // Check the midpoint of each curve classification
            for cc in out.this_classes.unwrap_or_default() {
                let curve = cc_curve(&geometry, &cc)?;
                let param = curve.natural_interval().mid();
                // FindPointClass (status ignored)
                match cc.find_point_class(k, &geometry, param, tol) {
                    Err(e) if e.is_unported() => return Err(e),
                    _ => {}
                }
                // If the curve is internal to the face, then the new face is a bad result:
                // AERN_MSG(sPtClass.GetClassType() != SM_PC_FACE, ..). GetClassType() is the
                // static SM_COMMON class id (SmPointClassification_TYPE), not the point
                // class, so the test never fails upstream (WR record in the module docs).
            }
        }
    }
    Ok(())
}

/// `SmFeatureExecutive::RebuildBrep`: dispatches on the rebuild type
/// ([`RebuildBrepType::FromOtherBrep`] rebuilds by extended surfaces).
fn rebuild_brep(
    k: &mut Kernel,
    patch: &mut Brep,
    loop_edges: &[EdgeId],
    rebuild_type: RebuildBrepType,
    ext_dist: f64,
    source: &mut Brep,
) -> Result<()> {
    // If rebuilding from surface, use the InnerLoop method if possible
    let rebuild_type = if rebuild_type == RebuildBrepType::FromOtherBrep {
        RebuildBrepType::ExtendSurface
    } else {
        rebuild_type
    };
    match rebuild_type {
        RebuildBrepType::PlanarCap => {
            let curves = edge_curves(source, loop_edges)?;
            planar_cap_rebuild(k, patch, &curves)
        }
        RebuildBrepType::ExtendSurface => {
            ext_surface_rebuild(k, patch, source, loop_edges, ext_dist)
        }
        RebuildBrepType::None | RebuildBrepType::FromOtherBrep => Ok(()),
    }
}

/// `SmFeatureExecutive::PlanarCapRebuild`: a planar face for each chain of the loop
/// curves.
fn planar_cap_rebuild(k: &mut Kernel, patch: &mut Brep, loop_curves: &[LoopCurve]) -> Result<()> {
    let tol = patch.tolerance();
    crate::topology::brep::p2::set_tolerance(k, patch, tol, true, true)?;

    // Get copy of curves for new planar face from loop edges
    let curves: Vec<Option<Curve>> = loop_curves.iter().map(|(c, _)| Some(c.clone())).collect();
    let chained = crate::curve::curve::p4::fix_chain_curves_list(k, curves, tol, false)?;
    let n_chains = chained.chain_starts.len();
    for ii in 0..n_chains {
        let start = *chained.chain_starts.get(ii).ok_or_else(sm_err)?;
        let end = if ii + 1 < n_chains {
            *chained.chain_starts.get(ii + 1).ok_or_else(sm_err)?
        } else {
            chained.curves.len()
        };
        // do { .. } while (sIndexStart < sIndexEnd): at least one curve
        let end = end.max(start + 1);
        let lp = chained.curves.get(start..end).ok_or_else(sm_err)?;
        // Create planar face
        planar_face(k, patch, lp, tol).map_err(|e| {
            if e.is_unported() {
                e
            } else {
                // SER_MSG(.., "Couldn't make a planar cap. Loop edges non-planar?")
                let code = match e {
                    SolidError::Upstream { code, .. } => code,
                    _ => crate::error::SM_ERR,
                };
                SolidError::Upstream {
                    code,
                    operation: OP,
                    entity: None,
                }
            }
        })?;
    }
    Ok(())
}

/// `SmFeatureExecutive::ExtSurfaceRebuild`: for each source face along the loop edges
/// a patch face: the face itself with its loop-edge inner loop as outer loop when the
/// loop edges are not on its outer loop, else its surface extended by `ext_dist` (or
/// twice the loop's box size) less the face; the faces are merged into `patch`,
/// trimmed ([`trim_patches`]), stitched and shrunk.
fn ext_surface_rebuild(
    k: &mut Kernel,
    patch: &mut Brep,
    source: &mut Brep,
    loop_edges: &[EdgeId],
    ext_dist: f64,
) -> Result<()> {
    let tol = source.tolerance();
    let mut faces: Vec<FaceId> = Vec::new();
    let mut test_merge_at_end = false;
    let mut temp_patch = new_brep(k)?;

    // Get all faces that use rLoopEdges
    for &e in loop_edges {
        for f in edge::faces(source, e, None)? {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }

    // Set the distance to extend surfaces
    let ext = if ext_dist < 0.0 {
        // Estimate void size to set surface extensions.
        let mut void_box = Extent3d::new();
        for &e in loop_edges {
            let this_box = edge_box(k, source, e);
            void_box = this_box.union(&void_box);
        }
        2. * void_box.max_length()
    } else {
        ext_dist
    };

    for &face in &faces {
        // make a brep for the new face
        let mut face_brep = new_brep(k)?;
        crate::topology::brep::p2::set_tolerance(k, &mut face_brep, tol, true, true)?;
        face_brep.set_editing_enabled(true);

        // Check if this is part of inner loop
        let outer_edges = face_edges(source, face, true)?;
        let common: Vec<EdgeId> = loop_edges
            .iter()
            .copied()
            .filter(|e| outer_edges.contains(e))
            .collect();

        if common.is_empty() {
            // Get an edge on this face that is part of crLoopEdges
            let f_edges = face_edges(source, face, false)?;
            let common: Vec<EdgeId> = loop_edges
                .iter()
                .copied()
                .filter(|e| f_edges.contains(e))
                .collect();
            let pe = *common.first().ok_or_else(|| SolidError::null_pointer(OP))?;

            // Get the loops of the face to copy
            let loops = crate::topology::face::p2::loops(source, face, false, None)?;

            // For the face to copy, remove all loops except the inner loop that shares an
            // edge w/ crLoopEdges. Make that one loop the outer loop by flipping it.
            let mut this_loop: Option<LoopId> = None;
            for &lp in &loops {
                let l_edges = crate::topology::loopuse::loop_edges(source, lp)?;
                if l_edges.contains(&Some(pe)) {
                    crate::topology::loopuse::flip_loop_orientation(source, lp)?;
                    this_loop = Some(lp);
                } else {
                    crate::topology::face::p1::remove_loop(source, face, lp)?;
                }
            }

            // Place a copy of this face into pFaceBrep
            keep_unported(copy_faces(k, source, &[face], &mut face_brep, true).map(drop))?;

            // Put the face back together for subsequent testing
            let this_loop = this_loop.ok_or_else(|| SolidError::null_pointer(OP))?;
            crate::topology::loopuse::flip_loop_orientation(source, this_loop)?;
            crate::topology::face::p1::remove_loop(source, face, this_loop)?;
            for &lp in &loops {
                crate::topology::face::p1::insert_loop(source, face, lp)?;
            }
        } else {
            // Extend the surface (status ignored; upstream dereferences the NULL result)
            let surface = source
                .face(face)?
                .surface
                .clone()
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let new_surf = surface
                .create_extended_surface(k, ext, ContinuityType::G1)
                .map_err(|e| {
                    if e.is_unported() {
                        e
                    } else {
                        SolidError::null_pointer(OP)
                    }
                })?;

            // Make a face from pSurface
            keep_unported(
                crate::blend::fillet_executive::api::create_face_from_surface(
                    k,
                    &mut face_brep,
                    &new_surf,
                )
                .map(drop),
            )?;

            // Make the outer loop of pFace into an inner loop of pNewFace
            let mut remove_brep = new_brep(k)?;
            keep_unported(copy_faces(k, source, &[face], &mut remove_brep, true).map(drop))?;
            face_brep = manifold_xor(k, face_brep, remove_brep)?;
        }

        // Copy faces from pFaceBrep to pTempPatch
        let these = bp1::faces(&face_brep, None)?;
        keep_unported(copy_faces(k, &mut face_brep, &these, &mut temp_patch, true).map(drop))?;
        test_merge_at_end = true;
    }

    if test_merge_at_end {
        let taken = std::mem::replace(patch, Brep::new());
        let (merged, _temp, stat) = piecewise_merge(k, taken, temp_patch, tol)?;
        *patch = merged;
        keep_unported(stat)?;
    }

    // Delete the faces we don't want in the patch
    let curves = edge_curves(source, loop_edges)?;
    keep_unported(trim_patches(k, patch, &curves, source, &faces))?;

    // Stitch and orient locals
    ignore_status(stitch_and_orient(k, patch))?;

    // Extended surfaces are likely much larger than the faces, so fix this.
    ignore_status(crate::topology::brep::p4::shrink_geometry(k, patch))?;
    Ok(())
}

/// `sm_TrimPatches`: removes the patch faces that use no loop curve and those whose
/// interior point lies in one of `faces` of `source` (or that contain one of theirs).
fn trim_patches(
    k: &mut Kernel,
    patch: &mut Brep,
    loop_curves: &[LoopCurve],
    source: &mut Brep,
    faces: &[FaceId],
) -> Result<()> {
    let tol = tol::brep_zone_tol_3d(k, patch, SmallTopoType::default());
    let patch_faces = bp1::faces(patch, None)?;
    let mut to_delete = Vec::new();
    let save = patch.editing_enabled();
    patch.set_editing_enabled(true);
    let r = (|| -> Result<()> {
        for &pf in &patch_faces {
            let mut delete_face = true;
            // Delete faces unless they share an edge with rLoopEdges
            for e in face_edges(patch, pf, false)? {
                if !delete_face {
                    break;
                }
                if on_loop_curve(k, patch, e, loop_curves, tol)? {
                    delete_face = false;
                }
            }
            if delete_face {
                to_delete.push(pf);
                continue;
            }

            // Get one point in this patch face
            let (puv, ppt) = match crate::topology::face::p2::points_in_face(k, patch, pf, 1) {
                Ok(p) => {
                    if let (Some(&uv), Some(&pt)) = (p.uv.first(), p.pv.first()) {
                        (uv, pt)
                    } else {
                        to_delete.push(pf);
                        continue;
                    }
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => {
                    to_delete.push(pf);
                    continue;
                }
            };

            // Compare with every face that should be done already
            for &this_face in faces {
                // Classify point from patch against face in pBrep
                if point_in_face(k, source, this_face, ppt, tol, puv)? {
                    delete_face = true;
                    break;
                }
                // Get a point in the face in pBrep
                let (tuv, tpt) =
                    match crate::topology::face::p2::points_in_face(k, source, this_face, 1) {
                        Ok(p) => match (p.uv.first(), p.pv.first()) {
                            (Some(&uv), Some(&pt)) => (uv, pt),
                            _ => continue,
                        },
                        Err(e) if e.is_unported() => return Err(e),
                        Err(_) => continue,
                    };
                if point_in_face(k, patch, pf, tpt, tol, tuv)? {
                    delete_face = true;
                    break;
                }
            }
            if delete_face {
                to_delete.push(pf);
            }
        }
        // Remove the faces
        ignore_status(remove_faces(k, patch, &to_delete))?;
        Ok(())
    })();
    patch.set_editing_enabled(save);
    r
}

/// `Point3DClassify(pt, tol, FALSE, pc, &uv)` gives `SM_PC_FACE` (its status ignored).
fn point_in_face(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    pt: Point3,
    tol: f64,
    uv: crate::geom::vector2d::Point2,
) -> Result<bool> {
    let mut pc = PointClassification::new(tol);
    let opts = crate::query::face::p1::Point3dClassifyOptions {
        guess_uv: Some(uv),
        ..Default::default()
    };
    ignore_status(crate::query::face::p1::point_3d_classify(
        k, b, f, pt, tol, false, &mut pc, opts,
    ))?;
    Ok(pc.point_class() == PointClassType::Face)
}

// ---- co-ported helpers -----------------------------------------------------------------

/// `SmTopologyTraverser::FixEdgeChainList` (CO-PORTED): orders `edges` into chains of
/// edges sharing vertices; `chain_start` gets the index of each chain's first edge.
///
/// # Errors
///
/// `SM_ERR` when edges of the list cross at a vertex (a figure 8); dangling ids.
pub fn fix_edge_chain_list(
    b: &Brep,
    edges: &mut Vec<EdgeId>,
    chain_start: &mut Vec<usize>,
) -> Result<()> {
    // No work
    if edges.is_empty() {
        chain_start.clear();
        return Ok(());
    }
    // SER_MSG(SM_ERR, "Edge chains cannot intersect (e.g., no figure-8 allowed)")
    let figure8 = sm_err;
    let common = |unused: &[EdgeId], local: &[EdgeId]| -> Vec<EdgeId> {
        local
            .iter()
            .copied()
            .filter(|e| unused.contains(e))
            .collect()
    };

    let mut chain_start_count = 0usize;
    let mut unused = edges.clone();
    let mut e = *unused.first().ok_or_else(sm_err)?; // Arbitrary starting edge
    let (start_vertex, mut v) = edge_vertices(b, e)?;

    // Reset input
    edges.clear();
    chain_start.clear();
    chain_start.push(0);

    // Check the StartVertex for too many chain Edges
    let local = vertex::edges(b, start_vertex, None)?;
    if common(&unused, &local).len() > 2 {
        return Err(figure8());
    }

    // Work through edges until none are left
    while !unused.is_empty() {
        chain_start_count += 1;

        // Get edges connected to this vertex
        let mut local = vertex::edges(b, v, None)?;
        // FindElement leaves the index 0 when the edge is not there
        let id = local.iter().position(|&x| x == e).unwrap_or(0);
        if id < local.len() {
            local.remove(id);
        }

        // Find Edges to be chained that connect to pVertex
        let connected = common(&unused, &local);
        if connected.len() > 1 {
            return Err(figure8());
        }

        // Remove Edge from list sUnusedEdges
        let id = unused.iter().position(|&x| x == e).unwrap_or(0);
        if id < unused.len() {
            unused.remove(id);
        }

        // Add to output
        edges.push(e);

        // Update edge and vertex for the next iteration
        if let [next] = connected.as_slice() {
            e = *next;
            v = edge::other_vertex(b, e, Some(v))?.ok_or_else(|| SolidError::null_pointer(OP))?;
            continue;
        }

        // Start a new chain if needed
        if let Some(&first) = unused.first() {
            e = first;
            let (sv, nv) = edge_vertices(b, e)?;
            v = nv;
            chain_start.push(chain_start_count);

            // Check the StartVertex for too many chain Edges
            let local = vertex::edges(b, sv, None)?;
            if common(&unused, &local).len() > 2 {
                return Err(figure8());
            }
        }
    }
    Ok(())
}

/// `SmEdge::GetEdgeNumberInBrep` (CO-PORTED): the index of `e` in its B-rep's
/// `GetEdges`, 0 when it is not there.
///
/// # Errors
///
/// Those of the edge list.
pub fn edge_number_in_brep(b: &Brep, e: EdgeId) -> Result<usize> {
    Ok(bp1::edges(b, None)?
        .iter()
        .position(|&x| x == e)
        .unwrap_or(0))
}

/// `SmEdge::GetVertices(pV1, pV2)`.
fn edge_vertices(b: &Brep, e: EdgeId) -> Result<(VertexId, VertexId)> {
    match edge::start_end_vertices(b, e)? {
        (Some(a), Some(z)) => Ok((a, z)),
        _ => Err(SolidError::null_pointer(OP)),
    }
}

// ---- callee wrappers ---------------------------------------------------------------------

/// `new (crContext) SmBrep()`.
fn new_brep(k: &mut Kernel) -> Result<Brep> {
    crate::topology::brep::p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT)
}

fn add_patch(patches: &mut Vec<Option<Brep>>, patch: Brep) -> PatchId {
    patches.push(Some(patch));
    PatchId(patches.len() - 1)
}

fn take_patch(patches: &mut [Option<Brep>], id: PatchId) -> Result<Brep> {
    patches
        .get_mut(id.0)
        .and_then(Option::take)
        .ok_or_else(|| SolidError::null_pointer(OP))
}

fn put_patch(patches: &mut [Option<Brep>], id: PatchId, patch: Brep) -> Result<()> {
    let slot = patches.get_mut(id.0).ok_or_else(sm_err)?;
    *slot = Some(patch);
    Ok(())
}

/// `new SmBrep(*pPatchToCopy)`: the copy constructor.
fn copy_patch(k: &mut Kernel, patches: &mut [Option<Brep>], id: PatchId) -> Result<Brep> {
    let src = take_patch(patches, id)?;
    // SmBrep(crBrepToCopy, NULL, NULL, bCopyAttributes = TRUE, bAddAnalytics = TRUE)
    let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&src);
    let mut env = state.env(None);
    let r = crate::topology::brep::p3::copy_brep(
        k,
        &mut env,
        &src,
        &crate::topology::loopuse::notify_ref,
        None,
        None,
        true,
        true,
    );
    put_patch(patches, id, src)?;
    r
}

/// `SmFace::GetEdges(rEdges, NULL, bTgtLoopsOnly)`.
fn face_edges(b: &Brep, f: FaceId, outer_only: bool) -> Result<Vec<EdgeId>> {
    crate::topology::face::p4::edges(b, f, None, outer_only, None)
}

/// `SmFace::CalculateBoundingBox` (status ignored: the initialised box).
fn face_box(k: &Kernel, b: &Brep, f: FaceId) -> Extent3d {
    crate::query::face::p1::calculate_bounding_box(k, b, f, false)
        .unwrap_or_else(|_| Extent3d::new())
}

/// `SmEdge::CalculateBoundingBox(&box)` (status ignored: the initialised box).
fn edge_box(k: &Kernel, b: &Brep, e: EdgeId) -> Extent3d {
    let req = crate::nurbs::b_spline_curve::BoundingBoxRequest {
        normal: true,
        ..Default::default()
    };
    crate::query::edge::calculate_bounding_box(k, b, e, req, false)
        .ok()
        .and_then(|bb| bb.normal)
        .unwrap_or_default()
}

/// The curve a face-intersection classification classifies (`GetCurve`).
fn cc_curve<'c>(
    geometry: &'c IntersectObjects<'_>,
    cc: &crate::intersect::curve_class::p1::CurveClassification<
        crate::topology::face::p3::IntersectObject,
    >,
) -> Result<&'c Curve> {
    use crate::intersect::point_class::ClassGeometry;
    let handle = cc.curve().ok_or_else(|| SolidError::null_pointer(OP))?;
    geometry
        .curve(handle)
        .ok_or_else(|| SolidError::null_pointer(OP))
}

/// `SmBrep::CopyFaces(faces, dest, .., bUseMerge)`: the new faces and whether every
/// face was copied.
fn copy_faces(
    k: &mut Kernel,
    src: &mut Brep,
    faces: &[FaceId],
    dest: &mut Brep,
    use_merge: bool,
) -> Result<(Vec<FaceId>, bool)> {
    // (arena-only notification records for both B-reps' objects, as `copy_component`)
    let mut state = crate::topology::brep::p2::ArenaNotifyState::new(dest);
    state.seed(src);
    let mut env = state.env(None);
    let face_ref = |f: FaceId| crate::topology::loopuse::notify_ref(EntityRef::Face(f));
    crate::topology::brep::p7::copy_faces(k, src, faces, dest, use_merge, &face_ref, &mut env)
}

/// `CopyFaces` from one B-rep into another with both B-reps' own attributes
/// ([`crate::topology::brep::p2::CopyNotifyState`]): each attribute goes to the new
/// objects as its behaviour says (`SM_NO_COPY`), as upstream's live on the objects.
fn copy_faces_carrying(
    k: &mut Kernel,
    src: &mut Brep,
    faces: &[FaceId],
    dest: &mut Brep,
    use_merge: bool,
) -> Result<(Vec<FaceId>, bool)> {
    use crate::topology::brep::p2::{copy_source_ref, CopyNotifyState};
    // (arena-only notification records for both B-reps' objects, holding both B-reps'
    // attributes: SM_NO_COPY gives each new object its source's, as upstream)
    let mut state = CopyNotifyState::with_attributes_of(src, dest)?;
    let face_ref = |f: FaceId| crate::topology::loopuse::notify_ref(EntityRef::Face(f));
    let r = crate::topology::brep::p7::copy_faces_from(
        k,
        src,
        faces,
        dest,
        use_merge,
        &copy_source_ref,
        &face_ref,
        &mut state.env(None),
    );
    let restored = state.restore_attributes(src, dest);
    let r = r?;
    restored?;
    Ok(r)
}

/// `SmBrep::RemoveFaces`.
fn remove_faces(k: &mut Kernel, b: &mut Brep, faces: &[FaceId]) -> Result<()> {
    let editing = b.editing_enabled();
    crate::topology::brep::p5::remove_faces(k, b, editing, faces)
}

/// `SmBrep::StitchAndOrient()` (defaults).
fn stitch_and_orient(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    crate::topology::brep::p5::stitch_and_orient(
        k,
        b,
        &crate::topology::brep::p5::StitchAndOrientOptions::default(),
    )
}

/// `CreatePlanarFaceWith3DCurves(NULL, curves, tol, pNewFace)`.
fn planar_face(k: &mut Kernel, b: &mut Brep, curves: &[Curve], tol: f64) -> Result<()> {
    let mut state = crate::topology::brep::p2::ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |f: FaceId| crate::topology::loopuse::notify_ref(EntityRef::Face(f));
    let mut fenv = crate::topology::brep::p8::FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    crate::topology::brep::p8::create_planar_face_with_3d_curves(k, b, None, curves, tol, &mut fenv)
        .map(drop)
}

/// `SmMerge(ctx, a, b, tol).PiecewiseMerge(NULL, FALSE, TRUE, ..)`: `a` with `b`'s
/// faces merged in, `b` (imprinted, not deleted) and the merge's status.
fn piecewise_merge(k: &mut Kernel, a: Brep, b: Brep, tol: f64) -> Result<(Brep, Brep, Result<()>)> {
    use crate::primitive::primitive_creation::model::PrimitiveModel;
    use crate::sweep::topology_sweep::access::SweepAccess;
    let mut model = PrimitiveModel::new(k, a);
    let other_id = model.add_brep(b)?;
    let merge = model.new_merge(
        PrimitiveModel::MAIN,
        PrimitiveModel::MAIN,
        other_id,
        tol,
        0.0,
    )?;
    let merged = model.piecewise_merge(merge, false, true);
    model.delete_merge(merge);
    let stat = match merged {
        Ok(id) if id == PrimitiveModel::MAIN => Ok(()),
        Ok(_) => Err(sm_err()),
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => Err(e),
    };
    let mut other = model.take_brep(other_id)?;
    let mut result = model.into_brep()?;
    result.refresh_cached_topology();
    other.refresh_cached_topology();
    Ok((result, other, stat))
}

/// `SmMerge(ctx, a, b).ManifoldBoolean(SM_BO_EXCLUSIVE_OR, a)` (status ignored): `a`
/// after the boolean.
fn manifold_xor(k: &mut Kernel, a: Brep, b: Brep) -> Result<Brep> {
    let mut driver = crate::boolean::merge::p3::DependencyMerge::carrying_attributes();
    let mut primary = Some(a);
    let mut other = Some(b);
    driver.construct(k, &mut primary, &mut other, 0.0, 0.0, None)?;
    let mut manifold = true;
    let mut lamina_edges = 0;
    let r = driver.manifold_boolean(
        k,
        &mut primary,
        &mut other,
        &mut manifold,
        &mut lamina_edges,
        BooleanOperation::ExclusiveOr,
        None,
    );
    let result = match r {
        Ok(Some(b)) => b,
        Ok(None) => primary.take().ok_or_else(|| SolidError::null_pointer(OP))?,
        Err(e) if e.is_unported() => return Err(e),
        Err(_) => primary.take().ok_or_else(|| SolidError::null_pointer(OP))?,
    };
    let mut result = result;
    result.refresh_cached_topology();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::{create_box, create_cylinder};

    fn shares_vertex(b: &Brep, a: EdgeId, z: EdgeId) -> bool {
        let (a0, a1) = edge_vertices(b, a).unwrap();
        let (z0, z1) = edge_vertices(b, z).unwrap();
        a0 == z0 || a0 == z1 || a1 == z0 || a1 == z1
    }

    #[test]
    fn fix_edge_chain_list_orders_a_shuffled_face_loop() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0).unwrap();
        let face = *bp1::faces(&b, None).unwrap().first().unwrap();
        let mut edges = face_edges(&b, face, false).unwrap();
        assert_eq!(edges.len(), 4);
        edges.swap(1, 2);
        let mut starts = Vec::new();
        fix_edge_chain_list(&b, &mut edges, &mut starts).unwrap();
        assert_eq!(edges.len(), 4);
        assert_eq!(starts, vec![0]);
        for w in edges.windows(2) {
            assert!(shares_vertex(&b, w[0], w[1]));
        }
    }

    /// A closed edge (one vertex at both ends) is a chain of its own; `GetVertices` must
    /// give both end vertices, not the unique list (which made one chain of every edge).
    #[test]
    fn fix_edge_chain_list_closed_edges_are_separate_chains() {
        let mut k = Kernel::default();
        let b = create_cylinder(&mut k, Point3::ZERO, 1.0, 2.0).unwrap();
        let closed: Vec<EdgeId> = bp1::edges(&b, None)
            .unwrap()
            .into_iter()
            .filter(|&e| {
                let (s, t) = edge_vertices(&b, e).unwrap();
                s == t
            })
            .collect();
        assert_eq!(closed.len(), 2);
        let mut edges = closed.clone();
        let mut starts = Vec::new();
        fix_edge_chain_list(&b, &mut edges, &mut starts).unwrap();
        assert_eq!(edges, closed);
        assert_eq!(starts, vec![0, 1]);
    }

    #[test]
    fn fix_edge_chain_list_empty() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let mut edges = Vec::new();
        let mut starts = vec![3];
        fix_edge_chain_list(&b, &mut edges, &mut starts).unwrap();
        assert!(starts.is_empty());
    }

    #[test]
    fn edge_number_in_brep_is_the_get_edges_index() {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        for (i, e) in bp1::edges(&b, None).unwrap().into_iter().enumerate() {
            assert_eq!(edge_number_in_brep(&b, e).unwrap(), i);
        }
    }

    /// `PartitionFeature`'s `CopyFaces` notifies `SM_NO_COPY` on each copied face and edge
    /// (`SmMerge::MakeFaceuse`), so copy-behaviour attributes reach the feature B-rep and
    /// temporary ones do not; the base keeps its other objects' attributes.
    #[test]
    fn defeature_carries_attributes_as_their_behaviours_say() {
        use crate::geom::attribute::{Attribute, Behavior};
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0).unwrap();
        let faces = bp1::faces(&b, None).unwrap();
        let f0 = faces[0];
        let other = faces[1];
        let e0 = face_edges(&b, f0, false).unwrap()[0];
        b.add_attribute(EntityRef::Face(f0), Attribute::new(20001, Behavior::Copy))
            .unwrap();
        b.add_attribute(EntityRef::Edge(e0), Attribute::new(20002, Behavior::Copy))
            .unwrap();
        b.add_attribute(
            EntityRef::Face(other),
            Attribute::new(20003, Behavior::Copy),
        )
        .unwrap();
        b.add_attribute(EntityRef::Face(f0), Attribute::new(20004, Behavior::Temp))
            .unwrap();
        let fbrep = {
            let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
            ex.add_feature(
                &mut k,
                &[f0],
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::FromOtherBrep,
            )
            .unwrap()
            .unwrap();
            ex.do_defeature(&mut k).unwrap();
            ex.into_feature_breps()
                .into_iter()
                .next()
                .flatten()
                .unwrap()
        };
        let has = |br: &Brep, id: u64, face: bool| {
            if face {
                bp1::faces(br, None)
                    .unwrap()
                    .into_iter()
                    .any(|f| br.find_attribute(EntityRef::Face(f), id).is_some())
            } else {
                bp1::edges(br, None)
                    .unwrap()
                    .into_iter()
                    .any(|e| br.find_attribute(EntityRef::Edge(e), id).is_some())
            }
        };
        assert!(
            has(&fbrep, 20001, true),
            "copy face attribute on the feature"
        );
        assert!(
            has(&fbrep, 20002, false),
            "copy edge attribute on the feature"
        );
        assert!(!has(&fbrep, 20004, true), "temporary attribute not copied");
        assert!(!has(&fbrep, 20003, true), "other faces stay in the base");
        assert!(b.find_attribute(EntityRef::Face(other), 20003).is_some());
    }

    #[test]
    fn add_feature_without_faces_makes_none() {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        let made = ex
            .add_feature(
                &mut k,
                &[],
                RebuildBrepType::ExtendSurface,
                RebuildBrepType::FromOtherBrep,
            )
            .unwrap();
        assert!(made.is_none());
        assert!(ex.features().is_empty());
    }
}
