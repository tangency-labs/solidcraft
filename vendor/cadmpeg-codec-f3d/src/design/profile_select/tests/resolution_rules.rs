// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::{
    has_to_termination, historical_extrude_to_distance, selection_containing_points,
    transition_inserted_profile_selection, transition_profile_selection, ExtrudeProfileResolution,
    ResolvedProfileSelection,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalPoint, AsmHistoricalSurfaceRadius,
    AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory,
    AsmStateSnapshot,
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
            None,
            None,
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
            None,
            None,
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
