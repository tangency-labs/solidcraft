// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::TAU;

use cadmpeg_ir::features::SketchProfileBoundaryUse;
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchPlacement,
};

use crate::design::geometry::AtomicSketchRegion;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalCylinder, AsmHistoricalEdge, AsmHistoricalEdgeSample,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};

use super::{
    changed_planes, face_cap, face_cap_reading, face_outlines_cover_caps, outlines_agree,
    polygon_area, Agreement, PlaneFrame, Sampled,
};

const TOLERANCE: f64 = 1e-6;
/// The cap face every test topology draws.
const CAP: i64 = 100;

fn xy_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#cap-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn xy_frame() -> PlaneFrame {
    PlaneFrame::of_sketch(&xy_sketch()).unwrap()
}

fn slot(index: usize) -> i64 {
    i64::try_from(index).unwrap()
}

/// A topology whose cap face has one loop of `edges`, each `(start, end)`
/// vertex pair stored in that direction, at height 2 above the sketch.
fn loop_topology(corners: &[(f64, f64)], edges: &[(usize, usize)]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: CAP,
            member_refs: vec![200],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 200,
            member_refs: (0..edges.len()).map(|k| 300 + slot(k)).collect(),
        }],
        ..AsmHistoricalTopology::default()
    };
    for (k, (u, v)) in corners.iter().enumerate() {
        let k = slot(k);
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: 500 + k,
            carrier: 600 + k,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: 600 + k,
            position: Point3::new(*u, *v, 2.0),
        });
    }
    for (k, (start, end)) in edges.iter().enumerate() {
        let k = slot(k);
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: 300 + k,
            owner_loop: 200,
            edge: 400 + k,
            next: 0,
            previous: 0,
            radial_next: 0,
        });
        topology.edge_vertices.push(AsmHistoricalEdge {
            edge: 400 + k,
            start_vertex: 500 + slot(*start),
            end_vertex: 500 + slot(*end),
        });
    }
    topology
}

/// Gives every edge a straight carrier through its stored ends.
fn straight_carriers(topology: &mut AsmHistoricalTopology) {
    for row in topology.edge_vertices.clone() {
        let at = |vertex: i64| {
            let point = topology
                .vertex_points
                .iter()
                .find(|binding| binding.entity == vertex)
                .unwrap()
                .carrier;
            topology
                .point_positions
                .iter()
                .find(|position| position.point == point)
                .unwrap()
                .position
        };
        let (start, end) = (at(row.start_vertex), at(row.end_vertex));
        topology
            .edge_curves
            .push(AsmHistoricalOptionalCarrierBinding {
                entity: row.edge,
                carrier: Some(row.edge + 1000),
            });
        topology.curve_axes.push(AsmHistoricalCurveAxis {
            curve: row.edge + 1000,
            origin: start,
            direction: end.vector_from(start),
        });
    }
}

fn unit_square_corners() -> Vec<(f64, f64)> {
    vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
}

#[test]
fn a_loop_whose_first_edge_is_stored_backwards_keeps_every_corner() {
    // The first edge is stored from corner 1 to corner 0; the loop runs
    // 0 -> 1 -> 2 -> 3, so it must be drawn from 0 to 1 before the rest.
    let mut topology = loop_topology(&unit_square_corners(), &[(1, 0), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    let cap = face_cap(&xy_frame(), &topology, CAP, TOLERANCE).unwrap();
    assert_eq!(cap.loops[0].len(), 4);
    for corner in unit_square_corners() {
        assert!(
            cap.loops[0]
                .iter()
                .any(|point| (point.u - corner.0).hypot(point.v - corner.1) < 1e-12),
            "{corner:?} missing from {:?}",
            cap.loops[0]
        );
    }
    assert!((polygon_area(&cap.loops[0]).abs() - 1.0).abs() < 1e-12);
}

#[test]
fn a_loop_that_does_not_close_proves_no_cap() {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 1)]);
    straight_carriers(&mut topology);
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());
}

