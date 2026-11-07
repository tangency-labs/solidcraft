//! Tests of the convex hull and fitted shapes (`hull3.rs`).

use solidcore_record::Vec3;
use solidcraft_geom::{Mesh, Vec3Geom};

/// The eight corners of a box centred on the origin.
fn box_corners(x: f64, y: f64, z: f64) -> Vec<Vec3> {
    let mut out = Vec::new();
    for sx in [-0.5, 0.5] {
        for sy in [-0.5, 0.5] {
            for sz in [-0.5, 0.5] {
                out.push(Vec3::new(sx * x, sy * y, sz * z));
            }
        }
    }
    out
}

/// A closed box mesh, turned by `turn` (rows of a rotation) about the origin and moved by `to`.
fn turned_box_mesh(x: f64, y: f64, z: f64, turn: [[f64; 3]; 3], to: Vec3) -> Mesh {
    let positions = box_corners(x, y, z)
        .into_iter()
        .map(|p| {
            let r = |k: usize| turn[k][0] * p.x + turn[k][1] * p.y + turn[k][2] * p.z;
            Vec3::new(r(0), r(1), r(2)) + to
        })
        .collect();
    let triangles =
        vec![[0, 1, 3], [0, 3, 2], [4, 6, 7], [4, 7, 5], [0, 4, 5], [0, 5, 1], [2, 3, 7], [2, 7, 6], [0, 2, 6], [0, 6, 4], [1, 5, 7], [1, 7, 3]];
    Mesh { positions, triangles, ..Mesh::default() }
}

const TURN_30_Z: [[f64; 3]; 3] = [[0.866_025_403_784_438_6, -0.5, 0.0], [0.5, 0.866_025_403_784_438_6, 0.0], [0.0, 0.0, 1.0]];
const NO_TURN: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[test]
fn convex_hull_of_a_cube_with_inner_points_has_8_vertices_and_12_triangles() {
    let mut points = box_corners(10.0, 10.0, 10.0);
    points.extend([Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), Vec3::new(-4.0, 4.0, -4.0)]);
    let hull = super::convex_hull(&points).unwrap_or_else(|| panic!("no hull"));
    assert_eq!((hull.positions.len(), hull.triangles.len()), (8, 12));
    let m = hull.mass_properties().unwrap_or_else(|| panic!("hull is not closed"));
    assert!((m.volume - 1000.0).abs() < 1e-9, "{}", m.volume);
}

#[test]
fn convex_hull_ignores_points_on_its_faces() {
    let mut points = box_corners(10.0, 10.0, 10.0);
    points.extend([Vec3::new(5.0, 0.0, 0.0), Vec3::new(5.0, 2.0, -3.0), Vec3::new(0.0, 5.0, 5.0)]);
    let hull = super::convex_hull(&points).unwrap_or_else(|| panic!("no hull"));
    let m = hull.mass_properties().unwrap_or_else(|| panic!("hull is not closed"));
    assert!((m.volume - 1000.0).abs() < 1e-9, "{}", m.volume);
}

#[test]
fn convex_hull_volume_of_a_tetrahedron_is_exact() {
    // Edges of 6, 4 and 3 along the axes: volume 6 · 4 · 3 / 6.
    let points = [Vec3::ZERO, Vec3::new(6.0, 0.0, 0.0), Vec3::new(0.0, 4.0, 0.0), Vec3::new(0.0, 0.0, 3.0)];
    let hull = super::convex_hull(&points).unwrap_or_else(|| panic!("no hull"));
    let m = hull.mass_properties().unwrap_or_else(|| panic!("hull is not closed"));
    assert!((m.volume - 12.0).abs() < 1e-12, "{}", m.volume);
}

#[test]
fn convex_hull_of_points_on_a_sphere_is_closed_and_holds_them_all() {
    let mut points = Vec::new();
    for i in 0..40 {
        for j in 0..80 {
            let (t, f) = (std::f64::consts::PI * (f64::from(i) + 0.5) / 40.0, std::f64::consts::TAU * f64::from(j) / 80.0);
            points.push(Vec3::new(7.0 * t.sin() * f.cos(), 7.0 * t.sin() * f.sin(), 7.0 * t.cos()));
        }
    }
    let hull = super::convex_hull(&points).unwrap_or_else(|| panic!("no hull"));
    let m = hull.mass_properties().unwrap_or_else(|| panic!("hull is not closed"));
    let sphere = 4.0 / 3.0 * std::f64::consts::PI * 343.0;
    assert!(m.volume > 0.97 * sphere && m.volume < sphere, "{} of {sphere}", m.volume);
}

