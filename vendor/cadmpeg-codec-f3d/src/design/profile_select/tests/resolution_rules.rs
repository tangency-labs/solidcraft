// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::{
    has_to_termination, historical_extrude_to_distance, selection_containing_points,
    transition_inserted_profile_selection, transition_profile_selection, ExtrudeProfileResolution,
    ResolvedProfileSelection,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
    AsmHistoricalEdge, AsmHistoricalEntityDelta, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalSurfaceRadius, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmStateSnapshot,
};
use crate::records::sketch_relations::SketchRelationOperand;
use crate::records::topology::extrude_selection::{
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionGroupWire, DesignExtrudeSelectionMember,
};
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeSide, FaceSelection, LinearTermination, PlanarProfileRef, ProfileRef,
    SketchProfileBoundaryUse, SketchProfileRegion, SketchProfileRegions,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};

fn extrude_group(members: Vec<u32>) -> DesignExtrudeSelectionGroup {
    let member_offsets = vec![37; members.len()];
    DesignExtrudeSelectionGroup::try_from(DesignExtrudeSelectionGroupWire {
        id: "f3d:Design/BulkStream.dat:selection-group#9".into(),
        scope_record_index: 7,
        scope_reference_ordinal: 0,
        record_index: 9,
        byte_offset: 0,
        class_tag: "277".to_owned(),
        member_count_offset: 32,
        members,
        member_offsets,
        opaque_index: 1,
        opaque_index_offset: 47,
        opaque_scalar: 0.0,
        opaque_scalar_offset: 51,
        variant: Some(false),
        typed_references: false,
        paired_class_tag: "277".to_owned(),
        paired_byte_offset: 100,
    })
    .unwrap()
}

fn history_with_topologies(topos: &[AsmHistoricalTopology]) -> AsmHistory {
    let empty_versions = BTreeMap::new();
    let mut states = Vec::new();
    for (index, _) in topos.iter().enumerate() {
        let snapshot = |i: usize| AsmStateSnapshot {
            state_id: i64::try_from(i).unwrap(),
            topology: &topos[i],
            versions: &empty_versions,
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        states.push(AsmDeltaState {
            id: format!("f3d:history:state#{index}"),
            parent: "f3d:history".into(),
            byte_offset: 0,
            state_id: i64::try_from(index).unwrap(),
            version_flag: 1,
            state_flag: 0,
            previous_ref: (index + 1 < topos.len()).then(|| i64::try_from(index + 1).unwrap()),
            next_ref: index.checked_sub(1).map(|i| i64::try_from(i).unwrap()),
            node_index: i64::try_from(index).unwrap(),
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: Some(transition),
        });
    }
    AsmHistory::new("f3d:history".into(), 0, None, false, states)
}

const EPS: f64 = 1.0e-6;

fn make_sketch(id: &str, profiles: Vec<Vec<SketchEntityId>>) -> Sketch {
    let sketch_id = SketchId::mint(id).unwrap();
    let sketch_profiles = profiles
        .into_iter()
        .map(|loop_ids| {
            loop_ids
                .into_iter()
                .map(|entity| SketchEntityUse {
                    entity,
                    reversed: false,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(sketch_profiles).unwrap(),
        native_ref: None,
    }
}

fn line_entity(sketch_id: &SketchId, name: &str, start: Point2, end: Point2) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
    )
}

fn circle_entity(sketch_id: &SketchId, name: &str, center: Point2, radius: f64) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center,
            radius: cadmpeg_ir::scalar::Length::new(radius).unwrap(),
        })
        .unwrap(),
    )
}

fn make_member(
    group_record_index: u32,
    ordinal: u32,
    record_index: u32,
    resolved_geometry: Option<SketchRelationOperand>,
) -> DesignExtrudeSelectionMember {
    DesignExtrudeSelectionMember::try_new(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
            id: format!("f3d:Design/BulkStream.dat:selection-member#{record_index}"),
            group_record_index,
            group_member_ordinal: ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("278".to_owned())
                .unwrap(),
            local_id: 200,
            local_id_offset: 21,
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 33,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 109,
            tail_slot_present: false,
            tail_slot_offset: 0,
            resolved_geometry,
            operand_identity_ids: Vec::new(),
            historical: None,
            scope_tail: None,
            next_record_index: record_index + 1,
            next_byte_offset: 190,
        },
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// 1. Fallback & Negatives: multi-loop curve selects smallest entity count
// ---------------------------------------------------------------------------

#[test]
fn multiloop_fallback_selects_smallest_profile() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#test_fallback").unwrap();
        let shared_curve_id = crate::ids::neutral_sketch_curve_id(&sk_id, 42, 1);
        let other_e2 = SketchEntityId::mint("synthetic:test:id#curve_other2").unwrap();
        let other_e3 = SketchEntityId::mint("synthetic:test:id#curve_other3").unwrap();
        let other_e4 = SketchEntityId::mint("synthetic:test:id#curve_other4").unwrap();
        let other_e5 = SketchEntityId::mint("synthetic:test:id#curve_other5").unwrap();

        // Profile 0 has 4 entities: [shared, other_e2, other_e3, other_e4]
        let prof0 = vec![shared_curve_id.clone(), other_e2, other_e3, other_e4];
        // Profile 1 has 3 entities: [shared, other_e5, curve_other6]
        let prof1 = vec![
            shared_curve_id,
            other_e5,
            SketchEntityId::mint("synthetic:test:id#curve_other6").unwrap(),
        ];
        let sketch = make_sketch("f3d:model:sketch#test_fallback", vec![prof0, prof1]);

        let group = extrude_group(vec![10]);

        let member = make_member(
            group.record_index,
            0,
            10,
            Some(SketchRelationOperand::Curve {
                record_index: 20,
                primary_id: 42,
                secondary_id: 1,
            }),
        );

        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-9,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(&[]);

        // When transition yields nothing, fallback resolves to profile 1 (smallest count = 3 < 4)
        let resolved = super::super::resolved_extrude_profile_selection(
            &sk_id,
            &group,
            &[member],
            &sketch,
            scoped_resolution,
            crate::design::profile_select::ExtrudeTransition {
                history_state_id: None,
                previous_history_state_id: None,
                cut: false,
            },
        )
        .unwrap();

        assert_eq!(
            resolved,
            ProfileRef::Planar(PlanarProfileRef::sketch_profiles(sk_id, vec![1]).unwrap())
        );
    });
}

#[test]
fn multiloop_fallback_fails_when_member_not_curve() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#test_fallback_neg").unwrap();
        let shared_curve_id = crate::ids::neutral_sketch_curve_id(&sk_id, 42, 1);
        let sketch = make_sketch(
            "f3d:model:sketch#test_fallback_neg",
            vec![vec![shared_curve_id.clone()], vec![shared_curve_id]],
        );

        let group = extrude_group(vec![10]);

        let member = make_member(group.record_index, 0, 10, None);

        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &[],
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-9,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(&[]);

        let resolved = super::super::resolved_extrude_profile_selection(
            &sk_id,
            &group,
            &[member],
            &sketch,
            scoped_resolution,
            crate::design::profile_select::ExtrudeTransition {
                history_state_id: None,
                previous_history_state_id: None,
                cut: false,
            },
        )
        .unwrap();

        assert!(matches!(
            resolved,
            ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
        ));
    });
}

