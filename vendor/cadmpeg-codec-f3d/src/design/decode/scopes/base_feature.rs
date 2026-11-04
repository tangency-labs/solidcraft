use crate::design::decode::text::{fixed_guid_end, fixed_relaxed_guid_text, fixed_utf16_ascii_eq};
use crate::layout::base_feature_class_377_prefix as class_377;
use crate::layout::base_feature_class_452_262_compact as class_452_compact;
use crate::layout::base_feature_class_452_262_expanded as class_452_expanded;
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::base_feature::DesignBaseFeatureEntry;
use crate::records::feature::base_feature::DesignLegacyBaseFeatureBody;
use crate::records::feature::base_feature::{
    local_reference_field, DesignBaseFeatureBodyReferenceForm,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

use super::shared_frames::marked_record_reference;

fn marked_u64_reference(bytes: &[u8], marker_offset: usize, marker_value: u8) -> Option<u64> {
    if bytes.get(marker_offset) != Some(&marker_value) {
        return None;
    }
    View::u64_le_at(bytes, marker_offset + 1)
}

fn exact_body_based_on_faces_property(
    bytes: &[u8],
    start: usize,
    property_marker: usize,
) -> Option<()> {
    if bytes.get(start + property_marker) != Some(&class_377::TAG_BODY_BASED_ON_FACES_MARKER_VALUE)
        || View::u32_le_at(bytes, start + property_marker + 1)?
            != class_377::TAG_BODY_BASED_ON_FACES_COUNT_VALUE
        || View::u32_le_at(bytes, start + property_marker + 5)?
            != class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE
        || bytes.get(start + property_marker + 9..start + property_marker + 28)?
            != b"TagBodyBasedOnFaces"
        || View::u32_le_at(bytes, start + property_marker + 28)?
            != class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE
        || bytes.get(start + property_marker + 32..start + property_marker + 53)?
            != b"IntrinsicMetaTypebool"
        || View::u16_le_at(bytes, start + property_marker + 53)?
            != class_377::TAG_BODY_BASED_ON_FACES_VALUE_VALUE
    {
        return None;
    }
    Some(())
}

#[derive(Clone, Copy)]
struct BaseFeatureScopeTailLayout {
    frame_length: usize,
    reference_count: usize,
    generic_scope_reference_marker: usize,
    generic_scope_reference_record: usize,
    generic_scope_reference_field: usize,
    history_state_id: usize,
    kind_length: usize,
    kind: usize,
    feature_ordinal: usize,
    previous_history_state_id: usize,
}

fn exact_base_feature_scope_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    layout: BaseFeatureScopeTailLayout,
) -> Option<()> {
    if scope.reference_members().len() != 1
        || scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset())
        || scope.frame_length() != u64::try_from(layout.frame_length).ok()?
        || scope.reference_count_offset()
            != scope.byte_offset() + u64::try_from(layout.reference_count).ok()?
        || !scope.reference_members().offsets().copied().eq([
            scope.byte_offset() + u64::try_from(layout.generic_scope_reference_record).ok()?
        ])
        || scope.kind_offset() != scope.byte_offset() + u64::try_from(layout.kind).ok()?
        || scope.feature_ordinal_offset()
            != scope.byte_offset() + u64::try_from(layout.feature_ordinal).ok()?
        || scope.previous_history_state_id_offset()
            != Some(scope.byte_offset() + u64::try_from(layout.previous_history_state_id).ok()?)
        || View::u32_le_at(bytes, start + layout.reference_count)?
            != class_377::REFERENCE_COUNT_VALUE
        || bytes.get(start + layout.generic_scope_reference_marker)
            != Some(&class_377::GENERIC_SCOPE_REFERENCE_MARKER_VALUE)
        || marked_record_reference(bytes, start + layout.generic_scope_reference_marker)
            != Some(*scope.reference_members().values().next()?)
        || bytes
            .get(start + layout.generic_scope_reference_field..start + layout.history_state_id)?
            != [0; 6]
        || View::u32_le_at(bytes, start + layout.history_state_id)?
            != scope
                .history_state_id()
                .and_then(|id| u32::try_from(id).ok())?
        || View::u32_le_at(bytes, start + layout.kind_length)? != class_377::KIND_LENGTH_VALUE
    {
        return None;
    }
    let kind_end = fixed_utf16_ascii_eq(bytes, start + layout.kind_length, "Base Feature")?;
    if kind_end != start + layout.feature_ordinal
        || View::u32_le_at(bytes, start + layout.feature_ordinal)? != scope.feature_ordinal.get()
    {
        return None;
    }
    let previous_state = View::u32_le_at(bytes, start + layout.previous_history_state_id)?;
    let previous_matches = match scope.previous_history_state_id() {
        Some(id) => u32::try_from(id).ok() == Some(previous_state),
        None => previous_state == u32::MAX,
    };
    previous_matches.then_some(())
}

