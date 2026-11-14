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
//! Two more cases of 3ae7bfa (#2497): a draft that leans off its profile's plane builds, its
//! arc's wall the ruled B-spline on its oblique cone; and with `keep_exact_pcurves` a seam
//! that crosses a span end of its cone keeps its pcurve as built, cut at the crossing.
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
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::{
    create_taper_extrude_with_corner, OffsetCornerType, TaperExtrude,
};
use solidcore::topology::brep::p1::vertices;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1::{faceuse_edgeuses, uv_trim_curve};
use solidcore::topology::{edge, face, vertex};
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
    arc_in(k, c, r, x, Vec3::Z.cross(x), degrees)
}

/// The circle about `c` of radius `r` in the plane of `x` and `y`, from `x` towards `y`
/// over `degrees`.
fn arc_in(k: &Kernel, c: Point3, r: f64, x: Vec3, y: Vec3, degrees: f64) -> Curve {
    let frame = Axis2Placement::new(c, x, y);
    let ivl = Extent1d::from_min_max(0.0, degrees);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&ivl), None).unwrap())
}

fn taper(k: &mut Kernel, cs: &[Curve]) -> Brep {
    match draft(k, cs, 5.0) {
        TaperExtrude::Solid(b) => b,
        TaperExtrude::Refused(r) => panic!("draft refused: {r:?}"),
    }
}

