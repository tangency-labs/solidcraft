//! Mirrored occurrences: a placement kept at the mirror image of another one across a plane.
//!
//! A placement is a rigid move and a reflection is not one, so an occurrence cannot hold the
//! reflection itself. It holds the reflection across the mirror plane followed by a reflection
//! of the part in a plane of its own frame (`local`): two reflections make a rigid move. When
//! the part is symmetric about that plane of its own, the occurrence shows exactly the mirror
//! image of the source. An opposite-hand part is a component whose bodies are the source's
//! reflected in that same plane of its own: placed this way it is the mirror image of any part.
//!
//! The relation is kept in the design ([`crate::joints::Assembly::mirrors`]) and the engine
//! places the mirrored occurrence again whenever the design changes, so it follows its source
//! and the plane.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Mesh, Plane, Vec3};

use crate::{DocError, Document, Mat, ModelState, PlaneRef, Result};

/// The most mirror relations a design keeps.
pub const MAX_MIRRORS: usize = 10_000;

/// An occurrence placed as the mirror image of another one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OccurrenceMirror {
    /// The occurrence this relation places.
    pub occurrence: u64,
    /// The occurrence it mirrors.
    pub source: u64,
    /// The mirror plane, in the frame of component `frame`.
    pub plane: PlaneRef,
    /// The component whose frame `plane` is in (0: the design's own).
    #[serde(default)]
    pub frame: u64,
    /// The reflection in the part's own frame that follows the mirror (a reflection in one of
    /// its mid-planes), so that the placement is rigid.
    pub local: Mat,
}

/// The reflection across `pl`, as a column-major matrix.
pub fn reflection(pl: &Plane) -> Mat {
    let n = pl.normal();
    let d = pl.origin.dot(n);
    let c = [n.x, n.y, n.z];
    let mut m = crate::IDENTITY;
    for (j, col) in m.iter_mut().enumerate().take(3) {
        for (i, cell) in col.iter_mut().enumerate().take(3) {
            *cell = if i == j { 1.0 } else { 0.0 } - 2.0 * c[i] * c[j];
        }
    }
    m[3] = [2.0 * d * n.x, 2.0 * d * n.y, 2.0 * d * n.z, 1.0];
    m
}

/// Whether `m` is a reflection in a plane: orthonormal, turned inside out, finite and in range.
pub fn is_reflection(m: &Mat) -> bool {
    let axis = |i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
    let (x, y, z) = (axis(0), axis(1), axis(2));
    m.iter().flatten().all(|v| v.is_finite())
        && m[3][..3].iter().all(|v| v.abs() <= 1e9)
        && [x, y, z].iter().all(|v| (v.dot(*v) - 1.0).abs() < 1e-6)
        && x.dot(y).abs() < 1e-6
        && x.dot(z).abs() < 1e-6
        && y.dot(z).abs() < 1e-6
        && x.cross(y).dot(z) < 0.0
        && [m[0][3], m[1][3], m[2][3]].iter().all(|v| v.abs() < 1e-9)
        && (m[3][3] - 1.0).abs() < 1e-9
}

/// The mirror plane of `m` in world coordinates.
pub fn world_plane(doc: &Document, st: Option<&ModelState>, plane: &PlaneRef, frame: u64) -> Result<Plane> {
    let (vals, _) = doc.param_values();
    let pl = doc.resolve_plane_in(&vals, plane, 0, st)?;
    let f = doc.component_transform(frame);
    Plane::new(crate::apply_point(&f, pl.origin), crate::apply_vector(&f, pl.x), crate::apply_vector(&f, pl.y))
        .ok_or_else(|| DocError::Invalid("the mirror plane is degenerate".into()))
}

