// SPDX-License-Identifier: Apache-2.0
//! Exact derived-instance, component-insert, copy-paste-component and component-pattern occurrence scopes.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;

use crate::design::decode::text::relaxed_guid_end;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::ids::native_stream;
use crate::layout::component_insert_carrier_334_prefix as component_carrier_334;
use crate::layout::component_insert_identity_scope_272_prefix as identity_insert;
use crate::layout::component_insert_identity_scope_compact as component_identity_scope;
use crate::layout::component_insert_identity_scope_shifted_prefix as component_identity_shifted;
use crate::layout::component_insert_matrix_scope_414_264_prefix as component_matrix_414;
use crate::layout::component_insert_relation_345_57 as component_insert_relation_345;
use crate::layout::component_insert_relation_child_393_58 as component_insert_relation_child_393;
use crate::layout::component_insert_scope_283_262_257 as component_scope_283_257;
use crate::layout::component_insert_scope_283_262_385 as component_scope_283_385;
use crate::layout::derived_instance_relation_310_57 as derived_instance_relation_310;
use crate::layout::derived_instance_scope_279_261 as derived_instance_279_261;
use crate::records::feature::assembly_features;
use crate::records::feature::assembly_features::DesignComponentInsertConstruction;
use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::records::feature::assembly_features::DesignCopyPasteComponentOperation;
use crate::records::feature::assembly_features::DesignDerivedInstanceConstruction;
use crate::records::feature::patterns;
use crate::records::feature::patterns::DesignRectangularPatternInstances;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

pub(super) fn exact_derived_instance_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignDerivedInstanceConstruction>, CodecError> {
    if !matches!(
        scope.kind(),
        scope::DesignFeatureKind::DerivedInstance | scope::DesignFeatureKind::CopyPaste
    ) {
        return Ok(None);
    }
    let parsed = (|| {
        if scope.kind() != scope::DesignFeatureKind::DerivedInstance
            || scope.class_tag.as_str() != "279"
            || scope.paired_class_tag.as_str() != "261"
            || scope.frame_length() != u64_from_index(derived_instance_279_261::LEN)
            || scope.reference_members().len() != 1
        {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        if bytes.get(
            start + derived_instance_279_261::REFERENCE_MARKER
                ..start + derived_instance_279_261::REFERENCE_RECORD_INDEX,
        )? != [derived_instance_279_261::REFERENCE_MARKER_VALUE]
            || bytes.get(
                start + derived_instance_279_261::REFERENCE_RECORD_INDEX + 4
                    ..start + derived_instance_279_261::REFERENCE_COUNT,
            )? != [0; 6]
            || View::u32_le_at(bytes, start + derived_instance_279_261::REFERENCE_COUNT)?
                != derived_instance_279_261::REFERENCE_COUNT_VALUE
            || marked_record_reference(bytes, start + derived_instance_279_261::RELATION_REFERENCE)?
                != *scope.reference_members().values().next()?
            || bytes.get(start + derived_instance_279_261::RELATION_REFERENCE + 11) != Some(&0)
        {
            return None;
        }
        let reference_record_index = View::u32_le_at(
            bytes,
            start + derived_instance_279_261::REFERENCE_RECORD_INDEX,
        )?;
        let transform_offset = start + derived_instance_279_261::TRANSFORM;
        let transform = rigid_transform_at(bytes, transform_offset)?;

        let relation_record_index = *scope.reference_members().values().next()?;
        let relation_at = records.first_at_or_after(0, relation_record_index)?;
        let (relation_kind, _) =
            lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_graphic)?;
        if relation_at >= start
            || relation_kind != "310"
            || next_indexed_record_offset(bytes, relation_at + 1)?
                != relation_at + derived_instance_relation_310::LEN
            || bytes.get(
                relation_at + derived_instance_relation_310::INDEXED_HEADER + 11
                    ..relation_at + derived_instance_relation_310::CARRIER_MARKER,
            )? != [0; 10]
            || bytes.get(relation_at + derived_instance_relation_310::CARRIER_MARKER)
                != Some(&derived_instance_relation_310::CARRIER_MARKER_VALUE)
            || bytes.get(
                relation_at + derived_instance_relation_310::CARRIER_RECORD_INDEX + 4
                    ..relation_at + derived_instance_relation_310::MIDDLE_MARKER,
            )? != [0; 8]
            || bytes.get(relation_at + derived_instance_relation_310::MIDDLE_MARKER)
                != Some(&derived_instance_relation_310::MIDDLE_MARKER_VALUE)
            || bytes.get(
                relation_at + derived_instance_relation_310::MIDDLE_RECORD_INDEX + 4
                    ..relation_at + derived_instance_relation_310::SCOPE_MARKER,
            )? != [0; 7]
            || bytes.get(relation_at + derived_instance_relation_310::SCOPE_MARKER)
                != Some(&derived_instance_relation_310::SCOPE_MARKER_VALUE)
            || View::u32_le_at(
                bytes,
                relation_at + derived_instance_relation_310::SCOPE_RECORD_INDEX,
            )? != scope.record_index
            || bytes.get(
                relation_at + derived_instance_relation_310::SCOPE_RECORD_INDEX + 4
                    ..relation_at + derived_instance_relation_310::LEN,
            )? != [0; 6]
        {
            return None;
        }
        let carrier_record_index = View::u32_le_at(
            bytes,
            relation_at + derived_instance_relation_310::CARRIER_RECORD_INDEX,
        )?;
        let stream = native_stream(&scope.id)?;
        let mut candidates = occurrences.iter().filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.class_tag.as_str() == "380"
                && occurrence.record_index == carrier_record_index
                && occurrence.byte_offset() < u64_from_index(relation_at)
                && occurrence.transform().map(|frame| frame.value) == Some(transform)
        });
        let carrier = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let transform_offset = u64::try_from(transform_offset).ok()?;
        Some(Ok(DesignDerivedInstanceConstruction {
            reference_record_index,
            relation_record_index,
            carrier_record_index,
            component_guid: carrier.component_guid.clone(),
            occurrence_guid: carrier.occurrence_guid.clone(),
            transform,
            transform_offset,
        }))
    })();
    parsed.transpose()
}

