//! The outline of a display mesh's smooth surfaces as a camera sees them: where a curved surface
//! turns from facing the camera to facing away. A sphere's outline is a circle, a cylinder's
//! its two sides. An application draws it where it draws no faces (a wireframe view), since the
//! outline is not an edge of the body.
//!
//! [`Mesh::folds`] finds once, for a mesh, the pairs of triangles that meet smoothly at a mesh
//! edge; [`outline`] then picks, for one view, the folds whose triangles face opposite ways.
//! Display only: an outline is drawn, never measured, and decides nothing.

use std::collections::BTreeMap;

use solidcore_record::Vec3;

use crate::Mesh;
use crate::vec::VecExt;

/// Two triangles of a smooth surface that meet at a mesh edge: the edge's ends and the two
/// triangles' unit normals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fold {
    a: Vec3,
    b: Vec3,
    na: Vec3,
    nb: Vec3,
}

/// Where a camera looks from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Eye {
    /// A perspective view: the eye's position.
    At(Vec3),
    /// An orthographic view: the direction from the model towards the viewer.
    Along(Vec3),
}

impl Eye {
    /// How much a surface with normal `n` at `p` faces the camera: positive towards it.
    fn facing(self, n: Vec3, p: Vec3) -> f64 {
        match self {
            Eye::At(eye) => n.dot(eye - p),
            Eye::Along(back) => n.dot(back),
        }
    }
}

/// Two triangles that turn further apart than this (the cosine of 60 degrees) meet at a crease,
/// a body edge that is drawn as an edge, not as part of an outline.
const CREASE_COS: f64 = 0.5;

/// Meshes with more triangles than this have no folds: finding them would take too long for a
/// view to wait on.
pub const MAX_FOLD_TRIANGLES: usize = 4_000_000;

/// A vertex position's grid cell: a mesh repeats a vertex where its normal changes (along a seam
/// or a face's border), so vertices are matched by where they are.
type Cell = (i64, i64, i64);

/// A mesh edge seen so far: its two ends, its first triangle, its second, and how many
/// triangles share it.
type Shared = (Vec3, Vec3, usize, Option<usize>, u32);

impl Mesh {
    /// The folds of this mesh: each mesh edge shared by exactly two triangles that meet at less
    /// than 60 degrees, in a fixed order. Empty past [`MAX_FOLD_TRIANGLES`].
    pub fn folds(&self) -> Vec<Fold> {
        if self.triangles.len() > MAX_FOLD_TRIANGLES {
            return Vec::new();
        }
        let Some((lo, hi)) = self
            .positions
            .iter()
            .fold(None, |b: Option<(Vec3, Vec3)>, p| {
                Some(b.map_or((*p, *p), |(lo, hi)| (lo.min(*p), hi.max(*p))))
            })
        else {
            return Vec::new();
        };
        let tol = lo.dist(hi).max(1.0) * 1e-7;
        if !tol.is_finite() {
            return Vec::new();
        }
        let cell = |p: Vec3| -> Cell {
            (
                (p.x / tol).round() as i64,
                (p.y / tol).round() as i64,
                (p.z / tol).round() as i64,
            )
        };
        let corner = |k: u32| self.positions.get(k as usize).copied();
        let mut normals: Vec<Option<Vec3>> = Vec::new();
        // Each mesh edge, by its ends' cells in order.
        let mut shared: BTreeMap<(Cell, Cell), Shared> = BTreeMap::new();
        for (ti, t) in self.triangles.iter().enumerate() {
            let (Some(p0), Some(p1), Some(p2)) = (corner(t[0]), corner(t[1]), corner(t[2])) else {
                normals.push(None);
                continue;
            };
            normals.push((p1 - p0).cross(p2 - p0).normalized());
            for (a, b) in [(p0, p1), (p1, p2), (p2, p0)] {
                let (ca, cb) = (cell(a), cell(b));
                if ca == cb {
                    continue;
                }
                let key = if ca < cb { (ca, cb) } else { (cb, ca) };
                shared
                    .entry(key)
                    .and_modify(|e| {
                        e.3 = Some(ti);
                        e.4 = e.4.saturating_add(1);
                    })
                    .or_insert((a, b, ti, None, 1));
            }
        }
        shared
            .into_values()
            .filter_map(|(a, b, first, second, count)| {
                let second = second.filter(|_| count == 2)?;
                let na = normals.get(first).copied().flatten()?;
                let nb = normals.get(second).copied().flatten()?;
                (na.dot(nb) >= CREASE_COS).then_some(Fold { a, b, na, nb })
            })
            .collect()
    }
}

