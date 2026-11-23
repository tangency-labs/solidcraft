//! #2933 (WR-445): a corner setback is measured from the set-back vertex on every edge, with
//! the default kernel. A box 60 x 40 x 20 with r5 rounds on the three edges at (60, 40, 20)
//! and that corner set back 8, as the goldens' `real-box-high-corner-setback-eight` builds it.
//! Each round ends on its section 8 from the vertex, so the six section ends lie on the
//! setback planes x = 52, y = 32 and z = 12, and the body is convex: its volume lies in the
//! bounds the kernel case `ladder/07-fillets/013-corner-setback` derives in closed form.
//! The corner's patch (its six faces in the setback cube) meets each round and each box face
//! tangentially along every one of its edges, and every edge lies within the kernel's 4e-5 of
//! its faces.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::Point3;
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{
    brep_bounding_box, brep_is_manifold_solid, face_bounding_box, surface_closest_point,
};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, vertex, Brep, FaceId};
use solidcore::Kernel;

const CORNER: [f64; 3] = [60.0, 40.0, 20.0];

/// The box with its far corner set back 8, built with the default kernel.
fn build() -> (Kernel, Brep) {
    build_with(Kernel::default())
}

/// The box with its far corner set back 8, built with `k`.
fn build_with(mut k: Kernel) -> (Kernel, Brep) {
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let corner = Point3::new(CORNER[0], CORNER[1], CORNER[2]);
    let v = b
        .vertices()
        .ids()
        .find(|&v| vertex::point(&b, v).unwrap() == corner)
        .unwrap();
    let edges: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            s == Some(v) || t == Some(v)
        })
        .collect();
    assert_eq!(edges.len(), 3);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_constant_radius_edges(&mut k, &edges, 5.0, 1, 1, 1.0)
            .unwrap();
        // set back (`AddSetBackVertex`), so the corner is the Setback, not the Ball
        s.add_set_back_vertex(v);
        s.create_corners(&k, None).unwrap();
        let corners: Vec<_> = s
            .fillet_corners()
            .unwrap()
            .into_iter()
            .filter(|&id| s.corner(id).unwrap().filleted_vertex() == Some(v))
            .collect();
        assert_eq!(corners.len(), 1);
        s.set_corner_setback(corners[0], 8.0).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    (k, b)
}

#[test]
fn each_round_ends_eight_from_the_vertex() {
    let (_, b) = build();
    // the rounds' sections at the setback planes: x = 52 (the x edge), y = 32, z = 12
    let ends = [
        [52.0, 35.0, 20.0],
        [52.0, 40.0, 15.0],
        [55.0, 32.0, 20.0],
        [60.0, 32.0, 15.0],
        [55.0, 40.0, 12.0],
        [60.0, 35.0, 12.0],
    ];
    let points: Vec<Point3> = b
        .vertices()
        .ids()
        .map(|v| vertex::point(&b, v).unwrap())
        .collect();
    for end in ends {
        let want = Point3::new(end[0], end[1], end[2]);
        let nearest = points
            .iter()
            .map(|p| p.distance(want))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1e-6, "no vertex at {end:?}: nearest {nearest}");
    }
}

#[test]
fn the_set_back_box_is_convex_and_inside_its_bounds() {
    let (k, mut b) = build();
    assert!(brep_is_manifold_solid(&b).unwrap());
    let x = brep_bounding_box(&k, &b, true).unwrap();
    for (i, limit) in CORNER.iter().enumerate() {
        assert!(x.max_at(i) <= limit + 1e-6, "{i}: {}", x.max_at(i));
    }
    // 013-corner-setback's closed-form bounds: the hull of the sections in the corner cube
    // below, the three rounds' prisms above
    let volume = brep_volume_default(&k, &mut b, 1e-6).unwrap();
    assert!(
        (47_271.975_7..=47_384.893_5).contains(&volume),
        "volume {volume}"
    );
}

/// The corner patch's faces: those lying in the setback cube [52, 60] x [32, 40] x [12, 20]
/// at the corner (a patch running past the setback leaves it, as the fold before #2933's
/// part 1 did, and is not counted).
fn patch_faces(k: &Kernel, b: &Brep) -> Vec<FaceId> {
    let lo = [CORNER[0] - 8.0, CORNER[1] - 8.0, CORNER[2] - 8.0];
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let x = face_bounding_box(k, b, f, true).unwrap();
            (0..3).all(|i| x.min_at(i) >= lo[i] - 1e-6 && x.max_at(i) <= CORNER[i] + 1e-6)
        })
        .collect()
}

/// The largest angle (degrees) between the normals of an edge's two faces, sampled along
/// it, over every edge of a face in `patch` (the patch's inner seams, its seams with the
/// rounds and its spring curves on the box's faces), whatever its angle: a fold there counts.
fn max_seam_angle(k: &Kernel, b: &Brep, patch: &[FaceId]) -> f64 {
    let mut worst: f64 = 0.0;
    for e in b.edges().ids() {
        let faces = edge::faces(b, e, None).unwrap();
        if !faces.iter().any(|f| patch.contains(f)) {
            continue;
        }
        let [f1, f2] = faces[..] else {
            panic!("patch edge {e:?} has {} faces", faces.len())
        };
        let (s1, s2) = (
            face::surface(b, f1).unwrap().unwrap(),
            face::surface(b, f2).unwrap().unwrap(),
        );
        let c = edge::curve(b, e).unwrap().unwrap();
        let iv = edge::interval(b, e).unwrap();
        for i in 0..=20 {
            let p = c
                .evaluate_point(k, iv.min() + iv.length() * f64::from(i) / 20.0)
                .unwrap();
            let normal = |s: &Surface| {
                let uv = surface_closest_point(k, s, p).unwrap().uvs[0];
                s.evaluate_normal(k, uv, Side::Left, Side::Left).unwrap()
            };
            let (n1, n2) = (normal(s1), normal(s2));
            worst = worst.max(n1.cross(n2).length().atan2(n1.dot(n2).abs()).to_degrees());
        }
    }
    worst
}

#[test]
fn the_patch_meets_the_rounds_tangentially_within_tolerance() {
    let (k, mut b) = build();
    b.refresh_cached_topology();
    assert!(solidcore::topology::integrity::check(&b).is_ok());
    // the fill's six patches, two per round, all inside the setback cube
    let patch = patch_faces(&k, &b);
    assert_eq!(patch.len(), 6, "{patch:?}");
    let seam = max_seam_angle(&k, &b, &patch);
    assert!(seam < 0.01, "seam {seam} degrees");
    let tol = b
        .edges()
        .ids()
        .map(|e| edge::tolerance(&b, e).unwrap())
        .fold(0.0, f64::max);
    assert!(tol <= 4.0e-5 * (1.0 + 1e-9), "edge tolerance {tol}");
}

/// With the rounds as their definitions, the six patch faces
/// are the Setback's fill: each keeps the Setback's definition (`procedural_blend`), which the
/// N x N patch has not.
#[test]
fn with_round_definitions_the_six_patches_are_the_setback_fill() {
    let k = Kernel::default();
    let (_, b) = build_with(k);
    let setback = b
        .face_list()
        .iter()
        .filter(|&&f| {
            face::surface(&b, f).unwrap().is_some_and(|s| {
                s.procedural_blend()
                    .and_then(|d| d.corner())
                    .is_some_and(|c| c.kind() == CornerKind::Setback)
            })
        })
        .count();
    assert_eq!(setback, 6);
}
