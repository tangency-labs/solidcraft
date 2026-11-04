// SPDX-License-Identifier: Apache-2.0
//! Semantic validation of the Fusion `f3d` native namespace.
//!
//! [`validate_native_charged`] loads the `f3d` native namespace from a decoded
//! [`CadIr`] and checks the settled byte frames and cross-record relationships
//! of every Fusion Design record family: body maps and bounds, parameter
//! scopes and their feature operands, sketch geometry and relations, dimension
//! loci, persistent identity links, and the ASM history graph. It returns the
//! [`Finding`] values in a fixed emission order; callers append them to the
//! generic IR validation report.

mod body_ownership;
mod emboss;
mod sheet_metal_settings;
mod xref;

use xref::validate_xref_placement_sources;

use cadmpeg_core::convert::f64_from_index;

use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_legacy_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_one_sided_to_face_layout;
use crate::design::decode::scopes::extrude::is_class_296_symmetric_distance_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_layout;
use crate::design::decode::scopes::extrude::is_class_296_two_sided_to_faces_scope;
use crate::design::decode::scopes::legacy_class_397::Class397SymmetricFrame;
use crate::design::decode::scopes::legacy_class_415;
use crate::layout::assembly_class_307_264_joint_origin_scope as class_307_joint_origin;
use crate::layout::assembly_class_363_264_frame_363_carrier as class_363_carrier;
use crate::layout::assembly_class_363_264_frame_388_identity as class_363_identity;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::layout::class_296_261_legacy_extrude_prefix_scalar_at_54 as class_296_legacy_prefix;
use crate::layout::class_296_261_legacy_one_sided_distance_tail as class_296_legacy_distance;
use crate::layout::class_296_261_legacy_one_sided_to_face_tail as class_296_legacy_to_face;
use crate::layout::class_296_261_one_sided_to_face_extrude_prefix as class_296_to_face;
use crate::layout::class_296_261_symmetric_distance_extrude_prefix as class_296_symmetric;
use crate::layout::class_296_261_two_sided_to_faces_extrude_prefix as class_296_two_faces;
use crate::layout::legacy_class_397_symmetric_extrude_frame as class_397;
use crate::layout::legacy_class_415_symmetric_extrude_prefix as class_415;
use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::{design, history, ids, native, records};
use cadmpeg_core::decode::id_from_index;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{
    check::{Check, Finding},
    Severity,
};

const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10: f64 = 1.0e-10;
const EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8: f64 = 1.0e-8;

/// Resolve the native design stream that owns a record `id`, defaulting to the
/// primary design stream when the id carries no stream qualifier.
fn design_stream(id: &str) -> &str {
    ids::native_stream(id).unwrap_or(ids::DEFAULT_STREAM)
}

/// Report whether a native `stream` scope contains the design `entry`, either
/// directly or through an `f3d:xref/` qualifier.
fn design_stream_contains_entry(stream: &str, entry: &str) -> bool {
    ids::native_scope_matches(stream, entry)
}

/// Admit the empty reference table used by a legacy Combine tool operand.
fn body_recipe_reference_table_is_admitted(
    scope: Option<&records::feature::scope::DesignParameterScope>,
    operand: &records::topology::body_recipe::DesignBodyRecipeOperand,
) -> bool {
    !operand.references().is_empty()
        || matches!(
            operand.owner,
            records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
        ) && scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Combine
                && scope.combine_operation().is_some_and(|operation| {
                    operation
                        .tools
                        .iter()
                        .any(|tool| tool.record_index == operand.record_index())
                })
        })
}

/// A rigid `As-built` alignment has no parameter owners, so its angle and
/// offset are zero.
fn rigid_alignment_scalars_hold(
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    rigid_as_built: bool,
) -> bool {
    !rigid_as_built
        || !alignment.owners.is_empty()
        || (alignment.angle() == 0.0 && alignment.offset() == [0.0; 3])
}

fn valid_component_insert_role(role: &str) -> bool {
    role.encode_utf16().count() <= 256
        && (crate::bytes::is_guid_relaxed(role)
            || role.starts_with("urn:")
            || (crate::bytes::is_guid_prefix(role)
                && role.as_bytes().get(36) == Some(&b'_')
                && role
                    .get(37..)
                    .is_some_and(|suffix| suffix.starts_with("urn:"))))
}

/// A rigid group whose operands were read keeps one member after its first
/// two for each locator its frame stores, each taking the locator reference
/// that follows the previous one, with the frame its locator carries. A
/// group whose operands were not read, and every other scope, has no further
/// members.
fn rigid_group_members_link(
    scope: &records::feature::scope::DesignParameterScope,
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    rigid_as_built: bool,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> bool {
    if rigid_as_built && alignment.form.is_some() {
        let Some(layout) = design::assembly::RigidGroupLayout::for_scope(scope) else {
            return false;
        };
        Some(layout.members) == alignment.additional_members.len().checked_add(2)
            && alignment.additional_members.iter().enumerate().all(|(ordinal, member)| {
                let records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } = &member.qualifier else {
                    return false;
                };
                let Some(locator_offset) = ordinal.checked_add(2)
                    .and_then(|member| member.checked_mul(layout.stride))
                    .and_then(|offset| offset.checked_add(layout.first_reference)) else {
                    return false;
                };
                valid_assembly_operand_path_link(scope, path, locator_offset, stream_types)
                    && as_built_operand_frame_link(scope, path, &member.frame, stream_types)
            })
    } else {
        alignment.additional_members.is_empty()
    }
}

fn as_built_operand_frame_link(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> bool {
    use crate::layout::assembly_wide_operand_path_locator as wide_locator;
    let locator = path.link().locator_byte_offset;
    let identity = frame.transform.rows() == cadmpeg_ir::transform::Transform::identity().rows();
    let length = path
        .byte_offset()
        .checked_sub(locator)
        .and_then(|length| usize::try_from(length).ok());
    let rigid = design::assembly::rigid_as_built_scope(scope);
    let layout = rigid_group_locator_layout(path, stream_types);
    if rigid {
        let Some(layout) = layout else {
            return false;
        };
        let reference = if layout.null_construction {
            crate::layout::indexed_design_record_header::RECORD_INDEX
        } else {
            layout.construction_reference + 1
        };
        let transform = layout.transform.unwrap_or(layout.identity_flag);
        return locator.checked_add(u64_from_index(reference)) == Some(frame.reference_offset)
            && locator.checked_add(u64_from_index(transform)) == Some(frame.transform_offset)
            && (layout.transform.is_some() || identity)
            && frame.reference_record_index != 0
            && (!layout.null_construction
                || frame.reference_record_index == path.link().locator_record_index);
    }
    let (reference, flag, matrix) = if design::assembly::wide_as_built_scope(scope) {
        if !matches!(
            length,
            Some(
                crate::layout::assembly_wide_operand_path_identity_locator::LEN
                    | crate::layout::assembly_wide_operand_path_matrix_locator::LEN
            )
        ) {
            return false;
        }
        (
            wide_locator::NONZERO_RECORD_REFERENCE + 1,
            Some(wide_locator::IDENTITY_FLAG),
            wide_locator::TRANSFORM,
        )
    } else {
        (
            path_locator::NONZERO_RECORD_REFERENCE + 1,
            None,
            path_locator::TRANSFORM,
        )
    };
    locator.checked_add(u64_from_index(reference)) == Some(frame.reference_offset)
        && (locator.checked_add(u64_from_index(matrix)) == Some(frame.transform_offset)
            || identity
                && flag.is_some_and(|flag| {
                    locator.checked_add(u64_from_index(flag)) == Some(frame.transform_offset)
                }))
        && frame.reference_record_index != 0
}

fn rigid_group_locator_layout(
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> Option<design::assembly::RigidGroupLocatorLayout> {
    let link = path.link();
    design::assembly::RigidGroupLocatorLayout::admit(
        stream_types
            .get(&u64::from(link.locator_record_index))
            .copied(),
        usize::try_from(path.byte_offset().checked_sub(link.locator_byte_offset)?).ok()?,
    )
}

fn valid_assembly_operand_path_link(
    scope: &records::feature::scope::DesignParameterScope,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
    locator_marker_offset: usize,
    stream_types: &std::collections::HashMap<u64, (&str, u32)>,
) -> bool {
    let link = &path.link();
    let Ok(locator_marker_offset) = u64::try_from(locator_marker_offset) else {
        return false;
    };
    let Some(locator_reference_offset) = scope
        .byte_offset()
        .checked_add(locator_marker_offset)
        .and_then(|offset| offset.checked_add(1))
    else {
        return false;
    };
    let variable_reference = !design::assembly::rigid_as_built_scope(scope)
        && design::assembly::variable_reference_assembly_generation(
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
    let wide = design::assembly::AssemblyScopeGeneration::for_scope(scope)
        .operand_frame_variant()
        .is_some_and(design::assembly::AssemblyOperandFrameVariant::is_wide_family)
        || design::assembly::wide_as_built_scope(scope);
    let (locator_length, scope_backlink, wrapper_reference) =
        if design::assembly::rigid_as_built_scope(scope) {
            let Some(layout) = rigid_group_locator_layout(path, stream_types) else {
                return false;
            };
            let (length, backlink, wrapper, _, _) = layout.offsets();
            (
                u64_from_index(length),
                u64_from_index(backlink + 1),
                u64_from_index(wrapper + 1),
            )
        } else if wide {
            use crate::layout::assembly_wide_operand_path_identity_locator as identity;
            use crate::layout::assembly_wide_operand_path_matrix_locator as matrix;
            let (length, backlink, wrapper) = match path
                .byte_offset()
                .checked_sub(link.locator_byte_offset)
                .and_then(|length| usize::try_from(length).ok())
            {
                Some(identity::LEN) => (
                    identity::LEN,
                    identity::SCOPE_BACKLINK,
                    identity::WRAPPER_REFERENCE,
                ),
                Some(matrix::LEN) => (
                    matrix::LEN,
                    matrix::SCOPE_BACKLINK,
                    matrix::WRAPPER_REFERENCE,
                ),
                _ => return false,
            };
            (
                u64_from_index(length),
                u64_from_index(backlink + 1),
                u64_from_index(wrapper + 1),
            )
        } else if variable_reference {
            (
                u64_from_index(variable_path_locator::LEN),
                u64_from_index(variable_path_locator::SCOPE_BACKLINK + 1),
                u64_from_index(variable_path_locator::WRAPPER_REFERENCE + 1),
            )
        } else {
            (
                u64_from_index(path_locator::LEN),
                u64_from_index(path_locator::SCOPE_BACKLINK + 1),
                u64_from_index(path_locator::WRAPPER_REFERENCE + 1),
            )
        };
    let Some(path_byte_offset) = link.locator_byte_offset.checked_add(locator_length) else {
        return false;
    };
    let Some(locator_scope_reference_offset) = link.locator_byte_offset.checked_add(scope_backlink)
    else {
        return false;
    };
    let Some(wrapper_reference_offset) = link.locator_byte_offset.checked_add(wrapper_reference)
    else {
        return false;
    };
    let Some(path_reference_offset) = link.wrapper_byte_offset.checked_add(27) else {
        return false;
    };
    link.locator_reference_offset == locator_reference_offset
        && link.locator_record_index.checked_add(1) == Some(path.record_index)
        // A variable-reference or wide locator's path can continue over
        // further records before its wrapper.
        && if variable_reference || wide {
            link.locator_record_index
                .checked_add(2)
                .zip(link.locator_record_index.checked_add(65))
                .is_some_and(|(first, last)| (first..=last).contains(&link.wrapper_record_index))
        } else {
            link.locator_record_index.checked_add(2) == Some(link.wrapper_record_index)
        }
        && path.byte_offset() == path_byte_offset
        && link.locator_scope_reference_offset == locator_scope_reference_offset
        && link.wrapper_reference_offset == wrapper_reference_offset
        && link.wrapper_byte_offset > path.byte_offset()
        && link.path_reference_offset == path_reference_offset
}

fn valid_class_363_operand_path_link(
    scope: &records::feature::scope::DesignParameterScope,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    path: &records::feature::assembly::DesignAssemblyOperandPath,
) -> bool {
    let link = &path.link();
    link.locator_class_tag.as_str() == "363"
        && link.wrapper_class_tag.as_str() == "388"
        && path.class_tag().as_str() == "386"
        && link.locator_record_index == frame.reference_record_index
        && link.locator_reference_offset == frame.reference_offset
        && link
            .locator_byte_offset
            .checked_add(u64_from_index(class_363_carrier::SCOPE_REFERENCE + 1))
            == Some(link.locator_scope_reference_offset)
        && link
            .wrapper_byte_offset
            .checked_add(u64_from_index(class_363_identity::OCCURRENCE_GUID + 4))
            == Some(link.path_reference_offset)
        && link.wrapper_reference_offset < link.wrapper_byte_offset
        && link.locator_scope_reference_offset > link.locator_byte_offset
        && link.locator_reference_offset >= scope.byte_offset()
        && link.locator_reference_offset < scope.paired_byte_offset()
        && path.byte_offset() < link.locator_byte_offset
        && path.occurrence_guids()[0].offset == link.path_reference_offset
        && path.identity_guids()[0].offset > path.occurrence_guids()[0].offset
}

fn valid_class_307_joint_origin_qualifier(
    native: &native::F3dNative,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    frame: &records::feature::assembly::DesignAssemblyOperandFrame,
    qualifier: &records::feature::assembly::DesignAssemblyOperandQualifier,
) -> bool {
    let records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin {
        scope_record_index,
        class_tag,
        byte_offset,
        paired_class_tag,
        paired_byte_offset,
    } = qualifier
    else {
        return false;
    };
    frame.reference_record_index == *scope_record_index
        && class_tag.as_str() == "307"
        && paired_class_tag.as_str() == "264"
        && byte_offset.checked_add(u64_from_index(class_307_joint_origin::LEN))
            == Some(*paired_byte_offset)
        && design_header_matches(
            records_by_index,
            stream,
            *scope_record_index,
            class_tag.as_str(),
            *byte_offset,
        )
        && native
            .design_parameter_scopes
            .iter()
            .filter(|target_scope| {
                design_stream(&target_scope.id) == stream
                    && target_scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                    && target_scope.record_index == *scope_record_index
                    && target_scope.class_tag == *class_tag
                    && target_scope.byte_offset() == *byte_offset
                    && target_scope.paired_class_tag == *paired_class_tag
                    && target_scope.paired_byte_offset() == *paired_byte_offset
                    && target_scope.frame_length() == u64_from_index(class_307_joint_origin::LEN)
                    && target_scope.joint_origin_transform() == Some(frame.transform)
            })
            .count()
            == 1
}

fn valid_sketch_profile_region_selection(
    profile: &records::topology::sketch_profile::DesignSketchProfileOperand,
    selection: &records::topology::sketch_profile::DesignSketchProfileRegionSelection,
) -> bool {
    let Some(expected_region_count_offset) = selection
        .byte_offset
        .checked_add(u64_from_index(region_selection::REGION_COUNT))
    else {
        return false;
    };
    if profile.record_index.checked_add(3) != Some(selection.record_index)
        || selection.byte_offset <= profile.paired_byte_offset()
        || selection.region_count_offset != expected_region_count_offset
        || selection.regions.is_empty()
    {
        return false;
    }
    let Some(mut cursor) = selection
        .byte_offset
        .checked_add(u64_from_index(region_selection::LEN))
    else {
        return false;
    };
    for (region_ordinal, region) in selection.regions.iter().enumerate() {
        if region_ordinal != 0 {
            let Some(next) = cursor.checked_add(1) else {
                return false;
            };
            cursor = next;
        }
        if region.member_count_offset != cursor || region.members.is_empty() {
            return false;
        }
        let Some(next) = cursor.checked_add(4) else {
            return false;
        };
        cursor = next;
        for member in &region.members {
            let (Some(curve_primary_id_offset), Some(incidence_words_offset), Some(next)) = (
                cursor.checked_add(4),
                cursor.checked_add(8),
                cursor.checked_add(40),
            ) else {
                return false;
            };
            if member.kind_offset != cursor
                || member.curve_primary_id_offset != curve_primary_id_offset
                || member.incidence_words_offset != incidence_words_offset
            {
                return false;
            }
            cursor = next;
        }
    }
    cursor.checked_add(5) == Some(selection.companion_byte_offset)
}

fn design_header_matches(
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    record_index: u32,
    class_tag: &str,
    byte_offset: u64,
) -> bool {
    records_by_index
        .get(&(stream, record_index))
        .is_some_and(|header| {
            header.class_tag.as_str() == class_tag && header.byte_offset == byte_offset
        })
}

fn valid_axial_selector_identity(
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    selector: &records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
    limit: u64,
) -> bool {
    let utf16_len = |value: &str| u64::try_from(value.encode_utf16().count()).ok();
    let utf16_end =
        |offset: u64, value: &str| utf16_len(value)?.checked_mul(2)?.checked_add(offset);
    let Some(selector_asset_end) = utf16_end(
        selector.selector_asset_id_offset,
        selector.selector_asset_id.as_str(),
    ) else {
        return false;
    };
    let Some(selector_context_end) = utf16_end(
        selector.selector_context_id_offset,
        selector.selector_context_id.as_str(),
    ) else {
        return false;
    };
    let Some(external_asset_end) = utf16_end(
        selector.external_asset_id_offset,
        selector.external_asset_id.as_str(),
    ) else {
        return false;
    };
    let Some(external_link_end) = utf16_end(
        selector.external_link_name_offset,
        &selector.external_link_name,
    ) else {
        return false;
    };
    let Some(external_link_len) = utf16_len(&selector.external_link_name) else {
        return false;
    };
    let external_end = match &selector.external_version {
        None => external_link_end.checked_add(1),
        Some(version) => {
            let property_key = version.property_key.value.as_str();
            let property_key_offset = version.property_key.offset;
            let version_urn = version.version_urn.value.as_str();
            let version_urn_offset = version.version_urn.offset;
            let version_len = utf16_len(version_urn);
            if external_link_end.checked_add(5) != Some(property_key_offset)
                || !version_len.is_some_and(|length| (1..=256).contains(&length))
                || utf16_end(property_key_offset, property_key).and_then(|end| end.checked_add(4))
                    != Some(version_urn_offset)
            {
                None
            } else {
                utf16_end(version_urn_offset, version_urn)
            }
        }
    };
    let Some(external_end) = external_end else {
        return false;
    };
    let Some(occurrence_role_end) = utf16_end(
        selector.occurrence_role_offset,
        selector.occurrence_role.as_str(),
    ) else {
        return false;
    };
    let selector_pair_is_referenced = scope
        .reference_members()
        .values()
        .zip(scope.reference_members().values().skip(1))
        .filter(|(first, second)| {
            [**first, **second] == [selector.axis_record_index, selector.selector_record_index]
        })
        .count()
        == 1;
    let selector_records_are_unique = [selector.axis_record_index, selector.selector_record_index]
        .iter()
        .all(|record_index| {
            scope
                .reference_members()
                .values()
                .filter(|member| *member == record_index)
                .count()
                == 1
        });

    design_header_matches(
        records_by_index,
        stream,
        selector.axis_record_index,
        selector.axis_class_tag.as_str(),
        selector.axis_byte_offset,
    ) && design_header_matches(
        records_by_index,
        stream,
        selector.selector_record_index,
        selector.selector_class_tag.as_str(),
        selector.selector_byte_offset,
    ) && selector.axis_record_index.checked_add(3) == Some(selector.selector_record_index)
        && selector.selector_record_index.checked_add(3) == Some(selector.nested_record_index)
        && selector.selector_record_index.checked_add(5) == Some(selector.role_record_index)
        && selector.axis_byte_offset < selector.axis_paired_byte_offset
        && selector.axis_paired_byte_offset < selector.selector_byte_offset
        && selector.selector_byte_offset < selector.selector_paired_byte_offset
        && external_end <= selector.selector_paired_byte_offset
        && selector.selector_paired_byte_offset < selector.role_byte_offset
        && occurrence_role_end <= limit
        && selector.selector_byte_offset.checked_add(23)
            == Some(selector.nested_record_index_offset)
        && selector.selector_byte_offset.checked_add(41) == Some(selector.selector_asset_id_offset)
        && selector_asset_end.checked_add(4) == Some(selector.selector_context_id_offset)
        && selector_context_end.checked_add(13) == Some(selector.occurrence_reference_offset)
        && selector.occurrence_reference_offset.checked_add(15)
            == Some(selector.external_object_reference_offset)
        && selector.external_object_reference_offset.checked_add(9)
            == Some(selector.external_segment_offset)
        && selector.external_segment_offset.checked_add(8)
            == Some(selector.external_asset_id_offset)
        && external_asset_end.checked_add(5) == Some(selector.external_link_name_offset)
        && selector.role_byte_offset.checked_add(29) == Some(selector.occurrence_role_offset)
        && selector
            .external_asset_id
            .as_str()
            .eq_ignore_ascii_case(selector.selector_asset_id.as_str())
        && selector.occurrence_reference != 0
        && selector.external_object_reference != 0
        && (1..=256).contains(&external_link_len)
        && selector_pair_is_referenced
        && selector_records_are_unique
}

fn valid_axial_assembly_targets(
    native: &native::F3dNative,
    records_by_index: &HashMap<(&str, u32), &records::decal::DesignRecordHeader>,
    stream: &str,
    scope: &records::feature::scope::DesignParameterScope,
    frames: &[records::feature::assembly::DesignAssemblyOperandFrame; 2],
    targets: &[&records::feature::assembly::DesignAssemblyAxialOperandTarget; 2],
) -> bool {
    targets
        .iter()
        .copied()
        .zip(frames)
        .all(|(target, frame)| match target {
            records::feature::assembly::DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index,
                construction_record_index,
                construction_class_tag,
                construction_byte_offset,
                construction_transform_offset,
                axis_record_index_offsets,
                construction_paired_class_tag: _,
                construction_paired_byte_offset,
                selectors,
            } => {
                let selectors_ordered = selectors[0].axis_byte_offset
                    < selectors[0].selector_byte_offset
                    && selectors[0].selector_byte_offset < selectors[0].role_byte_offset
                    && selectors[0].role_byte_offset < selectors[1].axis_byte_offset
                    && selectors[1].axis_byte_offset < selectors[1].selector_byte_offset
                    && selectors[1].selector_byte_offset < selectors[1].role_byte_offset
                    && selectors[1].role_byte_offset < *construction_byte_offset;
                let component_scopes = native
                    .design_parameter_scopes
                    .iter()
                    .filter(|target_scope| {
                        design_stream(&target_scope.id) == stream
                            && target_scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::ComponentInsert
                            && target_scope.record_index == *component_insert_scope_record_index
                            && target_scope.component_insert_construction().is_some_and(
                                |construction| {
                                    construction
                                        .neutron_role
                                        .eq_ignore_ascii_case(selectors[0].occurrence_role.as_str())
                                },
                            )
                    })
                    .count();
                frame.reference_record_index == *construction_record_index
                    && scope
                        .reference_members()
                        .values()
                        .filter(|record_index| **record_index == *construction_record_index)
                        .count()
                        == 1
                    && *construction_byte_offset > scope.paired_byte_offset()
                    && construction_byte_offset.checked_add(48)
                        == Some(*construction_transform_offset)
                    && construction_byte_offset.checked_add(193)
                        == Some(axis_record_index_offsets[0])
                    && construction_byte_offset.checked_add(209)
                        == Some(axis_record_index_offsets[1])
                    && construction_byte_offset.checked_add(380)
                        == Some(*construction_paired_byte_offset)
                    && design_header_matches(
                        records_by_index,
                        stream,
                        *construction_record_index,
                        construction_class_tag.as_str(),
                        *construction_byte_offset,
                    )
                    && selectors_ordered
                    && valid_axial_selector_identity(
                        records_by_index,
                        stream,
                        scope,
                        &selectors[0],
                        selectors[1].axis_byte_offset,
                    )
                    && valid_axial_selector_identity(
                        records_by_index,
                        stream,
                        scope,
                        &selectors[1],
                        *construction_byte_offset,
                    )
                    && selectors[0].selects_same_object(&selectors[1])
                    && selectors[0]
                        .occurrence_role
                        .as_str()
                        .eq_ignore_ascii_case(selectors[1].occurrence_role.as_str())
                    && component_scopes == 1
            }
            records::feature::assembly::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index,
            } => {
                frame.reference_record_index == *scope_record_index
                    && native
                        .design_parameter_scopes
                        .iter()
                        .filter(|target_scope| {
                            design_stream(&target_scope.id) == stream
                                && target_scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                && target_scope.record_index == *scope_record_index
                                && target_scope.joint_origin_transform() == Some(frame.transform)
                        })
                        .count()
                        == 1
            }
        })
}

use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

fn reload_native_arena<T: serde::de::DeserializeOwned>(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    name: &str,
) -> Result<Vec<T>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    if !ir.fragment {
        return namespace
            .arena_as_for_decode(decode, name)
            .map_err(Into::into);
    }
    let mut records = Vec::new();
    for record in namespace.arena_iter_as_for_decode(decode, name) {
        match record {
            Ok(record) => records.push(record),
            Err(error) => {
                let error = CodecError::from(error);
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
            }
        }
    }
    Ok(records)
}

