//! The surface fits behind [`super::mesh_recognize`]: least squares, then certification.
//!
//! Each fit returns the surface and the greatest distance from it over the patch's `samples`,
//! and `None` when the numbers do not come out as a usable surface at all (a cylinder of no
//! radius, a cone of no taper, a near-singular system). Certification against `tolerance` is the
//! caller's, so a fit that is merely poor is reported and then rejected, never silently accepted.
//!
//! A fit is made from the `vertices`, which lie on the surface the mesh was tessellated from, and
//! measured over the `samples`: the vertices *and* a point inside each triangle. The inside points
//! are what pin the surface down. A coarse cone has lateral corners in two rings only, the base
//! and the apex, and a sphere through those two rings is exact at every corner while standing
//! millimetres off the middle of each facet. Measuring inside the facets also means a mesh
//! certifies only when it really is within `tolerance` of the surface, so a coarsely faceted
//! cylinder stays the flat bands it is until the caller allows for its chord error.

use solidcore_record::Vec3;
use solidcraft_geom::inertia::{Tensor3, principal};
use solidcraft_geom::prelude::*;

use super::Surface;

/// A radius or a distance beyond this many times the patch's own size is not geometry, it is a
/// near-flat fit running away to infinity.
const RUNAWAY: f64 = 1e3;

/// The mean of some points, `None` when there are none or they are not finite.
pub(super) fn centroid(points: &[Vec3]) -> Option<Vec3> {
    if points.is_empty() {
        return None;
    }
    let sum = points.iter().fold(Vec3::new(0., 0., 0.), |s, p| s + *p);
    let mean = sum / points.len() as f64;
    (mean.x.is_finite() && mean.y.is_finite() && mean.z.is_finite()).then_some(mean)
}

/// Sum of `(p - about) (p - about)^T`.
fn covariance(points: &[Vec3], about: Vec3) -> Tensor3 {
    let mut m = [[0.; 3]; 3];
    for p in points {
        let d = *p - about;
        let d = [d.x, d.y, d.z];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                if let (Some(a), Some(b)) = (d.get(i), d.get(j)) {
                    *cell += a * b;
                }
            }
        }
    }
    m
}

/// Sum of `v v^T`, about the origin rather than the mean.
fn outer_sum(vectors: &[Vec3]) -> Tensor3 {
    covariance(vectors, Vec3::new(0., 0., 0.))
}

/// The direction a cloud leans on least: the eigenvector of the smallest eigenvalue.
fn smallest_axis(m: &Tensor3) -> Option<Vec3> {
    let (_, axes) = principal(m, 0.)?;
    let [axis, _, _] = axes;
    axis.normalized()
}

/// Two unit directions square to `axis` and to each other.
fn basis(axis: Vec3) -> Option<(Vec3, Vec3)> {
    let away = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vec3::new(1., 0., 0.)
    } else if axis.y.abs() <= axis.z.abs() {
        Vec3::new(0., 1., 0.)
    } else {
        Vec3::new(0., 0., 1.)
    };
    let u = axis.cross(away).normalized()?;
    let v = axis.cross(u).normalized()?;
    Some((u, v))
}