/// The profile `cs` drafted `deg` degrees inward over 10 with `CurveExtension` corners.
fn draft(k: &mut Kernel, cs: &[Curve], deg: f64) -> TaperExtrude {
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_taper_extrude_with_corner(k, &refs, 10.0, deg, 3, OffsetCornerType::CurveExtension)
        .unwrap()
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

/// `int_0^1 g`, composite Simpson over 2000 panels (to rounding for the smooth integrands
/// here).
fn simpson(g: impl Fn(f64) -> f64) -> f64 {
    let n = 2000;
    let dx = 1.0 / f64::from(n);
    let mut sum = g(0.0) + g(1.0);
    for i in 1..n {
        sum += if i % 2 == 1 { 4.0 } else { 2.0 } * g(f64::from(i) * dx);
    }
    sum * dx / 3.0
}

/// The points of an 11 x 11 grid of `s`'s natural domain.
fn grid(k: &Kernel, s: &Surface) -> Vec<Point3> {
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut out = Vec::new();
    for i in 0..=10 {
        for j in 0..=10 {
            let u = dom.u_min() + (dom.u_max() - dom.u_min()) * f64::from(i) / 10.0;
            let v = dom.v_min() + (dom.v_max() - dom.v_min()) * f64::from(j) / 10.0;
            out.push(s.evaluate_point(k, Point2::new(u, v)).unwrap());
        }
    }
    out
}

/// #2497: a draft that leans off its profile's plane builds, its arc's wall the B-spline
/// ruled on its oblique cone.
///
/// The profile is a half disc R 8 about the origin O in the plane tilted 5 degrees about x
/// (e1 = x, e2 = (0, cos 5, sin 5), normal m = e1 x e2), its chord along e2, drafted 5
/// degrees over 10. `CreateTaperExtrude` snaps the computed normal to Z (|n.z| > 0.98), so
/// the height h = 10 Z leans off m. The section at the fraction f of the height lies in
/// the plane parallel to the profile's through f h, offset inward in it by f e,
/// e = 10 tan 5 cos 5 (the offset's part in the plane): the circle about f h of radius
/// 8 - f e and the chord f e along e1. The arc's wall is the oblique circular cone through
/// those circles, apex A = (8 / e) h, every point on the ruling from A to the profile's
/// circle; no cone of revolution (no `SmCone` face). The caps are the planes f = 0, f = 1,
/// the chord's wall the plane of the chords. The volume is `(h . m) int_0^1 S(f) df`, S the
/// circular segment `rho^2 acos(c / rho) - c sqrt(rho^2 - c^2)`, rho = 8 - f e, c = f e
/// (832.0008995786...); the B-rep's is 1.3e-10 over it, as the same half disc drafted
/// upright is today (2.1e-10). Before, the walls took Z for the plane's normal, so the seam
/// at 8 e2 left its walls and the draft was refused (`NotExact`).
#[test]
fn an_oblique_drafted_wall_is_ruled_on_its_oblique_cone() {
    let a = 5.0_f64.to_radians();
    let e2 = Vec3::new(0.0, a.cos(), a.sin());
    let m = Vec3::X.cross(e2);
    let o = Point3::new(0.0, 0.0, 0.0);
    let mut k = Kernel::default();
    let cs = [
        arc_in(&k, o, 8.0, -e2, Vec3::X, 180.0),
        line(o + 8.0 * e2, o - 8.0 * e2),
    ];
    let TaperExtrude::Solid(mut b) = draft(&mut k, &cs, 5.0) else {
        panic!("the oblique draft is refused");
    };
    assert_eq!(counts(&b), (4, 6, 4));
    assert!(cone_faces(&k, &b).is_empty());
    let h = Vec3::new(0.0, 0.0, 10.0);
    let e = 10.0 * tan5() * a.cos();
    let apex = o + (8.0 / e) * h;
    // each face's surface: a cap, the chord's wall or the arc's wall
    let (mut caps, mut chord, mut cone) = (0, 0, 0);
    for f in get_faces(&b).unwrap() {
        let s = face::surface(&b, f).unwrap().unwrap();
        let pts = grid(&k, s);
        // the fraction of the height, and the point less its height (in the profile's plane)
        let at = |q: Point3| {
            let t = (q - o).dot(m) / h.dot(m);
            (t, q - t * h)
        };
        let off = |g: &dyn Fn(f64, Point3) -> f64| {
            pts.iter()
                .map(|&q| {
                    let (t, p) = at(q);
                    g(t, p).abs()
                })
                .fold(0.0, f64::max)
        };
        let cap = off(&|t, _| t).min(off(&|t, _| t - 1.0));
        let plane = off(&|t, p| (p - o).dot(Vec3::X) - t * e);
        if cap < 1e-12 {
            caps += 1;
        } else if plane < 1e-12 {
            chord += 1;
        } else {
            // the distance from the ruling from the apex to the profile's circle in the
            // point's direction in the profile's plane
            let on_cone = pts
                .iter()
                .map(|&q| {
                    let (_, p) = at(q);
                    let base = o + 8.0 * (p - o).unitize().unwrap();
                    let dir = (apex - base).unitize().unwrap();
                    ((q - base) - (q - base).dot(dir) * dir).length()
                })
                .fold(0.0, f64::max);
            assert!(on_cone < 1e-12, "off the oblique cone by {on_cone:e}");
            cone += 1;
        }
    }
    assert_eq!((caps, chord, cone), (2, 1, 1));
    check(&mut k, &mut b);
    let segment = |f: f64| {
        let (rho, c) = (8.0 - f * e, f * e);
        rho * rho * (c / rho).acos() - c * (rho * rho - c * c).sqrt()
    };
    let want = h.dot(m) * simpson(segment);
    let v = volume(&k, &mut b, 1e-10);
    assert!((v - want).abs() <= 1e-9 * want, "volume {v} want {want}");
}

/// The largest distance, over 65 points of each edge, between an edge and its stored
/// pcurve on a cone face lifted onto the cone.
fn cone_pcurve_gap(k: &Kernel, b: &Brep) -> f64 {
    let mut worst: f64 = 0.0;
    for f in get_faces(b).unwrap() {
        let s = face::surface(b, f).unwrap().unwrap();
        if !matches!(s.kind(), SurfaceKind::Cone(_)) {
            continue;
        }
        let fu = face::p2::upward_faceuse(b, f).unwrap().unwrap();
        for eu in faceuse_edgeuses(b, fu, false, None).unwrap() {
            let pc = uv_trim_curve(b, eu).unwrap().0.cloned().unwrap();
            let e = b.edgeuse(eu).unwrap().edge.unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            for i in 0..=64 {
                let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 64.0;
                let uv = pc.evaluate_point(k, t).unwrap();
                let on = s.evaluate_point(k, Point2::new(uv.x, uv.y)).unwrap();
                worst = worst.max(on.distance(c.evaluate_point(k, t).unwrap()));
            }
        }
    }
    worst
}

/// A 240 degree segment of R 8 about the origin, from -90 degrees counterclockwise to 150,
/// closed by its chord, drafted `deg` with `keep_exact_pcurves`.
fn segment_240(deg: f64) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.keep_exact_pcurves = true;
    let o = Point3::new(0.0, 0.0, 0.0);
    let x = Vec3::new(0.0, -1.0, 0.0);
    let at = |deg: f64| {
        let t = deg.to_radians();
        Point3::new(8.0 * t.cos(), 8.0 * t.sin(), 0.0)
    };
    let cs = [arc(&k, o, 8.0, x, 240.0), line(at(150.0), at(-90.0))];
    let TaperExtrude::Solid(b) = draft(&mut k, &cs, deg) else {
        panic!("the {deg} degree draft is refused");
    };
    (k, b)
}

