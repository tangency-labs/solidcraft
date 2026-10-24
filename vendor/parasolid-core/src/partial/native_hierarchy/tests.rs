// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Synthetic record graphs; no private CAD geometry is embedded here.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::partial::topology::Record;

fn u16_at(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn append(bytes: &mut Vec<u8>, tag: u8, declaration: &[u8], raw: &[u8]) -> usize {
    bytes.extend([0, tag]);
    bytes.extend(declaration);
    let start = bytes.len();
    bytes.extend(raw);
    start
}

fn fixture() -> (Vec<u8>, Tables, Vec<usize>) {
    let mut data = Vec::new();
    let mut tables = Tables::default();
    let mut body_starts = Vec::new();
    // A solid and a sheet. No coordinates or connected-component heuristics
    // are available; the native graph is the sole membership evidence.
    for (index, (id, kind)) in [(3, 1), (33, 3)].into_iter().enumerate() {
        let region_id = 101 + index as u16 * 10;
        let shell_id = 201 + index as u16 * 10;
        let face_id = 301 + index as u16 * 10;
        let mut raw = [0_u8; 89];
        u16_at(&mut raw, 0, id);
        raw[5] = 5;
        raw[24..32].copy_from_slice(&1_f64.to_be_bytes());
        raw[32..40].copy_from_slice(&1e-8_f64.to_be_bytes());
        u16_at(&mut raw, 42, if index == 0 { 33 } else { 1 });
        u16_at(&mut raw, 44, if index == 0 { 1 } else { 3 });
        raw[46] = 1;
        u16_at(&mut raw, 47, 2);
        raw[49] = kind;
        raw[50] = 1;
        u16_at(&mut raw, 51, shell_id);
        u16_at(&mut raw, 65, region_id);
        u16_at(&mut raw, 87, 1);
        body_starts.push(append(&mut data, 12, if index == 0 { BODY_DECLARATION } else { &[] }, &raw));

        let mut raw = [0_u8; 21];
        for (at, value) in [(0, region_id), (8, id), (10, 1), (12, 1), (14, shell_id), (16, 1), (19, 1)] {
            u16_at(&mut raw, at, value);
        }
        raw[5] = 5;
        raw[18] = if kind == 1 { b'S' } else { b'V' };
        append(&mut data, 19, if index == 0 { REGION_DECLARATION } else { &[] }, &raw);

        let mut raw = [0_u8; 22];
        for (at, value) in [(0, shell_id), (8, id), (10, 1), (12, face_id), (14, 1), (16, 1), (18, region_id), (20, face_id)] {
            u16_at(&mut raw, at, value);
        }
        raw[5] = 5;
        append(&mut data, 13, &[], &raw);
        tables.bridges.insert(
            face_id,
            Record {
                read_ranges: Vec::new(),
                attr: face_id,
                refs: vec![1, 1, 1, shell_id, 1],
                marker: None,
                xyz_m: None,
                xyz_offset: None,
                owner: None,
                offset: 0,
            },
        );
    }
    (data, tables, body_starts)
}

#[test]
fn native_ids_determine_kind_and_membership_without_geometry() {
    let (data, tables, _) = fixture();
    let result = scan(&data, SCHEMA, &tables).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!((result[0].attr, result[0].kind, &result[0].refs), (3, BodyKind::Solid, &vec![301]));
    assert_eq!((result[1].attr, result[1].kind, &result[1].refs), (33, BodyKind::Sheet, &vec![311]));
    assert_eq!(result[1].regions[0].shells[0].attr, 211);
    assert_eq!(&data[result[1].offset..result[1].offset + 2], &[0, 12]);
}

#[test]
fn reviewed_36001_key_keeps_exact_declarations_and_complete_ownership() {
    let (data, mut tables, _) = fixture();
    let schema = "SCH_3601228_36001_13006";
    assert!(scan(&data, schema, &tables).is_some());
    assert!(!verified_read_spans(&data, schema, &tables).is_empty());
    for unknown in ["SCH_3601229_36001_13006", "SCH_3601228_36002_13006", "SCH_3601228_36001"] {
        assert!(scan(&data, unknown, &tables).is_none());
    }
    let mut changed = data.clone();
    changed[4] ^= 1;
    assert!(scan(&changed, schema, &tables).is_none());
    tables.bridges.get_mut(&301).unwrap().refs[3] = 211;
    assert!(scan(&data, schema, &tables).is_none());
}

#[test]
fn schema_and_embedded_declarations_are_required() {
    let (data, tables, _) = fixture();
    for schema in ["SCH_3701229_37103_13006", "SCH_SW_37102_13006", ""] {
        assert!(scan(&data, schema, &tables).is_none());
    }
    let mut changed = data;
    changed[8] ^= 1;
    assert!(scan(&changed, SCHEMA, &tables).is_none());
}

#[test]
fn invalid_body_links_and_kinds_never_yield_a_partial_native_claim() {
    let (data, tables, starts) = fixture();
    for (at, value) in [(starts[0] + 42, 3), (starts[1] + 44, 99), (starts[0] + 65, 999), (starts[0] + 42, 0)] {
        let mut bad = data.clone();
        u16_at(&mut bad, at, value);
        assert!(scan(&bad, SCHEMA, &tables).is_none());
    }
    let mut bad = data.clone();
    bad[starts[0] + 49] = 2; // wire is outside this profile
    assert!(scan(&bad, SCHEMA, &tables).is_none());
    for end in 0..data.len() {
        assert!(scan(&data[..end], SCHEMA, &tables).is_none(), "truncation at {end}");
    }
}

#[test]
fn duplicated_and_unowned_records_or_faces_fail_closed() {
    let (data, tables, starts) = fixture();
    let mut duplicate = data.clone();
    duplicate.extend([0, 12]);
    duplicate.extend_from_slice(&data[starts[1]..starts[1] + 89]);
    assert!(scan(&duplicate, SCHEMA, &tables).is_none());
    for (reference, value) in [(0, 301), (1, 999), (3, 211)] {
        let mut tables = Tables { bridges: tables.bridges.clone(), ..Tables::default() };
        tables.bridges.get_mut(&301).unwrap().refs[reference] = value;
        assert!(scan(&data, SCHEMA, &tables).is_none());
    }
    let mut changed = Tables { bridges: tables.bridges.clone(), ..Tables::default() };
    let mut extra = changed.bridges[&301].clone();
    extra.attr = 999;
    changed.bridges.insert(999, extra);
    assert!(scan(&data, SCHEMA, &changed).is_none());
    changed = Tables { bridges: tables.bridges.clone(), ..Tables::default() };
    changed.bridges.remove(&301);
    assert!(scan(&data, SCHEMA, &changed).is_none());
}

#[test]
fn hierarchy_updates_and_ambiguous_partition_selection_are_unsupported() {
    let (data, tables, starts) = fixture();
    assert!(recover(&[(&data, SCHEMA, false)], &tables).is_some());
    assert!(recover(&[(&data, SCHEMA, false), (&data, SCHEMA, false)], &tables).is_none());
    assert!(recover(&[(&data, SCHEMA, true)], &tables).is_none());
    let mut delta = vec![0, 12];
    delta.extend_from_slice(&data[starts[1]..starts[1] + 89]);
    assert!(recover(&[(&data, SCHEMA, false), (&delta, SCHEMA, true)], &tables).is_none());
}

#[test]
fn exterior_void_shell_uses_null_legacy_body_and_is_not_a_material_region() {
    let (mut data, tables, starts) = fixture();
    let (_, material) = declaration(&data, 19, REGION_DECLARATION).unwrap();
    u16_at(&mut data, starts[0] + 65, 103);
    u16_at(&mut data, material + 12, 103);
    let mut raw = [0_u8; 21];
    for (at, value) in [(0, 103), (8, 3), (10, 101), (12, 1), (14, 203), (16, 1), (19, 1)] {
        u16_at(&mut raw, at, value);
    }
    raw[5] = 5;
    raw[18] = b'V';
    append(&mut data, 19, &[], &raw);
    let mut raw = [0_u8; 22];
    for (at, value) in [(0, 203), (8, 1), (10, 1), (12, 1), (14, 1), (16, 1), (18, 103), (20, 301)] {
        u16_at(&mut raw, at, value);
    }
    raw[5] = 5;
    let shell = append(&mut data, 13, &[], &raw);
    let result = scan(&data, SCHEMA, &tables).unwrap();
    assert_eq!(result[0].regions.len(), 1);
    assert_eq!(result[0].regions[0].attr, 101);
    u16_at(&mut data, shell + 8, 99);
    assert!(scan(&data, SCHEMA, &tables).is_none());
}
