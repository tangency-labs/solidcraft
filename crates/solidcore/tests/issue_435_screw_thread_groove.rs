//! tangency-labs/solidcore#435: a cylinder minus a helical thread groove fails SMLib's
//! own check with `SmFace` test #34 ("Face has partially Coincident Edges"), the
//! SolidCraft rig's case 1250: an M10x1.5 groove along the whole 12 mm of a 10 mm shaft.
//!
//! The groove is the external thread's ISO 965 minor-diameter profile (17H/24 deep, as
//! the rig's M33 and M44 cases in `issue_410_435_thread_groove.rs`; p/4 at the root, flanks
//! at 30 degrees, carried 0.05 past the crest) under a screw motion: sections in meridian planes,
//! sixteen a turn, skinned cubically from a pitch below the shaft to a pitch above it.
//! (`create_curve_sweep` along a helix is not a thread: its frame twists about 21 degrees a
//! turn, so the profile runs into the shaft's core.) The SolidCraft rig cuts its groove
//! with the fork's screw sweep, which SMLib does not have.
//!
//! Upstream (the C++ reference gives the same reports on the same body): the flank/end-face
//! curves cross the rim at a grazing angle, two edges end 1.46e-5 from their vertices
//! (`SmBrep` test #0) and two faces fail test #34. With
//! `Kernel::fixes.imprint_trims_edge_to_proto_vertex` and
//! `imprint_extends_edge_to_proto_vertex` (WR-452, WR-453) the result passes the whole check,
//! and its volume is Pappus's. That needs the boolean's edge uses to own their UV trim
//! curves under the result's names: a merge left those it made under its own namespace's
//! keys, so the port's check reported `SmEdgeuse` test #6 on them, which upstream never does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use std::f64::consts::PI;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

const PITCH: f64 = 1.5;
const RADIUS: f64 = 5.0;
const LENGTH: f64 = 12.0;

/// The thread depth, 17H/24 with H = p*sqrt(3)/2 (the basic profile's 5H/8 cuts a body
/// that passes the check upstream too).
fn depth() -> f64 {
    17.0 / 24.0 * (3.0_f64.sqrt() / 2.0 * PITCH)
}

/// The groove's section, (radius, axial offset) corners, root first.
fn profile() -> [(f64, f64); 4] {
    let root = RADIUS - depth();
    let outer = RADIUS + 0.05;
    let half_root = PITCH / 8.0;
    let half_outer = half_root + (outer - root) * 30.0_f64.to_radians().tan();
    [
        (root, -half_root),
        (outer, -half_outer),
        (outer, half_outer),
        (root, half_root),
    ]
}

/// The groove: the section moved by a screw motion of `PITCH` a turn, from `z0`, over
/// `turns` turns, `per_turn` sections a turn, skinned with degree 3 and capped.
fn screw_groove(k: &mut Kernel, z0: f64, turns: f64, per_turn: usize) -> Brep {
    let prof = profile();
    let n = (turns * per_turn as f64).round() as usize;
    let mut curves = Vec::new();
    for i in 0..=n {
        let turn = i as f64 / per_turn as f64;
        let (s, c) = (2.0 * PI * turn).sin_cos();
        let pts: Vec<Point3> = prof
            .iter()
            .map(|&(r, z)| Point3::new(r * c, r * s, z0 + z + PITCH * turn))
            .collect();
        for j in 0..pts.len() {
            let side = create_line_segment(pts[j], pts[(j + 1) % pts.len()]).unwrap();
            curves.push(Curve::from(side));
        }
    }
    let counts = vec![prof.len(); n + 1];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_skin_primitive(k, &counts, &refs, 3, true)
        .unwrap()
        .brep
}

/// Case 1250: the shaft minus the groove, run a pitch past each end.
fn threaded_shaft(trim: bool, extend: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.imprint_trims_edge_to_proto_vertex = trim;
    k.fixes.imprint_extends_edge_to_proto_vertex = extend;
    let shaft = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), RADIUS, LENGTH).unwrap();
    let turns = (LENGTH + 2.0 * PITCH) / PITCH;
    let groove = screw_groove(&mut k, -PITCH, turns, 16);
    let body = boolean_difference(&mut k, shaft, groove).unwrap();
    (k, body)
}

/// The check's report lines.
fn check(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    check_brep(k, b, CheckLevel::Level2).unwrap().lines()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// Pappus: the shaft less `LENGTH / PITCH` turns of the section's part inside the shaft
/// (a trapezoid p/4 wide at the root, widening at 30 degrees a side), 748.42 mm^3.
fn analytic_volume() -> f64 {
    let d = depth();
    let a = PITCH / 4.0;
    let b = a + 2.0 * d * 30.0_f64.to_radians().tan();
    let area = d * (a + b) / 2.0;
    let centroid = RADIUS - d + d * (a + 2.0 * b) / (3.0 * (a + b));
    PI * RADIUS * RADIUS * LENGTH - LENGTH / PITCH * 2.0 * PI * centroid * area
}

fn count(lines: &[String], prefix: &str) -> usize {
    lines.iter().filter(|l| l.starts_with(prefix)).count()
}

#[test]
fn issue_435_m10_full_length_upstream_fails_test_34() {
    let (mut k, mut b) = threaded_shaft(false, false);
    let lines = check(&mut k, &mut b);
    assert_eq!(count(&lines, "SmFace::Test #34 "), 2, "{lines:#?}");
    assert_eq!(count(&lines, "SmBrep::Test #0 "), 2, "{lines:#?}");
    assert_eq!(lines.len(), 4, "{lines:#?}");
}

#[test]
fn issue_435_m10_full_length_is_valid_with_the_imprint_fixes() {
    let (mut k, mut b) = threaded_shaft(true, true);
    let lines = check(&mut k, &mut b);
    assert!(lines.is_empty(), "{lines:#?}");
    // The skin's cubic through sixteen sections a turn is 6.5e-5 short of the screw.
    let (v, want) = (volume(&k, &mut b), analytic_volume());
    assert!((v - want).abs() < 1e-4 * want, "volume {v} against {want}");
}

/// A boolean's edge uses own their UV trim curves under the result's names (`SmEdgeuse`
/// test #6), so a plain difference of two cylinders passes the check.
#[test]
fn boolean_result_uv_trims_are_owned_by_their_edge_uses() {
    let mut k = Kernel::default();
    let nut = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 8.0, 12.0).unwrap();
    let bore = create_cylinder(&mut k, Point3::new(0.0, 0.0, -1.0), 4.0, 14.0).unwrap();
    let mut b = boolean_difference(&mut k, nut, bore).unwrap();
    let lines = check(&mut k, &mut b);
    assert!(lines.is_empty(), "{lines:#?}");
}
