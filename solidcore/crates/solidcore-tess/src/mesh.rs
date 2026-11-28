//! The indexed triangle mesh a plan evaluates to, with its edge polylines and how each edge is
//! drawn. Data and presentation only: what a mesh measures (volume, a ray's hits, inside) is the
//! kernel's, never decided from a display mesh.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

use crate::vec::VecExt as _;

/// How many times [`Mesh::pieces_off`] halves a triangle that lies partly on the surface: each
/// halving splits the longest side, so 24 takes a sliver 2^12 times the resolution long down to it.
const OFF_SPLITS: u32 = 24;

/// How many pieces [`Mesh::pieces_off`] may look at for one triangle of the mesh: past it the
/// pieces left are decided by their centroids. Along the line where a surface ends the split
/// pieces grow about as the square root of their count, so this is far more than that needs; a
/// test that flips back and forth all over a triangle would otherwise split it into millions.
const OFF_BUDGET: usize = 4096;

/// A triangle's corners, each with its normal.
pub type NormalTriangle = [(Vec3, Vec3); 3];

/// An indexed triangle mesh with per-vertex normals, a face id per triangle (the B-rep face it
/// came from) and edge polylines (B-rep edges for display and picking, or mesh outlines).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Vertex normals, one per position.
    pub normals: Vec<Vec3>,
    /// Triangles as vertex index triples (counter-clockwise seen from outside).
    pub triangles: Vec<[u32; 3]>,
    /// B-rep face index per triangle.
    pub tri_face: Vec<u32>,
    /// Edge polylines; mesh outlines have `display_only_edges` set.
    pub edges: Vec<Vec<Vec3>>,
    /// These polylines belong to a triangle mesh, not selectable B-rep edges. Rebuilt with
    /// the display mesh; never part of the saved design's geometry.
    #[serde(skip)]
    pub display_only_edges: bool,
    /// Per edge: a seam between two pieces of the same smooth surface (not drawn).
    #[serde(default)]
    pub seams: Vec<bool>,
    /// Display-only tangent joins. Edge identities, picking and face groups stay intact.
    #[serde(skip)]
    pub smooth_edges: Vec<bool>,
    /// Display-protected boundaries: certified current creases or authored splits.
    /// Overrides coarse seam fits; not a geometric/topological authority.
    #[serde(skip)]
    pub sharp_edges: Vec<bool>,
    /// Per edge: the B-rep faces it bounds (one or two).
    #[serde(default)]
    pub edge_faces: Vec<Vec<u32>>,
}

impl Mesh {
    /// The number of triangles stored in the mesh.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Visibility in shaded edge presentation, independent of selection/topology.
    #[must_use]
    pub fn shaded_edge_visible(&self, index: usize) -> bool {
        self.edge_visible(index) && !self.smooth_edges.get(index).copied().unwrap_or(false)
    }

    /// Visibility in wireframe, including triangle-mesh outlines without B-rep identities.
    #[must_use]
    pub fn edge_visible(&self, index: usize) -> bool {
        !self.seams.get(index).copied().unwrap_or(false)
            || self.sharp_edges.get(index).copied().unwrap_or(false)
    }

    /// Whether edge `index` can be picked: a visible B-rep edge, not a mesh outline.
    #[must_use]
    pub fn edge_pickable(&self, index: usize) -> bool {
        !self.display_only_edges && self.edge_visible(index)
    }

    /// Edges bounding face `f`.
    #[must_use]
    pub fn face_edges(&self, f: u32) -> Vec<usize> {
        self.edge_faces
            .iter()
            .enumerate()
            .filter(|(_, fs)| fs.contains(&f))
            .map(|(i, _)| i)
            .collect()
    }

    /// The part of this mesh off a surface, as triangles with their corner normals: `on` says
    /// whether a point lies on the surface (within its own tolerance). A triangle whose corners,
    /// side middles and centroid are all on it is left out and one with none of them on it is
    /// kept whole; one that is both is halved across its longest side and each half asked again,
    /// down to `resolution`, where its centroid decides (as it does past a bounded number of
    /// halvings and of pieces per triangle). Only the triangles along the line where the surface
    /// ends are split, so a long facet the surface covers in part keeps only its part off it.
    /// Normals of new corners are their side's corners' averaged. A triangle with a corner that
    /// has no position or no normal is left out. For display (a preview's ghost of removed
    /// material, say); nothing measured is decided from it.
    #[must_use]
    pub fn pieces_off(&self, on: impl Fn(Vec3) -> bool, resolution: f64) -> Vec<NormalTriangle> {
        let mut out = Vec::new();
        for t in &self.triangles {
            let corner = |k: u32| {
                Some((
                    *self.positions.get(k as usize)?,
                    *self.normals.get(k as usize)?,
                ))
            };
            if let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) {
                let mut budget = OFF_BUDGET;
                pieces_off(&on, resolution, [a, b, c], 0, &mut budget, &mut out);
            }
        }
        out
    }

    /// The corners of triangle `t`, `None` where an index is out of range.
    #[must_use]
    pub fn tri(&self, t: &[u32; 3]) -> Option<[Vec3; 3]> {
        Some([
            *self.positions.get(t[0] as usize)?,
            *self.positions.get(t[1] as usize)?,
            *self.positions.get(t[2] as usize)?,
        ])
    }
}