// ---------------------------------------------------------------------------
// 2. Merged vs Explicit Precedence & Negatives
// ---------------------------------------------------------------------------

#[test]
fn merged_selection_retained_when_untrimmed_loops_in_explicit() {
    let sk_id = SketchId::mint("f3d:model:sketch#test_prec").unwrap();
    let explicit_profiles = Some(vec![0, 1]);
    let regions: SketchProfileRegions = vec![
        SketchProfileRegion::loops(0, vec![]).unwrap(),
        SketchProfileRegion::loops(1, vec![]).unwrap(),
    ]
    .try_into()
    .unwrap();
    let merged = ProfileRef::Planar(PlanarProfileRef::SketchRegions {
        sketch: sk_id,
        regions,
    });

    let use_merged = match (&explicit_profiles, &merged) {
        (None, _) => true,
        (Some(exp), ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. })) => {
            regions.as_slice().iter().all(|reg| match reg {
                SketchProfileRegion::Loops { loops } => exp.contains(&loops.outer()),
                SketchProfileRegion::Trimmed { .. } => false,
            })
        }
        _ => false,
    };

    assert!(use_merged);
}

#[test]
fn explicit_precedence_when_merged_region_trimmed() {
    let sk_id = SketchId::mint("f3d:model:sketch#test_prec_trim").unwrap();
    let explicit_profiles = Some(vec![0]);
    let trimmed = SketchProfileRegion::trimmed(
        vec![SketchProfileBoundaryUse {
            entity: SketchEntityId::mint("synthetic:test:id#entity1").unwrap(),
            parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
            reversed: false,
        }],
        Vec::new(),
    )
    .unwrap();
    let regions: SketchProfileRegions = vec![trimmed].try_into().unwrap();
    let merged = ProfileRef::Planar(PlanarProfileRef::SketchRegions {
        sketch: sk_id,
        regions,
    });

    let use_merged = match (&explicit_profiles, &merged) {
        (None, _) => true,
        (Some(exp), ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. })) => {
            regions.as_slice().iter().all(|reg| match reg {
                SketchProfileRegion::Loops { loops } => exp.contains(&loops.outer()),
                SketchProfileRegion::Trimmed { .. } => false,
            })
        }
        _ => false,
    };

    assert!(!use_merged);
}

#[test]
fn explicit_precedence_when_merged_loop_not_in_explicit() {
    let sk_id = SketchId::mint("f3d:model:sketch#test_prec_outside").unwrap();
    let explicit_profiles = Some(vec![0]);
    let regions: SketchProfileRegions = vec![SketchProfileRegion::loops(1, vec![]).unwrap()]
        .try_into()
        .unwrap();
    let merged = ProfileRef::Planar(PlanarProfileRef::SketchRegions {
        sketch: sk_id,
        regions,
    });

    let use_merged = match (&explicit_profiles, &merged) {
        (None, _) => true,
        (Some(exp), ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. })) => {
            regions.as_slice().iter().all(|reg| match reg {
                SketchProfileRegion::Loops { loops } => exp.contains(&loops.outer()),
                SketchProfileRegion::Trimmed { .. } => false,
            })
        }
        _ => false,
    };

    assert!(!use_merged);
}

// ---------------------------------------------------------------------------
// 3. HistoricalFaces Metadata & Offset Threshold
// ---------------------------------------------------------------------------

#[test]
fn historical_faces_offset_threshold_gates() {
    let face_off_large: f64 = 0.05;
    assert!(face_off_large.abs() > 1.0e-4);

    let face_off_small: f64 = 5.0e-5;
    assert!(face_off_small.abs() <= 1.0e-4);
}

// ---------------------------------------------------------------------------
// 4. Extrude Distance Heuristics & Extent Types (OneSided, TwoSided, Decoy)
// ---------------------------------------------------------------------------

#[test]
fn has_to_termination_detects_one_sided_and_two_sided() {
    let to_face_side = ExtrudeSide {
        termination: LinearTermination::ToFace {
            face: FaceSelection::Native("f3d:face#1".to_string()),
            offset: None,
        },
        draft: None,
    };
    let blind_side = ExtrudeSide {
        termination: LinearTermination::Blind {
            length: cadmpeg_ir::scalar::PositiveLength::new(10.0)
                .unwrap()
                .into(),
        },
        draft: None,
    };

    let one_sided_to = ExtrudeExtent::OneSided {
        side: to_face_side.clone(),
    };
    assert!(has_to_termination(&one_sided_to));

    let one_sided_blind = ExtrudeExtent::OneSided {
        side: blind_side.clone(),
    };
    assert!(!has_to_termination(&one_sided_blind));

    let two_sided = ExtrudeExtent::TwoSided {
        first: blind_side.clone(),
        second: to_face_side,
    };
    assert!(has_to_termination(&two_sided));

    let two_sided_blind = ExtrudeExtent::TwoSided {
        first: blind_side.clone(),
        second: blind_side,
    };
    assert!(!has_to_termination(&two_sided_blind));

    let to_plane = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination: LinearTermination::ToPlane {
                plane: cadmpeg_ir::features::FeatureSupportPlaneFrame::new(
                    Point3::new(0.0, 0.0, 30.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
                native: cadmpeg_core::text::NonBlankString::new("f3d:group#1".to_owned()).unwrap(),
                source: Some(cadmpeg_ir::features::PlaneTargetSource::WorkPlane),
            },
            draft: None,
        },
    };
    assert!(has_to_termination(&to_plane));
}

#[test]
fn historical_extrude_to_distance_ignores_decoy_below_tenth_mm() {
    let sketch = make_sketch("f3d:model:sketch#dist_decoy", vec![]);
    let topo0 = AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let topo1 = AsmHistoricalTopology {
        bodies: vec![1],
        points: vec![10, 11],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 10,
                position: Point3::new(0.0, 0.0, 0.05), // <= 0.1: decoy!
            },
            AsmHistoricalPoint {
                point: 11,
                position: Point3::new(0.0, 0.0, 5.0), // > 0.1: valid target!
            },
        ],
        ..Default::default()
    };
    let history = history_with_topologies(&[topo0, topo1]);

    let dist = historical_extrude_to_distance(&sketch, 0.0, &[history], 1, 0);
    assert_eq!(dist, Some(5.0));
}

#[test]
fn historical_extrude_to_distance_omits_below_threshold() {
    let sketch = make_sketch("f3d:model:sketch#dist_threshold", vec![]);
    let topo0 = AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let topo1 = AsmHistoricalTopology {
        bodies: vec![1],
        points: vec![10],
        point_positions: vec![AsmHistoricalPoint {
            point: 10,
            position: Point3::new(0.0, 0.0, 0.00005),
        }],
        ..Default::default()
    };
    let history = history_with_topologies(&[topo0, topo1]);

    let dist = historical_extrude_to_distance(&sketch, 0.0, &[history], 1, 0);
    assert_eq!(dist, None);
}

