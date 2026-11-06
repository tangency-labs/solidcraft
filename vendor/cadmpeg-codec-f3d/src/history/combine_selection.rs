// SPDX-License-Identifier: Apache-2.0
//! Admit Combine selection refinements while retaining source operands on refusal.

use cadmpeg_ir::features::{BodySelection, CombineOperands, FeatureId};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

pub(super) fn refine(
    operands: &mut CombineOperands,
    feature: &FeatureId,
    edit: impl FnOnce(&mut BodySelection, &mut BodySelection),
) -> Option<LossNote> {
    operands.try_edit(edit).err().map(|error| {
        F3dLossCode::FeatureCombineSelectionNative.note(format!(
            "feature {feature} retains its source Combine selections: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_selection_refinement_retains_disjoint_source_operands_on_overlap() {
        let feature = FeatureId::mint("f3d:test:feature#combine").unwrap();
        let source = CombineOperands::new(
            BodySelection::Native("f3d:test:selection#target".into()),
            BodySelection::Native("f3d:test:selection#tools".into()),
        )
        .unwrap();
        let selected = |name| {
            BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![
                    cadmpeg_ir::ids::BodyId::mint(format!("f3d:test:body#{name}")).unwrap(),
                ])
                .unwrap(),
            )
        };
        let mut operands = source.clone();
        let loss = refine(&mut operands, &feature, |target, tools| {
            *target = selected("one");
            *tools = selected("one");
        })
        .unwrap();
        assert_eq!(operands, source);
        assert_eq!(loss.code.namespace(), "f3d");
        assert_eq!(loss.code.local_code(), "feature.combine-selection-native");

        let loss = refine(&mut operands, &feature, |target, tools| {
            *target = selected("one");
            *tools = selected("two");
        });
        assert!(loss.is_none());
        assert_eq!(operands.target(), &selected("one"));
        assert_eq!(operands.tools(), &selected("two"));
    }
}
