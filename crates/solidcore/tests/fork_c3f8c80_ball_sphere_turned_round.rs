//! The fork's capi `ball sphere_turned_round` (tangency-labs/smlib@c3f8c80, as v1.1.0-alpha
//! leaves it): the box corner's Ball (WR-697) turned
//! round by `SmFace::SwapUV`, as `SmBrep::OrientTrimmedSurfaces` turns a face, then the body
//! oriented outward again. The corner's face stays the ball patch it was, on the sphere about
//! (37, 27, 17) of radius 3, bounded on it, G1 to the three rounds, and the body sound at the
//! box corner's closed-form volume (`wr697_ball_corner.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{orient_trimmed_surfaces, with_kernel_callbacks};
use solidcore::topology::edgeuse::p1::{primary_edgeuse, radial};
use solidcore::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::topology_traverser::{face_edges, Marks};
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

/// The box corner's closed form (`wr697_ball_corner.rs`): the spandrel r^2 (1 - pi/4) of
/// each round along its edge outside the cube of side r at the corner, and the cube less
/// the ball's octant, r^3 (1 - pi/6).
fn box_volume(rad: f64) -> f64 {
    let spandrel = rad * rad * (1.0 - PI / 4.0);
    24000.0 - spandrel * (90.0 - 3.0 * rad) - rad * rad * rad * (1.0 - PI / 6.0)
}

/// The corner's face of `b`: the one face whose three vertices are `v` (within 1e-9).
fn corner_face(b: &Brep, v: &[Point3; 3]) -> FaceId {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let mut pts: Vec<Point3> = Vec::new();
        for e in face_edges(b, f).unwrap() {
            let v0 = edge::vertex(b, e).unwrap();
            for vv in [v0, edge::other_vertex(b, e, v0).unwrap()]
                .into_iter()
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
    assert_eq!(out.len(), 1, "one corner face");
    out[0]
}

/// The worst deviations of face `f` from the sphere about `c` of radius `rad` (its surface
/// on a 17 x 17 grid of its natural domain, its edges at 33 stations) and of its surface
/// normal from the outward radial (inside the grid: the pole's side has no one normal);
/// `f` is a ball patch carrying that sphere.
fn on_the_ball(k: &Kernel, b: &Brep, f: FaceId, c: Point3, rad: f64) -> (f64, f64, f64) {
    let s = face2::surface(b, f).unwrap().unwrap();
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
                continue;
            }
            let n = s
                .evaluate_normal(k, uv, Side::Right, Side::Right)
                .unwrap()
                .unitize()
                .unwrap();
            normal_off = normal_off.max((n - (p - c).unitize().unwrap()).length());
        }
    }
    for e in face_edges(b, f).unwrap() {
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

/// As the capi's `worst_seam_angle`: the largest angle (radians) between `f` and its
/// neighbour across each of its edges that is not a point, at 31 stations inside the edge,
/// from the edge uses' binormals (into each face, square to the edge; a tangent seam reads
/// 0), and the number of edges measured.
fn worst_seam_angle(k: &Kernel, b: &mut Brep, f: FaceId) -> (f64, usize) {
    let mut worst = 0.0f64;
    let mut seams = 0;
    for e in face_edges(b, f).unwrap() {
        if edge::curve(b, e).unwrap().is_none()
            || edge::start_vertex(b, e).unwrap() == edge::end_vertex(b, e).unwrap()
        {
            continue;
        }
        let a = primary_edgeuse(b, e).unwrap().unwrap();
        let o = radial(b, a).unwrap().unwrap();
        assert_ne!(a, o, "an edge of the corner face with one face");
        seams += 1;
        let ivl = edge::interval(b, e).unwrap();
        let opts = BinormalOptions::default();
        for st in 1..32 {
            let t = ivl.evaluate(f64::from(st) / 32.0);
            let ba = evaluate_binormal(k, b, a, t, false, &opts)
                .unwrap()
                .binormal
                .unitize()
                .unwrap();
            let bb = evaluate_binormal(k, b, o, t, false, &opts)
                .unwrap()
                .binormal
                .unitize()
                .unwrap();
            // tangent faces' binormals are opposed; the angle from the cross product,
            // which resolves it below acos's 1e-8
            worst = worst.max(ba.cross(bb).length().atan2(-ba.dot(bb)));
        }
    }
    (worst, seams)
}

#[test]
fn ball_patch_turned_round_and_oriented_back_stays_on_the_ball() {
    let rad = 3.0;
    let mut k = all_fixes();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    // capi `ball box_corner`: the three edges at the corner (40, 30, 20) rounded r 3 in one
    // call, the ball about (37, 27, 17)
    let edges: Vec<EdgeId> = [[20.0, 30.0, 20.0], [40.0, 15.0, 20.0], [40.0, 30.0, 10.0]]
        .iter()
        .map(|&p| pick(&k, &b, p))
        .collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; 3];
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(&mut k, &edges, &[rad; 3], &xs, 1, 1.0)
            .unwrap();
        s.do_fillet(&mut k).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let c = Point3::new(37.0, 27.0, 17.0);
    let vs = [
        Point3::new(40.0, 27.0, 17.0),
        Point3::new(37.0, 30.0, 17.0),
        Point3::new(37.0, 27.0, 20.0),
    ];

    let f = corner_face(&b, &vs);
    let (_, _, before) = on_the_ball(&k, &b, f, c, rad);
    solidcore::topology::face::p1::swap_uv(&k, &mut b, f).unwrap();
    let (_, _, swapped) = on_the_ball(&k, &b, f, c, rad);
    let oriented = with_kernel_callbacks(&mut k, &mut b, |k, b, env| {
        let mut marks = Marks::default();
        orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)
    })
    .unwrap();

    let v = volume(&k, &mut b);
    let want = box_volume(rad);
    let n = counts(&b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let f = corner_face(&b, &vs);
    let (s, e, nrm) = on_the_ball(&k, &b, f, c, rad);
    let (angle, seams) = worst_seam_angle(&k, &mut b, f);
    eprintln!(
        "normal off outward: before {before:.3e}, swapped {swapped:.3e}; oriented {oriented:?}; \
         volume {v:.12} want {want:.12} ({:.3e} rel); counts {n:?}; off {s:.3e} {e:.3e} {nrm:.3e}; \
         {seams} seams, worst {angle:.3e} rad; {lines:?}",
        (v - want).abs() / want
    );
    // built outward, the swap turns the patch into the body (its normal the inward radial,
    // 2 off the outward one), and the orientation turns it out again
    assert!(before <= 1e-9, "{before}");
    assert!((swapped - 2.0).abs() <= 1e-9, "{swapped}");
    assert!(!oriented.maybe_not_closed_solid);
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (10, 21, 13));
    assert!(s <= 1e-12 && e <= 1e-12 && nrm <= 1e-9, "{s} {e} {nrm}");
    assert!(seams == 3 && angle <= 1e-9, "{seams} {angle}");
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}