/// The unit square cap with edge 2, from corner 2 to corner 3, stripped of
/// its curve axis and, when `bulge` is given, sampled at `(0.5, 1 + bulge)`.
fn square_with_unproven_edge(bulge: Option<f64>) -> AsmHistoricalTopology {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes.remove(2);
    if let Some(bulge) = bulge {
        topology.edge_samples.push(AsmHistoricalEdgeSample {
            edge: 402,
            interior: Point3::new(0.5, 1.0 + bulge, 2.0),
            tangent: None,
        });
    }
    topology
}

#[test]
fn an_edge_without_a_proven_carrier_is_read_from_its_samples() {
    // The strict reader refuses what only samples support.
    assert!(face_cap(
        &xy_frame(),
        &square_with_unproven_edge(None),
        CAP,
        TOLERANCE
    )
    .is_none());
    // No axis and no sample off the chord: the chord.
    for bulge in [None, Some(0.0)] {
        let cap = face_cap_reading(
            &xy_frame(),
            &square_with_unproven_edge(bulge),
            CAP,
            TOLERANCE,
        )
        .unwrap();
        assert!(cap.assumed());
        assert_eq!(
            cap.measured
                .iter()
                .filter(|points| points.is_some())
                .count(),
            1
        );
        assert!(matches!(cap.pieces[2], super::Piece::Line(..)));
        assert!((polygon_area(&cap.loops[0]).abs() - 1.0).abs() < 1e-12);
    }

    // A sample off the chord: the arc through the ends and the sample.
    let cap = face_cap_reading(
        &xy_frame(),
        &square_with_unproven_edge(Some(0.25)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    let super::Piece::Arc { center, radius, .. } = cap.pieces[2] else {
        panic!("an arc piece");
    };
    // The circle through (1, 1), (0.5, 1.25) and (0, 1).
    assert!((center.u - 0.5).abs() < 1e-12 && (center.v - 0.625).abs() < 1e-12);
    assert!((radius - 0.625).abs() < 1e-12);
    let mut ring = Vec::new();
    cap.pieces[2].push(&mut ring);
    assert!(ring.iter().all(|point| point.v >= 1.0 - 1e-12));

    // An edge whose carrier is proven is not reported.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    assert!(!face_cap_reading(&xy_frame(), &topology, CAP, TOLERANCE)
        .unwrap()
        .assumed());
}

#[test]
fn rows_that_contradict_an_edge_reading_read_no_cap() {
    // A curve axis across the chord is not a line's, and no sample shows the
    // edge's curve.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes[2].direction = Vector3::new(0.0, 0.0, 1.0);
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());

    // A curve axis along the chord with a sample off it.
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    topology.edge_samples.push(AsmHistoricalEdgeSample {
        edge: 402,
        interior: Point3::new(0.5, 1.25, 2.0),
        tangent: None,
    });
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());

    // A curve axis in the cap plane with a sample off the chord is not a
    // circular arc's in the cap.
    // Edge 402 keeps its binding to curve 1402.
    let mut topology = square_with_unproven_edge(Some(0.25));
    topology.curve_axes.push(AsmHistoricalCurveAxis {
        curve: 1402,
        origin: Point3::new(0.5, 0.625, 2.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
    });
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());
}

#[test]
fn an_edge_with_two_rows_of_one_kind_proves_no_cap() {
    let corners = unit_square_corners();
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0)];
    let mut topology = loop_topology(&corners, &edges);
    straight_carriers(&mut topology);
    let axis = topology.curve_axes[2].clone();
    topology.curve_axes.push(axis);
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());

    let mut topology = loop_topology(&corners, &edges);
    straight_carriers(&mut topology);
    let curve = topology.edge_curves[2].clone();
    topology.edge_curves.push(curve);
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());

    let mut topology = loop_topology(&corners, &edges);
    straight_carriers(&mut topology);
    let point = topology.vertex_points[1].clone();
    topology.vertex_points.push(AsmHistoricalCarrierBinding {
        carrier: point.carrier + 5000,
        ..point
    });
    assert!(face_cap(&xy_frame(), &topology, CAP, TOLERANCE).is_none());
}

