//! `query::tangent_edge` (#3141): whether an edge's faces meet tangentially all along it,
//! against closed forms: the angle between their outward normals is bracketed by the verdicts
//! just below it (not tangent, by a witness) and just above it (no witness).
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
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::CPoint;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::{edge_closest_point, get_edges};
use solidcore::query::edge::edge_faces;
use solidcore::query::edge_turn::{convex_at, turn_at};
use solidcore::query::tangent_edge::{tangent_all_along, Tangency};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::edgeuse::p1::{self as eu1, set_uv_trim_curve, UvTrimCurveArg};
use solidcore::topology::{edge, face, EdgeId, EdgeuseId, FaceId, VertexId};
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

/// Edge `e`'s faces part by `want` (radians) somewhere along it, to `tol`: a witness proves
/// them more than `want - tol` apart, and none proves them more than `want + tol` apart.
fn parts_by(k: &Kernel, b: &mut Brep, e: EdgeId, want: f64, tol: f64) {
    let below = tangent_all_along(k, b, e, want - tol).unwrap();
    assert_eq!(below, Some(Tangency::NotTangent), "{e:?} below {want}");
    let above = tangent_all_along(k, b, e, want + tol).unwrap();
    assert_ne!(above, Some(Tangency::NotTangent), "{e:?} above {want}");
}

#[test]
fn every_edge_of_a_box_is_square() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 6.0, 4.0).unwrap();
    let edges = get_edges(&b).unwrap();
    assert_eq!(edges.len(), 12);
    for e in edges {
        parts_by(&k, &mut b, e, FRAC_PI_2, 1e-12);
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
    parts_by(&k, &mut b, ridge, 2.0 * PI / 3.0, 1e-12);
    let base = pick(&k, &b, Point3::new(5.0, half, 0.0));
    parts_by(&k, &mut b, base, 2.0 * PI / 3.0, 1e-12);
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
        parts_by(&k, &mut b, e, FRAC_PI_2, 1e-9);
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
        assert_eq!(
            tangent_all_along(&k, &mut b, e, SMOOTH).unwrap(),
            Some(Tangency::Tangent)
        );
        // the round's faces are tangent at each end too: the side's enclosure holds 0, so
        // the edge is neither convex nor concave there
        for v in [
            edge::start_vertex(&b, e).unwrap(),
            edge::end_vertex(&b, e).unwrap(),
        ] {
            let v = v.expect("an end");
            assert_eq!(convex_at(&k, &mut b, e, v, SMOOTH).unwrap(), None, "{at:?}");
        }
    }
    let back = pick(&k, &b, Point3::new(5.0, 6.0, 4.0));
    parts_by(&k, &mut b, back, FRAC_PI_2, 1e-12);
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
            assert_eq!(
                convex_at(&k, &mut b, e, v, SMOOTH).unwrap(),
                Some(convex),
                "{at:?}"
            );
        }
    }
}

/// A prism 5 high on (0, 0), (10, 0), (20, 10 tan a), (20, 12), (0, 12): its vertical edge at
/// (10, 0) is between two walls whose normals are `a` apart (0.3 degrees), planes, so the bound
/// is the angle itself (one point, `L` 1).
fn kinked(k: &mut Kernel, a: f64) -> Brep {
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let pts = [
        p(0.0, 0.0),
        p(10.0, 0.0),
        p(20.0, 10.0 * a.tan()),
        p(20.0, 12.0),
        p(0.0, 12.0),
    ];
    let curves: Vec<Curve> = (0..5)
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % 5]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 5.0, true).unwrap()
}

