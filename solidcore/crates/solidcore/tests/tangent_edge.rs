//! `query::tangent_edge` (#3141): whether an edge's faces meet tangentially all along it, and
//! the least angle between their outward normals, against closed forms.
//!
//! - a box: every edge's faces meet square, pi/2;
//! - a wedge whose two sloped faces meet at an inside angle of 60 degrees along its ridge: the
//!   outward normals are 120 degrees apart there;
//! - a cylinder: its side meets its caps square, pi/2;
//! - a box with one edge rounded: the round meets both its faces tangentially, so its two
//!   edges along the box are smooth (angle 0), while the box's other edges stay square.
//!
//! `query::edge_turn`, at a vertex:
//! - two edges of a box's corner turn pi/2 from running on into each other;
//! - a stadium's straight top edge runs on into its end arc with no turn (the arc leaves the
//!   line's end along the line);
//! - an L-section prism's inner vertical edge is concave, its outer ones convex.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::{edge_closest_point, get_edges};
use solidcore::query::edge_turn::{convex_at, turn_at};
use solidcore::query::tangent_edge::{least_angle_along, tangent_all_along, Tangency};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::{edge, EdgeId, VertexId};
use solidcore::{Brep, Kernel};
use std::f64::consts::{FRAC_PI_2, PI};

/// Half a degree: SolidCraft's smooth-edge angle.
const SMOOTH: f64 = 0.5 * PI / 180.0;

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    get_edges(b)
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            edge_closest_point(k, b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 1e-6)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("an edge through the point")
        .0
}

fn angle(k: &Kernel, b: &mut Brep, e: EdgeId) -> f64 {
    let a = least_angle_along(k, b, e).unwrap().expect("two faces");
    assert_eq!(a.margin, 0.0, "no blend faces here");
    a.angle
}

#[test]
fn every_edge_of_a_box_is_square() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 4.0).unwrap();
    let edges = get_edges(&b).unwrap();
    assert_eq!(edges.len(), 12);
    for e in edges {
        let a = angle(&k, &mut b, e);
        assert!((a - FRAC_PI_2).abs() < 1e-12, "{e:?}: {a}");
        assert_eq!(
            tangent_all_along(&k, &mut b, e, SMOOTH).unwrap(),
            Some(Tangency::NotTangent)
        );
    }
}

/// A prism 10 long along x on the equilateral triangle (-h tan 30, 0), (h tan 30, 0), (0, h) in
/// (y, z): every long edge is a 60 degree inside angle, so its faces' outward normals are
/// pi - 60 degrees = 120 degrees apart.
#[test]
fn a_wedge_s_ridge_is_its_dihedral_s_supplement() {
    let mut k = Kernel::default();
    let h = 5.0;
    let half = h * (PI / 6.0).tan();
    let p = |y: f64, z: f64| Point3::new(0.0, y, z);
    let pts = [p(-half, 0.0), p(half, 0.0), p(0.0, h)];
    let curves: Vec<Curve> = (0..3)
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % 3]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let x = Vec3::new(1.0, 0.0, 0.0);
    let mut b = create_linear_sweep(&mut k, &refs, x, 10.0, true).unwrap();
    let ridge = pick(&k, &b, Point3::new(5.0, 0.0, h));
    let a = angle(&k, &mut b, ridge);
    assert!((a - 2.0 * PI / 3.0).abs() < 1e-12, "{a}");
    let base = pick(&k, &b, Point3::new(5.0, half, 0.0));
    let a = angle(&k, &mut b, base);
    assert!((a - 2.0 * PI / 3.0).abs() < 1e-12, "{a}");
}

/// A cylinder r 3, 7 high: its side meets each cap square along the cap's circle, and meets
/// itself smoothly across its seam, the line x = 3, y = 0 (picked off the seam: a pick at
/// (3, 0, 0) is as near the seam as the circle).
#[test]
fn a_cylinder_s_side_meets_its_caps_square() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 3.0, 7.0).unwrap();
    for at in [Point3::new(0.0, 3.0, 0.0), Point3::new(0.0, -3.0, 7.0)] {
        let e = pick(&k, &b, at);
        let a = angle(&k, &mut b, e);
        assert!((a - FRAC_PI_2).abs() < 1e-9, "{at:?}: {a}");
        assert_eq!(
            tangent_all_along(&k, &mut b, e, SMOOTH).unwrap(),
            Some(Tangency::NotTangent)
        );
    }
    let seam = pick(&k, &b, Point3::new(3.0, 0.0, 3.5));
    assert_eq!(
        tangent_all_along(&k, &mut b, seam, SMOOTH).unwrap(),
        Some(Tangency::Tangent)
    );
}