/// Read-only indexes over the loaded `f3d` native namespace, shared by the
/// per-family validators. Every map is derived purely from the namespace and
/// borrows it for the duration of a [`validate_native`] call.
struct Ctx<'a, 'd> {
    decode: &'a DecodeContext<'d>,
    /// The decoded document, for model-side body, face, and edge identity.
    ir: &'a CadIr,
    /// The loaded native namespace.
    native: &'a native::F3dNative,
    /// Design record headers keyed by `(stream, record_index)`.
    records_by_index: HashMap<(&'a str, u32), &'a records::decal::DesignRecordHeader>,
    /// Construction recipes keyed by recipe id.
    recipes_by_id: HashMap<&'a str, &'a records::recipes::ConstructionRecipe>,
    /// Parameters keyed by `(stream, record_index)`.
    parameters_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameter>,
    /// Parameter owners keyed by `(stream, record_index)`.
    owners_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterOwner>,
    /// Parameter companions keyed by `(stream, record_index)`.
    companions_by_index: HashMap<(&'a str, u32), &'a records::parameters::DesignParameterCompanion>,
    /// Parameter scopes keyed by `(stream, record_index)`.
    scopes_by_index: HashMap<(&'a str, u32), &'a records::feature::scope::DesignParameterScope>,
    /// Entity headers keyed by `(stream, entity_suffix)`.
    entities_by_suffix: HashMap<(&'a str, u64), &'a records::entity_header::DesignEntityHeader>,
    /// Sketch geometry record indices keyed by `(stream, record_index)`.
    sketch_geometry_indices: HashSet<(&'a str, u32)>,
    /// Sketch placements keyed by `(stream, scope_record_index)`.
    placements_by_scope:
        HashMap<(&'a str, u32), &'a records::sketch_placement::DesignSketchPlacement>,
    /// Extrude selection groups keyed by `(stream, record_index)`.
    groups_by_index: HashMap<
        (&'a str, u32),
        &'a records::topology::extrude_selection::DesignExtrudeSelectionGroup,
    >,
    /// Construction operand groups keyed by `(stream, record_index)`.
    operand_groups_by_index: HashMap<
        (&'a str, u32),
        &'a records::topology::construction::DesignConstructionOperandGroup,
    >,
    /// Extrude selection members keyed by `(stream, group_record_index, ordinal)`.
    members_by_slot: HashMap<
        (&'a str, u32, u32),
        &'a records::topology::extrude_selection::DesignExtrudeSelectionMember,
    >,
    /// Sketch owner entity ids keyed by `(stream, suffix)`.
    sketch_owner_ids: HashMap<(&'a str, u32), &'a str>,
}

fn push_constant_finding(
    findings: &mut Vec<Finding>,
    check: Check,
    message: &'static str,
    entity: Option<String>,
) {
    findings.push(Finding {
        related: Vec::new(),
        check,
        severity: Severity::Error,
        message: message.into(),
        entity,
    });
}

impl<'a, 'd> Ctx<'a, 'd> {
    /// Build every shared index over `native` up front. All builds are pure and
    /// emit no findings, so their eager construction does not affect the
    /// observable finding order.
    fn new(ir: &'a CadIr, native: &'a native::F3dNative, decode: &'a DecodeContext<'d>) -> Self {
        let records_by_index = native
            .design_record_headers
            .iter()
            .map(|record| ((design_stream(&record.id), record.record_index), record))
            .collect::<std::collections::HashMap<_, _>>();
        let recipes_by_id = native
            .construction_recipes
            .iter()
            .map(|recipe| (recipe.id.as_str(), recipe))
            .collect::<std::collections::HashMap<_, _>>();
        let parameters_by_index = native
            .design_parameters
            .iter()
            .map(|parameter| {
                (
                    (design_stream(&parameter.id), parameter.record_index),
                    parameter,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let owners_by_index = native
            .design_parameter_owners
            .iter()
            .map(|owner| ((design_stream(owner.id()), owner.record_index()), owner))
            .collect::<std::collections::HashMap<_, _>>();
        let companions_by_index = native
            .design_parameter_companions
            .iter()
            .map(|companion| {
                (
                    (design_stream(companion.id()), companion.record_index()),
                    companion,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let scopes_by_index = native
            .design_parameter_scopes
            .iter()
            .map(|scope| ((design_stream(&scope.id), scope.record_index), scope))
            .collect::<std::collections::HashMap<_, _>>();
        let entities_by_suffix = native
            .design_entity_headers
            .iter()
            .map(|entity| {
                (
                    (design_stream(&entity.id), entity.entity_id.suffix()),
                    entity,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let sketch_geometry_indices = native
            .sketch_points
            .iter()
            .map(|point| (design_stream(&point.id), point.record_index))
            .chain(
                native
                    .sketch_curve_identities
                    .iter()
                    .map(|curve| (design_stream(&curve.id), curve.record_index)),
            )
            .collect::<std::collections::HashSet<_>>();
        let placements_by_scope = native
            .design_sketch_placements
            .iter()
            .filter_map(|placement| {
                Some((
                    (design_stream(&placement.id), placement.scope_record_index?),
                    placement,
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let groups_by_index = native
            .design_extrude_selection_groups
            .iter()
            .map(|group| ((design_stream(&group.id), group.record_index), group))
            .collect::<std::collections::HashMap<_, _>>();
        let operand_groups_by_index = native
            .design_construction_operand_groups
            .iter()
            .map(|group| ((design_stream(&group.id), group.record_index), group))
            .collect::<std::collections::HashMap<_, _>>();
        let members_by_slot = native
            .design_extrude_selection_members
            .iter()
            .map(|member| {
                (
                    (
                        design_stream(&member.id),
                        member.group_record_index,
                        member.group_member_ordinal,
                    ),
                    member,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let sketch_owner_ids = native
            .design_entity_headers
            .iter()
            .filter(|header| header.in_sketch_module())
            .filter_map(|header| {
                Some((
                    (
                        design_stream(&header.id),
                        u32::try_from(header.entity_id.suffix()).ok()?,
                    ),
                    header.entity_id.as_str(),
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        Ctx {
            decode,
            ir,
            native,
            records_by_index,
            recipes_by_id,
            parameters_by_index,
            owners_by_index,
            companions_by_index,
            scopes_by_index,
            entities_by_suffix,
            sketch_geometry_indices,
            placements_by_scope,
            groups_by_index,
            operand_groups_by_index,
            members_by_slot,
            sketch_owner_ids,
        }
    }

    /// Whether the document holds the design-intent records and the record
    /// headers they read. A decode below full depth stops before them, and a
    /// contained design-intent failure drops them, so in either case the
    /// stored scopes and owners carry no design-intent bindings.
    fn holds_design_intent(&self) -> bool {
        self.ir.decode_depth() == DecodeDepth::Full && self.native.design_intent_failures.is_empty()
    }
}

/// Validate the `f3d` native namespace of `ir` against its decoded IR.
pub(crate) fn validate_native(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<Finding>, CodecError> {
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(Vec::new());
    };
    let loaded = if ir.fragment {
        native::F3dNative::load_fragment(decode, namespace)
    } else {
        native::F3dNative::load_inner(decode, namespace).map(|native| (native, Vec::new()))
    };
    let (native, skipped) = match loaded.map_err(CodecError::from) {
        Ok(native) => native,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            return Ok(vec![Finding {
                related: Vec::new(),
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "Fusion native namespace does not match the expected arena shape".into(),
                entity: None,
            }]);
        }
    };
    let mut findings = validate_loaded(decode, ir, &native)?;
    findings.extend(skipped.into_iter().map(|reason| Finding {
        check: Check::FragmentSkipped,
        severity: Severity::Info,
        entity: None,
        related: Vec::new(),
        message: format!("skipped native arena row: {reason}"),
    }));
    Ok(findings)
}

fn validate_loaded(
    decode: &DecodeContext<'_>,
    ir: &CadIr,
    native: &native::F3dNative,
) -> Result<Vec<Finding>, CodecError> {
    let ctx = Ctx::new(ir, native, decode);
    let mut findings = Vec::new();
    let mut expected_face_operands = reload_native_arena(decode, ir, "design_face_operands")?;
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    history::bind_face_operand_history_candidates(
        decode,
        &mut expected_face_operands,
        &native.design_parameter_scopes,
        &native.design_construction_operand_groups,
        &native.construction_recipes,
        history::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
    )?;
    let decoded_profile_face_groups = native
        .design_face_operands
        .iter()
        .filter_map(|operand| Some((design_stream(&operand.id), operand.group_record_index()?)))
        .collect::<std::collections::HashSet<_>>();
    let face_group_members = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| {
            group.extrude_role().is_some_and(|role| {
                matches!(
                    role,
                    records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                )
            }) || (group.extrude_role()
                == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
                && decoded_profile_face_groups
                    .contains(&(design_stream(&group.id), group.record_index)))
        })
        .flat_map(|group| {
            let native_stream = design_stream(&group.id);
            group
                .members()
                .iter()
                .map(|member| &member.value)
                .map(move |member| (native_stream, group.scope_record_index, *member))
        })
        .collect::<BTreeSet<_>>();
    validate_act(&ctx, &mut findings)?;
    validate_body_bindings(&ctx, &mut findings)?;
    validate_body_bounds(&ctx, &mut findings);
    validate_canvas_images(&ctx, &mut findings);
    validate_decal_images(&ctx, &mut findings);
    validate_mesh_features(&ctx, &mut findings);
    validate_component_occurrences(&ctx, &mut findings);
    validate_xref_placement_sources(&ctx, &mut findings)?;
    validate_configurations(&ctx, &mut findings);
    validate_design_intent_failures(&ctx, &mut findings);
    validate_feature_timelines(&ctx, &mut findings)?;
    validate_parameter_scopes(&ctx, &mut findings)?;
    emboss::validate(&ctx, &mut findings);
    sheet_metal_settings::validate(&ctx, &mut findings);
    validate_extrude_selection_groups(&ctx, &mut findings);
    validate_construction_operand_groups(&ctx, &mut findings);
    validate_path_feature_operand_roles(&ctx, &mut findings);
    validate_extrude_parameter_operands(&ctx, &mut findings);
    let fillet_radius_group_records = validate_fillet_radius_groups(&ctx, &mut findings);
    validate_fillet_operand_groups(&ctx, &mut findings, &fillet_radius_group_records);
    let operand_identity_groups = validate_construction_operand_identities(&ctx, &mut findings);
    let edge_identity_records =
        validate_edge_identity_operands(decode, &ctx, &mut findings, &expected_face_operands)?;
    let body_recipe_operand_records = validate_body_recipe_operands(decode, &ctx, &mut findings)?;
    let edge_operand_records = validate_edge_operands(decode, &ctx, &mut findings)?;
    let edge_treatment_vertex_records =
        validate_edge_treatment_vertex_operands(decode, &ctx, &mut findings)?;
    validate_operand_group_carriers(
        &ctx,
        &mut findings,
        &operand_identity_groups,
        &edge_identity_records,
        &body_recipe_operand_records,
        &edge_operand_records,
        &edge_treatment_vertex_records,
    );
    validate_extrude_selection_members(&ctx, &mut findings);
    validate_entity_selection_operands(&ctx, &mut findings);
    for selection in &native.design_profile_curve_selections {
        validate_profile_curve_selection(selection, &mut findings);
    }
    validate_extrude_selection_group_members(&ctx, &mut findings);
    validate_edge_treatment_groups(
        &ctx,
        &mut findings,
        &edge_operand_records,
        &edge_identity_records,
        &edge_treatment_vertex_records,
    );
    let face_operand_records =
        validate_face_operands(&ctx, &mut findings, &expected_face_operands)?;
    // A one-member termination group whose member is not a face operand is
    // resolved by the construction operand identity that the group owns.
    let identity_group_members = native
        .design_construction_operand_groups
        .iter()
        .filter(|group| {
            let stream = design_stream(&group.id);
            group.members().len() == 1
                && group.extrude_role().is_some_and(|role| {
                    matches!(
                        role,
                        records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                    )
                })
                && native
                    .design_construction_operand_identities
                    .iter()
                    .any(|identity| {
                        design_stream(&identity.id) == stream
                            && identity.group_record_index == group.record_index
                    })
        })
        .flat_map(|group| {
            let stream = design_stream(&group.id);
            group
                .members()
                .iter()
                .map(move |member| (stream, group.scope_record_index, member.value))
        })
        .collect::<HashSet<_>>();
    validate_face_group_member_resolution(
        &mut findings,
        face_group_members,
        &face_operand_records,
        &identity_group_members,
        &native.design_entity_selection_operands,
        &native.design_parameter_scopes,
    );
    validate_face_source_groups(&ctx, &mut findings);
    validate_sketch_placements(&ctx, &mut findings);
    validate_parameter_owners(&ctx, &mut findings);
    validate_parameter_companions(&ctx, &mut findings);
    let dimension_recipe_ids = validate_dimension_recipe_records(&ctx, &mut findings);
    validate_dimension_companion_recipes(&ctx, &mut findings, &dimension_recipe_ids);
    let locus_pair_companions = validate_dimension_locus_pairs(&ctx, &mut findings);
    validate_dimension_annotation_frames(&ctx, &mut findings);
    validate_dimension_presentation_frames(&ctx, &mut findings);
    let locus_group_companions = validate_dimension_locus_groups(&ctx, &mut findings);
    validate_dimension_null_locus_pairs(
        &ctx,
        &mut findings,
        &locus_pair_companions,
        &locus_group_companions,
    );
    validate_parameters(&ctx, &mut findings);
    validate_entity_headers(&ctx, &mut findings);
    validate_sketch_relations(&ctx, &mut findings);
    validate_sketch_geometry_identities(&ctx, &mut findings);
    validate_sketch_relation_owners(&ctx, &mut findings);
    validate_body_links(&ctx, &mut findings);
    validate_subentity_tags(&ctx, &mut findings);
    validate_history_graphs(&ctx, &mut findings);
    Ok(findings)
}

/// Validate ACT record identity, table/group joins, ordered registries, and the
/// stored document-root discriminator.
fn validate_act(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut record_indices = HashSet::new();
    for entity in &native.act_entities {
        let stream = entity.stream();

        let unique_index = record_indices.insert((stream, entity.record_index()));
        if !unique_index {
            push_constant_finding(findings, Check::NativeLinks, "Fusion ACT entity has an invalid identity, table membership, or change-group frame", Some(entity.id().to_owned()));
        }
    }

    let mut guid_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut guid_offsets = HashSet::new();
    for guid in &native.act_guids {
        let stream = guid.stream();

        guid_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), guid.id().as_str()));
        let unique_ordinal = guid_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT GUID stream index missing"))?
            .0
            .insert(guid.ordinal);
        let unique_offset = guid_offsets.insert((stream, guid.byte_offset()));
        let valid = unique_offset && unique_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT GUID-pool entry has an invalid identity, ordinal, offset, or GUID",
                Some(guid.id().to_owned()),
            );
        }
    }

    let mut table_reference_ordinals =
        std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut table_reference_offsets = HashSet::new();
    for reference in &native.act_table_references {
        let stream = reference.stream();

        table_reference_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), reference.id().as_str()));
        let unique_ordinal = table_reference_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT table stream index missing"))?
            .0
            .insert(reference.ordinal);
        let unique_offset = table_reference_offsets.insert((stream, reference.byte_offset()));
        let valid = unique_ordinal && unique_offset;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT table reference has an invalid identity, ordinal, or offset",
                Some(reference.id().to_owned()),
            );
        }
    }

    let mut registry_ordinals = std::collections::BTreeMap::<&str, (HashSet<u32>, &str)>::new();
    let mut registry_offsets = HashSet::new();
    let mut registry_names = HashSet::new();
    for channel in &native.act_registry_channels {
        let stream = channel.stream();

        registry_ordinals
            .entry(stream)
            .or_insert((HashSet::new(), channel.id().as_str()));
        let unique_ordinal = registry_ordinals
            .get_mut(stream)
            .ok_or_else(|| CodecError::malformed("F3D ACT registry stream index missing"))?
            .0
            .insert(channel.ordinal);
        let unique_offset = registry_offsets.insert((stream, channel.byte_offset()));
        let unique_name = registry_names.insert((stream, channel.name()));
        let valid = unique_offset && unique_name && unique_ordinal;
        if !valid {
            push_constant_finding(findings, Check::NativeLinks, "Fusion ACT channel-registry entry has an invalid identity, ordinal, offset, name, or GUID", Some(channel.id().to_owned()));
        }
    }

    let mut root_counts = BTreeMap::<&str, (&str, usize)>::new();
    for root in &native.act_component_links {
        let stream = root.stream();

        root_counts
            .entry(stream)
            .or_insert((root.id().as_str(), 0))
            .1 += usize::from(act_link_is_document_root(
            native,
            stream,
            root.references.tracked_entity,
        ));
        let unique_record_index = record_indices.insert((stream, root.record_index));
        if !unique_record_index {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT component link repeats an ACT record index",
                Some(root.id().to_owned()),
            );
        }
    }

    if ctx.ir.fragment {
        findings.push(Finding {
            check: Check::FragmentSkipped, severity: Severity::Info, entity: None,
            related: Vec::new(),
            message: "skipped f3d.act_document_root and f3d.act_ordinal_coverage: complete ACT stream required".into(),
        });
        return Ok(());
    }
    for (witness, count) in root_counts.into_values() {
        if count != 1 {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ACT stream does not have exactly one document-root component link",
                Some(witness.to_owned()),
            );
        }
    }
    for (ordinals, witness, family) in guid_ordinals
        .into_values()
        .map(|(ordinals, witness)| (ordinals, witness, "GUID pool"))
        .chain(
            table_reference_ordinals
                .into_values()
                .map(|(ordinals, witness)| (ordinals, witness, "table reference")),
        )
        .chain(
            registry_ordinals
                .into_values()
                .map(|(ordinals, witness)| (ordinals, witness, "channel registry")),
        )
    {
        let contiguous = id_from_index(ordinals.len()).is_some_and(|length| {
            ordinals
                .iter()
                .max()
                .and_then(|maximum| maximum.checked_add(1))
                == Some(length)
        });
        if !contiguous {
            let message = match family {
                "GUID pool" => "Fusion ACT GUID pool ordinals are not contiguous from zero",
                "table reference" => {
                    "Fusion ACT table reference ordinals are not contiguous from zero"
                }
                _ => "Fusion ACT channel registry ordinals are not contiguous from zero",
            };
            push_constant_finding(
                findings,
                Check::NativeLinks,
                message,
                Some(witness.to_owned()),
            );
        }
    }
    Ok(())
}

/// Validate unique configuration entries and a single authored table.
fn validate_configurations(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let mut entry_names = HashSet::new();
    for configuration in &ctx.native.design_configurations {
        let name = configuration.entry_name().as_str();
        if !entry_names.insert(name) {
            let id = configuration.id();
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design configuration entry name is duplicated",
                Some(id),
            );
        }
    }
    let mut nonempty_tables = ctx
        .native
        .design_configurations
        .iter()
        .filter(|configuration| {
            configuration
                .tables()
                .iter()
                .any(|table| !table.rows.is_empty())
        });
    let first = nonempty_tables.next();
    if nonempty_tables.next().is_some() {
        let id = first.map(super::records::configuration::DesignConfiguration::id);
        push_constant_finding(
            findings,
            Check::NativeLinks,
            "Fusion Design configurations have no single authored table order",
            id,
        );
    }
}

/// Report a contained design-intent failure beside features or sketches of the
/// same document. A failure record turns off the checks that resolve design
/// intent, so it must not sit beside the records those checks read. An F3Z
/// member's failure is scoped to its occurrence; the other documents of the
/// archive keep their design records.
fn validate_design_intent_failures(ctx: &Ctx, findings: &mut Vec<Finding>) {
    for failure in &ctx.native.design_intent_failures {
        let beside_design = ctx
            .ir
            .model
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .chain(
                ctx.ir
                    .model
                    .sketches
                    .iter()
                    .map(|sketch| sketch.id.as_str()),
            )
            .any(|id| crate::ids::same_native_occurrence(id, &failure.id));
        if beside_design {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion design-intent failure record accompanies decoded design records",
                Some(failure.id.clone()),
            );
        }
    }
}

/// Validate authored Design timeline order and its exact type and scope joins.
/// Scope history order is checked only in a document that holds the ASM
/// histories qualifying each history state.
fn validate_feature_timelines(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    let native = ctx.native;
    let mut type_ordinals = HashMap::<&str, u32>::new();
    let mut timeline_ordinals = HashMap::<&str, u32>::new();
    let mut entity_type_counts = HashMap::<(&str, u64), usize>::new();
    let mut expected = BTreeMap::<(&str, u64), (String, u32, bool, &str)>::new();
    let mut design_types = native.design_types.iter().collect::<Vec<_>>();
    design_types.sort_by(|left, right| {
        (
            ids::native_stream(left.id()).unwrap_or_default(),
            left.byte_offset,
        )
            .cmp(&(
                ids::native_stream(right.id()).unwrap_or_default(),
                right.byte_offset,
            ))
    });
    for design_type in design_types {
        let Some(meta_stream) = ids::native_stream(design_type.id()) else {
            continue;
        };
        let Some(segment) = ids::design_segment(design_type.id()) else {
            continue;
        };

        let type_ordinal = type_ordinals.entry(meta_stream).or_default();
        let class_tag = type_ordinal.checked_add(256).map(|tag| tag.to_string());
        *type_ordinal = type_ordinal.checked_add(1).ok_or_else(|| {
            CodecError::Malformed("F3D feature timeline type ordinal overflows".into())
        })?;
        for entity_id in design_type.entities.values() {
            *entity_type_counts.entry((segment, *entity_id)).or_default() += 1;
        }
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID)
        {
            continue;
        }

        let source_ordinal = timeline_ordinals.entry(segment).or_default();
        for entity_id in design_type.entities.values() {
            let valid_type =
                crate::design::decode::meta::is_supported_feature_timeline_type(design_type)
                    && class_tag.as_ref().is_some_and(|tag| {
                        records::references::DesignClassTag::try_from(tag.clone()).is_ok()
                    });
            let Some(class_tag) = class_tag.clone() else {
                continue;
            };

            if expected
                .insert(
                    (segment, *entity_id),
                    (
                        class_tag,
                        *source_ordinal,
                        valid_type,
                        design_type.id().as_str(),
                    ),
                )
                .is_some()
            {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design feature-timeline type repeats an entity identity",
                    Some(design_type.id().to_owned()),
                );
            }
            *source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("F3D feature timeline source ordinal overflows".into())
            })?;
        }
    }

    let mut actual = native.design_feature_timelines.iter().collect::<Vec<_>>();
    actual.sort_by(|left, right| {
        (left.segment(), left.source_ordinal).cmp(&(right.segment(), right.source_ordinal))
    });
    let mut actual_records = HashSet::<(&str, u64)>::new();
    let mut item_records = HashSet::<(&str, u64)>::new();
    for timeline in actual {
        let segment = timeline.segment();
        let expected_type = expected.get(&(segment, timeline.record_index.get()));
        let unique_record = actual_records.insert((segment, timeline.record_index.get()));
        let record_valid =
            expected_type.is_some_and(|(class_tag, source_ordinal, valid_type, _)| {
                *valid_type
                    && timeline.class_tag.as_str() == class_tag
                    && timeline.source_ordinal == *source_ordinal
            }) && entity_type_counts.get(&(segment, timeline.record_index.get())) == Some(&1)
                && entity_type_counts.get(&(segment, timeline.context_record_index.get()))
                    == Some(&1)
                && unique_record;
        let mut items_valid = true;
        for item in timeline.frame().items().iter().map(|item| item.value) {
            items_valid &= entity_type_counts.get(&(segment, item)) == Some(&1)
                && item_records.insert((segment, item));
        }
        if !record_valid || !items_valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature timeline has an invalid typed frame",
                Some(timeline.id().to_owned()),
            );
        }
    }
    for ((segment, entity_id), (_, _, _, type_id)) in &expected {
        if !actual_records.contains(&(*segment, *entity_id)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design feature-timeline type has no decoded record",
                Some((*type_id).to_owned()),
            );
        }
    }

    let mut scope_positions = HashMap::<&str, u64>::new();
    match crate::design::feature_project::authored_scope_ordinals_per_stream(
        &native.design_parameter_scopes,
        &native.design_feature_timelines,
    ) {
        Ok(authored) => {
            for scope in &native.design_parameter_scopes {
                let stream = design_stream(&scope.id);
                let Some(position) = authored.get(&(stream, scope.record_index)) else {
                    continue;
                };

                scope_positions.insert(scope.id.as_str(), *position);
            }
        }
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => {
            let entity = native
                .design_parameter_scopes
                .first()
                .map(|scope| scope.id.clone());
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scopes have no complete authored order",
                entity,
            );
        }
    }

    if !ctx.holds_design_intent() {
        return Ok(());
    }
    let scope_history = crate::design::feature_project::ScopeHistoryGraph::new(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.design_component_naming_spaces,
        &native.asm_histories,
    );
    for scope in &native.design_parameter_scopes {
        let Some(position) = scope_positions.get(scope.id.as_str()).copied() else {
            continue;
        };
        match scope_history.predecessor(scope, |candidate| {
            scope_positions.contains_key(candidate.id.as_str())
        }) {
            Ok(crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor)) => {
                if scope_positions
                    .get(predecessor.id.as_str())
                    .is_some_and(|predecessor| *predecessor >= position)
                {
                    push_constant_finding(
                        findings,
                        Check::NativeLinks,
                        "Fusion Design history edge runs forward in its feature timeline",
                        Some(scope.id.clone()),
                    );
                }
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design scope history-state dependency is cyclic",
                Some(scope.id.clone()),
            ),
            Ok(
                crate::design::feature_project::ScopeHistoryPredecessor::None
                | crate::design::feature_project::ScopeHistoryPredecessor::Ambiguous,
            ) => {}
        }
    }
    Ok(())
}

fn mesh_record_offset_is(
    record: &records::mesh::DesignMeshRecordIdentity,
    relative: u64,
    offset: u64,
) -> bool {
    record.byte_offset().checked_add(relative) == Some(offset)
}

/// Validate complete `Base Mesh Feature` record graphs and their neutral links.
/// The neutral feature link is checked only in a document that holds features.
fn validate_mesh_features(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let mut feature_ids = HashSet::new();
    let mut scope_records = HashSet::new();
    let mut collection_records = HashSet::new();
    let mut body_records = HashSet::new();
    let mut entry_records = HashSet::new();
    let mut guid_records = HashSet::new();
    let mut wrapper_records = HashSet::new();
    let mut state_records = HashSet::new();
    let mut node_records = HashSet::new();
    let mut auxiliary_records = HashSet::new();
    let mut collection_owner_records = HashSet::new();
    let mut body_owner_records = HashMap::new();
    let mut texture_table_records = HashSet::new();
    let mut filename_records = HashMap::new();
    let mut projected_tessellations = HashSet::new();
    let asset_ids = ctx
        .ir
        .model
        .assets
        .iter()
        .map(|asset| &asset.id)
        .collect::<std::collections::HashSet<_>>();
    let tessellation_ids = ctx
        .ir
        .model
        .tessellations
        .iter()
        .map(|tessellation| tessellation.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for feature in &ctx.native.design_mesh_features {
        let stream = design_stream(&feature.id);
        let scope = ctx
            .scopes_by_index
            .get(&(stream, feature.scope().record().record_index()));
        let mut valid = feature_ids.insert(feature.id.as_str())
            && scope_records.insert((stream, feature.scope().record().record_index()))
            && collection_records.insert((stream, feature.collection().record().record_index()))
            && texture_table_records
                .insert((stream, feature.texture_table.record().record_index()))
            && collection_owner_records
                .insert((stream, feature.collection_owner.record().record_index()))
            && feature
                .scope()
                .record()
                .byte_offset()
                .checked_add(scope.map_or(0, |scope| scope.frame_length()))
                == Some(feature.scope().base_record().byte_offset())
            && mesh_record_offset_is(
                feature.collection_owner.record(),
                262,
                feature.collection_owner.backlink_offset(),
            )
            && scope.is_some_and(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature
                    && scope.byte_offset() == feature.scope().record().byte_offset()
                    && scope.paired_byte_offset() == feature.scope().base_record().byte_offset()
            });

        let mut resources = feature.texture_table.resources().iter().collect::<Vec<_>>();
        resources.sort_by_key(|left| left.filename_ordinal);
        let mut resources_valid = true;
        for resource in &resources {
            let filename_key = (stream, resource.file.record().record_index());
            let filename_record_consistent = filename_records
                .get(&filename_key)
                .is_none_or(|record| *record == resource.file.record());

            filename_records
                .entry(filename_key)
                .or_insert(resource.file.record());
            resources_valid = filename_record_consistent && asset_ids.contains(&resource.asset);
            if !resources_valid {
                break;
            }
        }
        valid &= resources_valid;

        for body in feature.bodies() {
            let owner_key = (stream, body.owner_record.record_index());
            let owner_consistent = body_owner_records
                .get(&owner_key)
                .is_none_or(|record| *record == &body.owner_record);

            body_owner_records
                .entry(owner_key)
                .or_insert(&body.owner_record);
            let body_valid = body_records.insert((stream, body.placement.record().record_index()))
                && entry_records.insert((stream, body.entry.record().record_index()))
                && guid_records.insert((stream, body.guid.record().record_index()))
                && wrapper_records.insert((stream, body.wrapper_record.record_index()))
                && state_records.insert((stream, body.scene_state.record().record_index()))
                && node_records.insert((stream, body.scene_node.record_index()))
                && auxiliary_records.insert((stream, body.scene_auxiliary_record.record_index()))
                && owner_consistent
                && body.scene_node.frame_length() == 133;
            let projection_valid = if body_valid {
                match body.tessellation_id.as_deref() {
                    Some(id) => tessellation_ids.contains(id) && projected_tessellations.insert(id),
                    None => true,
                }
            } else {
                false
            };
            valid &= body_valid && projection_valid;
        }
        let projected = || {
            feature
                .bodies()
                .iter()
                .filter_map(|body| body.tessellation_id.as_deref())
        };
        if ctx.holds_design_intent() && projected().next().is_some() {
            valid &= scope.is_some_and(|scope| {
                ctx.ir.model.features.iter().any(|neutral| {
                    neutral.native_ref.as_deref() == Some(scope.id.as_str())
                        && matches!(
                            neutral.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::MeshImport { tessellations })
                                if tessellations.iter().map(String::as_str).eq(projected())
                        )
                })
            });
        }
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design mesh feature has an invalid frame or object graph",
                Some(feature.id.clone()),
            );
        }
    }
}

/// Validate Canvas scope and Design object joins.
fn validate_canvas_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut scope_bindings = HashSet::new();
    let mut geometry_records = HashSet::new();
    let geometry_entities = native
        .design_types
        .iter()
        .filter(|design_type| {
            matches!(
                design_type.module.as_str(),
                records::entity_header::DESIGN_MODULE_BODY
                    | records::entity_header::DESIGN_MODULE_GEOMETRY
            )
        })
        .flat_map(|design_type| {
            let design_segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (design_segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    let component_entities = native
        .design_types
        .iter()
        .filter(|design_type| {
            matches!(
                design_type.module.as_str(),
                records::entity_header::DESIGN_MODULE_FUSION
                    | records::entity_header::DESIGN_MODULE_COMPONENT
            )
        })
        .flat_map(|design_type| {
            let design_segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (design_segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    for image in &native.design_canvas_images {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, image.scope_record_index));
        let scope_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Canvas
        });
        let scope_unique = if scope_valid {
            scope_bindings.insert((native_stream, image.scope_record_index))
        } else {
            false
        };
        let geometry_unique = if scope_unique {
            geometry_records.insert((native_stream, image.geometry().record_index()))
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && geometry_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && geometry_entities.contains(&(design_segment, u64::from(image.plane_entity_suffix)))
            && component_entities
                .contains(&(design_segment, u64::from(image.component_entity_suffix)));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Canvas image has an invalid frame or Design object join",
                Some(image.id.clone()),
            );
        }
    }
}

/// Validate Decal native and neutral object joins.
fn validate_decal_images(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    const TARGET_ROLE: DesignOperandRole = DesignOperandRole::BODIES_A;
    let mut scope_bindings = HashSet::new();
    let mut asset_records = HashSet::new();
    let fusion_entities = ctx
        .native
        .design_types
        .iter()
        .filter(|design_type| design_type.module == records::entity_header::DESIGN_MODULE_FUSION)
        .flat_map(|design_type| {
            let segment = ids::design_segment(design_type.id());
            design_type
                .entities
                .values()
                .map(move |suffix| (segment, *suffix))
        })
        .collect::<std::collections::HashSet<_>>();
    for image in &ctx.native.design_decal_images {
        let native_stream = design_stream(&image.id);
        let design_segment = ids::design_segment(&image.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, image.scope_record_index()));
        let group = ctx
            .operand_groups_by_index
            .get(&(native_stream, image.target_group_record_index));
        let operand = group.and_then(|group| {
            let member = group.members().first()?.value;
            ctx.native
                .design_body_recipe_operands
                .iter()
                .find(|operand| {
                    design_stream(&operand.id) == native_stream
                        && operand.scope_record_index == image.scope_record_index()
                        && operand.record_index() == member
                        && operand.owner.group() == Some((group.record_index, 0))
                })
        });
        let projected =
            if image.mapping_mode == crate::records::decal::DesignDecalMappingMode::FitToFaces {
                if let Some(operand) = operand {
                    let mut faces = (operand
                        .references()
                        .iter()
                        .flat_map(|reference| reference.candidate_faces.iter()))
                    .cloned()
                    .collect::<Vec<_>>();
                    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
                    faces.dedup();
                    (!faces.is_empty()).then_some((operand, faces))
                } else {
                    None
                }
            } else {
                None
            };
        let neutral_is_valid = projected.is_none_or(|(operand, expected_faces)| {
            scope.is_some_and(|scope| {
                ctx.ir.model.features.iter().any(|feature| {
                    feature.native_ref.as_deref() == Some(scope.id.as_str())
                        && matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Decal {
                                asset,
                                faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native },
                                mapping: cadmpeg_ir::features::DecalMapping::FitToFaces,
                                opacity: None,
                            }) if faces == &expected_faces
                                && native == &operand.id
                                && ctx.ir.model.assets.iter().any(|candidate| {
                                    candidate.id == *asset
                                        && candidate.name.as_ref().map(cadmpeg_core::text::NonBlankString::as_str) == Some(image.asset.name())
                                })
                        )
                })
            })
        });
        let scope_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal
        });
        let scope_unique = if scope_valid {
            scope_bindings.insert((native_stream, image.scope_record_index()))
        } else {
            false
        };
        let asset_unique = if scope_unique {
            asset_records.insert((native_stream, image.asset.record_index()))
        } else {
            false
        };
        let valid = scope_valid
            && scope_unique
            && asset_unique
            && scope.is_some_and(|scope| scope.byte_offset() == image.scope_byte_offset())
            && fusion_entities.contains(&(design_segment, u64::from(image.asset.entity_suffix())))
            && group.is_some_and(|group| {
                group.scope_record_index == image.scope_record_index()
                    && group.role() == TARGET_ROLE
                    && group.members().len() == 1
            })
            && operand.is_some()
            && neutral_is_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Decal image has an invalid frame or Design object join",
                Some(image.id.clone()),
            );
        }
    }
}

/// Validate the ordered Design body-map binding entries and their pair runs.
fn validate_body_bindings(
    ctx: &Ctx<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let native = ctx.native;
    let ownership_types = body_ownership::OwnerTypes::new(native, ctx.decode)?;
    let mut binding_offsets = HashSet::new();
    let mut binding_groups =
        std::collections::HashMap::<(&str, u64), Vec<&records::bodies::DesignBodyBinding>>::new();
    for binding in &native.design_body_bindings {
        let native_stream = design_stream(binding.id());
        let source_outside_fragment = ctx.ir.fragment
            && binding.body.is_some()
            && !native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
            });
        if source_outside_fragment {
            findings.push(Finding { check: Check::FragmentSkipped, severity: Severity::Info,
                entity: Some(binding.id().to_owned()), related: Vec::new(),
                message: "skipped f3d.body_binding_source_resolution: source BREP keys are outside this fragment".into() });
        }
        let resolved_valid = if source_outside_fragment {
            true
        } else if let Some(body) = &binding.body {
            let has_named_source = native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && key.source_brep.as_deref() == Some(binding.blob_name())
            });
            let source_keys = native.body_native_keys.iter().filter(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && if has_named_source {
                        key.source_brep.as_deref() == Some(binding.blob_name())
                    } else {
                        key.source_brep.is_none()
                    }
            });
            match crate::brep::resolve_body_selector(source_keys, binding.asm_body_key) {
                Ok(Some(resolved)) => resolved == body,
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => false,
            }
        } else if binding.history_only {
            !native.body_native_keys.iter().any(|key| {
                ids::same_native_occurrence(key.source_namespace.as_str(), binding.id())
                    && key.source_brep.as_deref() == Some(binding.blob_name())
                    && key.asm_body_key == Some(binding.asm_body_key)
            })
        } else {
            true
        };
        let valid = design_stream_contains_entry(native_stream, binding.stream())
            && ownership_types.matches(binding)
            && resolved_valid
            && binding_offsets.insert((native_stream, binding.asm_body_key_offset()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body binding has an invalid ordered map entry",
                Some(binding.id().to_owned()),
            );
        }
        binding_groups
            .entry((native_stream, binding.blob_name_offset()))
            .or_default()
            .push(binding);
    }
    for bindings in binding_groups.values_mut() {
        bindings.sort_by_key(|left| left.pair_ordinal());
        let complete = bindings
            .first()
            .is_some_and(|first| usize::try_from(first.pair_count()).ok() == Some(bindings.len()))
            && bindings.iter().enumerate().all(|(ordinal, binding)| {
                usize::try_from(binding.pair_ordinal()).ok() == Some(ordinal)
                    && binding.pair_count() == bindings[0].pair_count()
                    && binding.blob_name() == bindings[0].blob_name()
                    && binding.stream() == bindings[0].stream()
            });
        if !complete {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body map has an incomplete ordered pair run",
                bindings.first().map(|binding| binding.id().to_owned()),
            );
        }
    }
    Ok(())
}

/// Validate each Design body-bounds repeated record frame.
fn validate_body_bounds(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut bounded_bodies = HashSet::new();
    for bounds in &native.design_body_bounds {
        let native_stream = design_stream(bounds.id());
        let mut expected_bindings = native
            .design_body_bindings
            .iter()
            .filter(|binding| {
                design_stream_contains_entry(native_stream, binding.stream())
                    && binding.entity_suffix == bounds.entity_suffix()
            })
            .collect::<Vec<_>>();
        expected_bindings.sort_by_key(|left| left.asm_body_key_offset());
        let valid_frame = ctx
            .entities_by_suffix
            .get(&(native_stream, bounds.entity_suffix()))
            .is_some_and(|entity| {
                entity.module() == Some(records::entity_header::DESIGN_MODULE_BODY)
                    && entity.byte_offset == bounds.entity_byte_offset()
            })
            && bounds.body_binding_ids().eq(expected_bindings
                .iter()
                .map(|binding| binding.id().as_str()));
        let valid = if valid_frame {
            bounded_bodies.insert((native_stream, bounds.entity_suffix()))
        } else {
            false
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body bounds have an invalid repeated record frame",
                Some(bounds.id().to_owned()),
            );
        }
    }
}

fn placement_ordered_references_hold(
    scope: &records::feature::scope::DesignParameterScope,
    alignment: &records::feature::assembly::DesignAssemblyAlignment,
    owners: &[(u32, u32)],
) -> bool {
    use crate::layout::assembly_as_built_scope_owner_run as layout;
    let Some(count) = design::assembly::as_built_additional_owner_count(scope.frame_length())
    else {
        return false;
    };
    let Some(run) = &alignment.additional_owner_run else {
        return false;
    };
    let table = scope.reference_members();
    usize::try_from(run.count.value) == Ok(count)
        && run.count.offset == scope.byte_offset() + u64_from_index(layout::ADDITIONAL_OWNER_COUNT)
        && run.references.len() == count
        && owners.len() == 4 + count
        && owners
            .iter()
            .enumerate()
            .all(|(ordinal, (local, _))| usize::try_from(*local) == Ok(ordinal))
        && alignment.owners.len() == 4
        && alignment
            .owners
            .iter()
            .zip(owners)
            .all(|(reference, (_, index))| reference.value == *index)
        && table.len() == 9 + count
        && (count != 0
            || table
                .values_in(5..9)
                .is_some_and(|values| values.eq(alignment.owners.iter().map(|owner| &owner.value))))
        && run
            .references
            .iter()
            .zip(owners.iter().skip(4))
            .enumerate()
            .all(|(ordinal, (reference, (_, index)))| {
                reference.value == *index
                    && reference.offset
                        == scope.byte_offset()
                            + u64_from_index(layout::ADDITIONAL_OWNER_REFERENCES + 11 * ordinal)
                    && table.values().any(|value| value == index)
            })
        && (0..table.len())
            .filter(|&start| {
                table.values_in(start..start + 4).is_some_and(|values| {
                    values.eq(alignment.owners.iter().map(|owner| &owner.value))
                })
            })
            .count()
            == 1
}

