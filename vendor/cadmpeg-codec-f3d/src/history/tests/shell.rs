// SPDX-License-Identifier: Apache-2.0
use crate::history::bind_feature_face_selections;
use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalPlane,
    AsmHistoricalTopology, AsmHistory,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureInputTopology,
    FeatureOperation, ShellMode,
};
use cadmpeg_ir::math::{Point3, Vector3};

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#42";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#43";

/// Faces 1 to 6 of a 40 x 30 x 20 mm box with the top as face 2, and the
/// inner walls 11 and 13 to 16 that a 2 mm inside shell adds to the others.
fn topology(walled: bool) -> AsmHistoricalTopology {
    let mut faces = vec![
        (1, Point3::new(20.0, 15.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        (
            2,
            Point3::new(20.0, 15.0, 20.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        (3, Point3::new(0.0, 30.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)),
        (4, Point3::new(40.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (5, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, -1.0, 0.0)),
        (6, Point3::new(40.0, 30.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
    ];
    if walled {
        faces.extend([
            (
                11,
                Point3::new(20.0, 15.0, 2.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            (13, Point3::new(2.0, 30.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
            (
                14,
                Point3::new(38.0, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
            ),
            (15, Point3::new(0.0, 2.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
            (
                16,
                Point3::new(40.0, 28.0, 0.0),
                Vector3::new(0.0, -1.0, 0.0),
            ),
        ]);
    }
    AsmHistoricalTopology {
        faces: faces.iter().map(|(face, ..)| *face).collect(),
        face_surfaces: faces
            .iter()
            .map(|(face, ..)| AsmHistoricalCarrierBinding {
                entity: *face,
                carrier: 100 + face,
            })
            .collect(),
        surface_planes: faces
            .iter()
            .map(|(face, origin, normal)| AsmHistoricalPlane {
                surface: 100 + face,
                origin: *origin,
                normal: *normal,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

/// A state whose faces in `revised` carry a second record revision.
fn state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    next_ref: Option<i64>,
    revised: &[i64],
) -> crate::test_support::history::State {
    let versions = topology
        .faces
        .iter()
        .map(|&face| AsmEntityVersion {
            entity_ref: face,
            record_ref: if revised.contains(&face) { 2 } else { 1 },
        })
        .collect();
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
        versions,
    )
}

fn history(updated: &[i64]) -> AsmHistory {
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(2, topology(true), Some(1), updated),
            state(1, topology(false), None, &[]),
        ],
    )
}

fn scope(record_index: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::Shell, record_index);
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
    group_in(GROUP_ID, scope_record_index, members)
}

fn group_in(id: &str, scope_record_index: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| ordinal * 11)
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": id,
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
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    mode: Option<ShellMode>,
    updated: &[i64],
) -> FaceSelection {
    bind_group(GROUP_ID, scope, groups, mode, updated)
}

fn bind_group(
    group_id: &str,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    mode: Option<ShellMode>,
    updated: &[i64],
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
            FeatureOperation::Shell {
                bodies: None,
                removed_faces: FaceSelection::Native(group_id.into()),
                thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
                outward: Some(false),
                mode,
                join: None,
                resolve_intersections: None,
                allow_self_intersections: None,
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
            histories: &mut [history(updated)],
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::Shell { removed_faces, .. }) =
        feature.evaluation.definition()
    else {
        panic!("expected a Shell");
    };
    removed_faces.clone()
}

fn selected_slots(selection: &FaceSelection) -> Vec<String> {
    let FaceSelection::Historical { faces, .. } = selection else {
        panic!("expected a historical selection, got {selection:?}");
    };
    faces
        .iter()
        .map(|face| face.as_str().rsplit(':').next().unwrap().to_owned())
        .collect()
}

const TOP: &[i64] = &[2];
const ALL: &[i64] = &[1, 2, 3, 4, 5, 6];

#[test]
fn shell_group_binds_to_the_face_its_transition_opens() {
    for updated in [TOP, ALL] {
        assert_eq!(
            selected_slots(&bind(&scope(42), &[group(42, &[7])], None, updated)),
            ["2"]
        );
    }
}

#[test]
fn a_shell_on_both_sides_walls_at_half_its_thickness() {
    assert_eq!(
        bind(
            &scope(42),
            &[group(42, &[7])],
            Some(ShellMode::BothSides),
            ALL
        ),
        FaceSelection::Native(GROUP_ID.into()),
        "each side of a 2 mm shell on both sides is 1 mm, and the walls are 2 mm away"
    );
}

#[test]
fn a_skin_shell_states_no_selection() {
    assert_eq!(
        bind(&scope(42), &[group(42, &[7])], Some(ShellMode::Skin), TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_of_another_scope_does_not_bind() {
    assert_eq!(
        bind(&scope(42), &[group(41, &[7])], None, TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_id_must_name_one_group() {
    assert_eq!(
        bind(&scope(42), &[group(42, &[7]), group(42, &[7])], None, TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_with_more_members_than_open_faces_does_not_bind() {
    assert_eq!(
        bind(&scope(42), &[group(42, &[7, 8])], None, TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_of_another_stream_does_not_bind() {
    const OTHER: &str = "f3d:Design/Other.dat:group#43";
    assert_eq!(
        bind_group(OTHER, &scope(42), &[group_in(OTHER, 42, &[7])], None, TOP),
        FaceSelection::Native(OTHER.into())
    );
}
