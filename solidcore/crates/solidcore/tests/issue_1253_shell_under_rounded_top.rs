//! tangency-labs/solidcore#1253: SolidCraft ladder case 09/010
//! (`tests/kernel-cases/ladder/09-shells-and-offsets/010-shell-under-rounded-top`): a
//! 60 x 40 x 20 box with its four top edges rounded r3, shelled 2 mm inward open at the
//! bottom. The SMLib fork built without fused multiply-adds (`-ffp-contract=off`) refuses
//! the result in its validity check (`SmBSplineCurve::Test` #13, a 2D B-spline with an
//! extremely small radius of curvature, 3.8e-6 inside a 1e-4 band); with fused
//! multiply-adds it passes. The Rust port never fuses, and builds a manifold solid of the
//! closed-form volume whose B-spline curves all pass `AssertValid`.
//!
//! The rounds meet at the top corners in mitres (the vertical edges stay sharp), so the
//! part is the box less `(1 - pi/4) r^2` per unit length of rounded edge, plus the
//! `r^3 (5/3 - pi/2)` each mitre corner removed twice. The shell's cavity is the same
//! shape at 56 x 36 x 18 with r1 rounds, open through the removed bottom face.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{planar_faces_at, report, shell, volume};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::CurveKind;
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::{Brep, Kernel};

/// An `l x w x h` box with its four top edges rounded `r`, mitred at the corners.
fn top_rounded_box(l: f64, w: f64, h: f64, r: f64) -> f64 {
    l * w * h - (1.0 - PI / 4.0) * r * r * (2.0 * l + 2.0 * w)
        + 4.0 * r * r * r * (5.0 / 3.0 - PI / 2.0)
}

fn part(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let edges: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(k, e.interval.min()).unwrap();
            let q = c.evaluate_point(k, e.interval.max()).unwrap();
            (p.z - 20.0).abs() < 1e-9 && (q.z - 20.0).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(edges.len(), 4, "the four top edges");
    // `SmApiFilletEdges`, circular cross-section
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

#[test]
fn issue_1253_part_input() {
    let mut k = Kernel::default();
    let mut b = part(&mut k);
    let want = top_rounded_box(60.0, 40.0, 20.0, 3.0);
    let got = volume(&k, &mut b).unwrap();
    assert!((got - want).abs() <= 1e-6 * want, "{got} vs {want}");
}

#[test]
fn issue_1253_open_bottom_shell() {
    let mut k = Kernel::default();
    let b = part(&mut k);
    let bottom = planar_faces_at(&k, &b, 2, 0.0);
    assert_eq!(bottom.len(), 1, "the bottom face");
    let result = shell(&mut k, b, -2.0, &bottom).unwrap();

    // the check the unfused C++ fails: `SmBSplineCurve::AssertValid` on every B-spline
    // edge curve and trimming curve of the shell
    let curves = result
        .edgeuses()
        .iter()
        .filter_map(|(_, u)| u.uv_trim_curve.as_ref())
        .chain(result.edges().iter().filter_map(|(_, e)| e.curve.as_ref()));
    let mut checked = 0;
    for c in curves {
        if let CurveKind::BSpline(bs) = c.kind() {
            checked += 1;
            let mut list = AssertArray::new();
            let ok = bs
                .assert_valid(&k, Some(&mut list), false, ObjectKey(0))
                .unwrap();
            let failed: Vec<_> = list.reports().iter().filter(|r| !r.ok).collect();
            assert!(ok && failed.is_empty(), "{failed:?}");
        }
    }
    assert!(checked > 0, "the shell has B-spline curves to check");

    let want = top_rounded_box(60.0, 40.0, 20.0, 3.0) - top_rounded_box(56.0, 36.0, 18.0, 1.0);
    assert!(report(
        &k,
        "issue_1253 open bottom",
        Ok(result),
        Some(want),
        1e-6
    ));
}