/// Validate feature parameter scopes and their paired feature-operation frames.
/// The links resolve into design-intent records, so a document without them
/// has nothing to check.
fn validate_parameter_scopes(ctx: &Ctx, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    if !ctx.holds_design_intent() {
        return Ok(());
    }
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let placements_by_scope = &ctx.placements_by_scope;
    let mut scope_indices = HashSet::new();
    let mut stream_types_by_stream = std::collections::HashMap::new();
    for scope in &native.design_parameter_scopes {
        let native_stream = design_stream(&scope.id);
        let stream_types = stream_types_by_stream
            .entry(native_stream)
            .or_insert_with(|| {
                native_stream
                    .strip_prefix("f3d:")
                    .and_then(ids::decode_identity_key_component)
                    .map(|entry| {
                        design::decode::meta::stream_types_by_entity(&native.design_types, &entry)
                    })
                    .unwrap_or_default()
            });
        let unique_index = scope_indices.insert((native_stream, scope.record_index));
        let entity_link = scope.sketch_entity().map(|binding| {
            entities_by_suffix
                .get(&(native_stream, binding.entity_id.suffix()))
                .is_some_and(|entity| {
                    entity.entity_id == binding.entity_id
                        && binding.entity_reference_offset > scope.byte_offset()
                        && binding.entity_reference_offset < scope.paired_byte_offset()
                })
        });
        let valid_sketch_profile =
            |profile: &records::topology::sketch_profile::DesignSketchProfileOperand| {
                let header = records_by_index.get(&(native_stream, profile.record_index));
                let entity = entities_by_suffix.get(&(native_stream, profile.entity_id.suffix()));
                usize::try_from(profile.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&profile.record_index)
                    && header.is_some_and(|header| {
                        header.byte_offset == profile.byte_offset()
                            && header.class_tag == profile.class_tag
                    })
                    && entity.is_some_and(|entity| {
                        entity.in_sketch_module() && entity.entity_id == profile.entity_id
                    })
                    && profile.region_selection.as_ref().is_none_or(|selection| {
                        valid_sketch_profile_region_selection(profile, selection)
                    })
            };
        let native_profiles_link = scope.extrude_native_profiles().is_none_or(|profiles| {
            profiles.len() > 1
                && scope.extrude_profile().is_none()
                && profiles.iter().all(valid_sketch_profile)
                && profiles
                    .iter()
                    .map(|profile| profile.record_index)
                    .collect::<HashSet<_>>()
                    .len()
                    == profiles.len()
        });
        let extrude_profile_link =
            scope.extrude_profile().is_none_or(valid_sketch_profile) && native_profiles_link;

        let sweep_profile_link = scope.sweep_profile().is_none_or(valid_sketch_profile);
        let base_flange_profile_link = scope.base_flange_profile().is_none_or(valid_sketch_profile);
        let emboss_profile_link = scope
            .emboss_operand_construction()
            .and_then(|construction| construction.sketch_profile.as_ref())
            .is_none_or(valid_sketch_profile);
        let move_link = design::decode::scopes::direct_face::move_operation_links_hold(
            scope,
            scope.move_operation().and_then(|operation| {
                records_by_index
                    .get(&(native_stream, operation.transform_record_index))
                    .copied()
            }),
        );
        let offset_faces_link = match scope.payload() {
            records::feature::scope::DesignScopePayload::OffsetFaces(Some(operation))
            | records::feature::scope::DesignScopePayload::DecalerLesFaces(Some(operation)) => {
                design::decode::scopes::direct_face::offset_faces_distance_reference_matches(
                    scope,
                    operation.distance_record_index,
                )
            }
            _ => true,
        };
        let base_flange_link = match scope.base_flange_operation() {
            None => true,
            Some(operation) => {
                scope.reference_members().values().copied().eq([
                    operation.profile_group_record_index,
                    operation.profile_record_index,
                    operation.thickness_record_index,
                    operation.settings_record_index,
                ]) && scope.base_flange_profile().is_some_and(|profile| {
                    profile.record_index == operation.profile_record_index
                        && profile.scope_reference_ordinal == 1
                }) && scope
                    .byte_offset()
                    .checked_add(123)
                    .is_some_and(|expected_offset| operation.thickness_offset == expected_offset)
                    && operation.thickness_offset < scope.paired_byte_offset()
            }
        };
        let edge_flange_link = match scope.edge_flange_operation() {
            None => true,
            Some(operation) => {
                // The ordered reference table is in record-index order, so the
                // check is that every role names a distinct table entry and that
                // the entries no role claims are exactly the width owners.
                let edge_count = operation.selection.shape().edges().count();
                let claimed = operation
                    .selection
                    .shape()
                    .edges()
                    .flat_map(|edge| {
                        [
                            edge.wrapper,
                            edge.group_record_index.get(),
                            edge.operand_record_index(),
                            edge.aggregate_operand_record_index,
                        ]
                    })
                    .chain(operation.selection.shape().owner_indices().copied())
                    .chain(operation.auxiliary_reference_record_indices.iter().copied())
                    .chain([
                        operation.selection.aggregate_group_record_index(),
                        operation.height_owner_record_index,
                        operation.angle_owner_record_index,
                        operation.settings_record_index,
                    ])
                    .collect::<Vec<_>>();
                let mut claimed = claimed;
                if let records::feature::sheet_metal::DesignEdgeFlangeHeightExtent::ToObject {
                    target_group_record_index,
                    target_operand_record_index,
                    offset_owner_record_index,
                    ..
                } = operation.selection.shape().height()
                {
                    for index in [
                        target_group_record_index,
                        target_operand_record_index,
                        offset_owner_record_index,
                    ] {
                        claimed.push(index);
                    }
                }
                let unique_claimed = claimed
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>();
                let corner_layout =
                    matches!(scope.class_tag.as_str(), "361" | "362") && edge_count > 1;
                let corner_form = !corner_layout
                    || (scope.paired_class_tag.as_str() == "258"
                        && design::decode::scopes::sheet_metal::corner_flange::operation_holds(
                            operation,
                            scope.byte_offset(),
                            scope.frame_length(),
                        )
                        && scope
                            .reference_members()
                            .values()
                            .zip(scope.reference_members().values().skip(1))
                            .all(|(left, right)| left < right));
                corner_form
                    && edge_count > 0
                    && claimed.len() == scope.reference_members().len()
                    && unique_claimed.len() == claimed.len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let hem_link = match scope.hem_operation() {
            None => true,
            Some(operation) => {
                let mut claimed = vec![
                    operation.edge_wrapper_record_index,
                    operation.edge_group_record_index.get(),
                    operation.edge_operand_record_index(),
                    operation.aggregate_group_record_index.get(),
                    operation.aggregate_operand_record_index(),
                    operation.settings_record_index,
                ];
                match &operation.parameter_owners {
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLength {
                        gap_owner_record_index,
                        length_owner_record_index,
                    } => claimed.extend([*gap_owner_record_index, *length_owner_record_index]),
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::RadiusAngle {
                        radius_owner_record_index,
                        angle_owner_record_index,
                    } => claimed.extend([*radius_owner_record_index, *angle_owner_record_index]),
                    crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLengthRadius {
                        gap_owner_record_index,
                        length_owner_record_index,
                        radius_owner_record_index,
                    } => claimed.extend([
                        *gap_owner_record_index,
                        *length_owner_record_index,
                        *radius_owner_record_index,
                    ]),
                }
                claimed.iter().copied().collect::<HashSet<_>>().len() == claimed.len()
                    && claimed.len() == scope.reference_members().len()
                    && claimed.iter().all(|index| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == index)
                    })
                    && operation.bend_radius_offset > scope.byte_offset()
                    && operation.bend_radius_offset < scope.paired_byte_offset()
            }
        };
        let copy_paste_link = match scope.copy_paste_bodies_operation() {
            None => true,
            Some(operation) => {
                let group_header =
                    records_by_index.get(&(native_stream, operation.body_group_record_index));
                let relation_header =
                    records_by_index.get(&(native_stream, operation.relation_record_index));
                scope.reference_members().values().next()
                    == Some(&operation.body_group_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .skip(1)
                        .copied()
                        .eq(operation.bodies().iter().map(|body| body.operand.value))
                    && group_header.is_some_and(|header| {
                        header.byte_offset == operation.body_group_byte_offset()
                            && header.class_tag == operation.body_group_class_tag
                    })
                    && relation_header.is_some_and(|header| {
                        header.byte_offset == operation.relation_byte_offset()
                            && header.class_tag == operation.relation_class_tag
                    })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.source.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                            }) || native.design_body_members.iter().any(|member| {
                                design_stream(member.id()) == native_stream
                                    && member.entity_suffix == u64::from(suffix)
                            })
                        })
                    && operation
                        .bodies()
                        .iter()
                        .map(|body| body.copied.value)
                        .all(|suffix| {
                            native.design_body_bindings.iter().any(|binding| {
                                design_stream(binding.id()) == native_stream
                                    && binding.entity_suffix == u64::from(suffix)
                                    && (binding.body.is_some() || binding.history_only)
                            })
                        })
            }
        };
        let rectangular_pattern_link = match scope.rectangular_pattern_construction() {
            None => true,
            Some(construction) => {
                let instances_link = construction.instances().is_none_or(|instances| {
                    let active = [
                        (construction.u_count(), construction.u_extent()),
                        (construction.v_count(), construction.v_extent()),
                    ]
                    .into_iter()
                    .filter(|(count, _)| *count > 1)
                    .collect::<Vec<_>>();
                    let [(count, extent)] = active.as_slice() else {
                        return false;
                    };
                    let Ok(count) = usize::try_from(*count) else {
                        return false;
                    };
                    let Some(reference_end) = count.checked_add(5) else {
                        return false;
                    };
                    let expected_records = scope
                        .reference_members()
                        .values()
                        .next()
                        .into_iter()
                        .chain(
                            scope
                                .reference_members()
                                .values_in(6..reference_end)
                                .into_iter()
                                .flatten(),
                        )
                        .copied();
                    let Some(first) = instances
                        .frames()
                        .next()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let Some(last) = instances
                        .frames()
                        .next_back()
                        .map(|frame| &frame.transform.value)
                    else {
                        return false;
                    };
                    let delta = [
                        last[0][3] - first[0][3],
                        last[1][3] - first[1][3],
                        last[2][3] - first[2][3],
                    ];
                    let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
                    let component_link =
                        valid_component_pattern_occurrences(native, native_stream, instances);
                    instances
                        .frames()
                        .map(|frame| frame.record_index)
                        .eq(expected_records)
                        && instances.instance_count() == count
                        && instances.frames().all(|frame| {
                            let transform = &frame.transform.value;
                            (0..3).all(|row| {
                                (0..3).all(|column| {
                                    (transform[row][column] - first[row][column]).abs()
                                        <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E10
                                })
                            })
                        })
                        && (distance - extent.abs()).abs()
                            <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                        && instances.frames().enumerate().all(|(ordinal, frame)| {
                            let transform = &frame.transform.value;
                            let (Some(ordinal), Some(divisor)) =
                                (f64_from_index(ordinal), f64_from_index(count - 1))
                            else {
                                return false;
                            };
                            let fraction = ordinal / divisor;
                            (0..3).all(|axis| {
                                (transform[axis][3] - first[axis][3] - delta[axis] * fraction).abs()
                                    <= EPS_VALIDATE_VALIDATE_PARAMETER_SCOPES_E8
                            })
                        })
                        && instances.frames().all(|frame| {
                            records_by_index
                                .get(&(native_stream, frame.record_index))
                                .is_some_and(|header| frame.transform.offset > header.byte_offset)
                        })
                        && component_link
                });
                instances_link
                    && native
                        .design_parameter_owners
                        .iter()
                        .filter(|owner| {
                            design_stream(owner.id()) == native_stream
                                && owner.scope_record_index() == scope.record_index
                        })
                        .count()
                        == 4
                    && construction
                        .owner_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                                && records_by_index.contains_key(&(native_stream, *record_index))
                        })
                    && construction
                        .owner_record_indices
                        .iter()
                        .zip(construction.value_offsets)
                        .zip([
                            f64::from(construction.u_count()),
                            f64::from(construction.v_count()),
                            construction.u_extent(),
                            construction.v_extent(),
                        ])
                        .enumerate()
                        .all(|(ordinal, ((record_index, value_offset), value))| {
                            native.design_parameter_owners.iter().any(|owner| {
                                design_stream(owner.id()) == native_stream
                                    && owner.record_index() == *record_index
                                    && owner.scope_record_index() == scope.record_index
                                    && u32::try_from(ordinal) == Ok(owner.local_ordinal())
                                    && owner.evaluated_value().get() == value
                                    && owner.evaluated_value_offset() == value_offset
                            })
                        })
            }
        };
        let assembly_alignment_link = match scope.assembly_alignment() {
            None => true,
            Some(alignment)
                if design::decode::scopes::assembly_v3::registered(scope, stream_types)
                    && alignment.frame_sense.is_some() =>
            {
                design::decode::scopes::assembly_v3::valid_alignment(
                    scope,
                    alignment,
                    stream_types,
                    &native.design_parameter_owners,
                )
            }
            Some(alignment) => {
                let values = if alignment.owners.len() == 2 {
                    vec![alignment.angle(), alignment.offset()[2]]
                } else if design::assembly::placement_ordered_as_built_scope(scope) {
                    vec![
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                        alignment.angle(),
                    ]
                } else {
                    vec![
                        alignment.angle(),
                        alignment.offset()[0],
                        alignment.offset()[1],
                        alignment.offset()[2],
                    ]
                };
                let generation = design::assembly::AssemblyScopeGeneration::for_scope(scope);
                let operand_frame_variant = generation.operand_frame_variant();
                let variable_reference = !design::assembly::rigid_as_built_scope(scope)
                    && design::assembly::variable_reference_assembly_generation(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                    );
                let compact_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Compact)
                );
                let axial_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Axial)
                );
                let wide_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::Wide)
                );
                let compact_wide_frames = matches!(
                    operand_frame_variant,
                    Some(design::assembly::AssemblyOperandFrameVariant::CompactWide)
                );
                let wide_as_built = design::assembly::wide_as_built_scope(scope);
                let rigid_as_built = design::assembly::rigid_as_built_scope(scope);
                let as_built_frames = scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                    && (scope.frame_length() == 399 || wide_as_built);
                let as_built_421_generation = design::assembly::legacy_as_built_421_generation(
                    scope.frame_length(),
                    scope.class_tag.as_str(),
                    scope.paired_class_tag.as_str(),
                );
                let as_built_421 = as_built_421_generation.is_some();
                let operand_paths = alignment.operand_path_refs();
                let wide_family_offsets = operand_frame_variant
                    .and_then(crate::design::decode::scopes::wide_family_frame_offsets);
                let frame_reference_offsets =
                    if let Some([first, _, second, _]) = wide_family_offsets {
                        [u64_from_index(first + 1), u64_from_index(second + 1)]
                    } else if axial_frames {
                        [29, 168]
                    } else if compact_frames {
                        [25, 165]
                    } else {
                        [29, 169]
                    };
                let frame_transform_offsets =
                    if let Some([_, first, _, second]) = wide_family_offsets {
                        [u64_from_index(first), u64_from_index(second)]
                    } else if axial_frames {
                        [39, 178]
                    } else if compact_frames {
                        [36, 176]
                    } else {
                        [40, 180]
                    };
                let assembly_owner_count = native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .count();
                let alignment_lane_bounds = generation.alignment_lane_bounds(assembly_owner_count);
                let operand_frames_link = if let Some(
                    records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                        carriers,
                        ..
                    },
                ) = &alignment.form
                {
                    carriers
                        .references()
                        .into_iter()
                        .enumerate()
                        .all(|(ordinal, reference)| {
                            let reference_ordinal = ordinal * 2;
                            scope
                                .reference_members()
                                .values()
                                .nth(reference_ordinal)
                                .copied()
                                == Some(reference.value)
                                && scope
                                    .reference_members()
                                    .offsets()
                                    .nth(reference_ordinal)
                                    .copied()
                                    == Some(reference.offset)
                                && records_by_index.contains_key(&(native_stream, reference.value))
                        })
                } else {
                    alignment.operand_frames().is_none_or(|frames| {
                        frames[0].reference_record_index != frames[1].reference_record_index
                            && frames.iter().enumerate().all(|(ordinal, frame)| {
                                let offsets_match = if as_built_frames {
                                    operand_paths.as_ref().is_some_and(|paths| {
                                        as_built_operand_frame_link(
                                            scope,
                                            paths[ordinal],
                                            frame,
                                            stream_types,
                                        )
                                    })
                                } else {
                                    Some(frame.reference_offset)
                                        == scope
                                            .byte_offset()
                                            .checked_add(frame_reference_offsets[ordinal])
                                        && Some(frame.transform_offset)
                                            == scope
                                                .byte_offset()
                                                .checked_add(frame_transform_offsets[ordinal])
                                };
                                let reference_exists = if as_built_frames {
                                    frame.reference_record_index != 0
                                } else {
                                    records_by_index.contains_key(&(
                                        native_stream,
                                        frame.reference_record_index,
                                    ))
                                };
                                offsets_match && reference_exists
                            })
                    })
                };
                let solved_frame_link = alignment.solved_frame().is_none_or(|frame| {
                    let Some(generation) = as_built_421_generation else {
                        return false;
                    };
                    let Some(header) =
                        records_by_index.get(&(native_stream, frame.reference_record_index))
                    else {
                        return false;
                    };
                    as_built_421
                        && scope.reference_members().values().nth(8).copied()
                            == Some(frame.reference_record_index)
                        && scope.reference_members().offsets().nth(8).copied()
                            == Some(frame.reference_offset)
                        && header.class_tag.as_str() == generation.frame_class_tag()
                        && frame.class_tag == header.class_tag
                        && frame.record_byte_offset == header.byte_offset
                        && Some(frame.transform_offset)
                            == frame
                                .record_byte_offset
                                .checked_add(u64_from_index(generation.matrix_offset()))
                });
                let operand_qualifiers_link = match alignment.form.as_ref() {
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(
                        operands,
                    )) => {
                        let frames = operands.each_ref().map(|operand| operand.frame.clone());
                        match (&operands[0].qualifier, &operands[1].qualifier) {
                            (records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path: second }) => {
                                let paths = [first, second];

                            let class_363_carriers = paths
                                .iter()
                                .all(|path| path.link().locator_class_tag.as_str() == "363");
                            if class_363_carriers {
                                !axial_frames
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && paths.iter().zip(&frames).all(|(path, frame)| {
                                        valid_class_363_operand_path_link(scope, frame, path)
                                    })
                            } else {
                                let locator_offsets =
                                    generation.operand_path_locator_offsets();
                                let first_start = paths[0].link().locator_byte_offset;
                                let second_start = paths[1].link().locator_byte_offset;
                                let envelope_ends = paths.each_ref().map(|path| {
                                    let continuation_count = if variable_reference || wide_as_built
                                        || operand_frame_variant.is_some_and(
                                            design::assembly::AssemblyOperandFrameVariant::is_wide_family,
                                        ) {
                                        path.link()
                                            .wrapper_record_index
                                            .checked_sub(path.link().locator_record_index)?
                                            .checked_sub(2)?
                                    } else {
                                        0
                                    };
                                    let stride = if rigid_as_built {
                                        rigid_group_locator_layout(path, stream_types)?.reference_stride()
                                    } else { 11 };
                                    u64::try_from(path_wrapper::PATH_REFERENCE + stride)
                                        .ok()?
                                        .checked_add(u64::from(continuation_count).checked_mul(u64_from_index(stride))?)
                                        .and_then(|length| {
                                            path.link().wrapper_byte_offset.checked_add(length)
                                        })
                                });
                                !axial_frames
                                    && locator_offsets.is_some_and(|offsets| {
                                        paths.iter().zip(offsets).all(|(path, offset)| {
                                            valid_assembly_operand_path_link(scope, path, offset, stream_types)
                                        })
                                    })
                                    && paths[0].link().locator_record_index
                                        != paths[1].link().locator_record_index
                                    && matches!(envelope_ends, [Some(first_end), Some(second_end)]
                                if !(first_start < second_end && second_start < first_end))
                                    && paths.iter().all(|path| {
                                        (path.admitted_by_type()
                                            || matches!(path.class_tag().as_str(), "294" | "299" | "307" | "329" | "330" | "386" | "390"))
                                            && !matches!(path.link().locator_class_tag.as_str(), "363" | "378")
                                            && (path.admitted_by_type()
                                                || matches!(
                                                path.class_tag().as_str(),
                                                "294" | "299" | "307" | "330" | "386"
                                            ) || path.occurrence_guids().first().is_some_and(
                                                |guid| {
                                                    native
                                                        .design_component_occurrences
                                                        .iter()
                                                        .filter(|occurrence| {
                                                            design_stream(&occurrence.id)
                                                                == native_stream
                                                                && occurrence
                                                                    .occurrence_guid.as_str()
                                                                    .eq_ignore_ascii_case(guid.value.as_str())
                                                        })
                                                        .count()
                                                        == 1
                                                },
                                            ))
                                    })
                            }

                            }
                            (records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: first },
                             records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { target: second }) => {
                                axial_frames && valid_axial_assembly_targets(native, records_by_index, native_stream, scope, &frames, &[first, second])
                            }
                            _ if variable_reference && operands.iter().any(|operand| matches!(operand.qualifier, records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. })) => {
                                frames[0].reference_record_index != frames[1].reference_record_index
                                    && operands.iter().all(|operand| match &operand.qualifier {
                                        records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath { path } => valid_class_363_operand_path_link(scope, &operand.frame, path),
                                        qualifier @ records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin { .. } => valid_class_307_joint_origin_qualifier(native, records_by_index, native_stream, &operand.frame, qualifier),
                                        records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget { .. } => false,
                                    })
                            }
                            _ => false,
                        }
                    }
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::Frames {
                        ..
                    }) => axial_frames,
                    Some(records::feature::assembly::DesignAssemblyAlignmentForm::LimitsOnly {
                        ..
                    }) => as_built_421,
                    None
                    | Some(
                        records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                            ..
                        }
                        | records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                            ..
                        },
                    ) => true,
                };
                let joint_origin_envelope_link = alignment
                    .joint_origin_scope_record_index()
                    .is_none_or(|record_index| {
                        scope.class_tag.as_str() == "276"
                            && scope.paired_class_tag.as_str() == "258"
                            && scope.frame_length() == 604
                            && native.design_parameter_scopes.iter().any(|target| {
                                design_stream(&target.id) == native_stream
                                    && target.kind()
                                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                                    && target.record_index == record_index
                                    && scope
                                        .byte_offset()
                                        .checked_add(36)
                                        .is_some_and(|offset| {
                                            target.joint_origin_transform_offset() == Some(offset)
                                        })
                            })
                    });
                let alignment_scalars_link = if let Some(generation) = as_built_421_generation {
                    match (alignment.limits(), alignment.owners.as_slice()) {
                        (Some(limits), [angle, offset_x, offset_y, offset_z]) => {
                            let alignment_lanes = [
                                (
                                    offset_x.value,
                                    offset_x.offset,
                                    alignment.offset()[0],
                                    0_u32,
                                ),
                                (
                                    offset_y.value,
                                    offset_y.offset,
                                    alignment.offset()[1],
                                    1_u32,
                                ),
                                (
                                    offset_z.value,
                                    offset_z.offset,
                                    alignment.offset()[2],
                                    2_u32,
                                ),
                                (angle.value, angle.offset, alignment.angle(), 3_u32),
                            ];
                            let limit_order = if generation.reverse_limit_order() {
                                [1, 0]
                            } else {
                                [0, 1]
                            };
                            let limit_lanes = limit_order.into_iter().zip([4_u32, 5]).map(
                                |(index, local_ordinal)| {
                                    (
                                        limits.owner_record_indices[index],
                                        limits.value_offsets[index],
                                        [limits.minimum(), limits.maximum()][index],
                                        local_ordinal,
                                    )
                                },
                            );
                            limits.kind == generation.limit_kind()
                                && alignment_lanes.into_iter().chain(limit_lanes).all(
                                    |(record_index, value_offset, value, local_ordinal)| {
                                        native.design_parameter_owners.iter().any(|owner| {
                                            design_stream(owner.id()) == native_stream
                                                && owner.record_index() == record_index
                                                && owner.scope_record_index() == scope.record_index
                                                && owner.local_ordinal() == local_ordinal
                                                && owner.evaluated_value().get() == value
                                                && owner.evaluated_value_offset() == value_offset
                                        })
                                    },
                                )
                        }
                        _ => false,
                    }
                } else {
                    rigid_alignment_scalars_hold(alignment, rigid_as_built)
                        && alignment_lane_bounds.is_some_and(|(alignment_start, alignment_end)| {
                            alignment_end.checked_sub(alignment_start).is_some_and(
                                |expected_offset| alignment.owners.len() == expected_offset,
                            ) && alignment.owners.iter().zip(&values).enumerate().all(
                                |(ordinal, (lane, value))| {
                                    native.design_parameter_owners.iter().any(|owner| {
                                        design_stream(owner.id()) == native_stream
                                            && owner.record_index() == lane.value
                                            && owner.scope_record_index() == scope.record_index
                                            && u32::try_from(alignment_start + ordinal)
                                                == Ok(owner.local_ordinal())
                                            && owner.evaluated_value().get() == *value
                                            && owner.evaluated_value_offset() == lane.offset
                                    })
                                },
                            )
                        })
                };
                let alignment_reference_link = if let Some(generation) = as_built_421_generation {
                    alignment.limits().is_some_and(|limits| {
                        let limit_reference_indices = if generation.reverse_limit_order() {
                            [
                                limits.owner_record_indices[1],
                                limits.owner_record_indices[0],
                            ]
                        } else {
                            limits.owner_record_indices
                        };
                        alignment.owners.len() == 4
                            && scope
                                .reference_members()
                                .values()
                                .skip(4)
                                .take(4)
                                .copied()
                                .eq([
                                    alignment.owners[1].value,
                                    alignment.owners[2].value,
                                    alignment.owners[3].value,
                                    alignment.owners[0].value,
                                ])
                            && scope
                                .reference_members()
                                .values()
                                .skip(9)
                                .take(2)
                                .eq(limit_reference_indices.iter())
                    })
                } else if wide_frames || compact_wide_frames {
                    // The wide scope lists its four owners first.
                    scope
                        .reference_members()
                        .values()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value))
                } else if !design::assembly::rigid_as_built_scope(scope)
                    && design::assembly::variable_reference_assembly_generation(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                    )
                {
                    (0..scope.reference_members().len())
                        .filter(|&start| {
                            scope
                                .reference_members()
                                .values_in(start..start + alignment.owners.len())
                                .is_some_and(|values| {
                                    values.eq(alignment.owners.iter().map(|owner| &owner.value))
                                })
                        })
                        .count()
                        == 1
                } else if design::assembly::placement_ordered_as_built_scope(scope) {
                    let mut owners = native
                        .design_parameter_owners
                        .iter()
                        .filter(|owner| {
                            design_stream(owner.id()) == native_stream
                                && owner.scope_record_index() == scope.record_index
                        })
                        .map(|owner| (owner.local_ordinal(), owner.record_index()))
                        .collect::<Vec<_>>();
                    owners.sort_unstable();
                    placement_ordered_references_hold(scope, alignment, &owners)
                } else {
                    scope
                        .reference_members()
                        .values()
                        .rev()
                        .take(alignment.owners.len())
                        .eq(alignment.owners.iter().map(|owner| &owner.value).rev())
                };
                let additional_members_link =
                    rigid_group_members_link(scope, alignment, rigid_as_built, stream_types);
                operand_frames_link
                    && (alignment.additional_owner_run.is_none()
                        || design::assembly::placement_ordered_as_built_scope(scope))
                    && solved_frame_link
                    && operand_qualifiers_link
                    && additional_members_link
                    && joint_origin_envelope_link
                    && alignment_reference_link
                    && alignment_scalars_link
            }
        };
        let component_insert_link = match scope.component_insert_construction() {
            None => true,
            Some(construction) => {
                let relation =
                    records_by_index.get(&(native_stream, construction.relation_record_index));
                let frame_matches_transform = match (
                    scope.frame_length(),
                    scope.paired_class_tag.as_str(),
                    construction.placement.as_ref(),
                ) {
                    (272, "262", None) if scope.class_tag.as_str() == "357" => true,
                    (276, "262", None)
                        if matches!(scope.class_tag.as_str(), "357" | "360" | "362") =>
                    {
                        true
                    }
                    (261, "263", None) if scope.class_tag.as_str() == "296" => true,
                    (261, "261", None) if scope.class_tag.as_str() == "410" => true,
                    (261, "258", None) if scope.class_tag.as_str() == "426" => true,
                    (261, "266", None) if scope.class_tag.as_str() == "434" => true,
                    (257 | 261 | 267, "264", None) if scope.class_tag.as_str() == "414" => true,
                    (257, "262", None) if scope.class_tag.as_str() == "283" => true,
                    (385, "262", Some(matrix)) if scope.class_tag.as_str() == "283" => {
                        scope
                            .byte_offset()
                            .checked_add(46)
                            .is_some_and(|expected_offset| matrix.scope.offset == expected_offset)
                            && matrix.carrier_offset.is_none()
                    }
                    (404, _, Some(matrix)) => {
                        scope
                            .byte_offset()
                            .checked_add(54)
                            .is_some_and(|expected_offset| matrix.scope.offset == expected_offset)
                            && matrix
                                .carrier_offset
                                .is_some_and(|offset| offset < construction.neutron_role_offset)
                    }
                    (frame_length, paired, Some(matrix)) => {
                        let scope_delta = match (frame_length, paired) {
                            (399, "259") => Some(50),
                            (381, "261") => Some(49),
                            (395, "258") => Some(46),
                            (389, "264") if scope.class_tag.as_str() == "414" => Some(50),
                            _ => None,
                        };
                        scope_delta.is_some_and(|delta| {
                            scope
                                .byte_offset()
                                .checked_add(delta)
                                .is_some_and(|expected_offset| {
                                    matrix.scope.offset == expected_offset
                                })
                        }) && matrix
                            .carrier_offset
                            .is_some_and(|offset| construction.neutron_role_offset < offset)
                    }
                    _ => false,
                };
                let role_valid = valid_component_insert_role(&construction.neutron_role);
                scope
                    .reference_members()
                    .values()
                    .copied()
                    .eq([construction.relation_record_index])
                    && construction.carrier_record_index != construction.relation_record_index
                    && role_valid
                    && frame_matches_transform
                    && relation.is_some_and(|relation| {
                        construction
                            .carrier_transform_offset()
                            .is_none_or(|offset| offset < relation.byte_offset)
                    })
                    && (native.xref_references.is_empty()
                        || crate::xref::component_insert_reference(
                            &native.xref_references,
                            native_stream,
                            construction,
                        )
                        .is_some())
            }
        };
        let copy_paste_component_link = match scope.copy_paste_component_operation() {
            None => true,
            Some(operation) => {
                let source = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.source_occurrence_record_index
                    });
                let copied = native
                    .design_component_occurrences
                    .iter()
                    .find(|occurrence| {
                        design_stream(&occurrence.id) == native_stream
                            && occurrence.record_index == operation.copied_occurrence_record_index
                    });
                let source_at = match scope.frame_length() {
                    529 => 38,
                    525 => 34,
                    _ => 0,
                };
                source_at != 0
                    && scope
                        .reference_members()
                        .values()
                        .copied()
                        .eq([operation.relation_record_index])
                    && operation.source_occurrence_record_index
                        != operation.copied_occurrence_record_index
                    && Some(operation.source_transform_offset)
                        == scope.byte_offset().checked_add(source_at)
                    && Some(operation.copied_transform_offset)
                        == scope.byte_offset().checked_add(source_at + 156)
                    && source.is_some_and(|source| {
                        source
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && source
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.source_occurrence_guid.as_str())
                            && source.transform().is_none()
                    })
                    && copied.is_some_and(|copied| {
                        copied
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(operation.component_guid.as_str())
                            && copied
                                .occurrence_guid
                                .as_str()
                                .eq_ignore_ascii_case(operation.copied_occurrence_guid.as_str())
                            && copied.transform().map(|frame| frame.value)
                                == Some(operation.copied_transform)
                    })
            }
        };
        let draft_link = match scope.draft_operation() {
            None => true,
            Some(operation) => {
                scope.reference_members().len() >= 6
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.angle_record_index)
                    && scope
                        .reference_members()
                        .values()
                        .any(|value| value == &operation.opposite_angle_record_index)
                    && operation.angle_record_index != operation.opposite_angle_record_index
                    && operation.angle_offset > scope.paired_byte_offset()
                    && operation.opposite_angle_offset > operation.angle_offset
                    && records_by_index.contains_key(&(native_stream, operation.angle_record_index))
                    && records_by_index
                        .contains_key(&(native_stream, operation.opposite_angle_record_index))
            }
        };
        let combine_link = match scope.combine_operation() {
            None => true,
            Some(operation) => {
                let expected_selections = scope
                    .reference_members()
                    .values()
                    .skip(1)
                    .step_by(2)
                    .copied()
                    .collect::<HashSet<_>>();
                let selections = std::iter::once(operation.target_record_index)
                    .chain(operation.tools.iter().map(|tool| tool.record_index))
                    .collect::<Vec<_>>();
                let actual_selections = selections.iter().copied().collect::<HashSet<_>>();
                let valid_external =
                    |selection: &records::feature::combine::DesignCombineBodySelection| {
                        selection.external_identity.as_ref().is_none_or(|identity| {
                            let Some(header) =
                                records_by_index.get(&(native_stream, selection.record_index))
                            else {
                                return false;
                            };
                            header.byte_offset.checked_add(44)
                                == Some(identity.selector_asset_id_offset())
                        })
                    };
                let compact_scope = scope.class_tag.as_str() == "387"
                    && scope.paired_class_tag.as_str() == "258"
                    && design::decode::scopes::parameter_scope::parameter_scope_payload_length(
                        scope,
                    ) == Some(314);
                let extended_reference_scope = scope.class_tag.as_str() == "329"
                    && scope.paired_class_tag.as_str() == "261"
                    && scope.frame_length() == 363;
                scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && selections.len() == scope.reference_members().len() / 2
                    && actual_selections.len() == selections.len()
                    && actual_selections == expected_selections
                    && operation.tools.iter().all(valid_external)
                    && match operation.form {
                        records::feature::combine::DesignCombineForm::Standard => {
                            !compact_scope
                                && !extended_reference_scope
                                && scope.byte_offset().checked_add(20).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(25).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::Compact => {
                            compact_scope
                                && scope.byte_offset().checked_add(21).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(25).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::OperandLists => {
                            !compact_scope
                                && !extended_reference_scope
                                && scope.byte_offset().checked_add(21).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(29).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                        records::feature::combine::DesignCombineForm::ExtendedReference => {
                            extended_reference_scope
                                && scope.byte_offset().checked_add(31).is_some_and(
                                    |expected_offset| operation.operation_offset == expected_offset,
                                )
                                && scope.byte_offset().checked_add(30).is_some_and(
                                    |expected_offset| {
                                        operation.keep_tools_offset == expected_offset
                                    },
                                )
                        }
                    }
            }
        };
        let thread_link = match scope.thread_construction() {
            None => true,
            Some(construction) => {
                let references = scope
                    .reference_members()
                    .values()
                    .copied()
                    .collect::<Vec<_>>();
                let expected_groups = construction.form.face_groups(&references);
                construction
                    .form
                    .owners_hold(scope, &native.design_parameter_owners)
                    && construction
                        .form
                        .reference_count_holds(scope.reference_members().len())
                    && thread_class_pair_holds(
                        construction.form,
                        &scope.class_tag,
                        &scope.paired_class_tag,
                    )
                    && construction.face_group_record_indices == expected_groups
                    && thread_designation_offset_holds(
                        construction.form,
                        construction.designation_offset,
                        scope.byte_offset(),
                    )
                    && match construction.form {
                        records::feature::thread::DesignThreadForm::Compact(Some(reference))
                        | records::feature::thread::DesignThreadForm::CompactExtended(reference) => {
                            reference.offset > construction.designation_offset
                                && reference.offset < scope.paired_byte_offset()
                                && records_by_index
                                    .contains_key(&(native_stream, reference.value.get()))
                        }
                        records::feature::thread::DesignThreadForm::Compact(None)
                        | records::feature::thread::DesignThreadForm::Standard
                        | records::feature::thread::DesignThreadForm::StandardLegacy
                        | records::feature::thread::DesignThreadForm::StandardExtended
                        | records::feature::thread::DesignThreadForm::StandardCompact
                        | records::feature::thread::DesignThreadForm::CompactLegacy => true,
                    }
                    && construction
                        .face_group_record_indices
                        .iter()
                        .enumerate()
                        .all(|(group_ordinal, record_index)| {
                            let compact_member = if matches!(
                                construction.form,
                                records::feature::thread::DesignThreadForm::Compact(_)
                                    | records::feature::thread::DesignThreadForm::CompactExtended(
                                        _
                                    )
                                    | records::feature::thread::DesignThreadForm::CompactLegacy
                            ) {
                                let Some(reference_ordinal) =
                                    group_ordinal.checked_mul(2).and_then(|ordinal| {
                                        ordinal
                                            .checked_add(construction.form.face_reference_start())
                                    })
                                else {
                                    return false;
                                };
                                let Some(member_ordinal) = reference_ordinal.checked_add(1) else {
                                    return false;
                                };
                                let Some(member_record_index) =
                                    scope.reference_members().values().nth(member_ordinal)
                                else {
                                    return false;
                                };
                                let Ok(scope_reference_ordinal) = u32::try_from(reference_ordinal)
                                else {
                                    return false;
                                };
                                Some((scope_reference_ordinal, *member_record_index))
                            } else {
                                None
                            };
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == *record_index
                                        && group.role() == DesignOperandRole::ROLE_0X10
                                        && compact_member.is_none_or(
                                            |(scope_reference_ordinal, member_record_index)| {
                                                group.scope_reference_ordinal
                                                    == scope_reference_ordinal
                                                    && group
                                                        .members()
                                                        .iter()
                                                        .map(|member| member.value)
                                                        .eq([member_record_index])
                                            },
                                        )
                                });
                            groups.next().is_some() && groups.next().is_none()
                        })
            }
        };
        let joint_origin_link = scope.joint_origin_frame().is_none_or(|origin| {
            let transform = origin.joint_origin_transform;
            let transform_offset = origin.joint_origin_transform_offset;
            let inline = match (scope.frame_length(), &origin.reference) {
                (385, None) => Some(transform_offset) == scope.byte_offset().checked_add(49),
                (336 | 347, Some(reference)) => {
                    Some(transform_offset) == scope.byte_offset().checked_add(60)
                        && Some(reference.joint_origin_reference_offset)
                            == scope.byte_offset().checked_add(46)
                        && scope
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                }
                _ => false,
            };
            let assembly_operand = origin.reference.is_none()
                && native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.assembly_alignment().is_some_and(|alignment| {
                            alignment.operand_frames().is_some_and(|frames| {
                                frames.iter().any(|frame| {
                                    frame.reference_record_index == scope.record_index
                                        && frame.transform == transform
                                        && frame.transform_offset == transform_offset
                                })
                            })
                        })
                });
            let single_operand_assembly = origin.reference.as_ref().is_some_and(|reference| {
                native.design_parameter_scopes.iter().any(|assembly| {
                    design_stream(&assembly.id) == native_stream
                        && assembly.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Assemble
                        && assembly.class_tag.as_str() == "276"
                        && assembly.paired_class_tag.as_str() == "258"
                        && assembly.frame_length() == 604
                        && Some(transform_offset) == assembly.byte_offset().checked_add(36)
                        && assembly
                            .reference_members()
                            .values()
                            .any(|value| value == &reference.joint_origin_reference)
                        && Some(reference.joint_origin_reference_offset)
                            == assembly.byte_offset().checked_add(25)
                })
            });
            inline || assembly_operand || single_operand_assembly
        });
        let work_point_link = valid_work_point_construction(ctx, scope, native_stream)?;
        let work_plane_link = valid_work_plane_construction(ctx, scope, native_stream);
        let valid = match scope.extrude_prologue() {
            Some(records::feature::extrude::DesignExtrudePrologue::LegacyDistance {
                prefix_zero_offset,
                operation_offset,
                extent_kind_offset,
                direction_reversed_offset,
                solid_operation_offset,
                ..
            }) => {
                let marker_offset = scope.byte_offset().checked_add(20);
                let prefix_valid =
                    match prefix_zero_offset {
                        None => {
                            marker_offset
                                .and_then(|offset| offset.checked_add(1))
                                .is_some_and(|expected_offset| operation_offset == expected_offset)
                                && scope.byte_offset().checked_add(208).is_some_and(
                                    |expected_offset| {
                                        scope.reference_count_offset() == expected_offset
                                    },
                                )
                        }
                        Some(offset) => {
                            marker_offset
                                .and_then(|offset| offset.checked_add(1))
                                .is_some_and(|expected_offset| offset == expected_offset)
                                && offset.checked_add(4).is_some_and(|expected_offset| {
                                    operation_offset == expected_offset
                                })
                                && scope.byte_offset().checked_add(212).is_some_and(
                                    |expected_offset| {
                                        scope.reference_count_offset() == expected_offset
                                    },
                                )
                        }
                    };
                prefix_valid
                    && operation_offset
                        .checked_add(4)
                        .is_some_and(|expected_offset| extent_kind_offset == expected_offset)
                    && extent_kind_offset
                        .checked_add(4)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && direction_reversed_offset
                        .checked_add(1)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
            }
            Some(records::feature::extrude::DesignExtrudePrologue::ShiftedReferenceAware {
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let expected_layout =
                    match (scope.class_tag.as_str(), scope.paired_class_tag.as_str()) {
                        ("357", "258") | ("275" | "361", "262") => Some((
                            538_u64,
                            292_u64,
                            13_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        )),
                        ("349", "266") => Some((
                            538_u64,
                            292_u64,
                            13_usize,
                            [2, 1],
                            [2, 0],
                            records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                            288_u64,
                        )),
                        ("323", "263")
                            if scope.byte_offset().checked_add(292).is_some_and(
                                |expected_offset| scope.reference_count_offset() == expected_offset,
                            ) =>
                        {
                            Some((
                                516_u64,
                                292_u64,
                                11_usize,
                                [2, 1],
                                [2, 0],
                                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces,
                                288_u64,
                            ))
                        }
                        ("323", "263")
                            if scope.byte_offset().checked_add(272).is_some_and(
                                |expected_offset| scope.reference_count_offset() == expected_offset,
                            ) =>
                        {
                            Some((
                                485_u64,
                                272_u64,
                                10_usize,
                                [3, 0],
                                [4, 4],
                                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll,
                                129_u64,
                            ))
                        }
                        _ => None,
                    };
                expected_layout.is_some_and(
                    |(
                        frame_length,
                        reference_count_offset,
                        reference_member_count,
                        expected_direction_face_extend_values,
                        expected_side_extent_discriminators,
                        expected_extent,
                        second_side_extent_offset,
                    )| {
                        scope.frame_length() == frame_length
                            && scope.byte_offset().checked_add(frame_length).is_some_and(
                                |expected_offset| scope.paired_byte_offset() == expected_offset,
                            )
                            && scope
                                .byte_offset()
                                .checked_add(reference_count_offset)
                                .is_some_and(|expected_offset| {
                                    scope.reference_count_offset() == expected_offset
                                })
                            && scope.reference_members().len() == reference_member_count
                            && scope
                                .byte_offset()
                                .checked_add(27)
                                .is_some_and(|expected_offset| operation_offset == expected_offset)
                            && direction_face_extend_values == expected_direction_face_extend_values
                            && side_extent_discriminators == expected_side_extent_discriminators
                            && extent == expected_extent
                            && scope
                                .byte_offset()
                                .checked_add(116)
                                .zip(scope.byte_offset().checked_add(second_side_extent_offset))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(31)
                                .zip(scope.byte_offset().checked_add(35))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    direction_face_extend_offsets == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(39)
                                .is_some_and(|expected_offset| {
                                    direction_reversed_offset == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(40)
                                .is_some_and(|expected_offset| {
                                    solid_operation_offset == expected_offset
                                })
                            && scope
                                .byte_offset()
                                .checked_add(41)
                                .is_some_and(|expected_offset| start_offset == expected_offset)
                    },
                )
            }
            Some(records::feature::extrude::DesignExtrudePrologue::ReferenceAware {
                reference,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                first_side_target_ordinal,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let typed_references = scope_has_typed_references(scope);
                let prefix_valid = reference.map_or(
                    direct_extrude_operation_valid(
                        scope.byte_offset(),
                        typed_references,
                        operation_offset,
                    ),
                    |reference| {
                        let padding_end =
                            reference
                                .record_index_offset
                                .checked_add(4)
                                .and_then(|offset| {
                                    offset.checked_add(u64::from(reference.trailing_zero_count))
                                });
                        let marker_valid = match reference.operation_prefix_marker_offset {
                            None => Some(operation_offset) == padding_end,
                            Some(marker_offset) => {
                                Some(marker_offset) == padding_end
                                    && marker_offset.checked_add(1).is_some_and(|expected_offset| {
                                        operation_offset == expected_offset
                                    })
                            }
                        };
                        scope
                            .byte_offset()
                            .checked_add(26)
                            .is_some_and(|expected_offset| {
                                reference.record_index_offset == expected_offset
                            })
                            && matches!(reference.trailing_zero_count, 7 | 8)
                            && marker_valid
                            && scope
                                .reference_members()
                                .values()
                                .any(|value| value == &reference.record_index)
                    },
                );
                let target_ordinal_valid = first_side_target_ordinal.is_none_or(|target| {
                    usize::try_from(target.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| {
                            scope.reference_members().values().nth(ordinal).copied()
                        })
                        .is_some_and(|record_index| {
                            let mut groups = native
                                .design_construction_operand_groups
                                .iter()
                                .filter(|group| {
                                    design_stream(&group.id) == native_stream
                                        && group.scope_record_index == scope.record_index
                                        && group.record_index == record_index
                                        && group.scope_reference_ordinal
                                            == target.scope_reference_ordinal
                                        && group.role() == DesignOperandRole::ROLE_0X5
                                        && group.extrude_role().is_none()
                                });
                            target.scope_reference_ordinal_offset.checked_add(5)
                                == Some(side_extent_discriminator_offsets[0])
                                && groups.next().is_some()
                                && groups.next().is_none()
                        })
                });
                let target_prefix_length = if first_side_target_ordinal.is_some() {
                    5
                } else {
                    0
                };
                let legacy_class_415_layout = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_delta| {
                        legacy_class_415::is_symmetric_distance_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_delta,
                            scope.reference_members().len(),
                        )
                    });
                let legacy_class_415_one_sided_layout = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_delta| {
                        legacy_class_415::is_one_sided_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_delta,
                            scope.reference_members().len(),
                        )
                    });
                let legacy_class_415_extent = legacy_class_415_layout
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                    && direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 1]
                    && extent == records::feature::extrude::DesignExtrudeExtent::SymmetricDistance
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_415::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::DIRECTION))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_415::FACE_EXTEND)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::DIRECTION_REVERSED))
                        .is_some_and(|expected_offset| {
                            direction_reversed_offset == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::GEOMETRY_KIND))
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_415::START_SUPPORT))
                        .is_some_and(|expected_offset| start_offset == expected_offset);
                let first_side_offset_valid = operation_offset
                    .checked_add(49)
                    .and_then(|offset| offset.checked_add(target_prefix_length))
                    .and_then(|offset| side_extent_discriminator_offsets[0].checked_sub(offset))
                    .is_some_and(|slot_expansion| {
                        extent_slot_expansion_valid(typed_references, slot_expansion)
                    });
                let second_side_offset_valid = (if side_extent_discriminators == [2, 2] {
                    side_extent_discriminator_offsets[0].checked_add(u64_from_index(
                        crate::layout::current_extrude_face_target_payload::SECOND_SIDE_EXTENT,
                    ))
                } else if side_extent_discriminators[0] == 2 {
                    scope.reference_count_offset().checked_sub(4)
                } else {
                    side_extent_discriminator_offsets[0].checked_add(13)
                })
                .is_some_and(|expected_offset| {
                    side_extent_discriminator_offsets[1] == expected_offset
                }) || (legacy_class_415_one_sided_layout
                    && scope.reference_count_offset().checked_sub(4).is_some_and(
                        |expected_offset| side_extent_discriminator_offsets[1] == expected_offset,
                    ));
                let standard_extent = design::decode::scopes::extrude::exact_extrude_extent(
                    direction_face_extend_values[0],
                    side_extent_discriminators,
                ) == Some(extent);
                let dual_lane_symmetric_extent = direction_face_extend_values == [3, 2]
                    && side_extent_discriminators == [1, 1]
                    && extent == records::feature::extrude::DesignExtrudeExtent::SymmetricDistance;
                prefix_valid
                    && matches!(direction_face_extend_values[0], 1..=3)
                    && (standard_extent || legacy_class_415_extent || dual_lane_symmetric_extent)
                    && first_side_target_ordinal.is_none_or(|_| side_extent_discriminators[0] == 2)
                    && target_ordinal_valid
                    && first_side_offset_valid
                    && second_side_offset_valid
                    && operation_offset
                        .checked_add(4)
                        .zip(operation_offset.checked_add(8))
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && operation_offset
                        .checked_add(14)
                        .is_some_and(|expected_offset| start_offset == expected_offset)
                    && operation_offset
                        .checked_add(13)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && operation_offset
                        .checked_add(12)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && side_extent_discriminator_offsets[1]
                        .checked_add(4)
                        .is_some_and(|end| end <= scope.reference_count_offset())
            }
            Some(records::feature::extrude::DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset,
                operation_offset,
                direction_face_extend_values,
                side_extent_discriminators,
                side_extent_discriminator_offsets,
                extent,
                direction_face_extend_offsets,
                direction_reversed_offset,
                solid_operation_offset,
                start_offset,
                ..
            }) => {
                let field_shift =
                    match operation_prefix_marker_offset {
                        None if scope
                            .byte_offset()
                            .checked_add(27)
                            .is_some_and(|expected_offset| operation_offset == expected_offset) =>
                        {
                            Some(0)
                        }
                        Some(marker_offset)
                            if scope.byte_offset().checked_add(27).is_some_and(
                                |expected_offset| marker_offset == expected_offset,
                            ) && marker_offset.checked_add(1).is_some_and(
                                |expected_offset| operation_offset == expected_offset,
                            ) =>
                        {
                            Some(1)
                        }
                        _ => None,
                    };
                let compact_extent_offsets = if operation_prefix_marker_offset.is_none()
                    && scope
                        .byte_offset()
                        .checked_add(26)
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .and_then(|offset| match offset {
                            251 => scope
                                .byte_offset()
                                .checked_add(105)
                                .zip(scope.byte_offset().checked_add(109))
                                .map(|(first, second)| [first, second]),
                            281 => scope
                                .byte_offset()
                                .checked_add(124)
                                .zip(scope.byte_offset().checked_add(128))
                                .map(|(first, second)| [first, second]),
                            _ => None,
                        })
                } else {
                    None
                };
                let class_296_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_one_sided_to_face_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_to_face::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_to_face::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_296_to_face::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_296_symmetric_extent_offsets =
                    if scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .is_some_and(|reference_count_offset| {
                            is_class_296_symmetric_distance_layout(
                                scope.class_tag.as_str(),
                                scope.paired_class_tag.as_str(),
                                scope.frame_length(),
                                reference_count_offset,
                                scope.reference_members().len(),
                            )
                        })
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                    {
                        scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_symmetric::FIRST_SIDE_EXTENT))
                            .zip(scope.byte_offset().checked_add(u64_from_index(
                                class_296_symmetric::SECOND_SIDE_EXTENT,
                            )))
                            .map(|(first, second)| [first, second])
                    } else {
                        None
                    };
                let class_296_two_faces_extent_offsets =
                    if scope
                        .reference_count_offset()
                        .checked_sub(scope.byte_offset())
                        .is_some_and(|reference_count_offset| {
                            is_class_296_two_sided_to_faces_layout(
                                scope.class_tag.as_str(),
                                scope.paired_class_tag.as_str(),
                                scope.frame_length(),
                                reference_count_offset,
                                scope.reference_members().len(),
                            )
                        })
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                    {
                        scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_296_two_faces::FIRST_SIDE_EXTENT))
                            .zip(scope.byte_offset().checked_add(u64_from_index(
                                class_296_two_faces::SECOND_SIDE_EXTENT,
                            )))
                            .map(|(first, second)| [first, second])
                    } else {
                        None
                    };
                let class_296_legacy_to_face_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_to_face_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(scope.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_to_face::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_296_legacy_distance_extent_offsets = if scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .is_some_and(|reference_count_offset| {
                        is_class_296_legacy_one_sided_distance_layout(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    })
                    && scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::OPERATION))
                        .is_some_and(|expected_offset| operation_offset == expected_offset)
                {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_296_legacy_prefix::FIRST_SIDE_EXTENT))
                        .zip(scope.byte_offset().checked_add(u64_from_index(
                            class_296_legacy_distance::SECOND_SIDE_EXTENT,
                        )))
                        .map(|(first, second)| [first, second])
                } else {
                    None
                };
                let class_397_frame = scope
                    .reference_count_offset()
                    .checked_sub(scope.byte_offset())
                    .and_then(|reference_count_offset| {
                        Class397SymmetricFrame::new(
                            scope.class_tag.as_str(),
                            scope.paired_class_tag.as_str(),
                            scope.frame_length(),
                            reference_count_offset,
                            scope.reference_members().len(),
                        )
                    });
                let extent_valid = if let Some(frame) = class_397_frame {
                    operation_prefix_marker_offset.is_none()
                        && scope
                            .byte_offset()
                            .checked_add(u64_from_index(class_397::OPERATION))
                            .is_some_and(|expected_offset| operation_offset == expected_offset)
                        && direction_face_extend_values
                            == [class_397::DIRECTION_VALUE, class_397::FACE_EXTEND_VALUE]
                        && extent.is_some()
                        && extent
                            == frame
                                .extent(direction_face_extend_values[0], side_extent_discriminators)
                } else if compact_extent_offsets.is_some() {
                    matches!(
                        (
                            direction_face_extend_values,
                            side_extent_discriminators,
                            extent,
                        ),
                        (
                            [1, _],
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                        ) | (
                            [3, _],
                            [1, 0],
                            Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                        ) | (
                            [2, 0],
                            [1, 2],
                            Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace)
                        )
                    )
                } else if class_296_extent_offsets.is_some() {
                    direction_face_extend_values[0] == 1
                        && matches!(direction_face_extend_values[1], 1 | 2)
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                } else if class_296_symmetric_extent_offsets.is_some() {
                    direction_face_extend_values == [3, 2]
                        && side_extent_discriminators == [1, 0]
                        && extent
                            == Some(
                                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance,
                            )
                } else if class_296_two_faces_extent_offsets.is_some() {
                    direction_face_extend_values[0] == 2
                        && matches!(direction_face_extend_values[1], 1 | 2)
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces)
                } else if class_296_legacy_to_face_extent_offsets.is_some() {
                    direction_face_extend_values == [1, 1]
                        && side_extent_discriminators == [2, 0]
                        && extent
                            == Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                } else if class_296_legacy_distance_extent_offsets.is_some() {
                    direction_face_extend_values == [1, 2]
                        && side_extent_discriminators == [1, 0]
                        && extent
                            == Some(
                                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance,
                            )
                } else {
                    matches!(direction_face_extend_values[0], 1..=3)
                        && matches!(
                            (
                                direction_face_extend_values[0],
                                side_extent_discriminators,
                                extent,
                            ),
                            (
                                1,
                                [1, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedDistance)
                            ) | (
                                1,
                                [2, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                            ) | (
                                1,
                                [3, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext),
                            ) | (
                                1,
                                [4, 0],
                                Some(records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll)
                            ) | (
                                2,
                                [1, 1],
                                Some(records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance)
                            ) | (
                                3,
                                [1, 0 | 1],
                                Some(records::feature::extrude::DesignExtrudeExtent::SymmetricDistance)
                            ) | (
                                3,
                                [4, 4],
                                Some(records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll)
                            )
                        )
                };
                let side_offsets_valid = if class_397_frame.is_some() {
                    scope
                        .byte_offset()
                        .checked_add(u64_from_index(class_397::FIRST_SIDE_EXTENT))
                        .zip(
                            scope
                                .byte_offset()
                                .checked_add(u64_from_index(class_397::SECOND_SIDE_EXTENT)),
                        )
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            side_extent_discriminator_offsets == expected_offset
                        })
                } else {
                    compact_extent_offsets
                        .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_symmetric_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_two_faces_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_legacy_to_face_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || class_296_legacy_distance_extent_offsets
                            .is_some_and(|offsets| side_extent_discriminator_offsets == offsets)
                        || field_shift.is_some_and(|field_shift| {
                            (if direction_face_extend_values[0] == 2 {
                                if scope
                                    .reference_count_offset()
                                    .checked_sub(scope.byte_offset())
                                    .and_then(|offset| offset.checked_sub(field_shift))
                                    == Some(283)
                                {
                                    scope
                                        .byte_offset()
                                        .checked_add(166 + field_shift)
                                        .zip(scope.byte_offset().checked_add(181 + field_shift))
                                        .map(|(first, second)| [first, second])
                                } else {
                                    scope
                                        .byte_offset()
                                        .checked_add(155 + field_shift)
                                        .zip(scope.byte_offset().checked_add(178 + field_shift))
                                        .map(|(first, second)| [first, second])
                                }
                            } else if side_extent_discriminators[0] == 2 {
                                let first_offset = side_extent_discriminator_offsets[0];
                                if matches!(
                                    first_offset
                                        .checked_sub(scope.byte_offset())
                                        .and_then(|offset| offset.checked_sub(field_shift)),
                                    Some(106 | 116)
                                ) {
                                    scope
                                        .reference_count_offset()
                                        .checked_sub(4)
                                        .map(|second| [first_offset, second])
                                } else {
                                    Some([0, 0])
                                }
                            } else if scope
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .zip(scope.byte_offset().checked_add(129 + field_shift))
                                .map(|(first, second)| [first, second])
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets == expected_offset
                                })
                            {
                                Some(side_extent_discriminator_offsets)
                            } else if scope
                                .byte_offset()
                                .checked_add(116 + field_shift)
                                .is_some_and(|expected_offset| {
                                    side_extent_discriminator_offsets[0] == expected_offset
                                })
                            {
                                scope
                                    .byte_offset()
                                    .checked_add(116 + field_shift)
                                    .zip(scope.byte_offset().checked_add(130 + field_shift))
                                    .map(|(first, second)| [first, second])
                            } else {
                                scope
                                    .byte_offset()
                                    .checked_add(106 + field_shift)
                                    .zip(scope.byte_offset().checked_add(110 + field_shift))
                                    .map(|(first, second)| [first, second])
                            })
                            .is_some_and(|expected_offset| {
                                side_extent_discriminator_offsets == expected_offset
                            })
                        })
                };
                (field_shift.is_some()
                    || compact_extent_offsets.is_some()
                    || class_296_extent_offsets.is_some()
                    || class_296_symmetric_extent_offsets.is_some()
                    || class_296_two_faces_extent_offsets.is_some()
                    || class_296_legacy_to_face_extent_offsets.is_some()
                    || class_296_legacy_distance_extent_offsets.is_some())
                    && extent_valid
                    && side_offsets_valid
                    && operation_offset
                        .checked_add(4)
                        .zip(operation_offset.checked_add(8))
                        .map(|(first, second)| [first, second])
                        .is_some_and(|expected_offset| {
                            direction_face_extend_offsets == expected_offset
                        })
                    && operation_offset
                        .checked_add(14)
                        .is_some_and(|expected_offset| start_offset == expected_offset)
                    && operation_offset
                        .checked_add(13)
                        .is_some_and(|expected_offset| solid_operation_offset == expected_offset)
                    && operation_offset
                        .checked_add(12)
                        .is_some_and(|expected_offset| direction_reversed_offset == expected_offset)
                    && direction_face_extend_offsets[1] < scope.reference_count_offset()
            }
            None => true,
        } && match &scope.payload() {
            records::feature::scope::DesignScopePayload::SurfaceStitch(operation) => {
                operation.gap_tolerance_offset > scope.paired_byte_offset()
                    && scope.reference_members().len() >= 4
                    && scope.reference_members().len().is_multiple_of(2)
                    && scope.reference_members().values().rev().nth(1)
                        == Some(&operation.tolerance_record_index)
                    && scope.reference_members().values().next_back()
                        == Some(&operation.settings_record_index)
            }
            _ => true,
        } && match &scope.payload() {
            records::feature::scope::DesignScopePayload::SurfaceRuled(operation) => {
                scope
                    .byte_offset()
                    .checked_add(20)
                    .is_some_and(|expected_offset| operation.method_offset == expected_offset)
                    && scope
                        .byte_offset()
                        .checked_add(27)
                        .is_some_and(|expected_offset| {
                            operation.alternate_face_offset == expected_offset
                        })
                    && scope
                        .byte_offset()
                        .checked_add(50)
                        .is_some_and(|expected_offset| operation.corner_offset == expected_offset)
                    && scope.reference_members().values().next()
                        == Some(&operation.distance_owner_record_index)
                    && scope.reference_members().values().nth(1)
                        == Some(&operation.angle_owner_record_index)
                    && operation.distance_owner_record_index != operation.angle_owner_record_index
                    && !operation.edge_group_record_indices.is_empty()
                    && operation
                        .edge_group_record_indices
                        .iter()
                        .all(|record_index| {
                            scope
                                .reference_members()
                                .values()
                                .any(|value| value == record_index)
                        })
                    && match operation.method {
                        records::feature::surface_ops::DesignRuledSurfaceMethod::Direction => {
                            operation.direction_entity_id.is_some()
                        }
                        records::feature::surface_ops::DesignRuledSurfaceMethod::Normal
                        | records::feature::surface_ops::DesignRuledSurfaceMethod::Tangent => {
                            operation.direction_entity_id.is_none()
                        }
                    }
            }
            _ => true,
        } && scope
            .reference_members()
            .values()
            .all(|record_index| records_by_index.contains_key(&(native_stream, *record_index)))
            && records_by_index.contains_key(&(native_stream, scope.record_index))
            && entity_link.unwrap_or(true)
            && extrude_profile_link
            && sweep_profile_link
            && base_flange_profile_link
            && emboss_profile_link
            && move_link
            && offset_faces_link
            && base_flange_link
            && edge_flange_link
            && hem_link
            && copy_paste_link
            && rectangular_pattern_link
            && assembly_alignment_link
            && scope.align_envelope().is_none_or(|envelope| {
                design::decode::scopes::align::align_envelope_links_hold(scope, envelope)
                    && envelope.construction.as_ref().is_none_or(|construction| {
                        records_by_index
                            .get(&(native_stream, construction.record_index))
                            .is_some_and(|header| {
                                header.byte_offset == construction.byte_offset
                                    && header.class_tag == construction.class_tag
                            })
                    })
            })
            && component_insert_link
            && copy_paste_component_link
            && scope.occurrence_transfer.as_ref().is_none_or(|operation| {
                operation.links_valid(
                    scope,
                    |index| {
                        records_by_index
                            .get(&(native_stream, index))
                            .map(|header| header.byte_offset)
                    },
                    |index, guid| {
                        native
                            .design_types
                            .iter()
                            .filter(|kind| {
                                ids::design_segment(kind.id()) == ids::design_segment(&scope.id)
                                    && kind.type_guid.as_str().eq_ignore_ascii_case(guid)
                                    && kind
                                        .entities
                                        .values()
                                        .any(|entity| *entity == u64::from(index))
                            })
                            .count()
                            == 1
                    },
                )
            })
            && draft_link
            && combine_link
            && thread_link
            && joint_origin_link
            && work_point_link
            && work_plane_link
            && (scope.sketch_entity().is_none()
                || placements_by_scope.contains_key(&(native_stream, scope.record_index)))
            && unique_index;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter scope has an invalid paired frame",
                Some(scope.id.clone()),
            );
        }
    }
    Ok(())
}