/// A cap bounded by one closed edge where it meets a cylinder of `radius`
/// about `center` along `axis`, with its seam at angle 0.
fn circle_topology(center: (f64, f64), radius: f64, axis: Vector3) -> AsmHistoricalTopology {
    let mut topology = loop_topology(&[(center.0 + radius, center.1)], &[(0, 0)]);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: 101,
        member_refs: vec![201],
    });
    topology.coedge_topology.push(AsmHistoricalCoedge {
        coedge: 350,
        owner_loop: 201,
        edge: 400,
        next: 0,
        previous: 0,
        radial_next: 0,
    });
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 101,
        carrier: 700,
    });
    topology.surface_cylinders.push(AsmHistoricalCylinder {
        surface: 700,
        origin: Point3::new(center.0, center.1, -5.0),
        axis,
        radius,
    });
    topology.edge_samples.push(AsmHistoricalEdgeSample {
        edge: 400,
        interior: Point3::new(center.0 - radius, center.1, 2.0),
        tangent: None,
    });
    topology
}

#[test]
fn a_closed_edge_is_a_circle_only_on_a_cylinder_about_the_cap_normal() {
    let cap = face_cap(
        &xy_frame(),
        &circle_topology((1.0, 1.0), 2.0, Vector3::new(0.0, 0.0, 1.0)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    let circle = std::f64::consts::PI * 4.0;
    assert!((polygon_area(&cap.loops[0]).abs() - circle).abs() < 1e-2 * circle);
    let [super::Piece::Arc {
        center,
        radius,
        sweep,
        ..
    }] = cap.pieces.as_slice()
    else {
        panic!("one arc piece");
    };
    assert!((center.u - 1.0).abs() < 1e-12 && (center.v - 1.0).abs() < 1e-12);
    assert!((radius - 2.0).abs() < 1e-12 && (sweep.abs() - TAU).abs() < 1e-12);
    assert!(face_cap(
        &xy_frame(),
        &circle_topology((1.0, 1.0), 2.0, Vector3::new(1.0, 0.0, 0.0)),
        CAP,
        TOLERANCE,
    )
    .is_none());
    // A neighbour face without a surface binding leaves the circle unproven.
    let mut unbound = circle_topology((1.0, 1.0), 2.0, Vector3::new(0.0, 0.0, 1.0));
    unbound
        .face_surfaces
        .retain(|binding| binding.entity != 101);
    assert!(face_cap(&xy_frame(), &unbound, CAP, TOLERANCE).is_none());
}

fn ctx() -> cadmpeg_core::decode::DecodeContext<'static> {
    cadmpeg_test_support::service_decode_context()
}

fn line(id: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        xy_sketch().id,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap(),
    )
}

fn circle(id: &str, center: (f64, f64), radius: f64) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        xy_sketch().id,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(center.0, center.1),
            radius: Length::new(radius).unwrap(),
        })
        .unwrap(),
    )
}

fn boundary_use(entity: &SketchEntity, range: [f64; 2]) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: entity.id().clone(),
        parameter_range: DirectedParameterRange::new(range).unwrap(),
        reversed: false,
    }
}

/// A region bounded by `entities` over `ranges`.
fn region(entities: &[SketchEntity], ranges: &[[f64; 2]]) -> AtomicSketchRegion {
    AtomicSketchRegion {
        boundary: entities
            .iter()
            .zip(ranges)
            .map(|(entity, range)| boundary_use(entity, *range))
            .collect(),
        polyline: Vec::new(),
        encloses_island: false,
        island_absence_assumed: false,
    }
}

fn sampled(region: &AtomicSketchRegion, point: (f64, f64)) -> Vec<Sampled<'_>> {
    vec![Sampled {
        region,
        point: Point2::new(point.0, point.1),
        nested: Vec::new(),
    }]
}

fn polygon(name: &str, corners: &[(f64, f64)]) -> Vec<SketchEntity> {
    (0..corners.len())
        .map(|k| {
            line(
                &format!("{name}-{k}"),
                corners[k],
                corners[(k + 1) % corners.len()],
            )
        })
        .collect()
}

