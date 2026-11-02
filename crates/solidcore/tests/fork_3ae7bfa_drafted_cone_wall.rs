//! Regression test for the fork's smlib#116 (tangency-labs/smlib@3ae7bfa and @dda881f,
//! #2342): the drafted tangent peg.
//!
//! The part is the fork's capi `tangent_peg(5 deg)` (capi test `drafted_peg_one_cone`,
//! 10dd6d4): a bar 40 x 16 whose far end is a half disc R 8 about (40, 8), drafted 5
//! degrees inward over 10 with `CurveExtension` corners (the fork's
//! `SMLIB_DRAFT_CORNER_CURVATURE`), and a disc R 8 - 10 tan 5 standing on its top, drafted
//! the same over 10 more, joined. Both arcs' walls lie on one cone about x = 40, y = 8 of
//! semi-angle 5 degrees, radius 8 - z tan 5.
//!
//! - 3ae7bfa (no flag: `CurveExtension` is the fork's own corner rule, WR-520's area): a
//!   drafted arc's wall is its exact cone (`sm_DraftCone`), not the B-spline ruled between
//!   its ends, and its seams' pcurves are cut at the cone's spans.
//! - dda881f (`cover_wraps_across_closed_seam`, WR-599): the union's cover reads the
//!   closure of a surface whose NURBS swaps u and v (the cones) in the step's direction, so
//!   it takes the disc's cone face over the bar's and the two are one face; the vertex
//!   V = (40, 10 tan 5, 10) where the front top edge meets the concave arc has three edges.
//!
//! Volumes in closed form (the capi test's): each wall leans in by t tan 5 at height t, so
//! the bar's section at height t is the rectangle x in [d, 40], y in [d, 16 - d] and the half
//! disc R 8 - d, the disc's the disc R 8 - d (d = t tan 5):
//! `V_bar = int_0^10 ((40 - d)(16 - 2d) + pi (8 - d)^2 / 2) dt`,
//! `V_disc = int_10^20 pi (8 - d)^2 dt`. The fork at dda881f (built locally, the same inputs:
//! the arc an `SmCircle` over 180 degrees, `SmApiCreateTaperExtrude(.., 10, 5, 3,
//! SM_OC_CURVE_EXTENSION, ..)`, `SmApiBooleanUnion`) gives the bar 6 faces, 12 edges, 8
//! vertices, V 6884.533175779990; the disc 3, 3, 2, V 1407.079018122895; the peg 7 faces,
//! 15 edges, 10 vertices, V 8291.612193902547, three edges at V, one cone face.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::{
    create_taper_extrude_with_corner, OffsetCornerType, TaperExtrude,
};
use solidcore::topology::brep::p1::vertices;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{face, vertex};
use solidcore::{Brep, Kernel};

const PI: f64 = core::f64::consts::PI;

fn tan5() -> f64 {
    5.0_f64.to_radians().tan()
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
}

/// The circle about `c` of radius `r` from angle `from` (the frame's x axis) over
/// `degrees`, as the capi's `arc` builds it (`SmCircle::CreateCanonical`).
fn arc(k: &Kernel, c: Point3, r: f64, x: Vec3, degrees: f64) -> Curve {
    let frame = Axis2Placement::new(c, x, Vec3::Z.cross(x));
    let ivl = Extent1d::from_min_max(0.0, degrees);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&ivl), None).unwrap())
}

fn taper(k: &mut Kernel, cs: &[Curve]) -> Brep {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    match create_taper_extrude_with_corner(k, &refs, 10.0, 5.0, 3, OffsetCornerType::CurveExtension)
        .unwrap()
    {
        TaperExtrude::Solid(b) => b,
        TaperExtrude::Refused(r) => panic!("draft refused: {r:?}"),
    }
}

fn bar(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let cs = [
        line(p(0.0, 0.0, 0.0), p(40.0, 0.0, 0.0)),
        arc(k, p(40.0, 8.0, 0.0), 8.0, Vec3::new(0.0, -1.0, 0.0), 180.0),
        line(p(40.0, 16.0, 0.0), p(0.0, 16.0, 0.0)),
        line(p(0.0, 16.0, 0.0), p(0.0, 0.0, 0.0)),
    ];
    taper(k, &cs)
}

