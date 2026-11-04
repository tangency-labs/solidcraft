// SPDX-License-Identifier: Apache-2.0

pub(super) fn native(valid: bool, duplicate: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::{Located, ReferenceRun},
        topology::construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
    };
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        DesignFeatureKind::Hole,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![100, 101]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{stream}:design-construction-operand-group#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: "277".to_owned().try_into().unwrap(),
        members: vec![Located {
            value: 101,
            offset: 1_026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1_021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1_058,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1_062,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
        ),
        role_offset: 1_040,
        paired_class_tag: "258".to_owned().try_into().unwrap(),
        paired_byte_offset: 1_080,
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native
        .design_construction_operand_groups
        .push(group.clone());
    if duplicate {
        native.design_construction_operand_groups.push(group);
    }
    if valid {
        native.design_parameter_scopes.push(scope);
        for (index, offset) in [(100, 1_000), (101, 1_100)] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{index}"),
                record_index: index,
                class_tag: "277".to_owned().try_into().unwrap(),
                byte_offset: offset,
            });
        }
    }
    native
}

fn tail_findings(byte_offset: u64, flag: bool) -> Vec<cadmpeg_ir::report::check::Finding> {
    reference_findings(byte_offset, flag, 10, 1_030)
}

fn reference_findings(
    byte_offset: u64,
    flag: bool,
    reference_span: u64,
    reference_offset: u64,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    use crate::records::{
        decal::DesignRecordHeader,
        identity::Located,
        sketch_placement::SketchPlacementMatrix,
        topology::construction::{
            DesignConstructionOperandDualTransform, DesignConstructionOperandFlag,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        },
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = native(true, false);
        let class_tag: crate::records::references::DesignClassTag =
            "280".to_owned().try_into().unwrap();
        let mut frame = DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1_021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: vec![Located {
                    value: 102,
                    offset: reference_offset,
                }],
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1_048 + reference_span,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1_052 + reference_span,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap();
        if flag {
            frame
                .try_set_trailing_flags(vec![DesignConstructionOperandFlag {
                    record_index: 102,
                    byte_offset,
                    class_tag: class_tag.clone(),
                    value: true,
                    value_offset: byte_offset.saturating_add(22),
                }])
                .unwrap();
        } else {
            frame
                .try_set_trailing_dual_transforms(vec![DesignConstructionOperandDualTransform {
                    record_index: 102,
                    byte_offset,
                    class_tag: class_tag.clone(),
                    first_transform: SketchPlacementMatrix::IDENTITY,
                    first_transform_offset: byte_offset.saturating_add(21),
                    second_transform: SketchPlacementMatrix::IDENTITY,
                    second_transform_offset: byte_offset.saturating_add(149),
                }])
                .unwrap();
        }
        native.design_construction_operand_groups[0].frame = frame;
        native.design_construction_operand_groups[0].paired_byte_offset = 1_070 + reference_span;
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#102".into(),
            record_index: 102,
            class_tag,
            byte_offset,
        });
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::super::validate_construction_operand_groups(&ctx, &mut findings);
        findings
    })
}

#[test]
fn construction_group_accepts_short_and_typed_reference_spans() {
    for span in [10, 50] {
        assert!(reference_findings(2_000, true, span, 1_030).is_empty());
    }
}

#[test]
fn construction_group_rejects_mutated_reference_offsets() {
    for span in [10, 50] {
        for offset in [1_029, 1_031, u64::MAX] {
            let findings = reference_findings(2_000, true, span, offset);
            assert_eq!(findings.len(), 1);
            assert_eq!(
                findings[0].message,
                "Fusion Design construction operand group has an invalid frame"
            );
        }
    }
}

#[test]
fn construction_group_preserves_representable_tail_offsets() {
    assert!(tail_findings(2_000, false).is_empty());
    assert!(tail_findings(2_000, true).is_empty());
}

#[test]
fn construction_group_rejects_overflowed_first_transform() {
    assert_eq!(tail_findings(u64::MAX, false).len(), 1);
}

#[test]
fn construction_group_rejects_overflowed_second_transform() {
    assert_eq!(tail_findings(u64::MAX - 100, false).len(), 1);
}

#[test]
fn construction_group_rejects_overflowed_flag() {
    assert_eq!(tail_findings(u64::MAX, true).len(), 1);
}

