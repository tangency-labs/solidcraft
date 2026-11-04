// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::draft::exact_draft_operation_with_owners;
use crate::design::decode::scopes::fixed_parameters::{
    exact_fixed_chamfer_parameters, exact_fixed_fillet_parameters,
};
use crate::design::decode::scopes::path_feature::exact_path_feature_construction;
use crate::records::feature::direct_face::DesignDraftOperation;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::fixed_parameters::{
    DesignFixedChamferParameters, DesignFixedFilletParameters,
};
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{
    DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
    DesignParameterSource,
};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity};
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use crate::test_support::lp_utf16;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

pub(super) fn continue_fixed_kind_operations(
    bytes: Vec<u8>,
    scope: DesignParameterScope,
    thicken_group: &DesignConstructionOperandGroup,
) {
    let (bytes, scope) = fixed_kind_edge_and_revolve_operations(bytes, scope, thicken_group);
    super::fixed_kind_path_operations::fixed_kind_path_operations(bytes, scope, thicken_group);
}

fn fixed_kind_edge_and_revolve_operations(
    mut bytes: Vec<u8>,
    scope: DesignParameterScope,
    thicken_group: &DesignConstructionOperandGroup,
) -> (Vec<u8>, DesignParameterScope) {
    let draft_start = bytes.len();
    for (record_index, ordinal, value) in [(175u32, 0u8, 0.4f64), (176, 1, 0.0)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut draft_scope = scope.clone();
    draft_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Draft
                .try_into()
                .unwrap();
            draft.frame_length = 361;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                175, 176, 181, 182, 186, 190, 193,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let expected = Some(DesignDraftOperation {
        angle: cadmpeg_ir::scalar::Angle::new(0.4).unwrap(),
        angle_record_index: 175,
        angle_offset: u64_from_index(draft_start + 40),
        opposite_angle_record_index: 176,
        opposite_angle_offset: u64_from_index(draft_start + 155),
        flags: None,
    });
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &draft_scope,
            &[],
        ),
        expected
    );

    // The ordered reference table is in record-index order, so the scalar lanes
    // hold no fixed position in it. Their local ordinals order them, and moving
    // them within the table must not change the recovered operation.
    draft_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                181, 182, 186, 190, 193, 175, 176,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &draft_scope,
            &[],
        ),
        expected
    );

    // A table that reaches only one of the two lanes has no complete operation.
    draft_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                175, 181, 182, 186, 190, 193,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );

    draft_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![175, 176, 181, 182, 186]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );
    draft_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                175, 176, 181, 182, 186, 190, 193,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let fillet_start = bytes.len();
    for (record_index, ordinal, value) in [
        (77u32, 0u8, 1.0f64),
        (78, 1, 0.0),
        (79, 2, 0.65),
        (87, 3, 0.4),
        (88, 4, 0.2),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut fillet_scope = scope.clone();
    fillet_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Fillet
                .try_into()
                .unwrap();
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![77, 50, 78, 79, 87, 88]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_fillet_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &fillet_scope,
            &[],
            &[],
        ),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::feature::fixed_parameters::DesignFixedFilletGroup::try_new(
                Some(crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                    value: crate::test_support::real(1.0),
                    record_index: 77,
                    value_offset: u64_from_index(fillet_start + 40),
                }),
                crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Variable {
                    start: crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                        value: crate::test_support::real(0.0),
                        record_index: 78,
                        value_offset: u64_from_index(fillet_start + 115 + 40)
                    },
                    end: crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                        value: crate::test_support::real(0.65),
                        record_index: 79,
                        value_offset: u64_from_index(fillet_start + 230 + 40)
                    },
                    intermediate: vec![crate::records::feature::fixed_parameters::DesignFixedFilletIntermediate {
                        radius: crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                            value: crate::test_support::real(0.4),
                            record_index: 87,
                            value_offset: u64_from_index(fillet_start + 345 + 40)
                        },
                        parameter: crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                            value: crate::test_support::real(0.2),
                            record_index: 88,
                            value_offset: u64_from_index(fillet_start + 460 + 40)
                        },
                    }],
                },
            )
            .unwrap()],
        })
    );
    fillet_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 77]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_fillet_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &fillet_scope,
            &[],
            &[],
        ),
        Some(DesignFixedFilletParameters {
            groups: vec![
                crate::records::feature::fixed_parameters::DesignFixedFilletGroup::try_new(
                    None,
                    crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(
                        crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                            value: crate::test_support::real(1.0),
                            record_index: 77,
                            value_offset: u64_from_index(fillet_start + 40)
                        }
                    ),
                )
                .unwrap()
            ],
        })
    );

    let dynamic_scalar_at = bytes.len();
    let mut dynamic_scalar = vec![0; 103];
    dynamic_scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    dynamic_scalar[4..7].copy_from_slice(b"406");
    dynamic_scalar[7..11].copy_from_slice(&89u32.to_le_bytes());
    dynamic_scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    dynamic_scalar[24] = 1;
    dynamic_scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    dynamic_scalar[40..48].copy_from_slice(&0.5f64.to_le_bytes());
    dynamic_scalar[48] = 1;
    dynamic_scalar[49..53].copy_from_slice(&90u32.to_le_bytes());
    dynamic_scalar[67] = 1;
    dynamic_scalar[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    dynamic_scalar[80] = 1;
    dynamic_scalar[81..85].copy_from_slice(&91u32.to_le_bytes());
    dynamic_scalar[92] = 1;
    dynamic_scalar[93..97].copy_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&dynamic_scalar);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&89u32.to_le_bytes());
    fillet_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![89]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_fillet_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &fillet_scope,
            &[],
            &[],
        ),
        Some(DesignFixedFilletParameters {
            groups: vec![
                crate::records::feature::fixed_parameters::DesignFixedFilletGroup::try_new(
                    None,
                    crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(
                        crate::records::feature::fixed_parameters::DesignFixedFilletScalar {
                            value: crate::test_support::real(0.5),
                            record_index: 89,
                            value_offset: u64_from_index(dynamic_scalar_at + 40)
                        }
                    ),
                )
                .unwrap()
            ],
        })
    );

    let second_group_at = bytes.len();
    for (record_index, ordinal, value) in [
        (92u32, 0u8, 1.0f64),
        (95, 1, 0.5),
        (98, 2, 0.75),
        (101, 3, 0.25),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"406");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"259");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    fillet_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![92, 95, 98, 101]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let fixed = exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[],
        &[],
    )
    .expect("two constant-radius Fillet scalar groups");
    assert_eq!(fixed.groups.len(), 2);
    assert_eq!(
        fixed.groups[0]
            .law()
            .radii()
            .map(|scalar| scalar.value.get())
            .collect::<Vec<_>>(),
        [0.5]
    );
    assert_eq!(
        fixed.groups[1]
            .law()
            .radii()
            .map(|scalar| scalar.value.get())
            .collect::<Vec<_>>(),
        [0.25]
    );
    assert_eq!(
        fixed.groups[1]
            .tangency_weight()
            .map(|weight| (weight.value.get(), weight.value_offset)),
        Some((0.75, u64_from_index(second_group_at + 2 * 115 + 40)))
    );

    fillet_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    let (p92, o92) = make_linked_param(&fillet_scope, 92, "Radius", 1.0);
    let (p95, o95) = make_linked_param(&fillet_scope, 95, "TangencyWeight", 0.5);
    let (p98, o98) = make_linked_param(&fillet_scope, 98, "Radius", 0.75);
    let (p101, o101) = make_linked_param(&fillet_scope, 101, "TangencyWeight", 0.25);
    let swapped_fixed = exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p92.clone(), p95.clone(), p98.clone(), p101.clone()],
        &[o92.clone(), o95.clone(), o98.clone(), o101.clone()],
    )
    .expect("swapped constant-radius Fillet scalar groups");
    assert_eq!(swapped_fixed.groups.len(), 2);
    assert_eq!(
        swapped_fixed.groups[0]
            .law()
            .radii()
            .map(|scalar| scalar.value.get())
            .collect::<Vec<_>>(),
        [1.0]
    );
    assert_eq!(
        swapped_fixed.groups[0]
            .tangency_weight()
            .map(|weight| weight.value.get()),
        Some(0.5)
    );
    assert_eq!(
        swapped_fixed.groups[1]
            .law()
            .radii()
            .map(|scalar| scalar.value.get())
            .collect::<Vec<_>>(),
        [0.75]
    );
    assert_eq!(
        swapped_fixed.groups[1]
            .tangency_weight()
            .map(|weight| weight.value.get()),
        Some(0.25)
    );

    let (p92_def, o92_def) = make_linked_param(&fillet_scope, 92, "TangencyWeight", 1.0);
    let (p95_def, o95_def) = make_linked_param(&fillet_scope, 95, "Radius", 0.5);
    let default_fixed = exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p92_def, p95_def, p98.clone(), p101.clone()],
        &[o92_def, o95_def, o98.clone(), o101.clone()],
    )
    .expect("group 0 default-ordered, group 1 swapped");
    assert_eq!(
        default_fixed.groups[0]
            .law()
            .radii()
            .map(|scalar| scalar.value.get())
            .collect::<Vec<_>>(),
        [0.5]
    );
    assert_eq!(
        default_fixed.groups[0]
            .tangency_weight()
            .map(|weight| weight.value.get()),
        Some(1.0)
    );

    // Partial linkage: only lane 92 linked, lane 95 unlinked
    assert!(exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p92.clone(), p98.clone(), p101.clone()],
        &[o92.clone(), o98.clone(), o101.clone()],
    )
    .is_none());

    // Partial linkage: only lane 95 linked, lane 92 unlinked
    assert!(exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p95.clone(), p98.clone(), p101.clone()],
        &[o95.clone(), o98.clone(), o101.clone()],
    )
    .is_none());

    // Contradictory linkage: both lanes linked to Radius
    let (p95_rad, o95_rad) = make_linked_param(&fillet_scope, 95, "Radius", 0.5);
    assert!(exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p92.clone(), p95_rad, p98.clone(), p101.clone()],
        &[o92.clone(), o95_rad, o98.clone(), o101.clone()],
    )
    .is_none());

    // Contradictory linkage: both lanes linked to TangencyWeight
    let (p92_tw, o92_tw) = make_linked_param(&fillet_scope, 92, "TangencyWeight", 1.0);
    assert!(exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p92_tw, p95.clone(), p98.clone(), p101.clone()],
        &[o92_tw, o95.clone(), o98.clone(), o101.clone()],
    )
    .is_none());

    // Contradictory linkage: unexpected source kind (ChordLen)
    let (p92_chord, o92_chord) = make_linked_param(&fillet_scope, 92, "ChordLen", 1.0);
    assert!(exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p92_chord, p95.clone(), p98.clone(), p101.clone()],
        &[o92_chord, o95.clone(), o98.clone(), o101.clone()],
    )
    .is_none());

    // Contradictory linkage: owner exists but parameter record missing
    assert!(exact_fixed_fillet_parameters(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &fillet_scope,
        &[p98.clone(), p101.clone()],
        &[o92.clone(), o95.clone(), o98.clone(), o101.clone()],
    )
    .is_none());

    let chamfer_scalar_start = bytes.len();
    let mut chamfer_scalar = vec![0; 104];
    chamfer_scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    chamfer_scalar[4..7].copy_from_slice(b"277");
    chamfer_scalar[7..11].copy_from_slice(&86u32.to_le_bytes());
    chamfer_scalar[24] = 1;
    chamfer_scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    chamfer_scalar[35] = 0;
    chamfer_scalar[40..48].copy_from_slice(&0.04f64.to_le_bytes());
    chamfer_scalar.extend_from_slice(&3u32.to_le_bytes());
    chamfer_scalar.extend_from_slice(b"261");
    chamfer_scalar.extend_from_slice(&86u32.to_le_bytes());
    bytes.extend_from_slice(&chamfer_scalar);
    let mut chamfer_scope = scope.clone();
    chamfer_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Chamfer
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![86]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::EqualDistance {
            distance: crate::records::feature::fixed_parameters::DesignFixedChamferDistance {
                value: cadmpeg_ir::scalar::PositiveReal::new(0.04).expect("checked fixture value"),
                record_index: 86,
                value_offset: u64_from_index(chamfer_scalar_start + 40),
            },
        })
    );
    let second_chamfer_scalar_start = bytes.len();
    let mut second_chamfer_scalar = chamfer_scalar[..104].to_vec();
    second_chamfer_scalar[7..11].copy_from_slice(&96u32.to_le_bytes());
    second_chamfer_scalar[35] = 1;
    second_chamfer_scalar[40..48].copy_from_slice(&0.08f64.to_le_bytes());
    second_chamfer_scalar.extend_from_slice(&3u32.to_le_bytes());
    second_chamfer_scalar.extend_from_slice(b"261");
    second_chamfer_scalar.extend_from_slice(&96u32.to_le_bytes());
    bytes.extend_from_slice(&second_chamfer_scalar);
    chamfer_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![86, 96]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::TwoDistances {
            first: crate::records::feature::fixed_parameters::DesignFixedChamferDistance {
                value: cadmpeg_ir::scalar::PositiveReal::new(0.04).expect("checked fixture value"),
                record_index: 86,
                value_offset: u64_from_index(chamfer_scalar_start + 40),
            },
            second: crate::records::feature::fixed_parameters::DesignFixedChamferDistance {
                value: cadmpeg_ir::scalar::PositiveReal::new(0.08).expect("checked fixture value"),
                record_index: 96,
                value_offset: u64_from_index(second_chamfer_scalar_start + 40),
            },
        })
    );
    chamfer_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    let indexed_owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:parameter-owner#97".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 97,
            scope_record_index: chamfer_scope.record_index,
            local_ordinal: 0,
            evaluated_value: 0.04,
            evaluated_value_offset: 40,
            parameter_record_index: 98,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 99,
        },
    )
    .unwrap();
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &chamfer_scope,
            std::slice::from_ref(&indexed_owner),
        ),
        None
    );

    let revolve_start = bytes.len();
    let mut revolve = vec![0; 386];
    revolve[25..29].copy_from_slice(&4u32.to_le_bytes());
    revolve[29..33].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&revolve);
    let revolve_scalar_start = bytes.len();
    for (record_index, ordinal, value) in [(1_779u32, 0u8, 3.5f64), (1_780, 1, 0.0)] {
        let mut scalar = vec![0; 105];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"321");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"265");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut revolve_scope = scope.clone();
    revolve_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(revolve_start);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Revolve
                .try_into()
                .unwrap();
            draft.frame_length = 386;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                200, 201, 202, 203, 1_779, 1_780, 204,
            ]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let revolve_construction = exact_path_feature_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &revolve_scope,
        &[],
    );
    assert_eq!(
        revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(revolve_start + 25),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(3.5).unwrap()
                ),
                angle_record_index: 1_779,
                angle_offset: u64_from_index(revolve_scalar_start + 40),
                second_side: None,
                opposite_angle: Some(crate::records::identity::Located {
                    value: 1_780,
                    offset: u64_from_index(revolve_scalar_start + 116 + 40)
                }),
                solid: None,
                reversed: false,
            }
        ))
    );

    let indexed_revolve_start = bytes.len();
    let indexed_angle_record_index = 1_790u32;
    let mut indexed_revolve = vec![0; 377];
    indexed_revolve[21..25].copy_from_slice(&2u32.to_le_bytes());
    indexed_revolve[25..29].copy_from_slice(&2u32.to_le_bytes());
    indexed_revolve[30..34].copy_from_slice(&1u32.to_le_bytes());
    indexed_revolve[34] = 1;
    indexed_revolve[35..43].copy_from_slice(&u64::from(indexed_angle_record_index).to_le_bytes());
    bytes.extend_from_slice(&indexed_revolve);
    let mut indexed_revolve_scope = revolve_scope.clone();
    indexed_revolve_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(indexed_revolve_start);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    indexed_revolve_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("407".to_owned()).unwrap();
    indexed_revolve_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    indexed_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 377;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                200, 201, 202, 203, 204, 205, 1_790, 1_791,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let indexed_angle = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: indexed_revolve_scope.id.clone(),
            byte_offset: 5,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("372".to_owned())
                .unwrap(),
            record_index: indexed_angle_record_index,
            scope_record_index: indexed_revolve_scope.record_index,
            local_ordinal: 0,
            evaluated_value: std::f64::consts::TAU,
            evaluated_value_offset: 45,
            parameter_record_index: 1_791,
            owned_ordinal: 8,
            variant: Some(0),
            companion_record_index: 1_792,
        },
    )
    .unwrap();
    let indexed_revolve_construction = exact_path_feature_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &indexed_revolve_scope,
        std::slice::from_ref(&indexed_angle),
    );
    assert_eq!(
        indexed_revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::Cut,
                operation_offset: u64_from_index(indexed_revolve_start + 21),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap()
                ),
                angle_record_index: indexed_angle_record_index,
                angle_offset: 45,
                second_side: None,
                opposite_angle: None,
                solid: None,
                reversed: false,
            }
        ))
    );

    let class403_start = bytes.len();
    let mut class403_revolve = vec![0; 387];
    class403_revolve[21..25].copy_from_slice(&2u32.to_le_bytes());
    class403_revolve[25..29].copy_from_slice(&2u32.to_le_bytes());
    class403_revolve[29..31].copy_from_slice(&[0, 1]);
    class403_revolve[34] = 1;
    class403_revolve[35..39].copy_from_slice(&indexed_angle_record_index.to_le_bytes());
    let mut class403_guid = Vec::new();
    lp_utf16(&mut class403_guid, "00000000-0000-0000-0000-000000000000");
    class403_revolve[107..183].copy_from_slice(&class403_guid);
    bytes.extend_from_slice(&class403_revolve);
    let mut class403_scope = revolve_scope.clone();
    class403_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("403".to_owned()).unwrap();
    class403_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    class403_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(class403_start);
            draft.frame_length = 387;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                200,
                201,
                202,
                203,
                204,
                205,
                206,
                indexed_angle_record_index,
            ]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut class403_angle = indexed_angle.clone();
    {
        let mut wire =
            crate::records::parameters::DesignParameterOwnerWire::from(class403_angle.clone());
        wire.scope_record_index = class403_scope.record_index;
        wire.byte_offset = u64_from_index(class403_start);
        wire.evaluated_value_offset = u64_from_index(class403_start + 40);
        class403_angle = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &class403_scope,
            std::slice::from_ref(&class403_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::Cut,
                operation_offset: u64_from_index(class403_start + 21),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap()
                ),
                angle_record_index: indexed_angle_record_index,
                angle_offset: u64_from_index(class403_start + 40),
                second_side: None,
                opposite_angle: None,
                solid: None,
                reversed: false,
            }
        ))
    );
    bytes[class403_start + 34] = 0;
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &class403_scope,
            std::slice::from_ref(&class403_angle),
        ),
        None
    );
    bytes[class403_start + 34] = 1;
    let mut localized_class403_scope = class403_scope.clone();
    localized_class403_scope
        .try_edit(|draft| {
            draft.frame_length = 393;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    localized_class403_scope.source_kind = Some("R\u{e9}volution".to_owned());
    let class403_construction = |scope| {
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            scope,
            std::slice::from_ref(&class403_angle),
        )
    };
    assert!(class403_construction(&class403_scope).is_some());
    assert_eq!(
        class403_construction(&localized_class403_scope),
        class403_construction(&class403_scope)
    );

    {
        use crate::layout::referenced_angle_revolve_prologue as referenced_layout;

        let referenced_start = bytes.len();
        let mut referenced = vec![0; 365];
        referenced[referenced_layout::MARKER] = 1;
        referenced[referenced_layout::OPERATION..referenced_layout::OPERATION + 4]
            .copy_from_slice(&4u32.to_le_bytes());
        referenced[referenced_layout::EXTENT_KIND..referenced_layout::EXTENT_KIND + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        referenced[referenced_layout::PROJECT_AXIS] = 1;
        referenced[referenced_layout::SOLID] = 1;
        referenced[referenced_layout::ANGLE_REFERENCE_MARKER] = 1;
        referenced[referenced_layout::ANGLE_REFERENCE_MARKER + 1
            ..referenced_layout::ANGLE_REFERENCE_MARKER + 5]
            .copy_from_slice(&indexed_angle_record_index.to_le_bytes());
        bytes.extend_from_slice(&referenced);
        let mut referenced_scope = revolve_scope.clone();
        referenced_scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(referenced_start);
                draft.frame_length = 365;
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                    200,
                    201,
                    202,
                    203,
                    indexed_angle_record_index,
                    206,
                ]);
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let construction = |bytes: &[u8]| {
            exact_path_feature_construction(
                bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(bytes),
                &referenced_scope,
                std::slice::from_ref(&class403_angle),
            )
        };
        let expected = Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(referenced_start + referenced_layout::OPERATION),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap(),
                ),
                angle_record_index: indexed_angle_record_index,
                angle_offset: u64_from_index(class403_start + 40),
                second_side: None,
                opposite_angle: None,
                solid: Some(true),
                reversed: false,
            },
        ));
        assert_eq!(construction(&bytes), expected);
        // The Project Axis flag is `0` or `1`; any other value is not a flag.
        bytes[referenced_start + referenced_layout::PROJECT_AXIS] = 0;
        assert_eq!(construction(&bytes), expected);
        bytes[referenced_start + referenced_layout::PROJECT_AXIS] = 2;
        assert_eq!(construction(&bytes), None);
        bytes[referenced_start + referenced_layout::PROJECT_AXIS] = 1;
        // A localized kind such as `Révolution` lengthens the frame by two
        // bytes per extra code unit; the token-independent frame is the same.
        let mut localized_scope = referenced_scope.clone();
        localized_scope
            .try_edit(|draft| {
                draft.frame_length = 371;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let localized = |scope: &crate::records::feature::scope::DesignParameterScope| {
            exact_path_feature_construction(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                scope,
                std::slice::from_ref(&class403_angle),
            )
        };
        assert_eq!(localized(&localized_scope), None);
        localized_scope.source_kind = Some("R\u{e9}volution".to_owned());
        assert_eq!(localized(&localized_scope), expected);
        bytes[referenced_start + referenced_layout::ANGLE_REFERENCE_MARKER + 1] ^= 1;
        assert_eq!(construction(&bytes), None);
        bytes[referenced_start + referenced_layout::ANGLE_REFERENCE_MARKER + 1] ^= 1;
    }

    {
        use crate::layout::two_sided_revolve_prologue as two_sided_layout;

        let (angle_record_index, opposite_record_index) = (1_810u32, 1_820u32);
        let two_sided_start = bytes.len();
        // A 372-byte token-independent frame with the seven-code-unit kind.
        let mut two_sided = vec![0; 386];
        two_sided[two_sided_layout::MARKER] = 1;
        two_sided[two_sided_layout::OPERATION..two_sided_layout::OPERATION + 4]
            .copy_from_slice(&4u32.to_le_bytes());
        two_sided[two_sided_layout::EXTENT_KIND..two_sided_layout::EXTENT_KIND + 4]
            .copy_from_slice(&2u32.to_le_bytes());
        two_sided[two_sided_layout::SOLID] = 1;
        two_sided[two_sided_layout::TWO_SIDES] = 1;
        for (marker, record_index) in [
            (
                two_sided_layout::OPPOSITE_ANGLE_REFERENCE_MARKER,
                opposite_record_index,
            ),
            (two_sided_layout::ANGLE_REFERENCE_MARKER, angle_record_index),
        ] {
            two_sided[marker] = 1;
            two_sided[marker + 1..marker + 5].copy_from_slice(&record_index.to_le_bytes());
        }
        bytes.extend_from_slice(&two_sided);
        let mut two_sided_scope = revolve_scope.clone();
        two_sided_scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(two_sided_start);
                draft.frame_length = 386;
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                    200,
                    201,
                    202,
                    203,
                    angle_record_index,
                    opposite_record_index,
                    206,
                ]);
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        // The owners reuse the class-403 owner frame; the second sits 200
        // bytes later.
        let along_offset = class403_angle.evaluated_value_offset();
        let owner = |record_index, local_ordinal, value: f64, shift: u64| {
            let mut wire =
                crate::records::parameters::DesignParameterOwnerWire::from(class403_angle.clone());
            wire.record_index = record_index;
            wire.parameter_record_index = record_index + 1;
            wire.companion_record_index = record_index + 2;
            wire.scope_record_index = two_sided_scope.record_index;
            wire.local_ordinal = local_ordinal;
            wire.evaluated_value = value;
            wire.byte_offset += shift;
            wire.evaluated_value_offset += shift;
            crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap()
        };
        let construction = |bytes: &[u8], along: f64, against: f64| {
            exact_path_feature_construction(
                bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(bytes),
                &two_sided_scope,
                &[
                    owner(angle_record_index, 0, along, 0),
                    owner(opposite_record_index, 1, against, 200),
                ],
            )
        };
        let expected = |symmetric, along: f64, against: f64| {
            Some(DesignPathFeatureConstruction::Revolve(
                crate::records::feature::path_features::DesignRevolveConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: u64_from_index(two_sided_start + two_sided_layout::OPERATION),
                    travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                        cadmpeg_ir::scalar::PositiveAngle::new(along).unwrap(),
                    ),
                    angle_record_index,
                    angle_offset: along_offset,
                    opposite_angle: None,
                    second_side: Some(
                        crate::records::feature::path_features::DesignRevolveSecondSide {
                            symmetric,
                            angle: cadmpeg_ir::scalar::PositiveAngle::new(against).unwrap(),
                            angle_record_index: opposite_record_index,
                            angle_offset: along_offset + 200,
                        },
                    ),
                    solid: None,
                    reversed: false,
                },
            ))
        };
        let (sixty, thirty) = (std::f64::consts::FRAC_PI_3, std::f64::consts::FRAC_PI_6);
        assert_eq!(
            construction(&bytes, sixty, thirty),
            expected(false, sixty, thirty)
        );
        bytes[two_sided_start + two_sided_layout::TWO_SIDES] = 0;
        bytes[two_sided_start + two_sided_layout::SYMMETRIC] = 1;
        assert_eq!(
            construction(&bytes, sixty, sixty),
            expected(true, sixty, sixty)
        );
        // Symmetric mirrors one angle, so its two owners agree.
        assert_eq!(construction(&bytes, sixty, thirty), None);
        // Exactly one of the Symmetric and Two Sides flags is set.
        bytes[two_sided_start + two_sided_layout::TWO_SIDES] = 1;
        assert_eq!(construction(&bytes, sixty, sixty), None);
        bytes[two_sided_start + two_sided_layout::TWO_SIDES] = 0;
        // A surface revolve is not this form.
        bytes[two_sided_start + two_sided_layout::SOLID] = 0;
        assert_eq!(construction(&bytes, sixty, sixty), None);
        bytes[two_sided_start + two_sided_layout::SOLID] = 1;
    }

    let legacy_revolve_start = bytes.len();
    let legacy_angle_record_index = 1_800u32;
    let mut legacy_revolve = vec![0; 359];
    legacy_revolve[20] = 1;
    legacy_revolve[25..29].copy_from_slice(&4u32.to_le_bytes());
    legacy_revolve[29..33].copy_from_slice(&2u32.to_le_bytes());
    legacy_revolve[34..38].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&legacy_revolve);
    let mut legacy_revolve_scope = revolve_scope.clone();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(legacy_revolve_start);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    legacy_revolve_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("409".to_owned()).unwrap();
    legacy_revolve_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("257".to_owned()).unwrap();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 359;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                200,
                201,
                202,
                203,
                legacy_angle_record_index,
                204,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let legacy_angle = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: legacy_revolve_scope.id.clone(),
            byte_offset: 15,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("372".to_owned())
                .unwrap(),
            record_index: legacy_angle_record_index,
            scope_record_index: legacy_revolve_scope.record_index,
            local_ordinal: 0,
            evaluated_value: std::f64::consts::TAU,
            evaluated_value_offset: 55,
            parameter_record_index: 1_801,
            owned_ordinal: 8,
            variant: Some(0),
            companion_record_index: 1_802,
        },
    )
    .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(legacy_revolve_start + 25),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap()
                ),
                angle_record_index: legacy_angle_record_index,
                angle_offset: 55,
                second_side: None,
                opposite_angle: None,
                solid: None,
                reversed: false,
            }
        ))
    );
    legacy_revolve_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("323".to_owned()).unwrap();
    legacy_revolve_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 381;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                legacy_angle_record_index,
                200,
                201,
                202,
                203,
                204,
                205,
                206,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(legacy_revolve_start + 25),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap()
                ),
                angle_record_index: legacy_angle_record_index,
                angle_offset: 55,
                second_side: None,
                opposite_angle: None,
                solid: None,
                reversed: false,
            }
        ))
    );
    legacy_revolve_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("385".to_owned()).unwrap();
    legacy_revolve_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 369;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                200,
                201,
                202,
                203,
                legacy_angle_record_index,
                204,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::path_features::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(legacy_revolve_start + 25),
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU).unwrap()
                ),
                angle_record_index: legacy_angle_record_index,
                angle_offset: 55,
                second_side: None,
                opposite_angle: None,
                solid: None,
                reversed: false,
            }
        ))
    );
    revolve_scope.id = "stream:scope".into();
    {
        let value = revolve_construction;
        revolve_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let mut revolve_profile = thicken_group.clone();
    revolve_profile.id = "stream:profile".into();
    revolve_profile.scope_record_index = revolve_scope.record_index;
    revolve_profile.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let mut revolve_axis = revolve_profile.clone();
    revolve_axis.id = "stream:axis".into();
    revolve_axis.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X21,
        );
    assert_eq!(
        crate::design::feature_project::project_fixed_revolve_with_entities(
            &revolve_scope,
            &[revolve_profile, revolve_axis],
            &[],
            &[],
            &[],
            (&[], &[]),
        ),
        None
    );

    indexed_revolve_scope.id = "stream:indexed-revolve".into();
    {
        let value = indexed_revolve_construction;
        indexed_revolve_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let mut indexed_profile = thicken_group.clone();
    indexed_profile.id = "stream:indexed-profile".into();
    indexed_profile.scope_record_index = indexed_revolve_scope.record_index;
    indexed_profile.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let mut indexed_axis = indexed_profile.clone();
    indexed_axis.id = "stream:indexed-axis".into();
    indexed_axis.record_index = 899;
    indexed_axis
        .try_set_members(vec![crate::records::identity::Located {
            value: 900,
            offset: indexed_axis.members()[0].offset,
        }])
        .unwrap();
    indexed_axis.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X21,
        );
    let mut indexed_bodies = indexed_profile.clone();
    indexed_bodies.id = "stream:indexed-bodies".into();
    indexed_bodies.record_index = 901;
    indexed_bodies.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_A,
        );
    let mut axis_selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "stream:indexed-axis-selection".into(),
                scope_record_index: indexed_revolve_scope.record_index,
                group_record_index: indexed_axis.record_index,
                group_member_ordinal: 0,
                record_index: 900,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 0,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 0,
                identity_record_index: 903,
                identity_record_offset: 0,
                primary_identity: 100,
                primary_identity_offset: 29,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: crate::records::identity::Located {
                        value: 104,
                        offset: 37,
                    },
                    curve_identity: None,
                }),
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 904,
                next_byte_offset: 45,
            },
        )
        .unwrap();
    let axis_placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: "stream:indexed-axis-placement".into(),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_100".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 904,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned())
            .unwrap(),
    };
    let axis_curve = SketchCurveIdentity {
        id: "stream:indexed-axis-curve".into(),
        record_index: 905,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("450".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(104).unwrap(),
        secondary_id: 0,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(1.0, 2.0, 3.0),
                Point3::new(1.0, -3.0, 3.0),
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        planar_z_noise: None,
    };
    let projected = crate::design::feature_project::project_fixed_revolve_with_entities(
        &indexed_revolve_scope,
        &[
            indexed_profile.clone(),
            indexed_axis.clone(),
            indexed_bodies.clone(),
        ],
        &[],
        std::slice::from_ref(&axis_selection),
        &[],
        (&[axis_placement], &[axis_curve]),
    );
    assert!(matches!(
        projected,
        Some(FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            op: cadmpeg_ir::features::BooleanOp::Cut,
        })) if construction.axis().is_some_and(|axis|
            axis.origin == Point3::new(1.0, 2.0, 3.0)
                && axis.direction == Vector3::new(0.0, -1.0, 0.0))
    ));
    let mut draft = axis_selection.into_draft();
    draft.secondary = None;
    draft.primary_identity_offset = draft.identity_record_offset + 21;
    draft.next_byte_offset = draft.identity_record_offset + 29;
    axis_selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(draft)
            .unwrap();
    axis_selection.historical_face_candidates = vec![
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind::Face,
                entity_ref: 40,
                state_ids: vec![1],
            },
            face_slot: 40,
        },
    ];
    let historical_definition =
        crate::design::feature_project::project_fixed_revolve_with_entities(
            &indexed_revolve_scope,
            &[
                indexed_profile.clone(),
                indexed_axis.clone(),
                indexed_bodies.clone(),
            ],
            &[],
            std::slice::from_ref(&axis_selection),
            &[],
            (&[], &[]),
        )
        .unwrap();
    assert!(matches!(
        historical_definition,
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_none()
    ));
    let mut feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: crate::ids::neutral_feature_id(&indexed_revolve_scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(historical_definition),
        native_ref: Some(indexed_revolve_scope.id.clone()),
    };
    let surface_id =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#53").expect("identity grammar");
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile.clone(), indexed_axis.clone()],
        std::slice::from_ref(&axis_selection),
        &[],
        &[cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#40").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:shell#1").expect("identity grammar"),
            surface: surface_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        }],
        &[cadmpeg_ir::geometry::Surface {
            id: surface_id,
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(4.0, 5.0, 6.0),
                    Vector3::new(0.0, 0.0, -2.0).unit().unwrap(),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        }],
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_some_and(|axis|
            axis.origin == Point3::new(4.0, 5.0, 6.0)
                && axis.direction == Vector3::new(0.0, 0.0, -1.0))
    ));

    let face_axis_operand =
        DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
            id: "stream:indexed-face-axis".into(),
            scope_record_index: indexed_revolve_scope.record_index,
            scope_reference_ordinal: 2,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: indexed_axis.record_index,
                group_member_ordinal: 0,
            }),
            record_index: 900,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            paired_byte_offset: 16,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "262".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 903,
            recipe_record_byte_offset: 32,
            recipe_id: "stream:indexed-face-axis-recipe".into(),
            recipe_prefix_offset: 43,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: 0,
            recipe_program: vec![0, -1],

            recipe_nodes: Vec::new(),
            candidate_faces: vec![
                cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-a").expect("identity grammar"),
                cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-b").expect("identity grammar"),
            ],
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 905,
            next_byte_offset: 200,
        })
        .unwrap();
    let face_axis_definition = crate::design::feature_project::project_fixed_revolve_with_entities(
        &indexed_revolve_scope,
        &[
            indexed_profile.clone(),
            indexed_axis.clone(),
            indexed_bodies,
        ],
        &[],
        &[],
        std::slice::from_ref(&face_axis_operand),
        (&[], &[]),
    )
    .expect("face-recipe axis retains a neutral Revolve before geometry binding");
    let mut face_axis_feature = cadmpeg_ir::features::Feature {
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            face_axis_definition.clone(),
            cadmpeg_ir::features::DistinctMembers::try_from(
                (feature.clone()).evaluation.outputs().clone(),
            )
            .unwrap(),
        ),
        ..feature.clone()
    };
    let axis_faces = [
        cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-a").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:shell#axis")
                .expect("identity grammar"),
            surface: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-a")
                .expect("identity grammar"),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        },
        cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-b").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:shell#axis")
                .expect("identity grammar"),
            surface: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-b")
                .expect("identity grammar"),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        },
    ];
    let mut axis_surfaces = [
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-a")
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::new(1.0, 2.0, 3.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        4.0,
                    )
                    .unwrap(),
                ),
            ),
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-b")
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        Point3::new(1.0, 2.0, 8.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        5.0,
                    )
                    .unwrap(),
                ),
            ),
            source_object: None,
        },
    ];
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut face_axis_feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile.clone(), indexed_axis.clone()],
        &[],
        std::slice::from_ref(&face_axis_operand),
        &axis_faces,
        &axis_surfaces,
    );
    assert!(matches!(
        face_axis_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_some_and(|axis|
            axis.origin == Point3::new(1.0, 2.0, 3.0)
                && axis.direction == Vector3::new(0.0, 0.0, 1.0))
    ));
    let mut conflicting_face_axis_feature = cadmpeg_ir::features::Feature {
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            face_axis_definition,
            cadmpeg_ir::features::DistinctMembers::try_from((feature).evaluation.outputs().clone())
                .unwrap(),
        ),
        ..feature
    };
    axis_surfaces[1].geometry =
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(2.0, 2.0, 8.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        ));
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut conflicting_face_axis_feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile, indexed_axis],
        &[],
        std::slice::from_ref(&face_axis_operand),
        &axis_faces,
        &axis_surfaces,
    );
    assert!(matches!(
        conflicting_face_axis_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_none()
    ));

    (bytes, scope)
}

