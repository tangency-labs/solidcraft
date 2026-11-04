// SPDX-License-Identifier: Apache-2.0
//! Emboss selection links, scalar owners, and native control lanes.

use crate::layout::{
    emboss_counted_suffix as suffix, emboss_native_pair as pair,
    emboss_scope_reference_table_prefix as fixed_table, emboss_single_control_head as single,
    emboss_two_control_head as double,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::decode::u64_from_index;
use serde::{Deserialize, Serialize};

const PROFILE_FIRST_SINGLE_REFERENCE_ORDER: [usize; 7] = [4, 8, 5, 7, 2, 6, 0];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum DesignEmbossControlWords {
    One([u32; 1]),
    Two([u32; 2]),
}

cadmpeg_core::named_optional_field!(deserialize_emboss_native_pair, [u64; 2], "native_pair");
cadmpeg_core::named_optional_field!(
    deserialize_emboss_sketch_profile,
    DesignSketchProfileOperand,
    "sketch_profile"
);
cadmpeg_core::named_optional_field!(
    deserialize_emboss_additional_pairs,
    Vec<[u64; 2]>,
    "additional_native_pairs"
);

/// Construction links and native controls carried by an Emboss scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignEmbossConstruction {
    pub(crate) control_words: DesignEmbossControlWords,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_emboss_native_pair"
    )]
    pub(crate) native_pair: Option<[u64; 2]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_emboss_additional_pairs"
    )]
    pub(crate) additional_native_pairs: Option<Vec<[u64; 2]>>,
    pub(crate) depth_owner: Located<u32>,
    pub(crate) auxiliary_group: Located<u32>,
    pub(crate) horizontal_owner: Located<u32>,
    pub(crate) rotation_owner: Located<u32>,
    pub(crate) face_group: Located<u32>,
    pub(crate) vertical_owner: Located<u32>,
    pub(crate) profile_group: Located<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_emboss_sketch_profile"
    )]
    pub(crate) sketch_profile: Option<DesignSketchProfileOperand>,
}

impl DesignEmbossConstruction {
    pub(crate) fn scalar_owners(&self) -> [(Located<u32>, &'static str); 4] {
        [
            (self.depth_owner, "Depth"),
            (self.horizontal_owner, "Horizontal Distance"),
            (self.vertical_owner, "Vertical Distance"),
            (self.rotation_owner, "Rotation Angle"),
        ]
    }

    fn references(&self) -> [Located<u32>; 7] {
        [
            self.depth_owner,
            self.auxiliary_group,
            self.horizontal_owner,
            self.rotation_owner,
            self.face_group,
            self.vertical_owner,
            self.profile_group,
        ]
    }

    fn reference_order_matches(&self, scope: &DesignParameterScope, ordinals: [usize; 7]) -> bool {
        self.references()
            .iter()
            .zip(ordinals)
            .all(|(field, ordinal)| {
                scope.reference_members().values().nth(ordinal) == Some(&field.value)
            })
    }

    pub(crate) fn has_fixed_operands(&self, scope: &DesignParameterScope) -> bool {
        self.pair_count() == Some(1)
            && matches!(self.control_words, DesignEmbossControlWords::Two(_))
            && u32::try_from(scope.reference_members().len()).ok()
                == Some(fixed_table::REFERENCE_COUNT_VALUE)
            && self.reference_order_matches(scope, PROFILE_FIRST_SINGLE_REFERENCE_ORDER)
    }

    fn pair_count(&self) -> Option<usize> {
        let additional = self.additional_native_pairs.as_deref().unwrap_or(&[]);
        if self.native_pair.is_none() {
            return additional.is_empty().then_some(0);
        }
        additional.len().checked_add(1)
    }

    pub(crate) fn links_match(&self, scope: &DesignParameterScope) -> bool {
        self.pair_count()
            .is_some_and(|count| self.links_match_for_pair_count(scope, count))
    }

    pub(crate) fn links_match_for_pair_count(
        &self,
        scope: &DesignParameterScope,
        pair_count: usize,
    ) -> bool {
        let (head_length, head_fields) = match self.control_words {
            DesignEmbossControlWords::One(_) => (
                single::LEN,
                [
                    single::DEPTH_OWNER_REFERENCE,
                    single::AUXILIARY_GROUP_REFERENCE,
                    single::HORIZONTAL_OWNER_REFERENCE,
                ],
            ),
            DesignEmbossControlWords::Two(_) => (
                double::LEN,
                [
                    double::DEPTH_OWNER_REFERENCE,
                    double::AUXILIARY_GROUP_REFERENCE,
                    double::HORIZONTAL_OWNER_REFERENCE,
                ],
            ),
        };
        let Some(suffix_start) = pair_count
            .checked_mul(pair::LEN)
            .and_then(|length| head_length.checked_add(length))
        else {
            return false;
        };
        if suffix_start.checked_add(suffix::LEN).is_none() {
            return false;
        }
        if scope.kind() != DesignFeatureKind::Emboss
            || u32::try_from(pair_count).is_err()
            || u32::try_from(scope.reference_members().len()).is_err()
            || scope
                .byte_offset()
                .checked_add(u64_from_index(suffix_start + suffix::REFERENCE_COUNT))
                != Some(scope.reference_count_offset())
            || self.sketch_profile.as_ref().is_some_and(|profile| {
                pair_count != 1
                    || !self.has_fixed_operands(scope)
                    || profile.scope_reference_ordinal != 1
                    || scope.reference_members().values().nth(1) != Some(&profile.record_index)
            })
        {
            return false;
        }
        let fields = self.references();
        let relative_offsets = [
            head_fields[0] + 1,
            head_fields[1] + 1,
            head_fields[2] + 1,
            suffix_start + suffix::ROTATION_OWNER_REFERENCE + 1,
            suffix_start + suffix::FACE_GROUP_REFERENCE + 1,
            suffix_start + suffix::VERTICAL_OWNER_REFERENCE + 1,
            suffix_start + suffix::PROFILE_GROUP_REFERENCE + 1,
        ];
        if !fields
            .iter()
            .zip(relative_offsets)
            .enumerate()
            .all(|(ordinal, (field, relative))| {
                field.value != 0
                    && !fields[..ordinal]
                        .iter()
                        .any(|prior| prior.value == field.value)
                    && scope.byte_offset().checked_add(u64_from_index(relative))
                        == Some(field.offset)
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| *value == field.value)
            })
            || scope.reference_members().offsets().count() != scope.reference_members().len()
        {
            return false;
        }
        let mut prior = None;
        for (ordinal, (value, offset)) in scope
            .reference_members()
            .values()
            .zip(scope.reference_members().offsets())
            .enumerate()
        {
            if prior.is_some_and(|previous| previous >= *value)
                || scope
                    .reference_count_offset()
                    .checked_add(5 + 11 * u64_from_index(ordinal))
                    != Some(*offset)
            {
                return false;
            }
            prior = Some(*value);
        }
        true
    }
}

rewrite_native_scalar!(DesignEmbossControlWords);
rewrite_native_record!(DesignEmbossConstruction, []; {control_words, native_pair, additional_native_pairs, depth_owner, auxiliary_group, horizontal_owner, rotation_owner, face_group, vertical_owner, profile_group, sketch_profile});