#[test]
fn hole_construction_group_accepts_its_retained_auxiliary_role() {
    use crate::records::topology::construction::DesignConstructionOperandRole;
    use crate::records::topology::extrude_selection::DesignOperandRole;
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = native(true, false);
        native.design_construction_operand_groups[0].operand_role =
            DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X21);
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::super::validate_construction_operand_groups(&ctx, &mut findings);
        assert!(findings.is_empty());
        native.design_construction_operand_groups[0].operand_role =
            DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        super::super::validate_construction_operand_groups(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
    });
}

#[test]
fn move_face_group_requires_the_selected_face_role() {
    use crate::records::{
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::ReferenceRun,
        topology::{
            construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole,
        },
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        for kind in [DesignFeatureKind::MoveFace, DesignFeatureKind::DeplacerFace] {
            let mut native = native(true, false);
            let mut scope = DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#10",
                kind,
                10,
            );
            scope
                .try_edit(|draft| {
                    draft.reference_members = ReferenceRun::unlocated(vec![100, 101, 300]);
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                    draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            native.design_parameter_scopes = vec![scope];
            native.design_construction_operand_groups[0].operand_role =
                DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X10);
            let ctx = super::super::Ctx::new(&ir, &native, decode);
            let mut findings = Vec::new();
            super::super::validate_construction_operand_groups(&ctx, &mut findings);
            assert!(findings.is_empty(), "{findings:?}");
            native.design_construction_operand_groups[0].operand_role =
                DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
            let ctx = super::super::Ctx::new(&ir, &native, decode);
            super::super::validate_construction_operand_groups(&ctx, &mut findings);
            assert_eq!(findings.len(), 1);
        }
    });
}

#[test]
fn thread_face_group_does_not_require_a_specialized_thread_payload() {
    use crate::records::{
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::ReferenceRun,
        topology::{
            construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole,
        },
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let mut native = native(true, false);
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Thread,
            10,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![100, 101]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        native.design_parameter_scopes = vec![scope];
        native.design_construction_operand_groups[0].operand_role =
            DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X10);
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::super::validate_construction_operand_groups(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        native.design_construction_operand_groups[0].operand_role =
            DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        super::super::validate_construction_operand_groups(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
    });
}

#[test]
fn revolve_target_group_requires_a_scope_without_an_angle_construction() {
    use crate::records::{
        feature::{
            extrude::DesignExtrudeOperation,
            path_features::{DesignPathFeatureConstruction, DesignRevolveConstruction},
            scope::{DesignFeatureKind, DesignParameterScope},
        },
        identity::ReferenceRun,
        topology::{
            construction::DesignConstructionOperandRole, extrude_selection::DesignOperandRole,
        },
    };
    let scope = |angle: bool| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Revolve,
            10,
        );
        scope
            .try_edit(|draft| {
                if angle {
                    draft.payload =
                        DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                            operation: DesignExtrudeOperation::NewBody,
                            operation_offset: 0,
                            travel:
                                crate::records::feature::path_features::DesignRevolveTravel::Angle(
                                    cadmpeg_ir::scalar::PositiveAngle::new(1.0).unwrap(),
                                ),
                            angle_record_index: 300,
                            angle_offset: 0,
                            second_side: None,
                            opposite_angle: None,
                            solid: None,
                            reversed: false,
                        })
                        .into();
                }
                draft.reference_members = ReferenceRun::unlocated(vec![100, 101]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        for (angle, role, finding_count) in [
            (false, DesignOperandRole::ROLE_0X5, 0),
            (true, DesignOperandRole::ROLE_0X5, 1),
            (false, DesignOperandRole::ROLE_0X10, 1),
        ] {
            let mut native = native(true, false);
            native.design_parameter_scopes = vec![scope(angle)];
            native.design_construction_operand_groups[0].operand_role =
                DesignConstructionOperandRole::Other(role);
            let ctx = super::super::Ctx::new(&ir, &native, decode);
            let mut findings = Vec::new();
            super::super::validate_construction_operand_groups(&ctx, &mut findings);
            assert_eq!(
                findings.len(),
                finding_count,
                "{angle} {role:?}: {findings:?}"
            );
        }
    });
}

