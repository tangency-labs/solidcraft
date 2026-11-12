//! Shapes the analysis tests are checked on.
//!
//! A box square to the axes, because that is the shape whose exact answer is known: the
//! lattice lands on its walls, so the discretisation has no geometric error left and what a
//! test measures is the physics and nothing else.

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

/// A closed box from `min` to `max`, its six sides numbered 0 to 5 in the order
/// −x, +x, −y, +y, −z, +z.
pub fn block(min: Vec3, max: Vec3) -> Mesh {
    let (x0, y0, z0, x1, y1, z1) = (min.x, min.y, min.z, max.x, max.y, max.z);
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let mut mesh = Mesh::default();
    let sides = [
        (Vec3::new(-1., 0., 0.), [p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1), p(x0, y1, z0)]),
        (Vec3::new(1., 0., 0.), [p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)]),
        (Vec3::new(0., -1., 0.), [p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)]),
        (Vec3::new(0., 1., 0.), [p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1), p(x1, y1, z0)]),
        (Vec3::new(0., 0., -1.), [p(x0, y0, z0), p(x0, y1, z0), p(x1, y1, z0), p(x1, y0, z0)]),
        (Vec3::new(0., 0., 1.), [p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)]),
    ];
    for (face, (normal, corners)) in sides.into_iter().enumerate() {
        let base = mesh.positions.len() as u32;
        for c in corners {
            mesh.positions.push(c);
            mesh.normals.push(normal);
        }
        mesh.triangles.push([base, base + 1, base + 2]);
        mesh.triangles.push([base, base + 2, base + 3]);
        mesh.tri_face.push(face as u32);
        mesh.tri_face.push(face as u32);
    }
    mesh
}

/// A box given as a corner and a size, which is how the tests read.
pub fn block_at(min: [f64; 3], size: [f64; 3]) -> Mesh {
    let min = Vec3::new(min[0], min[1], min[2]);
    block(min, min + Vec3::new(size[0], size[1], size[2]))
}

/// A cylinder about the z axis, from `z0` to `z1`, cut into `segments` flats around.
///
/// Face 0 is the curved side, 1 the cap at `z0`, 2 the cap at `z1`. Nothing about it lands on a
/// lattice, which is the point: it is the shape that tells a staircase from the real thing.
pub fn cylinder(radius: f64, z0: f64, z1: f64, segments: usize) -> Mesh {
    let n = segments.max(3);
    let mut mesh = Mesh::default();
    let ring = |z: f64| -> Vec<Vec3> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                Vec3::new(radius * a.cos(), radius * a.sin(), z)
            })
            .collect()
    };
    let (low, high) = (ring(z0), ring(z1));
    let push = |a: Vec3, b: Vec3, c: Vec3, normal: Vec3, face: u32, mesh: &mut Mesh| {
        let base = mesh.positions.len() as u32;
        for p in [a, b, c] {
            mesh.positions.push(p);
            mesh.normals.push(normal);
        }
        mesh.triangles.push([base, base + 1, base + 2]);
        mesh.tri_face.push(face);
    };
    for i in 0..n {
        let j = (i + 1) % n;
        let (Some(bi), Some(bj), Some(ti), Some(tj)) = (low.get(i), low.get(j), high.get(i), high.get(j)) else {
            continue;
        };
        // Outward is straight out from the axis, which is what the winding below gives.
        let out = Vec3::new(bi.x + bj.x, bi.y + bj.y, 0.0).normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        push(*bi, *bj, *tj, out, 0, &mut mesh);
        push(*bi, *tj, *ti, out, 0, &mut mesh);
    }
    let down = Vec3::new(0.0, 0.0, -1.0);
    let up = Vec3::new(0.0, 0.0, 1.0);
    let (centre_low, centre_high) = (Vec3::new(0.0, 0.0, z0), Vec3::new(0.0, 0.0, z1));
    for i in 0..n {
        let j = (i + 1) % n;
        if let (Some(bi), Some(bj)) = (low.get(i), low.get(j)) {
            push(centre_low, *bj, *bi, down, 1, &mut mesh);
        }
        if let (Some(ti), Some(tj)) = (high.get(i), high.get(j)) {
            push(centre_high, *ti, *tj, up, 2, &mut mesh);
        }
    }
    mesh
}