pub(super) fn exact_component_insert_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignComponentInsertConstruction>, CodecError> {
    let parsed = (|| {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let relation_record_index = *scope.reference_members().values().next()?;
        if scope.kind() != scope::DesignFeatureKind::ComponentInsert
            || scope.reference_members().len() != 1
        {
            return None;
        }
        let (transform, transform_at, occurrence_identity) =
            match (scope.frame_length(), scope.paired_class_tag.as_str()) {
                (399, "259")
                    if bytes.get(start + 11..start + 20)? == [0; 9]
                        && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                        && bytes.get(start + 33..start + 37)? == [0; 4]
                        && bytes.get(start + 37) == Some(&1)
                        && View::u32_le_at(bytes, start + 38)? == relation_record_index
                        && bytes.get(start + 42..start + 50)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
                {
                    let transform_at = start + 50;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 25)?,
                    )
                }
                (381, "261")
                    if bytes.get(start + 11..start + 20)? == [0; 9]
                        && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                        && bytes.get(start + 33..start + 37)? == [0; 4]
                        && bytes.get(start + 37) == Some(&1)
                        && View::u32_le_at(bytes, start + 38)? == relation_record_index
                        && bytes.get(start + 42..start + 49)? == [0, 0, 0, 0, 0, 0, 1] =>
                {
                    let transform_at = start + 49;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 25)?,
                    )
                }
                (395, "258")
                    if bytes.get(start + 11..start + 21)? == [0; 10]
                        && bytes.get(start + 29..start + 33)? == [0; 4]
                        && bytes.get(start + 33) == Some(&1)
                        && View::u32_le_at(bytes, start + 34)? == relation_record_index
                        && bytes.get(start + 38..start + 46)? == [0, 0, 0, 0, 0, 0, 1, 0] =>
                {
                    let transform_at = start + 46;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 21)?,
                    )
                }
                (404, _)
                    if bytes.get(start + 11..start + 20)? == [0; 9]
                        && bytes.get(start + 20..start + 25)? == [1, 0, 0, 0, 0]
                        && bytes.get(start + 25..start + 29)? == [0; 4]
                        && bytes.get(start + 37..start + 41)? == [0; 4]
                        && bytes.get(start + 41) == Some(&1)
                        && View::u32_le_at(bytes, start + 42)? == relation_record_index
                        && bytes.get(start + 46..start + 52)? == [0; 6]
                        && bytes.get(start + 52..start + 54)? == [1, 0] =>
                {
                    let transform_at = start + 54;
                    (
                        rigid_transform_at(bytes, transform_at)?,
                        Some(transform_at),
                        View::u64_le_at(bytes, start + 29)?,
                    )
                }
                (272, "262") if scope.class_tag.as_str() == "357" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_identity_insert_scope(bytes, start, relation_record_index, false)?,
                ),
                (276, "262") if matches!(scope.class_tag.as_str(), "357" | "360" | "362") => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_identity_insert_scope(bytes, start, relation_record_index, true)?,
                ),
                (261, "263") if scope.class_tag.as_str() == "296" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
                ),
                (261, "261") if scope.class_tag.as_str() == "410" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
                ),
                (261, "258") if scope.class_tag.as_str() == "426" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
                ),
                (261, "266") if scope.class_tag.as_str() == "434" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
                ),
                (261, "264") if scope.class_tag.as_str() == "414" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_component_insert_identity_scope(bytes, start, relation_record_index)?,
                ),
                (257 | 267, "264") if scope.class_tag.as_str() == "414" => (
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                    None,
                    exact_component_insert_identity_scope_shifted(
                        bytes,
                        start,
                        relation_record_index,
                    )?,
                ),
                (389, "264") if scope.class_tag.as_str() == "414" => {
                    exact_component_insert_scope_414_264_389(bytes, start, relation_record_index)?
                }
                (257, "262") if scope.class_tag.as_str() == "283" => {
                    exact_component_insert_scope_283_262_257(bytes, start, relation_record_index)?
                }
                (385, "262") if scope.class_tag.as_str() == "283" => {
                    exact_component_insert_scope_283_262_385(bytes, start, relation_record_index)?
                }
                _ => return None,
            };
        let relation_at = records.first_at_or_after(0, relation_record_index)?;
        let (carrier_record_index, placements) = if matches!(scope.frame_length(), 272 | 276 | 404)
        {
            if relation_at >= start
                || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 58
                || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
                || bytes.get(relation_at + 21) != Some(&1)
                || bytes.get(relation_at + 26..relation_at + 32)? != [0; 6]
                || bytes.get(relation_at + 32..relation_at + 35)? != [1, 0, 0]
                || bytes.get(relation_at + 35) != Some(&1)
                || bytes.get(relation_at + 40..relation_at + 47)? != [0; 7]
                || bytes.get(relation_at + 47) != Some(&1)
                || View::u32_le_at(bytes, relation_at + 48)? != scope.record_index
                || bytes.get(relation_at + 52..relation_at + 58)? != [0; 6]
            {
                return None;
            }
            let carrier_record_index = View::u32_le_at(bytes, relation_at + 22)?;
            let mut placements = Vec::new();
            if scope.frame_length() != 404 {
                let carrier_at =
                    unique_indexed_record_before(records, carrier_record_index, relation_at)?;
                let (role, offset) = crate::xref::identity_component_insert_carrier(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                placements.push((role, offset, None));
            }
            for &carrier_at in records
                .offsets(carrier_record_index)
                .iter()
                .filter(|at| scope.frame_length() == 404 && **at < relation_at)
            {
                for at in carrier_at + 11..relation_at {
                    let Some(after_role) = fixed_guid_end(bytes, at) else {
                        continue;
                    };
                    if bytes.get(after_role..after_role + 12)
                        != Some(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                    {
                        continue;
                    }
                    for transform_at in carrier_at + 11..at {
                        if rigid_transform_at(bytes, transform_at) == Some(transform) {
                            let role = lp_utf16_bounded(bytes, at, 36..=36)?.0;
                            placements.push((role, at + 4, Some(transform_at)));
                        }
                    }
                }
            }
            (carrier_record_index, placements)
        } else if scope.class_tag.as_str() == "426" && scope.paired_class_tag.as_str() == "258" {
            let (carrier_record_index, role, role_offset) =
                exact_component_insert_class_426_relation(
                    bytes,
                    records,
                    relation_at,
                    start,
                    relation_record_index,
                    scope.record_index,
                )?;
            let role = role.to_string();
            (
                carrier_record_index,
                [(role, role_offset, None)].into_iter().collect::<Vec<_>>(),
            )
        } else {
            if relation_at >= start
                || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 57
                || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
                || bytes.get(relation_at + 21) != Some(&1)
                || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
                || bytes.get(relation_at + 34) != Some(&1)
                || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
                || bytes.get(relation_at + 46) != Some(&1)
                || View::u32_le_at(bytes, relation_at + 47)? != scope.record_index
                || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
            {
                return None;
            }
            let carrier_record_index = View::u32_le_at(bytes, relation_at + 22)?;
            let carrier_at =
                unique_indexed_record_before(records, carrier_record_index, relation_at)?;
            if scope.class_tag.as_str() == "283" && scope.paired_class_tag.as_str() == "262" {
                let (role, role_offset) = exact_component_insert_carrier_334(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                (
                    carrier_record_index,
                    [(role, role_offset, None)].into_iter().collect::<Vec<_>>(),
                )
            } else if scope.class_tag.as_str() == "296" && scope.paired_class_tag.as_str() == "263"
            {
                let (role, role_offset) = crate::xref::grouped_component_insert_identity(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                let role = role.to_string();
                (
                    carrier_record_index,
                    [(role, role_offset, None)].into_iter().collect::<Vec<_>>(),
                )
            } else if scope.class_tag.as_str() == "410" && scope.paired_class_tag.as_str() == "261"
            {
                let (role, role_offset) = crate::xref::grouped_component_insert_identity_class380(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                let role = role.to_string();
                (
                    carrier_record_index,
                    [(role, role_offset, None)].into_iter().collect::<Vec<_>>(),
                )
            } else if scope.class_tag.as_str() == "434" && scope.paired_class_tag.as_str() == "266"
            {
                let (role, role_offset) = crate::xref::grouped_component_insert_identity_class341(
                    bytes,
                    carrier_at,
                    relation_at,
                    carrier_record_index,
                )?;
                let role = role.to_string();
                (
                    carrier_record_index,
                    [(role, role_offset, None)].into_iter().collect::<Vec<_>>(),
                )
            } else if scope.class_tag.as_str() == "414" && scope.paired_class_tag.as_str() == "264"
            {
                let (role, role_offset, carrier_transform_offset) =
                    match crate::xref::repeated_target_component_insert(
                        bytes,
                        carrier_at,
                        relation_at,
                        carrier_record_index,
                        transform.into(),
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                (
                    carrier_record_index,
                    [(role, role_offset, carrier_transform_offset)]
                        .into_iter()
                        .collect::<Vec<_>>(),
                )
            } else {
                let mut placements = Vec::new();
                for at in carrier_at + 11..relation_at {
                    let Some(after_role) = relaxed_guid_end(bytes, at) else {
                        continue;
                    };
                    if bytes.get(after_role..after_role + 2) != Some(&[0, 0]) {
                        continue;
                    }
                    let transform_at = after_role.checked_add(2)?;
                    if rigid_transform_at(bytes, transform_at) == Some(transform) {
                        let role = lp_utf16_bounded(bytes, at, 36..=38)?.0;

                        placements.push((role, at + 4, Some(transform_at)));
                    }
                }
                if scope.frame_length() == 381 {
                    let legacy = legacy_component_insert_placements(
                        bytes,
                        carrier_at,
                        relation_at,
                        carrier_record_index,
                        transform,
                    );

                    placements.extend(legacy);
                }
                (carrier_record_index, placements)
            }
        };
        if placements.len() != 1 {
            return None;
        }
        let (neutron_role, neutron_role_offset, carrier_transform_offset) =
            placements.into_iter().next()?;
        Some(Ok(DesignComponentInsertConstruction {
            relation_record_index,
            carrier_record_index,
            occurrence_identity: Some(occurrence_identity),
            neutron_role,
            neutron_role_offset: u64::try_from(neutron_role_offset).ok()?,
            placement: match (transform_at, carrier_transform_offset) {
                (Some(offset), carrier_offset) => {
                    Some(assembly_features::DesignComponentInsertMatrix {
                        scope: crate::records::identity::Located {
                            value: transform,
                            offset: u64::try_from(offset).ok()?,
                        },
                        carrier_offset: carrier_offset.map(u64::try_from).transpose().ok()?,
                    })
                }
                (None, None) => None,
                (None, Some(_)) => return None,
            },
        }))
    })();
    parsed.transpose()
}

fn exact_component_insert_class_426_relation<'a>(
    bytes: &'a [u8],
    records: &IndexedRecordOffsets,
    relation_at: usize,
    scope_at: usize,
    relation_record_index: u32,
    scope_record_index: u32,
) -> Option<(u32, crate::bytes::utf16::Utf16View<'a>, usize)> {
    let relation_end = relation_at + component_insert_relation_345::LEN;
    let (relation_class, relation_after_tag) =
        lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_digit)?;
    if relation_class != "345"
        || relation_after_tag != relation_at + 7
        || View::u32_le_at(bytes, relation_after_tag)? != relation_record_index
        || relation_at >= scope_at
        || next_indexed_record_offset(bytes, relation_at + 1)? != relation_end
        || bytes.get(
            relation_at + component_insert_relation_345::INDEXED_HEADER + 11
                ..relation_at + component_insert_relation_345::FIRST_MARKER,
        )? != [0; 10]
        || bytes.get(relation_at + component_insert_relation_345::FIRST_MARKER)
            != Some(&component_insert_relation_345::FIRST_MARKER_VALUE)
        || bytes.get(
            relation_at + component_insert_relation_345::FIRST_CARRIER_RECORD_INDEX + 4
                ..relation_at + component_insert_relation_345::SECOND_MARKER,
        )? != [0; 8]
        || bytes.get(relation_at + component_insert_relation_345::SECOND_MARKER)
            != Some(&component_insert_relation_345::SECOND_MARKER_VALUE)
        || bytes.get(
            relation_at + component_insert_relation_345::SECOND_CHILD_RECORD_INDEX + 4
                ..relation_at + component_insert_relation_345::SCOPE_MARKER,
        )? != [0; 7]
        || bytes.get(relation_at + component_insert_relation_345::SCOPE_MARKER)
            != Some(&component_insert_relation_345::SCOPE_MARKER_VALUE)
        || View::u32_le_at(
            bytes,
            relation_at + component_insert_relation_345::SCOPE_RECORD_INDEX,
        )? != scope_record_index
        || bytes.get(
            relation_at + component_insert_relation_345::SCOPE_RECORD_INDEX + 4..relation_end,
        )? != [0; 6]
    {
        return None;
    }

    let paired_at = relation_end;
    let (paired_class, paired_after_tag) =
        lp_ascii_filtered_view(bytes, paired_at, 3..=3, u8::is_ascii_digit)?;
    if paired_class != "258"
        || paired_after_tag != paired_at + 7
        || View::u32_le_at(bytes, paired_after_tag)? != relation_record_index
    {
        return None;
    }

    let carrier_record_index = View::u32_le_at(
        bytes,
        relation_at + component_insert_relation_345::FIRST_CARRIER_RECORD_INDEX,
    )?;
    let child_record_index = View::u32_le_at(
        bytes,
        relation_at + component_insert_relation_345::SECOND_CHILD_RECORD_INDEX,
    )?;
    let child_at = records.first_at_or_after(paired_at + 11, child_record_index)?;
    let child_end = child_at + component_insert_relation_child_393::LEN;
    let (child_class, child_after_tag) =
        lp_ascii_filtered_view(bytes, child_at, 3..=3, u8::is_ascii_digit)?;
    if child_class != "393"
        || child_after_tag != child_at + 7
        || View::u32_le_at(bytes, child_after_tag)? != child_record_index
        || next_indexed_record_offset(bytes, paired_at + 1)? != child_at
        || next_indexed_record_offset(bytes, child_at + 1)? != child_end
        || child_end != scope_at
        || bytes
            .get(child_at + 11..child_at + component_insert_relation_child_393::RELATION_MARKER)?
            != [0; 20]
        || bytes.get(child_at + component_insert_relation_child_393::RELATION_MARKER)
            != Some(&component_insert_relation_child_393::RELATION_MARKER_VALUE)
        || View::u32_le_at(
            bytes,
            child_at + component_insert_relation_child_393::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(
            child_at + component_insert_relation_child_393::RELATION_RECORD_INDEX + 4
                ..child_at + component_insert_relation_child_393::OPAQUE_TOKEN,
        )? != [0; 6]
        || View::u64_le_at(
            bytes,
            child_at + component_insert_relation_child_393::OPAQUE_TOKEN,
        )
        .is_none()
        || bytes.get(child_at + component_insert_relation_child_393::OPAQUE_TOKEN + 8..child_end)?
            != [0; 8]
    {
        return None;
    }

    let carrier_at = unique_indexed_record_before(records, carrier_record_index, relation_at)?;
    let (role, role_offset) = crate::xref::grouped_component_insert_identity_class369(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
    )?;
    Some((carrier_record_index, role, role_offset))
}

fn exact_component_insert_carrier_334(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    let parsed = (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
        if class_tag != "334"
            || after_tag != carrier_at + 7
            || View::u32_le_at(bytes, after_tag)? != carrier_record_index
        {
            return None;
        }
        fixed_guid_end(
            bytes,
            carrier_at + component_carrier_334::COMPONENT_IDENTITY,
        )?;

        let role_start = carrier_at + component_carrier_334::NEUTRON_ROLE;
        Some(role_start)
    })();
    let role_start = parsed?;
    let (role, role_end) = direct_utf16_role_until_tail(bytes, role_start, relation_at)?;
    (|| {
        if !crate::bytes::is_guid_prefix(&role)
            || role.as_bytes().get(36) != Some(&b'_')
            || !role.get(37..)?.starts_with("urn:")
            || bytes.get(role_end)? != &0
            || bytes.get(role_end + 1)? == &0
            || bytes.get(role_end + 2..role_end + 6)? != [0; 4]
            || View::u32_le_at(bytes, role_end + 6)? == 0
        {
            return None;
        }
        fixed_guid_end(bytes, role_end + COMPONENT_CARRIER_ROLE_TAIL_BYTES)?;
        Some((role, role_start))
    })()
}

const COMPONENT_CARRIER_ROLE_TAIL_BYTES: usize = 10;

fn direct_utf16_role_until_tail(
    bytes: &[u8],
    start: usize,
    limit: usize,
) -> Option<(String, usize)> {
    let end = (|| {
        let mut at = start;
        while at.checked_add(COMPONENT_CARRIER_ROLE_TAIL_BYTES)? <= limit {
            if bytes.get(at)? == &0
                && bytes.get(at + 2..at + 6)? == [0; 4]
                && View::u32_le_at(bytes, at + 6).is_some_and(|value| value != 0)
            {
                return Some(at);
            }
            let code_unit = View::u16_le_at(bytes, at)?;
            let byte = u8::try_from(code_unit).ok()?;
            if !byte.is_ascii_graphic() {
                return None;
            }
            at = at.checked_add(2)?;
        }
        None
    })();
    let end = end?;

    let mut role = String::new();
    for at in (start..end).step_by(2) {
        let unit = View::u16_le_at(bytes, at)?;
        let byte = u8::try_from(unit).ok()?;
        role.push(char::from(byte));
    }
    Some((role, end))
}

fn exact_component_insert_scope_283_262_257(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<(
    crate::records::sketch_placement::SketchPlacementMatrix,
    Option<usize>,
    u64,
)> {
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(
            start + component_scope_283_257::RELATION_MARKER
                ..start + component_scope_283_257::RELATION_MARKER + 1,
        )? != [1]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 38..start + 44)? != [0; 6]
        || bytes.get(start + 44..start + 46)? != [1, 1]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::NULL_GUID_CODE_UNIT_COUNT,
        )? != 36
    {
        return None;
    }
    let after_null_guid = fixed_utf16_ascii_eq(
        bytes,
        start + component_scope_283_257::NULL_GUID_CODE_UNIT_COUNT,
        NULL_COMPONENT_INSERT_GUID,
    )?;
    if after_null_guid != start + component_scope_283_257::REFERENCE_COUNT - 3
        || View::u32_le_at(bytes, start + component_scope_283_257::REFERENCE_COUNT)? != 1
        || bytes.get(start + component_scope_283_257::REFERENCE_MARKER) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::REFERENCE_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 134..start + 140)? != [0; 6]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_257::PREVIOUS_HISTORY_STATE_ID,
        )? != u32::MAX
    {
        return None;
    }
    Some((
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
        None,
        View::u64_le_at(bytes, start + component_scope_283_257::OCCURRENCE_IDENTITY)?,
    ))
}