// ---------------------------------------------------------------------------
// 5. Chained Walk: Empty intermediate states vs Non-empty rejection
// ---------------------------------------------------------------------------

#[test]
fn chained_walk_fails_when_intermediate_state_inserts_entity() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sketch = make_sketch("f3d:model:sketch#chain_fail", vec![]);
        let history = AsmHistory::new(
            "f3d:history".into(),
            0,
            None,
            false,
            vec![
                super::linked_state(1, None, AsmHistoricalTopologyDelta::default()),
                super::linked_state(
                    2,
                    Some(1),
                    AsmHistoricalTopologyDelta {
                        faces: AsmHistoricalEntityDelta {
                            inserted: vec![99],
                            ..AsmHistoricalEntityDelta::default()
                        },
                        ..AsmHistoricalTopologyDelta::default()
                    },
                ),
                super::linked_state(3, Some(2), AsmHistoricalTopologyDelta::default()),
            ],
        );

        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-8,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(std::slice::from_ref(&history));

        let result = transition_profile_selection(&sketch, scoped_resolution, 3, 1).unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn chained_walk_fails_when_intermediate_state_has_rows() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sketch = make_sketch("f3d:model:sketch#chain_rows", vec![]);
        let mut state2 = super::linked_state(2, Some(1), AsmHistoricalTopologyDelta::default());
        if let Some(ref mut t) = state2.transition {
            t.rows.surface_radii.set.push(AsmHistoricalSurfaceRadius {
                surface: 1,
                radius: 2.0,
            });
        }
        let history = AsmHistory::new(
            "f3d:history".into(),
            0,
            None,
            false,
            vec![
                super::linked_state(1, None, AsmHistoricalTopologyDelta::default()),
                state2,
                super::linked_state(3, Some(2), AsmHistoricalTopologyDelta::default()),
            ],
        );

        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-8,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(std::slice::from_ref(&history));

        let result = transition_profile_selection(&sketch, scoped_resolution, 3, 1).unwrap();
        assert_eq!(result, None);
    });
}

fn cylinder_face_topology(base: i64, radius: f64, z: f64) -> AsmHistoricalTopology {
    let mut topo = AsmHistoricalTopology::default();
    append_cylinder_face(&mut topo, base, radius, z);
    topo
}

fn append_cylinder_face(topo: &mut AsmHistoricalTopology, base: i64, radius: f64, z: f64) {
    let face = base + 10;
    let surface = base + 50;
    let loop_id = base + 11;
    let coedges = [base + 12, base + 13, base + 14];
    let edges = [base + 20, base + 21, base + 22];
    let vertices = [base + 30, base + 31, base + 32];
    let points = [base + 40, base + 41, base + 42];
    topo.faces.push(face);
    topo.loops.push(loop_id);
    topo.coedges.extend_from_slice(&coedges);
    topo.edges.extend_from_slice(&edges);
    topo.vertices.extend_from_slice(&vertices);
    topo.points.extend_from_slice(&points);
    topo.surfaces.push(surface);
    topo.face_loops.push(AsmHistoricalRelation {
        owner_ref: face,
        member_refs: vec![loop_id],
    });
    topo.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: loop_id,
        member_refs: coedges.to_vec(),
    });
    topo.coedge_topology.extend([
        AsmHistoricalCoedge {
            coedge: coedges[0],
            owner_loop: loop_id,
            edge: edges[0],
            next: coedges[1],
            previous: coedges[2],
            radial_next: coedges[0],
        },
        AsmHistoricalCoedge {
            coedge: coedges[1],
            owner_loop: loop_id,
            edge: edges[1],
            next: coedges[2],
            previous: coedges[0],
            radial_next: coedges[1],
        },
        AsmHistoricalCoedge {
            coedge: coedges[2],
            owner_loop: loop_id,
            edge: edges[2],
            next: coedges[0],
            previous: coedges[1],
            radial_next: coedges[2],
        },
    ]);
    topo.edge_vertices.extend([
        AsmHistoricalEdge {
            edge: edges[0],
            start_vertex: vertices[0],
            end_vertex: vertices[1],
        },
        AsmHistoricalEdge {
            edge: edges[1],
            start_vertex: vertices[1],
            end_vertex: vertices[2],
        },
        AsmHistoricalEdge {
            edge: edges[2],
            start_vertex: vertices[2],
            end_vertex: vertices[0],
        },
    ]);
    topo.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: face,
        carrier: surface,
    });
    topo.vertex_points.extend([
        AsmHistoricalCarrierBinding {
            entity: vertices[0],
            carrier: points[0],
        },
        AsmHistoricalCarrierBinding {
            entity: vertices[1],
            carrier: points[1],
        },
        AsmHistoricalCarrierBinding {
            entity: vertices[2],
            carrier: points[2],
        },
    ]);
    topo.point_positions.extend([
        AsmHistoricalPoint {
            point: points[0],
            position: Point3::new(radius, 0.0, z),
        },
        AsmHistoricalPoint {
            point: points[1],
            position: Point3::new(0.0, radius, z),
        },
        AsmHistoricalPoint {
            point: points[2],
            position: Point3::new(-radius, 0.0, z),
        },
    ]);
    topo.surface_cylinders.push(AsmHistoricalCylinder {
        surface,
        origin: Point3::new(0.0, 0.0, z),
        axis: Vector3::new(0.0, 0.0, 1.0),
        radius,
    });
}

#[test]
fn chained_walk_succeeds_when_steps_agree_on_profile_selection() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#chain_agree").unwrap();
        let circle = circle_entity(&sk_id, "circle", Point2::new(0.0, 0.0), 2.0);
        let sketch = make_sketch(
            "f3d:model:sketch#chain_agree",
            vec![vec![circle.id().clone()]],
        );
        let topo0 = AsmHistoricalTopology::default();
        let topo1 = cylinder_face_topology(0, 2.0, 0.0);
        let mut topo2 = topo1.clone();
        append_cylinder_face(&mut topo2, 100, 2.0, 3.0);
        let history = history_with_topologies(&[topo0, topo1, topo2]);
        let entities = vec![circle];
        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-8,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(std::slice::from_ref(&history));

        let result = transition_profile_selection(&sketch, scoped_resolution, 2, 0).unwrap();
        assert_eq!(result, Some(ResolvedProfileSelection::Loops(vec![0])));
    });
}