fn make_linked_param(
    scope: &DesignParameterScope,
    lane_record_index: u32,
    source_kind: &str,
    value: f64,
) -> (DesignParameter, DesignParameterOwner) {
    let param_record_index = lane_record_index + 1;
    let stream = crate::ids::native_stream(&scope.id).unwrap_or("f3d:Design/BulkStream.dat");
    let base = u64::from(param_record_index) * 200 + 100;
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: format!("{stream}:design-parameter#{param_record_index}"),
        byte_offset: base,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index: param_record_index,
        source_ordinal: 0,
        source: DesignParameterSource::new(source_kind.to_owned(), Some(lane_record_index), None)
            .unwrap(),
        expression: format!("{value}"),
        expression_offset: base + 20,
        source_kind_offset: base + 40,
        unit: None,
        name: source_kind.into(),
        name_offset: base + 60,
        evaluated_value: value,
        evaluated_value_offset: base + 80,
    })
    .unwrap();
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{stream}:design-parameter-owner#{lane_record_index}"),
        byte_offset: base.saturating_sub(99),
        frame_length: 99,
        class_tag: "268".to_owned().try_into().unwrap(),
        record_index: lane_record_index,
        scope_record_index: scope.record_index,
        local_ordinal: 0,
        evaluated_value: value,
        evaluated_value_offset: base.saturating_sub(59),
        parameter_record_index: param_record_index,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: param_record_index + 1,
    })
    .unwrap();
    (parameter, owner)
}
