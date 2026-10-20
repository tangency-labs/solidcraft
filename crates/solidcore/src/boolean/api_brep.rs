//! High-level boolean entry points (`SM_API`): two-B-rep booleans (union, difference,
//! intersection, merge, with or without merge options), N-ary booleans over a list of
//! B-reps, booleans between lists of B-reps or surfaces, and CSG trees in postfix
//! notation.
//!
//! Upstream: `source/SM_API/src/SmApiBrep.cpp`, `source/SM_API/inc/SmApiBrep.h`.
//!
//! | Rust                                          | Upstream                                     |
//! |-----------------------------------------------|----------------------------------------------|
//! | `merge_breps_inputs_are_valid` (private)      | `SmApiMergeBrepsInputsAreValid` (static)     |
//! | `merge_breps_kernel_operation` (private)      | `SmApiMergeBrepsKernelOperation` (static)    |
//! | `validate_boolean_operands` (private)         | `SmApiValidateBooleanOperands` (static)      |
//! | `merge_boolean` (private)                     | the shared body of the two-B-rep entry points |
//! | [`boolean`], [`BooleanOutcome`]               | `SmApiBoolean`                               |
//! | [`boolean_union`]                             | `SmApiBooleanUnion`                          |
//! | [`boolean_difference`]                        | `SmApiBooleanDifference`                     |
//! | [`boolean_intersection`]                      | `SmApiBooleanIntersection`                   |
//! | [`boolean_merge`]                             | `SmApiBooleanMerge`                          |
//! | [`merge_breps`]                               | `SmApiMergeBreps`                            |
//! | [`boolean_with_options`]                      | `SmApiBooleanWithOptions`                    |
//! | [`boolean_lists`], [`BooleanListsResult`]     | `SmApiBooleanLists`                          |
//! | [`boolean_trees`]                             | `SmApiBooleanTrees`                          |
//!
//! Deviations:
//! - **Ownership.** Upstream passes `SmBrep*` operands that the kernel modifies in
//!   place (the first becomes `rpResult`) or deletes (the second). The port takes the
//!   operands by value and returns the result; a second operand upstream keeps
//!   (`SM_BO_PARTIAL_MERGE`, `SM_BO_IMPRINT_CLASSIFY`, `bKeepOtherBrep`) is handed
//!   back in [`BooleanOutcome::other`]. A `NULL` operand, the same B-rep passed twice
//!   (`SmApiValidateBooleanOperands`) and a `NULL` or duplicate list entry
//!   (`SmApiMergeBrepsInputsAreValid`) cannot be expressed with owned values, so those
//!   checks reduce to the remaining ones. Operands rejected by a two-B-rep entry point
//!   are dropped (upstream leaves them with the caller); [`merge_breps`] takes the list
//!   `&mut` and leaves it untouched on rejection, as upstream.
//! - **Out-params.** `rpResult`, `rResultBreps` / `rResultSurfaces` are return values;
//!   on failure there is none (upstream clears `rpResult`).
//! - **Context.** `SmApiGetOrCreateContext()` is the caller's [`Kernel`], taken by
//!   every entry point (the merge engine reads its tolerances).
//! - **Statuses.** `SM_ERR_INVALID_INPUT` is [`SolidError::InvalidInput`]; `SM_ERR`
//!   is [`SolidError::Upstream`] with [`crate::error::SM_ERR`].
//! - **Dependencies.** List folding and tree evaluation call `merge::p3`; its
//!   construction and topology calls retain narrowly scoped dependency errors.
//!   Two-B-rep entry points still await the `merge_p2` engine constructor.
//! - **List context.** `boolean_lists` takes a mutable kernel for the connected
//!   component traversal's mark locks.
//! - **Nullable tree output.** Tree operands are `Option<Brep>` slots, preserving
//!   consumed entries on failure and defined null outputs of repeated leaves.
//! - The debug drawing (`SM_DEBUG_CODE`) and `SM_ASSERT_VALID` checks are not ported.