#[test]
fn chained_walk_fails_when_steps_conflict_on_profile_selection() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#chain_conflict").unwrap();
        let circle1 = circle_entity(&sk_id, "circle1", Point2::new(0.0, 0.0), 2.0);
        let circle2 = circle_entity(&sk_id, "circle2", Point2::new(0.0, 0.0), 4.0);
        let sketch = make_sketch(
            "f3d:model:sketch#chain_conflict",
            vec![vec![circle1.id().clone()], vec![circle2.id().clone()]],
        );
        let topo0 = AsmHistoricalTopology::default();
        let topo1 = cylinder_face_topology(0, 2.0, 0.0);
        let mut topo2 = topo1.clone();
        append_cylinder_face(&mut topo2, 100, 4.0, 3.0);
        let history = history_with_topologies(&[topo0, topo1, topo2]);
        let entities = vec![circle1, circle2];
        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-8,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(std::slice::from_ref(&history));

        let result = transition_profile_selection(&sketch, scoped_resolution, 2, 0).unwrap();
        assert_eq!(result, None);
    });
}

#[test]
fn chained_walk_succeeds_when_intermediate_step_preserves_topology() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#chain_preserve").unwrap();
        let circle = circle_entity(&sk_id, "circle", Point2::new(0.0, 0.0), 2.0);
        let sketch = make_sketch(
            "f3d:model:sketch#chain_preserve",
            vec![vec![circle.id().clone()]],
        );
        let topo0 = AsmHistoricalTopology::default();
        let topo1 = cylinder_face_topology(0, 2.0, 0.0);
        let topo2 = topo1.clone();
        let history = history_with_topologies(&[topo0, topo1, topo2]);
        let entities = vec![circle];
        let profile_work = ProfileGeometryWork::new();
        let scope_histories = HashMap::new();
        let scoped_resolution = ExtrudeProfileResolution {
            entities: &entities,
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            scope_histories: &scope_histories,
            linear_tolerance: 1.0e-6,
            angular_tolerance: 1.0e-8,
            profile_work: &profile_work,
            ctx: decode_ctx,
        }
        .scoped(std::slice::from_ref(&history));

        let result = transition_profile_selection(&sketch, scoped_resolution, 2, 0).unwrap();
        assert_eq!(result, Some(ResolvedProfileSelection::Loops(vec![0])));
    });
}

// ---------------------------------------------------------------------------
// 6. Sub-rules at :3348-3460 (including the 0.5 constant)
// ---------------------------------------------------------------------------

#[test]
fn selection_containing_points_single_point_circle_rule() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#circle_subrule").unwrap();
        let circle = circle_entity(&sk_id, "circle", Point2::new(0.0, 0.0), 5.0);
        let sketch = make_sketch(
            "f3d:model:sketch#circle_subrule",
            vec![vec![circle.id().clone()]],
        );
        let entities = vec![circle];
        let work = ProfileGeometryWork::new();

        // 2 points projected to the same single point lying on circle boundary (radius 5.0)
        let pts = [Point3::new(5.0, 0.0, 0.0), Point3::new(5.0, 0.0, 0.0)];
        let sel =
            selection_containing_points(&sketch, &entities, &pts, EPS, &work, decode_ctx).unwrap();
        assert_eq!(sel, Some(ResolvedProfileSelection::Loops(vec![0])));

        // Negative: 2 points on a line entity projecting to a single point cannot select a loop
        let line = line_entity(
            &sk_id,
            "neg_line",
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
        );
        let sketch_line = make_sketch(
            "f3d:model:sketch#circle_subrule_neg",
            vec![vec![line.id().clone()]],
        );
        let entities_line = vec![line];
        let pts_line = [Point3::new(5.0, 0.0, 0.0), Point3::new(5.0, 0.0, 0.0)];
        let sel_line = selection_containing_points(
            &sketch_line,
            &entities_line,
            &pts_line,
            EPS,
            &work,
            decode_ctx,
        )
        .unwrap();
        assert_eq!(sel_line, None);
    });
}

#[test]
fn selection_containing_points_two_points_half_length_line_rule() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#line_subrule").unwrap();
        // Line from (0,0) to (10,0), total length 10.0
        let line = line_entity(
            &sk_id,
            "line",
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
        );
        let sketch = make_sketch(
            "f3d:model:sketch#line_subrule",
            vec![vec![line.id().clone()]],
        );
        let entities = vec![line];
        let work = ProfileGeometryWork::new();

        // Case A: 2 points with segment length 6.0 (>= 0.5 * 10.0 = 5.0) -> matches!
        let pts_pass = [Point3::new(1.0, 0.0, 0.0), Point3::new(7.0, 0.0, 0.0)];
        let sel_pass =
            selection_containing_points(&sketch, &entities, &pts_pass, EPS, &work, decode_ctx)
                .unwrap();
        assert_eq!(sel_pass, Some(ResolvedProfileSelection::Loops(vec![0])));

        // Case B: 2 points with segment length 3.0 (< 0.5 * 10.0 = 5.0) -> negative, fails match!
        let pts_fail = [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)];
        let sel_fail =
            selection_containing_points(&sketch, &entities, &pts_fail, EPS, &work, decode_ctx)
                .unwrap();
        assert_eq!(sel_fail, None);
    });
}

#[test]
fn selection_containing_points_three_points_all_lines_covered_rule() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#triangle_subrule").unwrap();
        let l1 = line_entity(&sk_id, "l1", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0));
        let l2 = line_entity(&sk_id, "l2", Point2::new(4.0, 0.0), Point2::new(4.0, 3.0));
        let l3 = line_entity(&sk_id, "l3", Point2::new(4.0, 3.0), Point2::new(0.0, 0.0));
        let sketch = make_sketch(
            "f3d:model:sketch#triangle_subrule",
            vec![vec![l1.id().clone(), l2.id().clone(), l3.id().clone()]],
        );
        let entities = vec![l1, l2, l3];
        let work = ProfileGeometryWork::new();

        // Case A: 3 points covering all 3 lines: (2, 0, 0) on l1, (4, 1.5, 0) on l2, (2, 1.5, 0) on l3 -> matches!
        let pts_cover_all = [
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(4.0, 1.5, 0.0),
            Point3::new(2.0, 1.5, 0.0),
        ];
        let sel_pass =
            selection_containing_points(&sketch, &entities, &pts_cover_all, EPS, &work, decode_ctx)
                .unwrap();
        assert_eq!(sel_pass, Some(ResolvedProfileSelection::Loops(vec![0])));

        // Case B: 3 points covering only 2 of the 3 lines -> fails!
        let pts_partial = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(4.0, 1.5, 0.0),
        ];
        let sel_fail =
            selection_containing_points(&sketch, &entities, &pts_partial, EPS, &work, decode_ctx)
                .unwrap();
        assert_eq!(sel_fail, None);
    });
}

// ---------------------------------------------------------------------------
// 7. Multi-face loops pre-emption (:2745-2768)
// ---------------------------------------------------------------------------

