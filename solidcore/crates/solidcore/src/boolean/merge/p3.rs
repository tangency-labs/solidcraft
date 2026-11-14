//! Piecewise merges, list booleans and postfix CSG evaluation.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/inc/SmMerge.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`piecewise_merge`] | `SmMerge::PiecewiseMerge` |
//! | [`merge_breps`] | `SmMerge::merge_breps` |
//! | [`boolean_lists`] | `SmMerge::BooleanLists` |
//! | [`boolean_lists_with_notifications`] | `SmMerge::BooleanLists` with caller notification state |
//! | [`boolean_trees`] | `SmMerge::BooleanTrees` |
//! | [`non_manifold_boolean`] | `SmMerge::NonManifoldBoolean` |
//!
//! Deviations: nullable owned arena B-reps replace pointers; `BooleanDriver` and
//! `PiecewiseAccess` borrow the dependency owners' engine/storage rather than
//! copying their implementations. Tree inputs retain null slots on failure;
//! invalid indices and null operands dereferenced by a binary operation return
//! `InvalidInput` instead of upstream
//! undefined behavior, and an `InvalidInput` from `ManifoldBoolean` (undefined
//! behaviour the port detects) is returned as is rather than as
//! `SM_ERR`. List inputs are copied once, as upstream requires. Debug
//! drawing and the compile-time false IO experiment are omitted. No recursion;
//! connected-component extraction is bounded by the initial face count, with an
//! error if removal fails to make progress.
//! `PiecewiseMerge` ends on an error of its first `IntersectInsertRelate` (SolidCraft
//! fix, tangency-labs/smlib af8db37, UB-242), where upstream ignores the status and
//! goes on with half-imprinted B-reps.
//! `BooleanLists` copies its surface operands with the virtual `Copy` (the derived surface,
//! its natural domain and its kind; the function's documented intent), and its planar
//! outputs too. Upstream's base copy constructor slices every derived surface to a
//! geometry-less `SmSurface` (WR-196), so there a surface list always fails with `SM_ERR`
//! and the planar 2D path is never taken.
//! Component-copy notifications use caller-owned records/attributes through
//! `boolean_lists_with_notifications`; the arena-only entry passes an arena-only
//! notification store (default topology records, no attributes), because the arena
//! carries no attribute records, so upstream's attribute propagation has nothing to act
//! on.
//! Skip relations reuse `geom::tol::Relation` and its documented sorted-handle
//! query order, rather than upstream's address-dependent hash order.
//! `DependencyMerge` runs `ManifoldBoolean` through the `boolean.merge_p4` port
//! on the engine members `SmMerge::SmMerge` (`boolean.merge_p2`) builds
//! ([`MergeEngine::from_merge`]); called before `construct` it refuses rather than
//! default the engine's state.

use super::p1::{MergeOptions, MergeState};
use super::p2::MergeOperand;
use super::p4::{BooleanKernels, KernelManifoldIntersector, ManifoldState};
use crate::boolean::topology_intersector::p1::TopologyIntersector;
use crate::error::{ignored, EntityRef, Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::geom::attribute::AttributeStore;
use crate::geom::context::{with_mark_lock, MarkLock, ALL_MARKS};
use crate::geom::tol::Relation;
use crate::kernel::Kernel;
use crate::surface::surface::p2::PLANE_TYPE;
use crate::surface::surface::Surface;
use crate::topology::brep::{p1 as bp, p7 as bp7};
use crate::topology::proto_topology::p1::CopyBrepMap;
use crate::topology::topology_traverser::{collect_faces, Marks};
use crate::topology::{Brep, FaceId};

const OP: &str = "boolean::merge::p3";

/// Calls owned by the constructor and manifold-boolean units. Implementations
/// retain their canonical merge engine between `construct` and `manifold_boolean`.
/// `Operand` permits cross-arena handles as well as owned B-reps.
pub trait BooleanDriver {
    /// A non-null owned B-rep or a non-null handle in the driver's storage.
    type Operand;
    /// Whether two operands refer to the same B-rep. Owned-value drivers return
    /// false because distinct owned operands cannot alias; handle drivers compare
    /// their canonical identities, rather than geometry equality.
    fn same_operand(&self, left: &Self::Operand, right: &Self::Operand) -> bool;
    /// `SmMerge(context, primary, other, approx_tol, angle_tol, options)`.
    fn construct(
        &mut self,
        k: &mut Kernel,
        primary: &mut Option<Self::Operand>,
        other: &mut Option<Self::Operand>,
        approx_tol: f64,
        angle_tol: f64,
        options: Option<&MergeOptions>,
    ) -> Result<()>;
    /// `ManifoldBoolean(operation, result, caller_lock)`, sharing the canonical
    /// merge's manifold flag and intersector lamina-edge control. The implementation
    /// updates operand slots when upstream deletes or replaces a B-rep.
    fn manifold_boolean(
        &mut self,
        k: &mut Kernel,
        primary: &mut Option<Self::Operand>,
        other: &mut Option<Self::Operand>,
        manifold: &mut bool,
        lamina_edges: &mut u64,
        operation: BooleanOperation,
        caller_lock: Option<&mut MarkLock>,
    ) -> Result<Option<Self::Operand>>;
}

/// The `SmMerge` the API wrappers drive: `construct` (`boolean.merge_p2`) builds the
/// engine and `ManifoldBoolean` runs the `boolean.merge_p4` port on it. A default
/// adapter has none until it is constructed. No engine state or operand result is faked.
///
/// A driver made by [`DependencyMerge::carrying_attributes`] also carries the
/// operands' own attributes ([`Brep::add_attribute`]) through each merge it runs
/// (not upstream: upstream's live on the objects the merge edits): `construct` moves
/// them into the engine's store ([`MergeEngine::seed_operand_attributes`]) and
/// `ManifoldBoolean` gives the survivors back to the B-reps it leaves
/// ([`MergeEngine::restore_operand_attributes`]).
#[derive(Debug, Default)]
pub struct DependencyMerge {
    engine: Option<MergeEngine>,
    carry_attributes: bool,
}

/// The `SmMerge` members that `ManifoldBoolean` reads and writes. Their owner
/// reads results (such as `ExtractSeparate`'s component arrays) through
/// [`DependencyMerge::engine`] or takes them back with [`DependencyMerge::into_engine`].
#[derive(Debug)]
pub struct MergeEngine {
    /// `m_vTI` and the merge options, tolerances and mates (canonical adapter).
    pub intersector: KernelManifoldIntersector,
    /// Retained intersection lists, extracted components and traversal marks.
    pub state: ManifoldState,
    /// The attribute store the merge's notifications and region data use.
    pub attributes: AttributeStore,
}

impl MergeEngine {
    /// The engine members of a merge built by `SmMerge(context, pBrep, pOtherBrep,
    /// dApproxTol3d, dAngleTolRadians, pOptMergeOptions)` ([`MergeState::with_breps`]):
    /// its options, tag and `m_bManifoldBoolean`, its intersector's (possibly computed)
    /// tolerances and `m_bIntersectionLoopClosed`. The engine owns a fresh attribute
    /// store, so its notification keys use namespace 0. The adapter always names both
    /// operand slots; a null other B-rep is refused by `ManifoldBoolean` itself.
    ///
    /// # Errors
    /// None in practice (namespace 0 is valid); those of [`KernelManifoldIntersector::new`].
    pub fn from_merge(merge: &MergeState<TopologyIntersector<MergeOperand>>) -> Result<Self> {
        let mut intersector = KernelManifoldIntersector::new(0)?;
        intersector.merge.options = merge.options;
        intersector.merge.tag_id = merge.tag_id;
        intersector.merge.manifold_boolean = merge.manifold_boolean;
        intersector.merge.intersection_loop_closed = merge.intersection_loop_closed;
        intersector.set_intersection_tolerances(
            merge.intersector.this_approx_tol_3d(),
            merge.intersector.this_ang_tol_rad(),
        );
        let mut state = ManifoldState::default();
        if let Some(closed) = merge.intersection_loop_closed {
            state.intersection_loop_closed = closed;
        }
        Ok(Self {
            intersector,
            state,
            attributes: AttributeStore::default(),
        })
    }

    /// Hand geometry created under this engine's notification namespace back to each
    /// operand's recorded naming scheme ([`Brep::naming`](crate::topology::Brep::naming)).
    ///
    /// # Errors
    /// [`SolidError::NotFound`] for a dangling topology record (none in practice).
    pub fn adopt_operand_owners(
        &self,
        primary: Option<&mut Brep>,
        other: Option<&mut Brep>,
    ) -> Result<()> {
        if let Some(namespace) = self.intersector.namespace() {
            for (body, is_other) in [(primary, false), (other, true)] {
                if let Some(body) = body {
                    crate::boolean::merge::p4::adopt_namespace_owners(body, namespace, is_other)?;
                }
            }
        }
        Ok(())
    }

    /// (not upstream: upstream's attributes stay on the operands' objects, which the
    /// merge edits in place.) Moves each operand's own attributes
    /// ([`Brep::add_attribute`]) into the engine's store under the merge's names of
    /// their objects, so the merge's notifications propagate them with SMLib's
    /// handlers. Pair with [`Self::restore_operand_attributes`].
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] for an engine using the caller's identities (it has
    /// no names of its own for the operands) or an attribute list whose key names no
    /// object.
    pub fn seed_operand_attributes(
        &mut self,
        primary: Option<&mut Brep>,
        other: Option<&mut Brep>,
    ) -> Result<()> {
        const SEED: &str = "boolean::merge::p3::MergeEngine::seed_operand_attributes";
        let namespace = self
            .intersector
            .namespace()
            .ok_or(SolidError::invalid_input(
                SEED,
                "the engine names no operand objects",
            ))?;
        for (body, is_other) in [(primary, false), (other, true)] {
            let Some(body) = body else { continue };
            let store = body.take_attributes();
            self.attributes.absorb(store, &|key| {
                crate::topology::loopuse::entity_of_notify_key(key)
                    .map(|e| {
                        crate::boolean::merge::p4::notification_key(namespace, is_other, e).key
                    })
                    .ok_or(SolidError::invalid_input(
                        SEED,
                        "attribute list names no object",
                    ))
            })?;
        }
        Ok(())
    }

    /// (not upstream) Hands the engine's attributes back to the B-reps the merge
    /// leaves: each live object of `result` (the primary slot's arena) and of a kept
    /// `other` gets the attribute list the merge's notifications left on it; every other
    /// object's list (an object the merge deleted, or of the other operand when it is
    /// consumed) is cleared as upstream's object destructor does. A reference attribute
    /// on objects of both returned B-reps becomes one attribute per B-rep.
    ///
    /// # Errors
    /// None in practice.
    pub fn restore_operand_attributes(
        &mut self,
        result: Option<&mut Brep>,
        other: Option<&mut Brep>,
    ) -> Result<()> {
        let Some(namespace) = self.intersector.namespace() else {
            return Ok(());
        };
        for (body, is_other) in [(result, false), (other, true)] {
            let Some(body) = body else { continue };
            let mut store = self.attributes.clone();
            let live = &*body;
            let listed = live.listed_entities();
            store.retain_rekeyed(&|key| {
                crate::boolean::merge::p4::entity_of_notification_key(namespace, key)
                    .filter(|&(o, e)| o == is_other && live.still_has(&listed, e))
                    .map(|(_, e)| crate::topology::loopuse::notify_ref(e).key)
            })?;
            body.set_attributes(store);
        }
        Ok(())
    }
}

