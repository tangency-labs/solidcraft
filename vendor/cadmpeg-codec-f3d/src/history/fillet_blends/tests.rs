// SPDX-License-Identifier: Apache-2.0

use super::{bind_fillet_groups, group_edges, transition_blends, Blend};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalCylinder, AsmHistoricalEdgeSample, AsmHistoricalEntityDelta,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalRelation, AsmHistoricalRowsDelta,
    AsmHistoricalSurfaceRadius, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#42";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#43";

fn z() -> Vector3 {
    Vector3::new(0.0, 0.0, 1.0)
}

fn y() -> Vector3 {
    Vector3::new(0.0, 1.0, 0.0)
}

/// Faces bounded by one loop each, with one coedge per boundary edge.
fn faces(boundaries: &[(i64, &[i64])]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology::default();
    for &(face, edges) in boundaries {
        let loop_slot = 1_000 + face;
        topology.faces.push(face);
        topology.surfaces.push(100 + face);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: face,
            carrier: 100 + face,
        });
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_slot],
        });
        let coedges = edges
            .iter()
            .map(|edge| 10_000 * face + edge)
            .collect::<Vec<_>>();
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_slot,
            member_refs: coedges.clone(),
        });
        for (coedge, edge) in coedges.into_iter().zip(edges) {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge,
                owner_loop: loop_slot,
                edge: *edge,
                next: coedge,
                previous: coedge,
                radial_next: coedge,
            });
        }
    }
    topology
}

/// A straight edge with its axis row and midpoint tangent along `direction`.
fn line(topology: &mut AsmHistoricalTopology, edge: i64, direction: Vector3) {
    topology.edges.push(edge);
    topology
        .edge_curves
        .push(AsmHistoricalOptionalCarrierBinding {
            entity: edge,
            carrier: Some(100 + edge),
        });
    topology.curve_axes.push(AsmHistoricalCurveAxis {
        curve: 100 + edge,
        origin: Point3::new(0.0, 0.0, 0.0),
        direction,
    });
    topology.edge_samples.push(AsmHistoricalEdgeSample {
        edge,
        interior: Point3::new(0.0, 0.0, 0.0),
        tangent: Some(direction),
    });
}

/// Faces 10 and 11 meet at edge 17 along z; face 12 caps both at edge 18
/// along y and edge 19 along y.
fn input(edge_17: Vector3) -> AsmHistoricalTopology {
    let mut topology = faces(&[(10, &[17, 18]), (11, &[17, 19]), (12, &[18, 19])]);
    line(&mut topology, 17, edge_17);
    line(&mut topology, 18, y());
    line(&mut topology, 19, y());
    topology
}

/// The fillet of edge 17: blend face 20 meets faces 10, 11, and 12.
fn output() -> AsmHistoricalTopology {
    let mut topology = faces(&[(10, &[30]), (11, &[31]), (12, &[32]), (20, &[30, 31, 32])]);
    topology.surface_cylinders.push(AsmHistoricalCylinder {
        surface: 120,
        origin: Point3::new(3.0, 3.0, 0.0),
        axis: z(),
        radius: 3.0,
    });
    topology.surface_radii.push(AsmHistoricalSurfaceRadius {
        surface: 120,
        radius: 3.0,
    });
    topology
}

/// The fillet of edges 17 and 18: blend face 21 along y meets faces 10 and 12.
fn two_blends() -> AsmHistoricalTopology {
    let mut topology = faces(&[
        (10, &[30, 33]),
        (11, &[31]),
        (12, &[32, 34]),
        (20, &[30, 31, 32]),
        (21, &[33, 34]),
    ]);
    for (surface, axis) in [(120, z()), (121, y())] {
        topology.surface_cylinders.push(AsmHistoricalCylinder {
            surface,
            origin: Point3::new(3.0, 3.0, 0.0),
            axis,
            radius: 3.0,
        });
        topology.surface_radii.push(AsmHistoricalSurfaceRadius {
            surface,
            radius: 3.0,
        });
    }
    topology
}

