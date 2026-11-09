//! tangency-labs/solidcore#968 (a SolidCraft regression case): an 80 x 80 x 40 block with
//! a pocket 60 x 60 at the top, 15 deep, drafted out 10 degrees (its floor a square of
//! `s = 60 - 30 tan 10` at z = 25), R3 on the pocket's 4 rim edges and 4 slanted corner
//! edges, then the floor cut 5 further down by its own boundary swept straight down
//! (variant (a) of the issue).
//!
//! SolidCraft's previous kernel kept the old floor as a face of no thickness over a sealed
//! void. Here the cut removes exactly the floor's prism: `5 A`, `A = s^2 - 4 delta`, where
//! each corner round takes `delta = 1.854828` from the square (the issue's closed form),
//! the result is one solid region with no void, and no face is left at z = 25.
//!
//! The rounds' result is not checked: SMLib's check refuses it before the cut (tests #59
//! and #82, an edge and its vertices about 0.84 off at three of the pocket's top corners,
//! where the convex rim rounds meet the concave corner rounds), which is a separate
//! matter from the cut.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::{create_linear_sweep, create_taper_extrude};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, edgeuse, face, loopuse, EdgeId, FaceId};
use solidcore::{Brep, Kernel};

/// The floor's side at z = 25.
fn floor_side() -> f64 {
    60.0 - 30.0 * 10f64.to_radians().tan()
}

/// The area one corner round takes from the floor's square (the issue's closed form).
const DELTA: f64 = 1.854828;

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The ends of edge `e`.
fn ends(k: &Kernel, b: &Brep, e: EdgeId) -> (Point3, Point3) {
    let c = edge::curve(b, e).unwrap().unwrap();
    let ivl = edge::interval(b, e).unwrap();
    (
        c.evaluate_point(k, ivl.min()).unwrap(),
        c.evaluate_point(k, ivl.max()).unwrap(),
    )
}

/// The edges of face `f`'s loops, in loop order.
fn face_edges(b: &Brep, f: FaceId) -> Vec<EdgeId> {
    let mut edges = Vec::new();
    for l in face::p2::loops(b, f, false, None).unwrap() {
        for eu in loopuse::loop_edgeuses(b, l).unwrap() {
            edges.push(edgeuse::p1::edge(b, eu).unwrap().unwrap());
        }
    }
    edges
}

/// The planar faces whose edges all lie at height `z`.
fn faces_at(k: &Kernel, b: &Brep, z: f64) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Plane(_)
            ) && face_edges(b, f).iter().all(|&e| {
                let (p, q) = ends(k, b, e);
                (p.z - z).abs() < 1e-6 && (q.z - z).abs() < 1e-6
            })
        })
        .collect()
}

/// Steps 1 to 3: the block, the drafted pocket and the R3 rounds.
fn rounded_pocket(k: &mut Kernel) -> Brep {
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 80.0, 80.0, 40.0).unwrap();
    // the pocket: its floor square at z = 25 extruded 20 up, drafted out 10 degrees, so
    // that it meets the top (z = 40) in the 60 x 60 square
    let s = floor_side();
    let (lo, hi) = (40.0 - s / 2.0, 40.0 + s / 2.0);
    let corners = [(lo, lo), (hi, lo), (hi, hi), (lo, hi)];
    let square: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            Curve::from(
                create_line_segment(Vec3::new(a.0, a.1, 25.0), Vec3::new(b.0, b.1, 25.0)).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = square.iter().map(Some).collect();
    // the walls meet at corner edges (WR-521), as a drafted pocket's do
    k.fixes.taper_tangent_corner_planes = true;
    let tool = create_taper_extrude(k, &refs, 20.0, -10.0, 3, false).unwrap();
    k.fixes.taper_tangent_corner_planes = false;
    let mut pocketed = boolean_difference(k, block, tool).unwrap();

    // R3 on the rim edges (z = 40) and the slanted corner edges of the pocket
    let inside = |p: Point3| p.x > 5.0 && p.x < 75.0 && p.y > 5.0 && p.y < 75.0;
    let edges: Vec<EdgeId> = pocketed
        .edge_list()
        .iter()
        .copied()
        .filter(|&e| {
            let (p, q) = ends(k, &pocketed, e);
            let rim = (p.z - 40.0).abs() < 1e-6 && (q.z - 40.0).abs() < 1e-6;
            let slanted = (p.z - q.z).abs() > 10.0;
            inside(p) && inside(q) && (rim || slanted)
        })
        .collect();
    assert_eq!(edges.len(), 8, "the pocket's rim and corner edges");
    fillet_edges(k, &mut pocketed, &edges, 3.0, 1, 1, 1.0).unwrap();
    pocketed
}

#[test]
fn issue_968_floor_cut_straight_down_leaves_no_skin() {
    let mut k = Kernel::default();
    let mut rounded = rounded_pocket(&mut k);
    let before = volume(&k, &mut rounded);

    // the floor: 4 lines and, where the corner rounds meet it, 4 quarter ellipses
    let floors = faces_at(&k, &rounded, 25.0);
    assert_eq!(floors.len(), 1, "one floor face");
    let mut boundary = Vec::new();
    for e in face_edges(&rounded, floors[0]) {
        let mut curve = edge::curve(&rounded, e).unwrap().unwrap().clone();
        let mut ivl = edge::interval(&rounded, e).unwrap();
        if ivl != curve.natural_interval() {
            curve.trim(&k, &mut ivl, TrimMode::Notify).unwrap();
        }
        boundary.push(curve);
    }
    assert_eq!(boundary.len(), 8, "the floor's boundary");

    // step 4 (a): the floor's prism, its boundary swept 5 straight down, cut away
    let refs: Vec<Option<&Curve>> = boundary.iter().map(Some).collect();
    let prism = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, -1.0), 5.0, true).unwrap();
    let mut cut = boolean_difference(&mut k, rounded, prism).unwrap();

    let s = floor_side();
    let removed = 5.0 * (s * s - 4.0 * DELTA);
    let got = before - volume(&k, &mut cut);
    // DELTA has 7 significant digits
    assert!(
        (got - removed).abs() <= 1e-6 * removed,
        "removed {got}, want {removed}, relative {:e}",
        (got - removed).abs() / removed
    );
    // one solid region besides the infinite one: no sealed void
    assert_eq!(p1::regions(&cut, None).unwrap().len(), 2, "regions");
    assert!(is_manifold_solid(&cut).unwrap(), "not a manifold solid");
    // the old floor is gone; the new one is at z = 20
    assert!(
        faces_at(&k, &cut, 25.0).is_empty(),
        "a face is left at z = 25"
    );
    assert_eq!(faces_at(&k, &cut, 20.0).len(), 1, "the new floor");
}
