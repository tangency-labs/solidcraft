//! tangency-labs/solidcore#479 (WR-402): a taper extrude of a profile of two curves that
//! meet at both ends (a line and a half circle) pivots the draft at the profile's first
//! corner. `Kernel::fixes.taper_pivot_nearest_corner` pivots at the nearest corner of the
//! two links instead; the rest of the fix is covered by the unit test in
//! `primitive::primitive_creation::p3::ruled`. This pins SMLib's result.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel};

fn taper(k: &mut Kernel) -> solidcore::Result<Brep> {
    let line = Curve::from(
        create_line_segment(Vec3::new(-20.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0)).unwrap(),
    );
    let arc = Curve::from(create_arc(Vec3::ZERO, 20.0, 0.0, 180.0).unwrap());
    create_taper_extrude(k, &[Some(&line), Some(&arc)], 10.0, -3.0, 3, false)
}

/// Without the field the taper is SMLib's: the shell does not close, so the result is no
/// manifold solid and encloses no volume (with the field: a manifold solid of volume
/// 6555.9139).
#[test]
fn taper_pivot_nearest_corner_is_off_by_default() {
    let mut k = Kernel::default();
    assert!(!k.fixes.taper_pivot_nearest_corner);
    let mut solid = taper(&mut k).unwrap();
    assert!(!is_manifold_solid(&solid).unwrap());
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&k, &mut solid, 1e-6, &attributes).unwrap();
    assert!(volume.abs() < 1e-9, "volume {volume}");
}
