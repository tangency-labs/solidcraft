// SPDX-License-Identifier: Apache-2.0

use super::{scope_between, state};
use crate::history::bind_feature_outputs;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPlane,
    AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
};
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineOperands, DistinctMembers, FaceSelection, Feature,
    FeatureDatumPlaneFrame, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    SplitBodyTool,
};
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::math::{Point3, Vector3};

fn split_body_test_feature(scope_id: &str) -> Feature {
    Feature {
        id: FeatureId::mint("test:model:feature#split").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Split Body".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::SplitBody {
                targets: BodySelection::Unresolved,
                tool: SplitBodyTool::Faces(FaceSelection::Faces(vec![])),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope_id.into()),
        product_definition: None,
    }
}

fn split_body_with_face_tool(scope_id: &str, face_slots: &[i64]) -> Feature {
    let faces = face_slots
        .iter()
        .map(|slot| FaceId::mint(format!("f3d:brep:face#{slot}")).unwrap())
        .collect();
    let mut feature = split_body_test_feature(scope_id);
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Unresolved,
            tool: SplitBodyTool::Faces(FaceSelection::Faces(faces)),
        }));
    feature
}

fn attach_shell_face(
    topo: &mut AsmHistoricalTopology,
    shell_id: i64,
    loop_id: i64,
    face_id: i64,
    plane: AsmHistoricalPlane,
) {
    topo.faces.push(face_id);
    topo.faces.sort_unstable();
    topo.faces.dedup();
    if let Some(rel) = topo
        .shell_faces
        .iter_mut()
        .find(|r| r.owner_ref == shell_id)
    {
        if !rel.member_refs.contains(&face_id) {
            rel.member_refs.push(face_id);
        }
    }
    topo.face_loops.push(AsmHistoricalRelation {
        owner_ref: face_id,
        member_refs: vec![loop_id],
    });
    topo.face_loops.sort_by_key(|r| r.owner_ref);
    topo.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: face_id,
        carrier: plane.surface,
    });
    topo.face_surfaces.sort_by_key(|b| b.entity);
    if !topo
        .surface_planes
        .iter()
        .any(|p| p.surface == plane.surface)
    {
        topo.surface_planes.push(plane);
        topo.surface_planes.sort_by_key(|p| p.surface);
    }
}

