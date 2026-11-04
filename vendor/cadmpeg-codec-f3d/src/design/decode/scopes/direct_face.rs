// SPDX-License-Identifier: Apache-2.0
//! Exact direct-face, move and scale operation scopes.

use cadmpeg_core::decode::u64_from_index;

use super::parameter_scope::parameter_scope_payload_length;
use super::point_data::exact_point_data_construction;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use super::thicken_shell::exact_legacy_thicken_class_347;
use super::thicken_shell::exact_shell_class_369_261;
use super::thicken_shell::exact_shell_wall_slots;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::move_transform_frame_253 as move_frame;
use crate::layout::move_transform_frame_254 as move_frame_254;
use crate::layout::move_transform_frame_273 as move_frame_273;
use crate::layout::move_transform_frame_274 as move_frame_274;
use crate::records::feature::body_ops::DesignScaleOperation;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::direct_face::DesignMoveOperation;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use std::collections::HashMap;

/// Primary-header offset of the marked distance reference of an `OffsetFaces`
/// scope, or `None` when its prologue is not a known form.
///
/// The 253- and 264-byte token-independent forms are identified by length.
/// The other forms are identified by their prologue and by the offset of the
/// ordered-reference count, because their scope tail can be 77 or 78 bytes.
/// A marker-one prologue stores `1` at offset 20 and a u32 zero at 21, so its
/// fields start four bytes later than those of a marker-zero prologue.
fn offset_faces_distance_marker(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<usize> {
    let references = scope.reference_members().len();
    if matches!(
        (parameter_scope_payload_length(scope), references),
        (Some(264), 4) | (Some(253), 3)
    ) && bytes.get(start + 25) == Some(&1)
    {
        return Some(25);
    }
    let count_offset = usize::try_from(scope.reference_count_offset())
        .ok()?
        .checked_sub(start)?;
    let distance_marker = match (bytes.get(start + 20)?, count_offset) {
        (1, 130) if View::u32_le_at(bytes, start + 21) == Some(0) => 25,
        (0, 126) => 21,
        _ => return None,
    };
    // A u32 one follows the marked distance reference, then the marked
    // reference to the selected-face group, the first ordered reference.
    let first_reference = scope.reference_members().values().next()?;
    (View::u32_le_at(bytes, start + distance_marker + 11) == Some(1)
        && marked_record_reference(bytes, start + distance_marker + 15) == Some(*first_reference))
    .then_some(distance_marker)
}

pub(crate) fn offset_faces_distance_reference_matches(
    scope: &DesignParameterScope,
    distance_record_index: u32,
) -> bool {
    scope.reference_members().len() >= 3
        && scope.reference_members().values().next_back() == Some(&distance_record_index)
}

pub(super) fn exact_direct_face_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Shell)
        && scope.class_tag.as_str() == "369"
        && scope.paired_class_tag.as_str() == "261"
    {
        return exact_shell_class_369_261(bytes, records, scope);
    }
    if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Shell) {
        if let Some(operation) = exact_shell_wall_slots(bytes, records, scope) {
            return Some(operation);
        }
    }
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::OffsetFaces => {
            let distance_marker = offset_faces_distance_marker(bytes, scope, start)?;
            let distance_record_index = marked_record_reference(bytes, start + distance_marker)?;
            if !offset_faces_distance_reference_matches(scope, distance_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, distance_record_index)?;
            Some(DesignDirectFaceOperation::OffsetFaces(
                direct_face::DesignOffsetFacesOperation {
                    distance: scalar.value,
                    distance_record_index,
                    distance_offset: scalar.value_offset,
                },
            ))
        }
        DesignFeatureFamily::Thicken if scope.reference_members().len() >= 3 => {
            if let Some(operation) = exact_legacy_thicken_class_347(bytes, records, scope) {
                return Some(operation);
            }
            let (reference_offset, thickness_is_first) = match parameter_scope_payload_length(scope)
            {
                Some(length)
                    if length
                        == 276
                            + 11 * u64::try_from(
                                scope.reference_members().len().checked_sub(2)?,
                            )
                            .ok()?
                        && bytes.get(start + 34) == Some(&1)
                        && View::u32_le_at(bytes, start + 35)
                            == scope.reference_members().values().nth(1).copied()
                        && bytes.get(start + 39..start + 45) == Some(&[0; 6])
                        && matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46..start + 48) == Some(&[1, 1])
                        && View::u32_le_at(bytes, start + 48)
                            == scope.reference_members().values().next().copied() =>
                {
                    (47, true)
                }
                Some(281)
                    if matches!(bytes.get(start + 45), Some(0 | 1))
                        && bytes.get(start + 46) == Some(&1) =>
                {
                    (46, false)
                }
                Some(287) if bytes.get(start + 47) == Some(&1) => (47, false),
                _ => return None,
            };
            let thickness_record_index = View::u32_le_at(bytes, start + reference_offset + 1)?;
            let expected_thickness = if thickness_is_first {
                scope.reference_members().values().next()
            } else {
                scope.reference_members().values().next_back()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
            if scalar.value.get() == 0.0 {
                return None;
            }
            Some(DesignDirectFaceOperation::Thicken(
                direct_face::DesignThickenOperation {
                    signed_thickness: scalar.value,
                    thickness_record_index,
                    thickness_offset: scalar.value_offset,
                },
            ))
        }
        DesignFeatureFamily::Shell if scope.reference_members().len() == 3 => {
            let (thickness_record_index, thickness_is_first, outward, outward_offset) =
                match parameter_scope_payload_length(scope) {
                    Some(268)
                        if bytes.get(start + 11..start + 20) == Some(&[0; 9])
                            && bytes.get(start + 20) == Some(&1)
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22..start + 25) == Some(&[0; 3])
                            && bytes.get(start + 25..start + 29) == Some(&[1, 0, 0, 1])
                            && bytes.get(start + 33..start + 51) == Some(&[0; 18])
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && View::u32_le_at(bytes, start + 56)
                                == scope.reference_members().values().nth(1).copied()
                            && bytes.get(start + 60..start + 66) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 29)?,
                            true,
                            bytes[start + 20] == 1 || bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    Some(268)
                        if bytes.get(start + 11..start + 20) == Some(&[0; 9])
                            && bytes.get(start + 20) == Some(&1)
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22..start + 25) == Some(&[0; 3])
                            && bytes.get(start + 25..start + 27) == Some(&[1, 0])
                            && bytes.get(start + 27) == Some(&1)
                            && bytes.get(start + 32..start + 51) == Some(&[0; 19])
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && View::u32_le_at(bytes, start + 56)
                                == scope.reference_members().values().nth(1).copied()
                            && bytes.get(start + 60..start + 66) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 28)?,
                            true,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    Some(268)
                        if matches!(bytes.get(start + 25), Some(0 | 1))
                            && bytes.get(start + 26) == Some(&0)
                            && bytes.get(start + 27) == Some(&1)
                            && View::u32_le_at(bytes, start + 51) == Some(1)
                            && bytes.get(start + 55) == Some(&1)
                            && View::u32_le_at(bytes, start + 56)
                                == scope.reference_members().values().next().copied() =>
                    {
                        (
                            View::u32_le_at(bytes, start + 28)?,
                            false,
                            bytes[start + 25] != 0,
                            start + 25,
                        )
                    }
                    Some(258)
                        if bytes.get(start + 11..start + 21) == Some(&[0; 10])
                            && matches!(bytes.get(start + 21), Some(0 | 1))
                            && bytes.get(start + 22) == Some(&1)
                            && bytes.get(start + 27..start + 42) == Some(&[0; 15])
                            && View::u32_le_at(bytes, start + 42) == Some(1)
                            && bytes.get(start + 46) == Some(&1)
                            && View::u32_le_at(bytes, start + 47)
                                == scope.reference_members().values().next().copied()
                            && bytes.get(start + 51..start + 57) == Some(&[0; 6]) =>
                    {
                        (
                            View::u32_le_at(bytes, start + 23)?,
                            false,
                            bytes[start + 21] != 0,
                            start + 21,
                        )
                    }
                    _ => return None,
                };
            let expected_thickness = if thickness_is_first {
                scope.reference_members().values().next()
            } else {
                scope.reference_members().values().next_back()
            };
            if expected_thickness != Some(&thickness_record_index) {
                return None;
            }
            let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
            Some(DesignDirectFaceOperation::Shell(
                direct_face::DesignShellOperation {
                    thickness: cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
                    thickness_record_index,
                    thickness_offset: scalar.value_offset,
                    outward,
                    outward_offset: u64::try_from(outward_offset).ok()?,
                    wall_slots: None,
                },
            ))
        }
        _ => None,
    }
}

