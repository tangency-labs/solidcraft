//! tangency-labs/solidcore#1502 (test-only; the issue is closed by the merged
//! `issue_1502_thicken_rim_halves.rs`): the original sphere-cap thicken from the issue —
//! a sphere r10 split by the plane z = 5, the spherical face above the plane taken alone,
//! thickened out by 1.
//!
//! Expected: the sector between radii 10 and 11 over the cap's solid angle pi,
//! V = pi/3 (11^3 - 10^3) = 331 pi/3 = 346.622389446...
//!
//! Issue #1502 attributes the C++ failure to a second trim point repeated across a closed
//! offset curve's seam. These Rust-only tests do not establish that internal mechanism or
//! compare the cap against C++. They exercise the existing #403 opt-in
//! `Kernel::fixes.offset_closed_curve_trim_arc` (WR-463), which keeps each half's own arc.
//! `issue_1502_thicken_rim_halves.rs` covers related stitched quarter-sphere sheets and
//! equator/pole caps.
//!
//! This file keeps the *original* z = 5 cap, which that test does not construct:
//! - as the port's own construction makes it, the rim is one closed circle and the cap
//!   thickens soundly at the default and with the #403 opt-in;
//! - with the sphere turned 180 degrees about z the rim is two open half circles, the
//!   issue's shape: there the default yields a non-manifold, zero-volume result and the
//!   #403 opt-in gives the sound solid of the expected volume. The tests characterize
//!   this difference without inferring the default's internal failure mechanism.
//!
//! Volumes are measured at 1e-8 volume accuracy and asserted to 1e-6 relative (the
//! issue's tolerance, no waiver): the cap as the port constructs it gives 346.62237757092566
//! and the half-rim shape with the #403 opt-in 346.622377582594, both 3.4e-8 relative
//! below 331 pi/3.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{planar_faces_at, Notifications};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::p2::ELLIPSE_TYPE;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p5::{remove_faces, transform};
use solidcore::{Brep, EdgeId, Kernel};

/// The sphere r10 cap above z = 5 as a sheet: the sphere's face of the split solid with
/// the flat cut face removed. With `turn` the sphere is turned 180 degrees about z first,
/// producing two open half-circle rim edges (the issue's shape).
fn cap_sheet(k: &mut Kernel, turn: bool) -> Brep {
    let mut ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    if turn {
        let half_turn = Axis2Placement::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
        transform(k, &mut ball, &half_turn, None, false).unwrap();
    }
    // a box below the plane z = 5, so the difference keeps the cap above it
    let below = create_box(k, Point3::new(-15.0, -15.0, -15.0), 30.0, 30.0, 20.0).unwrap();
    let mut cap = boolean_difference(k, ball, below).unwrap();
    let flat = planar_faces_at(k, &cap, 2, 5.0);
    assert_eq!(flat.len(), 1, "one flat cut face at z = 5");
    cap.set_editing_enabled(true);
    remove_faces(k, &mut cap, true, &flat).unwrap();
    cap.refresh_cached_topology();
    assert_eq!(cap.face_list().len(), 1, "one spherical face");
    cap
}

/// `SmApiShellBrepFull` thickening the sheet `distance` out (a solid, merged results).
fn thicken(k: &mut Kernel, mut sheet: Brep, distance: f64) -> solidcore::Result<Brep> {
    let mut notifications = Notifications::new(&mut sheet);
    OffsetExecutive::shell_brep(
        k,
        sheet,
        ShellOptions {
            offset: OffsetOptions {
                distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        },
        &[],
        &mut notifications.context(),
    )
    .result
    // as `api_shell_brep_full` does for its caller: the port's cached topology fields
    .map(|mut b| {
        b.refresh_cached_topology();
        b
    })
}

/// The issue's configuration: the existing #403 opt-in on (WR-463), nothing else.
fn wr463() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.offset_closed_curve_trim_arc = true;
    k
}

/// The sector between the sphere's radii 10 and 11 over the cap's solid angle pi.
fn want() -> f64 {
    PI / 3.0 * (11.0_f64.powi(3) - 10.0_f64.powi(3))
}