fn transition(inserted: &[i64], deleted_edges: &[i64]) -> AsmHistoricalTransition {
    AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                inserted: inserted.to_vec(),
                ..AsmHistoricalEntityDelta::default()
            },
            edges: AsmHistoricalEntityDelta {
                deleted: deleted_edges.to_vec(),
                ..AsmHistoricalEntityDelta::default()
            },
            ..AsmHistoricalTopologyDelta::default()
        },
        record_revisions: Vec::new(),
        rows: AsmHistoricalRowsDelta::default(),
    }
}

fn blends(input: &AsmHistoricalTopology, output: &AsmHistoricalTopology) -> Option<Vec<Blend>> {
    transition_blends(&transition(&[20], &[17, 18, 19]), input, output)
}

#[test]
fn a_blend_replaces_the_deleted_edge_along_its_spine() {
    assert_eq!(
        blends(&input(z()), &output()),
        Some(vec![Blend {
            radius: 3.0,
            edge: 17
        }])
    );
}

#[test]
fn a_blend_with_no_deleted_edge_along_its_spine_states_nothing() {
    assert_eq!(blends(&input(y()), &output()), None);
    assert_eq!(
        transition_blends(&transition(&[20], &[18, 19]), &input(z()), &output()),
        None
    );
}

#[test]
fn a_blend_with_two_deleted_edges_along_its_spine_states_nothing() {
    let mut two = input(z());
    two.edge_samples.retain(|sample| sample.edge != 18);
    two.curve_axes.retain(|axis| axis.curve != 118);
    two.edge_samples.push(AsmHistoricalEdgeSample {
        edge: 18,
        interior: Point3::new(0.0, 0.0, 0.0),
        tangent: Some(z()),
    });
    two.curve_axes.push(AsmHistoricalCurveAxis {
        curve: 118,
        origin: Point3::new(0.0, 0.0, 0.0),
        direction: z(),
    });
    assert_eq!(blends(&two, &output()), None);
}

#[test]
fn a_new_carrier_without_a_spine_or_a_radius_states_nothing() {
    let mut no_spine = output();
    no_spine.surface_cylinders.clear();
    assert_eq!(blends(&input(z()), &no_spine), None);
    let mut no_radius = output();
    no_radius.surface_radii.clear();
    assert_eq!(blends(&input(z()), &no_radius), None);
    let mut two_radii = output();
    two_radii.surface_radii.push(AsmHistoricalSurfaceRadius {
        surface: 120,
        radius: 4.0,
    });
    assert_eq!(blends(&input(z()), &two_radii), None);
}

#[test]
fn an_inserted_face_on_a_preceding_carrier_is_not_a_blend() {
    let mut output = output();
    output.faces.push(21);
    output.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 21,
        carrier: 110,
    });
    assert_eq!(
        transition_blends(&transition(&[20, 21], &[17, 18, 19]), &input(z()), &output),
        Some(vec![Blend {
            radius: 3.0,
            edge: 17
        }])
    );
}

#[test]
fn two_blends_replace_the_edges_along_their_spines() {
    assert_eq!(
        transition_blends(
            &transition(&[20, 21], &[17, 18, 19]),
            &input(z()),
            &two_blends()
        ),
        Some(vec![
            Blend {
                radius: 3.0,
                edge: 17
            },
            Blend {
                radius: 3.0,
                edge: 18
            }
        ])
    );
}

#[test]
fn an_inserted_face_without_one_carrier_states_nothing() {
    let mut output = two_blends();
    output.faces.push(22);
    output.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 22,
        carrier: 130,
    });
    output.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 22,
        carrier: 131,
    });
    assert_eq!(
        transition_blends(
            &transition(&[20, 21, 22], &[17, 18, 19]),
            &input(z()),
            &output
        ),
        None
    );
}