/// Where relation `m` places its occurrence, within that occurrence's parent: the source's
/// world placement reflected across the plane, then reflected in the part's own plane.
pub fn mirrored_transform(doc: &Document, st: Option<&ModelState>, m: &OccurrenceMirror) -> Result<Mat> {
    let lost = |id: u64| DocError::Invalid(format!("occurrence {id} of a mirror no longer exists"));
    let target = doc.occurrences.iter().find(|o| o.id == m.occurrence).ok_or_else(|| lost(m.occurrence))?;
    if !doc.occurrences.iter().any(|o| o.id == m.source) {
        return Err(lost(m.source));
    }
    if !is_reflection(&m.local) {
        return Err(DocError::Invalid("a mirror's own reflection is not a reflection".into()));
    }
    let s = reflection(&world_plane(doc, st, &m.plane, m.frame)?);
    let source = crate::joints::occurrence_world(doc, m.source);
    let world = crate::mat_mul(&crate::mat_mul(&s, &source), &m.local);
    let parent = doc.component_transform(target.parent);
    let inverse = crate::mat_inverse(&parent).ok_or_else(|| DocError::Invalid("the mirrored occurrence's parent is singular".into()))?;
    let local = crate::mat_mul(&inverse, &world);
    if !crate::valid_transfer_matrix(&local) {
        return Err(DocError::Invalid("the mirrored placement is not a rigid move".into()));
    }
    Ok(local)
}

/// The occurrences whose placement the design's mirror relations change, with where they go,
/// and a message for each relation that cannot be followed now. A relation whose occurrence or
/// source is gone is listed in the third value, to be dropped.
#[allow(clippy::type_complexity)]
pub fn follow(doc: &Document, st: Option<&ModelState>) -> (Vec<(u64, Mat)>, Vec<String>, Vec<usize>) {
    let (mut moves, mut notes, mut gone) = (Vec::new(), Vec::new(), Vec::new());
    let exists = |id: u64| doc.occurrences.iter().any(|o| o.id == id);
    for (k, m) in doc.assembly.mirrors.iter().enumerate().take(MAX_MIRRORS) {
        if !exists(m.occurrence) || !exists(m.source) || m.occurrence == m.source {
            gone.push(k);
            continue;
        }
        match mirrored_transform(doc, st, m) {
            Ok(t) => {
                let now = doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.transform);
                let same = now.is_some_and(|n| n.iter().flatten().zip(t.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12));
                if !same {
                    moves.push((m.occurrence, t));
                }
            }
            Err(e) => notes.push(e.to_string()),
        }
    }
    (moves, notes, gone)
}

/// The mesh of everything a component shows (its bodies and, through its occurrences, those of
/// the components inside it), in its own frame.
pub fn component_mesh(doc: &Document, st: &ModelState, component: u64) -> Mesh {
    let mut out = Mesh::default();
    let owners = doc.model_body_components(st);
    let mut stack = vec![(component, crate::IDENTITY, 0usize)];
    while let Some((c, at, depth)) = stack.pop() {
        if depth > 64 || out.triangles.len() > 2_000_000 {
            break;
        }
        for (b, owner) in st.bodies.iter().zip(&owners) {
            if *owner != c {
                continue;
            }
            let m = doc.model_body_offset(b).map_or(at, |offset| crate::mat_mul(&at, offset));
            let mesh = b.mesh();
            let Ok(base) = u32::try_from(out.positions.len()) else { break };
            out.positions.extend(mesh.positions.iter().map(|p| crate::apply_point(&m, *p)));
            out.triangles.extend(mesh.triangles.iter().map(|t| [t[0].saturating_add(base), t[1].saturating_add(base), t[2].saturating_add(base)]));
        }
        for o in doc.occurrences.iter().filter(|o| o.parent == c && c != 0).take(10_000) {
            stack.push((o.component, crate::mat_mul(&at, &o.transform), depth + 1));
        }
    }
    out
}

/// The plane of the part's own frame through the middle of its box, square to `axis` (0, 1,
/// 2: X, Y, Z), or `None` for a part with nothing to show.
pub fn mid_plane(mesh: &Mesh, axis: usize) -> Option<Plane> {
    let b = mesh_bounds(mesh)?;
    let c = (b.0 + b.1) * 0.5;
    let n = [Vec3::X, Vec3::Y, Vec3::Z].get(axis).copied()?;
    Plane::from_normal(c, n)
}

fn mesh_bounds(mesh: &Mesh) -> Option<(Vec3, Vec3)> {
    let mut it = mesh.positions.iter().filter(|p| p.is_finite());
    let first = *it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
}

