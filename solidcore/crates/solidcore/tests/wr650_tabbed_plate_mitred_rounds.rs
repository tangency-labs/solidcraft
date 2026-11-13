//! Regression test for WR-650 (the SolidCraft fork's tangency-labs/smlib@93db777 and
//! @b298cc2, #1986), from SolidCraft's tabbed-plate recipe: a disc of radius 25 with a
//! tab (two R2 corners) on top, extruded 2, its six top edges rounded r 0.5. The mitred
//! concave corners at (±4, y), where the tab's sides meet the R25 wall, trace onto the
//! wall's continuation past its arc, which `offset_extension_as_itself` carries on as the
//! cylinder itself (WR-633), in arc spans that stop at the closed turn (WR-650).
//!
//! SolidCraft runs it with its whole preset of fixes on; the round of the mitred corners
//! needs `fillet_mitre_gap_patch` and `fillet_mitre_concave_face_corners` (without them
//! the level 2 check reports `SmSurfOfRevolution` test 7, and 3.8e-4 of the volume is
//! missing), and with `offset_extension_as_itself` off the fillet fails (status 8).
//!
//! The body must be sound, the plate's volume twice its outline's area, and the removed
//! volume within 1e-6 relative of the round's closed form, integrated over the quarter
//! circle of the round's section.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const H: f64 = 2.0;
const R: f64 = 0.5;

fn theta() -> f64 {
    (4.0_f64 / 25.0).acos()
}

fn y0() -> f64 {
    (625.0_f64 - 16.0).sqrt()
}

fn cy() -> f64 {
    280.0 / 9.0
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
    Curve::from(
        create_arc(
            Vec3::new(c.0, c.1, 0.0),
            r,
            from.to_degrees(),
            to.to_degrees(),
        )
        .unwrap(),
    )
}

/// The tabbed plate, its outline chained head to tail, extruded to z 0..2.
fn plate(k: &mut Kernel) -> Brep {
    let (t, y, cy) = (theta(), y0(), cy());
    let curves = [
        // pi - theta to 2 pi + theta, a turn back (create_arc takes angles in [-360, 360])
        arc((0.0, 0.0), 25.0, PI - t - 2.0 * PI, t),
        line((4.0, y), (4.0, cy)),
        arc((2.0, cy), 2.0, 0.0, PI / 2.0),
        line((2.0, cy + 2.0), (-2.0, cy + 2.0)),
        arc((-2.0, cy), 2.0, PI / 2.0, PI),
        line((-4.0, cy), (-4.0, y)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

/// SolidCraft's outline area, its R25 wall moved in by `da` and the tab's walls by `dt`.
fn area(da: f64, dt: f64) -> f64 {
    let cy = cy();
    let big = 25.0 - da;
    let rho = 2.0 - dt;
    let tab = rho * rho * PI + 2.0 * rho * (2.0 + cy) + 4.0 * (cy + rho);
    let yc = (big * big - (4.0 - dt) * (4.0 - dt)).sqrt();
    let at = yc.atan2(4.0 - dt);
    0.5 * (big * big * (PI + 2.0 * at) + tab) + (4.0 - dt) * (cy - yc)
}

/// The volume the r 0.5 rounds take off, SolidCraft's integral over the round's section.
fn want_removed() -> f64 {
    let n = 8192;
    let dphi = (PI / 2.0) / f64::from(n);
    (0..n)
        .map(|i| {
            let phi = (f64::from(i) + 0.5) * dphi;
            let d = R * (1.0 - phi.cos());
            (area(0.0, 0.0) - area(d, d)) * R * phi.cos() * dphi
        })
        .sum()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// The six top edges, each the one passing within 0.01 of SolidCraft's pick point.
fn top_edges(k: &Kernel, b: &Brep) -> Vec<EdgeId> {
    let cy = cy();
    let picks = [
        Point3::new(0.0, -25.0, H),
        Point3::new(4.0, 27.8945, H),
        Point3::new(3.41421356, 32.5253247, H),
        Point3::new(0.0, cy + 2.0, H),
        Point3::new(-3.41421356, 32.5253247, H),
        Point3::new(-4.0, 27.8945, H),
    ];
    let near = |e: EdgeId, p: Point3| {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        (0..=4000).any(|i| {
            let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 4000.0;
            (curve.evaluate_point(k, t).unwrap() - p).length() < 0.01
        })
    };
    picks
        .iter()
        .map(|&p| {
            let found: Vec<EdgeId> = b
                .edge_list()
                .iter()
                .copied()
                .filter(|&e| near(e, p))
                .collect();
            assert_eq!(found.len(), 1, "edges near the pick {p:?}: {found:?}");
            found[0]
        })
        .collect()
}

/// The plate's volume, then the rounded body's soundness (a manifold solid passing the
/// level 2 check) and the volume the rounds took off, or the fillet error code, with
/// SolidCraft's preset of fixes (all 182 of `solidcraft-flags/612ac9a36`) on.
fn round() -> (f64, Result<(bool, f64), i32>) {
    let mut k = Kernel::default();
    k.fixes.analytic_copy_keeps_parameterization = true;
    k.fixes.classify_edge_interval = true;
    k.fixes.curve_contacts_at_curve_ends = true;
    k.fixes.curve_loop_normal_from_area = true;
    k.fixes.face_tight_box_searches_trim = true;
    k.fixes.refine_cc_range_ends = true;
    k.fixes.simple_coincidence_crossing_is_point = true;
    let mut b = plate(&mut k);
    let before = volume(&k, &mut b);
    let edges = top_edges(&k, &b);
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, R, 1, 1, 1.0)
            .unwrap();
        if session.do_fillet(&mut k).is_err() {
            return (before, Err(session.error_info().code));
        }
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let sound = is_manifold_solid(&b).unwrap()
        && check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .is_valid();
    let after = volume(&k, &mut b);
    (before, Ok((sound, before - after)))
}

#[test]
fn wr650_tabbed_plate_rounds_mitred_on_the_arcs_continuation() {
    let plate_want = H * area(0.0, 0.0);
    let want = want_removed();
    let (before, rounded) = round();
    assert!(
        (before - plate_want).abs() <= 1e-10 * plate_want,
        "plate volume {before}, expected {plate_want}"
    );
    let (sound, removed) = rounded.unwrap_or_else(|code| panic!("the rounds failed: {code}"));
    assert!(sound, "the rounded plate is not sound");
    assert!(
        (removed - want).abs() <= 1e-6 * want,
        "removed {removed}, expected {want}"
    );
}
