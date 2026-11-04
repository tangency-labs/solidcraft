// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

mod treatment;

use crate::history::bind_scope_histories;
use crate::history::bodies_intersecting;
use crate::history::bound_history_state_pair;
use crate::history::bound_scope_history;
use crate::history::edge_changes_across_state_chain;
use crate::history::edge_recipe_reference_context;
use crate::history::effective_scope_previous_history_state_id;
use crate::history::face_changes_across_state_chain;
use crate::history::header_scope_previous_history_state_id;
use crate::history::historical_edge_axis;
use crate::history::historical_face_support_contexts;
use crate::history::historical_loop_boundary;
use crate::history::historical_topology;
use crate::history::preceding_support_face_slots;
use crate::history::resolve_pattern_face_by_surface_radius;
use crate::history::selection::boundary_edges_in_changes;
use crate::history::selection::faces_in_topology;
use crate::history::selection::historical_edge_context;
use crate::history::selection::historical_identity_edge;
use crate::history::selection::historical_pattern_identity_axes;
use crate::history::selection::historical_pattern_identity_axes_for_selection;
use crate::history::selection::incident_loop_counts_satisfy_sides;
use crate::history::selection::recipe_selector_candidates;
use crate::history::selection::snapshot_edge_identity_revision;
use crate::history::selection::EdgeContextIndex;
use crate::history::selection::HistoricalIdentityIndex;
use crate::history::terminal_edge_recipe_faces;
use crate::history::terminal_edge_recipe_reference_faces;
use crate::history::unique_header_history_state_pair;
use crate::history::unique_history_state;
use crate::history::unique_history_state_pair;
use crate::history::EdgeBoundaryContext;
use crate::history::FaceBoundaryArena;
use crate::history_records::AsmDeltaState;
use crate::history_records::AsmHistoricalCoedge;
use crate::history_records::AsmHistoricalEdge;
use crate::history_records::AsmHistoricalRelation;
use crate::history_records::AsmHistoricalTopology;
use crate::history_records::AsmHistoricalTransition;
use crate::history_records::AsmHistoryRecord;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::historical_context::AsmFaceBoundary;
use std::collections::{BTreeSet, HashMap, HashSet};

#[test]
fn header_state_pairs_ignore_bound_transition_predecessors() {
    let state = |state_id, next_ref| {
        (
            AsmDeltaState {
                next_ref,
                ..change_state(state_id)
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let mut history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(10, Some(6)), state(6, Some(4)), state(4, None)],
    );
    history.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(4);
    let histories = [history];
    let (_, current, previous) =
        unique_header_history_state_pair(&histories, 10, 6).expect("header state pair");
    assert_eq!(current.state_id, 10);
    assert_eq!(previous.state_id, 6);
    assert!(unique_history_state_pair(&histories, 10, 6).is_none());

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#0",
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        0,
    );
    scope
        .try_edit(|draft| draft.history_state_id = Some(10))
        .unwrap();
    assert_eq!(
        header_scope_previous_history_state_id(&scope, &histories),
        Some(6)
    );
    assert_eq!(
        effective_scope_previous_history_state_id(&scope, &histories),
        None
    );
}

