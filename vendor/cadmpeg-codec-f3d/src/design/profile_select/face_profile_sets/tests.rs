// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
    FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, LinearTermination,
    PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::NonZeroLength;

use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalEdge, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPlane, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology, AsmHistory,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, face::DesignFaceOperand,
};

use super::{
    bind_extrude_face_profile_sets, bind_one, cap_profile_faces, deleted_profile_faces,
    face_vertex_rings, member_order, reaches_a_cap, CapReading, CapSearch, DeletedFaces,
    FaceProfileSetResolution, PassedOver, SearchLimits,
};
use crate::design::profile_select::cap_regions::{face_cap, PlaneFrame};

const TOLERANCE: f64 = 1e-6;
const LIMITS: SearchLimits = SearchLimits {
    pool_faces: 256,
    outline_points: 4096,
    face_edges: 4096,
    cap_faces: 256,
};

/// A cap reading without its passed-over candidates and dispute reason.
#[derive(Debug, PartialEq)]
enum Reading {
    Selected(Vec<i64>),
    Disputed,
    Absent,
    Capped,
}

fn read(search: &CapSearch<'_>) -> Reading {
    match cap_profile_faces(search) {
        CapReading::Selected(faces, _) => Reading::Selected(faces),
        CapReading::Disputed(_) => Reading::Disputed,
        CapReading::Absent(_) => Reading::Absent,
        CapReading::Capped => Reading::Capped,
    }
}

/// The faces of the deleted-face reading when the transition updates none.
fn deleted(
    preceding: &AsmHistoricalTopology,
    faces: &[i64],
    members: usize,
    resolved: &[i64],
) -> Option<Vec<i64>> {
    deleted_profile_faces(preceding, (faces, &[]), members, resolved, TOLERANCE)
        .map(|deleted| deleted.faces)
}

fn caps(
    preceding: &AsmHistoricalTopology,
    topology: &AsmHistoricalTopology,
    inserted: &[i64],
    length: f64,
    members: usize,
    resolved: &[i64],
) -> Reading {
    caps_within(
        preceding, topology, inserted, length, members, resolved, LIMITS,
    )
}

fn caps_within(
    preceding: &AsmHistoricalTopology,
    topology: &AsmHistoricalTopology,
    inserted: &[i64],
    length: f64,
    members: usize,
    resolved: &[i64],
    limits: SearchLimits,
) -> Reading {
    let changed: HashSet<i64> = preceding.faces.iter().copied().collect();
    read(&CapSearch {
        preceding,
        topology,
        inserted,
        length,
        members,
        resolved,
        changed: &changed,
        new_body: false,
        tolerance: TOLERANCE,
        limits,
    })
}

/// The cap reading when the transition changes `changed` and, with
/// `new_body`, the Extrude makes a new body.
fn caps_given(
    preceding: &AsmHistoricalTopology,
    topology: &AsmHistoricalTopology,
    (inserted, length, members): (&[i64], f64, usize),
    changed: &HashSet<i64>,
    new_body: bool,
    limits: SearchLimits,
) -> Reading {
    read(&CapSearch {
        preceding,
        topology,
        inserted,
        length,
        members,
        resolved: &[],
        changed,
        new_body,
        tolerance: TOLERANCE,
        limits,
    })
}

/// The cap reading for `resolved` faces when the transition changes `changed`.
fn caps_resolved(
    preceding: &AsmHistoricalTopology,
    topology: &AsmHistoricalTopology,
    (inserted, length, members): (&[i64], f64, usize),
    resolved: &[i64],
    changed: &HashSet<i64>,
) -> Reading {
    read(&CapSearch {
        preceding,
        topology,
        inserted,
        length,
        members,
        resolved,
        changed,
        new_body: false,
        tolerance: TOLERANCE,
        limits: LIMITS,
    })
}

/// Polygonal planar faces normal to z with straight edges.
struct Faces {
    topology: AsmHistoricalTopology,
    next: i64,
}

impl Faces {
    fn new() -> Self {
        Self {
            topology: AsmHistoricalTopology::default(),
            next: 1000,
        }
    }

    fn id(&mut self) -> i64 {
        self.next += 1;
        self.next
    }

    /// Adds `face` over `[u0, u1] x [v0, v1]` at height `z`.
    fn rectangle(self, face: i64, (u0, v0): (f64, f64), (u1, v1): (f64, f64), z: f64) -> Self {
        self.polygon(face, &[(u0, v0), (u1, v0), (u1, v1), (u0, v1)], z)
    }

    /// Adds `face` with one loop through `corners` at height `z`.
    fn polygon(self, face: i64, corners: &[(f64, f64)], z: f64) -> Self {
        self.face_with_loops(face, &[corners], z)
    }

    /// Adds `face` with one loop through each corner list at height `z`.
    fn face_with_loops(mut self, face: i64, loops: &[&[(f64, f64)]], z: f64) -> Self {
        let loop_ids: Vec<i64> = loops.iter().map(|_| self.id()).collect();
        let surface = self.id();
        for (corners, loop_) in loops.iter().zip(loop_ids.iter().copied()) {
            self.add_loop(corners, loop_, z);
        }
        self.topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: loop_ids,
        });
        self.topology
            .face_surfaces
            .push(AsmHistoricalCarrierBinding {
                entity: face,
                carrier: surface,
            });
        self.topology.surface_planes.push(AsmHistoricalPlane {
            surface,
            origin: Point3::new(0.0, 0.0, z),
            normal: Vector3::new(0.0, 0.0, 1.0),
        });
        self.topology.faces.push(face);
        self
    }

    fn add_loop(&mut self, corners: &[(f64, f64)], loop_: i64, z: f64) {
        let corners: Vec<Point3> = corners
            .iter()
            .map(|(u, v)| Point3::new(*u, *v, z))
            .collect();
        let count = corners.len();
        let mut vertices = Vec::new();
        for corner in corners.iter().copied() {
            let (vertex, point) = (self.id(), self.id());
            self.topology
                .vertex_points
                .push(AsmHistoricalCarrierBinding {
                    entity: vertex,
                    carrier: point,
                });
            self.topology.point_positions.push(AsmHistoricalPoint {
                point,
                position: corner,
            });
            vertices.push(vertex);
        }
        let mut coedges = Vec::new();
        for k in 0..count {
            let (coedge, edge, curve) = (self.id(), self.id(), self.id());
            self.topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge,
                owner_loop: loop_,
                edge,
                next: 0,
                previous: 0,
                radial_next: 0,
            });
            self.topology.edge_vertices.push(AsmHistoricalEdge {
                edge,
                start_vertex: vertices[k],
                end_vertex: vertices[(k + 1) % count],
            });
            self.topology
                .edge_curves
                .push(AsmHistoricalOptionalCarrierBinding {
                    entity: edge,
                    carrier: Some(curve),
                });
            self.topology.curve_axes.push(AsmHistoricalCurveAxis {
                curve,
                origin: corners[k],
                direction: corners[(k + 1) % count].vector_from(corners[k]),
            });
            coedges.push(coedge);
        }
        self.topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: coedges,
        });
    }
}

