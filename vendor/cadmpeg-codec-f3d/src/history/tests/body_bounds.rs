// SPDX-License-Identifier: Apache-2.0
//! Axis extremes of circular edges in historical body bounds.

use crate::history::circle_arc_extremes;
use cadmpeg_ir::math::{Point3, Vector3};

fn close(a: Point3, b: Point3) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9
}

#[test]
fn a_whole_circle_reaches_its_radius_along_every_in_plane_axis() {
    let center = Point3::new(0.0, 0.0, 9.0);
    let seam = Point3::new(0.0, -30.0, 9.0);
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let points = circle_arc_extremes(center, normal, 30.0, seam, None, seam, true);
    for expected in [
        Point3::new(30.0, 0.0, 9.0),
        Point3::new(-30.0, 0.0, 9.0),
        Point3::new(0.0, 30.0, 9.0),
        Point3::new(0.0, -30.0, 9.0),
    ] {
        assert!(
            points.iter().any(|point| close(*point, expected)),
            "{expected:?}"
        );
    }
    assert_eq!(points.len(), 4);
}

#[test]
fn an_open_arc_keeps_only_the_extremes_on_its_side() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let start = Point3::new(1.0, 0.0, 0.0);
    let end = Point3::new(-1.0, 0.0, 0.0);
    // Counterclockwise through the top: the upper extreme belongs to the arc.
    let upper = circle_arc_extremes(
        center,
        normal,
        1.0,
        start,
        Some(Point3::new(0.0, 1.0, 0.0)),
        end,
        false,
    );
    assert!(upper
        .iter()
        .any(|point| close(*point, Point3::new(0.0, 1.0, 0.0))));
    assert!(!upper
        .iter()
        .any(|point| close(*point, Point3::new(0.0, -1.0, 0.0))));
    // Clockwise through the bottom: the lower one does.
    let lower = circle_arc_extremes(
        center,
        normal,
        1.0,
        start,
        Some(Point3::new(0.0, -1.0, 0.0)),
        end,
        false,
    );
    assert!(lower
        .iter()
        .any(|point| close(*point, Point3::new(0.0, -1.0, 0.0))));
    assert!(!lower
        .iter()
        .any(|point| close(*point, Point3::new(0.0, 1.0, 0.0))));
    // Without an interior point the side is unknown.
    assert!(circle_arc_extremes(center, normal, 1.0, start, None, end, false).is_empty());
}
