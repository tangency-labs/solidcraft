//! tangency-labs/solidcore#400 (regression): a box joined with a cylinder whose seam lies on
//! a box face shells into a body that fails SMLib's check (test #61, bad edge; rig case
//! shell/1494). Not fixed in the fork; the Rust port shells it into a manifold solid of
//! the closed form's volume (plan/findings/offset.offset_executive_p2--issue-400-865-shell-probes.md).
//!
//! The rig's part is not public, so this is a reconstruction: a 40 x 40 x 20 box and an
//! r10 cylinder about (30, 20) from z = -10 to 30, which touches the box face x = 40
//! along the line x = 40, y = 20 (the cylinder's seam when it starts on +x) and sticks
//! out 10 above and below the box. Shelled closed 2 inward, the offsets meet sharply,
//! so the cavity is the box shrunk by 2 joined with the cylinder shrunk by 2:
//! 36 x 36 x 16 plus an r8 cylinder over the 20 of its length outside that box.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{report, shell, volume};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::{Brep, Kernel};

fn part(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let post = create_cylinder(k, Point3::new(30.0, 20.0, -10.0), 10.0, 40.0).unwrap();
    boolean_union(k, block, post).unwrap()
}

#[test]
fn issue_400_part_input() {
    let mut k = Kernel::default();
    let mut b = part(&mut k);
    let want = 32_000.0 + 2_000.0 * PI;
    let got = volume(&k, &mut b).unwrap();
    eprintln!(
        "PROBE issue_400 input: faces={} volume={got} want={want}",
        b.face_list().len()
    );
    assert!((got - want).abs() <= 1e-5 * want, "{got} vs {want}");
}

#[test]
fn issue_400_closed_shell() {
    let mut k = Kernel::default();
    let b = part(&mut k);
    let outer = 32_000.0 + 2_000.0 * PI;
    let cavity = 36.0 * 36.0 * 16.0 + PI * 64.0 * 20.0;
    let result = shell(&mut k, b, -2.0, &[]);
    assert!(report(
        &k,
        "issue_400 closed",
        result,
        Some(outer - cavity),
        1e-5
    ));
}