fn exact_component_insert_scope_283_262_385(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<(
    crate::records::sketch_placement::SketchPlacementMatrix,
    Option<usize>,
    u64,
)> {
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 44..start + 52)? != [1, 0, 0, 0, 0, 0, 0, 0]
        || bytes.get(start + 38..start + 44)? != [0; 6]
        || bytes.get(
            start + component_scope_283_385::RELATION_MARKER
                ..start + component_scope_283_385::RELATION_MARKER + 1,
        )? != [1]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_385::RELATION_RECORD_INDEX,
        )? != relation_record_index
    {
        return None;
    }
    let transform_at = start + component_scope_283_385::TRANSFORM;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let after_null_guid = fixed_utf16_ascii_eq(
        bytes,
        start + component_scope_283_385::NULL_GUID_CODE_UNIT_COUNT,
        NULL_COMPONENT_INSERT_GUID,
    )?;
    if after_null_guid != start + component_scope_283_385::REFERENCE_COUNT - 3
        || View::u32_le_at(bytes, start + component_scope_283_385::REFERENCE_COUNT)? != 1
        || bytes.get(start + component_scope_283_385::REFERENCE_MARKER) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_scope_283_385::REFERENCE_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 262..start + 268)? != [0; 6]
        || View::u32_le_at(
            bytes,
            start + component_scope_283_385::PREVIOUS_HISTORY_STATE_ID,
        )? != u32::MAX
    {
        return None;
    }
    Some((
        transform,
        Some(transform_at),
        View::u64_le_at(bytes, start + component_scope_283_385::OCCURRENCE_IDENTITY)?,
    ))
}

