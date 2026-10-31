//! tangency-labs/solidcore#2110: the union of two cylinders that touch inside at one
//! point. A (R 10) runs along x through (y, z) = (0, -10), x -20..20; B (R 5) runs along
//! y through (x, z) = (0, -5), y -20..20. Both tops pass through the origin with the
//! same tangent plane z = 0, and B leaves A through its wall at both ends.
//!
//! Their intersection is two loops round B that meet at the origin, a node where the
//! surface normals are parallel. Upstream's marcher steps straight through it, so the
//! traced curves cross there without ending (WR-77, #393). Unless a seam passes through
//! the origin, the merge then goes wrong in a way that depends on where the seams lie:
//! the union fails 1001 in `MakeFaceuse`, or drops B and returns A alone (a body that
//! checks clean), or returns a body that fails SMLib's check with `SmFace` test #29
//! (Face crosses Seam) and `SmEdgeuse` test #7 (`Bad UVTrimCurve`), the failure #2110
//! reports. The pinned C++ does the same (difftest `issue2110_*` cases in
//! `fixtures/boolean.api_brep/issue_2110_tangent_cylinders.json`; at the issue's seams
//! the C++ crashes checking the result, so that case compares the union only).
//!
//! With `UpstreamFixes::ssi_split_curves_at_singular_points` (the fork's
//! tangency-labs/smlib@3c9d1d3, `sm_SplitCurvesAtSingularPoints`) the curves end at the
//! node, and every seam placement (64 tried, 45 degrees apart) unites to a body that
//! checks clean at level 2 with the closed-form volume
//! `V = 4000 pi + 1000 pi - V_int`, `V_int` the integral over the disc
//! `x^2 + (z + 5)^2 <= 25` of `2 sqrt(100 - (z + 10)^2)` (tanh-sinh quadrature,
//! 1270.66448382136; `V` = 14437.2987841276). The fitted intersection curves keep the
//! results within 4e-7 of it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use offset_fixture::volume;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel, Result, SolidError};

/// `5000 pi - V_int` (the module doc).
const EXPECTED: f64 = 5000.0 * PI - 1_270.664_483_821_36;

/// A cylinder of `radius` and length 40 turned `seam` degrees about Z (its seam), then
/// `angle` degrees about `axis` onto its axis, then moved by `to`.
fn cylinder(k: &mut Kernel, radius: f64, seam: f64, axis: Vec3, angle: f64, to: Vec3) -> Brep {
    let o = Point3::new(0.0, 0.0, 0.0);
    let mut b = create_cylinder(k, o, radius, 40.0).unwrap();
    let z = Vec3::new(0.0, 0.0, 1.0);
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, z, seam).unwrap();
    api_rotate(k, Some(TransformObject::Brep(&mut b)), o, axis, angle).unwrap();
    api_translate(k, Some(TransformObject::Brep(&mut b)), to).unwrap();
    b
}

/// A's seam `seam_a` degrees round from its bottom line (180 is its top, through the
/// origin), B's `seam_b` round from its +x side (270 is its top).
fn union(split: bool, seam_a: f64, seam_b: f64) -> (Kernel, Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.ssi_split_curves_at_singular_points = split;
    let y = Vec3::new(0.0, 1.0, 0.0);
    let x = Vec3::new(1.0, 0.0, 0.0);
    let a = cylinder(&mut k, 10.0, seam_a, y, 90.0, Vec3::new(-20.0, 0.0, -10.0));
    let b = cylinder(&mut k, 5.0, seam_b, x, -90.0, Vec3::new(0.0, -20.0, -5.0));
    let u = boolean_union(&mut k, a, b);
    (k, u)
}

fn check_lines(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    check_brep(k, b, CheckLevel::Level2).unwrap().lines()
}

fn assert_sound(seam_a: f64, seam_b: f64) {
    let (mut k, u) = union(true, seam_a, seam_b);
    let mut u = u.unwrap_or_else(|e| panic!("seams {seam_a} {seam_b}: {e:?}"));
    assert!(brep_is_manifold_solid(&u).unwrap());
    let lines = check_lines(&mut k, &mut u);
    assert!(lines.is_empty(), "seams {seam_a} {seam_b}: {lines:?}");
    let v = volume(&k, &mut u).unwrap();
    assert!(
        (v - EXPECTED).abs() <= 1e-6 * EXPECTED,
        "seams {seam_a} {seam_b}: volume {v}, expected {EXPECTED}"
    );
}

#[test]
fn split_checks_clean_at_the_issue_seams() {
    // without the flag these fail SmFace #29 and SmEdgeuse #7
    assert_sound(135.0, 225.0);
}

#[test]
fn split_keeps_b_where_upstream_drops_it() {
    assert_sound(0.0, 90.0);
}

#[test]
fn split_builds_where_upstream_fails_1001() {
    assert_sound(0.0, 0.0);
    assert_sound(270.0, 315.0);
}

#[test]
fn split_side_seams() {
    assert_sound(90.0, 45.0);
    assert_sound(225.0, 135.0);
}

#[test]
fn split_seams_through_the_node() {
    assert_sound(180.0, 270.0);
}

#[test]
fn upstream_fails_the_check_at_the_issue_seams() {
    let (mut k, u) = union(false, 135.0, 225.0);
    let mut u = u.unwrap();
    let lines = check_lines(&mut k, &mut u);
    assert!(
        lines
            .iter()
            .any(|l| l.contains("SmFace::Test #29 Face crosses Seam")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("SmEdgeuse::Test #7 Bad UVTrimCurve")),
        "{lines:?}"
    );
}

#[test]
fn upstream_drops_b() {
    let (mut k, u) = union(false, 0.0, 90.0);
    let mut u = u.unwrap();
    assert!(check_lines(&mut k, &mut u).is_empty());
    let v = volume(&k, &mut u).unwrap();
    let a = 4000.0 * PI;
    assert!((v - a).abs() <= 1e-6 * a, "volume {v}, A alone {a}");
}

#[test]
fn upstream_fails_1001() {
    let (_, u) = union(false, 0.0, 0.0);
    match u {
        Err(SolidError::Upstream { code: 1001, .. }) => {}
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn upstream_builds_with_seams_through_the_node() {
    let (mut k, u) = union(false, 180.0, 270.0);
    let mut u = u.unwrap();
    assert!(check_lines(&mut k, &mut u).is_empty());
    let v = volume(&k, &mut u).unwrap();
    assert!((v - EXPECTED).abs() <= 1e-6 * EXPECTED, "volume {v}");
}