#[test]
fn regions_and_cap_must_cover_each_others_outline() {
    let mut topology = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut topology);
    let cap = face_cap(&xy_frame(), &topology, CAP, TOLERANCE).unwrap();
    let whole_lines = [[0.0, 1.0]; 4];

    let same = polygon("same", &unit_square_corners());
    let same_region = region(&same, &whole_lines);
    assert_eq!(
        outlines_agree(
            &cap,
            &sampled(&same_region, (0.5, 0.5)),
            &[0],
            &same,
            TOLERANCE,
            &ctx()
        )
        .unwrap(),
        Agreement::Exact
    );

    // Equal area and a shared interior point, but another outline.
    let shifted_corners: Vec<(f64, f64)> = unit_square_corners()
        .iter()
        .map(|(u, v)| (u + 0.4, *v))
        .collect();
    let shifted = polygon("shifted", &shifted_corners);
    let shifted_region = region(&shifted, &whole_lines);
    assert_eq!(
        outlines_agree(
            &cap,
            &sampled(&shifted_region, (0.5, 0.5)),
            &[0],
            &shifted,
            TOLERANCE,
            &ctx()
        )
        .unwrap(),
        Agreement::No
    );
}

#[test]
fn a_polygon_inscribed_in_a_small_circular_cap_does_not_match_it() {
    // A circular cap of radius 0.01 with its seam at a corner of the
    // inscribed square: the square's corners all lie on the circle and its
    // area differs by about 0.000114.
    let radius = 0.01;
    let cap = face_cap(
        &xy_frame(),
        &circle_topology((0.0, 0.0), radius, Vector3::new(0.0, 0.0, 1.0)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    let square = polygon(
        "inscribed",
        &[(radius, 0.0), (0.0, radius), (-radius, 0.0), (0.0, -radius)],
    );
    let square_region = region(&square, &[[0.0, 1.0]; 4]);
    assert_eq!(
        outlines_agree(
            &cap,
            &sampled(&square_region, (0.0, 0.0)),
            &[0],
            &square,
            TOLERANCE,
            &ctx()
        )
        .unwrap(),
        Agreement::No
    );

    // The circle itself matches, whichever angle its own seam is at.
    let disc = [circle("disc", (0.0, 0.0), radius)];
    for start in [0.0, 1.0] {
        let disc_region = region(&disc, &[[start, start + TAU]]);
        assert_eq!(
            outlines_agree(
                &cap,
                &sampled(&disc_region, (0.0, 0.0)),
                &[0],
                &disc,
                TOLERANCE,
                &ctx()
            )
            .unwrap(),
            Agreement::Exact
        );
    }
}

#[test]
fn a_cut_plane_compares_the_faces_it_deleted_with_the_faces_it_inserted() {
    // The cut deletes a 2 x 1 face and inserts a 1 x 1 face on the same
    // plane, so it removed the right half.
    let mut before = loop_topology(
        &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)],
        &[(0, 1), (1, 2), (2, 3), (3, 0)],
    );
    straight_carriers(&mut before);
    let mut after = loop_topology(&unit_square_corners(), &[(0, 1), (1, 2), (2, 3), (3, 0)]);
    straight_carriers(&mut after);
    for topology in [&mut before, &mut after] {
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: CAP,
            carrier: 800,
        });
        topology
            .surface_planes
            .push(crate::history_records::AsmHistoricalPlane {
                surface: 800,
                origin: Point3::new(0.0, 0.0, 2.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            });
    }
    let faces = crate::history_records::AsmHistoricalEntityDelta {
        inserted: vec![CAP],
        deleted: vec![CAP],
        updated: Vec::new(),
    };
    let planes = changed_planes(&xy_sketch(), (&before, &after), &faces, TOLERANCE).unwrap();
    let [plane] = planes.as_slice() else {
        panic!("one changed plane");
    };
    assert!((plane.offset - 2.0).abs() < 1e-12);
    let held = |point: Point2| plane.before.iter().any(|cap| cap.contains(point));
    let holds = |point: Point2| plane.after.iter().any(|cap| cap.contains(point));
    assert!(held(Point2::new(1.5, 0.5)) && !holds(Point2::new(1.5, 0.5)));
    assert!(held(Point2::new(0.5, 0.5)) && holds(Point2::new(0.5, 0.5)));

    // A changed face whose surface has two plane rows leaves the planes unread.
    let mut twice = before.clone();
    twice
        .surface_planes
        .push(crate::history_records::AsmHistoricalPlane {
            surface: 800,
            origin: Point3::new(0.0, 0.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        });
    assert!(changed_planes(&xy_sketch(), (&twice, &after), &faces, TOLERANCE).is_none());

    // A changed face on the plane without a proven cap leaves the plane out.
    after.curve_axes.remove(1);
    assert!(
        changed_planes(&xy_sketch(), (&before, &after), &faces, TOLERANCE)
            .unwrap()
            .is_empty()
    );
}