/// Entity of the document-root component in a Design that numbers it 3.
const STANDARD_ROOT_COMPONENT_ENTITY: u32 = 3;
/// Type GUID of the Design component entities.
const COMPONENT_TYPE_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";

/// Whether an ACT component link tracks the document-root component. The
/// root is entity 3 of the component type. A Design whose component type has
/// no entity 3 (a component saved as its own document) has its root link name
/// another entity of that type.
fn act_link_is_document_root(
    native: &crate::native::F3dNative,
    stream: &str,
    tracked: u32,
) -> bool {
    if tracked == STANDARD_ROOT_COMPONENT_ENTITY {
        return true;
    }
    let folder = stream.rsplitn(3, '/').nth(2);
    let component_entities = |entity: u32| {
        native.design_types.iter().any(|kind| {
            kind.type_guid
                .as_str()
                .eq_ignore_ascii_case(COMPONENT_TYPE_GUID)
                && ids::native_stream(kind.id()).and_then(|design| design.rsplitn(3, '/').nth(2))
                    == folder
                && kind
                    .entities
                    .values()
                    .any(|value| *value == u64::from(entity))
        })
    };
    !component_entities(STANDARD_ROOT_COMPONENT_ENTITY) && component_entities(tracked)
}

/// Whether a scope stores 51-byte typed reference-table entries: the last
/// entry's target is followed by the 7-byte entry tail and the kind string.
fn scope_has_typed_references(scope: &records::feature::scope::DesignParameterScope) -> bool {
    scope
        .reference_members()
        .offsets()
        .last()
        .and_then(|last| scope.kind_offset().checked_sub(*last))
        == Some(u64_from_index(crate::bytes::TYPED_LOCAL_REFERENCE_LEN) + 7)
}

/// Whether a reference-aware Extrude scope without a prologue reference
/// starts its operation fields at an admitted offset. The plain form starts
/// them at offset 24 or 28; a typed-reference scope starts them at offset 22
/// or 26.
fn direct_extrude_operation_valid(
    scope_offset: u64,
    typed_references: bool,
    operation_offset: u64,
) -> bool {
    let admitted: &[usize] = if typed_references {
        &[
            crate::layout::typed_reference_extrude_prologue::OPERATION,
            crate::layout::shifted_typed_reference_extrude_prologue::OPERATION,
        ]
    } else {
        &[crate::layout::offset_24_extrude_prologue::OPERATION, 28]
    };
    admitted
        .iter()
        .any(|delta| scope_offset.checked_add(u64_from_index(*delta)) == Some(operation_offset))
}

/// Whether the span between the empty-slot end of the operation fields and
/// the first side extent holds whole present extent slots. A present slot
/// replaces its one-byte absent marker with a reference, which widens the
/// span by 10 bytes (plain) or 50 bytes (typed) for each of the seven slots.
fn extent_slot_expansion_valid(typed_references: bool, slot_expansion: u64) -> bool {
    let unit = if typed_references { 50 } else { 10 };
    slot_expansion <= 7 * unit && slot_expansion.is_multiple_of(unit)
}

fn valid_work_point_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> Result<bool, CodecError> {
    let Some(construction) = scope.work_point_construction() else {
        return Ok(true);
    };
    let native = ctx.native;
    if construction.point_record_byte_offset >= construction.position_offset
        || construction.position_offset >= construction.reference_type_offset
        || !scope
            .reference_members()
            .values()
            .any(|value| value == &construction.point_record_index)
        || ctx
            .records_by_index
            .get(&(native_stream, construction.point_record_index))
            .is_none_or(|header| header.byte_offset != construction.point_record_byte_offset)
    {
        return Ok(false);
    }

    construction
        .rule
        .inputs()
        .iter()
        .try_fold(true, |valid, input| {
            if !valid {
                return Ok(false);
            }
            let header = ctx
                .records_by_index
                .get(&(native_stream, input.record_index()));
            Ok(scope
                .reference_members()
                .values()
                .any(|value| value == &input.record_index())
                && input.reference_offset > construction.reference_type_offset
                && header.is_some() && match input.carrier() {
                None => true,
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id,
                    },
                ) => native.design_edge_operands.iter().any(|operand| {
                    operand.id == *operand_id
                        && design_stream(&operand.id) == native_stream
                        && operand.scope_record_index == scope.record_index
                        && operand.record_index() == input.record_index()
                }),
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
                        recipe: vertex,
                    },
                ) => valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), vertex),
                Some(records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
                    selection,
                }) => {
                    header.is_some_and(|header| {
                        header.class_tag == selection.class_tag
                            && selection.asset_id_offset() > header.byte_offset
                    }) && u32::try_from(selection.primary_identity)
                        .ok()
                        .and_then(|identity| identity.checked_add(1))
                        == Some(selection.work_plane_scope_record_index)
                        && native.design_parameter_scopes.iter().any(|plane| {
                            design_stream(&plane.id) == native_stream
                                && plane.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::WorkPlane
                                && plane.record_index == selection.work_plane_scope_record_index
                        })
                }
                Some(
                    records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                        selection,
                    },
                ) => {
                    header.is_some_and(|header| {
                        header.class_tag == selection.class_tag
                            && selection.asset_id_offset() > header.byte_offset
                    }) && u32::try_from(selection.point_persistent_id).is_ok()
                        && !selection.point_native_id.trim().is_empty()
                        && native.sketch_points.iter().any(|point| {
                            point.id == selection.point_native_id
                                && design_stream(&point.id) == native_stream
                                && point.owner_reference == Some(selection.sketch_record_index)
                                && point.persistent_id() == Some(selection.point_persistent_id)
                        })
                }
            })
        })
}

fn valid_work_plane_construction(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
) -> bool {
    let Some(frame) = scope.work_plane_frame() else {
        return true;
    };
    let placements = &ctx.native.design_placement_constructions;
    let typed_operand =
        crate::design::decode::placement_construction::work_planes::has_operand(scope, placements);
    if typed_operand {
        return crate::design::decode::placement_construction::work_planes::solved_frame(
            scope, placements,
        )
        .as_ref()
            == Some(frame);
    }
    let Some(construction) = &frame.work_plane_construction else {
        return true;
    };
    let placement_record_index = &construction.placement_record_index;
    let inputs = construction.inputs();
    let Some([placement, first, second, third, extra_offset]) =
        scope.reference_members().values_array()
    else {
        return false;
    };
    let Some(placement_header) = ctx
        .records_by_index
        .get(&(native_stream, *placement_record_index))
    else {
        return false;
    };
    let transform_offset = frame.work_plane_transform_offset;
    let Some(owner) = ctx.native.design_parameter_owners.iter().find(|owner| {
        design_stream(owner.id()) == native_stream
            && owner.record_index() == *extra_offset
            && owner.scope_record_index() == scope.record_index
            && owner.evaluated_value().get() == 0.0
    }) else {
        return false;
    };

    placement == placement_record_index
        && [
            inputs[0].record_index(),
            inputs[1].record_index(),
            inputs[2].record_index(),
        ] == [*first, *second, *third]
        && scope.work_plane_reference() == Some(*extra_offset)
        && scope
            .work_plane_frame()
            .and_then(|frame| frame.reference.as_ref())
            .is_some()
        && transform_offset > placement_header.byte_offset
        && inputs.iter().all(|input| {
            valid_vertex_recipe(ctx, scope, native_stream, input.record_index(), input)
        })
        && ctx.native.design_parameters.iter().any(|parameter| {
            design_stream(&parameter.id) == native_stream
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(owner.record_index())
                && parameter.source_kind() == "ExtraOffset"
                && parameter.evaluated_value().get() == 0.0
        })
}

