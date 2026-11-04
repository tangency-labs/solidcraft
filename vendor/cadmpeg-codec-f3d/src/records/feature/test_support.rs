// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::scope::{DesignFeatureKind, DesignParameterScopeDraft};
use crate::records::identity::{Located, ReferenceRun};

impl DesignParameterScopeDraft {
    pub(crate) fn with_fixture_layout(mut self) -> Self {
        self.paired_byte_offset = self.byte_offset + self.frame_length;
        self.layout_fixture_references();
        self.layout_fixture_tail();
        self
    }

    pub(crate) fn layout_fixture_references(&mut self) {
        self.reference_members = ReferenceRun::located(
            self.reference_members
                .values()
                .copied()
                .enumerate()
                .map(|(ordinal, value)| Located {
                    value,
                    offset: self.reference_count_offset + 5 + 11 * u64_from_index(ordinal),
                })
                .collect(),
        );
        self.kind_offset =
            self.reference_count_offset + 12 + 11 * u64_from_index(self.reference_members.len());
    }

    pub(crate) fn layout_fixture_tail(&mut self) {
        let (tail_length, previous_delta) =
            match (self.payload.kind(), self.previous_history_state_id) {
                (DesignFeatureKind::CopyPasteBodies, Some(_)) => (110, 53),
                (DesignFeatureKind::CopyPasteBodies, None) => (80, 0),
                _ => (72, 30),
            };
        self.feature_ordinal_offset = self.paired_byte_offset - tail_length;
        self.previous_history_state_id_offset = self.previous_history_state_id.map(|previous| {
            self.history_state_id.get_or_insert(previous + 1);
            self.feature_ordinal_offset + previous_delta
        });
    }
}