const NULL_COMPONENT_INSERT_GUID: &str = "00000000-0000-0000-0000-000000000000";

fn exact_identity_insert_scope(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
    shifted: bool,
) -> Option<u64> {
    let delta = if shifted { 4 } else { 0 };
    let prefix = if shifted {
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0][..]
    } else {
        &[0; 14][..]
    };
    if bytes.get(
        start + identity_insert::ZERO_PREFIX..start + identity_insert::OCCURRENCE_IDENTITY + delta,
    )? != prefix
    {
        return None;
    }
    let at = start + delta;
    if bytes
        .get(at + identity_insert::ZERO_AFTER_IDENTITY..at + identity_insert::RELATION_REFERENCE)?
        != [0; 4]
        || bytes.get(at + identity_insert::RELATION_REFERENCE) != Some(&1)
        || View::u32_le_at(bytes, at + identity_insert::RELATION_REFERENCE + 1)?
            != relation_record_index
        || bytes.get(
            at + identity_insert::ZERO_AFTER_RELATION..at + identity_insert::IDENTITY_MARKERS,
        )? != [0; 6]
        || bytes.get(at + identity_insert::IDENTITY_MARKERS..at + identity_insert::NULL_GUID)?
            != [1, 1]
        || fixed_utf16_ascii_eq(
            bytes,
            at + identity_insert::NULL_GUID,
            "00000000-0000-0000-0000-000000000000",
        )? != at + identity_insert::LEN
    {
        return None;
    }
    View::u64_le_at(bytes, at + identity_insert::OCCURRENCE_IDENTITY)
}

