// SPDX-License-Identifier: Apache-2.0
use super::{
    decode_slots, Binder, HistoricalRecordArchive, Slots, Statistics, MAX_HISTORY_BINDING_WORK,
};
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
use std::collections::BTreeMap;

fn record(slot: i64, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index: usize::try_from(slot).unwrap(),
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn refs(slot: i64, name: &str, fields: &[(usize, i64)]) -> Record {
    let mut tokens = vec![Token::Ref(-1); 12];
    for &(field, reference) in fields {
        tokens[field] = Token::Ref(reference);
    }
    record(slot, name, tokens)
}

fn fixture() -> BTreeMap<i64, Record> {
    let mut records = vec![
        refs(0, "asmheader", &[]),
        refs(1, "body", &[(3, 2)]),
        refs(2, "lump", &[(4, 3), (5, 1)]),
        refs(3, "shell", &[(5, 10), (7, 2)]),
    ];
    for (base, next, partner) in [(10, 20, 23), (20, -1, 13)] {
        records.extend([
            refs(
                base,
                "face",
                &[(3, next), (4, base + 2), (5, 3), (7, base + 1)],
            ),
            record(
                base + 1,
                "plane-surface",
                vec![
                    Token::Ref(-1),
                    Token::Position([0.0, 0.0, 0.0]),
                    Token::Vector3([0.0, 0.0, 1.0]),
                    Token::Vector3([1.0, 0.0, 0.0]),
                ],
            ),
            refs(base + 2, "loop", &[(4, base + 3), (5, base)]),
            refs(
                base + 3,
                "coedge",
                &[
                    (3, base + 3),
                    (4, base + 3),
                    (5, partner),
                    (6, base + 4),
                    (8, base + 2),
                ],
            ),
            refs(
                base + 4,
                "edge",
                &[(3, base + 5), (5, base + 5), (7, base + 3), (8, base + 7)],
            ),
            refs(base + 5, "vertex", &[(3, base + 4), (4, base + 6)]),
            record(
                base + 6,
                "point",
                vec![Token::Ref(-1), Token::Position([0.0, 0.0, 0.0])],
            ),
            record(
                base + 7,
                "straight-curve",
                vec![
                    Token::Ref(-1),
                    Token::Position([0.0, 0.0, 0.0]),
                    Token::Vector3([1.0, 0.0, 0.0]),
                ],
            ),
        ]);
    }
    records
        .into_iter()
        .map(|record| (i64::try_from(record.index).unwrap(), record))
        .collect()
}

fn ring(base: i64, face: i64, next: i64, partner: i64) -> [Record; 6] {
    [
        refs(base, "loop", &[(3, next), (4, base + 1), (5, face)]),
        refs(
            base + 1,
            "coedge",
            &[
                (3, base + 1),
                (4, base + 1),
                (5, partner),
                (6, base + 2),
                (8, base),
            ],
        ),
        refs(
            base + 2,
            "edge",
            &[(3, base + 3), (5, base + 3), (7, base + 1), (8, base + 5)],
        ),
        refs(base + 3, "vertex", &[(3, base + 2), (4, base + 4)]),
        record(
            base + 4,
            "point",
            vec![Token::Ref(-1), Token::Position([0.0, 0.0, 0.0])],
        ),
        record(
            base + 5,
            "straight-curve",
            vec![
                Token::Ref(-1),
                Token::Position([0.0, 0.0, 0.0]),
                Token::Vector3([1.0, 0.0, 0.0]),
            ],
        ),
    ]
}

/// Face 10 gains inner loops 30 and 60, partnered with hole-wall faces 50 and 80.
fn holed_fixture() -> BTreeMap<i64, Record> {
    let mut records = fixture();
    set_ref(&mut records, 12, 3, 30);
    set_ref(&mut records, 20, 3, 50);
    for (face, next, partner, wall) in [(50, 80, 31, 30), (80, -1, 61, 60)] {
        records.extend(
            [
                refs(
                    face,
                    "face",
                    &[(3, next), (4, face + 2), (5, 3), (7, face + 1)],
                ),
                record(
                    face + 1,
                    "plane-surface",
                    vec![
                        Token::Ref(-1),
                        Token::Position([0.0, 0.0, 0.0]),
                        Token::Vector3([0.0, 0.0, 1.0]),
                        Token::Vector3([1.0, 0.0, 0.0]),
                    ],
                ),
            ]
            .into_iter()
            .chain(ring(face + 2, face, -1, partner))
            .chain(ring(wall, 10, if wall == 30 { 60 } else { -1 }, face + 3))
            .map(|record| (i64::try_from(record.index).unwrap(), record)),
        );
    }
    records
}

fn holed_change(change: impl FnOnce(&mut BTreeMap<i64, Record>) -> Slots) -> Statistics {
    let a = holed_fixture();
    let mut b = a.clone();
    let changed = change(&mut b);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
    stats
}

fn set_ref(records: &mut BTreeMap<i64, Record>, slot: i64, field: usize, reference: i64) {
    std::sync::Arc::make_mut(&mut records.get_mut(&slot).unwrap().tokens)[field] =
        Token::Ref(reference);
}

fn compare_step(
    a: &BTreeMap<i64, Record>,
    b: &BTreeMap<i64, Record>,
    changed: &Slots,
    saved_limit: Option<i64>,
) -> Statistics {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let mut bytes = Vec::new();
    if let Some(limit) = saved_limit {
        bytes.extend_from_slice(b"ASM BinaryFile8");
        bytes.extend_from_slice(&23100_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(&u64::try_from(limit).unwrap().to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut archive: HistoricalRecordArchive = a
        .iter()
        .map(|(&slot, record)| (slot, record.clone()))
        .collect();
    let versions_a = a.keys().map(|&slot| (slot, slot)).collect();
    let mut versions_b = BTreeMap::new();
    for (&slot, record) in b {
        let revision = if changed.contains(&slot) {
            slot + 1000
        } else {
            slot
        };
        archive.insert(revision, record.clone());
        versions_b.insert(slot, revision);
    }
    let work = WorkBudget::new(MAX_HISTORY_BINDING_WORK);
    let mut binder = Binder::new(&archive, saved_limit, &work);
    let mut stats = Statistics::default();
    binder
        .update(&a.keys().copied().collect(), &versions_a)
        .unwrap();
    binder.rebuild_units();
    let full_previous = binder.full(&ctx, &bytes, &mut stats).unwrap().unwrap();
    let mut previous = super::IndexedTopology::from(full_previous.clone());
    assert!(!full_previous.faces.is_empty());
    assert!(!full_previous.coedges.is_empty());
    let actual = binder
        .incremental(
            &ctx,
            &mut previous,
            changed,
            &versions_b,
            &bytes,
            &mut stats,
        )
        .unwrap();
    let expected = decode_slots(
        &ctx,
        &binder.live,
        &b.keys().copied().collect(),
        &bytes,
        &mut Statistics::default(),
        &WorkBudget::new(MAX_HISTORY_BINDING_WORK),
        None,
    )
    .unwrap();
    assert_eq!(actual.as_ref().map(|_| previous.materialize()), expected);
    if let Some(expected) = expected.as_ref() {
        let old_snapshot = || crate::history_records::AsmStateSnapshot {
            state_id: 1,
            topology: expected,
            versions: &versions_b,
        };
        let new_snapshot = || crate::history_records::AsmStateSnapshot {
            state_id: 2,
            topology: &actual.as_ref().unwrap().1,
            versions: &versions_a,
        };
        let full_new_snapshot = || crate::history_records::AsmStateSnapshot {
            state_id: 2,
            topology: &full_previous,
            versions: &versions_a,
        };
        assert_eq!(
            super::transition_for_changes(
                crate::history_records::AsmStateSnapshot {
                    topology: &actual.as_ref().unwrap().0,
                    ..old_snapshot()
                },
                new_snapshot(),
                changed
            ),
            super::AsmHistoricalTransition::between(Some(old_snapshot()), full_new_snapshot()),
        );
    }
    stats
}

fn incremental_change(change: impl FnOnce(&mut BTreeMap<i64, Record>) -> Slots) {
    let a = fixture();
    let mut b = a.clone();
    let changed = change(&mut b);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn face_insertion() {
    let mut a = fixture();
    let b = a.clone();
    // The older state inserts a face that is absent in the newer state.
    for slot in 20..28 {
        a.remove(&slot);
    }
    set_ref(&mut a, 10, 3, -1);
    set_ref(&mut a, 13, 5, -1);
    let stats = compare_step(&a, &b, &(20..28).chain([10, 13]).collect(), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn face_deletion() {
    incremental_change(|b| {
        for slot in 20..28 {
            b.remove(&slot);
        }
        set_ref(b, 10, 3, -1);
        set_ref(b, 13, 5, -1);
        (20..28).chain([10, 13]).collect()
    });
}

#[test]
fn neighbour_partner_change() {
    incremental_change(|b| {
        set_ref(b, 13, 5, -1);
        Slots::from([13])
    });
}

#[test]
fn carrier_update() {
    incremental_change(|b| {
        b.get_mut(&11).unwrap().tokens = vec![
            Token::Ref(-1),
            Token::Position([1.0, 2.0, 3.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
        ]
        .into();
        Slots::from([11])
    });
}

#[test]
fn restamped_records_enter_the_transition_without_decoding() {
    let a = fixture();
    let stats = compare_step(&a, &a, &Slots::from([11, 13, 14, 16]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
    assert_eq!(stats.decoded_records, a.len());
}

#[test]
fn signed_zero_is_a_content_change() {
    let a = fixture();
    let mut b = a.clone();
    b.get_mut(&16).unwrap().tokens = vec![Token::Ref(-1), Token::Position([-0.0, 0.0, 0.0])].into();
    let stats = compare_step(&a, &b, &Slots::from([16]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
    assert!(stats.decoded_records > a.len());
}

#[test]
fn clean_multi_loop_face_splits_per_loop() {
    let a = holed_fixture();
    let archive = a
        .iter()
        .map(|(&slot, record)| (slot, record.clone()))
        .collect();
    let work = WorkBudget::new(MAX_HISTORY_BINDING_WORK);
    let mut binder = Binder::new(&archive, None, &work);
    binder
        .update(
            &a.keys().copied().collect(),
            &a.keys().map(|&s| (s, s)).collect(),
        )
        .unwrap();
    binder.rebuild_units();
    let unit = &binder.units.faces[&10];
    assert_eq!(unit.chain, vec![12, 30, 60]);
    assert_eq!(
        unit.parts.keys().copied().collect::<Vec<_>>(),
        vec![10, 12, 30, 60]
    );
    assert_eq!(unit.context, Slots::from([10, 11, 12, 30, 60]));
    assert!(!unit.parts[&30].contains(&10));
    let neighbours = binder.units.neighbours(&super::Parts::from([(10, 30)]));
    assert_eq!(neighbours, super::Parts::from([(10, 30), (50, 50)]));
}

#[test]
fn hole_wall_change_decodes_only_its_loop() {
    let full = holed_fixture().len();
    let stats = holed_change(|b| {
        set_ref(b, 53, 5, -1);
        Slots::from([53])
    });
    assert!(stats.decoded_records - full < full / 2, "{stats:?}");
}

#[test]
fn split_face_surface_change() {
    holed_change(|b| {
        b.get_mut(&11).unwrap().tokens = vec![
            Token::Ref(-1),
            Token::Position([1.0, 2.0, 3.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
        ]
        .into();
        Slots::from([11])
    });
}

#[test]
fn split_face_loops_sharing_a_curve() {
    holed_change(|b| {
        set_ref(b, 62, 8, 35);
        Slots::from([62])
    });
}

#[test]
fn split_face_chain_drops_a_loop() {
    holed_change(|b| {
        set_ref(b, 30, 3, -1);
        Slots::from([30])
    });
}

#[test]
fn split_face_chain_gains_a_loop() {
    let b = holed_fixture();
    let mut a = b.clone();
    set_ref(&mut a, 30, 3, -1);
    let stats = compare_step(&a, &b, &Slots::from([30]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn split_face_loop_changes_owner() {
    holed_change(|b| {
        set_ref(b, 60, 5, 80);
        Slots::from([60])
    });
}

#[test]
fn split_face_deletion() {
    holed_change(|b| {
        for slot in (10..18).chain(30..36).chain(60..66) {
            b.remove(&slot);
        }
        set_ref(b, 3, 5, 20);
        for coedge in [23, 53, 83] {
            set_ref(b, coedge, 5, -1);
        }
        (10..18)
            .chain(30..36)
            .chain(60..66)
            .chain([3, 23, 53, 83])
            .collect()
    });
}

#[test]
fn loop_shared_with_another_face_chain_falls_back() {
    let mut a = holed_fixture();
    set_ref(&mut a, 82, 3, 60);
    let mut b = a.clone();
    set_ref(&mut b, 53, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([53]), None);
    assert_eq!(stats.fallbacks.get("shared-loop-chain"), Some(&1));
}

#[test]
fn shell_face_chain_change() {
    incremental_change(|b| {
        set_ref(b, 3, 5, 20);
        Slots::from([3])
    });
}

#[test]
fn uncovered_record_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(40, record(40, "unused", Vec::new()));
    let stats = compare_step(&a, &b, &Slots::from([40]), None);
    assert_eq!(stats.fallbacks.get("uncovered-change"), Some(&1));
}

#[test]
fn changed_definition_with_uncovered_reference_falls_back() {
    let mut a = fixture();
    a.insert(
        40,
        record(
            40,
            "unused",
            vec![
                Token::SubtypeOpen,
                Token::Ident("ref".into()),
                Token::Long(0),
                Token::SubtypeClose,
            ],
        ),
    );
    a.get_mut(&11).unwrap().tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("example".into()),
        Token::SubtypeClose,
    ]
    .into();
    let mut b = a.clone();
    b.get_mut(&11).unwrap().tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("other".into()),
        Token::SubtypeClose,
    ]
    .into();
    let stats = compare_step(&a, &b, &Slots::from([11]), None);
    assert_eq!(stats.fallbacks.get("uncovered-change"), Some(&1));
}

#[test]
fn subset_reference_uses_unchanged_external_definition() {
    let mut a = fixture();
    a.insert(
        40,
        record(
            40,
            "unused",
            vec![
                Token::SubtypeOpen,
                Token::Ident("example".into()),
                Token::SubtypeClose,
            ],
        ),
    );
    a.get_mut(&11).unwrap().tokens = vec![
        Token::SubtypeOpen,
        Token::Ident("ref".into()),
        Token::Long(0),
        Token::SubtypeClose,
    ]
    .into();
    let mut b = a.clone();
    set_ref(&mut b, 13, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([13]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn shared_attribute_falls_back() {
    let mut a = fixture();
    set_ref(&mut a, 1, 0, 40);
    set_ref(&mut a, 10, 0, 40);
    a.insert(40, record(40, "color-attrib", vec![Token::Ref(-1)]));
    let mut b = a.clone();
    set_ref(&mut b, 13, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([13]), None);
    assert_eq!(stats.fallbacks.get("global-dependency"), Some(&1));
}

#[test]
fn wire_change_falls_back() {
    let mut a = fixture();
    set_ref(&mut a, 3, 6, 30);
    a.extend([
        (30, refs(30, "wire", &[(4, 31)])),
        (31, refs(31, "coedge", &[(3, 31), (4, 31), (6, 32)])),
        (32, refs(32, "edge", &[(3, 15), (5, 15), (8, 17)])),
    ]);
    let mut b = a.clone();
    set_ref(&mut b, 32, 8, 27);
    let stats = compare_step(&a, &b, &Slots::from([32]), None);
    assert_eq!(stats.fallbacks.get("wire-change"), Some(&1));
}

#[test]
fn saved_edge_ownership_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    for slot in [20, 22, 23] {
        b.remove(&slot);
    }
    set_ref(&mut b, 10, 3, -1);
    set_ref(&mut b, 13, 5, -1);
    set_ref(&mut b, 24, 7, -1);
    let stats = compare_step(&a, &b, &Slots::from([10, 13, 20, 22, 23, 24]), Some(30));
    assert_eq!(stats.fallbacks.get("saved-edge-ownership"), Some(&1));
}

#[test]
fn empty_shell_falls_back_before_scratch_retention() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(4, refs(4, "shell", &[(7, 2)]));
    set_ref(&mut b, 3, 3, 4);
    let stats = compare_step(&a, &b, &Slots::from([3, 4]), None);
    assert_eq!(stats.fallbacks.get("empty-shell"), Some(&1));
}

#[test]
fn subset_decode_failure_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    b.get_mut(&16).unwrap().tokens = vec![Token::Position([f64::NAN, 0.0, 0.0])].into();
    let stats = compare_step(&a, &b, &Slots::from([16]), None);
    assert_eq!(stats.fallbacks.get("subset-decode"), Some(&1));
}

#[test]
fn container_decode_failure_falls_back() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(
        40,
        record(
            40,
            "unknown-attrib",
            vec![
                Token::Ref(-1),
                Token::Long(0),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(1),
                Token::Double(f64::NAN),
            ],
        ),
    );
    set_ref(&mut b, 1, 0, 40);
    let stats = compare_step(&a, &b, &Slots::from([1, 40]), None);
    assert_eq!(stats.fallbacks.get("container-decode"), Some(&1));
}

#[test]
fn container_with_incomplete_saved_edge_walk_falls_back() {
    let mut a = fixture();
    set_ref(&mut a, 14, 7, -1);
    let mut b = a.clone();
    set_ref(&mut b, 1, 5, 14);
    let stats = compare_step(&a, &b, &Slots::from([1]), Some(30));
    assert_eq!(stats.fallbacks.get("container-saved-edge"), Some(&1));
}

#[test]
fn inherited_attribute_insertion_updates_its_owner_unit() {
    incremental_change(|b| {
        b.insert(40, refs(40, "color-attrib", &[(4, 20)]));
        // A current attribute has an integer marker after the reserved ref.
        std::sync::Arc::make_mut(&mut b.get_mut(&40).unwrap().tokens)[1] = Token::Long(0);
        Slots::from([40])
    });
}

#[test]
fn orphan_container_falls_back() {
    let mut a = fixture();
    // This live shell is not owned by any body closure.
    a.insert(4, refs(4, "shell", &[(5, 10)]));
    let mut b = a.clone();
    set_ref(&mut b, 13, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([13]), None);
    assert_eq!(stats.fallbacks.get("orphan-container"), Some(&1));
}

#[test]
fn existing_saved_container_falls_back_for_container_edit() {
    let mut a = fixture();
    a.insert(30, refs(30, "edge", &[(3, 15), (5, 15), (8, 17)]));
    let mut b = a.clone();
    set_ref(&mut b, 3, 5, 20);
    let stats = compare_step(&a, &b, &Slots::from([3]), Some(40));
    assert_eq!(stats.fallbacks.get("saved-free-containers"), Some(&1));
}

#[test]
fn losing_face_admission_changes_saved_edge_ownership() {
    let a = fixture();
    let mut b = a.clone();
    set_ref(&mut b, 20, 5, -1);
    let stats = compare_step(&a, &b, &Slots::from([20]), Some(30));
    assert_eq!(stats.fallbacks.get("saved-edge-ownership"), Some(&1));
}

fn append_subtype(records: &mut BTreeMap<i64, Record>, slot: i64, name: &str, index: i64) {
    let record = records.get_mut(&slot).unwrap();
    let mut tokens = record.tokens.to_vec();
    tokens.extend([
        Token::SubtypeOpen,
        Token::Ident(name.into()),
        Token::Long(index),
        Token::SubtypeClose,
    ]);
    record.tokens = tokens.into();
}

fn assert_reference_dirty(a: &BTreeMap<i64, Record>, b: &BTreeMap<i64, Record>, changed: &Slots) {
    let mut archive: HistoricalRecordArchive = a.iter().map(|(&s, r)| (s, r.clone())).collect();
    let versions_a = a.keys().map(|&s| (s, s)).collect();
    let versions_b = b
        .iter()
        .map(|(&s, r)| {
            archive.insert(s + 1000, r.clone());
            (s, s + 1000)
        })
        .collect();
    let work = WorkBudget::new(MAX_HISTORY_BINDING_WORK);
    let mut binder = Binder::new(&archive, None, &work);
    binder
        .update(&a.keys().copied().collect(), &versions_a)
        .unwrap();
    assert!(binder
        .resolution_changes(changed, &versions_b)
        .contains(&21));
}

#[test]
fn inserted_definition_dirties_unchanged_reference_outside_changed_face() {
    let mut a = fixture();
    set_ref(&mut a, 13, 5, -1);
    set_ref(&mut a, 23, 5, -1);
    append_subtype(&mut a, 17, "example", 0);
    append_subtype(&mut a, 21, "ref", 0);
    let mut b = a.clone();
    append_subtype(&mut b, 11, "inserted", 0);
    let changed = Slots::from([11]);
    assert_reference_dirty(&a, &b, &changed);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn definition_content_change_dirties_unchanged_reference() {
    let mut a = fixture();
    append_subtype(&mut a, 11, "example", 0);
    append_subtype(&mut a, 21, "ref", 0);
    let mut b = a.clone();
    let tokens = std::sync::Arc::make_mut(&mut b.get_mut(&11).unwrap().tokens);
    tokens[6] = Token::Long(1);
    let changed = Slots::from([11]);
    assert_reference_dirty(&a, &b, &changed);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn definition_reference_dependencies_propagate_between_records() {
    let mut a = fixture();
    append_subtype(&mut a, 11, "first", 0);
    append_subtype(&mut a, 17, "second", 0);
    append_subtype(&mut a, 17, "ref", 0);
    append_subtype(&mut a, 21, "ref", 1);
    let mut b = a.clone();
    std::sync::Arc::make_mut(&mut b.get_mut(&11).unwrap().tokens)[6] = Token::Long(1);
    let changed = Slots::from([11]);
    assert_reference_dirty(&a, &b, &changed);
    let stats = compare_step(&a, &b, &changed, None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn container_owner_insertion_updates_closure_without_a_forward_link() {
    let a = fixture();
    let mut b = a.clone();
    b.insert(40, refs(40, "lump", &[(4, 3), (5, 1)]));
    set_ref(&mut b, 3, 7, 40);
    let stats = compare_step(&a, &b, &Slots::from([3, 40]), None);
    assert!(stats.fallbacks.is_empty(), "{:?}", stats.fallbacks);
}

#[test]
fn saved_container_before_later_body_binds_in_key_order() {
    let mut records = fixture();
    records.insert(100, refs(100, "body", &[]));
    let mut saved = records[&14].clone();
    saved.index = 40;
    records.insert(40, saved);
    let mut bytes = b"ASM BinaryFile8".to_vec();
    bytes.extend_from_slice(&23100_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&50_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let input: Vec<_> = records.values().cloned().collect();
    let decoded =
        crate::brep::decode_history_topology(&ctx, &input, &bytes, crate::ids::ID_FORMAT).unwrap();
    let topology = super::historical_topology_with_tags(&decoded)
        .unwrap()
        .unwrap();
    assert_eq!(topology.bodies, vec![1, 40, 100]);
    assert!(super::IndexedTopology::ordered(&topology));
    let versions = records.keys().map(|&slot| (slot, slot)).collect();
    let snapshot = crate::history_records::AsmStateSnapshot {
        state_id: 1,
        topology: &topology,
        versions: &versions,
    };
    assert!(super::AsmHistoricalTransition::between(None, snapshot).is_some());
    assert!(super::transition_for_changes(snapshot, snapshot, &Slots::from([40])).is_some());
    let archive = records
        .iter()
        .map(|(&slot, record)| (slot, record.clone()))
        .collect();
    let work = WorkBudget::new(MAX_HISTORY_BINDING_WORK);
    let mut binder = Binder::new(&archive, Some(50), &work);
    binder
        .update(&records.keys().copied().collect(), &versions)
        .unwrap();
    let full = binder
        .full(&ctx, &bytes, &mut Statistics::default())
        .unwrap()
        .unwrap();
    assert_eq!(full, topology);
}

#[test]
fn binding_work_cap_counts_decoded_records() {
    let records = fixture();
    let archive: HistoricalRecordArchive = records
        .iter()
        .map(|(&slot, record)| (slot, record.clone()))
        .collect();
    let versions = records.keys().map(|&slot| (slot, slot)).collect();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for (limit, binds) in [(records.len() - 1, false), (records.len(), true)] {
        let work = WorkBudget::new(limit);
        let mut binder = Binder::new(&archive, None, &work);
        binder
            .update(&records.keys().copied().collect(), &versions)
            .unwrap();
        let full = binder.full(&ctx, &[], &mut Statistics::default()).unwrap();
        assert_eq!(full.is_some(), binds, "limit {limit}");
        assert_eq!(work.exhausted(), !binds, "limit {limit}");
    }
}