impl DependencyMerge {
    /// Adopt the members of a constructed `SmMerge`. For the constructor's owner.
    #[must_use]
    pub fn with_engine(engine: MergeEngine) -> Self {
        Self {
            engine: Some(engine),
            carry_attributes: false,
        }
    }

    /// The adopted engine, if any.
    #[must_use]
    pub fn engine(&self) -> Option<&MergeEngine> {
        self.engine.as_ref()
    }

    /// A driver whose merges carry the operands' own attributes (see the type).
    #[must_use]
    pub fn carrying_attributes() -> Self {
        Self {
            engine: None,
            carry_attributes: true,
        }
    }

    /// The adopted engine, mutably, if any.
    pub fn engine_mut(&mut self) -> Option<&mut MergeEngine> {
        self.engine.as_mut()
    }

    /// Give the adopted engine back to its owner, with every result written to it,
    /// including after a failed call.
    #[must_use]
    pub fn into_engine(self) -> Option<MergeEngine> {
        self.engine
    }
}

impl BooleanDriver for DependencyMerge {
    type Operand = Brep;

    fn same_operand(&self, _left: &Brep, _right: &Brep) -> bool {
        false
    }

    fn construct(
        &mut self,
        k: &mut Kernel,
        primary: &mut Option<Brep>,
        other: &mut Option<Brep>,
        approx_tol: f64,
        angle_tol: f64,
        options: Option<&MergeOptions>,
    ) -> Result<()> {
        // SM_ASSERT(pBrep != NULL); the other B-rep may be null (the stitch methods).
        let first = primary.as_ref().ok_or(SolidError::invalid_input(
            OP,
            "the merge has no primary B-rep",
        ))?;
        let merge = MergeState::with_breps(
            k,
            (first, MergeOperand::Primary),
            other.as_ref().map(|o| (o, MergeOperand::Other)),
            approx_tol,
            angle_tol,
            options,
        )?;
        let mut engine = MergeEngine::from_merge(&merge)?;
        if self.carry_attributes {
            // (not upstream) the operands' attributes go to the merge's objects
            engine.seed_operand_attributes(primary.as_mut(), other.as_mut())?;
        }
        self.engine = Some(engine);
        Ok(())
    }

    /// The operands share `k` (one context). An assigned `rpResult` is the
    /// primary B-rep, moved out of its slot; `ExtractSeparate` leaves it
    /// unassigned and its components in the engine's manifold state.
    fn manifold_boolean(
        &mut self,
        k: &mut Kernel,
        primary: &mut Option<Brep>,
        other: &mut Option<Brep>,
        manifold: &mut bool,
        lamina_edges: &mut u64,
        operation: BooleanOperation,
        caller_lock: Option<&mut MarkLock>,
    ) -> Result<Option<Brep>> {
        // A member function of a constructed SmMerge: `construct` comes first.
        let Some(engine) = self.engine.as_mut() else {
            return Err(SolidError::invalid_input(
                OP,
                "the Boolean runs before the merge is constructed",
            ));
        };
        engine.intersector.set_intersect_lamina_edges(*lamina_edges);
        let mut kernels = BooleanKernels {
            primary: k,
            other: None,
        };
        let assigned = engine.intersector.manifold_boolean(
            &mut kernels,
            primary,
            other,
            &mut engine.state,
            &mut engine.attributes,
            *manifold,
            operation,
            caller_lock,
        )?;
        // The engine's notification namespace (and its attribute store) ends with the
        // merge: geometry the merge made owns under the record's own name in the
        // B-rep's scheme, as every other record's geometry (upstream's owner is the
        // record itself).
        engine.adopt_operand_owners(primary.as_mut(), other.as_mut())?;
        if self.carry_attributes {
            // (not upstream) the attributes the merge's notifications left on the
            // result's (the primary slot's arena) and the kept other B-rep's objects
            // go back to those B-reps
            engine.restore_operand_attributes(primary.as_mut(), other.as_mut())?;
        }
        Ok(if assigned { primary.take() } else { None })
    }
}

/// Evaluate all postfix trees, then append unused operands in original order.
/// Leaves transfer an operand to the stack and clear its input slot immediately.
/// Ordinary failures of `ManifoldBoolean` become `SM_ERR`; Unported propagates, and so
/// does `InvalidInput`, the port's report of undefined behaviour upstream.
/// The token array is read-only, matching upstream despite its mutable signature.
pub fn boolean_trees<D: BooleanDriver>(
    k: &mut Kernel,
    breps: &mut Vec<Option<D::Operand>>,
    trees: &[i64],
    options: Option<&MergeOptions>,
    driver: &mut D,
) -> Result<()> {
    let mut stack = Vec::new();
    for &node in trees {
        if node >= 0 {
            let index = usize::try_from(node)
                .map_err(|_| SolidError::invalid_input(OP, "B-rep index overflow"))?;
            let slot = breps
                .get_mut(index)
                .ok_or_else(|| SolidError::invalid_input(OP, "B-rep index out of range"))?;
            stack.push(slot.take());
        } else {
            if stack.len() < 2 {
                return Err(SolidError::stage_failed(
                    OP,
                    "a Boolean operator has fewer than two operands",
                ));
            }
            let mut other = stack.pop().flatten();
            let mut primary = stack.pop().flatten();
            if primary.is_none() || other.is_none() {
                return Err(SolidError::invalid_input(
                    OP,
                    "binary operation on a null tree operand",
                ));
            }
            driver.construct(k, &mut primary, &mut other, 0.0, 0.0, options)?;
            let operation = match node {
                -1 => BooleanOperation::Union,
                -2 => BooleanOperation::Intersection,
                -3 => BooleanOperation::Difference,
                -4 => BooleanOperation::Merge,
                -5 => BooleanOperation::ExclusiveOr,
                -6 => BooleanOperation::Slice,
                _ => {
                    return Err(SolidError::stage_failed(
                        OP,
                        "a Boolean operator code is not one the tree knows",
                    ))
                }
            };
            k.check_cancelled(OP)?;
            let result = driver
                .manifold_boolean(
                    k,
                    &mut primary,
                    &mut other,
                    &mut true,
                    &mut 0,
                    operation,
                    None,
                )
                .map_err(|e| {
                    // undefined behaviour the port detects (InvalidInput) keeps its
                    // error, as Unported does
                    if e.is_unported_or_cancelled()
                        || matches!(
                            e,
                            SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }
                        )
                    {
                        e
                    } else {
                        SolidError::failed_after(OP, "a Boolean of the tree failed", e)
                    }
                })?;
            stack.push(result);
        }
    }
    stack.extend(breps.iter_mut().filter_map(Option::take).map(Some));
    *breps = stack;
    Ok(())
}

