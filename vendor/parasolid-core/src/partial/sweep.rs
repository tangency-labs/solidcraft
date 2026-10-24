// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Source-unit sweep/spin definitions; no derived surface evaluation.
use super::view::View;
use std::collections::HashMap;

/// A parsed swept- or spun-surface carrier.
#[derive(Debug, Clone)]
pub struct SweepCarrier {
    /// Stream-local attribute id of the record.
    pub attr: u16,
    /// Tag-byte offset in the stream.
    pub offset: usize,
    /// Attribute of the profile curve carrier.
    pub profile_attr: u16,
    /// Construction-specific fields.
    pub kind: SweepKind,
}

/// The construction a [`SweepCarrier`] encodes.
#[derive(Debug, Clone)]
pub enum SweepKind {
    /// `00 43`: translation of the profile along a unit direction.
    Swept {
        /// Unit sweep direction (dimensionless).
        direction: [f64; 3],
    },
    /// `00 44`: revolution of the profile about an axis.
    Spun {
        /// Point on the spin axis, in source length units.
        base: [f64; 3],
        /// Unit spin-axis direction.
        axis: [f64; 3],
    },
}

fn unit3(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    let x = View::f64_be_at(bytes, at)?;
    let y = View::f64_be_at(bytes, at + 8)?;
    let z = View::f64_be_at(bytes, at + 16)?;
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return None;
    }
    let norm = (x * x + y * y + z * z).sqrt();
    if (norm - 1.0).abs() > 1.0e-9 {
        return None;
    }
    Some([x, y, z])
}

fn point_source(bytes: &[u8], at: usize) -> Option<[f64; 3]> {
    let x = View::f64_be_at(bytes, at)?;
    let y = View::f64_be_at(bytes, at + 8)?;
    let z = View::f64_be_at(bytes, at + 16)?;
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return None;
    }
    Some([x, y, z])
}

/// Parse one swept/spun record whose `00 43`/`00 44` tag starts at `off`.
fn parse_sweep(bytes: &[u8], off: usize) -> Option<SweepCarrier> {
    if bytes.get(off) != Some(&0x00) {
        return None;
    }
    let tt = *bytes.get(off + 1)?;
    if tt != 0x43 && tt != 0x44 {
        return None;
    }
    let mut p = off + 2;
    if bytes.get(p) == Some(&0xff) {
        p += 1;
    }
    let attr = View::u16_be_at(bytes, p)?;
    if attr == 0 {
        return None;
    }
    if !matches!(bytes.get(p + 16), Some(0x2b | 0x2d)) {
        return None;
    }
    let profile_attr = View::u16_be_at(bytes, p + 17)?;
    if profile_attr == 0 {
        return None;
    }
    let values = p + 19;
    let kind = if tt == 0x43 {
        SweepKind::Swept { direction: unit3(bytes, values)? }
    } else {
        SweepKind::Spun { base: point_source(bytes, values)?, axis: unit3(bytes, values + 24)? }
    };
    Some(SweepCarrier { attr, offset: off, profile_attr, kind })
}

/// Scan a stream for swept/spun surface carriers, keyed by attribute.
pub fn scan_sweep_carriers(bytes: &[u8]) -> HashMap<u16, SweepCarrier> {
    let mut out = HashMap::new();
    for off in 0..bytes.len().saturating_sub(20) {
        if let Some(carrier) = parse_sweep(bytes, off) {
            out.entry(carrier.attr).or_insert(carrier);
        }
    }
    out
}
