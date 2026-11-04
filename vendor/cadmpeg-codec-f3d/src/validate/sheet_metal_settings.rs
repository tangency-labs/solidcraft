// SPDX-License-Identifier: Apache-2.0
//! Validate the common settings carrier's indexed and typed links.

use super::{design_stream, Ctx};
use crate::layout::sheet_metal_settings_prefix as settings_prefix;
use crate::records::sheet_metal_settings::{PRESENTATION_GUID, RULE_GUID, SETTINGS_GUID};
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn validate(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    if ctx.native.design_sheet_metal_settings.is_empty() {
        return;
    }
    let mut types = std::collections::HashMap::new();
    for row in &ctx.native.design_types {
        let stream = design_stream(row.id()).strip_suffix("MetaStream.dat");
        for &id in row.entities.values() {
            let key = (stream, id);
            if let Some(prior) = types.get_mut(&key) {
                *prior = None;
            } else {
                types.insert(key, Some(row));
            }
        }
    }
    let mut indices = std::collections::HashSet::new();
    for settings in &ctx.native.design_sheet_metal_settings {
        let stream = design_stream(settings.id());
        let type_stream = stream.strip_suffix("BulkStream.dat");
        let typed = |index, guid: &str, version: Option<u32>| {
            types
                .get(&(type_stream, index))
                .and_then(|row| *row)
                .is_some_and(|row| {
                    row.type_guid.as_str().eq_ignore_ascii_case(guid)
                        && version.is_none_or(|expected| expected == row.version)
                })
        };
        let unique = indices.insert((stream, settings.record_index));
        let header = u32::try_from(settings.record_index)
            .ok()
            .and_then(|index| ctx.records_by_index.get(&(stream, index)));
        let primary_type = types
            .get(&(type_stream, settings.record_index))
            .and_then(|row| *row);
        let class_matches = header
            .zip(primary_type)
            .is_some_and(|(header, primary_type)| {
                let ordinal = ctx
                    .native
                    .design_types
                    .iter()
                    .filter(|row| {
                        design_stream(row.id()).strip_suffix("MetaStream.dat") == type_stream
                            && row.byte_offset < primary_type.byte_offset
                    })
                    .count();
                header.class_tag.as_str().parse::<usize>().ok() == ordinal.checked_add(256)
            });
        let expected_pair = header.and_then(|header| {
            let header_width =
                cadmpeg_core::decode::u64_from_index(header.class_tag.as_str().len()) + 8;
            let optional_width: u64 = settings
                .inputs
                .slots()
                .iter()
                .map(|slot| if slot.is_some() { 11 } else { 1 })
                .sum();
            let version_flags = settings.inputs.rule_reference_tail_width();
            let delta = header_width
                + cadmpeg_core::decode::u64_from_index(settings_prefix::LEN)
                + optional_width
                + 33
                + version_flags
                + 1;
            header.byte_offset.checked_add(delta)
        });
        let references_resolve = settings.record_references().all(|index| {
            u32::try_from(index)
                .ok()
                .is_some_and(|index| ctx.records_by_index.contains_key(&(stream, index)))
        });
        let valid = unique
            && class_matches
            && expected_pair == Some(settings.paired_byte_offset)
            && header.is_some_and(|header| header.byte_offset == settings.byte_offset)
            && typed(
                settings.record_index,
                SETTINGS_GUID,
                Some(settings.inputs.version()),
            )
            && typed(settings.rule_record_index, RULE_GUID, None)
            && typed(settings.presentation_record_index, PRESENTATION_GUID, None)
            && references_resolve;
        if !valid {
            super::push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion sheet-metal settings has an invalid indexed or typed link",
                Some((settings.id()).to_owned()),
            );
        }
    }
}

#[cfg(test)]
mod tests;