/// Fold at least two distinct B-reps in the upstream last-to-first order. Operation codes
/// are 0 union, 1 intersection, 2 difference, 3 merge. Validation is non-consuming;
/// accepted input is drained before tree evaluation, including on later failure.
pub fn merge_breps<D: BooleanDriver>(
    k: &mut Kernel,
    breps: &mut Vec<D::Operand>,
    operation: u32,
    options: Option<&MergeOptions>,
    driver: &mut D,
) -> Result<D::Operand> {
    if breps.len() < 2 || operation > 3 {
        return Err(SolidError::invalid_input(
            OP,
            "fewer than two B-reps or operation outside 0..=3",
        ));
    }
    for (index, operand) in breps.iter().enumerate() {
        if breps
            .iter()
            .take(index)
            .any(|earlier| driver.same_operand(operand, earlier))
        {
            return Err(SolidError::invalid_input(OP, "duplicate B-rep operand"));
        }
    }
    let mut ordered = Vec::new();
    let mut trees = vec![0];
    while let Some(brep) = breps.pop() {
        let index = i64::try_from(ordered.len())
            .map_err(|_| SolidError::invalid_input(OP, "too many B-reps"))?;
        ordered.push(Some(brep));
        if index != 0 {
            trees.push(index);
            trees.push(-(i64::from(operation) + 1));
        }
    }
    boolean_trees(k, &mut ordered, &trees, options, driver)?;
    if ordered.len() != 1 {
        return Err(SolidError::stage_failed(
            OP,
            "the Boolean tree does not reduce to one B-rep",
        ));
    }
    ordered
        .pop()
        .flatten()
        .ok_or_else(|| SolidError::stage_failed(OP, "the Boolean tree has no result"))
}

/// Temporarily disable manifold enforcement, set lamina intersection mode to 1,
/// and call `ManifoldBoolean`. The manifold flag is restored on every exit;
/// lamina mode deliberately remains changed, as upstream.
pub fn non_manifold_boolean<D: BooleanDriver>(
    k: &mut Kernel,
    driver: &mut D,
    primary: &mut Option<D::Operand>,
    other: &mut Option<D::Operand>,
    manifold: &mut bool,
    lamina_edges: &mut u64,
    operation: BooleanOperation,
) -> Result<Option<D::Operand>> {
    let saved = *manifold;
    *manifold = false;
    *lamina_edges = 1;
    let result =
        driver.manifold_boolean(k, primary, other, manifold, lamina_edges, operation, None);
    *manifold = saved;
    result
}

/// Read/edit access needed by `PiecewiseMerge`. Surface and B-rep values are
/// stable handles in the implementation's canonical storage. Attribute methods
/// refer to the shared `SM_AI_SURFACEMAP` attribute and its live user list.
/// No second relationship map or surface table is maintained by this unit.
/// The sweep store supplies its retained merge engine and caller-owned bodies;
/// the unit's test drivers also exercise skipped-surface bookkeeping.
pub trait PiecewiseAccess: BooleanDriver {
    /// Stable surface identity (including the containing B-rep).
    type Surface: Copy;
    /// Current `m_vTI.m_pBrep` and `m_vTI.m_pOther` handles.
    fn operands(&self) -> (Option<Self::Operand>, Option<Self::Operand>);
    /// Replace the intersector's operand pointers. Deleted handles must remain
    /// invalid: a saved handle reinstalled after an upstream deletion must be
    /// rejected on access, never alias a subsequently allocated arena slot.
    fn set_operands(&mut self, primary: Option<Self::Operand>, other: Option<Self::Operand>);
    /// `GetSurfaces`, in B-rep list order.
    fn surfaces(&self, brep: Self::Operand) -> Result<Vec<Self::Surface>>;
    /// `IntersectInsertRelate()`. Its error ends the merge (SolidCraft fix
    /// tangency-labs/smlib af8db37; upstream ignores the status).
    fn intersect_insert_relate(&mut self, k: &mut Kernel) -> Result<()>;
    /// `m_pBToO->RemoveAll(); m_pOToB->RemoveAll()`.
    fn clear_relationships(&mut self) -> Result<()>;
    /// `SetSubset`, copying the supplied list.
    fn set_subset(&mut self, surfaces: &[Self::Surface]);
    /// Add a new reference `SM_AI_SURFACEMAP` pointer attribute. When
    /// `only_if_missing` is true, preserve an existing attribute.
    fn surface_map(&mut self, surface: Self::Surface, only_if_missing: bool) -> Result<()>;
    /// `FindAttribute(SM_AI_SURFACEMAP)->GetUsers()`. None means no attribute.
    fn surface_map_users(&self, surface: Self::Surface) -> Result<Option<Vec<Self::Surface>>>;
    /// A new empty B-rep in the output context, SetTolerance(source tolerance,
    /// TRUE, TRUE) in the reference old-tolerance build, editing enabled.
    fn new_face_brep(&mut self, k: &mut Kernel, source: Self::Operand) -> Result<Self::Operand>;
    /// The manifold flag (`m_bManifoldBoolean`).
    fn manifold_flag(&self) -> bool;
    /// Set the manifold flag and wire/lamina-edge intersection controls.
    fn set_controls(&mut self, manifold: bool, wires: bool, lamina: u64);
    /// `ProcessWiresAndShellVertices(Merge, keep, mark1, mark2)`. Its keep list
    /// is local and discarded, as upstream. `lock` is the preceding call's lock.
    fn process_wires(&mut self, k: &mut Kernel, lock: &MarkLock) -> Result<()>;
    /// Release the paired mark lock against the real operand contexts on every
    /// exit, including an error from `ManifoldBoolean` or wire processing.
    fn release_marks(&mut self, k: &mut Kernel, lock: &mut MarkLock) -> Result<()>;
    /// `RegisterExistingSSI(skip, user, empty3d, emptyuv1, emptyuv2,
    /// emptyorientations, emptydeviations)` (all five arrays are initially empty).
    fn register_empty_ssi(&mut self, skip: Self::Surface, user: Self::Surface) -> Result<()>;
    /// `ClearExistingSSI()` after the second merge of a stand-alone face.
    fn clear_existing_ssi(&mut self) -> Result<()>;
    /// `Notify(PreEdit, primary, NULL, NULL)`; ordinary failure is ignored.
    fn notify_pre_edit(&mut self, k: &mut Kernel, primary: Self::Operand) -> Result<()>;
    /// Delete the current other operand, then set that pointer to null.
    fn delete_other(&mut self, k: &mut Kernel) -> Result<()>;
}

/// Merge each surface of the other operand, optionally through a stand-alone
/// face B-rep. `skip_pairs` is the surface relation (other surface, skipped primary
/// surface); an empty non-null relation still enables attribute and SSI cleanup.
/// `use_existing_topology` is deliberately unused upstream.
pub fn piecewise_merge<D: PiecewiseAccess>(
    k: &mut Kernel,
    driver: &mut D,
    skip_pairs: Option<&Relation<D::Surface, D::Surface>>,
    use_existing_topology: bool,
    create_new_breps_for_faces: bool,
) -> Result<Option<D::Operand>>
where
    D::Operand: Copy,
    D::Surface: Ord,
{
    let (final_result, original_other) = driver.operands();
    final_result.ok_or_else(|| SolidError::entity_missing(OP, "the merge has no result B-rep"))?;
    let original_other = original_other
        .ok_or_else(|| SolidError::entity_missing(OP, "the merge has no second B-rep"))?;
    // The first surface query is observable even though upstream overwrites it.
    let _ = driver.surfaces(original_other)?;
    // SER: a failed IntersectInsertRelate (a failed imprint leaves both B-reps
    // half edited) ends the merge (SolidCraft change, tangency-labs/smlib af8db37;
    // upstream ignores its status).
    driver.intersect_insert_relate(k)?;
    piecewise_merge_related(
        k,
        driver,
        skip_pairs,
        use_existing_topology,
        create_new_breps_for_faces,
    )
}