/// Whether the part `mesh` shows is its own mirror image in `pl`: every reflected corner of its
/// triangles (a sample of them on a big mesh) lies on its surface, within a tolerance for the
/// facets, and its centre of volume lies on the plane.
pub fn symmetric_about(mesh: &Mesh, pl: &Plane) -> bool {
    let Some((lo, hi)) = mesh_bounds(mesh) else { return false };
    let size = (hi - lo).len();
    if !(size.is_finite() && size > 0.0) {
        return false;
    }
    let tol = (size * 2e-3).max(1e-3);
    let n = pl.normal();
    match centre_of_volume(mesh) {
        Some(c) if (c - pl.origin).dot(n).abs() <= tol => {}
        _ => return false,
    }
    let set = solidcraft_geom::mesh_distance::TriangleSet::new(mesh);
    let m = reflection(pl);
    let step = (mesh.positions.len() / 512).max(1);
    mesh.positions.iter().step_by(step).all(|p| {
        let q = crate::apply_point(&m, *p);
        let point = Mesh { positions: vec![q, q, q], triangles: vec![[0, 1, 2]], ..Default::default() };
        set.closest(&solidcraft_geom::mesh_distance::TriangleSet::new(&point), tol).is_some()
    })
}

/// The centre of the volume a closed triangle mesh bounds.
fn centre_of_volume(mesh: &Mesh) -> Option<Vec3> {
    let (lo, _) = mesh_bounds(mesh)?;
    let (mut volume, mut moment) = (0.0, Vec3::ZERO);
    for t in &mesh.triangles {
        let Some([a, b, c]) = mesh.tri(t) else { continue };
        let (a, b, c) = (a - lo, b - lo, c - lo);
        let v = a.dot(b.cross(c)) / 6.0;
        volume += v;
        moment += (a + b + c) * (v / 4.0);
    }
    (volume.abs() > 1e-12 && volume.is_finite()).then(|| lo + moment * (1.0 / volume))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(lo: Vec3, hi: Vec3) -> Mesh {
        let p = |x: bool, y: bool, z: bool| Vec3::new(if x { hi.x } else { lo.x }, if y { hi.y } else { lo.y }, if z { hi.z } else { lo.z });
        let positions = vec![
            p(false, false, false),
            p(true, false, false),
            p(true, true, false),
            p(false, true, false),
            p(false, false, true),
            p(true, false, true),
            p(true, true, true),
            p(false, true, true),
        ];
        let triangles =
            vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        Mesh { positions, triangles, ..Default::default() }
    }

    fn joined(a: Mesh, b: Mesh) -> Mesh {
        let mut out = a;
        let base = out.positions.len() as u32;
        out.positions.extend(b.positions);
        out.triangles.extend(b.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
        out
    }

    #[test]
    fn a_reflection_in_a_plane_is_its_own_inverse_and_turns_inside_out() {
        let pl = Plane::from_normal(Vec3::new(5.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        let m = reflection(&pl);
        assert!(is_reflection(&m));
        assert!(!is_reflection(&crate::IDENTITY));
        let twice = crate::mat_mul(&m, &m);
        assert!(crate::is_identity(&twice), "{twice:?}");
        // A point on the plane stays, a point off it goes to the other side.
        let on = pl.origin + pl.x * 3.0;
        assert!((crate::apply_point(&m, on) - on).len() < 1e-12);
        let off = pl.origin + pl.normal() * 2.0;
        assert!((crate::apply_point(&m, off) - (pl.origin - pl.normal() * 2.0)).len() < 1e-12);
    }

    #[test]
    fn a_block_is_symmetric_about_its_mid_planes_and_an_l_is_not() {
        let plate = block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 5.0));
        for axis in 0..3 {
            assert!(symmetric_about(&plate, &mid_plane(&plate, axis).unwrap()), "axis {axis}");
        }
        // Not about a plane off its middle.
        assert!(!symmetric_about(&plate, &Plane::from_normal(Vec3::new(10.0, 0.0, 0.0), Vec3::X).unwrap()));
        // An L: a base and an upright at one end. Symmetric across Y only.
        let l = joined(block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 5.0)), block(Vec3::new(0.0, 0.0, 5.0), Vec3::new(5.0, 30.0, 35.0)));
        assert!(!symmetric_about(&l, &mid_plane(&l, 0).unwrap()));
        assert!(symmetric_about(&l, &mid_plane(&l, 1).unwrap()));
        assert!(!symmetric_about(&l, &mid_plane(&l, 2).unwrap()));
        assert!(!symmetric_about(&Mesh::default(), &Plane::XY));
    }
}
