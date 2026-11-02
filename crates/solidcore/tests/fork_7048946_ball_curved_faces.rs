//! smlib#110 (the fork's tangency-labs/smlib@7048946, cases 9a5a8ce and 3969288): three
//! equal convex or concave rounds at a vertex of planes, cylinders, cones and spheres end
//! on the one ball touching the three faces, about the faces' triple offset point, and the
//! corner's face is that ball's triangle (`Kernel::fixes.fillet_blend_corner`, WR-697).
//! Before 7048946 `ClassifyBall` took only planes, and these corners were the old convex
//! corner's patch, off the sphere.
//!
//! The cases are the fork's capi `ball` cases `d_prism_corners`, `d_pocket_corners` and
//! `cone_frustum_corners` (`capi/tests/ops_kernel.cpp`), built from the same curves the capi
//! builds (an `SmCircle` arc in the frame of its start, swept). Values are checked against
//! closed forms: the ball centres, and Steiner's volume for the D prism (the rounded body is
//! the inner body grown by r) and the D pocket's void. The fork built locally at dda881f
//! (from SMLib calls, not the capi) gives the same counts, every ball patch on its sphere to
//! 1.2e-14, and volumes 2247.51814446744 (prism), 30426.3156673132 (pocket) and
//! 2134.91230051228 (frustum) by `SmApiBrepComputeVolume` at 1e-12.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cone, create_cylinder};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, EdgeId, VertexId};
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
fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], r: f64) {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![r; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
            .unwrap();
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            panic!("fillet failed: {} {}", info.code, info.comment);
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
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

/// For each centre, the least worst deviation from its sphere of radius `rad` of any rational
/// B-spline face of `b` (sampled 17 x 17 over its natural domain).
fn sphere_offs(k: &Kernel, b: &Brep, centres: &[Point3], rad: f64) -> Vec<f64> {
    let mut best = vec![f64::INFINITY; centres.len()];
    for f in get_faces(b).unwrap() {
        let s = face2::surface(b, f).unwrap().unwrap();
        let net = match s.kind() {
            SurfaceKind::BSpline(net) => net,
            SurfaceKind::BallPatch(patch) => patch.net(),
            _ => continue,
        };
        if !net.is_rational() {
            continue;
        }
        let dom = s.natural_uv_domain_with_nurb(k).unwrap();
        for (c, bst) in centres.iter().zip(best.iter_mut()) {
            let mut off = 0.0f64;
            for i in 0..=16 {
                for j in 0..=16 {
                    let uv = dom.evaluate(f64::from(i) / 16.0, f64::from(j) / 16.0);
                    let p = s.evaluate_point(k, uv).unwrap();
                    off = off.max(((p - *c).length() - rad).abs());
                }
            }
            *bst = bst.min(off);
        }
    }
    best
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(create_line_segment(a, b).unwrap())
}

/// The capi's arc about `c` in z = c.z, radius `r`, from angle `a0` to `a1` counterclockwise:
/// an `SmCircle` in the frame of its start (capi `smlib_sweeps.cpp`'s arc).
fn arc(k: &Kernel, c: Point3, r: f64, a0: f64, a1: f64) -> Curve {
    let axis = Vec3::Z;
    let reference = Vec3::X;
    let y = axis.cross(reference);
    let x0 = reference * a0.cos() + y * a0.sin();
    let y0 = axis.cross(x0);
    let frame = Axis2Placement::new(c, x0, y0);
    let interval = Extent1d::from_min_max(0.0, (a1 - a0) * 180.0 / PI);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&interval), None).unwrap())
}

/// capi `d_prism`: the disc of radius `rr` about (cx, cy) less x > cx + a, from z0 to z0 + h.
fn d_prism(k: &mut Kernel, cx: f64, cy: f64, rr: f64, a: f64, z0: f64, height: f64) -> Brep {
    let al = (a / rr).acos();
    let h = (rr * rr - a * a).sqrt();
    let l = line(
        Point3::new(cx + a, cy - h, z0),
        Point3::new(cx + a, cy + h, z0),
    );
    let c = arc(k, Point3::new(cx, cy, z0), rr, al, 2.0 * PI - al);
    create_linear_sweep(k, &[Some(&l), Some(&c)], Vec3::Z, height, true).unwrap()
}

