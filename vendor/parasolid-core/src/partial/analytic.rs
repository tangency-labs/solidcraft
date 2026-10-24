// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Partial compact analytic records in source units. See PARTIAL_READERS.md.
use super::layout::compact_analytic_header as analytic;
use super::{ReadSpan, view::View};
use crate::BinaryReader;

/// Accepted native tag and finite parameter values; no model-space scaling.
#[derive(Debug, Clone)]
pub struct AnalyticCarrier {
    pub tag: u8,
    pub attr: u16,
    pub offset: usize,
    pub end: usize,
    /// Source orientation byte, `+` or `-`, relative to the parameter frame.
    pub orientation: u8,
    pub values: Vec<f64>,
    pub read_spans: Vec<ReadSpan>,
}
fn norm3(v: &[f64]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
pub mod tag {
    pub const LINE: u8 = 0x1e;
    pub const CIRCLE: u8 = 0x1f;
    pub const ELLIPSE: u8 = 0x20;
    pub const PLANE: u8 = 0x32;
    pub const CYLINDER: u8 = 0x33;
    pub const CONE: u8 = 0x34;
    pub const SPHERE: u8 = 0x35;
    pub const TORUS: u8 = 0x36;
}

const COMPACT_REF_COUNT: usize = 5;
const DELTAS_REF_STRIDE: usize = 3;
const DELTAS_MARKER_OFFSET: usize = analytic::REFS + COMPACT_REF_COUNT * DELTAS_REF_STRIDE;

/// f64 count for each analytic tag; `None` if the tag is not an analytic carrier.
fn analytic_value_count(tt: u8) -> Option<usize> {
    Some(match tt {
        tag::LINE => 6,
        tag::CIRCLE => 10,
        tag::ELLIPSE => 11,
        tag::PLANE => 9,
        tag::CYLINDER => 10,
        tag::CONE => 12,
        tag::SPHERE => 10,
        tag::TORUS => 11,
        _ => return None,
    })
}

fn unit_length(values: &[f64]) -> bool {
    (norm3(values) - 1.0).abs() <= 1.0e-9
}

fn orthonormal(left: &[f64], right: &[f64]) -> bool {
    unit_length(left) && unit_length(right) && (left[0] * right[0] + left[1] * right[1] + left[2] * right[2]).abs() <= 1.0e-9
}

fn valid_carrier_frame(tt: u8, values: &[f64]) -> bool {
    match tt {
        tag::LINE => unit_length(&values[3..6]),
        tag::CIRCLE | tag::ELLIPSE | tag::PLANE => orthonormal(&values[3..6], &values[6..9]),
        tag::CYLINDER => orthonormal(&values[3..6], &values[7..10]),
        tag::CONE => orthonormal(&values[3..6], &values[9..12]),
        tag::SPHERE => orthonormal(&values[4..7], &values[7..10]),
        tag::TORUS => orthonormal(&values[3..6], &values[8..11]),
        _ => false,
    }
}

fn valid_carrier_scalars(tt: u8, values: &[f64]) -> bool {
    match tt {
        tag::LINE | tag::PLANE => true,
        tag::CIRCLE => values[9] > 0.0,
        tag::ELLIPSE => values[9] >= values[10] && values[10] > 0.0,
        tag::CYLINDER => values[6] > 0.0,
        tag::CONE => {
            values[6] >= 0.0
                && values[7].abs() > f64::EPSILON
                && values[8] > 0.0
                && (values[7] * values[7] + values[8] * values[8] - 1.0).abs() <= 1.0e-9
        }
        tag::SPHERE => values[3] > 0.0,
        tag::TORUS => values[6].abs() > f64::EPSILON && values[7] > 0.0,
        _ => false,
    }
}

fn analytic_marker_candidates(body: &[u8], hdr: usize) -> Option<Vec<usize>> {
    let mut candidates = Vec::with_capacity(2);

    let partition_marker = hdr.checked_add(analytic::MARKER)?;
    if matches!(body.get(partition_marker), Some(0x2b | 0x2d)) {
        candidates.push(partition_marker);
    }

    // Deltas records encode each of the five references as [hi][lo][01].
    // The marker follows that fixed-width roster, so its position is not a
    // search result. The terminators distinguish this framing from arbitrary
    // marker-like bytes in the reference and ordinal fields.
    let refs_at = hdr.checked_add(analytic::REFS)?;
    let tripled_marker = hdr.checked_add(DELTAS_MARKER_OFFSET)?;
    let tripled_refs = (0..COMPACT_REF_COUNT)
        .all(|index| refs_at.checked_add(index * DELTAS_REF_STRIDE + DELTAS_REF_STRIDE - 1).and_then(|at| body.get(at)) == Some(&1));
    if tripled_refs && matches!(body.get(tripled_marker), Some(0x2b | 0x2d)) {
        candidates.push(tripled_marker);
    }

    Some(candidates)
}

fn parse_carrier_at_marker(body: &[u8], off: usize, tt: u8, attr: u16, n: usize, marker_at: usize) -> Option<AnalyticCarrier> {
    let values_at = marker_at.checked_add(1)?;
    let end = values_at.checked_add(n.checked_mul(8)?)?;
    let mut view = BinaryReader::with_position(body, values_at).ok()?;
    let vals = (0..n).map(|_| view.f64().ok()).collect::<Option<Vec<_>>>()?;
    if vals.iter().any(|value| !value.is_finite()) {
        return None;
    }
    if !valid_carrier_frame(tt, &vals) || !valid_carrier_scalars(tt, &vals) {
        return None;
    }

    Some(AnalyticCarrier {
        read_spans: vec![
            ReadSpan::new(off, off + 4 + usize::from(body.get(off + 2) == Some(&0xff)), "analytic.identity", Some(attr)),
            ReadSpan::new(marker_at, end, "analytic.values", Some(attr)),
        ],
        attr,
        offset: off,
        end,
        orientation: body[marker_at],
        tag: tt,
        values: vals,
    })
}

/// Try to parse a compact analytic carrier whose tag byte pair `00 TT` begins at
/// `off`. The partition and deltas framings are both considered, and a carrier
/// is returned only when exactly one framing passes all structural and geometry
/// invariants.
pub fn parse_carrier(body: &[u8], off: usize) -> Option<AnalyticCarrier> {
    if body.get(off) != Some(&0x00) {
        return None;
    }
    let tt = *body.get(off.checked_add(1)?)?;
    let n = analytic_value_count(tt)?;

    // The optional 0xff after the tag shifts the fixed header by one byte.
    let tag_end = off.checked_add(2)?;
    let has_ff = body.get(tag_end) == Some(&0xff);
    let hdr = tag_end.checked_add(usize::from(has_ff))?;
    let attr = View::u16_be_at(body, hdr)?;
    let mut candidates =
        analytic_marker_candidates(body, hdr)?.into_iter().filter_map(|marker_at| parse_carrier_at_marker(body, off, tt, attr, n, marker_at));
    let carrier = candidates.next()?;
    candidates.next().is_none().then_some(carrier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_orientation_preserves_curve_and_surface_parameters_in_both_framings() {
        for (tag, values) in
            [(tag::LINE, vec![0.25_f64, -0.5, 0.75, 0.0, 0.0, 1.0]), (tag::PLANE, vec![0.25, -0.5, 0.75, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0])]
        {
            for delta in [false, true] {
                for extended in [false, true] {
                    let mut bytes = vec![0xa5; 7];
                    let offset = bytes.len();
                    bytes.extend([0, tag]);
                    if extended {
                        bytes.push(0xff);
                    }
                    bytes.extend([0, 42, 0, 0, 0, 0]);
                    for _ in 0..5 {
                        bytes.extend([0, 1]);
                        if delta {
                            bytes.push(1);
                        }
                    }
                    let marker = bytes.len();
                    bytes.push(b'+');
                    for value in &values {
                        bytes.extend(value.to_be_bytes());
                    }
                    let positive = parse_carrier(&bytes, offset).unwrap();
                    bytes[marker] = b'-';
                    let negative = parse_carrier(&bytes, offset).unwrap();
                    assert_eq!(positive.orientation, b'+');
                    assert_eq!(negative.orientation, b'-');
                    for carrier in [&positive, &negative] {
                        assert_eq!(carrier.tag, tag);
                        assert_eq!(carrier.attr, 42);
                        assert_eq!(carrier.offset, offset);
                        assert_eq!(carrier.end, bytes.len());
                        assert_eq!(carrier.values, values);
                        assert_eq!(carrier.read_spans[1].offset, marker);
                        assert_eq!(carrier.read_spans[1].byte_len, bytes.len() - marker);
                    }
                    assert_eq!(positive.read_spans, negative.read_spans);
                    bytes[marker] = b'?';
                    assert!(parse_carrier(&bytes, offset).is_none());
                }
            }
        }
    }
}