/// [`piecewise_merge`] after its leading `m_vTI.IntersectInsertRelate()`: the caller
/// has already intersected, inserted and related the operands on the intersector
/// that `SmMerge::m_vTI` names (as `SmFilletExecutive::DoPiecewiseMerge` does with
/// its own `m_vTI`, which carries its corners' registered SSI), and the
/// face-at-a-time merges follow on `driver`.
///
/// # Errors
/// As [`piecewise_merge`].
pub fn piecewise_merge_related<D: PiecewiseAccess>(
    k: &mut Kernel,
    driver: &mut D,
    skip_pairs: Option<&Relation<D::Surface, D::Surface>>,
    use_existing_topology: bool,
    create_new_breps_for_faces: bool,
) -> Result<Option<D::Operand>>
where
    D::Operand: Copy,
    D::Surface: Ord,
{
    let (final_result, original_other) = driver.operands();
    let final_result = final_result
        .ok_or_else(|| SolidError::entity_missing(OP, "the merge has no result B-rep"))?;
    let original_other = original_other
        .ok_or_else(|| SolidError::entity_missing(OP, "the merge has no second B-rep"))?;
    let surfaces = driver.surfaces(original_other)?;
    driver.clear_relationships()?;
    if let Some(pairs) = skip_pairs {
        for &surface in &surfaces {
            driver.surface_map(surface, false)?;
            for skip in pairs.seconds(surface) {
                driver.surface_map(skip, true)?;
            }
        }
    }
    for (index, &surface) in surfaces.iter().enumerate() {
        driver.set_subset(&[surface]);
        // bUseExistingTopology's upstream branch contains comments only.
        let _ = use_existing_topology;
        if create_new_breps_for_faces {
            let new_brep = driver.new_face_brep(k, final_result)?;
            driver.set_operands(Some(new_brep), Some(original_other));
        }
        let mut manifold = driver.manifold_flag();
        driver.set_controls(manifold, false, 0);
        let (mut primary, mut other) = driver.operands();
        let mut lock = MarkLock::unbound();
        let first = driver.manifold_boolean(
            k,
            &mut primary,
            &mut other,
            &mut manifold,
            &mut 0,
            BooleanOperation::PartialMerge,
            Some(&mut lock),
        );
        driver.set_operands(primary, other);
        let stage = first.and_then(|_| {
            if index + 1 == surfaces.len() {
                driver.process_wires(k, &lock)?;
            }
            Ok(())
        });
        let release = driver.release_marks(k, &mut lock);
        stage?;
        ignored(release)?;
        driver.clear_relationships()?;
        if create_new_breps_for_faces {
            if let Some(pairs) = skip_pairs {
                let users = driver.surface_map_users(surface)?.ok_or_else(|| {
                    SolidError::entity_missing(OP, "the surface has no users in the surface map")
                })?;
                for user in users {
                    for skip in pairs.seconds(surface) {
                        if let Some(skip_users) = driver.surface_map_users(skip)? {
                            for skip_user in skip_users {
                                driver.register_empty_ssi(skip_user, user)?;
                            }
                        }
                    }
                }
            }
            let (new_brep, _) = driver.operands();
            driver.set_operands(Some(final_result), new_brep);
            driver.set_subset(&[]);
            driver.set_controls(false, false, 3);
            ignored(driver.notify_pre_edit(k, final_result))?;
            let (mut primary, mut other) = driver.operands();
            let merged = driver.manifold_boolean(
                k,
                &mut primary,
                &mut other,
                &mut false,
                &mut 3,
                BooleanOperation::Merge,
                None,
            );
            driver.set_operands(primary, other);
            if let Err(error) = merged {
                if driver.operands().1.is_some() {
                    driver.delete_other(k)?;
                }
                return Err(error);
            }
            driver.clear_relationships()?;
            if skip_pairs.is_some() {
                driver.clear_existing_ssi()?;
            }
        }
    }
    let (result, _) = driver.operands();
    if create_new_breps_for_faces {
        driver.set_operands(result, Some(original_other));
    }
    Ok(result)
}

fn empty_brep(
    allocation_context: &mut Kernel,
    model_size_estimate: f64,
    large_small_size_ratio: f64,
    stack_context: Option<&Kernel>,
) -> Result<Brep> {
    // `new SmBrep(...)` (topology::brep::p3::new_brep; pOptContextForStack is the
    // allocating kernel, as there)
    let _ = stack_context;
    crate::topology::brep::p3::new_brep(
        allocation_context,
        model_size_estimate,
        large_small_size_ratio,
    )
}

/// The identity of a source B-rep's objects in the arena-only copy and merge: the
/// destination's scheme (`loopuse::notify_ref`, as `ArenaNotifyState`) in a namespace of
/// its own (bit 40), so a source object never shares a key with a destination object.
fn source_ref(e: EntityRef) -> crate::geom::object::ObjectRef {
    let r = crate::topology::loopuse::notify_ref(e);
    crate::geom::object::ObjectRef {
        type_code: r.type_code,
        key: crate::geom::attribute::ObjectKey(r.key.0 | 1 << 40),
    }
}

/// `new SmBrep(source, pOptCopyBrepMap, pOptReverseMap, bCopyAttributes, bAddAnalytics)`
/// (`topology::brep::p3::copy_brep`): the list operands carry no notification state, so
/// the copy notifies through an arena-only store (`ArenaNotifyState`: default records,
/// no context), as the arena entry's component copies do. (not upstream) The store holds
/// the source's own attributes under [`source_ref`], so the copy's `SM_NO_COPY`
/// notifications give the copy's objects theirs; the copy keeps them.
pub(crate) fn copy_brep(
    k: &mut Kernel,
    source: &Brep,
    copy_map: Option<&mut CopyBrepMap<EntityRef>>,
    reverse_map: Option<&mut CopyBrepMap<EntityRef>>,
    copy_attributes: bool,
    add_analytics: bool,
) -> Result<Brep> {
    let mut state = crate::topology::brep::p2::ArenaNotifyState::default();
    crate::topology::brep::p2::seed_brep_attributes(
        &mut state.attributes,
        source.attribute_store().clone(),
        &source_ref,
    )?;
    let mut brep = crate::topology::brep::p3::copy_brep(
        k,
        &mut state.env(None),
        source,
        &source_ref,
        copy_map,
        reverse_map,
        copy_attributes,
        add_analytics,
    )?;
    brep.set_naming(crate::topology::NamingScheme::Notify);
    state.restore_attributes(&mut brep)?;
    Ok(brep)
}

/// `SmBrep::MergeBrep(source, pOptCopyBrepMap, pOptReverseMap, bAddAnalytics)`
/// (`topology::brep::p3::merge_brep`), notifying as [`copy_brep`].
pub(crate) fn combine_brep(
    k: &mut Kernel,
    target: &mut Brep,
    source: &Brep,
    copy_map: Option<&mut CopyBrepMap<EntityRef>>,
    reverse_map: Option<&mut CopyBrepMap<EntityRef>>,
    add_analytics: bool,
) -> Result<()> {
    // (not upstream) both B-reps' own attributes, the source's under `source_ref`; the
    // target keeps those its objects end with
    let mut state = crate::topology::brep::p2::ArenaNotifyState {
        attributes: target.take_attributes(),
        ..Default::default()
    };
    let seeded = crate::topology::brep::p2::seed_brep_attributes(
        &mut state.attributes,
        source.attribute_store().clone(),
        &source_ref,
    );
    let r = seeded.and_then(|()| {
        crate::topology::brep::p3::merge_brep(
            k,
            &mut state.env(None),
            target,
            source,
            &source_ref,
            copy_map,
            reverse_map,
            add_analytics,
        )
    });
    let restored = state.restore_attributes(target);
    r?;
    restored
}

fn stitch_and_orient(
    k: &mut Kernel,
    brep: &mut Brep,
    tolerance: f64,
    allow_spine_edges: bool,
    faces_to_swap: Option<&mut Vec<FaceId>>,
    just_unify: Option<&mut bool>,
) -> Result<()> {
    crate::topology::brep::p5::stitch_and_orient(
        k,
        brep,
        &crate::topology::brep::p5::StitchAndOrientOptions {
            stitch_tolerance: tolerance,
            allow_spine_edges,
            faces_to_swap: faces_to_swap.map(|faces| faces.clone()),
            just_unify: just_unify.map(|value| *value),
        },
    )
}

fn shrink_geometry(k: &mut Kernel, brep: &mut Brep) -> Result<()> {
    crate::topology::brep::p4::shrink_geometry(k, brep)
}

fn boolean_2d(
    k: &mut Kernel,
    mut primary: Brep,
    mut other: Brep,
    operation: BooleanOperation,
    notifications: Option<&mut crate::topology::edge::NotifyEnv<'_>>,
) -> Result<Brep> {
    use crate::primitive::primitive_creation::p1::{Boolean2d, PrimitiveCreation};
    let operation = match operation {
        BooleanOperation::Intersection => Boolean2d::Intersection,
        BooleanOperation::Union => Boolean2d::Union,
        BooleanOperation::ExclusiveOr => Boolean2d::ExclusiveOr,
        BooleanOperation::Difference => Boolean2d::Difference,
        BooleanOperation::Merge => Boolean2d::Merge,
        _ => return Err(SolidError::invalid_input(OP, "unsupported planar Boolean")),
    };
    let mut primitive = PrimitiveCreation::new(None, 0, false);
    let mut primary_marks = Marks::default();
    let mut other_marks = Marks::default();
    let mut result = None;
    if let Some(notify) = notifications {
        primitive.boolean_2d(
            k,
            &mut primary,
            &mut other,
            operation,
            &mut result,
            (&mut primary_marks, &mut other_marks),
            notify,
        )?;
    } else {
        // As in component copying, bare arenas have no attribute records. Seed
        // both operands, retaining caller-supplied notifications when available.
        let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&primary);
        state.seed(&other);
        primitive.boolean_2d(
            k,
            &mut primary,
            &mut other,
            operation,
            &mut result,
            (&mut primary_marks, &mut other_marks),
            &mut state.env(None),
        )?;
    }
    // Upstream dereferences pBrep3 next, even when Boolean2D left it null.
    result.ok_or_else(|| SolidError::entity_missing(OP, "the planar Boolean made no result B-rep"))
}

