//! tangency-labs/solidcore#1376: `FilletSession::set_corner_ease` (the fork's
//! `SmFilletCorner::SetEaseDist` and `SmFilletExecutive::EaseSetBackSections`,
//! tangency-labs/smlib `e74bbd3`). The corner (40, 40, 40) of a 40 mm box, its three edges
//! rounded r 8 and set back 12 (the fork's `corner_setback_eased` test in
//! `capi/tests/ops_kernel.cpp`, `56f6e81`; SolidCraft ladder 07/015). Over the ease of 4
//! before the setback each round's section eases from the circle into the
//! curvature-continuous quintic: on the top face's contact line of the round along X, at 16
//! and more from the vertex the cross curvature is the circle's 1/8, at 12 (the setback) 0,
//! falling in between, continuous where the ease starts. At the hole's six corners (the
//! sections' ends) the section and the face curve both have zero curvature. An ease of 0 (the
//! default) is today's body exactly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::vector2d::Point2;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, vertex, Brep, FaceId};
use solidcore::{Kernel, VertexId};

const CORNER: [f64; 3] = [40.0, 40.0, 40.0];
const SETBACK: f64 = 12.0;

fn at_point(b: &Brep, v: VertexId, p: [f64; 3]) -> bool {
    let q = vertex::point(b, v).unwrap();
    (q.x - p[0]).abs() + (q.y - p[1]).abs() + (q.z - p[2]).abs() < 1e-6
}

/// What to round and how.
#[derive(Clone, Copy)]
struct Spec {
    /// The corner's ease (`None`: the session's default, never set).
    ease: Option<f64>,
    /// Every edge rounded and every corner set back and eased, else the three at `CORNER`.
    all: bool,
    /// The curvature-continuous blend (`G2` blend curve at thirds), else the round.
    g2_blend: bool,
}

const ROUND: Spec = Spec {
    ease: None,
    all: false,
    g2_blend: false,
};

/// The box with the edges `spec` names rounded r 8 and their corners set back 12 and eased,
/// and the faces of the set-back corners' patches (`set_mark_corner_patches`).
fn build(spec: Spec) -> (Kernel, Brep, Vec<FaceId>) {
    let mut k = Kernel::default();
    k.fixes.fillet_setback_from_start_vertex = true;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 40.0, 40.0).unwrap();
    let mut edges = Vec::new();
    for e in b.edges().ids() {
        let (s, t) = edge::start_end_vertices(&b, e).unwrap();
        if spec.all
            || [s, t]
                .into_iter()
                .flatten()
                .any(|v| at_point(&b, v, CORNER))
        {
            edges.push(e);
        }
    }
    assert_eq!(edges.len(), if spec.all { 12 } else { 3 });
    let patch;
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.set_mark_corner_patches(true);
        let (xsect, continuity) = if spec.g2_blend {
            s.set_blend_curve_at_thirds(true);
            (2, 2)
        } else {
            (1, 1)
        };
        s.prepare_constant_radius_edges(&mut k, &edges, 8.0, xsect, continuity, 1.0)
            .unwrap();
        s.create_corners(&k, None).unwrap();
        let corners = s.fillet_corners().unwrap();
        let found: Vec<_> = corners
            .iter()
            .copied()
            .filter(|&id| {
                spec.all
                    || s.corner(id)
                        .unwrap()
                        .vertex
                        .is_some_and(|v| at_point(s.target(), v, CORNER))
            })
            .collect();
        assert_eq!(found.len(), if spec.all { 8 } else { 1 });
        for &corner in &found {
            s.set_corner_setback(corner, SETBACK).unwrap();
            if let Some(ease) = spec.ease {
                s.set_corner_ease(corner, ease).unwrap();
            }
        }
        s.do_filleting(&mut k, None).unwrap();
        patch = s
            .corner_patch_faces()
            .unwrap()
            .into_iter()
            .map(|(_, f)| f)
            .collect();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    (k, b, patch)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The curvature of `e`'s curve at its end on `v`.
