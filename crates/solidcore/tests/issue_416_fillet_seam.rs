//! tangency-labs/solidcore#416: a 25 x 25 x 10 box whose bottom edges are rounded R2, then
//! its vertical edges R2 (each one's tangent chain runs down the seam where two bottom
//! rounds meet). The corner topology's drop of the corner point onto the rounds' surfaces
//! (`SmFilletDegenerateCorner::MakeCornerTopology`) runs with point testing off, so it
//! must not reach the face classifier: the round used to stop on
//! `Unported SmFace::PointClassify`, and now reaches the solver as upstream does.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
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

fn tray(fix: bool) -> (f64, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_corner_ball_patch = fix;
    let mut tray = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 25.0, 25.0, 10.0).unwrap();
    let bottom = edges_where(&k, &tray, |m| m.z.abs() < 1e-6);
    fillet_edges(&mut k, &mut tray, &bottom, 2.0, 1, 1, 1.0).unwrap();
    let rounded = volume(&k, &mut tray);
    // each vertical edge, and the seam between two bottom rounds it continues down (its
    // tangent chain)
    let near_side = |v: f64, side: f64| (v.min(side - v) > 0.1) && (v.min(side - v) < 1.9);
    let vertical = edges_where(&k, &tray, |m| {
        (m.z - 6.0).abs() < 1e-6
            || (m.z > 0.1 && m.z < 1.9 && near_side(m.x, 25.0) && near_side(m.y, 25.0))
    });
    let r = fillet_edges(&mut k, &mut tray, &vertical, 2.0, 1, 1, 1.0);
    (rounded, volume(&k, &mut tray), r)
}

#[test]
fn the_degenerate_corner_drop_does_not_stop_on_the_face_classifier() {
    let (_, _, r) = tray(false);
    assert!(
        !matches!(r, Err(solidcore::SolidError::Unported { .. })),
        "{r:?}"
    );
}

#[test]
fn the_corner_ball_is_made_where_two_bottom_rounds_meet_with_the_fix() {
    // to 1e-7 relative: each vertical edge loses (1 - pi/4) 4 along 8, each corner the octant of the two
    // bottom rounds' crossing (16/3, a Steinmetz solid's eighth) less the corner ball's
    // (4 pi / 3)
    let removed = 4.0 * (32.0 * (1.0 - PI / 4.0) + 16.0 / 3.0 - 4.0 * PI / 3.0);

    let (rounded, after, r) = tray(true);
    r.unwrap();
    assert!(
        (after - (rounded - removed)).abs() < 1e-7 * after,
        "{rounded} {after}"
    );

    // default: upstream's 1001
    let (_, _, r) = tray(false);
    assert!(
        matches!(r, Err(solidcore::SolidError::Upstream { code: 1001, .. })),
        "{r:?}"
    );
}