#[test]
fn state_pairs_are_resolved_within_one_reachable_history() {
    let state = |history: &str, state_id: i64, previous_state_id: Option<i64>| {
        (
            AsmDeltaState {
                id: format!("{history}:state#{state_id}"),
                parent: history.into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: previous_state_id,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = |id: &str, current| {
        crate::test_support::history::history(
            id.into(),
            0,
            None,
            false,
            vec![state(id, current, Some(2)), state(id, 2, None)],
        )
    };
    let histories = [history("first", 7), history("second", 9)];
    let (resolved, current, previous) =
        unique_history_state_pair(&histories, 9, 2).expect("state-local pair");
    assert_eq!(resolved.id, "second");
    assert_eq!(current.state_id, 9);
    assert_eq!(previous.state_id, 2);
    assert!(unique_history_state(&histories, 2).is_none());

    let duplicate_pair = [history("first", 9), history("second", 9)];
    assert!(unique_history_state_pair(&duplicate_pair, 9, 2).is_none());

    let indirect = crate::test_support::history::history(
        "indirect".into(),
        0,
        None,
        false,
        vec![
            state("indirect", 23, Some(21)),
            state("indirect", 21, Some(11)),
            state("indirect", 11, None),
        ],
    );
    let direct = crate::test_support::history::history(
        "direct".into(),
        0,
        None,
        false,
        vec![state("direct", 23, Some(11)), state("direct", 11, None)],
    );
    let histories = [indirect, direct];
    let (resolved, _, _) = unique_history_state_pair(&histories, 23, 11)
        .expect("direct transition takes precedence over a reachable pair");
    assert_eq!(resolved.id, "direct");
}

#[test]
fn ambiguous_scope_histories_use_exact_result_body_sources() {
    use crate::records::{
        bodies::DesignBodyBinding,
        topology::{
            body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
            body_recipe::DesignOperandOwner,
        },
    };
    use cadmpeg_ir::ids::FaceId;

    let state = |history: &str, state_id: i64, previous_state_id: Option<i64>| {
        (
            AsmDeltaState {
                id: format!("{history}:asm-delta-state#{state_id}"),
                parent: history.into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: previous_state_id,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = |source: &str| {
        let id = format!("f3d:asset/Breps.BlobParts/BREP.{source}.smbh:asm-history#1");
        crate::test_support::history::history(
            id.clone(),
            0,
            None,
            false,
            vec![state(&id, 9, Some(2)), state(&id, 2, None)],
        )
    };
    let histories = [history("first"), history("second")];
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Revolve,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(9);
            draft.previous_history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let next_scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::Sketch,
        200,
    );
    let binding = DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire {
        component_owner: None,
        component_owner_unresolved: false,
        history_only: false,
        id: format!("{stream}:design-body-binding#0"),
        stream: "Design/BulkStream.dat".into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 1,
        asm_body_key_offset: 0,
        entity_suffix: 150,
        entity_suffix_offset: 8,
        blob_name: "BREP.second.smbh".into(),
        blob_name_offset: 16,
        body: None,
    })
    .unwrap();
    let scopes = vec![scope.clone(), next_scope];
    let bindings =
        { bind_scope_histories(&scopes, std::slice::from_ref(&binding), &[], &histories) };
    assert_eq!(bindings[&scope.id], histories[1].id);
    assert_eq!(
        bound_scope_history(&scope.id, &bindings, &histories)
            .expect("scope binding resolves one history")
            .id,
        histories[1].id
    );

    let operand = DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: format!("{stream}:design-body-recipe-operand#120"),
            scope_record_index: scope.record_index,
            owner: DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: 0,
            },
            record_index: 120,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![DesignBodyRecipeReference {
                design_reference: 1,
                design_reference_offset: 25,
                form: 4,
                form_offset: 33,
                candidate_faces: vec![
                    FaceId::mint("f3d:brep/second.smbh/brep:entity#1").expect("identity grammar")
                ],
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: 123,
            nested_record_index_offset: 38,
            recipe_id: format!("{stream}:construction-recipe#1"),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 124,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let bindings =
        { bind_scope_histories(&scopes, &[], std::slice::from_ref(&operand), &histories) };
    assert_eq!(bindings[&scope.id], histories[1].id);
}

#[test]
fn state_pairs_use_raw_next_links_before_transitions_are_derived() {
    let state = |state_id, node_index, previous_ref, next_ref| {
        (
            AsmDeltaState {
                id: format!("history:state-{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref,
                next_ref,
                node_index,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            None,
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            state(10, 0, None, Some(2)),
            state(6, 2, Some(0), Some(3)),
            state(4, 3, Some(2), Some(1)),
            state(2, 1, Some(3), None),
        ],
    );
    let histories = [history];
    let (resolved, current, previous) =
        unique_history_state_pair(&histories, 10, 6).expect("raw direct state pair");
    assert_eq!(resolved.id, "history");
    assert_eq!(current.state_id, 10);
    assert_eq!(previous.state_id, 6);
    let (_, current, previous) =
        unique_history_state_pair(&histories, 10, 4).expect("raw reachable state pair");
    assert_eq!(current.state_id, 10);
    assert_eq!(previous.state_id, 4);
    let mut omitted_predecessor = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#0",
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        0,
    );
    omitted_predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(10);
        })
        .unwrap();
    assert_eq!(
        effective_scope_previous_history_state_id(&omitted_predecessor, &histories),
        Some(6)
    );

    let mut root = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#1",
        crate::records::feature::scope::DesignFeatureKind::BaseFlange,
        1,
    );
    root.try_edit(|draft| {
        draft.history_state_id = Some(4);
    })
    .unwrap();
    let mut successor = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:native:scope#2",
        crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
        2,
    );
    successor
        .try_edit(|draft| {
            draft.history_state_id = Some(10);
            draft.previous_history_state_id = Some(6);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![root, successor];
    let bindings = bind_scope_histories(&scopes, &[], &[], &histories);
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings["f3d:native:scope#1"], "history");
    assert_eq!(bindings["f3d:native:scope#2"], "history");

    let mut inconsistent = histories[0].clone();
    inconsistent.states_mut()[0].transition = Some(AsmHistoricalTransition {
        record_revisions: Vec::new(),
        rows: crate::history_records::AsmHistoricalRowsDelta::default(),
        previous_state_id: Some(4),
        records: Default::default(),
        topology: Default::default(),
    });
    assert_eq!(
        inconsistent.check_transitions(),
        Err(crate::history_records::AsmTransitionError::MissingTransition)
    );
}

use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCurveAxis, AsmHistoricalOptionalCarrierBinding,
    AsmHistoricalSurfaceRadius,
};

#[test]
fn circular_pattern_face_uses_unique_rigid_surface_radius() {
    let preceding = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 102,
            },
        ],
        surface_radii: vec![
            AsmHistoricalSurfaceRadius {
                surface: 101,
                radius: 2.5,
            },
            AsmHistoricalSurfaceRadius {
                surface: 102,
                radius: 4.0,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 21,
                carrier: 201,
            },
            AsmHistoricalCarrierBinding {
                entity: 22,
                carrier: 202,
            },
        ],
        surface_radii: vec![
            AsmHistoricalSurfaceRadius {
                surface: 201,
                radius: 2.5,
            },
            AsmHistoricalSurfaceRadius {
                surface: 202,
                radius: 2.5,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let candidates = [
        cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(21)).expect("identity grammar"),
        cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(22)).expect("identity grammar"),
    ];
    assert_eq!(
        resolve_pattern_face_by_surface_radius(
            &candidates,
            &preceding,
            &result,
            &HashSet::from([11, 12]),
        ),
        Some(11)
    );

    let mut ambiguous = preceding;
    ambiguous.surface_radii[1].radius = 2.5;
    assert_eq!(
        resolve_pattern_face_by_surface_radius(
            &candidates,
            &ambiguous,
            &result,
            &HashSet::from([11, 12]),
        ),
        None
    );
}