fn end_curvature(k: &Kernel, b: &Brep, e: solidcore::EdgeId, v: VertexId) -> f64 {
    let ivl = edge::interval(b, e).unwrap();
    let t = if edge::start_vertex(b, e).unwrap() == Some(v) {
        ivl.min()
    } else {
        ivl.max()
    };
    let curve = edge::curve(b, e).unwrap().unwrap();
    let d = curve.evaluate(k, t, 2, true, true).unwrap();
    let speed = d[1].length();
    d[1].cross(d[2]).length() / (speed * speed * speed)
}

/// The normal curvature, at the point of `b`'s non-planar faces outside the corner's patch
/// nearest `p` (within 1e-6), of the section square to `along` (unit): the round's cross
/// curvature there.
fn cross_curvature(k: &Kernel, b: &Brep, patch: &[FaceId], p: Point3, along: Vec3) -> f64 {
    for f in p1::faces(b, None).unwrap() {
        if patch.contains(&f) {
            continue;
        }
        let s = b.face(f).unwrap().surface.as_ref().unwrap();
        if matches!(s.kind(), SurfaceKind::Plane(_)) {
            continue;
        }
        let domain = s.natural_uv_domain_with_nurb(k).unwrap();
        let Ok(sols) = s.global_point_solve::<NoObject>(
            k,
            None,
            &domain,
            SolverOperation::Minimize,
            p,
            1e-7,
            None,
            SolutionRequested::Single,
        ) else {
            continue;
        };
        let Some(sol) = sols.get(0) else {
            continue;
        };
        let uv = Point2::new(sol.start.param(0).unwrap(), sol.start.param(1).unwrap());
        let [at, su, sv, suv, suu, svv] = s.evaluate_2nd_derivatives(k, uv, true, true).unwrap();
        if (at - p).length() > 1e-6 {
            continue;
        }
        let mut n = su.cross(sv);
        n = n * (1.0 / n.length());
        let mut t = along.cross(n);
        t = t * (1.0 / t.length());
        let (e, ff, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = e * g - ff * ff;
        let a = (g * t.dot(su) - ff * t.dot(sv)) / det;
        let c = (e * t.dot(sv) - ff * t.dot(su)) / det;
        return (a * a * suu.dot(n) + 2.0 * a * c * suv.dot(n) + c * c * svv.dot(n)).abs();
    }
    f64::INFINITY
}

/// The worst end curvature over the edges at the hole's six corners, and how many edge ends
/// were measured.
fn worst_at_hole_corners(k: &Kernel, b: &Brep) -> (f64, usize) {
    let ends = [
        [28.0, 32.0, 40.0],
        [28.0, 40.0, 32.0],
        [32.0, 28.0, 40.0],
        [40.0, 28.0, 32.0],
        [32.0, 40.0, 28.0],
        [40.0, 32.0, 28.0],
    ];
    let vertices = p1::vertices(b, None).unwrap();
    let (mut worst, mut met) = (0.0_f64, 0);
    for p in ends {
        let v = vertices
            .iter()
            .copied()
            .find(|&v| at_point(b, v, p))
            .expect("a vertex at each of the hole's corners");
        for e in vertex::edges(b, v, None).unwrap() {
            worst = worst.max(end_curvature(k, b, e, v));
            met += 1;
        }
    }
    (worst, met)
}

#[test]
fn ease_zero_is_the_body_without_one() {
    let (k, mut plain, _) = build(ROUND);
    let (k0, mut zero, _) = build(Spec {
        ease: Some(0.0),
        ..ROUND
    });
    assert_eq!(
        p1::faces(&plain, None).unwrap().len(),
        p1::faces(&zero, None).unwrap().len()
    );
    let pv: Vec<Point3> = p1::vertices(&plain, None)
        .unwrap()
        .into_iter()
        .map(|v| vertex::point(&plain, v).unwrap())
        .collect();
    let zv: Vec<Point3> = p1::vertices(&zero, None)
        .unwrap()
        .into_iter()
        .map(|v| vertex::point(&zero, v).unwrap())
        .collect();
    assert_eq!(pv, zv);
    assert_eq!(volume(&k, &mut plain), volume(&k0, &mut zero));
    // without an ease the hole's corners are not flat: the section is the circle there
    let (worst, _) = worst_at_hole_corners(&k, &plain);
    assert!(worst > 0.1, "{worst}");
}

#[test]
fn an_eased_setback_ends_the_rounds_curvature_continuous() {
    let (k, b, patch) = build(Spec {
        ease: Some(4.0),
        ..ROUND
    });
    assert!(is_manifold_solid(&b).unwrap());
    assert!(!patch.is_empty());
    // at each of the hole's six corners a section and a face curve, both flat across there
    let (worst, met) = worst_at_hole_corners(&k, &b);
    eprintln!("issue_1376: worst curvature at the hole's corners {worst:.3e} over {met} edge ends");
    assert!(met >= 12, "{met}");
    assert!(worst <= 1e-6, "{worst}");
    // the round along X, on its top contact line y 32, z 40: the circle's curvature up to
    // the ease, none at the setback, falling between, continuous where the ease starts
    let x = Vec3::new(1.0, 0.0, 0.0);
    let at = [
        12.0,
        13.0,
        14.0,
        15.0,
        16.0 - 1e-4,
        16.0 + 1e-4,
        18.0,
        20.0,
        24.0,
    ];
    let kc: Vec<f64> = at
        .iter()
        .map(|&d| cross_curvature(&k, &b, &patch, Point3::new(40.0 - d, 32.0, 40.0), x))
        .collect();
    eprintln!("issue_1376: cross curvature at 12..24: {kc:?}");
    assert!(kc[0] <= 1e-6, "{kc:?}");
    assert!(
        kc[0] < kc[1] && kc[1] < kc[2] && kc[2] < kc[3] && kc[3] < kc[4],
        "{kc:?}"
    );
    for &c in &kc[4..] {
        assert!((c - 0.125).abs() <= 1e-4, "{kc:?}");
    }
}

#[test]
fn an_eased_curvature_continuous_blend_meets_the_contact_lines_flat() {
    // its sections are the quintic already; eased, its face curves meet the contact lines
    // curvature-continuously
    let (k, b, _) = build(Spec {
        ease: Some(4.0),
        g2_blend: true,
        ..ROUND
    });
    assert!(is_manifold_solid(&b).unwrap());
    let (worst, met) = worst_at_hole_corners(&k, &b);
    eprintln!("issue_1376: G2 blend, worst curvature at the hole's corners {worst:.3e}");
    assert!(met >= 12, "{met}");
    assert!(worst <= 1e-6, "{worst}");
    // not eased, the face curves are the conics: curved where they leave the contact lines
    let (k, b, _) = build(Spec {
        g2_blend: true,
        ..ROUND
    });
    let (worst, _) = worst_at_hole_corners(&k, &b);
    assert!(worst > 0.1, "{worst}");
}

#[test]
fn every_corner_eased_eases_each_round_at_both_ends() {
    // 12 + 3 twice is 30, inside the 40 edge: the fork's test of this box (an ease of 4
    // there does not stitch in the fork either)
    let (k, b, patch) = build(Spec {
        ease: Some(3.0),
        all: true,
        g2_blend: false,
    });
    assert!(is_manifold_solid(&b).unwrap());
    // on the round along X at y 40, z 40, the top contact line: flat across at both
    // setbacks, the circle's curvature in the middle
    let x = Vec3::new(1.0, 0.0, 0.0);
    let k0 = cross_curvature(&k, &b, &patch, Point3::new(12.0, 32.0, 40.0), x);
    let k1 = cross_curvature(&k, &b, &patch, Point3::new(20.0, 32.0, 40.0), x);
    let k2 = cross_curvature(&k, &b, &patch, Point3::new(28.0, 32.0, 40.0), x);
    eprintln!("issue_1376: eased at both ends, cross curvature {k0:.3e} {k1} {k2:.3e}");
    assert!(
        k0 <= 1e-6 && (k1 - 0.125).abs() <= 1e-4 && k2 <= 1e-6,
        "{k0} {k1} {k2}"
    );
}
