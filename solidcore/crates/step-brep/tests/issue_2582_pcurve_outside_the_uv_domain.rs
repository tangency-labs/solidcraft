//! A STEP file whose pcurve leaves the face's UV domain, on a periodic surface.
//!
//! `pcurves.rs` checks a face's file pcurves in four steps: each piece must lie along
//! its edge within the B-rep tolerance, run monotonically along it, chain end to start
//! around the loop (shifted by whole periods on a periodic surface), and stay inside the
//! face's UV domain. A non-periodic parameter cannot reach the last check: measured on
//! this rig, the wall face's v domain is the loop's extent widened by 5% (-0.200001 to
//! 4.200001 for the height 4), which is 20000 times the body's tolerance of 1e-5, so a
//! pcurve that leaves the domain in v is off its edge long before. It takes a periodic
//! parameter, where a piece can run past its edge and come back onto it.
//!
//! The case here is a cylinder whose two cap circles carry a pcurve that runs three
//! times round instead of once (u from 0 to 1080 degrees instead of 0 to 360). Every
//! point of it is on its edge, because the edge is a closed circle and the surface's u
//! is periodic; the loop still closes, because the two circles run opposite ways. Only
//! the UV domain check sees it: the wall face's u domain is one turn, and the piece
//! spans three.
//!
//! Three turns is the smallest whole number of them that reaches this check. A smaller
//! overrun fails one of the two earlier checks instead, and which one depends on which
//! of the pcurve's several parameters for a point of the edge the projection search
//! finds: measured on this rig, 1.05, 1.1, 1.2, 1.3 and 1.75 turns end off the edge's
//! ends, 1.25, 1.5, 2, 2.5 and 4 turns are not monotonic along the edge, and 3 and 5
//! turns are outside the domain.
//!
//! Either way the face keeps the kernel's projected UV trim curves and the body builds
//! exactly as it does without the file's pcurves, with the closed-form volume and area
//! of the cylinder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::PI;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives as api;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use step_brep::{brep_to_model, build_with_file_pcurves, BuildMode, BuildOptions};
use step_import::model::Model;

/// The cylinder's radius.
const RADIUS: f64 = 2.0;
/// The cylinder's height.
const HEIGHT: f64 = 4.0;

/// The reason `pcurves.rs` gives for a piece outside the domain.
const OUTSIDE: &str = "pcurve outside the face's UV domain";
/// The reason the two planar caps give: the exporter writes no pcurve for a plane, so
/// the cap's circle has none on the cap's own surface.
const NO_PCURVE: &str = "edge use without a pcurve on the face's surface";

/// Stitched builds only: the file's pcurves are used on the stitched build.
fn stitched(file_pcurves: bool) -> BuildOptions {
    BuildOptions {
        file_pcurves,
        mode: BuildMode::StitchOnly,
        ..BuildOptions::default()
    }
}

/// The imported model of an exported cylinder, with each pcurve that spans a whole turn
/// of u stretched to `turns` turns from its first point on.
///
/// `brep_to_model` samples the UV trims in the kernel's NURB parameters, where a
/// cylinder's u is an angle in radians, so a whole turn spans `2 * PI`; `step_export`
/// converts them to the file's degrees.
fn cylinder_file(turns: f64) -> Model {
    let mut kernel = Kernel::default();
    let cylinder =
        api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, 0.0), RADIUS, HEIGHT).unwrap();
    let mut model = brep_to_model(&cylinder).unwrap();
    let mut stretched = 0;
    for curves in model.pcurves.values_mut() {
        for points in curves.iter_mut() {
            let us = points.iter().map(|p| p[0]);
            let lo = us.clone().fold(f64::INFINITY, f64::min);
            let hi = us.fold(f64::NEG_INFINITY, f64::max);
            if (hi - lo - 2.0 * PI).abs() > 1e-6 {
                continue;
            }
            for p in points.iter_mut() {
                p[0] = lo + (p[0] - lo) * turns;
            }
            stretched += 1;
        }
    }
    // The wall's two cap circles; its seam uses run along v, not u.
    assert_eq!(stretched, 2, "turns {turns}");
    let text = step_export::write(&model).unwrap();
    step_import::import(text.as_bytes(), &step_import::Limits::default()).unwrap()
}

/// What one build of the file's single body gave.
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    counts: (usize, usize, usize),
    manifold: Option<bool>,
    level2_failures: Option<usize>,
}