#[test]
fn historical_edge_axis_uses_the_state_specific_curve_carrier() {
    let topology = AsmHistoricalTopology {
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 7,
            carrier: Some(27),
        }],
        curve_axes: vec![AsmHistoricalCurveAxis {
            curve: 27,
            origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        historical_edge_axis(7, &topology),
        Some((
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        ))
    );
    assert_eq!(historical_edge_axis(8, &topology), None);
}

#[test]
fn historical_edge_axis_uses_a_unique_incident_surface_axis() {
    let surface_axis =
        |surface, origin, direction| crate::history_records::AsmHistoricalSurfaceAxis {
            surface,
            origin,
            direction,
        };
    let mut topology = AsmHistoricalTopology {
        face_loops: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![21],
        }],
        loop_coedges: vec![crate::history_records::AsmHistoricalRelation {
            owner_ref: 21,
            member_refs: vec![31],
        }],
        coedge_topology: vec![crate::history_records::AsmHistoricalCoedge {
            coedge: 31,
            owner_loop: 21,
            edge: 7,
            next: 31,
            previous: 31,
            radial_next: 31,
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 11,
            carrier: 41,
        }],
        surface_axes: vec![surface_axis(
            41,
            cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        )],
        ..AsmHistoricalTopology::default()
    };
    let expected = Some((
        cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
    ));
    assert_eq!(historical_edge_axis(7, &topology), expected);

    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 11,
        carrier: 42,
    });
    topology.surface_axes.push(surface_axis(
        42,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    ));
    assert_eq!(historical_edge_axis(7, &topology), None);

    topology.face_surfaces.pop();
    topology
        .face_loops
        .push(crate::history_records::AsmHistoricalRelation {
            owner_ref: 12,
            member_refs: vec![22],
        });
    topology
        .loop_coedges
        .push(crate::history_records::AsmHistoricalRelation {
            owner_ref: 22,
            member_refs: vec![32],
        });
    topology
        .coedge_topology
        .push(crate::history_records::AsmHistoricalCoedge {
            coedge: 32,
            owner_loop: 22,
            edge: 7,
            next: 32,
            previous: 32,
            radial_next: 32,
        });
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 12,
        carrier: 42,
    });
    assert_eq!(historical_edge_axis(7, &topology), None);
}

#[test]
fn historical_pattern_face_axis_uses_one_analytic_surface_carrier() {
    let raw_axes = |axes: Vec<crate::records::feature::patterns::DesignAxis>| {
        axes.into_iter()
            .map(|axis| (axis.origin.get(), *axis.direction.as_raw()))
            .collect::<Vec<_>>()
    };
    let origin = cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0);
    let topology = AsmHistoricalTopology {
        faces: vec![11],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 11,
            carrier: 41,
        }],
        surface_axes: vec![crate::history_records::AsmHistoricalSurfaceAxis {
            surface: 41,
            origin,
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![(
            AsmDeltaState {
                id: "state".into(),
                parent: "history".into(),
                byte_offset: 0,
                state_id: 1,
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
            Some(topology.clone()),
            Vec::new(),
        )],
    );
    assert_eq!(
        raw_axes({
            historical_pattern_identity_axes_for_selection(
                Some((AsmHistoricalEntityKind::Face, 11, &[1])),
                &history,
            )
        }),
        vec![(origin, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))]
    );

    let mut planar_history = history.clone();
    crate::test_support::history::edit(&mut planar_history, 0, |topology, _| {
        topology.surface_axes.clear();
        topology.surface_planes = vec![crate::history_records::AsmHistoricalPlane {
            surface: 41,
            origin,
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
        }];
    });
    assert_eq!(
        raw_axes({
            historical_pattern_identity_axes_for_selection(
                Some((AsmHistoricalEntityKind::Face, 11, &[1])),
                &planar_history,
            )
        }),
        vec![(origin, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))]
    );

    let mut ambiguous_history = history.clone();
    ambiguous_history.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .rows
        .face_surfaces
        .set
        .push(AsmHistoricalCarrierBinding {
            entity: 11,
            carrier: 42,
        });
    assert_eq!(
        ambiguous_history.check_transitions(),
        Err(crate::history_records::AsmTransitionError::UnsortedFamily)
    );
    assert!({
        historical_pattern_identity_axes_for_selection(
            Some((AsmHistoricalEntityKind::Face, 11, &[1])),
            &ambiguous_history,
        )
    }
    .is_empty());

    let missing_carrier = crate::test_support::history::history(
        history.id.clone(),
        history.byte_offset,
        history.preamble,
        false,
        vec![
            (
                history.states()[0].clone(),
                history.topology(&history.states()[0]).cloned(),
                history.entity_versions(&history.states()[0]),
            ),
            (
                AsmDeltaState {
                    state_id: 2,
                    node_index: 1,
                    ..history.states()[0].clone()
                },
                Some(AsmHistoricalTopology {
                    faces: vec![11],
                    ..Default::default()
                }),
                Vec::new(),
            ),
        ],
    );
    assert!({
        historical_pattern_identity_axes_for_selection(
            Some((AsmHistoricalEntityKind::Face, 11, &[1, 2])),
            &missing_carrier,
        )
    }
    .is_empty());
    let identities = HistoricalIdentityIndex::build(std::slice::from_ref(&missing_carrier), [11]);
    assert_eq!(
        raw_axes(historical_pattern_identity_axes(
            11,
            &identities,
            &missing_carrier,
            Some(1)
        )),
        vec![(origin, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))]
    );
    assert!(
        { historical_pattern_identity_axes(11, &identities, &missing_carrier, None) }.is_empty()
    );
}

