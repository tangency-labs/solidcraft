// SPDX-License-Identifier: Apache-2.0
//! ASM record-token writers for synthetic SMBH fixtures.
#![allow(clippy::unwrap_used)]

pub(crate) fn push_u8_string(b: &mut Vec<u8>, s: &str) {
    let length = u8::try_from(s.len()).expect("test string fits its one-byte length");
    b.push(0x07);
    b.push(length);
    b.extend_from_slice(s.as_bytes());
}

pub(crate) fn t_ref(b: &mut Vec<u8>, v: i64) {
    b.push(0x0c);
    b.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn t_long(b: &mut Vec<u8>, v: i64) {
    b.push(0x04);
    b.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn t_dbl(b: &mut Vec<u8>, v: f64) {
    b.push(0x06);
    b.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn t_pos(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x13);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}

pub(crate) fn t_vec(b: &mut Vec<u8>, p: [f64; 3]) {
    b.push(0x14);
    for c in p {
        b.extend_from_slice(&c.to_le_bytes());
    }
}

pub(crate) fn t_ident(b: &mut Vec<u8>, s: &str) {
    let length = u8::try_from(s.len()).expect("test identifier fits its one-byte length");
    b.push(0x0d);
    b.push(length);
    b.extend_from_slice(s.as_bytes());
}

pub(crate) fn t_u16_string(b: &mut Vec<u8>, value: &str) {
    b.push(0x08);
    b.extend_from_slice(&u16::try_from(value.len()).unwrap().to_le_bytes());
    b.extend_from_slice(value.as_bytes());
}

pub(crate) fn renamed_generated_subtype(mut bytes: Vec<u8>, old: &str, new: &str) -> Vec<u8> {
    let old = old.as_bytes();
    let position = bytes
        .windows(old.len())
        .position(|window| window == old)
        .expect("generated subtype name");
    assert!(matches!(
        bytes.get(position.wrapping_sub(2)),
        Some(0x0d | 0x0e)
    ));
    bytes[position - 1] = u8::try_from(new.len()).expect("short subtype name");
    bytes.splice(position..position + old.len(), new.bytes());
    bytes
}

pub(crate) fn t_subident(b: &mut Vec<u8>, s: &str) {
    let length = u8::try_from(s.len()).expect("test subidentifier fits its one-byte length");
    b.push(0x0e);
    b.push(length);
    b.extend_from_slice(s.as_bytes());
}

pub(crate) fn t_end(b: &mut Vec<u8>) {
    b.push(0x11);
}

pub(super) fn t_attribute_base(b: &mut Vec<u8>, next: i64, previous: i64, owner: i64) {
    t_ref(b, -1);
    t_long(b, -1);
    t_ref(b, next);
    t_ref(b, previous);
    t_ref(b, owner);
}

/// Push a `0x15` enum token carrying the signed `int_width`-8 value.
pub(super) fn push_native_enum(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x15);
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn t_str(b: &mut Vec<u8>, s: &str) {
    b.push(0x07);
    b.push(u8::try_from(s.len()).expect("short string"));
    b.extend_from_slice(s.as_bytes());
}

/// Push a `tag`-prefixed little-endian i64 (used for `0x04` longs and `0x15`
/// enum values in B-spline block fixtures).
pub(crate) fn push_tagged_i64(b: &mut Vec<u8>, tag: u8, v: i64) {
    b.push(tag);
    b.extend_from_slice(&v.to_le_bytes());
}

#[test]
fn one_byte_token_builders_refuse_oversize_strings_before_writing() {
    for write in [push_u8_string, t_ident, t_subident] {
        let mut bytes = vec![0xaa];
        write(&mut bytes, &"a".repeat(255));
        assert_eq!(bytes[2], 255);

        let before = bytes.clone();
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            write(&mut bytes, &"a".repeat(256));
        }));
        assert!(refused.is_err());
        assert_eq!(bytes, before);
    }
}
