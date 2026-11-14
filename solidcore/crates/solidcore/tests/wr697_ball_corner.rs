//! WR-697 (smlib#79 step 4c, the fork's tangency-labs/smlib@79d4763, a6243fa, f8e6709,
//! 4241a1b): three constant rounds of one radius r and one convexity at a vertex of three
//! planes are the blend corner's Ball. The ball of
//! radius r touching the three planes is centred at C, r off each; each round ends on its
//! section through C, a great circle of that ball, and the corner's face is the ball's
//! triangle between the three contacts, one rational patch exactly on the sphere, G1 to
//! every round.
//!
//! The cases are the fork's capi `ball` cases (5e8c9c1 `capi/tests/ops_kernel.cpp`,
//! commits c3f8c80, ce9efb4, 9c58248, f8059d4), with their closed-form volumes: each round
//! takes (convex) or adds (concave) its spandrel along its edge outside the cube of side r
//! at the corner, and the corner the cube less the ball's octant, r^3 (1 - pi/6).
//!
//! WR-716 (#2345, smlib#89's brief): chamfers meeting at a vertex where every edge is
//! chamfered are the blend corner's chamfer event (`Kernel::fixes.fillet_chamfer_corner`),
//! closed by the chamfer planes themselves: the Mitre (the default) or the Facet
//! (`FilletSession::set_chamfer_corner`). Those cases are below the Ball's, each with its
//! closed form worked out in its comment.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::blend_corner::chamfer::ChamferWidth;
use solidcore::blend::blend_corner::BlendChamferCorner;
use solidcore::blend::definition_check::{
    bare_blend_faces, BlendFaceKind, BlendFaceLog, CornerKind,
};
use solidcore::blend::fillet_executive::api::{chamfer_fillet, fillet_edges_per_edge};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, FitSize};
use solidcore::blend::fillet_geom::p1::FilletCornerType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_pyramid};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::query::face::p2::face_vertices;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{create_planar_face_with_3d_curves, FaceBuildEnv};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{edge, EdgeId, FaceId, VertexId};
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

/// Every fix but `geometric_g2_continuity`.
fn all_fixes() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
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

/// The corner's face of `b`: one face with the three vertices `v`, a ball patch (a
/// rational B-spline carrying its sphere, never the analytic sphere) on the sphere about
/// `c` of radius `rad`, its normal out
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
    // the fork's f8e6709: an SmBallPatch carrying the corner ball's sphere
    let SurfaceKind::BallPatch(patch) = s.kind() else {
        panic!("the Ball's face is a ball patch, got {:?}", s.kind());
    };
    assert!(patch.net().is_rational());
    assert!(patch.centre().distance(c) <= 1e-12, "{:?}", patch.centre());
    assert!((patch.radius() - rad).abs() <= 1e-12, "{}", patch.radius());
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
    let mut k = all_fixes();
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

/// capi `ball pocket_corner`: a 40 x 30 x 20 block with a 20 x 10 x 10 pocket, the three
/// concave edges at the pocket's floor corner (10, 10, 10) rounded r 2: the ball about
/// (12, 12, 12), added material.
#[test]
fn pocket_corner_is_the_balls_octant() {
    let mut k = all_fixes();
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
    let mut k = all_fixes();
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
    let mut k = all_fixes();
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
    let mut k = all_fixes();
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

// ---- Chamfer corners (WR-716, #2345) ------------------------------------------------
//
// In a corner frame (u, v, w), measured from the vertex into the blend's side (the
// material at a convex corner, the air at a concave one), the three faces at a square
// corner are u = 0, v = 0 and w = 0. A linear section of constant radius d on a square
// edge sets back d on both faces (the ball touches each face d from the edge), so the
// chamfer of the edge along u, setback a, is the plane v + w = a; along v, setback b,
// u + w = b; along w, setback c, u + v = c.
//
// Mitre. The three planes meet at M, the 3x3 solve: u + v + w = (a + b + c) / 2, so
// M = ((b + c - a) / 2, (a + c - b) / 2, (a + b - c) / 2). The rails cross on w = 0 at
// X_w = (b, a, 0), on u = 0 at X_u = (0, c, b) and on v = 0 at X_v = (c, 0, a). The
// mitre removes (convex) or adds (concave) the union of the three edges' prisms; by
// inclusion and exclusion, with the prisms' own volumes a^2 / 2 times their lengths,
//   P_u P_v = {v + w < a, u + w < b}: int_0^min(a, b) (a - w)(b - w) dw,
//   P_u P_w = {v + w < a, u + v < c}: int_0^min(a, c) (a - v)(c - v) dv,
//   P_v P_w = {u + w < b, u + v < c}: int_0^min(b, c) (b - u)(c - u) du,
// and the triple overlap integrated slice by slice (worked out per case).
//
// Facet. The plane through X_u, X_v, X_w cuts off, beyond the mitre, the tetrahedron
// M X_u X_v X_w: |det(X_u - M, X_v - M, X_w - M)| / 6 more.

// Every fix but `geometric_g2_continuity`, with or without `fillet_chamfer_corner`.
fn chamfer_fixes(on: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (on || name != "fillet_chamfer_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

// A refused chamfer: the executive's error code, its reason and its noted fit.
#[derive(Debug)]
struct Refused {
    code: i32,
    comment: String,
    fit: Option<FitSize<EdgeId>>,
}

// Chamfers `edges` together, linear sections of constant radius `d[i]`, with the
// corners of type `corner`; stitched.
fn chamfer(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    d: &[f64],
    corner: BlendChamferCorner,
) -> Result<(), Refused> {
    let xs = vec![FilletSurfaceGeneratorType::Linear; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, edges, d, &xs, 1, 1.0)
            .unwrap();
        s.set_chamfer_corner(corner);
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            return Err(Refused {
                code: info.code,
                comment: info.comment.clone(),
                fit: s.fit_size(),
            });
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

// A corner frame: `o` the vertex, `a` the unit directions of u, v, w.
struct Frame {
    o: Point3,
    a: [Vec3; 3],
}

impl Frame {
    fn at(&self, u: f64, v: f64, w: f64) -> Point3 {
        self.o + self.a[0] * u + self.a[1] * v + self.a[2] * w
    }
    fn dir(&self, u: f64, v: f64, w: f64) -> Vec3 {
        unit(self.a[0] * u + self.a[1] * v + self.a[2] * w)
    }
}

// The vertices of edge `e`.
fn ends(b: &Brep, e: EdgeId) -> (Point3, Point3) {
    let v0 = edge::vertex(b, e).unwrap().unwrap();
    let v1 = edge::other_vertex(b, e, Some(v0)).unwrap().unwrap();
    (b.vertex(v0).unwrap().point, b.vertex(v1).unwrap().point)
}

// The edges of `b` from `p` to `q` (either way, to 1e-9).
fn edges_between(b: &Brep, p: Point3, q: Point3) -> Vec<EdgeId> {
    get_edges(b)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let (s, t) = ends(b, e);
            (s.distance(p) <= 1e-9 && t.distance(q) <= 1e-9)
                || (s.distance(q) <= 1e-9 && t.distance(p) <= 1e-9)
        })
        .collect()
}

// How far edge `e`'s own geometry is from the exact segment `p`-`q`: infinite unless its
// curve is a `Line`; else the worst of its vertices' distances from `p` and `q` and its
// points' from the line.
fn off_exact_segment(k: &Kernel, b: &Brep, e: EdgeId, p: Point3, q: Point3) -> f64 {
    let curve = edge::curve(b, e).unwrap().unwrap();
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return f64::INFINITY;
    }
    let (s, t) = ends(b, e);
    let mut off = (s.distance(p) + t.distance(q)).min(s.distance(q) + t.distance(p));
    let dir = unit(q - p);
    let ivl = edge::interval(b, e).unwrap();
    for i in 0..=16 {
        let x = curve
            .evaluate_point(k, ivl.evaluate(f64::from(i) / 16.0))
            .unwrap();
        off = off.max(((x - p) - dir * (x - p).dot(dir)).length());
    }
    off
}

// How far edge `e` is from the exact segment `p`-`q`: infinite unless its curve is a
// `Line` and its faces planes; else the worst of its vertices' distances from `p` and
// `q`, its points' from the line, and, on each of its edge uses, the distance of its own
// UV trim curve, mapped by its face's plane, from the edge, and of that trim from the
// straight line in UV between its ends.
fn off_exact_line(k: &Kernel, b: &Brep, e: EdgeId, p: Point3, q: Point3) -> f64 {
    let mut off = off_exact_segment(k, b, e, p, q);
    if !off.is_finite() {
        return off;
    }
    let curve = edge::curve(b, e).unwrap().unwrap();
    let ivl = edge::interval(b, e).unwrap();
    for eu in edge::edgeuses(b, e, None, None).unwrap() {
        let f = eu1::face(b, eu).unwrap().unwrap();
        let surface = face2::surface(b, f).unwrap().unwrap();
        if !matches!(surface.kind(), SurfaceKind::Plane(_)) {
            return f64::INFINITY;
        }
        // the edge use's own trim, else its mate's (on the same face)
        let Some(uv) = eu1::uv_trim_curve(b, eu).unwrap().0 else {
            return f64::INFINITY;
        };
        let w = |i: i32| {
            uv.evaluate_point(k, ivl.evaluate(f64::from(i) / 16.0))
                .unwrap()
        };
        let (w0, w1) = (w(0), w(16));
        for i in 0..=16 {
            let at = ivl.evaluate(f64::from(i) / 16.0);
            let wi = w(i);
            let x = curve.evaluate_point(k, at).unwrap();
            let y = surface.evaluate_point(k, Point2::new(wi.x, wi.y)).unwrap();
            off = off.max(x.distance(y));
            let line = w0 + (w1 - w0) * (f64::from(i) / 16.0);
            off = off.max(wi.distance(line));
        }
    }
    off
}

// The mitre at a corner of three: for each rail crossing X_i, one edge X_i-M; returns
// the worst distance of those edges from the exact segments (`off_exact_line`).
fn mitre_off(k: &Kernel, b: &Brep, m: Point3, xs: &[Point3; 3]) -> f64 {
    mitre_off_by(k, b, m, xs, off_exact_line)
}

// The same, each edge measured by `measure`.
fn mitre_off_by(
    k: &Kernel,
    b: &Brep,
    m: Point3,
    xs: &[Point3; 3],
    measure: fn(&Kernel, &Brep, EdgeId, Point3, Point3) -> f64,
) -> f64 {
    let mut off = 0.0f64;
    for &x in xs {
        let es = edges_between(b, x, m);
        assert_eq!(es.len(), 1, "one mitre edge from {x:?} to {m:?}");
        off = off.max(measure(k, b, es[0], x, m));
    }
    off
}

// The faces of `b` whose vertices are exactly `v` (to 1e-9).
fn faces_on(b: &Brep, v: &[Point3]) -> Vec<FaceId> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let mut pts: Vec<Point3> = Vec::new();
        for e in solidcore::topology::topology_traverser::face_edges(b, f).unwrap() {
            let (s, t) = ends(b, e);
            for p in [s, t] {
                if !pts.iter().any(|q| q.distance(p) <= 1e-9) {
                    pts.push(p);
                }
            }
        }
        if pts.len() == v.len() && v.iter().all(|w| pts.iter().any(|p| p.distance(*w) <= 1e-9)) {
            out.push(f);
        }
    }
    out
}