#[test]
fn snapshot_edge_identity_requires_one_edge_record_and_positive_revision() {
    let record = |index, name: &str, revision_id| AsmHistoryRecord {
        id: format!("record-{index}-{name}"),
        parent: "state".into(),
        revision_id,
        byte_offset: 0,
        framing: crate::history_records::AsmHistoryRecordFraming::Framed {
            index,
            name: name.into(),
            entity_references: Vec::new(),
        },
        raw_bytes: Vec::new().into(),
    };
    let history = |records| {
        crate::test_support::history::history(
            "history".into(),
            0,
            None,
            false,
            vec![(
                AsmDeltaState {
                    id: "state".into(),
                    parent: "history".into(),
                    byte_offset: 0,
                    state_id: 1,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: 0,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records,
                    transition: None,
                },
                None,
                Vec::new(),
            )],
        )
    };

    assert_eq!(
        snapshot_edge_identity_revision(3, &history(vec![record(3, "edge", Some(17))])),
        Some(17)
    );
    assert_eq!(
        snapshot_edge_identity_revision(3, &history(vec![record(3, "face", Some(17))])),
        None
    );
    assert_eq!(
        snapshot_edge_identity_revision(
            3,
            &history(vec![
                record(3, "edge", Some(17)),
                record(3, "face", Some(18))
            ]),
        ),
        None
    );
    assert_eq!(
        snapshot_edge_identity_revision(3, &history(vec![record(3, "edge", Some(0))])),
        None
    );
}

#[test]
fn historical_identity_edge_requires_unique_incidence() {
    let mut topology = AsmHistoricalTopology {
        edges: vec![7, 8],
        coedges: vec![17, 18],
        curves: vec![27],
        pcurves: vec![37],
        coedge_topology: vec![
            crate::history_records::AsmHistoricalCoedge {
                coedge: 17,
                owner_loop: 0,
                edge: 7,
                next: 18,
                previous: 18,
                radial_next: 17,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 18,
                owner_loop: 0,
                edge: 8,
                next: 17,
                previous: 17,
                radial_next: 18,
            },
        ],
        edge_curves: vec![
            crate::history_records::AsmHistoricalOptionalCarrierBinding {
                entity: 7,
                carrier: Some(27),
            },
        ],
        coedge_pcurves: vec![
            crate::history_records::AsmHistoricalOptionalCarrierBinding {
                entity: 17,
                carrier: Some(37),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        historical_identity_edge(AsmHistoricalEntityKind::Coedge, 17, &topology),
        Some(7)
    );
    assert_eq!(
        historical_identity_edge(AsmHistoricalEntityKind::Curve, 27, &topology),
        Some(7)
    );
    assert_eq!(
        historical_identity_edge(AsmHistoricalEntityKind::Pcurve, 37, &topology),
        Some(7)
    );
    topology.edge_curves.push(
        crate::history_records::AsmHistoricalOptionalCarrierBinding {
            entity: 8,
            carrier: Some(27),
        },
    );
    assert_eq!(
        historical_identity_edge(AsmHistoricalEntityKind::Curve, 27, &topology),
        None
    );
}

#[test]
fn terminal_edge_recipe_faces_use_exact_then_alternate_references() {
    use cadmpeg_ir::ids::FaceId;

    let reference = |candidate_faces, alternate_selector_faces| {
        crate::records::dimensions::DesignRecipeReference {
            selector: 1,
            selector_offset: 0,
            token: "1".into(),
            token_offset: 0,
            design_reference: 1,
            design_reference_offset: 0,
            candidate_faces,
            candidate_edges: Vec::new(),
            alternate_selector_faces,
            alternate_selector_edges: Vec::new(),
        }
    };
    assert_eq!(
        terminal_edge_recipe_reference_faces(
            &[
                reference(
                    vec![FaceId::mint("test:model:face#face-c").expect("identity grammar")],
                    vec![FaceId::mint("test:model:face#ignored").expect("identity grammar")],
                ),
                reference(
                    Vec::new(),
                    vec![FaceId::mint("test:model:face#face-d").expect("identity grammar")]
                ),
                reference(
                    vec![FaceId::mint("test:model:face#face-a").expect("identity grammar")],
                    Vec::new()
                ),
            ],
            None
        )
        .unwrap(),
        vec![
            vec![FaceId::mint("test:model:face#face-c").expect("identity grammar")],
            vec![FaceId::mint("test:model:face#face-d").expect("identity grammar")],
            vec![FaceId::mint("test:model:face#face-a").expect("identity grammar")],
        ]
    );
    let reference_faces = terminal_edge_recipe_reference_faces(
        &[
            reference(
                vec![FaceId::mint("test:model:face#face-c").expect("identity grammar")],
                vec![FaceId::mint("test:model:face#ignored").expect("identity grammar")],
            ),
            reference(
                Vec::new(),
                vec![FaceId::mint("test:model:face#face-d").expect("identity grammar")],
            ),
            reference(
                vec![FaceId::mint("test:model:face#face-e").expect("identity grammar")],
                Vec::new(),
            ),
        ],
        Some(&[std::num::NonZeroU32::new(2).unwrap()]),
    )
    .unwrap();
    assert_eq!(
        reference_faces,
        vec![vec![
            FaceId::mint("test:model:face#face-d").expect("identity grammar")
        ]]
    );
    assert_eq!(
        terminal_edge_recipe_faces(
            &[
                FaceId::mint("test:model:face#face-b").expect("identity grammar"),
                FaceId::mint("test:model:face#face-a").expect("identity grammar")
            ],
            &reference_faces
        ),
        vec![
            FaceId::mint("test:model:face#face-a").expect("identity grammar"),
            FaceId::mint("test:model:face#face-b").expect("identity grammar"),
            FaceId::mint("test:model:face#face-d").expect("identity grammar"),
        ]
    );
}

#[test]
fn bound_state_pair_keeps_repeated_numeric_ids_in_one_history() {
    let state = |parent: &str, state_id: i64, previous_state_id: Option<i64>| {
        (
            AsmDeltaState {
                id: format!("{parent}:state-{state_id}"),
                parent: parent.into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: previous_state_id,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            None,
            Vec::new(),
        )
    };
    let history = |id: &str| {
        crate::test_support::history::history(
            id.into(),
            0,
            None,
            false,
            vec![
                state(id, 11, Some(10)),
                state(id, 10, Some(9)),
                state(id, 9, None),
            ],
        )
    };
    let histories = [history("history-a"), history("history-b")];
    let bindings = HashMap::from([("scope".into(), "history-b".into())]);

    let (selected, state, previous) =
        bound_history_state_pair("scope", 11, 9, &bindings, &histories)
            .expect("scope-bound repeated state pair");
    assert_eq!(selected.id, "history-b");
    assert_eq!(state.parent, "history-b");
    assert_eq!(previous.parent, "history-b");
}

#[test]
fn boundary_edge_change_partition_preserves_boundary_order() {
    assert_eq!(boundary_edges_in_changes(&[8, 3, 5, 2], &[2, 8]), [8, 2]);
    assert!(boundary_edges_in_changes(&[8, 3], &[1, 2]).is_empty());
}

#[test]
fn result_face_support_maps_only_to_one_preceding_owner() {
    use cadmpeg_ir::ids::FaceId;

    let result_faces = [FaceId::mint("f3d:brep:entity#40").expect("identity grammar")];
    let result = AsmHistoricalTopology {
        faces: vec![40],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 20,
        }],
        ..AsmHistoricalTopology::default()
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![4, 5],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 4,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 5,
                carrier: 21,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        preceding_support_face_slots(&result_faces, &result, &preceding),
        [4]
    );

    let mut ambiguous = preceding.clone();
    ambiguous.face_surfaces[1].carrier = 20;
    assert!(preceding_support_face_slots(&result_faces, &result, &ambiguous).is_empty());

    let mut ambiguous_result = result.clone();
    ambiguous_result
        .face_surfaces
        .push(AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 21,
        });
    assert!(preceding_support_face_slots(&result_faces, &ambiguous_result, &preceding).is_empty());
}

