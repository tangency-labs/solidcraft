// SPDX-License-Identifier: Apache-2.0
//! Record-timestamp evidence for scope references that name deleted members.

use crate::layout::record_timestamp_entry as entry;
use crate::layout::record_timestamp_table_prefix as table;
use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use serde::{Deserialize, Serialize};

/// One record-timestamp table entry: a record index at the offset of its
/// encoding, and the entry's timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignRecordTimestamp {
    /// Record index named by the entry, at the offset of its encoding.
    pub(crate) record_index: Located<u32>,
    /// Unix-epoch timestamp in microseconds.
    pub(crate) timestamp: u64,
}

/// The complete record-timestamp table of a scope and the scope references
/// that name deleted members.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignDeletedMembers {
    /// Record index of the record-timestamp table.
    pub(crate) table_record_index: u32,
    /// Byte offset of the table's indexed header.
    pub(crate) table_byte_offset: u64,
    /// Per-file dynamic class tag of the table.
    pub(crate) table_class_tag: DesignClassTag,
    /// The stored table, from its indexed header through its owner entry.
    pub(crate) table_bytes: cadmpeg_ir::native::bytes::NativeBytes,
    /// The counted table entries, in order.
    pub(crate) entries: Vec<DesignRecordTimestamp>,
    /// The owner entry after the counted entries.
    pub(crate) owner: DesignRecordTimestamp,
    /// Deleted member record indices; each is the record index of an entry.
    pub(crate) members: Vec<u32>,
}

impl DesignDeletedMembers {
    /// Offset of the record index of table entry `ordinal`; the owner entry
    /// follows the counted entries.
    pub(crate) fn entry_index_offset(&self, ordinal: usize) -> Option<u64> {
        let stride = u64::try_from(entry::LEN).ok()?;
        self.table_byte_offset
            .checked_add(u64::try_from(table::LEN).ok()?)?
            .checked_add(u64::try_from(ordinal).ok()?.checked_mul(stride)?)?
            .checked_add(u64::try_from(entry::RECORD_INDEX).ok()?)
    }

    /// Whether every entry and the owner sit on the table's entry grid, the
    /// owner names `scope_record_index`, and the members are distinct table
    /// entries.
    pub(crate) fn frame_holds(&self, scope_record_index: u32) -> bool {
        let grid_holds = self.entries.iter().enumerate().all(|(ordinal, entry)| {
            self.entry_index_offset(ordinal) == Some(entry.record_index.offset)
        });
        let owner_holds = self.owner.record_index.value == scope_record_index
            && self.entry_index_offset(self.entries.len()) == Some(self.owner.record_index.offset);
        let entry_indices = self
            .entries
            .iter()
            .map(|entry| entry.record_index.value)
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        let members_hold = !self.members.is_empty()
            && self
                .members
                .iter()
                .all(|member| entry_indices.contains(member) && seen.insert(*member));
        grid_holds && owner_holds && members_hold
    }
}

rewrite_native_record!(DesignRecordTimestamp, []; {record_index, timestamp});
rewrite_native_record!(DesignDeletedMembers, []; {table_record_index, table_byte_offset, table_class_tag, table_bytes, entries, owner, members});