pub(super) fn exact_move_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignMoveOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Move) {
        return None;
    }
    unique_move_transform(bytes, records, scope.reference_members().values())
}

/// Primary-header offset of the marked transform reference of a `MoveFace`
/// scope, or `None` when its prologue is not a known form.
///
/// A marker-one prologue stores `1` at offset 20 and a u32 zero at 21, so its
/// fields start four bytes later than those of a marker-zero prologue.
fn move_face_transform_marker(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<usize> {
    let count_offset = usize::try_from(scope.reference_count_offset())
        .ok()?
        .checked_sub(start)?;
    let transform_marker = match (bytes.get(start + 20)?, count_offset) {
        (1, 131) if View::u32_le_at(bytes, start + 21) == Some(0) => 26,
        (0, 127) => 22,
        _ => return None,
    };
    // A byte one precedes the marked transform reference. A u32 one follows it,
    // then the marked reference to the selected-face group, the first reference.
    let first_reference = scope.reference_members().values().next()?;
    (bytes.get(start + transform_marker - 1) == Some(&1)
        && View::u32_le_at(bytes, start + transform_marker + 11) == Some(1)
        && marked_record_reference(bytes, start + transform_marker + 15) == Some(*first_reference))
    .then_some(transform_marker)
}

pub(super) fn exact_move_face_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignMoveOperation> {
    if !is_move_face(scope) || scope.reference_members().len() < 3 {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let transform_marker = move_face_transform_marker(bytes, scope, start)?;
    let transform_record_index = scope.reference_members().values().next_back()?;
    if marked_record_reference(bytes, start + transform_marker) != Some(*transform_record_index) {
        return None;
    }
    move_face_transform(bytes, records, *transform_record_index)
}

fn is_move_face(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.kind(),
        scope::DesignFeatureKind::MoveFace | scope::DesignFeatureKind::DeplacerFace
    )
}

/// The rigid transform of the record a `MoveFace` scope marks. The marked
/// reference selects the record, so its class tag and frame length do not.
fn move_face_transform(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<DesignMoveOperation> {
    let mut candidate = None;
    for (start, paired) in records.frames(record_index) {
        let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if View::u32_le_at(bytes, after_tag) != Some(record_index)
            || paired.checked_sub(start)? < move_frame::TRANSFORM + 128
        {
            continue;
        }
        let Some(next) = move_transform_at(bytes, start, record_index) else {
            continue;
        };
        if candidate.replace(next).is_some() {
            return None;
        }
    }
    candidate
}

/// The form and matrix of the transform frame at `start`, or `None` when its
/// reserved bytes, form, or matrix are not those of a rigid transform.
fn move_transform_at(bytes: &[u8], start: usize, record_index: u32) -> Option<DesignMoveOperation> {
    if bytes.get(start + indexed_header::LEN..start + move_frame::FORM)
        != Some(&[0; move_frame::FORM - indexed_header::LEN])
        || bytes.get(start + move_frame::RESERVED_ZERO) != Some(&0)
    {
        return None;
    }
    let form =
        direct_face::DesignMoveForm::try_from(View::u32_le_at(bytes, start + move_frame::FORM)?)
            .ok()?;
    let mut view = View::over_retained(bytes.get(start + move_frame::TRANSFORM..)?);
    let mut transform = [[0.0; 4]; 4];
    for row in &mut transform {
        for value in row {
            *value = view.f64_le()?;
        }
    }
    let transform =
        crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform).ok()?;
    Some(DesignMoveOperation {
        transform,
        transform_offset: u64_from_index(start + move_frame::TRANSFORM),
        transform_record_index: record_index,
        form,
        form_offset: u64_from_index(start + move_frame::FORM),
    })
}

/// The one admitted rigid-transform frame among `record_indices`.
fn unique_move_transform<'a>(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_indices: impl IntoIterator<Item = &'a u32>,
) -> Option<DesignMoveOperation> {
    let mut candidate = None;
    for record_index in record_indices {
        for (start, paired) in records.frames(*record_index) {
            let class_tag_len = usize::try_from(View::u32_le_at(bytes, start)?).ok()?;
            if class_tag_len > 2000 {
                return None;
            }
            let after_tag = start.checked_add(4)?.checked_add(class_tag_len)?;
            let class_tag = std::str::from_utf8(bytes.get(start + 4..after_tag)?).ok()?;
            if !class_tag.bytes().all(|byte| byte.is_ascii_graphic()) {
                return None;
            }
            let frame_length = paired.checked_sub(start)?;
            if View::u32_le_at(bytes, after_tag) != Some(*record_index) {
                continue;
            }
            let Some(lengths) = move_transform_lengths(class_tag) else {
                continue;
            };
            if !lengths.contains(&frame_length) {
                continue;
            }
            let expected_paired_class = match class_tag {
                "447" => Some("263"),
                "456" => Some("258"),
                "452" => Some("262"),
                _ => None,
            };
            if expected_paired_class.is_some_and(|expected| {
                lp_ascii_filtered_view(bytes, paired, 3..=3, u8::is_ascii_digit)
                    .is_none_or(|(paired_class_tag, _)| paired_class_tag != expected)
            }) {
                continue;
            }
            let Some(next) = move_transform_at(bytes, start, *record_index) else {
                continue;
            };
            if !move_transform_form_holds(class_tag, next.form) {
                continue;
            }
            if candidate.replace(next).is_some() {
                return None;
            }
        }
    }
    candidate
}