#[test]
fn active_face_support_retains_invariant_preceding_owners() {
    use cadmpeg_ir::ids::FaceId;

    let state = |state_id, topology| {
        (
            AsmDeltaState {
                id: format!("state-{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology),
            Vec::new(),
        )
    };
    let active = AsmHistoricalTopology {
        faces: vec![40],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 20,
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(2, active.clone()), state(3, active)],
    );
    let preceding = AsmHistoricalTopology {
        faces: vec![4, 5],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 4,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 5,
                carrier: 20,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let changed_faces = HashSet::from([5]);
    assert_eq!(
        historical_face_support_contexts(
            &[FaceId::mint("f3d:brep:entity#40").expect("identity grammar")],
            &history,
            &preceding,
            &changed_faces,
            &mut crate::history::topology_cache::TopologyQueryCache::default()
        ),
        [
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot: 40,
                surface_slot: 20,
                preceding_face_slots: vec![4, 5],
                preceding_face_boundaries: Vec::new(),
                changed_preceding_face_slots: vec![5],
            }
        ]
    );

    let mut variant = history;
    crate::test_support::history::edit(&mut variant, 1, |topology, _| {
        topology.face_surfaces[0].carrier = 21;
    });
    assert_eq!(
        historical_face_support_contexts(
            &[FaceId::mint("f3d:brep:entity#4").expect("identity grammar")],
            &variant,
            &preceding,
            &changed_faces,
            &mut crate::history::topology_cache::TopologyQueryCache::default()
        ),
        [
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot: 4,
                surface_slot: 20,
                preceding_face_slots: vec![4, 5],
                preceding_face_boundaries: Vec::new(),
                changed_preceding_face_slots: vec![5],
            }
        ]
    );
    assert!({
        historical_face_support_contexts(
            &[FaceId::mint("f3d:brep:entity#40").expect("identity grammar")],
            &variant,
            &preceding,
            &changed_faces,
            &mut crate::history::topology_cache::TopologyQueryCache::default(),
        )
    }
    .is_empty());
}

