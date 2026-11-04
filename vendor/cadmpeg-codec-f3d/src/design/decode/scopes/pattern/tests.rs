// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::exact_pattern_identity_wrapper;
use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment;
use crate::design::decode::scopes::axial_assembly::bind_joint_origin_frames_from_assemblies;
use crate::design::decode::scopes::pattern::select_circular_pattern_axis;
use crate::design::test_support::assembly_operand_frame_fixture;
use crate::records::feature::patterns::DesignCircularPatternConstruction;
use crate::records::feature::patterns::RectangularPatternSearchCap;
use crate::records::feature::patterns::{
    MAX_RECTANGULAR_PATTERN_INSTANCES, MAX_RECTANGULAR_PATTERN_RECORD_BYTES,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use crate::test_support::push_marked_reference;

fn tested_circular_pattern_construction_with_owners(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignCircularPatternConstruction> {
    super::exact_circular_pattern_construction_with_owners(bytes, records, scope, parameter_owners)
        .unwrap()
}

fn tested_rectangular_pattern_construction(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<crate::records::feature::patterns::DesignRectangularPatternConstruction> {
    super::exact_rectangular_pattern_construction(bytes, records, scope, parameter_owners).unwrap()
}

fn tested_rectangular_pattern_instances(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    construction: &crate::records::feature::patterns::DesignRectangularPatternConstruction,
) -> Option<crate::records::feature::patterns::DesignRectangularPatternInstances> {
    match super::exact_rectangular_pattern_instances(bytes, records, scope, construction) {
        super::InstanceSearch::Resolved(instances) => Some(*instances),
        super::InstanceSearch::Unresolved | super::InstanceSearch::Capped(_) => None,
    }
}

#[test]
fn circular_pattern_identity_wrapper_closes_on_its_persistent_identity() {
    let mut bytes = Vec::new();
    let record_index = 80;
    indexed_header(&mut bytes, *b"308", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&40u64.to_le_bytes());
    lp_utf16(&mut bytes, "384d79a0-c23e-42aa-b993-74df1f8dfcae");
    lp_utf16(&mut bytes, "352c47d7-42ba-443e-9de1-ae0e37cc129d");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    push_marked_reference(&mut bytes, record_index + 1);
    indexed_header(&mut bytes, *b"305", record_index + 1);
    bytes.extend_from_slice(&[0; 10]);
    push_marked_reference(&mut bytes, record_index + 2);
    indexed_header(&mut bytes, *b"300", record_index + 2);
    bytes.extend_from_slice(&[0; 10]);
    let identity_offset = bytes.len();
    bytes.extend_from_slice(&503u64.to_le_bytes());
    indexed_header(&mut bytes, *b"308", record_index + 3);

    assert_eq!(
        exact_pattern_identity_wrapper(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            record_index,
        ),
        Some((503, u64_from_index(identity_offset)))
    );
    bytes[identity_offset - 1] = 1;
    assert_eq!(
        exact_pattern_identity_wrapper(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            record_index,
        ),
        None
    );
}

#[test]
fn circular_pattern_axis_prefers_one_inline_carrier() {
    use crate::design::decode::scopes::pattern::CircularPatternAxisCandidate;
    use crate::records::feature::patterns::DesignCircularPatternAxis;

    let historical = DesignCircularPatternAxis::HistoricalEdge {
        wrappers: vec![
            crate::records::feature::patterns::DesignPatternAxisWrapper {
                record_index: 11,
                identity_offset: 23,
            },
        ]
        .try_into()
        .unwrap(),
        persistent_identity: 17,
        resolved: None,
    };
    let inline = DesignCircularPatternAxis::Inline {
        origin: crate::test_support::reals([1.0, 2.0, 3.0]),
        origin_offset: 29,
        direction: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        direction_offset: 53,
    };

    let historical_only = [CircularPatternAxisCandidate {
        axis: historical.clone(),
        axis_record_index: 10,
        selection_record_index: 11,
    }];
    assert_eq!(
        select_circular_pattern_axis(&historical_only)
            .map(|index| &historical_only[index])
            .map(|candidate| (
                candidate.axis_record_index,
                candidate.selection_record_index
            )),
        Some((10, 11))
    );

    let mixed = [
        CircularPatternAxisCandidate {
            axis: historical.clone(),
            axis_record_index: 10,
            selection_record_index: 11,
        },
        CircularPatternAxisCandidate {
            axis: inline.clone(),
            axis_record_index: 20,
            selection_record_index: 21,
        },
    ];
    assert_eq!(
        select_circular_pattern_axis(&mixed)
            .map(|index| &mixed[index])
            .map(|candidate| (
                candidate.axis_record_index,
                candidate.selection_record_index
            )),
        Some((20, 21))
    );

    let duplicate_inline = [
        CircularPatternAxisCandidate {
            axis: inline.clone(),
            axis_record_index: 20,
            selection_record_index: 21,
        },
        CircularPatternAxisCandidate {
            axis: inline,
            axis_record_index: 30,
            selection_record_index: 31,
        },
    ];
    assert!(select_circular_pattern_axis(&duplicate_inline).is_none());

    let duplicate_historical = [
        CircularPatternAxisCandidate {
            axis: historical.clone(),
            axis_record_index: 10,
            selection_record_index: 11,
        },
        CircularPatternAxisCandidate {
            axis: historical,
            axis_record_index: 12,
            selection_record_index: 13,
        },
    ];
    assert!(select_circular_pattern_axis(&duplicate_historical).is_none());
}

fn append_header(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"999");
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

fn append_transform_record(bytes: &mut Vec<u8>, record_index: u32, translation: [f64; 3]) {
    append_header(bytes, record_index);
    for value in [
        1.0,
        0.0,
        0.0,
        translation[0],
        0.0,
        1.0,
        0.0,
        translation[1],
        0.0,
        0.0,
        1.0,
        translation[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn pattern_constructions_require_exact_scalar_and_operand_frames() {
    run_pattern_constructions_fixture();
}

#[allow(
    clippy::large_stack_arrays,
    reason = "This pattern fixture keeps the decoded scope records inline for frame assertions."
)]
fn run_pattern_constructions_fixture() {
    let scope_record_index = 10_u32;
    let count_record_index = 20_u32;
    let angle_record_index = 30_u32;
    let axis_record_index = 40_u32;
    let selection_record_index = 43_u32;
    let mut bytes = Vec::new();

    let count_start = bytes.len();
    let mut count = vec![0; 99];
    count[0..4].copy_from_slice(&3_u32.to_le_bytes());
    count[4..7].copy_from_slice(b"357");
    count[7..11].copy_from_slice(&count_record_index.to_le_bytes());
    count[19] = 1;
    count[20..24].copy_from_slice(&1_u32.to_le_bytes());
    count[24] = 1;
    count[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    count[40..44].copy_from_slice(&25_u32.to_le_bytes());
    count[44] = 1;
    count[45..49].copy_from_slice(&(count_record_index + 2).to_le_bytes());
    count[55..59].copy_from_slice(&99_u32.to_le_bytes());
    count[63] = 1;
    count[64..68].copy_from_slice(&scope_record_index.to_le_bytes());
    count[76] = 1;
    count[77..81].copy_from_slice(&(count_record_index + 1).to_le_bytes());
    count[88] = 1;
    count[89..93].copy_from_slice(&scope_record_index.to_le_bytes());
    count.extend_from_slice(&3_u32.to_le_bytes());
    count.extend_from_slice(b"258");
    count.extend_from_slice(&count_record_index.to_le_bytes());
    bytes.extend_from_slice(&count);

    let angle_start = bytes.len();
    let mut angle = vec![0; 103];
    angle[0..4].copy_from_slice(&3_u32.to_le_bytes());
    angle[4..7].copy_from_slice(b"354");
    angle[7..11].copy_from_slice(&angle_record_index.to_le_bytes());
    angle[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    angle[24] = 1;
    angle[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    angle[35] = 1;
    angle[40..48].copy_from_slice(&std::f64::consts::TAU.to_le_bytes());
    angle[48] = 1;
    angle[49..53].copy_from_slice(&77_u32.to_le_bytes());
    angle[67] = 1;
    angle[68..72].copy_from_slice(&scope_record_index.to_le_bytes());
    angle[80] = 1;
    angle[81..85].copy_from_slice(&78_u32.to_le_bytes());
    angle[92] = 1;
    angle[93..97].copy_from_slice(&scope_record_index.to_le_bytes());
    angle.extend_from_slice(&3_u32.to_le_bytes());
    angle.extend_from_slice(b"258");
    angle.extend_from_slice(&angle_record_index.to_le_bytes());
    bytes.extend_from_slice(&angle);

    let axis_start = bytes.len();
    let mut axis = vec![0; 195];
    axis[0..4].copy_from_slice(&3_u32.to_le_bytes());
    axis[4..7].copy_from_slice(b"379");
    axis[7..11].copy_from_slice(&axis_record_index.to_le_bytes());
    axis[21..25].copy_from_slice(&8_u32.to_le_bytes());
    for (offset, value) in [1.0_f64, 2.0, 3.0].into_iter().enumerate() {
        axis[25 + offset * 8..33 + offset * 8].copy_from_slice(&value.to_le_bytes());
    }
    axis[49..57].copy_from_slice(&(-1.0_f64).to_le_bytes());
    axis[89..93].copy_from_slice(&9_u32.to_le_bytes());
    axis[93..97].copy_from_slice(&1_u32.to_le_bytes());
    axis[97] = 1;
    axis[98..102].copy_from_slice(&selection_record_index.to_le_bytes());
    axis[110..114].copy_from_slice(&1_u32.to_le_bytes());
    axis[114] = 1;
    axis[115..119].copy_from_slice(&79_u32.to_le_bytes());
    axis[125..133].copy_from_slice(&0x0000_0004_0000_0000_u64.to_le_bytes());
    axis[143..147].copy_from_slice(&99_u32.to_le_bytes());
    axis[147..155].copy_from_slice(&0.5_f64.to_le_bytes());
    axis[155..159].copy_from_slice(&99_u32.to_le_bytes());
    axis[159] = 1;
    axis[160..164].copy_from_slice(&(axis_record_index + 2).to_le_bytes());
    axis[172] = 1;
    axis[173..177].copy_from_slice(&(axis_record_index + 1).to_le_bytes());
    axis[184] = 1;
    axis[185..189].copy_from_slice(&scope_record_index.to_le_bytes());
    axis.extend_from_slice(&3_u32.to_le_bytes());
    axis.extend_from_slice(b"258");
    axis.extend_from_slice(&axis_record_index.to_le_bytes());
    bytes.extend_from_slice(&axis);

    let mut scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("291".to_owned())
                .unwrap(),
            record_index: scope_record_index,
            frame_length: 329,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: Some(2),

            previous_history_state_id: Some(1),
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![
                    count_record_index,
                    angle_record_index,
                    axis_record_index,
                    selection_record_index,
                ],
                vec![0; 4],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::CPattern
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 329,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert_eq!(
        tested_circular_pattern_construction_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        ),
        Some(DesignCircularPatternConstruction {
            count: std::num::NonZeroU32::new(25).unwrap(),
            count_record_index,
            count_offset: u64_from_index(count_start + 40),
            angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU)
                .expect("checked fixture value"),
            angle_record_index,
            angle_offset: u64_from_index(angle_start + 40),
            axis: crate::records::feature::patterns::DesignCircularPatternAxis::Inline {
                origin: crate::test_support::reals([1.0, 2.0, 3.0]),
                origin_offset: u64_from_index(axis_start + 25),
                direction: cadmpeg_ir::units::UnitVector3::normalized(
                    cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0),
                )
                .expect("unit axis"),
                direction_offset: u64_from_index(axis_start + 49),
            },
            axis_record_index,
            selection_record_index,
        })
    );

    for (offset, value) in [0.0_f64, 0.0, 2.0].into_iter().enumerate() {
        bytes[axis_start + 49 + offset * 8..axis_start + 57 + offset * 8]
            .copy_from_slice(&value.to_le_bytes());
    }
    let normalized = tested_circular_pattern_construction_with_owners(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[],
    )
    .expect("non-unit axis displacement is normalized");
    let crate::records::feature::patterns::DesignCircularPatternAxis::Inline { direction, .. } =
        normalized.axis
    else {
        panic!("inline axis expected");
    };
    assert_eq!(
        *direction.as_raw(),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    );

    let mut zero_displacement = bytes.clone();
    zero_displacement[axis_start + 49..axis_start + 73].fill(0);
    assert!(tested_circular_pattern_construction_with_owners(
        &zero_displacement,
        &crate::design::test_support::indexed_record_offsets_for_test(&zero_displacement),
        &scope,
        &[],
    )
    .is_none());

    bytes[count_start + 4] = b'x';
    bytes[angle_start + 4] = b'x';
    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
                byte_offset: (evaluated_value_offset) - 40,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("457".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal,
                evaluated_value,
                evaluated_value_offset,
                parameter_record_index: record_index + 1,
                owned_ordinal: local_ordinal,
                variant: Some(0),
                companion_record_index: record_index + 2,
            },
        )
        .unwrap()
    };
    let owners = [
        owner(count_record_index, 0, 25.0, 101),
        owner(angle_record_index, 1, std::f64::consts::TAU, 202),
    ];
    let owner_backed = tested_circular_pattern_construction_with_owners(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap();
    assert_eq!(owner_backed.count.get(), 25);
    assert_eq!(owner_backed.count_offset, 101);
    assert_eq!(owner_backed.angle.get(), std::f64::consts::TAU);
    assert_eq!(owner_backed.angle_offset, 202);
    bytes[count_start + 4] = b'3';
    bytes[angle_start + 4] = b'3';

    bytes[axis_start + 89..axis_start + 93].copy_from_slice(&6_u32.to_le_bytes());
    assert!(tested_circular_pattern_construction_with_owners(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &[]
    )
    .is_some());
    bytes[axis_start + 89..axis_start + 93].fill(0);
    assert_eq!(
        tested_circular_pattern_construction_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        ),
        None
    );
    bytes[axis_start + 89..axis_start + 93].copy_from_slice(&9_u32.to_le_bytes());

    bytes[axis_start + 57..axis_start + 65].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        tested_circular_pattern_construction_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        ),
        None
    );
    bytes[axis_start + 57..axis_start + 65].fill(0);
    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.extend([axis_record_index, selection_record_index]);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        tested_circular_pattern_construction_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[]
        ),
        None
    );

    scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::RPattern
                .try_into()
                .unwrap();
        })
        .unwrap();
    let rectangular_owners = [
        owner(50, 0, 3.0, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 10.0, 503),
        owner(53, 3, 0.0, 504),
    ];
    let rectangular = tested_rectangular_pattern_construction(
        &[],
        &crate::design::test_support::indexed_record_offsets_for_test(&[]),
        &scope,
        &rectangular_owners,
    )
    .expect("exact rectangular-pattern scalar lanes");
    assert_eq!(rectangular.u_count(), 3);
    assert_eq!(rectangular.v_count(), 1);
    assert_eq!(rectangular.u_extent(), 10.0);
    assert_eq!(rectangular.v_extent(), 0.0);
    assert_eq!(rectangular.owner_record_indices, [50, 51, 52, 53]);
    assert_eq!(rectangular.value_offsets, [501, 502, 503, 504]);
    assert_eq!(rectangular.instances(), None);

    append_transform_record(&mut bytes, 100, [2.0, 3.0, 4.0]);
    for record_index in 50..=53 {
        append_header(&mut bytes, record_index);
    }
    append_header(&mut bytes, 110);
    append_transform_record(&mut bytes, 120, [2.0, 3.0, 9.0]);
    append_transform_record(&mut bytes, 130, [2.0, 3.0, 14.0]);
    append_header(&mut bytes, 140);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 50, 51, 52, 53, 110, 120, 130, 140,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let rectangular = tested_rectangular_pattern_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &rectangular_owners,
    )
    .expect("rectangular-pattern placement run");
    let instances = rectangular.instances().expect("exact placement run");
    assert_eq!(
        instances
            .frames()
            .map(|frame| frame.record_index)
            .collect::<Vec<_>>(),
        [100, 120, 130]
    );
    assert_eq!(
        instances
            .frames()
            .map(|frame| frame.transform.value[2][3])
            .collect::<Vec<_>>(),
        [4.0, 9.0, 14.0]
    );

    let mut invalid_inactive_spacing = rectangular_owners.clone();
    {
        let mut wire = crate::records::parameters::DesignParameterOwnerWire::from(
            invalid_inactive_spacing[3].clone(),
        );
        wire.evaluated_value = 1.0;
        invalid_inactive_spacing[3] =
            crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }
    assert_eq!(
        tested_rectangular_pattern_construction(
            &[],
            &crate::design::test_support::indexed_record_offsets_for_test(&[]),
            &scope,
            &invalid_inactive_spacing
        ),
        None
    );
    let mut duplicate_lane = rectangular_owners.clone();
    {
        let mut wire =
            crate::records::parameters::DesignParameterOwnerWire::from(duplicate_lane[3].clone());
        wire.local_ordinal = 2;
        duplicate_lane[3] =
            crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }
    assert_eq!(
        tested_rectangular_pattern_construction(
            &[],
            &crate::design::test_support::indexed_record_offsets_for_test(&[]),
            &scope,
            &duplicate_lane
        ),
        None
    );
    let mut excess_lane = rectangular_owners.to_vec();
    excess_lane.push(owner(54, 4, 1.0, 505));
    assert_eq!(
        tested_rectangular_pattern_construction(
            &[],
            &crate::design::test_support::indexed_record_offsets_for_test(&[]),
            &scope,
            &excess_lane
        ),
        None
    );

    scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Assemble
                .try_into()
                .unwrap();
            draft.frame_length = 627;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 51, 52, 53]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .expect("exact assembly scalar lanes");
    assert_eq!(alignment.angle(), 3.0);
    assert_eq!(alignment.offset(), [1.0, 10.0, 0.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [50, 51, 52, 53]
    );
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.offset)
            .collect::<Vec<_>>(),
        [501, 502, 503, 504]
    );
    assert_eq!(alignment.operand_frames(), None);

    let mut placement_and_alignment_owners = rectangular_owners.to_vec();
    placement_and_alignment_owners.extend([
        owner(60, 4, 0.25, 601),
        owner(61, 5, 4.0, 602),
        owner(62, 6, 5.0, 603),
        owner(63, 7, 6.0, 604),
    ]);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                50, 51, 52, 53, 60, 61, 62, 63,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .unwrap()
    .is_none());
    scope
        .try_edit(|draft| {
            draft.frame_length = 732;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .unwrap()
    .expect("assembly alignment after four placement lanes");
    assert_eq!(alignment.angle(), 0.25);
    assert_eq!(alignment.offset(), [4.0, 5.0, 6.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [60, 61, 62, 63]
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 604;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let datum_envelope_alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .unwrap()
    .expect("JointOrigin datum-envelope alignment after four placement lanes");
    assert_eq!(datum_envelope_alignment.angle(), 0.25);
    assert_eq!(datum_envelope_alignment.offset(), [4.0, 5.0, 6.0]);
    assert_eq!(
        datum_envelope_alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [60, 61, 62, 63]
    );
    assert!(datum_envelope_alignment.operand_frames().is_none());

    let mut short_axial_owners = rectangular_owners.to_vec();
    short_axial_owners.extend([owner(64, 4, 0.5, 605), owner(65, 5, 2.0, 606)]);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 51, 52, 53, 64, 65]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &short_axial_owners,
    )
    .unwrap()
    .is_none());
    scope
        .try_edit(|draft| {
            draft.frame_length = 705;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let short_axial_alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &short_axial_owners,
    )
    .unwrap()
    .expect("six-owner alignment belongs to the 705-byte axial frame");
    assert_eq!(short_axial_alignment.angle(), 0.5);
    assert_eq!(short_axial_alignment.offset(), [0.0, 0.0, 2.0]);

    let mut legacy_alignment_owners = placement_and_alignment_owners.clone();
    legacy_alignment_owners.extend([owner(64, 8, 0.5, 605), owner(65, 9, 2.0, 606)]);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                50, 51, 52, 53, 60, 61, 62, 63, 64, 65,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &legacy_alignment_owners,
    )
    .unwrap()
    .is_none());
    scope
        .try_edit(|draft| {
            draft.frame_length = 772;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &legacy_alignment_owners,
    )
    .unwrap()
    .expect("legacy assembly axial alignment lanes");
    assert_eq!(alignment.angle(), 0.5);
    assert_eq!(alignment.offset(), [0.0, 0.0, 2.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [64, 65]
    );
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.offset)
            .collect::<Vec<_>>(),
        [605, 606]
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 51, 52, 53]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let assembly_bytes = assembly_operand_frame_fixture(scope_record_index);
    scope
        .try_edit(|draft| {
            draft.frame_length = 637;
            draft.paired_byte_offset = 637;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    let frames = exact_assembly_alignment(
        &assembly_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&assembly_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_frames())
    .expect("exact assembly operand frames");
    assert_eq!(
        frames.map(|frame| (
            frame.reference_record_index,
            frame.reference_offset,
            frame.transform_offset,
            [
                frame.transform[0][3],
                frame.transform[1][3],
                frame.transform[2][3]
            ]
        )),
        [
            (70, 29, 40, [1.0, 2.0, 3.0]),
            (80, 169, 180, [4.0, 5.0, 6.0]),
        ]
    );
    let mut legacy_assembly_bytes = vec![0_u8; 633];
    legacy_assembly_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    for (legacy_reference, legacy_transform, modern_reference, modern_transform) in
        [(24, 36, 28, 40), (164, 176, 168, 180)]
    {
        legacy_assembly_bytes[legacy_reference..legacy_reference + 5]
            .copy_from_slice(&assembly_bytes[modern_reference..modern_reference + 5]);
        legacy_assembly_bytes[legacy_transform..legacy_transform + 128]
            .copy_from_slice(&assembly_bytes[modern_transform..modern_transform + 128]);
    }
    legacy_assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    legacy_assembly_bytes.extend_from_slice(b"258");
    legacy_assembly_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let legacy_assembly_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            frame_length: 633,
            paired_byte_offset: 633,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            ..scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let legacy_frames = exact_assembly_alignment(
        &legacy_assembly_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&legacy_assembly_bytes),
        &legacy_assembly_scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_frames())
    .expect("compact assembly operand frames");
    assert_eq!(legacy_frames[0].reference_offset, 25);
    assert_eq!(legacy_frames[0].transform_offset, 36);

    let mut dynamic_standard_bytes = assembly_bytes.clone();
    dynamic_standard_bytes[641..644].copy_from_slice(b"262");
    let dynamic_standard_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "262".to_owned(),
            )
            .unwrap(),
            ..scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert!(exact_assembly_alignment(
        &dynamic_standard_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&dynamic_standard_bytes),
        &dynamic_standard_scope,
        &rectangular_owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_frames().is_some()));

    let mut dynamic_compact_bytes = legacy_assembly_bytes.clone();
    dynamic_compact_bytes[637..640].copy_from_slice(b"262");
    let dynamic_compact_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "262".to_owned(),
            )
            .unwrap(),
            ..legacy_assembly_scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    assert!(exact_assembly_alignment(
        &dynamic_compact_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&dynamic_compact_bytes),
        &dynamic_compact_scope,
        &rectangular_owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_frames().is_some()));

    let mut axial_assembly_bytes = vec![0_u8; 772];
    axial_assembly_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    axial_assembly_bytes[20..25].copy_from_slice(&[1, 0, 0, 0, 0]);
    for (axial_reference, axial_transform, modern_reference, modern_transform) in
        [(28, 39, 28, 40), (167, 178, 168, 180)]
    {
        axial_assembly_bytes[axial_reference..axial_reference + 5]
            .copy_from_slice(&assembly_bytes[modern_reference..modern_reference + 5]);
        axial_assembly_bytes[axial_transform..axial_transform + 128]
            .copy_from_slice(&assembly_bytes[modern_transform..modern_transform + 128]);
    }
    axial_assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    axial_assembly_bytes.extend_from_slice(b"261");
    axial_assembly_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let axial_assembly_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            frame_length: 772,
            paired_byte_offset: 772,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                50, 51, 52, 53, 60, 61, 62, 63, 64, 65,
            ]),
            ..scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let axial_alignment = exact_assembly_alignment(
        &axial_assembly_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&axial_assembly_bytes),
        &axial_assembly_scope,
        &legacy_alignment_owners,
    )
    .unwrap()
    .expect("legacy assembly alignment and operand frames");
    assert_eq!(axial_alignment.angle(), 0.5);
    assert_eq!(axial_alignment.offset(), [0.0, 0.0, 2.0]);
    let axial_frames = axial_alignment.operand_frames().unwrap();
    assert_eq!(axial_frames[0].reference_offset, 29);
    assert_eq!(axial_frames[0].transform_offset, 39);
    assert_eq!(axial_frames[1].reference_offset, 168);
    assert_eq!(axial_frames[1].transform_offset, 178);

    let mut short_axial_bytes = axial_assembly_bytes[..705].to_vec();
    short_axial_bytes.extend_from_slice(&3_u32.to_le_bytes());
    short_axial_bytes.extend_from_slice(b"261");
    short_axial_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let short_axial_scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            frame_length: 705,
            paired_byte_offset: 705,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            reference_members: crate::records::identity::ReferenceRun::unlocated(vec![
                50, 51, 52, 53, 64, 65,
            ]),
            ..scope.clone().into_draft()
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let short_axial_alignment = exact_assembly_alignment(
        &short_axial_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&short_axial_bytes),
        &short_axial_scope,
        &short_axial_owners,
    )
    .unwrap()
    .expect("short axial assembly alignment and operand frames");
    assert_eq!(short_axial_alignment.angle(), 0.5);
    assert_eq!(short_axial_alignment.offset(), [0.0, 0.0, 2.0]);
    assert_eq!(
        short_axial_alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [64, 65]
    );
    assert!(short_axial_alignment.operand_paths().is_none());
    let short_axial_frames = short_axial_alignment
        .operand_frames()
        .expect("short axial operand frames");
    assert_eq!(short_axial_frames[0].reference_offset, 29);
    assert_eq!(short_axial_frames[0].transform_offset, 39);
    assert_eq!(short_axial_frames[1].reference_offset, 168);
    assert_eq!(short_axial_frames[1].transform_offset, 178);

    let mut first_joint_origin = scope.clone();
    first_joint_origin
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::JointOrigin
                .try_into()
                .unwrap();
        })
        .unwrap();
    first_joint_origin.record_index = 70;
    first_joint_origin
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![draft.record_index]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut second_joint_origin = first_joint_origin.clone();
    second_joint_origin.record_index = 80;
    let mut linked_assembly = axial_assembly_scope.clone();
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        linked_assembly.payload_mut()
    {
        *slot = Some(axial_alignment.clone());
    }
    let mut linked_scopes = [linked_assembly, first_joint_origin, second_joint_origin];
    bind_joint_origin_frames_from_assemblies(&axial_assembly_bytes, &mut linked_scopes);
    assert_eq!(linked_scopes[1].joint_origin_transform_offset(), Some(39));
    assert_eq!(
        linked_scopes[1].joint_origin_transform(),
        Some(axial_frames[0].transform)
    );
    assert_eq!(linked_scopes[2].joint_origin_transform_offset(), Some(178));
    assert_eq!(
        linked_scopes[2].joint_origin_transform(),
        Some(axial_frames[1].transform)
    );
    assert_eq!(
        linked_scopes[0].assembly_alignment().and_then(
            crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index
        ),
        None
    );

    let mut single_frame_bytes = vec![0_u8; 604];
    single_frame_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    single_frame_bytes[24] = 1;
    single_frame_bytes[25..29].copy_from_slice(&90_u32.to_le_bytes());
    single_frame_bytes[36..164].copy_from_slice(&assembly_bytes[40..168]);
    single_frame_bytes[164] = 1;
    single_frame_bytes[165..169].copy_from_slice(&91_u32.to_le_bytes());
    single_frame_bytes[175..179].copy_from_slice(&1_u32.to_le_bytes());
    let mut single_frame_assembly = scope.clone();
    single_frame_assembly.class_tag =
        crate::records::references::DesignClassTag::try_from("276".to_owned()).unwrap();
    single_frame_assembly.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    single_frame_assembly
        .try_edit(|draft| {
            draft.frame_length = 604;
            draft.paired_byte_offset = 604;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                placement_and_alignment_owners
                    .iter()
                    .map(crate::records::parameters::DesignParameterOwner::record_index)
                    .collect(),
            );
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        single_frame_assembly.payload_mut()
    {
        *slot = Some(datum_envelope_alignment);
    }
    let mut single_frame_joint_origin = scope.clone();
    single_frame_joint_origin
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::JointOrigin
                .try_into()
                .unwrap();
        })
        .unwrap();
    single_frame_joint_origin.record_index = 91;
    single_frame_joint_origin
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![draft.record_index]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut single_frame_scopes = [single_frame_assembly, single_frame_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut single_frame_scopes);
    assert_eq!(
        single_frame_scopes[1].joint_origin_transform_offset(),
        Some(36)
    );
    assert_eq!(
        single_frame_scopes[1].joint_origin_transform(),
        Some(axial_frames[0].transform)
    );
    assert_eq!(single_frame_scopes[1].joint_origin_reference(), Some(90));
    assert_eq!(
        single_frame_scopes[1].joint_origin_reference_offset(),
        Some(25)
    );
    assert_eq!(
        single_frame_scopes[0].assembly_alignment().and_then(
            crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index
        ),
        Some(91)
    );

    let mut conflicting_assembly = single_frame_scopes[0].clone();
    conflicting_assembly.assembly_alignment_mut().unwrap().form = None;
    let mut conflicting_joint_origin = single_frame_scopes[1].clone();
    let frame = conflicting_joint_origin.joint_origin_frame_mut().unwrap();
    let mut transform = frame.joint_origin_transform.rows();
    transform[2][3] += 1.0;
    frame.joint_origin_transform = transform.try_into().unwrap();
    let mut conflicting_scopes = [conflicting_assembly, conflicting_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut conflicting_scopes);
    assert_eq!(
        conflicting_scopes[0].assembly_alignment().and_then(
            crate::records::feature::assembly::DesignAssemblyAlignment::joint_origin_scope_record_index
        ),
        None
    );

    single_frame_bytes[175..179].copy_from_slice(&2_u32.to_le_bytes());
    let mut invalid_joint_origin = single_frame_scopes[1].clone();
    if let crate::records::feature::scope::DesignScopePayloadMut::JointOrigin(slot) =
        invalid_joint_origin.payload_mut()
    {
        *slot = None;
    }
    let mut invalid_single_frame_scopes = [single_frame_scopes[0].clone(), invalid_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut invalid_single_frame_scopes);
    assert_eq!(
        invalid_single_frame_scopes[1].joint_origin_transform(),
        None
    );

    let mut compact_bytes = assembly_bytes[..627].to_vec();
    compact_bytes.extend_from_slice(&3_u32.to_le_bytes());
    compact_bytes.extend_from_slice(b"264");
    compact_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let mut compact_scope = scope.clone();
    compact_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("459".to_owned()).unwrap();
    compact_scope
        .try_edit(|draft| {
            draft.frame_length = 627;
            draft.paired_byte_offset = 627;
            draft.layout_fixture_tail();
        })
        .unwrap();
    compact_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("264".to_owned()).unwrap();
    assert!(exact_assembly_alignment(
        &compact_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&compact_bytes),
        &compact_scope,
        &rectangular_owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_frames().is_some()));
}

