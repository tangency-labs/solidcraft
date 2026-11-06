// SPDX-License-Identifier: Apache-2.0
//! Read the Design `DesignConfigurationRoot` record that names the active configuration row.

use crate::bytes::lp_utf16_bounded;
use crate::container::ContainerScan;
use crate::layout::configuration_root_prefix as root;
use crate::layout::indexed_design_record_header as header;
use crate::records::configuration::DesignConfigurationActiveRow;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

const ROOT_KEY: &[u8] = b"DesignConfigurationRoot";

/// Active rows named by root records, and the count of root records outside
/// the admitted form.
#[derive(Debug, Default)]
pub(crate) struct ConfigurationRoots {
    pub(crate) rows: Vec<DesignConfigurationActiveRow>,
    pub(crate) unparsed: usize,
}

pub(crate) fn decode_configuration_roots(
    scan: &ContainerScan,
) -> Result<ConfigurationRoots, CodecError> {
    let mut roots = ConfigurationRoots::default();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(frames) = super::meta::design_primary_frames(bytes, &meta) else {
            continue;
        };
        for frame in frames {
            let Some(record) = bytes.get(frame.start..frame.member_end) else {
                continue;
            };
            if is_root_record(record) {
                match active_row(record) {
                    Some(row) => roots.rows.push(row),
                    None => roots.unparsed += 1,
                }
            }
        }
    }
    Ok(roots)
}

fn is_root_record(record: &[u8]) -> bool {
    View::u32_le_at(record, root::KEY_LENGTH) == Some(root::KEY_LENGTH_VALUE)
        && record.get(root::KEY..root::ZERO_RUN_11) == Some(ROOT_KEY)
}

fn active_row(record: &[u8]) -> Option<DesignConfigurationActiveRow> {
    if View::u32_le_at(record, root::ZERO_U32) != Some(root::ZERO_U32_VALUE)
        || record.get(root::ZERO_RUN_11..root::CONSTANT_ONE)? != [0; 11]
        || View::u32_le_at(record, root::CONSTANT_ONE) != Some(root::CONSTANT_ONE_VALUE)
    {
        return None;
    }
    let (table_id, table_id_end) = lp_utf16_bounded(record, root::LEN, 1..=256)?;
    let (row_id, row_id_end) = lp_utf16_bounded(record, table_id_end, 1..=256)?;
    if View::u32_le_at(record, row_id_end) != Some(0) || record.len() != row_id_end + 4 {
        return None;
    }
    Some(DesignConfigurationActiveRow {
        record_index: View::u32_le_at(record, header::RECORD_INDEX)?,
        table_id,
        row_id,
    })
}

#[cfg(test)]
mod tests;