#[test]
fn two_blends_that_replace_one_edge_state_nothing() {
    let mut output = output();
    output.faces.push(21);
    output.surfaces.push(121);
    output.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 21,
        carrier: 121,
    });
    output.face_loops.push(AsmHistoricalRelation {
        owner_ref: 21,
        member_refs: vec![1_021],
    });
    output.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: 1_021,
        member_refs: vec![210_030, 210_031],
    });
    for (coedge, edge) in [(210_030, 30), (210_031, 31)] {
        output.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: 1_021,
            edge,
            next: coedge,
            previous: coedge,
            radial_next: coedge,
        });
    }
    output.surface_cylinders.push(AsmHistoricalCylinder {
        surface: 121,
        origin: Point3::new(3.0, 3.0, 0.0),
        axis: z(),
        radius: 3.0,
    });
    output.surface_radii.push(AsmHistoricalSurfaceRadius {
        surface: 121,
        radius: 3.0,
    });
    assert_eq!(
        transition_blends(&transition(&[20, 21], &[17, 18, 19]), &input(z()), &output),
        None
    );
}

#[test]
fn a_transition_without_a_new_carrier_states_nothing() {
    assert_eq!(
        transition_blends(&transition(&[], &[17]), &input(z()), &output()),
        None
    );
}

#[test]
fn each_blend_belongs_to_the_one_group_of_its_radius() {
    let blends = [
        Blend {
            radius: 3.0,
            edge: 9,
        },
        Blend {
            radius: 2.0,
            edge: 7,
        },
        Blend {
            radius: 3.000_000_000_000_000_4,
            edge: 8,
        },
    ];
    assert_eq!(
        group_edges(&[Some(3.0), Some(2.0), None], &blends),
        Some(vec![vec![8, 9], vec![7], Vec::new()])
    );
    assert_eq!(group_edges(&[Some(3.0), Some(3.0)], &blends[..1]), None);
    assert_eq!(group_edges(&[Some(2.0), None], &blends[..1]), None);
    assert_eq!(group_edges(&[Some(3.000_01)], &blends[..1]), None);
}

fn scope(record_index: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::Fillet, record_index);
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
        "class_tag": "325",
        "record_index": 43,
        "scope_record_index": scope_record_index,
        "scope_reference_ordinal": 0,
        "members": members,
        "member_offsets": member_offsets,
        "role": 34_359_738_368_u64,
        "role_offset": 0,
        "paired_byte_offset": 200,
        "paired_class_tag": "258",
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

fn fillet(edges: EdgeSelection, radius: RadiusSpec) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
            edges,
            radius,
            tangency_weight: None,
        }),
    })
}

fn three_mm() -> RadiusSpec {
    RadiusSpec::Constant {
        radius: cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
    }
}

fn bound(
    edges: EdgeSelection,
    radius: RadiusSpec,
    groups: &[DesignConstructionOperandGroup],
) -> EdgeSelection {
    let mut definition = fillet(edges, radius);
    bind_fillet_groups(
        &mut definition,
        &FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
        &scope(42),
        groups,
        1,
        (&transition(&[20], &[17, 18, 19]), &input(z()), &output()),
    );
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
        panic!("expected a fillet");
    };
    groups.into_iter().next().unwrap().edges
}

fn slots(selection: &EdgeSelection) -> Vec<String> {
    let EdgeSelection::Historical { edges, .. } = selection else {
        panic!("expected a historical selection, got {selection:?}");
    };
    edges
        .iter()
        .map(|edge| edge.as_str().rsplit(':').next().unwrap().to_owned())
        .collect()
}

fn native() -> EdgeSelection {
    EdgeSelection::Native(GROUP_ID.into())
}

#[test]
fn a_group_binds_to_the_edge_its_blend_replaces() {
    assert_eq!(
        slots(&bound(native(), three_mm(), &[group(42, &[7])])),
        ["17"]
    );
    assert_eq!(
        slots(&bound(native(), three_mm(), &[group(42, &[7, 7])])),
        ["17"],
        "a repeated member counts once"
    );
}