/// [`Mesh::pieces_off`] for one triangle `t`, `depth` halvings in, with `budget` pieces of its
/// mesh triangle left to look at.
fn pieces_off(
    on: &impl Fn(Vec3) -> bool,
    resolution: f64,
    t: NormalTriangle,
    depth: u32,
    budget: &mut usize,
    out: &mut Vec<NormalTriangle>,
) {
    *budget = budget.saturating_sub(1);
    let [(a, _), (b, _), (c, _)] = t;
    let probes = [
        a,
        b,
        c,
        (a + b) / 2.0,
        (b + c) / 2.0,
        (c + a) / 2.0,
        (a + b + c) / 3.0,
    ];
    let hits = probes.map(on);
    if hits.iter().all(|h| *h) {
        return;
    }
    let mixed = hits.iter().any(|h| *h);
    let sides = [a.dist(b), b.dist(c), c.dist(a)];
    let longest = sides.iter().copied().fold(0.0, f64::max);
    if !mixed || depth >= OFF_SPLITS || *budget < 2 || longest <= resolution {
        // Wholly off, or as small as asked (or as far as it may go): then its centroid decides.
        if !mixed || !hits[6] {
            out.push(t);
        }
        return;
    }
    // Halve across the longest side, from its middle to the corner opposite: the halves keep
    // the triangle's winding.
    let k = sides.iter().position(|s| *s >= longest).unwrap_or(0);
    let (Some(&(pi, ni)), Some(&(pj, nj)), Some(&o)) =
        (t.get(k), t.get((k + 1) % 3), t.get((k + 2) % 3))
    else {
        return;
    };
    let mid = ((pi + pj) / 2.0, (ni + nj).normalized().unwrap_or(ni));
    pieces_off(on, resolution, [(pi, ni), mid, o], depth + 1, budget, out);
    pieces_off(on, resolution, [mid, (pj, nj), o], depth + 1, budget, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A strip from z = 0 to z = 10 with the surface below z = 5: what is kept lies above it,
    /// to within the resolution, and covers the upper half's area.
    #[test]
    fn pieces_off_keep_only_what_is_off_the_surface() {
        let toward = Vec3::new(0.0, -1.0, 0.0);
        let mesh = Mesh {
            positions: vec![
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 10.0),
            ],
            normals: vec![toward; 3],
            triangles: vec![[0, 1, 2]],
            ..Mesh::default()
        };
        let pieces = mesh.pieces_off(|p| p.z < 5.0, 0.01);
        assert!(!pieces.is_empty());
        let lowest = pieces
            .iter()
            .flatten()
            .map(|(p, _)| p.z)
            .fold(f64::INFINITY, f64::min);
        assert!(lowest > 5.0 - 0.02, "a piece reaches down to z = {lowest}");
        let area: f64 = pieces
            .iter()
            .map(|[(a, _), (b, _), (c, _)]| (*b - *a).cross(*c - *a).len() / 2.0)
            .sum();
        // The part above z = 5: a triangle with legs 0.5 and 5, area 1.25.
        assert!((area - 1.25).abs() < 0.02, "area {area}");
        // Same winding as the input: normals of the pieces' planes point where the input's does.
        let n =
            (mesh.positions[1] - mesh.positions[0]).cross(mesh.positions[2] - mesh.positions[0]);
        assert!(
            pieces
                .iter()
                .all(|[(a, _), (b, _), (c, _)]| (*b - *a).cross(*c - *a).dot(n) > 0.0)
        );
        assert!(
            mesh.pieces_off(|_| true, 0.01).is_empty(),
            "all on the surface: nothing"
        );
        assert_eq!(
            mesh.pieces_off(|_| false, 0.01).len(),
            1,
            "all off: the triangle whole"
        );
    }

    /// A test that flips back and forth all over the triangle (a fine checkerboard) is split
    /// only as far as the budget allows, not into millions of pieces.
    #[test]
    fn pieces_off_stay_bounded_on_a_noisy_surface() {
        let n = Vec3::new(0.0, 0.0, 1.0);
        let mesh = Mesh {
            positions: vec![
                Vec3::ZERO,
                Vec3::new(100.0, 0.0, 0.0),
                Vec3::new(0.0, 100.0, 0.0),
            ],
            normals: vec![n; 3],
            triangles: vec![[0, 1, 2]],
            ..Mesh::default()
        };
        let checker = |p: Vec3| ((p.x * 3.7).floor() + (p.y * 3.7).floor()).rem_euclid(2.0) < 1.0;
        let pieces = mesh.pieces_off(checker, 1e-9);
        assert!(!pieces.is_empty());
        assert!(pieces.len() <= OFF_BUDGET, "{} pieces", pieces.len());
    }
}