#[test]
fn numerical_ranges_rectangular_pattern_accepts_finite_large_extent() {
    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("291".to_owned())
                .unwrap(),
            record_index: 0,
            frame_length: 329,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: Some(2),

            previous_history_state_id: Some(1),
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100, 50, 51, 52, 53, 110, 120, 130, 140],
                vec![0; 9],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::RPattern
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 329,
        }
        .with_fixture_layout(),
    )
    .unwrap();

    let mut bytes = Vec::new();
    append_transform_record(&mut bytes, 100, [0., 0., 0.]);
    for index in 50..=53 {
        append_header(&mut bytes, index);
    }
    append_header(&mut bytes, 110);
    append_transform_record(&mut bytes, 120, [0., 0., 5e199]);
    append_transform_record(&mut bytes, 130, [0., 0., 1e200]);
    append_header(&mut bytes, 140);
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                u_count: 3,
                v_count: 1,
                u_extent: 1e200,
                v_extent: 0.,
                owner_record_indices: [50, 51, 52, 53],
                value_offsets: [501, 502, 503, 504],
                instances: None,
                instance_search_cap: None,
                directions: Vec::new(),
                secondary_direction: None,
            },
        )
        .unwrap();
    let instances = tested_rectangular_pattern_instances(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &construction,
    )
    .unwrap();
    assert_eq!(instances.instance_count(), 3);
    assert_eq!(
        instances.frames().last().unwrap().transform.value[2][3],
        1e200
    );
}

