// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
//! Closed bodies that use an edge four times or a collapsed edge once.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId, PointId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    BodyKind, Coedge, Edge, EdgeCarrier, FaceLoops, Loop, LoopBoundary, LoopRing, Point, Sense,
    Vertex,
};
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::polyhedron::{box_faces, edge_touching_cubes, polyhedron};
use std::io::Cursor;

use crate::F3dCodec;

fn source_less_round_trip(ir: &CadIr) -> cadmpeg_ir::codec::DecodeResult {
    let checked = cadmpeg_ir::validate::validate_neutral(ir, Vec::new()).unwrap();
    assert!(checked.is_ok(), "fixture findings: {:?}", checked.findings);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less solid encode");
    F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less solid round trip")
}

#[test]
fn a_solid_with_a_four_use_edge_round_trips_as_a_solid() {
    let round_trip = source_less_round_trip(&edge_touching_cubes().unwrap());

    let model = &round_trip.ir().model;
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(model.faces.len(), 12);
    assert_eq!(model.edges.len(), 23);
    assert!(model.edges.iter().any(|edge| model
        .coedges
        .iter()
        .filter(|c| c.edge == edge.id)
        .count()
        == 4));
}

#[test]
fn a_solid_with_a_collapsed_edge_loop_round_trips_as_a_solid() {
    let corners = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
    let faces = box_faces([0, 1, 2, 3, 4, 5, 6, 7]);
    let faces = faces.iter().map(<[usize; 4]>::as_slice).collect::<Vec<_>>();
    let mut ir = polyhedron(&corners, &faces).unwrap();

    let point = PointId::mint("test:model:point#apex").unwrap();
    let vertex = VertexId::mint("test:model:vertex#apex").unwrap();
    let edge = EdgeId::mint("test:model:edge#apex").unwrap();
    let coedge = CoedgeId::mint("test:model:coedge#apex").unwrap();
    let apex_loop = LoopId::mint("test:model:loop#apex").unwrap();
    let top = &mut ir.model.faces[1];
    let outer = top.loops.iter().next().unwrap().clone();
    top.loops = FaceLoops::unspecified(vec![outer, apex_loop.clone()]);
    let top = top.id.clone();
    ir.model.points.push(Point::new(
        point.clone(),
        FinitePoint3::new(Point3::new(5.0, 5.0, 10.0)).unwrap(),
        None,
    ));
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: edge.clone(),
        carrier: EdgeCarrier::Free,
        start: vertex.clone(),
        end: vertex,
        tolerance: None,
    });
    ir.model.coedges.push(Coedge {
        id: coedge.clone(),
        owner_loop: apex_loop.clone(),
        edge,
        radial_next: coedge.clone(),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    });
    ir.model.loops.push(Loop {
        id: apex_loop,
        face: top,
        boundary: LoopBoundary::Ring(LoopRing::new(vec![coedge], Vec::new()).unwrap()),
    });
    ir.finalize();

    let round_trip = source_less_round_trip(&ir);

    let model = &round_trip.ir().model;
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, BodyKind::Solid);
    assert_eq!(model.faces.len(), 6);
    assert_eq!(model.edges.len(), 13);
    let collapsed = model
        .edges
        .iter()
        .find(|edge| edge.start == edge.end)
        .unwrap();
    let carrier = collapsed.curve().unwrap();
    assert!(model.curves.iter().any(|curve| curve.id == *carrier
        && matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
        )));
}
