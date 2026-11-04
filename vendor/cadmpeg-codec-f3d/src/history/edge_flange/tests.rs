// SPDX-License-Identifier: Apache-2.0

use super::common_inside_radius;
use crate::history_records::AsmHistoricalCylinder;
use cadmpeg_ir::math::{Point3, Vector3};

const RADIUS_ASSERT_TOLERANCE: f64 = 1.0e-10;

fn carriers() -> [AsmHistoricalCylinder; 4] {
    let cylinder = |surface, axis, radius| AsmHistoricalCylinder {
        surface,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis,
        radius,
    };
    [
        cylinder(10, Vector3::new(0.0, 0.0, 1.0), 0.7),
        cylinder(20, Vector3::new(0.0, 0.0, 1.0), 2.4),
        cylinder(30, Vector3::new(1.0, 0.0, 0.0), 0.7),
        cylinder(40, Vector3::new(1.0, 0.0, 0.0), 2.4),
    ]
}

fn axes() -> [Vector3; 2] {
    [Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)]
}

#[test]
fn flange_carriers_prove_a_common_inside_radius() {
    let cylinders = carriers();
    let radius = common_inside_radius(cylinders.iter(), axes(), 1.7).unwrap();
    assert!((radius.get() - 0.7).abs() <= RADIUS_ASSERT_TOLERANCE);
    let mut reversed = cylinders;
    reversed.reverse();
    assert!(
        (common_inside_radius(reversed.iter(), axes(), 1.7)
            .unwrap()
            .get()
            - 0.7)
            .abs()
            <= RADIUS_ASSERT_TOLERANCE
    );
}

#[test]
fn flange_carriers_refuse_incomplete_or_conflicting_radius_proofs() {
    let cylinders = carriers();
    for length in 0..cylinders.len() {
        assert!(common_inside_radius(cylinders[..length].iter(), axes(), 1.7).is_none());
    }
    assert!(
        common_inside_radius(cylinders.iter().chain(cylinders[..1].iter()), axes(), 1.7).is_none()
    );
    for thickness in [0.0, -1.7, f64::NAN, 1.8] {
        assert!(common_inside_radius(cylinders.iter(), axes(), thickness).is_none());
    }
    assert!(common_inside_radius(cylinders.iter(), [axes()[0]; 2], 1.7).is_none());
    let mut inconsistent = cylinders.clone();
    inconsistent[2].radius = 0.9;
    inconsistent[3].radius = 2.6;
    assert!(common_inside_radius(inconsistent.iter(), axes(), 1.7).is_none());
    let mut displaced = cylinders.clone();
    displaced[1].origin = Point3::new(0.3, 0.0, 0.0);
    assert!(common_inside_radius(displaced.iter(), axes(), 1.7).is_none());
    let mut unrelated = cylinders.clone();
    unrelated[0].axis = Vector3::new(0.0, 1.0, 0.0);
    assert!(common_inside_radius(unrelated.iter(), axes(), 1.7).is_none());
    for radius in [0.0, -0.7, f64::NAN] {
        let mut invalid = cylinders.clone();
        invalid[0].radius = radius;
        assert!(common_inside_radius(invalid.iter(), axes(), 1.7).is_none());
    }
}

#[test]
fn flange_radius_requires_a_bound_history_state() {
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#45",
        crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
        45,
    );
    assert!(super::inside_radius_for_scope(&scope, [50, 60], 1.7, &[]).is_none());
}
