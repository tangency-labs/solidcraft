// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::records::xref::{XrefOperationPose, XrefPlacementSource};
use crate::test_support::local_reference;

fn frame(prefix: usize, explicit: bool) -> Vec<u8> {
    let mut body = 3_u32.to_le_bytes().to_vec();
    body.extend(b"318");
    body.extend(20_u64.to_le_bytes());
    body.extend(0_u32.to_le_bytes());
    body.extend(std::iter::repeat_n(0, prefix));
    body.extend(0_u32.to_le_bytes());
    body.extend(1_u32.to_le_bytes());
    body.extend(local_reference(10));
    body.extend(local_reference(3));
    body.extend(0_u32.to_le_bytes());
    body.extend(1_u32.to_le_bytes());
    body.extend([1, 0]);
    body.extend(local_reference(30));
    body.push(u8::from(!explicit));
    if explicit {
        let mut matrix = IDENTITY_ROWS;
        matrix[0][3] = 1.0;
        for value in matrix.into_iter().flatten() {
            body.extend(value.to_le_bytes());
        }
    }
    body
}

fn reference() -> XrefReference {
    XrefReference {
        id: "f3d:xref:reference#0-occurrence-0".into(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: Some(XrefPlacementSource {
            frame_bytes: Vec::new().into(),
            occurrence_guid: None,
            stream: "f3d:Design/BulkStream.dat".into(),
            carrier_record_index: 10,
            owner: None,
        }),
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: "role".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
        current_pose: None,
        operation_pose: None,
        contained_poses: Vec::new(),
        owned_poses: Vec::new(),
    }
}

#[test]
fn operation_pose_gate_and_validators_accept_both_prologues_and_matrix_forms() {
    for prefix in [2, 6] {
        for explicit in [false, true] {
            let body = frame(prefix, explicit);
            let pose = parse(&body).unwrap();
            let matrix = pose.matrix;
            assert_eq!(pose.target_record_index, 10);
            assert_eq!(matrix[0][3], if explicit { 1.0 } else { 0.0 });
            let mut reference = reference();
            reference.operation_pose = Some(XrefOperationPose {
                carrier_record_index: 20,
                frame_bytes: body.clone().into(),
                transform: Some(matrix.try_into().unwrap()),
            });
            assert!(reference_matches(&reference));
            let mut local = DesignComponentOccurrence::try_new(crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                id: "local".into(), class_tag: "318".to_owned().try_into().unwrap(), record_index: 10, byte_offset: 0,
                component_record_index: 3, component_guid: "00000000-0000-0000-0000-000000000003".to_owned().try_into().unwrap(),
                occurrence_guid: "00000000-0000-0000-0000-000000000010".to_owned().try_into().unwrap(),
                placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base, owner_entity: 4,
            }).unwrap();
            local.operation_pose = reference.operation_pose.clone();
            assert!(local_matches(&local));
            for end in 0..body.len() {
                let truncated = &body[..end];
                assert!(parse(truncated).is_none());
                reference.operation_pose.as_mut().unwrap().frame_bytes = truncated.to_vec().into();
                local.operation_pose = reference.operation_pose.clone();
                assert!(!reference_matches(&reference));
                assert!(!local_matches(&local));
            }
            reference.operation_pose.as_mut().unwrap().frame_bytes = body.into();
            reference
                .placement_source
                .as_mut()
                .unwrap()
                .carrier_record_index = 11;
            assert!(!reference_matches(&reference));
        }
    }
}

#[test]
fn operation_pose_refuses_cursor_flags_nonrigid_and_tail() {
    let body = frame(6, true);
    let mut bad_tag = body.clone();
    bad_tag[4..7].copy_from_slice(b"abc");
    assert!(parse(&bad_tag).is_none());
    let matrix_at = body.len() - 128;
    let cursor_at = matrix_at - 22;
    for (at, value) in [(cursor_at, 1), (matrix_at - 14, 2), (matrix_at - 1, 2)] {
        let mut bad = body.clone();
        bad[at] = value;
        assert!(parse(&bad).is_none());
    }
    let mut bad = body.clone();
    bad[matrix_at..matrix_at + 8].copy_from_slice(&2.0_f64.to_le_bytes());
    assert!(parse(&bad).is_none());
    let mut bad = body;
    bad.push(0);
    assert!(parse(&bad).is_none());
}

