//! Mass properties of the primitives against their closed forms, a sheet's, and the
//! rejected arguments.
//!
//! Source: tangency-labs/smlib `capi/tests/measure.cpp` (Apache-2.0, Copyright SolidCraft
//! contributors), replay/1.0.1 at f8fd83e. The C test's NURBS-arc box check is a kernel test, not a C
//! API one, and is left out.

#![allow(clippy::unwrap_used)] // test helpers

use std::f64::consts::PI;

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_plane;
use solidcore_smlib_api::{Brep, Smlib, Status};

/// Closed-form values: area, volume, centroid, `Ixx Iyy Izz` and `Iyz Izx Ixy` about the
/// centroid.
struct Expected {
    name: &'static str,
    body: Brep,
    v: [f64; 11],
}

fn bx(api: &mut Smlib, name: &'static str, o: [f64; 3], a: f64, b: f64, c: f64) -> Expected {
    let v = a * b * c;
    Expected {
        name,
        body: api.make_box(o, [a, b, c]).unwrap(),
        v: [
            2.0 * (a * b + b * c + c * a),
            v,
            o[0] + a / 2.0,
            o[1] + b / 2.0,
            o[2] + c / 2.0,
            v * (b * b + c * c) / 12.0,
            v * (a * a + c * c) / 12.0,
            v * (a * a + b * b) / 12.0,
            0.0,
            0.0,
            0.0,
        ],
    }
}

fn cylinder(api: &mut Smlib, name: &'static str, o: [f64; 3], r: f64, h: f64) -> Expected {
    let v = PI * r * r * h;
    let ixx = v * (3.0 * r * r + h * h) / 12.0;
    Expected {
        name,
        body: api.make_cylinder(o, r, h).unwrap(),
        v: [
            2.0 * PI * r * r + 2.0 * PI * r * h,
            v,
            o[0],
            o[1],
            o[2] + h / 2.0,
            ixx,
            ixx,
            v * r * r / 2.0,
            0.0,
            0.0,
            0.0,
        ],
    }
}

fn sphere(api: &mut Smlib, name: &'static str, o: [f64; 3], r: f64) -> Expected {
    let v = 4.0 / 3.0 * PI * r * r * r;
    let i = 0.4 * v * r * r;
    Expected {
        name,
        body: api.make_sphere(o, r).unwrap(),
        v: [
            4.0 * PI * r * r,
            v,
            o[0],
            o[1],
            o[2],
            i,
            i,
            i,
            0.0,
            0.0,
            0.0,
        ],
    }
}

/// A frustum (or a cone, `rt = 0`): its volume integrals are polynomials in z of degree at
/// most 4, so three-point Gauss-Legendre gives them exactly.
fn cone(api: &mut Smlib, name: &'static str, o: [f64; 3], rb: f64, rt: f64, h: f64) -> Expected {
    let gx = [-(0.6f64.sqrt()), 0.0, 0.6f64.sqrt()];
    let gw = [5.0 / 9.0, 8.0 / 9.0, 5.0 / 9.0];
    let (mut v, mut mz, mut izz, mut zz) = (0.0, 0.0, 0.0, 0.0);
    for (x, g) in gx.iter().zip(gw) {
        let z = h / 2.0 * (1.0 + x);
        let w = g * h / 2.0;
        let rho = rb + (rt - rb) * z / h;
        let disc = PI * rho * rho;
        v += w * disc;
        mz += w * disc * z;
        zz += w * disc * z * z;
        izz += w * PI * rho * rho * rho * rho / 2.0;
    }
    let zc = mz / v;
    let ixx = izz / 2.0 + zz - v * zc * zc;
    let s = ((rb - rt) * (rb - rt) + h * h).sqrt();
    Expected {
        name,
        body: api.make_cone(o, rb, rt, h).unwrap(),
        v: [
            PI * rb * rb + PI * rt * rt + PI * (rb + rt) * s,
            v,
            o[0],
            o[1],
            o[2] + zc,
            ixx,
            ixx,
            izz,
            0.0,
            0.0,
            0.0,
        ],
    }
}

/// A torus about the z axis (Pappus for area and volume).
fn torus(api: &mut Smlib, name: &'static str, o: [f64; 3], big: f64, r: f64) -> Expected {
    let v = 2.0 * PI * PI * big * r * r;
    let ixx = v * (big * big / 2.0 + 5.0 * r * r / 8.0);
    Expected {
        name,
        body: api.make_torus(o, big, r).unwrap(),
        v: [
            4.0 * PI * PI * big * r,
            v,
            o[0],
            o[1],
            o[2],
            ixx,
            ixx,
            v * (big * big + 3.0 * r * r / 4.0),
            0.0,
            0.0,
            0.0,
        ],
    }
}