// The facet on the rail crossings `xs` (in loop order): one face with those vertices, a
// `Plane` whose unit normal is `normal` (out of the body), its edges X_i-X_(i+1) the
// exact segments. Returns the worst of its normal's distance from `normal` and its
// edges' from the segments.
fn facet_off(k: &Kernel, b: &Brep, xs: &[Point3], normal: Vec3) -> f64 {
    let faces = faces_on(b, xs);
    assert_eq!(faces.len(), 1, "one facet on {xs:?}");
    let s = face2::surface(b, faces[0]).unwrap().unwrap();
    assert!(
        matches!(s.kind(), SurfaceKind::Plane(_)),
        "the facet is a plane, got {:?}",
        s.kind()
    );
    let mid = s.natural_uv_domain_with_nurb(k).unwrap().evaluate(0.5, 0.5);
    let n = s
        .evaluate_normal(
            k,
            mid,
            solidcore::nurbs::knots::Side::Right,
            solidcore::nurbs::knots::Side::Right,
        )
        .unwrap();
    let mut off = (unit(n) - normal).length();
    for i in 0..xs.len() {
        let (p, q) = (xs[i], xs[(i + 1) % xs.len()]);
        let es = edges_between(b, p, q);
        assert_eq!(es.len(), 1, "one facet edge from {p:?} to {q:?}");
        off = off.max(off_exact_line(k, b, es[0], p, q));
    }
    off
}

// The faces, edges and vertices, the check's lines and the volume of `b`.
fn report(k: &mut Kernel, b: &mut Brep) -> ((usize, usize, usize), Vec<String>, f64) {
    let n = counts(b);
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    let v = volume(k, b);
    (n, lines, v)
}

// Points are exact when they are within rounding at the part's scale.
const EXACT: f64 = 1e-13;

// The 20 cube's eight corners, in their frames (u, v, w along the cube's edges inward).
fn cube_frames() -> Vec<Frame> {
    let mut out = Vec::new();
    for i in 0..8_u32 {
        let c = [i & 1, (i >> 1) & 1, (i >> 2) & 1].map(f64::from);
        let s = c.map(|ci| 1.0 - 2.0 * ci);
        out.push(Frame {
            o: Point3::new(20.0 * c[0], 20.0 * c[1], 20.0 * c[2]),
            a: [
                Vec3::new(s[0], 0.0, 0.0),
                Vec3::new(0.0, s[1], 0.0),
                Vec3::new(0.0, 0.0, s[2]),
            ],
        });
    }
    out
}

// The 20 cube's twelve edges chamfered d = 1/sqrt 2 (a = b = c = d at every corner):
// M = (d/2, d/2, d/2), X_w = (d, d, 0) and its turns. Per corner the pairs overlap by
// int_0^d (d - w)^2 dw = d^3 / 3 each; the triple overlap, sliced by w, is the square
// [0, d - w]^2 less, below w = d/2, its corner u + v > d, (d - 2w)^2 / 2: d^3 / 3 -
// d^3 / 12 = d^3 / 4. So each corner gives back 3 d^3 / 3 - d^3 / 4 = 3 d^3 / 4 of the
// prisms' 12 x 20 x d^2 / 2 = 60, and the cube keeps 8000 - 60 + 6 d^3 =
// 7940 + (3/2) sqrt 2 (d^3 = sqrt 2 / 4). Faces 6 + 12, edges 24 rails + 24 mitres,
// vertices 8 x (3 + 1).
const CUBE_MITRE: f64 = 7940.0 + 1.5 * std::f64::consts::SQRT_2;

fn cube_mitres_off(k: &Kernel, b: &Brep) -> f64 {
    cube_mitres_off_by(k, b, off_exact_line)
}

fn cube_mitres_off_by(
    k: &Kernel,
    b: &Brep,
    measure: fn(&Kernel, &Brep, EdgeId, Point3, Point3) -> f64,
) -> f64 {
    let d = std::f64::consts::FRAC_1_SQRT_2;
    let mut off = 0.0f64;
    for f in cube_frames() {
        let m = f.at(d / 2.0, d / 2.0, d / 2.0);
        let xs = [f.at(d, d, 0.0), f.at(0.0, d, d), f.at(d, 0.0, d)];
        off = off.max(mitre_off_by(k, b, m, &xs, measure));
    }
    off
}