#[test]
fn topology_changes_span_only_complete_acyclic_state_chains() {
    let state = |id, faces: &[i64], edges: &[i64], versions: &[(i64, i64)]| {
        (
            change_state(id),
            Some(AsmHistoricalTopology {
                faces: faces.to_vec(),
                edges: edges.to_vec(),
                ..Default::default()
            }),
            versions
                .iter()
                .map(
                    |&(entity_ref, record_ref)| crate::history_records::AsmEntityVersion {
                        entity_ref,
                        record_ref,
                    },
                )
                .collect(),
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            state(3, &[10], &[20], &[(10, 110), (20, 220)]),
            state(
                2,
                &[10, 11],
                &[20, 21],
                &[(10, 110), (11, 11), (20, 220), (21, 21)],
            ),
            state(
                1,
                &[10, 11],
                &[20, 21],
                &[(10, 10), (11, 11), (20, 20), (21, 21)],
            ),
        ],
    );
    let result = &history.states()[0];
    let states = history
        .states()
        .iter()
        .map(|s| (s.state_id, Some(s)))
        .collect();
    assert_eq!(
        face_changes_across_state_chain(result, 1, &states),
        Some(HashSet::from([10, 11]))
    );
    let incomplete = HashMap::from([(1, Some(&history.states()[2])), (3, Some(result))]);
    assert_eq!(
        face_changes_across_state_chain(result, 1, &incomplete),
        None
    );
    assert_eq!(
        edge_changes_across_state_chain(result, 1, &states)
            .map(|changes| (changes.deleted, changes.updated)),
        Some((HashSet::from([21]), HashSet::from([20])))
    );
    assert_eq!(
        edge_changes_across_state_chain(result, 1, &incomplete),
        None
    );
    let mut cyclic_history = history.clone();
    cyclic_history.states_mut()[1]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(3);
    let cyclic = cyclic_history
        .states()
        .iter()
        .map(|s| (s.state_id, Some(s)))
        .collect();
    let result = &cyclic_history.states()[0];
    assert_eq!(face_changes_across_state_chain(result, 1, &cyclic), None);
    assert_eq!(edge_changes_across_state_chain(result, 1, &cyclic), None);
    assert_eq!(
        cyclic_history.check_transitions(),
        Err(crate::history_records::AsmTransitionError::PreviousState)
    );
}