/// Solve `a x = b` by Gaussian elimination with partial pivoting. `None` when the system is
/// near-singular or the answer is not finite, which is how a degenerate patch bows out.
fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    let scale = a.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    if !(scale > 0.) || !scale.is_finite() {
        return None;
    }
    for i in 0..N {
        let mut pivot = i;
        for r in (i + 1)..N {
            let (Some(candidate), Some(best)) = (a.get(r).and_then(|row| row.get(i)), a.get(pivot).and_then(|row| row.get(i))) else {
                return None;
            };
            if candidate.abs() > best.abs() {
                pivot = r;
            }
        }
        if a.get(pivot).and_then(|row| row.get(i)).is_none_or(|x| x.abs() <= scale * 1e-12) {
            return None;
        }
        a.swap(i, pivot);
        b.swap(i, pivot);
        let p = *a.get(i).and_then(|row| row.get(i))?;
        for r in (i + 1)..N {
            let f = *a.get(r).and_then(|row| row.get(i))? / p;
            if f == 0. {
                continue;
            }
            for c in i..N {
                let above = *a.get(i).and_then(|row| row.get(c))?;
                let cell = a.get_mut(r).and_then(|row| row.get_mut(c))?;
                *cell -= f * above;
            }
            let above = *b.get(i)?;
            let cell = b.get_mut(r)?;
            *cell -= f * above;
        }
    }
    let mut x = [0.; N];
    for i in (0..N).rev() {
        let mut sum = *b.get(i)?;
        for c in (i + 1)..N {
            sum -= *a.get(i).and_then(|row| row.get(c))? * *x.get(c)?;
        }
        let diagonal = *a.get(i).and_then(|row| row.get(i))?;
        let cell = x.get_mut(i)?;
        *cell = sum / diagonal;
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Accumulate the normal equations of a least-squares problem: one `row . x = rhs` at a time.
struct Normals<const N: usize> {
    a: [[f64; N]; N],
    b: [f64; N],
}

impl<const N: usize> Normals<N> {
    fn new() -> Normals<N> {
        Normals { a: [[0.; N]; N], b: [0.; N] }
    }

    fn add(&mut self, row: [f64; N], rhs: f64) {
        for (i, &ri) in row.iter().enumerate() {
            for (j, &rj) in row.iter().enumerate() {
                if let Some(cell) = self.a.get_mut(i).and_then(|r| r.get_mut(j)) {
                    *cell += ri * rj;
                }
            }
            if let Some(cell) = self.b.get_mut(i) {
                *cell += ri * rhs;
            }
        }
    }

    fn solve(self) -> Option<[f64; N]> {
        solve(self.a, self.b)
    }
}

/// The greatest of `f` over the points, `None` when it is not a finite number.
fn worst(points: &[Vec3], f: impl Fn(Vec3) -> f64) -> Option<f64> {
    let mut deviation = 0.;
    for p in points {
        let d = f(*p);
        if !d.is_finite() {
            return None;
        }
        deviation = f64::max(deviation, d);
    }
    Some(deviation)
}

/// A flat face through the patch's centroid. The triangle normals give its facing, so the plane
/// faces out of the body as the mesh does.
pub(super) fn fit_plane(vertices: &[Vec3], samples: &[Vec3], normals: &[Vec3]) -> Option<(Surface, f64)> {
    let origin = centroid(vertices)?;
    let summed = normals.iter().fold(Vec3::new(0., 0., 0.), |s, n| s + *n).normalized();
    // The plane the points themselves lie in, and not the one their triangles face. A patch that
    // has taken in a few facets of the round it runs into tilts a mean of normals by degrees,
    // because the mean counts facets and not area, and a couple of degrees over the length of a
    // wall is millimetres: the wall then fails to be flat, and comes back as a cylinder the size
    // of a building.
    let normal = match smallest_axis(&covariance(vertices, origin)).or(summed) {
        Some(n) if summed.is_none_or(|s| s.dot(n) >= 0.) => n,
        Some(n) => -n,
        None => return None,
    };
    let deviation = worst(samples, |p| (p - origin).dot(normal).abs())?;
    Some((Surface::Plane { origin, normal }, deviation))
}

/// A sphere through the patch: `|p - c|^2 = r^2` is linear in the centre and `|c|^2 - r^2`.
pub(super) fn fit_sphere(vertices: &[Vec3], samples: &[Vec3], span: f64) -> Option<(Surface, f64)> {
    let mut normals = Normals::<4>::new();
    for p in vertices {
        normals.add([-2. * p.x, -2. * p.y, -2. * p.z, 1.], -p.len2());
    }
    let [x, y, z, k] = normals.solve()?;
    let centre = Vec3::new(x, y, z);
    let radius = (centre.len2() - k).max(0.).sqrt();
    if !radius.is_finite() || radius <= 0. || radius > span * RUNAWAY {
        return None;
    }
    let deviation = worst(samples, |p| ((p - centre).len() - radius).abs())?;
    Some((Surface::Sphere { centre, radius }, deviation))
}

/// A circular cylinder. Every normal of a cylinder is square to its axis, so the axis is the
/// direction the normals lean on least; the radius is then a circle fitted square to it.
pub(super) fn fit_cylinder(vertices: &[Vec3], samples: &[Vec3], normals: &[Vec3], span: f64) -> Option<(Surface, f64)> {
    let axis = smallest_axis(&outer_sum(normals))?;
    let (u, v) = basis(axis)?;
    let mut system = Normals::<3>::new();
    for p in vertices {
        let (x, y) = (p.dot(u), p.dot(v));
        system.add([-2. * x, -2. * y, 1.], -(x * x + y * y));
    }
    let [cu, cv, k] = system.solve()?;
    let radius = (cu * cu + cv * cv - k).max(0.).sqrt();
    if !radius.is_finite() || radius <= 0. || radius > span * RUNAWAY {
        return None;
    }
    let on_axis = u * cu + v * cv;
    let centroid = centroid(vertices)?;
    let origin = on_axis + axis * (centroid - on_axis).dot(axis);
    let deviation = worst(samples, |p| (radial(p, origin, axis) - radius).abs())?;
    Some((Surface::Cylinder { origin, axis, radius }, deviation))
}

/// How far `p` sits from the line through `origin` along unit `axis`.
fn radial(p: Vec3, origin: Vec3, axis: Vec3) -> f64 {
    let d = p - origin;
    (d - axis * d.dot(axis)).len()
}

/// A circular cone. The normals of a cone all make one angle with its axis, so as points on the
/// unit sphere they lie on a circle: the plane through them is square to the axis. Radius against
/// height is then a straight line, its slope the half-angle's tangent, reaching zero at the apex.
pub(super) fn fit_cone(vertices: &[Vec3], samples: &[Vec3], normals: &[Vec3], span: f64) -> Option<(Surface, f64)> {
    let mean = centroid(normals)?;
    let axis = smallest_axis(&covariance(normals, mean))?;
    let origin = centroid(vertices)?;
    let (slope, offset, axis) = match taper(vertices, origin, axis) {
        Some((slope, _)) if slope < 0. => {
            let axis = -axis;
            let (slope, offset) = taper(vertices, origin, axis)?;
            (slope, offset, axis)
        }
        Some((slope, offset)) => (slope, offset, axis),
        None => return None,
    };
    let half_angle = slope.atan();
    if !(0.02..=1.48).contains(&half_angle) {
        return None;
    }
    let apex = origin + axis * (-offset / slope);
    if !apex.is_finite() || (apex - origin).len() > span * RUNAWAY {
        return None;
    }
    // Distance to the cone is the distance in the (height, radius) half-plane to its straight
    // generating line, which the slope shortens.
    let scale = 1. / (1. + slope * slope).sqrt();
    let deviation = worst(samples, |p| {
        let t = (p - origin).dot(axis);
        (radial(p, origin, axis) - (slope * t + offset)).abs() * scale
    })?;
    Some((Surface::Cone { apex, axis, half_angle }, deviation))
}

/// Least squares `radius = slope * height + offset` about `origin` along `axis`.
fn taper(vertices: &[Vec3], origin: Vec3, axis: Vec3) -> Option<(f64, f64)> {
    let mut system = Normals::<2>::new();
    for p in vertices {
        let t = (*p - origin).dot(axis);
        system.add([t, 1.], radial(*p, origin, axis));
    }
    let [slope, offset] = system.solve()?;
    (slope.abs() > 1e-6).then_some((slope, offset))
}

/// A torus about a known axis line. With the axis known the tube is a circle in the (radius,
/// height) half-plane, and fitting that circle is the same algebra as any other.
///
/// The axis cannot be found from the patch itself -- a fillet turns about whatever it runs around
/// -- which is why this one takes the axis instead of fitting it.
pub(super) fn fit_torus(vertices: &[Vec3], samples: &[Vec3], axis: (Vec3, Vec3), tolerance: f64, span: f64) -> Option<(Surface, f64)> {
    let (point, direction) = axis;
    let mut system = Normals::<3>::new();
    for p in vertices {
        let t = (*p - point).dot(direction);
        let r = radial(*p, point, direction);
        system.add([2. * r, 2. * t, 1.], r * r + t * t);
    }
    let [major, height, k] = system.solve()?;
    let minor = (k + major * major + height * height).max(0.).sqrt();
    if !minor.is_finite() || minor <= tolerance || minor > span * RUNAWAY || !(major > 0.) || major > span * RUNAWAY {
        return None;
    }
    let centre = point + direction * height;
    let deviation = worst(samples, |p| {
        let t = (p - point).dot(direction) - height;
        ((radial(p, point, direction) - major).hypot(t) - minor).abs()
    })?;
    Some((Surface::Torus { centre, axis: direction, major, minor }, deviation))
}
