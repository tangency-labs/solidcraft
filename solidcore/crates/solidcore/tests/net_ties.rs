//! Which control points of a surface's net move together ([`BSplineSurface::net_ties`]): the
//! columns that meet at the seam of a closed surface, and the row that collapses to a pole. A
//! control-point edit moves each tied group as one, so a closed surface stays closed and a pole
//! stays a point.
#![allow(clippy::unwrap_used)] // test helpers

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_primitives::{
    create_cone_surface, create_cylinder_surface, create_torus_surface,
};
use solidcore::surface::sphere::Sphere;

const TOL: f64 = 1e-9;

type Groups = Vec<Vec<(usize, usize)>>;

/// The net's counts in u and v.
fn counts(s: &BSplineSurface) -> (usize, usize) {
    let net = s.control_point_net().unwrap();
    (net.u_count, net.v_count)
}

/// `groups` with each group's members in order and the groups in the order of their first
/// member.
fn sorted(mut groups: Groups) -> Groups {
    for g in &mut groups {
        g.sort_unstable();
    }
    groups.sort_unstable();
    groups
}

/// The seam pairs of a surface closed in u (its sweep): column 0 with column `nu - 1`, at every
/// v index in `js`.
fn u_seam(nu: usize, js: impl Iterator<Item = usize>) -> Groups {
    js.map(|j| vec![(0, j), (nu - 1, j)]).collect()
}

/// The row of v index `j` collapsed to one point: every u index.
fn pole_row(nu: usize, j: usize) -> Vec<(usize, usize)> {
    (0..nu).map(|i| (i, j)).collect()
}

#[test]
fn a_cylinders_seam_columns_move_together() {
    let k = Kernel::default();
    let c = create_cylinder_surface(&k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    let s = c.rev().base();
    let (nu, nv) = counts(s);
    // the full turn in u (four quadratic arcs), the line in v
    assert_eq!((nu, nv), (9, 2));
    assert_eq!(sorted(s.net_ties(TOL).unwrap()), u_seam(nu, 0..nv));
}

#[test]
fn a_cones_apex_row_moves_as_one_and_its_seam_columns_together() {
    let k = Kernel::default();
    let c = create_cone_surface(&k, Point3::new(0.0, 0.0, 0.0), 5.0, 0.0, 10.0).unwrap();
    let s = c.rev().base();
    let (nu, nv) = counts(s);
    assert_eq!((nu, nv), (9, 2));
    // the base circle's seam pair, and the apex: its whole row (which holds its seam pair)
    let expected = sorted(vec![vec![(0, 0), (nu - 1, 0)], pole_row(nu, nv - 1)]);
    assert_eq!(sorted(s.net_ties(TOL).unwrap()), expected);
}

#[test]
fn a_spheres_two_pole_rows_each_move_as_one() {
    let k = Kernel::default();
    let frame = Axis2Placement::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let sphere = Sphere::create_canonical(&k, &frame, 5.0).unwrap();
    let s = sphere.base().base();
    let (nu, nv) = counts(s);
    // the full turn in u, the half circle from pole to pole in v (two quadratic arcs)
    assert_eq!((nu, nv), (9, 5));
    let mut expected = u_seam(nu, 1..nv - 1);
    expected.push(pole_row(nu, 0));
    expected.push(pole_row(nu, nv - 1));
    assert_eq!(sorted(s.net_ties(TOL).unwrap()), sorted(expected));
}

#[test]
fn a_torus_is_tied_across_both_seams() {
    let k = Kernel::default();
    let t = create_torus_surface(&k, Point3::new(0.0, 0.0, 0.0), 10.0, 3.0).unwrap();
    let s = t.base().base();
    let (nu, nv) = counts(s);
    // a full turn each way
    assert_eq!((nu, nv), (9, 9));
    // the four corners are one point of both seams; every other boundary point has one twin
    let mut expected = vec![vec![(0, 0), (nu - 1, 0), (0, nv - 1), (nu - 1, nv - 1)]];
    expected.extend(u_seam(nu, 1..nv - 1));
    expected.extend((1..nu - 1).map(|i| vec![(i, 0), (i, nv - 1)]));
    assert_eq!(sorted(s.net_ties(TOL).unwrap()), sorted(expected));
}

#[test]
fn an_open_spline_has_no_ties() {
    let s = BSplineSurface::create_bilinear_surface(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(0.0, 10.0, 1.0),
        Point3::new(10.0, 10.0, 0.0),
    )
    .unwrap();
    assert!(s.net_ties(TOL).unwrap().is_empty());
}

#[test]
fn a_tolerance_that_is_not_a_distance_is_refused() {
    let k = Kernel::default();
    let c = create_cylinder_surface(&k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    for tol in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(c.rev().base().net_ties(tol).is_err());
    }
}