/// #2497: with `keep_exact_pcurves`, a seam that crosses a span end of its drafted cone
/// keeps its pcurve as built, cut at the crossing.
///
/// The 240 degree segment drafted `deg` degrees: the arc's wall is the cone about Z of
/// semi-angle `deg` narrowing up, apex (0, 0, 8 / tan deg), radius 8 - z tan deg. Drafted
/// 30 degrees its corners move to -7.2 and 67.2 degrees, past the ends of the cone's
/// middle span, so each seam crosses a span end: its pcurve is the rational quartic of
/// each span, joined C0 at the crossing, one interior knot, where the lifted point lies on
/// the span end (a knot of the cone's v), and on its edge to rounding. Drafted 20 degrees
/// no seam crosses (no interior knot), and the volume is the closed form
/// `10 int_0^1 S(f) df`, S the major segment `rho^2 acos(-c / rho) + c sqrt(rho^2 - c^2)`,
/// rho = 8 - f e, c = 4 - f e, e = 10 tan deg (900.37531668103...). (At 30 degrees the
/// top cap's arc trim is a projection, a cubic fit, and the volume is 5.3e-6 short.)
/// Before, the cone was padded past its face (5% of
/// the seams' reach and 1e-3), so `SmBrep::ShrinkGeometry` trimmed it to the face where
/// that was more than a thousandth of the face's size (30 degrees), removing every trim
/// curve and projecting the edges again: cubic fits 3.17e-6 off their edges.
#[test]
fn a_seam_cut_at_a_span_end_keeps_its_exact_pcurve() {
    for (deg, cuts) in [(20.0_f64, 0), (30.0, 1)] {
        let (mut k, mut b) = segment_240(deg);
        assert_eq!(counts(&b), (4, 6, 4), "{deg}");
        check(&mut k, &mut b);
        let tan = deg.to_radians().tan();
        let cone = get_faces(&b)
            .unwrap()
            .into_iter()
            .find(|&f| {
                matches!(
                    face::surface(&b, f).unwrap().unwrap().kind(),
                    SurfaceKind::Cone(_)
                )
            })
            .unwrap();
        let s = face::surface(&b, cone).unwrap().unwrap();
        let SurfaceKind::Cone(c) = s.kind() else {
            unreachable!()
        };
        assert!(
            (c.semi_angle_deg() + deg).abs() < 1e-12,
            "semi-angle {}",
            c.semi_angle_deg()
        );
        let apex = c.apex(&k).unwrap().point;
        assert!(
            apex.distance(Point3::new(0.0, 0.0, 8.0 / tan)) < 1e-12,
            "apex {apex:?}"
        );
        let off = grid(&k, s)
            .into_iter()
            .map(|q| (q.x.hypot(q.y) - (8.0 - q.z * tan)).abs())
            .fold(0.0, f64::max);
        assert!(off < 1e-12, "{deg}: off the cone by {off:e}");
        let gap = cone_pcurve_gap(&k, &b);
        assert!(gap < 1e-12, "{deg}: pcurves off their edges by {gap:e}");
        // the seams' pcurves (the quartics): cut on a span end of the cone, an interior knot
        // of its angle's direction (the other, along the height, has none)
        let inner_knots = |dir| {
            let (knots, _) = s.knots_with_nurb(&k, dir, None).unwrap();
            knots[1..knots.len() - 1].to_vec()
        };
        let span_ends = [inner_knots(SurfParamType::U), inner_knots(SurfParamType::V)];
        let fu = face::p2::upward_faceuse(&b, cone).unwrap().unwrap();
        let mut seams = 0;
        for eu in faceuse_edgeuses(&b, fu, false, None).unwrap() {
            let pc = uv_trim_curve(&b, eu).unwrap().0.cloned().unwrap();
            if pc.degree().unwrap() != 4 {
                continue;
            }
            seams += 1;
            let (knots, _) = pc.knots(&k, None).unwrap();
            let inner = &knots[1..knots.len() - 1];
            assert_eq!(inner.len(), cuts, "{deg}: knots {knots:?}");
            for &t in inner {
                let uv = pc.evaluate_point(&k, t).unwrap();
                let to_end = span_ends[0]
                    .iter()
                    .map(|ku| (ku - uv.x).abs())
                    .chain(span_ends[1].iter().map(|kv| (kv - uv.y).abs()))
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    to_end < 1e-12,
                    "{deg}: cut at {uv:?}, span ends {span_ends:?}"
                );
            }
        }
        assert_eq!(seams, 2, "{deg}");
        if cuts == 0 {
            let e = 10.0 * tan;
            let want = 10.0
                * simpson(|f| {
                    let (rho, c) = (8.0 - f * e, 4.0 - f * e);
                    rho * rho * (-c / rho).acos() + c * (rho * rho - c * c).sqrt()
                });
            let v = volume(&k, &mut b, 1e-10);
            assert!((v - want).abs() <= 1e-11 * want, "volume {v} want {want}");
        }
    }
}