#[test]
fn circular_pattern_historical_wrappers_resolve_an_edge_axis() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScopeDraft};
    let scope = DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#1".into(),
            byte_offset: 0,
            class_tag: "291".to_owned().try_into().unwrap(),
            record_index: 1,
            frame_length: 329,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![20],
                vec![0],
                "reference_members",
            )
            .unwrap(),
            payload: DesignFeatureKind::CPattern.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "258".to_owned().try_into().unwrap(),
            paired_byte_offset: 329,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"308", 50);
    bytes.resize(129, 0);
    for at in [21, 36, 51] {
        bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
    }
    for (at, index) in [
        (25, 80u32),
        (40, 20),
        (55, 80),
        (66, 1),
        (93, 52),
        (106, 51),
        (118, 1),
    ] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }
    for at in [77, 89] {
        bytes[at..at + 4].copy_from_slice(&7u32.to_le_bytes());
    }
    bytes[81..89].copy_from_slice(&0.5f64.to_le_bytes());
    indexed_header(&mut bytes, *b"258", 50);
    indexed_header(&mut bytes, *b"308", 80);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&40u64.to_le_bytes());
    lp_utf16(&mut bytes, "384d79a0-c23e-42aa-b993-74df1f8dfcae");
    lp_utf16(&mut bytes, "352c47d7-42ba-443e-9de1-ae0e37cc129d");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    push_marked_reference(&mut bytes, 81);
    indexed_header(&mut bytes, *b"305", 81);
    bytes.extend_from_slice(&[0; 10]);
    push_marked_reference(&mut bytes, 82);
    indexed_header(&mut bytes, *b"300", 82);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&503u64.to_le_bytes());
    indexed_header(&mut bytes, *b"308", 83);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let (axis, selection) =
        super::exact_legacy_circular_pattern_axis(&bytes, &records, 0, 129, 50, &scope)
            .unwrap()
            .unwrap();
    assert_eq!(selection, 20);
    assert!(
        matches!(axis, crate::records::feature::patterns::DesignCircularPatternAxis::HistoricalEdge {
        wrappers, persistent_identity: 503, resolved: None,
    } if wrappers.len() == 2 && wrappers[0] == wrappers[1])
    );
}

