// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::decode::scopes::direct_face::exact_direct_face_operation;
use crate::design::decode::scopes::fixed_parameters::exact_fixed_extrude_parameters;
use crate::design::decode::scopes::work_geometry::{
    exact_work_axis_construction, exact_work_plane_frame,
};
use crate::design::test_support::{parameter_owner_frame, parameter_record};
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::work_plane_legacy_337_matrix_frame as work_plane_337;
use crate::layout::work_plane_legacy_class_322_332_matrix_frame as work_plane_class_322_332;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::lp_utf16;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::{Feature, FeatureDefinition};

#[test]
fn class_369_shell_scope_uses_ordered_scalar_and_body_group() {
    let mut frame = vec![0; shell_369_261::LEN];
    frame[shell_369_261::FEATURE_FORM] = shell_369_261::FEATURE_FORM_VALUE;
    frame[shell_369_261::OUTWARD] = 0;
    frame[shell_369_261::SCALAR_MARKER] = shell_369_261::SCALAR_MARKER_VALUE;
    frame[shell_369_261::GROUP_FORM] = shell_369_261::GROUP_FORM_VALUE;
    frame[shell_369_261::GUID_CODE_UNIT_COUNT..shell_369_261::GUID]
        .copy_from_slice(&shell_369_261::GUID_CODE_UNIT_COUNT_VALUE.to_le_bytes());
    let mut guid = Vec::new();
    lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
    frame[shell_369_261::GUID..shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES]
        .copy_from_slice(&guid[4..]);
    frame[shell_369_261::REFERENCE_COUNT..shell_369_261::REFERENCE_ENTRY_0]
        .copy_from_slice(&shell_369_261::REFERENCE_COUNT_VALUE.to_le_bytes());
    for (offset, record_index) in [
        (shell_369_261::SCALAR_REFERENCE, 9_000u32),
        (shell_369_261::GROUP_REFERENCE, 200u32),
        (shell_369_261::REFERENCE_ENTRY_2, 201u32),
    ] {
        frame[offset] = 1;
        frame[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    frame[shell_369_261::HISTORY_STATE_ID..shell_369_261::KIND_CODE_UNIT_COUNT]
        .copy_from_slice(&8_323u32.to_le_bytes());
    frame[shell_369_261::KIND_CODE_UNIT_COUNT..shell_369_261::KIND]
        .copy_from_slice(&shell_369_261::KIND_CODE_UNIT_COUNT_VALUE.to_le_bytes());
    let mut kind = Vec::new();
    lp_utf16(&mut kind, "Shell");
    frame[shell_369_261::KIND..shell_369_261::FEATURE_ORDINAL].copy_from_slice(&kind[4..]);
    frame[shell_369_261::FEATURE_ORDINAL..shell_369_261::FEATURE_ORDINAL + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    frame[shell_369_261::PREVIOUS_HISTORY_STATE_ID..shell_369_261::PREVIOUS_HISTORY_STATE_ID + 4]
        .copy_from_slice(&8_322u32.to_le_bytes());

    let mut bytes = frame;
    let scalar_start = bytes.len();
    let mut scalar = vec![0; 105];
    scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    scalar[4..7].copy_from_slice(b"321");
    scalar[7..11].copy_from_slice(&9_000u32.to_le_bytes());
    scalar[24] = 1;
    scalar[25..29].copy_from_slice(&42u32.to_le_bytes());
    scalar[40..48].copy_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&scalar);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"265");
    bytes.extend_from_slice(&9_000u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:test:shell-369#42",
        crate::records::feature::scope::DesignFeatureKind::Shell,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("369".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(shell_369_261::LEN);
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![9_000, 200, 201]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert!(matches!(
        exact_direct_face_operation(&bytes, &records, &scope),
        Some(DesignDirectFaceOperation::Shell(crate::records::feature::direct_face::DesignShellOperation {
            thickness,
            thickness_record_index: 9_000,
            outward: false,
            thickness_offset,
            outward_offset: 21,
            wall_slots: None,
        })) if thickness.get() == 0.25 && thickness_offset == u64_from_index(scalar_start + 40)
    ));

    let mut wrong_pair = scope.clone();
    wrong_pair.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    assert!(exact_direct_face_operation(&bytes, &records, &wrong_pair).is_none());

    let mut invalid_outward = bytes;
    invalid_outward[shell_369_261::OUTWARD] = 2;
    assert!(exact_direct_face_operation(&invalid_outward, &records, &scope).is_none());
}

#[test]
fn class_322_261_work_plane_332_byte_frame_decodes_its_matrix_only_for_that_pair() {
    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = vec![0; work_plane_class_322_332::LEN];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"322");
    bytes[7..11].copy_from_slice(&85u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = work_plane_class_322_332::MATRIX + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&85u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#322",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![85]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-322/261 WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(work_plane_class_322_332::MATRIX)
    );
    assert_eq!(decoded.reference, None);

    let mut wrong_pair = bytes;
    wrong_pair[work_plane_class_322_332::LEN + 4..work_plane_class_322_332::LEN + 7]
        .copy_from_slice(b"262");
    assert_eq!(
        exact_work_plane_frame(
            &wrong_pair,
            &crate::design::test_support::indexed_record_offsets_for_test(&wrong_pair),
            &scope,
        ),
        None
    );
}

#[test]
fn legacy_work_plane_class_350_frame_decodes_its_matrix() {
    const EPS_WORK_PLANE_CLASS_350_TEST_VALUE: f64 = 1.0e-12;

    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 4.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, -1.5],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = vec![0; work_plane_337::LEN];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"350");
    bytes[7..11].copy_from_slice(&76u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = work_plane_337::MATRIX + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&76u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#1",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![76]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-350 WorkPlane frame");
    for (actual_row, expected_row) in decoded.transform.iter().zip(transform.iter()) {
        for (actual, expected) in actual_row.iter().zip(expected_row.iter()) {
            assert!((actual - expected).abs() < EPS_WORK_PLANE_CLASS_350_TEST_VALUE);
        }
    }
    assert_eq!(
        decoded.transform_offset,
        u64_from_index(work_plane_337::MATRIX)
    );
    assert_eq!(decoded.reference, None);
}

#[test]
fn legacy_work_plane_class_400_frame_decodes_its_matrix() {
    let transform: [[f64; 4]; 4] = [
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0, -2.25],
        [-1.0, 0.0, 0.0, 0.75],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = vec![0; 345];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"400");
    bytes[7..11].copy_from_slice(&72u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&72u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#1",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![72]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let decoded = exact_work_plane_frame(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
    )
    .expect("class-400 WorkPlane frame");
    assert_eq!(decoded.transform, transform.try_into().unwrap());
    assert_eq!(decoded.transform_offset, 49);
    assert_eq!(decoded.reference, None);
}