use crate::boolean::merge::MergeOptions;
use crate::error::{Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::Brep;

const OP_BOOLEAN: &str = "boolean::api_brep::boolean";
const OP_MERGE_BREPS: &str = "boolean::api_brep::merge_breps";

/// The outcome of a two-B-rep boolean ([`boolean`], [`boolean_with_options`]).
#[derive(Debug, Clone)]
pub struct BooleanOutcome {
    /// `rpResult`: the first operand, modified in place by the merge.
    pub result: Brep,
    /// The second operand when the operation keeps it (`SM_BO_PARTIAL_MERGE`,
    /// `SM_BO_IMPRINT_CLASSIFY`, or [`MergeOptions::keep_other_brep`]); `None` when
    /// upstream deletes it.
    pub other: Option<Brep>,
}

/// The outputs of [`boolean_lists`].
pub use crate::boolean::merge::p3::BooleanListsResult;

/// Validates a public N-ary boolean input list without consuming it
/// (`SmApiMergeBrepsInputsAreValid`): at least two B-reps. Upstream also rejects a
/// `NULL` or repeated entry, which owned values cannot be.
fn merge_breps_inputs_are_valid(breps: &[Brep]) -> bool {
    breps.len() >= 2
}

/// Maps the public boolean operation to `SmMerge::merge_breps`' legacy zero-based
/// operation selector (`SmApiMergeBrepsKernelOperation`): union 0, intersection 1,
/// difference 2, merge 3; `None` for every other operation.
fn merge_breps_kernel_operation(operation: BooleanOperation) -> Option<u32> {
    match operation {
        BooleanOperation::Union => Some(0),
        BooleanOperation::Intersection => Some(1),
        BooleanOperation::Difference => Some(2),
        BooleanOperation::Merge => Some(3),
        _ => None,
    }
}

/// Validates the operands of a two-B-rep boolean before the kernel modifies or
/// deletes either of them (`SmApiValidateBooleanOperands`). `SM_BO_EXTRACT_SEPARATE`
/// has no single result (upstream's kernel deletes both inputs and leaves the pieces
/// in `SmMerge` members that nothing frees). Upstream also rejects a `NULL` operand
/// and the same B-rep twice, which owned values cannot be.
fn validate_boolean_operands(operation: BooleanOperation) -> Result<()> {
    if operation == BooleanOperation::ExtractSeparate {
        return Err(SolidError::invalid_input(
            OP_BOOLEAN,
            "SM_BO_EXTRACT_SEPARATE has no single result",
        ));
    }
    Ok(())
}

/// The body shared by the two-B-rep entry points after validation: build the merge
/// (`SmMerge sMerge(*pContext, pBrep1, pBrep2)`, or with `0, 0, &sOptions` when
/// `options` is given), then run `ManifoldBoolean` when both operands are manifold
/// solids (`SmBrep::IsManifoldSolid`) and `NonManifoldBoolean` otherwise.
fn merge_boolean(
    k: &Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
    options: Option<MergeOptions>,
) -> Result<BooleanOutcome> {
    // The merge engine takes the context, both operands, the operation and the
    // options; none of it is reachable until `SmMerge` is ported.
    let _ = (k, brep1, brep2, operation, options);
    // UNPORTED(boolean.merge_p2): SmMerge::SmMerge
    Err(SolidError::unported("SmMerge::SmMerge", "boolean.merge_p2"))
}

/// Booleans two B-reps (`SmApiBoolean`): `brep1` is modified into the result; `brep2`
/// is consumed (returned in [`BooleanOutcome::other`] for the operations upstream keeps
/// it for).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `SM_BO_EXTRACT_SEPARATE`; otherwise the merge's
/// error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean(
    k: &Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
) -> Result<BooleanOutcome> {
    validate_boolean_operands(operation)?;
    merge_boolean(k, brep1, brep2, operation, None)
}

/// Unites two B-reps (`SmApiBooleanUnion`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_union(k: &Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Union)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Union, None).map(|o| o.result)
}

/// Subtracts `brep2` from `brep1` (`SmApiBooleanDifference`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_difference(k: &Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Difference)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Difference, None).map(|o| o.result)
}

/// Intersects two B-reps (`SmApiBooleanIntersection`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_intersection(k: &Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Intersection)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Intersection, None).map(|o| o.result)
}