/// The box's top front edge rounded r 1: the round runs into the top along y = 1, z = 4 and
/// into the front along y = 0, z = 3, tangentially; the box's top back edge stays square.
#[test]
fn a_round_meets_its_faces_smoothly() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 4.0).unwrap();
    let e = pick(&k, &b, Point3::new(5.0, 0.0, 4.0));
    fillet_edges(&mut k, &mut b, &[e], 1.0, 1, 2, 0.0).unwrap();
    for at in [Point3::new(5.0, 1.0, 4.0), Point3::new(5.0, 0.0, 3.0)] {
        let e = pick(&k, &b, at);
        let a = angle(&k, &mut b, e);
        assert!(a < 1e-9, "{at:?}: {a}");
        assert_eq!(
            tangent_all_along(&k, &mut b, e, SMOOTH).unwrap(),
            Some(Tangency::Tangent)
        );
    }
    let back = pick(&k, &b, Point3::new(5.0, 6.0, 4.0));
    let a = angle(&k, &mut b, back);
    assert!((a - FRAC_PI_2).abs() < 1e-12, "{a}");
}

/// The vertex edges `e` and `f` share.
fn shared(b: &Brep, e: EdgeId, f: EdgeId) -> VertexId {
    let ends = |x: EdgeId| {
        [
            edge::start_vertex(b, x).unwrap(),
            edge::end_vertex(b, x).unwrap(),
        ]
    };
    let (a, c) = (ends(e), ends(f));
    a.into_iter()
        .flatten()
        .find(|v| c.contains(&Some(*v)))
        .expect("a shared vertex")
}

#[test]
fn a_box_corner_turns_a_right_angle() {
    let mut k = Kernel::default();
    let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 4.0).unwrap();
    let e = pick(&k, &b, Point3::new(5.0, 0.0, 4.0));
    let f = pick(&k, &b, Point3::new(0.0, 3.0, 4.0));
    let v = shared(&b, e, f);
    let turn = turn_at(&k, &b, e, f, v).unwrap().expect("both directions");
    assert!((turn - FRAC_PI_2).abs() < 1e-12, "{turn}");
}

/// A prism 4 high on a stadium: a line from (0, 0) to (20, 0), a half circle of radius 5
/// about (20, 5), the line back and the other half circle about (0, 5).
fn stadium(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let curves = [
        Curve::from(create_line_segment(p(0.0, 0.0), p(20.0, 0.0)).unwrap()),
        Curve::from(create_arc(p(20.0, 5.0), 5.0, -90.0, 90.0).unwrap()),
        Curve::from(create_line_segment(p(20.0, 10.0), p(0.0, 10.0)).unwrap()),
        Curve::from(create_arc(p(0.0, 5.0), 5.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 4.0, true).unwrap()
}

#[test]
fn a_line_runs_on_into_its_tangent_arc_without_a_turn() {
    let mut k = Kernel::default();
    let b = stadium(&mut k);
    let line = pick(&k, &b, Point3::new(10.0, 0.0, 4.0));
    let arc = pick(&k, &b, Point3::new(25.0, 5.0, 4.0));
    let v = shared(&b, line, arc);
    let turn = turn_at(&k, &b, line, arc, v)
        .unwrap()
        .expect("both directions");
    assert!(turn < 1e-12, "{turn}");
    // and the wall's vertical edge there turns square from both
    let wall = pick(&k, &b, Point3::new(20.0, 0.0, 2.0));
    let turn = turn_at(&k, &b, line, wall, v)
        .unwrap()
        .expect("both directions");
    assert!((turn - FRAC_PI_2).abs() < 1e-12, "{turn}");
}

/// The L section (0, 0), (10, 0), (10, 4), (4, 4), (4, 10), (0, 10), 5 high: its vertical edge
/// at (4, 4) is the inside corner, the one at (10, 0) an outside one.
#[test]
fn an_l_section_s_inside_corner_is_concave() {
    let mut k = Kernel::default();
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let pts = [
        p(0.0, 0.0),
        p(10.0, 0.0),
        p(10.0, 4.0),
        p(4.0, 4.0),
        p(4.0, 10.0),
        p(0.0, 10.0),
    ];
    let curves: Vec<Curve> = (0..6)
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % 6]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut b = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 5.0, true).unwrap();
    for (at, convex) in [
        (Point3::new(4.0, 4.0, 2.5), false),
        (Point3::new(10.0, 0.0, 2.5), true),
    ] {
        let e = pick(&k, &b, at);
        for v in [
            edge::start_vertex(&b, e).unwrap(),
            edge::end_vertex(&b, e).unwrap(),
        ] {
            let v = v.expect("an end");
            assert_eq!(convex_at(&k, &mut b, e, v).unwrap(), Some(convex), "{at:?}");
        }
    }
}
