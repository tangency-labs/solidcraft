//! What the application draws of a large sheet with small bends and holes, and that what
//! reads the same mesh (bounds, picks, inside tests) answers as before.

use std::f64::consts::{FRAC_PI_2, TAU};

use serde_json::{Value, json};
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_kernel::{DISPLAY_ANGLE, FaceSurface, face_surface};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

const HOLES: [[f64; 2]; 4] = [[30.0, 20.0], [1500.0, 20.0], [2970.0, 980.0], [30.0, 980.0]];

/// A 3000 x 1000 x 2 sheet with bend radius 1, four holes of diameter 4 and a 40 mm flange on
/// each long side.
fn large_sheet() -> Session {
    let mut s = Session::default();
    run(&mut s, "sheet.manage_rules", json!({"name": "AL2", "thickness": "2 mm", "k_factor": "0.5", "bend_radius": "1 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [3000, 1000]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Holes"}));
    for c in HOLES {
        run(&mut s, "sketch.circle.center", json!({"center": c, "diameter": 4}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Holes", "distance": 10, "through_all": true, "operation": "cut"}));
    run(&mut s, "sheet.flange", json!({"edges": [[1500, 0, 2], [1500, 1000, 2]], "height": 40}));
    s
}

/// The widest angle between neighbouring lines of vertices around a cylinder face, and how
/// many such steps the face has.
fn steps_around(m: &Mesh, face: usize, origin: Vec3, axis: Vec3) -> (usize, f64) {
    let u = axis.any_perp();
    let v = axis.cross(u);
    let mut a: Vec<f64> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, f)| **f as usize == face)
        .flat_map(|(t, _)| t.iter())
        .map(|k| m.positions[*k as usize] - origin)
        .map(|d| d.dot(v).atan2(d.dot(u)))
        .collect();
    a.sort_by(f64::total_cmp);
    a.dedup_by(|x, y| (*x - *y).abs() < 1e-7);
    // Leave out the widest gap: the part of the turn the face does not cover.
    let n = a.len();
    let mut gaps: Vec<f64> = (0..n).map(|i| (a[(i + 1) % n] - a[i]).rem_euclid(TAU)).collect();
    gaps.sort_by(f64::total_cmp);
    gaps.pop();
    (gaps.len(), gaps.last().copied().unwrap_or(0.0))
}

#[test]
fn a_large_sheet_shows_its_small_bends_and_holes_round() {
    let s = large_sheet();
    let st = s.world_state();
    let b = &st.bodies[0];
    let m = b.mesh();
    assert!(b.body.size() * 1e-3 > 3.0, "the chord tolerance is more than any radius here");
    let (mut bends, mut walls) = (0, 0);
    for face in 0..b.body.face_count() {
        let Some(FaceSurface::Cylinder { origin, axis, radius }) = face_surface(&b.body, face) else { continue };
        let (steps, widest) = steps_around(&m, face, origin, *axis);
        assert!(widest <= DISPLAY_ANGLE * 1.001, "radius {radius}: a facet spans {} degrees", widest.to_degrees());
        if (radius - 2.0).abs() < 1e-6 {
            // Half of a hole's wall.
            walls += 1;
            assert!(steps >= (TAU / 2.0 / DISPLAY_ANGLE).ceil() as usize, "{steps} facets around half a hole");
        } else {
            // A bend's inner (1 mm) or outer (3 mm) side: a quarter turn.
            bends += 1;
            assert!((radius - 1.0).abs() < 1e-6 || (radius - 3.0).abs() < 1e-6, "radius {radius}");
            assert!(steps >= (FRAC_PI_2 / DISPLAY_ANGLE).ceil() as usize, "radius {radius}: {steps} facets across the bend");
        }
    }
    assert_eq!((bends, walls), (4, 8));
    // The edges the viewport draws: every hole's rim is round, on both sides of the sheet.
    for [x, y] in HOLES {
        for z in [0.0, 2.0] {
            let on = |p: &Vec3| (p.z - z).abs() < 1e-6 && ((p.x - x).hypot(p.y - y) - 2.0).abs() < 1e-6;
            let rim: Vec<&Vec<Vec3>> = m.edges.iter().filter(|e| e.iter().all(on)).collect();
            let segments: usize = rim.iter().map(|e| e.len() - 1).sum();
            assert!(segments >= (TAU / DISPLAY_ANGLE).ceil() as usize, "{segments} segments around the rim at {x}, {y}, {z}");
            let widest = rim.iter().flat_map(|e| e.windows(2)).map(|w| 2.0 * (w[0].dist(w[1]) / 4.0).asin()).fold(0.0, f64::max);
            assert!(widest <= DISPLAY_ANGLE * 1.001, "a rim segment spans {} degrees", widest.to_degrees());
        }
    }
    // And the scene the renderers take draws those same polylines.
    let scene = crate::view::scene(&s, &crate::view::home_camera(&s));
    let drawn: usize =
        scene.lines.iter().filter(|l| l.points.iter().all(|p| ((p.x - 30.0).hypot(p.y - 20.0) - 2.0).abs() < 1e-6)).map(|l| l.points.len() - 1).sum();
    assert!(drawn >= 2 * (TAU / DISPLAY_ANGLE).ceil() as usize, "{drawn} rim segments in the scene");
}

#[test]
fn what_reads_the_display_mesh_of_a_large_sheet_answers_as_before() {
    let s = large_sheet();
    let st = s.world_state();
    let b = &st.bodies[0];
    let m = b.mesh();
    let coarse = b.body.mesh(&solidcraft_kernel::MeshTolerance::chord((b.body.size() * 1e-3).max(1e-3))).unwrap();
    // Bounds: the sheet's box is set by flat faces, so the rounder bends move nothing.
    let (now, before) = (m.bounds(), coarse.bounds());
    assert!(now.min.dist(before.min) < 1e-9 && now.max.dist(before.max) < 1e-9, "{now:?} {before:?}");
    let exact = solidcraft_kernel::measure(&b.body).unwrap().bbox;
    assert!(now.min.dist(exact.min) < 1e-6 && now.max.dist(exact.max) < 1e-6, "{now:?} {exact:?}");
    // The cost: the sheet takes 2212 triangles against the chord mesh's 92 when measured, a factor of
    // 24; what grows is the number of round faces and the facets across each, not the size of the body.
    assert!(
        m.triangles.len() <= 30 * coarse.triangles.len() && m.triangles.len() < 4000,
        "{} triangles, {} at the chord tolerance",
        m.triangles.len(),
        coarse.triangles.len()
    );
    // The same faces and edges, in the same order, between the same faces, with the same ends.
    assert_eq!(m.edge_faces, coarse.edge_faces);
    assert_eq!(m.tri_face.iter().max(), coarse.tri_face.iter().max());
    assert_eq!(m.edges.len(), coarse.edges.len());
    for (fine, rough) in m.edges.iter().zip(&coarse.edges) {
        assert!(fine[0].dist(rough[0]) < 1e-9 && fine[fine.len() - 1].dist(rough[rough.len() - 1]) < 1e-9);
    }
    // Picks: a point on a round face (on the true cylinder, which a flat strip across a bend
    // misses by up to 0.9 mm) is found on that face, as is one in the middle of a flat face.
    let mut round = 0;
    for face in 0..b.body.face_count() {
        let Some(FaceSurface::Cylinder { origin, axis, radius }) = face_surface(&b.body, face) else { continue };
        let corners: Vec<Vec3> =
            m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f as usize == face).flat_map(|(t, _)| t.map(|k| m.positions[k as usize])).collect();
        let middle = corners.iter().fold(Vec3::ZERO, |a, p| a + *p) / corners.len() as f64;
        // The middle of the face's corners, pushed out onto the cylinder.
        let along = origin + axis * (middle - origin).dot(axis);
        let p = along + (middle - along).normalized().unwrap() * radius;
        assert_eq!(solidcraft_doc::appearance::face_index_at(b, p), Some(face), "radius {radius} at {p:?}");
        round += 1;
    }
    assert_eq!(round, 12);
    let top = solidcraft_doc::appearance::face_index_at(b, Vec3::new(1500.0, 500.0, 2.0)).unwrap();
    assert!(matches!(face_surface(&b.body, top), Some(FaceSurface::Plane { normal, .. }) if normal.dist(Vec3::Z) < 1e-9));
    // Inside and outside: in the sheet, in a flange's wall, in a hole and above the sheet.
    assert!(m.contains(Vec3::new(1500.0, 500.0, 1.0)) && m.contains(Vec3::new(1500.0, now.min.y + 1.0, now.max.z - 10.0)));
    assert!(!m.contains(Vec3::new(30.0, 20.0, 1.0)) && !m.contains(Vec3::new(1500.0, 500.0, 3.0)));
}