/// A large arc's drafted wall keeps its seams' pcurves (tangency-labs/solidcraft#16003).
///
/// The segment of R 746.6148 about the origin between -17.4 and 17.4 degrees, closed by its
/// chord at x = d = R cos 17.4, drafted `deg` degrees over 10: the arc's wall is the cone about
/// Z of semi-angle `deg`, radius R - z tan deg; the chord's wall the plane x = d + z tan deg.
/// Each seam's pcurve on the cone is a rational quartic whose weights grow with the radius
/// (about 450 here); scaled by one factor they stay the same curve, inside the weight range a
/// B-spline takes ([1e-3, 100]). The section at the fraction f of the height is the segment
/// `rho^2 acos(c / rho) - c sqrt(rho^2 - c^2)`, rho = R - f e, c = d + f e, e = 10 tan deg,
/// so the volume is `10 int_0^1 S(f) df`. Before, the weights were left as built and each
/// draft was refused (`NotExact`) at the corner.
#[test]
fn a_large_arcs_drafted_wall_keeps_its_seams() {
    let r = 746.6148;
    let half = 17.4_f64.to_radians();
    let d = r * half.cos();
    let o = Point3::new(0.0, 0.0, 0.0);
    for deg in [15.0_f64, -15.0] {
        let mut k = Kernel::default();
        let at = |t: f64| Point3::new(r * t.cos(), r * t.sin(), 0.0);
        let from = Vec3::new(half.cos(), -half.sin(), 0.0);
        let cs = [arc(&k, o, r, from, 2.0 * 17.4), line(at(half), at(-half))];
        let TaperExtrude::Solid(mut b) = draft(&mut k, &cs, deg) else {
            panic!("the {deg} degree draft is refused");
        };
        assert_eq!(counts(&b), (4, 6, 4), "{deg}");
        check(&mut k, &mut b);
        let tan = deg.to_radians().tan();
        let cones: Vec<Surface> = get_faces(&b)
            .unwrap()
            .into_iter()
            .map(|f| face::surface(&b, f).unwrap().unwrap().clone())
            .filter(|s| matches!(s.kind(), SurfaceKind::Cone(_)))
            .collect();
        assert_eq!(cones.len(), 1, "{deg}");
        let SurfaceKind::Cone(c) = cones[0].kind() else {
            unreachable!()
        };
        assert!(
            (c.semi_angle_deg() + deg).abs() < 1e-12,
            "{deg}: semi-angle {}",
            c.semi_angle_deg()
        );
        let off = grid(&k, &cones[0])
            .into_iter()
            .map(|q| (q.x.hypot(q.y) - (r - q.z * tan)).abs())
            .fold(0.0, f64::max);
        assert!(off < 1e-9, "{deg}: off the cone by {off:e}");
        let gap = cone_pcurve_gap(&k, &b);
        assert!(gap < 1e-10, "{deg}: pcurves off their edges by {gap:e}");
        // the two seams, from the profile's corners to the far end's, each on both walls
        let mut seams = 0;
        for e in get_edges(&b).unwrap() {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let pts: Vec<Point3> = (0..=64)
                .map(|i| {
                    let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 64.0;
                    c.evaluate_point(&k, t).unwrap()
                })
                .collect();
            let rise = pts.iter().map(|q| q.z).fold(0.0, f64::max);
            if pts.iter().map(|q| q.z).fold(10.0, f64::min) > 1e-9 || rise < 10.0 - 1e-9 {
                continue;
            }
            seams += 1;
            let off = pts
                .iter()
                .map(|q| {
                    let cone = q.x.hypot(q.y) - (r - q.z * tan);
                    let plane = q.x - (d + q.z * tan);
                    cone.abs().max(plane.abs())
                })
                .fold(0.0, f64::max);
            assert!(off < 1e-9, "{deg}: a seam off its walls by {off:e}");
        }
        assert_eq!(seams, 2, "{deg}");
        let e = 10.0 * tan;
        let want = 10.0
            * simpson(|f| {
                let (rho, c) = (r - f * e, d + f * e);
                rho * rho * (c / rho).acos() - c * (rho * rho - c * c).sqrt()
            });
        // (measured 1.1e-7 and 1.9e-8 short: the volume is taken over the faces' trims, and two
        // planar faces' pcurves are fits up to 8.8e-6 off their edges; the same segment of
        // R 100 or R 8, which built before, is 3e-7 to 1.9e-6 short the same way)
        let v = volume(&k, &mut b, 1e-10);
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "{deg}: volume {v} want {want}"
        );
    }
}

