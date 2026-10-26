//! tangency-labs/solidcore#480: a union of a half-annulus lip onto a ring whose outer
//! cylinder walls are 1e-4 to 1e-3 apart (10 to 100 times `SM_ZONE_TOL_3D`). Upstream's
//! `SmApiBoolean` fails ("not a solid") at those offsets and is valid at 0 and 0.01. The
//! two r 5 cylinders cross at a grazing angle beside the coincident cap/top planes. The
//! Rust port gives a valid union at every offset (WR-362); this guards it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

/// The base ring (r 3..5 about (0, 10), z 0..2) united with the lip (r 4..5 about
/// (e, 10), y >= 10, z 2..20); its volume.
fn ring_and_lip(e: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    let outer = create_cylinder(&mut k, Point3::new(0.0, 10.0, 0.0), 5.0, 2.0)?;
    let inner = create_cylinder(&mut k, Point3::new(0.0, 10.0, -1.0), 3.0, 4.0)?;
    let ring = boolean_difference(&mut k, outer, inner)?;

    let lip_outer = create_cylinder(&mut k, Point3::new(e, 10.0, 2.0), 5.0, 18.0)?;
    let lip_inner = create_cylinder(&mut k, Point3::new(e, 10.0, 1.0), 4.0, 20.0)?;
    let tube = boolean_difference(&mut k, lip_outer, lip_inner)?;
    let half = create_box(&mut k, Point3::new(e - 10.0, 0.0, 0.0), 20.0, 10.0, 30.0)?;
    let lip = boolean_difference(&mut k, tube, half)?;

    let mut joined = boolean_union(&mut k, ring, lip)?;
    volume(&k, &mut joined)
}

/// base pi (25 - 9) 2 plus half of pi (25 - 16) 18: 355.000 (issue: 354.99997).
fn want() -> f64 {
    let pi = core::f64::consts::PI;
    pi * 16.0 * 2.0 + 0.5 * pi * 9.0 * 18.0
}

#[test]
fn issue_480_ring_lip_union_at_every_offset() {
    let mut report = Vec::new();
    for e in [0.0, 1e-4, 1e-3, 1e-2] {
        match ring_and_lip(e) {
            Ok(v) if (v - want()).abs() <= 1e-4 * want() => {}
            other => report.push(format!("e = {e}: {other:?}")),
        }
    }
    assert!(report.is_empty(), "want {}: {report:#?}", want());
}