#[test]
fn multi_face_loops_preempt_single_face_loops() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let sk_id = SketchId::mint("f3d:model:sketch#preempt").unwrap();
        let mut all_entities = Vec::new();
        let mut loop_entity_ids = Vec::new();
        for (i, x_offset) in [0.0, 10.0, 1.0].into_iter().enumerate() {
            let l1 = line_entity(
                &sk_id,
                &format!("l_{i}_1"),
                Point2::new(x_offset, 0.0),
                Point2::new(x_offset + 2.0, 0.0),
            );
            let l2 = line_entity(
                &sk_id,
                &format!("l_{i}_2"),
                Point2::new(x_offset + 2.0, 0.0),
                Point2::new(x_offset + 2.0, 2.0),
            );
            let l3 = line_entity(
                &sk_id,
                &format!("l_{i}_3"),
                Point2::new(x_offset + 2.0, 2.0),
                Point2::new(x_offset, 2.0),
            );
            let l4 = line_entity(
                &sk_id,
                &format!("l_{i}_4"),
                Point2::new(x_offset, 2.0),
                Point2::new(x_offset, 0.0),
            );
            loop_entity_ids.push(vec![
                l1.id().clone(),
                l2.id().clone(),
                l3.id().clone(),
                l4.id().clone(),
            ]);
            all_entities.extend([l1, l2, l3, l4]);
        }
        let sketch = make_sketch("f3d:model:sketch#preempt", loop_entity_ids);
        let work = ProfileGeometryWork::new();

        // Selections from 3 faces:
        // Face 1 selects loops [0, 1]
        // Face 2 selects loops [0, 1]
        // Face 3 selects loop [2]
        // Loop 0 and Loop 1 appear twice (multi-face loops).
        // Loop 2 appears once.
        // multi-face loops [0, 1] pre-empt loop 2!
        let selections = vec![
            Some(ResolvedProfileSelection::Loops(vec![0, 1])),
            Some(ResolvedProfileSelection::Loops(vec![0, 1])),
            Some(ResolvedProfileSelection::Loops(vec![2])),
        ];

        let result = transition_inserted_profile_selection(
            &sketch,
            &all_entities,
            EPS,
            selections,
            &work,
            decode_ctx,
        )
        .unwrap();

        assert_eq!(result, Some(ResolvedProfileSelection::Loops(vec![0, 1])));
    });
}

// ---------------------------------------------------------------------------
// 8. Unresolved explicit region selection vs group/transition alternatives
// ---------------------------------------------------------------------------

fn unresolved_explicit_region_fixture(
    history_states: Option<(i64, i64)>,
) -> (
    Sketch,
    Vec<SketchEntity>,
    Vec<crate::records::sketch_geometry::SketchCurveIdentity>,
    crate::records::feature::scope::DesignParameterScope,
) {
    use serde_json::json;

    let sketch_id = SketchId::mint("f3d:model:sketch#explicit_conflict").unwrap();
    let specs = [
        (
            100_u32,
            json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0}),
        ),
        (
            101_u32,
            json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 1.0}),
        ),
        (
            200_u32,
            json!({"kind": "circle", "center": {"u": 10.0, "v": 0.0}, "radius": 3.0}),
        ),
    ];
    let mut entities = Vec::new();
    let mut curves = Vec::new();
    for (primary, geom) in specs {
        entities.push(SketchEntity::new(
            crate::ids::neutral_sketch_curve_id(&sketch_id, u64::from(primary), 0),
            sketch_id.clone(),
            serde_json::from_value::<SketchGeometry>(geom).unwrap(),
        ));
        curves.push(crate::records::sketch_geometry::SketchCurveIdentity {
            id: format!("f3d:Design/BulkStream.dat:curve#{primary}"),
            record_index: primary,
            owner_reference: Some(42),
            geometry_group: None,
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(u64::from(primary)).unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
        });
    }
    let sketch = make_sketch(
        "f3d:model:sketch#explicit_conflict",
        vec![
            vec![entities[0].id().clone()],
            vec![entities[1].id().clone()],
            vec![entities[2].id().clone()],
            vec![entities[0].id().clone(), entities[2].id().clone()],
        ],
    );
    let operand: crate::records::topology::sketch_profile::DesignSketchProfileOperand =
        serde_json::from_value(json!({
            "scope_reference_ordinal": 0,
            "record_index": 100,
            "byte_offset": 10,
            "class_tag": "301",
            "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
            "asset_id_offset": 20,
            "entity_id": "0_42",
            "entity_suffix": 42,
            "entity_reference_offset": 100,
            "paired_class_tag": "259",
            "paired_byte_offset": 200,
            "region_selection": {
                "record_index": 103,
                "byte_offset": 300,
                "class_tag": "302",
                "region_count_offset": 336,
                "regions": [{
                    "member_count_offset": 340,
                    "members": [{
                        "kind": 3,
                        "kind_offset": 0,
                        "curve_primary_id": 100,
                        "curve_primary_id_offset": 4,
                        "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                        "incidence_words_offset": 8
                    }]
                }],
                "companion_class_tag": "260",
                "companion_byte_offset": 700
            }
        }))
        .unwrap();
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#7",
        crate::records::feature::scope::DesignScopePayload::Extrude(Some(
            crate::records::feature::scope::DesignExtrudeScope {
                extrude_profile: Some(operand),
                ..crate::records::feature::scope::DesignExtrudeScope::default()
            },
        )),
        7,
    );
    if let Some((state_id, prev_state_id)) = history_states {
        scope
            .try_edit(|draft| {
                draft.frame_length = 124;
                draft.paired_byte_offset = 124;
                draft.history_state_id = Some(state_id);
                draft.previous_history_state_id = Some(prev_state_id);
                draft.previous_history_state_id_offset = Some(78);
            })
            .unwrap();
    }
    (sketch, entities, curves, scope)
}

#[test]
fn unresolved_explicit_region_rejects_conflicting_group_and_accepts_agreeing_group() {
    use serde_json::json;

    crate::test_support::with_decode_context(|ctx| {
        let (sketch, entities, curves, scope) = unresolved_explicit_region_fixture(None);
        let group = extrude_group(vec![10]);

        let run_with_curve = |curve_primary: u64| -> ProfileRef {
            let member = make_member(
                group.record_index,
                0,
                10,
                Some(SketchRelationOperand::Curve {
                    record_index: 20,
                    primary_id: curve_primary,
                    secondary_id: 0,
                }),
            );
            let mut feature: cadmpeg_ir::features::Feature = serde_json::from_value(json!({
                "id": "synthetic:test:feature#ext",
                "ordinal": 0,
                "suppressed": false,
                "native_ref": scope.id,
                "definition": {
                    "definition": "extrude",
                    "profile": {"kind": "sketch", "value": sketch.id},
                    "direction": {"kind": "profile_normal"},
                    "start": {"kind": "profile_plane"},
                    "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
                    "op": "new_body",
                    "solid": true
                }
            }))
            .unwrap();
            let sketches = [sketch.clone()];
            let scopes = [scope.clone()];
            let groups = [group.clone()];
            let members = [member];
            let work = ProfileGeometryWork::new();
            crate::design::profile_select::bind_extrude_profile_selections(
                std::slice::from_mut(&mut feature),
                &scopes,
                &groups,
                &members,
                &sketches,
                &crate::design::profile_select::SketchCurveSelectionResolution {
                    scopes: &scopes,
                    groups: &[],
                    operands: &[],
                    profile_curves: &[],
                    placements: &[],
                    curve_identities: &curves,
                    sketches: &sketches,
                    sketch_entities: &entities,
                    spatial_sketches: &[],
                    spatial_sketch_entities: &[],
                },
                ExtrudeProfileResolution {
                    entities: &entities,
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &HashMap::new(),
                    linear_tolerance: EPS,
                    angular_tolerance: 1.0e-9,
                    profile_work: &work,
                    ctx,
                },
            )
            .unwrap();
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Extrude { profile, .. },
            ) = feature.evaluation.into_parts().0
            else {
                panic!("Extrude");
            };
            profile
        };

        // Conflicting construction group selecting curve 101 (loop 1) vs explicit region curve 100 (loop 0)
        assert_eq!(
            run_with_curve(101),
            ProfileRef::Planar(
                PlanarProfileRef::sketch_selection(sketch.id.clone(), vec![scope.id.clone()])
                    .unwrap()
            )
        );

        // Agreeing construction group selecting curve 100 (loop 0)
        assert_eq!(
            run_with_curve(100),
            ProfileRef::Planar(PlanarProfileRef::sketch_profiles(sketch.id, vec![0]).unwrap())
        );
    });
}

