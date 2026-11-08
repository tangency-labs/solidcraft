//! tangency-labs/solidcore#410: a cylinder minus a thread groove leaves edge ends about
//! 1e-5 from their vertices against a tolerance of 5e-6, so SMLib's own check reports
//! them (`SmBrep` tests #0 and #2, "Bad Gap"; past the edge's and vertex's tolerances
//! together, test #61 "Bad Edge").
//!
//! The grooves are cut by a screw motion: the section in a meridian plane, sixteen
//! sections a turn, skinned cubically from a pitch below the body to a pitch above it.
//! (`create_curve_sweep` along a helix is not a thread: its frame twists about 21 degrees
//! a turn, so its profile runs into the body. The SolidCraft rig uses the fork's screw
//! sweep, which SMLib does not have.)
//!
//! An M33x3.5 groove on a 14 mm shaft (the rig's case 2193): upstream, two edges end
//! 1.52e-5 from their vertices (the C++ reference reports the same two test #0 lines on
//! the same body); with `Kernel::fixes.imprint_trims_edge_to_proto_vertex` and
//! `imprint_extends_edge_to_proto_vertex` (WR-452, WR-453) every edge ends within 1.2e-6
//! of its vertex. An internal M10x1.5 thread (a nut of radius 8 bored to the minor
//! radius 4.188, the groove out to the major radius 5) has no gap over half a tolerance
//! either way, as in the C++.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use std::f64::consts::PI;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel};

/// A groove: the section's (radius, axial offset) corners moved by a screw motion of
/// pitch `p` from `z0` over `turns` turns, sixteen sections a turn, skinned with degree 3
/// and capped.
fn screw_groove(k: &mut Kernel, p: f64, z0: f64, turns: f64, prof: &[(f64, f64)]) -> Brep {
    const PER_TURN: usize = 16;
    let n = (turns * PER_TURN as f64).round() as usize;
    let mut curves = Vec::new();
    for i in 0..=n {
        let turn = i as f64 / PER_TURN as f64;
        let (s, c) = (2.0 * PI * turn).sin_cos();
        let pts: Vec<Point3> = prof
            .iter()
            .map(|&(r, z)| Point3::new(r * c, r * s, z0 + z + p * turn))
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

fn kernel(fixes: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.imprint_trims_edge_to_proto_vertex = fixes;
    k.fixes.imprint_extends_edge_to_proto_vertex = fixes;
    k
}

/// An external thread: a shaft of radius `r` and length `len` minus a groove of pitch `p`,
/// `depth` deep with a root flat `flat` wide, its flanks at 30 degrees, to `r + 0.05`.
fn external(k: &mut Kernel, p: f64, r: f64, len: f64, depth: f64, flat: f64) -> Brep {
    let root = r - depth;
    let outer = r + 0.05;
    let half_root = flat / 2.0;
    let half_outer = half_root + (outer - root) * 30.0_f64.to_radians().tan();
    let prof = [
        (root, -half_root),
        (outer, -half_outer),
        (outer, half_outer),
        (root, half_root),
    ];
    let shaft = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), r, len).unwrap();
    let groove = screw_groove(k, p, -p, (len + 2.0 * p) / p, &prof);
    boolean_difference(k, shaft, groove).unwrap()
}

/// The internal M10x1.5 thread: a nut of radius 8 and length 12 bored to 4.188, minus a
/// groove from 0.05 inside the bore out to the major radius 5, p/8 wide there.
fn internal_m10(k: &mut Kernel) -> Brep {
    let (p, len) = (1.5, 12.0);
    let nut = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 8.0, len).unwrap();
    let bore = create_cylinder(k, Point3::new(0.0, 0.0, -1.0), 4.188, len + 2.0).unwrap();
    let nut = boolean_difference(k, nut, bore).unwrap();
    let (inner, major) = (4.188 - 0.05, 5.0);
    let half_major = p / 16.0;
    let half_inner = half_major + (major - inner) * 30.0_f64.to_radians().tan();
    let prof = [
        (inner, -half_inner),
        (major, -half_major),
        (major, half_major),
        (inner, half_inner),
    ];
    let groove = screw_groove(k, p, -p, (len + 2.0 * p) / p, &prof);
    boolean_difference(k, nut, groove).unwrap()
}

/// The largest distance of an edge's end from its vertex, and that vertex's tolerance.
fn worst_vertex_gap(k: &Kernel, b: &Brep) -> (f64, f64) {
    let (mut worst, mut tol) = (0.0_f64, 0.0);
    for e in p1::edges(b, None).unwrap() {
        let Some(curve) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let interval = edge::interval(b, e).unwrap();
        let ends = [
            (edge::start_vertex(b, e).unwrap(), interval.min()),
            (edge::end_vertex(b, e).unwrap(), interval.max()),
        ];
        for (v, t) in ends {
            let Some(v) = v else { continue };
            let gap = curve
                .evaluate_point(k, t)
                .unwrap()
                .distance(vertex::point(b, v).unwrap());
            if gap > worst {
                (worst, tol) = (gap, vertex::tolerance(b, v).unwrap());
            }
        }
    }
    (worst, tol)
}

/// The check's whole-body (`SmBrep`) report lines: the gap and bad-edge tests.
fn brep_reports(k: &mut Kernel, b: &mut Brep) -> Vec<String> {
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    lines
        .into_iter()
        .filter(|l| l.starts_with("SmBrep::"))
        .collect()
}

#[test]
fn issue_410_m33x3_5_upstream_leaves_vertex_gaps() {
    let mut k = kernel(false);
    let mut b = external(&mut k, 3.5, 16.5, 14.0, 2.15, 0.875);
    let (gap, tol) = worst_vertex_gap(&k, &b);
    assert!(gap > tol / 2.0, "gap {gap:e}, tolerance {tol:e}");
    let lines = brep_reports(&mut k, &mut b);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert!(
        lines.iter().all(|l| l.starts_with("SmBrep::Test #0 ")),
        "{lines:#?}"
    );
}

#[test]
fn issue_410_m33x3_5_edges_end_at_their_vertices_with_the_imprint_fixes() {
    let mut k = kernel(true);
    let mut b = external(&mut k, 3.5, 16.5, 14.0, 2.15, 0.875);
    let (gap, tol) = worst_vertex_gap(&k, &b);
    assert!(gap < 2e-6, "gap {gap:e}, tolerance {tol:e}");
    let lines = brep_reports(&mut k, &mut b);
    assert!(lines.is_empty(), "{lines:#?}");
}

#[test]
fn issue_410_internal_m10x1_5_has_no_vertex_gap() {
    for fixes in [false, true] {
        let mut k = kernel(fixes);
        let mut b = internal_m10(&mut k);
        let (gap, tol) = worst_vertex_gap(&k, &b);
        assert!(
            gap < tol / 2.0,
            "fixes {fixes}: gap {gap:e}, tolerance {tol:e}"
        );
        let lines = brep_reports(&mut k, &mut b);
        assert!(lines.is_empty(), "fixes {fixes}: {lines:#?}");
    }
}
