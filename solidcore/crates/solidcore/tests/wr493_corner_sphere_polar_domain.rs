//! `fixes.found_torus_sphere_keep_tested_nurb` (WR-493) on a small round's corner sphere:
//! the kept NURB's rebuild takes the sweep interval from one iso circle and the polar
//! curve from another, so they differ by more than `check_brep` allows, up to 7.8e-9
//! degrees at R 0.02 (`SmSurfOfRevolution` tests #6 and #7). The sphere is on its
//! polar curve's interval now. Other surfaces can still report; this pins the spheres.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

fn rounded_wedge(k: &mut Kernel, deg: f64, r: f64) -> Brep {
    let a = deg.to_radians();
    let p = [(0.0, 0.0), (40.0, 0.0), (40.0 * a.cos(), 40.0 * a.sin())];
    let c: Vec<Curve> = (0..3)
        .map(|i| {
            let (s, e) = (p[i], p[(i + 1) % 3]);
            Curve::from(
                create_line_segment(Vec3::new(s.0, s.1, 0.0), Vec3::new(e.0, e.1, 0.0)).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = c.iter().map(Some).collect();
    let mut b = create_taper_extrude(k, &refs, 10.0, 0.0, 3, false).unwrap();
    let edges = b.edge_list().to_vec();
    fillet_edges(k, &mut b, &edges, r, 1, 1, 1.0).unwrap();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let a = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &a).unwrap()
}

#[test]
fn small_corner_spheres_pass_the_check_with_the_tested_nurb_kept() {
    for deg in [45.0, 90.0, 120.0, 140.0, 155.0] {
        for r in [0.02, 0.07, 0.15] {
            let mut k = Kernel::default();
            k.fixes.found_torus_sphere_keep_tested_nurb = true;
            let mut b = rounded_wedge(&mut k, deg, r);
            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            let lines = check.lines();
            let on_sphere: Vec<_> = lines
                .iter()
                .filter(|l| l.contains("owner SmSphere"))
                .collect();
            assert!(on_sphere.is_empty(), "{deg} deg, r {r}: {on_sphere:?}");

            let mut d = Kernel::default();
            let mut off = rounded_wedge(&mut d, deg, r);
            let (v_on, v_off) = (volume(&k, &mut b), volume(&d, &mut off));
            assert!(
                (v_on - v_off).abs() < 1e-6 * v_off,
                "{deg} deg, r {r}: {v_on} against {v_off}"
            );
        }
    }
}