fn model() -> (cadmpeg_ir::CadIr, BTreeMap<String, OperationPose>) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let reference = reference();
    let key = reference.id.clone();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.native
        .namespace_mut("f3d")
        .set_arena(&ctx, "xref_references", &[reference])
        .unwrap();
    for i in 0..2 {
        ir.model.occurrences.push(Occurrence {
            id: format!("f3d:model:occurrence#{i}").try_into().unwrap(),
            prototype: cadmpeg_ir::products::PrototypeReference::Local {
                definition: "f3d:model:definition#test".to_owned().try_into().unwrap(),
            },
            parent: OccurrenceParent::Root {},
            ordinal: i,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(key.clone()),
        });
    }
    let body = frame(6, true);
    let matrix = parse(&body).unwrap().matrix;
    (
        ir,
        BTreeMap::from([(
            key,
            OperationPose {
                matrix,
                frame: body,
                carrier: 20,
                source_root_record_index: 3,
            },
        )]),
    )
}

#[test]
fn operation_pose_updates_every_instance_of_one_definition() {
    let (mut ir, poses) = model();
    let mut losses = Vec::new();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        project(&ctx, &mut ir, &poses, &BTreeSet::new(), &mut losses).unwrap(),
        2
    );
    for occurrence in &ir.model.occurrences {
        assert_eq!(occurrence.transform.rows()[0][3], 10.0);
    }
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::OperationPoseApplied.kind()));
    let refs = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<XrefReference>("xref_references")
        .unwrap();
    assert!(reference_matches(&refs[0]));
    assert!(refs[0].current_pose.is_none());
    assert!(refs[0].operation_pose.is_some());
}

#[test]
fn operation_pose_conflict_keeps_every_instance_native() {
    let (mut ir, poses) = model();
    let before = ir.model.occurrences.clone();
    let protected = BTreeSet::from([ir.model.occurrences[0].id.clone()]);
    let mut losses = Vec::new();
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        project(&ctx, &mut ir, &poses, &protected, &mut losses).unwrap(),
        0
    );
    assert_eq!(ir.model.occurrences, before);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::OperationPoseDisagrees.kind()));
    ir.model.occurrences.clear();
    losses.clear();
    project(&ctx, &mut ir, &poses, &BTreeSet::new(), &mut losses).unwrap();
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::OperationPoseNative.kind()));
}

#[test]
fn operation_pose_cannot_move_a_carrier_selected_descendant() {
    let (mut ir, poses) = model();
    let mut child = ir.model.occurrences[0].clone();
    child.id = "f3d:model:occurrence#selected-child".try_into().unwrap();
    child.native_ref = None;
    child.parent = OccurrenceParent::Occurrence {
        occurrence: ir.model.occurrences[0].id.clone(),
    };
    let protected = BTreeSet::from([child.id.clone()]);
    ir.model.occurrences.push(child);
    let before = ir.model.occurrences.clone();
    let mut losses = Vec::new();
    project(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
        &poses,
        &protected,
        &mut losses,
    )
    .unwrap();
    assert_eq!(ir.model.occurrences, before);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::OperationPoseDisagrees.kind()));
}

#[test]
fn operation_pose_registration_refuses_other_versions_and_duplicate_targets() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let stream = "Design/BulkStream.dat";
    for version in [None, Some(2), Some(3), Some(4)] {
        let mut table = OperationTables::default();
        register(&ctx, &mut table, stream, &frame(6, true), version).unwrap();
        assert!(table.poses.is_empty());
        assert_eq!(table.native, 1);
    }
    for version in [0, 1] {
        let mut table = OperationTables::default();
        let prefix = if version == 0 { 2 } else { 6 };
        register(
            &ctx,
            &mut table,
            stream,
            &frame(prefix, true),
            Some(version),
        )
        .unwrap();
        assert_eq!(table.poses.values().filter(|p| p.is_some()).count(), 1);
        register(
            &ctx,
            &mut table,
            stream,
            &frame(prefix, false),
            Some(version),
        )
        .unwrap();
        assert!(table.poses.values().all(Option::is_none));
        assert_eq!(table.native, 2);
    }
}