/// The pieces of the outline among `folds` seen from `eye`: each fold whose two triangles face
/// opposite ways, as its edge's two ends.
pub fn outline(folds: &[Fold], eye: Eye) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
    folds.iter().filter_map(move |f| {
        let mid = (f.a + f.b) * 0.5;
        (eye.facing(f.na, mid) * eye.facing(f.nb, mid) < 0.0).then_some((f.a, f.b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube's mesh: 12 flat triangles that meet at 0 or 90 degrees.
    fn cube() -> Mesh {
        let p = Vec3::new;
        let positions = vec![
            p(0., 0., 0.),
            p(1., 0., 0.),
            p(1., 1., 0.),
            p(0., 1., 0.),
            p(0., 0., 1.),
            p(1., 0., 1.),
            p(1., 1., 1.),
            p(0., 1., 1.),
        ];
        let triangles = vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ];
        Mesh {
            normals: vec![Vec3::Z; positions.len()],
            tri_face: vec![0; triangles.len()],
            positions,
            triangles,
            ..Default::default()
        }
    }

    /// A sphere of radius 1 about the origin, `n` steps round and `n / 2` from pole to pole.
    fn sphere(n: usize) -> Mesh {
        let rows = n / 2;
        let mut positions = Vec::new();
        for i in 0..=rows {
            let polar = std::f64::consts::PI * i as f64 / rows as f64;
            for j in 0..n {
                let around = std::f64::consts::TAU * j as f64 / n as f64;
                positions.push(Vec3::new(
                    polar.sin() * around.cos(),
                    polar.sin() * around.sin(),
                    polar.cos(),
                ));
            }
        }
        let at = |i: usize, j: usize| (i * n + j % n) as u32;
        let mut triangles = Vec::new();
        for i in 0..rows {
            for j in 0..n {
                triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
                triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
        Mesh {
            normals: positions.clone(),
            tri_face: vec![0; triangles.len()],
            positions,
            triangles,
            ..Default::default()
        }
    }

    #[test]
    fn a_sphere_has_one_closed_outline_on_it_from_any_side() {
        let folds = sphere(48).folds();
        for eye in [
            Eye::At(Vec3::new(0.0, -10.0, 0.0)),
            Eye::At(Vec3::new(4.0, 3.0, 5.0)),
            Eye::Along(Vec3::new(0.0, 0.0, 1.0)),
        ] {
            let lines: Vec<_> = outline(&folds, eye).collect();
            assert!(lines.len() >= 48, "{eye:?}: {} pieces", lines.len());
            // Every piece is on the sphere, and every end is shared by two pieces: one loop
            // all the way round.
            let mut ends: BTreeMap<Cell, u32> = BTreeMap::new();
            for (a, b) in &lines {
                for p in [a, b] {
                    assert!((p.len() - 1.0).abs() < 1e-9);
                    let c = (
                        (p.x * 1e6).round() as i64,
                        (p.y * 1e6).round() as i64,
                        (p.z * 1e6).round() as i64,
                    );
                    *ends.entry(c).or_default() += 1;
                }
            }
            assert!(ends.values().all(|n| *n == 2), "{eye:?}: one closed loop");
        }
    }

    #[test]
    fn a_sphere_seen_from_near_has_a_smaller_outline_than_from_far() {
        // From a point at distance d from a unit sphere's centre, the outline is the circle of
        // radius sqrt(1 - 1/d²) at 1/d along the view: the orthographic outline (radius 1) is
        // the limit far away. The outline pieces sit on the mesh, so they come within one
        // mesh step of the exact circle.
        let folds = sphere(96).folds();
        for (eye, d) in [
            (Eye::At(Vec3::new(0.0, -2.0, 0.0)), 2.0),
            (Eye::Along(Vec3::new(0.0, -1.0, 0.0)), f64::INFINITY),
        ] {
            let radius = (1.0 - 1.0 / (d * d)).sqrt();
            let step = std::f64::consts::TAU / 96.0;
            for (a, b) in outline(&folds, eye) {
                let mid = (a + b) * 0.5;
                let off_axis = (mid.x * mid.x + mid.z * mid.z).sqrt();
                assert!(
                    (off_axis - radius).abs() < step,
                    "{eye:?}: {off_axis} for {radius}"
                );
            }
        }
    }

    #[test]
    fn a_box_has_no_outline_of_its_own() {
        let folds = cube().folds();
        // The two triangles of each square side meet flat; the sides meet at creases.
        assert_eq!(folds.len(), 6);
        assert_eq!(
            outline(&folds, Eye::At(Vec3::new(3.0, -4.0, 5.0))).count(),
            0
        );
        assert_eq!(
            outline(&folds, Eye::Along(Vec3::new(1.0, 1.0, 1.0))).count(),
            0
        );
    }

    #[test]
    fn folds_come_in_the_same_order_every_time() {
        let mesh = sphere(24);
        assert_eq!(mesh.folds(), mesh.folds());
    }

    #[test]
    fn an_empty_or_broken_mesh_has_no_folds() {
        assert_eq!(Mesh::default().folds(), Vec::new());
        let mut broken = cube();
        broken.triangles.push([0, 1, 99]);
        // The triangle that names a missing vertex is skipped; the rest is as before.
        assert_eq!(broken.folds().len(), 6);
    }
}