fn remove_faces(k: &mut Kernel, brep: &mut Brep, faces: &[FaceId]) -> Result<()> {
    let editing_enabled = brep.editing_enabled();
    crate::topology::brep::p5::remove_faces(k, brep, editing_enabled, faces)
}

fn list_operand(k: &mut Kernel, breps: &[Brep], surfaces: &[Surface]) -> Result<(Brep, bool)> {
    let (mut result, planar) = if let Some((first, rest)) = breps.split_first() {
        let mut result = copy_brep(k, first, None, None, true, true)?;
        for source in rest {
            // MergeBrep's ordinary status is ignored upstream.
            ignored(crate::error::passed_on(
                combine_brep(k, &mut result, source, None, None, true),
                k.fixes.brep_edit_statuses_pass_on,
            )?)?;
        }
        (result, false)
    } else {
        if surfaces.is_empty() {
            return Err(SolidError::stage_failed(
                OP,
                "there are no surfaces to merge",
            ));
        }
        let mut result = empty_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
            None,
        )?;
        let mut planar = false;
        // surfaces, make into single brep with multiple faces.
        for source in surfaces {
            // Upstream records ONLY the last surface's kind ("we hope they are all the
            // same").
            // the virtual Copy: the derived surface, its natural domain and its kind, where
            // upstream's base copy constructor slices the surface (WR-196)
            let copy = source.copy(k)?;
            planar = copy.is_kind_of(PLANE_TYPE)?;
            let domain = copy.natural_uv_domain_with_nurb(k)?;
            crate::topology::brep::sheet::face_on(k, &mut result, copy, &domain)?;
        }
        (result, planar)
    };
    stitch_and_orient(k, &mut result, -0.1, false, None, None)?;
    Ok((result, planar))
}

/// `CopyFaces(faces, component, use_merge = TRUE)` of the arena-only list entry
/// (`topology::brep::p7::copy_faces`'s merge branch): the copy notifies through an
/// arena-only store whose source identities ([`source_ref`]) are distinct from the
/// component's (`loopuse::notify_ref`, faces by `face_ref`), seeded with `result`'s own
/// attributes and the component's; the component gets the attributes its objects end
/// with. The ordinary status is not acted on, as `copy_faces`' merge branch does not.
pub(crate) fn copy_component(
    k: &mut Kernel,
    result: &mut Brep,
    connected: &[FaceId],
    component: &mut Brep,
    face_ref: &dyn Fn(FaceId) -> crate::geom::object::ObjectRef,
) -> Result<()> {
    use crate::topology::loopuse::notify_ref;
    use crate::topology::{EdgeId, VertexId};
    let mut state = crate::topology::brep::p2::ArenaNotifyState::new(result);
    state.seed(component);
    crate::topology::brep::p3::seed_records(&mut state.records, result, &source_ref);
    crate::topology::brep::p2::seed_brep_attributes(
        &mut state.attributes,
        result.attribute_store().clone(),
        &source_ref,
    )?;
    // the component's own attributes (none for a new component), under its names
    let own = component.take_attributes();
    crate::topology::brep::p2::seed_brep_attributes(&mut state.attributes, own, &|e| match e {
        EntityRef::Face(f) => face_ref(f),
        _ => notify_ref(e),
    })?;
    {
        let mut copy = super::p2::TopologyCopy {
            vertices: super::p1::VertexCopy {
                records: &mut state.records,
                attributes: &mut state.attributes,
                source_key: &|v: VertexId| source_ref(EntityRef::Vertex(v)),
                destination_key: &notify_ref,
            },
            edge_source_key: &|e: EdgeId| source_ref(EntityRef::Edge(e)),
            edge_destination_key: &|e: EdgeId| notify_ref(EntityRef::Edge(e)),
            face_source_key: &|f: FaceId| source_ref(EntityRef::Face(f)),
            // (`result`'s lists, a surface's included, are under `source_ref` here, so
            // the surface copy's read under a surface owner's own name could only find a
            // list of the component's: none of the result surface's own)
            surface_attributes: false,
            face_destination_key: face_ref,
        };
        ignored(bp7::copy_faces_with_merge(
            super::p2::MergeContext::Split(k, None),
            result,
            connected,
            component,
            &mut crate::boolean::topology_intersector::p1::Relations::default(),
            &mut copy,
        ))?;
    }
    // the component's faces are named by `face_ref` (the raw arena index), its other
    // objects by `notify_ref`
    let faces: std::collections::BTreeMap<u64, FaceId> = bp::faces(component, None)?
        .into_iter()
        .map(|f| (face_ref(f).key.0, f))
        .collect();
    crate::topology::brep::p2::restore_decoded_attributes(
        component,
        core::mem::take(&mut state.attributes),
        &|key| match faces.get(&key.0) {
            Some(&f) => Some(EntityRef::Face(f)),
            None => crate::topology::loopuse::entity_of_notify_key(key)
                .filter(|e| !matches!(e, EntityRef::Face(_) | EntityRef::Brep(_))),
        },
    )
}

/// The independent B-rep or planar surface outputs of [`boolean_lists`].
#[derive(Debug, Default)]
pub struct BooleanListsResult {
    /// One B-rep per separate solid, for 3D or non-planar inputs.
    pub breps: Vec<Brep>,
    /// One surface per face when every input is 2D coplanar.
    pub surfaces: Vec<Surface>,
}

/// Combine copies of two B-rep/surface lists, run the requested Boolean, and
/// return either planar surfaces or one B-rep per connected component. Operation
/// 0 intersection, 1 union, 2 xor, 3 A minus B, 4 B minus A. Nonempty B-rep lists
/// take precedence over surface lists; inputs remain unchanged.
pub fn boolean_lists(
    k: &mut Kernel,
    breps1: &[Brep],
    surfaces1: &[Surface],
    breps2: &[Brep],
    surfaces2: &[Surface],
    operation: i32,
    options: Option<&MergeOptions>,
) -> Result<BooleanListsResult> {
    boolean_lists_with_notifications(
        k, breps1, surfaces1, breps2, surfaces2, operation, options, None,
    )
}