fn rectangular_instance_fixture() -> (
    Vec<u8>,
    crate::design::decode::sketch::IndexedRecordOffsets,
    DesignParameterScope,
    crate::records::feature::patterns::DesignRectangularPatternConstruction,
) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        0,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 50, 51, 52, 53, 110, 120, 130, 140,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut bytes = Vec::new();
    append_transform_record(&mut bytes, 100, [0., 0., 0.]);
    for index in 50..=53 {
        append_header(&mut bytes, index);
    }
    append_header(&mut bytes, 110);
    append_transform_record(&mut bytes, 120, [0., 0., 5e199]);
    append_transform_record(&mut bytes, 130, [0., 0., 1e200]);
    append_header(&mut bytes, 140);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                u_count: 3,
                v_count: 1,
                u_extent: 1e200,
                v_extent: 0.,
                owner_record_indices: [50, 51, 52, 53],
                value_offsets: [501, 502, 503, 504],
                instances: None,
                instance_search_cap: None,
                directions: Vec::new(),
                secondary_direction: None,
            },
        )
        .unwrap();
    (bytes, records, scope, construction)
}

#[test]
fn rectangular_pattern_local_search_ceilings_cap_the_search() {
    let (bytes, records, scope, construction) = rectangular_instance_fixture();
    let mut wire =
        crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
            construction,
        );
    wire.u_count = u32::try_from(MAX_RECTANGULAR_PATTERN_INSTANCES + 1).unwrap();
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(wire)
            .unwrap();
    assert_eq!(
        super::exact_rectangular_pattern_instances(&bytes, &records, &scope, &construction),
        super::InstanceSearch::Capped(RectangularPatternSearchCap::Instances),
        "instances past the local count ceiling stay unresolved"
    );
    let (mut bytes, _, scope, construction) = rectangular_instance_fixture();
    bytes.splice(
        139..139,
        std::iter::repeat_n(0, MAX_RECTANGULAR_PATTERN_RECORD_BYTES),
    );
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    assert_eq!(
        super::exact_rectangular_pattern_instances(&bytes, &records, &scope, &construction),
        super::InstanceSearch::Capped(RectangularPatternSearchCap::RecordBytes),
        "instances past the local record-span ceiling stay unresolved"
    );
}

