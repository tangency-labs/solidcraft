// SPDX-License-Identifier: Apache-2.0
//! Read the document's modelling length unit from the Design `UnitSystems`
//! collection.
//!
//! The collection names six unit systems; five are presets with fixed
//! `ModelingLength` names and the sixth, `Custom`, holds the document's active
//! settings. Its `ModelingLength` entry carries the display length unit under
//! the property name `modelingLengthName`.

use cadmpeg_core::decode::index_from_u32;

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::indexed_design_record_header as indexed_header;
use cadmpeg_core::decode::View;

/// An indexed-record header: `u32 3`, three class-tag digits, `u32 index`.
const HEADER_LEN: usize = indexed_header::LEN;
/// One reference slot: `01`, a `u32` record index, and six zero bytes.
const REFERENCE_LEN: usize = 11;
/// The collection names six unit systems.
const UNIT_SYSTEM_COUNT: u32 = 6;
/// Seventeen quantity families plus `ModelingLength` and `ModelingMass`.
const UNIT_ENTRY_COUNT: u32 = 19;
/// The system holding the document's active settings.
const CUSTOM_SYSTEM: &str = "Custom";
/// The property name of the `Custom` system's `ModelingLength` entry.
const MODELING_LENGTH_PROPERTY: &str = "modelingLengthName";
/// The namespace every unit-system record stores.
const SYSTEM_NAMESPACE: &str = "NaFusion";
/// The namespace every unit-entry record stores.
const ENTRY_NAMESPACE: &str = "NsCommonData";
/// The length unit names the `ModelingLength` entry takes.
const LENGTH_UNIT_NAMES: [&str; 5] = ["millimeter", "centimeter", "meter", "inch", "foot"];

/// Read one LP-ASCII field, returning it with the offset past its payload.
///
/// A stored key, name, or namespace is graphic ASCII; a label is display text,
/// so the space is admissible alongside it.
fn ascii_at(bytes: &[u8], at: usize) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if length > 256 {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    let raw = bytes.get(start..end)?;
    if !raw
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    Some((std::str::from_utf8(raw).ok()?, end))
}

/// Read the `u32` field at `at` and check it equals `expected`, returning the
/// offset past it.
fn expect_u32(bytes: &[u8], at: usize, expected: u32) -> Option<usize> {
    (View::u32_le_at(bytes, at)? == expected).then(|| at + 4)
}

/// Check that the four bytes at `at` are zero, returning the offset past them.
fn expect_zero_quad(bytes: &[u8], at: usize) -> Option<usize> {
    (bytes.get(at..at.checked_add(4)?)? == [0u8; 4]).then_some(at + 4)
}

/// Read the record index out of one `01 + u32 index + six zero bytes` slot.
fn reference_at(bytes: &[u8], at: usize) -> Option<u32> {
    if bytes.get(at) != Some(&1)
        || bytes.get(at.checked_add(5)?..at.checked_add(REFERENCE_LEN)?)? != [0u8; 6]
    {
        return None;
    }
    View::u32_le_at(bytes, at + 1)
}

/// Read a `u32 expected` count followed by that many reference slots.
fn references<const N: usize>(bytes: &[u8], at: usize) -> Option<[u32; N]> {
    let mut position = expect_u32(bytes, at, u32::try_from(N).ok()?)?;
    let mut out = [0; N];
    for slot in &mut out {
        *slot = reference_at(bytes, position)?;
        position = position.checked_add(REFERENCE_LEN)?;
    }
    Some(out)
}

/// The payload of one unit-system record: its key and its unit-entry
/// references. The record stores the key, a label, byte `01`, the name
/// `<key>UnitSystemName`, the `NaFusion` namespace, four zero bytes, and the
/// counted entry references.
fn unit_system(bytes: &[u8], at: usize) -> Option<(&str, [u32; index_from_u32(UNIT_ENTRY_COUNT)])> {
    let (key, position) = ascii_at(bytes, at)?;
    let (_label, position) = ascii_at(bytes, position)?;
    (bytes.get(position) == Some(&1)).then_some(())?;
    let (name, position) = ascii_at(bytes, position + 1)?;
    (name.strip_prefix(key) == Some("UnitSystemName")).then_some(())?;
    let (namespace, position) = ascii_at(bytes, position)?;
    (namespace == SYSTEM_NAMESPACE).then_some(())?;
    let position = expect_zero_quad(bytes, position)?;
    Some((key, references(bytes, position)?))
}

