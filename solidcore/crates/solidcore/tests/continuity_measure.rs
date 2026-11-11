//! #3142: the continuity measure across an edge (G0 to G4) and the curvature vectors combs are
//! drawn from, against closed forms: a box's right-angled edges, a round between two planes
//! (tangent, its curvature jumping by 1 / r, constant on both sides), and a cylinder's circles
//! and parameter lines.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::f64::consts::FRAC_PI_2;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::brep::evaluate_surface_normal;
use solidcore::query::continuity::{continuity_at, curve_curvature, iso_curvature};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::edge::faces;
use solidcore::topology::face::p2::faceuses;
use solidcore::topology::{EdgeId, FaceId, FaceuseId};
use solidcore::{Brep, Kernel};

/// The use of face `f` whose normal points away from `centre` (the bodies here are convex).
fn outward(k: &Kernel, b: &Brep, f: FaceId, centre: Point3) -> FaceuseId {
    let (Some(x), Some(y)) = faceuses(b, f).unwrap() else {
        panic!("face {f:?} has no two uses");
    };
    let s = b.face(f).unwrap().surface.as_ref().unwrap();
    let mid = s.natural_uv_domain_with_nurb(k).unwrap().evaluate(0.5, 0.5);
    let p = s.evaluate_point(k, mid).unwrap();
    let n = evaluate_surface_normal(k, Some(s), mid).unwrap();
    let same = n.dot(p - centre) > 0.0;
    let ux = b.faceuse(x).unwrap();
    let x_same = matches!(
        ux.orientation,
        solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same
    );
    if same == x_same {
        x
    } else {
        y
    }
}

/// Edge `e`'s two faces' outward uses.
fn uses(k: &Kernel, b: &Brep, e: EdgeId, centre: Point3) -> [FaceuseId; 2] {
    let fs = faces(b, e, None).unwrap();
    assert_eq!(fs.len(), 2, "edge {e:?}");
    [outward(k, b, fs[0], centre), outward(k, b, fs[1], centre)]
}

/// Five stations over the edge's interval, the ends included.
fn stations(b: &Brep, e: EdgeId) -> Vec<f64> {
    let iv = b.edge(e).unwrap().interval;
    (0..=4).map(|i| iv.evaluate(f64::from(i) / 4.0)).collect()
}

/// The edge whose curve passes nearest `p` (at its middle).
fn edge_at(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        let rec = b.edge(e).unwrap();
        let Some(c) = rec.curve.as_ref() else {
            continue;
        };
        let d = c
            .evaluate_point(k, rec.interval.evaluate(0.5))
            .unwrap()
            .distance(p);
        if d < best.0 {
            best = (d, Some(e));
        }
    }
    best.1.unwrap()
}

#[test]
fn a_box_edge_is_a_closed_right_angle_with_flat_faces() {
    let mut k = Kernel::default();
    let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let centre = Point3::new(5.0, 10.0, 15.0);
    let edges = b.edge_list().to_vec();
    assert_eq!(edges.len(), 12);
    for e in edges {
        let fu = uses(&k, &b, e, centre);
        let mut guess = [None, None];
        for t in stations(&b, e) {
            let c = continuity_at(&k, &b, e, fu, t, guess).unwrap().unwrap();
            assert!(c.gap <= 1e-12, "{e:?} at {t}: gap {}", c.gap);
            assert!((c.normal_angle - FRAC_PI_2).abs() <= 1e-12, "{c:?}");
            assert!(
                c.jumps.iter().all(|&j| j == 0.0),
                "planes bend nowhere: {c:?}"
            );
            guess = [Some(c.uv[0]), Some(c.uv[1])];
        }
    }
}