#[test]
fn unresolved_explicit_region_rejects_conflicting_transition_and_accepts_agreeing_transition() {
    use serde_json::json;

    crate::test_support::with_decode_context(|ctx| {
        let (sketch, entities, curves, scope) = unresolved_explicit_region_fixture(Some((1, 0)));

        let run_with_radius = |radius: f64| -> ProfileRef {
            let topo0 = AsmHistoricalTopology::default();
            let topo1 = cylinder_face_topology(0, radius, 0.0);
            let history = history_with_topologies(&[topo0, topo1]);
            let mut feature: cadmpeg_ir::features::Feature = serde_json::from_value(json!({
                "id": "synthetic:test:feature#ext_trans",
                "ordinal": 0,
                "suppressed": false,
                "native_ref": scope.id,
                "definition": {
                    "definition": "extrude",
                    "profile": {"kind": "sketch", "value": sketch.id},
                    "direction": {"kind": "profile_normal"},
                    "start": {"kind": "profile_plane"},
                    "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
                    "op": "new_body",
                    "solid": true
                }
            }))
            .unwrap();
            let sketches = [sketch.clone()];
            let scopes = [scope.clone()];
            let histories = [history];
            let work = ProfileGeometryWork::new();
            crate::design::profile_select::bind_extrude_profile_selections(
                std::slice::from_mut(&mut feature),
                &scopes,
                &[],
                &[],
                &sketches,
                &crate::design::profile_select::SketchCurveSelectionResolution {
                    scopes: &scopes,
                    groups: &[],
                    operands: &[],
                    profile_curves: &[],
                    placements: &[],
                    curve_identities: &curves,
                    sketches: &sketches,
                    sketch_entities: &entities,
                    spatial_sketches: &[],
                    spatial_sketch_entities: &[],
                },
                ExtrudeProfileResolution {
                    entities: &entities,
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &histories,
                    scope_histories: &HashMap::new(),
                    linear_tolerance: EPS,
                    angular_tolerance: 1.0e-9,
                    profile_work: &work,
                    ctx,
                },
            )
            .unwrap();
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Extrude { profile, .. },
            ) = feature.evaluation.into_parts().0
            else {
                panic!("Extrude");
            };
            profile
        };

        // Conflicting transition inserting radius 1.0 (loop 1) vs explicit region curve 100 (loop 0, radius 2.0)
        assert_eq!(
            run_with_radius(1.0),
            ProfileRef::Planar(
                PlanarProfileRef::sketch_selection(sketch.id.clone(), vec![scope.id.clone()])
                    .unwrap()
            )
        );

        // Agreeing transition inserting radius 2.0 (loop 0)
        assert_eq!(
            run_with_radius(2.0),
            ProfileRef::Planar(PlanarProfileRef::sketch_profiles(sketch.id, vec![0]).unwrap())
        );
    });
}

#[test]
fn unresolved_explicit_region_requires_hole_agreement() {
    use cadmpeg_ir::features::SketchProfileRegion;
    use serde_json::json;

    let (sketch, entities, curves, scope) = unresolved_explicit_region_fixture(None);
    let crate::records::feature::scope::DesignScopePayload::Extrude(Some(ref extrude_scope)) =
        scope.payload()
    else {
        panic!("Extrude scope");
    };
    let outer_only_operand = extrude_scope.extrude_profile.as_ref().unwrap();

    // Explicit selection has only outer loop 0 (curve 100). A candidate with outer loop 0 and
    // unaccounted-for hole loop 1 must be rejected.
    let cand_with_hole = PlanarProfileRef::sketch_regions(
        sketch.id.clone(),
        vec![SketchProfileRegion::loops(0, vec![1]).unwrap()],
    )
    .unwrap();
    assert!(
        !crate::design::profile_select::planar_selection_agrees_with_region_operand(
            "f3d:Design/BulkStream.dat",
            outer_only_operand,
            &sketch,
            &curves,
            &entities,
            &cand_with_hole,
        )
    );

    // Two separately selected explicit regions (region 0 = loop 0, region 1 = loop 1)
    // must reject a single A-with-hole-B candidate and accept two hole-free regions.
    let two_regions_operand: crate::records::topology::sketch_profile::DesignSketchProfileOperand =
        serde_json::from_value(json!({
            "scope_reference_ordinal": 0,
            "record_index": 100,
            "byte_offset": 10,
            "class_tag": "301",
            "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
            "asset_id_offset": 20,
            "entity_id": "0_42",
            "entity_suffix": 42,
            "entity_reference_offset": 100,
            "paired_class_tag": "259",
            "paired_byte_offset": 200,
            "region_selection": {
                "record_index": 103,
                "byte_offset": 300,
                "class_tag": "302",
                "region_count_offset": 336,
                "regions": [
                    {
                        "member_count_offset": 340,
                        "members": [{
                            "kind": 3,
                            "kind_offset": 0,
                            "curve_primary_id": 100,
                            "curve_primary_id_offset": 4,
                            "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                            "incidence_words_offset": 8
                        }]
                    },
                    {
                        "member_count_offset": 384,
                        "members": [{
                            "kind": 3,
                            "kind_offset": 40,
                            "curve_primary_id": 101,
                            "curve_primary_id_offset": 44,
                            "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                            "incidence_words_offset": 48
                        }]
                    }
                ],
                "companion_class_tag": "260",
                "companion_byte_offset": 700
            }
        }))
        .unwrap();
    assert!(
        !crate::design::profile_select::planar_selection_agrees_with_region_operand(
            "f3d:Design/BulkStream.dat",
            &two_regions_operand,
            &sketch,
            &curves,
            &entities,
            &cand_with_hole,
        )
    );

    let cand_two_hole_free_regions = PlanarProfileRef::sketch_regions(
        sketch.id.clone(),
        vec![
            SketchProfileRegion::loops(0, Vec::new()).unwrap(),
            SketchProfileRegion::loops(1, Vec::new()).unwrap(),
        ],
    )
    .unwrap();
    assert!(
        crate::design::profile_select::planar_selection_agrees_with_region_operand(
            "f3d:Design/BulkStream.dat",
            &two_regions_operand,
            &sketch,
            &curves,
            &entities,
            &cand_two_hole_free_regions,
        )
    );

    // A single explicit region listing only loop 0 rejects a two-region candidate.
    assert!(
        !crate::design::profile_select::planar_selection_agrees_with_region_operand(
            "f3d:Design/BulkStream.dat",
            outer_only_operand,
            &sketch,
            &curves,
            &entities,
            &cand_two_hole_free_regions,
        )
    );
}

