//! The indexed triangle mesh a plan evaluates to, with its edge polylines and how each edge is
//! drawn. Data and presentation only: what a mesh measures (volume, a ray's hits, inside) is the
//! kernel's, never decided from a display mesh.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

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
