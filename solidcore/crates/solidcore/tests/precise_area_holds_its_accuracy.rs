//! The precise-property integration holds the accuracy it is asked for. A box's face swept
//! 90 degrees round the z axis through its corner makes four quarter annuli of radii 5 and 5 sqrt 2
//! (area 6.25 pi). Two of them lie on a plane whose UV directions run at 45 degrees, so
//! each iso-u integral runs across the annulus and the inner arc turns in u at its
//! middle. There the integration missed its accuracy for two reasons:
//! - at the arc's middle `du/dt = 0`, and the integrand was left as the previous step's
//!   start value (upstream), so the step from there began from a stale slope (-7.7e-2
//!   relative at accuracy 1e-2);
//! - a first step of half the arc was accepted on the embedded error estimate, which
//!   read 3.1e-5 where the step was off by 3.5e-3 (-5.1e-4 relative at 1e-4).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::mass::integrator::properties::face_area;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_brep::{non_manifold_rotational_sweep, SweepSelection};
use solidcore::topology::brep::p1::faces;
use solidcore::Kernel;
use std::f64::consts::PI;

/// `boolean.api_brep`'s `rotational_sweep_face0`: box 5, its first face swept 90 degrees
/// round the z axis through the origin.
#[test]
fn quarter_annuli_hold_every_requested_accuracy() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 5.0, 5.0).unwrap();
    let face = b.face_list()[0];
    let selected = [Some(face)];
    let selection = SweepSelection {
        faces: Some(&selected),
        ..SweepSelection::default()
    };
    non_manifold_rotational_sweep(
        &mut k,
        &mut b,
        None,
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        90.0,
        true,
        false,
        selection,
    )
    .unwrap();
    let exact = 6.25 * PI;
    let mut annuli = 0;
    for f in faces(&b, None).unwrap() {
        if (face_area(&k, &mut b, f, 1e-10).unwrap() - exact).abs() > 1e-8 * exact {
            continue;
        }
        annuli += 1;
        for accuracy in [1e-2, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8] {
            let a = face_area(&k, &mut b, f, accuracy).unwrap();
            let rel = (a - exact) / exact;
            assert!(
                rel.abs() <= accuracy,
                "face {f:?} at accuracy {accuracy:e}: area {a} is off by {rel:e}"
            );
        }
    }
    assert_eq!(annuli, 4);
}
