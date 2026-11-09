// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

use crate::test_support::scene::override_row as encoded;

fn decode(bytes: &[u8]) -> (Vec<DisplayPose>, Vec<LossNote>) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let mut losses = Vec::new();
    let rows = read(&ctx, view, "world", &mut losses).unwrap();
    (rows, losses)
}

#[test]
fn scene_one_and_two_level_paths_preserve_matrix_and_offset() {
    for path in [vec![3, 17], vec![3, 17, 23, 29]] {
        let mut bytes = vec![99; 7];
        bytes.extend(encoded(&path));
        let (rows, losses) = decode(&bytes);
        assert!(losses.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].byte_offset, 7);
        assert_eq!(rows[0].path.0, path);
        assert_eq!(
            rows[0].transform.0,
            [
                [1., 0., 0., 20.],
                [0., 1., 0., 30.],
                [0., 0., 1., 40.],
                [0., 0., 0., 1.]
            ]
        );
    }
}

#[test]
fn scene_refusals_are_counted_once_per_stream_and_later_rows_survive() {
    let mut truncated = encoded(&[3, 17]);
    truncated.pop();
    let (rows, losses) = decode(&truncated);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::SceneOverrideRefused.kind());
    assert!(losses[0]
        .message
        .contains("1 display override candidate(s)"));
    let mut invalid = encoded(&[3]);
    invalid.extend(encoded(&[3, 17, 23]));
    invalid.extend(encoded(&[3, 17]));
    let (rows, losses) = decode(&invalid);
    assert_eq!(rows.len(), 1);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("2 display override candidate(s)"));
}

#[test]
fn scene_rejects_wrong_prologue_markers_counts_indices_and_nonrigid_matrices() {
    let original = encoded(&[3, 17]);
    let matrix = COUNTED_NAME.len() + 5;
    let path_count = original.len() - 28;
    let mut cases = Vec::new();
    for offset in [COUNTED_NAME.len(), matrix + 64, matrix + 65] {
        let mut bytes = original.clone();
        bytes[offset] = 7;
        cases.push(bytes);
    }
    for count in [1, 3, 1024, u32::MAX] {
        let mut bytes = original.clone();
        bytes[path_count..path_count + 4].copy_from_slice(&count.to_le_bytes());
        cases.push(bytes);
    }
    for (slot, value) in [
        (0, f32::NAN),
        (0, f32::INFINITY),
        (0, 2.),
        (0, -1.),
        (3, 1.),
        (15, 0.),
    ] {
        let mut bytes = original.clone();
        bytes[matrix + slot * 4..matrix + slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        cases.push(bytes);
    }
    let mut oversized = original.clone();
    oversized[path_count + 4] = 1;
    cases.push(oversized);
    for bytes in cases {
        let (rows, losses) = decode(&bytes);
        assert!(rows.is_empty());
        assert_eq!(losses.len(), 1);
    }
}

#[test]
fn scene_counted_name_and_complete_following_grammar_gate_admission() {
    let mut bytes = encoded(&[3, 17]);
    bytes[0] = 25;
    let (rows, losses) = decode(&bytes);
    assert!(rows.is_empty());
    assert!(losses.is_empty());
    let (rows, losses) = decode(COUNTED_NAME);
    assert!(rows.is_empty());
    assert_eq!(losses.len(), 1);
    let (rows, losses) = decode(b"unrelated scene content");
    assert!(rows.is_empty());
    assert!(losses.is_empty());
}

fn occurrence(
    id: &str,
    parent: Option<&str>,
    reference: Option<&str>,
) -> cadmpeg_ir::products::Occurrence {
    let mut wire = serde_json::json!({
        "id":id, "parent":parent.map_or_else(||serde_json::json!({"kind":"root"}), |id|serde_json::json!({"kind":"occurrence","occurrence":id})),
        "prototype":{"scope":"local","definition":"f3d:test:definition#1"},
        "ordinal":0,"transform":[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.]],"scale":[1.,1.,1.]
    });
    if let Some(reference) = reference {
        wire["native_ref"] = serde_json::json!(reference);
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn scene_binding_is_exact_and_preserves_all_three_states() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.occurrences = vec![
        occurrence("f3d:test:occurrence#root", None, None),
        occurrence(
            "f3d:test:occurrence#a",
            Some("f3d:test:occurrence#root"),
            Some("a"),
        ),
        occurrence(
            "f3d:test:occurrence#child",
            Some("f3d:test:occurrence#a"),
            Some("child"),
        ),
        occurrence(
            "f3d:test:occurrence#duplicate",
            Some("f3d:test:occurrence#root"),
            Some("a"),
        ),
    ];
    let sources = Sources::from([("a".to_owned(), [3, 17]), ("child".to_owned(), [23, 29])]);
    let mut losses = Vec::new();
    let mut rows = Vec::new();
    for (ordinal, path) in [
        &[3, 17][..],
        &[3, 17, 23, 29],
        &[3, 17, 23, 31],
        &[5, 17, 23, 29],
    ]
    .into_iter()
    .enumerate()
    {
        let mut found = read(
            &ctx,
            View::over_retained(&encoded(path)),
            "world",
            &mut losses,
        )
        .unwrap();
        found[0].id = format!("f3d:test:pose#{ordinal}");
        rows.extend(found);
    }
    ir.native
        .namespace_mut("f3d")
        .set_arena(&ctx, "display_poses", &rows)
        .unwrap();
    let unchanged = ir.model.occurrences.clone();
    bind(&ctx, &mut ir, &sources, &mut losses).unwrap();
    let rows: Vec<DisplayPose> = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as_for_decode(&ctx, "display_poses")
        .unwrap();
    assert_eq!(rows[0].binding, SceneBinding::Ambiguous {});
    assert_eq!(
        rows[1].binding,
        SceneBinding::Bound {
            occurrence: OccurrenceId::mint("f3d:test:occurrence#child").unwrap()
        }
    );
    assert_eq!(rows[2].binding, SceneBinding::Unbound {});
    assert_eq!(rows[3].binding, SceneBinding::Unbound {});
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SceneOverrideUnbound.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SceneOverrideAmbiguous.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.message.starts_with("2 display override")));
    assert_eq!(ir.model.occurrences, unchanged);
    for binding in [SceneBinding::Ambiguous {}, SceneBinding::Unbound {}] {
        assert!(serde_json::to_value(binding)
            .unwrap()
            .get("occurrence")
            .is_none());
    }
    assert!(serde_json::from_value::<SceneBinding>(
        serde_json::json!({"state":"ambiguous","occurrence":"f3d:test:occurrence#a"})
    )
    .is_err());
}