#[test]
fn operation_pose_uses_one_matrix_even_inside_the_agreement_tolerance() {
    let (mut ir, poses) = model();
    for (i, o) in ir.model.occurrences.iter_mut().enumerate() {
        o.transform = Transform::affine([
            [1.0, 0.0, 0.0, if i == 0 { 10.0 } else { 10.0 + 1.0e-13 }],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
    }
    let mut losses = Vec::new();
    assert_eq!(
        project(
            &cadmpeg_test_support::service_decode_context(),
            &mut ir,
            &poses,
            &BTreeSet::new(),
            &mut losses
        )
        .unwrap(),
        0
    );
    assert_eq!(
        ir.model.occurrences[0].transform,
        ir.model.occurrences[1].transform
    );
    assert_eq!(ir.model.occurrences[0].transform.rows()[0][3], 10.0);
}

#[test]
fn operation_pose_reads_typed_local_tables_without_a_redirections_entry() {
    use std::io::{Cursor, Write};
    for version in [0, 3, 4] {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, options);
        let mut body = frame(2, true);
        body[4..7].copy_from_slice(if version == 0 { b"256" } else { b"bad" });
        let meta = crate::test_support::streams_test::design_metastream_with_records(
            &[(TYPE_GUID, "", version, "Fusion", &[20])],
            &[(20, 0)],
        );
        for (name, bytes) in [("MetaStream.dat", meta), ("BulkStream.dat", body)] {
            zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), options)
                .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let table = read(&ctx, &scan).unwrap();
        assert_eq!(table.poses.len(), usize::from(version == 0));
        assert_eq!(table.native, usize::from(version != 0));
        if version == 0 {
            assert_eq!(
                table
                    .poses
                    .values()
                    .next()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .matrix[0][3],
                1.0
            );
        }
    }
}

#[test]
fn operation_pose_requires_the_selected_source_root_frame() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let stream = "Design/BulkStream.dat";
    let scope = crate::ids::native_scope(stream);
    for root in [Some(3), Some(4), None] {
        let mut table = OperationTables::default();
        register(&ctx, &mut table, stream, &frame(2, true), Some(0)).unwrap();
        let roots = root
            .map(|root| BTreeMap::from([(scope.clone(), root)]))
            .unwrap_or_default();
        let mut losses = Vec::new();
        bind_source_roots(&mut table, &roots, &mut losses);
        assert_eq!(table.poses.values().any(Option::is_some), root == Some(3));
        assert_eq!(losses.is_empty(), root == Some(3));
        assert!(losses
            .iter()
            .all(|loss| loss.code == F3dLossCode::OperationPoseNative.kind()));
    }
}

#[test]
fn operation_pose_does_not_claim_a_solved_world_pose() {
    let (mut ir, poses) = model();
    let ctx = cadmpeg_test_support::service_decode_context();
    project(&ctx, &mut ir, &poses, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let mut refs = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<XrefReference>("xref_references")
        .unwrap();
    let definition = refs[0].operation_pose.clone().unwrap();
    assert!(refs[0].current_pose.is_none());
    assert!(reference_matches(&refs[0]));
    refs[0].current_pose = Some(crate::records::xref::XrefCurrentPose {
        carrier_record_index: definition.carrier_record_index,
        frame_bytes: definition.frame_bytes,
        transform: definition.transform,
    });
    assert!(!crate::xref::current_pose_matches(&refs[0]));
}

#[test]
fn operation_pose_keeps_an_agreeing_carrier_constraint_and_its_native_evidence() {
    let (mut ir, poses) = model();
    for occurrence in &mut ir.model.occurrences {
        occurrence.transform =
            crate::design::components::neutral_transform(poses.values().next().unwrap().matrix)
                .unwrap();
    }
    let protected = BTreeSet::from([ir.model.occurrences[0].id.clone()]);
    let before = ir.model.occurrences.clone();
    let mut losses = Vec::new();
    project(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
        &poses,
        &protected,
        &mut losses,
    )
    .unwrap();
    assert_eq!(ir.model.occurrences, before);
    let refs = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<XrefReference>("xref_references")
        .unwrap();
    assert!(refs[0].operation_pose.is_some());
    assert!(losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::OperationPoseDisagrees.kind()));
}

#[test]
fn operation_pose_conflict_preserves_an_independent_sibling_pose() {
    let (mut ir, mut poses) = model();
    let mut sibling = ir.model.occurrences[0].clone();
    sibling.id = "f3d:model:occurrence#sibling".try_into().unwrap();
    let key = "f3d:xref:reference#1-occurrence-0".to_owned();
    sibling.native_ref = Some(key.clone());
    ir.model.occurrences.push(sibling);
    let pose = poses.values().next().unwrap().clone();
    poses.insert(key.clone(), pose);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut refs = ir
        .native
        .namespace("f3d")
        .unwrap()
        .arena_as::<XrefReference>("xref_references")
        .unwrap();
    let mut reference = refs[0].clone();
    reference.id = key;
    refs.push(reference);
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        native.xref_references = refs;
    });
    let protected = BTreeSet::from([ir.model.occurrences[0].id.clone()]);
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &poses, &protected, &mut losses).unwrap();
    assert_eq!(ir.model.occurrences[0].transform, Transform::identity());
    assert_eq!(ir.model.occurrences[1].transform, Transform::identity());
    assert!((ir.model.occurrences[2].transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::OperationPoseDisagrees.kind()));
}
