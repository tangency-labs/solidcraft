//! The fork's tangency-labs/smlib@95bf434, under WR-588's
//! `Kernel::fixes.face_tight_box_searches_trim`: `sm_CellDirectedMax` takes a cell's greatest
//! point as the face's only when it classifies inside the face (`SM_PC_FACE`), not on an edge
//! or a vertex, which is within the face's zone tolerance of its boundary inside or out.
//!
//! The fork's case: the corner of a box of 10 with its three edges there rounded r 1 is a
//! sphere octant about (1, 1, 1) in [0, 1]^3. Its sphere's points just past the arc in y = 1
//! classify on the boundary, and the face boxed to y 1.0000243; with the fix it boxes to
//! [0, 1]^3, to 1e-9.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::extent3d::Extent3d;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::face::p3::calculate_tight_bounding_box;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// The edges of `b` with an end at `p`.
fn edges_at(k: &Kernel, b: &Brep, p: Point3) -> Vec<EdgeId> {
    b.edges()
        .ids()
        .filter(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            s.distance(p) < 1e-6 || t.distance(p) < 1e-6
        })
        .collect()
}

/// The tight box of the sphere octant at the box's rounded corner, with the search as given.
fn octant_box(searches_trim: bool) -> Extent3d {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let edges = edges_at(&k, &b, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(edges.len(), 3);
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, 1.0, 1, 1, 1.0)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    k.fixes.face_tight_box_searches_trim = searches_trim;
    let spheres: Vec<_> = b
        .faces()
        .ids()
        .filter(|&f| {
            face::surface(&b, f)
                .unwrap()
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Sphere(_)))
        })
        .collect();
    assert_eq!(spheres.len(), 1, "one sphere corner");
    calculate_tight_bounding_box(&k, &b, spheres[0]).unwrap()
}

/// With the search the octant boxes to [0, 1]^3 (to 1e-9). Before 95bf434 points classified
/// on its boundary arcs took the box past them (y 1.0000243).
#[test]
fn fork_95bf434_rounded_corner_boxes_to_its_octant() {
    let bx = octant_box(true);
    let (lo, hi) = (bx.min(), bx.max());
    for (what, got, want) in [
        ("x min", lo.x, 0.0),
        ("y min", lo.y, 0.0),
        ("z min", lo.z, 0.0),
        ("x max", hi.x, 1.0),
        ("y max", hi.y, 1.0),
        ("z max", hi.z, 1.0),
    ] {
        assert!((got - want).abs() <= 1e-9, "{what}: {got}, want {want}");
    }
}

/// Without the search the box is the surface's over the face's domain points that classify
/// on the face, as upstream; it is not tighter than the octant.
#[test]
fn rounded_corner_box_without_the_search_holds_the_octant() {
    let bx = octant_box(false);
    let (lo, hi) = (bx.min(), bx.max());
    assert!(lo.x <= 1e-9 && lo.y <= 1e-9 && lo.z <= 1e-9, "{lo:?}");
    assert!(
        hi.x >= 1.0 - 1e-9 && hi.y >= 1.0 - 1e-9 && hi.z >= 1.0 - 1e-9,
        "{hi:?}"
    );
}
