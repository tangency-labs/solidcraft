//! WR-755: `faces_coincide` decides a cone (a cylinder is a cone of semi-angle 0), sphere or
//! torus pair from the definitions, within the larger face tolerance. For each kind: a pair
//! 0.3 tol apart passes the gate, a pair 2 tol apart does not.

#![allow(clippy::unwrap_used, clippy::disallowed_methods)]

mod common;

use common::kernel_copy;
use solidcore::boolean::merge::p1::faces_coincide;
use solidcore::query::face::p1::faces_on_other_surfaces;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore_ops::{make_box, make_cone, make_cylinder, make_sphere, make_torus, Body};

/// Whether the one curved face of `a` coincides with the one curved face of `b` (and the
/// larger of their tolerances).
fn gate(a: &Body, b: &Body) -> (bool, f64) {
    let curved = |body: &Body| {
        let (k, brep) = kernel_copy(body);
        let f = *brep
            .face_list()
            .iter()
            .find(|&&f| {
                !matches!(
                    face::surface(&brep, f).unwrap().unwrap().kind(),
                    SurfaceKind::Plane(_)
                )
            })
            .unwrap();
        (k, brep, f)
    };
    let (k, ba, fa) = curved(a);
    let (_, bb, fb) = curved(b);
    let tol = face::tolerance(&ba, fa)
        .unwrap()
        .max(face::tolerance(&bb, fb).unwrap());
    (faces_coincide(&k, &ba, fa, &bb, fb).unwrap(), tol)
}

/// `pair(d)`: the second body with its definition off by `d` (a radius, a centre or an axis).
fn check(name: &str, pair: impl Fn(f64) -> (Body, Body)) {
    let (_, tol) = gate(&pair(0.0).0, &pair(0.0).1);
    for (d, expected) in [(0.0, true), (0.3 * tol, true), (2.0 * tol, false)] {
        let (a, b) = pair(d);
        assert_eq!(gate(&a, &b).0, expected, "{name}: off by {d} (tol {tol})");
    }
}

#[test]
fn cylinder_radius() {
    check("cylinder radius", |d| {
        (
            make_cylinder([0.0; 3], 5.0, 10.0).unwrap(),
            make_cylinder([0.0; 3], 5.0 + d, 10.0).unwrap(),
        )
    });
}

#[test]
fn cylinder_axis() {
    check("cylinder axis", |d| {
        (
            make_cylinder([0.0; 3], 5.0, 10.0).unwrap(),
            make_cylinder([d, 0.0, 0.0], 5.0, 10.0).unwrap(),
        )
    });
}

#[test]
fn cylinder_shorter_along_its_axis_is_the_same_surface() {
    assert!(
        gate(
            &make_cylinder([0.0; 3], 5.0, 10.0).unwrap(),
            &make_cylinder([0.0, 0.0, 3.0], 5.0, 4.0).unwrap()
        )
        .0
    );
}

#[test]
fn cone_radius() {
    check("cone radius", |d| {
        (
            make_cone([0.0; 3], 5.0, 3.0, 10.0).unwrap(),
            make_cone([0.0; 3], 5.0 + d, 3.0 + d, 10.0).unwrap(),
        )
    });
}

#[test]
fn cone_half_angle() {
    // the top radius moves by d, a slope change of d / 10 over the cone's reach
    check("cone half angle", |d| {
        (
            make_cone([0.0; 3], 5.0, 3.0, 10.0).unwrap(),
            make_cone([0.0; 3], 5.0, 3.0 + d, 10.0).unwrap(),
        )
    });
}

#[test]
fn cone_cut_off_along_its_axis_is_the_same_surface() {
    // the radius is 4.6 at z = 2 on the cone of radii 5 at 0 and 3 at 10
    assert!(
        gate(
            &make_cone([0.0; 3], 5.0, 3.0, 10.0).unwrap(),
            &make_cone([0.0, 0.0, 2.0], 4.6, 3.0, 8.0).unwrap()
        )
        .0
    );
}

#[test]
fn sphere_radius() {
    check("sphere radius", |d| {
        (
            make_sphere([0.0; 3], 5.0).unwrap(),
            make_sphere([0.0; 3], 5.0 + d).unwrap(),
        )
    });
}

#[test]
fn sphere_centre() {
    check("sphere centre", |d| {
        (
            make_sphere([0.0; 3], 5.0).unwrap(),
            make_sphere([0.0, d, 0.0], 5.0).unwrap(),
        )
    });
}