fn exact_component_insert_identity_scope(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<u64> {
    const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
    if bytes.get(start + 11..start + 20)? != [0; 9]
        || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || bytes.get(start + 33..start + 37)? != [0; 4]
        || bytes.get(start + 37) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_identity_scope::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 42..start + 48)? != [0; 6]
        || bytes.get(
            start + component_identity_scope::IDENTITY_MARKERS
                ..start + component_identity_scope::IDENTITY_MARKERS + 2,
        )? != [1, 1]
        || View::u32_le_at(
            bytes,
            start + component_identity_scope::OPAQUE_CODE_UNIT_COUNT,
        )? != 36
    {
        return None;
    }
    let after_opaque_guid = fixed_utf16_ascii_eq(
        bytes,
        start + component_identity_scope::OPAQUE_CODE_UNIT_COUNT,
        NULL_GUID,
    )?;
    if after_opaque_guid != start + component_identity_scope::OPAQUE_UTF16_PAYLOAD + 72 {
        return None;
    }
    View::u64_le_at(bytes, start + component_identity_scope::OCCURRENCE_IDENTITY)
}

fn exact_component_insert_identity_scope_shifted(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<u64> {
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 29..start + 33)? != [0; 4]
        || bytes.get(start + component_identity_shifted::RELATION_MARKER) != Some(&1)
        || View::u32_le_at(
            bytes,
            start + component_identity_shifted::RELATION_RECORD_INDEX,
        )? != relation_record_index
        || bytes.get(start + 38..start + 44)? != [0; 6]
        || bytes.get(
            start + component_identity_shifted::IDENTITY_MARKERS
                ..start + component_identity_shifted::IDENTITY_MARKERS + 2,
        )? != [1, 1]
    {
        return None;
    }
    let after_null_guid = fixed_utf16_ascii_eq(
        bytes,
        start + component_identity_shifted::NULL_GUID_CODE_UNIT_COUNT,
        NULL_COMPONENT_INSERT_GUID,
    )?;
    if after_null_guid != start + component_identity_shifted::LEN {
        return None;
    }
    View::u64_le_at(
        bytes,
        start + component_identity_shifted::OCCURRENCE_IDENTITY,
    )
}