/// Faces 1, 2 and 3 side by side on the plane z = 0, and face 4 below them.
fn preceding() -> AsmHistoricalTopology {
    Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(2, (1.0, 0.0), (2.0, 1.0), 0.0)
        .rectangle(3, (2.0, 0.0), (3.0, 1.0), 0.0)
        .rectangle(4, (0.0, 0.0), (3.0, 1.0), -5.0)
        .topology
}

/// One inserted cap, face 10, over `[0, u] x [0, 1]` at z = -2.
fn capped(u: f64) -> AsmHistoricalTopology {
    Faces::new()
        .rectangle(10, (0.0, 0.0), (u, 1.0), -2.0)
        .topology
}

#[test]
fn a_cap_proves_the_coplanar_faces_it_repeats() {
    let preceding = preceding();
    let cap = capped(2.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Selected(vec![1, 2])
    );
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[2]),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn a_cap_proves_nothing_against_another_member_count_or_resolved_face() {
    let preceding = preceding();
    let cap = capped(2.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 3, &[]),
        Reading::Disputed
    );
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[3]),
        Reading::Disputed
    );
    assert_eq!(
        caps(&preceding, &cap, &[10], 3.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_cap_that_cuts_through_a_face_proves_nothing() {
    let preceding = preceding();
    let cap = capped(1.25);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 1, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_cap_that_two_planes_repeat_proves_only_the_plane_of_a_resolved_or_changed_face() {
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(5, (0.0, 0.0), (1.0, 1.0), -4.0)
        .topology;
    let cap = capped(1.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 1, &[]),
        Reading::Disputed
    );
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 1, &[1]),
        Reading::Disputed
    );
    let one = HashSet::from([1]);
    let given = |changed: &HashSet<i64>, new_body| {
        caps_given(&preceding, &cap, (&[10], 2.0, 1), changed, new_body, LIMITS)
    };
    assert_eq!(given(&one, false), Reading::Selected(vec![1]));
    assert_eq!(given(&one, true), Reading::Disputed);
    assert_eq!(given(&HashSet::new(), false), Reading::Absent);
}