fn two_solid_boxes_with_planes(
    b1: i64,
    min1: Point3,
    max1: Point3,
    b2: i64,
    min2: Point3,
    max2: Point3,
    planes: Vec<AsmHistoricalPlane>,
) -> AsmHistoricalTopology {
    let mut topo = AsmHistoricalTopology {
        bodies: vec![b1, b2],
        body_regions: vec![
            AsmHistoricalRelation {
                owner_ref: b1,
                member_refs: vec![2],
            },
            AsmHistoricalRelation {
                owner_ref: b2,
                member_refs: vec![102],
            },
        ],
        region_shells: vec![
            AsmHistoricalRelation {
                owner_ref: 2,
                member_refs: vec![3],
            },
            AsmHistoricalRelation {
                owner_ref: 102,
                member_refs: vec![103],
            },
        ],
        shell_faces: vec![
            AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![4],
            },
            AsmHistoricalRelation {
                owner_ref: 103,
                member_refs: vec![104],
            },
        ],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 4,
                member_refs: vec![5],
            },
            AsmHistoricalRelation {
                owner_ref: 104,
                member_refs: vec![105],
            },
        ],
        loop_coedges: vec![
            AsmHistoricalRelation {
                owner_ref: 5,
                member_refs: vec![6, 7],
            },
            AsmHistoricalRelation {
                owner_ref: 105,
                member_refs: vec![106, 107],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 8,
                next: 7,
                previous: 7,
                radial_next: 7,
            },
            AsmHistoricalCoedge {
                coedge: 7,
                owner_loop: 5,
                edge: 8,
                next: 6,
                previous: 6,
                radial_next: 6,
            },
            AsmHistoricalCoedge {
                coedge: 106,
                owner_loop: 105,
                edge: 108,
                next: 107,
                previous: 107,
                radial_next: 107,
            },
            AsmHistoricalCoedge {
                coedge: 107,
                owner_loop: 105,
                edge: 108,
                next: 106,
                previous: 106,
                radial_next: 106,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 8,
                start_vertex: 10,
                end_vertex: 11,
            },
            AsmHistoricalEdge {
                edge: 108,
                start_vertex: 110,
                end_vertex: 111,
            },
        ],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
            AsmHistoricalCarrierBinding {
                entity: 110,
                carrier: 120,
            },
            AsmHistoricalCarrierBinding {
                entity: 111,
                carrier: 121,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 20,
                position: min1,
            },
            AsmHistoricalPoint {
                point: 21,
                position: max1,
            },
            AsmHistoricalPoint {
                point: 120,
                position: min2,
            },
            AsmHistoricalPoint {
                point: 121,
                position: max2,
            },
        ],
        faces: vec![4, 104],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 4,
                carrier: 14,
            },
            AsmHistoricalCarrierBinding {
                entity: 104,
                carrier: 114,
            },
        ],
        surface_planes: vec![
            AsmHistoricalPlane {
                surface: 14,
                origin: Point3::new(0.0, 0.0, -10.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 114,
                origin: Point3::new(0.0, 0.0, 10.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    for (i, p) in planes.into_iter().enumerate() {
        let offset = i64::try_from(i).unwrap() * 2;
        attach_shell_face(&mut topo, 3, 5, 200 + offset, p.clone());
        attach_shell_face(&mut topo, 103, 105, 201 + offset, p);
    }
    topo
}

fn add_third_solid_box(topo: &mut AsmHistoricalTopology, b3: i64, min3: Point3, max3: Point3) {
    topo.bodies.push(b3);
    topo.bodies.sort_unstable();
    topo.body_regions.push(AsmHistoricalRelation {
        owner_ref: b3,
        member_refs: vec![302],
    });
    topo.body_regions.sort_by_key(|r| r.owner_ref);
    topo.region_shells.push(AsmHistoricalRelation {
        owner_ref: 302,
        member_refs: vec![303],
    });
    topo.region_shells.sort_by_key(|r| r.owner_ref);
    topo.shell_faces.push(AsmHistoricalRelation {
        owner_ref: 303,
        member_refs: vec![304],
    });
    topo.shell_faces.sort_by_key(|r| r.owner_ref);
    topo.face_loops.push(AsmHistoricalRelation {
        owner_ref: 304,
        member_refs: vec![305],
    });
    topo.face_loops.sort_by_key(|r| r.owner_ref);
    topo.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: 305,
        member_refs: vec![306, 307],
    });
    topo.loop_coedges.sort_by_key(|r| r.owner_ref);
    topo.coedge_topology.extend([
        AsmHistoricalCoedge {
            coedge: 306,
            owner_loop: 305,
            edge: 308,
            next: 307,
            previous: 307,
            radial_next: 307,
        },
        AsmHistoricalCoedge {
            coedge: 307,
            owner_loop: 305,
            edge: 308,
            next: 306,
            previous: 306,
            radial_next: 306,
        },
    ]);
    topo.coedge_topology.sort_by_key(|c| c.coedge);
    topo.edge_vertices.push(AsmHistoricalEdge {
        edge: 308,
        start_vertex: 310,
        end_vertex: 311,
    });
    topo.edge_vertices.sort_by_key(|e| e.edge);
    topo.vertex_points.extend([
        AsmHistoricalCarrierBinding {
            entity: 310,
            carrier: 320,
        },
        AsmHistoricalCarrierBinding {
            entity: 311,
            carrier: 321,
        },
    ]);
    topo.vertex_points.sort_by_key(|v| v.entity);
    topo.point_positions.extend([
        AsmHistoricalPoint {
            point: 320,
            position: min3,
        },
        AsmHistoricalPoint {
            point: 321,
            position: max3,
        },
    ]);
    topo.point_positions.sort_by_key(|p| p.point);
    topo.faces.push(304);
    topo.faces.sort_unstable();
    topo.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 304,
        carrier: 314,
    });
    topo.face_surfaces.sort_by_key(|f| f.entity);
    topo.surface_planes.push(AsmHistoricalPlane {
        surface: 314,
        origin: min3,
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
    topo.surface_planes.sort_by_key(|p| p.surface);
}

#[test]
fn split_plane_only_emitted_for_split_body() {
    let scope = scope_between(1, 0);

    let state1_topo = AsmHistoricalTopology {
        faces: vec![10],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 20,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        ..AsmHistoricalTopology::default()
    };

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    // Without resolved result-body evidence, an unresolved tool with one inserted planar face
    // preserves the readable plane and emits an inference loss.
    let mut feature_unresolved = split_body_test_feature(&scope.id);
    let unresolved_losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature_unresolved),
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature_unresolved
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"1,2,3;0,0,1".to_string())
    );
    assert!(unresolved_losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.kind()
    }));

    // With a resolved face tool, split_plane is emitted for SplitBody without inference loss.
    let mut feature_split = split_body_with_face_tool(&scope.id, &[10]);
    let face_tool_losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"1,2,3;0,0,1".to_string())
    );
    assert!(face_tool_losses.iter().all(|loss| {
        loss.code != crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.kind()
    }));

    // With a resolved DatumPlane feature tool, split_plane is emitted for SplitBody.
    let datum_id = FeatureId::mint("test:model:feature#datum").unwrap();
    let datum_feature = Feature {
        id: datum_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPlane".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::DatumPlane {
                frame: FeatureDatumPlaneFrame::new(
                    Point3::new(4.0, 5.0, 6.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: None,
        product_definition: None,
    };
    let mut feature_datum_split = split_body_test_feature(&scope.id);
    feature_datum_split.ordinal = 1;
    feature_datum_split
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Unresolved,
            tool: SplitBodyTool::Plane { plane: datum_id },
        }));
    let mut features = [datum_feature, feature_datum_split];
    let datum_tool_losses = bind_feature_outputs(
        &mut features,
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        features[1]
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"4,5,6;0,1,0".to_string())
    );
    assert!(datum_tool_losses.iter().all(|loss| {
        loss.code != crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.kind()
    }));

    // Non-SplitBody operations do not emit split_plane.
    let mut feature_extrude = Feature {
        id: FeatureId::mint("test:model:feature#extrude").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Native("scope".into()),
                ),
                direction: cadmpeg_ir::features::ExtrudeDirection::default(),
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
                extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: cadmpeg_ir::features::LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: cadmpeg_ir::features::BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: Some(false),
                allow_multi_profile_faces: Some(false),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
        product_definition: None,
    };

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_extrude),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_extrude
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}

