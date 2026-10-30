//! tangency-labs/solidcore#1502: thickening a sphere cap whose rim is two half circles.
//! Upstream, `TrimExtendedEdge` can take the first trim point's twin across the lifted
//! offset circle's seam as an open rim half's second trim point. Each half then gets
//! the whole circle and the shell fails (1001).
//!
//! Test-only: the issue does not reproduce in the Rust port. On these bodies each rim
//! half's second trim point already lands on its own other vertex: the fork's guard,
//! tried as an opt-in, changed no result.
//!
//! Each body is a ball (r 10) cut by a half space, shelled open at the flat face by +1
//! or -1, with and without an offset solid. One cut is at the equator and one through
//! the poles, so the rim crosses the sphere's seam differently. The ninth body is the
//! two stitched quarter-sphere sheets thickened by +1. All runs have
//! `offset_closed_curve_trim_arc` (#403) on, so each half of a closed rim keeps its own
//! arc. Every result is a manifold solid of the spherical shell's volume
//! 2/3 π (`r_out³` − `r_in³`), to the accuracy the volume integration is asked for (1e-6;
//! the results come within 1.4e-6 relative).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use offset_fixture::{volume, Notifications};
use solidcore::blend::fillet_executive::api::create_face_from_surface;
use solidcore::boolean::api_brep::boolean_intersection;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol;
use solidcore::math::Vec3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::{Brep, FaceId, Kernel};

/// Relative tolerance on the volume: the integration is asked for 1e-6, and the shelled
/// caps come within 1.4e-6.
const REL: f64 = 1e-5;

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.offset_closed_curve_trim_arc = true;
    k
}

fn shell_options(distance: f64, create_offset_solid: bool) -> ShellOptions {
    ShellOptions {
        offset: OffsetOptions {
            distance,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid,
        merge_sheet_side_walls: false,
    }
}

/// The ball r 10 at the origin cut by the box at `origin` of `size` (a half space), shelled
/// open at its flat face by `distance`.
fn shelled_cap(origin: Vec3, size: Vec3, distance: f64, solid: bool) -> (Kernel, Brep) {
    let mut k = kernel();
    let ball = create_sphere(&mut k, Vec3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let half = create_box(&mut k, origin, size.x, size.y, size.z).unwrap();
    let mut cap = boolean_intersection(&mut k, ball, half).unwrap();
    let flat: Vec<FaceId> = cap
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            cap.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| s.is_kind_of(Plane::TYPE).unwrap())
        })
        .collect();
    assert_eq!(flat.len(), 1);
    let mut notifications = Notifications::new(&mut cap);
    let out = OffsetExecutive::shell_brep(
        &mut k,
        cap,
        shell_options(distance, solid),
        &flat,
        &mut notifications.context(),
    )
    .result
    .unwrap();
    (k, out)
}

/// The cut at the equator (z >= 0).
fn equator(distance: f64, solid: bool) -> (Kernel, Brep) {
    shelled_cap(
        Vec3::new(-20.0, -20.0, 0.0),
        Vec3::new(40.0, 40.0, 20.0),
        distance,
        solid,
    )
}

/// The cut through the poles (x >= 0).
fn poles(distance: f64, solid: bool) -> (Kernel, Brep) {
    shelled_cap(
        Vec3::new(0.0, -20.0, -20.0),
        Vec3::new(20.0, 40.0, 40.0),
        distance,
        solid,
    )
}

/// The upper hemisphere r 10 as two quarter-sphere sheets (longitudes 0-180 and
/// 180-360) stitched along their meridians, thickened by +1: its rim is two half circles.
fn stitched_quarters_thickened() -> (Kernel, Brep) {
    let mut k = kernel();
    let mut sheet =
        solidcore::topology::brep::p3::new_brep(&mut k, tol::USE_DEFAULT, tol::USE_DEFAULT)
            .unwrap();
    for (lon0, lon1) in [(0.0, 180.0), (180.0, 360.0)] {
        let mut quarter = Sphere::new(
            &k,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent2d::from_bounds(lon0, 0.0, lon1, 90.0),
            10.0,
            SphereOptions::default(),
        )
        .unwrap();
        quarter.make_nurb(&k).unwrap();
        create_face_from_surface(&mut k, &mut sheet, &Surface::from(quarter)).unwrap();
    }
    stitch_faces(&mut k, &mut sheet, 1.0e-5, None, false).unwrap();
    assert_eq!(sheet.face_list().len(), 2);
    assert!(!brep_is_manifold_solid(&sheet).unwrap());
    let mut notifications = Notifications::new(&mut sheet);
    let out = OffsetExecutive::shell_brep(
        &mut k,
        sheet,
        shell_options(1.0, true),
        &[],
        &mut notifications.context(),
    )
    .result
    .unwrap();
    (k, out)
}

/// The result is a manifold solid of `faces` faces whose volume is the hemispherical
/// shell between radii 10 and `10 + distance`.
fn assert_sound((k, mut b): (Kernel, Brep), distance: f64, faces: usize) {
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), faces);
    let (outer, inner) = if distance > 0.0 {
        (10.0 + distance, 10.0)
    } else {
        (10.0, 10.0 + distance)
    };
    let expected = 2.0 / 3.0 * PI * (outer * outer * outer - inner * inner * inner);
    let v = volume(&k, &mut b).unwrap();
    assert!(
        (v - expected).abs() <= REL * expected,
        "volume {v}, expected {expected}"
    );
}

#[test]
fn equator_cap_shelled_out_as_offset_solid() {
    assert_sound(equator(1.0, true), 1.0, 3);
}

#[test]
fn equator_cap_shelled_in_as_offset_solid() {
    assert_sound(equator(-1.0, true), -1.0, 3);
}

#[test]
fn equator_cap_shelled_out() {
    assert_sound(equator(1.0, false), 1.0, 3);
}

#[test]
fn equator_cap_shelled_in() {
    assert_sound(equator(-1.0, false), -1.0, 3);
}

#[test]
fn poles_cap_shelled_out_as_offset_solid() {
    assert_sound(poles(1.0, true), 1.0, 5);
}

#[test]
fn poles_cap_shelled_in_as_offset_solid() {
    assert_sound(poles(-1.0, true), -1.0, 5);
}

#[test]
fn poles_cap_shelled_out() {
    assert_sound(poles(1.0, false), 1.0, 5);
}

#[test]
fn poles_cap_shelled_in() {
    assert_sound(poles(-1.0, false), -1.0, 5);
}

#[test]
fn stitched_hemisphere_quarters_thickened_out() {
    assert_sound(stitched_quarters_thickened(), 1.0, 6);
}