#[test]
fn an_unreadable_cap_voids_the_cap_proof() {
    let preceding = preceding();
    let mut cap = capped(2.0);
    cap.edge_curves.clear();
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn deleted_faces_on_the_resolved_plane_are_the_profile() {
    let preceding = preceding();
    assert_eq!(deleted(&preceding, &[2, 1], 2, &[1]), Some(vec![1, 2]));
}

#[test]
fn deleted_faces_prove_nothing_off_the_plane_without_a_resolved_face_or_by_count() {
    let preceding = preceding();
    assert_eq!(deleted(&preceding, &[1, 4], 2, &[1]), None);
    assert_eq!(deleted(&preceding, &[1, 2], 2, &[]), None);
    assert_eq!(deleted(&preceding, &[1, 2], 3, &[1]), None);
    assert_eq!(deleted(&preceding, &[1, 2], 2, &[3]), None);
    assert_eq!(deleted(&preceding, &[1, 1], 2, &[1]), None);
    assert_eq!(deleted(&preceding, &[1, 1, 2], 2, &[1]), None);
}

#[test]
fn resolved_members_keep_their_positions_and_the_rest_take_slot_order() {
    assert_eq!(
        member_order(&[Some(5), None, None], &[9, 5, 7]),
        Some(vec![5, 7, 9])
    );
    assert_eq!(
        member_order(&[None, Some(9), None], &[9, 5, 7]),
        Some(vec![5, 9, 7])
    );
    assert_eq!(member_order(&[Some(5), None], &[7, 8]), None);
    assert_eq!(member_order(&[None, None], &[7]), None);
    assert_eq!(member_order(&[Some(5), Some(5), None], &[5, 7, 9]), None);
}

#[test]
fn one_unreadable_cap_voids_the_caps_beside_it() {
    let preceding = preceding();
    let readable = Faces::new().rectangle(10, (0.0, 0.0), (2.0, 1.0), -2.0);
    let last_readable = readable.next;
    let mut cap_faces = readable
        .rectangle(11, (2.0, 0.0), (3.0, 1.0), -2.0)
        .topology;
    assert_eq!(
        caps(&preceding, &cap_faces, &[10, 11], 2.0, 3, &[]),
        Reading::Selected(vec![1, 2, 3])
    );
    cap_faces
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    assert_eq!(
        caps(&preceding, &cap_faces, &[10], 2.0, 2, &[]),
        Reading::Selected(vec![1, 2])
    );
    assert_eq!(
        caps(&preceding, &cap_faces, &[10, 11], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn caps_at_another_height_belong_to_another_plane() {
    let preceding = preceding();
    let cap_faces = Faces::new()
        .rectangle(10, (0.0, 0.0), (2.0, 1.0), -2.0)
        .rectangle(11, (2.0, 0.0), (3.0, 1.0), -3.0)
        .topology;
    assert_eq!(
        caps(&preceding, &cap_faces, &[10, 11], 2.0, 2, &[]),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn a_face_with_two_surface_bindings_is_on_no_plane() {
    let mut preceding = preceding();
    let first = preceding.face_surfaces.remove(0);
    preceding.face_surfaces.insert(
        0,
        AsmHistoricalCarrierBinding {
            entity: first.entity,
            carrier: first.carrier + 5000,
        },
    );
    preceding.face_surfaces.insert(0, first);
    let cap = capped(2.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
    assert_eq!(deleted(&preceding, &[1, 2], 2, &[2]), None);
}

#[test]
fn faces_that_miss_a_cap_corner_prove_nothing() {
    // Face 2 lacks the corner (2, 1) of the cap by a triangle far below the
    // area tolerance; the cap corner then lies on no outline segment.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .polygon(
            2,
            &[
                (1.0, 0.0),
                (2.0, 0.0),
                (2.0, 0.999),
                (1.999, 1.0),
                (1.0, 1.0),
            ],
            0.0,
        )
        .topology;
    let cap = capped(2.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_dent_off_the_cap_boundary_proves_nothing() {
    // Face 2 dents the cap's top edge; the dent's inner vertex lies on no
    // cap piece, while the area and the cap corners still agree.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .polygon(
            2,
            &[
                (1.0, 0.0),
                (2.0, 0.0),
                (2.0, 1.0),
                (1.501, 1.0),
                (1.5005, 0.9995),
                (1.5, 1.0),
                (1.0, 1.0),
            ],
            0.0,
        )
        .topology;
    let cap = capped(2.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

/// Face 1 over the cap and an unreadable face 2 over `[u0, u0 + 1] x [0.2, 0.8]`.
fn with_unreadable(u0: f64) -> AsmHistoricalTopology {
    let readable = Faces::new().rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0);
    let last_readable = readable.next;
    let mut topology = readable
        .rectangle(2, (u0, 0.2), (u0 + 1.0, 0.8), 0.0)
        .topology;
    topology
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    topology
}

#[test]
fn an_unreadable_face_that_reaches_into_a_cap_voids_the_cap_proof() {
    let cap = capped(1.0);
    assert_eq!(
        caps(&with_unreadable(0.5), &cap, &[10], 2.0, 1, &[]),
        Reading::Disputed
    );
    assert_eq!(
        caps(&with_unreadable(1.5), &cap, &[10], 2.0, 1, &[]),
        Reading::Selected(vec![1])
    );
}

#[test]
fn an_unreadable_face_around_a_cap_voids_the_cap_proof() {
    // Face 2's vertices all lie outside the cap, and the cap lies inside it.
    let readable = Faces::new().rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0);
    let last_readable = readable.next;
    let mut preceding = readable
        .rectangle(2, (-0.5, -0.5), (1.5, 1.5), 0.0)
        .topology;
    preceding
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    assert_eq!(
        caps(&preceding, &capped(1.0), &[10], 2.0, 1, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_failing_selection_disputes_only_when_it_could_be_the_profile() {
    // Cap 11 repeats face 4 for one member while the profile has two.
    let preceding = preceding();
    let cap_faces = Faces::new()
        .rectangle(10, (0.0, 0.0), (2.0, 1.0), -2.0)
        .rectangle(11, (0.0, 0.0), (3.0, 1.0), -7.0)
        .topology;
    let given = |changed: &HashSet<i64>, new_body| {
        caps_given(
            &preceding,
            &cap_faces,
            (&[10, 11], 2.0, 2),
            changed,
            new_body,
            LIMITS,
        )
    };
    assert_eq!(
        given(&HashSet::from([1, 2]), false),
        Reading::Selected(vec![1, 2])
    );
    assert_eq!(given(&HashSet::from([1, 2, 4]), false), Reading::Disputed);
    assert_eq!(given(&HashSet::from([1, 2]), true), Reading::Disputed);
}

#[test]
fn an_unreadable_face_in_a_cap_disputes_a_selection_that_can_be_the_profile() {
    let cap = capped(1.0);
    let given = |preceding: &AsmHistoricalTopology, changed: &HashSet<i64>| {
        caps_given(preceding, &cap, (&[10], 2.0, 1), changed, false, LIMITS)
    };
    let inside = with_unreadable(0.5);
    assert_eq!(given(&inside, &HashSet::from([1])), Reading::Disputed);
    assert_eq!(given(&inside, &HashSet::from([1, 2])), Reading::Disputed);
    assert_eq!(given(&inside, &HashSet::new()), Reading::Absent);
    assert_eq!(
        given(&with_unreadable(1.5), &HashSet::from([1])),
        Reading::Selected(vec![1])
    );
}

#[test]
fn a_coincident_unreadable_face_disputes_the_selection() {
    // Face 2 covers face 1 exactly; its vertices lie on the cap boundary.
    let readable = Faces::new().rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0);
    let last_readable = readable.next;
    let mut preceding = readable.rectangle(2, (0.0, 0.0), (1.0, 1.0), 0.0).topology;
    preceding
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    assert_eq!(
        caps_given(
            &preceding,
            &capped(1.0),
            (&[10], 2.0, 1),
            &HashSet::from([1]),
            false,
            LIMITS
        ),
        Reading::Disputed
    );
}

#[test]
fn an_oversized_face_on_the_plane_caps_a_selection_that_can_be_the_profile() {
    // Face 3 has five edges against a limit of four.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(2, (1.0, 0.0), (2.0, 1.0), 0.0)
        .polygon(
            3,
            &[(2.0, 0.0), (3.0, 0.0), (3.0, 1.0), (2.5, 1.0), (2.0, 1.0)],
            0.0,
        )
        .topology;
    let cap = capped(2.0);
    let limits = SearchLimits {
        face_edges: 4,
        ..LIMITS
    };
    let given = |changed: &HashSet<i64>| {
        caps_given(&preceding, &cap, (&[10], 2.0, 2), changed, false, limits)
    };
    assert_eq!(given(&HashSet::from([1, 2])), Reading::Capped);
    assert_eq!(given(&HashSet::from([1, 2, 3])), Reading::Capped);
    assert_eq!(given(&HashSet::new()), Reading::Absent);
    let small = SearchLimits {
        face_edges: 3,
        ..LIMITS
    };
    assert_eq!(
        caps_given(
            &preceding,
            &cap,
            (&[10], 2.0, 2),
            &HashSet::from([1, 2]),
            false,
            small
        ),
        Reading::Capped
    );
}

#[test]
fn a_face_of_short_loops_reaches_every_cap() {
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .polygon(2, &[(1.5, 0.2), (1.5, 0.8)], 0.0)
        .topology;
    let cap = capped(1.0);
    let given = |changed: &HashSet<i64>| {
        caps_given(&preceding, &cap, (&[10], 2.0, 1), changed, false, LIMITS)
    };
    assert_eq!(given(&HashSet::from([1])), Reading::Disputed);
    assert_eq!(given(&HashSet::new()), Reading::Absent);
}

#[test]
fn a_short_hole_leaves_the_reach_to_the_outer_loop() {
    // Face 2 lies beside the cap; its two-vertex hole makes it unreadable.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .face_with_loops(
            2,
            &[
                &[(1.5, 0.2), (2.5, 0.2), (2.5, 0.8), (1.5, 0.8)],
                &[(2.0, 0.4), (2.0, 0.6)],
            ],
            0.0,
        )
        .topology;
    assert_eq!(
        caps_given(
            &preceding,
            &capped(1.0),
            (&[10], 2.0, 1),
            &HashSet::from([1]),
            false,
            LIMITS
        ),
        Reading::Selected(vec![1])
    );
}

#[test]
fn a_new_body_profile_proves_from_its_caps() {
    let preceding = preceding();
    assert_eq!(
        caps_given(
            &preceding,
            &capped(2.0),
            (&[10], 2.0, 2),
            &HashSet::new(),
            true,
            LIMITS
        ),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn an_unreadable_face_around_a_cap_in_its_hole_does_not_reach_it() {
    // Face 2 rings the cap; the mean of its outer vertices lies in its hole.
    let readable = Faces::new().rectangle(1, (0.5, 0.5), (1.5, 1.5), 0.0);
    let last_readable = readable.next;
    let mut preceding = readable
        .face_with_loops(
            2,
            &[
                &[(-1.0, -1.0), (3.0, -1.0), (3.0, 3.0), (-1.0, 3.0)],
                &[(0.0, 0.0), (0.0, 2.0), (2.0, 2.0), (2.0, 0.0)],
            ],
            0.0,
        )
        .topology;
    preceding
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    let cap = Faces::new()
        .rectangle(10, (0.5, 0.5), (1.5, 1.5), -2.0)
        .topology;
    assert_eq!(
        caps_given(
            &preceding,
            &cap,
            (&[10], 2.0, 1),
            &HashSet::from([1]),
            false,
            LIMITS
        ),
        Reading::Selected(vec![1])
    );
}

#[test]
fn a_face_on_a_plane_at_an_angle_is_in_no_pool() {
    let mut preceding = preceding();
    let surface = preceding.face_surfaces[1].carrier;
    for plane in &mut preceding.surface_planes {
        if plane.surface == surface {
            plane.normal = Vector3::new(1.0, 0.0, 1.0);
        }
    }
    assert_eq!(
        caps(&preceding, &capped(2.0), &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_resolved_face_alone_lets_a_selection_be_the_profile() {
    let preceding = preceding();
    assert_eq!(
        caps_resolved(
            &preceding,
            &capped(2.0),
            (&[10], 2.0, 2),
            &[1],
            &HashSet::new()
        ),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn coincident_readable_faces_double_the_area() {
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(2, (0.0, 0.0), (1.0, 1.0), 0.0)
        .topology;
    assert_eq!(
        caps(&preceding, &capped(1.0), &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn an_oversized_cap_caps_the_search() {
    // The cap has five edges against a limit of four; the faces have four.
    let preceding = preceding();
    let cap = Faces::new()
        .polygon(
            10,
            &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (0.0, 1.0)],
            -2.0,
        )
        .topology;
    let limits = SearchLimits {
        face_edges: 4,
        ..LIMITS
    };
    assert_eq!(
        caps_within(&preceding, &cap, &[10], 2.0, 2, &[], limits),
        Reading::Capped
    );
    assert_eq!(
        caps_within(&preceding, &cap, &[10], 2.0, 2, &[], LIMITS),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn an_unchanged_oversized_face_beside_a_skipped_selection_caps_nothing() {
    // Face 2 is changed and outside the cap, face 1 is unchanged inside it,
    // and face 3 has five edges against a limit of four.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(2, (1.0, 0.0), (2.0, 1.0), 0.0)
        .polygon(
            3,
            &[(2.0, 0.0), (3.0, 0.0), (3.0, 1.0), (2.5, 1.0), (2.0, 1.0)],
            0.0,
        )
        .topology;
    let limits = SearchLimits {
        face_edges: 4,
        ..LIMITS
    };
    assert_eq!(
        caps_given(
            &preceding,
            &capped(1.0),
            (&[10], 2.0, 1),
            &HashSet::from([2]),
            false,
            limits
        ),
        Reading::Absent
    );
}

#[test]
fn a_face_whose_loops_cannot_be_read_reaches_every_cap() {
    let mut preceding = with_unreadable(1.5);
    let loops: Vec<i64> = preceding
        .face_loops
        .iter()
        .filter(|row| row.owner_ref == 2)
        .flat_map(|row| row.member_refs.clone())
        .collect();
    preceding
        .loop_coedges
        .retain(|row| !loops.contains(&row.owner_ref));
    let given = |changed: &HashSet<i64>| {
        caps_given(
            &preceding,
            &capped(1.0),
            (&[10], 2.0, 1),
            changed,
            false,
            LIMITS,
        )
    };
    assert_eq!(given(&HashSet::from([1, 2])), Reading::Disputed);
    assert_eq!(given(&HashSet::from([1])), Reading::Disputed);
}

#[test]
fn an_unreadable_face_that_can_be_the_profile_disputes_an_unchanged_selection() {
    // Face 1 is selected and unchanged; face 2 reaches into the cap and is changed.
    assert_eq!(
        caps_given(
            &with_unreadable(0.5),
            &capped(1.0),
            (&[10], 2.0, 1),
            &HashSet::from([2]),
            false,
            LIMITS
        ),
        Reading::Disputed
    );
}

#[test]
fn an_unreadable_cap_over_faces_that_cannot_be_the_profile_disputes_nothing() {
    let preceding = preceding();
    let mut cap = capped(2.0);
    cap.edge_curves.clear();
    assert_eq!(
        caps_given(
            &preceding,
            &cap,
            (&[10], 2.0, 2),
            &HashSet::new(),
            false,
            LIMITS
        ),
        Reading::Absent
    );
}

#[test]
fn a_new_body_skips_planes_and_selections_without_faces_in_its_caps() {
    // Face 5 lies at the second side's height outside the cap; cap 11 is
    // unreadable and has no faces at its heights.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(2, (1.0, 0.0), (2.0, 1.0), 0.0)
        .rectangle(5, (5.0, 0.0), (6.0, 1.0), -4.0)
        .topology;
    let readable = Faces::new().rectangle(10, (0.0, 0.0), (2.0, 1.0), -2.0);
    let last_readable = readable.next;
    let mut cap_faces = readable
        .rectangle(11, (0.0, 0.0), (2.0, 1.0), -20.0)
        .topology;
    cap_faces
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    assert_eq!(
        caps_given(
            &preceding,
            &cap_faces,
            (&[10, 11], 2.0, 2),
            &HashSet::new(),
            true,
            LIMITS
        ),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn a_selection_that_cannot_be_the_profile_beside_a_changed_face_disputes_nothing() {
    // Cap 11 repeats unchanged face 4; changed face 6 shares its plane outside it.
    let mut preceding = preceding();
    preceding = Faces {
        next: 5000,
        topology: preceding,
    }
    .rectangle(6, (7.0, 0.0), (8.0, 1.0), -5.0)
    .topology;
    let cap_faces = Faces::new()
        .rectangle(10, (0.0, 0.0), (2.0, 1.0), -2.0)
        .rectangle(11, (0.0, 0.0), (3.0, 1.0), -7.0)
        .topology;
    assert_eq!(
        caps_given(
            &preceding,
            &cap_faces,
            (&[10, 11], 2.0, 2),
            &HashSet::from([1, 2, 6]),
            false,
            LIMITS
        ),
        Reading::Selected(vec![1, 2])
    );
}

#[test]
fn an_unreadable_face_over_a_cap_corner_reaches_it() {
    // Face 2's vertices and vertex mean lie outside the cap; it holds the
    // cap corners (0, 0) and (1, 0).
    let readable = Faces::new().rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0);
    let last_readable = readable.next;
    let mut preceding = readable
        .rectangle(2, (-0.5, -0.5), (5.0, 0.5), 0.0)
        .topology;
    preceding
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    assert_eq!(
        caps_given(
            &preceding,
            &capped(1.0),
            (&[10], 2.0, 1),
            &HashSet::from([1, 2]),
            false,
            LIMITS
        ),
        Reading::Disputed
    );
}

#[test]
fn an_unreadable_neighbour_that_shares_a_cap_edge_does_not_reach_it() {
    let readable = Faces::new().rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0);
    let last_readable = readable.next;
    let mut preceding = readable.rectangle(2, (1.0, 0.0), (2.0, 1.0), 0.0).topology;
    preceding
        .edge_curves
        .retain(|row| row.entity <= last_readable);
    assert_eq!(
        caps_given(
            &preceding,
            &capped(1.0),
            (&[10], 2.0, 1),
            &HashSet::from([1, 2]),
            false,
            LIMITS
        ),
        Reading::Selected(vec![1])
    );
}

#[test]
fn a_sliver_face_on_the_cap_voids_the_cap_proof() {
    // Face 2 is narrower than the sampling clearance.
    let preceding = Faces::new()
        .rectangle(1, (0.0, 0.0), (1.0, 1.0), 0.0)
        .rectangle(2, (1.0, 0.0), (1.000_005, 1.0), 0.0)
        .topology;
    let cap = Faces::new()
        .rectangle(10, (0.0, 0.0), (1.000_005, 1.0), -2.0)
        .topology;
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_search_past_its_local_caps_states_capped() {
    let preceding = preceding();
    let cap = capped(2.0);
    let within = |pool_faces, outline_points, cap_faces| {
        caps_within(
            &preceding,
            &cap,
            &[10],
            2.0,
            2,
            &[],
            SearchLimits {
                pool_faces,
                outline_points,
                face_edges: 4096,
                cap_faces,
            },
        )
    };
    assert_eq!(within(2, 4096, 1), Reading::Capped);
    assert_eq!(within(3, 7, 1), Reading::Capped);
    assert_eq!(within(3, 8, 0), Reading::Capped);
    assert_eq!(within(3, 8, 1), Reading::Selected(vec![1, 2]));
}

#[test]
fn a_surface_with_two_plane_rows_is_no_plane() {
    let mut preceding = preceding();
    let surface = preceding.face_surfaces[0].carrier;
    preceding.surface_planes.insert(
        1,
        AsmHistoricalPlane {
            surface,
            origin: Point3::new(0.0, 0.0, 1.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        },
    );
    let cap = capped(2.0);
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
    assert_eq!(deleted(&preceding, &[1, 2], 2, &[2]), None);
}

#[test]
fn deleted_faces_outside_the_preceding_faces_prove_nothing() {
    let mut preceding = preceding();
    preceding.faces.retain(|face| *face != 2);
    assert_eq!(deleted(&preceding, &[1, 2], 2, &[1]), None);
}

const GROUP: &str = "f3d:Design/BulkStream.dat:design-construction-operand-group#100";

fn scope() -> DesignParameterScope {
    scope_between(2, 1)
}

fn scope_between(state: i64, previous: i64) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        DesignFeatureKind::Extrude,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = Some(previous);
            draft.reference_members = {
                let values: Vec<u32> = vec![100, 110, 111];
                let offsets = (0..values.len())
                    .map(|ordinal| 14 + 11 * u64_from_index(ordinal))
                    .collect();
                crate::records::identity::ReferenceRun::from_columns(
                    values,
                    offsets,
                    "reference_members",
                )
                .unwrap()
            };
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn group() -> DesignConstructionOperandGroup {
    group_at(100, 0, &[110, 111])
}

fn group_at(record_index: u32, ordinal: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..members.len())
        .map(|index| 11 * u64_from_index(index))
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:design-construction-operand-group#{record_index}"),
        "scope_record_index": 42,
        "scope_reference_ordinal": ordinal,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "308",
        "members": members,
        "member_offsets": member_offsets,
        "frame": {
            "member_count_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "variant": false
        },
        "role": 279_172_874_240_u64,
        "extrude_role": "profile",
        "role_offset": 0,
        "paired_class_tag": "266",
        "paired_byte_offset": 0
    }))
    .unwrap()
}

fn operand(record_index: u32, ordinal: u32, slots: Vec<i64>) -> DesignFaceOperand {
    operand_in(100, record_index, ordinal, slots)
}

fn operand_in(
    group_record_index: u32,
    record_index: u32,
    ordinal: u32,
    slots: Vec<i64>,
) -> DesignFaceOperand {
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": format!("f3d:Design/BulkStream.dat:design-face-operand#{record_index}"),
        "scope_record_index": 42,
        "scope_reference_ordinal": ordinal + 1,
        "group_record_index": group_record_index,
        "group_member_ordinal": ordinal,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "310",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": record_index + 3,
        "recipe_record_byte_offset": 32,
        "recipe_id": format!(
            "f3d:Design/BulkStream.dat:construction-recipe#{}",
            record_index + 3
        ),
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "next_record_index": record_index + 4,
        "next_byte_offset": 160
    }))
    .unwrap();
    operand.resolved_face_slots = slots;
    operand
}

fn state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    next_ref: Option<i64>,
) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("f3d:history:state#{state_id}"),
            parent: "f3d:history".into(),
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
        Vec::new(),
    )
}

/// Faces 1 and 2 give way to the cap 10 of a 2-deep cut; faces 3 and 4 stay.
fn cut_history() -> AsmHistory {
    let mut current = capped(2.0);
    current.faces.extend([3, 4]);
    current.faces.sort_unstable();
    history_to(current)
}

/// Faces 1 and 2 go without a cap; faces 3 and 4 stay.
fn through_history() -> AsmHistory {
    history_to(AsmHistoricalTopology {
        faces: vec![3, 4],
        ..AsmHistoricalTopology::default()
    })
}

fn history_to(current: AsmHistoricalTopology) -> AsmHistory {
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(1, preceding(), None), state(2, current, Some(1))],
    )
}

fn extrude(termination: LinearTermination) -> Feature {
    extrude_from(ExtrudeStart::ProfilePlane {}, termination, None)
}

fn extrude_from(
    start: ExtrudeStart,
    termination: LinearTermination,
    draft: Option<cadmpeg_ir::scalar::SlopeAngle>,
) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#f3d:feature:extrude").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        native_ref: Some(scope().id),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Native(GROUP.into())),
                direction: ExtrudeDirection::ReversedProfileNormal {},
                start,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide { termination, draft },
                },
                op: BooleanOp::Cut,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )),
    }
}

fn blind() -> LinearTermination {
    LinearTermination::Blind {
        length: NonZeroLength::new(2.0).unwrap(),
    }
}

fn bind(features: &mut [Feature], operands: &[DesignFaceOperand]) -> Vec<LossNote> {
    bind_in(&cut_history(), features, operands)
}

fn bind_in(
    history: &AsmHistory,
    features: &mut [Feature],
    operands: &[DesignFaceOperand],
) -> Vec<LossNote> {
    bind_with(&scope(), &[group()], history, features, operands)
}

fn bind_with(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    history: &AsmHistory,
    features: &mut [Feature],
    operands: &[DesignFaceOperand],
) -> Vec<LossNote> {
    let scope_histories = HashMap::from([(scope.id.clone(), history.id.clone())]);
    crate::test_support::with_decode_context(|ctx| {
        bind_extrude_face_profile_sets(
            features,
            &FaceProfileSetResolution {
                scopes: std::slice::from_ref(scope),
                groups,
                operands,
                histories: std::slice::from_ref(history),
                scope_histories: &scope_histories,
                linear_tolerance: TOLERANCE,
                ctx,
            },
        )
    })
    .unwrap()
}

fn loss_codes(losses: &[LossNote]) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    losses.iter().map(|loss| loss.code.clone()).collect()
}

fn profile_faces(feature: &Feature) -> Option<Vec<String>> {
    profile_faces_after(feature, 1)
}

fn profile_faces_after(feature: &Feature, previous: i64) -> Option<Vec<String>> {
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile:
            ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
                state,
                faces,
                native,
            }),
        ..
    }) = feature.evaluation.definition()
    else {
        return None;
    };
    let input = crate::ids::neutral_feature_id(&scope());
    assert_eq!(
        *state,
        crate::ids::feature_input_topology_id(&input, previous)
    );
    assert_eq!(native.as_slice(), [GROUP.to_owned()]);
    Some(faces.iter().map(ToString::to_string).collect())
}

