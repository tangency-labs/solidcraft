//! WR-493: a sphere found in a NURB keeps that NURB, and its rebuild takes the sweep
//! interval from one iso circle and the polar curve from another, so they can differ by
//! more than `check_brep` allows (`SmSurfOfRevolution` tests #6 and #7); the sphere is on
//! its polar curve's interval now ([`found_spheres_sweep_their_polar_curves_interval`]).
//! A small round's body checks clean with no report on a sphere and has the rolling-ball
//! closed-form volume ([`rounded_prism_volume`]); its corners are ball patches on this
//! base, so it pins the round, not the found spheres.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_sphere;
use solidcore::surface::sphere::Sphere;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p1 as bp1;
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

/// The NURBS form of the primitive sphere of radius `r`, moved to `c`.
fn sphere_form(k: &mut Kernel, c: Vec3, r: f64) -> BSplineSurface {
    let b = create_sphere(k, Vec3::new(0.0, 0.0, 0.0), r).unwrap();
    let f = bp1::faces(&b, None).unwrap()[0];
    let mut s = b
        .face(f)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .copy(k)
        .unwrap();
    let mut place = Axis2Placement::default();
    place
        .set_canonical(c, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
        .unwrap();
    s.transform(k, &place, None).unwrap();
    as_b_spline(k, &s).unwrap().unwrap().into_owned()
}

/// A sphere found in a NURB keeps that NURB, and its sweep interval is its polar curve's,
/// bit for bit. The forms are patches of a sphere's NURBS form (a corner patch of a round
/// is one), small to large and near to far from the origin. The rebuild from the NURB takes
/// the sweep interval from another iso circle than the polar curve; without the step that
/// sets it from the polar curve, 19 of these 84 differ (up to 4.2e-6 degrees, a patch of
/// radius 1e-3 at 1e5).
#[test]
fn found_spheres_sweep_their_polar_curves_interval() {
    let (mut found, mut patches) = (0, 0);
    for r in [1e-3, 0.02, 0.07, 0.15, 1.0, 2.5, 40.0] {
        for c in [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(40.0, 3.0, 5.0),
            Vec3::new(1e5, 0.0, 0.0),
        ] {
            let mut k = Kernel::default();
            let whole = sphere_form(&mut k, c, r);
            let d0 = whole.natural_uv_domain();
            let (w, h) = (d0.max().x - d0.min().x, d0.max().y - d0.min().y);
            for (fu, fv) in [(0.0, 0.0), (0.13, 0.21), (0.37, 0.05), (0.61, 0.44)] {
                let form = if fu == 0.0 {
                    whole.clone()
                } else {
                    patches += 1;
                    let sub = Extent2d::from_bounds(
                        d0.min().x + fu * w,
                        d0.min().y + fv * h,
                        d0.min().x + (fu + 0.29) * w,
                        d0.min().y + (fv + 0.33) * h,
                    );
                    whole.copy_sub_patch(&sub).unwrap()
                };
                let sp = Sphere::is_nurb_surface_sphere(&k, &form, 1.0)
                    .unwrap()
                    .unwrap_or_else(|| panic!("r {r} at {c:?}, patch {fu}: not found"));
                found += 1;
                let polar = sp
                    .base()
                    .polar_curve()
                    .expect("a polar curve")
                    .step_interval();
                let d = sp.base().step_uv_domain();
                assert_eq!(
                    [d.min().x.to_bits(), d.max().x.to_bits()],
                    [polar.min().to_bits(), polar.max().to_bits()],
                    "r {r} at {c:?}, patch {fu}: sweep {d:?}, polar curve {polar:?}"
                );
            }
        }
    }
    assert_eq!((found, patches), (84, 63));
}
