//! WR-697 (smlib#79 step 4c, the fork's tangency-labs/smlib@79d4763, a6243fa, f8e6709,
//! 4241a1b): three constant rounds of one radius r and one convexity at a vertex of three
//! planes are the blend corner's Ball (`Kernel::fixes.fillet_blend_corner`). The ball of
//! radius r touching the three planes is centred at C, r off each; each round ends on its
//! section through C, a great circle of that ball, and the corner's face is the ball's
//! triangle between the three contacts, one rational patch exactly on the sphere, G1 to
//! every round.
//!
//! The cases are the fork's capi `ball` cases (5e8c9c1 `capi/tests/ops_kernel.cpp`,
//! commits c3f8c80, ce9efb4, 9c58248, f8059d4), with their closed-form volumes: each round
//! takes (convex) or adds (concave) its spandrel along its edge outside the cube of side r
//! at the corner, and the corner the cube less the ball's octant, r^3 (1 - pi/6).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, EdgeId, FaceId, VertexId};
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner`.
fn all_fixes(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> EdgeId {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.unwrap().0
}

/// Rounds the edges at `at` at radius `r` together.
fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], r: f64) -> Result<(), (i32, String)> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![r; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
            .unwrap();
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            return Err((info.code, info.comment.clone()));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

/// Faces, edges and vertices of `b`.
fn counts(b: &Brep) -> (usize, usize, usize) {
    let edges = get_edges(b).unwrap();
    let mut vertices: Vec<VertexId> = Vec::new();
    for &e in &edges {
        let v0 = edge::vertex(b, e).unwrap();
        let v1 = edge::other_vertex(b, e, v0).unwrap();
        for v in [v0, v1].into_iter().flatten() {
            if !vertices.contains(&v) {
                vertices.push(v);
            }
        }
    }
    (get_faces(b).unwrap().len(), edges.len(), vertices.len())
}

/// The spandrel of a square edge rounded r: r^2 (1 - pi/4).
fn spandrel(r: f64) -> f64 {
    r * r * (1.0 - PI / 4.0)
}

/// The faces of `b` whose three vertices are `v` (within 1e-9).
fn corner_faces(b: &Brep, v: &[Point3; 3]) -> Vec<FaceId> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let mut pts: Vec<Point3> = Vec::new();
        for e in solidcore::topology::topology_traverser::face_edges(b, f).unwrap() {
            for vv in [edge::vertex(b, e).unwrap()]
                .into_iter()
                .chain([edge::other_vertex(b, e, edge::vertex(b, e).unwrap()).unwrap()])
                .flatten()
            {
                let p = b.vertex(vv).unwrap().point;
                if !pts.iter().any(|q| q.distance(p) <= 1e-9) {
                    pts.push(p);
                }
            }
        }
        if pts.len() == 3 && v.iter().all(|w| pts.iter().any(|p| p.distance(*w) <= 1e-9)) {
            out.push(f);
        }
    }
    out
}

/// The corner's face of `b`: one face with the three vertices `v`, a rational B-spline
/// (never the analytic sphere) on the sphere about `c` of radius `rad`, its normal out
/// of the body (away from `c` convex, towards it concave), its edges on the sphere
/// within `edge_tol`. Returns the worst surface, edge and normal deviations.
fn ball_face(
    k: &Kernel,
    b: &Brep,
    c: Point3,
    rad: f64,
    v: &[Point3; 3],
    convex: bool,
) -> (f64, f64, f64) {
    let faces = corner_faces(b, v);
    assert_eq!(faces.len(), 1, "one corner face");
    let f = faces[0];
    let s = face2::surface(b, f).unwrap().unwrap();
    let SurfaceKind::BSpline(net) = s.kind() else {
        panic!("the Ball's face is a B-spline patch, got {:?}", s.kind());
    };
    assert!(net.is_rational());
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let (mut surf_off, mut normal_off, mut edge_off) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..=16 {
        for j in 0..=16 {
            let uv = dom.evaluate(f64::from(i) / 16.0, f64::from(j) / 16.0);
            let p = s.evaluate_point(k, uv).unwrap();
            surf_off = surf_off.max(((p - c).length() - rad).abs());
            if i == 0 || i == 16 || j == 0 || j == 16 {
                continue; // the pole's side has no one normal
            }
            let n = s
                .evaluate_normal(
                    k,
                    uv,
                    solidcore::nurbs::knots::Side::Right,
                    solidcore::nurbs::knots::Side::Right,
                )
                .unwrap()
                .unitize()
                .unwrap();
            let out = (p - c).unitize().unwrap();
            let want = if convex { out } else { -out };
            normal_off = normal_off.max((n - want).length());
        }
    }
    for e in solidcore::topology::topology_traverser::face_edges(b, f).unwrap() {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        for t in 0..=32 {
            let p = curve
                .evaluate_point(k, ivl.min() + (ivl.max() - ivl.min()) * f64::from(t) / 32.0)
                .unwrap();
            edge_off = edge_off.max(((p - c).length() - rad).abs());
        }
    }
    (surf_off, edge_off, normal_off)
}

const BOX_EDGES: [[f64; 3]; 3] = [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]];

fn box_volume(rad: f64) -> f64 {
    24000.0 - spandrel(rad) * (90.0 - 3.0 * rad) - rad * rad * rad * (1.0 - PI / 6.0)
}

/// capi `ball box_corner`: a 40 x 30 x 20 box, the three edges at its corner (40, 30, 20)
/// rounded r 3: the ball about (37, 27, 17).
#[test]
fn box_corner_is_the_balls_octant() {
    let mut k = all_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    round(&mut k, &mut b, &BOX_EDGES, 3.0).unwrap();
    let v = volume(&k, &mut b);
    let want = box_volume(3.0);
    let n = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let c = Point3::new(37.0, 27.0, 17.0);
    let vs = [
        Point3::new(40.0, 27.0, 17.0),
        Point3::new(37.0, 30.0, 17.0),
        Point3::new(37.0, 27.0, 20.0),
    ];
    let (s, e, nrm) = ball_face(&k, &b, c, 3.0, &vs, true);
    eprintln!(
        "volume {v:.12} want {want:.12}; counts {n:?}; off {s:.3e} {e:.3e} {nrm:.3e}; {lines:?}"
    );
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 21, 13));
    assert!(s <= 1e-12 && e <= 1e-12 && nrm <= 1e-9);
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}

/// Without the flag the corner is the pinned 3x3 corner, whose analytic corner surface is
/// the `SmSphere` (`MakeAnalyticCornerSurface`): the same octant, but not the fork's
/// rational patch (9c58248: "never the `SmSphere`").
#[test]
fn without_the_flag_the_box_corner_is_the_pinned_sphere() {
    let mut k = all_fixes(false);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    round(&mut k, &mut b, &BOX_EDGES, 3.0).unwrap();
    let vs = [
        Point3::new(40.0, 27.0, 17.0),
        Point3::new(37.0, 30.0, 17.0),
        Point3::new(37.0, 27.0, 20.0),
    ];
    let faces = corner_faces(&b, &vs);
    assert_eq!(faces.len(), 1);
    let s = face2::surface(&b, faces[0]).unwrap().unwrap();
    assert!(matches!(s.kind(), SurfaceKind::Sphere(_)), "{:?}", s.kind());
}

/// capi `ball pocket_corner`: a 40 x 30 x 20 block with a 20 x 10 x 10 pocket, the three
/// concave edges at the pocket's floor corner (10, 10, 10) rounded r 2: the ball about
/// (12, 12, 12), added material.
#[test]
fn pocket_corner_is_the_balls_octant() {
    let mut k = all_fixes(true);
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let cut = create_box(&mut k, Point3::new(10.0, 10.0, 10.0), 20.0, 10.0, 11.0).unwrap();
    let mut b = boolean_difference(&mut k, block, cut).unwrap();
    let rad = 2.0;
    round(
        &mut k,
        &mut b,
        &[[20.0, 10.0, 10.0], [10.0, 15.0, 10.0], [10.0, 10.0, 15.0]],
        rad,
    )
    .unwrap();
    let v = volume(&k, &mut b);
    let want = 22000.0 + spandrel(rad) * (40.0 - 3.0 * rad) + rad * rad * rad * (1.0 - PI / 6.0);
    let n = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let c = Point3::new(12.0, 12.0, 12.0);
    let vs = [
        Point3::new(10.0, 12.0, 12.0),
        Point3::new(12.0, 10.0, 12.0),
        Point3::new(12.0, 12.0, 10.0),
    ];
    let (s, e, nrm) = ball_face(&k, &b, c, rad, &vs, false);
    eprintln!(
        "volume {v:.12} want {want:.12}; counts {n:?}; off {s:.3e} {e:.3e} {nrm:.3e}; {lines:?}"
    );
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (15, 33, 21));
    assert!(s <= 1e-12 && e <= 1e-12 && nrm <= 1e-9);
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}

fn unit(v: Vec3) -> Vec3 {
    v / v.length()
}

/// The prism swept `height` along +z from the closed polygon `pts` (in z = 0).
fn prism(k: &mut Kernel, pts: &[Point3], dir: Vec3, height: f64) -> Brep {
    let sides: Vec<Curve> = (0..pts.len())
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % pts.len()]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, dir, height, true).unwrap()
}

/// The box 400 wide on the far side (along `n`) of the plane through `at` with normal `n`.
fn half_space(k: &mut Kernel, at: Vec3, n: Vec3) -> Brep {
    let u = unit(n.cross(if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }));
    let v = n.cross(u);
    let h = 200.0;
    let c = [
        at + (u + v) * h,
        at + (v - u) * h,
        at - (u + v) * h,
        at + (u - v) * h,
    ];
    prism(k, &c, n, 400.0)
}

/// capi `ball prism_corner`: a kite prism 10 tall, its corner V = (0, 0) of 120 degrees
/// between sides square at their far ends to the sides on to (20, 0); the three edges at
/// the top of V rounded r 2. The ball about (2 r / sqrt 3, 0, 10 - r). Removed: the
/// spandrel along each top edge's 10 - r / sqrt 3, the 120 degree spandrel
/// r^2 (1/sqrt 3 - pi/6) along the upright's 10 - r, and at the ball's centre the region
/// between the three planes and the planes through it square to them, r^3 cot 60, less
/// the ball's sector there, r^3 (pi/3) / 3.
#[test]
fn prism_corner_is_on_the_ball() {
    let mut k = all_fixes(true);
    let s3 = 3.0f64.sqrt();
    let rad = 2.0;
    let kite = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(5.0, -5.0 * s3, 0.0),
        Point3::new(20.0, 0.0, 0.0),
        Point3::new(5.0, 5.0 * s3, 0.0),
    ];
    let mut b = prism(&mut k, &kite, Vec3::Z, 10.0);
    let v0 = volume(&k, &mut b);
    round(
        &mut k,
        &mut b,
        &[
            [2.5, 2.5 * s3, 10.0],
            [2.5, -2.5 * s3, 10.0],
            [0.0, 0.0, 5.0],
        ],
        rad,
    )
    .unwrap();
    let v = volume(&k, &mut b);
    let want = 1000.0 * s3
        - (2.0 * spandrel(rad) * (10.0 - rad / s3)
            + rad * rad * (1.0 / s3 - PI / 6.0) * (10.0 - rad)
            + rad * rad * rad * (1.0 / s3 - PI / 9.0));
    let n = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let c = Point3::new(2.0 * rad / s3, 0.0, 10.0 - rad);
    let vs = [
        Point3::new(2.0 * rad / s3, 0.0, 10.0),
        Point3::new(rad / (2.0 * s3), rad / 2.0, 10.0 - rad),
        Point3::new(rad / (2.0 * s3), -rad / 2.0, 10.0 - rad),
    ];
    let (s, e, nrm) = ball_face(&k, &b, c, rad, &vs, true);
    eprintln!("prism {v0:.12}: volume {v:.12} want {want:.12}; counts {n:?}; off {s:.3e} {e:.3e} {nrm:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 21, 13));
    assert!(s <= 1e-12 && e <= 1e-12 && nrm <= 1e-9);
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}

/// capi `ball oblique_corner`: a corner of three planes none square to another, through
/// the origin with outward normals along (1, 0.2, 0), (0, 1, 0.2), (0.2, 0, 1), its edges
/// 20 long and cut square at their far ends; the three edges at the origin rounded r 2.
/// The ball about C (`n_i . C = -r`). Removed: each edge's spandrel of its dihedral angle
/// along its 20 less the distance to C's foot on it, and at C the hexahedron between the
/// planes and the end sections' planes through C less the ball's sector there.
#[test]
fn oblique_corner_is_on_the_ball() {
    let mut k = all_fixes(true);
    let rad = 2.0;
    let len = 20.0;
    let n = [
        unit(Vec3::new(1.0, 0.2, 0.0)),
        unit(Vec3::new(0.0, 1.0, 0.2)),
        unit(Vec3::new(0.2, 0.0, 1.0)),
    ];
    let mut e = [Vec3::ZERO; 3];
    for i in 0..3 {
        e[i] = unit(n[i].cross(n[(i + 1) % 3]));
        if e[i].dot(n[(i + 2) % 3]) > 0.0 {
            e[i] = -e[i];
        }
    }
    let det = n[0].dot(n[1].cross(n[2]));
    let c = (n[1].cross(n[2]) + n[2].cross(n[0]) + n[0].cross(n[1])) * (-rad / det);
    let mut p = [Vec3::ZERO; 3];
    let mut foot = [Vec3::ZERO; 3];
    let mut removed = 0.0;
    for i in 0..3 {
        p[i] = c + n[i] * rad;
        foot[i] = e[i] * c.dot(e[i]);
        let a = (-n[i].dot(n[(i + 1) % 3])).acos();
        removed += rad * rad * (1.0 / (a / 2.0).tan() - (PI - a) / 2.0) * (len - c.dot(e[i]));
    }
    let mut hexa = 0.0;
    for i in 0..3 {
        let (fa, fb) = (foot[(i + 2) % 3], foot[i]);
        hexa += (-c).dot((fa - c).cross(p[i] - c)).abs() / 6.0;
        hexa += (-c).dot((p[i] - c).cross(fb - c)).abs() / 6.0;
    }
    let omega = 2.0
        * (n[0].dot(n[1].cross(n[2])).abs())
            .atan2(1.0 + n[0].dot(n[1]) + n[1].dot(n[2]) + n[2].dot(n[0]));
    removed += hexa - rad * rad * rad * omega / 3.0;

    let mut body = create_box(&mut k, Point3::new(-40.0, -40.0, -40.0), 80.0, 80.0, 80.0).unwrap();
    for i in 0..3 {
        let hs = half_space(&mut k, Vec3::ZERO, n[i]);
        body = boolean_difference(&mut k, body, hs).unwrap();
        let hs = half_space(&mut k, e[i] * len, e[i]);
        body = boolean_difference(&mut k, body, hs).unwrap();
    }
    assert_eq!(counts(&body), (6, 12, 8));
    let v0 = volume(&k, &mut body);
    let want = v0 - removed;
    let points: Vec<[f64; 3]> = e
        .iter()
        .map(|ei| [ei.x * len / 2.0, ei.y * len / 2.0, ei.z * len / 2.0])
        .collect();
    let mut b = body;
    round(&mut k, &mut b, &points, rad).unwrap();
    let v = volume(&k, &mut b);
    let cnt = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let (s, eo, nrm) = ball_face(&k, &b, c, rad, &p, true);
    eprintln!("oblique: volume {v:.12} want {want:.12}; counts {cnt:?}; off {s:.3e} {eo:.3e} {nrm:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(cnt, (10, 21, 13));
    // the fork holds the edges to 1e-13 of the part's size (f8059d4): the diagonal of its
    // box, here its vertices' (its faces are planes and rounds between them)
    let (mut lo, mut hi) = (
        Vec3::new(BIG_DOUBLE, BIG_DOUBLE, BIG_DOUBLE),
        -Vec3::new(BIG_DOUBLE, BIG_DOUBLE, BIG_DOUBLE),
    );
    for e in get_edges(&b).unwrap() {
        for v in [edge::vertex(&b, e).unwrap()].into_iter().flatten() {
            let q = b.vertex(v).unwrap().point;
            lo = Vec3::new(lo.x.min(q.x), lo.y.min(q.y), lo.z.min(q.z));
            hi = Vec3::new(hi.x.max(q.x), hi.y.max(q.y), hi.z.max(q.z));
        }
    }
    let size = (hi - lo).length();
    eprintln!("oblique size {size}");
    assert!(
        s <= 1e-12 && eo <= 1e-13 * size && nrm <= 1e-9,
        "{s} {eo} {nrm}"
    );
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}

/// The fork's `AddSetBackVertex` (79d4763): a vertex the caller sets back is no Ball;
/// the box corner is then the 3x3 corner (its analytic sphere).
#[test]
fn a_set_back_vertex_is_no_ball() {
    let mut k = all_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let edges: Vec<EdgeId> = BOX_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
    let target = Point3::new(40.0, 30.0, 20.0);
    let mut vertex = None;
    for &e in &edges {
        for v in [edge::vertex(&b, e).unwrap()]
            .into_iter()
            .chain([edge::other_vertex(&b, e, edge::vertex(&b, e).unwrap()).unwrap()])
            .flatten()
        {
            if b.vertex(v).unwrap().point.distance(target) <= 1e-9 {
                vertex = Some(v);
            }
        }
    }
    let vertex = vertex.unwrap();
    let xs = vec![FilletSurfaceGeneratorType::Circular; 3];
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(&mut k, &edges, &[3.0; 3], &xs, 1, 1.0)
            .unwrap();
        s.add_set_back_vertex(vertex);
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let vs = [
        Point3::new(40.0, 27.0, 17.0),
        Point3::new(37.0, 30.0, 17.0),
        Point3::new(37.0, 27.0, 20.0),
    ];
    let faces = corner_faces(&b, &vs);
    assert_eq!(faces.len(), 1);
    let s = face2::surface(&b, faces[0]).unwrap().unwrap();
    assert!(matches!(s.kind(), SurfaceKind::Sphere(_)), "{:?}", s.kind());
}