/// A pattern scope with a first group of role `first` over reference one and,
/// when given, a second group of role `second` over reference three.
fn pattern_native(
    kind: crate::records::feature::scope::DesignFeatureKind,
    first: crate::records::topology::extrude_selection::DesignOperandRole,
    second: Option<crate::records::topology::extrude_selection::DesignOperandRole>,
) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader, feature::scope::DesignParameterScope, identity::ReferenceRun,
        topology::construction::DesignConstructionOperandRole,
    };
    let stream = "f3d:Design/BulkStream.dat";
    let mut native = native(true, false);
    let mut scope =
        DesignParameterScope::empty(&format!("{stream}:design-parameter-scope#10"), kind, 10);
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(if second.is_some() {
                vec![100, 101, 102, 103]
            } else {
                vec![100, 101]
            });
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    native.design_parameter_scopes = vec![scope];
    native.design_construction_operand_groups[0].operand_role =
        DesignConstructionOperandRole::Other(first);
    if let Some(second) = second {
        let mut group =
            serde_json::to_value(&native.design_construction_operand_groups[0]).unwrap();
        group["id"] = format!("{stream}:design-construction-operand-group#2000").into();
        group["record_index"] = 102.into();
        group["scope_reference_ordinal"] = 2.into();
        group["byte_offset"] = 2_000.into();
        group["member_offsets"] = serde_json::json!([2_026]);
        group["members"] = serde_json::json!([103]);
        group["frame"]["member_count_offset"] = 2_021.into();
        group["frame"]["opaque_index_offset"] = 2_058.into();
        group["frame"]["opaque_scalar_offset"] = 2_062.into();
        group["role_offset"] = 2_040.into();
        group["paired_byte_offset"] = 2_080.into();
        let mut group: crate::records::topology::construction::DesignConstructionOperandGroup =
            serde_json::from_value(group).unwrap();
        group.operand_role = DesignConstructionOperandRole::Other(second);
        native.design_construction_operand_groups.push(group);
        for (index, offset) in [(102, 2_000), (103, 2_100)] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{index}"),
                record_index: index,
                class_tag: "277".to_owned().try_into().unwrap(),
                byte_offset: offset,
            });
        }
    }
    native
}

fn invalid_group_records(native: &crate::native::F3dNative) -> Vec<String> {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let ctx = super::super::Ctx::new(&ir, native, decode);
        let mut findings = Vec::new();
        super::super::validate_construction_operand_groups(&ctx, &mut findings);
        findings
            .iter()
            .filter(|finding| {
                finding.message == "Fusion Design construction operand group has an invalid frame"
            })
            .map(|finding| finding.entity.clone().unwrap_or_default())
            .collect()
    })
}

#[test]
fn a_pattern_admits_a_selected_face_group_and_refuses_other_face_roles() {
    use crate::records::feature::scope::DesignFeatureKind::{CPattern, RPattern};
    use crate::records::topology::extrude_selection::DesignOperandRole as Role;
    for kind in [RPattern, CPattern] {
        for role in [Role::ROLE_0X10, Role::BODIES_A, Role::BODIES_B] {
            let native = pattern_native(kind.clone(), role, None);
            assert!(invalid_group_records(&native).is_empty(), "{role:?}");
        }
        for role in [Role::ROLE_0X5, Role::ROLE_0X21, Role::ROLE_0X9] {
            let native = pattern_native(kind.clone(), role, None);
            assert_eq!(invalid_group_records(&native).len(), 1, "{role:?}");
        }
    }
}

#[test]
fn a_mixed_pattern_scope_judges_each_group_by_its_own_role() {
    use crate::records::feature::scope::DesignFeatureKind::RPattern;
    use crate::records::topology::extrude_selection::DesignOperandRole as Role;
    let admitted = pattern_native(RPattern, Role::BODIES_A, Some(Role::ROLE_0X10));
    assert!(invalid_group_records(&admitted).is_empty());
    let refused = pattern_native(RPattern, Role::ROLE_0X10, Some(Role::ROLE_0X5));
    assert_eq!(
        invalid_group_records(&refused),
        ["f3d:Design/BulkStream.dat:design-construction-operand-group#2000"]
    );
}

#[test]
fn a_selected_face_group_stays_refused_outside_the_patterns() {
    use crate::records::feature::scope::DesignFeatureKind::Scale;
    use crate::records::topology::extrude_selection::DesignOperandRole as Role;
    let native = pattern_native(Scale, Role::ROLE_0X10, None);
    assert_eq!(invalid_group_records(&native).len(), 1);
}