/// A triangular prism: a bar `width` wide along y whose cross-section is the right triangle
/// with legs `leg` along x and z, so that one of its faces lies at 45° to both.
///
/// Face 0 is the slant, 1 the side at x = 0, 2 the bottom at z = 0, 3 and 4 the triangular ends.
/// The slant's area is exactly `leg * √2 * width`, which a lattice standing in for it over-states
/// by √2 however fine it is cut.
pub fn wedge(leg: f64, width: f64) -> Mesh {
    let mut mesh = Mesh::default();
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, normal: Vec3, face: u32, mesh: &mut Mesh| {
        let base = mesh.positions.len() as u32;
        for v in [a, b, c, d] {
            mesh.positions.push(v);
            mesh.normals.push(normal);
        }
        mesh.triangles.push([base, base + 1, base + 2]);
        mesh.triangles.push([base, base + 2, base + 3]);
        mesh.tri_face.push(face);
        mesh.tri_face.push(face);
    };
    let s = 1.0 / 2.0f64.sqrt();
    // The slant runs from (leg, 0) down to (0, leg) in x and z; outward is up and out.
    quad(p(leg, 0.0, 0.0), p(leg, width, 0.0), p(0.0, width, leg), p(0.0, 0.0, leg), Vec3::new(s, 0.0, s), 0, &mut mesh);
    quad(p(0.0, 0.0, 0.0), p(0.0, 0.0, leg), p(0.0, width, leg), p(0.0, width, 0.0), Vec3::new(-1.0, 0.0, 0.0), 1, &mut mesh);
    quad(p(0.0, 0.0, 0.0), p(leg, 0.0, 0.0), p(leg, width, 0.0), p(0.0, width, 0.0), Vec3::new(0.0, 0.0, -1.0), 2, &mut mesh);
    let tri = |a: Vec3, b: Vec3, c: Vec3, normal: Vec3, face: u32, mesh: &mut Mesh| {
        let base = mesh.positions.len() as u32;
        for v in [a, b, c] {
            mesh.positions.push(v);
            mesh.normals.push(normal);
        }
        mesh.triangles.push([base, base + 1, base + 2]);
        mesh.tri_face.push(face);
    };
    tri(p(0.0, 0.0, 0.0), p(0.0, 0.0, leg), p(leg, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), 3, &mut mesh);
    tri(p(0.0, width, 0.0), p(leg, width, 0.0), p(0.0, width, leg), Vec3::new(0.0, 1.0, 0.0), 4, &mut mesh);
    mesh
}

/// A bar `length` along x and `width` along y whose height rises from `h0` at one end to `h1`
/// at the other, so that its cross-section shrinks as the heat runs along it.
///
/// Face 0 is the end at x = 0, 1 the end at x = length, 2 the bottom, 3 the slanted top, 4 and
/// 5 the sides. The resistance along it is `L·ln(h1/h0) / (k·w·(h1−h0))` exactly, as long as the
/// slope is gentle enough for the heat to run straight.
pub fn taper(length: f64, width: f64, h0: f64, h1: f64) -> Mesh {
    let mut mesh = Mesh::default();
    let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
    let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, normal: Vec3, face: u32, mesh: &mut Mesh| {
        let base = mesh.positions.len() as u32;
        for v in [a, b, c, d] {
            mesh.positions.push(v);
            mesh.normals.push(normal);
        }
        mesh.triangles.push([base, base + 1, base + 2]);
        mesh.triangles.push([base, base + 2, base + 3]);
        mesh.tri_face.push(face);
        mesh.tri_face.push(face);
    };
    let (l, w) = (length, width);
    quad(p(0., 0., 0.), p(0., 0., h0), p(0., w, h0), p(0., w, 0.), Vec3::new(-1., 0., 0.), 0, &mut mesh);
    quad(p(l, 0., 0.), p(l, w, 0.), p(l, w, h1), p(l, 0., h1), Vec3::new(1., 0., 0.), 1, &mut mesh);
    quad(p(0., 0., 0.), p(l, 0., 0.), p(l, w, 0.), p(0., w, 0.), Vec3::new(0., 0., -1.), 2, &mut mesh);
    // The top rises by h1 − h0 over the length, so its outward normal leans back along x.
    let rise = h1 - h0;
    let n = Vec3::new(-rise, 0.0, l).normalized().unwrap_or(Vec3::new(0., 0., 1.));
    quad(p(0., 0., h0), p(l, 0., h1), p(l, w, h1), p(0., w, h0), n, 3, &mut mesh);
    quad(p(0., 0., 0.), p(l, 0., 0.), p(l, 0., h1), p(0., 0., h0), Vec3::new(0., -1., 0.), 4, &mut mesh);
    quad(p(0., w, 0.), p(0., w, h0), p(l, w, h1), p(l, w, 0.), Vec3::new(0., 1., 0.), 5, &mut mesh);
    mesh
}