#[test]
fn cube_corners_are_exact_mitres() {
    let mut k = chamfer_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let edges = get_edges(&b).unwrap();
    let d = std::f64::consts::FRAC_1_SQRT_2;
    chamfer(&mut k, &mut b, &edges, &[d; 12], BlendChamferCorner::Mitre).unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let off = cube_mitres_off(&k, &b);
    eprintln!("cube mitre: volume {v:.12} want {CUBE_MITRE:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (18, 48, 32));
    assert!(off <= EXACT * 20.0, "{off}");
    assert!((v - CUBE_MITRE).abs() <= 1e-12 * CUBE_MITRE, "{v}");
}

// The constant-distance chamfer (`SmApiChamferFillet`, chord 1, so setbacks 1/sqrt 2 on
// the square edges; every corner a bevel): the same mitres, the same volume.
#[test]
#[ignore = "the constant-distance solver's chamfer surfaces are not exact planes past their edges' ends (inner control points of their extension spans off by up to 1.3e-9), so the faces the mitres lie on are off by 4.3e-10 at the corners; the event's own geometry is checked by cube_constant_distance_corners_are_mitres_of_exact_lines"]
fn cube_constant_distance_corners_are_exact_mitres() {
    let mut k = chamfer_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    chamfer_fillet(&mut k, &mut b, 1.0).unwrap();
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let off = cube_mitres_off(&k, &b);
    eprintln!(
        "cube chord mitre: volume {v:.12} want {CUBE_MITRE:.12}; {n:?}; off {off:.3e}; {lines:?}"
    );
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (18, 48, 32));
    assert!(off <= EXACT * 20.0, "{off}");
    assert!((v - CUBE_MITRE).abs() <= 1e-12 * CUBE_MITRE, "{v}");
}

