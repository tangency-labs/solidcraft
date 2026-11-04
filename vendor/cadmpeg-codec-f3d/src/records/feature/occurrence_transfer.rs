// SPDX-License-Identifier: Apache-2.0
//! Component occurrence ownership and placements carried by paste scopes.

use super::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID;
use super::scope::DesignParameterScope;
use crate::layout::{copy_paste_prefix as copy_prefix, cut_paste_prefix as cut_prefix};
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::u64_from_index;
use serde::{Deserialize, Serialize};

pub(crate) const CONTEXT_TYPE: &str = "C3423E04-7E85-4B44-8E31-A5FA9C100052";
pub(crate) const OWNER_TYPE: &str = "3A6D1E62-99F8-4E32-BAAA-DBA0A2CFB27C";
pub(crate) const COMPONENT_TYPE: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignOccurrenceTransfer {
    pub(crate) relation_record_index: u32,
    pub(crate) occurrence_record_index: u32,
    pub(crate) component_record_index: u64,
    pub(crate) target_owner: DesignPasteOwner,
    pub(crate) target_transform: SketchPlacementMatrix,
    pub(crate) target_transform_offset: Option<u64>,
    pub(crate) source: DesignOccurrenceTransferSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DesignOccurrenceTransferSource {
    Cut {
        removal_record_index: u32,
        source_relation_record_index: u32,
        source_owner: DesignPasteOwner,
        occurrence_key: DesignRelaxedGuidText,
    },
    Copy {
        source_transform: SketchPlacementMatrix,
        source_transform_offset: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignPasteOwner {
    /// Record index of the owner collection.
    pub(crate) collection: u32,
    /// Record index of the context that holds the collection.
    pub(crate) context: u32,
    /// Record index of the containing component aggregate.
    pub(crate) component: u32,
}

impl DesignPasteOwner {
    fn indices(&self) -> [u32; 3] {
        [self.component, self.context, self.collection]
    }
}

impl DesignOccurrenceTransfer {
    pub(crate) fn record_indices(&self) -> impl Iterator<Item = u32> {
        let source = match &self.source {
            DesignOccurrenceTransferSource::Cut {
                removal_record_index,
                source_relation_record_index,
                source_owner,
                ..
            } => [
                Some(*removal_record_index),
                Some(*source_relation_record_index),
                Some(source_owner.component),
                Some(source_owner.context),
                Some(source_owner.collection),
            ],
            DesignOccurrenceTransferSource::Copy { .. } => [None; 5],
        };
        [
            Some(self.relation_record_index),
            Some(self.occurrence_record_index),
            u32::try_from(self.component_record_index).ok(),
        ]
        .into_iter()
        .chain(source)
        .chain(self.target_owner.indices().map(Some))
        .flatten()
    }

    pub(crate) fn links_valid(
        &self,
        scope: &DesignParameterScope,
        record_offset: impl Fn(u32) -> Option<u64>,
        has_type: impl Fn(u32, &str) -> bool,
    ) -> bool {
        let before = |a, b| {
            record_offset(a)
                .zip(record_offset(b))
                .is_some_and(|(a, b)| a < b)
        };
        let owner_valid = |owner: &DesignPasteOwner| {
            has_type(owner.component, COMPONENT_TYPE)
                && has_type(owner.context, CONTEXT_TYPE)
                && has_type(owner.collection, OWNER_TYPE)
                && before(owner.component, owner.context)
                && before(owner.context, owner.collection)
        };
        if !self.valid_for_scope(scope)
            || !has_type(self.occurrence_record_index, OCCURRENCE_PLACEMENT_TYPE_GUID)
            || !u32::try_from(self.component_record_index)
                .ok()
                .is_some_and(|index| has_type(index, COMPONENT_TYPE))
            || !self
                .record_indices()
                .all(|index| record_offset(index).is_some_and(|at| at < scope.byte_offset()))
            || !before(self.occurrence_record_index, self.relation_record_index)
            || !owner_valid(&self.target_owner)
        {
            return false;
        }
        match &self.source {
            DesignOccurrenceTransferSource::Copy { .. } => true,
            DesignOccurrenceTransferSource::Cut {
                source_owner,
                removal_record_index,
                source_relation_record_index,
                ..
            } => {
                owner_valid(source_owner)
                    && before(self.occurrence_record_index, *source_relation_record_index)
                    && before(*source_relation_record_index, self.relation_record_index)
                    && before(self.relation_record_index, *removal_record_index)
            }
        }
    }

    pub(crate) fn valid_for_scope(&self, scope: &DesignParameterScope) -> bool {
        if self.record_indices().any(|index| index == 0)
            || self.component_record_index == 0
            || self.component_record_index > u64::from(u32::MAX)
            || !scope
                .reference_members()
                .values()
                .copied()
                .eq([self.relation_record_index])
            || self.occurrence_record_index == self.relation_record_index
            || self.relation_record_index == scope.record_index
        {
            return false;
        }
        if let DesignOccurrenceTransferSource::Cut {
            removal_record_index,
            source_relation_record_index,
            ..
        } = &self.source
        {
            let indices = [
                self.occurrence_record_index,
                *source_relation_record_index,
                self.relation_record_index,
                *removal_record_index,
                scope.record_index,
            ];
            if indices
                .iter()
                .enumerate()
                .any(|(at, index)| indices[..at].contains(index))
            {
                return false;
            }
        }
        let target_at = match (&self.source, scope.kind_name(), scope.frame_length()) {
            (DesignOccurrenceTransferSource::Cut { .. }, "CutPaste", 277) => None,
            (DesignOccurrenceTransferSource::Cut { .. }, "CutPaste", 405) => {
                Some(u64_from_index(cut_prefix::LEN))
            }
            (DesignOccurrenceTransferSource::Cut { .. }, "CutPaste", 409) => {
                Some(u64_from_index(cut_prefix::LEN) + 4)
            }
            (
                DesignOccurrenceTransferSource::Copy {
                    source_transform_offset,
                    ..
                },
                "CopyPaste",
                length @ (525 | 529),
            ) => {
                let source_at = u64_from_index(copy_prefix::SOURCE_TRANSFORM)
                    + if length == 525 { 0 } else { 4 };
                if scope.byte_offset().checked_add(source_at) != Some(*source_transform_offset) {
                    return false;
                }
                Some(source_at + u64_from_index(copy_prefix::LEN - copy_prefix::SOURCE_TRANSFORM))
            }
            _ => return false,
        };
        match target_at {
            Some(offset) => scope.byte_offset().checked_add(offset) == self.target_transform_offset,
            None => {
                self.target_transform_offset.is_none()
                    && self.target_transform == SketchPlacementMatrix::IDENTITY
            }
        }
    }
}

rewrite_native_record!(DesignPasteOwner, []; {collection, context, component});
rewrite_native_record!(DesignOccurrenceTransfer, []; {relation_record_index, occurrence_record_index, component_record_index, target_owner, target_transform, target_transform_offset, source});
rewrite_native_enum!(DesignOccurrenceTransferSource, []; {Cut {removal_record_index, source_relation_record_index, source_owner, occurrence_key}, Copy {source_transform, source_transform_offset}});

#[cfg(test)]
mod tests;