/// Merges `brep2` into `brep1` (`SmApiBooleanMerge`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_merge(k: &Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Merge)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Merge, None).map(|o| o.result)
}

/// Applies one boolean operation across a list of at least two B-reps
/// (`SmApiMergeBreps`). The list is validated first and left untouched on rejection;
/// once validation passes the B-reps are consumed (the list is drained), as upstream.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for fewer than two B-reps or an operation other than
/// union, intersection, difference or merge; otherwise the merge's error
/// (including [`SolidError::Unported`] for missing merge-engine dependencies).
#[allow(clippy::ptr_arg)] // the list is drained on success, as upstream consumes it
pub fn merge_breps(
    k: &mut Kernel,
    breps: &mut Vec<Brep>,
    operation: BooleanOperation,
) -> Result<Brep> {
    // The output is valid only on success. Reject every unsafe list shape before the
    // kernel has a chance to consume it.
    let kernel_operation = match merge_breps_kernel_operation(operation) {
        Some(op) if merge_breps_inputs_are_valid(breps) => op,
        _ => {
            return Err(SolidError::invalid_input(
                OP_MERGE_BREPS,
                "fewer than two B-reps or an operation other than union, intersection, difference or merge",
            ))
        }
    };
    // `SmMerge::merge_breps(rBreps, lKernelOperation, rpResult)`, then `SM_ERR` when it
    // succeeds without a result.
    crate::boolean::merge::p3::merge_breps(
        k,
        breps,
        kernel_operation,
        None,
        &mut crate::boolean::merge::p3::DependencyMerge::default(),
    )
}

/// Booleans two B-reps with full merge-option control (`SmApiBooleanWithOptions`):
/// cookie cutter, imprinting, post-processing, imprint-and-classify and keeping the
/// second operand.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `SM_BO_EXTRACT_SEPARATE`; otherwise the merge's
/// error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_with_options(
    k: &Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
    options: MergeOptions,
) -> Result<BooleanOutcome> {
    validate_boolean_operands(operation)?;
    merge_boolean(k, brep1, brep2, operation, Some(options))
}

/// Booleans two lists of B-reps or surfaces (`SmApiBooleanLists`, wrapping
/// `SmMerge::BooleanLists`). `operation` is `0` intersect, `1` union, `2` exclusive
/// or, `3` A minus B, `4` B minus A. The inputs are not modified.
///
/// # Errors
///
/// The first dependency error reached by `merge::p3::boolean_lists`.
pub fn boolean_lists(
    k: &mut Kernel,
    breps1: &[Brep],
    surfaces1: &[Surface],
    breps2: &[Brep],
    surfaces2: &[Surface],
    operation: i32,
) -> Result<BooleanListsResult> {
    crate::boolean::merge::p3::boolean_lists(
        k, breps1, surfaces1, breps2, surfaces2, operation, None,
    )
}

