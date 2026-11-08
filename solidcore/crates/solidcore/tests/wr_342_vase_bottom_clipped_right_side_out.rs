//! Regression test for WR-342 (tangency-labs/solidcore#974, the fork's commit "an inset
//! face that does not fit its offset surface's domain is clipped right side out";
//! SolidCraft rig shell/4567): a vase whose profile meets the axis at a slant, shelled
//! 2 mm inward and open at the top.
//!
//! The vase is a full turn of the profile (0, 0) - (15, 5) - (15, 30) - (0, 30) in the
//! XY plane about the y axis: a conical bottom whose generator starts on the axis, a
//! cylindrical wall and a flat top. The bottom's inward offset generator crosses the
//! axis, so (with WR-343's `revolution_offset_axis_both_sides`) the offset surface is
//! cut there and its domain no longer holds the face's. `BuildOffsetOfFaceuse` then
//! clips the face to the offset domain with a 2D boolean of planar faces made from the
//! face's UV loops. Taken from the inset's opposite face use, upstream's loops run
//! clockwise, the planar face is inside out, the intersection is empty and the bottom
//! gets no offset: the shell comes back open, of no volume. With
//! `offset_clip_right_side_out` the bottom is offset and the shell is closed, its
//! volume the vase's less the cavity bounded by the exact offsets (a cylinder r 13 and
//! the bottom cone moved 2 along its normal):
//! `pi R^2 (h/3 + H - h) - pi (R - 2)^2 ((H - y1) + (y1 - y0) / 3)`, with
//! `y0 = 2 / cos a` and `y1 = (h / R)(R - 2) + y0`, `tan a = h / R`.
//! The offsets are B-spline approximations, so the volume is held to 1e-5 relative.
//! The vase is revolved both ways round the axis (the rig's was about -y).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell, volume};
use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::{Brep, Kernel};

/// Wall radius, height of the conical bottom, height of the vase.
const R: f64 = 15.0;
const HB: f64 = 5.0;
const H: f64 = 30.0;

/// The vase revolved about +y (`up`) or -y.
fn vase(k: &mut Kernel, up: bool) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let profile = [
        (p(0.0, 0.0), p(R, HB)),
        (p(R, HB), p(R, H)),
        (p(R, H), p(0.0, H)),
        (p(0.0, H), p(0.0, 0.0)),
    ];
    let curves: Vec<Curve> = profile
        .iter()
        .map(|&(a, b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    let curves: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let (origin, axis) = if up {
        (p(0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
    } else {
        (p(0.0, H), Vec3::new(0.0, -1.0, 0.0))
    };
    create_rotational_sweep(k, &curves, origin, axis, 360.0, true).unwrap()
}

/// The vase shelled 2 inward, open at its top, with WR-342's `fixed` (WR-343 on).
fn shelled(up: bool, fixed: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.revolution_offset_axis_both_sides = true;
    k.fixes.offset_clip_right_side_out = fixed;
    let part = vase(&mut k, up);
    let open = planar_faces_at(&k, &part, 1, H);
    assert_eq!(open.len(), 1, "the top");
    let b = shell(&mut k, part, -2.0, &open).unwrap();
    (k, b)
}

fn closed_form() -> f64 {
    let a = (HB / R).atan();
    let y0 = 2.0 / a.cos();
    let y1 = HB / R * (R - 2.0) + y0;
    let vase = PI * R * R * (HB / 3.0 + H - HB);
    let cavity = PI * (R - 2.0).powi(2) * ((H - y1) + (y1 - y0) / 3.0);
    vase - cavity
}

#[test]
fn wr_342_vase_shells_closed_with_the_fix() {
    for up in [true, false] {
        let (k, mut b) = shelled(up, true);
        assert!(brep_is_manifold_solid(&b).unwrap(), "up {up}");
        let v = volume(&k, &mut b).unwrap();
        let exact = closed_form();
        assert!(
            ((v - exact) / exact).abs() < 1e-5,
            "up {up}: {v} against {exact}"
        );
    }
}

/// Upstream: the bottom gets no offset, and the shell is open, of no volume.
#[test]
fn wr_342_default_leaves_the_bottom_without_an_offset() {
    for up in [true, false] {
        let (k, mut b) = shelled(up, false);
        assert!(!brep_is_manifold_solid(&b).unwrap(), "up {up}");
        assert!(volume(&k, &mut b).unwrap().abs() < 1e-9, "up {up}");
    }
}