fn valid_vertex_recipe(
    ctx: &Ctx,
    scope: &records::feature::scope::DesignParameterScope,
    native_stream: &str,
    record_index: u32,
    vertex: &records::feature::work_geometry::DesignVertexRecipe,
) -> bool {
    let native = ctx.native;
    let header = ctx.records_by_index.get(&(native_stream, record_index));
    let recipe = ctx.recipes_by_id.get(vertex.recipe_id.as_str());
    let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
        &vertex.recipe_prefix_bytes,
        vertex.recipe_prefix_offset(),
    );
    for reference in &mut expected_references {
        design::decode::dimension_frames::bind_recipe_reference_candidates(
            reference,
            &native.persistent_subentity_tags,
            Some(&scope.id),
        );
    }
    let prefix_length = u64::try_from(vertex.recipe_prefix_bytes.len()).ok();
    let family_name_length = u64::try_from(design::construction_recipe_family_name_len(
        records::recipes::ConstructionRecipeKind::Vertex,
    ))
    .ok();
    let program_byte_length = u64::try_from(vertex.recipe_program.len())
        .ok()
        .and_then(|length| length.checked_mul(4));
    let resolution_is_valid = match vertex.resolution {
        None => true,
        Some(resolution) => {
            let state_id = resolution.state_id;
            let vertex_slot = resolution.vertex_slot();
            let mut states = native
                .asm_histories
                .iter()
                .flat_map(|history| history.states().iter().map(move |state| (history, state)))
                .filter(|(_, state)| state.state_id == state_id);
            states.next().is_some_and(|(history, state)| {
                states.next().is_none()
                    && history.topology(state).map_or_else(
                        || history::projection_was_finalized(&native.asm_histories),
                        |topology| topology.vertices.contains(&vertex_slot),
                    )
            })
        }
    };
    vertex.record_index() == record_index
        && header.is_some_and(|header| {
            header.byte_offset == vertex.byte_offset() && header.class_tag == vertex.class_tag
        })
        && prefix_length.is_some_and(|prefix_length| {
            vertex
                .recipe_prefix_offset()
                .checked_add(prefix_length)
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
        })
        && vertex.recipe_references == expected_references
        && resolution_is_valid
        && recipe.is_some_and(|recipe| {
            design_stream(&recipe.id) == native_stream
                && recipe.kind == records::recipes::ConstructionRecipeKind::Vertex
                && recipe.byte_offset > vertex.recipe_record_byte_offset()
                && recipe.byte_offset < vertex.next_byte_offset()
                && family_name_length.is_some_and(|family_name_length| {
                    recipe
                        .byte_offset
                        .checked_add(family_name_length)
                        .is_some_and(|expected_offset| {
                            vertex.recipe_program_offset == expected_offset
                        })
                })
        })
        && program_byte_length.is_some_and(|program_byte_length| {
            program_byte_length != 0
                && vertex
                    .recipe_program_offset
                    .checked_add(program_byte_length)
                    .is_some_and(|expected_offset| expected_offset == vertex.next_byte_offset())
        })
}

fn validate_component_occurrences(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    struct LowerAscii<'a>(&'a str);
    impl std::fmt::Display for LowerAscii<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for character in self.0.chars() {
                write!(formatter, "{}", character.to_ascii_lowercase())?;
            }
            Ok(())
        }
    }
    let mut identities = HashSet::new();
    let mut record_indices = HashSet::new();
    for occurrence in &ctx.native.design_component_occurrences {
        let stream = design_stream(&occurrence.id);
        let key = format!("{}", LowerAscii(occurrence.occurrence_guid.as_str()));
        let unique_identity = identities.insert((stream, key));
        let unique_record = record_indices.insert((stream, occurrence.record_index));
        // A class-256 carrier stores a placed frame only after its first
        // occurrence; every other carrier class can place ordinal one.
        let placement_admitted = match occurrence.placement() {
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
            | records::feature::assembly_features::DesignComponentOccurrencePlacement::Identity {
                ..
            } => true,
            records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                ordinal,
                ..
            } => occurrence.class_tag.as_str() != "256" || ordinal.get() > 1,
        };
        // The duplicated references must agree within one carrier, which
        // the decoder checks. The component-record reference names the
        // definition; carriers of one definition can store different
        // carrier GUIDs.
        for (holds, message) in [
            (
                unique_identity,
                "Fusion Design component occurrence GUID repeats in its Design stream",
            ),
            (
                unique_record,
                "Fusion Design component occurrence record index repeats in its Design stream",
            ),
            (
                placement_admitted,
                "Fusion Design class-256 component occurrence has a placed frame at ordinal one",
            ),
        ] {
            if !holds {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    message,
                    Some(occurrence.id.clone()),
                );
            }
        }
    }
}

fn valid_component_pattern_occurrences(
    native: &native::F3dNative,
    stream: &str,
    instances: &records::feature::patterns::DesignRectangularPatternInstances,
) -> bool {
    let records::feature::patterns::DesignRectangularPatternInstances::Components {
        component_guid,
        seed,
        generated,
    } = instances
    else {
        return true;
    };
    native
        .design_component_occurrences
        .iter()
        .any(|occurrence| {
            design_stream(&occurrence.id) == stream
                && occurrence
                    .component_guid
                    .as_str()
                    .eq_ignore_ascii_case(component_guid.as_str())
                && occurrence
                    .occurrence_guid
                    .as_str()
                    .eq_ignore_ascii_case(seed.occurrence_guid.as_str())
                && matches!(
                    occurrence.placement(),
                    crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base
                )
        })
        && generated.iter().enumerate().all(|(ordinal, row)| {
            native
                .design_component_occurrences
                .iter()
                .any(|occurrence| {
                    design_stream(&occurrence.id) == stream
                        && occurrence
                            .component_guid
                            .as_str()
                            .eq_ignore_ascii_case(component_guid.as_str())
                        && occurrence
                            .occurrence_guid
                            .as_str()
                            .eq_ignore_ascii_case(row.occurrence_guid.as_str())
                        && u32::try_from(ordinal).ok().and_then(|ordinal| ordinal.checked_add(2)) == Some(occurrence.occurrence_ordinal())
                        && occurrence.transform() == Some(row.instance.transform)
                })
        })
}

/// Validate compact selection groups and their counted member frames.
fn validate_extrude_selection_groups(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut group_slots = HashSet::new();
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let frame_valid = scope.is_some_and(|scope| {
            (design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Extrude)
                || scope
                    .emboss_operand_construction()
                    .is_some_and(|construction| {
                        group.record_index == construction.auxiliary_group.value
                            && group.scope_reference_ordinal == 8
                            && !group.typed_references
                    }))
                && usize::try_from(group.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.record_index)
        }) && header.is_some_and(|header| {
            header.byte_offset == group.byte_offset() && header.class_tag == group.class_tag
        }) && group
            .members()
            .iter()
            .all(|member| records_by_index.contains_key(&(native_stream, member.value)));
        let valid = if frame_valid {
            group_slots.insert((
                native_stream,
                group.scope_record_index,
                group.scope_reference_ordinal,
            ))
        } else {
            false
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design selection group has an invalid counted frame",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate construction operand groups and their role discriminators.
fn validate_construction_operand_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut operand_group_slots = HashSet::new();
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let header = records_by_index.get(&(native_stream, group.record_index));
        let frame = &group.frame;
        let member_run_end = group.members().last().map_or_else(
            || frame.member_count_offset.checked_add(4),
            |member| member.offset.checked_add(10),
        );
        let frame_valid = group
            .byte_offset
            .checked_add(
                if scope.is_some_and(|scope| {
                    scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                        || (scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::SplitFace
                            && group.role() == DesignOperandRole::ROLE_0X21)
                        || (scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::Split
                            && matches!(
                                group.role(),
                                DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X21
                            ))
                }) {
                    88
                } else {
                    21
                },
            )
            .is_some_and(|expected_offset| frame.member_count_offset == expected_offset)
            && group.members().first().is_none_or(|member| {
                frame
                    .member_count_offset
                    .checked_add(5)
                    .is_some_and(|expected_offset| member.offset == expected_offset)
            })
            && frame.trailing_records().first().is_none_or(|record| {
                group
                    .role_offset()
                    .checked_sub(record.offset)
                    .is_some_and(|span| matches!(span, 10 | 50))
            })
            && member_run_end.is_some_and(|end| group.role_offset() >= end)
            && group.role().raw().trailing_zeros() >= 32
            && frame
                .opaque_scalar_offset()
                .checked_add(8)
                .is_some_and(|expected_offset| group.paired_byte_offset > expected_offset)
            && frame
                .auxiliary_records
                .iter()
                .map(|record| &record.value)
                .chain(frame.trailing_records().iter().map(|record| &record.value))
                .all(|record_index| records_by_index.contains_key(&(native_stream, *record_index)))
            && frame.trailing_transforms().iter().all(|transform| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == transform.record_index())
                    && records_by_index
                        .get(&(native_stream, transform.record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == transform.byte_offset()
                                && header.class_tag == transform.class_tag
                        })
                    && records_by_index
                        .get(&(native_stream, transform.following_record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == transform.following_byte_offset()
                                && header.class_tag == transform.following_class_tag
                        })
            })
            && frame.trailing_dual_transforms().iter().all(|transform| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == transform.record_index)
                    && records_by_index
                        .get(&(native_stream, transform.record_index))
                        .is_some_and(|header| {
                            header.byte_offset == transform.byte_offset
                                && header.class_tag == transform.class_tag
                        })
                    && transform
                        .byte_offset
                        .checked_add(21)
                        .is_some_and(|expected_offset| {
                            transform.first_transform_offset == expected_offset
                        })
                    && transform
                        .byte_offset
                        .checked_add(149)
                        .is_some_and(|expected_offset| {
                            transform.second_transform_offset == expected_offset
                        })
            })
            && frame.trailing_flags().iter().all(|flag| {
                frame
                    .trailing_records()
                    .iter()
                    .any(|record| record.value == flag.record_index)
                    && records_by_index
                        .get(&(native_stream, flag.record_index))
                        .is_some_and(|header| {
                            header.byte_offset == flag.byte_offset
                                && header.class_tag == flag.class_tag
                        })
                    && flag
                        .byte_offset
                        .checked_add(22)
                        .is_some_and(|expected_offset| flag.value_offset == expected_offset)
            })
            && frame.auxiliary_paths().iter().all(|path| {
                frame
                    .auxiliary_records
                    .iter()
                    .any(|record| record.value == path.record_index())
                    && records_by_index
                        .get(&(native_stream, path.record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == path.byte_offset()
                                && header.class_tag == path.class_tag
                        })
                    && path.scope_record_index == group.scope_record_index
                    && records_by_index.contains_key(&(native_stream, path.nested_record_index()))
                    && records_by_index
                        .get(&(native_stream, path.following_record_index()))
                        .is_some_and(|header| {
                            header.byte_offset == path.following_byte_offset()
                                && header.class_tag == path.following_class_tag
                        })
            });
        let valid = scope.is_some_and(|scope| {
            let role_is_valid = match design::design_feature_family(&scope.kind()) {
                Some(design::DesignFeatureFamily::Extrude) => {
                    match group.operand_role {
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesA
                        | records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB => true,
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile => {
                            scope.extrude_profile().is_none_or(|profile| {
                                group.members().first().map(|member| &member.value)
                                    == Some(&profile.record_index)
                            })
                        }
                        records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                            encoding,
                            ..
                        } => match encoding {
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces => true,
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::LegacyTermination => {
                                scope.extrude_prologue().and_then(
                                    records::feature::extrude::DesignExtrudePrologue::extent,
                                ) == Some(
                                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                                ) || is_class_296_two_sided_to_faces_scope(scope)
                            }
                            records::topology::extrude_selection::DesignExtrudeFaceEncoding::SelectedStart => {
                                scope
                                    .extrude_prologue()
                                    .map(records::feature::extrude::DesignExtrudePrologue::start)
                                    == Some(records::feature::extrude::DesignExtrudeStart::FromFace)
                                    || design::decode::operands::extrude_has_selected_termination(scope)
                            }
                        },
                        records::topology::construction::DesignConstructionOperandRole::Other(role) => {
                            role == DesignOperandRole::ROLE_0X5
                        }
                    }
                }
                Some(
                    design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer,
                ) => group.extrude_role().is_none(),
                Some(design::DesignFeatureFamily::Coil) => {
                    group.role()
                        == if scope.kind()
                            == crate::records::feature::scope::DesignFeatureKind::CoilPrimitive
                            && scope.reference_members().len() == 10
                            && scope.coil_operation_offset() == scope.byte_offset().checked_add(22)
                        {
                            DesignOperandRole::BODIES_A
                        } else {
                            DesignOperandRole::BODIES_B
                        }
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Move) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::OffsetFaces) => {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Draft) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::ReplaceFace) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X9 | DesignOperandRole::ROLE_0X10
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Revolve) => {
                    (matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::PROFILE
                    ) || (group.role() == DesignOperandRole::ROLE_0X5
                        && matches!(
                            scope.payload(),
                            records::feature::scope::DesignScopePayload::Revolve(
                                None | Some(
                                    records::feature::path_features::DesignRevolveConstruction {
                                        travel:
                                            records::feature::path_features::DesignRevolveTravel::ToTargets(
                                                _
                                            ),
                                        ..
                                    }
                                )
                            )
                        )))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Shell) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X10
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Thicken) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X5 | DesignOperandRole::ROLE_0X12
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Loft) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::BODIES_B
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::PROFILE
                                | DesignOperandRole::ROLE_0X43
                                | DesignOperandRole::ROLE_0X7
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Sweep) => {
                    (!scope.has_path_construction()
                        || matches!(
                            group.role(),
                            DesignOperandRole::BODIES_A
                                | DesignOperandRole::BODIES_B
                                | DesignOperandRole::ROLE_0X5
                                | DesignOperandRole::FACES
                                | DesignOperandRole::PROFILE
                        ))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Pipe) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                    ) && group.extrude_role().is_none()
                }
                Some(
                    design::DesignFeatureFamily::CircularPattern
                    | design::DesignFeatureFamily::RectangularPattern,
                ) => {
                    (matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X10
                    ) || is_path_seed_group(group))
                        && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Mirror) => {
                    mirror_group_role_admitted(group.role()) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfacePatch) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::PROFILE
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceOffset) => {
                    group.role() == DesignOperandRole::PROFILE && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceRuled) => {
                    group.role() == DesignOperandRole::BODIES_B
                        && group.extrude_role().is_none()
                        && scope.ruled_surface_operation().is_some_and(|operation| {
                            operation
                                .edge_group_record_indices
                                .contains(&group.record_index)
                        })
                }
                Some(design::DesignFeatureFamily::BoundaryFill) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X5
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Hole) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SurfaceTrim) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Split) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X9
                            | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Scale) => {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::Thread) => {
                    group.role() == DesignOperandRole::ROLE_0X10
                        && group.extrude_role().is_none()
                        && scope.thread_construction().is_none_or(|construction| {
                            construction
                                .face_group_record_indices
                                .contains(&group.record_index)
                        })
                }
                Some(design::DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B
                            | DesignOperandRole::ROLE_0X21
                            | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(design::DesignFeatureFamily::SheetMetalHem) => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                Some(_) => false,
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::Emboss =>
                {
                    scope.emboss_operand_construction().is_some_and(|construction| {
                        (group.record_index == construction.profile_group.value
                            && group.role() == DesignOperandRole::PROFILE)
                            || (group.record_index == construction.face_group.value
                                && group.role() == DesignOperandRole::ROLE_0X10)
                    }) && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::RemoveBody =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch =>
                {
                    group.role() == DesignOperandRole::ROLE_0X5 && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                {
                    matches!(
                        group.role(),
                        DesignOperandRole::ROLE_0X10 | DesignOperandRole::ROLE_0X21
                    ) && group.extrude_role().is_none()
                }
                None if matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::MoveFace
                        | crate::records::feature::scope::DesignFeatureKind::DeplacerFace
                        | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                ) =>
                {
                    group.role() == DesignOperandRole::ROLE_0X10 && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::Decal =>
                {
                    group.role() == DesignOperandRole::BODIES_A && group.extrude_role().is_none()
                }
                None if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::BaseFlange =>
                {
                    group.role() == DesignOperandRole::PROFILE
                        && group.extrude_role().is_none()
                        && scope.base_flange_profile().as_ref().is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                }
                None if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hem => {
                    matches!(
                        group.role(),
                        DesignOperandRole::BODIES_B | DesignOperandRole::ROLE_0X43
                    ) && group.extrude_role().is_none()
                }
                None => false,
            };
            (design::design_feature_family(&scope.kind()).is_some()
                || matches!(
                    scope.kind(),
                    crate::records::feature::scope::DesignFeatureKind::Emboss
                        | crate::records::feature::scope::DesignFeatureKind::RemoveBody
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                        | crate::records::feature::scope::DesignFeatureKind::SplitFace
                        | crate::records::feature::scope::DesignFeatureKind::MoveFace
                        | crate::records::feature::scope::DesignFeatureKind::DeplacerFace
                        | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                        | crate::records::feature::scope::DesignFeatureKind::Decal
                        | crate::records::feature::scope::DesignFeatureKind::BaseFlange
                        | crate::records::feature::scope::DesignFeatureKind::EdgeFlange
                        | crate::records::feature::scope::DesignFeatureKind::Hem
                ))
                && role_is_valid
                && usize::try_from(group.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.record_index)
                && group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == member)
                    })
        }) && header.is_some_and(|header| {
            header.byte_offset == group.byte_offset && header.class_tag == group.class_tag
        }) && frame_valid
            && (!group.members().is_empty()
                || scope.is_some_and(|scope| {
                    matches!(
                        design::design_feature_family(&scope.kind()),
                        Some(
                            design::DesignFeatureFamily::CircularPattern
                                | design::DesignFeatureFamily::RectangularPattern
                        )
                    )
                }) && is_path_seed_group(group))
            && {
                let mut seen = HashSet::new();
                for member in group.members() {
                    seen.insert(member.value);
                }
                seen.len() == group.members().len()
            }
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|member| records_by_index.contains_key(&(native_stream, *member)))
            && operand_group_slots.insert((native_stream, group.scope_record_index, group.scope_reference_ordinal));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has an invalid frame",
                Some(group.id.clone()),
            );
        }
    }
}

fn is_path_seed_group(
    group: &records::topology::construction::DesignConstructionOperandGroup,
) -> bool {
    group.role() == DesignOperandRole::PATH_SEED
        && group.members().is_empty()
        && group.frame.auxiliary_records.len() == 2
        && group
            .frame
            .auxiliary_paths()
            .iter()
            .map(records::topology::construction::DesignConstructionOperandPath::record_index)
            .eq(group
                .frame
                .auxiliary_records
                .iter()
                .map(|record| record.value))
        && group.frame.trailing_records().len() == 1
        && !group.frame.variant
}

/// Validate path-feature operand roles against the scope construction.
///
/// The role grammar is shared with the fixed Loft projector, though the
/// projector additionally gates admission on scope member count, group
/// ordinals, and legacy carrier pairing (this validator remains role-only and
/// reports valid for shapes the projector leaves Native). Keeping the
/// predicate independent of native byte offsets lets validation reject a
/// malformed role combination without rejecting a valid section/guide mix.
pub(crate) fn loft_operand_roles_are_valid(
    operation: records::feature::extrude::DesignExtrudeOperation,
    groups: &[(DesignOperandRole, usize)],
) -> bool {
    const SECTION: DesignOperandRole = DesignOperandRole::PROFILE;
    const FACE_SECTION: DesignOperandRole = DesignOperandRole::ROLE_0X43;
    const GUIDE: DesignOperandRole = DesignOperandRole::ROLE_0X5;
    const CENTERLINE: DesignOperandRole = DesignOperandRole::ROLE_0X7;
    let is_body = |role: DesignOperandRole| {
        matches!(
            role,
            DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
        )
    };

    let body_count = groups.iter().filter(|(role, _)| is_body(*role)).count();
    let expected_body_count =
        usize::from(operation != records::feature::extrude::DesignExtrudeOperation::NewBody);
    if body_count != expected_body_count {
        return false;
    }
    let operands = || groups.iter().filter(|(role, _)| !is_body(*role));
    let section_count = operands()
        .filter(|(role, _)| matches!(*role, SECTION | FACE_SECTION))
        .count();
    let guide_count = operands().filter(|(role, _)| *role == GUIDE).count();
    let centerline_count = operands().filter(|(role, _)| *role == CENTERLINE).count();

    if section_count >= 2 {
        let roles_are_known = operands()
            .all(|(role, _)| matches!(*role, SECTION | FACE_SECTION | GUIDE | CENTERLINE));
        return roles_are_known
            && centerline_count <= 1
            && !(guide_count > 0 && centerline_count > 0)
            && operands().count() == section_count + guide_count + centerline_count;
    }

    if operation != records::feature::extrude::DesignExtrudeOperation::NewBody {
        return false;
    }

    if section_count == 1 && operands().all(|(role, _)| matches!(*role, FACE_SECTION | GUIDE)) {
        let mut point_ordinals = operands()
            .enumerate()
            .filter(|(_, (role, member_count))| *role == GUIDE && *member_count == 1)
            .map(|(ordinal, _)| ordinal);
        let Some(point_ordinal) = point_ordinals.next() else {
            return false;
        };
        return point_ordinals.next().is_none()
            && (point_ordinal == 0 || point_ordinal + 1 == operands().count())
            && operands()
                .enumerate()
                .all(|(ordinal, (role, member_count))| {
                    ordinal == point_ordinal || *role != GUIDE || *member_count != 1
                });
    }

    if section_count == 0 && operands().count() >= 2 {
        let all_sections = operands().all(|(role, _)| *role == SECTION);
        let all_guides = operands().all(|(role, _)| *role == GUIDE);
        return all_sections || all_guides;
    }

    false
}

/// Validate the operand-group roles of path-feature scopes. The groups are
/// design-intent records, so a document without them has nothing to check.
fn validate_path_feature_operand_roles(ctx: &Ctx, findings: &mut Vec<Finding>) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    // A Pipe construction's complete group relation is admitted with the
    // native records; Intersect has no assigned role sequence.
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        scope.has_path_construction()
            && scope.kind() != records::feature::scope::DesignFeatureKind::Pipe
    }) {
        let native_stream = design_stream(&scope.id);
        let groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .collect::<Vec<_>>();
        let role_count = |role| groups.iter().filter(|group| group.role() == role).count();
        let group_roles = groups
            .iter()
            .map(|group| (group.role(), group.members().len()))
            .collect::<Vec<_>>();
        let valid = match &scope.payload() {
            records::feature::scope::DesignScopePayload::Revolve(Some(
                crate::records::feature::path_features::DesignRevolveConstruction {
                    operation,
                    travel,
                    angle_record_index,
                    opposite_angle,
                    second_side,
                    ..
                },
            )) => {
                let body_count = role_count(DesignOperandRole::BODIES_A)
                    + role_count(DesignOperandRole::BODIES_B);
                let expected_body_count = usize::from(
                    *operation != records::feature::extrude::DesignExtrudeOperation::NewBody,
                );
                let target_groups = match travel {
                    records::feature::path_features::DesignRevolveTravel::Angle(_) => Vec::new(),
                    records::feature::path_features::DesignRevolveTravel::ToTargets(targets) => {
                        std::iter::once(targets.side_one)
                            .chain(targets.side_two)
                            .map(|located| located.value)
                            .collect()
                    }
                };
                target_groups.iter().all(|record_index| {
                    groups.iter().any(|group| {
                        group.record_index == *record_index
                            && group.role() == DesignOperandRole::ROLE_0X5
                            && group.members().len() == 1
                    })
                }) && scope
                    .reference_members()
                    .values()
                    .any(|value| value == angle_record_index)
                    && opposite_angle.is_none_or(|located| {
                        scope
                            .reference_members()
                            .values()
                            .any(|value| value == &located.value)
                    })
                    && second_side.is_none_or(|side| {
                        side.angle_record_index != *angle_record_index
                            && scope
                                .reference_members()
                                .values()
                                .any(|value| value == &side.angle_record_index)
                    })
                    && groups.len() == 2 + expected_body_count + target_groups.len()
                    && role_count(DesignOperandRole::ROLE_0X21) == 1
                    && role_count(DesignOperandRole::PROFILE) == 1
                    && body_count == expected_body_count
            }
            records::feature::scope::DesignScopePayload::Loft(Some(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation, ..
                },
            )) => loft_operand_roles_are_valid(*operation, &group_roles),
            records::feature::scope::DesignScopePayload::Sweep(Some(
                records::feature::scope::DesignSweepScope {
                    construction:
                        Some(records::feature::path_features::DesignSweepConstruction {
                            operation, ..
                        }),
                    ..
                },
            )) => {
                let path_count = role_count(DesignOperandRole::ROLE_0X5);
                let profile_count = role_count(DesignOperandRole::PROFILE);
                let guide_surface_count = role_count(DesignOperandRole::FACES);
                let guide_profile_frame = scope.sweep_profile().is_some_and(|profile| {
                    let profile_groups = || {
                        groups
                            .iter()
                            .filter(|group| group.role() == DesignOperandRole::PROFILE)
                    };
                    profile_groups()
                        .filter(|group| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                        .count()
                        == 1
                        && profile_groups()
                            .filter(|group| {
                                !group
                                    .members()
                                    .iter()
                                    .map(|member| member.value)
                                    .eq([profile.record_index])
                            })
                            .filter(|group| {
                                !group.members().is_empty()
                                    && group.members().iter().map(|member| &member.value).all(
                                        |member| {
                                            native.design_entity_selection_operands.iter().any(
                                                |operand| {
                                                    design_stream(&operand.id) == native_stream
                                                        && operand.scope_record_index
                                                            == scope.record_index
                                                        && operand.group_record_index
                                                            == group.record_index
                                                        && operand.record_index() == *member
                                                },
                                            )
                                        },
                                    )
                            })
                            .count()
                            == 1
                });
                let body_count = role_count(DesignOperandRole::BODIES_A)
                    + role_count(DesignOperandRole::BODIES_B);
                let common_roles =
                    (profile_count == 1 && guide_surface_count == 0 && matches!(path_count, 1 | 2))
                        || (profile_count == 2
                            && guide_surface_count == 1
                            && path_count == 1
                            && guide_profile_frame);
                common_roles
                    && match operation {
                        records::feature::extrude::DesignExtrudeOperation::NewBody => {
                            groups.len() == path_count + profile_count + guide_surface_count
                                && body_count == 0
                        }
                        records::feature::extrude::DesignExtrudeOperation::Join
                        | records::feature::extrude::DesignExtrudeOperation::Cut
                        | records::feature::extrude::DesignExtrudeOperation::Intersect => {
                            guide_surface_count == 0
                                && groups.len() == path_count + 2
                                && body_count == 1
                        }
                    }
            }
            _ => false,
        };
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-feature operand roles conflict with its construction",
                Some(scope.id.clone()),
            );
        }
    }
}

/// Validate Extrude profile, operation, start, and extent operand agreement.
/// The body operands are design-intent records, so the operation is checked
/// against them only in a document that holds them.
fn validate_extrude_parameter_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            design::design_feature_family(&scope.kind()),
            Some(
                design::DesignFeatureFamily::Extrude
                    | design::DesignFeatureFamily::Fillet
                    | design::DesignFeatureFamily::Chamfer
            )
        )
    }) {
        let native_stream = design_stream(&scope.id);
        if design::design_feature_family(&scope.kind())
            == Some(design::DesignFeatureFamily::Extrude)
        {
            let mut profile_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Profile)
                });
            let first_profile_group = profile_groups.next();
            let second_profile_group = profile_groups.next();
            let profile_matches_operand = scope.extrude_profile().is_none_or(|profile| {
                match (first_profile_group, second_profile_group) {
                    (None, _) => {
                        usize::try_from(profile.scope_reference_ordinal)
                            .ok()
                            .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                            == Some(&profile.record_index)
                    }
                    (Some(group), None) => {
                        group.members().first().map(|member| &member.value)
                            == Some(&profile.record_index)
                    }
                    (Some(_), Some(_)) => false,
                }
            });
            if !profile_matches_operand {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude profile conflicts with its profile operand group",
                    Some(scope.id.clone()),
                );
            }
            let has_body_operands = native
                .design_construction_operand_groups
                .iter()
                .any(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role()
                            == Some(records::topology::extrude_selection::DesignExtrudeOperandRole::Bodies)
                });
            let face_operand_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                })
                .count();
            let target_shape_group_count = native
                .design_construction_operand_groups
                .iter()
                .filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.role() == DesignOperandRole::ROLE_0X5
                        && group.extrude_role().is_none()
                        && !group.members().is_empty()
                        && group
                            .members()
                            .iter()
                            .map(|member| &member.value)
                            .enumerate()
                            .all(|(ordinal, record_index)| {
                                id_from_index(ordinal).is_some_and(|ordinal| {
                                    native.design_body_recipe_operands.iter().any(|operand| {
                                        design_stream(&operand.id) == native_stream
                                            && operand.scope_record_index == scope.record_index
                                            && operand.owner.group()
                                                == Some((group.record_index, ordinal))
                                            && operand.record_index() == *record_index
                                    })
                                })
                            })
                })
                .count();
            let operation_matches_operands = match scope
                .extrude_prologue()
                .map(records::feature::extrude::DesignExtrudePrologue::operation)
            {
                Some(records::feature::extrude::DesignExtrudeOperation::NewBody) => {
                    !has_body_operands
                }
                Some(
                    records::feature::extrude::DesignExtrudeOperation::Join
                    | records::feature::extrude::DesignExtrudeOperation::Cut
                    | records::feature::extrude::DesignExtrudeOperation::Intersect,
                ) => has_body_operands || scope.history_state_id().is_none(),
                None => true,
            };
            if ctx.holds_design_intent() && !operation_matches_operands {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Extrude operation conflicts with its body operands",
                    Some(scope.id.clone()),
                );
            }
            let Some(prologue) = scope.extrude_prologue() else {
                continue;
            };
            let Some(extrude_extent) = prologue.extent() else {
                continue;
            };
            let parameter_kind_count = |source_kind: &str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(|parameter| parameter.source_kind() == source_kind)
                    .count()
            };
            let parameter_kind_values = |source_kind: &'static str| {
                native
                    .design_parameter_owners
                    .iter()
                    .filter(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                    })
                    .filter_map(|owner| {
                        parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                    })
                    .filter(move |parameter| parameter.source_kind() == source_kind)
                    .map(|parameter| parameter.evaluated_value().get())
            };
            let along_count = parameter_kind_count("AlongDistance");
            let against_count = parameter_kind_count("AgainstDistance");
            let fixed_lanes = scope.fixed_extrude_parameters();
            // A fixed lane is usually also the parameter owner of its source
            // kind; it adds a carrier only when it is not.
            let lane_count = |source_kind: &str,
                              lane: Option<
                &records::feature::fixed_parameters::DesignFixedExtrudeScalar,
            >| {
                let lane_is_owner = lane.is_some_and(|lane| {
                    native.design_parameter_owners.iter().any(|owner| {
                        design_stream(owner.id()) == native_stream
                            && owner.scope_record_index() == scope.record_index
                            && owner.record_index() == lane.record_index
                            && parameters_by_index
                                .get(&(native_stream, owner.parameter_record_index()))
                                .is_some_and(|parameter| parameter.source_kind() == source_kind)
                    })
                });
                parameter_kind_count(source_kind) + usize::from(lane.is_some() && !lane_is_owner)
            };
            let profile_offset_count = lane_count(
                "ProfileOffset",
                fixed_lanes.and_then(|fixed| fixed.profile_offset.as_ref()),
            );
            let side_one_offset_count = lane_count(
                "Side1Offset",
                fixed_lanes.and_then(|fixed| fixed.side_one_offset.as_ref()),
            );
            // A to-face Extrude's fixed lanes carry only its termination
            // offset and taper.
            let fixed_lanes_are_to_face_lanes = fixed_lanes.is_none_or(|fixed| {
                fixed.along_distance.is_none()
                    && fixed.against_distance.is_none()
                    && fixed.side_two_taper_angle.is_none()
            });
            let omitted_zero_side_one_offset =
                crate::design::face_resolve::extrude_omits_zero_side_one_offset(
                    scope,
                    &prologue,
                    side_one_offset_count,
                );
            let mut side_one_offsets = parameter_kind_values("Side1Offset");
            let side_one_offset_is_absent = match side_one_offsets.next() {
                None => true,
                Some(0.0) => side_one_offsets.next().is_none(),
                Some(_) => false,
            };
            let side_two_offset_count = lane_count(
                "Side2Offset",
                fixed_lanes.and_then(|fixed| fixed.side_two_offset.as_ref()),
            );
            let has_fixed_extrude_parameters = scope.fixed_extrude_parameters().is_some();
            let has_fixed_along = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.along_distance.is_some());
            let fixed_along_uses_reversal = scope
                .fixed_extrude_parameters()
                .as_ref()
                .and_then(|fixed| fixed.along_distance.as_ref())
                .is_some_and(|distance| {
                    matches!(
                        distance,
                        records::feature::fixed_parameters::DesignFixedExtrudeDistance::DistanceConstruction(_)
                    )
                });
            let has_one_along_carrier = along_count <= 1 && (along_count == 1 || has_fixed_along);
            let has_fixed_against = scope
                .fixed_extrude_parameters()
                .as_ref()
                .is_some_and(|fixed| fixed.against_distance.is_some());
            let has_one_against_carrier =
                against_count <= 1 && (against_count == 1 || has_fixed_against);
            let class_296_two_faces_layout = scope
                .reference_count_offset()
                .checked_sub(scope.byte_offset())
                .is_some_and(|reference_count_offset| {
                    is_class_296_two_sided_to_faces_layout(
                        scope.class_tag.as_str(),
                        scope.paired_class_tag.as_str(),
                        scope.frame_length(),
                        reference_count_offset,
                        scope.reference_members().len(),
                    )
                });
            let extent_matches_operands = match extrude_extent {
                records::feature::extrude::DesignExtrudeExtent::OneSidedDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && (!prologue.direction_reversed() || fixed_along_uses_reversal)
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedToFace => {
                    along_count == 0
                        && fixed_lanes_are_to_face_lanes
                        && against_count == 0
                        && if target_shape_group_count == 1 {
                            side_one_offset_is_absent
                        } else {
                            side_one_offset_count == 1 || omitted_zero_side_one_offset
                        }
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces => {
                    along_count == 0
                        && !has_fixed_along
                        && against_count == 0
                        && !has_fixed_against
                        && side_one_offset_count == 1
                        && side_two_offset_count == 1
                        && (!prologue.direction_reversed()
                            || class_296_two_faces_layout
                            || design::decode::operands::extrude_has_selected_termination(scope))
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistance => {
                    has_one_along_carrier
                        && has_one_against_carrier
                        && side_one_offset_count == 0
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace => {
                    along_count == 1
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_count == 0
                        && side_two_offset_count == 1
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricDistance => {
                    has_one_along_carrier
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::SymmetricThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                        && !prologue.direction_reversed()
                }
                records::feature::extrude::DesignExtrudeExtent::OneSidedThroughNext
                | records::feature::extrude::DesignExtrudeExtent::OneSidedThroughAll => {
                    along_count == 0
                        && !has_fixed_extrude_parameters
                        && against_count == 0
                        && side_one_offset_is_absent
                }
            };
            let extrude_start = prologue.start();
            let start_matches_operands = match extrude_start {
                records::feature::extrude::DesignExtrudeStart::ProfilePlane => {
                    profile_offset_count == 0
                }
                records::feature::extrude::DesignExtrudeStart::OffsetProfilePlane
                | records::feature::extrude::DesignExtrudeStart::FromFace => {
                    profile_offset_count == 1
                }
            };
            let expected_face_group_count = usize::from(
                matches!(
                    extrude_extent,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace
                ) && target_shape_group_count == 0,
            ) + 2 * usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces
            )) + usize::from(matches!(
                extrude_extent,
                records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace
            )) + usize::from(matches!(
                extrude_start,
                records::feature::extrude::DesignExtrudeStart::FromFace
            ));
            let mut face_groups = native.design_construction_operand_groups.iter().filter(|group| {
                    design_stream(&group.id) == native_stream
                        && group.scope_record_index == scope.record_index
                        && group.extrude_role().is_some_and(|role| {
                            matches!(role, records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_))
                        })
                }).collect::<Vec<_>>();
            face_groups.sort_by(|left, right| {
                left.scope_reference_ordinal
                    .cmp(&right.scope_reference_ordinal)
            });
            let expected_face_roles = match (extrude_start, extrude_extent) {
                (
                    records::feature::extrude::DesignExtrudeStart::FromFace,
                    records::feature::extrude::DesignExtrudeExtent::OneSidedToFace,
                ) if target_shape_group_count == 0 => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Start,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (records::feature::extrude::DesignExtrudeStart::FromFace, _) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Start]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::OneSidedToFace)
                    if target_shape_group_count == 0 =>
                {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedToFaces) => vec![
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                    records::topology::extrude_selection::DesignExtrudeFaceRole::Termination,
                ],
                (_, records::feature::extrude::DesignExtrudeExtent::TwoSidedDistanceToFace) => {
                    vec![records::topology::extrude_selection::DesignExtrudeFaceRole::Termination]
                }
                _ => Vec::new(),
            };
            let invalid_face_groups_hide_extent_conflict =
                class_296_two_faces_layout && face_operand_group_count == 0;
            if ctx.holds_design_intent()
                && !invalid_face_groups_hide_extent_conflict
                && (!extent_matches_operands
                    || !start_matches_operands
                    || face_operand_group_count != expected_face_group_count
                    || face_groups
                        .iter()
                        .map(|group| group.extrude_face_role())
                        .ne(expected_face_roles.iter().copied().map(Some)))
            {
                push_constant_finding(findings, Check::NativeLinks, "Fusion Design Extrude start or extent conflicts with its parameters and face operands", Some(scope.id.clone()));
            }
        }
        if design::design_feature_family(&scope.kind()) == Some(design::DesignFeatureFamily::Sweep)
        {
            let mut profile_groups =
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|group| {
                        design_stream(&group.id) == native_stream
                            && group.scope_record_index == scope.record_index
                            && group.role() == DesignOperandRole::PROFILE
                    });
            let profile_group = profile_groups.next();
            let profile_matches_operand = profile_groups.next().is_none()
                && scope.sweep_profile().is_none_or(|profile| {
                    profile_group.is_some_and(|group| {
                        group
                            .members()
                            .iter()
                            .map(|member| member.value)
                            .eq([profile.record_index])
                    })
                });
            if !profile_matches_operand {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Sweep profile conflicts with its profile operand group",
                    Some(scope.id.clone()),
                );
            }
        }
    }
}

