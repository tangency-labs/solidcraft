// SPDX-License-Identifier: Apache-2.0
//! Decode MetaStream-bounded common sheet-metal settings frames.

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::layout::sheet_metal_settings_prefix as settings_prefix;
use crate::metastream::{primary_record_frames, PrimaryRecordFrame};
use crate::records::sheet_metal_settings::{
    DesignSheetMetalSettings, DesignSheetMetalSettingsInputs, DesignSheetMetalSettingsWire,
    V3RuleReferenceTail, PAIRED_GUID, PRESENTATION_GUID, RULE_GUID, SETTINGS_GUID,
};
use cadmpeg_core::container::{ContainerEntry, ContainerRole};
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Settings also occur in the flat-pattern asset's Design segment. This does
/// not admit that asset to feature or parameter decoding.
pub(super) fn is_settings_stream(
    scan: &ContainerScan,
    entry: &ContainerEntry,
    role: ContainerRole,
) -> bool {
    if scan.is_design_stream(entry, role) {
        return true;
    }
    let member = match role {
        ContainerRole::Metastream => "MetaStream.dat",
        ContainerRole::Bulkstream => "BulkStream.dat",
        _ => return false,
    };
    entry.role == role
        && entry.name.strip_prefix("FlatPatternAssetSubType/Design1/") == Some(member)
}

pub(super) fn has_supported_settings(meta: &crate::metastream::MetaStream) -> bool {
    meta.types.iter().any(|row| {
        row.type_guid.as_str().eq_ignore_ascii_case(SETTINGS_GUID)
            && matches!(row.version, 2 | 3)
            && row.entities.values().len() != 0
    })
}

/// The stream and record index of every record a settings record links: the
/// record itself, its present input slots, and its rule, presentation, owner
/// and context records. Their headers are kept with the Design record
/// headers.
pub(crate) fn linked_record_indices(settings: &[DesignSheetMetalSettings]) -> Vec<(String, u32)> {
    settings
        .iter()
        .flat_map(|settings| {
            let stream = crate::ids::native_stream(settings.id())
                .unwrap_or(crate::ids::DEFAULT_STREAM)
                .to_owned();
            settings
                .record_references()
                .filter_map(|index| u32::try_from(index).ok())
                .map(move |index| (stream.clone(), index))
        })
        .collect()
}

pub(crate) fn decode_settings(
    scan: &ContainerScan,
) -> Result<Vec<DesignSheetMetalSettings>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| is_settings_stream(scan, entry, ContainerRole::Bulkstream))
    {
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        if !has_supported_settings(&meta) {
            continue;
        }
        let mut by_entity = HashMap::new();
        for row in &meta.types {
            for &id in row.entities.values() {
                if by_entity.contains_key(&id) {
                    return Err(CodecError::Malformed(
                        "F3D settings type registration repeats an entity ID".into(),
                    ));
                }
                by_entity.insert(id, (row.type_guid.as_str(), row.version));
            }
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let frames = primary_record_frames(&meta, bytes.len())?;
        let primary =
            (frames.iter().map(|frame| frame.entity_id)).collect::<std::collections::HashSet<_>>();
        let rule_starts = frames
            .iter()
            .map(|frame| (frame.entity_id, frame.start))
            .collect::<HashMap<_, _>>();
        for frame in &frames {
            let Some((guid, version)) = by_entity.get(&frame.entity_id) else {
                continue;
            };
            if !guid.eq_ignore_ascii_case(SETTINGS_GUID) || !matches!(version, 2 | 3) {
                continue;
            }
            let Some(fields) =
                parse_settings(bytes, *frame, *version, &primary, &by_entity, &meta.types)
            else {
                return Err(CodecError::Malformed(
                    "F3D sheet-metal settings frame or typed link is invalid".into(),
                ));
            };
            let rule = rule_starts
                .get(&fields.rule)
                .and_then(|start| rule_thickness(bytes, *start));
            let id = super::text::design_record_id(
                &entry.name,
                ":design-sheet-metal-settings#",
                u64_from_index(frame.start),
            );
            let record = DesignSheetMetalSettings::try_from(DesignSheetMetalSettingsWire {
                id,
                record_index: frame.entity_id,
                byte_offset: u64_from_index(frame.start),
                paired_byte_offset: u64_from_index(frame.member_end),
                mask: fields.mask,
                inputs: fields.inputs,
                rule_record_index: fields.rule,
                presentation_record_index: fields.presentation,
                owner_record_index: fields.owner,
                context_record_index: fields.context,
                rule_thickness: rule.map(|(thickness, _)| thickness),
                rule_thickness_offset: rule.map(|(_, offset)| offset),
            })
            .map_err(CodecError::Malformed)?;
            out.push(record);
        }
    }
    Ok(out)
}