#[test]
fn rectangular_pattern_run_search_stops_at_its_work_cap() {
    fn append_translation(bytes: &mut Vec<u8>, translation: [f64; 3]) {
        for value in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let decoyed_instances = |decoys: u32| {
        let (_, _, scope, construction) = rectangular_instance_fixture();
        let mut wire =
            crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
                construction,
            );
        wire.u_extent = 2.0;
        let construction =
            crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(wire)
                .unwrap();
        let mut bytes = Vec::new();
        append_transform_record(&mut bytes, 100, [0., 0., 0.]);
        for decoy in 0..decoys {
            append_translation(&mut bytes, [10. + f64::from(decoy), 0., 0.]);
        }
        for index in 50..=53 {
            append_header(&mut bytes, index);
        }
        append_header(&mut bytes, 110);
        append_transform_record(&mut bytes, 120, [0., 0., 1.]);
        append_transform_record(&mut bytes, 130, [0., 0., 2.]);
        for decoy in 0..decoys {
            append_translation(&mut bytes, [-10. - f64::from(decoy), 0., 0.]);
        }
        append_header(&mut bytes, 140);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        super::exact_rectangular_pattern_instances(&bytes, &records, &scope, &construction)
    };
    assert!(
        matches!(decoyed_instances(8), super::InstanceSearch::Resolved(_)),
        "one endpoint pair spans the extent"
    );
    assert_eq!(
        decoyed_instances(1024),
        super::InstanceSearch::Capped(RectangularPatternSearchCap::RunWork),
        "endpoint pairs past the local work cap leave the instances unresolved"
    );
}