/// Validate Fillet radius-law parameter assignments; returns the assigned groups.
fn validate_fillet_radius_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let construction_groups_by_index = &ctx.operand_groups_by_index;
    let mut fillet_radius_group_records = HashSet::new();
    let mut fillet_radius_group_slots = HashSet::new();
    for assignment in &native.design_fillet_radius_groups {
        let native_stream = design_stream(&assignment.id);
        let scope = scopes_by_index.get(&(native_stream, assignment.scope_record_index));
        let group =
            construction_groups_by_index.get(&(native_stream, assignment.group_record_index));
        let assignment_parameter = |record_index: u32| {
            let parameter = *parameters_by_index.get(&(native_stream, record_index))?;
            let owner = *owners_by_index.get(&(native_stream, parameter.owner_record_index()?))?;
            (scope.is_some_and(|scope| scope.names_owner(owner))
                && owner.parameter_record_index() == record_index)
                .then_some(parameter)
        };
        let tangency_weight = assignment
            .tangency_weight_parameter_record_index
            .and_then(&assignment_parameter);
        let is_fillet = |scope: &&records::feature::scope::DesignParameterScope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        };
        let valid = scope.is_some_and(is_fillet)
            && group.is_some_and(|group| {
                group.scope_record_index == assignment.scope_record_index
                    && group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq(assignment.edge_operand_record_indices.iter().copied())
            })
            && match &assignment.law {
                records::topology::fillet::DesignFilletRadiusLaw::Constant {
                    radius_parameter_record_index,
                } => {
                    assignment_parameter(*radius_parameter_record_index).is_some_and(|parameter| {
                        parameter.source_kind() == "Radius"
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    })
                }
                records::topology::fillet::DesignFilletRadiusLaw::Chordal {
                    chord_length_parameter_record_index,
                } => assignment_parameter(*chord_length_parameter_record_index).is_some_and(
                    |parameter| {
                        parameter.source_kind() == "ChordLen"
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    },
                ),
                records::topology::fillet::DesignFilletRadiusLaw::Asymmetric {
                    offset_one_parameter_record_index,
                    offset_two_parameter_record_index,
                } => [
                    (*offset_one_parameter_record_index, "EdgeOffset1"),
                    (*offset_two_parameter_record_index, "EdgeOffset2"),
                ]
                .into_iter()
                .all(|(record_index, kind)| {
                    assignment_parameter(record_index).is_some_and(|parameter| {
                        parameter.source_kind() == kind
                            && parameter
                                .unit()
                                .map(|field| field.value.as_str())
                                .is_some_and(design::feature_project::design_length_unit)
                            && parameter.evaluated_value().get() > 0.0
                    })
                }),
                records::topology::fillet::DesignFilletRadiusLaw::Variable {
                    start_radius_parameter_record_index,
                    end_radius_parameter_record_index,
                    middle: midpoint_records,
                } => {
                    let radius = |record_index: u32, kind: &str| {
                        assignment_parameter(record_index)
                            .filter(|parameter| {
                                parameter.source_kind() == kind
                                    && parameter
                                        .unit()
                                        .map(|field| field.value.as_str())
                                        .is_some_and(design::feature_project::design_length_unit)
                                    && parameter.evaluated_value().get() >= 0.0
                            })
                            .map(|parameter| parameter.evaluated_value().get())
                    };
                    let start = radius(*start_radius_parameter_record_index, "StartRadius");
                    let end = radius(*end_radius_parameter_record_index, "EndRadius");
                    let mut middle_positive = false;
                    let mut previous_position = None;
                    let mut middle_valid = true;
                    for row in midpoint_records {
                        let Some(middle_radius) =
                            radius(row.radius_parameter_record_index, "MidRadius")
                        else {
                            middle_valid = false;
                            break;
                        };
                        let Some(position) = assignment_parameter(row.parameter_record_index)
                            .filter(|parameter| {
                                parameter.source_kind() == "MidParams"
                                    && parameter.unit().is_none()
                                    && (0.0..1.0).contains(&parameter.evaluated_value().get())
                            })
                            .map(|parameter| parameter.evaluated_value().get())
                        else {
                            middle_valid = false;
                            break;
                        };
                        if previous_position.is_some_and(|previous| previous >= position) {
                            middle_valid = false;
                            break;
                        }
                        middle_positive |= middle_radius > 0.0;
                        previous_position = Some(position);
                    }
                    start.zip(end).is_some_and(|(start, end)| {
                        middle_valid && (start > 0.0 || end > 0.0 || middle_positive)
                    })
                }
            }
            && assignment
                .tangency_weight_parameter_record_index
                .is_none_or(|_| {
                    tangency_weight.is_some_and(|parameter| {
                        parameter.source_kind() == "TangencyWeight" && parameter.unit().is_none()
                    })
                })
            && fillet_radius_group_records.insert((native_stream, assignment.group_record_index))
            && fillet_radius_group_slots.insert((
                native_stream,
                assignment.scope_record_index,
                assignment.group_ordinal,
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet radius group has an invalid parameter assignment",
                Some(assignment.id.clone()),
            );
        }
    }
    fillet_radius_group_records
}

/// Whether a Mirror scope owns an operand group of this role: faces or
/// bodies to mirror, the mirror plane, or the `0x10` selection run.
fn mirror_group_role_admitted(role: DesignOperandRole) -> bool {
    matches!(
        role,
        DesignOperandRole::BODIES_A
            | DesignOperandRole::ROLE_0X5
            | DesignOperandRole::BODIES_B
            | DesignOperandRole::ROLE_0X10
    )
}

/// Whether a Mirror face operand uses an admitted recipe kind for its group
/// role: a whole-face recipe in a body run, or a face or bounded-face recipe
/// in the mirror-plane run.
fn mirror_operand_recipe_admitted(
    role: DesignOperandRole,
    kind: records::recipes::ConstructionRecipeKind,
) -> bool {
    match role {
        DesignOperandRole::BODIES_B => kind == records::recipes::ConstructionRecipeKind::Face,
        DesignOperandRole::ROLE_0X5 => matches!(
            kind,
            records::recipes::ConstructionRecipeKind::Face
                | records::recipes::ConstructionRecipeKind::BoundedFace
        ),
        _ => false,
    }
}

/// Whether a Revolve axis operand uses an admitted face-recipe spelling. The
/// axis face is selected either by its whole-face recipe or by a bounded-face
/// recipe.
fn revolve_axis_recipe_kind_admitted(kind: records::recipes::ConstructionRecipeKind) -> bool {
    matches!(
        kind,
        records::recipes::ConstructionRecipeKind::Face
            | records::recipes::ConstructionRecipeKind::BoundedFace
    )
}

/// Whether a full-round face operand selects a face. The selection is
/// resolved to face slots when the input state names a unique face; otherwise
/// the operand keeps the faces its recipe is bound against, which are the
/// preceding-state or changed faces it was read with.
fn full_round_face_is_selected(operand: &records::topology::face::DesignFaceOperand) -> bool {
    !operand.resolved_face_slots.is_empty()
        || !operand.preceding_candidate_faces.is_empty()
        || !operand.changed_candidate_faces.is_empty()
}

/// Report Fillet operand groups that carry no radius assignment.
fn validate_fillet_operand_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    fillet_radius_group_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let scope = scopes_by_index.get(&(native_stream, group.scope_record_index));
        let is_fillet = scope.is_some_and(|scope| {
            design::design_feature_family(&scope.kind())
                == Some(design::DesignFeatureFamily::Fillet)
        });
        let mut fixed_edge_group_count = 0;
        let mut is_fixed_edge_group = false;
        if let Some(scope) = scope {
            for candidate in &native.design_construction_operand_groups {
                if design_stream(&candidate.id) == native_stream
                    && candidate.scope_record_index == scope.record_index
                    && !candidate.members().is_empty()
                    && candidate
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            native.design_edge_operands.iter().any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == scope.record_index
                                    && operand.record_index() == *member
                            })
                        })
                {
                    fixed_edge_group_count += 1;
                    is_fixed_edge_group |= candidate.record_index == group.record_index;
                }
            }
        }
        let has_radius_assignment =
            fillet_radius_group_records.contains(&(native_stream, group.record_index));
        let has_parameter_owner = native.design_parameter_owners.iter().any(|owner| {
            design_stream(owner.id()) == native_stream
                && owner.scope_record_index() == group.scope_record_index
        });
        let sole_compact_group_shape = scope.is_some_and(|scope| {
            native
                .design_construction_operand_groups
                .iter()
                .filter(|candidate| {
                    design_stream(&candidate.id) == native_stream
                        && candidate.scope_record_index == scope.record_index
                })
                .count()
                == 1
                && group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| {
                        native.design_edge_identity_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == group.record_index
                                && operand.record_index() == *member
                        })
                    })
        });
        let full_round_group_shape = is_fillet
            && group.role() == DesignOperandRole::BODIES_A
            && !has_radius_assignment
            && !has_parameter_owner
            && scope.is_some_and(|scope| {
                native
                    .design_construction_operand_groups
                    .iter()
                    .filter(|candidate| {
                        design_stream(&candidate.id) == native_stream
                            && candidate.scope_record_index == scope.record_index
                    })
                    .count()
                    == 1
                    && group.members().len() == 1
                    && native.design_edge_operands.iter().all(|operand| {
                        design_stream(&operand.id) != native_stream
                            || operand.scope_record_index != scope.record_index
                            || operand.record_index() != group.members()[0].value
                    })
                    && native.design_face_operands.iter().any(|operand| {
                        design_stream(&operand.id) == native_stream
                            && operand.scope_record_index == scope.record_index
                            && operand.group_record_index() == Some(group.record_index)
                            && operand.group_member_ordinal() == Some(0)
                            && operand.record_index() == group.members()[0].value
                            && operand.recipe_kind
                                == records::recipes::ConstructionRecipeKind::BoundedFace
                    })
            });
        let valid_full_round_group = full_round_group_shape
            && !group.frame.variant
            && group.frame.trailing_records().len() == 1
            && group.frame.trailing_flags().len() == 1
            && group.frame.trailing_records()[0].value
                == group.frame.trailing_flags()[0].record_index
            && group.frame.trailing_flags()[0].value
            && native.design_face_operands.iter().any(|operand| {
                design_stream(&operand.id) == native_stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index() == Some(group.record_index)
                    && operand.group_member_ordinal() == Some(0)
                    && operand.record_index() == group.members()[0].value
                    && full_round_face_is_selected(operand)
            });
        if full_round_group_shape {
            if !valid_full_round_group {
                push_constant_finding(
                    findings,
                    Check::NativeLinks,
                    "Fusion Design Fillet full-round face group is invalid",
                    Some(group.id.clone()),
                );
            }
            continue;
        }
        let in_selection_pair = scope.is_some_and(|scope| {
            let mut scope_groups = native
                .design_construction_operand_groups
                .iter()
                .filter(|candidate| {
                    design_stream(&candidate.id) == native_stream
                        && candidate.scope_record_index == scope.record_index
                })
                .collect::<Vec<_>>();
            scope_groups.sort_by_key(|candidate| candidate.scope_reference_ordinal);
            design::decode::operands::fillet_edge_and_face_groups(&scope_groups).is_some_and(
                |pair| {
                    let face_backed = pair
                        .iter()
                        .filter(|candidate| candidate.role() == DesignOperandRole::ROLE_0X10)
                        .flat_map(|candidate| {
                            candidate
                                .members()
                                .iter()
                                .map(move |member| (candidate, member))
                        })
                        .all(|(candidate, member)| {
                            native.design_face_operands.iter().any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == scope.record_index
                                    && operand.group_record_index() == Some(candidate.record_index)
                                    && operand.record_index() == member.value
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            })
                        });
                    face_backed
                        && pair
                            .iter()
                            .any(|candidate| candidate.record_index == group.record_index)
                },
            )
        });
        let has_fixed_assignment = scope
            .and_then(|scope| scope.fixed_fillet_parameters().map(|fixed| (scope, fixed)))
            .is_some_and(|(scope, fixed)| {
                native.design_parameter_owners.iter().all(|owner| {
                    design_stream(owner.id()) != native_stream
                        || owner.scope_record_index() != scope.record_index
                }) && fixed
                    .groups
                    .iter()
                    .flat_map(|group| {
                        group
                            .tangency_weight()
                            .into_iter()
                            .map(|tangency| tangency.record_index)
                            .chain(group.law().radii().map(|scalar| scalar.record_index))
                            .chain(
                                group
                                    .law()
                                    .intermediate()
                                    .iter()
                                    .map(|row| row.parameter.record_index),
                            )
                    })
                    .all(|record_index| {
                        scope
                            .reference_members()
                            .values()
                            .filter(|member| **member == record_index)
                            .count()
                            == 1
                    })
                    && ((fixed_edge_group_count == fixed.groups.len() && is_fixed_edge_group)
                        || (fixed.groups.len() == 1
                            && (sole_compact_group_shape || in_selection_pair)))
            });
        let retained_fixed_assignment = !has_parameter_owner
            && scope.is_some_and(|scope| {
                design::decode::operands::fixed_fillet_assignment_is_unresolved(
                    scope,
                    &native.design_construction_operand_groups,
                ) && ctx
                    .ir
                    .model
                    .features
                    .iter()
                    .filter(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
                    .all(|feature| {
                        matches!(
                            feature.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Native { .. }
                            )
                        )
                    })
            });
        if is_fillet
            && !retained_fixed_assignment
            && (group.role() == DesignOperandRole::BODIES_B || sole_compact_group_shape)
            && !has_fixed_assignment
            && !has_radius_assignment
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Fillet operand group has no radius assignment",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate construction operand identity chains; returns identity-backed groups.
fn validate_construction_operand_identities<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut operand_identity_groups = HashSet::new();
    for identity in &native.design_construction_operand_identities {
        let native_stream = design_stream(&identity.id);
        let group = operand_groups_by_index.get(&(native_stream, identity.group_record_index));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile());
        let wrapper_shape = identity.wrappers().iter().all(|wrapper| {
            records_by_index
                .get(&(native_stream, wrapper.record_index))
                .is_some_and(|header| {
                    header.byte_offset == wrapper.byte_offset
                        && header.class_tag == wrapper.class_tag
                })
        });
        let transform = group.and_then(|group| group.frame.trailing_transforms().first());
        let tracking_shape = identity.tracking_path().is_none_or(|path| {
            records_by_index
                .get(&(native_stream, path.wrapper_record_index()))
                .is_some_and(|header| {
                    header.byte_offset == path.wrapper_byte_offset()
                        && header.class_tag == path.wrapper_class_tag
                })
                && records_by_index
                    .get(&(native_stream, path.carrier_record_index()))
                    .is_some_and(|header| {
                        header.byte_offset == path.carrier_byte_offset()
                            && header.class_tag == path.carrier_class_tag
                    })
                && records_by_index
                    .get(&(native_stream, path.following_record_index()))
                    .is_some_and(|header| {
                        header.byte_offset == path.following_byte_offset()
                            && header.class_tag == path.following_class_tag
                    })
        });
        let chain_entry = identity
            .wrappers()
            .first()
            .map(|wrapper| {
                (
                    wrapper.record_index,
                    wrapper.byte_offset,
                    &wrapper.class_tag,
                )
            })
            .or_else(|| {
                identity.tracking_path().map(|path| {
                    (
                        path.wrapper_record_index(),
                        path.wrapper_byte_offset(),
                        &path.wrapper_class_tag,
                    )
                })
            })
            .unwrap_or((
                identity.following_record_index(),
                identity.following_byte_offset(),
                identity.following_class_tag(),
            ));
        let chain_entry_shape = transform.is_none_or(|transform| {
            chain_entry
                == (
                    transform.following_record_index(),
                    transform.following_byte_offset(),
                    &transform.following_class_tag,
                )
        });
        let following_shape =
            if identity.tracking_path().is_some() || !identity.wrappers().is_empty() {
                true
            } else {
                transform.is_some_and(|transform| {
                    identity.following_record_index() == transform.following_record_index()
                        && identity.following_byte_offset() == transform.following_byte_offset()
                        && *identity.following_class_tag() == transform.following_class_tag
                })
            } && records_by_index
                .get(&(native_stream, identity.following_record_index()))
                .is_some_and(|header| {
                    header.byte_offset == identity.following_byte_offset()
                        && header.class_tag == *identity.following_class_tag()
                });
        let persistent_shape = identity.persistent_identity().is_none_or(|persistent| {
            selected_profile.is_none_or(|profile| profile.asset_id == persistent.asset_id)
                && (persistent.next_record_index != 0
                    || (identity
                        .following_byte_offset()
                        .checked_add(190)
                        .is_some_and(|expected_offset| {
                            persistent.next_byte_offset() == expected_offset
                        })
                        && !records_by_index.values().any(|header| {
                            design_stream(&header.id) == native_stream
                                && header.byte_offset == persistent.next_byte_offset()
                        })))
                && records_by_index
                    .get(&(native_stream, persistent.next_record_index))
                    // The header arena indexes records named by Design entity
                    // reference lists. A nested identity can terminate at a
                    // structurally parsed record that no entity names, so the
                    // arena is not an exhaustive index of terminal records.
                    .is_none_or(|header| header.byte_offset == persistent.next_byte_offset())
        });
        let valid = group.is_some_and(|group| {
            let trailing = group
                .frame
                .trailing_records()
                .first()
                .map(|record| record.value);
            transform
                .map(super::records::topology::construction::DesignConstructionOperandTransform::record_index)
                .or_else(|| {
                    identity
                        .wrappers()
                        .first()
                        .map(|wrapper| wrapper.record_index)
                })
                .or_else(|| {
                    identity
                        .tracking_path()
                        .map(super::records::topology::construction::DesignConstructionTrackingPath::wrapper_record_index)
                })
                == trailing
        }) && wrapper_shape
            && tracking_shape
            && chain_entry_shape
            && following_shape
            && persistent_shape
            && operand_identity_groups.insert((native_stream, identity.group_record_index));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand identity has an invalid nested frame",
                Some(identity.id.clone()),
            );
        }
    }
    operand_identity_groups
}

/// Validate edge identity operands; returns their backing record set.
fn validate_edge_identity_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut expected_edge_identity_operands =
        reload_native_arena(decode, ctx.ir, "design_edge_identity_operands")?;
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    history::selection::bind_edge_identity_history(
        &mut expected_edge_identity_operands,
        &native.design_construction_operand_identities,
        &native.design_parameter_scopes,
        history::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
        &HashSet::new(),
    );
    history::selection::bind_edge_identity_bounded_face_rules(
        &mut expected_edge_identity_operands,
        expected_face_operands,
    );
    let expected_edge_identity_operands = expected_edge_identity_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut edge_identity_slots = HashSet::new();
    let mut edge_identity_records = HashSet::new();
    for operand in &native.design_edge_identity_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let valid = scope.is_some_and(|scope| {
            matches!(
                design::design_feature_family(&scope.kind()),
                Some(design::DesignFeatureFamily::Fillet | design::DesignFeatureFamily::Chamfer)
            )
        }) && group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && expected_edge_identity_operands.get(operand.id.as_str())
            == Some(&operand)
            && edge_identity_slots.insert((
                native_stream,
                operand.group_record_index,
                operand.group_member_ordinal,
            ))
            && edge_identity_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge identity operand has an invalid fixed frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(edge_identity_records)
}

/// Validate whole-body recipe operands; returns their backing record set.
fn validate_body_recipe_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let mut expected_operands = reload_native_arena(decode, ctx.ir, "design_body_recipe_operands")?;
    design::decode::operands::bind_body_recipe_operand_candidates(
        &mut expected_operands,
        &native.construction_recipes,
        &native.persistent_subentity_tags,
        &native.design_parameter_scopes,
    );
    history::bind_body_recipe_operand_history_candidates(
        &mut expected_operands,
        &native.construction_recipes,
        &native.design_parameter_scopes,
        history::BindingHistories::Shared(&native.asm_histories),
    )?;
    let expected_operands = expected_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut member_slots = HashSet::new();
    let mut operand_records = HashSet::new();
    for operand in &native.design_body_recipe_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let valid_owner = scope.is_some_and(|scope| match operand.owner {
            records::topology::body_recipe::DesignOperandOwner::Group {
                group_record_index,
                group_member_ordinal,
            } => operand_groups_by_index
                .get(&(native_stream, group_record_index))
                .is_some_and(|group| {
                    group.scope_record_index == operand.scope_record_index
                        && usize::try_from(group_member_ordinal)
                            .ok()
                            .and_then(|ordinal| {
                                group.members().get(ordinal).map(|member| &member.value)
                            })
                            == Some(&operand.record_index())
                }),
            records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal,
            } => {
                (scope.kind() == crate::records::feature::scope::DesignFeatureKind::Hole
                    || (!scope_reference_ordinal.is_multiple_of(2)
                        && scope.combine_operation().is_some_and(|operation| {
                            operation.target_record_index == operand.record_index()
                                || operation
                                    .tools
                                    .iter()
                                    .any(|tool| tool.record_index == operand.record_index())
                        })))
                    && usize::try_from(scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index())
            }
        });
        let valid = valid_owner
            && header.is_some_and(|header| {
                header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
            })
            && body_recipe_reference_table_is_admitted(scope.copied(), operand)
            && recipe.is_some_and(|recipe| {
                let selector_is_valid = recipe.design.as_ref().is_some_and(|design| {
                    let design_id = &design.id;
                    let design_id_offset = design_id.offset;
                    let Some(selector) = design.selector else {
                        return false;
                    };
                    u64::try_from(design_id.value.len())
                        .ok()
                        .is_some_and(|length| {
                            let selector_follows_id =
                                design_id_offset.checked_add(length) == Some(selector.byte_offset);
                            let prefix_frame =
                                selector.byte_offset.checked_add(20) == Some(recipe.byte_offset);
                            let body_suffix_frame = recipe
                                .byte_offset
                                .checked_add(u64_from_index(b"body_recipe_data".len()))
                                .and_then(|offset| offset.checked_add(12))
                                == Some(design_id_offset)
                                && selector.value == operand.next_record_index();
                            selector_follows_id
                                && selector.value != 0
                                && (prefix_frame || body_suffix_frame)
                        })
                });
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == records::recipes::ConstructionRecipeKind::Body
                    && recipe.byte_offset > operand.context_id_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
                    && selector_is_valid
            })
            && expected_operands.get(operand.id.as_str()) == Some(&operand)
            && member_slots.insert((native_stream, operand.scope_record_index, operand.owner))
            && operand_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design body recipe operand has an invalid nested frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(operand_records)
}

/// Report operand groups lacking a typed member carrier.
fn validate_operand_group_carriers<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    operand_identity_groups: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    body_recipe_operand_records: &HashSet<(&'a str, u32)>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    for group in &native.design_construction_operand_groups {
        let native_stream = design_stream(&group.id);
        let mut identity_members = native
            .design_edge_identity_operands
            .iter()
            .filter(|operand| {
                design_stream(&operand.id) == native_stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
            })
            .collect::<Vec<_>>();
        identity_members.sort_by_key(|left| left.group_member_ordinal);
        let has_exact_identity_members = !group.members().is_empty()
            && identity_members.len() == group.members().len()
            && identity_members
                .iter()
                .enumerate()
                .all(|(ordinal, operand)| {
                    usize::try_from(operand.group_member_ordinal) == Ok(ordinal)
                        && group.members().get(ordinal).map(|member| &member.value)
                            == Some(&operand.record_index())
                        && edge_identity_records.contains(&(native_stream, operand.record_index()))
                });
        let has_exact_entity_selection_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native
                            .design_entity_selection_operands
                            .iter()
                            .any(|operand| {
                                design_stream(&operand.id) == native_stream
                                    && operand.scope_record_index == group.scope_record_index
                                    && operand.group_record_index == group.record_index
                                    && operand.group_member_ordinal == ordinal
                                    && operand.record_index() == *record_index
                            })
                    })
                });
        let has_exact_face_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_face_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.group_record_index() == Some(group.record_index)
                                && operand.group_member_ordinal() == Some(ordinal)
                                && operand.record_index() == *record_index
                        })
                    })
                });
        let has_exact_body_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .enumerate()
                .all(|(ordinal, record_index)| {
                    id_from_index(ordinal).is_some_and(|ordinal| {
                        native.design_body_recipe_operands.iter().any(|operand| {
                            design_stream(&operand.id) == native_stream
                                && operand.scope_record_index == group.scope_record_index
                                && operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == *record_index
                                && body_recipe_operand_records
                                    .contains(&(native_stream, operand.record_index()))
                        })
                    })
                });
        let has_exact_topology_recipe_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    edge_operand_records.contains(&(native_stream, *record_index))
                        || edge_treatment_vertex_records.contains(&(native_stream, *record_index))
                });
        let has_exact_sketch_profile_member = group.members().len() == 1
            && ctx
                .scopes_by_index
                .get(&(native_stream, group.scope_record_index))
                .is_some_and(|scope| {
                    scope
                        .extrude_profile()
                        .or(scope.sweep_profile())
                        .or(scope.base_flange_profile())
                        .is_some_and(|profile| {
                            group
                                .members()
                                .iter()
                                .map(|member| member.value)
                                .eq([profile.record_index])
                        })
                });
        let has_exact_group_members = !group.members().is_empty()
            && group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|record_index| {
                    native
                        .design_construction_operand_groups
                        .iter()
                        .any(|member| {
                            design_stream(&member.id) == native_stream
                                && member.scope_record_index == group.scope_record_index
                                && member.scope_reference_ordinal > group.scope_reference_ordinal
                                && member.record_index == *record_index
                        })
                });
        let has_exact_trailing_carrier = group.frame.trailing_records().is_empty()
            || operand_identity_groups.contains(&(native_stream, group.record_index))
            || (group.frame.trailing_transforms().len()
                + group.frame.trailing_dual_transforms().len()
                + group.frame.trailing_flags().len()
                == group.frame.trailing_records().len()
                && group
                    .frame
                    .trailing_records()
                    .iter()
                    .map(|record| &record.value)
                    .all(|record_index| {
                        group
                            .frame
                            .trailing_transforms()
                            .iter()
                            .any(|transform| transform.record_index() == *record_index)
                            || group
                                .frame
                                .trailing_dual_transforms()
                                .iter()
                                .any(|transform| transform.record_index == *record_index)
                            || group
                                .frame
                                .trailing_flags()
                                .iter()
                                .any(|flag| flag.record_index == *record_index)
                    }));
        let has_exact_member_carrier = operand_identity_groups
            .contains(&(native_stream, group.record_index))
            || has_exact_identity_members
            || has_exact_entity_selection_members
            || has_exact_face_members
            || has_exact_body_recipe_members
            || has_exact_topology_recipe_members
            || has_exact_sketch_profile_member
            || has_exact_group_members;
        if !has_exact_member_carrier {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact typed member",
                Some(group.id.clone()),
            );
        }
        if !has_exact_trailing_carrier {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design construction operand group has no exact trailing carrier",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate Extrude selection members against their resolved sketch geometry.
fn validate_extrude_selection_members(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let groups_by_index = &ctx.groups_by_index;
    let mut member_slots = HashSet::new();
    let mut member_records = HashSet::new();
    for member in &native.design_extrude_selection_members {
        let native_stream = design_stream(&member.id);
        let group = groups_by_index.get(&(native_stream, member.group_record_index));
        let header = records_by_index.get(&(native_stream, member.record_index()));
        let selected_profile = group
            .and_then(|group| scopes_by_index.get(&(native_stream, group.scope_record_index)))
            .and_then(|scope| scope.extrude_profile());
        let selected_sketch =
            selected_profile.and_then(|profile| u32::try_from(profile.entity_id.suffix()).ok());
        let point_targets = native.sketch_points.iter().filter_map(|point| {
            (selected_sketch.is_some()
                && design_stream(&point.id) == native_stream
                && (point.owner_reference.is_none() || point.owner_reference == selected_sketch)
                && point.persistent_id() == Some(member.local_id))
            .then_some(records::sketch_relations::SketchRelationOperand::Point {
                record_index: point.record_index,
                persistent_id: point.persistent_id(),
            })
        });
        let curve_targets = native.sketch_curve_identities.iter().filter_map(|curve| {
            (selected_sketch.is_some()
                && design_stream(&curve.id) == native_stream
                && (curve.owner_reference.is_none() || curve.owner_reference == selected_sketch)
                && (curve.primary_id.get() == member.local_id
                    || curve.secondary_id != 0 && curve.secondary_id == member.local_id))
                .then_some(records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                })
        });
        let mut targets = point_targets.chain(curve_targets);
        let first_target = targets.next();
        let expected_target = if targets.next().is_none() {
            first_target
        } else {
            None
        };
        let mut expected_identities = native
            .design_construction_operand_identities
            .iter()
            .filter(|identity| {
                design_stream(&identity.id) == native_stream
                    && identity.following_record_index() == member.record_index()
                    && identity.following_byte_offset() == member.byte_offset()
                    && identity.persistent_identity().is_some_and(|persistent| {
                        persistent.local_id == member.local_id
                            && persistent.asset_id == member.asset_id
                            && persistent.context_id == member.context_id
                    })
            })
            .collect::<Vec<_>>();
        expected_identities.sort_by(|left, right| {
            left.wrappers()
                .first()
                .map(|wrapper| wrapper.byte_offset)
                .cmp(&right.wrappers().first().map(|wrapper| wrapper.byte_offset))
        });
        let expected_history = history::selection::historical_extrude_selection_identity_kind(
            member,
            &native.design_component_naming_spaces,
            &native.design_body_bindings,
            &native.asm_histories,
        );
        let history_matches = if history::projection_was_finalized(&native.asm_histories) {
            if let Some(binding) = member.historical.as_ref() {
                binding
                    .state_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == binding.state_ids.len()
                    && binding.state_ids.iter().all(|state_id| {
                        native
                            .asm_histories
                            .iter()
                            .flat_map(crate::history_records::AsmHistory::states)
                            .any(|state| state.state_id == *state_id)
                    })
            } else {
                true
            }
        } else {
            expected_history
                .as_ref()
                .map(|(kind, entity_ref, states)| (*kind, *entity_ref, states.as_slice()))
                == member.historical.as_ref().map(|binding| {
                    (
                        binding.kind,
                        binding.entity_ref,
                        binding.state_ids.as_slice(),
                    )
                })
        };
        let terminal_next = member.next_record_index == 0
            && !records_by_index.values().any(|header| {
                design_stream(&header.id) == native_stream
                    && header.byte_offset == member.next_byte_offset()
            });
        let valid = group.is_some_and(|group| {
            usize::try_from(member.group_member_ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal))
                .map(|reference| reference.value)
                == Some(member.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == member.byte_offset() && header.class_tag == member.class_tag
        }) && member.scope_tail.is_none_or(|tail| {
            group.is_some_and(|group| tail.scope_record_index == group.scope_record_index)
        }) && selected_profile
            .is_none_or(|profile| profile.asset_id == member.asset_id)
            && member.resolved_geometry == expected_target
            && member
                .operand_identity_ids
                .iter()
                .map(String::as_str)
                .eq(expected_identities
                    .iter()
                    .map(|identity| identity.id.as_str()))
            && history_matches
            && (member.next_record_index != 0 || terminal_next)
            && member_slots.insert((
                native_stream,
                member.group_record_index,
                member.group_member_ordinal,
            ))
            && member_records.insert((native_stream, member.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection member has an invalid fixed frame",
                Some(member.id.clone()),
            );
        }
    }
}

