// SPDX-License-Identifier: Apache-2.0
//! Deterministic member payload estimates without a serialized output buffer.

use std::io::{self, Write};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Decoded;

#[derive(Default)]
pub(super) struct ByteCounter(u64);

impl ByteCounter {
    pub(super) fn add(&mut self, bytes: usize) -> Result<(), CodecError> {
        self.add_u64(u64_from_index(bytes))
    }

    pub(super) fn add_u64(&mut self, bytes: u64) -> Result<(), CodecError> {
        self.0 = self
            .0
            .checked_add(bytes)
            .ok_or_else(|| CodecError::malformed("F3Z retained-byte estimate exceeds u64"))?;
        Ok(())
    }

    pub(super) fn total(&self) -> u64 {
        self.0
    }
}

impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(u64_from_index(bytes.len()))
            .ok_or_else(|| io::Error::other("F3Z retained-byte estimate exceeds u64"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn count_decoded(bytes: &mut ByteCounter, decoded: &Decoded) -> Result<(), CodecError> {
    let body = &decoded.body;
    serde_json::to_writer(
        bytes,
        &(
            &decoded.ir,
            (
                &body.transfer,
                &*body.coverage,
                &body.losses,
                &body.notes,
                &body.transfer_ledger,
            ),
            &decoded.source_fidelity,
        ),
    )
    .map_err(CodecError::malformed)
}