/// The D section of radius `rp` and flat `ap`: its area and perimeter.
fn d_section(rp: f64, ap: f64) -> (f64, f64) {
    let hp = (rp * rp - ap * ap).sqrt();
    let al = (ap / rp).acos();
    (
        rp * rp * (PI - al) + ap * hp,
        2.0 * rp * (PI - al) + 2.0 * hp,
    )
}

/// The D prism (R 10, flat 4, H 10), every edge r 2: four Balls on the convex cylinder.
fn d_prism_rounded(blend: bool) -> (Kernel, Brep) {
    let mut k = all_fixes(blend);
    let (rr, a, height, rad): (f64, f64, f64, f64) = (10.0, 4.0, 10.0, 2.0);
    let h = (rr * rr - a * a).sqrt();
    let mut b = d_prism(&mut k, 0.0, 0.0, rr, a, 0.0, height);
    round(
        &mut k,
        &mut b,
        &[
            [a, 0.0, height],
            [a, 0.0, 0.0],
            [a, h, 5.0],
            [a, -h, 5.0],
            [-rr, 0.0, height],
            [-rr, 0.0, 0.0],
        ],
        rad,
    );
    (k, b)
}

/// The D prism's closed forms: Steiner's volume and the four ball centres.
fn d_prism_want() -> (f64, Vec<Point3>) {
    let (rr, a, height, rad): (f64, f64, f64, f64) = (10.0, 4.0, 10.0, 2.0);
    let (rp, ap, hh) = (rr - rad, a - rad, height - 2.0 * rad);
    let hp = (rp * rp - ap * ap).sqrt();
    let (area, perim) = d_section(rp, ap);
    let want = area * hh
        + (2.0 * area + perim * hh) * rad
        + (PI * hh + 0.5 * PI * perim) * rad * rad
        + 4.0 / 3.0 * PI * rad * rad * rad;
    let mut centres = Vec::new();
    for sy in [1.0, -1.0] {
        for zc in [height - rad, rad] {
            centres.push(Point3::new(ap, sy * hp, zc));
        }
    }
    (want, centres)
}