/// Validate entity-selection operand nested frames.
fn validate_entity_selection_operands(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let operand_groups_by_index = &ctx.operand_groups_by_index;
    let mut entity_selection_slots = HashSet::new();
    for operand in &native.design_entity_selection_operands {
        let native_stream = design_stream(&operand.id);
        let group = operand_groups_by_index.get(&(native_stream, operand.group_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let valid = group.is_some_and(|group| {
            group.scope_record_index == operand.scope_record_index
                && usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == *operand.class_tag()
        }) && entity_selection_slots.insert((
            native_stream,
            operand.group_record_index,
            operand.group_member_ordinal,
        ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity-selection operand has an invalid nested frame",
                Some(operand.id.clone()),
            );
        }
    }
}

/// Report Extrude selection groups with missing or inconsistent members.
fn validate_extrude_selection_group_members(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let members_by_slot = &ctx.members_by_slot;
    for group in &native.design_extrude_selection_groups {
        let native_stream = design_stream(&group.id);
        if ctx
            .scopes_by_index
            .get(&(native_stream, group.scope_record_index))
            .is_some_and(|scope| scope.emboss_operand_construction().is_some())
        {
            continue;
        }
        let complete = (0..group.members().len()).all(|ordinal| {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            let Some(member) = members_by_slot.get(&(native_stream, group.record_index, ordinal))
            else {
                return false;
            };
            let next = usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| group.members().get(ordinal + 1));
            next.is_none_or(|next_record_index| {
                let Some(next_ordinal) = ordinal.checked_add(1) else {
                    return false;
                };
                let next_member =
                    members_by_slot.get(&(native_stream, group.record_index, next_ordinal));
                member.next_record_index == next_record_index.value
                    && next_member.is_some_and(|next_member| {
                        member.next_byte_offset() == next_member.byte_offset()
                    })
            })
        });
        let context_id = members_by_slot
            .get(&(native_stream, group.record_index, 0))
            .map(|member| member.context_id.as_str());
        let context_consistent = context_id.is_some_and(|context_id| {
            (0..group.members().len()).all(|ordinal| {
                u32::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| {
                        members_by_slot.get(&(native_stream, group.record_index, ordinal))
                    })
                    .is_some_and(|member| member.context_id.as_str() == context_id)
            })
        });
        if !(complete && context_consistent) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude selection group has missing members",
                Some(group.id.clone()),
            );
        }
    }
}

/// Bytes from a face recipe header to the recipe program, by recipe kind.
///
/// `None` states a recipe kind that carries no face operand: it states no
/// program offset at all.
fn recipe_program_operand_length(kind: records::recipes::ConstructionRecipeKind) -> Option<u64> {
    match kind {
        records::recipes::ConstructionRecipeKind::Face => Some(16),
        records::recipes::ConstructionRecipeKind::BoundedFace => Some(24),
        records::recipes::ConstructionRecipeKind::Body
        | records::recipes::ConstructionRecipeKind::Edge
        | records::recipes::ConstructionRecipeKind::Vertex => None,
    }
}

fn recipe_reference_frames_match(
    actual: &[records::dimensions::DesignRecipeReference],
    expected: &[records::dimensions::DesignRecipeReference],
    ignore_derived_candidates: bool,
) -> bool {
    if !ignore_derived_candidates {
        return actual == expected;
    }
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            actual.selector == expected.selector
                && actual.selector_offset == expected.selector_offset
                && actual.token == expected.token
                && actual.token_offset == expected.token_offset
                && actual.design_reference == expected.design_reference
                && actual.design_reference_offset == expected.design_reference_offset
        })
}

/// Validate edge operands and their recipe frames; returns their record set.
fn validate_edge_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(&native.asm_histories)
        || ctx.ir.fragment && native.asm_histories.is_empty();
    let mut edge_operand_slots = HashSet::new();
    let mut edge_operand_records = HashSet::new();
    let mut expected_edge_operands = reload_native_arena(decode, ctx.ir, "design_edge_operands")?;
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    let expected_face_boundaries = history::bind_edge_operand_history_candidates(
        &mut expected_edge_operands,
        &native.design_parameter_scopes,
        &native.construction_recipes,
        history::BindingHistories::Shared(&native.asm_histories),
        &scope_histories,
    )?;
    validate_face_boundaries(
        ctx,
        findings,
        (!historical_candidates_retained).then_some(expected_face_boundaries.as_slice()),
    );
    let expected_edge_operands = expected_edge_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    for operand in &native.design_edge_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let expected_faces = match design_reference {
            Some(design_reference) => design::decode::operands::edge_operand_candidate_faces(
                design_reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            ),
            None => Vec::new(),
        };
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset(),
        );
        if !historical_candidates_retained {
            for reference in &mut expected_references {
                design::decode::dimension_frames::bind_recipe_reference_candidates(
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                );
            }
        }
        let expected_surface_patch_recipe_structure = scope
            .filter(|scope| {
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            })
            .and_then(|_| {
                design::decode::operands::surface_patch_recipe_structure(
                    &operand.recipe_program,
                    operand.recipe_references.len(),
                )
            });
        let terminal_group_member = native
            .design_construction_operand_groups
            .iter()
            .any(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.members().last().map(|member| &member.value)
                        == Some(&operand.record_index())
            });
        let valid = scope.is_some_and(|scope| {
            design::decode::operands::has_edge_recipe_operands(&scope.kind())
                && usize::try_from(operand.scope_reference_ordinal)
                    .ok()
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&operand.record_index())
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && (operand
            .record_index()
            .checked_add(scope.map_or(4, |scope| {
                design::decode::operands::edge_recipe_terminal_delta(&scope.kind())
            }))
            .is_some_and(|expected_offset| operand.next_record_index == expected_offset)
            || terminal_group_member)
            && operand
                .recipe_prefix_offset()
                .checked_add(u64_from_index(operand.recipe_prefix_bytes.len()))
                .zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4)))
                .is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == crate::records::recipes::ConstructionRecipeKind::Edge
                    && recipe.byte_offset > operand.recipe_record_byte_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
            })
            && design::decode::operands::edge_recipe_structure(
                ctx.decode,
                &operand.recipe_program,
            )? == operand.recipe_structure
            && expected_surface_patch_recipe_structure == operand.surface_patch_recipe_structure
            && (historical_candidates_retained || expected_faces == operand.candidate_faces)
            && (ctx.ir.fragment && native.asm_histories.is_empty()
                || expected_edge_operands.get(operand.id.as_str()) == Some(&operand));
        let valid = valid
            && edge_operand_slots.insert((
                native_stream,
                operand.scope_record_index,
                operand.scope_reference_ordinal,
            ))
            && edge_operand_records.insert((native_stream, operand.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand has an invalid scope or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(edge_operand_records)
}

/// Validate `asm_face_boundaries` and every edge-operand reference to them.
///
/// Each boundary belongs to one delta state, carries the identity derived from
/// that state and its face slot, and is named by at least one reference
/// context. Each context names result-state boundaries of its result faces and
/// preceding-state boundaries of its preceding and support faces, in face
/// order. `expected` holds the boundaries re-derived from retained historical
/// topology, when the projection still retains it.
fn validate_face_boundaries<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected: Option<&[records::topology::historical_context::AsmFaceBoundary]>,
) {
    use records::topology::historical_context::AsmFaceBoundary;
    let native = ctx.native;
    if ctx.ir.fragment && native.asm_histories.is_empty() && !native.design_edge_operands.is_empty()
    {
        findings.push(Finding {
            check: Check::FragmentSkipped,
            severity: Severity::Info,
            entity: None,
            related: Vec::new(),
            message:
                "skipped f3d.historical_face_boundaries: cited ASM states are outside this fragment"
                    .into(),
        });
        return;
    }
    let mut states = HashMap::new();
    for history in &native.asm_histories {
        for state in history.states() {
            states.insert(state.id.as_str(), state);
        }
    }
    let mut referenced = HashSet::new();
    for operand in &native.design_edge_operands {
        let stream = design_stream(&operand.id);
        let result_state_id = ctx
            .scopes_by_index
            .get(&(stream, operand.scope_record_index))
            .and_then(|scope| scope.history_state_id());
        let mut result_parent = None;
        let mut preceding_parent = None;
        let mut valid = true;
        let mut check =
            |ids: &'a [String], slots: &[i64], parent: &mut Option<&'a str>, state_id| {
                let mut remaining = slots.iter();
                for id in ids {
                    referenced.insert(id.as_str());
                    let boundary = AsmFaceBoundary::find(&native.asm_face_boundaries, id);
                    let state = boundary.and_then(|boundary| states.get(boundary.parent.as_str()));
                    valid &= boundary.zip(state).is_some_and(|(boundary, state)| {
                        Some(state.state_id) == state_id
                            && *parent.get_or_insert(boundary.parent.as_str()) == boundary.parent
                            && remaining.any(|slot| *slot == boundary.face_slot)
                    });
                }
            };
        for context in &operand.recipe_reference_contexts {
            let slots = |faces: &[cadmpeg_ir::ids::FaceId]| {
                faces
                    .iter()
                    .filter_map(|face| history::stable_ref(face.as_str()))
                    .collect::<Vec<_>>()
            };
            check(
                &context.result_face_boundary_ids,
                &slots(&context.result_faces),
                &mut result_parent,
                result_state_id,
            );
            check(
                &context.preceding_face_boundary_ids,
                &slots(&context.preceding_faces),
                &mut preceding_parent,
                operand.recipe_state_id,
            );
            check(
                &context.preceding_support_face_boundary_ids,
                &context.preceding_support_face_slots,
                &mut preceding_parent,
                operand.recipe_state_id,
            );
        }
        valid &= result_parent
            .zip(preceding_parent)
            .is_none_or(|(result, preceding)| {
                states
                    .get(result)
                    .zip(states.get(preceding))
                    .is_some_and(|(result, preceding)| result.parent == preceding.parent)
            });
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge operand names an invalid historical face boundary",
                Some(operand.id.clone()),
            );
        }
    }
    let expected = expected.map(|expected| {
        expected
            .iter()
            .map(|boundary| (boundary.id.as_str(), boundary))
            .collect::<HashMap<_, _>>()
    });
    for boundary in &native.asm_face_boundaries {
        let valid = states.get(boundary.parent.as_str()).is_some_and(|state| {
            AsmFaceBoundary::identity(&state.id, state.byte_offset, boundary.face_slot).as_deref()
                == Some(boundary.id.as_str())
        }) && referenced.contains(boundary.id.as_str())
            && expected
                .as_ref()
                .is_none_or(|expected| expected.get(boundary.id.as_str()) == Some(&boundary));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM face boundary has an invalid state, identity, or reference",
                Some(boundary.id.clone()),
            );
        }
    }
}

fn validate_edge_treatment_vertex_operands<'a>(
    decode: &DecodeContext<'_>,
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> Result<HashSet<(&'a str, u32)>, CodecError> {
    let native = ctx.native;
    let mut expected = reload_native_arena::<
        records::feature::work_geometry::DesignEdgeTreatmentVertexOperand,
    >(decode, ctx.ir, "design_edge_treatment_vertex_operands")?;
    for operand in &mut expected {
        for reference in &mut operand.recipe.recipe_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
                Some(&operand.id),
            );
        }
    }
    let scope_histories = history::bind_scope_histories(
        &native.design_parameter_scopes,
        &native.design_body_bindings,
        &native.design_body_recipe_operands,
        &native.asm_histories,
    );
    history::bind_edge_treatment_vertex_history(
        &mut expected,
        &native.design_parameter_scopes,
        &native.asm_histories,
        &scope_histories,
    )?;
    let expected = expected
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut records = HashSet::new();
    for operand in &native.design_edge_treatment_vertex_operands {
        let stream = design_stream(&operand.id);
        let scope = ctx
            .scopes_by_index
            .get(&(stream, operand.scope_record_index));
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == stream
                    && group.scope_record_index == operand.scope_record_index
                    && group.record_index == operand.group_record_index
            });
        let group = groups.next();
        let expected_id = crate::ids::native_scoped_id(
            stream,
            "edge-treatment-vertex-operand",
            operand.recipe.byte_offset(),
        );
        let valid = operand.id == expected_id
            && scope.is_some_and(|scope| {
                design::decode::operands::has_edge_recipe_operands(&scope.kind())
                    && usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.recipe.record_index())
            })
            && group.is_some_and(|group| {
                usize::try_from(operand.group_member_ordinal)
                    .ok()
                    .and_then(|ordinal| group.members().get(ordinal).map(|member| &member.value))
                    == Some(&operand.recipe.record_index())
            })
            && groups.next().is_none()
            && expected.get(operand.id.as_str()) == Some(&operand);
        let valid = valid && records.insert((stream, operand.recipe.record_index()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion edge-treatment vertex operand has an invalid group or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(records)
}

/// Report Fillet/Chamfer edge groups with incomplete selection operands. The
/// groups are design-intent records, so a document without them has nothing
/// to check.
fn validate_edge_treatment_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    edge_operand_records: &HashSet<(&'a str, u32)>,
    edge_identity_records: &HashSet<(&'a str, u32)>,
    edge_treatment_vertex_records: &HashSet<(&'a str, u32)>,
) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    for scope in native.design_parameter_scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::Fillet
                | crate::records::feature::scope::DesignFeatureKind::Chamfer
        )
    }) {
        let native_stream = design_stream(&scope.id);
        let mut groups = native
            .design_construction_operand_groups
            .iter()
            .filter(|group| {
                design_stream(&group.id) == native_stream
                    && group.scope_record_index == scope.record_index
            })
            .peekable();
        let complete = groups.peek().is_some()
            && groups.all(|group| {
                let recipe_backed =
                    group
                        .members()
                        .iter()
                        .map(|member| &member.value)
                        .all(|member| {
                            edge_operand_records.contains(&(native_stream, *member))
                                || edge_treatment_vertex_records.contains(&(native_stream, *member))
                        });
                let identity_backed = group
                    .members()
                    .iter()
                    .map(|member| &member.value)
                    .all(|member| edge_identity_records.contains(&(native_stream, *member)));
                recipe_backed || identity_backed
            });
        if !complete {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design edge-treatment group has incomplete selection operands",
                Some(scope.id.clone()),
            );
        }
    }
}

/// Validate face operands and their recipe frames; returns their record set.
fn validate_face_operands<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    expected_face_operands: &[records::topology::face::DesignFaceOperand],
) -> Result<HashSet<(&'a str, u32, u32)>, CodecError> {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let recipes_by_id = &ctx.recipes_by_id;
    let scopes_by_index = &ctx.scopes_by_index;
    let historical_candidates_retained = history::projection_was_finalized(&native.asm_histories)
        || ctx.ir.fragment && native.asm_histories.is_empty();
    let face_groups_by_index = native
        .design_construction_operand_groups
        .iter()
        .map(|group| ((design_stream(&group.id), group.record_index), group))
        .collect::<std::collections::HashMap<_, _>>();
    let expected_face_operands = expected_face_operands
        .iter()
        .map(|operand| (operand.id.as_str(), operand))
        .collect::<std::collections::HashMap<_, _>>();
    let mut face_operand_records = HashSet::new();
    for operand in &native.design_face_operands {
        let native_stream = design_stream(&operand.id);
        let scope = scopes_by_index.get(&(native_stream, operand.scope_record_index));
        let header = records_by_index.get(&(native_stream, operand.record_index()));
        let recipe = recipes_by_id.get(operand.recipe_id.as_str());
        let mut expected_faces = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0)
            .into_iter()
            .flat_map(|design_reference| {
                native
                    .persistent_subentity_tags
                    .iter()
                    .filter(move |tag| {
                        crate::ids::same_native_occurrence(&tag.id, &operand.id)
                            && tag.design_references.contains(&design_reference)
                    })
                    .filter_map(|tag| match &tag.target {
                        cadmpeg_ir::attributes::AttributeTarget::Face(id) => Some(id),
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        expected_faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        expected_faces.dedup();
        let mut expected_references = design::decode::dimension_frames::decode_recipe_references(
            &operand.recipe_prefix_bytes,
            operand.recipe_prefix_offset(),
        );
        if !historical_candidates_retained {
            for reference in &mut expected_references {
                design::decode::dimension_frames::bind_recipe_reference_candidates(
                    reference,
                    &native.persistent_subentity_tags,
                    Some(&operand.id),
                );
            }
        }
        let recipe_design_reference = recipe
            .and_then(|recipe| recipe.record_index)
            .map(|record_index| i64::from(record_index.value))
            .filter(|value| *value >= 0);
        let referenced_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.candidate_faces)
            .collect::<std::collections::HashSet<_>>();
        let expected_unreferenced_faces = expected_faces
            .iter()
            .copied()
            .filter(|face| !referenced_faces.contains(face))
            .collect::<Vec<_>>();
        let mut expected_alternate_selector_faces = expected_references
            .iter()
            .filter(|reference| Some(reference.design_reference) == recipe_design_reference)
            .flat_map(|reference| &reference.alternate_selector_faces)
            .collect::<Vec<_>>();
        expected_alternate_selector_faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        expected_alternate_selector_faces.dedup();
        let expected_node_offsets = operand
            .recipe_program
            .windows(3)
            .enumerate()
            .filter(|(_, values)| *values == [-1, -1, 2])
            .map(|(index, _)| {
                u64_from_index(index)
                    .checked_mul(4)
                    .and_then(|delta| operand.recipe_program_offset.checked_add(delta))
                    .ok_or_else(|| {
                        CodecError::Malformed("F3D face recipe node offset overflows".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let expected_nodes = expected_node_offsets
            .iter()
            .copied()
            .zip(
                expected_node_offsets
                    .iter()
                    .copied()
                    .skip(1)
                    .chain(std::iter::once(operand.next_byte_offset())),
            )
            .collect::<Vec<_>>();
        let valid_program =
            match design::decode::operands::face_recipe_program_kind(&operand.recipe_program) {
                Some(design::decode::operands::FaceRecipeProgramKind::Terminal) => {
                    operand.recipe_nodes.is_empty()
                }
                Some(design::decode::operands::FaceRecipeProgramKind::Counted { .. }) => {
                    operand.recipe_nodes.len() == expected_nodes.len()
                        && operand.recipe_nodes.iter().zip(expected_nodes).try_fold(
                            true,
                            |valid, (node, (start, end))| -> Result<bool, CodecError> {
                                if !valid {
                                    return Ok(false);
                                }
                                Ok(node.byte_offset == start
                                    && node.end_byte_offset == end
                                    && node.program.get(0..3) == Some(&[-1, -1, 2])
                                    && node.recipe_structure
                                        == node
                                            .program
                                            .get(3..)
                                            .map(|program| {
                                                design::decode::operands::face_recipe_structure(
                                                    ctx.decode, program,
                                                )
                                            })
                                            .transpose()?
                                            .flatten()
                                    && u64_from_index(node.program.len())
                                        .checked_mul(4)
                                        .and_then(|delta| start.checked_add(delta))
                                        .is_some_and(|expected_offset| expected_offset == end))
                            },
                        )?
                        && operand.recipe_nodes.first().is_none_or(|first_node| {
                            first_node
                                .byte_offset
                                .checked_sub(operand.recipe_program_offset)
                                .and_then(|byte_offset| usize::try_from(byte_offset / 4).ok())
                                .is_some_and(|first_node_index| {
                                    operand
                                        .recipe_nodes
                                        .iter()
                                        .flat_map(|node| node.program.iter().copied())
                                        .eq(operand
                                            .recipe_program
                                            .iter()
                                            .copied()
                                            .skip(first_node_index))
                                })
                        })
                }
                None => false,
            };
        let expected_history = expected_face_operands.get(operand.id.as_str());
        let valid = scope.is_some_and(|scope| {
            let family = design::design_feature_family(&scope.kind());
            match (operand.group_record_index(), operand.group_member_ordinal()) {
                (Some(group_record_index), Some(group_member_ordinal)) => {
                    let group = face_groups_by_index
                        .get(&(native_stream, group_record_index))
                        .copied();
                    let exact_group_member = group.is_some_and(|group| {
                        group.scope_record_index == operand.scope_record_index
                            && usize::try_from(operand.scope_reference_ordinal)
                                .ok()
                                .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                                == Some(&group_record_index)
                            && usize::try_from(group_member_ordinal)
                                .ok()
                                .and_then(|ordinal| {
                                    group.members().get(ordinal).map(|member| &member.value)
                                })
                                == Some(&operand.record_index())
                    });
                    exact_group_member
                        && match family {
                            Some(
                                design::DesignFeatureFamily::Extrude
                                | design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken
                                | design::DesignFeatureFamily::Split,
                            ) => true,
                            Some(design::DesignFeatureFamily::ReplaceFace) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Loft) => {
                                group.is_some_and(|group| {
                                    matches!(
                                        group.role(),
                                        DesignOperandRole::PROFILE | DesignOperandRole::ROLE_0X43
                                    )
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Sweep) => {
                                group.is_some_and(|group| group.role() == DesignOperandRole::FACES)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::SurfaceOffset) => {
                                group
                                    .is_some_and(|group| group.role() == DesignOperandRole::PROFILE)
                                    && operand.recipe_kind
                                        == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(design::DesignFeatureFamily::Draft) => {
                                group.is_some_and(|group| match group.role() {
                                    DesignOperandRole::ROLE_0X10 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::BoundedFace
                                    }
                                    DesignOperandRole::ROLE_0X21 => {
                                        operand.recipe_kind
                                            == records::recipes::ConstructionRecipeKind::Face
                                    }
                                    _ => false,
                                })
                            }
                            Some(design::DesignFeatureFamily::Revolve) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X21
                                }) && revolve_axis_recipe_kind_admitted(operand.recipe_kind)
                            }
                            Some(design::DesignFeatureFamily::CircularPattern) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::BODIES_B
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::Face
                            }
                            Some(design::DesignFeatureFamily::Mirror) => {
                                group.is_some_and(|group| {
                                    mirror_operand_recipe_admitted(group.role(), operand.recipe_kind)
                                })
                            }
                            Some(design::DesignFeatureFamily::Thread) => {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                        && scope.thread_construction().is_none_or(|construction| {
                                            construction
                                                .face_group_record_indices
                                                .contains(&group.record_index)
                                        })
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            Some(
                                design::DesignFeatureFamily::Fillet
                                | design::DesignFeatureFamily::Chamfer,
                            ) => {
                                operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                                    && native.design_edge_identity_operands.iter().any(|identity| {
                                        design_stream(&identity.id) == native_stream
                                            && identity.scope_record_index
                                                == operand.scope_record_index
                                            && identity.group_record_index == group_record_index
                                            && identity.group_member_ordinal == group_member_ordinal
                                            && identity.record_index() == operand.record_index()
                                            && identity.class_tag == operand.class_tag
                                    })
                            }
                            None if scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::Emboss =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                        && scope.emboss_operand_construction().is_some_and(|construction| {
                                            group.record_index == construction.face_group.value
                                        })
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            None if scope.kind()
                                == crate::records::feature::scope::DesignFeatureKind::SplitFace =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            None if matches!(
                                scope.kind(),
                                crate::records::feature::scope::DesignFeatureKind::MoveFace
                                    | crate::records::feature::scope::DesignFeatureKind::DeplacerFace
                                    | crate::records::feature::scope::DesignFeatureKind::DeleteFace
                                    | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                            ) =>
                            {
                                group.is_some_and(|group| {
                                    group.role() == DesignOperandRole::ROLE_0X10
                                }) && operand.recipe_kind
                                    == records::recipes::ConstructionRecipeKind::BoundedFace
                            }
                            _ => false,
                        }
                }
                (None, None) => {
                    let direct_member = usize::try_from(operand.scope_reference_ordinal)
                        .ok()
                        .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                        == Some(&operand.record_index());
                    direct_member
                        && match family {
                            Some(
                                design::DesignFeatureFamily::OffsetFaces
                                | design::DesignFeatureFamily::Shell
                                | design::DesignFeatureFamily::Thicken,
                            ) => true,
                            Some(design::DesignFeatureFamily::Split) => split_direct_face_admitted(
                                operand.scope_reference_ordinal,
                                operand.recipe_kind,
                            ),
                            Some(design::DesignFeatureFamily::Hole) => matches!(
                                operand.recipe_kind,
                                records::recipes::ConstructionRecipeKind::BoundedFace
                                    | records::recipes::ConstructionRecipeKind::Face
                            ),
                            Some(design::DesignFeatureFamily::Assemble)
                                if scope.kind()
                                    == crate::records::feature::scope::DesignFeatureKind::AsBuilt
                                    && design::assembly::legacy_as_built_421_generation(
                                        scope.frame_length(),
                                        scope.class_tag.as_str(),
                                        scope.paired_class_tag.as_str(),
                                    )
                                    .is_some() =>
                            {
                                matches!(
                                    (operand.scope_reference_ordinal, operand.recipe_kind),
                                    (1, records::recipes::ConstructionRecipeKind::BoundedFace)
                                        | (3, records::recipes::ConstructionRecipeKind::Face)
                                )
                            }
                            _ => false,
                        }
                }
                _ => false,
            }
        }) && header.is_some_and(|header| {
            header.byte_offset == operand.byte_offset() && header.class_tag == operand.class_tag
        }) && operand
            .recipe_prefix_offset().checked_add(u64_from_index(operand.recipe_prefix_bytes.len())).zip(recipe.and_then(|recipe| recipe.byte_offset.checked_sub(4))).is_some_and(|(prefix_end, recipe_prefix_end)| prefix_end == recipe_prefix_end)
            && recipe_reference_frames_match(
                &operand.recipe_references,
                &expected_references,
                historical_candidates_retained,
            )
            && valid_program
            && recipe_program_operand_length(operand.recipe_kind).is_some_and(|operand_length| {
                recipe.is_some_and(|recipe| {
                    recipe.byte_offset.checked_add(operand_length).is_some_and(|expected_offset| operand.recipe_program_offset == expected_offset)
                })
            })
            && u64_from_index(operand.recipe_program.len()).checked_mul(4).and_then(|delta| operand
                    .recipe_program_offset.checked_add(delta)).is_some_and(|expected_offset| operand.next_byte_offset() == expected_offset)
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.candidate_faces, &expected_faces))
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.unreferenced_candidate_faces, &expected_unreferenced_faces))
            && (historical_candidates_retained
                || face_ids_match_refs(&operand.alternate_selector_candidate_faces, &expected_alternate_selector_faces))
            && (ctx.ir.fragment && native.asm_histories.is_empty() || expected_history.is_some_and(|expected| {
                operand.preceding_candidate_faces == expected.preceding_candidate_faces
                    && operand.changed_candidate_faces == expected.changed_candidate_faces
                    && operand.historical_support_contexts == expected.historical_support_contexts
            }))
            && recipe.is_some_and(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && recipe.kind == operand.recipe_kind
                    && recipe.byte_offset > operand.recipe_record_byte_offset()
                    && recipe.byte_offset < operand.next_byte_offset()
            });
        let valid = valid
            && face_operand_records.insert((
                native_stream,
                operand.scope_record_index,
                operand.record_index(),
            ));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design face operand has an invalid scope or recipe frame",
                Some(operand.id.clone()),
            );
        }
    }
    Ok(face_operand_records)
}

/// A `Split` direct face operand is the reference-one member, or a
/// reference-zero or reference-two tool face with a `face_recipe_data` recipe.
fn split_direct_face_admitted(
    scope_reference_ordinal: u32,
    recipe_kind: records::recipes::ConstructionRecipeKind,
) -> bool {
    scope_reference_ordinal == 1
        || (matches!(scope_reference_ordinal, 0 | 2)
            && recipe_kind == records::recipes::ConstructionRecipeKind::Face)
}

fn face_ids_match_refs(
    actual: &[cadmpeg_ir::ids::FaceId],
    expected: &[&cadmpeg_ir::ids::FaceId],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == *expected)
}

/// Report face-group members with no resolved recipe operand.
fn validate_face_group_member_resolution(
    findings: &mut Vec<Finding>,
    face_group_members: BTreeSet<(&str, u32, u32)>,
    face_operand_records: &HashSet<(&str, u32, u32)>,
    identity_group_members: &HashSet<(&str, u32, u32)>,
    entity_selection_operands: &[records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[records::feature::scope::DesignParameterScope],
) {
    let entity_selection_records = entity_selection_operands
        .iter()
        .map(|operand| {
            (
                design_stream(&operand.id),
                operand.scope_record_index,
                operand.record_index(),
            )
        })
        .collect::<std::collections::HashSet<_>>();
    let native_profiles = scopes
        .iter()
        .flat_map(|scope| {
            let stream = design_stream(&scope.id);
            scope
                .extrude_native_profiles()
                .into_iter()
                .flatten()
                .map(move |profile| (stream, scope.record_index, profile.record_index))
        })
        .collect::<HashSet<_>>();
    for member in face_group_members {
        if !native_profiles.contains(&member)
            && !face_operand_records.contains(&member)
            && !identity_group_members.contains(&member)
            && !entity_selection_records.contains(&member)
        {
            let entity = {
                format!(
                    "{}:design-face-group-member#{}:{}",
                    member.0, member.1, member.2
                )
            };
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Extrude face group has an unresolved recipe operand",
                Some(entity),
            );
        }
    }
}

/// Validate retained Face source carriers and their persistent identities.
fn validate_face_source_groups(ctx: &Ctx, findings: &mut Vec<Finding>) {
    use design::decode::operands::{
        face_source_carrier_spec, face_source_profile_pair_count, face_source_record_is_registered,
        FaceSourceType,
    };
    let native = ctx.native;
    let mut carrier_records = HashSet::new();
    for group in &native.design_face_source_groups {
        let native_stream = design_stream(&group.id);
        let scope = ctx
            .scopes_by_index
            .get(&(native_stream, group.scope_record_index));
        let carrier_header = ctx
            .records_by_index
            .get(&(native_stream, group.carrier_record_index));
        let paired_header = ctx
            .records_by_index
            .get(&(native_stream, group.paired_record_index));
        let carrier_ordinal = usize::try_from(group.carrier_reference_ordinal).ok();
        let source_spec = face_source_carrier_spec(group.source_members.len());
        let registered = |record, class: &str, role| {
            face_source_record_is_registered(&native.design_types, &group.id, record, class, role)
        };
        let scope_links_valid = scope.is_some_and(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face
                && registered(
                    scope.record_index,
                    scope.class_tag.as_str(),
                    FaceSourceType::Scope,
                )
                && face_source_profile_pair_count(&native.design_types, scope, |record| {
                    ctx.records_by_index
                        .get(&(native_stream, record))
                        .map(|header| header.class_tag.as_str())
                }) == Some(group.source_members.len())
                && carrier_ordinal
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.carrier_record_index)
                && carrier_ordinal
                    .and_then(|ordinal| ordinal.checked_add(1))
                    .and_then(|ordinal| scope.reference_members().values().nth(ordinal))
                    == Some(&group.paired_record_index)
        });
        let headers_valid = carrier_header.is_some_and(|header| {
            header.byte_offset == group.carrier_span.start()
                && header.class_tag == group.carrier_class_tag
                && registered(
                    header.record_index,
                    header.class_tag.as_str(),
                    FaceSourceType::Carrier,
                )
        }) && paired_header.is_some_and(|header| {
            header.byte_offset == group.carrier_span.end()
                && header.class_tag == group.paired_class_tag
                && registered(
                    header.record_index,
                    header.class_tag.as_str(),
                    FaceSourceType::Settings,
                )
        });
        let source_offsets_valid = source_spec.is_some_and(|layout| {
            let Ok(source_reference_offset) = u64::try_from(layout.source_reference_offset) else {
                return false;
            };
            if layout
                .scalar_offset
                .checked_add(crate::layout::sheet_metal_region_carrier_scalar_lane::LEN)
                .is_none_or(|length| {
                    u64::try_from(length).map_or(true, |length| {
                        group
                            .carrier_span
                            .start()
                            .checked_add(length)
                            .is_none_or(|end| end > group.carrier_span.end())
                    })
                })
            {
                return false;
            }
            group
                .source_members
                .iter()
                .enumerate()
                .all(|(ordinal, member)| {
                    let Ok(ordinal) = u64::try_from(ordinal) else {
                        return false;
                    };
                    group
                        .carrier_span
                        .start()
                        .checked_add(source_reference_offset)
                        .and_then(|offset| {
                            offset.checked_add(
                                ordinal.checked_mul(
                                    u64::try_from(
                                        crate::layout::sheet_metal_region_identity_reference::LEN,
                                    )
                                    .ok()?,
                                )?,
                            )
                        })
                        == Some(member.offset)
                })
        });
        let mut source_records = HashSet::new();
        let source_members_valid = if let Some(layout) = source_spec {
            if group.source_members.len() == layout.source_count {
                let mut valid = true;
                for member in group.source_members.iter().map(|member| &member.value) {
                    let unique_record = source_records.insert(member.record_index);
                    let persistent = &member.persistent_identity;
                    let local_id_offset = member.byte_offset.checked_add(21);
                    let asset_id_offset = member.byte_offset.checked_add(33);
                    let member_valid = unique_record
                        && registered(
                            member.record_index,
                            member.class_tag.as_str(),
                            FaceSourceType::Identity,
                        )
                        && member.byte_offset > group.carrier_span.start()
                        && local_id_offset == Some(persistent.local_id_offset())
                        && asset_id_offset == Some(persistent.asset_id_offset())
                        && persistent.context_id_offset() > persistent.asset_id_offset()
                        && persistent.tail_slot_offset() > persistent.context_id_offset()
                        && persistent.next_byte_offset() > member.byte_offset;
                    if !member_valid {
                        valid = false;
                        break;
                    }
                }
                valid
            } else {
                false
            }
        } else {
            false
        };
        let valid = carrier_records.insert((native_stream, group.carrier_record_index))
            && scope_links_valid
            && headers_valid
            && source_offsets_valid
            && source_members_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design Face source carrier has invalid links or offsets",
                Some(group.id.clone()),
            );
        }
    }
}

/// Validate sketch placement frames and their scope links.
fn validate_sketch_placements(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let scopes_by_index = &ctx.scopes_by_index;
    let mut placement_records = HashSet::new();
    let mut placement_scopes = HashSet::new();
    let mut visibility_offsets = HashSet::new();
    let mut visibility_ordinals = HashSet::new();
    let collections = native
        .design_types
        .iter()
        .filter(|registration| {
            registration
                .type_guid
                .as_str()
                .eq_ignore_ascii_case("5275CBA4-3D7D-40DC-A651-1DFF3FC8AFBF")
                && registration.version == 4
                && registration.module == "Geometry"
                && registration.base_type_guid.value().is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case("A7AEA631-985B-4DD1-8CE2-DE2C-14B54081")
                })
        })
        .flat_map(|registration| {
            registration
                .entities
                .values()
                .map(move |entity| (ids::design_segment(registration.id()), *entity))
        })
        .collect::<HashSet<_>>();
    for placement in &native.design_sketch_placements {
        let native_stream = design_stream(&placement.id);
        let unique_record = placement_records.insert((native_stream, placement.record_index));
        let unique_scope = match placement.scope_record_index {
            Some(index) => placement_scopes.insert((native_stream, index)),
            None => true,
        };
        let scope = placement
            .scope_record_index
            .and_then(|index| scopes_by_index.get(&(native_stream, index)));
        let visibility_valid = if let Some(visibility) = placement.visibility.as_ref() {
            let header_valid = ctx
                .entities_by_suffix
                .get(&(native_stream, placement.entity_id.suffix()))
                .is_some_and(|entity| visibility.stream_ordinal_offset() > entity.byte_offset);
            visibility
                .ordinal_collection
                .map_or(header_valid, |collection| {
                    let collection = collection.get();
                    if header_valid
                        && collections.contains(&(ids::design_segment(&placement.id), collection))
                        && visibility_ordinals.insert((
                            native_stream,
                            collection,
                            visibility.stream_ordinal.get(),
                        ))
                    {
                        visibility_offsets.insert((native_stream, visibility.visible_offset()))
                    } else {
                        false
                    }
                })
        } else {
            true
        };
        let scope_valid = if placement.member_run_head() {
            scope.is_none_or(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
            })
        } else {
            scope.is_some_and(|scope| {
                design::design_feature_family(&scope.kind())
                    == Some(design::DesignFeatureFamily::Sketch)
                    && scope
                        .sketch_entity()
                        .is_some_and(|binding| binding.entity_id == placement.entity_id)
            })
        };
        let valid = scope_valid && unique_record && unique_scope && visibility_valid;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design sketch placement has an invalid frame or scope link",
                Some(placement.id.clone()),
            );
        }
    }
}