/// A planar face parallel to the XY sketch: its slot, its height, and its
/// loops as corner rings.
type PlaneFace = (i64, f64, Vec<Vec<(f64, f64)>>);

/// Faces whose edges all have straight carriers.
fn plane_faces(faces: &[PlaneFace]) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology::default();
    let mut next = 10_000;
    let mut mint = || {
        next += 1;
        next
    };
    for (face, height, loops) in faces {
        let surface = mint();
        topology.faces.push(*face);
        topology.surfaces.push(surface);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: *face,
            carrier: surface,
        });
        topology
            .surface_planes
            .push(crate::history_records::AsmHistoricalPlane {
                surface,
                origin: Point3::new(0.0, 0.0, *height),
                normal: Vector3::new(0.0, 0.0, 1.0),
            });
        let mut face_loops = Vec::new();
        for ring in loops {
            let loop_ = mint();
            face_loops.push(loop_);
            topology.loops.push(loop_);
            let vertices: Vec<i64> = ring
                .iter()
                .map(|(u, v)| {
                    let (vertex, point) = (mint(), mint());
                    topology.vertices.push(vertex);
                    topology.points.push(point);
                    topology.vertex_points.push(AsmHistoricalCarrierBinding {
                        entity: vertex,
                        carrier: point,
                    });
                    topology.point_positions.push(AsmHistoricalPoint {
                        point,
                        position: Point3::new(*u, *v, *height),
                    });
                    vertex
                })
                .collect();
            let mut coedges = Vec::new();
            for k in 0..ring.len() {
                let (edge, coedge, curve) = (mint(), mint(), mint());
                let (start, end) = (ring[k], ring[(k + 1) % ring.len()]);
                topology.edges.push(edge);
                topology.coedges.push(coedge);
                topology.curves.push(curve);
                coedges.push(coedge);
                topology.coedge_topology.push(AsmHistoricalCoedge {
                    coedge,
                    owner_loop: loop_,
                    edge,
                    next: 0,
                    previous: 0,
                    radial_next: 0,
                });
                topology.edge_vertices.push(AsmHistoricalEdge {
                    edge,
                    start_vertex: vertices[k],
                    end_vertex: vertices[(k + 1) % ring.len()],
                });
                topology
                    .edge_curves
                    .push(AsmHistoricalOptionalCarrierBinding {
                        entity: edge,
                        carrier: Some(curve),
                    });
                topology.curve_axes.push(AsmHistoricalCurveAxis {
                    curve,
                    origin: Point3::new(start.0, start.1, *height),
                    direction: Vector3::new(end.0 - start.0, end.1 - start.1, 0.0),
                });
            }
            topology.loop_coedges.push(AsmHistoricalRelation {
                owner_ref: loop_,
                member_refs: coedges,
            });
        }
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: *face,
            member_refs: face_loops,
        });
    }
    topology
}

