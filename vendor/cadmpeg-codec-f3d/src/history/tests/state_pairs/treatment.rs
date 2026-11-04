// SPDX-License-Identifier: Apache-2.0
//! Treatment candidate behavior over historical topology.
#![allow(clippy::unwrap_used)]

use crate::history::{treatment_edge_candidates, treatment_radius_candidates};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalRelation,
    AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};

#[test]
fn treatment_radius_candidates_require_a_new_radius_carrier_and_deleted_support_edge() {
    use cadmpeg_ir::ids::FaceId;

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
    let preceding = AsmHistoricalTopology {
        faces: vec![10, 11],
        surfaces: vec![100, 101],
        face_loops: vec![relation(10, vec![110]), relation(11, vec![111])],
        loop_coedges: vec![relation(110, vec![1100]), relation(111, vec![1110])],
        coedge_topology: vec![coedge(1100, 110, 17), coedge(1110, 111, 17)],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        faces: vec![10, 11, 20],
        surfaces: vec![100, 101, 200],
        surface_radii: vec![AsmHistoricalSurfaceRadius {
            surface: 200,
            radius: 3.0,
        }],
        face_loops: vec![
            relation(10, vec![210]),
            relation(11, vec![211]),
            relation(20, vec![220]),
        ],
        loop_coedges: vec![
            relation(210, vec![2100]),
            relation(211, vec![2110]),
            relation(220, vec![2200, 2201]),
        ],
        coedge_topology: vec![
            coedge(2100, 210, 30),
            coedge(2110, 211, 31),
            coedge(2200, 220, 30),
            coedge(2201, 220, 31),
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = crate::history::treatment_index::TreatmentCache::default();
    for _ in 0..1_000 {
        let (radii, edges) =
            treatment_edge_candidates(None, &[20], &result, &preceding, &[17], &mut cache);
        assert_eq!(edges, [17]);
        assert_eq!(radii.len(), 1);
        assert_eq!(radii[0].radius.get(), 3.0);
    }
    ctx.finish_session().unwrap();
    let candidates = {
        treatment_radius_candidates(
            Some(&[FaceId::mint("f3d:brep:entity#10").expect("identity grammar")]),
            &[20],
            &result,
            &preceding,
            &[17],
            &mut crate::history::treatment_index::TreatmentCache::default(),
        )
    };
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].edge_slot, 17);
    assert_eq!(candidates[0].radius.get(), 3.0);
    assert_eq!(
        treatment_edge_candidates(
            None,
            &[20],
            &result,
            &preceding,
            &[17],
            &mut crate::history::treatment_index::TreatmentCache::default()
        )
        .1,
        [17]
    );

    let mut existing_carrier = preceding.clone();
    existing_carrier.surfaces.push(200);
    assert!(treatment_radius_candidates(
        Some(&[FaceId::mint("f3d:brep:entity#10").expect("identity grammar")]),
        &[20],
        &result,
        &existing_carrier,
        &[17],
        &mut crate::history::treatment_index::TreatmentCache::default()
    )
    .is_empty());
    assert!(treatment_edge_candidates(
        None,
        &[20],
        &result,
        &preceding,
        &[18],
        &mut crate::history::treatment_index::TreatmentCache::default()
    )
    .1
    .is_empty());
    assert!(treatment_radius_candidates(
        Some(&[FaceId::mint("f3d:brep:entity#10").expect("identity grammar")]),
        &[20],
        &result,
        &preceding,
        &[18],
        &mut crate::history::treatment_index::TreatmentCache::default()
    )
    .is_empty());
}

/// Faces 10 and 11 meet at edge 17 along z, and face 12 caps them at edges
/// 18 and 19 along y. Blend face 20 on new surface 200 meets all three.
fn capped_blend(cylinder: bool) -> (AsmHistoricalTopology, AsmHistoricalTopology) {
    use crate::history_records::{
        AsmHistoricalCurveAxis, AsmHistoricalCylinder, AsmHistoricalEdgeSample,
        AsmHistoricalOptionalCarrierBinding,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let state = |boundaries: &[(i64, &[i64])]| {
        let mut topology = AsmHistoricalTopology::default();
        for &(face, edges) in boundaries {
            topology.faces.push(face);
            topology.surfaces.push(10 * face);
            topology.face_surfaces.push(AsmHistoricalCarrierBinding {
                entity: face,
                carrier: 10 * face,
            });
            topology.face_loops.push(AsmHistoricalRelation {
                owner_ref: face,
                member_refs: vec![1_000 + face],
            });
            let coedges = edges
                .iter()
                .map(|edge| 100 * face + edge)
                .collect::<Vec<_>>();
            topology.loop_coedges.push(AsmHistoricalRelation {
                owner_ref: 1_000 + face,
                member_refs: coedges.clone(),
            });
            for (coedge, edge) in coedges.into_iter().zip(edges) {
                topology.coedge_topology.push(AsmHistoricalCoedge {
                    coedge,
                    owner_loop: 1_000 + face,
                    edge: *edge,
                    next: coedge,
                    previous: coedge,
                    radial_next: coedge,
                });
            }
        }
        topology
    };
    let mut preceding = state(&[(10, &[17, 18]), (11, &[17, 19]), (12, &[18, 19])]);
    for (edge, direction) in [
        (17, Vector3::new(0.0, 0.0, 1.0)),
        (18, Vector3::new(0.0, 1.0, 0.0)),
        (19, Vector3::new(0.0, 1.0, 0.0)),
    ] {
        preceding.edges.push(edge);
        preceding
            .edge_curves
            .push(AsmHistoricalOptionalCarrierBinding {
                entity: edge,
                carrier: Some(10 * edge),
            });
        preceding.curve_axes.push(AsmHistoricalCurveAxis {
            curve: 10 * edge,
            origin: Point3::new(0.0, 0.0, 0.0),
            direction,
        });
        preceding.edge_samples.push(AsmHistoricalEdgeSample {
            edge,
            interior: Point3::new(0.0, 0.0, 0.0),
            tangent: Some(direction),
        });
    }
    let mut result = state(&[(10, &[30]), (11, &[31]), (12, &[32]), (20, &[30, 31, 32])]);
    result.surface_radii.push(AsmHistoricalSurfaceRadius {
        surface: 200,
        radius: 3.0,
    });
    if cylinder {
        result.surface_cylinders.push(AsmHistoricalCylinder {
            surface: 200,
            origin: Point3::new(3.0, 3.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 3.0,
        });
    }
    (preceding, result)
}

#[test]
fn a_cylinder_blend_pairs_only_the_deleted_edge_along_its_axis() {
    let (preceding, result) = capped_blend(true);
    let (radii, edges) = treatment_edge_candidates(
        None,
        &[20],
        &result,
        &preceding,
        &[17, 18, 19],
        &mut crate::history::treatment_index::TreatmentCache::default(),
    );
    assert_eq!(edges, [17]);
    assert_eq!(
        radii
            .iter()
            .map(|candidate| candidate.edge_slot)
            .collect::<Vec<_>>(),
        [17]
    );
}

#[test]
fn a_blend_carrier_without_a_spine_pairs_every_deleted_support_edge() {
    let (preceding, result) = capped_blend(false);
    let (radii, edges) = treatment_edge_candidates(
        None,
        &[20],
        &result,
        &preceding,
        &[17, 18, 19],
        &mut crate::history::treatment_index::TreatmentCache::default(),
    );
    assert_eq!(edges, [17, 18, 19]);
    assert_eq!(radii.len(), 3);
}
