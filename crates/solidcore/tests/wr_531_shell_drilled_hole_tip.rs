//! Regression test for WR-531 (the fork's tangency-labs/smlib@d5dadaa, capi test
//! `shelled_box_with_a_drilled_hole`): a box with a blind drilled hole (r 3, a 118
//! degree tip) shelled 2 mm inward, open at a side and, separately, at the top. The
//! hole's tip is a cone's apex whose seam is its only edge: the extended offset builds
//! no offset there, so the cone's offset ends open on a circle. `ShellBrep` then hands
//! back the offset as it is, not a closed solid (upstream returns `SM_SUCCESS` with that
//! non-manifold result, `SmOffsetExecutive.cpp:255-262`); the fork's shell fails (1001)
//! because its WR-454 check (`shell_empty_cavity_fails`, tangency-labs/smlib@fbe43bb)
//! refuses that open offset. With `offset_point_vertex_caps` the apex's offset is a
//! sphere zone of radius 2 and the shell is a closed solid.
//!
//! SMLib's level-2 check (`check_brep`) of the result reports exactly the lines of
//! #1670 (open, fixed by #1674): a merged shell result is labelled
//! `NamingScheme::Notify` while its edges' curves are owned under the caller's merge
//! and result identities, so `SmBrep` test 61's owner comparison fails on those edges
//! and `AssertValid` is skipped. Any other report line fails the test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// A 40 x 40 x 20 box with a hole r 3 drilled 10 deep from the top at (20, 20), its
/// tip a 118 degree cone.
fn drilled_box(k: &mut Kernel) -> Brep {
    let (r, top, depth) = (3.0, 20.0, 10.0);
    let tip = r / 59.0_f64.to_radians().tan();
    let bottom = top - depth;
    let at = |x: f64, z: f64| Point3::new(20.0 + x, 20.0, z);
    let profile = [
        (at(0.0, bottom - tip), at(r, bottom)),
        (at(r, bottom), at(r, top + 1.0)),
        (at(r, top + 1.0), at(0.0, top + 1.0)),
        (at(0.0, top + 1.0), at(0.0, bottom - tip)),
    ];
    let curves: Vec<Curve> = profile
        .iter()
        .map(|&(a, b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let curves: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let drill = create_rotational_sweep(
        k,
        &curves,
        at(0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        360.0,
        true,
    )
    .unwrap();
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    boolean_difference(k, block, drill).unwrap()
}

/// The drilled box shelled 2 inward, open at the top or at x = 0, with WR-531's `fixed`
/// and WR-454's `refuse_open`.
fn shelled(fixed: bool, refuse_open: bool, open_top: bool) -> solidcore::Result<(Kernel, Brep)> {
    let mut k = Kernel::default();
    k.fixes.offset_point_vertex_caps = fixed;
    k.fixes.shell_empty_cavity_fails = refuse_open;
    let part = drilled_box(&mut k);
    let open = if open_top {
        planar_faces_at(&k, &part, 2, 20.0)
    } else {
        planar_faces_at(&k, &part, 0, 0.0)
    };
    assert_eq!(open.len(), 1, "the open face");
    let b = shell(&mut k, part, -2.0, &open)?;
    Ok((k, b))
}

/// The level-2 check's failure count and report lines.
fn check(k: &mut Kernel, b: &mut Brep) -> (usize, Vec<String>) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    (c.failure_count(), c.lines())
}

/// #1670's report of an edge whose curve is owned under the caller's identities.
const TEST_61_OWNER: &str =
    "SmBrep::Test #61 Valid: Bad Edge fails pE->ValidateGeometry(FALSE) check [owner SmEdge]";

/// #1670's report lines for `edges` such edges: one test 61 line each, then the
/// skipped walk.
fn issue_1670_lines(edges: usize) -> Vec<String> {
    let mut lines = vec![TEST_61_OWNER.to_string(); edges];
    lines.push("AssertValid skipped: ValidatePointers failed".to_string());
    lines
}

fn spheres(b: &Brep) -> usize {
    b.face_list()
        .iter()
        .filter(|&&f| {
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Sphere(_)))
        })
        .count()
}

#[test]
fn wr_531_drilled_hole_shells_with_a_sphere_cap() {
    // TODO(#1670): return to a clean check_brep once tangency-labs/solidcore#1674 merges.
    // Each case's count of #1670 edges, side open then top open, without and with
    // WR-454's check (which leaves a closed result alone).
    let issue_1670_edges = [[18, 18], [18, 18]];
    let mut checked = Vec::new();
    for (open_top, edges) in [false, true].into_iter().zip(issue_1670_edges) {
        for (refuse_open, edges) in [false, true].into_iter().zip(edges) {
            let case = format!("open_top {open_top} refuse_open {refuse_open}");
            let (mut k, mut b) = shelled(true, refuse_open, open_top).unwrap();
            assert!(
                brep_is_manifold_solid(&b).unwrap(),
                "{case}: not a closed solid"
            );
            assert_eq!(spheres(&b), 1, "{case}: one cap on a sphere");
            let (failed, lines) = check(&mut k, &mut b);
            eprintln!("wr_531 {case}: {failed} failures {lines:?}");
            checked.push((case, failed, lines, edges));
        }
    }
    // (every case is printed before the first comparison)
    for (case, failed, lines, edges) in checked {
        let want = issue_1670_lines(edges);
        assert_eq!(failed, want.len(), "{case}: {lines:?}");
        assert_eq!(lines, want, "{case}");
    }
}

/// Without the opt-in the cone's offset stays open at the tip: the pinned `ShellBrep`
/// hands it back as a success, no closed solid, and WR-454's check refuses it (the
/// fork's failing shell).
#[test]
fn wr_531_without_the_opt_in_the_shell_is_open() {
    for open_top in [false, true] {
        let (_, b) = shelled(false, false, open_top).unwrap();
        assert!(!brep_is_manifold_solid(&b).unwrap(), "open_top {open_top}");
        assert_eq!(spheres(&b), 0, "open_top {open_top}");
        assert!(
            shelled(false, true, open_top).is_err(),
            "open_top {open_top}"
        );
    }
}
