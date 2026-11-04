// SPDX-License-Identifier: Apache-2.0
use crate::design::dimensions::{line_angle_matches, parallel_line_offset};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
fn line(start: Point2, end: Point2) -> SketchGeometry {
    SketchGeometryDefinition::Line { start, end }
        .try_into()
        .unwrap()
}
#[test]
fn numerical_ranges_line_angle_is_independent_of_lengths() {
    for length in [1e-10, 1.0, 1e200] {
        let a = line(Point2::new(0., 0.), Point2::new(length, 0.));
        let b = line(Point2::new(0., 0.), Point2::new(length, length));
        assert!(line_angle_matches(&a, &b, std::f64::consts::FRAC_PI_4));
        assert!(!line_angle_matches(&a, &b, 0.));
    }
}
#[test]
fn numerical_ranges_offset_rejects_huge_perpendicular_lines() {
    let a = line(Point2::new(0., 0.), Point2::new(1e200, 0.));
    let b = line(Point2::new(0., 1.), Point2::new(0., 1e200));
    assert_eq!(parallel_line_offset(&a, &b), None);
    let c = line(Point2::new(0., 1.), Point2::new(1e200, 1.));
    assert_eq!(parallel_line_offset(&a, &c), Some(1.));
}

#[test]
fn reflection_preserves_finite_points_at_extreme_scales() {
    use cadmpeg_ir::math::Point2;
    assert_eq!(
        super::super::reflect_point(
            Point2::new(1e200, 1.0),
            Point2::new(0.0, 0.0),
            Point2::new(1e200, 0.0)
        ),
        Some(Point2::new(1e200, -1.0))
    );
    assert_eq!(
        super::super::reflect_point(
            Point2::new(1e308, 2.0),
            Point2::new(1e308, 0.0),
            Point2::new(1e308, 1.0)
        ),
        Some(Point2::new(1e308, 2.0))
    );
    assert!(super::super::reflect_point(
        Point2::new(1.0, 1.0),
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 0.0)
    )
    .is_none());
}

#[test]
fn numerical_followup_parabola_bounds_use_transverse_length() {
    for focal in [2.0, 1e200] {
        let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
            vertex: Point2::new(0., 0.),
            axis_angle: cadmpeg_ir::scalar::Angle::new(0.).unwrap(),
            focal_length: cadmpeg_ir::scalar::Length::new(focal).unwrap(),
            bounds: Some([0.5 * focal, focal]),
        })
        .unwrap();
        assert!(super::super::point_lies_on_sketch_geometry(
            Point2::new(0.25 * focal, focal),
            &geometry,
        ));
        assert!(!super::super::point_lies_on_sketch_geometry(
            Point2::new(focal, 2. * focal),
            &geometry,
        ));
    }
}

#[test]
fn numerical_followup_parabola_bounds_allow_reversed_orientation() {
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
        vertex: Point2::new(0., 0.),
        axis_angle: cadmpeg_ir::scalar::Angle::new(0.).unwrap(),
        focal_length: cadmpeg_ir::scalar::Length::new(2.).unwrap(),
        bounds: Some([2., 1.]),
    })
    .unwrap();
    assert!(super::super::point_lies_on_sketch_geometry(
        Point2::new(0.5, 2.),
        &geometry
    ));
    assert!(!super::super::point_lies_on_sketch_geometry(
        Point2::new(2., 4.),
        &geometry
    ));
}
