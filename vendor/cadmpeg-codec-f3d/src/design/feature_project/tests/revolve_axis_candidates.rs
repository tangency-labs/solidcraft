// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::path_features::{
    DesignPathFeatureConstruction, DesignRevolveConstruction,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::entity_selection::{
    DesignEntitySelectionEdgeCandidate, DesignEntitySelectionFaceCandidate,
    DesignEntitySelectionOperand,
};
use crate::records::topology::extrude_selection::DesignOperandRole;

const PRECEDING_STATE: i64 = 2;

fn scope() -> DesignParameterScope {
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Revolve, 12);
    scope
        .try_edit(|draft| {
            draft.payload = DesignPathFeatureConstruction::Revolve(DesignRevolveConstruction {
                operation: crate::records::feature::extrude::DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                travel: crate::records::feature::path_features::DesignRevolveTravel::Angle(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
                ),
                angle_record_index: 300,
                angle_offset: 0,
                second_side: None,
                opposite_angle: None,
                solid: None,
                reversed: false,
            })
            .into();
            draft.history_state_id = Some(PRECEDING_STATE + 1);
            draft.previous_history_state_id = Some(PRECEDING_STATE);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn group(
    record_index: u32,
    member: u32,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:group#{record_index}"),
        "scope_record_index": 12,
        "scope_reference_ordinal": record_index - 100,
        "record_index": record_index,
        "byte_offset": 1000,
        "class_tag": "332",
        "members": [member],
        "member_offsets": [1026],
        "frame": {
            "member_count_offset": 1021,
            "opaque_index": 1,
            "opaque_index_offset": 1072,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 1076,
            "variant": false
        },
        "role": role.raw(),
        "role_offset": 1054,
        "paired_class_tag": "259",
        "paired_byte_offset": 1125
    }))
    .unwrap()
}

/// An axis selection whose identity also names an ASM face in `face_states`
/// and, when `edge` is set, an ASM edge of the preceding state.
fn axis_selection(
    primary_identity: u64,
    face_states: &[i64],
    edge: bool,
) -> DesignEntitySelectionOperand {
    let mut selection = serde_json::from_value::<DesignEntitySelectionOperand>(serde_json::json!({
        "id": "f3d:test:entity-selection#201", "scope_record_index": 12,
        "group_record_index": 101, "group_member_ordinal": 0,
        "record_index": 201, "byte_offset": 100, "class_tag": "307",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
        "identity_record_index": 204, "identity_record_offset": 300,
        "primary_identity": primary_identity, "primary_identity_offset": 321,
        "next_record_index": 205, "next_byte_offset": 329
    }))
    .unwrap();
    if !face_states.is_empty() {
        selection.historical_face_candidates = vec![DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 40,
                state_ids: face_states.to_vec(),
            },
            face_slot: 12,
        }];
    }
    if edge {
        selection.historical_edge_candidates = vec![DesignEntitySelectionEdgeCandidate {
            identity_ordinal: 0,
            local_id: primary_identity,
            historical_entity_kind: AsmHistoricalEntityKind::Edge,
            historical_entity_ref: 40,
            edge_slots: vec![17],
        }];
    }
    selection
}

fn construction_with(
    selection: &DesignEntitySelectionOperand,
    placements: &[crate::records::sketch_placement::DesignSketchPlacement],
) -> Option<cadmpeg_ir::features::RevolveConstruction> {
    let groups = [
        group(100, 200, DesignOperandRole::PROFILE),
        group(101, 201, DesignOperandRole::ROLE_0X21),
    ];
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Revolve { construction, .. },
    ) = super::super::project_fixed_revolve_with_entities(
        &scope(),
        &groups,
        &[],
        std::slice::from_ref(selection),
        &[],
        (placements, &[]),
    )?
    else {
        panic!("expected a revolve");
    };
    Some(construction)
}

fn construction(
    selection: &DesignEntitySelectionOperand,
) -> Option<cadmpeg_ir::features::RevolveConstruction> {
    construction_with(selection, &[])
}

#[test]
fn an_origin_axis_identity_keeps_its_axis_when_an_asm_entity_shares_it() {
    let y_axis = cadmpeg_ir::features::RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::FeatureDirection3::from(
            cadmpeg_ir::units::UnitVector3::Y_AXIS,
        ),
        reference: None,
    };
    for (name, selection) in [
        ("no candidate", axis_selection(40, &[], false)),
        (
            "face in the preceding state",
            axis_selection(40, &[PRECEDING_STATE], false),
        ),
        (
            "face created later",
            axis_selection(40, &[PRECEDING_STATE + 1], false),
        ),
        ("edge in the preceding state", axis_selection(40, &[], true)),
        (
            "edge and face",
            axis_selection(40, &[PRECEDING_STATE], true),
        ),
    ] {
        assert_eq!(
            construction(&selection).and_then(|construction| construction.axis().cloned()),
            Some(y_axis.clone()),
            "{name}"
        );
    }
}

#[test]
fn another_identity_with_a_face_candidate_keeps_its_historical_resolution() {
    let historical = construction(&axis_selection(45, &[PRECEDING_STATE], false))
        .expect("a revolve with a historical face axis");
    assert_eq!(historical.axis(), None);
    assert!(construction(&axis_selection(45, &[], false)).is_none());
}

#[test]
fn a_sketch_placement_that_owns_the_identity_refuses_the_origin_axis() {
    let placement = crate::records::sketch_placement::DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: "f3d:test:placement#40".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::from_parts("Sketch", 40),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned()).unwrap(),
        record_index: 700,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
            .unwrap(),
    };
    assert!(construction_with(&axis_selection(40, &[], false), &[placement]).is_none());
}