fn earlier(slots: &[i64]) -> EdgeSelection {
    let feature = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let prefix = crate::ids::feature_input_prefix(&feature, 1);
    EdgeSelection::historical(
        crate::ids::feature_input_topology_id(&feature, 1),
        slots
            .iter()
            .map(|slot| crate::ids::history_input_edge_id(&prefix, *slot))
            .collect(),
        GROUP_ID.into(),
    )
    .unwrap()
}

#[test]
fn a_bound_group_replaces_an_earlier_selection_of_deleted_edges() {
    assert_eq!(
        slots(&bound(earlier(&[18]), three_mm(), &[group(42, &[7])])),
        ["17"]
    );
}

#[test]
fn a_selection_that_names_a_kept_edge_stays() {
    for kept in [&[21][..], &[18, 21]] {
        assert_eq!(
            bound(earlier(kept), three_mm(), &[group(42, &[7])]),
            earlier(kept)
        );
    }
}

#[test]
fn a_group_with_another_member_count_keeps_its_selection() {
    assert_eq!(bound(native(), three_mm(), &[group(42, &[7, 8])]), native());
    let mut definition = fillet(native(), three_mm());
    bind_fillet_groups(
        &mut definition,
        &FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
        &scope(42),
        &[group(42, &[7])],
        1,
        (
            &transition(&[20, 21], &[17, 18, 19]),
            &input(z()),
            &two_blends(),
        ),
    );
    assert_eq!(
        definition,
        fillet(native(), three_mm()),
        "two blends round two edges"
    );
}

#[test]
fn a_group_of_another_scope_or_one_id_for_two_groups_keeps_its_selection() {
    assert_eq!(bound(native(), three_mm(), &[group(41, &[7])]), native());
    assert_eq!(
        bound(native(), three_mm(), &[group(42, &[7]), group(42, &[7])]),
        native()
    );
}

#[test]
fn a_group_without_a_constant_radius_keeps_its_selection() {
    let chordal = RadiusSpec::Chordal {
        chord_length: cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
    };
    assert_eq!(bound(native(), chordal, &[group(42, &[7])]), native());
}

#[test]
fn a_group_of_another_stream_keeps_its_selection() {
    const OTHER: &str = "f3d:Design/Other.dat:group#43";
    let mut other = group(42, &[7]);
    other = serde_json::from_value({
        let mut value = serde_json::to_value(&other).unwrap();
        value["id"] = OTHER.into();
        value
    })
    .unwrap();
    assert_eq!(
        bound(EdgeSelection::Native(OTHER.into()), three_mm(), &[other]),
        EdgeSelection::Native(OTHER.into())
    );
}

#[test]
fn a_blend_face_without_a_boundary_states_nothing() {
    let mut output = output();
    output
        .face_loops
        .retain(|relation| relation.owner_ref != 20);
    assert_eq!(blends(&input(z()), &output), None);
}

fn history() -> crate::history_records::AsmHistory {
    let state = |state_id: i64, topology: AsmHistoricalTopology, next_ref| {
        let versions = topology
            .faces
            .iter()
            .chain(&topology.edges)
            .map(|&entity_ref| crate::history_records::AsmEntityVersion {
                entity_ref,
                record_ref: 1,
            })
            .collect();
        (
            crate::history_records::AsmDeltaState {
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
    };
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(2, output(), Some(1)), state(1, input(z()), None)],
    )
}

fn bind_feature(native_ref: Option<&str>) -> EdgeSelection {
    let mut feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(fillet(
            native(),
            three_mm(),
        )),
        native_ref: native_ref.map(str::to_owned),
    };
    super::bind_fillet_blend_edges(
        std::slice::from_mut(&mut feature),
        super::FilletBlendInputs {
            scopes: &[scope(42)],
            groups: &[group(42, &[7])],
            histories: &mut [history()],
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        panic!("expected a fillet");
    };
    groups[0].edges.clone()
}

#[test]
fn a_fillet_binds_from_the_transition_of_its_scope() {
    assert_eq!(slots(&bind_feature(Some(SCOPE_ID))), ["17"]);
    assert_eq!(bind_feature(None), native());
}
