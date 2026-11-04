// SPDX-License-Identifier: Apache-2.0
//! Projected feature input topology.

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, UnresolvedFamily};

fn input_fixture() -> (
    Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:design:scope#input",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(4);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = Feature {
        product_definition: None,
        id: "f3d:model:feature#input".to_owned().try_into().unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![(
            AsmDeltaState {
                id: "f3d:history:state#4".into(),
                parent: "f3d:history".into(),
                byte_offset: 0,
                state_id: 4,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 0,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology {
                bodies: vec![1],
                faces: vec![2],
                edges: vec![3],
                vertices: vec![4],
                ..AsmHistoricalTopology::default()
            }),
            Vec::new(),
        )],
    );
    (feature, scope, history)
}

fn project() -> Vec<cadmpeg_ir::features::FeatureInputTopology> {
    let (feature, scope, mut history) = input_fixture();
    super::super::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_mut(&mut history),
        &[],
        &std::collections::HashMap::new(),
    )
}

#[test]
fn input_projection_preserves_member_order() {
    let projected = project();
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].vertices.as_slice()[0].as_str(),
        "f3d:history-input:vertex#5:input:4:4"
    );
}

#[test]
fn input_topology_uses_scope_binding_when_global_state_numbers_repeat() {
    let (feature, mut scope, mut history) = input_fixture();
    scope
        .try_edit(|draft| draft.history_state_id = Some(5))
        .unwrap();
    let mut current = history.states()[0].clone();
    current.id = "f3d:history:state#5".into();
    current.state_id = 5;
    current.node_index = 1;
    current.next_ref = Some(0);
    let topology = history.topology(&history.states()[0]).unwrap().clone();
    history = crate::test_support::history::history(
        history.id.clone(),
        history.byte_offset,
        history.preamble,
        false,
        vec![
            (
                history.states()[0].clone(),
                Some(topology.clone()),
                Vec::new(),
            ),
            (current, Some(topology), Vec::new()),
        ],
    );
    let mut other = history.clone();
    other.id = "f3d:other:history#0".into();
    other.states_mut()[0].id = "f3d:other:state#4".into();
    other.states_mut()[0].parent = other.id.clone();
    other.states_mut()[1].id = "f3d:other:state#5".into();
    other.states_mut()[1].parent = other.id.clone();
    crate::test_support::history::edit(&mut other, 0, |topology, _| {
        *topology = crate::history_records::AsmHistoricalTopology {
            faces: vec![99],
            ..Default::default()
        };
    });
    let mut histories = [history, other];
    let project = |histories: &mut [_], bindings: &std::collections::HashMap<String, String>| {
        super::super::project_feature_input_topologies(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&scope),
            histories,
            &[],
            bindings,
        )
    };
    assert!(project(&mut histories, &std::collections::HashMap::new()).is_empty());
    let bindings = std::collections::HashMap::from([(scope.id.clone(), histories[0].id.clone())]);
    let inputs = project(&mut histories, &bindings);
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].faces.as_slice(),
        &[crate::ids::history_input_face_id(
            &crate::ids::history_input_prefix(&feature.id.key(), 4),
            2
        )]
    );
    assert_eq!(
        inputs[0].native_ref.as_deref(),
        Some(histories[0].states()[0].id.as_str())
    );
    let bindings =
        std::collections::HashMap::from([(scope.id.clone(), "f3d:missing:history#0".into())]);
    assert!(project(&mut histories, &bindings).is_empty());
    assert!(super::super::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        &mut histories[..1],
        &[],
        &bindings,
    )
    .is_empty());
}

#[test]
fn input_edge_geometry_carries_samples_endpoints_and_faces() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
        AsmHistoricalEdgeSample, AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let topology = AsmHistoricalTopology {
        faces: vec![10, 11],
        loops: vec![20, 21],
        coedges: vec![30, 31],
        edges: vec![40, 41],
        vertices: vec![50, 51],
        points: vec![60, 61],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![20],
            },
            AsmHistoricalRelation {
                owner_ref: 11,
                member_refs: vec![21],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 30,
                owner_loop: 20,
                edge: 40,
                next: 30,
                previous: 30,
                radial_next: 31,
            },
            AsmHistoricalCoedge {
                coedge: 31,
                owner_loop: 21,
                edge: 40,
                next: 31,
                previous: 31,
                radial_next: 30,
            },
        ],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 40,
            start_vertex: 50,
            end_vertex: 51,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 50,
                carrier: 60,
            },
            AsmHistoricalCarrierBinding {
                entity: 51,
                carrier: 61,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 60,
                position: Point3::new(0.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 61,
                position: Point3::new(0.0, 0.0, 10.0),
            },
        ],
        edge_samples: vec![AsmHistoricalEdgeSample {
            edge: 40,
            interior: Point3::new(0.0, 0.0, 5.0),
            tangent: Some(Vector3::new(0.0, 0.0, 1.0)),
        }],
        ..AsmHistoricalTopology::default()
    };
    let feature: cadmpeg_ir::features::FeatureId =
        "f3d:model:feature#fillet".to_owned().try_into().unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature, 4);
    let geometry = super::super::project_input_edge_geometry(&topology, &prefix);
    let [edge] = geometry.as_slice() else {
        panic!("one sampled edge: {geometry:?}");
    };
    assert_eq!(edge.edge, crate::ids::history_input_edge_id(&prefix, 40));
    assert_eq!(
        edge.interior,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap()
    );
    assert_eq!(edge.tangent, Some(cadmpeg_ir::units::UnitVector3::Z_AXIS));
    assert_eq!(
        edge.start,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        edge.end,
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 10.0))
    );
    assert_eq!(
        edge.faces.iter().cloned().collect::<Vec<_>>(),
        vec![
            crate::ids::history_input_face_id(&prefix, 10),
            crate::ids::history_input_face_id(&prefix, 11),
        ]
    );
}

