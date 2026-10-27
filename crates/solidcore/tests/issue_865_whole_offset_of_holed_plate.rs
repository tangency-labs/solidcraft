//! tangency-labs/solidcore#865 (regression): the ladder 09/011 part fails shelled closed
//! (1001) and its `SmApiOffsetBrepFull` is a body of volume 0, in the SMLib fork. The
//! fork traced both to #977 (`RemoveExcessFaces` before the merge), whose closed-shell
//! case the Rust port covers with `issue_977_closed_shell_excess_faces_after_merge`.
//! The ladder part is not public; this probe takes the whole offset of #977's plate
//! (30 x 20 x 6, an r1.5 through hole 1 from the front side) 1.5 inward, by default and
//! with the #977 and #854 opt-ins (with them, a manifold solid of the closed form's volume;
//! plan/findings/offset.offset_executive_p2--issue-400-865-shell-probes.md).
//!
//! The inward offset is the plate shrunk by 1.5 (27 x 17 x 3) less the hole's offset
//! (r 3 about (15, 2.5)), which crosses the front's offset at y = 1.5.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{offset_full, report};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::{Brep, Kernel};

fn plate(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 6.0).unwrap();
    let hole = create_cylinder(k, Point3::new(15.0, 2.5, -1.0), 1.5, 8.0).unwrap();
    boolean_difference(k, plate, hole).unwrap()
}

fn want() -> f64 {
    let t: f64 = 1.5;
    let r = 1.5 + t;
    let d = 2.5 - t; // the hole offset's centre from the front's offset
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let disc_inside = PI * r * r - segment;
    ((30.0 - 2.0 * t) * (20.0 - 2.0 * t) - disc_inside) * (6.0 - 2.0 * t)
}

fn run(fixed: bool) -> bool {
    let mut k = Kernel::default();
    k.fixes.offset_excess_faces_after_merge = fixed;
    k.fixes.offset_resolve_crossing_loops = fixed;
    let mut b = plate(&mut k);
    let result = offset_full(&mut k, &mut b, -1.5);
    report(
        &k,
        &format!("issue_865 whole offset -1.5, opt-ins={fixed}"),
        result,
        Some(want()),
        1e-6,
    )
}

#[test]
fn issue_865_whole_offset_default() {
    // upstream behaviour: reported only
    let _ = run(false);
}

#[test]
fn issue_865_whole_offset_with_fixes() {
    assert!(run(true));
}
