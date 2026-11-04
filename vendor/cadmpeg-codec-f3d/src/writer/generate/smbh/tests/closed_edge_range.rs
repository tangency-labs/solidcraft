// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::TAU;

use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, EdgeCarrier};
use cadmpeg_test_support::solids::closed_edge;

use super::super::closed_edge_traversal;

fn solved(geometry: SolvedCurveGeometry) -> CurveGeometry {
    CurveGeometry::Solved(geometry)
}

/// The circle of radius 10 about `+z` whose parameter 0 lies at `(10, 0, 0)`.
fn circle() -> CurveGeometry {
    solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            10.0,
        )
        .expect("circle"),
    ))
}

/// The square path from `(0,0)` through `(1,0) (1,1) (0,1)` to `last` over
/// the knot domain `[0, 4]`.
fn square_path(periodic: bool, last: Point3) -> CurveGeometry {
    solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                last,
            ],
            None,
            periodic,
        )
        .expect("degree-1 path"),
    ))
}

fn square_loop(periodic: bool) -> CurveGeometry {
    square_path(periodic, Point3::new(0.0, 0.0, 0.0))
}

fn traversal(edge: &Edge, carrier: &CurveGeometry, vertex: Point3) -> Option<[f64; 2]> {
    closed_edge_traversal(edge, carrier, vertex, 0.0).ok()
}

fn assert_refused(edge: &Edge, carrier: &CurveGeometry, vertex: Point3) {
    assert!(
        matches!(
            closed_edge_traversal(edge, carrier, vertex, 0.0),
            Err(CodecError::NotImplemented(_))
        ),
        "an edge with no closed traversal is refused"
    );
}

#[test]
fn closed_circle_edge_traverses_one_revolution() {
    let edge = closed_edge("circle", "circle", "v0");
    assert_eq!(
        traversal(&edge, &circle(), Point3::new(10.0, 0.0, 0.0)),
        Some([0.0, TAU])
    );
}

#[test]
fn closed_ellipse_edge_traverses_one_revolution() {
    let edge = closed_edge("ellipse", "ellipse", "v0");
    let ellipse = solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            6.0,
            2.0,
        )
        .expect("ellipse"),
    ));
    assert_eq!(
        traversal(&edge, &ellipse, Point3::new(0.0, 0.0, 6.0)),
        Some([0.0, TAU])
    );
}

#[test]
fn closed_periodic_nurbs_edge_traverses_its_knot_domain() {
    let edge = closed_edge("loop", "loop", "v0");
    assert_eq!(
        traversal(&edge, &square_loop(true), Point3::new(0.0, 0.0, 0.0)),
        Some([0.0, 4.0])
    );
}

#[test]
fn closed_clamped_nurbs_edge_traverses_its_knot_domain() {
    let edge = closed_edge("loop", "loop", "v0");
    assert_eq!(
        traversal(&edge, &square_loop(false), Point3::new(0.0, 0.0, 0.0)),
        Some([0.0, 4.0])
    );
}

#[test]
fn closed_edge_on_an_open_nurbs_is_refused() {
    let edge = closed_edge("path", "path", "v0");
    let path = square_path(false, Point3::new(0.0, 0.5, 0.0));
    assert_refused(&edge, &path, Point3::new(0.0, 0.0, 0.0));
}

#[test]
fn closed_edge_off_the_domain_start_is_refused() {
    let edge = closed_edge("loop", "loop", "v0");
    assert_refused(&edge, &square_loop(false), Point3::new(1.0, 1.0, 0.0));
    assert_refused(&edge, &circle(), Point3::new(-10.0, 0.0, 0.0));
}

#[test]
fn edge_tolerance_admits_a_vertex_near_the_domain_start() {
    let edge = closed_edge("circle", "circle", "v0");
    let vertex = Point3::new(10.0, 0.001, 0.0);
    assert_refused(&edge, &circle(), vertex);
    assert_eq!(
        closed_edge_traversal(&edge, &circle(), vertex, 0.01).expect("the circle evaluates"),
        [0.0, TAU]
    );
}

#[test]
fn open_edge_or_open_carrier_is_refused() {
    let mut open = closed_edge("circle", "circle", "v0");
    open.end = cadmpeg_ir::ids::VertexId::mint("test:model:vertex#v1".to_owned())
        .expect("identity grammar");
    assert_refused(&open, &circle(), Point3::new(10.0, 0.0, 0.0));
    let line = solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).expect("line"),
    ));
    let mut on_line = closed_edge("line", "line", "v0");
    on_line.carrier =
        EdgeCarrier::new(on_line.curve().cloned(), None).expect("a curve with no range");
    assert_refused(&on_line, &line, Point3::new(0.0, 0.0, 0.0));
}
