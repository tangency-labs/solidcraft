//! Issue #1776: a plus profile's reflex corners reverse `ComputeCrvsNormal`, so
//! outward and inward drafts are exchanged. The opt-in area normal gives each
//! corner rule the requested direction, checked against closed-form volumes.
#![allow(clippy::unwrap_used, clippy::panic)]

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::sweep::api_brep::{
    create_taper_extrude_with_corner, OffsetCornerType, TaperExtrude,
};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Kernel, Vec3};

/// Width 10, arm length 30: area 1300, perimeter 280, eight convex and four
/// reflex corners. Offsets up to `10 tan(10 degrees)` preserve this topology.
fn plus() -> Vec<Curve> {
    let points = [
        (-5.0, -35.0),
        (5.0, -35.0),
        (5.0, -5.0),
        (35.0, -5.0),
        (35.0, 5.0),
        (5.0, 5.0),
        (5.0, 35.0),
        (-5.0, 35.0),
        (-5.0, 5.0),
        (-35.0, 5.0),
        (-35.0, -5.0),
        (-5.0, -5.0),
    ];
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&(x1, y1), &(x2, y2))| {
            Curve::from(
                Line::create_line_segment(3, Vec3::new(x1, y1, 0.0), Vec3::new(x2, y2, 0.0), None)
                    .unwrap(),
            )
        })
        .collect()
}

fn expected_volume(outward: bool, round: bool) -> f64 {
    let offset = 10.0 * 10.0_f64.to_radians().tan();
    let sign = if outward { 1.0 } else { -1.0 };
    let quadratic = if !round {
        4.0
    } else if outward {
        2.0 * std::f64::consts::PI - 4.0
    } else {
        8.0 - std::f64::consts::PI
    };
    // Integrate A(z) = 1300 + sign * 280 d(z) + quadratic * d(z)^2.
    10.0 * (1300.0 + sign * 140.0 * offset + quadratic * offset * offset / 3.0)
}

fn drafted_volume(fixed: bool, outward: bool, corner: OffsetCornerType) -> f64 {
    let mut kernel = Kernel::default();
    kernel.fixes.curve_loop_normal_from_area = fixed;
    let curves = plus();
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let draft = if outward { -10.0 } else { 10.0 };
    let result =
        create_taper_extrude_with_corner(&mut kernel, &refs, 10.0, draft, 3, corner).unwrap();
    let TaperExtrude::Solid(mut brep) = result else {
        panic!("unexpected refusal: {result:?}");
    };
    assert!(is_manifold_solid(&brep).unwrap());
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(&kernel, &mut brep, 1e-9, &attributes).unwrap()
}

fn assert_volume(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-9 * actual.abs().max(expected.abs()),
        "volume {actual}, expected {expected}"
    );
}

#[test]
fn default_plus_draft_exchanges_inward_and_outward() {
    for outward in [false, true] {
        assert_volume(
            drafted_volume(false, outward, OffsetCornerType::LinearExtension),
            expected_volume(!outward, false),
        );
    }
}

#[test]
fn area_normal_gives_both_mitred_rules_the_requested_draft() {
    for corner in [
        OffsetCornerType::LinearExtension,
        OffsetCornerType::CurveExtension,
    ] {
        for outward in [false, true] {
            assert_volume(
                drafted_volume(true, outward, corner),
                expected_volume(outward, false),
            );
        }
    }
}

#[test]
fn area_normal_gives_round_corners_the_requested_draft() {
    for outward in [false, true] {
        assert_volume(
            drafted_volume(true, outward, OffsetCornerType::FilletCorner),
            expected_volume(outward, true),
        );
    }
}