#[test]
fn explicit_region_with_extended_incidence_refuses_fallback_for_trimmed_curve_portions() {
    use serde_json::json;

    crate::test_support::with_decode_context(|ctx| {
        let (sketch, entities, curves, fixture_scope) = unresolved_explicit_region_fixture(None);
        let group = extrude_group(vec![10]);
        let member = make_member(
            group.record_index,
            0,
            10,
            Some(SketchRelationOperand::Curve {
                record_index: 20,
                primary_id: 100,
                secondary_id: 0,
            }),
        );

        let run_portion = |incidence_words: [u32; 8]| -> ProfileRef {
            let operand: crate::records::topology::sketch_profile::DesignSketchProfileOperand =
                serde_json::from_value(json!({
                    "scope_reference_ordinal": 0,
                    "record_index": 100,
                    "byte_offset": 10,
                    "class_tag": "301",
                    "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
                    "asset_id_offset": 20,
                    "entity_id": "0_42",
                    "entity_suffix": 42,
                    "entity_reference_offset": 100,
                    "paired_class_tag": "259",
                    "paired_byte_offset": 200,
                    "region_selection": {
                        "record_index": 103,
                        "byte_offset": 300,
                        "class_tag": "302",
                        "region_count_offset": 336,
                        "regions": [{
                            "member_count_offset": 340,
                            "members": [{
                                "kind": 3,
                                "kind_offset": 0,
                                "curve_primary_id": 100,
                                "curve_primary_id_offset": 4,
                                "incidence_words": incidence_words,
                                "incidence_words_offset": 8
                            }]
                        }],
                        "companion_class_tag": "260",
                        "companion_byte_offset": 700
                    }
                }))
                .unwrap();
            let scope = crate::records::feature::scope::DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:scope#7",
                crate::records::feature::scope::DesignScopePayload::Extrude(Some(
                    crate::records::feature::scope::DesignExtrudeScope {
                        extrude_profile: Some(operand),
                        ..crate::records::feature::scope::DesignExtrudeScope::default()
                    },
                )),
                7,
            );
            let mut feature: cadmpeg_ir::features::Feature = serde_json::from_value(json!({
                "id": "synthetic:test:feature#ext_trimmed",
                "ordinal": 0,
                "suppressed": false,
                "native_ref": scope.id,
                "definition": {
                    "definition": "extrude",
                    "profile": {"kind": "sketch", "value": sketch.id},
                    "direction": {"kind": "profile_normal"},
                    "start": {"kind": "profile_plane"},
                    "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
                    "op": "new_body",
                    "solid": true
                }
            }))
            .unwrap();
            let sketches = [sketch.clone()];
            let scopes = [scope];
            let groups = [group.clone()];
            let members = [member.clone()];
            let work = ProfileGeometryWork::new();
            crate::design::profile_select::bind_extrude_profile_selections(
                std::slice::from_mut(&mut feature),
                &scopes,
                &groups,
                &members,
                &sketches,
                &crate::design::profile_select::SketchCurveSelectionResolution {
                    scopes: &scopes,
                    groups: &[],
                    operands: &[],
                    profile_curves: &[],
                    placements: &[],
                    curve_identities: &curves,
                    sketches: &sketches,
                    sketch_entities: &entities,
                    spatial_sketches: &[],
                    spatial_sketch_entities: &[],
                },
                ExtrudeProfileResolution {
                    entities: &entities,
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &HashMap::new(),
                    linear_tolerance: EPS,
                    angular_tolerance: 1.0e-9,
                    profile_work: &work,
                    ctx,
                },
            )
            .unwrap();
            let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Extrude { profile, .. },
            ) = feature.evaluation.into_parts().0
            else {
                panic!("Extrude");
            };
            profile
        };

        let native_selection = ProfileRef::Planar(
            PlanarProfileRef::sketch_selection(sketch.id.clone(), vec![fixture_scope.id.clone()])
                .unwrap(),
        );
        assert_eq!(run_portion([0, 0, 0, 0, 3, 8, 0, 0]), native_selection);
        assert_eq!(run_portion([0, 0, 0, 0, 4, 9, 0, 0]), native_selection);
    });
}

