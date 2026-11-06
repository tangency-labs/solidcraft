// SPDX-License-Identifier: Apache-2.0

use super::{auxiliary_links_hold, links_hold};
use crate::records::feature::emboss::DesignEmbossConstruction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::identity::{Located, RecordedValue, ReferenceRun};
use crate::records::parameters::{
    DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
    DesignParameterSource,
};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::{
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionGroupWire, DesignOperandRole,
};

const STREAM: &str = "f3d:Design1/BulkStream.dat";

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#0"),
        DesignFeatureKind::Emboss,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 430;
            draft.paired_byte_offset = 430;
            draft.reference_count_offset = 235;
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 60, 63, 66, 69, 72]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.payload = DesignScopePayload::Emboss(Some(DesignEmbossConstruction {
                control_words: crate::records::feature::emboss::DesignEmbossControlWords::Two([
                    1, 0,
                ]),
                native_pair: Some([17, 23]),
                additional_native_pairs: None,
                depth_owner: Located {
                    value: 60,
                    offset: 29,
                },
                auxiliary_group: Located {
                    value: 72,
                    offset: 40,
                },
                horizontal_owner: Located {
                    value: 63,
                    offset: 51,
                },
                rotation_owner: Located {
                    value: 69,
                    offset: 82,
                },
                face_group: Located {
                    value: 48,
                    offset: 93,
                },
                vertical_owner: Located {
                    value: 66,
                    offset: 104,
                },
                profile_group: Located {
                    value: 41,
                    offset: 119,
                },
                sketch_profile: None,
            }));
        })
        .unwrap();
    scope
}

fn parameter(owner: u32, kind: &str, value: f64, unit: &str) -> DesignParameter {
    DesignParameter::try_from(DesignParameterDraft {
        id: format!("{STREAM}:design-parameter#{owner}"),
        byte_offset: 1000,
        class_tag: "900".to_owned().try_into().unwrap(),
        record_index: owner + 1,
        source_ordinal: 1,
        source: DesignParameterSource::new(kind.to_owned(), Some(owner), None).unwrap(),
        expression: "1".into(),
        expression_offset: 1020,
        source_kind_offset: 1040,
        unit: Some(RecordedValue {
            value: unit.to_owned(),
            offset: 1080,
        }),
        name: "Input".into(),
        name_offset: 1100,
        evaluated_value: value,
        evaluated_value_offset: 1120,
    })
    .unwrap()
}

fn owner(index: u32, value: f64) -> DesignParameterOwner {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:design-parameter-owner#{index}"),
        "byte_offset": 2000,
        "frame_length": 104,
        "class_tag": "901",
        "record_index": index,
        "scope_record_index": 40,
        "local_ordinal": 1,
        "evaluated_value": value,
        "evaluated_value_offset": 2040,
        "parameter_record_index": index + 1,
        "owned_ordinal": 1,
        "variant": 0,
        "companion_record_index": index + 2,
    }))
    .unwrap()
}

fn group(
    record: u32,
    ordinal: u32,
    member: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record}"),
        scope_record_index: 40,
        scope_reference_ordinal: ordinal,
        record_index: record,
        byte_offset: 3000,
        class_tag: "902".to_owned().try_into().unwrap(),
        members: vec![Located {
            value: member,
            offset: 3026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 3021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 3060,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 3064,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 3042,
        paired_class_tag: "903".to_owned().try_into().unwrap(),
        paired_byte_offset: 3112,
    })
    .unwrap()
}

#[test]
fn emboss_links_require_unique_scalar_owners_roles_units_and_group_members() {
    let scope = scope();
    let parameters = vec![
        parameter(60, "Depth", -0.125, "mm"),
        parameter(63, "Horizontal Distance", 0.0, "mm"),
        parameter(66, "Vertical Distance", 0.0, "mm"),
        parameter(69, "Rotation Angle", 0.0, "deg"),
    ];
    let owners = vec![
        owner(60, -0.125),
        owner(63, 0.0),
        owner(66, 0.0),
        owner(69, 0.0),
    ];
    let groups = vec![
        group(41, 0, 44, DesignOperandRole::PROFILE),
        group(48, 2, 51, DesignOperandRole::ROLE_0X10),
    ];
    assert!(links_hold(&scope, &parameters, &owners, &groups));
    assert!(!links_hold(&scope, &parameters[1..], &owners, &groups));
    assert!(!links_hold(&scope, &parameters, &owners[1..], &groups));
    assert!(!links_hold(&scope, &parameters, &owners, &groups[..1]));

    let mut changed = parameters.clone();
    changed[0] = parameter(60, "Horizontal Distance", -0.125, "mm");
    assert!(!links_hold(&scope, &changed, &owners, &groups));
    changed[0] = parameter(60, "Depth", -0.125, "deg");
    assert!(!links_hold(&scope, &changed, &owners, &groups));
    changed[0] = parameter(60, "Depth", 0.125, "mm");
    assert!(!links_hold(&scope, &changed, &owners, &groups));

    let mut duplicate_parameters = parameters.clone();
    duplicate_parameters.push(parameter(60, "Horizontal Distance", -0.125, "mm"));
    assert!(!links_hold(&scope, &duplicate_parameters, &owners, &groups));

    let mut duplicated = owners.clone();
    duplicated.push(owners[0].clone());
    assert!(!links_hold(&scope, &parameters, &duplicated, &groups));
    let mut wrong_scope: DesignParameterOwnerWire = owners[0].clone().into();
    wrong_scope.scope_record_index = 80;
    let mut changed_owners = owners.clone();
    changed_owners[0] = wrong_scope.try_into().unwrap();
    assert!(!links_hold(&scope, &parameters, &changed_owners, &groups));

    let wrong_groups = [
        group(41, 0, 44, DesignOperandRole::ROLE_0X10),
        group(48, 2, 51, DesignOperandRole::ROLE_0X10),
    ];
    assert!(!links_hold(&scope, &parameters, &owners, &wrong_groups));
    let wrong_member = [
        group(41, 0, 51, DesignOperandRole::PROFILE),
        group(48, 2, 51, DesignOperandRole::ROLE_0X10),
    ];
    assert!(!links_hold(&scope, &parameters, &owners, &wrong_member));
    let mut duplicate_groups = groups.clone();
    duplicate_groups.push(groups[0].clone());
    assert!(!links_hold(&scope, &parameters, &owners, &duplicate_groups));
}