#[test]
fn torus_major_radius() {
    check("torus major radius", |d| {
        (
            make_torus([0.0; 3], 8.0, 2.0).unwrap(),
            make_torus([0.0; 3], 8.0 + d, 2.0).unwrap(),
        )
    });
}

#[test]
fn torus_minor_radius() {
    check("torus minor radius", |d| {
        (
            make_torus([0.0; 3], 8.0, 2.0).unwrap(),
            make_torus([0.0; 3], 8.0, 2.0 + d).unwrap(),
        )
    });
}

#[test]
fn torus_centre() {
    check("torus centre", |d| {
        (
            make_torus([0.0; 3], 8.0, 2.0).unwrap(),
            make_torus([0.0, 0.0, d], 8.0, 2.0).unwrap(),
        )
    });
}

/// Two different kinds are left to the sector: where they touch tangentially they can stay
/// within the tolerance over a patch (a sphere cap thinner than it on a plane, #2412's
/// `box_and_sphere_poking_a_hair_through_a_face_is_a_closed_solid`).
#[test]
fn different_quadric_kinds_are_left_to_the_sector() {
    assert!(
        gate(
            &make_sphere([0.0; 3], 5.0).unwrap(),
            &make_cylinder([0.0; 3], 5.0, 10.0).unwrap()
        )
        .0
    );
    assert!(
        gate(
            &make_cylinder([0.0; 3], 2.0, 10.0).unwrap(),
            &make_torus([0.0; 3], 8.0, 2.0).unwrap()
        )
        .0
    );
}

/// The radial sector holds a coincidence back, and steps on, for faces the definitions show
/// to lie on different surfaces: two different kinds (tangent at most), or two of one kind
/// the gate tells apart. Two planes, and a sphere against a torus, may be one surface.
#[test]
fn faces_on_other_surfaces_are_told_by_their_definitions() {
    // whether the first planar (or first curved) faces of two bodies lie on other surfaces
    let other = |a: &Body, a_plane: bool, b: &Body, b_plane: bool| {
        let pick = |body: &Body, plane: bool| {
            let (k, brep) = kernel_copy(body);
            let f = *brep
                .face_list()
                .iter()
                .find(|&&f| {
                    matches!(
                        face::surface(&brep, f).unwrap().unwrap().kind(),
                        SurfaceKind::Plane(_)
                    ) == plane
                })
                .unwrap();
            (k, brep, f)
        };
        let (k, ba, fa) = pick(a, a_plane);
        let (_, bb, fb) = pick(b, b_plane);
        let ab = faces_on_other_surfaces(&k, &ba, fa, &bb, fb).unwrap();
        assert_eq!(ab, faces_on_other_surfaces(&k, &bb, fb, &ba, fa).unwrap());
        ab
    };
    let block = make_box([0.0; 3], [5.0, 5.0, 5.0]).unwrap();
    let cylinder = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let cone = make_cone([0.0; 3], 5.0, 3.0, 10.0).unwrap();
    let sphere = make_sphere([0.0; 3], 5.0).unwrap();
    let torus = make_torus([0.0; 3], 8.0, 2.0).unwrap();
    // two kinds
    assert!(other(&block, true, &cylinder, false));
    assert!(other(&block, true, &cone, false));
    assert!(other(&block, true, &sphere, false));
    assert!(other(&block, true, &torus, false));
    assert!(other(&cylinder, false, &sphere, false));
    assert!(other(&cone, false, &torus, false));
    // one kind, told apart: a cylinder beside it, a larger sphere
    let beside = make_cylinder([10.0, 0.0, 0.0], 5.0, 10.0).unwrap();
    assert!(other(&cylinder, false, &beside, false));
    let larger = make_sphere([0.0; 3], 7.0).unwrap();
    assert!(other(&sphere, false, &larger, false));
    // one surface, or no telling
    let shorter = make_cylinder([0.0, 0.0, 3.0], 5.0, 4.0).unwrap();
    assert!(!other(&cylinder, false, &shorter, false));
    assert!(!other(&block, true, &cylinder, true));
    assert!(!other(&sphere, false, &torus, false));
}

#[test]
fn cone_crossing_at_mid_height() {
    // both radii 4 at mid height; the ends part by d and the slopes by d / 5
    check("cone crossing at mid height", |d| {
        (
            make_cone([0.0; 3], 5.0, 3.0, 10.0).unwrap(),
            make_cone([0.0; 3], 5.0 + d, 3.0 - d, 10.0).unwrap(),
        )
    });
}