/// Walls 0.3 degrees apart: tangent at 0.5 (by the bound, not exact parallelism), not tangent
/// at 0.2 (by a witness), undecided at the angle itself (neither the bound nor the witness
/// clears it).
#[test]
fn walls_three_tenths_of_a_degree_apart() {
    let mut k = Kernel::default();
    let a = 0.3_f64.to_radians();
    let mut b = kinked(&mut k, a);
    let e = pick(&k, &b, Point3::new(10.0, 0.0, 2.5));
    let at = |b: &mut Brep, deg: f64| tangent_all_along(&k, b, e, deg.to_radians()).unwrap();
    assert_eq!(at(&mut b, 0.5), Some(Tangency::Tangent));
    assert_eq!(at(&mut b, 0.2), Some(Tangency::NotTangent));
    // undecided, and named where: a piece of the edge (the walls are planes, so the edge is
    // one piece, its middle)
    let Some(Tangency::Undecided { at: place, .. }) = at(&mut b, 0.3) else {
        panic!("{:?}", at(&mut b, 0.3));
    };
    let t = edge::interval(&b, e).unwrap();
    assert!(t.min() < place && place < t.max(), "{place} not on {t:?}");
}

/// A procedural round whose definition is enclosed (the ball's circle of one radius between
/// NURBS faces, `gn_def_face_of`): `wr698_gn_round_is_its_definition`'s `circle_root`, a
/// 60 x 40 x 10 plate with a pipe of radius 10 lying along it 4 degrees off x, sunk into
/// it, the root edge rounded r 2 by the ball's exact circle. The round's edges along the
/// root.
fn procedural_round(k: &mut Kernel) -> (Brep, Vec<EdgeId>) {
    use solidcore::boolean::api_brep::boolean_union;
    k.fixes.fillet_round_as_definition = true;
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let mut pipe = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
    let phi = 4.0_f64.to_radians();
    let place = Axis2Placement::new(
        Point3::new(0.0, 20.0, 16.0),
        Vec3::new(-phi.sin(), 0.0, -phi.cos()),
        Vec3::new(0.0, 1.0, 0.0),
    );
    solidcore::topology::brep::p5::transform(k, &mut pipe, &place, None, false).unwrap();
    let mut b = boolean_union(k, plate, pipe).unwrap();
    // the root edge nearest (30, 29.2, 10), as `circle_root` takes it
    let q = Point3::new(30.0, 29.2, 10.0);
    let root = get_edges(&b)
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            edge_closest_point(k, &b, e, q)
                .ok()
                .map(|c| (e, c.closest.distance))
        })
        .filter(|&(_, d)| d <= 1.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("the root edge")
        .0;
    fillet_edges(k, &mut b, &[root], 2.0, 1, 1, 1.0).unwrap();
    let rounds: Vec<FaceId> = b
        .faces()
        .ids()
        .filter(|&f| {
            face::p2::surface(&b, f)
                .unwrap()
                .is_some_and(|s| s.procedural_blend().is_some())
        })
        .collect();
    assert!(!rounds.is_empty(), "the round is a definition");
    // the pipe lies along x (its axis 4 degrees off it), so the root runs along x and the
    // round ends in cross-sections at the plate's sides: the edges there (spanning little in
    // x) meet those faces square, not tangentially; the rest run along the root, into the
    // plate's top and the pipe
    let mut edges: Vec<EdgeId> = Vec::new();
    for &f in &rounds {
        for e in face::p4::edges(&b, f, None, false, None).unwrap() {
            let t = edge::interval(&b, e).unwrap();
            let curve = edge::curve(&b, e).unwrap().expect("an edge curve");
            let x = |t: f64| curve.evaluate_point(k, t).unwrap().x;
            if (x(t.max()) - x(t.min())).abs() > 1.0 && !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    assert!(edges.len() >= 2, "{edges:?}");
    (b, edges)
}

/// The procedural round's edges along the root, decided on its net with its definition's
/// margin: Tangent on each. Where the pipe's foot crosses its net's C1 quarter knot (u =
/// 15.708, multiplicity 2) the definition's feet box is taken cell by cell
/// (tangency-labs/solidcraft#14284's C1-knot follow-up, and #15183 where the foot sits on the
/// knot just outside the guesses' box); before them both edges were Undecided (pi, pi) at
/// `s` = 87.04 (#15005). The module's unit test `a_procedural_round_s_edges_carry_its_margin`
/// checks every piece.
#[test]
fn a_procedural_round_is_smooth() {
    let mut k = Kernel::default();
    let (mut b, edges) = procedural_round(&mut k);
    // every edge measured before anything is asserted, so a failure names them all
    let seen: Vec<_> = edges
        .into_iter()
        .map(|e| (e, tangent_all_along(&k, &mut b, e, SMOOTH).unwrap()))
        .collect();
    eprintln!("procedural round edges: {seen:?}");
    assert!(
        seen.len() == 2
            && seen
                .iter()
                .all(|(_, verdict)| *verdict == Some(Tangency::Tangent)),
        "{seen:?}"
    );
}

/// The edge use of `e` on a face that is not a plane, and that face.
fn curved_use(b: &Brep, e: EdgeId) -> (EdgeuseId, FaceId) {
    edge::edgeuses(b, e, None, None)
        .unwrap()
        .into_iter()
        .find_map(|eu| {
            let f = eu1::face(b, eu).unwrap()?;
            let s = face::p2::surface(b, f).unwrap()?;
            (!matches!(s.kind(), SurfaceKind::Plane(_))).then_some((eu, f))
        })
        .expect("an edge use on a curved face")
}

/// The pcurve of `eu` moved by `(du, dv)` in its face's parameters, put back on `eu`.
fn shift_pcurve(k: &Kernel, b: &mut Brep, eu: EdgeuseId, du: f64, dv: f64) -> Vec<Point3> {
    let mut pc = eu1::uv_trim_curve(b, eu)
        .unwrap()
        .0
        .expect("a pcurve")
        .clone();
    let by = Axis2Placement::new(Point3::new(du, dv, 0.0), Vec3::X, Vec3::Y);
    pc.transform(k, &by, None).unwrap();
    let points = pc
        .b_spline_base()
        .expect("a NURBS pcurve")
        .control_polygon()
        .unwrap()
        .0;
    set_uv_trim_curve(b, eu, UvTrimCurveArg::New(Box::new(pc)), 0.0, true, None).unwrap();
    points
}

/// A cylinder's side face is periodic: its cap edge's pcurve stored a period off (shifted by
/// the face's whole domain along its periodic parameter) lies wholly past the net's domain.
/// The face's evaluator clamps there (it does not wrap), so the face's point is a whole
/// period's run from the pcurve's: not on its face, and the decision refuses it by name.
#[test]
fn a_pcurve_a_period_off_its_face_is_refused() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 3.0, 7.0).unwrap();
    let e = pick(&k, &b, Point3::new(0.0, 3.0, 0.0));
    let (eu, f) = curved_use(&b, e);
    let side = face::p2::surface(&b, f).unwrap().unwrap().clone();
    let dom = side.natural_uv_domain_with_nurb(&k).unwrap();
    let in_u = side.is_periodic(&k, &dom, SurfParamType::U).unwrap();
    let in_v = side.is_periodic(&k, &dom, SurfParamType::V).unwrap();
    assert!(in_u || in_v, "a cylinder's side is periodic");
    let (du, dv) = if in_u {
        (dom.x_length(), 0.0)
    } else {
        (0.0, dom.y_length())
    };
    assert_eq!(
        tangent_all_along(&k, &mut b, e, SMOOTH).unwrap(),
        Some(Tangency::NotTangent),
        "before the shift"
    );
    shift_pcurve(&k, &mut b, eu, du, dv);
    let err = tangent_all_along(&k, &mut b, e, SMOOTH).unwrap_err();
    assert!(
        format!("{err:?}").contains("leaving its face's net"),
        "{err:?}"
    );
}