/// [`boolean_lists`] with caller-owned topology records, attributes and identity
/// routing for the component-copy notifications. Without them (the arena-only entry)
/// the copy notifies through an arena-only store (`ArenaNotifyState`, a default record
/// per object of both B-reps): the arena carries no attribute records.
pub fn boolean_lists_with_notifications(
    k: &mut Kernel,
    breps1: &[Brep],
    surfaces1: &[Surface],
    breps2: &[Brep],
    surfaces2: &[Surface],
    operation: i32,
    options: Option<&MergeOptions>,
    mut notifications: Option<&mut crate::topology::edge::NotifyEnv<'_>>,
) -> Result<BooleanListsResult> {
    let (mut primary, planar1) = list_operand(k, breps1, surfaces1)?;
    let (mut other, planar2) = list_operand(k, breps2, surfaces2)?;
    let both_planar = planar1 && planar2;
    let kind = match operation {
        0 => BooleanOperation::Intersection,
        1 => BooleanOperation::Union,
        2 => BooleanOperation::ExclusiveOr,
        3 | 4 => BooleanOperation::Difference,
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "unsupported list Boolean operation",
            ))
        }
    };
    if operation == 4 {
        std::mem::swap(&mut primary, &mut other);
    }
    let mut result = if both_planar {
        boolean_2d(k, primary, other, kind, notifications.as_deref_mut())?
    } else {
        let mut primary = Some(primary);
        let mut other = Some(other);
        let mut driver = DependencyMerge::carrying_attributes();
        driver.construct(k, &mut primary, &mut other, 0.0, 0.0, options)?;
        non_manifold_boolean(
            k,
            &mut driver,
            &mut primary,
            &mut other,
            &mut true,
            &mut 0,
            kind,
        )?
        .ok_or_else(|| SolidError::entity_missing(OP, "the Boolean made no result B-rep"))?
    };
    shrink_geometry(k, &mut result)?;
    stitch_and_orient(k, &mut result, -0.1, false, None, None)?;
    let mut output = BooleanListsResult::default();
    if both_planar {
        for surface in bp::surfaces(&result, None)? {
            output.surfaces.push(surface.copy(k)?);
        }
    } else {
        let region = result.infinite_region().ok_or_else(|| {
            SolidError::entity_missing(OP, "the result B-rep has no infinite region")
        })?;
        let shells = result.region(region)?.get_shells(None, |_, _| Ok(false))?;
        match shells.len() {
            0 => {
                return Err(SolidError::stage_failed(
                    OP,
                    "the slice result has no shell",
                ))
            }
            1 => output.breps.push(result),
            _ => {
                let mut faces = bp::faces(&result, None)?;
                let initial_faces = faces.len();
                let mut marks = Marks::default();
                for _ in 0..initial_faces {
                    if faces.is_empty() {
                        break;
                    }
                    let complete = with_mark_lock(k, ALL_MARKS, |k, mark| {
                        let mut connected = Vec::new();
                        collect_faces(
                            k,
                            &result,
                            &mut marks,
                            faces.first().copied(),
                            mark,
                            &mut connected,
                            None,
                            None,
                        )?;
                        if connected.len() == faces.len() {
                            return Ok(true);
                        }
                        let mut component = empty_brep(
                            k,
                            crate::geom::tol::USE_DEFAULT,
                            crate::geom::tol::USE_DEFAULT,
                            None,
                        )?;
                        let face_ref = |face| {
                            crate::topology::brep_cache::arena_identity(
                                crate::topology::brep_cache::CacheObject::Face(face),
                            )
                        };
                        // Defaults: pOptNewFaces=NULL, bUseMerge=TRUE,
                        // pOptMergeObject=NULL. Returned outputs are not inspected.
                        if let Some(notify) = notifications.as_deref_mut() {
                            // (not upstream: upstream's objects are their own
                            // identities) the result under the caller's names, the
                            // component's objects under their own (`notify_ref`, its
                            // faces `face_ref`), as `copy_component` names them
                            let source_key = notify.key_of;
                            let mut component_env = crate::topology::edge::NotifyEnv {
                                records: &mut *notify.records,
                                attributes: &mut *notify.attributes,
                                context: notify.context.as_deref_mut(),
                                key_of: &crate::topology::loopuse::notify_ref,
                            };
                            bp7::copy_faces_from(
                                k,
                                &mut result,
                                &connected,
                                &mut component,
                                true,
                                source_key,
                                &face_ref,
                                &mut component_env,
                            )?;
                        } else {
                            // the arena entry keeps no notification state: the copy
                            // notifies through an arena-only store (a default record per
                            // object of both B-reps, the objects the copy makes added by
                            // their makers). (not upstream) The store holds the result's
                            // own attributes under `source_ref`, distinct from the
                            // component's identities, so the copy's `SM_NO_COPY`
                            // notifications give the component's objects theirs; the
                            // component keeps them (one attribute per B-rep for a
                            // reference attribute) and the result keeps its own.
                            copy_component(k, &mut result, &connected, &mut component, &face_ref)?;
                        }
                        ignored(crate::error::passed_on(
                            stitch_and_orient(k, &mut component, -0.1, false, None, None),
                            k.fixes.brep_edit_statuses_pass_on,
                        )?)?;
                        output.breps.push(component);
                        result.set_editing_enabled(true);
                        ignored(crate::error::passed_on(
                            remove_faces(k, &mut result, &connected),
                            k.fixes.brep_edit_statuses_pass_on,
                        )?)?;
                        let next = bp::faces(&result, None)?;
                        if next.len() >= faces.len() {
                            return Err(SolidError::invalid_input(
                                OP,
                                "component removal made no progress",
                            ));
                        }
                        faces = next;
                        Ok(false)
                    })??;
                    if complete {
                        output.breps.push(result);
                        return Ok(output);
                    }
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;

    #[derive(Default)]
    struct Trace {
        pairs: Vec<(Option<i64>, Option<i64>)>,
        fail: bool,
        undefined: bool,
        unported: bool,
        observed: Option<(bool, u64, BooleanOperation)>,
    }
    impl BooleanDriver for Trace {
        type Operand = i64;
        fn same_operand(&self, left: &i64, right: &i64) -> bool {
            left == right
        }
        fn construct(
            &mut self,
            _k: &mut Kernel,
            a: &mut Option<i64>,
            b: &mut Option<i64>,
            tolerance: f64,
            angle: f64,
            _options: Option<&MergeOptions>,
        ) -> Result<()> {
            assert_eq!((tolerance, angle), (0.0, 0.0));
            self.pairs.push((*a, *b));
            Ok(())
        }
        fn manifold_boolean(
            &mut self,
            _k: &mut Kernel,
            a: &mut Option<i64>,
            b: &mut Option<i64>,
            manifold: &mut bool,
            lamina: &mut u64,
            operation: BooleanOperation,
            _lock: Option<&mut MarkLock>,
        ) -> Result<Option<i64>> {
            self.observed = Some((*manifold, *lamina, operation));
            if self.fail {
                return Err(SolidError::null_pointer("test driver failure"));
            }
            if self.undefined {
                return Err(SolidError::invalid_input(
                    OP,
                    "test driver undefined behaviour",
                ));
            }
            if self.unported {
                return Err(SolidError::unported("test driver", "test.unit"));
            }
            let a = a.take().unwrap();
            let b = b.take().unwrap();
            Ok(Some(a - b))
        }
    }

    /// Only the calls before and at `IntersectInsertRelate` are reachable: it fails.
    impl PiecewiseAccess for Trace {
        type Surface = i64;
        fn operands(&self) -> (Option<i64>, Option<i64>) {
            (Some(1), Some(2))
        }
        fn set_operands(&mut self, _primary: Option<i64>, _other: Option<i64>) {
            unreachable!("set_operands after a failed IntersectInsertRelate")
        }
        fn surfaces(&self, _brep: i64) -> Result<Vec<i64>> {
            Ok(vec![7])
        }
        fn intersect_insert_relate(&mut self, _k: &mut Kernel) -> Result<()> {
            self.pairs.push((None, None));
            Err(SolidError::sm_err("test::imprint"))
        }
        fn clear_relationships(&mut self) -> Result<()> {
            unreachable!("clear_relationships after a failed IntersectInsertRelate")
        }
        fn set_subset(&mut self, _surfaces: &[i64]) {
            unreachable!("set_subset after a failed IntersectInsertRelate")
        }
        fn surface_map(&mut self, _surface: i64, _only_if_missing: bool) -> Result<()> {
            unreachable!("surface_map after a failed IntersectInsertRelate")
        }
        fn surface_map_users(&self, _surface: i64) -> Result<Option<Vec<i64>>> {
            unreachable!("surface_map_users after a failed IntersectInsertRelate")
        }
        fn new_face_brep(&mut self, _k: &mut Kernel, _source: i64) -> Result<i64> {
            unreachable!("new_face_brep after a failed IntersectInsertRelate")
        }
        fn manifold_flag(&self) -> bool {
            unreachable!("manifold_flag after a failed IntersectInsertRelate")
        }
        fn set_controls(&mut self, _manifold: bool, _wires: bool, _lamina: u64) {
            unreachable!("set_controls after a failed IntersectInsertRelate")
        }
        fn process_wires(&mut self, _k: &mut Kernel, _lock: &MarkLock) -> Result<()> {
            unreachable!("process_wires after a failed IntersectInsertRelate")
        }
        fn release_marks(&mut self, _k: &mut Kernel, _lock: &mut MarkLock) -> Result<()> {
            unreachable!("release_marks after a failed IntersectInsertRelate")
        }
        fn register_empty_ssi(&mut self, _skip: i64, _user: i64) -> Result<()> {
            unreachable!("register_empty_ssi after a failed IntersectInsertRelate")
        }
        fn clear_existing_ssi(&mut self) -> Result<()> {
            unreachable!("clear_existing_ssi after a failed IntersectInsertRelate")
        }
        fn notify_pre_edit(&mut self, _k: &mut Kernel, _primary: i64) -> Result<()> {
            unreachable!("notify_pre_edit after a failed IntersectInsertRelate")
        }
        fn delete_other(&mut self, _k: &mut Kernel) -> Result<()> {
            unreachable!("delete_other after a failed IntersectInsertRelate")
        }
    }

    #[test]
    fn piecewise_merge_ends_on_a_failed_intersect_insert_relate() {
        // af8db37 (#372): a failed imprint leaves both B-reps half edited; the merge
        // went on to relate, classify and merge them
        let mut k = Kernel::default();
        let mut driver = Trace::default();
        let error = piecewise_merge(&mut k, &mut driver, None, false, false).unwrap_err();
        assert!(matches!(error, SolidError::Upstream { .. }), "{error:?}");
        assert_eq!(driver.pairs, [(None, None)]);
    }
    #[test]
    fn planar_boolean_back_edge_reaches_intersector_and_checks_null_output() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::plane::Plane;
        use crate::topology::{Face, ShellId};
        let planar = || {
            let mut brep = Brep::new();
            let mut face = Face::new(ShellId::from_index(0));
            face.uv_domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
            let mut plane = Plane::create_canonical(&Axis2Placement::default()).unwrap();
            plane.adjust_step_uv_domain(&face.uv_domain).unwrap();
            face.surface = Some(Surface::from(plane));
            brep.add_face_record(face).unwrap();
            brep
        };
        for operation in [
            BooleanOperation::Intersection,
            BooleanOperation::Union,
            BooleanOperation::ExclusiveOr,
            BooleanOperation::Difference,
            BooleanOperation::Merge,
        ] {
            let mut kernel = Kernel::default();
            kernel.context.set_doing_boolean(true);
            let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&Brep::new());
            let result = boolean_2d(
                &mut kernel,
                planar(),
                planar(),
                operation,
                Some(&mut state.env(None)),
            );
            // IntersectInsertRelate runs (the edge-less fixture faces relate nothing);
            // copying an unrelated B face then needs its face use, which a bare face
            // record lacks, so Union, ExclusiveOr and Merge stop there
            let copies = matches!(
                operation,
                BooleanOperation::Union | BooleanOperation::ExclusiveOr | BooleanOperation::Merge
            );
            // the Intersection deletes the unrelated faces, which a bare face record cannot
            // be deleted as, and the Difference turns B's over, which needs its face use: those
            // failures are passed on (WR-658)
            let deletes = operation == BooleanOperation::Intersection;
            let turns = operation == BooleanOperation::Difference;
            if copies {
                assert!(
                    matches!(
                        result,
                        Err(SolidError::InvalidInput {
                            operation: "topology::face::p2",
                            ..
                        })
                    ),
                    "{operation:?}: {result:?}"
                );
            } else if deletes {
                assert!(
                    matches!(
                        result,
                        Err(SolidError::InvalidInput {
                            operation: "boolean::topology_intersector::p2::delete_faces",
                            ..
                        })
                    ),
                    "{operation:?}: {result:?}"
                );
            } else if turns {
                assert!(
                    matches!(
                        &result,
                        Err(e) if e.operation() == "topology::brep::p3"
                            && e.sm_status() == Some(crate::error::SM_ERR_NULL_POINTER)
                    ),
                    "{operation:?}: {result:?}"
                );
            } else {
                assert!(result.is_ok(), "{operation:?}: {result:?}");
            }
            assert!(kernel.context.doing_boolean());
        }
        let mut kernel = Kernel::default();
        let result = boolean_2d(
            &mut kernel,
            Brep::new(),
            planar(),
            BooleanOperation::Difference,
            None,
        );
        let error = result.unwrap_err();
        assert_eq!(error.operation(), OP);
        assert_eq!(error.sm_status(), Some(crate::error::SM_ERR_NULL_POINTER));
        assert_eq!(error.kind(), crate::error::ErrorKind::Missing);
        assert_eq!(
            error.to_string(),
            "boolean::merge::p3: the planar Boolean made no result B-rep"
        );
        assert!(!kernel.context.doing_boolean());
    }

    /// The bounded unit-square plane `z = height` (`SmPlane(origin, X, Y, scale, domain)`).
    fn square_plane(height: f64) -> Surface {
        Surface::from(crate::surface::plane::Plane::new(
            crate::math::Vec3::new(0.0, 0.0, height),
            crate::math::Vec3::new(1.0, 0.0, 0.0),
            crate::math::Vec3::new(0.0, 1.0, 0.0),
            crate::math::Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    /// The bounded unit-square plane `x0 <= x <= x0 + 1`, `0 <= y <= 1`, `z = 0`.
    fn square_plane_from(x0: f64) -> Surface {
        Surface::from(crate::surface::plane::Plane::new(
            crate::math::Vec3::new(x0, 0.0, 0.0),
            crate::math::Vec3::new(1.0, 0.0, 0.0),
            crate::math::Vec3::new(0.0, 1.0, 0.0),
            crate::math::Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    /// The whole unit sphere about the origin.
    fn unit_sphere(k: &Kernel) -> Surface {
        Surface::from(
            crate::surface::sphere::Sphere::new(
                k,
                crate::math::Vec3::ZERO,
                crate::math::Vec3::new(1.0, 0.0, 0.0),
                crate::math::Vec3::new(0.0, 1.0, 0.0),
                Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
                1.0,
                crate::surface::sphere::SphereOptions {
                    swap_uv: false,
                    inside_out: false,
                    circle_nurb: None,
                },
            )
            .unwrap(),
        )
    }

    /// `BooleanLists` copies its surface operands with the virtual `Copy` (WR-196): each
    /// surface makes a face over its natural domain, a list of planes is recognised as
    /// planar and takes the planar 2D path, whose outputs are copies of the derived
    /// surfaces; a list with B-reps never reads its surfaces.
    #[test]
    fn surface_lists_copy_each_surface_whole() {
        let mut k = Kernel::default();
        let (brep, planar) =
            list_operand(&mut k, &[], &[square_plane(0.0), square_plane(2.0)]).unwrap();
        assert!(planar);
        assert_eq!(bp::faces(&brep, None).unwrap().len(), 2);
        // the last surface's kind decides (upstream: "we hope they are all the same")
        let sphere = unit_sphere(&k);
        let (brep, planar) = list_operand(&mut k, &[], &[square_plane(0.0), sphere]).unwrap();
        assert!(!planar);
        assert_eq!(bp::faces(&brep, None).unwrap().len(), 2);
        // two unit squares overlapping in [0.5, 1] x [0, 1]: each output plane spans one
        // region of the result, [x0, x1] x [0, 1] at z = 0, from its natural domain
        let regions: [(i32, &[(f64, f64)]); 5] = [
            (0, &[(0.5, 1.0)]),
            (1, &[(0.0, 1.5)]),
            (2, &[(0.0, 0.5), (1.0, 1.5)]),
            (3, &[(0.0, 0.5)]),
            (4, &[(1.0, 1.5)]),
        ];
        for (operation, expected) in regions {
            let result = boolean_lists(
                &mut k,
                &[],
                &[square_plane_from(0.0)],
                &[],
                &[square_plane_from(0.5)],
                operation,
                None,
            )
            .unwrap();
            assert!(result.breps.is_empty(), "operation {operation}");
            let mut spans = Vec::new();
            for surface in &result.surfaces {
                assert!(
                    surface.is_kind_of(PLANE_TYPE).unwrap(),
                    "operation {operation}"
                );
                let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
                let low = surface.evaluate_point(&k, domain.min()).unwrap();
                let high = surface.evaluate_point(&k, domain.max()).unwrap();
                assert_eq!([low.y, low.z, high.y, high.z], [0.0, 0.0, 1.0, 0.0]);
                spans.push((low.x, high.x));
            }
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(spans, expected, "operation {operation}");
        }
        // a list ending in a plane is planar whatever comes before it, so the planar path
        // refuses a sphere in it
        let sphere = unit_sphere(&k);
        let result = boolean_lists(
            &mut k,
            &[],
            &[sphere, square_plane_from(0.0)],
            &[],
            &[square_plane_from(0.5)],
            1,
            None,
        );
        assert!(
            matches!(result, Err(SolidError::InvalidInput { .. })),
            "{result:?}"
        );
        // an operation BooleanLists does not know is refused once both lists are built
        let result = boolean_lists(
            &mut k,
            &[],
            &[square_plane(0.0)],
            &[],
            &[square_plane(1.0)],
            -1,
            None,
        );
        assert!(
            matches!(result, Err(SolidError::InvalidInput { operation: OP, .. })),
            "{result:?}"
        );
        // a list with a B-rep ignores its surfaces
        let (brep, planar) = list_operand(&mut k, &[Brep::new()], &[square_plane(0.0)]).unwrap();
        assert!(!planar);
        assert!(bp::faces(&brep, None).unwrap().is_empty());
    }

    #[test]
    fn fold_order_is_last_to_first_and_validation_preserves_input() {
        let mut k = Kernel::default();
        let mut driver = Trace::default();
        let mut input = vec![1, 2, 3];
        assert!(merge_breps(&mut k, &mut input, 4, None, &mut driver).is_err());
        assert_eq!(input, [1, 2, 3]);
        assert!(driver.pairs.is_empty());
        let mut duplicates = vec![1, 2, 1];
        assert!(merge_breps(&mut k, &mut duplicates, 0, None, &mut driver).is_err());
        assert_eq!(duplicates, [1, 2, 1]);
        assert!(driver.pairs.is_empty());
        assert_eq!(
            merge_breps(&mut k, &mut input, 2, None, &mut driver).unwrap(),
            0
        );
        assert!(input.is_empty());
        assert_eq!(driver.pairs, [(Some(3), Some(2)), (Some(1), Some(1))]);
    }
    #[test]
    fn forest_reorders_leaves_appends_unused_and_keeps_null_slots() {
        let mut k = Kernel::default();
        let mut driver = Trace::default();
        let mut input = vec![Some(11), Some(22), Some(33), Some(44)];
        boolean_trees(&mut k, &mut input, &[2, 0, 2], None, &mut driver).unwrap();
        assert_eq!(input, [Some(33), Some(11), None, Some(22), Some(44)]);
        assert!(driver.pairs.is_empty());
    }
    #[test]
    fn tree_errors_keep_consumed_input_slots_and_map_only_ordinary_failures() {
        let mut k = Kernel::default();
        let mut driver = Trace {
            fail: true,
            ..Trace::default()
        };
        let mut input = vec![Some(1), Some(2), Some(3)];
        let error = boolean_trees(&mut k, &mut input, &[0, 1, -1], None, &mut driver).unwrap_err();
        assert_eq!(error.sm_status(), Some(crate::error::SM_ERR));
        assert_eq!(error.kind(), crate::error::ErrorKind::Failed);
        // the failed Boolean is kept as the cause of the generic failure
        assert!(error.cause().is_some(), "{error:?}");
        assert_eq!(input, [None, None, Some(3)]);
        // an Unported failure passes through unmapped
        let mut driver = Trace {
            unported: true,
            ..Trace::default()
        };
        let mut input = vec![Some(1), Some(2)];
        let error = boolean_trees(&mut k, &mut input, &[0, 1, -1], None, &mut driver).unwrap_err();
        assert!(error.is_unported());
        // so does the port's report of upstream undefined behaviour (InvalidInput)
        let mut driver = Trace {
            undefined: true,
            ..Trace::default()
        };
        let mut input = vec![Some(1), Some(2)];
        let error = boolean_trees(&mut k, &mut input, &[0, 1, -1], None, &mut driver).unwrap_err();
        assert!(matches!(error, SolidError::InvalidInput { .. }));
        // the real engine's ordinary failure (empty operands) is mapped too
        let mut driver = DependencyMerge::default();
        let mut input = vec![Some(Brep::new()), Some(Brep::new())];
        let error = boolean_trees(&mut k, &mut input, &[0, 1, -1], None, &mut driver).unwrap_err();
        assert_eq!(error.sm_status(), Some(crate::error::SM_ERR));
        assert_eq!(error.kind(), crate::error::ErrorKind::Failed);
        // the failed Boolean is kept as the cause of the generic failure
        assert!(error.cause().is_some(), "{error:?}");
        assert_eq!(input.len(), 2);
        assert!(input.iter().all(Option::is_none));
    }
    #[test]
    fn dependency_merge_runs_manifold_boolean_on_a_constructed_engine() {
        let mut k = Kernel::default();
        let call = |driver: &mut DependencyMerge, k: &mut Kernel, other: Option<Brep>| {
            let mut primary = Some(Brep::new());
            let mut other = other;
            let result = non_manifold_boolean(
                k,
                driver,
                &mut primary,
                &mut other,
                &mut true,
                &mut 0,
                BooleanOperation::Union,
            );
            (result, primary.is_some(), other.is_some())
        };
        let (result, ..) = call(&mut DependencyMerge::default(), &mut k, Some(Brep::new()));
        assert!(matches!(result, Err(SolidError::InvalidInput { .. })));
        assert!(DependencyMerge::default().into_engine().is_none());
        // Retained state the owner moved in, and its options, survive the calls.
        let mut intersector = KernelManifoldIntersector::new(0).unwrap();
        let options = MergeOptions::new(true, false, false, true, true);
        intersector.merge.set_merge_options(Some(&options));
        let mut state = ManifoldState::default();
        state.other_components.push(Brep::new());
        state.intersection_loop_closed = false;
        let mut driver = DependencyMerge::with_engine(MergeEngine {
            intersector,
            state,
            attributes: AttributeStore::default(),
        });
        // merge_p4 checks both operands (NER) before touching either.
        let (result, ..) = call(&mut driver, &mut k, None);
        let error = result.unwrap_err();
        assert_eq!(error.sm_status(), Some(crate::error::SM_ERR_NULL_POINTER));
        assert_eq!(error.kind(), crate::error::ErrorKind::Missing);
        // The union of two empty operands completes: the primary becomes the result.
        let (result, primary, other) = call(&mut driver, &mut k, Some(Brep::new()));
        assert!(matches!(result, Ok(Some(ref b)) if b.faces().is_empty()));
        assert!(!primary && other);
        let engine = driver.into_engine().unwrap();
        assert_eq!(engine.intersector.intersect_lamina_edges(), 1);
        assert_eq!(engine.intersector.merge.options, options);
        assert_eq!(engine.state.other_components.len(), 1);
        // the run starts its intersection loop state afresh
        assert!(engine.state.intersection_loop_closed);
    }
    #[test]
    fn construct_builds_the_engine_manifold_boolean_runs_on() {
        let mut k = Kernel::default();
        let options = MergeOptions::new(true, false, false, true, true);
        let mut driver = DependencyMerge::default();
        let mut primary = Some(Brep::new());
        let mut other = Some(Brep::new());
        driver
            .construct(&mut k, &mut primary, &mut other, 1e-3, 0.0, Some(&options))
            .unwrap();
        let engine = driver.engine().unwrap();
        assert_eq!(engine.intersector.merge.options, options);
        assert_eq!(engine.intersector.merge.manifold_boolean, Some(true));
        assert_eq!(
            engine.intersector.merge.intersection_loop_closed,
            Some(true)
        );
        assert!(engine.state.intersection_loop_closed);
        // The constructed engine runs merge_p4's ManifoldBoolean to completion on the
        // empty operands.
        let result = non_manifold_boolean(
            &mut k,
            &mut driver,
            &mut primary,
            &mut other,
            &mut true,
            &mut 0,
            BooleanOperation::Union,
        );
        assert!(result.is_ok(), "{result:?}");
        let mut none = None;
        let error = DependencyMerge::default()
            .construct(&mut k, &mut none, &mut other, 1e-3, 0.0, None)
            .unwrap_err();
        assert!(matches!(error, SolidError::InvalidInput { .. }));
    }
    #[test]
    fn nonmanifold_restores_flag_on_failure_but_retains_lamina_mode() {
        let mut k = Kernel::default();
        let mut driver = Trace {
            fail: true,
            ..Trace::default()
        };
        let mut manifold = true;
        let mut lamina = 3;
        let error = non_manifold_boolean(
            &mut k,
            &mut driver,
            &mut Some(1),
            &mut Some(2),
            &mut manifold,
            &mut lamina,
            BooleanOperation::Difference,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            }
        ));
        assert_eq!(
            driver.observed,
            Some((false, 1, BooleanOperation::Difference))
        );
        assert!(manifold);
        assert_eq!(lamina, 1);
    }
    #[test]
    fn unknown_operator_constructs_before_rejection_and_underflow_does_not() {
        let mut k = Kernel::default();
        let mut driver = Trace::default();
        let mut input = vec![Some(1), Some(2)];
        assert!(boolean_trees(&mut k, &mut input, &[0, 1, -7], None, &mut driver).is_err());
        assert_eq!(driver.pairs, [(Some(1), Some(2))]);
        let mut input = vec![Some(1)];
        driver.pairs.clear();
        assert!(boolean_trees(&mut k, &mut input, &[0, -1, 100], None, &mut driver).is_err());
        assert!(driver.pairs.is_empty());
    }

    /// `combine_brep` (`MergeBrep` into the list's copy of its first B-rep) seeds the
    /// source's own attributes itself, so `topology::brep::p3::merge_brep` lends nothing
    /// more: each face and edge of a two-box list ends with exactly its own one list.
    #[test]
    fn list_operand_carries_each_breps_attributes_once() {
        use crate::geom::attribute::{Attribute, Behavior};
        let mut k = Kernel::default();
        let mut breps = Vec::new();
        for (value, x) in [(0_i64, 0.0), (1, 20.0)] {
            let mut b = crate::primitive::api_primitives::create_box(
                &mut k,
                crate::math::Point3::new(x, 0.0, 0.0),
                10.0,
                10.0,
                10.0,
            )
            .unwrap();
            let entities: Vec<EntityRef> = bp::faces(&b, None)
                .unwrap()
                .into_iter()
                .map(EntityRef::Face)
                .chain(
                    bp::edges(&b, None)
                        .unwrap()
                        .into_iter()
                        .map(EntityRef::Edge),
                )
                .collect();
            for e in entities {
                b.add_attribute(e, Attribute::long(7, value, Behavior::Copy))
                    .unwrap();
            }
            breps.push(b);
        }
        let (result, planar) = list_operand(&mut k, &breps, &[]).unwrap();
        assert!(!planar);
        let faces = bp::faces(&result, None).unwrap();
        let edges = bp::edges(&result, None).unwrap();
        assert_eq!((faces.len(), edges.len()), (12, 24));
        let mut values = Vec::new();
        for e in faces
            .into_iter()
            .map(EntityRef::Face)
            .chain(edges.into_iter().map(EntityRef::Edge))
        {
            let list = result.entity_attributes(e);
            assert_eq!(list.len(), 1, "{e:?}");
            values.push(list[0].1.long_value().unwrap());
        }
        values.sort_unstable();
        assert_eq!(values, [vec![0; 18], vec![1; 18]].concat());
        // the operands keep their own, once each
        for b in &breps {
            for f in bp::faces(b, None).unwrap() {
                assert_eq!(b.entity_attributes(EntityRef::Face(f)).len(), 1);
            }
        }
    }
}