/// A horn whose tip is where its line touches its arc builds (tangency-labs/solidcraft#16003).
///
/// The issue's window: the arc of radius r = 746.6148 about c = (-340.6148, 0) touches the
/// line x = 406 at t = (c + r, 0), and the app splits the region there into two horns. The
/// lower one, as the app passes it: the arc clockwise from t to s = (371.9150, -223.0132), the
/// line from s to (406, s.y) and up to t, leaning by k = (t.x - 406) / -s.y (-7e-11, the
/// tip's rounding), drafted 15 degrees inward over 10. At the tip the curves touch at 0
/// degrees; drafted, the line's wall x = 406 + k (y - s.y) - z tan 15 cuts the cone
/// R = r + z tan 15 near y = -sqrt(R^2 - (r - z tan 15)^2) = -2 sqrt(r z tan 15), so the tip's
/// seam rises from t into the horn. The section at the fraction f of the height,
/// e = 10 f tan 15, runs from y0 = s.y + e to that crossing y1, between the circle and the
/// line: `w (y1 - y0) + k ((y1 - s.y)^2 - (y0 - s.y)^2) / 2 - [G]_y0^y1`, w = 406 - e - c,
/// G(y) = (y sqrt(R^2 - y^2) + R^2 asin(y / R)) / 2, and the volume is `10 int_0^1 S(f) df`,
/// taken over f = q^2 (the crossing goes as the square root of f). Before, the line and the
/// circle crossed at the tip with a discriminant of -2.2e-11 against terms of 5e4, rounding's
/// sign, so the draft was refused (`Apart`) at (406, -223.0132).
#[test]
fn a_horn_drafted_from_its_touching_tip_builds() {
    let c = Point3::new(-340.614_824_473_321_73, 0.0, 0.0);
    let r = 746.614_824_457_444_7;
    let t = Point3::new(405.999_999_984_122_95, 0.0, 0.0);
    let s = Point3::new(371.915_036_667_901_5, -223.013_212_796_215_44, 0.0);
    let corner = Point3::new(406.0, s.y, 0.0);
    let mut k = Kernel::default();
    // as the app builds the arc: from x about -Z over its sweep in degrees
    let sweep = 0.303_329_283_634_688_53 * 180.0 / PI;
    let cs = [
        arc_in(&k, c, r, Vec3::X, Vec3::new(0.0, -1.0, 0.0), sweep),
        line(s, corner),
        line(corner, t),
    ];
    let mut b = match draft(&mut k, &cs, 15.0) {
        TaperExtrude::Solid(b) => b,
        TaperExtrude::Refused(r) => panic!("the lower horn's draft is refused: {r:?}"),
    };
    assert_eq!(counts(&b), (5, 9, 6));
    check(&mut k, &mut b);
    let cones = get_faces(&b)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            matches!(
                face::surface(&b, f).unwrap().unwrap().kind(),
                SurfaceKind::Cone(_)
            )
        })
        .count();
    assert_eq!(cones, 1);
    // the tip keeps its vertex, where the bottom arc, the bottom line and the seam meet
    assert_eq!(edges_at(&b, t), Some(3));
    let tan = 15.0_f64.to_radians().tan();
    let lean = (t.x - 406.0) / -s.y;
    let area = |f: f64| {
        let e = 10.0 * f * tan;
        let big = r + e;
        let w = 406.0 - e - c.x;
        let y0 = s.y + e;
        // the crossing with the upright line, then with the leaning one
        let y = -(big * big - w * w).max(0.0).sqrt();
        let x1 = w + lean * (y - s.y);
        let y1 = -(big * big - x1 * x1).max(0.0).sqrt();
        let g = |y: f64| 0.5 * (y * (big * big - y * y).sqrt() + big * big * (y / big).asin());
        let strip = w * (y1 - y0) + 0.5 * lean * ((y1 - s.y).powi(2) - (y0 - s.y).powi(2));
        strip - (g(y1) - g(y0))
    };
    let want = 10.0 * simpson(|q| 2.0 * q * area(q * q));
    let v = volume(&k, &mut b, 1e-10);
    assert!((v - want).abs() <= 1e-11 * want, "volume {v} want {want}");
}