/// The primitives whose faces are planes, cylinders and cones, against their closed forms.
#[test]
fn primitives_against_closed_forms() {
    let mut api = Smlib::default();
    let at0 = [0.0; 3];
    let far = [1000.0, -2000.0, 500.0];
    let cases = [
        bx(&mut api, "oracle 01-box 40x30x20", at0, 40.0, 30.0, 20.0),
        cylinder(&mut api, "oracle 02-cylinder r15 h50", at0, 15.0, 50.0),
        cylinder(&mut api, "oracle 38 cylinder r10 h50", at0, 10.0, 50.0),
        cone(&mut api, "frustum r10..4 h20", at0, 10.0, 4.0, 20.0),
        cone(&mut api, "cone r10 h20", at0, 10.0, 0.0, 20.0),
        cone(&mut api, "inverted frustum r3..9 h7", at0, 3.0, 9.0, 7.0),
        bx(&mut api, "box far", far, 3.0, 50.0, 0.5),
        cylinder(&mut api, "cylinder far", far, 0.25, 80.0),
        cone(&mut api, "frustum far", far, 6.0, 2.0, 9.0),
    ];
    against_closed_forms(&mut api, &cases);
}

/// The sphere and torus (doubly curved faces) against their closed forms. At 1e-8 these
/// take about 12 s each unoptimised and about 0.2 s optimised, so CI skips this test in
/// its unoptimised `cargo test --workspace` step and runs it in the difftest-profile step
/// (`cargo test --profile difftest -p solidcore-smlib-api --test measure`).
#[test]
fn doubly_curved_against_closed_forms() {
    let mut api = Smlib::default();
    let at0 = [0.0; 3];
    let far = [1000.0, -2000.0, 500.0];
    let cases = [
        sphere(&mut api, "sphere r12", at0, 12.0),
        torus(&mut api, "torus R22 r4", at0, 22.0, 4.0),
        sphere(&mut api, "sphere far", far, 7.0),
        torus(&mut api, "torus far", far, 30.0, 0.5),
    ];
    against_closed_forms(&mut api, &cases);
}

/// Each body's area, volume, centroid and inertia at 1e-8 against its closed form.
fn against_closed_forms(api: &mut Smlib, cases: &[Expected]) {
    // Relative errors: area, volume and inertia against themselves; the centroid against the
    // body's box diagonal, the products against its largest moment.
    let labels = ["area", "volume", "centroid", "inertia", "products"];
    let mut worst = [0.0f64; 5];
    for e in cases {
        let m = api.brep_mass_properties(&e.body, 1e-8).unwrap();
        let area = api.brep_area(&e.body, 1e-8).unwrap();
        let (lo, hi) = api.brep_bounds(&e.body).unwrap();
        let size = lo
            .iter()
            .zip(hi)
            .map(|(l, h)| (h - l).powi(2))
            .sum::<f64>()
            .sqrt();
        let imax = e.v[5].max(e.v[6]).max(e.v[7]);
        let mut err = [
            (m[0] / e.v[0] - 1.0).abs(),
            (m[1] / e.v[1] - 1.0).abs(),
            0.0,
            0.0,
            0.0,
        ];
        err[0] = err[0].max((area / e.v[0] - 1.0).abs());
        let pairs = || m.iter().zip(e.v);
        err[2] = pairs()
            .skip(2)
            .take(3)
            .fold(0.0, |w, (g, x)| w.max((g - x).abs() / size));
        err[3] = pairs()
            .skip(5)
            .take(3)
            .fold(0.0, |w, (g, x)| w.max((g / x - 1.0).abs()));
        err[4] = pairs()
            .skip(8)
            .fold(0.0, |w, (g, x)| w.max((g - x).abs() / imax));
        println!("{:<30} {:?}", e.name, err);
        for (w, x) in worst.iter_mut().zip(err) {
            *w = w.max(x);
        }
    }
    // Asked for 1e-8; the C test accepts 1e-6.
    for (label, w) in labels.iter().zip(worst) {
        assert!(w < 1e-6, "{label}: worst relative error {w:.2e}");
    }
}

/// A sheet has no material: its area, no volume or inertia, and the centroid of its area
/// (a rectangle's: the middle of its box).
#[test]
fn sheet() {
    let mut api = Smlib::default();
    let sheet = create_plane(api.kernel_mut(), Point3::ZERO, 10.0, 20.0).unwrap();
    let m = api.brep_mass_properties(&sheet, 1e-4).unwrap();
    assert!((m[0] - 200.0).abs() < 1e-9 && m[1] == 0.0, "{m:?}");
    let (lo, hi) = api.brep_bounds(&sheet).unwrap();
    for ((c, l), h) in m[2..5].iter().zip(lo).zip(hi) {
        assert!((c - 0.5 * (l + h)).abs() < 1e-9, "{m:?}");
    }
    for x in &m[5..11] {
        assert_eq!(*x, 0.0);
    }
    assert!((api.brep_area(&sheet, 1e-4).unwrap() - 200.0).abs() < 1e-9);
}

#[test]
fn rejected_arguments() {
    let mut api = Smlib::default();
    let b = api.make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    assert_eq!(
        api.brep_mass_properties(&b, f64::NAN).err(),
        Some(Status::ERR_ARG)
    );
    assert_eq!(
        api.brep_area(&b, f64::INFINITY).err(),
        Some(Status::ERR_ARG)
    );
}
