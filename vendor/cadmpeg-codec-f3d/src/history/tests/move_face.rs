// SPDX-License-Identifier: Apache-2.0
use crate::history::bind_feature_face_selections;
use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalPlane,
    AsmHistoricalTopology, AsmHistory,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_ir::features::{
    FaceMotion, FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureInputTopology, FeatureOperation,
};
use cadmpeg_ir::math::{Point3, Vector3};

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#42";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#43";

fn topology(top_z: f64) -> AsmHistoricalTopology {
    let plane = |surface, z| AsmHistoricalPlane {
        surface,
        origin: Point3::new(1.0, 2.0, z),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    AsmHistoricalTopology {
        faces: vec![10, 20],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        surface_planes: vec![plane(100, top_z), plane(200, 0.0)],
        ..AsmHistoricalTopology::default()
    }
}

fn state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    next_ref: Option<i64>,
    moved_revision: i64,
) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("history:state#{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        vec![
            AsmEntityVersion {
                entity_ref: 10,
                record_ref: moved_revision,
            },
            AsmEntityVersion {
                entity_ref: 20,
                record_ref: 2,
            },
        ],
    )
}

fn history(after_top_z: f64, before_top_z: f64) -> AsmHistory {
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(2, topology(after_top_z), Some(1), 3),
            state(1, topology(before_top_z), None, 1),
        ],
    )
}

fn scope(record_index: u32) -> DesignParameterScope {
    let mut scope =
        DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::MoveFace, record_index);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn group(scope_record_index: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| ordinal * 11)
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": GROUP_ID,
        "byte_offset": 0,
        "class_tag": "261",
        "record_index": 43,
        "scope_record_index": scope_record_index,
        "scope_reference_ordinal": 0,
        "members": members,
        "member_offsets": member_offsets,
        "role": 68_719_476_736_u64,
        "role_offset": 0,
        "paired_byte_offset": 200,
        "paired_class_tag": "266",
        "frame": {
            "member_count_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "trailing_flags": [],
            "trailing_record_indices": [],
            "trailing_record_offsets": [],
            "variant": false
        }
    }))
    .unwrap()
}

fn bind(
    groups: &[DesignConstructionOperandGroup],
    initial: FaceSelection,
    history: AsmHistory,
) -> FaceSelection {
    bind_with_scope(&scope(42), groups, initial, history)
}

fn bind_with_scope(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    initial: FaceSelection,
    history: AsmHistory,
) -> FaceSelection {
    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let mut feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::MoveFace {
                faces: initial,
                motion: FaceMotion::Offset {
                    distance: cadmpeg_ir::scalar::Length::new(3.0).unwrap(),
                },
            },
        )),
        native_ref: Some(SCOPE_ID.into()),
    };
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::ids::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id,
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        vertices: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: std::slice::from_ref(scope),
            groups,
            operands: &[],
            entity_operands: &[],
            body_recipe_operands: &[],
            histories: &mut [history],
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::MoveFace { faces, .. }) =
        feature.evaluation.definition()
    else {
        panic!("expected a MoveFace");
    };
    faces.clone()
}

fn native() -> FaceSelection {
    FaceSelection::Native(GROUP_ID.into())
}

#[test]
fn move_face_group_binds_to_the_face_its_transition_moves() {
    let FaceSelection::Historical { faces, .. } =
        bind(&[group(42, &[44])], native(), history(8.0, 5.0))
    else {
        panic!("expected a historical selection");
    };
    let faces: Vec<&str> = faces
        .iter()
        .map(cadmpeg_ir::ids::HistoricalFaceId::as_str)
        .collect();
    assert_eq!(faces.len(), 1);
    assert!(faces[0].ends_with(":10"), "{faces:?}");
}

#[test]
fn move_face_group_stays_native_when_its_member_count_differs_from_the_moved_faces() {
    assert_eq!(
        bind(&[group(42, &[44, 45])], native(), history(8.0, 5.0)),
        native()
    );
}

#[test]
fn move_face_group_of_another_scope_does_not_bind() {
    assert_eq!(
        bind(&[group(41, &[44])], native(), history(8.0, 5.0)),
        native()
    );
}

#[test]
fn move_face_group_id_must_name_one_group() {
    assert_eq!(
        bind(
            &[group(42, &[44]), group(42, &[44])],
            native(),
            history(8.0, 5.0)
        ),
        native()
    );
}

#[test]
fn a_selection_that_names_the_scope_stays_native() {
    let scope_native = FaceSelection::Native(SCOPE_ID.into());
    assert_eq!(
        bind(&[group(42, &[44])], scope_native.clone(), history(8.0, 5.0)),
        scope_native
    );
}

/// State 3 follows a state 2 that only revises face 20; state 3 moves face 10.
fn chain(middle_faces: Vec<i64>) -> AsmHistory {
    let mut middle = topology(5.0);
    middle.faces = middle_faces;
    let mut middle = state(2, middle, Some(1), 1);
    middle.2[1].record_ref = 4;
    let mut last = state(3, topology(8.0), Some(2), 3);
    last.2[1].record_ref = 4;
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![last, middle, state(1, topology(5.0), None, 1)],
    )
}

fn scope_to(state_id: i64) -> DesignParameterScope {
    let mut scope = scope(42);
    scope
        .try_edit(|draft| draft.history_state_id = Some(state_id))
        .unwrap();
    scope
}

#[test]
fn a_motion_across_two_transitions_binds_from_the_input_state() {
    let FaceSelection::Historical { state, faces, .. } = bind_with_scope(
        &scope_to(3),
        &[group(42, &[44])],
        native(),
        chain(vec![10, 20]),
    ) else {
        panic!("expected a historical selection");
    };
    assert!(state.as_str().ends_with(":1"), "{state:?}");
    let faces: Vec<&str> = faces
        .iter()
        .map(cadmpeg_ir::ids::HistoricalFaceId::as_str)
        .collect();
    assert_eq!(faces.len(), 1);
    assert!(faces[0].ends_with(":10"), "{faces:?}");
}

#[test]
fn a_face_deleted_in_an_earlier_transition_states_no_selection() {
    assert_eq!(
        bind_with_scope(&scope_to(3), &[group(42, &[44])], native(), chain(vec![10])),
        native()
    );
}

#[test]
fn a_face_that_only_an_earlier_transition_updates_counts() {
    let mut middle = state(2, topology(5.0), Some(1), 3);
    middle.2[1].record_ref = 4;
    let mut last = state(3, topology(8.0), Some(2), 3);
    last.2[1].record_ref = 4;
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![last, middle, state(1, topology(5.0), None, 1)],
    );
    let FaceSelection::Historical { faces, .. } =
        bind_with_scope(&scope_to(3), &[group(42, &[44])], native(), history)
    else {
        panic!("expected a historical selection");
    };
    let faces: Vec<&str> = faces
        .iter()
        .map(cadmpeg_ir::ids::HistoricalFaceId::as_str)
        .collect();
    assert_eq!(faces.len(), 1);
    assert!(faces[0].ends_with(":10"), "{faces:?}");
}