#[test]
fn scene_absence_and_empty_scene_produce_empty_arena_without_loss() {
    let absent = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let empty = crate::test_support::scene::archive(b"WorldSerializerEndMark");
    for bytes in [absent, empty] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, view).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut losses = Vec::new();
        assert!(!collect(&ctx, &scan, &mut ir, &mut losses).unwrap());
        let rows: Vec<DisplayPose> = ir
            .native
            .namespace("f3d")
            .unwrap()
            .arena_as_for_decode(&ctx, "display_poses")
            .unwrap();
        assert!(rows.is_empty());
        assert!(losses.is_empty());
    }
}

#[test]
fn scene_column_major_rotation_has_a_positive_control() {
    let mut bytes = encoded(&[3, 17]);
    let matrix = COUNTED_NAME.len() + 5;
    for (slot, value) in [(0, 0_f32), (1, 1.), (4, -1.), (5, 0.)] {
        bytes[matrix + slot * 4..matrix + slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let (rows, losses) = decode(&bytes);
    assert!(losses.is_empty());
    assert_eq!(
        rows[0].transform.0,
        [
            [0., -1., 0., 20.],
            [1., 0., 0., 30.],
            [0., 0., 1., 40.],
            [0., 0., 0., 1.]
        ]
    );
}

#[test]
fn scene_native_validation_shares_matrix_and_path_gates() {
    for count in [1, 3, 1024] {
        assert!(ScenePath::try_from(vec![3; count]).is_err());
    }
    assert!(ScenePath::try_from(vec![]).is_ok());
    let (rows, _) = decode(&encoded(&[3, 17]));
    let mut wire = serde_json::to_value(&rows[0]).unwrap();
    wire["path"] = serde_json::json!([3]);
    assert!(serde_json::from_value::<DisplayPose>(wire).is_err());
    let mut wire = serde_json::to_value(&rows[0]).unwrap();
    wire["transform"][0][0] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<DisplayPose>(wire).is_err());
}

#[test]
fn scene_native_validation_checks_bound_occurrence_identity() {
    let (mut rows, _) = decode(&encoded(&[3, 17]));
    let model = cadmpeg_ir::document::Model::default();
    assert!(validate(&rows, &model).is_empty());
    rows[0].binding = SceneBinding::Ambiguous {};
    assert!(validate(&rows, &model).is_empty());
    rows[0].binding = SceneBinding::Bound {
        occurrence: OccurrenceId::mint("f3d:test:occurrence#missing").unwrap(),
    };
    let findings = validate(&rows, &model);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
}

#[test]
fn scene_offsets_are_relative_to_the_retained_world_window() {
    let mut bytes = vec![99; 11];
    bytes.extend(encoded(&[3, 17]));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let world = view.child(11, bytes.len()).unwrap();
    let mut losses = Vec::new();
    let rows = read(&ctx, world, "world", &mut losses).unwrap();
    assert_eq!(rows[0].byte_offset, 0);
    assert!(losses.is_empty());
}

#[test]
fn scene_inserted_synthetic_root_shares_placement_path_and_child_uses_member_definition() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.occurrences = vec![
        occurrence("f3d:test:occurrence#holder", None, Some("xref")),
        occurrence(
            "f3d:test:occurrence#member",
            Some("f3d:test:occurrence#holder"),
            None,
        ),
        occurrence(
            "f3d:test:occurrence#child",
            Some("f3d:test:occurrence#member"),
            Some("carrier"),
        ),
    ];
    let sources = Sources::from([
        ("xref".to_owned(), [3, 4533]),
        ("carrier".to_owned(), [5, 991]),
    ]);
    let mut world = encoded(&[3, 4533]);
    world.extend(encoded(&[3, 4533, 5, 991]));
    let (rows, _) = decode(&world);
    ir.native
        .namespace_mut("f3d")
        .set_arena(&ctx, "display_poses", &rows)
        .unwrap();
    let mut losses = Vec::new();
    bind(&ctx, &mut ir, &sources, &mut losses).unwrap();
    let rows: Vec<DisplayPose> = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as_for_decode(&ctx, "display_poses")
        .unwrap();
    assert_eq!(rows[0].binding, SceneBinding::Ambiguous {});
    assert_eq!(
        rows[1].binding,
        SceneBinding::Bound {
            occurrence: OccurrenceId::mint("f3d:test:occurrence#child").unwrap()
        }
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::SceneOverrideAmbiguous.kind());
}

