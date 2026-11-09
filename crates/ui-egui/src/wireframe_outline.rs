//! Outlines of curved surfaces in the Wireframe visual style (#13347).
//!
//! Wireframe draws a body's edges and no faces. A curved surface's outline, where it turns
//! away from the camera, is not an edge, so a sphere (whose only edge is a hidden seam) drew
//! nothing at all, and a cylinder showed its end circles without its sides. The outline is
//! found on the display mesh: a mesh edge inside a smooth surface lies on it when one of its
//! two triangles faces the camera and the other faces away. The mesh only draws here; nothing
//! about the model is decided from it.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use solidcore_record::Vec3;
use solidcraft_engine::geom::Mesh;
use solidcraft_engine::geom::prelude::*;

/// Two triangles of a smooth surface that meet at a mesh edge: the edge's ends and the
/// triangles' unit normals.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fold {
    a: Vec3,
    b: Vec3,
    na: Vec3,
    nb: Vec3,
}

/// Where the camera looks from: a point (a perspective view) or, for an orthographic view, the
/// direction from the model towards the viewer.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Eye {
    At(Vec3),
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

/// Two triangles that turn further apart than this (cosine of 60 degrees) meet at a crease: a
/// body edge, which Wireframe draws already.
const CREASE_COS: f64 = 0.5;
/// Larger meshes get no outline: finding their folds would hold up the window.
const MAX_TRIANGLES: usize = 4_000_000;

/// The folds of `mesh`: each mesh edge shared by exactly two triangles that meet smoothly.
/// Vertices are matched by position, since a mesh repeats a vertex where its normal changes.
pub(crate) fn folds(mesh: &Mesh) -> Vec<Fold> {
    if mesh.triangles.len() > MAX_TRIANGLES {
        return Vec::new();
    }
    let tol = mesh.bounds().diagonal().max(1.0) * 1e-7;
    let key = |p: Vec3| ((p.x / tol).round() as i64, (p.y / tol).round() as i64, (p.z / tol).round() as i64);
    let corner = |k: u32| mesh.positions.get(k as usize).copied();
    let mut normals = Vec::with_capacity(mesh.triangles.len());
    // Mesh edge (its two ends' keys, in order) → the first triangle, its ends, and how many
    // triangles share it, with the second one.
    let mut shared: HashMap<_, (usize, Vec3, Vec3, Option<usize>, u32)> = HashMap::with_capacity(mesh.triangles.len() * 3 / 2);
    for (ti, t) in mesh.triangles.iter().enumerate() {
        let (Some(p0), Some(p1), Some(p2)) = (corner(t[0]), corner(t[1]), corner(t[2])) else {
            normals.push(None);
            continue;
        };
        normals.push((p1 - p0).cross(p2 - p0).normalized());
        for (a, b) in [(p0, p1), (p1, p2), (p2, p0)] {
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            shared
                .entry(k)
                .and_modify(|e| {
                    e.3 = Some(ti);
                    e.4 = e.4.saturating_add(1);
                })
                .or_insert((ti, a, b, None, 1));
        }
    }
    shared
        .into_values()
        .filter_map(|(first, a, b, second, count)| {
            let second = second.filter(|_| count == 2)?;
            let (na, nb) = (normals.get(first).copied().flatten()?, normals.get(second).copied().flatten()?);
            (na.dot(nb) >= CREASE_COS).then_some(Fold { a, b, na, nb })
        })
        .collect()
}

/// The pieces of the outline among `folds` seen from `eye`: the folds whose triangles face
/// opposite ways.
pub(crate) fn outline(folds: &[Fold], eye: Eye) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
    folds.iter().filter_map(move |f| {
        let mid = (f.a + f.b) * 0.5;
        (eye.facing(f.na, mid) * eye.facing(f.nb, mid) < 0.0).then_some((f.a, f.b))
    })
}