#[test]
fn rectangular_pattern_aggregate_search_span_caps_the_search() {
    const INSTANCE_COUNT: u32 = 17;
    const RECORD_SPAN: usize = MAX_RECTANGULAR_PATTERN_RECORD_BYTES;
    let (_, _, mut scope, construction) = rectangular_instance_fixture();
    let mut references = vec![100, 50, 51, 52, 53, 110];
    references.extend(1000..1000 + INSTANCE_COUNT - 1);
    references.push(2000);
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut bytes = Vec::new();
    append_transform_record(&mut bytes, 100, [0., 0., 0.]);
    bytes.resize(RECORD_SPAN, 0);
    for index in 50..=53 {
        append_header(&mut bytes, index);
    }
    append_header(&mut bytes, 110);
    for index in 1000..1000 + INSTANCE_COUNT - 1 {
        let start = bytes.len();
        append_transform_record(&mut bytes, index, [0., 0., 0.]);
        bytes.resize(start + RECORD_SPAN, 0);
    }
    append_header(&mut bytes, 2000);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut wire =
        crate::records::feature::patterns::DesignRectangularPatternConstructionWire::from(
            construction,
        );
    wire.u_count = INSTANCE_COUNT;
    let construction =
        crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(wire)
            .unwrap();
    assert_eq!(
        super::exact_rectangular_pattern_instances(&bytes, &records, &scope, &construction),
        super::InstanceSearch::Capped(RectangularPatternSearchCap::ScannedBytes),
        "instances past the aggregate span ceiling stay unresolved"
    );
}