/// The property name and unit name of one unit-entry record. The record stores
/// a key, a label, byte `01`, the property name, the `NsCommonData` namespace,
/// four zero bytes, and the UTF-16 unit name.
fn unit_entry(bytes: &[u8], at: usize) -> Option<(&str, &'static str)> {
    let (_key, position) = ascii_at(bytes, at)?;
    let (_label, position) = ascii_at(bytes, position)?;
    (bytes.get(position) == Some(&1)).then_some(())?;
    let (property, position) = ascii_at(bytes, position + 1)?;
    let (namespace, position) = ascii_at(bytes, position)?;
    (namespace == ENTRY_NAMESPACE).then_some(())?;
    let position = expect_zero_quad(bytes, position)?;
    let count = usize::try_from(View::u32_le_at(bytes, position)?).ok()?;
    if count > 64 {
        return None;
    }
    let start = position.checked_add(4)?;
    let end = count
        .checked_mul(2)
        .and_then(|size| start.checked_add(size))?;
    let raw = bytes.get(start..end)?;
    let value = LENGTH_UNIT_NAMES.iter().copied().find(|name| {
        name.len() == count
            && raw
                .chunks_exact(2)
                .zip(name.as_bytes())
                .all(|(unit, byte)| unit == [*byte, 0])
    })?;
    Some((property, value))
}

/// Offsets of the unit-system reference count following each `UnitSystems`
/// collection name. The name is the LP-ASCII string followed by two zero bytes.
fn collection_counts(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    const PREFIX: &[u8] = b"\x0b\x00\x00\x00UnitSystems\x00\x00";
    memchr::memmem::find_iter(bytes, PREFIX).map(|start| start + PREFIX.len())
}

/// The `Custom` system's `modelingLengthName` value, when one design
/// `BulkStream` carries a well-formed `UnitSystems` collection.
///
/// The collection is located by name rather than by offset, so every candidate
/// match is parsed and the first that yields the property wins. A value outside
/// the five stored length unit names is rejected: the search is a byte-window
/// scan, and the closed name set is what separates the collection from a window
/// that merely reads like one.
fn decode_modeling_length_unit(bytes: &[u8]) -> Option<String> {
    let offsets = IndexedRecordOffsets::build(bytes);
    let payloads = |record_index: u32| {
        offsets
            .offsets(record_index)
            .iter()
            .filter_map(|at| at.checked_add(HEADER_LEN))
    };
    for count_at in collection_counts(bytes) {
        let Some(systems) = references::<{ index_from_u32(UNIT_SYSTEM_COUNT) }>(bytes, count_at)
        else {
            continue;
        };
        for system in systems {
            for system_at in payloads(system) {
                let Some((key, entries)) = unit_system(bytes, system_at) else {
                    continue;
                };
                if key != CUSTOM_SYSTEM {
                    continue;
                }
                for entry in &entries {
                    for entry_at in payloads(*entry) {
                        let Some((property, value)) = unit_entry(bytes, entry_at) else {
                            continue;
                        };
                        if property == MODELING_LENGTH_PROPERTY {
                            return Some(value.to_owned());
                        }
                    }
                }
            }
        }
    }
    None
}