/// Combines B-reps by CSG trees in postfix notation (`SmApiBooleanTrees`, wrapping
/// `SmMerge::BooleanTrees`): a node `>= 0` is an index into `breps`, `-1` to `-6` are
/// union, intersection, difference, merge, exclusive or and slice. On success
/// `breps` holds one result per tree followed by the unused B-reps.
///
/// # Errors
///
/// Dependency errors, or `InvalidInput` for an out-of-range executed leaf or
/// a binary operation that would dereference a null operand upstream.
pub fn boolean_trees(
    k: &mut Kernel,
    breps: &mut Vec<Option<Brep>>,
    post_fix_trees: &[i64],
) -> Result<()> {
    crate::boolean::merge::p3::boolean_trees(
        k,
        breps,
        post_fix_trees,
        None,
        &mut crate::boolean::merge::p3::DependencyMerge::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [BooleanOperation; 11] = [
        BooleanOperation::Unknown,
        BooleanOperation::Union,
        BooleanOperation::Intersection,
        BooleanOperation::Difference,
        BooleanOperation::ExclusiveOr,
        BooleanOperation::Merge,
        BooleanOperation::PartialMerge,
        BooleanOperation::Imprint,
        BooleanOperation::ImprintClassify,
        BooleanOperation::ExtractSeparate,
        BooleanOperation::Slice,
    ];

    fn unported_unit(e: &SolidError) -> &'static str {
        match e {
            SolidError::Unported { unit, .. } => unit,
            _ => "",
        }
    }

    #[test]
    fn kernel_operation_mapping() {
        for op in ALL {
            let expect = match op {
                BooleanOperation::Union => Some(0),
                BooleanOperation::Intersection => Some(1),
                BooleanOperation::Difference => Some(2),
                BooleanOperation::Merge => Some(3),
                _ => None,
            };
            assert_eq!(merge_breps_kernel_operation(op), expect, "{op:?}");
        }
    }

    #[test]
    fn extract_separate_is_rejected() {
        let k = Kernel::default();
        let e = boolean(
            &k,
            Brep::new(),
            Brep::new(),
            BooleanOperation::ExtractSeparate,
        )
        .unwrap_err();
        assert!(matches!(e, SolidError::InvalidInput { .. }));
        let e = boolean_with_options(
            &k,
            Brep::new(),
            Brep::new(),
            BooleanOperation::ExtractSeparate,
            MergeOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(e, SolidError::InvalidInput { .. }));
    }

    #[test]
    fn valid_operands_reach_the_merge() {
        let k = Kernel::default();
        for op in ALL {
            if op == BooleanOperation::ExtractSeparate {
                continue;
            }
            let e = boolean(&k, Brep::new(), Brep::new(), op).unwrap_err();
            assert_eq!(unported_unit(&e), "boolean.merge_p2", "{op:?}");
        }
        for e in [
            boolean_union(&k, Brep::new(), Brep::new()).unwrap_err(),
            boolean_difference(&k, Brep::new(), Brep::new()).unwrap_err(),
            boolean_intersection(&k, Brep::new(), Brep::new()).unwrap_err(),
            boolean_merge(&k, Brep::new(), Brep::new()).unwrap_err(),
        ] {
            assert_eq!(unported_unit(&e), "boolean.merge_p2");
        }
    }

    #[test]
    fn merge_breps_rejects_without_consuming() {
        let mut k = Kernel::default();
        for n in 0..2 {
            let mut breps = vec![Brep::new(); n];
            let e = merge_breps(&mut k, &mut breps, BooleanOperation::Union).unwrap_err();
            assert!(matches!(e, SolidError::InvalidInput { .. }));
            assert_eq!(breps.len(), n);
        }
        for op in ALL {
            let mut breps = vec![Brep::new(); 3];
            let e = merge_breps(&mut k, &mut breps, op).unwrap_err();
            if merge_breps_kernel_operation(op).is_some() {
                assert_eq!(unported_unit(&e), "boolean.merge_p2", "{op:?}");
            } else {
                assert!(matches!(e, SolidError::InvalidInput { .. }), "{op:?}");
                assert_eq!(breps.len(), 3);
            }
        }
    }

    #[test]
    fn lists_and_trees_reach_the_merge() {
        let mut k = Kernel::default();
        let e = boolean_lists(&mut k, &[Brep::new()], &[], &[Brep::new()], &[], 1).unwrap_err();
        assert_eq!(unported_unit(&e), "topology.brep_p3");
        let mut breps = vec![Some(Brep::new()), Some(Brep::new())];
        let tree = vec![0, 1, -1];
        let e = boolean_trees(&mut k, &mut breps, &tree).unwrap_err();
        assert_eq!(unported_unit(&e), "boolean.merge_p2");
    }
    #[test]
    fn tree_api_preserves_defined_null_outputs_and_failure_slots() {
        let mut k = Kernel::default();
        let mut breps = vec![Some(Brep::new()), Some(Brep::new())];
        boolean_trees(&mut k, &mut breps, &[0, 0]).unwrap();
        assert_eq!(breps.len(), 3);
        assert!(breps.first().unwrap().is_some());
        assert!(breps.get(1).unwrap().is_none());
        assert!(breps.get(2).unwrap().is_some());
        let mut breps = vec![Some(Brep::new()), Some(Brep::new())];
        assert!(boolean_trees(&mut k, &mut breps, &[0, -1]).is_err());
        assert!(breps.first().unwrap().is_none());
        assert!(breps.get(1).unwrap().is_some());
    }
}