fn exact_component_insert_scope_414_264_389(
    bytes: &[u8],
    start: usize,
    relation_record_index: u32,
) -> Option<(
    crate::records::sketch_placement::SketchPlacementMatrix,
    Option<usize>,
    u64,
)> {
    if bytes.get(start + 11..start + 20)? != [0; 9]
        || bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || bytes.get(start + 33..start + 37)? != [0; 4]
        || bytes.get(start + component_matrix_414::RELATION_MARKER) != Some(&1)
        || View::u32_le_at(bytes, start + component_matrix_414::RELATION_RECORD_INDEX)?
            != relation_record_index
        || bytes.get(start + 42..start + 48)? != [0; 6]
        || bytes.get(
            start + component_matrix_414::MATRIX_MARKERS
                ..start + component_matrix_414::MATRIX_MARKERS + 2,
        )? != [1, 0]
    {
        return None;
    }
    let transform_at = start + component_matrix_414::TRANSFORM;
    let transform = rigid_transform_at(bytes, transform_at)?;
    let after_null_guid = fixed_utf16_ascii_eq(
        bytes,
        start + component_matrix_414::NULL_GUID_CODE_UNIT_COUNT,
        NULL_COMPONENT_INSERT_GUID,
    )?;
    if after_null_guid != start + component_matrix_414::LEN {
        return None;
    }
    Some((
        transform,
        Some(transform_at),
        View::u64_le_at(bytes, start + component_matrix_414::OCCURRENCE_IDENTITY)?,
    ))
}

