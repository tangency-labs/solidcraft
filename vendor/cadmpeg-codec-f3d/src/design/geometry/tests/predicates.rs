// SPDX-License-Identifier: Apache-2.0

use super::super::*;
#[test]
fn numerical_audit_area_keeps_translated_square() {
    for offset in [0., 1e8] {
        let p = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .map(|p| Point2::new(offset + p[0], offset + p[1]));
        assert_eq!(signed_polygon_area(&p), 1.);
    }
}
#[test]
fn numerical_audit_line_arc_does_not_keep_outside_root() {
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: super::positive_radius(0.001),
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    for start in [-1., -1e20] {
        assert_eq!(
            line_arc_intersection_points((Point2::new(start, 0.), Point2::new(0., 0.)), &arc)
                .map(|points| points.into_iter().flatten().collect::<Vec<_>>()),
            Some(vec![Point2::new(-0.001, 0.)])
        );
    }
}

const EPS_CHORD_DISTANCE: f64 = 1.0e-12;

#[test]
fn chord_arc_distance_takes_the_nearest_end_or_normal_pair() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

    let unit_arc = |start_angle, end_angle| ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: super::positive_radius(1.),
        start_angle,
        end_angle,
    };
    let below = (Point2::new(-2., -3.), Point2::new(2., -3.));
    for (chord, arc, expected) in [
        (below, unit_arc(0., TAU), 2.),
        (below, unit_arc(0., PI), 3.),
        (below, unit_arc(PI, 0.), 3.),
        (
            (Point2::new(0., 1.5), Point2::new(0., 3.)),
            unit_arc(0., TAU),
            0.5,
        ),
        (
            (Point2::new(1.2, -1.), Point2::new(1.2, 1.)),
            unit_arc(FRAC_PI_4, FRAC_PI_2),
            1.2 - FRAC_PI_4.cos(),
        ),
        (
            (Point2::new(0., 0.), Point2::new(2., 0.)),
            unit_arc(0., TAU),
            0.,
        ),
        (
            (Point2::new(-0.5, 0.), Point2::new(0.5, 0.)),
            unit_arc(0., TAU),
            0.5,
        ),
    ] {
        let distance = chord_boundary_segment_distance(chord, &arc);
        assert!(
            (distance - expected).abs() <= EPS_CHORD_DISTANCE,
            "{distance} != {expected}"
        );
    }
    let line = ProfileBoundarySegment::Line {
        start: Point2::new(-1., 1.),
        end: Point2::new(1., 1.),
    };
    assert_eq!(chord_boundary_segment_distance(below, &line), 4.);
}

#[test]
fn analytic_intersection_storage_has_fixed_capacity_and_preserves_cardinality() {
    let circle = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: super::positive_radius(1.),
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    for (height, expected) in [(2., 0), (1., 1), (0., 2)] {
        let points: [Option<Point2>; 2] = line_arc_intersection_points(
            (Point2::new(-2., height), Point2::new(2., height)),
            &circle,
        )
        .unwrap();
        assert_eq!(points.into_iter().flatten().count(), expected);
    }
}
