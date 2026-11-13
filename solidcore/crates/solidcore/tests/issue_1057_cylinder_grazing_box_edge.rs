//! tangency-labs/solidcore#1057 (WR-305): the square column [0, 1.5]^2, z -3.9..3.2, minus an
//! r 1.5 cylinder about x = 0.0003, y = 0. The cylinder touches the face y = 1.5 along
//! x = 0.0003 and passes 3e-8 inside the column's edge at x = 0, y = 1.5, so on each cap
//! its arc meets the edge y = 1.5 twice: tangent at x = 0.0003, and at the edge's vertex.
//! SMLib classifies the arc between them, within 3e-8 of the edge, inside the cap, splits
//! the caps along it and keeps a solid 3e-8 thick. The opt-in
//! `Kernel::fixes.classify_edge_interval` classifies such an interval to the edge.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, Kernel};

const R: f64 = 1.5;
/// The accuracy asked of the volume integrator (`SmApiBrepComputeVolume`), also the
/// relative bound on its result against the closed form.
const ACCURACY: f64 = 1e-6;
const X0: f64 = 0.0003;

/// The integral of sqrt(r^2 - u^2) over u = a..b.
fn half_disc(r: f64, a: f64, b: f64) -> f64 {
    let f = |u: f64| 0.5 * (u * (r * r - u * u).sqrt() + r * r * (u / r).asin());
    f(b) - f(a)
}

fn cut(k: &mut Kernel) -> Brep {
    let column = create_box(k, Point3::new(0.0, 0.0, -3.9), 1.5, 1.5, 7.1).unwrap();
    let cylinder = create_cylinder(k, Point3::new(X0, 0.0, -4.0), R, 7.5).unwrap();
    boolean_difference(k, column, cylinder).unwrap()
}

/// The regions other than the infinite one.
fn solid_regions(b: &Brep) -> usize {
    p1::regions(b, None)
        .unwrap()
        .into_iter()
        .filter(|&r| Some(r) != b.infinite_region())
        .count()
}

#[test]
fn issue_1057_classify_edge_interval_is_off_by_default() {
    assert!(!Kernel::default().fixes.classify_edge_interval);
}

#[test]
fn issue_1057_grazing_cut_leaves_one_solid() {
    let mut k = Kernel::default();
    k.fixes.classify_edge_interval = true;
    let mut b = cut(&mut k);
    assert_eq!(
        solid_regions(&b),
        1,
        "no sliver solid along the grazed edge"
    );
    assert!(is_manifold_solid(&b).unwrap());
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut b, ACCURACY, &attrs).unwrap();
    let want = 7.1 * (2.25 - half_disc(R, -X0, 1.5 - X0));
    assert!(
        (volume - want).abs() <= ACCURACY * want,
        "volume {volume}, want {want}"
    );
}