fn auxiliary_group() -> DesignExtrudeSelectionGroup {
    DesignExtrudeSelectionGroup::try_from(DesignExtrudeSelectionGroupWire {
        id: format!("{STREAM}:design-extrude-selection-group#72"),
        scope_record_index: 40,
        scope_reference_ordinal: 8,
        record_index: 72,
        byte_offset: 4000,
        class_tag: "904".into(),
        member_count_offset: 4032,
        members: vec![80],
        member_offsets: vec![4037],
        opaque_index: 1,
        opaque_index_offset: 4047,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 4051,
        variant: Some(false),
        typed_references: false,
        paired_class_tag: "905".into(),
        paired_byte_offset: 4100,
    })
    .unwrap()
}

#[test]
fn emboss_auxiliary_link_requires_one_counted_selection_in_the_same_scope_and_stream() {
    let scope = scope();
    let group = auxiliary_group();
    assert!(auxiliary_links_hold(&scope, std::slice::from_ref(&group)));
    assert!(!auxiliary_links_hold(&scope, &[]));
    assert!(!auxiliary_links_hold(
        &scope,
        &[group.clone(), group.clone()]
    ));
    for field in [
        "scope_record_index",
        "scope_reference_ordinal",
        "record_index",
    ] {
        let mut wire: DesignExtrudeSelectionGroupWire = group.clone().into();
        match field {
            "scope_record_index" => wire.scope_record_index = 90,
            "scope_reference_ordinal" => wire.scope_reference_ordinal = 7,
            _ => wire.record_index = 90,
        }
        let changed = wire.try_into().unwrap();
        assert!(!auxiliary_links_hold(&scope, &[changed]), "{field}");
    }
    let mut wire: DesignExtrudeSelectionGroupWire = group.into();
    wire.id = "f3d:Design2/BulkStream.dat:design-extrude-selection-group#72".into();
    assert!(!auxiliary_links_hold(&scope, &[wire.try_into().unwrap()]));
}

#[test]
fn emboss_validation_reports_missing_profile_and_face_operand_frames() {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameter_scopes.push(scope());
        for (index, kind, unit) in [
            (60, "Depth", "mm"),
            (63, "Horizontal Distance", "mm"),
            (66, "Vertical Distance", "mm"),
            (69, "Rotation Angle", "deg"),
        ] {
            native
                .design_parameters
                .push(parameter(index, kind, 0.0, unit));
            native.design_parameter_owners.push(owner(index, 0.0));
        }
        native.design_construction_operand_groups = vec![
            group(41, 0, 44, DesignOperandRole::PROFILE),
            group(48, 2, 51, DesignOperandRole::ROLE_0X10),
        ];
        native
            .design_extrude_selection_groups
            .push(auxiliary_group());
        native
            .design_record_headers
            .push(crate::records::decal::DesignRecordHeader {
                id: format!("{STREAM}:record#72"),
                byte_offset: 4000,
                class_tag: "904".to_owned().try_into().unwrap(),
                record_index: 72,
            });
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::validate(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
    });
}

#[test]
fn emboss_auxiliary_members_require_record_headers_without_contour_frames() {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.design_parameter_scopes.push(scope());
        native
            .design_extrude_selection_groups
            .push(auxiliary_group());
        for (index, offset) in [(72, 4000), (80, 4200)] {
            native
                .design_record_headers
                .push(crate::records::decal::DesignRecordHeader {
                    id: format!("{STREAM}:record#{index}"),
                    byte_offset: offset,
                    class_tag: "904".to_owned().try_into().unwrap(),
                    record_index: index,
                });
        }
        let mut findings = Vec::new();
        {
            let ctx = super::super::Ctx::new(&ir, &native, decode);
            super::super::validate_extrude_selection_groups(&ctx, &mut findings);
            super::super::validate_extrude_selection_group_members(&ctx, &mut findings);
        }
        assert!(findings.is_empty());
        native.design_record_headers.pop();
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        super::super::validate_extrude_selection_groups(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
    });
}