/// A round of radius r between two planes at a right angle: the faces meet tangentially, the
/// curvature jumps from 0 to 1 / r, and neither side's curvature changes (a circle's section
/// is the circle itself, a plane's a line), so the G3 and G4 jumps are 0.
#[test]
fn a_round_between_planes_is_tangent_and_its_curvature_jumps_by_its_inverse_radius() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let r = 4.0;
    let e = edge_at(&k, &b, Point3::new(20.0, 30.0, 20.0));
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 0.0).unwrap();
    let centre = Point3::new(20.0, 15.0, 10.0);
    // the round's two rails: its face's edges along x
    let round: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            !matches!(s.kind(), SurfaceKind::Plane(_))
        })
        .collect();
    assert_eq!(round.len(), 1, "one round");
    let rails = [
        edge_at(&k, &b, Point3::new(20.0, 30.0, 20.0 - r)),
        edge_at(&k, &b, Point3::new(20.0, 30.0 - r, 20.0)),
    ];
    for e in rails {
        let fu = uses(&k, &b, e, centre);
        let mut guess = [None, None];
        for t in stations(&b, e) {
            let c = continuity_at(&k, &b, e, fu, t, guess).unwrap().unwrap();
            assert!(c.gap <= 1e-9, "{c:?}");
            assert!(c.normal_angle <= 1e-6, "{c:?}");
            assert!((c.jumps[0] - 1.0 / r).abs() <= 1e-6 / r, "{c:?}");
            assert!(c.jumps[1] <= 1e-6 && c.jumps[2] <= 1e-6, "{c:?}");
            guess = [Some(c.uv[0]), Some(c.uv[1])];
        }
    }
}

/// A cylinder of radius R: its circles' curvature vectors point at the axis with length 1 / R;
/// its side's normal curvature is 1 / R around (along the circles' parameter) and 0 along the
/// axis.
#[test]
fn a_cylinder_s_circles_and_parameter_lines_bend_by_their_closed_forms() {
    let mut k = Kernel::default();
    let radius = 7.0;
    let b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), radius, 12.0).unwrap();
    let mut circles = 0;
    for &e in b.edge_list() {
        let iv = b.edge(e).unwrap().interval;
        for i in 0..=4 {
            let t = iv.evaluate(f64::from(i) / 4.0);
            let Some(c) = curve_curvature(&k, &b, e, t).unwrap() else {
                continue;
            };
            let towards = Vec3::new(-c.point.x, -c.point.y, 0.0);
            if towards.length() < 1.0 {
                continue; // not a circle (a seam line through the axis has none)
            }
            if c.curvature.length() <= 1e-12 {
                continue; // a straight seam
            }
            circles += 1;
            assert!(
                (c.curvature.length() - 1.0 / radius).abs() <= 1e-12,
                "{c:?}"
            );
            let dir = towards * (1.0 / towards.length());
            assert!(
                (c.curvature.dot(dir) - 1.0 / radius).abs() <= 1e-12,
                "{c:?}"
            );
        }
    }
    assert!(circles >= 10, "both circles measured: {circles}");
    let side = b
        .face_list()
        .iter()
        .copied()
        .find(|&f| {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            !matches!(s.kind(), SurfaceKind::Plane(_))
        })
        .unwrap();
    let s = b.face(side).unwrap().surface.as_ref().unwrap();
    let d = s.natural_uv_domain_with_nurb(&k).unwrap();
    let mut seen = [0.0_f64; 2];
    for along_u in [true, false] {
        let c = iso_curvature(&k, &b, side, d.evaluate(0.3, 0.6), along_u)
            .unwrap()
            .unwrap();
        seen[usize::from(along_u)] = c.curvature.length();
        let radial = Vec3::new(c.point.x, c.point.y, 0.0);
        assert!((radial.length() - radius).abs() <= 1e-9, "{c:?}");
    }
    // one parameter runs round (1 / R), the other along the axis (0)
    let (round, straight) = if seen[0] > seen[1] {
        (seen[0], seen[1])
    } else {
        (seen[1], seen[0])
    };
    assert!((round - 1.0 / radius).abs() <= 1e-12, "{seen:?}");
    assert!(straight <= 1e-12, "{seen:?}");
}