/// Validate parameter owner frames and their indexed parameter links. The
/// companion links resolve into design-intent records, so a document without
/// them has nothing to check.
fn validate_parameter_owners(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    if !ctx.holds_design_intent() {
        return;
    }
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let parameters_by_index = &ctx.parameters_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut owner_indices = HashSet::new();
    let mut owner_local_ordinals = HashSet::new();
    for owner in &native.design_parameter_owners {
        let native_stream = design_stream(owner.id());
        let unique_index = owner_indices.insert((native_stream, owner.record_index()));
        let parameter = parameters_by_index.get(&(native_stream, owner.parameter_record_index()));
        let legacy_68_frame = owner.frame_length() == 68;
        let scope_key = (native_stream, owner.scope_record_index());
        let detached = !records_by_index.contains_key(&scope_key)
            || ctx
                .scopes_by_index
                .get(&scope_key)
                .is_some_and(|scope| !scope.names_owner(owner));
        let frame_layout = !matches!(owner.frame_length(), 68 | 88)
            || parameter.is_some_and(|parameter| {
                owner.evaluated_value_offset() == parameter.evaluated_value_offset()
            });
        let unique_local_ordinal = if legacy_68_frame || detached {
            true
        } else {
            owner_local_ordinals.insert((
                native_stream,
                owner.scope_record_index(),
                owner.local_ordinal(),
            ))
        };
        let valid = frame_layout
            && records_by_index.contains_key(&(native_stream, owner.parameter_record_index()))
            && records_by_index.contains_key(&(native_stream, owner.companion_record_index()))
            && companions_by_index
                .get(&(native_stream, owner.companion_record_index()))
                .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
            && parameter.is_some_and(|parameter| {
                parameter.owner_record_index() == Some(owner.record_index())
                    && parameter.evaluated_value().get().to_bits()
                        == owner.evaluated_value().get().to_bits()
            })
            && unique_index
            && unique_local_ordinal;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter owner has an invalid frame or indexed link",
                Some(owner.id().to_owned()),
            );
        }
    }
}

/// Validate parameter companion prefixes and owned recipe runs.
fn validate_parameter_companions(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let mut companion_indices = HashSet::new();
    let mut companion_owners = HashSet::new();
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(companion.id());
        let payload = companion.payload();
        let payload_end =
            payload.and_then(|payload| payload.byte_offset().checked_add(payload.byte_length()));
        let mut expected_recipes = native
            .construction_recipes
            .iter()
            .filter(|recipe| {
                design_stream(&recipe.id) == native_stream
                    && payload.is_some_and(|payload| {
                        payload_end.is_some_and(|end| {
                            recipe.byte_offset >= payload.byte_offset() && recipe.byte_offset < end
                        })
                    })
            })
            .collect::<Vec<_>>();
        expected_recipes.sort_by_key(|left| left.byte_offset);
        let unique_index = companion_indices.insert((native_stream, companion.record_index()));
        let unique_owner = companion_owners.insert((native_stream, companion.owner_record_index()));
        let owner = owners_by_index.get(&(native_stream, companion.owner_record_index()));
        // The timestamp follows a plain (offset 42) or typed (offset 82)
        // owner reference.
        let valid = [42, 82].into_iter().any(|relative| {
            companion
                .byte_offset()
                .checked_add(relative)
                .is_some_and(|expected_offset| {
                    companion.timestamp_micros_offset() == expected_offset
                })
        }) && payload.is_none_or(|payload| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| payload.byte_offset() == expected_offset)
        }) && (payload.is_none() || payload_end.is_some())
            && payload
                .map_or(
                    &[][..],
                    crate::records::parameters::DesignCompanionPayload::owned_recipe_ids,
                )
                .iter()
                .map(String::as_str)
                .eq(expected_recipes.iter().map(|recipe| recipe.id.as_str()))
            && records_by_index.contains_key(&(native_stream, companion.record_index()))
            && owner
                .is_some_and(|owner| owner.companion_record_index() == companion.record_index())
            && unique_index
            && unique_owner;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter companion has an invalid prefix or owner link",
                Some(companion.id().to_owned()),
            );
        }
    }
}

/// Validate dimension recipe records; returns the owned recipe ids.
fn validate_dimension_recipe_records<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, &'a str)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let mut dimension_recipe_ids = HashSet::new();
    for record in &native.design_dimension_recipe_records {
        let native_stream = design_stream(&record.id);
        let companion = companions_by_index.get(&(native_stream, record.companion_record_index));
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let recipe = native
            .construction_recipes
            .iter()
            .find(|recipe| recipe.id == record.recipe_id);
        let companion_order_matches = companion.is_some_and(|companion| {
            usize::try_from(record.recipe_ordinal)
                .ok()
                .and_then(|ordinal| {
                    companion
                        .payload()
                        .and_then(|payload| payload.owned_recipe_ids().get(ordinal))
                })
                == Some(&record.recipe_id)
        });
        let frame_end = record.byte_offset.checked_add(record.frame_length);
        let prefix_end = record
            .prefix_offset
            .checked_add(u64_from_index(record.prefix_bytes.len()));
        let program_end = u64_from_index(record.program.len())
            .checked_mul(4)
            .and_then(|length| record.program_offset.checked_add(length));
        let mut decoded_references = design::decode::dimension_frames::decode_recipe_references(
            &record.prefix_bytes,
            record.prefix_offset,
        );
        for reference in &mut decoded_references {
            design::decode::dimension_frames::bind_recipe_reference_candidates(
                reference,
                &native.persistent_subentity_tags,
                Some(&record.id),
            );
        }
        let references_match = decoded_references == record.references;
        let edge_operands_match =
            design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
                record,
                &native.design_edge_operands,
            ) == record.matching_edge_operand_ids;
        let recipe_frame_matches = recipe.is_some_and(|recipe| {
            design_stream(&recipe.id) == native_stream
                && record
                    .byte_offset
                    .checked_add(11)
                    .is_some_and(|expected_offset| recipe.byte_offset >= expected_offset)
                && frame_end.is_some_and(|end| recipe.byte_offset < end)
                && prefix_end == recipe.byte_offset.checked_sub(4)
                && recipe
                    .byte_offset
                    .checked_add(u64_from_index(design::construction_recipe_family_name_len(
                        recipe.kind,
                    )))
                    .is_some_and(|expected_offset| record.program_offset == expected_offset)
        });
        let valid = record.frame_length >= 11
            && !record.prefix_bytes.is_empty()
            && references_match
            && edge_operands_match
            && record
                .byte_offset
                .checked_add(11)
                .is_some_and(|expected_offset| record.prefix_offset == expected_offset)
            && !record.program.is_empty()
            && record
                .byte_offset
                .checked_add(11)
                .is_some_and(|expected_offset| record.program_offset >= expected_offset)
            && program_end == frame_end
            && dimension_companion
            && companion_order_matches
            && recipe_frame_matches
            && dimension_recipe_ids.insert((native_stream, record.recipe_id.as_str()));
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension recipe has an invalid indexed-record owner",
                Some(record.id.clone()),
            );
        }
    }
    dimension_recipe_ids
}

/// Report dimension companions owning an unresolved construction recipe.
fn validate_dimension_companion_recipes<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    dimension_recipe_ids: &HashSet<(&'a str, &'a str)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    for companion in &native.design_parameter_companions {
        let native_stream = design_stream(companion.id());
        let dimension_companion = owners_by_index
            .get(&(native_stream, companion.owner_record_index()))
            .and_then(|owner| {
                parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
            })
            .is_some_and(|parameter| {
                parameter.kind() == records::parameters::DesignParameterKind::Dimension
            });
        if dimension_companion
            && companion.payload().is_some_and(|payload| {
                payload.owned_recipe_ids().iter().any(|recipe_id| {
                    !dimension_recipe_ids.contains(&(native_stream, recipe_id.as_str()))
                })
            })
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension companion has an unowned construction recipe",
                Some(companion.id().to_owned()),
            );
        }
    }
}

/// Validate dimension locus pairs; returns their companion set.
fn validate_dimension_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_pair_indices = HashSet::new();
    let mut locus_pair_companions = HashSet::new();
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let unique_index = locus_pair_indices.insert((native_stream, pair.record_index));
        let unique_companion =
            locus_pair_companions.insert((native_stream, pair.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, pair.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(owner.id()) == native_stream
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= pair.byte_offset()
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset(),
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && sketch_geometry_indices.contains(&(native_stream, pair.loci()[0].geometry_index()))
            && sketch_geometry_indices.contains(&(native_stream, pair.loci()[1].geometry_index()))
            && unique_index
            && unique_companion;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus pair has an invalid frame or geometry link",
                Some(pair.id.clone()),
            );
        }
    }
    locus_pair_companions
}

/// Validate dimension annotation frames and their operand runs.
fn validate_dimension_annotation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let scopes_by_index = &ctx.scopes_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut annotation_frame_indices = HashSet::new();
    for frame in &native.design_dimension_annotation_frames {
        let native_stream = design_stream(&frame.id);
        let unique_index = annotation_frame_indices.insert((native_stream, frame.record_index));
        let governing_owner = owners_by_index
            .get(&(native_stream, frame.governing_owner_record_index))
            .copied();
        let physical_interval_valid = match frame.companion_record_index {
            Some(record_index) => companions_by_index
                .get(&(native_stream, record_index))
                .is_some_and(|companion| {
                    companion
                        .payload_byte_offset()
                        .is_some_and(|expected_offset| frame.byte_offset() >= expected_offset)
                        && companion.payload().is_some_and(|payload| {
                            companion
                                .payload_byte_offset()
                                .and_then(|offset| offset.checked_add(payload.byte_length()))
                                .is_some_and(|expected_offset| {
                                    frame.paired_byte_offset() < expected_offset
                                })
                        })
                }),
            None => governing_owner.is_some_and(|owner| {
                scopes_by_index
                    .get(&(native_stream, owner.scope_record_index()))
                    .is_some_and(|scope| frame.byte_offset() >= scope.byte_offset())
                    && parameters_by_index
                        .get(&(native_stream, owner.parameter_record_index()))
                        .is_some_and(|parameter| {
                            frame.paired_byte_offset() < parameter.byte_offset()
                        })
            }),
        };
        let governing_link_valid = governing_owner.is_some_and(|owner| {
            owner.companion_record_index() == frame.governing_companion_record_index
                && parameters_by_index
                    .get(&(native_stream, owner.parameter_record_index()))
                    .is_some_and(|parameter| {
                        parameter.kind() == records::parameters::DesignParameterKind::Dimension
                    })
        });
        let operands_valid = frame.operands().iter().all(|operand| {
            operand
                .geometry_record_index
                .is_none_or(|index| sketch_geometry_indices.contains(&(native_stream, index.get())))
        });
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(frame.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && physical_interval_valid
            && governing_link_valid
            && operands_valid
            && owner_is_sketch;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension annotation frame has invalid links or offsets",
                Some(frame.id.clone()),
            );
        }
    }
}

/// Validate direct dimension presentation frames and their sketch-owner joins.
fn validate_dimension_presentation_frames(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let sketch_scope_by_entity = native
        .design_sketch_placements
        .iter()
        .filter_map(|placement| {
            Some((
                (design_stream(&placement.id), placement.entity_id.suffix()),
                placement.scope_record_index?,
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut presentation_frame_indices = HashSet::new();
    for frame in &native.design_dimension_presentation_frames {
        let native_stream = design_stream(&frame.id);
        let unique_index = presentation_frame_indices.insert((native_stream, frame.record_index));
        let owner = owners_by_index.get(&(native_stream, frame.governing_owner_record_index));
        let parameter =
            parameters_by_index.get(&(native_stream, frame.governing_parameter_record_index));
        let companion =
            companions_by_index.get(&(native_stream, frame.governing_companion_record_index));
        let owner_link_valid = owner.is_some_and(|owner| {
            owner.parameter_record_index() == frame.governing_parameter_record_index
                && owner.companion_record_index() == frame.governing_companion_record_index
                && parameter.is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
                && companion
                    .is_some_and(|companion| companion.owner_record_index() == owner.record_index())
        });
        let nearest_owner = native
            .design_parameter_owners
            .iter()
            .filter(|candidate| {
                design_stream(candidate.id()) == native_stream
                    && sketch_scope_by_entity
                        .get(&(native_stream, u64::from(frame.owner_reference)))
                        .is_some_and(|scope_record_index| {
                            candidate.scope_record_index() == *scope_record_index
                        })
                    && candidate.byte_offset() > frame.paired_byte_offset
                    && parameters_by_index
                        .get(&(native_stream, candidate.parameter_record_index()))
                        .is_some_and(|parameter| {
                            parameter.kind() == records::parameters::DesignParameterKind::Dimension
                        })
            })
            .min_by_key(|candidate| candidate.byte_offset());
        let governing_owner_is_nearest = nearest_owner.is_some_and(|candidate| {
            candidate.record_index() == frame.governing_owner_record_index
        });
        let operand_start = frame.byte_offset.checked_add(24);
        let operands_valid = !frame.operands.is_empty()
            && frame.operands.iter().enumerate().all(|(ordinal, operand)| {
                let Some(start) = u64_from_index(ordinal)
                    .checked_mul(15)
                    .and_then(|delta| operand_start.and_then(|offset| offset.checked_add(delta)))
                else {
                    return false;
                };
                start.checked_add(1).is_some_and(|expected_offset| {
                    operand.geometry_reference_offset == expected_offset
                }) && start
                    .checked_add(11)
                    .is_some_and(|expected_offset| operand.role_offset == expected_offset)
                    && sketch_geometry_indices
                        .contains(&(native_stream, operand.geometry_record_index.get()))
            });
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(frame.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        let valid = unique_index
            && frame.paired_byte_offset > frame.byte_offset
            && frame
                .paired_byte_offset
                .checked_sub(frame.byte_offset)
                .is_some_and(|expected_offset| frame.frame_length == expected_offset)
            && (u64_from_index(frame.operands.len()))
                .checked_mul(15)
                .and_then(|delta| operand_start.and_then(|offset| offset.checked_add(delta)))
                .is_some_and(|expected_offset| frame.presentation_byte_offset == expected_offset)
            && frame
                .presentation_byte_offset
                .checked_add(u64_from_index(frame.presentation_bytes.len()))
                .is_some_and(|expected_offset| expected_offset == frame.paired_byte_offset)
            && frame
                .paired_byte_offset
                .checked_add(20)
                .is_some_and(|expected_offset| frame.owner_reference_offset == expected_offset)
            && owner_link_valid
            && governing_owner_is_nearest
            && operands_valid
            && owner_is_sketch;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension presentation frame has invalid links or offsets",
                Some(frame.id.clone()),
            );
        }
    }
}

/// Validate dimension locus groups; returns their companion set.
fn validate_dimension_locus_groups<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
) -> HashSet<(&'a str, u32)> {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let entities_by_suffix = &ctx.entities_by_suffix;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut locus_group_indices = HashSet::new();
    let mut locus_group_companions = HashSet::new();
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        let unique_index = locus_group_indices.insert((native_stream, group.record_index));
        locus_group_companions.insert((native_stream, group.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, group.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| group.byte_offset >= expected_offset)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(owner.id()) == native_stream
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= group.byte_offset
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let count = group.loci.len();
        let loci_start = group.byte_offset.checked_add(24);
        let loci_offsets_valid = group.loci.iter().enumerate().all(|(ordinal, locus)| {
            let Some(start) = u64_from_index(ordinal)
                .checked_mul(15)
                .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)))
            else {
                return false;
            };
            start
                .checked_add(1)
                .is_some_and(|expected_offset| locus.geometry_reference_offset == expected_offset)
                && start
                    .checked_add(11)
                    .is_some_and(|expected_offset| locus.role_offset == expected_offset)
                && sketch_geometry_indices.contains(&(native_stream, locus.geometry_record_index))
        });
        let owner_start = u64_from_index(count)
            .checked_mul(15)
            .and_then(|delta| loci_start.and_then(|offset| offset.checked_add(delta)));
        let returns_start = owner_start.and_then(|offset| offset.checked_add(24));
        let returns_valid = group.loci.iter().enumerate().all(|(ordinal, locus)| {
            (u64_from_index(ordinal))
                .checked_mul(11)
                .and_then(|delta| returns_start.and_then(|offset| offset.checked_add(delta)))
                .and_then(|offset| offset.checked_add(1))
                .is_some_and(|expected_offset| locus.returned.offset == expected_offset)
                && sketch_geometry_indices.contains(&(native_stream, locus.returned.value))
        });
        let mut locus_members = group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .collect::<Vec<_>>();
        let mut return_members = group
            .loci
            .iter()
            .map(|locus| locus.returned.value)
            .collect::<Vec<_>>();
        locus_members.sort_unstable_by(Ord::cmp);
        return_members.sort_unstable_by(Ord::cmp);
        let owner_is_sketch = entities_by_suffix
            .get(&(native_stream, u64::from(group.owner_reference)))
            .is_some_and(|entity| entity.in_sketch_module());
        let frame_does_not_overlap = native.design_dimension_locus_groups.iter().all(|other| {
            design_stream(&other.id) != native_stream
                || other.companion_record_index != group.companion_record_index
                || other.record_index == group.record_index
                || group.next_byte_offset <= other.byte_offset
                || other.next_byte_offset <= group.byte_offset
        });
        let valid = companion_contains_frame
            && dimension_companion
            && (1..=64).contains(&count)
            && loci_offsets_valid
            && owner_start
                .and_then(|offset| offset.checked_add(2))
                .is_some_and(|expected_offset| group.owner_reference_offset == expected_offset)
            && owner_start
                .and_then(|offset| offset.checked_add(12))
                .is_some_and(|expected_offset| group.owner_role_offset == expected_offset)
            && owner_start
                .and_then(|offset| offset.checked_add(16))
                .is_some_and(|expected_offset| group.state_offset == expected_offset)
            && owner_is_sketch
            && returns_valid
            && locus_members == return_members
            && (u64_from_index(count))
                .checked_mul(11)
                .and_then(|delta| returns_start.and_then(|offset| offset.checked_add(delta)))
                .and_then(|offset| offset.checked_add(1))
                .is_some_and(|expected_offset| group.next_byte_offset == expected_offset)
            && group
                .next_byte_offset
                .checked_sub(group.byte_offset)
                .is_some_and(|expected_offset| group.frame_length == expected_offset)
            && unique_index
            && frame_does_not_overlap;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design dimension locus group has an invalid counted frame or geometry link",
                Some(group.id.clone()),
            );
        }
    }
    locus_group_companions
}

/// Validate null-locus dimension pairs against typed companions.
fn validate_dimension_null_locus_pairs<'a>(
    ctx: &Ctx<'a, '_>,
    findings: &mut Vec<Finding>,
    locus_pair_companions: &HashSet<(&'a str, u32)>,
    locus_group_companions: &HashSet<(&'a str, u32)>,
) {
    let native = ctx.native;
    let parameters_by_index = &ctx.parameters_by_index;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let sketch_geometry_indices = &ctx.sketch_geometry_indices;
    let mut null_locus_pair_indices = HashSet::new();
    let mut null_locus_pair_companions = HashSet::new();
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let unique_index = null_locus_pair_indices.insert((native_stream, pair.record_index));
        let unique_companion =
            null_locus_pair_companions.insert((native_stream, pair.companion_record_index));
        let companion = companions_by_index.get(&(native_stream, pair.companion_record_index));
        let companion_contains_frame = companion.is_some_and(|companion| {
            companion
                .payload_byte_offset()
                .is_some_and(|expected_offset| pair.byte_offset() >= expected_offset)
                && !native.design_parameter_owners.iter().any(|owner| {
                    design_stream(owner.id()) == native_stream
                        && owner.byte_offset() > companion.byte_offset()
                        && owner.byte_offset() <= pair.byte_offset()
                })
        });
        let dimension_companion = companion.is_some_and(|companion| {
            owners_by_index
                .get(&(native_stream, companion.owner_record_index()))
                .and_then(|owner| {
                    parameters_by_index.get(&(native_stream, owner.parameter_record_index()))
                })
                .is_some_and(|parameter| {
                    parameter.kind() == records::parameters::DesignParameterKind::Dimension
                })
        });
        let governs_following_dimension =
            design::decode::dimension_frames::following_dimension_companion_record_index(
                &pair.id,
                pair.paired_byte_offset(),
                &native.design_parameter_owners,
                &native.design_parameters,
            ) == Some(pair.governing_companion_record_index);
        let companion_has_typed_frame = locus_pair_companions
            .contains(&(native_stream, pair.companion_record_index))
            || locus_group_companions.contains(&(native_stream, pair.companion_record_index));
        let valid = companion_contains_frame
            && dimension_companion
            && governs_following_dimension
            && !companion_has_typed_frame
            && sketch_geometry_indices.contains(&(native_stream, pair.loci()[1].geometry_index()))
            && unique_index
            && unique_companion;
        if !valid {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design null-locus dimension pair has an invalid frame or geometry link",
                Some(pair.id.clone()),
            );
        }
    }
}

/// Validate parameter record identity uniqueness.
fn validate_parameters(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut parameter_indices = HashSet::new();
    for parameter in &native.design_parameters {
        let native_stream = design_stream(&parameter.id);
        if !parameter_indices.insert((native_stream, parameter.record_index)) {
            let id = parameter.id.clone();
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design parameter has an invalid frame, family discriminator, or owner",
                Some(id),
            );
        }
    }
}

/// Validate design entity reference runs and suffix uniqueness.
fn validate_entity_headers(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let records_by_index = &ctx.records_by_index;
    let mut entity_suffixes = HashSet::new();
    for header in &native.design_entity_headers {
        let native_stream = design_stream(&header.id);
        let references_resolve = header
            .reference_values()
            .all(|index| records_by_index.contains_key(&(native_stream, *index)));
        if !references_resolve {
            push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion design entity has an invalid reference run",
                Some(header.entity_id.as_str().to_owned()),
            );
        }
        let key = (native_stream, header.entity_id.suffix());
        if !entity_suffixes.insert(key) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design entity suffix is duplicated within its stream",
                Some(header.entity_id.as_str().to_owned()),
            );
        }
    }
}

/// Validate sketch relation owners and byte frames.
fn validate_sketch_relations(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let owner_matches = matches!(
            (
                sketch_owner_ids.get(&(native_stream, relation.owner_reference)),
                relation.owner_entity_id.as_ref().map(cadmpeg_core::text::NonBlankString::as_str),
            ),
            (Some(expected), Some(actual)) if *expected == actual
        );
        if !owner_matches {
            push_constant_finding(
                findings,
                Check::ReferentialIntegrity,
                "Fusion sketch relation has an invalid owner or byte frame",
                Some(relation.id.clone()),
            );
        }
    }
}

/// Validate sketch point and surface persistent identities and the typed
/// record of every sketch point, curve, and surface.
fn validate_sketch_geometry_identities(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let mut sketch_point_identities = HashSet::new();
    let mut sketch_geometry_records = HashSet::new();
    // An unresolved owner is not one shared sketch. Enforce uniqueness only
    // when the owning sketch reference is known.
    for point in &native.sketch_points {
        let duplicate = if let (Some(persistent_id), Some(owner_reference)) =
            (point.persistent_id(), point.owner_reference)
        {
            !sketch_point_identities.insert((
                design_stream(&point.id),
                owner_reference,
                persistent_id,
            ))
        } else {
            false
        };
        if duplicate {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch point has an invalid persistent identity",
                Some(point.id.clone()),
            );
        }
        if !sketch_geometry_records.insert((design_stream(&point.id), point.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(point.id.clone()),
            );
        }
    }
    // Distinct curve records of one Sketch can carry one identity pair, so
    // the pair is not checked for uniqueness.
    for curve in &native.sketch_curve_identities {
        if !sketch_geometry_records.insert((design_stream(&curve.id), curve.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(curve.id.clone()),
            );
        }
    }
    let mut sketch_surface_identities = HashSet::new();
    for surface in &native.sketch_surfaces {
        let duplicate = if let Some(owner_reference) = surface.owner_reference {
            !sketch_surface_identities.insert((
                design_stream(&surface.id),
                owner_reference,
                surface.persistent_id.get(),
            ))
        } else {
            false
        };
        if duplicate {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch surface has an invalid persistent identity",
                Some(surface.id.clone()),
            );
        }
        if !sketch_geometry_records.insert((design_stream(&surface.id), surface.record_index)) {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sketch geometry aliases another typed indexed record",
                Some(surface.id.clone()),
            );
        }
    }
}

fn emit_sketch_relation_finding(findings: &mut Vec<Finding>, entity: &str, message: &'static str) {
    let entity = {
        let mut copied = String::new();
        copied.push_str(entity);
        copied
    };
    findings.push(Finding {
        related: Vec::new(),
        check: Check::NativeLinks,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity),
    });
}

fn validate_sketch_relation_owners(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let owners_by_index = &ctx.owners_by_index;
    let companions_by_index = &ctx.companions_by_index;
    let placements_by_scope = &ctx.placements_by_scope;
    let sketch_owner_ids = &ctx.sketch_owner_ids;
    let typed_sketch_records = native
        .sketch_points
        .iter()
        .map(|point| (design_stream(&point.id), point.record_index))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (design_stream(&curve.id), curve.record_index)),
        )
        .chain(
            native
                .sketch_surfaces
                .iter()
                .map(|surface| (design_stream(&surface.id), surface.record_index)),
        )
        .collect::<std::collections::HashSet<_>>();
    let sketch_operands = native
        .sketch_points
        .iter()
        .map(|point| {
            (
                (design_stream(&point.id), point.record_index),
                records::sketch_relations::SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id(),
                },
            )
        })
        .chain(native.sketch_curve_identities.iter().map(|curve| {
            (
                (design_stream(&curve.id), curve.record_index),
                records::sketch_relations::SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                },
            )
        }))
        .chain(native.sketch_surfaces.iter().map(|surface| {
            (
                (design_stream(&surface.id), surface.record_index),
                records::sketch_relations::SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id.get(),
                },
            )
        }))
        .collect::<std::collections::HashMap<_, _>>();
    let mut relation_owners = std::collections::HashMap::new();
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            native
                .sketch_surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
    {
        let Some(owner_reference) = owner_reference else {
            continue;
        };
        let native_stream = design_stream(id);
        if sketch_owner_ids.contains_key(&(native_stream, owner_reference)) {
            relation_owners.insert((native_stream, record_index), owner_reference);
        }
    }
    for relation in &native.sketch_relations {
        let native_stream = design_stream(&relation.id);
        let agrees = |reference: &records::sketch_relations::SketchRelationReference| {
            let record_index = reference.record_index();
            match sketch_operands.get(&(native_stream, record_index)) {
                Some(expected) => reference.resolved() == Some(expected),
                None => {
                    reference.resolved()
                        == Some(&records::sketch_relations::SketchRelationOperand::Record {
                            record_index,
                        })
                }
            }
        };
        if !relation
            .members()
            .iter()
            .all(|member| agrees(&member.reference))
            || !relation
                .return_members()
                .iter()
                .all(|member| agrees(&member.reference))
        {
            emit_sketch_relation_finding(
                findings,
                &relation.id,
                "Fusion sketch relation typed operands disagree with its indexed references",
            );
        }
        for member in relation.all_member_indices() {
            if !typed_sketch_records.contains(&(native_stream, member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, member), relation.owner_reference)
                .is_some_and(|owner| owner != relation.owner_reference)
            {
                emit_sketch_relation_finding(
                    findings,
                    &relation.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for entity in native
        .design_entity_headers
        .iter()
        .filter(|entity| entity.in_sketch_module())
    {
        let native_stream = design_stream(&entity.id);
        let Ok(owner) = u32::try_from(entity.entity_id.suffix()) else {
            continue;
        };
        for member in entity.member_values() {
            if !typed_sketch_records.contains(&(native_stream, *member)) {
                continue;
            }
            if relation_owners
                .insert((native_stream, *member), owner)
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    findings,
                    &entity.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for pair in &native.design_dimension_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index()))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        for member in [
            pair.loci()[0].geometry_index(),
            pair.loci()[1].geometry_index(),
        ] {
            if relation_owners
                .insert((native_stream, member), owner)
                .is_some_and(|existing| existing != owner)
            {
                emit_sketch_relation_finding(
                    findings,
                    &pair.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for group in &native.design_dimension_locus_groups {
        let native_stream = design_stream(&group.id);
        for member in group
            .loci
            .iter()
            .map(|locus| locus.geometry_record_index)
            .chain(group.loci.iter().map(|locus| locus.returned.value))
        {
            if relation_owners
                .insert((native_stream, member), group.owner_reference)
                .is_some_and(|existing| existing != group.owner_reference)
            {
                emit_sketch_relation_finding(
                    findings,
                    &group.id,
                    "Fusion sketch member belongs to multiple sketch owners",
                );
            }
        }
    }
    for pair in &native.design_dimension_null_locus_pairs {
        let native_stream = design_stream(&pair.id);
        let owner = companions_by_index
            .get(&(native_stream, pair.governing_companion_record_index))
            .and_then(|companion| {
                owners_by_index.get(&(native_stream, companion.owner_record_index()))
            })
            .and_then(|parameter_owner| {
                placements_by_scope.get(&(native_stream, parameter_owner.scope_record_index()))
            })
            .and_then(|placement| u32::try_from(placement.entity_id.suffix()).ok());
        let Some(owner) = owner else {
            continue;
        };
        if relation_owners
            .insert((native_stream, pair.loci()[1].geometry_index()), owner)
            .is_some_and(|existing| existing != owner)
        {
            emit_sketch_relation_finding(
                findings,
                &pair.id,
                "Fusion sketch member belongs to multiple sketch owners",
            );
        }
    }
    for (id, record_index, owner_reference) in native
        .sketch_points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
    {
        if relation_owners
            .get(&(design_stream(id), record_index))
            .copied()
            != owner_reference
        {
            emit_sketch_relation_finding(
                findings,
                id,
                "Fusion sketch geometry owner disagrees with its relation graph",
            );
        }
    }
}

/// Validate persistent body links and their history ordering.
fn validate_body_links(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let mut body_links: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if body_ids.contains(id) => Some(id),
            _ => None,
        };
        let Some(target_key) = target_key else {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body link has an invalid target or group payload",
                Some(link.id.clone()),
            );
            continue;
        };
        body_links.entry(target_key).or_default().push(link);
    }
    for links in body_links.values_mut() {
        links.sort_by_key(|left| left.ordinal);
        if links
            .iter()
            .enumerate()
            .any(|(ordinal, link)| u32::try_from(ordinal) != Ok(link.ordinal))
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body links have noncanonical history ordering",
                links.first().map(|link| link.id.clone()),
            );
        }
    }
}

/// Validate persistent subentity tags and their group ordering.
fn validate_subentity_tags(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let face_ids = ir
        .model
        .faces
        .iter()
        .map(|face| &face.id)
        .collect::<std::collections::HashSet<_>>();
    let edge_ids = ir
        .model
        .edges
        .iter()
        .map(|edge| &edge.id)
        .collect::<std::collections::HashSet<_>>();
    let mut subentity_tags: std::collections::BTreeMap<_, Vec<_>> =
        std::collections::BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let Some(target_key) = (match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if face_ids.contains(id) => {
                Some((1_u8, id.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edge_ids.contains(id) => {
                Some((0_u8, id.as_str()))
            }
            _ => None,
        }) else {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tag has an invalid target or group payload",
                Some(tag.id.clone()),
            );
            continue;
        };
        subentity_tags.entry(target_key).or_default().push(tag);
    }
    for tags in subentity_tags.values_mut() {
        tags.sort_by_key(|left| left.ordinal);
        if tags
            .iter()
            .enumerate()
            .any(|(ordinal, tag)| u32::try_from(ordinal) != Ok(tag.ordinal))
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tags have noncanonical group ordering",
                tags.first().map(|tag| tag.id.clone()),
            );
        }
    }
}

/// Validate each ASM history graph as a coherent state chain.
fn validate_history_graphs(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    if ctx.ir.fragment
        && native.asm_histories.is_empty()
        && native
            .design_parameter_scopes
            .iter()
            .any(|scope| scope.history_state_id().is_some())
    {
        findings.push(Finding { check: Check::FragmentSkipped, severity: Severity::Info,
            entity: None, related: Vec::new(),
            message: "skipped f3d.history_state_links and f3d.historical_candidates: cited ASM states are outside this fragment".into() });
    }
    for history in &native.asm_histories {
        if let Err(error) = history.check_transitions() {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("Fusion ASM history transition is invalid: {error:?}"),
                entity: Some(history.id.clone()),
                related: Vec::new(),
            });
        }
        let coherent = history::graph_is_coherent(history);
        if !coherent {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM history graph is not a coherent doubly linked state chain",
                Some(history.id.clone()),
            );
        }
    }
}

/// Whether a Thread form admits the scope's class pair.
fn thread_class_pair_holds(
    form: records::feature::thread::DesignThreadForm,
    class_tag: &records::references::DesignClassTag,
    paired_class_tag: &records::references::DesignClassTag,
) -> bool {
    form.class_pair_holds(class_tag.as_str(), paired_class_tag.as_str())
}

fn thread_designation_offset_holds(
    form: records::feature::thread::DesignThreadForm,
    designation_offset: u64,
    scope_offset: u64,
) -> bool {
    form.designation_offset_holds(designation_offset, scope_offset)
}

#[cfg(test)]
mod tests;

fn validate_profile_curve_selection(
    selection: &records::topology::entity_selection::DesignProfileCurveSelection,
    findings: &mut Vec<Finding>,
) {
    if !selection.valid_layout() {
        push_constant_finding(
            findings,
            Check::NativeLinks,
            "Fusion profile curve selection has an invalid region layout",
            Some(selection.id.clone()),
        );
    }
}