/// The prism of `kinked` with `a` = 10 degrees: its walls A on y = 0 and B turned `a` from it
/// meet at the vertical edge through (10, 0). Wall A's surface is replaced by one degree-1
/// B-spline over both walls' planes, two spans in u with the knot 1/2 at that edge (u: (0, 0),
/// (10, 0), (20, 10 tan a); v: z from 0 to 5): A's region is its first span, as before. A's
/// pcurve on the edge is put on the knot line, `1e-15` past it and `1e-15` before it. Past it
/// the net is the second span's plane, B's, so the faces meet smoothly; before it, the first
/// span's, 10 degrees off; on it, the edge's box meets the line, so the piece is bounded over
/// both cells and is not proved smooth. Past the net's end the face's evaluator clamps: a
/// pcurve `1e-12` past it is taken, one 0.5 past it refused. The sweep cannot make such a
/// face (it splits a profile at a kink into faces), hence the surface put in by hand.
#[test]
fn a_pcurve_1e_15_off_a_knot_line_is_on_its_own_side() {
    let mut k = Kernel::default();
    let a = 10.0_f64.to_radians();
    let verdict = |k: &mut Kernel, at: f64| {
        let mut b = kinked(k, a);
        let e = pick(k, &b, Point3::new(10.0, 0.0, 2.5));
        // wall A: the face of the edge that also bounds wall A's bottom edge through (5, 0, 0)
        let bottom = pick(k, &b, Point3::new(5.0, 0.0, 0.0));
        let below = edge_faces(&b, bottom).unwrap();
        let f = edge_faces(&b, e)
            .unwrap()
            .into_iter()
            .find(|f| below.contains(f))
            .expect("wall A");
        let eu = edge::edgeuses(&b, e, None, None)
            .unwrap()
            .into_iter()
            .find(|&eu| eu1::face(&b, eu).unwrap() == Some(f))
            .expect("wall A's use");
        // the net's normal Su x Sv keeps the plane's sense: x along u, so z along v where the
        // plane's normal is -y, and down where it is +y
        let plane = face::p2::surface(&b, f).unwrap().unwrap().clone();
        let n = plane
            .evaluate_normal(k, Point2::new(0.0, 0.0), Side::Left, Side::Left)
            .unwrap();
        assert!(
            n.x.abs() < 1e-12 && n.z.abs() < 1e-12,
            "wall A's normal {n:?}"
        );
        let (z0, z1) = if n.y < 0.0 { (0.0, 5.0) } else { (5.0, 0.0) };
        let row = |x: f64, y: f64| vec![CPoint::new(x, y, z0, 1.0), CPoint::new(x, y, z1, 1.0)];
        let net = [row(0.0, 0.0), row(10.0, 0.0), row(20.0, 10.0 * a.tan())];
        let (u, v) = ([0.0, 0.0, 0.5, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        let s = BSplineSurface::from_data(2, 1, 1, 1, 4, 3, &net, &u, &v).unwrap();
        b.face_mut(f).unwrap().surface = Some(Surface::from(s));
        let t = edge::interval(&b, e).unwrap();
        let cpw = [
            CPoint::new(at, 0.0, 0.0, 1.0),
            CPoint::new(at, 1.0, 0.0, 1.0),
        ];
        let knots = [t.min(), t.min(), t.max(), t.max()];
        let pc = Curve::from(BSplineCurve::from_data(3, 1, 3, 1, &cpw, &knots).unwrap());
        set_uv_trim_curve(
            &mut b,
            eu,
            UvTrimCurveArg::New(Box::new(pc)),
            0.0,
            true,
            None,
        )
        .unwrap();
        tangent_all_along(k, &mut b, e, SMOOTH)
    };
    assert_eq!(
        verdict(&mut k, 0.5 + 1e-15).unwrap(),
        Some(Tangency::Tangent)
    );
    assert_eq!(
        verdict(&mut k, 0.5 - 1e-15).unwrap(),
        Some(Tangency::NotTangent)
    );
    assert_ne!(verdict(&mut k, 0.5).unwrap(), Some(Tangency::Tangent));
    // past the net's domain [0, 1] the face's evaluator clamps: 1e-12 past its end the
    // clamped point is within the edge's tolerance of the net's, so the pcurve is taken (the
    // second span's plane, smooth); 0.5 past it, it moves 10 or more: refused, named
    assert_eq!(
        verdict(&mut k, 1.0 + 1e-12).unwrap(),
        Some(Tangency::Tangent)
    );
    let err = verdict(&mut k, 1.5).unwrap_err();
    assert!(
        format!("{err:?}").contains("leaving its face's net"),
        "{err:?}"
    );
}
