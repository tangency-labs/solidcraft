//! tangency-labs/solidcore#420 (WR-421): a chamfer as wide as the corner rounds it runs
//! round. `CalcTorusFilletGeom` only handled a circular section, so a linear one went to
//! the offset trace, which collapses to a point where the neighbouring round's offset
//! reaches its axis. `fixes.fillet_chamfer_cone_on_arc` revolves the linear section into a
//! cone. The case is the fork's capi test: a 40 x 40 x 20 plate with its vertical edges
//! rounded R1, its bottom loop chamfered 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::{chamfer_fillet, fillet_edges};
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The midpoint of edge `e`.
fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// The edges whose midpoint satisfies `keep`.
fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// The plate's volume after its corners are rounded, after its bottom loop is chamfered,
/// and the chamfer's result.
fn chamfered_plate(fix: bool) -> (f64, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = fix;
    let mut plate = create_box(&mut k, Point3::new(-20.0, -20.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let corners = edges_where(&k, &plate, |m| (m.z - 10.0).abs() < 1e-6);
    fillet_edges(&mut k, &mut plate, &corners, 1.0, 1, 1, 1.0).unwrap();
    let rounded = volume(&k, &mut plate);
    let bottom = edges_where(&k, &plate, |m| m.z.abs() < 1e-6);
    let r = fillet_edges(&mut k, &mut plate, &bottom, 1.0, 0, 1, 1.0);
    (rounded, volume(&k, &mut plate), r)
}

#[test]
fn the_chamfer_runs_round_the_corner_rounds_with_the_fix() {
    // each straight side loses a 1 x 1 triangle along 38, each corner the triangle turned
    // a quarter about the axis: (pi / 2) / 3
    let removed = 4.0 * 38.0 * 0.5 + 4.0 * (PI / 2.0) / 3.0;

    let (rounded, after, r) = chamfered_plate(true);
    r.unwrap();
    assert!(
        (after - (rounded - removed)).abs() < 1e-7 * rounded,
        "{rounded} {after}"
    );

    // upstream traces the offset intersection, which collapses to a point, and fails
    let (_, _, r) = chamfered_plate(false);
    assert!(r.is_err(), "upstream should fail: {r:?}");
}

/// #379: `chamfer_fillet` of every edge of a 10 cube with a radius 2 through hole, by a chord
/// length of 1/sqrt(2) (legs 0.5).
fn chamfered_holed_cube(fix: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = fix;
    let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let hole = create_cylinder(&mut k, Point3::new(5.0, 5.0, -1.0), 2.0, 12.0).unwrap();
    let mut body = boolean_difference(&mut k, cube, hole).unwrap();
    let before = volume(&k, &mut body);
    chamfer_fillet(&mut k, &mut body, std::f64::consts::FRAC_1_SQRT_2).unwrap();
    (before, volume(&k, &mut body))
}

#[test]
fn the_rims_of_a_through_hole_are_chamfered_with_the_fix() {
    // a rim of radius r chamfered by legs a loses the triangle (centroid at r + a / 3) turned
    // once about the axis; the two rims of the hole
    let (r, a) = (2.0, 0.5);
    let rims = 2.0 * 2.0 * PI * (r + a / 3.0) * a * a / 2.0;

    let (before, off) = chamfered_holed_cube(false);
    let (_, on) = chamfered_holed_cube(true);
    // the cube's edges are chamfered either way, the rims only with the fix
    assert!((off - on - rims).abs() < 1e-3, "{before} {off} {on}");
}