#[test]
fn convex_hull_refuses_flat_short_or_non_finite_input() {
    let flat: Vec<Vec3> = (0..20).map(|i| Vec3::new(f64::from(i), f64::from(i * i % 7), 0.0)).collect();
    let line: Vec<Vec3> = (0..20).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect();
    let mut bad = box_corners(1.0, 1.0, 1.0);
    bad.push(Vec3::new(f64::NAN, 0.0, 0.0));
    let same = vec![Vec3::new(1.0, 1.0, 1.0); 10];
    for points in [flat, line, bad, same, Vec::new(), box_corners(1.0, 1.0, 1.0)[..3].to_vec()] {
        assert!(super::convex_hull(&points).is_none(), "{points:?}");
    }
}

#[test]
fn fit_box_of_a_turned_box_recovers_its_size() {
    let to = Vec3::new(3.0, -2.0, 1.0);
    let fit = super::fit_box(&turned_box_mesh(30.0, 20.0, 10.0, TURN_30_Z, to)).unwrap_or_else(|| panic!("no fit"));
    let mut sizes = [fit.size.x, fit.size.y, fit.size.z];
    sizes.sort_by(f64::total_cmp);
    for (got, want) in sizes.iter().zip([10.0, 20.0, 30.0]) {
        assert!((got - want).abs() < 1e-6, "{sizes:?}");
    }
    let off = Vec3::new(fit.frame[3][0], fit.frame[3][1], fit.frame[3][2]) - to;
    assert!(off.dot(off) < 1e-12, "{:?}", fit.frame);
}

#[test]
fn fit_cylinder_of_a_faceted_cylinder_recovers_radius_and_length() {
    let n = 64u32;
    let mut positions = Vec::new();
    for z in [-20.0, 20.0] {
        for k in 0..n {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            positions.push(Vec3::new(5.0 * a.cos(), 5.0 * a.sin(), z));
        }
    }
    let hull = super::convex_hull(&positions).unwrap_or_else(|| panic!("no hull"));
    let fit = super::fit_cylinder(&hull).unwrap_or_else(|| panic!("no fit"));
    assert!((fit.size.x - 5.0).abs() < 1e-6, "{:?}", fit.size);
    assert!((fit.size.z - 40.0).abs() < 1e-6, "{:?}", fit.size);
    assert!(fit.frame[2][2].abs() > 1.0 - 1e-6, "runs along the mesh's long axis: {:?}", fit.frame);
}

#[test]
fn fit_sphere_contains_every_point() {
    let mesh = turned_box_mesh(6.0, 8.0, 10.0, NO_TURN, Vec3::new(1.0, 1.0, 1.0));
    let fit = super::fit_sphere(&mesh).unwrap_or_else(|| panic!("no fit"));
    let c = Vec3::new(fit.frame[3][0], fit.frame[3][1], fit.frame[3][2]);
    for p in &mesh.positions {
        let d = *p - c;
        assert!(d.dot(d).sqrt() <= fit.size.x + 1e-9);
    }
    // Half the box's long diagonal: a box's corners all lie on one sphere.
    assert!((fit.size.x - (36.0f64 + 64.0 + 100.0).sqrt() / 2.0).abs() < 1e-9, "{}", fit.size.x);
}

#[test]
fn fits_refuse_an_empty_or_non_finite_mesh() {
    let empty = Mesh::default();
    let bad = Mesh { positions: vec![Vec3::new(f64::NAN, 0.0, 0.0)], ..Mesh::default() };
    for m in [&empty, &bad] {
        assert!(super::fit_box(m).is_none() && super::fit_cylinder(m).is_none() && super::fit_sphere(m).is_none());
    }
}

#[test]
fn convex_hull_is_the_same_on_every_run() {
    let mut points = Vec::new();
    for i in 0..30 {
        for j in 0..60 {
            let (t, f) = (std::f64::consts::PI * (f64::from(i) + 0.5) / 30.0, std::f64::consts::TAU * f64::from(j) / 60.0);
            points.push(Vec3::new(7.0 * t.sin() * f.cos(), 7.0 * t.sin() * f.sin(), 7.0 * t.cos()));
        }
    }
    let first = super::convex_hull(&points).unwrap_or_else(|| panic!("no hull"));
    for _ in 0..5 {
        let again = super::convex_hull(&points).unwrap_or_else(|| panic!("no hull"));
        assert_eq!(first.triangles, again.triangles);
    }
}
