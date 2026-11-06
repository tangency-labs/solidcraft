// SPDX-License-Identifier: Apache-2.0

fn simple_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: Vec::new(),
        }],
        ..Default::default()
    }
}

fn wire_topology(free_vertex: bool) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, vec![7])],
        shell_free_vertices: vec![relation(3, if free_vertex { vec![8] } else { Vec::new() })],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 28,
            },
            AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 29,
            },
        ],
        ..Default::default()
    }
}

#[test]
fn repeated_coedge_incidence_visits_each_edge_and_vertex_once() {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalRelation};
    use std::collections::BTreeSet;

    let mut topology = wire_topology(false);
    topology.shell_wire_edges[0].member_refs.clear();
    topology.shell_faces[0].member_refs.push(4);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: 4,
        member_refs: vec![5],
    });
    topology
        .face_surfaces
        .push(crate::history_records::AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 6,
        });
    topology.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: 5,
        member_refs: (100..200).collect(),
    });
    for coedge in 100..200 {
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: 5,
            edge: 7,
            next: if coedge == 199 { 100 } else { coedge + 1 },
            previous: if coedge == 100 { 199 } else { coedge - 1 },
            radial_next: coedge,
        });
    }
    let bodies = super::super::bodies_intersecting(&topology, &BTreeSet::from([29])).unwrap();
    assert_eq!(bodies, BTreeSet::from([1]));
}

/// `bodies` wire bodies in a row, each sharing its end vertex with the next
/// body's start vertex. Body `b` owns entities `100 * b + 1..=100 * b + 7`.
fn wire_row_topology(bodies: i64) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let mut topology = AsmHistoricalTopology::default();
    for body in 0..bodies {
        let base = 100 * body;
        let (region, shell, edge, start, point) =
            (base + 1, base + 2, base + 3, base + 4, base + 5);
        topology.bodies.push(base);
        topology.body_regions.push(relation(base, vec![region]));
        topology.region_shells.push(relation(region, vec![shell]));
        topology.shell_faces.push(relation(shell, Vec::new()));
        topology.shell_wire_edges.push(relation(shell, vec![edge]));
        topology
            .shell_free_vertices
            .push(relation(shell, Vec::new()));
        topology.edge_vertices.push(AsmHistoricalEdge {
            edge,
            start_vertex: start,
            end_vertex: start + 100,
        });
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: start,
            carrier: point,
        });
    }
    let (last_vertex, last_point) = (100 * bodies + 4, 100 * bodies + 5);
    topology.vertex_points.push(AsmHistoricalCarrierBinding {
        entity: last_vertex,
        carrier: last_point,
    });
    topology
}

#[test]
fn body_queries_over_many_bodies_find_every_owner_of_each_entity() {
    use std::collections::BTreeSet;

    const BODIES: i64 = 1_500;
    let topology = wire_row_topology(BODIES);
    let mut queries = Vec::new();
    let mut expected = Vec::new();
    for body in 0..BODIES {
        let base = 100 * body;
        queries.push(BTreeSet::from([base, base + 1, base + 2, base + 3]));
        expected.push(BTreeSet::from([base]));
        let mut sharing = BTreeSet::from([base]);
        if body > 0 {
            sharing.insert(base - 100);
        }
        queries.push(BTreeSet::from([base + 4]));
        expected.push(sharing.clone());
        queries.push(BTreeSet::from([base + 5]));
        expected.push(sharing);
    }
    queries.push(
        (0..=BODIES)
            .flat_map(|body| 100 * body..100 * body + 6)
            .collect(),
    );
    expected.push((0..BODIES).map(|body| 100 * body).collect());
    queries.push(BTreeSet::from([-1, 6, 100 * BODIES + 50]));
    expected.push(BTreeSet::new());
    let touched =
        super::super::bodies_touching(&topology, &queries.iter().collect::<Vec<_>>()).unwrap();
    assert_eq!(touched.len(), expected.len());
    for ((query, bodies), expected) in queries.iter().zip(&touched).zip(&expected) {
        assert_eq!(bodies, expected, "query {query:?}");
    }
}