fn rectangular_pattern_direction_frame(selection_kind: u32, displacement: [f64; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"439", 20);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&selection_kind.to_le_bytes());
    for value in [-2.0, 1.0, 0.5].into_iter().chain(displacement) {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(&mut bytes, 23);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(&mut bytes, 40);
    bytes.extend_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&245u32.to_le_bytes());
    bytes.extend_from_slice(&f64::to_le_bytes(1.5e-5));
    bytes.extend_from_slice(&245u32.to_le_bytes());
    push_marked_reference(&mut bytes, 22);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 2]);
    push_marked_reference(&mut bytes, 21);
    bytes.push(0);
    push_marked_reference(&mut bytes, 10);
    assert_eq!(bytes.len(), 220);
    indexed_header(&mut bytes, *b"263", 20);
    bytes
}

#[test]
fn rectangular_pattern_direction_record_stores_origin_and_displacement() {
    let bytes = rectangular_pattern_direction_frame(8, [0.0, -2.5, 0.0]);
    let direction = super::exact_rectangular_pattern_direction(&bytes, 0, 220, 20, 23, 10)
        .expect("direction record");
    assert_eq!(direction.record_index, 20);
    assert_eq!(direction.selection_record_index, 23);
    assert_eq!(direction.selection_kind, 8);
    assert_eq!(
        direction.origin,
        crate::test_support::reals([-2.0, 1.0, 0.5])
    );
    assert_eq!(direction.origin_offset, 25);
    assert_eq!(
        direction.displacement,
        crate::test_support::reals([0.0, -2.5, 0.0])
    );
    assert_eq!(direction.displacement_offset, 49);
    assert_eq!(
        direction.direction(),
        cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0))
    );

    let unit = rectangular_pattern_direction_frame(16, [1.0, 0.0, 0.0]);
    assert_eq!(
        super::exact_rectangular_pattern_direction(&unit, 0, 220, 20, 23, 10)
            .map(|direction| direction.selection_kind),
        Some(16)
    );
}