#[test]
fn split_plane_unresolved_when_multiple_inserted_planes_ambiguous() {
    let scope = scope_between(1, 0);

    let state1_topo = two_solid_boxes_with_planes(
        1,
        Point3::new(-1.0, 0.0, 1.0),
        Point3::new(-1.0, 0.0, 1.0),
        2,
        Point3::new(1.0, 0.0, 2.0),
        Point3::new(1.0, 0.0, 2.0),
        vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 1.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
            },
        ],
    );

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    let mut feature_split = split_body_test_feature(&scope.id);

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}

#[test]
fn split_plane_emitted_when_multiple_inserted_planes_are_equivalent() {
    let scope = scope_between(1, 0);

    let state1_topo = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 1.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(1.0, 1.0, 1.5),
                normal: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
    );

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    let mut feature_split = split_body_test_feature(&scope.id);

    bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"0,0,1.5;0,0,1".to_string())
    );
}

#[test]
fn split_plane_separates_body_centers_when_bodies_exist() {
    let scope = scope_between(1, 0);

    let topo_separating = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 1.5),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    let history_sep = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), topo_separating),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    let mut feature_split = split_body_test_feature(&scope.id);
    let sep_losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history_sep],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"0,0,1.5;0,0,1".to_string())
    );
    assert!(sep_losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.kind()
    }));

    let topo_non_separating = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    let history_non_sep = crate::test_support::history::history(
        "f3d:asm-history#2".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), topo_non_separating),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    let mut feature_split2 = split_body_test_feature(&scope.id);
    let non_sep_losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature_split2),
        std::slice::from_ref(&scope),
        &mut [history_non_sep],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feature_split2
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
    assert!(non_sep_losses.iter().all(|loss| {
        loss.code != crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.kind()
    }));
}

#[test]
fn split_plane_ignores_side_face_separating_unrelated_body() {
    let scope = scope_between(1, 0);

    // State 0 has body 1 (to be split) and unrelated body 3 at x = 10..11.
    let mut state0_topo = AsmHistoricalTopology::default();
    add_third_solid_box(
        &mut state0_topo,
        3,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(11.0, 1.0, 1.0),
    );
    // Add body 1 in state 0 as well.
    let state0_b1 = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 3.0),
        3,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(11.0, 1.0, 1.0),
        vec![],
    );

    // Case 1: State 1 splits body 1 into bodies 1 and 2 along z = 1.5, and also inserts a
    // side face on body 1 at x = 5.0 (which separates body 1 from unrelated body 3).
    // Only the plane z = 1.5 is tied to the split result pair (1, 2).
    let mut state1_with_valid_split = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(1.0, 1.0, 3.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 1.5),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    add_third_solid_box(
        &mut state1_with_valid_split,
        3,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(11.0, 1.0, 1.0),
    );
    attach_shell_face(
        &mut state1_with_valid_split,
        3,
        5,
        250,
        AsmHistoricalPlane {
            surface: 50,
            origin: Point3::new(5.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        },
    );

    let history_valid = crate::test_support::history::history(
        "f3d:asm-history#side-face-1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_with_valid_split),
            state(0, 0, None, state0_b1.clone()),
        ],
    );
    let mut feat_valid = split_body_test_feature(&scope.id);
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_valid),
        std::slice::from_ref(&scope),
        &mut [history_valid],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_valid
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"0,0,1.5;0,0,1".to_string())
    );

    // Case 2: The shared face on (1, 2) does not separate their centers (z = -5.0), while the
    // inserted side face on body 1 at x = 5.0 separates body 1 from unrelated body 3.
    // Must leave split_plane unresolved rather than emitting the side face.
    let mut state1_only_unrelated_sep = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(1.0, 1.0, 3.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, -5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    add_third_solid_box(
        &mut state1_only_unrelated_sep,
        3,
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(11.0, 1.0, 1.0),
    );
    attach_shell_face(
        &mut state1_only_unrelated_sep,
        3,
        5,
        250,
        AsmHistoricalPlane {
            surface: 50,
            origin: Point3::new(5.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        },
    );

    let history_unrelated = crate::test_support::history::history(
        "f3d:asm-history#side-face-2".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_only_unrelated_sep),
            state(0, 0, None, state0_b1),
        ],
    );
    let mut feat_unrelated = split_body_test_feature(&scope.id);
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_unrelated),
        std::slice::from_ref(&scope),
        &mut [history_unrelated],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_unrelated
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}

