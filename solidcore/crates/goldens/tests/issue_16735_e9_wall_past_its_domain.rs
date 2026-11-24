//! #16735: `blend.fillet_executive_p3`'s `stubs4-degenerate-brepForFillet2-e9-r0.5` body
//! has a B-spline wall (face 4) whose trim runs 3.3e-6 past its surface's natural domain,
//! `u = 1`. Clamped, that trim's image is the domain edge's point while its derivative is the
//! edge's partials, so the face's Green integral and the closure strip along it integrate
//! different curves: the strip's vector area was 2.9e-6 short in z (MEASURED per face:
//! strips -2.391e-5 against the edge path less the trims' -2.681e-5; every other face
//! agreed to 1e-12 or the probe's own 2e-8), and the volume moved with the origin, 2.9e-4
//! for a shift of 100 in z (`int (z - O_z) n_z dA`: only z shifts move it). The face and its
//! strips are now integrated on the surface continued past its domain (non-rational, so
//! certified; adapted from #15946 by @bherbruck): the shift moves it by 9.7e-9.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)] // test

#[path = "../../solidcore/tests/support/volume_leak.rs"]
mod volume_leak;

use goldens::case::repo_root;
use goldens::{registry, CaseContext};
use solidcore::math::Point3;
use solidcore::topology::Brep;
use solidcore::Kernel;

/// The body the case measures, an exact copy (`goldens::dump::capture`).
fn measured_body() -> Brep {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let reg = registry();
    let unit = reg
        .iter()
        .find(|u| u.id() == "blend.fillet_executive_p3")
        .unwrap();
    let cases = unit.cases(&ctx).unwrap();
    let case = cases
        .iter()
        .find(|c| c.name == "stubs4-degenerate-brepForFillet2-e9-r0.5")
        .unwrap();
    goldens::dump::capture();
    let out = unit.run_rust(case);
    let captured = goldens::dump::take();
    out.unwrap();
    captured.into_iter().next().unwrap().brep.unwrap()
}

/// The volume does not depend on the origin: shifts of 100 along z, x and y move it by no
/// more than `volume_leak`'s quadrature allowance, 1e-9 (V + 100 A).
#[test]
fn a_wall_trimmed_past_its_domain_is_closed() {
    let mut b = measured_body();
    let k = Kernel::default();
    let l = volume_leak::leak(&k, &mut b, 1e-10, 8);
    let o = Point3::new(0.0, 0.0, 0.0);
    let here = volume_leak::volume_about(&k, &mut b, o, 1e-10);
    for d in [
        Point3::new(0.0, 0.0, 100.0),
        Point3::new(100.0, 0.0, 0.0),
        Point3::new(0.0, 100.0, 0.0),
    ] {
        let there = volume_leak::volume_about(&k, &mut b, o + d, 1e-10);
        println!("shift {d:?}: {here} -> {there}");
        assert!(
            (there - here).abs() <= l.origin_allowance,
            "shift {d:?}: {here} -> {there}, allowance {:e}",
            l.origin_allowance
        );
    }
    assert!(l.origin_spread <= l.origin_allowance, "{l:?}");
}
