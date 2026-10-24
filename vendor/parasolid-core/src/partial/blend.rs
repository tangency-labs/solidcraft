// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Constant-radius rolling-ball surface carriers.
//!
//! A `00 38` record names two support surfaces and a spine curve. Equal offset
//! magnitudes define a circular rolling-ball envelope; their signs and two unit
//! scalars select the support-normal sides.

use std::collections::HashMap;

use super::view::View;

use super::layout::rolling_ball_blend_00_38 as blend_rec;

/// One exact constant-radius blend construction.
#[derive(Debug, Clone)]
pub struct BlendCarrier {
    /// Stream-local surface carrier attribute.
    pub attr: u16,
    /// Tag-byte offset in the stream.
    pub offset: usize,
    /// Ordered support references.
    pub supports: [BlendSupportRef; 2],
    /// Stored center/spine curve attribute.
    pub spine: u16,
    /// Signed rolling-ball radius in source length units.
    pub signed_radius: f64,
    /// Whether each support uses the opposite natural-normal side.
    pub reversed: [bool; 2],
}

/// Reference used by one rolling-ball support side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendSupportRef {
    /// Direct surface-carrier attribute.
    Surface(u16),
    /// Zero-offset support-pair attribute; topology selects one member.
    Pair(u16),
}

/// Two candidate surface carriers associated with one intersection curve.
#[derive(Debug, Clone)]
pub struct SupportPairCarrier {
    /// Ordered candidate surface-carrier attributes.
    pub supports: [u16; 2],
    /// Intersection-curve attribute.
    pub intersection: u16,
}

struct RawCarrier {
    attr: u16,
    selector: u8,
    references: [u16; 3],
    values: [f64; 4],
}

fn parse_raw(bytes: &[u8], offset: usize) -> Option<RawCarrier> {
    if bytes.get(offset..offset + 2) != Some(&[0x00, 0x38]) {
        return None;
    }
    let mut body = offset + 2;
    if bytes.get(body) == Some(&0xff) {
        body += 1;
    }
    let attr = View::u16_be_at(bytes, body + blend_rec::ATTR)?;
    if attr == 0 || !matches!(bytes.get(body + blend_rec::MARKER), Some(0x2b | 0x2d)) {
        return None;
    }
    let selector = *bytes.get(body + blend_rec::SELECTOR)?;
    if !matches!(selector, 0x45 | 0x52) {
        return None;
    }
    let references = [
        View::u16_be_at(bytes, body + blend_rec::SUPPORT0)?,
        View::u16_be_at(bytes, body + blend_rec::SUPPORT1)?,
        View::u16_be_at(bytes, body + blend_rec::SPINE)?,
    ];
    if references.contains(&0) {
        return None;
    }
    let values = [
        View::f64_be_at(bytes, body + blend_rec::OFFSET0)?,
        View::f64_be_at(bytes, body + blend_rec::OFFSET1)?,
        View::f64_be_at(bytes, body + blend_rec::SIDE0)?,
        View::f64_be_at(bytes, body + blend_rec::SIDE1)?,
    ];
    if values.iter().any(|value| !value.is_finite()) || (values[2].abs() - 1.0).abs() > 1.0e-12 || (values[3].abs() - 1.0).abs() > 1.0e-12 {
        return None;
    }
    Some(RawCarrier { attr, selector, references, values })
}

fn parse_blend(bytes: &[u8], offset: usize) -> Option<BlendCarrier> {
    let raw = parse_raw(bytes, offset)?;
    let [first_radius, second_radius, first_side, second_side] = raw.values;
    if first_radius.abs() <= f64::EPSILON || (first_radius.abs() - second_radius.abs()).abs() > 1.0e-12 {
        return None;
    }
    let supports = match raw.selector {
        0x45 => [BlendSupportRef::Pair(raw.references[0]), BlendSupportRef::Surface(raw.references[1])],
        0x52 => [BlendSupportRef::Surface(raw.references[0]), BlendSupportRef::Surface(raw.references[1])],
        _ => return None,
    };
    Some(BlendCarrier {
        attr: raw.attr,
        offset,
        supports,
        spine: raw.references[2],
        signed_radius: first_radius,
        reversed: [
            first_side.is_sign_negative(),
            second_side.is_sign_negative() ^ (first_radius.is_sign_negative() != second_radius.is_sign_negative()),
        ],
    })
}

/// Scan rolling-ball carriers and their zero-offset support-pair records.
pub fn scan(bytes: &[u8]) -> (HashMap<u16, BlendCarrier>, HashMap<u16, SupportPairCarrier>) {
    let mut blends = HashMap::new();
    let mut pairs = HashMap::new();
    for offset in 0..bytes.len().saturating_sub(57) {
        if let Some(carrier) = parse_blend(bytes, offset) {
            blends.entry(carrier.attr).or_insert(carrier);
        }
        if let Some(raw) = parse_raw(bytes, offset)
            && raw.selector == 0x52
            && raw.values[0].abs() <= f64::EPSILON
            && raw.values[1].abs() <= f64::EPSILON
        {
            pairs.entry(raw.attr).or_insert(SupportPairCarrier { supports: [raw.references[0], raw.references[1]], intersection: raw.references[2] });
        }
    }
    (blends, pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_constant_radius_support_and_spine_payload() {
        for selector in [0x45, 0x52] {
            let mut bytes = vec![0x00, 0x38, 0xff];
            bytes.extend_from_slice(&9u16.to_be_bytes());
            bytes.extend_from_slice(&17u32.to_be_bytes());
            for reference in [1u16, 2, 3, 4, 1] {
                bytes.extend_from_slice(&reference.to_be_bytes());
            }
            bytes.push(0x2b);
            bytes.push(selector);
            for reference in [11u16, 12, 13] {
                bytes.extend_from_slice(&reference.to_be_bytes());
            }
            for value in [-0.0005f64, -0.0005, 1.0, -1.0] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }

            let carrier = scan(&bytes).0.remove(&9).expect("blend carrier");
            assert_eq!(carrier.offset, 0);
            assert_eq!(
                carrier.supports,
                if selector == 0x45 {
                    [BlendSupportRef::Pair(11), BlendSupportRef::Surface(12)]
                } else {
                    [BlendSupportRef::Surface(11), BlendSupportRef::Surface(12)]
                }
            );
            assert_eq!(carrier.spine, 13);
            assert_eq!(carrier.signed_radius, -0.0005);
            assert_eq!(carrier.reversed, [false, true]);
        }
    }

    #[test]
    fn unequal_radius_magnitudes_are_not_a_constant_radius_blend() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [-0.0005f64, -0.0006, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert!(scan(&bytes).0.is_empty());
    }

    #[test]
    fn opposite_offset_sign_reverses_the_second_support() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [0.0005f64, -0.0005, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(scan(&bytes).0.get(&9).expect("blend").reversed, [false, true]);
    }

    #[test]
    fn zero_offsets_define_a_support_pair() {
        let mut bytes = vec![0x00, 0x38];
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&17u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(0x2b);
        bytes.push(0x52);
        for reference in [11u16, 12, 13] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        for value in [0.0f64, 0.0, 1.0, 1.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }

        let (_, pairs) = scan(&bytes);
        let pair = pairs.get(&9).expect("support pair");
        assert_eq!(pair.supports, [11, 12]);
        assert_eq!(pair.intersection, 13);
    }
}