/// A pattern scope whose only group is a path-carried seed: no member, two
/// auxiliary references that `paths` of which decode as construction-operand
/// paths, and one trailing reference.
fn path_seed_native(
    kind: crate::records::feature::scope::DesignFeatureKind,
    paths: usize,
    edit: impl FnOnce(&mut serde_json::Value),
) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        topology::{
            construction::{
                DesignConstructionOperandPath, DesignConstructionOperandPathDraft,
                DesignConstructionPathPlacement,
            },
            extrude_selection::DesignOperandRole,
        },
    };
    let stream = "f3d:Design/BulkStream.dat";
    let mut native = pattern_native(kind, DesignOperandRole::PATH_SEED, None);
    let mut group = serde_json::to_value(&native.design_construction_operand_groups[0]).unwrap();
    group["members"] = serde_json::json!([]);
    group["member_offsets"] = serde_json::json!([]);
    group["frame"]["auxiliary_record_indices"] = serde_json::json!([103, 106]);
    group["frame"]["auxiliary_record_offsets"] = serde_json::json!([1_026, 1_037]);
    group["frame"]["trailing_record_indices"] = serde_json::json!([109]);
    group["frame"]["trailing_record_offsets"] = serde_json::json!([1_030]);
    edit(&mut group);
    let mut group: crate::records::topology::construction::DesignConstructionOperandGroup =
        serde_json::from_value(group).unwrap();
    let path = |record_index: u32, byte_offset: u64, placement| {
        let scope_offset = byte_offset
            + match placement {
                DesignConstructionPathPlacement::Transform(_) => 198,
                DesignConstructionPathPlacement::Compact(_) => 70,
            };
        DesignConstructionOperandPath::try_new(DesignConstructionOperandPathDraft {
            record_index,
            byte_offset,
            class_tag: "390".to_owned().try_into().unwrap(),
            entity_ref: 55,
            entity_ref_offset: byte_offset + 57,
            placement,
            scope_record_index: 10,
            scope_record_index_offset: scope_offset,
            nested_record_index: record_index + 2,
            nested_record_index_offset: scope_offset + 11,
            following_record_index: record_index + 1,
            following_byte_offset: scope_offset + 27,
            following_class_tag: "295".to_owned().try_into().unwrap(),
        })
        .unwrap()
    };
    let decoded = [
        path(103, 1_200, DesignConstructionPathPlacement::Compact(true)),
        path(
            106,
            1_500,
            DesignConstructionPathPlacement::Transform(
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            ),
        ),
    ];
    group
        .frame
        .try_set_auxiliary_paths(decoded.into_iter().take(paths).collect())
        .unwrap();
    native.design_construction_operand_groups[0] = group;
    for (index, class_tag, offset) in [
        (103, "390", 1_200),
        (104, "295", 1_297),
        (105, "295", 1_400),
        (106, "390", 1_500),
        (107, "295", 1_725),
        (108, "295", 1_700),
        (109, "294", 1_800),
    ] {
        native.design_record_headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{index}"),
            record_index: index,
            class_tag: class_tag.to_owned().try_into().unwrap(),
            byte_offset: offset,
        });
    }
    native
}

#[test]
fn a_pattern_admits_a_path_carried_seed_group_without_members() {
    use crate::records::feature::scope::DesignFeatureKind::{CPattern, RPattern};
    for kind in [RPattern, CPattern] {
        assert!(invalid_group_records(&path_seed_native(kind, 2, |_| {})).is_empty());
    }
}

#[test]
fn a_path_carried_seed_group_keeps_its_exact_shape() {
    use crate::records::feature::scope::DesignFeatureKind::{Fillet, RPattern};
    let bare_references = path_seed_native(RPattern, 0, |_| {});
    let one_path = path_seed_native(RPattern, 1, |_| {});
    let variant = path_seed_native(RPattern, 2, |group| group["frame"]["variant"] = true.into());
    let member = path_seed_native(RPattern, 2, |group| {
        group["members"] = serde_json::json!([101]);
        group["member_offsets"] = serde_json::json!([1_026]);
    });
    let outside_patterns = path_seed_native(Fillet, 2, |_| {});
    for native in [bare_references, one_path, variant, member, outside_patterns] {
        assert_eq!(invalid_group_records(&native).len(), 1);
    }
}