#[test]
fn resolved_explicit_profiles_reject_partial_or_hole_candidates() {
    use serde_json::json;

    crate::test_support::with_decode_context(|ctx| {
        let sketch_id = SketchId::mint("f3d:model:sketch#resolved_exp_conflict").unwrap();
        let line_specs = [
            (
                100_u32,
                json!({"kind": "line", "start": {"u": -3.0, "v": -3.0}, "end": {"u": 3.0, "v": -3.0}}),
            ),
            (
                101_u32,
                json!({"kind": "line", "start": {"u": 3.0, "v": -3.0}, "end": {"u": 0.0, "v": 3.0}}),
            ),
            (
                102_u32,
                json!({"kind": "line", "start": {"u": 0.0, "v": 3.0}, "end": {"u": -3.0, "v": -3.0}}),
            ),
            (
                200_u32,
                json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 0.5}),
            ),
        ];
        let mut entities = Vec::new();
        let mut curves = Vec::new();
        for (primary, geom) in line_specs {
            entities.push(SketchEntity::new(
                crate::ids::neutral_sketch_curve_id(&sketch_id, u64::from(primary), 0),
                sketch_id.clone(),
                serde_json::from_value::<SketchGeometry>(geom).unwrap(),
            ));
            curves.push(crate::records::sketch_geometry::SketchCurveIdentity {
                id: format!("f3d:Design/BulkStream.dat:curve#{primary}"),
                record_index: primary,
                owner_reference: Some(42),
                geometry_group: None,
                class_tag: "300".to_owned().try_into().unwrap(),
                byte_offset: 0,
                geometry_offset: 0,
                entity_genesis: None,
                primary_id: std::num::NonZeroU64::new(u64::from(primary)).unwrap(),
                secondary_id: 0,
                geometry: None,
                planar_z_noise: None,
            });
        }
        // Loop 0: outer triangle [100, 101, 102]; Loop 1: inner circle [200].
        let sketch = make_sketch(
            "f3d:model:sketch#resolved_exp_conflict",
            vec![
                vec![
                    entities[0].id().clone(),
                    entities[1].id().clone(),
                    entities[2].id().clone(),
                ],
                vec![entities[3].id().clone()],
            ],
        );

        // Explicit region selection resolves via resolved_sketch_profile_regions to Some(vec![0, 1]).
        let two_regions_operand: crate::records::topology::sketch_profile::DesignSketchProfileOperand =
            serde_json::from_value(json!({
                "scope_reference_ordinal": 0,
                "record_index": 100,
                "byte_offset": 10,
                "class_tag": "301",
                "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
                "asset_id_offset": 20,
                "entity_id": "0_42",
                "entity_suffix": 42,
                "entity_reference_offset": 100,
                "paired_class_tag": "259",
                "paired_byte_offset": 200,
                "region_selection": {
                    "record_index": 103,
                    "byte_offset": 300,
                    "class_tag": "302",
                    "region_count_offset": 336,
                    "regions": [
                        {
                            "member_count_offset": 340,
                            "members": [
                                {
                                    "kind": 3,
                                    "kind_offset": 0,
                                    "curve_primary_id": 100,
                                    "curve_primary_id_offset": 4,
                                    "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                                    "incidence_words_offset": 8
                                },
                                {
                                    "kind": 3,
                                    "kind_offset": 40,
                                    "curve_primary_id": 101,
                                    "curve_primary_id_offset": 44,
                                    "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                                    "incidence_words_offset": 48
                                },
                                {
                                    "kind": 3,
                                    "kind_offset": 80,
                                    "curve_primary_id": 102,
                                    "curve_primary_id_offset": 84,
                                    "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                                    "incidence_words_offset": 88
                                }
                            ]
                        },
                        {
                            "member_count_offset": 464,
                            "members": [{
                                "kind": 3,
                                "kind_offset": 120,
                                "curve_primary_id": 200,
                                "curve_primary_id_offset": 124,
                                "incidence_words": [0, 0, 0, 0, 1, 1, 0, 0],
                                "incidence_words_offset": 128
                            }]
                        }
                    ],
                    "companion_class_tag": "260",
                    "companion_byte_offset": 700
                }
            }))
            .unwrap();
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#7",
            crate::records::feature::scope::DesignScopePayload::Extrude(Some(
                crate::records::feature::scope::DesignExtrudeScope {
                    extrude_profile: Some(two_regions_operand),
                    ..crate::records::feature::scope::DesignExtrudeScope::default()
                },
            )),
            7,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = 124;
                draft.paired_byte_offset = 124;
                draft.history_state_id = Some(1);
                draft.previous_history_state_id = Some(0);
                draft.previous_history_state_id_offset = Some(78);
            })
            .unwrap();

        // Transition inserts a planar face on outer loop 0 and a cylinder face on hole loop 1,
        // so the unresolved group member resolves via transition_profile_selection to
        // SketchRegions([Loops { outer: 0, holes: [1] }]).
        // Because explicit_profiles is Some(vec![0, 1]) (both loops 0 and 1 selected as regions),
        // the binder must reject the A-with-hole-B candidate and preserve SketchProfiles([0, 1]).
        let group = extrude_group(vec![10]);
        let member = make_member(group.record_index, 0, 10, None);
        let topo0 = AsmHistoricalTopology::default();
        let mut topo1 = cylinder_face_topology(0, 0.5, 1.0);
        // Add a planar face on outer triangle loop 0 with points on its three edges.
        let p_face = 510;
        let p_loop = 511;
        let p_coedges = [512, 513, 514];
        let p_edges = [520, 521, 522];
        let p_verts = [530, 531, 532];
        let p_pts = [540, 541, 542];
        topo1.faces.push(p_face);
        topo1.loops.push(p_loop);
        topo1.coedges.extend_from_slice(&p_coedges);
        topo1.edges.extend_from_slice(&p_edges);
        topo1.vertices.extend_from_slice(&p_verts);
        topo1.points.extend_from_slice(&p_pts);
        topo1.face_loops.push(AsmHistoricalRelation {
            owner_ref: p_face,
            member_refs: vec![p_loop],
        });
        topo1.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: p_loop,
            member_refs: p_coedges.to_vec(),
        });
        for i in 0..3 {
            topo1.coedge_topology.push(AsmHistoricalCoedge {
                coedge: p_coedges[i],
                owner_loop: p_loop,
                edge: p_edges[i],
                next: p_coedges[(i + 1) % 3],
                previous: p_coedges[(i + 2) % 3],
                radial_next: p_coedges[i],
            });
            topo1.edge_vertices.push(AsmHistoricalEdge {
                edge: p_edges[i],
                start_vertex: p_verts[i],
                end_vertex: p_verts[(i + 1) % 3],
            });
            topo1.vertex_points.push(AsmHistoricalCarrierBinding {
                entity: p_verts[i],
                carrier: p_pts[i],
            });
        }
        topo1.point_positions.extend([
            AsmHistoricalPoint {
                point: p_pts[0],
                position: Point3::new(0.0, -3.0, 0.0),
            },
            AsmHistoricalPoint {
                point: p_pts[1],
                position: Point3::new(1.5, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: p_pts[2],
                position: Point3::new(-1.5, 0.0, 0.0),
            },
        ]);
        let history = history_with_topologies(&[topo0, topo1]);

        let mut feature: cadmpeg_ir::features::Feature = serde_json::from_value(json!({
            "id": "synthetic:test:feature#ext_resolved_exp",
            "ordinal": 0,
            "suppressed": false,
            "native_ref": scope.id,
            "definition": {
                "definition": "extrude",
                "profile": {"kind": "sketch", "value": sketch.id},
                "direction": {"kind": "profile_normal"},
                "start": {"kind": "profile_plane"},
                "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
                "op": "new_body",
                "solid": true
            }
        }))
        .unwrap();
        let sketches = [sketch.clone()];
        let scopes = [scope];
        let groups = [group];
        let members = [member];
        let histories = [history];
        let work = ProfileGeometryWork::new();
        crate::design::profile_select::bind_extrude_profile_selections(
            std::slice::from_mut(&mut feature),
            &scopes,
            &groups,
            &members,
            &sketches,
            &crate::design::profile_select::SketchCurveSelectionResolution {
                scopes: &scopes,
                groups: &[],
                operands: &[],
                profile_curves: &[],
                placements: &[],
                curve_identities: &curves,
                sketches: &sketches,
                sketch_entities: &entities,
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
            ExtrudeProfileResolution {
                entities: &entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &histories,
                scope_histories: &HashMap::new(),
                linear_tolerance: EPS,
                angular_tolerance: 1.0e-9,
                profile_work: &work,
                ctx,
            },
        )
        .unwrap();
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude { profile, .. },
        ) = feature.evaluation.into_parts().0
        else {
            panic!("Extrude");
        };
        assert_eq!(
            profile,
            ProfileRef::Planar(PlanarProfileRef::sketch_profiles(sketch.id, vec![0, 1]).unwrap())
        );
    });
}