#[test]
fn split_plane_unresolved_when_inserted_face_is_unclassifiable() {
    let scope = scope_between(1, 0);

    let mut topo_missing_surface = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 1.5),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    topo_missing_surface.faces.push(210);

    let mut topo_missing_plane = topo_missing_surface.clone();
    topo_missing_plane
        .face_surfaces
        .push(AsmHistoricalCarrierBinding {
            entity: 210,
            carrier: 99,
        });

    let mut topo_degenerate_plane = topo_missing_plane.clone();
    topo_degenerate_plane
        .surface_planes
        .push(AsmHistoricalPlane {
            surface: 99,
            origin: Point3::new(0.0, 0.0, 1.5),
            normal: Vector3::new(0.0, 0.0, 0.0),
        });
    topo_degenerate_plane
        .surface_planes
        .sort_by_key(|p| p.surface);

    for (i, topo) in [
        topo_missing_surface,
        topo_missing_plane,
        topo_degenerate_plane,
    ]
    .into_iter()
    .enumerate()
    {
        let history = crate::test_support::history::history(
            format!("f3d:asm-history#unclassifiable-{i}"),
            0,
            None,
            false,
            vec![
                state(1, 1, Some(0), topo.clone()),
                state(0, 0, None, AsmHistoricalTopology::default()),
            ],
        );
        let mut feature_split = split_body_test_feature(&scope.id);
        bind_feature_outputs(
            std::slice::from_mut(&mut feature_split),
            std::slice::from_ref(&scope),
            &mut [history],
            &[],
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            feature_split
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("split_plane")),
            None,
            "case {i} must leave split_plane unresolved when an inserted face is unclassifiable"
        );
    }
}

#[test]
fn split_plane_unresolved_when_output_body_has_incomplete_topology_or_vertices() {
    let scope = scope_between(1, 0);

    let mut topo_partial_vertices = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 3.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 1.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
            },
        ],
    );
    topo_partial_vertices
        .point_positions
        .retain(|p| p.point != 21);

    let mut topo_unsolid_body = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 1.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
        ],
    );
    topo_unsolid_body.bodies.push(3);

    let state0_base = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![],
    );

    for (i, topo) in [topo_partial_vertices, topo_unsolid_body]
        .into_iter()
        .enumerate()
    {
        let history = crate::test_support::history::history(
            format!("f3d:asm-history#incomplete-body-{i}"),
            0,
            None,
            false,
            vec![
                state(1, 1, Some(0), topo.clone()),
                state(0, 0, None, state0_base.clone()),
            ],
        );
        let mut feature_split = split_body_test_feature(&scope.id);
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature_split),
            std::slice::from_ref(&scope),
            &mut [history],
            &[],
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            feature_split
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("split_plane")),
            None,
            "case {i} must leave split_plane unresolved when body evidence is incomplete"
        );
        assert_eq!(
            feature_split
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some("1:0,0,1;2:0,0,2"),
            "case {i} must preserve readable body centers"
        );
        if i == 0 {
            assert!(
                losses.iter().any(|loss| {
                    loss.code == crate::loss::F3dLossCode::HistoricalBodyCenterPartial.kind()
                }),
                "partial vertex evidence must emit HistoricalBodyCenterPartial"
            );
            assert!(
                losses.iter().all(|loss| {
                    loss.code != crate::loss::F3dLossCode::HistoricalBodyBoundsUnresolved.kind()
                }),
                "partial vertex evidence with readable bounds must not emit HistoricalBodyBoundsUnresolved"
            );

            let history_prev_only = crate::test_support::history::history(
                "f3d:asm-history#incomplete-prev-only".into(),
                0,
                None,
                false,
                vec![
                    state(1, 1, Some(0), AsmHistoricalTopology::default()),
                    state(0, 0, None, topo),
                ],
            );
            let mut feat_prev_only = split_body_test_feature(&scope.id);
            let prev_losses = bind_feature_outputs(
                std::slice::from_mut(&mut feat_prev_only),
                std::slice::from_ref(&scope),
                &mut [history_prev_only],
                &[],
                &Default::default(),
                None,
            )
            .unwrap();
            assert!(
                prev_losses.iter().all(|loss| {
                    loss.code != crate::loss::F3dLossCode::HistoricalBodyCenterPartial.kind()
                }),
                "non-output previous state must not emit HistoricalBodyCenterPartial"
            );
        }
    }

    let state0_coplanar = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![],
    );
    let mut topo_coplanar_incomplete = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 1.0),
        Point3::new(0.0, 0.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(0.0, 0.0, 1.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(1.0, 1.0, 1.5),
                normal: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
    );
    topo_coplanar_incomplete.bodies.push(3);
    let history_coplanar = crate::test_support::history::history(
        "f3d:asm-history#incomplete-coplanar".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), topo_coplanar_incomplete),
            state(0, 0, None, state0_coplanar),
        ],
    );
    let mut feat_coplanar = split_body_test_feature(&scope.id);
    let coplanar_losses = bind_feature_outputs(
        std::slice::from_mut(&mut feat_coplanar),
        std::slice::from_ref(&scope),
        &mut [history_coplanar],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_coplanar
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane"))
            .map(String::as_str),
        Some("0,0,1.5;0,0,1"),
        "coplanar inserted faces with a non-solid third body must emit split_plane"
    );
    assert!(
        coplanar_losses.iter().any(|loss| {
            loss.code == crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.kind()
                && loss.message.contains("incomplete body evidence")
        }),
        "coplanar inserted faces with incomplete body evidence must charge HistoricalSplitPlaneInferred naming incomplete body evidence"
    );
}

