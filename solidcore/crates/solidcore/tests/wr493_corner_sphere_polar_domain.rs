//! WR-493 on a small round's corner sphere: the sphere found in the round's NURB keeps
//! that NURB, and its rebuild takes the sweep interval from one iso circle and the polar
//! curve from another, so they differ by more than `check_brep` allows, up to 7.8e-9
//! degrees at R 0.02 (`SmSurfOfRevolution` tests #6 and #7). The sphere is on its
//! polar curve's interval now. Other surfaces can still report; this pins the spheres.
//! The volume is the rolling-ball closed form ([`rounded_prism_volume`]).
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

/// The volume of [`rounded_wedge`]'s body in closed form. Every edge of the convex
/// prism is rounded with radius `r` and every corner is a sphere of radius `r`, so the
/// body is the inner parallel body `Q` (the triangle offset `r` inward, a similar
/// triangle scaled by `(rho - r) / rho` about the incentre, `rho` the inradius, extruded
/// `10 - 2 r`) grown by `r` (`Q + r B`). Steiner's formula for a convex polytope:
/// `V(Q) + A(Q) r + r^2 sum_e l_e (pi - theta_e) / 2 + 4/3 pi r^3`, with `theta_e` the
/// dihedral angle at edge `e`: `pi / 2` at the cap edges, the triangle's angle at the
/// side edges, whose exterior angles sum to `2 pi`.
fn rounded_prism_volume(deg: f64, r: f64) -> f64 {
    let a = deg.to_radians();
    let (area, perimeter) = (800.0 * a.sin(), 80.0 + 80.0 * (a / 2.0).sin());
    let rho = 2.0 * area / perimeter;
    let s = (rho - r) / rho;
    let (tq, pq, hq) = (area * s * s, perimeter * s, 10.0 - 2.0 * r);
    let pi = std::f64::consts::PI;
    tq * hq
        + (2.0 * tq + pq * hq) * r
        + (pq * pi / 2.0 + hq * pi) * r * r
        + 4.0 / 3.0 * pi * r * r * r
}

#[test]
fn small_corner_spheres_pass_the_check_with_the_tested_nurb_kept() {
    for deg in [45.0, 90.0, 120.0, 140.0, 155.0] {
        for r in [0.02, 0.07, 0.15] {
            let mut k = Kernel::default();
            let mut b = rounded_wedge(&mut k, deg, r);
            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            let lines = check.lines();
            let on_sphere: Vec<_> = lines
                .iter()
                .filter(|l| l.contains("owner SmSphere"))
                .collect();
            assert!(on_sphere.is_empty(), "{deg} deg, r {r}: {on_sphere:?}");

            // the integrator's requested accuracy, 1e-6 relative, as the old comparison
            // with the NURB made from the fitted circles
            let (v, closed) = (volume(&k, &mut b), rounded_prism_volume(deg, r));
            assert!(
                (v - closed).abs() < 1e-6 * closed,
                "{deg} deg, r {r}: {v} against the closed form {closed}"
            );
        }
    }
}

/// The corner spheres' sweep interval is their polar curve's, bit for bit (#2073): each
/// corner face's NURB, found a sphere (`Sphere::is_nurb_surface_sphere`, as the fillet's
/// exact-surface test and the implicit form call it), keeps that NURB, and the rebuild from
/// it takes the sweep interval from another iso circle, which differs on these small
/// spheres, so without that step this fails.
#[test]
fn corner_spheres_sweep_their_polar_curves_interval() {
    use solidcore::surface::sphere::Sphere;
    use solidcore::surface::surface::SurfaceKind;
    let mut spheres = 0;
    for deg in [45.0, 90.0, 120.0, 140.0, 155.0] {
        for r in [0.02, 0.07, 0.15] {
            let mut k = Kernel::default();
            let b = rounded_wedge(&mut k, deg, r);
            for (fid, f) in b.faces().iter() {
                let Some(SurfaceKind::BSpline(nurb)) = f.surface.as_ref().map(|s| s.kind()) else {
                    continue;
                };
                let Some(sp) = Sphere::is_nurb_surface_sphere(&k, nurb, 1.0).unwrap() else {
                    continue;
                };
                spheres += 1;
                let polar = sp
                    .base()
                    .polar_curve()
                    .expect("a polar curve")
                    .step_interval();
                let d = sp.base().step_uv_domain();
                assert_eq!(
                    [d.min().x.to_bits(), d.max().x.to_bits()],
                    [polar.min().to_bits(), polar.max().to_bits()],
                    "{deg} deg, r {r}, {fid:?}: sweep {d:?}, polar curve {polar:?}"
                );
            }
        }
    }
    assert!(spheres > 0, "no corner sphere found");
    eprintln!("{spheres} corner spheres");
}