fn legacy_component_insert_placements(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    transform: crate::records::sketch_placement::SketchPlacementMatrix,
) -> Vec<(String, usize, Option<usize>)> {
    let Some((class_tag, after_tag)) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)
    else {
        return Vec::new();
    };
    if class_tag != "288"
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
    {
        return Vec::new();
    }
    let mut placements = Vec::new();
    for first_at in carrier_at + 11..relation_at {
        let Some(role_at) = fixed_guid_end(bytes, first_at) else {
            continue;
        };
        let Some(after_role) = fixed_guid_end(bytes, role_at) else {
            continue;
        };
        if bytes.get(after_role..after_role + 14)
            != Some(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
        {
            continue;
        }
        let Some(after_asset_guid) = fixed_guid_end(bytes, after_role + 14) else {
            continue;
        };
        let Some((asset_identity, after_asset_identity)) =
            lp_utf16_bounded(bytes, after_asset_guid + 1, 37..=256)
        else {
            continue;
        };
        if !asset_identity
            .split_once('_')
            .is_some_and(|(guid, locator)| {
                crate::bytes::is_guid_relaxed(guid) && locator.starts_with("urn:")
            })
            || bytes.get(after_asset_guid) != Some(&0)
            || bytes.get(after_asset_identity) != Some(&0)
        {
            continue;
        }
        let carrier_transform_at = after_asset_identity + 1;
        let after_transform = carrier_transform_at + 16 * 8;
        let Some((repeated_identity, after_repeated_identity)) =
            lp_utf16_bounded(bytes, after_transform + 4, 37..=256)
        else {
            continue;
        };
        if rigid_transform_at(bytes, carrier_transform_at) == Some(transform)
            && repeated_identity == asset_identity
            && bytes.get(after_transform..after_transform + 4) == Some(&[0; 4])
            && bytes.get(after_repeated_identity..relation_at)
                == Some(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        {
            let Some((role, _)) = lp_utf16_bounded(bytes, role_at, 36..=36) else {
                continue;
            };
            placements.push((role, role_at + 4, Some(carrier_transform_at)));
        }
    }
    placements
}

pub(super) fn exact_copy_paste_component_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<Option<DesignCopyPasteComponentOperation>, CodecError> {
    if !matches!(
        scope.kind(),
        scope::DesignFeatureKind::DerivedInstance | scope::DesignFeatureKind::CopyPaste
    ) {
        return Ok(None);
    }
    let parsed = (|| {
        let stream = native_stream(&scope.id)?;
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let relation_record_index = *scope.reference_members().values().next()?;
        // The compact frame omits one four-byte prologue field, so both placements
        // and every marked reference before them move four bytes earlier.
        let source_at = match (scope.kind_name(), scope.frame_length()) {
            ("CopyPaste", 529) => 38,
            ("CopyPaste", 525) => 34,
            _ => return None,
        };
        if scope.reference_members().len() != 1 {
            return None;
        }
        let source_transform_offset = start.checked_add(source_at)?;
        let copied_transform_offset = source_transform_offset.checked_add(156)?;
        let source_transform = rigid_transform_at(bytes, source_transform_offset)?;
        let copied_transform = rigid_transform_at(bytes, copied_transform_offset)?;
        let relation_at = records.first_at_or_after(0, relation_record_index)?;
        if relation_at >= start
            || next_indexed_record_offset(bytes, relation_at + 1)? != relation_at + 57
            || bytes.get(relation_at + 11..relation_at + 21)? != [0; 10]
            || bytes.get(relation_at + 21) != Some(&1)
            || bytes.get(relation_at + 26..relation_at + 34)? != [0; 8]
            || bytes.get(relation_at + 34) != Some(&1)
            || bytes.get(relation_at + 39..relation_at + 46)? != [0; 7]
            || bytes.get(relation_at + 46) != Some(&1)
            || View::u32_le_at(bytes, relation_at + 47)? != scope.record_index
            || bytes.get(relation_at + 51..relation_at + 57)? != [0; 6]
        {
            return None;
        }
        let copied_occurrence_record_index = View::u32_le_at(bytes, relation_at + 22)?;
        let mut copied_candidates = occurrences.iter().filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.record_index == copied_occurrence_record_index
                && occurrence.byte_offset() < u64_from_index(relation_at)
                && occurrence.transform().map(|frame| frame.value) == Some(copied_transform)
        });
        let copied = copied_candidates.next()?;
        if copied_candidates.next().is_some() {
            return None;
        }
        let mut source_candidates = occurrences.iter().filter(|occurrence| {
            native_stream(&occurrence.id) == Some(stream)
                && occurrence.byte_offset() < copied.byte_offset()
                && occurrence
                    .component_guid
                    .as_str()
                    .eq_ignore_ascii_case(copied.component_guid.as_str())
                && occurrence.transform().is_none()
        });
        let source = source_candidates.next()?;
        if source_candidates.next().is_some() {
            return None;
        }
        Some(Ok(DesignCopyPasteComponentOperation {
            relation_record_index,
            source_occurrence_record_index: source.record_index,
            copied_occurrence_record_index,
            component_guid: copied.component_guid.clone(),
            source_occurrence_guid: source.occurrence_guid.clone(),
            copied_occurrence_guid: copied.occurrence_guid.clone(),
            source_transform,
            source_transform_offset: u64_from_index(source_transform_offset),
            copied_transform,
            copied_transform_offset: u64_from_index(copied_transform_offset),
        }))
    })();
    parsed.transpose()
}