fn disc(k: &mut Kernel) -> Brep {
    let cs = [arc(
        k,
        Point3::new(40.0, 8.0, 10.0),
        8.0 - 10.0 * tan5(),
        Vec3::X,
        360.0,
    )];
    taper(k, &cs)
}

/// `int_a^b (c0 + c1 k t + c2 k^2 t^2) dt`, `k = tan 5`.
fn integral(a: f64, b: f64, c0: f64, c1: f64, c2: f64) -> f64 {
    let k = tan5();
    c0 * (b - a) + c1 * k * (b * b - a * a) / 2.0 + c2 * k * k * (b * b * b - a * a * a) / 3.0
}

fn bar_volume() -> f64 {
    // (40 - kt)(16 - 2kt) = 640 - 96 kt + 2 k^2 t^2, (8 - kt)^2 = 64 - 16 kt + k^2 t^2
    integral(0.0, 10.0, 640.0, -96.0, 2.0) + PI / 2.0 * integral(0.0, 10.0, 64.0, -16.0, 1.0)
}

fn disc_volume() -> f64 {
    PI * integral(10.0, 20.0, 64.0, -16.0, 1.0)
}

/// The volume at accuracy `acc`.
fn volume(k: &Kernel, b: &mut Brep, acc: f64) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, acc, &attrs).unwrap()
}

/// (faces, edges, vertices).
fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        get_faces(b).unwrap().len(),
        get_edges(b).unwrap().len(),
        vertices(b, None).unwrap().len(),
    )
}

/// The faces whose surface is a cone, each with the largest distance of an 11 x 11 grid
/// of its surface's points from the drafted cone `rho = 8 - z tan 5` about (40, 8), and its
/// semi-angle in degrees.
fn cone_faces(k: &Kernel, b: &Brep) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let s = face::surface(b, f).unwrap().unwrap();
        let SurfaceKind::Cone(c) = s.kind() else {
            continue;
        };
        let dom = s.natural_uv_domain_with_nurb(k).unwrap();
        let mut worst: f64 = 0.0;
        for i in 0..=10 {
            for j in 0..=10 {
                let u = dom.u_min() + (dom.u_max() - dom.u_min()) * f64::from(i) / 10.0;
                let v = dom.v_min() + (dom.v_max() - dom.v_min()) * f64::from(j) / 10.0;
                let q = s.evaluate_point(k, Point2::new(u, v)).unwrap();
                let rho = (q.x - 40.0).hypot(q.y - 8.0);
                worst = worst.max((rho - (8.0 - q.z * tan5())).abs());
            }
        }
        out.push((worst, c.semi_angle_deg()));
    }
    out
}

/// The number of edges at the vertex at `at`, `None` without one there.
fn edges_at(b: &Brep, at: Point3) -> Option<usize> {
    let v = vertices(b, None)
        .unwrap()
        .into_iter()
        .find(|&v| vertex::point(b, v).unwrap().distance(at) < 1e-9)?;
    Some(
        get_edges(b)
            .unwrap()
            .into_iter()
            .filter(|&e| {
                let (s, t) = vertex::edge_vertex_pair(b, e).unwrap();
                s == Some(v) || t == Some(v)
            })
            .count(),
    )
}

fn check(k: &mut Kernel, b: &mut Brep) {
    b.use_arena_naming().unwrap();
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "check_brep: {lines:?}");
}

/// The kernel of the peg's union: WR-599's cover across a closed seam (`cover`) and
/// WR-598's seam iso-curve join (`delete_vertex_joins_seam_iso_curve`), which merges the seam's vertex the cover leaves on the
/// bar's bottom arc (the fork's counts need both).
fn peg_kernel(cover: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.cover_wraps_across_closed_seam = cover;
    k.fixes.delete_vertex_joins_seam_iso_curve = true;
    k
}