fn combine_join_test_feature(scope_id: &str, target: Option<i64>, tool: Option<i64>) -> Feature {
    let target_sel = match target {
        Some(t) => BodySelection::Bodies(
            DistinctMembers::try_from(vec![BodyId::mint(format!("f3d:brep:entity#{t}")).unwrap()])
                .unwrap(),
        ),
        None => BodySelection::Unresolved,
    };
    let tool_sel = match tool {
        Some(t) => BodySelection::Bodies(
            DistinctMembers::try_from(vec![BodyId::mint(format!("f3d:brep:entity#{t}")).unwrap()])
                .unwrap(),
        ),
        None => BodySelection::Unresolved,
    };
    Feature {
        id: FeatureId::mint("test:model:feature#combine").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Combine".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: CombineOperands::new(target_sel, tool_sel).unwrap(),
                op: BooleanKind::Join,
                keep_tools: false,
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope_id.into()),
        product_definition: None,
    }
}

#[test]
fn body_absorption_requires_supported_op_and_affirmative_evidence() {
    let scope = scope_between(1, 0);

    let mut state0_base = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 10.0, 10.0),
        2,
        Point3::new(12.0, 0.0, 0.0),
        Point3::new(15.0, 5.0, 5.0),
        vec![],
    );
    let state1_unabsorbed_verts = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(20.0, 10.0, 10.0),
        2,
        Point3::new(12.0, 0.0, 0.0),
        Point3::new(15.0, 5.0, 5.0),
        vec![],
    );

    // Host grows to meet only half of b2's unique vertices (1 of 2): not absorbed even with Combine Join.
    let mut state1_half_verts = state1_unabsorbed_verts.clone();
    state1_half_verts
        .loop_coedges
        .iter_mut()
        .find(|r| r.owner_ref == 5)
        .unwrap()
        .member_refs
        .extend([30, 31]);
    state1_half_verts.coedge_topology.extend([
        AsmHistoricalCoedge {
            coedge: 30,
            owner_loop: 5,
            edge: 32,
            next: 31,
            previous: 31,
            radial_next: 31,
        },
        AsmHistoricalCoedge {
            coedge: 31,
            owner_loop: 5,
            edge: 32,
            next: 30,
            previous: 30,
            radial_next: 30,
        },
    ]);
    state1_half_verts.coedge_topology.sort_by_key(|c| c.coedge);
    state1_half_verts.edge_vertices.push(AsmHistoricalEdge {
        edge: 32,
        start_vertex: 33,
        end_vertex: 34,
    });
    state1_half_verts.edge_vertices.sort_by_key(|e| e.edge);
    state1_half_verts.vertex_points.extend([
        AsmHistoricalCarrierBinding {
            entity: 33,
            carrier: 120,
        },
        AsmHistoricalCarrierBinding {
            entity: 34,
            carrier: 36,
        },
    ]);
    state1_half_verts.vertex_points.sort_by_key(|v| v.entity);
    state1_half_verts.point_positions.push(AsmHistoricalPoint {
        point: 36,
        position: Point3::new(19.0, 9.0, 9.0),
    });
    state1_half_verts.point_positions.sort_by_key(|p| p.point);

    let history_half = crate::test_support::history::history(
        "f3d:asm-history#abs-half".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_half_verts),
            state(0, 0, None, state0_base.clone()),
        ],
    );
    let mut feat_half = combine_join_test_feature(&scope.id, Some(1), Some(2));
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_half),
        std::slice::from_ref(&scope),
        &mut [history_half],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_half
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        None
    );

    // State where b1 has all of b2's vertex positions, using distinct carriers (35, 36).
    let mut state1_distinct_carriers = state1_unabsorbed_verts.clone();
    state1_distinct_carriers
        .loop_coedges
        .iter_mut()
        .find(|r| r.owner_ref == 5)
        .unwrap()
        .member_refs
        .extend([30, 31]);
    state1_distinct_carriers.coedge_topology.extend([
        AsmHistoricalCoedge {
            coedge: 30,
            owner_loop: 5,
            edge: 32,
            next: 31,
            previous: 31,
            radial_next: 31,
        },
        AsmHistoricalCoedge {
            coedge: 31,
            owner_loop: 5,
            edge: 32,
            next: 30,
            previous: 30,
            radial_next: 30,
        },
    ]);
    state1_distinct_carriers
        .coedge_topology
        .sort_by_key(|c| c.coedge);
    state1_distinct_carriers
        .edge_vertices
        .push(AsmHistoricalEdge {
            edge: 32,
            start_vertex: 33,
            end_vertex: 34,
        });
    state1_distinct_carriers
        .edge_vertices
        .sort_by_key(|e| e.edge);
    state1_distinct_carriers.vertex_points.extend([
        AsmHistoricalCarrierBinding {
            entity: 33,
            carrier: 35,
        },
        AsmHistoricalCarrierBinding {
            entity: 34,
            carrier: 36,
        },
    ]);
    state1_distinct_carriers
        .vertex_points
        .sort_by_key(|v| v.entity);
    state1_distinct_carriers.point_positions.extend([
        AsmHistoricalPoint {
            point: 35,
            position: Point3::new(12.0, 0.0, 0.0),
        },
        AsmHistoricalPoint {
            point: 36,
            position: Point3::new(15.0, 5.0, 5.0),
        },
    ]);
    state1_distinct_carriers
        .point_positions
        .sort_by_key(|p| p.point);

    // 1. Without operand identity or carrier transfer, distinct carriers leave absorption unresolved.
    let history_distinct = crate::test_support::history::history(
        "f3d:asm-history#abs-distinct".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_distinct_carriers.clone()),
            state(0, 0, None, state0_base.clone()),
        ],
    );
    let mut feat_unresolved_combine = combine_join_test_feature(&scope.id, None, None);
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_unresolved_combine),
        std::slice::from_ref(&scope),
        &mut [history_distinct.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_unresolved_combine
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        None
    );

    // 2. Unsupported operation (SplitBody) never marks absorption even when carriers and positions match.
    let mut state1_shared_carriers = state1_distinct_carriers.clone();
    for vp in &mut state1_shared_carriers.vertex_points {
        if vp.entity == 33 {
            vp.carrier = 120;
        } else if vp.entity == 34 {
            vp.carrier = 121;
        }
    }
    let history_shared_carriers = crate::test_support::history::history(
        "f3d:asm-history#abs-shared".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_shared_carriers.clone()),
            state(0, 0, None, state0_base.clone()),
        ],
    );
    let mut feat_split = split_body_test_feature(&scope.id);
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_split),
        std::slice::from_ref(&scope),
        &mut [history_shared_carriers.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        None
    );

    // 3. Supported Combine Join with explicit operand identity marks b2 absorbed when not active.
    let mut feat_identity = combine_join_test_feature(&scope.id, Some(1), Some(2));
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_identity),
        std::slice::from_ref(&scope),
        &mut [history_distinct],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_identity
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        Some(&"2".to_string())
    );

    // 4. Shared point carriers without target/tool identity or transition deletion evidence
    // do not prove consumption: a separate unchanged body that shares carriers with the grown
    // host remains unabsorbed and is preserved in later affected_bodies.
    let scope2 = scope_between(2, 1);
    let mut state2_touch_b2 = state1_shared_carriers.clone();
    state2_touch_b2.point_positions.push(AsmHistoricalPoint {
        point: 220,
        position: Point3::new(14.0, 2.0, 2.0),
    });
    state2_touch_b2.point_positions.sort_by_key(|p| p.point);
    state2_touch_b2
        .vertex_points
        .push(AsmHistoricalCarrierBinding {
            entity: 210,
            carrier: 220,
        });
    state2_touch_b2.vertex_points.sort_by_key(|v| v.entity);
    state2_touch_b2.edge_vertices.push(AsmHistoricalEdge {
        edge: 208,
        start_vertex: 110,
        end_vertex: 210,
    });
    state2_touch_b2.edge_vertices.sort_by_key(|e| e.edge);
    state2_touch_b2.coedge_topology.extend([
        AsmHistoricalCoedge {
            coedge: 206,
            owner_loop: 105,
            edge: 208,
            next: 207,
            previous: 207,
            radial_next: 207,
        },
        AsmHistoricalCoedge {
            coedge: 207,
            owner_loop: 105,
            edge: 208,
            next: 206,
            previous: 206,
            radial_next: 206,
        },
    ]);
    state2_touch_b2.coedge_topology.sort_by_key(|c| c.coedge);
    state2_touch_b2
        .loop_coedges
        .iter_mut()
        .find(|r| r.owner_ref == 105)
        .unwrap()
        .member_refs
        .extend([206, 207]);
    for topo in [
        &mut state0_base,
        &mut state1_shared_carriers,
        &mut state2_touch_b2,
    ] {
        topo.shell_wire_edges = vec![
            AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![],
            },
            AsmHistoricalRelation {
                owner_ref: 103,
                member_refs: vec![],
            },
        ];
        topo.shell_free_vertices = vec![
            AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![],
            },
            AsmHistoricalRelation {
                owner_ref: 103,
                member_refs: vec![],
            },
        ];
        topo.edges = topo.edge_vertices.iter().map(|e| e.edge).collect();
        topo.edges.sort_unstable();
    }
    let history_separate_body = crate::test_support::history::history(
        "f3d:asm-history#abs-separate-body".into(),
        0,
        None,
        false,
        vec![
            state(2, 2, Some(1), state2_touch_b2),
            state(1, 1, Some(0), state1_shared_carriers.clone()),
            state(0, 0, None, state0_base.clone()),
        ],
    );
    let feat1_shared = combine_join_test_feature(&scope.id, None, None);
    let mut feat2_later = combine_join_test_feature(&scope2.id, None, None);
    feat2_later.id = FeatureId::mint("test:model:feature#later").unwrap();
    feat2_later.ordinal = 1;
    let mut seq_features = [feat1_shared, feat2_later];
    bind_feature_outputs(
        &mut seq_features,
        &[scope.clone(), scope2],
        &mut [history_separate_body.clone()],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        seq_features[0]
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        None
    );
    assert_eq!(
        seq_features[1]
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("affected_bodies")),
        Some(&"2".to_string())
    );

    // 5. Supported Combine Join with transition deletion evidence marks b2 absorbed when not active.
    let mut history_transition_deleted = crate::test_support::history::history(
        "f3d:asm-history#abs-transition-deleted".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_shared_carriers.clone()),
            state(0, 0, None, state0_base),
        ],
    );
    history_transition_deleted.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .topology
        .bodies
        .deleted
        .push(2);
    let mut feat_transition_deleted = combine_join_test_feature(&scope.id, None, None);
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_transition_deleted),
        std::slice::from_ref(&scope),
        &mut [history_transition_deleted],
        &[],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_transition_deleted
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        Some(&"2".to_string())
    );

    // 6. Supported Combine Join with explicit operand identity marks b2 absorbed when not active,
    // and excludes b2 when b2 is present in active_bodies.
    let mut active_b2 = super::active_body();
    active_b2.id = BodyId::mint("f3d:brep:entity#2").unwrap();
    let mut feat_active = combine_join_test_feature(&scope.id, Some(1), Some(2));
    bind_feature_outputs(
        std::slice::from_mut(&mut feat_active),
        std::slice::from_ref(&scope),
        &mut [history_shared_carriers],
        &[active_b2],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        feat_active
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
        None
    );

    let mut history_first_brep = history_separate_body;
    let first_brep_id = "f3d:asset/Breps.BlobParts/BREP.first.smbh:asm-history#1".to_string();
    history_first_brep.id.clone_from(&first_brep_id);
    for st in history_first_brep.states_mut() {
        st.parent.clone_from(&first_brep_id);
        st.id = format!(
            "f3d:asset/Breps.BlobParts/BREP.first.smbh:asm-delta-state#{}",
            st.state_id
        );
    }
    let scope2_brep = scope_between(2, 1);

    let mut active_other_brep = super::active_body();
    active_other_brep.id = BodyId::mint("f3d:brep/second.smbh/brep:entity#2").unwrap();
    let feat1_other_brep = combine_join_test_feature(&scope.id, Some(1), Some(2));
    let mut feat2_other_brep = combine_join_test_feature(&scope2_brep.id, None, None);
    feat2_other_brep.id = FeatureId::mint("test:model:feature#later-other-brep").unwrap();
    feat2_other_brep.ordinal = 1;
    let mut seq_other_brep = [feat1_other_brep, feat2_other_brep];
    bind_feature_outputs(
        &mut seq_other_brep,
        &[scope.clone(), scope2_brep.clone()],
        &mut [history_first_brep.clone()],
        &[active_other_brep],
        &Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        seq_other_brep[0]
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("deleted_bodies"))
            .map(String::as_str),
        Some("2")
    );
    assert_eq!(
        seq_other_brep[0]
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("body_centers"))
            .map(String::as_str),
        Some("1:10,5,5")
    );
    assert_eq!(
        seq_other_brep[1]
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("affected_bodies")),
        None
    );

    let mut active_same_brep = super::active_body();
    active_same_brep.id = BodyId::mint("f3d:brep/first.smbh/brep:entity#2").unwrap();
    let mut active_ambig_edge = super::active_body();
    active_ambig_edge.id = BodyId::mint("f3d:brep/first.smbh/brep:saved-edge-body#2").unwrap();
    for active_set in [
        vec![active_same_brep.clone()],
        vec![active_same_brep, active_ambig_edge],
    ] {
        let feat1_same = combine_join_test_feature(&scope.id, Some(1), Some(2));
        let mut feat2_same = combine_join_test_feature(&scope2_brep.id, None, None);
        feat2_same.id = FeatureId::mint("test:model:feature#later-same-brep").unwrap();
        feat2_same.ordinal = 1;
        let mut seq_same = [feat1_same, feat2_same];
        bind_feature_outputs(
            &mut seq_same,
            &[scope.clone(), scope2_brep.clone()],
            &mut [history_first_brep.clone()],
            &active_set,
            &Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            seq_same[0]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("deleted_bodies")),
            None
        );
        assert_eq!(
            seq_same[0]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some("1:10,5,5;2:13.5,2.5,2.5")
        );
        assert_eq!(
            seq_same[1]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("affected_bodies"))
                .map(String::as_str),
            Some("2")
        );
    }
}