#[test]
fn historical_topology_retains_ordered_ownership_and_incidence() {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Region, Sense, Shell, Vertex,
    };

    let id = |slot| format!("f3d:brep:entity#{slot}");
    let mut brep = cadmpeg_asm::brep::AsmBrep::default();
    brep.bodies.push(Body {
        id: BodyId::mint(id(1)).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint(id(2)).expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    brep.regions.push(Region {
        id: RegionId::mint(id(2)).expect("identity grammar"),
        body: BodyId::mint(id(1)).expect("identity grammar"),
        shells: vec![ShellId::mint(id(3)).expect("identity grammar")],
    });
    brep.shells.push(Shell::with_face(
        ShellId::mint(id(3)).expect("identity grammar"),
        RegionId::mint(id(2)).expect("identity grammar"),
        FaceId::mint(id(4)).expect("identity grammar"),
    ));
    brep.faces.push(Face {
        id: FaceId::mint(id(4)).expect("identity grammar"),
        shell: ShellId::mint(id(3)).expect("identity grammar"),
        surface: SurfaceId::mint(id(20)).expect("identity grammar"),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![
            LoopId::mint(id(5)).expect("identity grammar")
        ]),
        name: None,
        color: None,
        tolerance: None,
    });
    brep.loops.push(Loop {
        id: LoopId::mint(id(5)).expect("identity grammar"),
        face: FaceId::mint(id(4)).expect("identity grammar"),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(
                vec![CoedgeId::mint(id(6)).expect("identity grammar")],
                Vec::new(),
            )
            .expect("fixture ring admission"),
        ),
    });
    brep.coedges.push(Coedge {
        id: CoedgeId::mint(id(6)).expect("identity grammar"),
        owner_loop: LoopId::mint(id(5)).expect("identity grammar"),
        edge: EdgeId::mint(id(7)).expect("identity grammar"),
        radial_next: CoedgeId::mint(id(6)).expect("identity grammar"),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    brep.edges.push(Edge {
        id: EdgeId::mint(id(7)).expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(
            CurveId::mint(id(21)).expect("identity grammar"),
        )),
        start: VertexId::mint(id(8)).expect("identity grammar"),
        end: VertexId::mint(id(9)).expect("identity grammar"),
        tolerance: None,
    });
    for slot in [8, 9] {
        brep.vertices.push(Vertex {
            id: VertexId::mint(id(slot)).expect("identity grammar"),
            point: PointId::mint(id(slot + 20)).expect("identity grammar"),
            tolerance: None,
        });
    }

    let topology = historical_topology(&brep)
        .expect("topology budget")
        .expect("stable historical topology");
    assert_eq!(topology.body_regions[0].member_refs, [2]);
    assert_eq!(topology.region_shells[0].member_refs, [3]);
    assert_eq!(topology.shell_faces[0].member_refs, [4]);
    assert_eq!(topology.face_loops[0].member_refs, [5]);
    assert_eq!(topology.loop_coedges[0].member_refs, [6]);
    assert_eq!(topology.coedge_topology[0].edge, 7);
    assert_eq!(topology.coedge_topology[0].radial_next, 6);
    assert_eq!(
        historical_edge_context(7, &topology),
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: 7,
            incident_loops: vec![
                crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                    coedge_slot: 6,
                    loop_slot: 5,
                    face_slot: 4,
                    boundary_edge_count: 1,
                    coedge_ordinal: 0,
                    previous_edge_slot: 7,
                    next_edge_slot: 7,
                }
            ],
        }
    );
    let entry = |selector, boundary_edge_count| {
        crate::records::topology::edge_recipe::DesignTopologyRecipeEntry {
            selector,
            boundary_edge_count: std::num::NonZeroU32::new(boundary_edge_count).unwrap(),
            topology_triplets: [
                crate::records::topology::edge_recipe::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 0,
                    incident: Some(crate::records::topology::edge_recipe::DesignTopologyIncident {
                        ordinal: boundary_edge_count - 1,
                        side: crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding,
                    }),
                },
                crate::records::topology::edge_recipe::DesignTopologyRecipeTriplet {
                    outer: std::num::NonZeroU32::new(1).unwrap(),
                    middle: 1,
                    incident: Some(crate::records::topology::edge_recipe::DesignTopologyIncident {
                        ordinal: 0,
                        side: crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Following,
                    }),
                },
            ],
        }
    };
    let side = |entries: Vec<crate::records::topology::edge_recipe::DesignTopologyRecipeEntry>| {
        crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
            header_value: 0,
            scalars: vec![0, 0],
            payload_prefix: vec![0],

            entries,
        }
    };
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![
            side(vec![entry(1, 1), entry(2, 1)]),
            side(vec![entry(1, 2)]),
        ],
    };
    let loop_context = |coedge_slot, boundary_edge_count| {
        crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
            coedge_slot,
            loop_slot: coedge_slot + 10,
            face_slot: coedge_slot + 20,
            boundary_edge_count,
            coedge_ordinal: 0,
            previous_edge_slot: coedge_slot + 30,
            next_edge_slot: coedge_slot + 40,
        }
    };
    let contexts = [
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: 7,
            incident_loops: vec![loop_context(70, 1)],
        },
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: 8,
            incident_loops: vec![
                loop_context(80, 1),
                loop_context(81, 2),
                crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                    coedge_ordinal: 1,
                    ..loop_context(82, 2)
                },
            ],
        },
    ];
    let selectors = recipe_selector_candidates(Some(&structure), &contexts);
    assert_eq!(selectors.len(), 2);
    assert_eq!(selectors[0].selector, 1);
    assert_eq!(selectors[0].boundary_count_matching_edge_slots, [8]);
    assert_eq!(
        selectors[0]
            .clauses
            .iter()
            .map(|clause| clause
                .as_ref()
                .map(|clause| clause.triplet_edge_slots.clone()))
            .collect::<Vec<_>>(),
        [Some([vec![7, 8], vec![7, 8]]), Some([vec![8], vec![8]])]
    );
    assert_eq!(selectors[0].incidence_matching_edge_slots, [8]);
    assert_eq!(selectors[0].unique_incidence_edge_slot(), Some(8));
    assert_eq!(selectors[1].selector, 2);
    assert_eq!(selectors[1].boundary_count_matching_edge_slots, [7, 8]);
    assert_eq!(selectors[1].incidence_matching_edge_slots, [7, 8]);
    assert_eq!(selectors[1].unique_incidence_edge_slot(), None);
    assert_eq!(
        selectors[1]
            .clauses
            .iter()
            .map(|clause| clause
                .as_ref()
                .map(|clause| clause.triplet_edge_slots.clone()))
            .collect::<Vec<_>>(),
        [Some([vec![7, 8], vec![7, 8]]), None]
    );
    assert!(incident_loop_counts_satisfy_sides(
        &[4, 5],
        &[Some(5), Some(4)]
    ));
    assert!(!incident_loop_counts_satisfy_sides(
        &[5, 6],
        &[Some(5), Some(5)]
    ));
    assert!(incident_loop_counts_satisfy_sides(
        &[5, 5],
        &[Some(5), Some(5)]
    ));
    assert!(incident_loop_counts_satisfy_sides(&[5], &[None, Some(5)]));
    assert_eq!(topology.edge_vertices[0].start_vertex, 8);
    assert_eq!(topology.edge_vertices[0].end_vertex, 9);
    assert_eq!(topology.face_surfaces[0].carrier, 20);
    assert_eq!(topology.edge_curves[0].carrier, Some(21));
    assert_eq!(topology.coedge_pcurves[0].carrier, None);
    assert_eq!(topology.vertex_points[0].carrier, 28);
    assert_eq!(
        bodies_intersecting(&topology, &BTreeSet::from([20])).unwrap(),
        BTreeSet::from([1])
    );
    assert_eq!(
        bodies_intersecting(&topology, &BTreeSet::from([28])).unwrap(),
        BTreeSet::from([1])
    );
    assert_eq!(
        faces_in_topology(
            &[
                FaceId::mint(id(4)).expect("identity grammar"),
                FaceId::mint(id(99)).expect("identity grammar"),
                FaceId::mint("test:model:face#foreign").expect("identity grammar")
            ],
            &topology
        ),
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    let mut reference = crate::records::dimensions::DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "1".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 1,
        candidate_faces: vec![FaceId::mint(id(4)).expect("identity grammar")],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let boundary_state = |state_id: i64, byte_offset: u64| AsmDeltaState {
        id: format!("f3d:test:asm-delta-state#{byte_offset:010}"),
        byte_offset,
        ..change_state(state_id)
    };
    let result_state = boundary_state(2, 20);
    let preceding_state = boundary_state(1, 10);
    let mut face_boundaries = FaceBoundaryArena::default();
    let context = edge_recipe_reference_context(
        2,
        &reference,
        EdgeBoundaryContext {
            state: &result_state,
            topology: &topology,
            boundary_edges: &[7, 99],
        },
        EdgeBoundaryContext {
            state: &preceding_state,
            topology: &topology,
            boundary_edges: &[7, 98],
        },
        &HashSet::from([7]),
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
        &mut face_boundaries,
    );
    assert_eq!(context.reference_ordinal, 2);
    assert_eq!(
        context.result_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    let boundary = crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
        face_slot: 4,
        loops: vec![crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
            loop_slot: 5,
            boundary: crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(vec![
                crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                    coedge_slot: 6,
                    edge_slot: 7,
                },
            ]),
        }],
    };
    let face_boundaries = face_boundaries.into_records();
    let stored = |state: &AsmDeltaState| AsmFaceBoundary {
        id: AsmFaceBoundary::identity(&state.id, state.byte_offset, 4).unwrap(),
        parent: state.id.clone(),
        face_slot: boundary.face_slot,
        loops: boundary.loops.clone(),
    };
    assert_eq!(
        face_boundaries,
        [stored(&preceding_state), stored(&result_state)]
    );
    assert_eq!(context.result_face_boundary_ids, [stored(&result_state).id]);
    assert_eq!(context.result_shared_edge_slots, [7]);
    assert_eq!(
        context.preceding_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    assert_eq!(
        context.preceding_face_boundary_ids,
        [stored(&preceding_state).id]
    );
    assert_eq!(context.preceding_support_face_slots, [4]);
    assert_eq!(
        context.preceding_support_face_boundary_ids,
        [stored(&preceding_state).id]
    );
    assert_eq!(context.shared_edge_slots, [7]);
    assert_eq!(context.changed_shared_edge_slots, [7]);
    assert_eq!(context.changed_reference_edge_slots, [7]);
    reference.candidate_faces.clear();
    reference.alternate_selector_faces = vec![FaceId::mint(id(4)).expect("identity grammar")];
    let alternate_context = edge_recipe_reference_context(
        2,
        &reference,
        EdgeBoundaryContext {
            state: &result_state,
            topology: &topology,
            boundary_edges: &[7, 99],
        },
        EdgeBoundaryContext {
            state: &preceding_state,
            topology: &topology,
            boundary_edges: &[7, 98],
        },
        &HashSet::from([7]),
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
        &mut FaceBoundaryArena::default(),
    );
    assert_eq!(
        alternate_context.result_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    assert_eq!(
        alternate_context.preceding_faces,
        [FaceId::mint(id(4)).expect("identity grammar")]
    );
    assert_eq!(alternate_context.changed_reference_edge_slots, [7]);
    let support_only_context = edge_recipe_reference_context(
        2,
        &reference,
        EdgeBoundaryContext {
            state: &result_state,
            topology: &topology,
            boundary_edges: &[99],
        },
        EdgeBoundaryContext {
            state: &preceding_state,
            topology: &topology,
            boundary_edges: &[98],
        },
        &HashSet::from([7]),
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
        &mut FaceBoundaryArena::default(),
    );
    assert!(support_only_context.shared_edge_slots.is_empty());
    assert_eq!(support_only_context.changed_reference_edge_slots, [7]);
    let cyclic = AsmHistoricalTopology {
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 7,
                start_vertex: 1,
                end_vertex: 2,
            },
            AsmHistoricalEdge {
                edge: 8,
                start_vertex: 3,
                end_vertex: 2,
            },
            AsmHistoricalEdge {
                edge: 9,
                start_vertex: 1,
                end_vertex: 3,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let ordered_vertices = |edges: &[i64], topology: &AsmHistoricalTopology| {
        let coedges = edges
            .iter()
            .enumerate()
            .map(|(ordinal, edge_slot)| {
                crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                    coedge_slot: i64::try_from(ordinal).expect("fixture value fits i64"),
                    edge_slot: *edge_slot,
                }
            })
            .collect();
        match historical_loop_boundary(coedges, &crate::history::loop_index::LoopIndex::new(topology), &crate::history::topology_cache::endpoint_index(topology)) {
            crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Vertices(rows) => Some(
                rows.into_iter()
                    .map(|row| row.vertex_slot)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        }
    };
    assert_eq!(ordered_vertices(&[7, 8, 9], &cyclic), Some(vec![1, 2, 3]));
    let disconnected = AsmHistoricalTopology {
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 7,
                start_vertex: 1,
                end_vertex: 2,
            },
            AsmHistoricalEdge {
                edge: 8,
                start_vertex: 3,
                end_vertex: 4,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(ordered_vertices(&[7, 8], &disconnected), None);
}

#[test]
fn indexed_edge_context_reads_the_first_matching_rows() {
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let coedge = |coedge, owner_loop, edge| AsmHistoricalCoedge {
        coedge,
        owner_loop,
        edge,
        next: coedge,
        previous: coedge,
        radial_next: coedge,
    };
    let topology = AsmHistoricalTopology {
        face_loops: vec![relation(10, vec![100, 101]), relation(11, vec![100])],
        loop_coedges: vec![
            relation(100, vec![1, 2, 3]),
            relation(100, vec![3, 2, 1]),
            relation(101, vec![4]),
        ],
        coedge_topology: vec![
            coedge(1, 100, 7),
            coedge(2, 100, 8),
            coedge(3, 100, 9),
            coedge(4, 101, 7),
            coedge(2, 101, 9),
            coedge(5, 102, 7),
        ],
        ..AsmHistoricalTopology::default()
    };
    let index = EdgeContextIndex::new(&topology);
    assert_eq!(
        historical_edge_context(7, &topology).incident_loops.len(),
        2
    );
    for edge in [7, 8, 9, 99] {
        assert_eq!(
            index.context(edge),
            historical_edge_context(edge, &topology)
        );
    }
}

mod identity;

fn change_state(state_id: i64) -> crate::history_records::AsmDeltaState {
    AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        transition: None,
    }
}