/// capi `ball d_prism_corners`: 14 faces, 24 edges, 12 vertices; each corner the ball about
/// (a - r, +-h', H - r or r); the volume Steiner's to 1e-11.
#[test]
fn d_prism_corners_are_balls_on_the_cylinder() {
    let (mut k, mut b) = d_prism_rounded(true);
    let (want, centres) = d_prism_want();
    let v = volume(&k, &mut b);
    let n = counts(&b);
    let offs = sphere_offs(&k, &b, &centres, 2.0);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    eprintln!("d_prism: volume {v:.12} want {want:.12}; counts {n:?}; offs {offs:?}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (14, 24, 12));
    assert!(offs.iter().all(|&o| o <= 1e-12), "{offs:?}");
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}

/// Without the flag the corners are the pinned corner's, not on the ball (fork 9a5a8ce:
/// "the old convex corner's patch", the volume 2.2e-10 off Steiner's there).
#[test]
fn without_the_flag_the_d_prism_corners_are_no_balls() {
    let (k, b) = d_prism_rounded(false);
    let (_, centres) = d_prism_want();
    let offs = sphere_offs(&k, &b, &centres, 2.0);
    eprintln!("d_prism without the flag: offs {offs:?}");
    assert!(offs.iter().all(|&o| o > 1e-12), "{offs:?}");
}

/// capi `ball d_pocket_corners`: a 40 x 40 x 20 block less the D prism (R 8 about (20, 20),
/// flat x = 24, floor z = 10); the floor's chord and arc and the two uprights r 2. The two
/// floor corners are Balls on the concave cylinder about (24 - r, 20 +- h', 10 + r); 15
/// faces, 30 edges, 18 vertices; the volume the block less the opening of the D prism.
#[test]
fn d_pocket_corners_are_balls_on_the_concave_cylinder() {
    let mut k = all_fixes(true);
    let (rr, a, rad, floor, top): (f64, f64, f64, f64, f64) = (8.0, 4.0, 2.0, 10.0, 20.0);
    let h = (rr * rr - a * a).sqrt();
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let cut = d_prism(&mut k, 20.0, 20.0, rr, a, floor, 11.0);
    let mut b = boolean_difference(&mut k, block, cut).unwrap();
    round(
        &mut k,
        &mut b,
        &[
            [24.0, 20.0, 10.0],
            [12.0, 20.0, 10.0],
            [24.0, 20.0 + h, 15.0],
            [24.0, 20.0 - h, 15.0],
        ],
        rad,
    );
    let (rp, ap) = (rr - rad, a - rad);
    let hp = (rp * rp - ap * ap).sqrt();
    let (area, perim) = d_section(rp, ap);
    let void = (area + perim * rad + PI * rad * rad) * (top - floor - rad)
        + area * rad
        + perim * PI * rad * rad / 4.0
        + 2.0 * PI * rad * rad * rad / 3.0;
    let want = 40.0 * 40.0 * 20.0 - void;
    let centres = [
        Point3::new(20.0 + ap, 20.0 + hp, floor + rad),
        Point3::new(20.0 + ap, 20.0 - hp, floor + rad),
    ];
    let v = volume(&k, &mut b);
    let n = counts(&b);
    let offs = sphere_offs(&k, &b, &centres, rad);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    eprintln!("d_pocket: volume {v:.12} want {want:.12}; counts {n:?}; offs {offs:?}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (15, 30, 18));
    assert!(offs.iter().all(|&o| o <= 1e-12), "{offs:?}");
    assert!((v - want).abs() <= 1e-11 * want, "{v} {want}");
}

/// capi `ball cone_frustum_corners`: a cylinder (R 10, z 0 to 6) under a cone's frustum
/// (narrowing at 30 degrees to z 10), less x > 4, every edge r 1.5. Six Balls: two with two
/// planes (the closed forms), and the flat, the cylinder and the cone, one plane, whose centre
/// has no closed form in the build and is certified (Krawczyk): (a', +-sqrt(Rc^2 - a'^2), z*),
/// z* = 6 - r tan 15 deg. 20 faces, 36 edges, 18 vertices. (Its volume is the fork's
/// `cone_frustum_volume`, known failing there, smlib#103: not checked.)
#[test]
fn cone_frustum_corners_are_balls_some_certified() {
    let mut k = all_fixes(true);
    let (rr, z1, height, a, rad): (f64, f64, f64, f64, f64) = (10.0, 6.0, 10.0, 4.0, 1.5);
    let beta: f64 = PI / 6.0;
    let (tb, cb) = (beta.tan(), beta.cos());
    let (ap, rc) = (a - rad, rr - rad);
    let zs = z1 - rad * (beta / 2.0).tan();
    let zt = height - rad;
    let rho = |z: f64| {
        if z <= zs {
            rc
        } else {
            rr - (z - z1) * tb - rad / cb
        }
    };
    let cyl = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), rr, z1).unwrap();
    let cone = create_cone(
        &mut k,
        Point3::new(0.0, 0.0, z1),
        rr,
        rr - (height - z1) * tb,
        height - z1,
    )
    .unwrap();
    let u = boolean_union(&mut k, cyl, cone).unwrap();
    let bx = create_box(&mut k, Point3::new(a, -20.0, -1.0), 20.0, 40.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, u, bx).unwrap();
    assert_eq!(counts(&b), (5, 9, 6));
    let h0 = (rr * rr - a * a).sqrt();
    let zc = 8.0;
    let hc = ((rr - 2.0 * tb).powi(2) - a * a).sqrt();
    round(
        &mut k,
        &mut b,
        &[
            [-rr, 0.0, 0.0],
            [a, 0.0, 0.0],
            [a, h0, 3.0],
            [a, -h0, 3.0],
            [-rr, 0.0, z1],
            [a, hc, zc],
            [a, -hc, zc],
            [-(rr - 4.0 * tb), 0.0, height],
            [a, 0.0, height],
        ],
        rad,
    );
    let mut centres = Vec::new();
    for sy in [1.0, -1.0] {
        for z in [rad, zs, zt] {
            let p = rho(z);
            centres.push(Point3::new(ap, sy * (p * p - ap * ap).sqrt(), z));
        }
    }
    let v = volume(&k, &mut b);
    let n = counts(&b);
    let offs = sphere_offs(&k, &b, &centres, rad);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    eprintln!("cone_frustum: volume {v:.12}; counts {n:?}; offs {offs:?}; {lines:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(n, (20, 36, 18));
    assert!(offs.iter().all(|&o| o <= 1e-12), "{offs:?}");
}