/// `SmApiBrepComputeVolume` at `accuracy`.
fn volume_at(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

/// The sheet's circular edges as (edge, is closed).
fn circle_edges(b: &Brep) -> Vec<(EdgeId, bool)> {
    b.edge_list()
        .iter()
        .filter_map(|&e| {
            let curve = b.edge(e).unwrap().curve.as_ref()?;
            if !curve.is_kind_of(ELLIPSE_TYPE) {
                return None;
            }
            Some((e, solidcore::topology::edge::is_closed(b, e).unwrap()))
        })
        .collect()
}

/// The cap's boundary on z = 5 as (edge, is closed): the circular edges that sample the
/// circle of radius sqrt(75) there.
fn rim_edges(k: &Kernel, b: &Brep) -> Vec<(EdgeId, bool)> {
    let mut rim = Vec::new();
    for (e, closed) in circle_edges(b) {
        let edge = b.edge(e).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let mut on_rim = true;
        for i in 0..=16 {
            let t = edge.interval.evaluate(f64::from(i) / 16.0);
            let p = curve.evaluate_point(k, t).unwrap();
            on_rim &=
                (p.z - 5.0).abs() <= 1e-9 && (p.x.hypot(p.y) - (75.0_f64).sqrt()).abs() <= 1e-9;
        }
        if on_rim {
            rim.push((e, closed));
        }
    }
    rim
}

/// Both policy controls must start with the same two open half-circle rim edges.
fn assert_half_rims(k: &Kernel, sheet: &Brep) {
    let rim = rim_edges(k, sheet);
    eprintln!("issue_1502 z5 half-rim edges {rim:?}");
    assert_eq!(rim.len(), 2, "two rim halves: {rim:?}");
    assert!(
        rim.iter().all(|&(_, closed)| !closed),
        "rim halves are open"
    );
    let diameter = 2.0 * 75.0_f64.sqrt();
    for (e, _) in rim {
        let edge = sheet.edge(e).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let (a, b) = ends(k, sheet, e);
        assert!(
            (a.distance(b) - diameter).abs() <= 1e-9,
            "rim endpoints must be antipodal: {a:?}, {b:?}"
        );
        // Together with the sampled circle and antipodal ends, excluding extra turns
        // establishes a half circle without a detached curve's owning-tolerance query.
        assert!(
            edge.interval.length() <= curve.natural_interval().length(),
            "rim interval must not span extra turns"
        );
    }
}

/// The edge's end points.
fn ends(k: &Kernel, b: &Brep, e: EdgeId) -> (Point3, Point3) {
    let edge = b.edge(e).unwrap();
    let curve = edge.curve.as_ref().unwrap();
    (
        curve.evaluate_point(k, edge.interval.min()).unwrap(),
        curve.evaluate_point(k, edge.interval.max()).unwrap(),
    )
}

fn assert_sound((k, mut b): (Kernel, Brep), faces: usize) {
    solidcore::topology::integrity::check(&b).unwrap();
    assert!(brep_is_manifold_solid(&b).unwrap(), "not a manifold solid");
    assert_eq!(b.face_list().len(), faces, "face count");
    let v = volume_at(&k, &mut b, 1e-8);
    eprintln!("issue_1502 z5 cap volume {v} want {}", want());
    assert!(
        (v - want()).abs() <= 1e-6 * want(),
        "volume {v} want {}",
        want()
    );
}

/// The cap as the port's own construction makes it: its rim is one closed circle and the
/// seam is one open arc.
#[test]
fn z5_cap_rim_is_one_closed_circle() {
    let mut k = Kernel::default();
    let sheet = cap_sheet(&mut k, false);
    let rim = rim_edges(&k, &sheet);
    eprintln!("issue_1502 z5 cap rim edges {rim:?}");
    assert_eq!(rim.len(), 1, "one rim edge: {rim:?}");
    assert!(rim[0].1, "the rim is one closed circle");
    // the sheet's other circular edge is the seam, open, from the pole to the rim
    let seam: Vec<EdgeId> = circle_edges(&sheet)
        .into_iter()
        .filter(|&(e, closed)| !closed && e != rim[0].0)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(seam.len(), 1, "one open seam edge: {seam:?}");
    let (a, b) = ends(&k, &sheet, seam[0]);
    let pole = Point3::new(0.0, 0.0, 10.0);
    let on_rim = |p: Point3| (p.z - 5.0).abs() <= 1e-9;
    assert!(
        (a.distance(pole) <= 1e-9 && on_rim(b)) || (b.distance(pole) <= 1e-9 && on_rim(a)),
        "the seam does not run from the pole to the rim: {a:?} {b:?}"
    );
}

/// The default already gives the sound sector for the cap as the port constructs it.
#[test]
fn z5_cap_thickens_at_the_default() {
    let mut k = Kernel::default();
    let sheet = cap_sheet(&mut k, false);
    let shell = thicken(&mut k, sheet, 1.0).unwrap();
    assert_sound((k, shell), 3);
}

/// The issue's configuration: with the existing #403 opt-in the cap is the same sound
/// sector.
#[test]
fn z5_cap_thickens_with_the_wr463_option() {
    let mut k = wr463();
    let sheet = cap_sheet(&mut k, false);
    let shell = thicken(&mut k, sheet, 1.0).unwrap();
    assert_sound((k, shell), 3);
}

/// The issue's shape: the rim split into two open half circles. The #403 opt-in gives one
/// manifold solid of the sector's volume.
#[test]
fn z5_cap_half_rims_thicken_with_the_wr463_option() {
    let mut k = wr463();
    let sheet = cap_sheet(&mut k, true);
    assert_half_rims(&k, &sheet);
    let shell = thicken(&mut k, sheet, 1.0).unwrap();
    assert_sound((k, shell), 4);
}

/// Characterize the Rust default on the same half-rim shape: no manifold solid and zero
/// volume. The enabled-policy test above demonstrates the existing option's correction;
/// neither test diagnoses the default's internal failure mechanism.
#[test]
fn z5_cap_half_rims_are_non_manifold_at_the_default() {
    let mut k = Kernel::default();
    assert!(!k.fixes.offset_closed_curve_trim_arc);
    let sheet = cap_sheet(&mut k, true);
    assert_half_rims(&k, &sheet);
    let mut shell = thicken(&mut k, sheet, 1.0).unwrap();
    assert!(
        !brep_is_manifold_solid(&shell).unwrap(),
        "default half-rim characterization changed: now a manifold solid"
    );
    assert_eq!(volume_at(&k, &mut shell, 1e-8), 0.0, "a void, not a sector");
}