#[test]
fn split_and_body_properties_follow_bound_history_across_equal_state_numbers() {
    use std::collections::HashMap;

    let scope_h1_s1 = scope_between(1, 0);
    let mut scope_h1_s2 = scope_between(2, 1);
    scope_h1_s2.id = "f3d:Design/BulkStream.dat:design-parameter-scope#2".into();

    let mut scope_h2_s1 = scope_between(1, 0);
    scope_h2_s1.id = "f3d:Other/BulkStream.dat:design-parameter-scope#1".into();
    let mut scope_h2_s2 = scope_between(2, 1);
    scope_h2_s2.id = "f3d:Other/BulkStream.dat:design-parameter-scope#2".into();

    let h1_state1 = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 2.0, 1.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(2.0, 2.0, 3.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(0.0, 0.0, 1.5),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
    );
    let mut h1_state2 = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 2.0, 3.0),
        2,
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(2.0, 2.0, 3.0),
        vec![],
    );
    h1_state2
        .loop_coedges
        .iter_mut()
        .find(|r| r.owner_ref == 5)
        .unwrap()
        .member_refs
        .extend([30, 31]);
    h1_state2.coedge_topology.extend([
        AsmHistoricalCoedge {
            coedge: 30,
            owner_loop: 5,
            edge: 32,
            next: 31,
            previous: 31,
            radial_next: 31,
        },
        AsmHistoricalCoedge {
            coedge: 31,
            owner_loop: 5,
            edge: 32,
            next: 30,
            previous: 30,
            radial_next: 30,
        },
    ]);
    h1_state2.coedge_topology.sort_by_key(|c| c.coedge);
    h1_state2.edge_vertices.push(AsmHistoricalEdge {
        edge: 32,
        start_vertex: 33,
        end_vertex: 34,
    });
    h1_state2.edge_vertices.sort_by_key(|e| e.edge);
    h1_state2.vertex_points.extend([
        AsmHistoricalCarrierBinding {
            entity: 33,
            carrier: 120,
        },
        AsmHistoricalCarrierBinding {
            entity: 34,
            carrier: 121,
        },
    ]);
    h1_state2.vertex_points.sort_by_key(|v| v.entity);

    let mut h1_states = vec![
        state(2, 2, Some(1), h1_state2),
        state(1, 1, Some(0), h1_state1),
        state(0, 0, None, AsmHistoricalTopology::default()),
    ];
    for (st, _, _) in &mut h1_states {
        st.parent = "history:first".into();
        st.id = format!("history:first:state#{}", st.state_id);
    }
    let history_first =
        crate::test_support::history::history("history:first".into(), 0, None, false, h1_states);

    let h2_state1 = two_solid_boxes_with_planes(
        1,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(4.0, 2.0, 2.0),
        2,
        Point3::new(5.0, 0.0, 0.0),
        Point3::new(9.0, 2.0, 2.0),
        vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(4.5, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        }],
    );
    let h2_state2 = h2_state1.clone();
    let mut h2_states = vec![
        state(2, 2, Some(1), h2_state2),
        state(1, 1, Some(0), h2_state1),
        state(0, 0, None, AsmHistoricalTopology::default()),
    ];
    for (st, _, _) in &mut h2_states {
        st.parent = "history:second".into();
        st.id = format!("history:second:state#{}", st.state_id);
    }
    let history_second =
        crate::test_support::history::history("history:second".into(), 0, None, false, h2_states);

    let owners = HashMap::from([
        (scope_h1_s1.id.clone(), history_first.id.clone()),
        (scope_h1_s2.id.clone(), history_first.id.clone()),
        (scope_h2_s1.id.clone(), history_second.id.clone()),
        (scope_h2_s2.id.clone(), history_second.id.clone()),
    ]);
    let scopes = [scope_h1_s1, scope_h1_s2, scope_h2_s1, scope_h2_s2];

    for reversed in [false, true] {
        let mut f_h1_split = split_body_test_feature(&scopes[0].id);
        f_h1_split.id = FeatureId::mint("test:model:feature#h1-split").unwrap();
        f_h1_split.ordinal = 0;

        let mut f_h1_combine = combine_join_test_feature(&scopes[1].id, Some(1), Some(2));
        f_h1_combine.id = FeatureId::mint("test:model:feature#h1-combine").unwrap();
        f_h1_combine.ordinal = 1;

        let mut f_h2_split = split_body_with_face_tool(&scopes[2].id, &[200]);
        f_h2_split.id = FeatureId::mint("test:model:feature#h2-split").unwrap();
        f_h2_split.ordinal = 2;

        let mut f_h2_combine = combine_join_test_feature(&scopes[3].id, Some(1), Some(2));
        f_h2_combine.id = FeatureId::mint("test:model:feature#h2-combine").unwrap();
        f_h2_combine.ordinal = 3;

        let mut features = [f_h1_split, f_h1_combine, f_h2_split, f_h2_combine];
        let mut histories = if reversed {
            vec![history_second.clone(), history_first.clone()]
        } else {
            vec![history_first.clone(), history_second.clone()]
        };

        bind_feature_outputs(&mut features, &scopes, &mut histories, &[], &owners, None).unwrap();

        assert_eq!(
            features[0]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("split_plane"))
                .map(String::as_str),
            Some("0,0,1.5;0,0,1")
        );
        assert_eq!(
            features[0]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some("1:1,1,0.5;2:1,1,2.5")
        );
        assert_eq!(
            features[1]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("deleted_bodies"))
                .map(String::as_str),
            Some("2")
        );
        assert_eq!(
            features[1]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some("1:1,1,1.5")
        );
        assert_eq!(
            features[2]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("split_plane"))
                .map(String::as_str),
            Some("4.5,0,0;1,0,0")
        );
        assert_eq!(
            features[2]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some("1:2,1,1;2:7,1,1")
        );
        assert_eq!(
            features[3]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("deleted_bodies"))
                .map(String::as_str),
            None
        );
        assert_eq!(
            features[3]
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("body_centers"))
                .map(String::as_str),
            Some("1:2,1,1;2:7,1,1")
        );
    }
}