/// A history whose state 2 follows state 1.
fn cut_history(
    before: AsmHistoricalTopology,
    after: AsmHistoricalTopology,
) -> crate::history_records::AsmHistory {
    let state = |state_id: i64, topology| {
        (
            crate::history_records::AsmDeltaState {
                id: format!("synthetic:test:state#{state_id}"),
                parent: "synthetic:test:history".into(),
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
    crate::test_support::history::history(
        "synthetic:test:history".into(),
        0,
        None,
        false,
        vec![state(2, after), state(1, before)],
    )
}

/// A 20 x 20 window centred on the origin, braced by two bars along its
/// diagonals, and the corners of its four openings.
struct BracedWindow {
    sketch: Sketch,
    entities: Vec<cadmpeg_ir::sketches::SketchEntity>,
    openings: [Vec<(f64, f64)>; 4],
}

fn braced_window() -> BracedWindow {
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition,
    };
    let mut sketch = xy_sketch();
    let lines = [
        ("bottom", (-10.0, -10.0), (10.0, -10.0)),
        ("right", (10.0, -10.0), (10.0, 10.0)),
        ("top", (10.0, 10.0), (-10.0, 10.0)),
        ("left", (-10.0, 10.0), (-10.0, -10.0)),
        ("rising-upper", (-10.0, -8.0), (8.0, 10.0)),
        ("rising-lower", (-8.0, -10.0), (10.0, 8.0)),
        ("falling-upper", (-8.0, 10.0), (10.0, -8.0)),
        ("falling-lower", (-10.0, 8.0), (8.0, -10.0)),
    ];
    let entities: Vec<SketchEntity> = lines
        .iter()
        .map(|(name, start, end)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                })
                .unwrap(),
            )
        })
        .collect();
    // One stored loop that crosses itself and names every curve.
    let stored = entities
        .iter()
        .map(|entity| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false,
        })
        .collect::<Vec<_>>();
    sketch.profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(vec![stored]).unwrap();
    BracedWindow {
        sketch,
        entities,
        openings: [
            vec![(-8.0, -10.0), (8.0, -10.0), (0.0, -2.0)],
            vec![(10.0, -8.0), (10.0, 8.0), (2.0, 0.0)],
            vec![(8.0, 10.0), (-8.0, 10.0), (0.0, 2.0)],
            vec![(-10.0, 8.0), (-10.0, -8.0), (-2.0, 0.0)],
        ],
    }
}

/// The regions selected when the `cut` openings of `window` are cut through
/// a 40 x 40 plate of thickness 5 whose top face is the sketch plane.
fn cut_selection(
    window: &BracedWindow,
    cut: &[usize],
    work: &crate::design::geometry::ProfileGeometryWork,
) -> Option<super::ResolvedProfileSelection> {
    let plate = vec![(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)];
    let mut holed = vec![plate.clone()];
    holed.extend(cut.iter().map(|index| window.openings[*index].clone()));
    let before = plane_faces(&[(1, 0.0, vec![plate.clone()]), (2, -5.0, vec![plate])]);
    let after = plane_faces(&[(3, 0.0, holed.clone()), (4, -5.0, holed)]);
    let history = cut_history(before, after);
    let ctx = cadmpeg_test_support::service_decode_context();
    let resolution = super::super::ScopedExtrudeProfileResolution {
        entities: &window.entities,
        spatial_entities: &[],
        histories: std::slice::from_ref(&history),
        linear_tolerance: TOLERANCE,
        angular_tolerance: 1e-9,
        profile_work: work,
        ctx: &ctx,
    };
    super::removed_profile_selection(
        &window.sketch,
        resolution,
        2,
        1,
        Some(&super::ResolvedProfileSelection::Loops(vec![0])),
    )
    .unwrap()
}

#[test]
fn a_cut_through_an_x_braced_window_selects_its_four_openings() {
    use crate::loss::F3dLossCode;
    let window = braced_window();
    let work = crate::design::geometry::ProfileGeometryWork::new();
    let Some(super::ResolvedProfileSelection::Regions(regions)) =
        cut_selection(&window, &[0, 1, 2, 3], &work)
    else {
        panic!("sketch regions");
    };
    assert_eq!(regions.len(), 4);
    let mut sides = Vec::new();
    for region in &regions {
        let cadmpeg_ir::features::SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        // Each opening is bounded by one window side and one line of each bar.
        let names: Vec<String> = outer_boundary
            .iter()
            .map(|use_| {
                let id = use_.entity.to_string();
                id.rsplit('#').next().unwrap().to_owned()
            })
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        let count = |prefix: &str| names.iter().filter(|name| name.starts_with(prefix)).count();
        assert_eq!((count("rising"), count("falling")), (1, 1), "{names:?}");
        sides.extend(
            names
                .into_iter()
                .filter(|name| ["bottom", "right", "top", "left"].contains(&name.as_str())),
        );
    }
    sides.sort();
    assert_eq!(sides, ["bottom", "left", "right", "top"]);
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileCutRegionsAssumed.kind()
    );
    assert!(loss.message.starts_with("1 extrude profile selection(s)"));
}

#[test]
fn a_cut_that_leaves_stored_profile_curves_unused_selects_no_regions() {
    let window = braced_window();
    let work = crate::design::geometry::ProfileGeometryWork::new();
    // The bottom and top openings alone leave the left and right sides
    // bounding no cut region.
    assert!(cut_selection(&window, &[0, 2], &work).is_none());
    work.finish_selection();
    assert!(work.losses().is_empty());
}