/// [`folds`] of `mesh`, kept while the mesh is: a view change then only looks at the folds.
pub(crate) fn folds_of(mesh: &Arc<Mesh>) -> Arc<[Fold]> {
    type Kept = HashMap<usize, (Weak<Mesh>, Arc<[Fold]>)>;
    static KEPT: LazyLock<Mutex<Kept>> = LazyLock::new(Default::default);
    let at = Arc::as_ptr(mesh) as usize;
    if let Some((_, found)) =
        KEPT.lock().unwrap_or_else(|e| e.into_inner()).get(&at).filter(|(w, _)| w.as_ptr() == Arc::as_ptr(mesh) && w.strong_count() > 0)
    {
        return found.clone();
    }
    let made: Arc<[Fold]> = folds(mesh).into();
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    // Meshes that are gone (an edit, another design) free their folds.
    kept.retain(|_, (w, _)| w.strong_count() > 0);
    kept.insert(at, (Arc::downgrade(mesh), made.clone()));
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube's mesh: 12 flat triangles, every pair at a mesh edge meeting at 0 or 90 degrees.
    fn cube() -> Mesh {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let positions = vec![p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.), p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)];
        let triangles =
            vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        Mesh { normals: vec![Vec3::Z; positions.len()], tri_face: vec![0; triangles.len()], positions, triangles, ..Default::default() }
    }

    /// A sphere of radius 1 about the origin, `n` steps around and `n / 2` from pole to pole.
    fn sphere(n: usize) -> Mesh {
        let rows = n / 2;
        let mut positions = Vec::new();
        for i in 0..=rows {
            let polar = std::f64::consts::PI * i as f64 / rows as f64;
            for j in 0..n {
                let around = std::f64::consts::TAU * j as f64 / n as f64;
                positions.push(Vec3::new(polar.sin() * around.cos(), polar.sin() * around.sin(), polar.cos()));
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
        Mesh { normals: positions.clone(), tri_face: vec![0; triangles.len()], positions, triangles, ..Default::default() }
    }

    #[test]
    fn a_sphere_has_an_outline_all_round_from_any_side() {
        let folds = folds(&sphere(48));
        for eye in [Eye::At(Vec3::new(0.0, -10.0, 0.0)), Eye::At(Vec3::new(4.0, 3.0, 5.0)), Eye::Along(Vec3::new(0.0, 0.0, 1.0))] {
            let lines: Vec<_> = outline(&folds, eye).collect();
            assert!(lines.len() >= 48, "{eye:?}: {} pieces", lines.len());
            // Every piece is on the sphere and on the ring it turns away on, and together they
            // go all the way round (a closed loop: every end is shared by two pieces).
            let mut ends: HashMap<(i64, i64, i64), u32> = HashMap::new();
            for (a, b) in &lines {
                for p in [a, b] {
                    assert!((p.len() - 1.0).abs() < 1e-9);
                    *ends.entry(((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64)).or_default() += 1;
                }
            }
            assert!(ends.values().all(|n| *n == 2), "{eye:?}: the outline is one closed loop");
        }
    }

    #[test]
    fn a_box_has_no_outline_of_its_own() {
        let folds = folds(&cube());
        // The two triangles of each square side meet flat; the sides meet at a crease.
        assert_eq!(folds.len(), 6);
        assert_eq!(outline(&folds, Eye::At(Vec3::new(3.0, -4.0, 5.0))).count(), 0);
        assert_eq!(outline(&folds, Eye::Along(Vec3::new(1.0, 1.0, 1.0))).count(), 0);
    }

    #[test]
    fn folds_are_kept_with_their_mesh() {
        let mesh = Arc::new(sphere(16));
        let first = folds_of(&mesh);
        assert!(Arc::ptr_eq(&first, &folds_of(&mesh)), "the same mesh finds its folds again");
        let other = Arc::new(sphere(16));
        assert!(!Arc::ptr_eq(&first, &folds_of(&other)), "an equal mesh elsewhere has its own");
    }
}