#[test]
fn input_edge_geometry_keeps_only_selected_edges() {
    use cadmpeg_ir::features::{FinitePoint3, HistoricalEdgeGeometry};
    use cadmpeg_ir::math::Point3;

    let (mut feature, _, _) = input_fixture();
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let selected = crate::ids::history_input_edge_id(&prefix, 40);
    let unselected = crate::ids::history_input_edge_id(&prefix, 41);
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::try_from(vec![
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: cadmpeg_ir::features::EdgeSelection::historical(
                        crate::ids::history_input_state_id(&prefix),
                        vec![selected.clone()],
                        "f3d:test:fillet-group".to_owned(),
                    )
                    .unwrap(),
                    radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved {
                        form: None,
                    },
                    tangency_weight: None,
                },
            ])
            .unwrap(),
        }),
    );
    let geometry = |edge: &cadmpeg_ir::ids::HistoricalEdgeId| HistoricalEdgeGeometry {
        edge: edge.clone(),
        interior: FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap(),
        tangent: None,
        start: None,
        end: None,
        faces: Default::default(),
    };
    let mut topologies = vec![cadmpeg_ir::features::FeatureInputTopology {
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: Default::default(),
        faces: Default::default(),
        edges: cadmpeg_ir::features::DistinctMembers::try_from(vec![
            selected.clone(),
            unselected.clone(),
        ])
        .unwrap(),
        vertices: Default::default(),
        edge_geometry: vec![geometry(&selected), geometry(&unselected)],
        native_ref: None,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::retain_selected_edge_geometry(
        &ctx,
        std::slice::from_ref(&feature),
        &mut topologies,
    )
    .unwrap();
    assert_eq!(topologies[0].edge_geometry, vec![geometry(&selected)]);
}

#[test]
fn input_edge_geometry_keeps_the_edges_that_bound_a_selected_face() {
    use cadmpeg_ir::features::{FinitePoint3, HistoricalEdgeGeometry};
    use cadmpeg_ir::math::Point3;

    let (mut feature, _, _) = input_fixture();
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    let removed = crate::ids::history_input_face_id(&prefix, 10);
    feature.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies: None,
            removed_faces: cadmpeg_ir::features::FaceSelection::historical(
                crate::ids::history_input_state_id(&prefix),
                vec![removed.clone()],
                "f3d:test:shell-faces".to_owned(),
            )
            .unwrap(),
            thickness: None,
            outward: None,
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        }),
    );
    let geometry = |edge: i64, faces: [i64; 2]| HistoricalEdgeGeometry {
        edge: crate::ids::history_input_edge_id(&prefix, edge),
        interior: FinitePoint3::new(Point3::new(0.0, 0.0, 5.0)).unwrap(),
        tangent: None,
        start: None,
        end: None,
        faces: cadmpeg_ir::features::DistinctMembers::try_from(
            faces
                .map(|face| crate::ids::history_input_face_id(&prefix, face))
                .to_vec(),
        )
        .unwrap(),
    };
    let mut topologies = vec![cadmpeg_ir::features::FeatureInputTopology {
        id: crate::ids::history_input_state_id(&prefix),
        input_of: feature.id.clone(),
        bodies: Default::default(),
        faces: Default::default(),
        edges: Default::default(),
        vertices: Default::default(),
        edge_geometry: vec![geometry(40, [10, 11]), geometry(41, [11, 12])],
        native_ref: None,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::retain_selected_edge_geometry(
        &ctx,
        std::slice::from_ref(&feature),
        &mut topologies,
    )
    .unwrap();
    assert_eq!(topologies[0].edge_geometry, vec![geometry(40, [10, 11])]);
}
