// SPDX-License-Identifier: Apache-2.0
//! Read relation mask semantics without allocating kind collections.

use crate::records::sketch_relations::{
    constraint_kinds_iter, SketchConstraintKind, SketchRelation,
};
use std::sync::OnceLock;

pub(super) fn unknown_constraint_bits(state: u64) -> u64 {
    static MASK: OnceLock<u64> = OnceLock::new();
    // Derive the fixed vocabulary mask from its owning iterator.
    let mask = MASK.get_or_init(|| {
        (0..u64::BITS).fold(0, |mask, bit| {
            let flag = 1u64 << bit;
            if constraint_kinds_iter(flag).next().is_some() {
                mask | flag
            } else {
                mask
            }
        })
    });
    state & !mask
}

pub(super) fn sole_constraint_kind(relation: &SketchRelation) -> Option<SketchConstraintKind> {
    let state = relation.definition.state();
    if unknown_constraint_bits(state) != 0 {
        return None;
    }
    let mut kinds = constraint_kinds_iter(state);
    let first = kinds.next()?;
    kinds.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::unknown_constraint_bits;
    use crate::records::sketch_relations::constraint_kinds_from_state;

    #[test]
    fn borrowed_relation_mask_matches_every_bit_and_unknown_union() {
        for state in std::iter::once(0)
            .chain((0..u64::BITS).map(|bit| 1u64 << bit))
            .chain([u64::MAX, 0x11, 0x20_0000_0020])
        {
            assert_eq!(
                unknown_constraint_bits(state),
                constraint_kinds_from_state(state).1
            );
        }
    }
}
