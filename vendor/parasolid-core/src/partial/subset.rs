// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Partial bounded-curve wrappers, with unscaled points and parameters.
use super::view::View;

#[derive(Debug, Clone)]
pub struct SubsetRecord {
    pub attr: u16,
    pub source_attr: u16,
    pub offset: usize,
    pub end: usize,
    pub values: [f64; 8],
}
/// Read finite type-133 wrapper values without certifying source-curve agreement.
pub fn scan(bytes: &[u8]) -> Vec<SubsetRecord> {
    let mut out = Vec::new();
    for offset in 0..bytes.len().saturating_sub(2) {
        if bytes.get(offset..offset + 2) != Some(&[0, 0x85]) {
            continue;
        }
        let header = offset + 2 + usize::from(bytes.get(offset + 2) == Some(&0xff));
        let marker = header + 16;
        if !matches!(bytes.get(marker), Some(0x2b | 0x2d)) {
            continue;
        }
        let Some(attr) = View::u16_be_at(bytes, header) else {
            continue;
        };
        let Some(source_attr) = View::u16_be_at(bytes, marker + 1) else {
            continue;
        };
        let values = (0..8).map(|i| View::f64_be_at(bytes, marker + 3 + i * 8)).collect::<Option<Vec<_>>>();
        let Some(values) = values.filter(|v| v.iter().all(|x| x.is_finite())) else {
            continue;
        };
        let Ok(values) = values.try_into() else {
            continue;
        };
        out.push(SubsetRecord { attr, source_attr, offset, end: marker + 67, values });
    }
    out
}
