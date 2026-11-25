//! SolidCraft #1986: a joined cylindrical wall is one face, not one face per slab.
//!
//! SolidCraft's truck prism path sewed a wall per slab and merged co-cylindrical faces only
//! for a cut, so joining a 120 degree annular sector r 6..10, z 0..20 to one r 3..6,
//! z 0..18 left the r 10 wall as two faces (z 0..18 and z 18..20). A 3 mm chamfer on the
//! top outer arc edge then read a 2 mm neighbour and was refused. solidcore's SMLib
//! boolean merges faces on one surface, so the same join has three arc walls (r 10, the
//! short r 6 step above z = 18, and r 3) and the chamfer succeeds. This test keeps that
//! case in the regression suite.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// The radius of the outer sector's outer wall.
const R_OUT: f64 = 10.0;
/// The outer sector's inner radius and the inner sector's outer radius.
const R_MID: f64 = 6.0;
/// The inner sector's inner radius.
const R_IN: f64 = 3.0;
/// The outer sector's top, and the inner sector's top where the step starts.
const TOP: f64 = 20.0;
const JOIN: f64 = 18.0;
/// Half of either sector's angle: both run from -60 to +60 degrees.
const HALF_DEG: f64 = 60.0;
/// The chamfer on the top outer arc edge.
const SIZE: f64 = 3.0;

fn at(r: f64, deg: f64) -> Point3 {
    let a = deg.to_radians();
    Point3::new(r * a.cos(), r * a.sin(), 0.0)
}

/// A counter-clockwise annular sector between `r0` and `r1`, from -60 to +60 degrees,
/// swept `height` along +Z. The inner arc is the outer arc's curve reversed: it closes the
/// loop clockwise, as the profile does in SolidCraft.
fn sector(k: &mut Kernel, r0: f64, r1: f64, height: f64) -> Brep {
    let c1 = Curve::from(create_line_segment(at(r0, -HALF_DEG), at(r1, -HALF_DEG)).unwrap());
    let c2 = Curve::from(create_arc(Point3::ZERO, r1, -HALF_DEG, HALF_DEG).unwrap());
    let c3 = Curve::from(create_line_segment(at(r1, HALF_DEG), at(r0, HALF_DEG)).unwrap());
    let mut c4 = Curve::from(create_arc(Point3::ZERO, r0, -HALF_DEG, HALF_DEG).unwrap());
    let interval = c4.natural_interval();
    c4.reverse_parameterization(k, &interval).unwrap();
    let refs = [Some(&c1), Some(&c2), Some(&c3), Some(&c4)];
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), height, true).unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The faces on one extruded/arc surface (a sector wall, analytic or NURBS).
fn arc_walls(b: &Brep) -> usize {
    p1::faces(b, None)
        .unwrap()
        .iter()
        .filter(|&&f| {
            let s = solidcore::topology::face::p2::surface(b, f).unwrap();
            matches!(s.map(Surface::kind), Some(SurfaceKind::SurfOfExtrusion(_)))
        })
        .count()
}

fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let interval = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (interval.min() + interval.max()), 0, true, false)
        .unwrap()[0]
}

/// The top outer arc edge at (`R_OUT`, 0, `TOP`): the one the issue's 3 mm chamfer runs along.
fn top_outer_edge(k: &Kernel, b: &Brep) -> EdgeId {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .find(|&e| midpoint(k, b, e).distance(Point3::new(R_OUT, 0.0, TOP)) < 1e-6)
        .expect("the top outer arc edge")
}

/// The issue's 3 mm chamfer on the top outer arc edge.
fn chamfer_top_outer(k: &mut Kernel, b: &mut Brep) {
    let edge = top_outer_edge(k, b);
    fillet_edges(k, b, &[edge], SIZE, 0, 1, 1.0).expect("the 3 mm chamfer");
}

#[test]
fn a_joined_annular_sector_keeps_one_wall_per_cylinder() {
    let mut k = Kernel::default();
    let outer = sector(&mut k, R_MID, R_OUT, TOP);
    let inner = sector(&mut k, R_IN, R_MID, JOIN);
    let mut joined = boolean_union(&mut k, outer, inner).unwrap();

    // pi/3 * ((10^2 - 6^2) * 20 + (6^2 - 3^2) * 18), the two 120 degree sectors.
    let want = std::f64::consts::PI / 3.0
        * ((R_OUT * R_OUT - R_MID * R_MID) * TOP + (R_MID * R_MID - R_IN * R_IN) * JOIN);
    let got = volume(&k, &mut joined);
    assert!(
        (got - want).abs() <= 1e-6 * want,
        "volume {got}, want {want}"
    );

    // The r 10 wall, the short r 6 step above z = 18 and the r 3 wall: three arc walls.
    // Before the SolidCraft fix the r 10 wall was two pieces, so this was four.
    assert_eq!(
        arc_walls(&joined),
        3,
        "one wall per cylinder, not one per slab"
    );
    assert_eq!(
        p1::faces(&joined, None).unwrap().len(),
        8,
        "five planes and three walls"
    );
    let one_face = p1::edges(&joined, None)
        .unwrap()
        .iter()
        .filter(|&&e| edge::faces(&joined, e, None).unwrap().len() == 1)
        .count();
    assert_eq!(one_face, 0, "no slit inside a merged face");

    // The issue's 3 mm chamfer on the top outer arc edge at (10, 0, 20) succeeds.
    // Its wedge is exactly 27*pi: a 3 mm leg on each face, revolved through 120 degrees.
    let before = volume(&k, &mut joined);
    chamfer_top_outer(&mut k, &mut joined);
    let after = volume(&k, &mut joined);
    let want_after = 1685.0 * std::f64::consts::PI / 3.0;
    assert!(
        (after - want_after).abs() <= 0.01,
        "chamfered join volume {after}, want {want_after}"
    );
    assert!(
        (before - after - 27.0 * std::f64::consts::PI).abs() <= 0.01,
        "chamfer removed {} mm3, want 27*pi",
        before - after
    );
}

/// The control that isolates the join: the outer sector alone takes the same 3 mm chamfer
/// on the same edge, because there is no z = 18 level slicing its wall. Without this, a
/// refused chamfer in the joined case could be read as "3 mm is too big for the edge"
/// rather than "the join split the wall".
#[test]
fn the_same_chamfer_fits_the_outer_sector_alone() {
    let mut k = Kernel::default();
    let mut outer = sector(&mut k, R_MID, R_OUT, TOP);
    let before = volume(&k, &mut outer);
    let want_before = 1280.0 * std::f64::consts::PI / 3.0;
    assert!(
        (before - want_before).abs() <= 1e-6 * want_before,
        "the outer sector holds {before} mm3, want {want_before} mm3"
    );
    chamfer_top_outer(&mut k, &mut outer);
    let after = volume(&k, &mut outer);
    let want_after = 1199.0 * std::f64::consts::PI / 3.0;
    assert!(
        (after - want_after).abs() <= 0.01,
        "the alone sector after chamfer holds {after} mm3, want {want_after} mm3"
    );
}