// The event's own geometry on the constant-distance chamfers: the faces, edges and
// vertices, a clean check, and every mitre the exact segment X_i-M as a `Line` between
// exact vertices (`off_exact_segment`; the faces it lies on are the solver's).
#[test]
fn cube_constant_distance_corners_are_mitres_of_exact_lines() {
    let mut k = chamfer_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    chamfer_fillet(&mut k, &mut b, 1.0).unwrap();
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let n = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let off = cube_mitres_off_by(&k, &b, off_exact_segment);
    eprintln!("cube chord mitre lines: {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (18, 48, 32));
    assert!(off <= EXACT * 20.0, "{off}");
}

// The cube's corners as facets: each the triangle X_u X_v X_w on u + v + w = 2d, its
// normal out of the cube towards the vertex. Beyond the mitre each removes the
// tetrahedron M X_u X_v X_w: X - M = (d/2)(1, 1, -1) and its turns, det = -d^3 / 2, so
// d^3 / 12; the cube keeps 7940 + (3/2) sqrt 2 - 8 sqrt 2 / 48 = 7940 + (4/3) sqrt 2.
// Faces 6 + 12 + 8, edges 24 rails + 24 facet edges, vertices 8 x 3.
#[test]
fn cube_corners_are_exact_facets() {
    let mut k = chamfer_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let edges = get_edges(&b).unwrap();
    let d = std::f64::consts::FRAC_1_SQRT_2;
    chamfer(&mut k, &mut b, &edges, &[d; 12], BlendChamferCorner::Facet).unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let want = 7940.0 + 4.0 / 3.0 * std::f64::consts::SQRT_2;
    let mut off = 0.0f64;
    for f in cube_frames() {
        let xs = [f.at(d, d, 0.0), f.at(0.0, d, d), f.at(d, 0.0, d)];
        off = off.max(facet_off(&k, &b, &xs, -f.dir(1.0, 1.0, 1.0)));
    }
    eprintln!("cube facet: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (26, 48, 24));
    assert!(off <= EXACT * 20.0, "{off}");
    assert!((v - want).abs() <= 1e-12 * want, "{v}");
}

// The 40 x 30 x 20 box's corner (40, 30, 20): u = 40 - x, v = 30 - y, w = 20 - z.
fn box_corner() -> Frame {
    Frame {
        o: Point3::new(40.0, 30.0, 20.0),
        a: [-Vec3::X, -Vec3::Y, -Vec3::Z],
    }
}

// The edges at the box's corner (40, 30, 20): along x (a), along y (b), along z (c).
const BOX_CORNER_EDGES: [[f64; 3]; 3] =
    [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]];

// `unequal_chamfers_at_a_corner`: the box's corner chamfered a = 2 (along x), b = 3
// (along y), c = 4 (along z), so v + w = 2, u + w = 3, u + v = 4. M = (2.5, 1.5, 0.5),
// inside; X_w = (3, 2, 0), X_u = (0, 4, 3), X_v = (4, 0, 2). Prisms 4/2 x 40 + 9/2 x 30
// + 16/2 x 20 = 375; pairs int_0^2 (2 - w)(3 - w) dw = 14/3, int_0^2 (2 - v)(4 - v) dv
// = 20/3, int_0^3 (3 - u)(4 - u) du = 27/2; the triple, sliced by w in [0, 2]: the
// rectangle (3 - w)(2 - w) less, below w = 1/2, its corner u + v > 4, (1 - 2w)^2 / 2:
// 14/3 - 1/12 = 55/12. The mitre removes 375 - 14/3 - 20/3 - 27/2 + 55/12 = 354.75.
// Faces 6 + 3; edges 9 kept, 3 x (2 rails + 1 end), 3 mitres; vertices 7 + 3 ends + 3
// crossings + M.
#[test]
fn unequal_chamfers_at_a_corner_mitre() {
    let mut k = chamfer_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
    chamfer(
        &mut k,
        &mut b,
        &edges,
        &[2.0, 3.0, 4.0],
        BlendChamferCorner::Mitre,
    )
    .unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let f = box_corner();
    let m = f.at(2.5, 1.5, 0.5);
    let xs = [
        f.at(3.0, 2.0, 0.0),
        f.at(0.0, 4.0, 3.0),
        f.at(4.0, 0.0, 2.0),
    ];
    let off = mitre_off(&k, &b, m, &xs);
    let want = 24000.0 - 354.75;
    eprintln!("unequal mitre: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (9, 21, 14));
    assert!(off <= EXACT * 40.0, "{off}");
    assert!((v - want).abs() <= 1e-12 * want, "{v}");
}

// The same corner as a facet: the plane through X_w, X_u, X_v is 10u + 9v + 4w = 48
// (30 + 18, 36 + 12, 40 + 8), M (40.5) on the vertex's side, so a real face; its normal
// out of the box is (10, 9, 4) / sqrt 197 in x, y, z. It removes the tetrahedron:
// X - M = (-2.5, 2.5, 2.5), (1.5, -1.5, 1.5), (0.5, 0.5, -0.5), det 7.5, so 1.25 more.
// Faces 10; edges 9 + 9 + 3 facet edges; vertices 7 + 3 + 3.
#[test]
fn unequal_chamfers_at_a_corner_facet() {
    let mut k = chamfer_fixes(true);
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
    chamfer(
        &mut k,
        &mut b,
        &edges,
        &[2.0, 3.0, 4.0],
        BlendChamferCorner::Facet,
    )
    .unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let f = box_corner();
    let xs = [
        f.at(3.0, 2.0, 0.0),
        f.at(0.0, 4.0, 3.0),
        f.at(4.0, 0.0, 2.0),
    ];
    let off = facet_off(&k, &b, &xs, -f.dir(10.0, 9.0, 4.0));
    let want = 24000.0 - 354.75 - 1.25;
    eprintln!("unequal facet: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 21, 13));
    assert!(off <= EXACT * 40.0, "{off}");
    assert!((v - want).abs() <= 1e-12 * want, "{v}");
}

// brett19's case on #2345: `fillet_edges_per_edge` with a linear section (not the chamfer
// API, which sets every corner to bevel) reaches the pinned 3x3 corner's non-bevel
// `m_bChamfer` branch. On the 2/3/4 corner that branch builds 10 faces and removes
// 24000 - 23640.833333 = 359.166667: neither the mitre's closed form (354.75, 9 faces,
// above) nor the facet's (356, 10 faces), and its corner face is a four-sided B-spline
// whose corners are not coplanar. With the flag, the same call builds the mitre, the
// session's default corner type (#2345 section 6, answer 1).
#[test]
fn per_edge_linear_chamfers_at_a_corner_are_the_mitre() {
    let corner_volume = |on: bool| {
        let mut k = chamfer_fixes(on);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
        let xs = [FilletSurfaceGeneratorType::Linear; 3];
        fillet_edges_per_edge(&mut k, &mut b, &edges, &[2.0, 3.0, 4.0], &xs, 1, 1.0).unwrap();
        let (n, lines, v) = report(&mut k, &mut b);
        let bspline_faces: Vec<FaceId> = get_faces(&b)
            .unwrap()
            .into_iter()
            .filter(|&f| {
                matches!(
                    face2::surface(&b, f).unwrap().unwrap().kind(),
                    SurfaceKind::BSpline(_)
                )
            })
            .collect();
        (k, b, n, lines, v, bspline_faces)
    };
    let mitre = 24000.0 - 354.75;
    let facet = 24000.0 - 356.0;

    // without the flag: the pinned branch, off both closed forms
    let (k, b, n, lines, v, patches) = corner_volume(false);
    eprintln!("per edge, pinned: volume {v:.9}; {n:?}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 22, 14));
    assert!((v - 23640.833333).abs() <= 1e-6, "{v}");
    assert!((v - mitre).abs() > 4.0 && (v - facet).abs() > 3.0, "{v}");
    assert_eq!(patches.len(), 1);
    let corners: Vec<Point3> = face_vertices(&b, patches[0], false, None)
        .unwrap()
        .into_iter()
        .map(|vx| solidcore::topology::vertex::point(&b, vx).unwrap())
        .collect();
    assert_eq!(corners.len(), 4, "{corners:?}");
    // the fourth corner's distance from the plane of the other three
    let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
    let normal = normal / normal.length();
    let out_of_plane = (corners[3] - corners[0]).dot(normal).abs();
    eprintln!("per edge, pinned: corner face's fourth corner {out_of_plane:.6} off the plane");
    assert!(out_of_plane > 1e-3, "{out_of_plane}");
    drop(k);

    // with the flag: the mitre, exactly
    let (k, b, n, lines, v, patches) = corner_volume(true);
    let f = box_corner();
    let xs = [
        f.at(3.0, 2.0, 0.0),
        f.at(0.0, 4.0, 3.0),
        f.at(4.0, 0.0, 2.0),
    ];
    let off = mitre_off(&k, &b, f.at(2.5, 1.5, 0.5), &xs);
    eprintln!("per edge, event: volume {v:.12} want {mitre:.12}; {n:?}; off {off:.3e}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (9, 21, 14));
    assert!(patches.is_empty());
    assert!(off <= EXACT * 40.0, "{off}");
    assert!((v - mitre).abs() <= 1e-12 * mitre, "{v}");
}

// The pocket's floor corner (10, 10, 10) of the 40 x 30 x 20 block less a 20 x 10 x 11
// box at (10, 10, 10): u = x - 10, v = y - 10, w = z - 10, into the pocket. Its three
// concave edges chamfered a = 3 (along x, 20 long), b = 2 (along y, 10 long), c = 2
// (along z, 10 long) add material. M = (1/2, 3/2, 3/2), inside; X_w = (2, 3, 0),
// X_u = (0, 2, 2), X_v = (2, 0, 3). Prisms 9/2 x 20 + 4/2 x 10 + 4/2 x 10 = 130; pairs
// int_0^2 (3 - w)(2 - w) dw = 14/3, int_0^2 (3 - v)(2 - v) dv = 14/3,
// int_0^2 (2 - u)^2 du = 8/3; the triple, sliced by u in [0, 2]: the square (2 - u)^2
// less, below u = 1/2, its corner v + w > 3, (1 - 2u)^2 / 2: 8/3 - 1/12 = 31/12. The
// mitre adds 130 - 12 + 31/12 = 1447/12 to 22000.
fn pocket() -> (Kernel, Brep, Vec<EdgeId>) {
    let mut k = chamfer_fixes(true);
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let cut = create_box(&mut k, Point3::new(10.0, 10.0, 10.0), 20.0, 10.0, 11.0).unwrap();
    let b = boolean_difference(&mut k, block, cut).unwrap();
    let edges = [[20.0, 10.0, 10.0], [10.0, 15.0, 10.0], [10.0, 10.0, 15.0]]
        .iter()
        .map(|&p| pick(&k, &b, p))
        .collect();
    (k, b, edges)
}

fn pocket_corner() -> Frame {
    Frame {
        o: Point3::new(10.0, 10.0, 10.0),
        a: [Vec3::X, Vec3::Y, Vec3::Z],
    }
}

// The concave mitre: the same event mirrored. Faces 11 + 3; vertices and edges as the
// Ball's corner (15, 33, 21) with the ball's face and its three sections traded for M
// and three mitres.
#[test]
fn pocket_floor_corner_concave_mitre() {
    let (mut k, mut b, edges) = pocket();
    chamfer(
        &mut k,
        &mut b,
        &edges,
        &[3.0, 2.0, 2.0],
        BlendChamferCorner::Mitre,
    )
    .unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let f = pocket_corner();
    let m = f.at(0.5, 1.5, 1.5);
    let xs = [
        f.at(2.0, 3.0, 0.0),
        f.at(0.0, 2.0, 2.0),
        f.at(2.0, 0.0, 3.0),
    ];
    let off = mitre_off(&k, &b, m, &xs);
    let want = 22000.0 + 1447.0 / 12.0;
    eprintln!("pocket mitre: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (14, 33, 22));
    assert!(off <= EXACT * 40.0, "{off}");
    assert!((v - want).abs() <= 1e-12 * want, "{v}");
}

// The concave facet: the plane through X_w, X_u, X_v is u + 2v + 2w = 8 (2 + 6, 4 + 4,
// 2 + 6), M (6.5) on the vertex's side; its normal out of the body points into the
// pocket, (1, 2, 2) / 3. It adds the tetrahedron M X_u X_v X_w: M is (8 - 6.5) / 3 = 1/2
// from the plane and the triangle's area is |(3, 6, 6)| / 2 = 9/2, so 3/4 more:
// 22000 + 1447/12 + 9/12 = 22000 + 364/3. Faces 15, as the Ball's.
#[test]
fn pocket_floor_corner_concave_facet() {
    let (mut k, mut b, edges) = pocket();
    chamfer(
        &mut k,
        &mut b,
        &edges,
        &[3.0, 2.0, 2.0],
        BlendChamferCorner::Facet,
    )
    .unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let f = pocket_corner();
    let xs = [
        f.at(2.0, 3.0, 0.0),
        f.at(0.0, 2.0, 2.0),
        f.at(2.0, 0.0, 3.0),
    ];
    let off = facet_off(&k, &b, &xs, f.dir(1.0, 2.0, 2.0));
    let want = 22000.0 + 364.0 / 3.0;
    eprintln!("pocket facet: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (15, 33, 21));
    assert!(off <= EXACT * 40.0, "{off}");
    assert!((v - want).abs() <= 1e-12 * want, "{v}");
}

// The box's corner chamfered a = 1, b = 1, c = 3: M = (3/2, 3/2, -1/2) is outside the
// corner (w < 0): chamfer c reaches past the crossing X_w = (1, 1, 0) of the other two
// (u + v = 2 < 3 there). With c scaled by t, w_M = (2 - 3t) / 2 (and u + v - 3t at X_w)
// is positive only for t < 2/3, so the largest c that keeps M inside is a + b = 2, an
// open bound (at it the mitre X_w-M has no length): the fit is 2 less the fit margin,
// 1000 edge tolerances. The facet is no remedy: through X_w, X_u = (0, 3, 1) and
// X_v = (3, 0, 1) it is u + v - w = 2, with the vertex (0) and M (3.5) on opposite sides,
// so it would cut the chamfers, not close them; it is refused with the same fit.
#[test]
fn a_mitre_point_outside_the_corner_is_refused_with_its_fit() {
    for corner in [BlendChamferCorner::Mitre, BlendChamferCorner::Facet] {
        let mut k = chamfer_fixes(true);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
        let margin = 1000.0 * edge::tolerance(&b, edges[2]).unwrap();
        let refused = chamfer(&mut k, &mut b, &edges, &[1.0, 1.0, 3.0], corner).unwrap_err();
        eprintln!("{corner:?}: {refused:?}");
        assert_eq!(refused.code, 8, "NoRoom");
        assert!(
            refused.comment.contains("outside the corner"),
            "{}",
            refused.comment
        );
        let fit = refused.fit.unwrap();
        assert_eq!(fit.edge, Some(edges[2]));
        assert_eq!(fit.kind, FitKind::Radius);
        assert!((fit.size - (2.0 - margin)).abs() <= 1e-12, "{}", fit.size);

        // and the fit builds: the union of the prisms with a = b = 1 and c, sliced as
        // above: 20 + 15 + 10 c^2 less the pairs 1/3 and twice c/2 - 1/6, plus the
        // triple 1/3 - (2 - c)^3 / 12
        let mut k = chamfer_fixes(true);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
        let c = fit.size;
        chamfer(&mut k, &mut b, &edges, &[1.0, 1.0, c], corner).unwrap();
        let (_, lines, v) = report(&mut k, &mut b);
        let mitre = 24000.0 - (35.0 + 10.0 * c * c - c + 1.0 / 3.0 - (2.0 - c).powi(3) / 12.0);
        // the facet's tetrahedron: M = (c/2, c/2, (2 - c)/2), X_w = (1, 1, 0),
        // X_u = (0, c, 1), X_v = (c, 0, 1)
        let m = Vec3::new(c / 2.0, c / 2.0, (2.0 - c) / 2.0);
        let (xw, xu, xv) = (
            Vec3::new(1.0, 1.0, 0.0) - m,
            Vec3::new(0.0, c, 1.0) - m,
            Vec3::new(c, 0.0, 1.0) - m,
        );
        let tet = xu.dot(xv.cross(xw)).abs() / 6.0;
        let want = if corner == BlendChamferCorner::Mitre {
            mitre
        } else {
            mitre - tet
        };
        eprintln!("{corner:?} at the fit {c}: volume {v:.12} want {want:.12}; {lines:?}");
        assert!(lines.is_empty(), "{lines:?}");
        assert!((v - want).abs() <= 1e-12 * want, "{v}");
    }
}

// `SmApiCreatePyramid`'s square pyramid, base 20 about the origin, apex (0, 0, 10): its
// faces are +-x + z = 10 and +-y + z = 10, 120 degrees apart across each apex edge
// (normals (1, 0, 1) and (0, 1, 1) at 60 degrees). The apex edges, from (0, 0, 10) to
// (+-10, +-10, 0), at their midpoints.
fn pyramid() -> (Kernel, Brep, Vec<EdgeId>) {
    let mut k = chamfer_fixes(true);
    let b = create_pyramid(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 10.0).unwrap();
    let edges = [
        [5.0, 5.0, 5.0],
        [-5.0, 5.0, 5.0],
        [-5.0, -5.0, 5.0],
        [5.0, -5.0, 5.0],
    ]
    .iter()
    .map(|&p| pick(&k, &b, p))
    .collect();
    (k, b, edges)
}

// N = 4: the pyramid's apex, its four edges chamfered radius r = 2 sqrt 2. Across a
// 120 degree edge the ball touches each face r cot 60 = r / sqrt 3 from the edge; in a
// horizontal section (a square of half side 10 - z) that rail cuts the square's corner
// l = (r / sqrt 3) sqrt(3/2) = r / sqrt 2 = 2 along each side, the same at every height.
// Two cuts on one side meet at its middle where 2 l = 2 (10 - z): the crossings
// X = (+-2, 0, 8), (0, +-2, 8) lie in the plane z = 8 (by the pyramid's symmetry), so
// the facet builds, its normal +z. Below it each section is the square less four
// corners l^2 / 2: int_0^8 4 (10 - z)^2 - 2 l^2 dz = (4/3)(1000 - 8) - 8 x 8 = 3776/3.
// Faces 5 + 4 + 1; edges 4 base, 4 x (2 rails + 1 end on the base), 4 facet edges;
// vertices 4 x 2 on the base and 4 crossings.
#[test]
fn pyramid_apex_of_four_equal_chamfers_is_a_facet() {
    let (mut k, mut b, edges) = pyramid();
    let r = 2.0 * std::f64::consts::SQRT_2;
    chamfer(&mut k, &mut b, &edges, &[r; 4], BlendChamferCorner::Facet).unwrap();
    let (n, lines, v) = report(&mut k, &mut b);
    let xs = [
        Point3::new(2.0, 0.0, 8.0),
        Point3::new(0.0, 2.0, 8.0),
        Point3::new(-2.0, 0.0, 8.0),
        Point3::new(0.0, -2.0, 8.0),
    ];
    let off = facet_off(&k, &b, &xs, Vec3::Z);
    let want = 3776.0 / 3.0;
    eprintln!("pyramid facet: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 20, 12));
    assert!(off <= EXACT * 20.0, "{off}");
    assert!((v - want).abs() <= 1e-12 * want, "{v}");
}

// The mitre of four or more chamfers is not built (the planes meet in one point only by
// coincidence; their general meeting, a roof of mitre lines, is not designed): refused,
// naming why, even at the regular apex where the facet builds.
#[test]
fn a_mitre_of_four_chamfers_is_refused() {
    let (mut k, mut b, edges) = pyramid();
    let r = 2.0 * std::f64::consts::SQRT_2;
    let refused = chamfer(&mut k, &mut b, &edges, &[r; 4], BlendChamferCorner::Mitre).unwrap_err();
    eprintln!("{refused:?}");
    assert_eq!(refused.code, 9, "CornerUnsupported");
    assert!(
        refused.comment.contains("mitre of four or more chamfers"),
        "{}",
        refused.comment
    );
}

// An irregular N = 4 apex: corner cuts l = r / sqrt 2 of 1, 3, 2, 2 on the edges towards
// (+, +), (-, +), (-, -), (+, -). The cuts on a side meet where their sum is the side,
// so the crossings are X(+x) = (3/2, 1/2, 17/2), X(+y) = (1, 2, 8), X(-x) =
// (-5/2, -1/2, 15/2), X(-y) = (0, -2, 8). The plane through X(-y), X(+y), X(+x) has
// normal (1, 4, 0) x (3/2, 5/2, 1/2) = (2, -1/2, -7/2), and X(-x) - X(-y) =
// (-5/2, 3/2, -1/2) is off it by 4 / sqrt(33/2), about 0.98: no plane holds the four
// crossings, so no facet. The corner is a patch (#2345 section 6, the owner's answer 3),
// whichever corner type was asked: the chamfers stop at their crossings, and an order-0
// fill closes the skew quadrilateral of their end sections, G0 to each chamfer.
//
// At four sides the fill is the bilinear map of the crossings. It is the interior map of
// least bending energy that is each end section along its edge of the domain (a square),
// and the least degree that meets those conditions is 2, where they hold for one map only:
// a quadratic linear along all four sides of a square is the bilinear one. So the corner
// face is the hyperbolic paraboloid through the crossings, its centre (the domain's
// centre) at their mean (-1/4, 0, 8), its edges the exact end sections.
//
// The volume, by the divergence theorem, is (1/3) the sum over the faces of the integral
// of x . n. For a bilinear patch that integral is the mean of its two triangulations', so
// the body's volume is the mean of the volumes of the two polyhedra whose corner faces are
// the patch's two triangulations, the other faces kept. Their faces, from the cuts (in a
// horizontal section, a square of half side a = 10 - z, each cut takes l from both sides
// at its corner): the base octagon (9, 10), (-7, 10), (-10, 7), (-10, -8), (-8, -10),
// (8, -10), (10, -8), (10, 9) at z = 0; each side face the triangle from its base edge to
// its crossing; each chamfer the quadrilateral from its base cut to its two crossings.
// Summed as cones from (0, 0, 2), which sees every face from inside, in exact arithmetic:
// 3754/3 with the corner split along X(+x)-X(-x), 3752/3 along X(+y)-X(-y). They differ
// by the crossings' tetrahedron, |det(X(+y) - X(+x), X(-x) - X(+x), X(-y) - X(+x))| / 6
// = 2/3, as they must. The mean: 1251.
#[test]
fn an_irregular_apex_of_four_chamfers_is_a_corner_patch() {
    let xp = Point3::new(1.5, 0.5, 8.5); // X(+x)
    let yp = Point3::new(1.0, 2.0, 8.0); // X(+y)
    let xm = Point3::new(-2.5, -0.5, 7.5); // X(-x)
    let ym = Point3::new(0.0, -2.0, 8.0); // X(-y)
    let want = 1251.0;
    let centre = (xp + yp + xm + ym) * 0.25;
    for corner in [BlendChamferCorner::Facet, BlendChamferCorner::Mitre] {
        let (mut k, mut b, edges) = pyramid();
        let s2 = std::f64::consts::SQRT_2;
        chamfer(
            &mut k,
            &mut b,
            &edges,
            &[s2, 3.0 * s2, 2.0 * s2, 2.0 * s2],
            corner,
        )
        .unwrap();
        let (n, lines, v) = report(&mut k, &mut b);
        let xs = [xp, yp, xm, ym];
        let patches = faces_on(&b, &xs);
        assert_eq!(patches.len(), 1, "one corner face on the crossings");
        let s = face2::surface(&b, patches[0]).unwrap().unwrap();
        assert!(
            matches!(s.kind(), SurfaceKind::BSpline(_)),
            "the patch is the fill, got {:?}",
            s.kind()
        );
        let mut off = s
            .evaluate_point(&k, Point2::new(0.0, 0.0))
            .unwrap()
            .distance(centre);
        for i in 0..4 {
            let (a, c) = (xs[i], xs[(i + 1) % 4]);
            let es = edges_between(&b, a, c);
            assert_eq!(es.len(), 1, "one corner edge from {a:?} to {c:?}");
            off = off.max(off_exact_segment(&k, &b, es[0], a, c));
        }
        eprintln!("{corner:?}, irregular apex: volume {v:.12} want {want:.12}; {n:?}; off {off:.3e}; {lines:?}");
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(n, (10, 20, 12));
        assert!(off <= EXACT * 20.0, "{off}");
        assert!((v - want).abs() <= 1e-12 * want, "{v}");
    }
}

// Five and six chamfers at an apex: the pyramid over the regular n-gon of radius 10 at
// z = 0 with its apex A = (0, 0, 10), its n apex edges e_i (to B_i) chamfered by balls of
// unequal offsets r_i, so the crossings are not in one plane and the corner is the patch:
// n bilinear quadrilaterals round the crossings' centroid C, quadrilateral i between C, the
// middle M_(i-1) of the side X_(i-1)-X_i, X_i and M_i (#2345, the owner's answers). No
// smooth patch carries the straight end sections past four sides, so they are n faces, G0
// to each other along the spokes C-M_i and to each chamfer along the halves of its end
// section.
//
// The closed forms, from the pyramid alone: face F_i (A, B_(i-1), B_i) has its outward unit
// normal n_i. Across e_i (faces F_i and F_(i+1), interior angle phi, cos phi = -n_i .
// n_(i+1)) a ball of offsets r touches each face s = r cot(phi / 2) from the edge, so each
// rail is e_i moved by s along the face's unit direction w square to e_i into the face, and
// the chamfer is the plane through the two rails. X_i = F_i ^ C_(i-1) ^ C_i (a 3x3 solve);
// each chamfer meets the base z = 0 on F_i and on F_(i+1). The body's faces: the base
// polygon of those 2n cuts, each F_i the triangle of its two cuts and X_i, each chamfer
// the quadrilateral of its two cuts, X_(i+1) and X_i, and the patch's quadrilaterals. Its
// volume by the divergence theorem, as cones from (0, 0, 2), which sees every planar face
// from inside: a planar face's is its fan's; a bilinear quadrilateral's integral of x . n
// is the mean of its two triangulations' (each signed round the quadrilateral's loop: one
// along a diagonal can fold), so its cone is their mean. For n = 5 that is 771.1342754128,
// for n = 6 850.3378387820.
//
// The quadrilaterals are not yet definitions: each is in the ledger of bare blend faces
// (#2489) as a chamfer corner's, until the corner definitions take it (#2488).
//
// Topology: faces n + 1 + n chamfers + n quadrilaterals; edges n on the base, n x (2 rails
// + 1 on the base), n spokes and 2n halves of the end sections; vertices 2n on the base, n
// crossings, n middles and C.
#[test]
fn five_and_six_chamfers_at_an_apex_are_bilinear_quadrilaterals() {
    for (n, r) in [
        (5usize, vec![1.0, 2.0, 1.5, 2.5, 1.0]),
        (6, vec![1.0, 2.0, 1.5, 2.5, 1.0, 2.0]),
    ] {
        let apex = Point3::new(0.0, 0.0, 10.0);
        let base: Vec<Point3> = (0..n)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / n as f64;
                Point3::new(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
            })
            .collect();
        // F_i between e_(i-1) and e_i, its outward unit normal
        let face_normal = |i: usize| {
            let (b0, b1) = (base[(i + n - 1) % n], base[i % n]);
            let m = unit((b0 - apex).cross(b1 - apex));
            if m.dot((b0 + b1) * 0.5 - Point3::new(0.0, 0.0, 5.0)) > 0.0 {
                m
            } else {
                -m
            }
        };
        // chamfer i as (unit normal c, offset h): c . x = h
        let chamfer_plane = |i: usize| {
            let t = unit(base[i] - apex);
            let (na, nb) = (face_normal(i), face_normal(i + 1));
            let cos_phi = -na.dot(nb);
            let s = r[i] * (1.0 + cos_phi) / (1.0 - cos_phi * cos_phi).sqrt();
            let into = |m: Vec3, other: Point3| {
                let w = unit(m.cross(t));
                if w.dot(other - apex) > 0.0 {
                    w
                } else {
                    -w
                }
            };
            let pa = apex + into(na, base[(i + n - 1) % n]) * s;
            let pb = apex + into(nb, base[(i + 1) % n]) * s;
            let c = unit((pb - pa).cross(t));
            (c, c.dot(pa))
        };
        let solve = |rows: [(Vec3, f64); 3]| {
            let [(a, p), (b, q), (c, w)] = rows;
            let d = a.dot(b.cross(c));
            (b.cross(c) * p + c.cross(a) * q + a.cross(b) * w) * (1.0 / d)
        };
        let base_plane = (Vec3::Z, 0.0);
        let face = |i: usize| (face_normal(i), face_normal(i).dot(apex));
        let xs: Vec<Point3> = (0..n)
            .map(|i| solve([face(i), chamfer_plane((i + n - 1) % n), chamfer_plane(i)]))
            .collect();
        let cut = |i: usize, on: usize| solve([base_plane, chamfer_plane(i), face(on)]);
        let centre = xs.iter().fold(Point3::ZERO, |a, x| a + *x) * (1.0 / n as f64);
        let mid = |i: usize| (xs[i % n] + xs[(i + 1) % n]) * 0.5;
        let mut planar: Vec<Vec<Point3>> =
            vec![(0..n).flat_map(|i| [cut(i, i), cut(i, i + 1)]).collect()];
        for i in 0..n {
            planar.push(vec![cut((i + n - 1) % n, i), cut(i, i), xs[i]]);
            planar.push(vec![cut(i, i), cut(i, i + 1), xs[(i + 1) % n], xs[i]]);
        }
        // signed cones from o: a planar face's fan all one sign (o sees it from inside); a
        // quadrilateral's two triangulations each taken round its own loop, signed by the
        // first (a triangulation along its other diagonal can fold, one triangle seen from
        // behind)
        let o = Point3::new(0.0, 0.0, 2.0);
        let tet = |a: Point3, b: Point3, c: Point3| (a - o).dot((b - o).cross(c - o)) / 6.0;
        let mut want: f64 = planar
            .iter()
            .map(|f| {
                (1..f.len() - 1)
                    .map(|j| tet(f[0], f[j], f[j + 1]))
                    .sum::<f64>()
                    .abs()
            })
            .sum();
        for (i, &x) in xs.iter().enumerate() {
            let (m0, m1) = (mid(i + n - 1), mid(i));
            let one = tet(centre, m0, x) + tet(centre, x, m1);
            let other = tet(centre, m0, m1) + tet(m0, x, m1);
            want += one.signum() * 0.5 * (one + other);
        }

        for corner in [BlendChamferCorner::Facet, BlendChamferCorner::Mitre] {
            let mut k = chamfer_fixes(true);
            let mut faces: Vec<Vec<Point3>> = (0..n)
                .map(|i| vec![base[i], base[(i + 1) % n], apex])
                .collect();
            faces.push(base.iter().rev().copied().collect());
            let mut b = polyhedron(&mut k, &faces);
            let edges: Vec<EdgeId> = base
                .iter()
                .map(|p| {
                    let m = (*p + apex) * 0.5;
                    pick(&k, &b, [m.x, m.y, m.z])
                })
                .collect();
            k.blend_face_log = Some(BlendFaceLog::default());
            chamfer(&mut k, &mut b, &edges, &r, corner).unwrap();
            // each quadrilateral is in the ledger of faces not yet definitions (#2489), as a
            // chamfer corner's, until the corner definitions take it (#2488)
            let made = k.blend_face_log.take().unwrap().last().to_vec();
            let bare: Vec<_> = bare_blend_faces(&b, &made)
                .unwrap()
                .into_iter()
                .filter(|f| f.kind == BlendFaceKind::Corner(CornerKind::Chamfer))
                .collect();
            assert_eq!(bare.len(), n, "{bare:?}");
            assert!(bare.iter().all(|f| f.surface == "b-spline"), "{bare:?}");
            let (counts, lines, v) = report(&mut k, &mut b);
            let mut off = 0.0_f64;
            for (i, &x) in xs.iter().enumerate() {
                for (p, q) in [(centre, mid(i)), (mid(i + n - 1), x), (x, mid(i))] {
                    let es = edges_between(&b, p, q);
                    assert_eq!(es.len(), 1, "n {n}: one edge from {p:?} to {q:?}");
                    off = off.max(off_exact_segment(&k, &b, es[0], p, q));
                }
            }
            // the quadrilaterals: bilinear, their corners C, the middles and the crossings
            let mut quads = 0;
            for f in get_faces(&b).unwrap() {
                let s = face2::surface(&b, f).unwrap().unwrap();
                let SurfaceKind::BSpline(bs) = s.kind() else {
                    continue;
                };
                quads += 1;
                for p in &bs.control_point_net().unwrap().points {
                    let near = std::iter::once(centre)
                        .chain(xs.iter().copied())
                        .chain((0..n).map(mid))
                        .map(|q| q.distance(*p))
                        .fold(f64::INFINITY, f64::min);
                    off = off.max(near);
                }
            }
            eprintln!(
                "n {n} {corner:?}: volume {v:.12} want {want:.12}; {counts:?}; quads {quads}; off {off:.3e}; {lines:?}"
            );
            assert!(lines.is_empty(), "{lines:?}");
            assert_eq!(counts, (3 * n + 1, 7 * n, 4 * n + 1));
            assert_eq!(quads, n);
            assert!(off <= EXACT * 20.0, "{off}");
            assert!((v - want).abs() <= 1e-12 * want, "{v}");
        }
    }
}

// A chamfer corner keeps the type of the corner it replaces, the convex NxN corner
// (`SmFilletConvexNxNCorner` with `SetChamfer`): it is closed by that corner's topology
// (the bevel's for the Mitre, the ring's for the Facet), not by two blends meeting. The
// fork's own events are Nx2 corners by type, as they took over the Nx2 corner's tests one
// for one; were the chamfer event one too, the executive would treat it as a two-blend
// corner: its squeeze of a rail edge too short at an Nx2 corner, the Nx2 branch of the
// fillet edges' connectivity, and a mitred Turn's corner edges (#2355's review; the
// big-radius conversion it also listed is deleted, #2400). At the 2/3/4 corner,
// as either type: NByNConvex.
#[test]
fn a_chamfer_corner_is_the_nxn_corner_by_type() {
    for corner in [BlendChamferCorner::Mitre, BlendChamferCorner::Facet] {
        let mut k = chamfer_fixes(true);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &edges,
            &[2.0, 3.0, 4.0],
            &[FilletSurfaceGeneratorType::Linear; 3],
            1,
            1.0,
        )
        .unwrap();
        s.set_chamfer_corner(corner);
        s.do_fillet(&mut k).unwrap();
        let mut types = Vec::new();
        for c in s.fillet_corners().unwrap() {
            let c = s.corner(c).unwrap();
            if c.chamfer_corner.is_some() {
                types.push(c.corner_type());
            }
        }
        assert_eq!(types, vec![FilletCornerType::NByNConvex], "{corner:?}");
    }
}

// The corner keeps its definition, the plain data tangency-labs/solidcore#2488's
// corner-definition type is to take: the ring's chamfers (edge, width, plane) and the
// crossings, with M at the Mitre (no facet) and the facet's normal at the Facet. At the
// 2/3/4 corner (u = 40 - x, v = 30 - y, w = 20 - z) chamfer a (along x) is v + w = 2, the
// plane y + z = 48, its normal towards the kept side (v + w > 2) (0, -1, -1) / sqrt 2, so
// its offset -48 / sqrt 2; b (along y) is u + w = 3: x + z = 57, (-1, 0, -1) / sqrt 2,
// -57 / sqrt 2; c (along z) is u + v = 4: x + y = 66, (-1, -1, 0) / sqrt 2, -66 / sqrt 2.
// The crossing on the face of a and b (w = 0) is X_w = (3, 2, 0), of b and c (u = 0)
// X_u = (0, 4, 3), of c and a (v = 0) X_v = (4, 0, 2); M = (2.5, 1.5, 0.5), and the
// facet's normal is (10, 9, 4) / sqrt 197 in x, y, z (`unequal_chamfers_at_a_corner_*`).
#[test]
fn a_chamfer_corner_keeps_its_definition() {
    let r2 = std::f64::consts::SQRT_2;
    let chamfers = [
        (2.0, Vec3::new(0.0, -1.0, -1.0) / r2, -48.0 / r2),
        (3.0, Vec3::new(-1.0, 0.0, -1.0) / r2, -57.0 / r2),
        (4.0, Vec3::new(-1.0, -1.0, 0.0) / r2, -66.0 / r2),
    ];
    let f = box_corner();
    for corner in [BlendChamferCorner::Mitre, BlendChamferCorner::Facet] {
        let mut k = chamfer_fixes(true);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let edges: Vec<EdgeId> = BOX_CORNER_EDGES.iter().map(|&p| pick(&k, &b, p)).collect();
        let def = {
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.prepare_per_edge_constant_radius(
                &mut k,
                &edges,
                &[2.0, 3.0, 4.0],
                &[FilletSurfaceGeneratorType::Linear; 3],
                1,
                1.0,
            )
            .unwrap();
            s.set_chamfer_corner(corner);
            s.do_fillet(&mut k).unwrap();
            let mut found = Vec::new();
            for c in s.fillet_corners().unwrap() {
                if let Some(event) = s.corner(c).unwrap().chamfer_corner.as_ref() {
                    found.push(event.definition.clone());
                }
            }
            assert_eq!(found.len(), 1, "one chamfer corner");
            found.pop().unwrap()
        };
        eprintln!("{corner:?}: {def:?}");
        assert_eq!(def.kind, corner);
        assert!(def.vertex.distance(f.o) <= EXACT * 40.0);
        let at = |e: EdgeId| edges.iter().position(|&x| x == e).unwrap();
        assert_eq!(def.chamfers.len(), 3);
        for c in &def.chamfers {
            let (size, normal, offset) = chamfers[at(c.edge)];
            let ChamferWidth::Offsets { prev, next } = c.width else {
                panic!("{:?}", c.width);
            };
            assert!((prev - size).abs() <= 1e-15 * size && (next - size).abs() <= 1e-15 * size);
            assert!((c.normal - normal).length() <= 1e-15, "{:?}", c.normal);
            assert!((c.offset - offset).abs() <= EXACT * 40.0, "{}", c.offset);
        }
        // X_i is on the face between chamfers i - 1 and i
        for i in 0..3 {
            let pair = (at(def.chamfers[(i + 2) % 3].edge), at(def.chamfers[i].edge));
            let want = match (pair.0.min(pair.1), pair.0.max(pair.1)) {
                (0, 1) => f.at(3.0, 2.0, 0.0),
                (1, 2) => f.at(0.0, 4.0, 3.0),
                _ => f.at(4.0, 0.0, 2.0),
            };
            assert!(
                def.crossings[i].distance(want) <= EXACT * 40.0,
                "{i}: {:?}",
                def.crossings[i]
            );
        }
        if corner == BlendChamferCorner::Mitre {
            assert!(def.mitre.unwrap().distance(f.at(2.5, 1.5, 0.5)) <= EXACT * 40.0);
            assert_eq!(def.facet_normal, None);
        } else {
            assert_eq!(def.mitre, None);
            let n = def.facet_normal.unwrap();
            assert!((n + f.dir(10.0, 9.0, 4.0)).length() <= 1e-15, "{n:?}");
        }
    }
}

// A solid from planar polygons (each a closed loop of corner points), stitched and
// oriented, as `SmApiCreatePyramid` builds its faces.
fn polyhedron(k: &mut Kernel, faces: &[Vec<Point3>]) -> Brep {
    let mut brep = solidcore::topology::brep::p3::new_brep(k, -1.0, -1.0).unwrap();
    let region = brep.infinite_region();
    let mut state = ArenaNotifyState::new(&brep);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face =
        |f| solidcore::topology::loopuse::notify_ref(solidcore::error::EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    for corners in faces {
        let curves: Vec<Curve> = (0..corners.len())
            .map(|i| {
                let next = corners[(i + 1) % corners.len()];
                Curve::from(Line::create_line_segment(3, corners[i], next, None).unwrap())
            })
            .collect();
        create_planar_face_with_3d_curves(k, &mut brep, region, &curves, 1.0e-5, &mut fenv)
            .unwrap()
            .unwrap();
    }
    stitch_and_orient(k, &mut brep, &StitchAndOrientOptions::default()).unwrap();
    brep.refresh_cached_topology();
    brep
}

// The unit vectors u_k at 0, 120 and 240 degrees in z = 0.
fn apex_directions() -> [Vec3; 3] {
    [0.0_f64, 120.0, 240.0].map(|a| Vec3::new(a.to_radians().cos(), a.to_radians().sin(), 0.0))
}

// A low apex of three faces at V = 0 on a triangular prism, and its three ridges. The faces
// are z = -eps u_k . (x, y); the ridge between the two faces other than face l runs along
// (-u_l, -eps/2) (there u_k . p = 1/2 for both, against -1 for u_l) to
// T_l = (-20 u_l, -10 eps); walls drop from the ridges' ends to the base z = -10.
//
// Across each ridge the faces turn by 2 beta, beta the angle between a face's normal
// (eps u_k, 1) / sqrt(1 + eps^2) and the chamfer's, which by symmetry is in the ridge's
// vertical plane and square to it, (-(eps/2) u_l, 1) / N with N = sqrt(1 + eps^2 / 4):
// cos beta = N / sqrt(1 + eps^2), so sin beta = (sqrt 3 / 2) eps / sqrt(1 + eps^2) and
// tan beta = (sqrt 3 / 2) eps / N. A ball of offsets r touching both faces sets back
// s = r tan beta and its chamfer plane lies delta = s sin beta below the ridge. By symmetry
// M = (0, 0, z_M), on the chamfer: z_M / N = -delta, so
// z_M = -(3/4) r eps^2 / sqrt(1 + eps^2); and X_k = (rho u_k, -eps rho) on face k, on the
// chamfer of ridge l != k: (-(eps/2) (u_l . u_k) rho - eps rho) / N = -(3/4) eps rho / N
// = -delta, so rho = r eps / sqrt(1 + eps^2).
//
// Rounding (`solve3_error`, u = 8 EPSILON): the chamfer normals' cofactors are each
// (sqrt 3 / 2) eps sqrt(1 + eps^2 / 16) / N^2 and their determinant
// (3 sqrt 3 / 8) eps^2 / N^3, so M's bound is u (s + |M|) 4 N sqrt(1 + eps^2 / 16) / eps.
// X_k's rows (face k and its two chamfers) are, to first order in eps, (eps w, -1) with
// w = -u_k, u_j / 2, u_l / 2, again an equilateral triangle of side sqrt 3 / 2 as M's
// u_l / 2 are, so the same 4 / eps, with |X_k| = rho sqrt(1 + eps^2) in place of |M|. The
// gate is 1e-13 (1 + s) (V = 0).
fn low_apex(eps: f64) -> (Kernel, Brep, Vec<EdgeId>) {
    let mut k = chamfer_fixes(true);
    let u = apex_directions();
    let top = u.map(|ul| Point3::new(-20.0 * ul.x, -20.0 * ul.y, -10.0 * eps));
    let base = u.map(|ul| Point3::new(-20.0 * ul.x, -20.0 * ul.y, -10.0));
    let apex = Point3::new(0.0, 0.0, 0.0);
    let mut faces = Vec::new();
    for kk in 0..3 {
        let (j, l) = ((kk + 1) % 3, (kk + 2) % 3);
        faces.push(vec![apex, top[j], top[l]]);
        faces.push(vec![top[j], top[l], base[l], base[j]]);
    }
    faces.push(base.to_vec());
    let b = polyhedron(&mut k, &faces);
    let edges = u
        .iter()
        .map(|ul| pick(&k, &b, [-10.0 * ul.x, -10.0 * ul.y, -5.0 * eps]))
        .collect();
    (k, b, edges)
}

// The kernel blends no edge whose faces turn by less than half a degree (`GetBlendEdgeuse`,
// with `fillet_half_degree_crease`; 2 degrees without), and here the faces turn by about
// sqrt 3 eps, so the flattest apex that can be chamfered is at eps = 5.0e-3. At
// eps = 1/100 (0.99 degrees), r = 400: s = 2 sqrt 3 / N = 3.46, rho = 4 / sqrt(1 + eps^2),
// |z_M| = 0.03. M's bound is 8 EPSILON (3.46 + 0.03)(400) = 2.5e-12 and X's
// 8 EPSILON (3.46 + 4)(400) = 5.3e-12, against the gate 1e-13 (4.46) = 4.5e-13: the points
// are not determined to the part's rounding, so the corner is refused, naming that. The
// existing checks pass it: the faces' normals' determinant,
// (3 sqrt 3 / 2) eps^2 / (1 + eps^2)^(3/2) = 2.6e-4, is far above the 1e-6 at which the
// event declines; M is inside every face by |z_M| / sqrt(1 + eps^2) = 0.03, and each X_k is
// on the kept side of the third chamfer by (3/2) eps rho / N - delta = delta = 0.03.
#[test]
fn a_nearly_flat_apex_is_refused() {
    for corner in [BlendChamferCorner::Mitre, BlendChamferCorner::Facet] {
        let (mut k, mut b, edges) = low_apex(1.0 / 100.0);
        let refused = chamfer(&mut k, &mut b, &edges, &[400.0; 3], corner).unwrap_err();
        eprintln!("{corner:?}: {refused:?}");
        assert_eq!(refused.code, 9, "CornerUnsupported");
        assert!(
            refused.comment.contains("nearly coplanar"),
            "{}",
            refused.comment
        );
        assert!(refused.fit.is_none(), "{:?}", refused.fit);
    }
}

// eps = 5/12, r = 13/5: sqrt(1 + eps^2) = 13/12, so rho = 1, X_k = (u_k, -5/12) and
// z_M = -(3/4)(13/5)(25/144)(12/13) = -5/16; s = 13 sqrt 3 / sqrt 601 = 0.918. The bounds
// are about 8 EPSILON (0.918 + 13/12)(48/5) = 3.4e-14 for X and 8 EPSILON
// (0.918 + 5/16)(48/5) = 2.1e-14 for M (4 / eps = 48/5, to first order), under the gate
// 1e-13 (1.918) = 1.9e-13: the mitre builds, M at its closed form, the mitres X_k-M exact
// lines. Faces 7 + 3; vertices 7 less the apex, plus the crossings and M, and at each T_l
// (where the chamfer ends across the two walls) T_l traded for its rails' ends on the top
// edges and the chamfer's crossing of the upright: 16; edges 12 less the ridges, plus 6
// rails, 3 mitres and 2 end edges at each T_l: 24.
#[test]
fn a_moderately_flat_apex_is_an_exact_mitre() {
    let (mut k, mut b, edges) = low_apex(5.0 / 12.0);
    chamfer(
        &mut k,
        &mut b,
        &edges,
        &[13.0 / 5.0; 3],
        BlendChamferCorner::Mitre,
    )
    .unwrap();
    let n = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let xs = apex_directions().map(|uk| Point3::new(uk.x, uk.y, -5.0 / 12.0));
    let m = Point3::new(0.0, 0.0, -5.0 / 16.0);
    let off = mitre_off(&k, &b, m, &xs);
    eprintln!("flat apex mitre: {n:?}; off {off:.3e}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 24, 16));
    assert!(off <= EXACT * 20.0, "{off}");
}