fn input_faces(slots: &[i64]) -> Vec<String> {
    let input = crate::ids::neutral_feature_id(&scope());
    let prefix = crate::ids::history_input_prefix(&input.key(), 1);
    slots
        .iter()
        .map(|slot| crate::ids::history_input_face_id(&prefix, *slot).to_string())
        .collect()
}

#[test]
fn a_blind_cut_profile_takes_the_faces_its_cap_covers_with_inference_and_order_losses() {
    let mut features = [extrude(blind())];
    let losses = bind(
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![])],
    );
    assert_eq!(profile_faces(&features[0]), Some(input_faces(&[1, 2])));
    assert_eq!(
        loss_codes(&losses),
        [
            F3dLossCode::FeatureExtrudeProfileCapFacesInferred.kind(),
            F3dLossCode::FeatureExtrudeProfileFaceOrderAssumed.kind()
        ]
    );
    assert!(losses.iter().all(|loss| loss.message.contains(GROUP)));
}

#[test]
fn a_resolved_member_keeps_its_face_and_one_unresolved_member_states_only_the_inference() {
    let mut features = [extrude(blind())];
    let losses = bind(
        &mut features,
        &[operand(110, 0, vec![2]), operand(111, 1, vec![])],
    );
    assert_eq!(profile_faces(&features[0]), Some(input_faces(&[2, 1])));
    assert_eq!(
        loss_codes(&losses),
        [F3dLossCode::FeatureExtrudeProfileCapFacesInferred.kind()]
    );
}

