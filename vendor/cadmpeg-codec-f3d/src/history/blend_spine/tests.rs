// SPDX-License-Identifier: Apache-2.0

use super::{blend_spines, edge_geometry, BlendSpine, EdgeGeometry};
use crate::history_records::{
    AsmHistoricalCurveAxis, AsmHistoricalCylinder, AsmHistoricalEdgeSample,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalSurfaceAxis, AsmHistoricalSurfaceRadius,
    AsmHistoricalTopology,
};
use cadmpeg_ir::math::{Point3, Vector3};

fn point(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn vector(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

fn edge(origin: Point3, axis: Vector3, tangent: Vector3) -> EdgeGeometry {
    EdgeGeometry {
        axis: Some((origin, axis)),
        tangent: Some(tangent),
    }
}

fn along_z() -> BlendSpine {
    BlendSpine::Line {
        direction: vector(0.0, 0.0, 1.0),
    }
}

fn around_z() -> BlendSpine {
    BlendSpine::Circle {
        origin: point(1.0, 2.0, 0.0),
        direction: vector(0.0, 0.0, 1.0),
    }
}

#[test]
fn a_cylinder_blend_replaces_a_line_along_its_axis() {
    let line = edge(
        point(0.0, 30.0, 0.0),
        vector(0.0, 0.0, -1.0),
        vector(0.0, 0.0, -1.0),
    );
    assert!(along_z().admits(line));
}

#[test]
fn a_cylinder_blend_does_not_replace_a_cap_edge_across_its_axis() {
    let cap = edge(
        point(0.0, 30.0, 0.0),
        vector(0.0, -1.0, 0.0),
        vector(0.0, -1.0, 0.0),
    );
    assert!(!along_z().admits(cap));
}

#[test]
fn a_cylinder_blend_does_not_replace_a_circle_about_a_parallel_axis() {
    let circle = edge(
        point(5.0, 0.0, 0.0),
        vector(0.0, 0.0, 1.0),
        vector(1.0, 0.0, 0.0),
    );
    assert!(!along_z().admits(circle));
}

#[test]
fn a_cylinder_blend_does_not_replace_a_circle_whose_midpoint_runs_along_its_axis() {
    let circle = edge(
        point(5.0, 0.0, 0.0),
        vector(1.0, 0.0, 0.0),
        vector(0.0, 0.0, 1.0),
    );
    assert!(!along_z().admits(circle));
}

#[test]
fn a_torus_blend_replaces_a_circle_around_its_axis() {
    let rim = edge(
        point(1.0, 2.0, 7.0),
        vector(0.0, 0.0, -1.0),
        vector(1.0, 0.0, 0.0),
    );
    assert!(around_z().admits(rim));
}

#[test]
fn a_torus_blend_does_not_replace_a_circle_off_its_axis_line() {
    let rim = edge(
        point(1.5, 2.0, 7.0),
        vector(0.0, 0.0, 1.0),
        vector(1.0, 0.0, 0.0),
    );
    assert!(!around_z().admits(rim));
}

#[test]
fn a_torus_blend_does_not_replace_a_tilted_circle() {
    let rim = edge(
        point(1.0, 2.0, 7.0),
        vector(0.0, 1.0, 0.0),
        vector(1.0, 0.0, 0.0),
    );
    assert!(!around_z().admits(rim));
}

#[test]
fn a_torus_blend_does_not_replace_a_line_on_its_axis() {
    let line = edge(
        point(1.0, 2.0, 7.0),
        vector(0.0, 0.0, 1.0),
        vector(0.0, 0.0, 1.0),
    );
    assert!(!around_z().admits(line));
}

#[test]
fn an_edge_without_an_axis_row_or_a_tangent_is_not_replaced() {
    let line = edge(
        point(0.0, 0.0, 0.0),
        vector(0.0, 0.0, 1.0),
        vector(0.0, 0.0, 1.0),
    );
    for spine in [along_z(), around_z()] {
        assert!(!spine.admits(EdgeGeometry { axis: None, ..line }));
        assert!(!spine.admits(EdgeGeometry {
            tangent: None,
            ..line
        }));
        assert!(!spine.admits(EdgeGeometry::default()));
    }
}

fn axis(surface: i64) -> AsmHistoricalSurfaceAxis {
    AsmHistoricalSurfaceAxis {
        surface,
        origin: point(1.0, 2.0, 0.0),
        direction: vector(0.0, 0.0, 2.0),
    }
}

fn radius(surface: i64) -> AsmHistoricalSurfaceRadius {
    AsmHistoricalSurfaceRadius {
        surface,
        radius: 2.0,
    }
}

fn cylinder(surface: i64) -> AsmHistoricalCylinder {
    AsmHistoricalCylinder {
        surface,
        origin: point(1.0, 2.0, 0.0),
        axis: vector(0.0, 0.0, 2.0),
        radius: 2.0,
    }
}

#[test]
fn a_cylinder_row_states_a_line_spine_and_an_axis_with_a_radius_states_a_circle() {
    let result = AsmHistoricalTopology {
        surfaces: vec![10, 11, 12, 13, 14],
        surface_cylinders: vec![cylinder(10)],
        surface_axes: vec![axis(10), axis(11), axis(12)],
        surface_radii: vec![radius(10), radius(11), radius(13)],
        ..AsmHistoricalTopology::default()
    };
    let spines = blend_spines(&result);
    assert_eq!(spines.get(&10), Some(&along_z()));
    assert_eq!(spines.get(&11), Some(&around_z()));
    assert_eq!(spines.get(&12), None, "an axis without a radius is a cone");
    assert_eq!(
        spines.get(&13),
        None,
        "a radius without an axis is a sphere"
    );
    assert_eq!(spines.get(&14), None);
}

#[test]
fn a_surface_with_two_rows_states_no_spine() {
    let result = AsmHistoricalTopology {
        surfaces: vec![10, 11, 12],
        surface_cylinders: vec![cylinder(10), cylinder(10)],
        surface_axes: vec![axis(11), axis(11), axis(12)],
        surface_radii: vec![radius(11), radius(12), radius(12)],
        ..AsmHistoricalTopology::default()
    };
    assert!(blend_spines(&result).is_empty());
}

#[test]
fn a_zero_axis_states_no_spine() {
    let mut zero_cylinder = cylinder(10);
    zero_cylinder.axis = vector(0.0, 0.0, 0.0);
    let mut zero_axis = axis(11);
    zero_axis.direction = vector(0.0, 0.0, 0.0);
    let result = AsmHistoricalTopology {
        surfaces: vec![10, 11],
        surface_cylinders: vec![zero_cylinder],
        surface_axes: vec![zero_axis],
        surface_radii: vec![radius(11)],
        ..AsmHistoricalTopology::default()
    };
    assert!(blend_spines(&result).is_empty());
}

#[test]
fn an_edge_reads_the_axis_row_of_its_curve_and_its_midpoint_tangent() {
    let binding = |entity, carrier| AsmHistoricalOptionalCarrierBinding { entity, carrier };
    let curve_axis = |curve| AsmHistoricalCurveAxis {
        curve,
        origin: point(0.0, 30.0, 0.0),
        direction: vector(0.0, 0.0, 1.0),
    };
    let sample = |edge| AsmHistoricalEdgeSample {
        edge,
        interior: point(0.0, 30.0, 10.0),
        tangent: Some(vector(0.0, 0.0, 1.0)),
    };
    let preceding = AsmHistoricalTopology {
        edges: vec![1, 2, 3, 4],
        edge_curves: vec![
            binding(1, Some(100)),
            binding(2, Some(100)),
            binding(2, Some(101)),
            binding(3, None),
            binding(4, Some(102)),
        ],
        curve_axes: vec![curve_axis(100), curve_axis(102), curve_axis(102)],
        edge_samples: vec![sample(1), sample(2), sample(3), sample(4)],
        ..AsmHistoricalTopology::default()
    };
    let geometry = edge_geometry(&preceding);
    assert!(along_z().admits(geometry[&1]));
    for edge in [2, 3, 4] {
        assert_eq!(geometry[&edge].axis, None, "edge {edge}");
        assert_eq!(geometry[&edge].tangent, Some(vector(0.0, 0.0, 1.0)));
    }
}