pub(super) fn bind_component_pattern_occurrences(
    scope: &mut DesignParameterScope,
    occurrences: &[DesignComponentOccurrence],
) -> Result<(), CodecError> {
    let Some(stream) = native_stream(&scope.id) else {
        return Ok(());
    };
    let byte_offset = scope.byte_offset();
    let Some(instances) = scope
        .rectangular_pattern_construction()
        .and_then(|construction| construction.instances())
    else {
        return Ok(());
    };
    let mut component_guid = None;
    for (ordinal, frame) in instances.frames().enumerate().skip(1) {
        let expected_ordinal = u32::try_from(ordinal)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::malformed("F3D pattern occurrence ordinal exceeds u32"))?;
        let Some(candidate) =
            unique_pattern_occurrence(stream, frame, expected_ordinal, occurrences)
        else {
            return Ok(());
        };
        if let Some(first_guid) = component_guid {
            if !candidate
                .component_guid
                .as_str()
                .eq_ignore_ascii_case(first_guid)
            {
                return Ok(());
            }
        } else {
            component_guid = Some(candidate.component_guid.as_str());
        }
    }
    let Some(component_guid) = component_guid else {
        return Ok(());
    };
    let mut seed_candidates = occurrences.iter().filter(|occurrence| {
        native_stream(&occurrence.id) == Some(stream)
            && occurrence.byte_offset() < byte_offset
            && occurrence
                .component_guid
                .as_str()
                .eq_ignore_ascii_case(component_guid)
            && matches!(
                occurrence.placement(),
                assembly_features::DesignComponentOccurrencePlacement::Base
            )
    });
    let Some(seed) = seed_candidates.next() else {
        return Ok(());
    };
    if seed_candidates.next().is_some() {
        return Ok(());
    }
    let Some(seed_frame) = instances.frames().next().copied() else {
        return Ok(());
    };
    let mut generated = Vec::new();
    for (ordinal, frame) in instances.frames().enumerate().skip(1) {
        let expected_ordinal = u32::try_from(ordinal)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::malformed("F3D pattern occurrence ordinal exceeds u32"))?;
        let candidate = unique_pattern_occurrence(stream, frame, expected_ordinal, occurrences)
            .ok_or_else(|| {
                CodecError::malformed("F3D pattern occurrence changed during binding")
            })?;
        generated.push(patterns::DesignPatternComponentInstance {
            instance: *frame,
            occurrence_guid: candidate.occurrence_guid.clone(),
        });
    }
    let bound = DesignRectangularPatternInstances::Components {
        component_guid: seed.component_guid.clone(),
        seed: patterns::DesignPatternComponentInstance {
            instance: seed_frame,
            occurrence_guid: seed.occurrence_guid.clone(),
        },
        generated,
    };
    if let Some(construction) = scope.rectangular_pattern_construction_mut() {
        construction
            .try_set_instances(Some(bound))
            .map_err(CodecError::malformed)?;
    }
    Ok(())
}

fn unique_pattern_occurrence<'a>(
    stream: &str,
    frame: &patterns::DesignPatternInstance,
    ordinal: u32,
    occurrences: &'a [DesignComponentOccurrence],
) -> Option<&'a DesignComponentOccurrence> {
    let mut candidates = occurrences.iter().filter(|occurrence| {
        native_stream(&occurrence.id) == Some(stream)
            && occurrence.transform().map(|frame| frame.offset) == Some(frame.transform.offset)
            && occurrence.occurrence_ordinal() == ordinal
    });
    let candidate = candidates.next();
    if candidates.next().is_some() {
        None
    } else {
        candidate
    }
}

fn unique_indexed_record_before(
    records: &IndexedRecordOffsets,
    record_index: u32,
    end: usize,
) -> Option<usize> {
    let offsets = records.offsets(record_index);
    let [at] = &offsets[..offsets.partition_point(|offset| *offset < end)] else {
        return None;
    };
    Some(*at)
}

#[cfg(test)]
mod tests;