#[test]
fn scene_pointer_text_checks_hexadecimal_forms_and_code_unit_bound() {
    for (pointer, admitted) in [
        ("aBc012".to_owned(), true),
        ("0xABCDEF".to_owned(), true),
        ("f".repeat(128), true),
        (String::new(), false),
        ("0x".to_owned(), false),
        ("g".to_owned(), false),
        ("0XABC".to_owned(), false),
        ("f".repeat(129), false),
    ] {
        let mut bytes = encoded(&[]);
        bytes.truncate(prefix::LEN);
        bytes.extend(u32::try_from(pointer.len()).unwrap().to_le_bytes());
        for unit in pointer.encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes.extend(2_u32.to_le_bytes());
        for index in [3_u64, 17] {
            bytes.extend(0_u32.to_be_bytes());
            bytes.extend(index.to_be_bytes());
        }
        let (rows, losses) = decode(&bytes);
        assert_eq!(rows.len(), usize::from(admitted));
        assert_eq!(losses.len(), usize::from(!admitted));
    }
}

#[test]
fn scene_container_depth_preserves_absent_native_namespace() {
    let bytes = crate::test_support::scene::archive(&encoded(&[3, 17]));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, view) = DecodeContext::read_root(
        &mut std::io::Cursor::new(bytes),
        &arena,
        &policy,
        cadmpeg_core::decode::DecodeDepth::Container,
    )
    .unwrap();
    let scan = crate::container::scan(&ctx, view).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut losses = Vec::new();
    assert!(!collect(&ctx, &scan, &mut ir, &mut losses).unwrap());
    assert!(ir.native.namespace("f3d").is_none());
    assert!(losses.is_empty());
}

#[test]
fn scene_tagged_pair_suffix_is_admitted_and_stays_unbound() {
    let mut world = crate::test_support::scene::override_row_tagged(&[3, 17, 5, 9], 2);
    world.extend(crate::test_support::scene::override_row(&[3, 17]));
    let (rows, losses) = decode(&world);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].tagged_from, Some(2));
    assert_eq!(rows[0].binding, SceneBinding::Unbound {});
    assert_eq!(rows[1].tagged_from, None);
    assert!(!losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SceneOverrideRefused.kind()));
}

#[test]
fn scene_tagged_elements_must_be_a_suffix_at_a_pair_boundary() {
    let mut odd = crate::test_support::scene::override_row_tagged(&[3, 17, 5, 9], 1);
    odd.extend(crate::test_support::scene::override_row_tagged(
        &[3, 17, 5, 9],
        0,
    ));
    let mut interrupted = crate::test_support::scene::override_row_tagged(&[3, 17, 5, 9], 2);
    let last_high_word = interrupted.len() - 12;
    interrupted[last_high_word] = 0;
    odd.extend(interrupted);
    let (rows, losses) = decode(&odd);
    assert!(rows.is_empty());
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::SceneOverrideRefused.kind())
            .count(),
        1
    );
}