fn ellipse(id: &str, major: f64, minor: f64) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        xy_sketch().id,
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(0.0, 0.0),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(major).unwrap(),
                minor_radius: Length::new(minor).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    )
}

#[test]
fn a_curve_that_is_not_a_line_or_arc_agrees_with_its_cap_at_samples() {
    // A half-ellipse cap over (-2, 0)..(2, 0): a proven straight edge back
    // along the major axis, and an unproven edge sampled at the top.
    let mut topology = loop_topology(&[(2.0, 0.0), (-2.0, 0.0)], &[(0, 1), (1, 0)]);
    straight_carriers(&mut topology);
    topology.curve_axes.remove(0);
    topology.edge_curves.remove(0);
    topology.edge_samples.push(AsmHistoricalEdgeSample {
        edge: 400,
        interior: Point3::new(0.0, 1.0, 2.0),
        tangent: None,
    });
    let cap = face_cap_reading(&xy_frame(), &topology, CAP, TOLERANCE).unwrap();
    assert!(cap.assumed());

    let agreement = |minor: f64| {
        let entities = [
            ellipse("half", 2.0, minor),
            line("chord", (-2.0, 0.0), (2.0, 0.0)),
        ];
        let half = region(&entities, &[[0.0, std::f64::consts::PI], [0.0, 1.0]]);
        outlines_agree(
            &cap,
            &sampled(&half, (0.0, 0.5)),
            &[0],
            &entities,
            TOLERANCE,
            &ctx(),
        )
        .unwrap()
    };
    assert_eq!(agreement(1.0), Agreement::Sampled);
    // The cap edge's top sample lies off a fuller ellipse.
    assert_eq!(agreement(1.5), Agreement::No);
}

#[test]
fn assumed_cap_readings_report_one_loss_per_kind_counting_selections() {
    use crate::loss::F3dLossCode;
    let work = crate::design::geometry::ProfileGeometryWork::new();
    work.note_cap_outline_assumed();
    work.note_cap_carrier_assumed();
    work.note_cap_carrier_assumed();
    work.finish_selection();
    work.note_cap_carrier_assumed();
    work.finish_selection();
    work.finish_selection();
    let losses = work.losses();
    let [outline, carrier] = losses.as_slice() else {
        panic!("expected two losses, got {losses:?}");
    };
    assert_eq!(
        outline.code,
        F3dLossCode::FeatureProfileCapOutlineAssumed.kind()
    );
    assert!(outline
        .message
        .starts_with("1 extrude profile selection(s)"));
    assert_eq!(
        carrier.code,
        F3dLossCode::FeatureProfileCapCarrierAssumed.kind()
    );
    assert!(carrier
        .message
        .starts_with("2 extrude profile selection(s)"));
}