fn move_transform_lengths(class_tag: &str) -> Option<&'static [usize]> {
    match class_tag {
        "296" | "362" | "433" | "447" | "293" | "393" | "442" | "451" | "456" | "452" => {
            Some(&[move_frame::LEN])
        }
        "349" => Some(&[move_frame_254::LEN, move_frame_274::LEN]),
        "368" => Some(&[move_frame_254::LEN]),
        "330" | "273" | "408" | "372" | "289" | "394" | "406" | "282" | "403" | "391" => {
            Some(&[move_frame_273::LEN])
        }
        _ => None,
    }
}

fn move_transform_form_holds(class_tag: &str, form: direct_face::DesignMoveForm) -> bool {
    match class_tag {
        "330" | "273" | "408" | "372" | "289" | "394" | "406" | "282" | "403" | "391" => {
            form == direct_face::DesignMoveForm::Form1
        }
        "452" => form == direct_face::DesignMoveForm::Form5,
        _ => true,
    }
}

pub(crate) fn move_operation_links_hold(
    scope: &DesignParameterScope,
    header: Option<&crate::records::decal::DesignRecordHeader>,
) -> bool {
    scope.move_operation().is_none_or(|operation| {
        header.is_some_and(|header| {
            (is_move_face(scope)
                || (move_transform_lengths(header.class_tag.as_str()).is_some()
                    && move_transform_form_holds(header.class_tag.as_str(), operation.form)))
                && scope
                    .reference_members()
                    .values()
                    .any(|index| *index == operation.transform_record_index)
                && header.record_index == operation.transform_record_index
                && header
                    .byte_offset
                    .checked_add(u64_from_index(move_frame::FORM))
                    == Some(operation.form_offset)
                && header
                    .byte_offset
                    .checked_add(u64_from_index(move_frame::TRANSFORM))
                    == Some(operation.transform_offset)
        })
    })
}