/// The document's modelling length unit, read from the first design
/// `BulkStream` that carries it.
///
/// An entry whose bytes cannot be read is skipped rather than failing the
/// decode: the unit is presentation metadata, and no geometry depends on it.
pub(crate) fn decode_document_length_unit(scan: &ContainerScan) -> Option<String> {
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        if let Ok(bytes) = scan.entry_bytes(&entry.name) {
            if let Some(unit) = decode_modeling_length_unit(bytes) {
                return Some(unit);
            }
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        CUSTOM_SYSTEM, ENTRY_NAMESPACE, LENGTH_UNIT_NAMES, SYSTEM_NAMESPACE, UNIT_ENTRY_COUNT,
        UNIT_SYSTEM_COUNT,
    };
    use crate::test_support::{lp_ascii, lp_utf16};

    use super::decode_modeling_length_unit;

    /// The six systems in collection order.
    const SYSTEMS: [&str; 6] = [
        "CmMKS",
        "MmMKS",
        "MMKS",
        "InchImperial",
        "Imperial",
        CUSTOM_SYSTEM,
    ];
    /// The nineteen unit entries a system stores, `ModelingLength` last but one.
    const ENTRIES: [&str; 19] = [
        "Length",
        "Mass",
        "Time",
        "Temperature",
        "Speed",
        "Volume",
        "Pressure",
        "Force",
        "Power",
        "Energy",
        "Current",
        "Substance",
        "Luminosity",
        "Angle",
        "Currency",
        "Percentage",
        "Pieces",
        "ModelingLength",
        "ModelingMass",
    ];

    fn header(out: &mut Vec<u8>, record_index: u32) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"001");
        out.extend_from_slice(&record_index.to_le_bytes());
    }

    fn reference(out: &mut Vec<u8>, record_index: u32) {
        out.push(1);
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
    }

    /// The first entry record index belonging to system `slot`.
    fn entry_base(slot: usize) -> u32 {
        100 + u32::try_from(slot).unwrap() * 19
    }

    /// A design stream carrying the collection, its six systems, and every
    /// system's nineteen unit entries. `lengths[slot]` is the `ModelingLength`
    /// name that system stores.
    pub(crate) fn stream(lengths: [&str; 6]) -> Vec<u8> {
        let mut out = Vec::new();
        header(&mut out, 1);
        lp_ascii(&mut out, "UnitSystems");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&UNIT_SYSTEM_COUNT.to_le_bytes());
        for slot in 0..SYSTEMS.len() {
            reference(&mut out, 10 + u32::try_from(slot).unwrap());
        }
        for (slot, key) in SYSTEMS.iter().enumerate() {
            header(&mut out, 10 + u32::try_from(slot).unwrap());
            lp_ascii(&mut out, key);
            lp_ascii(&mut out, &format!("{key} label"));
            out.push(1);
            lp_ascii(&mut out, &format!("{key}UnitSystemName"));
            lp_ascii(&mut out, SYSTEM_NAMESPACE);
            out.extend_from_slice(&[0u8; 4]);
            out.extend_from_slice(&UNIT_ENTRY_COUNT.to_le_bytes());
            for offset in 0..ENTRIES.len() {
                reference(&mut out, entry_base(slot) + u32::try_from(offset).unwrap());
            }
        }
        for (slot, length) in lengths.iter().enumerate() {
            for (offset, entry) in ENTRIES.iter().enumerate() {
                header(&mut out, entry_base(slot) + u32::try_from(offset).unwrap());
                lp_ascii(&mut out, entry);
                lp_ascii(&mut out, &format!("{entry} label"));
                out.push(1);
                lp_ascii(&mut out, &format!("{}Name", lower_camel(entry)));
                lp_ascii(&mut out, ENTRY_NAMESPACE);
                out.extend_from_slice(&[0u8; 4]);
                lp_utf16(
                    &mut out,
                    if *entry == "ModelingLength" {
                        length
                    } else {
                        "unit"
                    },
                );
            }
        }
        out
    }

    fn lower_camel(value: &str) -> String {
        let mut chars = value.chars();
        chars
            .next()
            .map(|first| first.to_ascii_lowercase().to_string() + chars.as_str())
            .unwrap_or_default()
    }

    #[test]
    fn reads_every_stored_length_unit_name_from_the_custom_system() {
        for unit in LENGTH_UNIT_NAMES {
            let bytes = stream(["centimeter", "millimeter", "meter", "inch", "foot", unit]);
            assert_eq!(
                decode_modeling_length_unit(&bytes).as_deref(),
                Some(unit),
                "expected the Custom system's {unit}"
            );
        }
    }

    #[test]
    fn preset_systems_do_not_supply_the_document_unit() {
        // Every preset holds a different fixed name; only `Custom` carries the
        // document's active setting.
        let bytes = stream(["centimeter", "millimeter", "meter", "foot", "foot", "inch"]);
        assert_eq!(decode_modeling_length_unit(&bytes).as_deref(), Some("inch"));
    }

    #[test]
    fn a_name_outside_the_stored_set_is_rejected() {
        let bytes = stream([
            "centimeter",
            "millimeter",
            "meter",
            "inch",
            "foot",
            "furlong",
        ]);
        assert_eq!(decode_modeling_length_unit(&bytes), None);
    }

    #[test]
    fn a_stream_without_the_collection_yields_no_unit() {
        let mut bytes = Vec::new();
        header(&mut bytes, 1);
        lp_ascii(&mut bytes, "BodiesRoot");
        assert_eq!(decode_modeling_length_unit(&bytes), None);
    }

    #[test]
    fn a_truncated_collection_yields_no_unit() {
        let full = stream(["centimeter", "millimeter", "meter", "inch", "foot", "inch"]);
        let truncated = &full[..full.len() / 2];
        assert_eq!(decode_modeling_length_unit(truncated), None);
    }
}