fn exact_base_feature_legacy_body_based_on_faces(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    if scope.class_tag.as_str() != "452" || scope.paired_class_tag.as_str() != "262" {
        return Ok(None);
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    Ok(
        match View::u32_le_at(bytes, start + class_452_compact::BODY_COUNT) {
            Some(1) => exact_base_feature_legacy_compact(bytes, scope, start)?,
            Some(2) => exact_base_feature_legacy_expanded(bytes, scope, start)?,
            _ => None,
        },
    )
}

fn exact_base_feature_legacy_compact(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    (|| {
        if scope.frame_length() != u64::try_from(class_452_compact::LEN).ok()? {
            return None;
        }
        if bytes.get(
            start + class_452_compact::ZERO_RUN_8..start + class_452_compact::BODY_COUNT_MARKER,
        )? != [0; 8]
            || bytes.get(start + class_452_compact::BODY_COUNT_MARKER)
                != Some(&class_452_compact::BODY_COUNT_MARKER_VALUE)
            || View::u32_le_at(bytes, start + class_452_compact::BODY_COUNT)?
                != class_452_compact::BODY_COUNT_VALUE
        {
            return None;
        }
        let body_entity_suffix = marked_u64_reference(
            bytes,
            start + class_452_compact::BODY_ENTITY_REFERENCE_MARKER,
            class_452_compact::BODY_ENTITY_REFERENCE_MARKER_VALUE,
        )?;
        if body_entity_suffix == 0 {
            return None;
        }
        let body_entity_field = local_reference_field(bytes.get(
            start + class_452_compact::BODY_ENTITY_REFERENCE_FIELD
                ..start + class_452_compact::TAG_BODY_BASED_ON_FACES_MARKER,
        )?)?;
        exact_body_based_on_faces_property(
            bytes,
            start,
            class_452_compact::TAG_BODY_BASED_ON_FACES_MARKER,
        )?;
        let mode = crate::records::feature::base_feature::DesignBaseFeatureCompactMode::try_from(
            *bytes.get(start + class_452_compact::MODE)?,
        )
        .ok()?;
        let parameter_body_record = marked_u64_reference(
            bytes,
            start + class_452_compact::PARAMETER_BODY_REFERENCE_MARKER,
            class_452_compact::PARAMETER_BODY_REFERENCE_MARKER_VALUE,
        )?;
        let scope_reference = marked_u64_reference(
            bytes,
            start + class_452_compact::SCOPE_REFERENCE_MARKER,
            class_452_compact::SCOPE_REFERENCE_MARKER_VALUE,
        )?;
        let auxiliary_record = marked_u64_reference(
            bytes,
            start + class_452_compact::AUXILIARY_REFERENCE_MARKER,
            class_452_compact::AUXILIARY_REFERENCE_MARKER_VALUE,
        )?;
        if bytes.get(start + class_452_compact::PARAMETER_BODY_COUNT)
            != Some(&class_452_compact::PARAMETER_BODY_COUNT_VALUE)
            || bytes.get(
                start + class_452_compact::PARAMETER_BODY_ZERO_RUN
                    ..start + class_452_compact::PARAMETER_BODY_REFERENCE_MARKER,
            )? != [0; 3]
            || parameter_body_record == 0
            || bytes.get(
                start + class_452_compact::PARAMETER_BODY_REFERENCE_FIELD
                    ..start + class_452_compact::SCOPE_REFERENCE_MARKER,
            )? != [0; 3]
            || scope_reference == 0
            || bytes.get(
                start + class_452_compact::SCOPE_REFERENCE_FIELD
                    ..start + class_452_compact::AUXILIARY_GROUP_MARKER,
            )? != [0; 2]
            || bytes.get(start + class_452_compact::AUXILIARY_GROUP_MARKER)
                != Some(&class_452_compact::AUXILIARY_GROUP_MARKER_VALUE)
            || bytes.get(
                start + class_452_compact::AUXILIARY_GROUP_ZERO_RUN
                    ..start + class_452_compact::AUXILIARY_REFERENCE_MARKER,
            )? != [0; 3]
            || auxiliary_record == 0
            || bytes.get(
                start + class_452_compact::AUXILIARY_REFERENCE_FIELD
                    ..start + class_452_compact::ENVELOPE_GUID_CODE_UNIT_COUNT,
            )? != [0; 10]
            || !scope
                .reference_members()
                .values()
                .copied()
                .eq([u32::try_from(scope_reference).ok()?])
        {
            return None;
        }
        let (envelope_guid, guid_end) = match fixed_relaxed_guid_text(
            bytes,
            start + class_452_compact::ENVELOPE_GUID_CODE_UNIT_COUNT,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if guid_end != start + class_452_compact::ZERO_RUN_AFTER_GUID
            || bytes.get(
                start + class_452_compact::ZERO_RUN_AFTER_GUID
                    ..start + class_452_compact::REFERENCE_COUNT,
            )? != [0; 3]
        {
            return None;
        }
        exact_base_feature_scope_tail(
            bytes,
            scope,
            start,
            BaseFeatureScopeTailLayout {
                frame_length: class_452_compact::LEN,
                reference_count: class_452_compact::REFERENCE_COUNT,
                generic_scope_reference_marker: class_452_compact::GENERIC_SCOPE_REFERENCE_MARKER,
                generic_scope_reference_record: class_452_compact::GENERIC_SCOPE_REFERENCE_RECORD,
                generic_scope_reference_field: class_452_compact::GENERIC_SCOPE_REFERENCE_FIELD,
                history_state_id: class_452_compact::HISTORY_STATE_ID,
                kind_length: class_452_compact::KIND_LENGTH,
                kind: class_452_compact::KIND,
                feature_ordinal: class_452_compact::FEATURE_ORDINAL,
                previous_history_state_id: class_452_compact::PREVIOUS_HISTORY_STATE_ID,
            },
        )?;
        Some(Ok(DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
            form: DesignBaseFeatureBodyReferenceForm::CompactOneBody {
                mode: Located {
                    value: mode,
                    offset: scope.byte_offset() + u64::try_from(class_452_compact::MODE).ok()?,
                },
                body: DesignLegacyBaseFeatureBody {
                    entity: DesignBaseFeatureEntry {
                        value: u32::try_from(body_entity_suffix).ok()?,
                        offset: scope.byte_offset()
                            + u64::try_from(class_452_compact::BODY_ENTITY_SUFFIX).ok()?,
                        field: body_entity_field,
                    },
                    parameter_body: Located {
                        value: parameter_body_record,
                        offset: scope.byte_offset()
                            + u64::try_from(class_452_compact::PARAMETER_BODY_RECORD).ok()?,
                    },
                    auxiliary: Located {
                        value: auxiliary_record,
                        offset: scope.byte_offset()
                            + u64::try_from(class_452_compact::AUXILIARY_RECORD).ok()?,
                    },
                },
            },
            scope_reference,
            scope_reference_offset: scope.byte_offset()
                + u64::try_from(class_452_compact::SCOPE_REFERENCE).ok()?,
            envelope_guid,
            envelope_guid_offset: scope.byte_offset()
                + u64::try_from(class_452_compact::ENVELOPE_GUID).ok()?,
            tag_body_based_on_faces_offset: scope.byte_offset()
                + u64::try_from(class_452_compact::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
        }))
    })()
    .transpose()
}

fn exact_base_feature_legacy_expanded(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    (|| {
        if scope.frame_length() != u64::try_from(class_452_expanded::LEN).ok()? {
            return None;
        }
        if bytes.get(
            start + class_452_expanded::ZERO_RUN_8..start + class_452_expanded::BODY_COUNT_MARKER,
        )? != [0; 8]
            || bytes.get(start + class_452_expanded::BODY_COUNT_MARKER)
                != Some(&class_452_expanded::BODY_COUNT_MARKER_VALUE)
            || View::u32_le_at(bytes, start + class_452_expanded::BODY_COUNT)?
                != class_452_expanded::BODY_COUNT_VALUE
        {
            return None;
        }
        let body_entity_suffixes = [
            marked_u64_reference(
                bytes,
                start + class_452_expanded::BODY_ENTITY_ONE_MARKER,
                class_452_expanded::BODY_ENTITY_ONE_MARKER_VALUE,
            )?,
            marked_u64_reference(
                bytes,
                start + class_452_expanded::BODY_ENTITY_TWO_MARKER,
                class_452_expanded::BODY_ENTITY_TWO_MARKER_VALUE,
            )?,
        ];
        if body_entity_suffixes.contains(&0) {
            return None;
        }
        let body_entity_fields = [
            local_reference_field(bytes.get(
                start + class_452_expanded::BODY_ENTITY_ONE_FIELD
                    ..start + class_452_expanded::BODY_ENTITY_TWO_MARKER,
            )?)?,
            local_reference_field(bytes.get(
                start + class_452_expanded::BODY_ENTITY_TWO_FIELD
                    ..start + class_452_expanded::TAG_BODY_BASED_ON_FACES_MARKER,
            )?)?,
        ];
        exact_body_based_on_faces_property(
            bytes,
            start,
            class_452_expanded::TAG_BODY_BASED_ON_FACES_MARKER,
        )?;
        let parameter_body_records = [
            marked_record_reference(bytes, start + class_452_expanded::PARAMETER_BODY_ONE_MARKER)?,
            marked_record_reference(bytes, start + class_452_expanded::PARAMETER_BODY_TWO_MARKER)?,
        ];
        let scope_reference =
            marked_record_reference(bytes, start + class_452_expanded::SCOPE_REFERENCE_MARKER)?;
        let auxiliary_records = [
            marked_record_reference(bytes, start + class_452_expanded::AUXILIARY_BODY_ONE_MARKER)?,
            marked_record_reference(bytes, start + class_452_expanded::AUXILIARY_BODY_TWO_MARKER)?,
        ];
        if parameter_body_records.contains(&0)
            || auxiliary_records.contains(&0)
            || bytes.get(start + class_452_expanded::PARAMETER_BODY_GROUP_MARKER)
                != Some(&class_452_expanded::PARAMETER_BODY_GROUP_MARKER_VALUE)
            || View::u32_le_at(bytes, start + class_452_expanded::PARAMETER_BODY_COUNT)?
                != class_452_expanded::PARAMETER_BODY_COUNT_VALUE
            || bytes.get(
                start + class_452_expanded::PARAMETER_BODY_ONE_FIELD
                    ..start + class_452_expanded::PARAMETER_BODY_TWO_MARKER,
            )? != [0; 6]
            || bytes.get(
                start + class_452_expanded::PARAMETER_BODY_TWO_FIELD
                    ..start + class_452_expanded::PARAMETER_BODY_SEPARATOR,
            )? != [0; 6]
            || bytes.get(start + class_452_expanded::PARAMETER_BODY_SEPARATOR)
                != Some(&class_452_expanded::PARAMETER_BODY_SEPARATOR_VALUE)
            || scope_reference != scope.reference_members().values().next().copied()?
            || bytes.get(
                start + class_452_expanded::SCOPE_REFERENCE_FIELD
                    ..start + class_452_expanded::AUXILIARY_BODY_COUNT,
            )? != [0; 6]
            || View::u32_le_at(bytes, start + class_452_expanded::AUXILIARY_BODY_COUNT)?
                != class_452_expanded::AUXILIARY_BODY_COUNT_VALUE
            || bytes.get(
                start + class_452_expanded::AUXILIARY_BODY_ONE_FIELD
                    ..start + class_452_expanded::AUXILIARY_BODY_TWO_MARKER,
            )? != [0; 6]
            || bytes.get(
                start + class_452_expanded::AUXILIARY_BODY_TWO_FIELD
                    ..start + class_452_expanded::AUXILIARY_BODY_ZERO_RUN,
            )? != [0; 6]
            || bytes.get(
                start + class_452_expanded::AUXILIARY_BODY_ZERO_RUN
                    ..start + class_452_expanded::ENVELOPE_GUID_CODE_UNIT_COUNT,
            )? != [0; 8]
        {
            return None;
        }
        let (envelope_guid, guid_end) = match fixed_relaxed_guid_text(
            bytes,
            start + class_452_expanded::ENVELOPE_GUID_CODE_UNIT_COUNT,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if guid_end != start + class_452_expanded::ZERO_RUN_AFTER_GUID
            || bytes.get(
                start + class_452_expanded::ZERO_RUN_AFTER_GUID
                    ..start + class_452_expanded::REFERENCE_COUNT,
            )? != [0; 3]
        {
            return None;
        }
        exact_base_feature_scope_tail(
            bytes,
            scope,
            start,
            BaseFeatureScopeTailLayout {
                frame_length: class_452_expanded::LEN,
                reference_count: class_452_expanded::REFERENCE_COUNT,
                generic_scope_reference_marker: class_452_expanded::GENERIC_SCOPE_REFERENCE_MARKER,
                generic_scope_reference_record: class_452_expanded::GENERIC_SCOPE_REFERENCE_RECORD,
                generic_scope_reference_field: class_452_expanded::GENERIC_SCOPE_REFERENCE_FIELD,
                history_state_id: class_452_expanded::HISTORY_STATE_ID,
                kind_length: class_452_expanded::KIND_LENGTH,
                kind: class_452_expanded::KIND,
                feature_ordinal: class_452_expanded::FEATURE_ORDINAL,
                previous_history_state_id: class_452_expanded::PREVIOUS_HISTORY_STATE_ID,
            },
        )?;
        Some(Ok(DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
            form: DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody {
                bodies: [
                    DesignLegacyBaseFeatureBody {
                        entity: DesignBaseFeatureEntry {
                            value: u32::try_from(body_entity_suffixes[0]).ok()?,
                            offset: scope.byte_offset()
                                + u64::try_from(class_452_expanded::BODY_ENTITY_ONE_SUFFIX).ok()?,
                            field: body_entity_fields[0],
                        },
                        parameter_body: Located {
                            value: u64::from(parameter_body_records[0]),
                            offset: scope.byte_offset()
                                + u64::try_from(class_452_expanded::PARAMETER_BODY_ONE_RECORD)
                                    .ok()?,
                        },
                        auxiliary: Located {
                            value: u64::from(auxiliary_records[0]),
                            offset: scope.byte_offset()
                                + u64::try_from(class_452_expanded::AUXILIARY_BODY_ONE_RECORD)
                                    .ok()?,
                        },
                    },
                    DesignLegacyBaseFeatureBody {
                        entity: DesignBaseFeatureEntry {
                            value: u32::try_from(body_entity_suffixes[1]).ok()?,
                            offset: scope.byte_offset()
                                + u64::try_from(class_452_expanded::BODY_ENTITY_TWO_SUFFIX).ok()?,
                            field: body_entity_fields[1],
                        },
                        parameter_body: Located {
                            value: u64::from(parameter_body_records[1]),
                            offset: scope.byte_offset()
                                + u64::try_from(class_452_expanded::PARAMETER_BODY_TWO_RECORD)
                                    .ok()?,
                        },
                        auxiliary: Located {
                            value: u64::from(auxiliary_records[1]),
                            offset: scope.byte_offset()
                                + u64::try_from(class_452_expanded::AUXILIARY_BODY_TWO_RECORD)
                                    .ok()?,
                        },
                    },
                ],
            },
            scope_reference: u64::from(scope_reference),
            scope_reference_offset: scope.byte_offset()
                + u64::try_from(class_452_expanded::SCOPE_REFERENCE).ok()?,
            envelope_guid,
            envelope_guid_offset: scope.byte_offset()
                + u64::try_from(class_452_expanded::ENVELOPE_GUID).ok()?,
            tag_body_based_on_faces_offset: scope.byte_offset()
                + u64::try_from(class_452_expanded::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
        }))
    })()
    .transpose()
}

fn exact_base_feature_direct_body_based_on_faces(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    (|| {
        if !matches!(
            (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
            ("365", "262") | ("377", "259")
        ) || scope.frame_length() != u64::try_from(class_377::LEN).ok()?
            || scope.byte_offset().checked_add(scope.frame_length())
                != Some(scope.paired_byte_offset())
            || scope.reference_members().len() != 1
            || scope.reference_count_offset()
                != scope.byte_offset() + u64::try_from(class_377::REFERENCE_COUNT).ok()?
            || !scope
                .reference_members()
                .offsets()
                .copied()
                .eq([scope.byte_offset()
                    + u64::try_from(class_377::GENERIC_SCOPE_REFERENCE_RECORD).ok()?])
            || scope.kind_offset()
                != scope.byte_offset() + u64::try_from(class_377::KIND_LENGTH + 4).ok()?
            || scope.feature_ordinal_offset()
                != scope.byte_offset() + u64::try_from(class_377::FEATURE_ORDINAL).ok()?
            || scope.previous_history_state_id_offset()
                != Some(
                    scope.byte_offset()
                        + u64::try_from(class_377::PREVIOUS_HISTORY_STATE_ID).ok()?,
                )
        {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        if bytes
            .get(start + class_377::ZERO_RUN_8..start + class_377::BODY_REFERENCE_COUNT_MARKER)?
            != [0; 8]
            || bytes.get(start + class_377::BODY_REFERENCE_COUNT_MARKER)
                != Some(&class_377::BODY_REFERENCE_COUNT_MARKER_VALUE)
            || View::u32_le_at(bytes, start + class_377::BODY_REFERENCE_COUNT)?
                != class_377::BODY_REFERENCE_COUNT_VALUE
        {
            return None;
        }
        let parameter_body_record =
            marked_record_reference(bytes, start + class_377::PARAMETER_BODY_REFERENCE_MARKER)?;
        let body_entity_suffix =
            marked_record_reference(bytes, start + class_377::BODY_ENTITY_REFERENCE_MARKER)?;
        if parameter_body_record == 0
            || body_entity_suffix == 0
            || View::u32_le_at(bytes, start + class_377::PARAMETER_BODY_RECORD)?
                != parameter_body_record
            || View::u32_le_at(bytes, start + class_377::BODY_ENTITY_SUFFIX)? != body_entity_suffix
            || bytes.get(
                start + class_377::PARAMETER_BODY_REFERENCE_FIELD
                    ..start + class_377::BODY_ENTITY_REFERENCE_MARKER,
            )? != [0; 10]
            || bytes.get(
                start + class_377::BODY_ENTITY_REFERENCE_FIELD
                    ..start + class_377::TAG_BODY_BASED_ON_FACES_MARKER,
            )? != [0; 10]
        {
            return None;
        }
        if bytes.get(start + class_377::TAG_BODY_BASED_ON_FACES_MARKER)
            != Some(&class_377::TAG_BODY_BASED_ON_FACES_MARKER_VALUE)
            || View::u32_le_at(bytes, start + class_377::TAG_BODY_BASED_ON_FACES_COUNT)?
                != class_377::TAG_BODY_BASED_ON_FACES_COUNT_VALUE
            || View::u32_le_at(bytes, start + class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH)?
                != class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE
            || bytes.get(
                start + class_377::TAG_BODY_BASED_ON_FACES_KEY
                    ..start + class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH,
            )? != b"TagBodyBasedOnFaces"
            || View::u32_le_at(
                bytes,
                start + class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH,
            )? != class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE
            || bytes.get(
                start + class_377::TAG_BODY_BASED_ON_FACES_TYPE
                    ..start + class_377::TAG_BODY_BASED_ON_FACES_VALUE,
            )? != b"IntrinsicMetaTypebool"
            || View::u16_le_at(bytes, start + class_377::TAG_BODY_BASED_ON_FACES_VALUE)?
                != class_377::TAG_BODY_BASED_ON_FACES_VALUE_VALUE
        {
            return None;
        }
        if bytes.get(start + class_377::PARAMETER_REFERENCE_GROUP_MARKER)
            != Some(&class_377::PARAMETER_REFERENCE_GROUP_MARKER_VALUE)
            || View::u32_le_at(bytes, start + class_377::PARAMETER_REFERENCE_GROUP_COUNT)?
                != class_377::PARAMETER_REFERENCE_GROUP_COUNT_VALUE
            || marked_record_reference(bytes, start + class_377::PARAMETER_REFERENCE_MARKER)?
                != parameter_body_record
            || bytes.get(
                start + class_377::PARAMETER_REFERENCE_FIELD
                    ..start + class_377::SCOPE_REFERENCE_MEMBER_MARKER,
            )? != [0; 7]
            || marked_record_reference(bytes, start + class_377::SCOPE_REFERENCE_MEMBER_MARKER)?
                != *scope.reference_members().values().next()?
            || bytes.get(
                start + class_377::SCOPE_REFERENCE_MEMBER_FIELD
                    ..start + class_377::AUXILIARY_GROUP_MARKER,
            )? != [0; 6]
            || bytes.get(start + class_377::AUXILIARY_GROUP_MARKER)
                != Some(&class_377::AUXILIARY_GROUP_MARKER_VALUE)
            || bytes.get(
                start + class_377::AUXILIARY_GROUP_ZERO_RUN
                    ..start + class_377::AUXILIARY_REFERENCE_MARKER,
            )? != [0; 3]
        {
            return None;
        }
        let auxiliary_record =
            marked_record_reference(bytes, start + class_377::AUXILIARY_REFERENCE_MARKER)?;
        if auxiliary_record == 0
            || bytes.get(
                start + class_377::AUXILIARY_REFERENCE_FIELD
                    ..start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT,
            )? != [0; 14]
            || View::u32_le_at(bytes, start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT)?
                != class_377::ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE
        {
            return None;
        }
        let (envelope_guid, guid_end) = match fixed_relaxed_guid_text(
            bytes,
            start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let previous_history_state_id =
            View::u32_le_at(bytes, start + class_377::PREVIOUS_HISTORY_STATE_ID)?;
        let previous_history_state_matches = match scope.previous_history_state_id() {
            Some(id) => u32::try_from(id).ok() == Some(previous_history_state_id),
            None => previous_history_state_id == u32::MAX,
        };
        if guid_end != start + class_377::ZERO_RUN_3
            || bytes.get(start + class_377::ZERO_RUN_3..start + class_377::REFERENCE_COUNT)?
                != [0; 3]
            || View::u32_le_at(bytes, start + class_377::REFERENCE_COUNT)?
                != class_377::REFERENCE_COUNT_VALUE
            || marked_record_reference(bytes, start + class_377::GENERIC_SCOPE_REFERENCE_MARKER)?
                != *scope.reference_members().values().next()?
            || View::u32_le_at(bytes, start + class_377::HISTORY_STATE_ID)?
                != scope
                    .history_state_id()
                    .and_then(|id| u32::try_from(id).ok())?
            || !previous_history_state_matches
            || View::u32_le_at(bytes, start + class_377::KIND_LENGTH)?
                != class_377::KIND_LENGTH_VALUE
        {
            return None;
        }
        let kind_end = fixed_utf16_ascii_eq(bytes, start + class_377::KIND_LENGTH, "Base Feature")?;
        if kind_end != start + class_377::FEATURE_ORDINAL
            || View::u32_le_at(bytes, start + class_377::FEATURE_ORDINAL)?
                != scope.feature_ordinal.get()
        {
            return None;
        }
        Some(Ok(DesignBaseFeatureConstruction::BodyBasedOnFaces {
            body: crate::records::identity::Located {
                value: body_entity_suffix,
                offset: scope.byte_offset() + u64::try_from(class_377::BODY_ENTITY_SUFFIX).ok()?,
            },
            parameter_body_record,
            parameter_body_record_offset: scope.byte_offset()
                + u64::try_from(class_377::PARAMETER_BODY_RECORD).ok()?,
            auxiliary_record,
            auxiliary_record_offset: scope.byte_offset()
                + u64::try_from(class_377::AUXILIARY_RECORD).ok()?,
            envelope_guid,
            envelope_guid_offset: scope.byte_offset()
                + u64::try_from(class_377::ENVELOPE_GUID).ok()?,
            tag_body_based_on_faces_offset: scope.byte_offset()
                + u64::try_from(class_377::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
        }))
    })()
    .transpose()
}

use crate::layout::base_feature_body_snapshot_body_entry as snapshot_entry;
use crate::layout::base_feature_body_snapshot_compact_preamble as snapshot_compact_preamble;
use crate::layout::base_feature_body_snapshot_expanded_preamble as snapshot_expanded_preamble;
use crate::layout::base_feature_body_snapshot_guid as snapshot_guid;
use crate::layout::base_feature_body_snapshot_linkage_tail as snapshot_tail;
use crate::layout::base_feature_body_snapshot_prefix as snapshot;
use crate::layout::base_feature_body_snapshot_scope_prefix as snapshot_scope;
use crate::layout::base_feature_compact_repeated_body_entry as compact_entry;
use crate::layout::base_feature_compact_result_body_count as compact_count;
use crate::layout::base_feature_legacy_444_zero_body as legacy_444_zero_body;
use crate::layout::base_feature_legacy_zero_body as legacy_zero_body;
use crate::layout::base_feature_result_body_entry as result_body_entry;
use crate::layout::base_feature_result_body_prefix as result_body;
use crate::layout::base_feature_typed_reference_zero_body as typed_zero_body;
use crate::layout::base_feature_wide_shifted_zero_body as shifted_zero_body;
use crate::layout::base_feature_wide_zero_body as wide_zero_body;
use crate::layout::timeline_body_owner_marked_prefix as owner_marked_prefix;
use crate::layout::timeline_body_owner_prefix as owner_prefix;
use crate::records::feature::scope;

fn read_base_feature_entry_at(
    bytes: &[u8],
    cursor: &mut usize,
) -> Option<DesignBaseFeatureEntry<u64>> {
    let at = *cursor;
    if bytes.get(at) != Some(&1) {
        return None;
    }
    let entry = DesignBaseFeatureEntry {
        value: View::u64_le_at(bytes, at.checked_add(result_body_entry::REFERENCE_VALUE)?)?,
        offset: u64::try_from(at.checked_add(result_body_entry::REFERENCE_VALUE)?).ok()?,
        field: local_reference_field(bytes.get(
            at.checked_add(result_body_entry::REFERENCE_FIELD)?
                ..at.checked_add(result_body_entry::LEN)?,
        )?)?,
    };
    *cursor = at.checked_add(result_body_entry::LEN)?;
    Some(entry)
}

/// Distance from the null GUID length field of the typed zero-body form to
/// the scope kind.
const TYPED_ZERO_BODY_GUID_COUNT_BEFORE_KIND: usize = 142;

/// The typed-reference zero-body form: the class-409 zero-body prefix with a
/// 51-byte typed shared-metadata reference, then zero bytes up to the null
/// GUID length field.
fn exact_typed_zero_body_base_feature(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<DesignBaseFeatureConstruction> {
    use crate::records::feature::base_feature::DesignBaseFeatureResults;

    let reference_at = start + typed_zero_body::SHARED_METADATA_MARKER;
    let metadata_record = crate::bytes::typed_local_reference_target(bytes, reference_at)?;
    let reference_end = start + typed_zero_body::LEN;
    let guid_count_at = usize::try_from(scope.kind_offset())
        .ok()?
        .checked_sub(TYPED_ZERO_BODY_GUID_COUNT_BEFORE_KIND)?;
    if bytes.get(start + typed_zero_body::ZERO_RUN_9..start + typed_zero_body::ZERO_BODY_MARKER)?
        != [0; 9]
        || bytes.get(start + typed_zero_body::ZERO_BODY_MARKER) != Some(&1)
        || bytes.get(
            start + typed_zero_body::ZERO_RUN_11..start + typed_zero_body::SHARED_METADATA_MARKER,
        )? != [0; 11]
        || !scope
            .reference_members()
            .values()
            .copied()
            .eq([metadata_record])
        || guid_count_at < reference_end
        || !bytes
            .get(reference_end..guid_count_at)?
            .iter()
            .all(|byte| *byte == 0)
    {
        return None;
    }
    Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
        metadata_record,
        metadata_record_offset: scope.byte_offset()
            + u64::try_from(typed_zero_body::SHARED_METADATA_RECORD).ok()?,
        metadata_field: bytes
            .get(start + typed_zero_body::SHARED_METADATA_FIELD..reference_end)?
            .to_vec()
            .into(),
    })
}

/// Paste object whose owned scope stores the pasted bodies.
const PASTE_BODIES_TYPE_GUID: &str = "BA3059E4-4B23-4D02-9D18-E7BB95C991F3";
/// Body-to-component object whose owned scope stores the moved bodies.
const BODY_TO_COMPONENT_TYPE_GUID: &str = "56225BC7-69B7-446A-9AEA-6AA25BC7806E";

/// A timeline object's reference to the Base Feature scope it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimelineBodyOwner {
    record: u32,
    body_count: usize,
    reference_offset: u64,
}

/// Index each scope named by a paste or body-to-component timeline object.
pub(super) fn timeline_body_owners<'a>(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    types: impl Iterator<Item = &'a crate::records::entity_header::SegmentType>,
) -> HashMap<u32, TimelineBodyOwner> {
    let mut owners = HashMap::new();
    for design_type in types.filter(|design_type| {
        [PASTE_BODIES_TYPE_GUID, BODY_TO_COMPONENT_TYPE_GUID]
            .iter()
            .any(|guid| guid.eq_ignore_ascii_case(design_type.type_guid.as_str()))
    }) {
        for &entity in design_type.entities.values() {
            let Some((scope, owner)) = u32::try_from(entity)
                .ok()
                .and_then(|record| parse_timeline_body_owner(bytes, records, record))
            else {
                continue;
            };
            owners.insert(scope, owner);
        }
    }
    owners
}

fn parse_timeline_body_owner(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    record: u32,
) -> Option<(u32, TimelineBodyOwner)> {
    let start = *records.offsets(record).first()?;
    let count_at = if bytes
        .get(start + owner_prefix::ZERO_RUN_10..start + owner_prefix::BODY_COUNT)?
        == [0; 10]
    {
        start + owner_prefix::BODY_COUNT
    } else if bytes
        .get(start + owner_marked_prefix::ZERO_RUN_9..start + owner_marked_prefix::MARKER)?
        == [0; 9]
        && bytes.get(start + owner_marked_prefix::MARKER) == Some(&1)
        && bytes
            .get(start + owner_marked_prefix::ZERO_U32..start + owner_marked_prefix::BODY_COUNT)?
            == [0; 4]
    {
        start + owner_marked_prefix::BODY_COUNT
    } else {
        return None;
    };
    let body_count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
    if body_count == 0 {
        return None;
    }
    let mut cursor = count_at + 4;
    for _ in 0..body_count {
        crate::bytes::take_reference(bytes, &mut cursor)?.into_local()?;
    }
    let reference_offset = u64::try_from(cursor.checked_add(1)?).ok()?;
    let (scope, _) = crate::bytes::take_reference(bytes, &mut cursor)?.into_local()?;
    Some((
        u32::try_from(scope).ok()?,
        TimelineBodyOwner {
            record,
            body_count,
            reference_offset,
        },
    ))
}

/// Read the parallel parameter-body and body-entity runs of an owned scope.
pub(super) fn exact_timeline_owned_base_feature(
    bytes: &[u8],
    scope: &DesignParameterScope,
    owner: TimelineBodyOwner,
) -> Option<DesignBaseFeatureConstruction> {
    if scope.kind() != scope::DesignFeatureKind::BaseFeature {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + result_body::ZERO_RUN_8..start + result_body::BODY_COUNT_MARKER)? != [0; 8]
        || bytes.get(start + result_body::BODY_COUNT_MARKER) != Some(&1)
        || usize::try_from(View::u32_le_at(
            bytes,
            start + result_body::COMBINED_BODY_REFERENCE_COUNT,
        )?)
        .ok()?
            != owner.body_count.checked_mul(2)?
    {
        return None;
    }
    let mut cursor = start + result_body::LEN;
    let mut run = |count: usize| -> Option<Vec<DesignBaseFeatureEntry<u64>>> {
        (0..count)
            .map(|_| read_base_feature_entry_at(bytes, &mut cursor))
            .collect()
    };
    let parameter_bodies = run(owner.body_count)?;
    let bodies = run(owner.body_count)?;
    Some(DesignBaseFeatureConstruction::TimelineOwnedBodies {
        parameter_bodies,
        bodies,
        owner_record: owner.record,
        owner_reference_offset: owner.reference_offset,
    })
}

/// Read the zero-body frame whose shared-metadata reference starts at offset 36.
fn exact_wide_zero_body_base_feature(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<DesignBaseFeatureConstruction> {
    use crate::records::feature::base_feature::DesignBaseFeatureResults;
    if scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset())
        || bytes
            .get(start + wide_zero_body::ZERO_RUN_9..start + wide_zero_body::ZERO_BODY_MARKER)?
            != [0; 9]
        || bytes.get(start + wide_zero_body::ZERO_BODY_MARKER) != Some(&1)
        || bytes.get(
            start + wide_zero_body::ZERO_RUN_15..start + wide_zero_body::SHARED_METADATA_MARKER,
        )? != [0; 15]
        || bytes.get(start + wide_zero_body::SHARED_METADATA_MARKER) != Some(&1)
        || bytes.get(
            start + wide_zero_body::ZERO_PADDING_8..start + wide_zero_body::GUID_CODE_UNIT_COUNT,
        )? != [0; 8]
        || View::u32_le_at(bytes, start + wide_zero_body::GUID_CODE_UNIT_COUNT)? != 36
    {
        return None;
    }
    let metadata_record = u32::try_from(View::u64_le_at(
        bytes,
        start + wide_zero_body::SHARED_METADATA_RECORD,
    )?)
    .ok()?;
    if !scope
        .reference_members()
        .values()
        .copied()
        .eq([metadata_record])
    {
        return None;
    }
    Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
        metadata_record,
        metadata_record_offset: scope.byte_offset()
            + u64::try_from(wide_zero_body::SHARED_METADATA_RECORD).ok()?,
        metadata_field: bytes
            .get(
                start + wide_zero_body::SHARED_METADATA_FIELD
                    ..start + wide_zero_body::ZERO_PADDING_8,
            )?
            .to_vec()
            .into(),
    })
}

fn exact_wide_shifted_zero_body_base_feature(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<DesignBaseFeatureConstruction> {
    use crate::records::feature::base_feature::DesignBaseFeatureResults;
    if scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset())
        || bytes.get(
            start + shifted_zero_body::ZERO_RUN_9..start + shifted_zero_body::ZERO_BODY_MARKER,
        )? != [0; 9]
        || bytes.get(start + shifted_zero_body::ZERO_BODY_MARKER) != Some(&1)
        || bytes.get(
            start + shifted_zero_body::ZERO_RUN_15
                ..start + shifted_zero_body::SHARED_METADATA_MARKER,
        )? != [0; 15]
        || bytes.get(start + shifted_zero_body::SHARED_METADATA_MARKER) != Some(&1)
        || bytes.get(
            start + shifted_zero_body::ZERO_PADDING_12
                ..start + shifted_zero_body::GUID_CODE_UNIT_COUNT,
        )? != [0; 12]
        || View::u32_le_at(bytes, start + shifted_zero_body::GUID_CODE_UNIT_COUNT)? != 36
    {
        return None;
    }
    let metadata_record = u32::try_from(View::u64_le_at(
        bytes,
        start + shifted_zero_body::SHARED_METADATA_RECORD,
    )?)
    .ok()?;
    if scope.reference_members().len() != 1
        || !scope
            .reference_members()
            .values()
            .copied()
            .eq([metadata_record])
    {
        return None;
    }
    if bytes
        .get(start + shifted_zero_body::ZERO_RUN_3..start + shifted_zero_body::REFERENCE_COUNT)?
        != [0; 3]
        || View::u32_le_at(bytes, start + shifted_zero_body::REFERENCE_COUNT)?
            != shifted_zero_body::REFERENCE_COUNT_VALUE
        || bytes.get(start + shifted_zero_body::SCOPE_REFERENCE_MARKER)
            != Some(&shifted_zero_body::SCOPE_REFERENCE_MARKER_VALUE)
        || View::u32_le_at(bytes, start + shifted_zero_body::SCOPE_REFERENCE_RECORD)?
            != metadata_record
        || bytes.get(
            start + shifted_zero_body::SCOPE_REFERENCE_FIELD
                ..start + shifted_zero_body::HISTORY_STATE,
        )? != [0; 6]
    {
        return None;
    }
    let history_state = View::u32_le_at(bytes, start + shifted_zero_body::HISTORY_STATE)?;
    if u64::from(history_state) != u64::try_from(scope.history_state_id()?).ok()? {
        return None;
    }
    let kind_units = View::u32_le_at(bytes, start + shifted_zero_body::KIND_LENGTH)?;
    let ordinal_offset = start
        .checked_add(shifted_zero_body::KIND_LENGTH)?
        .checked_add(4)?
        .checked_add(usize::try_from(kind_units.checked_mul(2)?).ok()?)?;
    if View::u32_le_at(bytes, ordinal_offset)? != scope.feature_ordinal.get() {
        return None;
    }
    Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
        metadata_record,
        metadata_record_offset: scope.byte_offset()
            + u64::try_from(shifted_zero_body::SHARED_METADATA_RECORD).ok()?,
        metadata_field: bytes
            .get(
                start + shifted_zero_body::SHARED_METADATA_FIELD
                    ..start + shifted_zero_body::ZERO_PADDING_12,
            )?
            .to_vec()
            .into(),
    })
}