fn counted_scope(
    one_word: bool,
    pair_count: usize,
    reference_count: usize,
) -> DesignParameterScope {
    use crate::records::feature::emboss::DesignEmbossControlWords;
    let ids = (0..reference_count)
        .map(|index| 100_001 + 7 * u32::try_from(index).unwrap())
        .collect::<Vec<_>>();
    let mut scope = scope();
    scope
        .try_edit(|draft| {
            draft.frame_length = 2048;
            draft.paired_byte_offset = 2048;
            let head = if one_word { 61 } else { 65 };
            let suffix = head + 16 * u64::try_from(pair_count).unwrap();
            draft.reference_count_offset = suffix + 154;
            draft.reference_members = ReferenceRun::unlocated(ids.clone());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            let DesignScopePayload::Emboss(Some(construction)) = &mut draft.payload else {
                unreachable!()
            };
            construction.control_words = if one_word {
                DesignEmbossControlWords::One([9])
            } else {
                DesignEmbossControlWords::Two([7, 9])
            };
            construction.native_pair = (pair_count > 0).then_some([8_000_003, 9_000_007]);
            construction.additional_native_pairs = (pair_count > 1).then(|| {
                (1..pair_count)
                    .map(|index| {
                        let ordinal = u64::try_from(index).unwrap();
                        [8_000_003 + ordinal, 9_000_007 + ordinal]
                    })
                    .collect()
            });
            let control_bytes = if one_word { 4 } else { 8 };
            construction.depth_owner = Located {
                value: ids[0],
                offset: 21 + control_bytes,
            };
            construction.auxiliary_group = Located {
                value: ids[2],
                offset: 32 + control_bytes,
            };
            construction.horizontal_owner = Located {
                value: ids[4],
                offset: 43 + control_bytes,
            };
            construction.rotation_owner = Located {
                value: ids[6],
                offset: suffix + 1,
            };
            construction.face_group = Located {
                value: ids[8],
                offset: suffix + 12,
            };
            construction.vertical_owner = Located {
                value: ids[10],
                offset: suffix + 23,
            };
            construction.profile_group = Located {
                value: ids[12],
                offset: suffix + 38,
            };
        })
        .unwrap();
    scope
}

#[test]
fn emboss_counted_validator_keeps_other_operands_native() {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for (one_word, pair_count, reference_count) in [
            (true, 0, 13),
            (false, 0, 19),
            (true, 2, 13),
            (false, 2, 19),
            (true, 5, 19),
            (false, 5, 13),
        ] {
            let mut native = crate::native::F3dNative::default();
            let scope = counted_scope(one_word, pair_count, reference_count);
            assert!(scope.emboss_construction().unwrap().links_match(&scope));
            assert!(scope.emboss_operand_construction().is_none());
            native.design_parameter_scopes.push(scope);
            let ctx = super::super::Ctx::new(&ir, &native, decode);
            let mut findings = Vec::new();
            super::validate(&ctx, &mut findings);
            assert!(findings.is_empty());
        }
    });
}

#[test]
fn emboss_counted_validator_refuses_inconsistent_offsets_counts_and_order() {
    for (one_word, pair_count) in [
        (true, 0),
        (false, 0),
        (true, 2),
        (false, 2),
        (true, 5),
        (false, 5),
    ] {
        let scope = counted_scope(one_word, pair_count, 13);
        let source = serde_json::to_value(&scope).unwrap();
        let decoded: DesignParameterScope = serde_json::from_value(source.clone()).unwrap();
        assert_eq!(decoded, scope);
        for field in [
            "depth_owner",
            "auxiliary_group",
            "horizontal_owner",
            "rotation_owner",
            "face_group",
            "vertical_owner",
            "profile_group",
        ] {
            let mut changed = source.clone();
            changed["emboss_construction"][field]["offset"] = serde_json::json!(1);
            assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());
        }
        for field in ["native_pair", "additional_native_pairs"] {
            let mut changed = source.clone();
            changed["emboss_construction"][field] = serde_json::Value::Null;
            let error = serde_json::from_value::<DesignParameterScope>(changed)
                .unwrap_err()
                .to_string();
            assert!(error.contains(field));
        }
        let mut changed = source.clone();
        changed["emboss_construction"]["additional_native_pairs"] = serde_json::Value::Array(
            (0..=pair_count)
                .map(|_| serde_json::json!([31, 37]))
                .collect(),
        );
        assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());
        let mut changed = source.clone();
        changed["reference_members"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());
        let mut changed = source;
        changed["reference_members"][12] = changed["reference_members"][0].clone();
        assert!(serde_json::from_value::<DesignParameterScope>(changed).is_err());
    }
}
