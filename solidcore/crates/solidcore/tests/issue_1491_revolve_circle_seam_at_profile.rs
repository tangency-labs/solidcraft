//! Regression test for tangency-labs/solidcore#1491: a full-turn revolve of one whole
//! circle went through upstream's one-curve special case
//! (`sm_CreateAnalyticRotationalSweep`), whose torus takes X from the circle's normal,
//! so its seam lies a quarter turn from the profile. Fork tangency-labs/smlib 9853e54
//! deletes that case; with `fixes.revolve_analytic_profile_faces` the circle goes
//! through the per-curve rule, its torus's X radial into the profile's half-plane.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_rotational_sweep;
use solidcore::topology::vertex::point;
use solidcore::{Brep, Kernel, Point3, Vec3};
use std::f64::consts::PI;

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The circle of radius 3 about (-30, 0, 0) in the plane y = 0, from (-27, 0, 0) (or
/// wound the other way), turned a whole turn about `axis`.
fn round_bar(k: &mut Kernel, axis: Vec3, other_way: bool) -> Brep {
    let y = if other_way { -Vec3::Z } else { Vec3::Z };
    let frame = Axis2Placement::new(Point3::new(-30.0, 0.0, 0.0), Vec3::X, y);
    let c = Curve::from(
        Ellipse::create_canonical_circle(
            k,
            &frame,
            3.0,
            Some(&Extent1d::from_min_max(0.0, 360.0)),
            None,
        )
        .unwrap(),
    );
    create_rotational_sweep(k, &[Some(&c)], Point3::ZERO, axis, 360.0, true).unwrap()
}

/// The torus face's frame X and Z, and the B-rep's vertices.
fn seam(b: &Brep) -> (Vec3, Vec3, Vec<Point3>) {
    let faces = b.face_list();
    assert_eq!(faces.len(), 1);
    let SurfaceKind::Torus(t) = b.face(faces[0]).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("a torus face");
    };
    let frame = t.base().position();
    let vertices = b
        .vertex_list()
        .iter()
        .map(|&v| point(b, v).unwrap())
        .collect();
    (frame.x_axis(), frame.z_axis(), vertices)
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-9
}

#[test]
fn issue_1491_whole_circle_revolve_has_its_seam_at_the_profile() {
    let want = 2.0 * PI * 30.0 * PI * 9.0;
    for (axis, other_way) in [(Vec3::Z, false), (Vec3::Z, true), (-Vec3::Z, false)] {
        let case = format!("axis {axis:?}, other way {other_way}");
        // the fix: X from the axis into the profile's half-plane, Z the turn axis, the
        // seam vertex the circle's start
        let mut k = Kernel::default();
        k.fixes.revolve_analytic_profile_faces = true;
        let mut b = round_bar(&mut k, axis, other_way);
        let (x, z, vertices) = seam(&b);
        assert!(near(x, -Vec3::X), "{case}: X {x:?}");
        assert!(near(z, axis), "{case}: Z {z:?}");
        assert!(!vertices.is_empty(), "{case}");
        for v in &vertices {
            assert!(
                near(*v, Point3::new(-27.0, 0.0, 0.0)),
                "{case}: vertex {v:?}"
            );
        }
        let got = volume(&k, &mut b);
        assert!((got - want).abs() <= 1e-6 * want, "{case}: {got} {want}");

        // upstream (the special case): X the circle's plane normal, a quarter turn off
        let mut k = Kernel::default();
        let b = round_bar(&mut k, axis, other_way);
        let (x, _, vertices) = seam(&b);
        assert!(
            x.dot(Vec3::Y).abs() > 1.0 - 1e-9,
            "{case}: upstream X {x:?}"
        );
        assert!(
            vertices.iter().all(|v| v.x.abs() < 1e-9),
            "{case}: upstream vertices {vertices:?}"
        );
    }
}

#[test]
fn issue_1491_seam_at_the_profile_unions_with_a_square_ring() {
    // the rig's sweeps/3006: the round bar joined to the 5 x 5 square swept round the
    // same circle (the square's seam in the plane x = 0)
    let mut k = Kernel::default();
    k.fixes.revolve_analytic_profile_faces = true;
    let round = round_bar(&mut k, Vec3::Z, false);
    let p = |y: f64, z: f64| Vec3::new(0.0, y, z);
    let corners = [p(-27.5, -2.5), p(-32.5, -2.5), p(-32.5, 2.5), p(-27.5, 2.5)];
    let sides: Vec<Curve> = (0..4)
        .map(|i| {
            Curve::from(
                BSplineCurve::create_line_segment(3, corners[i], corners[(i + 1) % 4]).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    let square =
        create_rotational_sweep(&mut k, &refs, Point3::ZERO, Vec3::Z, 360.0, true).unwrap();
    let mut joined = boolean_union(&mut k, round, square).unwrap();
    // the section: the disc of radius 3 and the square of half side 2.5; their
    // overlap is 4 quarters, each c h + r^2 (asin(h / r) - asin(c / r)) / 2 with
    // c = sqrt(r^2 - h^2) (23.772778)
    let (h, r) = (2.5_f64, 3.0_f64);
    let c = (r * r - h * h).sqrt();
    let overlap = 4.0 * (c * h + 0.5 * r * r * ((h / r).asin() - (c / r).asin()));
    let want = 2.0 * PI * 30.0 * (25.0 + 9.0 * PI - overlap);
    let got = volume(&k, &mut joined);
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "{got} {want} (overlap {overlap})"
    );
}
