// SPDX-License-Identifier: Apache-2.0
//! Validate the record links of decoded `PathPattern` settings.

use super::{design_stream, Ctx};
use crate::layout::{path_pattern_settings as settings_layout, path_pattern_start as start_layout};
use crate::records::feature::patterns::{PATH_PATTERN_SETTINGS_TYPE, PATH_PATTERN_START_TYPE};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::report::check::{Check, Finding};

pub(super) fn validate(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    for scope in &ctx.native.design_parameter_scopes {
        let Some(settings) = scope.path_pattern_settings() else {
            continue;
        };
        let stream = design_stream(&scope.id);
        let type_stream = stream.strip_suffix("BulkStream.dat");
        let mut rows = ctx
            .native
            .design_types
            .iter()
            .filter(|row| design_stream(row.id()).strip_suffix("MetaStream.dat") == type_stream)
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.byte_offset);
        let typed_header = |record_index: u32, (guid, version): (&str, u32)| {
            let mut headers = ctx.native.design_record_headers.iter().filter(|header| {
                design_stream(&header.id) == stream
                    && header.record_index == record_index
                    && header
                        .class_tag
                        .as_str()
                        .parse::<usize>()
                        .ok()
                        .and_then(|tag| tag.checked_sub(256))
                        .and_then(|ordinal| rows.get(ordinal))
                        .is_some_and(|row| row.type_guid.as_str().eq_ignore_ascii_case(guid))
            });
            let header = headers.next()?;
            let row = rows.get(
                header
                    .class_tag
                    .as_str()
                    .parse::<usize>()
                    .ok()?
                    .checked_sub(256)?,
            )?;
            (headers.next().is_none() && row.version == version).then_some(header.byte_offset)
        };
        let at = |base: u64, offset: usize| base.checked_add(u64_from_index(offset));
        let settings_valid =
            typed_header(settings.settings_record_index, PATH_PATTERN_SETTINGS_TYPE)
                .filter(|base| *base == settings.settings_byte_offset)
                .is_some_and(|base| {
                    Some(settings.flipped.offset) == at(base, settings_layout::FLIPPED)
                        && Some(settings.path_oriented.offset)
                            == at(base, settings_layout::PATH_ORIENTED)
                        && Some(settings.total_extent.offset)
                            == at(base, settings_layout::DISTANCE_TYPE)
                });
        let start_valid = typed_header(settings.start_record_index, PATH_PATTERN_START_TYPE)
            .is_some_and(|base| {
                Some(settings.start_parameter.offset) == at(base, start_layout::START_PARAMETER)
            });
        let members = scope.reference_members();
        let valid = scope.kind() == crate::records::feature::scope::DesignFeatureKind::PathPattern
            && settings_valid
            && start_valid
            && members
                .values()
                .any(|member| *member == settings.start_record_index)
            && members
                .values()
                .any(|member| *member == settings.path_group_record_index);
        if !valid {
            super::push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion Design path-pattern settings have an invalid record link",
                Some(scope.id.clone()),
            );
        }
    }
}

#[cfg(test)]
mod tests;