#[test]
fn a_profile_outside_a_blind_extent_stays_native() {
    let mut features = [extrude(LinearTermination::ThroughAll {})];
    let losses = bind(
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_profile_with_an_offset_start_or_a_draft_stays_native() {
    let offset = ExtrudeStart::OffsetProfilePlane {
        offset: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
    };
    let draft = Some(cadmpeg_ir::scalar::SlopeAngle::new(0.1).unwrap());
    for mut feature in [
        extrude_from(offset, blind(), None),
        extrude_from(ExtrudeStart::ProfilePlane {}, blind(), draft),
    ] {
        let losses = bind(
            std::slice::from_mut(&mut feature),
            &[operand(110, 0, vec![]), operand(111, 1, vec![])],
        );
        assert_eq!(profile_faces(&feature), None);
        assert!(losses.is_empty());
    }
}

#[test]
fn a_member_with_several_faces_or_a_repeated_face_stays_native() {
    for operands in [
        [operand(110, 0, vec![1, 2]), operand(111, 1, vec![])],
        [operand(110, 0, vec![1]), operand(111, 1, vec![1])],
    ] {
        let mut features = [extrude(blind())];
        let losses = bind(&mut features, &operands);
        assert_eq!(profile_faces(&features[0]), None);
        assert!(losses.is_empty());
    }
}

#[test]
fn without_a_cap_the_deleted_faces_on_the_resolved_plane_are_the_inferred_profile() {
    let history = through_history();
    let mut features = [extrude(blind())];
    let losses = bind_in(
        &history,
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![1])],
    );
    assert_eq!(profile_faces(&features[0]), Some(input_faces(&[2, 1])));
    assert_eq!(
        loss_codes(&losses),
        [F3dLossCode::FeatureExtrudeProfileDeletedFacesInferred.kind()]
    );
    assert!(losses[0].message.contains(GROUP));
    let mut features = [extrude(blind())];
    let losses = bind_in(
        &history,
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_native_profile_that_names_another_group_stays_native() {
    let mut features = [extrude(blind())];
    features[0].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) = definition
        {
            *profile = ProfileRef::Planar(PlanarProfileRef::Native(scope().id));
        }
    });
    let losses = bind(
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_scope_with_a_second_profile_root_stays_native() {
    let mut features = [extrude(blind())];
    let losses = bind_with(
        &scope(),
        &[group(), group_at(101, 3, &[112])],
        &cut_history(),
        &mut features,
        &[
            operand(110, 0, vec![]),
            operand(111, 1, vec![]),
            operand_in(101, 112, 0, vec![]),
        ],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_native_profile_whose_members_all_resolve_stays_native() {
    let mut features = [extrude(blind())];
    let losses = bind(
        &mut features,
        &[operand(110, 0, vec![1]), operand(111, 1, vec![2])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn only_the_transition_from_the_previous_state_proves_a_profile() {
    // State 2 repeats state 1, and state 3 is the cut.
    let mut current = capped(2.0);
    current.faces.extend([3, 4]);
    current.faces.sort_unstable();
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(1, preceding(), None),
            state(2, preceding(), Some(1)),
            state(3, current, Some(2)),
        ],
    );
    let operands = [operand(110, 0, vec![]), operand(111, 1, vec![])];
    let mut features = [extrude(blind())];
    bind_with(
        &scope_between(3, 1),
        &[group()],
        &history,
        &mut features,
        &operands,
    );
    assert_eq!(profile_faces(&features[0]), None);
    let mut features = [extrude(blind())];
    bind_with(
        &scope_between(3, 2),
        &[group()],
        &history,
        &mut features,
        &operands,
    );
    assert!(profile_faces_after(&features[0], 2).is_some());
}

#[test]
fn a_cap_set_that_the_deleted_faces_contradict_stays_native() {
    // The cap repeats faces 1 and 2; the transition deletes faces 1 and 3.
    let mut current = capped(2.0);
    current.faces.extend([2, 4]);
    current.faces.sort_unstable();
    let mut features = [extrude(blind())];
    let losses = bind_in(
        &history_to(current),
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![1])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert_eq!(
        loss_codes(&losses),
        [F3dLossCode::FeatureExtrudeProfileFaceSetDisputed.kind()]
    );
    assert!(losses[0].message.contains("[1, 3]"));
}

#[test]
fn a_disputed_cap_keeps_the_deleted_faces_from_proving_with_a_loss() {
    // The cap repeats faces 1, 2 and 3 for two members; the transition
    // deletes faces 1 and 2.
    let mut current = capped(3.0);
    current.faces.extend([3, 4]);
    current.faces.sort_unstable();
    let mut features = [extrude(blind())];
    let losses = bind_in(
        &history_to(current),
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![1])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert_eq!(
        loss_codes(&losses),
        [F3dLossCode::FeatureExtrudeProfileFaceSetDisputed.kind()]
    );
    assert!(losses[0].message.contains(GROUP));
}

#[test]
fn a_capped_search_keeps_the_profile_native_with_a_loss() {
    let scope = scope();
    let history = cut_history();
    let scope_histories = HashMap::from([(scope.id.clone(), history.id.clone())]);
    let operands = [operand(110, 0, vec![]), operand(111, 1, vec![])];
    let mut feature = extrude(blind());
    let mut losses = Vec::new();
    feature.evaluation.edit(|definition, _| {
        losses = crate::test_support::with_decode_context(|ctx| {
            bind_one(
                definition,
                &scope,
                &FaceProfileSetResolution {
                    scopes: std::slice::from_ref(&scope),
                    groups: &[group()],
                    operands: &operands,
                    histories: std::slice::from_ref(&history),
                    scope_histories: &scope_histories,
                    linear_tolerance: TOLERANCE,
                    ctx,
                },
                SearchLimits {
                    pool_faces: 1,
                    outline_points: 4096,
                    face_edges: 4096,
                    cap_faces: 4096,
                },
            )
        })
        .unwrap();
    });
    assert_eq!(
        loss_codes(&losses),
        [F3dLossCode::FeatureExtrudeProfileFaceSearchCapped.kind()]
    );
    assert!(losses[0].message.contains(GROUP));
    assert_eq!(profile_faces(&feature), None);
}

#[test]
fn an_extrude_without_a_profile_normal_direction_stays_native() {
    let mut features = [extrude(blind())];
    features[0].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) =
            definition
        {
            *direction = ExtrudeDirection::Unresolved {};
        }
    });
    let losses = bind(
        &mut features,
        &[operand(110, 0, vec![]), operand(111, 1, vec![])],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_member_with_an_active_face_stays_native() {
    let mut active = operand(110, 0, vec![]);
    active.resolved_active_face =
        Some(cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(1)).unwrap());
    let mut features = [extrude(blind())];
    let losses = bind(&mut features, &[active, operand(111, 1, vec![])]);
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_cap_or_preceding_face_without_one_plane_carrier_disputes() {
    let preceding = preceding();
    let mut cap = capped(2.0);
    let surface = cap.face_surfaces[0].carrier;
    cap.surface_planes.push(AsmHistoricalPlane {
        surface,
        origin: Point3::new(0.0, 0.0, -2.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
    assert_eq!(
        caps(&preceding, &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
    // Face 4 lies below the caps' reach, and its plane cannot be read.
    let mut twice = preceding.clone();
    let surface = twice.face_surfaces[3].carrier;
    twice.surface_planes.push(AsmHistoricalPlane {
        surface,
        origin: Point3::new(0.0, 0.0, -5.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
    assert_eq!(
        caps(&twice, &capped(2.0), &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
    let mut unbound = preceding;
    unbound.face_surfaces.retain(|binding| binding.entity != 4);
    assert_eq!(
        caps(&unbound, &capped(2.0), &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_cap_edge_with_two_axis_rows_disputes() {
    let mut cap = capped(2.0);
    let row = cap.curve_axes[0].clone();
    cap.curve_axes.push(AsmHistoricalCurveAxis {
        direction: Vector3::new(0.0, 0.0, 1.0),
        ..row
    });
    assert_eq!(
        caps(&preceding(), &cap, &[10], 2.0, 2, &[]),
        Reading::Disputed
    );
}

#[test]
fn a_cap_selection_names_the_faces_it_passes_over() {
    let preceding = preceding();
    let cap = capped(2.0);
    let changed: HashSet<i64> = preceding.faces.iter().copied().collect();
    assert_eq!(
        cap_profile_faces(&CapSearch {
            preceding: &preceding,
            topology: &cap,
            inserted: &[10],
            length: 2.0,
            members: 2,
            resolved: &[],
            changed: &changed,
            new_body: false,
            tolerance: TOLERANCE,
            limits: LIMITS,
        }),
        CapReading::Selected(
            vec![1, 2],
            PassedOver {
                outside: vec![3],
                clear: Vec::new(),
                skipped: 0,
            }
        )
    );
}

#[test]
fn the_deleted_faces_name_the_coplanar_faces_they_pass_over() {
    let preceding = preceding();
    assert_eq!(
        deleted_profile_faces(&preceding, (&[2, 1], &[3]), 2, &[1], TOLERANCE),
        Some(DeletedFaces {
            faces: vec![1, 2],
            updated: vec![3],
            kept: Vec::new(),
        })
    );
    assert_eq!(
        deleted_profile_faces(&preceding, (&[2, 1], &[]), 2, &[1], TOLERANCE),
        Some(DeletedFaces {
            faces: vec![1, 2],
            updated: Vec::new(),
            kept: vec![3],
        })
    );
    // Face 3's surface has no geometry row; its vertices keep it on the plane.
    let mut unknown = preceding;
    let surface = unknown.face_surfaces[2].carrier;
    unknown
        .surface_planes
        .retain(|plane| plane.surface != surface);
    assert_eq!(
        deleted_profile_faces(&unknown, (&[2, 1], &[]), 2, &[1], TOLERANCE)
            .map(|deleted| deleted.kept),
        Some(vec![3])
    );
}

#[test]
fn an_unreadable_face_with_a_caps_concave_outline_reaches_it() {
    // An L face with the outline of an L cap, and a square beside the cap.
    let l_shape = [
        (0.0, 0.0),
        (2.0, 0.0),
        (2.0, 1.0),
        (1.0, 1.0),
        (1.0, 2.0),
        (0.0, 2.0),
    ];
    let topology = Faces::new()
        .polygon(10, &l_shape, 0.0)
        .polygon(11, &l_shape, 0.0)
        .rectangle(12, (3.0, 0.0), (4.0, 1.0), 0.0)
        .topology;
    let frame =
        PlaneFrame::of_plane(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap();
    let caps = [face_cap(&frame, &topology, 10, TOLERANCE).unwrap()];
    let rings = |face| face_vertex_rings(&frame, &topology, face).unwrap();
    assert!(reaches_a_cap(&rings(11), &caps, TOLERANCE));
    assert!(!reaches_a_cap(&rings(12), &caps, TOLERANCE));
}

fn without_geometry_rows(
    mut topology: AsmHistoricalTopology,
    index: usize,
) -> AsmHistoricalTopology {
    let surface = topology.face_surfaces[index].carrier;
    topology
        .surface_planes
        .retain(|plane| plane.surface != surface);
    topology
}

#[test]
fn a_face_on_a_surface_without_geometry_rows_is_read_by_its_vertices() {
    let changed: HashSet<i64> = [1, 2].into_iter().collect();
    let search = |preceding: &AsmHistoricalTopology, topology: &AsmHistoricalTopology| {
        cap_profile_faces(&CapSearch {
            preceding,
            topology,
            inserted: &[10],
            length: 2.0,
            members: 2,
            resolved: &[],
            changed: &changed,
            new_body: false,
            tolerance: TOLERANCE,
            limits: LIMITS,
        })
    };
    // Face 3 lies beside the cap on the profile plane.
    assert_eq!(
        search(&without_geometry_rows(preceding(), 2), &capped(2.0)),
        CapReading::Selected(
            vec![1, 2],
            PassedOver {
                outside: Vec::new(),
                clear: vec![3],
                skipped: 0,
            }
        )
    );
    assert!(matches!(
        search(&without_geometry_rows(preceding(), 1), &capped(2.0)),
        CapReading::Disputed(_)
    ));
    assert!(matches!(
        search(&preceding(), &without_geometry_rows(capped(2.0), 0)),
        CapReading::Disputed(_)
    ));
}

#[test]
fn a_profile_whose_resolved_members_repeat_a_face_stays_native() {
    let mut features = [extrude(blind())];
    let losses = bind_with(
        &scope(),
        &[group_at(100, 0, &[110, 111, 112])],
        &cut_history(),
        &mut features,
        &[
            operand(110, 0, vec![1]),
            operand(111, 1, vec![1]),
            operand(112, 2, vec![]),
        ],
    );
    assert_eq!(profile_faces(&features[0]), None);
    assert!(losses.is_empty());
}

#[test]
fn a_face_with_a_point_that_is_not_finite_reaches_a_cap() {
    let topology = capped(2.0);
    let frame =
        PlaneFrame::of_plane(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)).unwrap();
    let caps = [face_cap(&frame, &topology, 10, TOLERANCE).unwrap()];
    let far = cadmpeg_ir::math::Point2::new(9.0, 9.0);
    let rings = vec![vec![
        far,
        cadmpeg_ir::math::Point2::new(10.0, 9.0),
        cadmpeg_ir::math::Point2::new(f64::NAN, 10.0),
    ]];
    assert!(reaches_a_cap(&rings, &caps, TOLERANCE));
    let rings = vec![vec![
        far,
        cadmpeg_ir::math::Point2::new(10.0, 9.0),
        cadmpeg_ir::math::Point2::new(10.0, 10.0),
    ]];
    assert!(!reaches_a_cap(&rings, &caps, TOLERANCE));
}
