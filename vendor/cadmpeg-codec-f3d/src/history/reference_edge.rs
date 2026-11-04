// SPDX-License-Identifier: Apache-2.0
//! Resolve the common boundary named by a recipe's primary side.

use crate::records::topology::edge_recipe::DesignEdgeRecipeStructure;
use std::collections::BTreeSet;

pub(super) fn primary_reference_common_edge(
    structure: &DesignEdgeRecipeStructure,
    mut boundary: impl FnMut(usize) -> Option<(i64, Vec<i64>)>,
) -> Option<i64> {
    if structure.root != 2 || structure.sides.len() != 2 {
        return None;
    }
    let side = structure.sides.first()?;
    let mut ordinals = BTreeSet::new();
    for value in std::iter::once(side.header_value).chain(side.scalars.iter().copied()) {
        if value != 0 {
            ordinals.insert(usize::try_from(value).ok()?.checked_sub(1)?);
        }
    }
    if ordinals.len() < 2 {
        return None;
    }
    let mut faces = BTreeSet::new();
    let mut common: Option<BTreeSet<i64>> = None;
    for ordinal in ordinals {
        let (face, edges) = boundary(ordinal)?;
        faces.insert(face);
        let edges = edges.into_iter().collect::<BTreeSet<_>>();
        match &mut common {
            Some(common) => common.retain(|edge| edges.contains(edge)),
            None => common = Some(edges),
        }
    }
    if faces.len() < 2 {
        return None;
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::primary_reference_common_edge;
    use crate::records::topology::edge_recipe::DesignEdgeRecipeStructure;

    const PROGRAM: &[i32] = &[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 0, 0, -1, 3, 0, -1, 0, -1, 0, -1,
        0, 0, -1,
    ];

    fn structure() -> DesignEdgeRecipeStructure {
        crate::design::decode::operands::edge_recipe_structure(
            &cadmpeg_test_support::service_decode_context(),
            PROGRAM,
        )
        .unwrap()
        .expect("complete two-side recipe")
    }

    #[test]
    fn primary_reference_program_refuses_each_truncation() {
        for end in 0..PROGRAM.len() {
            assert!(crate::design::decode::operands::edge_recipe_structure(
                &cadmpeg_test_support::service_decode_context(),
                &PROGRAM[..end],
            )
            .unwrap()
            .is_none());
        }
    }

    #[test]
    fn primary_references_resolve_one_common_boundary() {
        assert_eq!(
            primary_reference_common_edge(&structure(), |ordinal| {
                [(10, vec![30, 31]), (20, vec![31, 32])]
                    .get(ordinal)
                    .cloned()
            }),
            Some(31)
        );
    }

    #[test]
    fn primary_references_refuse_multiple_common_edges() {
        assert_eq!(
            primary_reference_common_edge(&structure(), |ordinal| {
                [(10, vec![30, 31]), (20, vec![30, 31])]
                    .get(ordinal)
                    .cloned()
            }),
            None
        );
    }

    #[test]
    fn primary_references_refuse_missing_or_disjoint_boundaries() {
        assert_eq!(primary_reference_common_edge(&structure(), |_| None), None);
        assert_eq!(
            primary_reference_common_edge(&structure(), |ordinal| {
                [(10, vec![30]), (20, vec![31])].get(ordinal).cloned()
            }),
            None
        );
        assert_eq!(
            primary_reference_common_edge(&structure(), |ordinal| {
                [(10, vec![30]), (20, Vec::new())].get(ordinal).cloned()
            }),
            None
        );
    }

    #[test]
    fn primary_references_require_distinct_faces_and_ordinals() {
        assert_eq!(
            primary_reference_common_edge(&structure(), |_| Some((10, vec![31]))),
            None
        );
        let mut repeated = structure();
        repeated.sides[0].scalars = vec![0, 1];
        assert_eq!(
            primary_reference_common_edge(&repeated, |_| Some((10, vec![31]))),
            None
        );
    }

    #[test]
    fn primary_references_refuse_truncated_or_invalid_ordinals() {
        let mut truncated = structure();
        truncated.sides.pop();
        assert_eq!(
            primary_reference_common_edge(&truncated, |_| Some((10, vec![31]))),
            None
        );
        let mut invalid = structure();
        invalid.sides[0].header_value = -1;
        assert_eq!(
            primary_reference_common_edge(&invalid, |_| Some((10, vec![31]))),
            None
        );
        let mut missing = structure();
        missing.sides[0].scalars = vec![0, 3];
        assert_eq!(
            primary_reference_common_edge(&missing, |ordinal| {
                [(10, vec![31]), (20, vec![31])].get(ordinal).cloned()
            }),
            None
        );
    }
}