fn measure(brep: &Brep) -> Outcome {
    let mut kernel = Kernel::default();
    let mut copy = brep.clone_standing_for_self().unwrap();
    let level2_failures = check_brep(&mut kernel, &mut copy, CheckLevel::Level2)
        .ok()
        .map(|check| check.failures().count());
    Outcome {
        counts: (
            brep.face_list().len(),
            brep.edge_list().len(),
            brep.vertex_list().len(),
        ),
        manifold: is_manifold_solid(brep).ok(),
        level2_failures,
    }
}

/// The body's volume and area.
fn properties(brep: &Brep) -> (f64, f64) {
    let kernel = Kernel::default();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = brep.clone_standing_for_self().unwrap();
    let volume = brep_volume(&kernel, &mut copy, 1e-9, &attrs).unwrap();
    let mut copy = brep.clone_standing_for_self().unwrap();
    let area = brep_area(&kernel, &mut copy, 1e-9).unwrap();
    (volume, area)
}

/// A one-turn pcurve is accepted: the wall face takes it, and no piece is found outside
/// the domain. This is the control for the case below.
#[test]
fn one_turn_pcurves_are_taken_by_the_wall_face() {
    let model = cylinder_file(1.0);
    let body = model.bodies.first().unwrap();
    let (_, stats) = build_with_file_pcurves(body, &model.file_pcurves, stitched(true)).unwrap();
    assert_eq!(stats.faces, 1, "{stats:?}");
    assert_eq!(stats.uses, 2, "{stats:?}");
    assert_eq!(stats.seam_uses, 2, "{stats:?}");
    assert_eq!(stats.fallbacks.get(OUTSIDE), None, "{stats:?}");
    assert_eq!(stats.fallbacks.get(NO_PCURVE), Some(&2), "{stats:?}");
}

/// Three turns of u for a one-turn edge: every sample is on the edge and the loop
/// closes, so only the UV domain check refuses the face.
#[test]
fn a_pcurve_three_times_round_the_cylinder_leaves_the_face_domain() {
    let model = cylinder_file(3.0);
    let body = model.bodies.first().unwrap();
    let (_, stats) = build_with_file_pcurves(body, &model.file_pcurves, stitched(true)).unwrap();
    assert_eq!(stats.faces, 0, "{stats:?}");
    assert_eq!(stats.uses, 0, "{stats:?}");
    assert_eq!(stats.fallbacks.get(OUTSIDE), Some(&1), "{stats:?}");
    assert_eq!(stats.curved_fallbacks.get(OUTSIDE), Some(&1), "{stats:?}");
    // Exactly one reason beyond the two planar caps': the one above.
    assert_eq!(stats.fallbacks.get(NO_PCURVE), Some(&2), "{stats:?}");
    assert_eq!(stats.fallbacks.len(), 2, "{stats:?}");
}

/// The refused face keeps the kernel's projected trims, so the body is the one the
/// build without the file's pcurves makes, with the cylinder's closed-form volume and
/// area.
#[test]
fn the_body_still_builds_with_its_closed_form_volume_and_area() {
    let model = cylinder_file(3.0);
    let body = model.bodies.first().unwrap();
    let (pcurves, _) = build_with_file_pcurves(body, &model.file_pcurves, stitched(true)).unwrap();
    let (plain, plain_stats) =
        build_with_file_pcurves(body, &model.file_pcurves, stitched(false)).unwrap();
    assert_eq!(plain_stats.faces, 0, "{plain_stats:?}");
    assert_eq!(measure(&pcurves), measure(&plain));
    assert_eq!(
        measure(&pcurves),
        Outcome {
            counts: (3, 3, 2),
            manifold: Some(true),
            // checked on a copy that keeps its geometry's owners (#2715): the 6 failures a
            // field-by-field clone gave were its 3 surfaces without owner and its 3 edges
            // failing `ValidateGeometry` for want of one
            level2_failures: Some(0),
        }
    );

    let (volume, area) = properties(&pcurves);
    let want_volume = PI * RADIUS * RADIUS * HEIGHT;
    let want_area = 2.0 * PI * RADIUS * RADIUS + 2.0 * PI * RADIUS * HEIGHT;
    assert!(
        (volume - want_volume).abs() <= 1e-9 * want_volume,
        "volume {volume}, closed form {want_volume}"
    );
    // The wall's facets leave the area a hundred-millionth above the closed form.
    assert!(
        (area - want_area).abs() <= 1e-8 * want_area,
        "area {area}, closed form {want_area}"
    );
    let (plain_volume, plain_area) = properties(&plain);
    assert_eq!((volume, area), (plain_volume, plain_area));
}
