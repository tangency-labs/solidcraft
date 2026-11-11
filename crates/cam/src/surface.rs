//! The surface a 3D toolpath follows: triangles of the part (and of anything else the tool
//! must stay clear of, such as fixtures) in work coordinates, filed for fast search.
//!
//! The triangles come from the part's exact surfaces, tessellated by the caller within the
//! machining tolerance, as a toolpath is itself only ever within a tolerance of the part. A
//! cutter that stays clear of every triangle and touches them where it should is within that
//! tolerance of the part.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb3, Bvh, Mesh};

use crate::frame::Frame;
use crate::{CamError, Result, invalid};

/// Most triangles in one surface.
pub const MAX_TRIANGLES: usize = 5_000_000;
/// Largest coordinate accepted (mm).
pub const MAX_COORD: f64 = 1.0e6;
/// The smallest sine of the angle between a ray and a triangle's plane at which [`ray_triangle`]
/// says where the ray meets it: nearer the plane than that, what is left of the arithmetic is
/// rounding, and the point it gives may be anywhere along the ray.
pub const PARALLEL_SINE: f64 = 1e-9;

#[derive(Clone, Debug)]
pub struct Surface {
    tris: Vec<[Vec3; 3]>,
    /// The surface's normal at each corner, pointing out of the material.
    normals: Vec<[Vec3; 3]>,
    /// Each triangle's own normal (from its winding), pointing out of the material.
    facets: Vec<Vec3>,
    bvh: Bvh,
    bounds: Aabb3,
}

fn facet_normal(t: &[Vec3; 3]) -> Option<Vec3> {
    (t[1] - t[0]).cross(t[2] - t[0]).normalized()
}

impl Surface {
    /// The surface of `triangles`, wound counter-clockwise seen from outside the material.
    /// Triangles without area are left out.
    pub fn new(triangles: Vec<[Vec3; 3]>) -> Result<Surface> {
        Surface::with_normals(triangles.into_iter().map(|t| (t, None)).collect())
    }

    /// The surface of `mesh`, with its vertex normals where it has one per position.
    pub fn from_mesh(mesh: &Mesh) -> Result<Surface> {
        if mesh.triangles.len() > MAX_TRIANGLES {
            return Err(CamError::TooLarge(format!("the surface has more than {MAX_TRIANGLES} triangles")));
        }
        let smooth = mesh.normals.len() == mesh.positions.len();
        let mut tris = Vec::with_capacity(mesh.triangles.len());
        for t in &mesh.triangles {
            let Some(p) = mesh.tri(t) else { return invalid("a mesh triangle refers to a vertex that does not exist") };
            let n = if smooth {
                let at = |i: u32| mesh.normals.get(i as usize).copied();
                match (at(t[0]), at(t[1]), at(t[2])) {
                    (Some(a), Some(b), Some(c)) => Some([a, b, c]),
                    _ => None,
                }
            } else {
                None
            };
            tris.push((p, n));
        }
        Surface::with_normals(tris)
    }

