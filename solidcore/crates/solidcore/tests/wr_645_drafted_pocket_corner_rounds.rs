//! WR-645 (found with tangency-labs/solidcore#968's model): R3 on the 4 rim edges and the
//! 4 corner edges of a pocket whose walls lean out from its floor. At each top corner
//! two convex rim rounds meet the concave corner round, and
//! `SmFilletCorner::MakeVariableRadiusFillet` turns the four-sided corner into a
//! variable-radius fillet between the top face and the corner round. Its rail on the
//! corner round replaces the corner round's end edge, but with the walls drafted it
//! runs off the end of that round (0.42 at R3, 10 degrees): the edge's tolerance and
//! its vertices' are raised to twice that, and SMLib's check refuses the body (tests
//! #59 and #82). With vertical walls the rail is that end edge and the corner is exact.
//!
//! The opt-in `Kernel::fixes.fillet_corner_variable_radius_rail_on_opposite_fillet`
//! refuses the conversion when the rail leaves the opposite fillet's surface, so the
//! corner is the patch on its exact edges, as upstream already builds one of the four
//! corners.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// The pocket's floor (z = 25) side for walls leaning out `draft` degrees that meet the
/// top (z = 40) in the 60 x 60 square.
fn floor_side(draft: f64) -> f64 {
    60.0 - 30.0 * draft.to_radians().tan()
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

/// The 80 x 80 x 40 block minus the pocket: the box z 25..45 over the floor square
/// intersected with four boxes, each turned `draft` degrees about a line through a floor
/// edge, so that the walls lean out and meet at corner edges.
fn pocketed(k: &mut Kernel, draft: f64) -> Brep {
    let s = floor_side(draft);
    let (lo, hi) = (40.0 - s / 2.0, 40.0 + s / 2.0);
    let mut tool = create_box(k, Point3::new(-100.0, -100.0, 25.0), 280.0, 280.0, 20.0).unwrap();
    let (along_y, along_x) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
    // (box origin, size, pivot, axis, angle): x >= lo, x <= hi, y >= lo, y <= hi at the floor
    let slabs = [
        (
            Point3::new(lo, -100.0, 0.0),
            (200.0, 280.0),
            Point3::new(lo, 0.0, 25.0),
            along_y,
            -draft,
        ),
        (
            Point3::new(hi - 200.0, -100.0, 0.0),
            (200.0, 280.0),
            Point3::new(hi, 0.0, 25.0),
            along_y,
            draft,
        ),
        (
            Point3::new(-100.0, lo, 0.0),
            (280.0, 200.0),
            Point3::new(0.0, lo, 25.0),
            along_x,
            draft,
        ),
        (
            Point3::new(-100.0, hi - 200.0, 0.0),
            (280.0, 200.0),
            Point3::new(0.0, hi, 25.0),
            along_x,
            -draft,
        ),
    ];
    for (origin, (l, w), pivot, axis, angle) in slabs {
        let mut slab = create_box(k, origin, l, w, 100.0).unwrap();
        if angle != 0.0 {
            api_rotate(
                k,
                Some(TransformObject::Brep(&mut slab)),
                pivot,
                axis,
                angle,
            )
            .unwrap();
        }
        tool = boolean_intersection(k, tool, slab).unwrap();
    }
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 80.0, 80.0, 40.0).unwrap();
    boolean_difference(k, block, tool).unwrap()
}

struct Rounded {
    counts: (usize, usize, usize),
    volume: f64,
    manifold: bool,
    max_edge_tolerance: f64,
    check: Vec<String>,
}

/// The pocket drafted `draft` degrees with its rim and corner edges rounded `radius`, with
/// the fix `on` or off.
fn rounded(draft: f64, radius: f64, on: bool) -> Rounded {
    let mut k = Kernel::default();
    k.fixes
        .fillet_corner_variable_radius_rail_on_opposite_fillet = on;
    let mut b = pocketed(&mut k, draft);
    let inside = |p: Point3| p.x > 5.0 && p.x < 75.0 && p.y > 5.0 && p.y < 75.0;
    let edges: Vec<EdgeId> = b
        .edge_list()
        .iter()
        .copied()
        .filter(|&e| {
            let (p, q) = ends(&k, &b, e);
            let rim = (p.z - 40.0).abs() < 1e-6 && (q.z - 40.0).abs() < 1e-6;
            let corner = (p.z - q.z).abs() > 10.0;
            inside(p) && inside(q) && (rim || corner)
        })
        .collect();
    assert_eq!(edges.len(), 8, "the rim and corner edges");
    fillet_edges(&mut k, &mut b, &edges, radius, 1, 1, 1.0).unwrap();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let volume = brep_volume(&k, &mut b, 1e-8, &attrs).unwrap();
    let max_edge_tolerance = b
        .edge_list()
        .iter()
        .map(|&e| edge::tolerance(&b, e).unwrap())
        .fold(0.0, f64::max);
    Rounded {
        counts: (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len(),
        ),
        volume,
        manifold: is_manifold_solid(&b).unwrap(),
        max_edge_tolerance,
        check: check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines(),
    }
}

#[test]
fn wr_645_drafted_pocket_corners_are_exact_with_the_fix() {
    // upstream: three corners' end edges 0.845 off, refused by SMLib's check
    let upstream = rounded(10.0, 3.0, false);
    assert!(
        upstream.max_edge_tolerance > 0.8,
        "upstream: {}",
        upstream.max_edge_tolerance
    );
    assert!(
        upstream.check.iter().any(|l| l.contains("Test #59")),
        "upstream: {:?}",
        upstream.check
    );

    // with the fix, at 10 and 2 degrees: the same faces, every corner exact, valid
    for draft in [10.0, 2.0] {
        let fixed = rounded(draft, 3.0, true);
        assert_eq!(fixed.counts, (23, 52, 32), "draft {draft}");
        assert!(fixed.manifold, "draft {draft}: not a manifold solid");
        assert!(
            fixed.max_edge_tolerance < 1e-3,
            "draft {draft}: edge tolerance {}",
            fixed.max_edge_tolerance
        );
        assert!(fixed.check.is_empty(), "draft {draft}: {:?}", fixed.check);
    }
}

#[test]
fn wr_645_vertical_pocket_walls_keep_the_variable_radius_corner() {
    // with vertical walls the rail is the corner round's end edge: the fix changes
    // nothing
    let upstream = rounded(0.0, 3.0, false);
    let fixed = rounded(0.0, 3.0, true);
    assert!(upstream.check.is_empty(), "upstream: {:?}", upstream.check);
    assert_eq!(fixed.counts, upstream.counts);
    assert_eq!(fixed.volume, upstream.volume);
    assert!(fixed.check.is_empty(), "fixed: {:?}", fixed.check);
}
