// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::tokens_test::{push_u8_string, t_dbl};

/// Build a synthetic ASM `BinaryFile8` BREP stream: a spec-shaped header
/// followed by a couple of filler records and a `delta_state` history marker.
pub(crate) fn synthetic_smbh() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8"); // 0..15 magic
    b.extend_from_slice(&23100u32.to_le_bytes()); // 15..19 save-format version
    b.extend_from_slice(&[0u8; 12]); // 19..31 zero
    b.extend_from_slice(&7u64.to_le_bytes()); // 31..39 entity-count word
    b.extend_from_slice(&3u64.to_le_bytes()); // 39..47 flags: history partition
    push_u8_string(&mut b, "Autodesk Neutron"); // 0x07 tag at offset 47
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    t_dbl(&mut b, 60.0); // scale
    t_dbl(&mut b, 1.0e-6); // resabs
    t_dbl(&mut b, 1.0e-10); // resnor

    // Some active-model filler (no delta_state here).
    b.extend_from_slice(&[0x0d, 0x04, b'b', b'o', b'd', b'y', 0x11]);
    let active_len = b.len();

    // History boundary: the preceding record's `0x11` terminator is followed
    // by the exact `0x0d 0x0b "delta_state"` record-name token.
    b.extend_from_slice(&[0x0d, 0x0b]);
    b.extend_from_slice(b"delta_state");
    b.extend_from_slice(&[0u8; 16]);

    // Sanity: the delta-state identifier starts immediately after the solved
    // record sequence.
    assert_eq!(&b[active_len..active_len + 2], &[0x0d, 0x0b]);
    assert_eq!(&b[active_len + 2..active_len + 13], b"delta_state");
    b
}

// ---- SAB record-stream fixtures ---------------------------------------------
//
// The helpers below assemble a minimal but genuine active model slice: an
// `asmheader` at RecordTable index 0 followed by a single planar face bounded by
// a closed three-coedge loop, with its edges, vertices, and points. Entity
// references are RecordTable indices; `-1` is null. This exercises the framer,
// topology graph builder, and analytic surface decode end to end.

/// The three `0x07`-tagged strings + three `0x06`-tagged doubles of a
/// `BinaryFile8` header, i.e. the bytes up to the start of the record stream.
pub(crate) fn smbh_header_prefix() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile8");
    b.extend_from_slice(&23100u32.to_le_bytes()); // save-format version
    b.extend_from_slice(&[0u8; 12]); // zero region
    b.extend_from_slice(&5u64.to_le_bytes()); // entity-count word
    b.extend_from_slice(&3u64.to_le_bytes()); // flags: history partition
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 231.6.3.65535 OSX");
    push_u8_string(&mut b, "Tue Mar 31 16:16:19 2026");
    t_dbl(&mut b, 60.0);
    t_dbl(&mut b, 1.0e-6);
    t_dbl(&mut b, 1.0e-10);
    b
}

/// Rewrite a `BinaryFile8` stream's save-format version word.
pub(crate) fn with_save_format(mut smbh: Vec<u8>, version: u32) -> Vec<u8> {
    smbh[15..19].copy_from_slice(&version.to_le_bytes());
    smbh
}

/// The `BinaryFile4` fixed header ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#1-asm-binary-header)): 15-byte magic, four little-endian
/// u32 words (save-format version, record count, entity count, flags), then the same
/// tagged string/double sequence as `BinaryFile8`.
pub(crate) fn bf4_header_prefix(flags: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile4");
    b.extend_from_slice(&22700u32.to_le_bytes()); // ACIS save-format version
    b.extend_from_slice(&0u32.to_le_bytes()); // record count (unwritten)
    b.extend_from_slice(&2u32.to_le_bytes()); // entity count
    b.extend_from_slice(&flags.to_le_bytes());
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 227.5.0.65535 NT");
    push_u8_string(&mut b, "Mon Aug  8 02:39:24 2022");
    t_dbl(&mut b, 50.0); // scale
    t_dbl(&mut b, 1.0e-6); // resabs
    t_dbl(&mut b, 1.0e-10); // resnor
    b
}