    fn with_normals(input: Vec<([Vec3; 3], Option<[Vec3; 3]>)>) -> Result<Surface> {
        if input.len() > MAX_TRIANGLES {
            return Err(CamError::TooLarge(format!("the surface has more than {MAX_TRIANGLES} triangles")));
        }
        let (mut tris, mut normals, mut facets, mut boxes) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut bounds = Aabb3::EMPTY;
        for (t, n) in input {
            if !t.iter().all(|p| p.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD && p.z.abs() <= MAX_COORD) {
                return invalid("a surface triangle has a coordinate that is not a finite number within 1000 m");
            }
            let Some(f) = facet_normal(&t) else { continue };
            // A vertex normal that disagrees with its triangle's side (or is not a direction)
            // is replaced by the triangle's own.
            let n = n.unwrap_or([f; 3]).map(|v| v.normalized().filter(|u| u.dot(f) > 0.0).unwrap_or(f));
            let mut b = Aabb3::EMPTY;
            for p in t {
                b.add(p);
            }
            bounds = bounds.union(&b);
            tris.push(t);
            normals.push(n);
            facets.push(f);
            boxes.push(b);
        }
        let bvh = Bvh::new(&boxes);
        Ok(Surface { tris, normals, facets, bvh, bounds })
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn bounds(&self) -> Aabb3 {
        self.bounds
    }

    pub fn triangle(&self, i: usize) -> Option<[Vec3; 3]> {
        self.tris.get(i).copied()
    }

    pub fn triangles(&self) -> &[[Vec3; 3]] {
        &self.tris
    }

    /// Triangle `i`'s own normal.
    pub fn facet_normal(&self, i: usize) -> Option<Vec3> {
        self.facets.get(i).copied()
    }

    /// The surface's normal at `p` on triangle `i`, blended from its corners' normals.
    pub fn normal_at(&self, i: usize, p: Vec3) -> Option<Vec3> {
        let (t, n) = (self.tris.get(i)?, self.normals.get(i)?);
        let [u, v, w] = barycentric(t, p)?;
        (n[0] * u + n[1] * v + n[2] * w).normalized().or_else(|| self.facet_normal(i))
    }

    /// Calls `f` with every triangle whose box `enter` accepts (a box around the triangles
    /// looked at, and every box above it).
    pub fn visit(&self, enter: impl FnMut(&Aabb3) -> bool, mut f: impl FnMut(usize, &[Vec3; 3])) {
        self.bvh.visit(enter, |i| {
            if let Some(t) = self.tris.get(i as usize) {
                f(i as usize, t);
            }
        });
    }

    /// The first triangle along the ray from `o` along `d` (not necessarily unit): the
    /// distance in multiples of `d` and the triangle. Either side of a triangle is hit.
    pub fn raycast(&self, o: Vec3, d: Vec3) -> Option<(f64, usize)> {
        self.bvh.nearest(o, d, |i| self.tris.get(i as usize).and_then(|t| ray_triangle(o, d, t))).map(|(t, i)| (t, i as usize))
    }

    /// The surface in `frame`'s coordinates.
    pub fn in_frame(&self, frame: &Frame) -> Result<Surface> {
        frame.validate()?;
        let input =
            self.tris.iter().zip(&self.normals).map(|(t, n)| (t.map(|p| frame.to_local(p)), Some(n.map(|v| frame.dir_to_local(v))))).collect();
        Surface::with_normals(input)
    }
}

/// The barycentric weights of `p` (projected onto the triangle's plane); `None` for a
/// triangle without area.
pub fn barycentric(t: &[Vec3; 3], p: Vec3) -> Option<[f64; 3]> {
    let (e0, e1, e2) = (t[1] - t[0], t[2] - t[0], p - t[0]);
    let (d00, d01, d11, d20, d21) = (e0.dot(e0), e0.dot(e1), e1.dot(e1), e2.dot(e0), e2.dot(e1));
    let den = d00 * d11 - d01 * d01;
    if !(den.abs() > 1e-300) {
        return None;
    }
    let v = (d11 * d20 - d01 * d21) / den;
    let w = (d00 * d21 - d01 * d20) / den;
    Some([1.0 - v - w, v, w])
}

/// Where the ray `o + t·d` (t ≥ 0) meets the triangle (Möller–Trumbore), either side. A ray in
/// the triangle's plane, to rounding (`PARALLEL_SINE`), meets it nowhere: such a ray runs
/// into the triangles beyond the edges it crosses, which say where.
pub fn ray_triangle(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    // `d · (e2 × e1)`: the sine of the ray's angle to the plane, times the ray's length and
    // twice the triangle's area.
    let det = e1.dot(p);
    if !(det.abs() > PARALLEL_SINE * d.len() * e1.cross(e2).len()) || det.abs() <= 1e-300 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(-1e-12..=1.0 + 1e-12).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-12 || u + v > 1.0 + 1e-12 {
        return None;
    }
    let at = e2.dot(q) * inv;
    (at >= 0.0 && at.is_finite()).then_some(at)
}

/// The nearest point of the triangle to `p`.
pub fn nearest_on_triangle(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    // Ericson, Real-Time Collision Detection, 5.1.5: by the Voronoi region `p` falls in.
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let den = va + vb + vc;
    if !(den.abs() > 1e-300) {
        return a;
    }
    let (v, w) = (vb / den, vc / den);
    a + ab * v + ac * w
}

/// Whether the segment `a–b`, grown by `r` all round, can meet the box.
pub fn capsule_meets_box(a: Vec3, b: Vec3, r: f64, bx: &Aabb3) -> bool {
    let bx = bx.inflated(r);
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (o, dv, lo, hi) in [(a.x, d.x, bx.min.x, bx.max.x), (a.y, d.y, bx.min.y, bx.max.y), (a.z, d.z, bx.min.z, bx.max.z)] {
        if dv.abs() <= 1e-300 {
            if o < lo || o > hi {
                return false;
            }
            continue;
        }
        let (mut u, mut v) = ((lo - o) / dv, (hi - o) / dv);
        if u > v {
            std::mem::swap(&mut u, &mut v);
        }
        t0 = t0.max(u);
        t1 = t1.min(v);
        if t0 > t1 {
            return false;
        }
    }
    true
}