/// The thickness a sheet-metal rule record states and its offset: after the
/// indexed header and ten zero bytes, a u32 word, then the first entry, an
/// LP-UTF16 `Thickness` followed by the f64 value in centimetres.
pub(crate) fn rule_thickness(
    bytes: &[u8],
    start: usize,
) -> Option<(cadmpeg_ir::scalar::PositiveReal, u64)> {
    use crate::layout::sheet_metal_rule_prefix as rule;
    if bytes.get(start.checked_add(rule::ZERO_RUN_11)?..start.checked_add(rule::ENTRY_WORD)?)?
        != [0; 10]
    {
        return None;
    }
    let (name, name_end) =
        crate::bytes::lp_utf16_bounded(bytes, start.checked_add(rule::FIRST_ENTRY_NAME)?, 1..=64)?;
    if name != "Thickness" || Some(name_end) != start.checked_add(rule::THICKNESS) {
        return None;
    }
    let thickness = cadmpeg_ir::scalar::PositiveReal::new(View::f64_le_at(bytes, name_end)?)?;
    Some((thickness, u64_from_index(name_end)))
}

struct SettingsFields {
    mask: u32,
    inputs: DesignSheetMetalSettingsInputs,
    rule: u64,
    presentation: u64,
    owner: u64,
    context: u64,
}

fn header<'a>(view: &mut View<'a>, record_index: u64) -> Option<&'a str> {
    let (tag, length) = lp_ascii_filtered_view(view.unread(), 0, 1..=10, u8::is_ascii_digit)?;
    view.skip(length)?;
    (u64::from(view.u32_le()?) == record_index).then_some(tag)
}
fn local(view: &mut View<'_>) -> Result<Option<u64>, ()> {
    match view.u8().ok_or(())? {
        0 => Ok(None),
        1 => {
            let value = view.u64_le().ok_or(())?;
            (view.take(2).ok_or(())? == [0, 0])
                .then_some(Some(value))
                .ok_or(())
        }
        _ => Err(()),
    }
}
fn parse_settings(
    bytes: &[u8],
    frame: PrimaryRecordFrame,
    version: u32,
    primary: &HashSet<u64>,
    types: &HashMap<u64, (&str, u32)>,
    registrations: &[crate::records::entity_header::SegmentTypeData],
) -> Option<SettingsFields> {
    let mut view = View::over_retained(bytes.get(frame.start..frame.end)?);
    let primary_class = header(&mut view, frame.entity_id)?;
    let ordinal = primary_class.parse::<usize>().ok()?.checked_sub(256)?;
    let registration = registrations.get(ordinal)?;
    if !registration
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(SETTINGS_GUID)
        || registration.version != version
    {
        return None;
    }
    if view.take(settings_prefix::OVERRIDE_MASK)? != [0; settings_prefix::OVERRIDE_MASK] {
        return None;
    }
    let mask = view.u32_le()?;
    let (inputs, rule) = match version {
        2 => {
            let mut slots = [None; 7];
            for slot in &mut slots {
                *slot = local(&mut view).ok()?;
            }
            let rule = local(&mut view).ok()??;
            if view.take(4)? != [0; 4] {
                return None;
            }
            (DesignSheetMetalSettingsInputs::V2 { slots }, rule)
        }
        3 => {
            let mut slots = [None; 9];
            for slot in &mut slots {
                *slot = local(&mut view).ok()?;
            }
            let rule = local(&mut view).ok()??;
            let rule_reference_tail = match view.u8()? {
                0 if view.u8()? == 0 => V3RuleReferenceTail::ZeroPair,
                1 if view.take(2)? == [0, 0] => V3RuleReferenceTail::OneZeroPair,
                _ => return None,
            };
            (
                DesignSheetMetalSettingsInputs::V3 {
                    slots,
                    rule_reference_tail,
                },
                rule,
            )
        }
        _ => return None,
    };
    if inputs
        .slots()
        .iter()
        .flatten()
        .any(|id| !primary.contains(id))
    {
        return None;
    }
    if !primary.contains(&rule) || !types.get(&rule)?.0.eq_ignore_ascii_case(RULE_GUID) {
        return None;
    }
    let presentation = local(&mut view).ok()??;
    if !primary.contains(&presentation)
        || !types
            .get(&presentation)?
            .0
            .eq_ignore_ascii_case(PRESENTATION_GUID)
        || view.u8()? != 0
    {
        return None;
    }
    let owner = local(&mut view).ok()??;
    if !primary.contains(&owner) || frame.start.checked_add(view.position())? != frame.member_end {
        return None;
    }
    let paired_class = header(&mut view, frame.entity_id)?;
    let ordinal = paired_class.parse::<usize>().ok()?.checked_sub(256)?;
    if !registrations
        .get(ordinal)?
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(PAIRED_GUID)
    {
        return None;
    }
    if view.take(8)? != [0; 8] {
        return None;
    }
    let context = local(&mut view).ok()??;
    if !primary.contains(&context) || view.remaining() != 0 {
        return None;
    }
    Some(SettingsFields {
        mask,
        inputs,
        rule,
        presentation,
        owner,
        context,
    })
}

#[cfg(test)]
mod tests;