pub(super) fn exact_base_feature_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    use crate::records::feature::base_feature::{
        DesignBaseFeatureEntry, DesignBaseFeatureResultBody, DesignBaseFeatureResults,
    };
    if scope.kind() != scope::DesignFeatureKind::BaseFeature {
        return Ok(None);
    }
    if let Some(snapshot) = exact_base_feature_body_snapshot(bytes, scope)? {
        return Ok(Some(snapshot));
    }
    if let Some(value) = exact_base_feature_legacy_body_based_on_faces(bytes, scope)? {
        return Ok(Some(value));
    }
    if let Some(value) = exact_base_feature_direct_body_based_on_faces(bytes, scope)? {
        return Ok(Some(value));
    }
    (|| {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        if let Some(construction) = exact_typed_zero_body_base_feature(bytes, scope, start) {
            return Some(Ok(construction));
        }
        if scope.frame_length() == 267 {
            return Some(Ok(DesignBaseFeatureConstruction::ResultBodies {
                bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
                metadata_record: View::u32_le_at(
                    bytes,
                    usize::try_from(scope.byte_offset()).ok()? + 37,
                )?,
                metadata_record_offset: scope.byte_offset() + 37,
                metadata_field: bytes.get(start + 45..start + 51)?.to_vec().into(),
            }));
        }
        let legacy_290_261 =
            scope.class_tag.as_str() == "290" && scope.paired_class_tag.as_str() == "261";
        let legacy_360_258 =
            scope.class_tag.as_str() == "360" && scope.paired_class_tag.as_str() == "258";
        let legacy_444_263 =
            scope.class_tag.as_str() == "444" && scope.paired_class_tag.as_str() == "263";
        if !legacy_444_263 && scope.frame_length() == 258 {
            if scope.byte_offset().checked_add(scope.frame_length())
                != Some(scope.paired_byte_offset())
            {
                return None;
            }
            let metadata_record = u32::try_from(View::u64_le_at(
                bytes,
                start + legacy_zero_body::SHARED_METADATA_RECORD,
            )?)
            .ok()?;
            if bytes.get(
                start + legacy_zero_body::ZERO_RUN_9..start + legacy_zero_body::ZERO_BODY_MARKER,
            )? != [0; 9]
                || bytes.get(start + legacy_zero_body::ZERO_BODY_MARKER) != Some(&1)
                || bytes.get(
                    start + legacy_zero_body::ZERO_RUN_11
                        ..start + legacy_zero_body::SHARED_METADATA_MARKER,
                )? != [0; 11]
                || bytes.get(start + legacy_zero_body::SHARED_METADATA_MARKER) != Some(&1)
                || !scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([metadata_record])
            {
                return None;
            }
            let uuid_offset = usize::try_from(scope.kind_offset())
                .ok()?
                .checked_sub(102)?;
            if uuid_offset < start + legacy_zero_body::ZERO_PADDING_8 {
                return None;
            }
            if !bytes
                .get(start + legacy_zero_body::ZERO_PADDING_8..uuid_offset)?
                .iter()
                .all(|byte| *byte == 0)
            {
                return None;
            }
            return Some(Ok(DesignBaseFeatureConstruction::ResultBodies {
                bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
                metadata_record,
                metadata_record_offset: scope.byte_offset()
                    + u64::try_from(legacy_zero_body::SHARED_METADATA_RECORD).ok()?,
                metadata_field: bytes
                    .get(
                        start + legacy_zero_body::SHARED_METADATA_FIELD
                            ..start + legacy_zero_body::ZERO_PADDING_8,
                    )?
                    .to_vec()
                    .into(),
            }));
        }
        if let Some(construction) = exact_wide_zero_body_base_feature(bytes, scope, start) {
            return Some(Ok(construction));
        }
        if let Some(construction) = exact_wide_shifted_zero_body_base_feature(bytes, scope, start) {
            return Some(Ok(construction));
        }
        if legacy_444_263 && scope.frame_length() == 258 {
            if scope.byte_offset().checked_add(scope.frame_length())
                != Some(scope.paired_byte_offset())
                || scope.reference_members().len() != 1
                || scope.reference_count_offset()
                    != scope.byte_offset()
                        + u64::try_from(legacy_444_zero_body::REFERENCE_COUNT).ok()?
                || !scope
                    .reference_members()
                    .offsets()
                    .copied()
                    .eq([scope.byte_offset()
                        + u64::try_from(legacy_444_zero_body::SCOPE_REFERENCE_RECORD).ok()?])
                || scope.kind_offset()
                    != scope.byte_offset()
                        + u64::try_from(legacy_444_zero_body::KIND_LENGTH + 4).ok()?
            {
                return None;
            }
            let metadata_record = u32::try_from(View::u64_le_at(
                bytes,
                start + legacy_444_zero_body::SHARED_METADATA_RECORD,
            )?)
            .ok()?;
            let guid_end =
                fixed_guid_end(bytes, start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT)?;
            if bytes.get(
                start + legacy_444_zero_body::ZERO_RUN_9
                    ..start + legacy_444_zero_body::ZERO_BODY_MARKER,
            )? != [0; 9]
                || bytes.get(start + legacy_444_zero_body::ZERO_BODY_MARKER)
                    != Some(&legacy_444_zero_body::ZERO_BODY_MARKER_VALUE)
                || bytes.get(
                    start + legacy_444_zero_body::ZERO_RUN_11
                        ..start + legacy_444_zero_body::SHARED_METADATA_MARKER,
                )? != [0; 11]
                || bytes.get(start + legacy_444_zero_body::SHARED_METADATA_MARKER)
                    != Some(&legacy_444_zero_body::SHARED_METADATA_MARKER_VALUE)
                || !scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([metadata_record])
                || bytes.get(
                    start + legacy_444_zero_body::SHARED_METADATA_ZERO_TAIL
                        ..start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
                )? != [0; 14]
                || guid_end != start + legacy_444_zero_body::ZERO_RUN_3
                || bytes.get(
                    start + legacy_444_zero_body::ZERO_RUN_3
                        ..start + legacy_444_zero_body::REFERENCE_COUNT,
                )? != [0; 3]
                || View::u32_le_at(bytes, start + legacy_444_zero_body::REFERENCE_COUNT)?
                    != legacy_444_zero_body::REFERENCE_COUNT_VALUE
                || bytes.get(start + legacy_444_zero_body::SCOPE_REFERENCE_MARKER)
                    != Some(&legacy_444_zero_body::SCOPE_REFERENCE_MARKER_VALUE)
                || View::u32_le_at(bytes, start + legacy_444_zero_body::SCOPE_REFERENCE_RECORD)?
                    != metadata_record
                || bytes.get(
                    start + legacy_444_zero_body::SCOPE_REFERENCE_FIELD
                        ..start + legacy_444_zero_body::HISTORY_STATE_ID,
                )? != [0; 6]
                || View::u32_le_at(bytes, start + legacy_444_zero_body::KIND_LENGTH)?
                    != legacy_444_zero_body::KIND_LENGTH_VALUE
            {
                return None;
            }
            return Some(Ok(DesignBaseFeatureConstruction::ResultBodies {
                bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
                metadata_record,
                metadata_record_offset: scope.byte_offset()
                    + u64::try_from(legacy_444_zero_body::SHARED_METADATA_RECORD).ok()?,
                metadata_field: bytes
                    .get(
                        start + legacy_444_zero_body::SHARED_METADATA_ZERO_TAIL
                            ..start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
                    )?
                    .to_vec()
                    .into(),
            }));
        }
        if bytes.get(start + result_body::ZERO_RUN_8..start + result_body::BODY_COUNT_MARKER)?
            != [0; 8]
            || bytes.get(start + result_body::BODY_COUNT_MARKER) != Some(&1)
        {
            return None;
        }
        let combined_count = usize::try_from(View::u32_le_at(
            bytes,
            start + result_body::COMBINED_BODY_REFERENCE_COUNT,
        )?)
        .ok()?;
        if combined_count == 0 || combined_count > 200_000 || combined_count % 2 != 0 {
            return None;
        }
        let body_count = combined_count / 2;
        let expanded = legacy_290_261
            || legacy_360_258
            || matches!(
                (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
                ("384", "264") | ("409", "262")
            );
        let compact = matches!(
            (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
            ("420", "258") | ("452", "266")
        );
        let body_run_length = u64::try_from(body_count.checked_mul(52)?).ok()?;
        let base_length = if legacy_290_261 {
            Some(261)
        } else if expanded || compact || legacy_444_263 {
            Some(262)
        } else {
            None
        };
        // The ordinary form is measured to its scope kind, which the fixed
        // scope tail of either length follows.
        let sized = match base_length {
            Some(base_length) => scope.frame_length() == base_length + body_run_length,
            None => {
                scope.kind_offset().checked_sub(scope.byte_offset()) == Some(169 + body_run_length)
            }
        };
        if !sized {
            return None;
        }
        let mut cursor = start + result_body::LEN;
        let mut entities = Vec::new();
        for _ in 0..body_count {
            entities.push(read_base_feature_entry_at(bytes, &mut cursor)?);
        }
        let mut references = Vec::new();
        for _ in 0..body_count {
            let entry = read_base_feature_entry_at(bytes, &mut cursor)?;
            references.push(DesignBaseFeatureEntry {
                value: u32::try_from(entry.value).ok()?,
                offset: entry.offset,
                field: entry.field,
            });
        }
        if expanded {
            if bytes.get(cursor) != Some(&1)
                || bytes.get(cursor + 1..cursor + 7) != Some(&[0; 6])
                || usize::try_from(View::u32_le_at(bytes, cursor + 7)?).ok()? != body_count
            {
                return None;
            }
            cursor += 11;
        } else if legacy_444_263 {
            if bytes.get(cursor + compact_count::COUNT_MARKER) != Some(&1)
                || bytes
                    .get(cursor + compact_count::ZERO_RUN_5..cursor + compact_count::REPEAT_MARKER)
                    != Some(&[0; 5])
                || bytes.get(cursor + compact_count::REPEAT_MARKER) != Some(&0)
                || usize::try_from(View::u32_le_at(bytes, cursor + compact_count::BODY_COUNT)?)
                    .ok()?
                    != body_count
            {
                return None;
            }
            cursor += compact_count::LEN;
        } else if compact {
            if bytes.get(cursor + compact_count::COUNT_MARKER) != Some(&1)
                || bytes
                    .get(cursor + compact_count::ZERO_RUN_5..cursor + compact_count::REPEAT_MARKER)
                    != Some(&[0; 5])
                || bytes.get(cursor + compact_count::REPEAT_MARKER) != Some(&1)
                || usize::try_from(View::u32_le_at(bytes, cursor + compact_count::BODY_COUNT)?)
                    .ok()?
                    != body_count
            {
                return None;
            }
            cursor += compact_count::LEN;
        } else {
            if bytes.get(cursor) != Some(&1) || bytes.get(cursor + 1..cursor + 11) != Some(&[0; 10])
            {
                return None;
            }
            cursor += 11;
            if usize::try_from(View::u32_le_at(bytes, cursor)?).ok()? != body_count {
                return None;
            }
            cursor += 4;
        }
        let mut repeated_reference_fields = Vec::new();
        for ordinal in 0..body_count {
            let expected = if compact {
                u32::try_from(entities[ordinal].value).ok()?
            } else {
                references[ordinal].value
            };
            if bytes.get(cursor + compact_entry::BODY_MARKER) != Some(&1)
                || View::u32_le_at(bytes, cursor + compact_entry::BODY_ENTITY_SUFFIX)? != expected
            {
                return None;
            }
            repeated_reference_fields.push(
                bytes
                    .get(cursor + compact_entry::BODY_FIELD..cursor + compact_entry::LEN)?
                    .try_into()
                    .ok()?,
            );
            cursor += compact_entry::LEN;
        }
        if bytes.get(cursor) != Some(&0) {
            return None;
        }
        cursor += 1;
        if bytes.get(cursor) != Some(&1) {
            return None;
        }
        let metadata_record = u32::try_from(View::u64_le_at(bytes, cursor + 1)?).ok()?;
        let metadata_record_offset = u64::try_from(cursor + 1).ok()?;
        let metadata_field_width = if expanded || compact || legacy_444_263 {
            2
        } else {
            6
        };
        let metadata_field = bytes
            .get(cursor + 9..cursor + 9 + metadata_field_width)?
            .to_vec();
        cursor += 9 + metadata_field_width;
        if usize::try_from(View::u32_le_at(bytes, cursor)?).ok()? != body_count {
            return None;
        }
        cursor += 4;
        let mut first = None;
        let mut rest = Vec::new();
        for ((entity, reference), field) in entities
            .into_iter()
            .zip(references)
            .zip(repeated_reference_fields)
        {
            if bytes.get(cursor) != Some(&1) {
                return None;
            }
            let result = DesignBaseFeatureEntry {
                value: View::u32_le_at(bytes, cursor + 1)?,
                offset: u64::try_from(cursor + 1).ok()?,
                field: bytes.get(cursor + 5..cursor + 11)?.try_into().ok()?,
            };
            let row = (
                DesignBaseFeatureResultBody {
                    entity,
                    reference,
                    result,
                },
                field,
            );
            if first.is_none() {
                first = Some(row);
            } else {
                rest.push(row);
            }
            cursor += 11;
        }
        let uuid_offset = usize::try_from(scope.kind_offset())
            .ok()?
            .checked_sub(102)?;
        let admitted = cursor <= uuid_offset
            && bytes
                .get(cursor..uuid_offset)
                .is_some_and(|padding| padding.iter().all(|byte| *byte == 0));
        let first = first?;
        admitted.then_some(Ok(DesignBaseFeatureConstruction::ResultBodies {
            bodies: DesignBaseFeatureResults::WithRepeatedFields { first, rest },
            metadata_record,
            metadata_record_offset,
            metadata_field: metadata_field.into(),
        }))
    })()
    .transpose()
}

fn exact_base_feature_body_snapshot(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    (|| {
        // Fixed prefix, linkage and GUID blocks, generic scope prefix, kind
        // prefix, ordinal, and closing tail; the kind payload adds 2L bytes.
        const FIXED_FRAME_LENGTH: u64 = 431;
        if scope.class_tag.as_str() != "314"
            || scope.paired_class_tag.as_str() != "259"
            || scope.reference_members().len() != 1
        {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let body_count =
            usize::try_from(View::u32_le_at(bytes, start + snapshot::BODY_COUNT)?).ok()?;
        let kind_width = scope
            .source_kind_name()
            .encode_utf16()
            .count()
            .checked_mul(2)?;
        let expected_frame_length = FIXED_FRAME_LENGTH
            .checked_add(u64::try_from(body_count.checked_mul(snapshot_entry::LEN)?).ok()?)?
            .checked_add(u64::try_from(kind_width).ok()?)?;
        if !(1..=200_000).contains(&body_count)
            || scope.frame_length() != expected_frame_length
            || bytes.get(start + snapshot::ZERO_RUN_8..start + snapshot::BODY_COUNT_MARKER)?
                != [0; 8]
            || bytes.get(start + snapshot::BODY_COUNT_MARKER) != Some(&1)
        {
            return None;
        }
        let mut cursor = start + snapshot::LEN;
        let mut bodies = Vec::new();
        for _ in 0..body_count {
            if bytes.get(cursor) != Some(&1) {
                return None;
            }
            bodies.push(
                crate::records::feature::base_feature::DesignBaseFeatureEntry {
                    value: View::u64_le_at(bytes, cursor + snapshot_entry::BODY_ENTITY_SUFFIX)?,
                    offset: u64::try_from(cursor + snapshot_entry::BODY_ENTITY_SUFFIX).ok()?,
                    field: local_reference_field(bytes.get(
                        cursor + snapshot_entry::BODY_ENTITY_FIELD..cursor + snapshot_entry::LEN,
                    )?)?,
                },
            );
            cursor += snapshot_entry::LEN;
        }
        let preamble = bytes.get(cursor..cursor + snapshot_expanded_preamble::LEN)?;
        let packed_guid_preamble = if preamble == [1, 0, 0, 0, 0, 1, 0, 0, 0] {
            true
        } else if preamble[..snapshot_compact_preamble::LEN] == [1, 0, 0, 0, 1, 0, 0, 0] {
            false
        } else {
            return None;
        };
        cursor += if packed_guid_preamble {
            snapshot_expanded_preamble::LEN
        } else {
            snapshot_compact_preamble::LEN
        };
        let first_guid_offset = cursor + snapshot_guid::GUID_UTF16;
        let (first_guid, after_first_guid) = match fixed_relaxed_guid_text(bytes, cursor) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let second_guid_offset = after_first_guid + snapshot_guid::GUID_UTF16;
        let (second_guid, after_second_guid) =
            match fixed_relaxed_guid_text(bytes, after_first_guid) {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        // The nine-byte preamble carries the linkage anchor in the final zero
        // byte of the second GUID's UTF-16 payload. Keep the full GUID for the
        // native record, but anchor the fixed tail at that shared byte.
        let after_guids = if packed_guid_preamble {
            after_second_guid.checked_sub(1)?
        } else {
            after_second_guid
        };
        if bytes.get(after_guids..after_guids + snapshot_tail::FIRST_BODY_MARKER)?
            != [0, 0, 1, 1, 0, 0, 0]
            || bytes.get(after_guids + snapshot_tail::FIRST_BODY_MARKER) != Some(&1)
            || View::u64_le_at(bytes, after_guids + snapshot_tail::FIRST_BODY_ENTITY_SUFFIX)?
                != bodies.first()?.value
            || bytes.get(
                after_guids + snapshot_tail::ZERO_RUN_3
                    ..after_guids + snapshot_tail::LINKAGE_MARKER,
            )? != [0; 3]
            || bytes.get(after_guids + snapshot_tail::LINKAGE_MARKER) != Some(&1)
        {
            return None;
        }
        let linkage_record = u32::try_from(View::u64_le_at(
            bytes,
            after_guids + snapshot_tail::LINKAGE_RECORD,
        )?)
        .ok()?;
        if linkage_record != *scope.reference_members().values().next()?
            || bytes.get(
                after_guids + snapshot_tail::ZERO_RUN_6
                    ..after_guids + snapshot_tail::RELATION_COUNT,
            )? != [0; 6]
            || View::u32_le_at(bytes, after_guids + snapshot_tail::RELATION_COUNT)? != 1
            || bytes.get(after_guids + snapshot_tail::AUXILIARY_MARKER) != Some(&1)
        {
            return None;
        }
        let auxiliary_record = u32::try_from(View::u64_le_at(
            bytes,
            after_guids + snapshot_tail::AUXILIARY_RECORD,
        )?)
        .ok()?;
        if bytes.get(
            after_guids + snapshot_tail::TRAILING_ZERO_RUN_6
                ..after_guids + snapshot_tail::TRAILING_ZERO_RUN_4,
        )? != [0; 6]
            || bytes.get(
                after_guids + snapshot_tail::TRAILING_ZERO_RUN_4..after_guids + snapshot_tail::LEN,
            )? != [0; 4]
        {
            return None;
        }
        let third_guid_at = after_guids + snapshot_tail::LEN;
        let third_guid_offset = third_guid_at + snapshot_guid::GUID_UTF16;
        let (third_guid, after_third_guid) = match fixed_relaxed_guid_text(bytes, third_guid_at) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let reference_count_at = after_third_guid.checked_add(snapshot_scope::REFERENCE_COUNT)?;
        let reference_marker = after_third_guid.checked_add(snapshot_scope::REFERENCE_MARKER)?;
        let state_at = after_third_guid.checked_add(snapshot_scope::HISTORY_STATE_ID)?;
        let kind_at = after_third_guid.checked_add(snapshot_scope::KIND_CODE_UNIT_COUNT)?;
        if bytes.get(after_third_guid..reference_count_at)? != [0; 3]
            || View::u32_le_at(bytes, reference_count_at)? != 1
            || bytes.get(reference_marker) != Some(&1)
            || View::u32_le_at(bytes, reference_marker + 1)?
                != *scope.reference_members().values().next()?
            || bytes.get(reference_marker + 5..state_at)? != [0; 6]
            || scope.reference_count_offset() != u64::try_from(reference_count_at).ok()?
            || scope.reference_members().offsets().next().copied()
                != Some(u64::try_from(reference_marker + 1).ok()?)
            || scope.kind_offset() != u64::try_from(kind_at + 4).ok()?
        {
            return None;
        }
        match scope.history_state_id() {
            Some(history_state_id)
                if View::u32_le_at(bytes, state_at)? != u32::try_from(history_state_id).ok()? =>
            {
                return None;
            }
            None if View::u32_le_at(bytes, state_at)? != u32::MAX => return None,
            _ => {}
        }
        let kind_count = usize::try_from(View::u32_le_at(bytes, kind_at)?).ok()?;
        if !(1..=256).contains(&kind_count) {
            return None;
        }
        let kind_start = kind_at.checked_add(4)?;
        let kind_end = kind_start.checked_add(kind_count.checked_mul(2)?)?;
        let mut kind_view = View::over_retained(bytes.get(kind_start..kind_end)?);
        if !std::iter::from_fn(|| kind_view.u16_le()).eq(scope.source_kind_name().encode_utf16())
            || View::u32_le_at(bytes, kind_end)? != scope.feature_ordinal.get()
            || scope.feature_ordinal_offset() != u64::try_from(kind_end).ok()?
            || scope.previous_history_state_id().is_some()
            || scope.previous_history_state_id_offset().is_some()
        {
            return None;
        }
        if scope.paired_byte_offset()
            != u64::try_from(start.checked_add(usize::try_from(scope.frame_length()).ok()?)?)
                .ok()?
        {
            return None;
        }
        Some(Ok(DesignBaseFeatureConstruction::BodySnapshot {
            bodies,
            related_guids: [first_guid, second_guid, third_guid],
            related_guid_offsets: [
                u64::try_from(first_guid_offset).ok()?,
                u64::try_from(second_guid_offset).ok()?,
                u64::try_from(third_guid_offset).ok()?,
            ],
            linkage_record,
            linkage_record_offset: u64::try_from(after_guids + snapshot_tail::LINKAGE_RECORD)
                .ok()?,
            auxiliary_record,
            auxiliary_record_offset: u64::try_from(after_guids + snapshot_tail::AUXILIARY_RECORD)
                .ok()?,
        }))
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