pub(super) fn exact_scale_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Result<Option<DesignScaleOperation>, cadmpeg_core::CodecError> {
    (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Scale) {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let (body_group_record_index, center_record_index, uniform_factor_offset, center) =
            if parameter_scope_payload_length(scope) == Some(303)
                && scope.reference_members().len() == 5
            {
                let [factor_record_index, body_group_record_index, _, _, center_record_index] =
                    scope.reference_members().values_array()?;
                if View::u32_le_at(bytes, start + 20)? != 1
                    || bytes.get(start + 24) != Some(&0)
                    || marked_record_reference(bytes, start + 33)? != *center_record_index
                    || marked_record_reference(bytes, start + 44)? != *factor_record_index
                    || View::u32_le_at(bytes, start + 55)? != 1
                    || bytes.get(start + 59) != Some(&0)
                    || View::u32_le_at(bytes, start + 60)? != 1
                    || View::u32_le_at(bytes, start + 64)? != 1
                    || marked_record_reference(bytes, start + 68)? != *body_group_record_index
                {
                    return None;
                }
                let center = exact_point_data_construction(
                    bytes,
                    records,
                    std::slice::from_ref(center_record_index),
                    stream_types,
                )
                .map(|point| (point.position, point.position_offset));
                (
                    *body_group_record_index,
                    *center_record_index,
                    start + 25,
                    center,
                )
            } else if scope.kind() == scope::DesignFeatureKind::Scale
                && matches!(
                    (scope.frame_length(), scope.reference_members().len()),
                    (307 | 309, 5) | (318, 6)
                )
            {
                if scope.frame_length() == 309 {
                    let (guid, _) = crate::bytes::lp_utf16_bounded_view(
                        bytes,
                        start.checked_add(75)?,
                        36..=36,
                    )?;
                    if !guid.eq_str("00000000-0000-0000-0000-000000000000")
                        || View::u24_le_at(bytes, start.checked_add(151)?)? != 0
                        || scope.reference_count_offset() != scope.byte_offset().checked_add(154)?
                        || scope.kind_offset() != scope.byte_offset().checked_add(221)?
                        || scope.feature_ordinal_offset() != scope.byte_offset().checked_add(231)?
                        || scope.previous_history_state_id_offset()
                            != scope.byte_offset().checked_add(262)
                    {
                        return None;
                    }
                }
                let mut references = scope.reference_members().values();
                let factor_record_index = references.next()?;
                let body_group_record_index = references.next()?;
                let center_record_index = references.next_back()?;
                if bytes.get(start + 16..start + 21)? != [0; 5]
                    || marked_record_reference(bytes, start + 29)? != *center_record_index
                    || marked_record_reference(bytes, start + 40)? != *factor_record_index
                    || View::u32_le_at(bytes, start + 51)? != 1
                    || bytes.get(start + 55) != Some(&0)
                    || View::u32_le_at(bytes, start + 56)? != 1
                    || View::u32_le_at(bytes, start + 60)? != 1
                    || marked_record_reference(bytes, start + 64)? != *body_group_record_index
                {
                    return None;
                }
                let point = exact_point_data_construction(
                    bytes,
                    records,
                    std::slice::from_ref(center_record_index),
                    stream_types,
                )?;
                (
                    *body_group_record_index,
                    *center_record_index,
                    start + 21,
                    Some((point.position, point.position_offset)),
                )
            } else {
                return None;
            };
        let uniform_factor =
            cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, uniform_factor_offset)?)?;
        Some(Ok(DesignScaleOperation {
            body_group_record_index,
            center_record_index,
            center_position: center
                .map(|(value, offset)| crate::records::identity::Located { value, offset }),
            uniform_factor,
            uniform_factor_offset: u64_from_index(uniform_factor_offset),
        }))
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