#[test]
fn a_face_polygon_inscribed_in_a_small_circular_cap_does_not_cover_it() {
    let radius = 0.01;
    let cap = face_cap(
        &xy_frame(),
        &circle_topology((0.0, 0.0), radius, Vector3::new(0.0, 0.0, 1.0)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    let same = face_cap(
        &xy_frame(),
        &circle_topology((0.0, 0.0), radius, Vector3::new(0.0, 0.0, 1.0)),
        CAP,
        TOLERANCE,
    )
    .unwrap();
    assert!(face_outlines_cover_caps(
        std::slice::from_ref(&cap),
        std::slice::from_ref(&same),
        TOLERANCE
    ));
    let mut inscribed = loop_topology(
        &[(radius, 0.0), (0.0, radius), (-radius, 0.0), (0.0, -radius)],
        &[(0, 1), (1, 2), (2, 3), (3, 0)],
    );
    straight_carriers(&mut inscribed);
    let square = face_cap(&xy_frame(), &inscribed, CAP, TOLERANCE).unwrap();
    assert!(!face_outlines_cover_caps(
        std::slice::from_ref(&cap),
        std::slice::from_ref(&square),
        TOLERANCE
    ));
}

#[test]
fn two_faces_cover_a_cap_by_their_shared_outline_and_not_their_shared_edge() {
    let cap = |corners: &[(f64, f64)]| {
        let mut topology = loop_topology(corners, &[(0, 1), (1, 2), (2, 3), (3, 0)]);
        straight_carriers(&mut topology);
        face_cap(&xy_frame(), &topology, CAP, TOLERANCE).unwrap()
    };
    let whole = cap(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)]);
    let left = cap(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    let right = cap(&[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)]);
    assert!(face_outlines_cover_caps(
        std::slice::from_ref(&whole),
        &[left, right],
        TOLERANCE
    ));
    let left = cap(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    assert!(!face_outlines_cover_caps(
        std::slice::from_ref(&whole),
        std::slice::from_ref(&left),
        TOLERANCE
    ));
}

#[test]
fn a_far_face_over_material_the_plane_already_held_selects_nothing() {
    let window = braced_window();
    let opening = vec![window.openings[0].clone()];
    let after = || plane_faces(&[(3, 0.0, opening.clone()), (4, -5.0, opening.clone())]);
    let select = |before| {
        let history = cut_history(before, after());
        let ctx = cadmpeg_test_support::service_decode_context();
        let work = crate::design::geometry::ProfileGeometryWork::new();
        let resolution = super::super::ScopedExtrudeProfileResolution {
            entities: &window.entities,
            spatial_entities: &[],
            histories: std::slice::from_ref(&history),
            linear_tolerance: TOLERANCE,
            angular_tolerance: 1e-9,
            profile_work: &work,
            ctx: &ctx,
        };
        super::cap_profile_selection(&window.sketch, resolution, 2, 1, None).unwrap()
    };
    // New material on an empty far plane is the selection.
    assert!(select(plane_faces(&[])).is_some());
    // The same far face over material the plane already held is what a cut left.
    assert!(select(plane_faces(&[
        (1, 0.0, opening.clone()),
        (2, -5.0, opening.clone()),
    ]))
    .is_none());
}

#[test]
fn a_cut_whose_regions_use_curves_outside_the_stored_profile_takes_the_proven_regions() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::sketches::SketchEntityUse;
    let mut window = braced_window();
    // The stored profile is the window frame alone; the opening a cut removes
    // is also bounded by the two bars.
    let frame = window
        .entities
        .iter()
        .filter(|entity| {
            let id = entity.id().to_string();
            ["bottom", "right", "top", "left"]
                .iter()
                .any(|side| id.ends_with(&format!("#{side}")))
        })
        .map(|entity| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false,
        })
        .collect::<Vec<_>>();
    assert_eq!(frame.len(), 4);
    window.sketch.profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(vec![frame]).unwrap();
    let plate = vec![(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)];
    let holed = vec![plate.clone(), window.openings[0].clone()];
    let before = plane_faces(&[(1, 0.0, vec![plate.clone()]), (2, -5.0, vec![plate])]);
    let after = plane_faces(&[(3, 0.0, holed.clone()), (4, -5.0, holed)]);
    let history = cut_history(before, after);
    let ctx = cadmpeg_test_support::service_decode_context();
    let work = crate::design::geometry::ProfileGeometryWork::new();
    let resolution = super::super::ScopedExtrudeProfileResolution {
        entities: &window.entities,
        spatial_entities: &[],
        histories: std::slice::from_ref(&history),
        linear_tolerance: TOLERANCE,
        angular_tolerance: 1e-9,
        profile_work: &work,
        ctx: &ctx,
    };
    let current = super::ResolvedProfileSelection::Loops(vec![0]);
    // The reading replaces only a stored profile read as loops.
    assert!(
        super::removed_regions_beyond_profile(&window.sketch, resolution, 2, 1, None)
            .unwrap()
            .is_none()
    );
    assert!(
        super::removed_profile_selection(&window.sketch, resolution, 2, 1, Some(&current),)
            .unwrap()
            .is_none()
    );
    let Some(super::ResolvedProfileSelection::Regions(regions)) =
        super::removed_regions_beyond_profile(&window.sketch, resolution, 2, 1, Some(&current))
            .unwrap()
    else {
        panic!("the removed opening");
    };
    assert_eq!(regions.len(), 1);
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileCutRegionsAssumed.kind()
    );
    assert!(loss.message.contains("curves outside the stored profile"));
}