#[test]
fn legacy_move_transform_classes_use_the_shared_253_byte_envelope() {
    let mut bytes = Vec::new();
    let classes: [(&str, u32); 6] = [
        ("393", 5),
        ("293", 1),
        ("451", 5),
        ("442", 5),
        ("447", 5),
        ("456", 5),
    ];

    for (ordinal, (class_tag, form)) in classes.into_iter().enumerate() {
        let record_index = 900 + u32::try_from(ordinal).expect("small test ordinal");
        let frame_at = bytes.len();
        let mut frame = vec![0; 253];
        frame[0..4].copy_from_slice(&3u32.to_le_bytes());
        frame[4..7].copy_from_slice(class_tag.as_bytes());
        frame[7..11].copy_from_slice(&record_index.to_le_bytes());
        frame[43..47].copy_from_slice(&form.to_le_bytes());
        let mut transform =
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
        transform[0][3] = f64::from(u32::try_from(ordinal).expect("fixture value fits u32"));
        for (cell, value) in transform.into_iter().flatten().enumerate() {
            let at = 48 + cell * 8;
            frame[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&frame);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(match class_tag {
            "447" => b"263",
            "456" => b"258",
            _ => b"262",
        });
        bytes.extend_from_slice(&record_index.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            &format!("f3d:test:legacy-move#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::Move,
            1_000 + u32::try_from(ordinal).expect("small test ordinal"),
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![record_index]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let decoded = crate::design::decode::scopes::direct_face::exact_move_operation(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .expect("legacy Move transform frame");

        assert_eq!(decoded.transform, transform.try_into().unwrap());
        assert_eq!(decoded.transform_record_index, record_index);
        assert_eq!(u32::from(decoded.form), form);
        assert_eq!(decoded.form_offset, u64_from_index(frame_at + 43));
        assert_eq!(decoded.transform_offset, u64_from_index(frame_at + 48));

        if class_tag == "456" {
            let paired_class_at = frame_at + 253 + 4;
            bytes[paired_class_at..paired_class_at + 3].copy_from_slice(b"262");
            assert!(
                crate::design::decode::scopes::direct_face::exact_move_operation(
                    &bytes,
                    &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                    &scope,
                )
                .is_none(),
                "class-456 Move requires paired class 258"
            );
        }
    }
}

#[test]
fn direct_work_axis_carriers_project_both_admitted_generations() {
    struct Case {
        scope: (&'static str, &'static str),
        carrier: (&'static str, &'static str),
        lengths: (usize, usize),
        values: [f64; 8],
    }
    let cases = [
        Case {
            scope: ("302", "262"),
            carrier: ("297", "306"),
            lengths: (268, 215),
            values: [1.5, 2.5, 3.5, 0.0, -3.0, 4.0, 0.0, 0.0],
        },
        Case {
            scope: ("361", "258"),
            carrier: ("335", "349"),
            lengths: (254, 195),
            values: [4.0, 5.0, 6.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        },
    ];

    for Case {
        scope: (scope_class, scope_paired_class),
        carrier: (carrier_class, support_class),
        lengths: (scope_length, carrier_length),
        values,
    } in cases
    {
        let carrier_record_index: u32 = 100;
        let support_record_index: u32 = 200;
        let mut bytes = vec![0; carrier_length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(carrier_class.as_bytes());
        bytes[7..11].copy_from_slice(&carrier_record_index.to_le_bytes());
        bytes[21..25].copy_from_slice(&8u32.to_le_bytes());
        for (ordinal, value) in values.into_iter().enumerate() {
            let at = 25 + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[89..93].copy_from_slice(&6u32.to_le_bytes());
        bytes[93..97].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(scope_paired_class.as_bytes());
        bytes.extend_from_slice(&carrier_record_index.to_le_bytes());

        let support_start = bytes.len();
        bytes.resize(support_start + 293, 0);
        bytes[support_start..support_start + 4].copy_from_slice(&3u32.to_le_bytes());
        bytes[support_start + 4..support_start + 7].copy_from_slice(support_class.as_bytes());
        bytes[support_start + 7..support_start + 11]
            .copy_from_slice(&support_record_index.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(scope_paired_class.as_bytes());
        bytes.extend_from_slice(&support_record_index.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            "f3d:test/BulkStream.dat:work-axis#1",
            crate::records::feature::scope::DesignFeatureKind::WorkAxis,
            1,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from(scope_class.to_owned()).unwrap();
        scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from(scope_paired_class.to_owned())
                .unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(scope_length);
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                    carrier_record_index,
                    support_record_index,
                ]);
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let construction = exact_work_axis_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
        )
        .expect("direct WorkAxis carrier");
        assert_eq!(construction.origin_offset, 25);
        assert_eq!(construction.displacement_offset, 49);
        assert!(matches!(
            construction.source,
            Some(
                crate::records::feature::work_geometry::DesignWorkAxisSource::DirectCarrier {
                    carrier_record_index: 100,
                    support_record_index: 200,
                }
            )
        ));
        if let crate::records::feature::scope::DesignScopePayloadMut::WorkAxis(slot) =
            scope.payload_mut()
        {
            *slot = Some(construction);
        }
        let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
            crate::test_support::with_decode_context(|ctx| {
                let scopes = std::slice::from_ref(&scope);
                let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    ctx,
                    &crate::design::feature_project::ProjectInputs {
                        scopes,
                        timelines: &timelines,
                        ..Default::default()
                    },
                )
                .expect("test projection has a synthetic exact timeline")
            });
        assert!(matches!(
            features.as_slice(), [Feature {
                evaluation,
                ..
            }] if matches!((evaluation.definition(),), (FeatureDefinition::Operation(FeatureOperation::DatumAxis { .. }),))));
    }
}

#[test]
fn fixed_extrude_owners_follow_parameter_source_kind_before_lane_ordinal() {
    let scope_record_index: u32 = 12;
    let mut bytes = Vec::new();
    let append_scalar = |bytes: &mut Vec<u8>, record_index: u32, ordinal: u8, value: f64| {
        let start = bytes.len();
        let mut frame = vec![0; 104];
        frame[0..4].copy_from_slice(&3u32.to_le_bytes());
        frame[4..7].copy_from_slice(b"277");
        frame[7..11].copy_from_slice(&record_index.to_le_bytes());
        frame[24] = 1;
        frame[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
        frame[35] = ordinal;
        frame[40..48].copy_from_slice(&value.to_le_bytes());
        frame.extend_from_slice(&3u32.to_le_bytes());
        frame.extend_from_slice(b"261");
        frame.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&frame);
        start
    };
    let taper_start = append_scalar(&mut bytes, 80, 0, -0.013_962_634_015_954_637);
    let along_start = append_scalar(&mut bytes, 82, 1, -2.5);

    let mut taper_parameter = parse_design_parameter_record(&parameter_record(
        Some(80),
        "taper",
        "TaperAngle",
        Some("rad"),
        "taper",
        -0.013_962_634_015_954_637,
    ))
    .expect("taper parameter");
    taper_parameter.id = "generated:parameter#81".into();
    taper_parameter.record_index = 81;
    let mut along_parameter = parse_design_parameter_record(&parameter_record(
        Some(82),
        "along",
        "AlongDistance",
        Some("cm"),
        "along",
        -2.5,
    ))
    .expect("along parameter");
    along_parameter.id = "generated:parameter#83".into();
    along_parameter.record_index = 83;

    let mut taper_owner = parse_parameter_owner(&parameter_owner_frame())
        .expect("taper owner")
        .into_record("Design/BulkStream.dat", 0)
        .unwrap();
    {
        let mut wire =
            crate::records::parameters::DesignParameterOwnerWire::from(taper_owner.clone());
        wire.id = "generated:owner#80".into();
        wire.record_index = 80;
        wire.scope_record_index = scope_record_index;
        wire.local_ordinal = 0;
        wire.parameter_record_index = 81;
        wire.companion_record_index = 82;
        taper_owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }
    let mut along_owner = taper_owner.clone();
    {
        let mut wire =
            crate::records::parameters::DesignParameterOwnerWire::from(along_owner.clone());
        wire.id = "generated:owner#82".into();
        wire.record_index = 82;
        wire.local_ordinal = 1;
        wire.parameter_record_index = 83;
        wire.companion_record_index = 84;
        along_owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }

    let mut scope = DesignParameterScope::empty(
        "generated:scope#12",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![80, 82]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 28,
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [1, 0],
                side_extent_discriminator_offsets: [77, 90],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::OneSidedDistance,
                direction_face_extend_offsets: [32, 36],
                direction_reversed: false,
                direction_reversed_offset: 40,
                solid_operation: true,
                solid_operation_offset: 41,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 42,
            });
    }

    let fixed = exact_fixed_extrude_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[taper_parameter, along_parameter],
        &[taper_owner, along_owner],
    )
    .expect("fixed owner lanes");
    assert!(matches!(
        fixed.along_distance,
        Some(DesignFixedExtrudeDistance::FixedScalar(DesignFixedExtrudeScalar {
            value,
            record_index: 82,
            value_offset,
        })) if value.get() == -2.5 && value_offset == u64_from_index(along_start + 40)
    ));
    assert!(matches!(
        fixed.taper_angle,
        Some(DesignFixedExtrudeScalar {
            value,
            record_index: 80,
            value_offset,
        }) if value.get() == -0.013_962_634_015_954_637 && value_offset == u64_from_index(taper_start + 40)
    ));
}