#[test]
fn rectangular_pattern_direction_record_refuses_a_foreign_frame() {
    let parse =
        |bytes: &[u8]| super::exact_rectangular_pattern_direction(bytes, 0, 220, 20, 23, 10);
    assert!(parse(&rectangular_pattern_direction_frame(9, [1.0, 0.0, 0.0])).is_none());
    assert!(parse(&rectangular_pattern_direction_frame(8, [0.0, 0.0, 0.0])).is_none());
    let bytes = rectangular_pattern_direction_frame(8, [1.0, 0.0, 0.0]);
    assert!(super::exact_rectangular_pattern_direction(&bytes, 0, 220, 20, 24, 10).is_none());
    assert!(super::exact_rectangular_pattern_direction(&bytes, 0, 220, 20, 23, 11).is_none());
    assert!(super::exact_rectangular_pattern_direction(&bytes, 0, 195, 20, 23, 10).is_none());
    let mut flag = bytes.clone();
    flag[194] = 0;
    assert!(parse(&flag).is_none());
    let mut negative_one = bytes;
    negative_one[100] = 0;
    assert!(parse(&negative_one).is_none());
}

fn spacing_direction_frame() -> Vec<u8> {
    let mut bytes = rectangular_pattern_direction_frame(8, [0.0, 2.0, 0.0]);
    bytes[89..93].copy_from_slice(&7u32.to_le_bytes());
    bytes.insert(93, 0);
    bytes
}

#[test]
fn rectangular_pattern_spacing_direction_validates_shifted_fields() {
    let bytes = spacing_direction_frame();
    let parse =
        |bytes: &[u8]| super::exact_rectangular_pattern_direction(bytes, 0, 221, 20, 23, 10);
    let direction = parse(&bytes).expect("spacing direction");
    assert_eq!(direction.distance_type, 7);
    assert_eq!(direction.selection_record_index, 23);
    assert_eq!(direction.origin_offset, 25);
    assert_eq!(direction.displacement_offset, 49);
    assert_eq!(
        direction.displacement,
        crate::test_support::reals([0.0, 2.0, 0.0])
    );
    for offset in [
        11, 73, 89, 93, 101, 118, 122, 123, 133, 135, 139, 150, 158, 168, 180, 184, 185, 195, 196,
        198, 199, 209, 210, 211, 222, 228,
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    assert!(super::exact_rectangular_pattern_direction(&bytes, 0, 220, 20, 23, 10).is_none());
    assert!(super::exact_rectangular_pattern_direction(&bytes, 0, 221, 20, 24, 10).is_none());
    assert!(super::exact_rectangular_pattern_direction(&bytes, 0, 221, 20, 23, 11).is_none());
    for end in 0..bytes.len() {
        assert!(parse(&bytes[..end]).is_none());
    }
}

fn spacing_payload() -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"373", 77);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    push_marked_reference(&mut bytes, 50);
    bytes.extend_from_slice(&[0; 4]);
    push_marked_reference(&mut bytes, 51);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    for value in [0.0f64, 0.0, -3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for index in [52, 53] {
        push_marked_reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&[0; 2]);
    for index in [51, 20] {
        push_marked_reference(&mut bytes, index);
    }
    assert_eq!(bytes.len(), 133);
    bytes.extend_from_slice(&[0; 7]);
    for index in [50, 10, 100] {
        push_marked_reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"307", 99);
    indexed_header(&mut bytes, *b"261", 100);
    bytes
}

#[test]
fn rectangular_pattern_payload_binds_secondary_vector_to_owners_and_seed() {
    use crate::records::feature::patterns::{
        DesignRectangularPatternConstruction, DesignRectangularPatternConstructionWire,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 20, 23]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let construction =
        DesignRectangularPatternConstruction::try_from(DesignRectangularPatternConstructionWire {
            u_count: 5,
            v_count: 3,
            u_extent: 1.7,
            v_extent: -0.9,
            owner_record_indices: [50, 51, 52, 53],
            value_offsets: [0; 4],
            instances: None,
            instance_search_cap: None,
            directions: Vec::new(),
            secondary_direction: None,
        })
        .unwrap();
    let direction =
        super::exact_rectangular_pattern_direction(&spacing_direction_frame(), 0, 221, 20, 23, 10)
            .unwrap();
    let parse = |bytes: &[u8]| {
        super::exact_rectangular_pattern_secondary_direction(
            bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(bytes),
            &scope,
            &construction,
            &[direction],
        )
    };
    let bytes = spacing_payload();
    let secondary = parse(&bytes).expect("linked secondary direction");
    assert_eq!(secondary.offset, 63);
    assert_eq!(
        secondary.value,
        crate::test_support::reals([0.0, 0.0, -3.0])
    );
    for offset in [
        11, 19, 20, 24, 25, 35, 39, 40, 50, 59, 87, 88, 98, 99, 109, 110, 111, 112, 122, 123, 140,
        141, 151, 152, 162, 163, 173,
    ] {
        let mut corrupt = bytes.clone();
        corrupt[offset] ^= 1;
        assert!(parse(&corrupt).is_none(), "offset {offset}");
    }
    let mut zero = bytes.clone();
    zero[63..87].fill(0);
    assert!(parse(&zero).is_none());
    let mut infinite = bytes.clone();
    infinite[63..71].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(parse(&infinite).is_none());
    let mut ambiguous = bytes.clone();
    ambiguous.extend_from_slice(&bytes);
    assert!(parse(&ambiguous).is_none());
}
