//! tangency-labs/solidcore#480: a union of a half-annulus lip onto a ring whose outer
//! cylinder walls are 1e-4 to 1e-3 apart (10 to 100 times `SM_ZONE_TOL_3D`). Upstream's
//! `SmApiBoolean` fails ("not a solid") at those offsets and is valid at 0 and 0.01. The
//! two r 5 cylinders cross at a grazing angle beside the coincident cap/top planes. The
//! Rust port gives a valid union at every offset (WR-362); this guards it.
//!
//! With the ring turned 90 degrees, its circles' seam vertex sits at (0, 15, 2), e / 2 from
//! where the two r 5 rims cross: for 4e-5 < e <= 2e-4 `SmProtoEdge::Resolve` splits the
//! short self-loop proto edge onto that one vertex and `SmMerge::MakeFaceuse` returns
//! `SM_ERR` (WR-214). That is the default (SMLib); the opt-in
//! `Kernel::fixes.proto_edge_self_loop_on_vertex_degenerate` drops the proto edge instead.
//! These are not difftest cases: the ring's own difference (two coaxial cylinders) already
//! reaches UB-175 in `~SmProtoTopologyManager`, so `boolean.api_brep` rejects the input.
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
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel, SolidError};

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

/// The base ring (r 3..5 about (0, 10), z 0..2), turned `turn_deg` about its axis, united
/// with the lip (r 4..5 about (e, 10), y >= 10, z 2..20).
fn ring_and_lip_body(k: &mut Kernel, e: f64, turn_deg: f64) -> solidcore::Result<Brep> {
    let outer = create_cylinder(k, Point3::new(0.0, 10.0, 0.0), 5.0, 2.0)?;
    let inner = create_cylinder(k, Point3::new(0.0, 10.0, -1.0), 3.0, 4.0)?;
    let mut ring = boolean_difference(k, outer, inner)?;
    if turn_deg != 0.0 {
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut ring)),
            Point3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            turn_deg,
        )?;
    }

    let lip_outer = create_cylinder(k, Point3::new(e, 10.0, 2.0), 5.0, 18.0)?;
    let lip_inner = create_cylinder(k, Point3::new(e, 10.0, 1.0), 4.0, 20.0)?;
    let tube = boolean_difference(k, lip_outer, lip_inner)?;
    let half = create_box(k, Point3::new(e - 10.0, 0.0, 0.0), 20.0, 10.0, 30.0)?;
    let lip = boolean_difference(k, tube, half)?;

    boolean_union(k, ring, lip)
}

/// [`ring_and_lip_body`] unturned; its volume.
fn ring_and_lip(e: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    let mut joined = ring_and_lip_body(&mut k, e, 0.0)?;
    volume(&k, &mut joined)
}

/// The offsets of the WR-214 window that the C++ build was checked at.
const SEAM_WINDOW: [f64; 4] = [5e-5, 7e-5, 1e-4, 1.5e-4];

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

/// WR-214 with the fix on: the ring turned so its seam vertex is beside the rims'
/// crossing; every offset of the window gives a manifold solid of the analytic volume.
#[test]
fn issue_480_ring_lip_union_with_the_seam_at_the_crossing() {
    let mut report = Vec::new();
    for e in SEAM_WINDOW {
        let mut k = Kernel::default();
        k.fixes.proto_edge_self_loop_on_vertex_degenerate = true;
        let got = ring_and_lip_body(&mut k, e, 90.0).and_then(|mut b| {
            let solid = is_manifold_solid(&b)?;
            Ok((solid, volume(&k, &mut b)?))
        });
        match got {
            Ok((true, v)) if (v - want()).abs() <= 1e-4 * want() => {}
            other => report.push(format!("e = {e}: {other:?}")),
        }
    }
    assert!(report.is_empty(), "want {}: {report:#?}", want());
}

/// WR-214 by default (SMLib): the same unions fail in `SmMerge::MakeFaceuse` with
/// `SM_ERR`, as the C++ build does.
#[test]
fn issue_480_ring_lip_union_with_the_seam_at_the_crossing_fails_by_default() {
    for e in SEAM_WINDOW {
        let mut k = Kernel::default();
        let r = ring_and_lip_body(&mut k, e, 90.0).map(|_| ());
        assert!(
            matches!(r, Err(SolidError::Upstream { code: 1001, .. })),
            "e = {e}: {r:?}"
        );
    }
}
