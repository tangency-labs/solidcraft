//! #1556 (WR-563): `SmFace::ComputePreciseProperties` starts every iso-u integral at the
//! face's lowest UV-domain v. On a face whose domain is the surface's unbounded one
//! (`±INFINITE_PARAMETER` in v) each integral runs ~1.2e6 parameter units below the
//! trims and the area is the small difference of large terms. With
//! `UpstreamFixes::precise_properties_trim_base_v` the integrals start at the lowest v of
//! the face's UV trim curves instead.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::mass::integrator::properties::face_centroid;
use solidcore::math::{Point3, INFINITE_PARAMETER};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::face::p2::{set_uv_domain, uv_domain};
use solidcore::Kernel;
use std::f64::consts::PI;

const ACCURACY: f64 = 1e-6;

/// Per face of a cylinder at (100,200,300): (area, centroid) with the domain made
/// unbounded in v.
fn unbounded(k: &mut Kernel, radius: f64) -> Vec<(f64, Point3)> {
    let mut b = create_cylinder(k, Point3::new(100.0, 200.0, 300.0), radius, 2.4 * radius).unwrap();
    let mut out = vec![];
    for f in faces(&b, None).unwrap() {
        let mut d = uv_domain(&b, f).unwrap();
        d.set_v_min(-INFINITE_PARAMETER).unwrap();
        d.set_v_max(INFINITE_PARAMETER).unwrap();
        set_uv_domain(&mut b, f, &d).unwrap();
        out.push(face_centroid(k, &mut b, f, ACCURACY).unwrap());
    }
    out
}

/// Exact (area, centroid) of the cylinder's faces: side, bottom cap, top cap, in the
/// order `faces` returns them.
fn exact(radius: f64) -> [(f64, Point3); 3] {
    let h = 2.4 * radius;
    let cap = PI * radius * radius;
    [
        (
            2.0 * PI * radius * h,
            Point3::new(100.0, 200.0, 300.0 + h / 2.0),
        ),
        (cap, Point3::new(100.0, 200.0, 300.0)),
        (cap, Point3::new(100.0, 200.0, 300.0 + h)),
    ]
}

const RADII: [f64; 3] = [5.0, 0.5, 0.05];

#[test]
fn flag_is_off_by_default() {
    assert!(!Kernel::default().fixes.precise_properties_trim_base_v);
}

#[test]
fn trim_base_v_gives_exact_area_and_centroid() {
    let mut k = Kernel::default();
    k.fixes.precise_properties_trim_base_v = true;
    for r in RADII {
        let got = unbounded(&mut k, r);
        assert_eq!(got.len(), 3);
        for (i, ((a, c), (ea, ec))) in got.iter().zip(exact(r)).enumerate() {
            let rel = (a - ea).abs() / ea;
            assert!(rel <= 2e-6, "r={r} face {i}: area {a} vs {ea} ({rel:e})");
            let d = (*c - ec).length();
            assert!(
                d <= 1e-6 * r,
                "r={r} face {i}: centroid {c:?} vs {ec:?} ({d:e})"
            );
        }
    }
}

/// Upstream behaviour (the flag off): every face's area misses its exact value by far more
/// than the requested accuracy, up to +485% on the thin cylinder's side.
#[test]
fn domain_base_v_loses_accuracy() {
    let mut k = Kernel::default();
    for r in RADII {
        for (i, ((a, _), (ea, _))) in unbounded(&mut k, r).iter().zip(exact(r)).enumerate() {
            let rel = (a - ea).abs() / ea;
            assert!(rel > 1e-4, "r={r} face {i}: area {a} vs {ea} ({rel:e})");
        }
    }
}