fn peg(k: &mut Kernel) -> Brep {
    let a = bar(k);
    let d = disc(k);
    boolean_union(k, a, d).unwrap()
}

/// V, where the front top edge meets the concave arc.
fn v_point() -> Point3 {
    Point3::new(40.0, 10.0 * tan5(), 10.0)
}

/// 3ae7bfa: the bar's drafted half-disc wall is one `SmCone` face on the drafted cone, of
/// semi-angle 5 degrees (negative: it narrows up its axis), in the cone's own
/// parametrization (the fork's natural domain [0, 1] x [0, 10]), and the bar is the fork's
/// 6 faces, 12 edges, 8 vertices with the closed-form volume. Before, the wall was the
/// B-spline ruled between its ends, which the B-rep copy's `CopyAndAddAnalytics` took for a
/// cone over the ruled surface's domain (in the fork's C++ it stays a B-spline).
#[test]
fn the_bars_arc_wall_is_its_exact_cone() {
    let mut k = Kernel::default();
    let mut b = bar(&mut k);
    assert_eq!(counts(&b), (6, 12, 8));
    let cones = cone_faces(&k, &b);
    assert_eq!(cones.len(), 1, "cone faces {cones:?}");
    let (dev, semi) = cones[0];
    assert!(dev < 1e-12, "off the cone by {dev:e}");
    assert!((semi + 5.0).abs() < 1e-12, "semi-angle {semi}");
    // in the cone's own parametrization, as the fork's: [0, 1] x [0, 10] (the ruled wall
    // taken for its cone was [-0.001, pi + 0.001] x [0, 1])
    let cone = get_faces(&b)
        .unwrap()
        .into_iter()
        .map(|f| face::surface(&b, f).unwrap().unwrap())
        .find(|s| matches!(s.kind(), SurfaceKind::Cone(_)))
        .unwrap();
    let dom = cone.natural_uv_domain_with_nurb(&k).unwrap();
    let got = [dom.u_min(), dom.u_max(), dom.v_min(), dom.v_max()];
    for (g, w) in got.iter().zip([0.0, 1.0, 0.0, 10.0]) {
        assert!((g - w).abs() < 1e-12, "cone domain {got:?}");
    }
    check(&mut k, &mut b);
    let v = volume(&k, &mut b, 1e-10);
    let want = bar_volume();
    assert!((v - want).abs() <= 1e-11 * want, "volume {v} want {want}");
}

/// dda881f with WR-599's cover: the union takes the bar's cone wall into the disc's, one
/// cone face, three edges at V; the fork's 7 faces, 15 edges, 10 vertices, and the
/// closed-form volume.
#[test]
fn the_drafted_peg_has_one_cone_face() {
    let mut k = peg_kernel(true);
    let mut b = peg(&mut k);
    assert_eq!(counts(&b), (7, 15, 10));
    let cones = cone_faces(&k, &b);
    assert_eq!(cones.len(), 1, "cone faces {cones:?}");
    // the disc's cone (its offset wall), the fork's own 6.7e-12 off
    assert!(cones[0].0 < 1e-11, "off the cone by {:e}", cones[0].0);
    assert_eq!(edges_at(&b, v_point()), Some(3));
    check(&mut k, &mut b);
    let v = volume(&k, &mut b, 1e-6);
    let want = bar_volume() + disc_volume();
    // (the fork at dda881f: 8291.612193902547 at accuracy 1e-10; here 1e-6, to keep the test
    // quick)
    assert!((v - want).abs() <= 1e-7 * want, "volume {v} want {want}");
}

/// WR-599's cover off (upstream refuses every crossing of a seam): the two cone walls stay
/// apart, four edges at V, as the fork before dda881f.
#[test]
fn without_the_cover_the_cone_walls_stay_two_faces() {
    let mut k = peg_kernel(false);
    let mut b = peg(&mut k);
    assert_eq!(counts(&b).0, 8);
    assert_eq!(cone_faces(&k, &b).len(), 2);
    assert_eq!(edges_at(&b, v_point()), Some(4));
    check(&mut k, &mut b);
}