/// `wire_row_topology(bodies)` with the last body's end vertex on the point
/// of its start vertex, which the previous body's end vertex also reaches.
fn shared_point_topology(bodies: i64) -> crate::history_records::AsmHistoricalTopology {
    let mut topology = wire_row_topology(bodies);
    let start = 100 * bodies - 96;
    let binding = topology
        .vertex_points
        .iter_mut()
        .find(|binding| binding.entity == start + 100)
        .unwrap();
    binding.carrier = start + 1;
    topology
}

#[test]
fn an_entity_one_body_reaches_twice_after_another_body_lists_each_body_once() {
    use std::collections::BTreeSet;

    let topology = shared_point_topology(3);
    assert_eq!(
        super::super::bodies_intersecting(&topology, &BTreeSet::from([205, 304])),
        Some(BTreeSet::from([100, 200]))
    );
}

#[test]
fn one_closure_walk_answers_every_query() {
    use std::collections::BTreeSet;

    let queries = [
        BTreeSet::from([205, 304]),
        BTreeSet::from([4, 104, 403]),
        BTreeSet::new(),
        (0..=500).collect(),
        BTreeSet::from([-1, 6]),
        BTreeSet::from([304]),
    ];
    let expected = [
        BTreeSet::from([100, 200, 300]),
        BTreeSet::from([0, 100]),
        BTreeSet::new(),
        BTreeSet::from([0, 100, 200, 300]),
        BTreeSet::new(),
        BTreeSet::from([200, 300]),
    ];
    let queries = queries.iter().collect::<Vec<_>>();
    let topology = shared_point_topology(4);
    assert_eq!(
        super::super::bodies_touching(&topology, &queries),
        Some(expected.to_vec())
    );

    let mut reversed = topology.clone();
    reversed.bodies.reverse();
    reversed.body_regions.reverse();
    reversed.region_shells.reverse();
    reversed.shell_faces.reverse();
    reversed.shell_wire_edges.reverse();
    reversed.shell_free_vertices.reverse();
    reversed.edge_vertices.reverse();
    reversed.vertex_points.reverse();
    assert_eq!(
        super::super::bodies_touching(&reversed, &queries),
        Some(expected.to_vec())
    );

    let incomplete = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    assert_eq!(super::super::bodies_touching(&incomplete, &[]), None);
}

#[test]
fn the_last_row_of_a_repeated_key_binds_the_closure() {
    use crate::history_records::AsmHistoricalCarrierBinding;
    use std::collections::BTreeSet;

    let mut topology = wire_topology(false);
    topology.vertex_points.insert(
        1,
        AsmHistoricalCarrierBinding {
            entity: 8,
            carrier: 30,
        },
    );
    let touched =
        super::super::bodies_touching(&topology, &[&BTreeSet::from([28]), &BTreeSet::from([30])]);
    assert_eq!(touched, Some(vec![BTreeSet::new(), BTreeSet::from([1])]));
    topology.vertex_points.reverse();
    let touched =
        super::super::bodies_touching(&topology, &[&BTreeSet::from([28]), &BTreeSet::from([30])]);
    assert_eq!(touched, Some(vec![BTreeSet::from([1]), BTreeSet::new()]));
}

#[test]
fn body_queries_validate_each_snapshot() {
    use std::collections::BTreeSet;
    let topology = simple_topology();
    let incomplete = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    assert_eq!(
        super::super::bodies_intersecting(&topology, &BTreeSet::from([1])),
        Some(BTreeSet::from([1]))
    );
    assert_eq!(
        super::super::bodies_intersecting(&topology, &BTreeSet::from([2])),
        Some(BTreeSet::new())
    );
    assert_eq!(
        super::super::bodies_intersecting(&incomplete, &BTreeSet::from([1])),
        None
    );
}
