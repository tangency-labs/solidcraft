// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Compact Parasolid offset-surface carriers.

use std::collections::HashMap;

use super::view::View;

use super::layout::offset_surface_00_3c as offset_surf;

const TAG: [u8; 2] = [0x00, 0x3c];
const COMMON_REFERENCE_COUNT: usize = 5;

/// One exact offset-surface construction, keyed by its stream-local attribute.
#[derive(Debug, Clone, Copy)]
pub struct OffsetCarrier {
    /// Attribute of the support-surface carrier.
    pub support: u16,
    /// Signed offset distance in source length units.
    pub distance: f64,
    /// Byte offset of the `00 3c` tag.
    pub offset: usize,
}

fn parse_payload(body: &[u8], tail: usize, tripled_support: bool, offset: usize) -> Option<OffsetCarrier> {
    let discriminator = *body.get(tail)?;
    matches!(discriminator, b'V' | b'I' | b'U').then_some(())?;
    match body.get(tail + (offset_surf::TRUE_OFFSET - offset_surf::DISCRIMINATOR))? {
        0 | 1 => {}
        _ => return None,
    }
    let support_at = tail.checked_add(offset_surf::SUPPORT - offset_surf::DISCRIMINATOR)?;
    let support = View::u16_be_at(body, support_at)?;
    (support > 1).then_some(())?;
    if tripled_support && body.get(support_at + 2) != Some(&1) {
        return None;
    }
    let distance_at = if tripled_support { support_at + 3 } else { tail + (offset_surf::DISTANCE - offset_surf::DISCRIMINATOR) };
    let distance = View::f64_be_at(body, distance_at)?;
    distance.is_finite().then_some(OffsetCarrier { support, distance, offset })
}

fn parse_at(body: &[u8], offset: usize) -> Option<(u16, OffsetCarrier)> {
    (body.get(offset..offset + TAG.len())? == TAG).then_some(())?;
    let mut header = offset.checked_add(TAG.len())?;
    if body.get(header) == Some(&0xff) {
        header += 1;
    }
    let attr = View::u16_be_at(body, header + offset_surf::ATTR)?;
    (attr > 1).then_some(())?;

    let references = header.checked_add(offset_surf::REFS)?;
    let partition_marker = header.checked_add(offset_surf::MARKER)?;
    let partition =
        matches!(body.get(partition_marker), Some(0x2b | 0x2d)).then(|| parse_payload(body, partition_marker + 1, false, offset)).flatten();

    let tripled_marker = references.checked_add(COMMON_REFERENCE_COUNT * 3)?;
    let tripled_references = (0..COMMON_REFERENCE_COUNT).all(|index| body.get(references + index * 3 + 2) == Some(&1));
    let tripled = (tripled_references && matches!(body.get(tripled_marker), Some(0x2b | 0x2d)))
        .then(|| parse_payload(body, tripled_marker + 1, true, offset))
        .flatten();

    match (partition, tripled) {
        (Some(carrier), None) | (None, Some(carrier)) => Some((attr, carrier)),
        (None, None) | (Some(_), Some(_)) => None,
    }
}

/// Scan all structurally valid type-60 offset-surface records.
pub fn scan(body: &[u8]) -> HashMap<u16, OffsetCarrier> {
    let mut out = HashMap::new();
    for offset in 0..body.len().saturating_sub(1) {
        if let Some((attr, carrier)) = parse_at(body, offset) {
            out.insert(attr, carrier);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partition(discriminator: u8, flag: u8, support: u16, distance: f64) -> Vec<u8> {
        let mut bytes = TAG.to_vec();
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&907u32.to_be_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(b'+');
        bytes.push(discriminator);
        bytes.push(flag);
        bytes.extend_from_slice(&support.to_be_bytes());
        bytes.extend_from_slice(&distance.to_be_bytes());
        bytes
    }

    fn deltas(distance: f64) -> Vec<u8> {
        let mut bytes = TAG.to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&907u32.to_be_bytes());
        for reference in [1u16, 1, 1, 1, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(b'-');
        bytes.push(b'I');
        bytes.push(0);
        bytes.extend_from_slice(&6u16.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&distance.to_be_bytes());
        bytes
    }

    #[test]
    fn parses_partition_and_deltas_framing() {
        let partition = scan(&partition(b'V', 1, 6, -0.0025));
        let carrier = partition.get(&12).expect("partition offset surface");
        assert_eq!(carrier.support, 6);
        assert!((carrier.distance + 0.0025).abs() < 1.0e-12);

        let deltas = scan(&deltas(0.0045));
        let carrier = deltas.get(&12).expect("deltas offset surface");
        assert_eq!(carrier.support, 6);
        assert!((carrier.distance - 0.0045).abs() < 1.0e-12);
    }

    #[test]
    fn rejects_invalid_fields_and_nonfinite_converted_distance() {
        assert!(scan(&partition(b'X', 1, 6, 1.0)).is_empty());
        assert!(scan(&partition(b'V', 2, 6, 1.0)).is_empty());
        assert!(scan(&partition(b'V', 1, 1, 1.0)).is_empty());
        assert!(scan(&partition(b'V', 1, 6, f64::INFINITY)).is_empty());
        assert_eq!(scan(&partition(b'V', 1, 6, f64::MAX))[&12].distance, f64::MAX);

        let mut malformed_deltas = deltas(1.0);
        let support_terminator = malformed_deltas.len() - 9;
        malformed_deltas[support_terminator] = 0;
        assert!(scan(&malformed_deltas).is_empty());
    }
}
