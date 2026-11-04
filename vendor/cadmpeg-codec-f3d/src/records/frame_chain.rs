// SPDX-License-Identifier: Apache-2.0
//! Checked indexed-record frame bases.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecordFrameChain {
    record_index: u32,
    byte_offset: u64,
}

impl RecordFrameChain {
    pub(super) fn try_new(
        record_index: u32,
        byte_offset: u64,
        index_span: u32,
        byte_span: u64,
    ) -> Result<Self, String> {
        record_index
            .checked_add(index_span)
            .ok_or("record_index frame extent overflows")?;
        byte_offset
            .checked_add(byte_span)
            .ok_or("byte_offset frame extent overflows")?;
        Ok(Self {
            record_index,
            byte_offset,
        })
    }

    pub(super) fn index(self, delta: u32) -> u32 {
        self.record_index + delta
    }
    pub(super) fn offset(self, delta: u64) -> u64 {
        self.byte_offset + delta
    }
}

mod identity_rewrite;
