//! The indexed triangle mesh a plan evaluates to, with its edge polylines and how each edge is
//! drawn. Data and presentation only: what a mesh measures (volume, a ray's hits, inside) is the
//! kernel's, never decided from a display mesh.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

/// Counts accepted by a consumer of display buffers and their face labels.
#[derive(Clone, Copy, Debug)]
pub struct MeshBufferLimits {
    /// Maximum vertex positions and normals.
    pub positions: usize,
    /// Maximum indexed triangles.
    pub triangles: usize,
    /// Maximum edge polylines.
    pub edges: usize,
    /// Maximum total points across all edge polylines.
    pub stations: usize,
    /// Exclusive upper bound on face labels.
    pub faces: usize,
    /// An explicit label for unowned whole-surface display, allowed by the
    /// consumer's authoritative source rather than inferred from the buffers.
    pub unowned_face: Option<u32>,
}

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
    /// Validate only buffer sizes, finite coordinates, indices and metadata
    /// shape. This performs no geometric measurement or construction.
    #[must_use]
    pub fn buffers_within(&self, limits: MeshBufferLimits) -> bool {
        let finite =
            |point: &Vec3| point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
        let valid_face =
            |face: u32| (face as usize) < limits.faces || limits.unowned_face == Some(face);
        self.positions.len() <= limits.positions
            && self.normals.len() == self.positions.len()
            && self.triangles.len() <= limits.triangles
            && self.tri_face.len() == self.triangles.len()
            && self.edges.len() <= limits.edges
            && [
                self.seams.len(),
                self.smooth_edges.len(),
                self.sharp_edges.len(),
                self.edge_faces.len(),
            ]
            .into_iter()
            .all(|count| count <= self.edges.len())
            && self
                .edges
                .iter()
                .try_fold(0usize, |sum, edge| {
                    sum.checked_add(edge.len())
                        .filter(|&sum| sum <= limits.stations)
                })
                .is_some()
            && self.positions.iter().all(finite)
            && self.normals.iter().all(finite)
            && self
                .triangles
                .iter()
                .flatten()
                .all(|&vertex| (vertex as usize) < self.positions.len())
            && self.tri_face.iter().copied().all(valid_face)
            && self.edges.iter().flatten().all(finite)
            && self
                .edge_faces
                .iter()
                .all(|faces| faces.len() <= 2 && faces.iter().copied().all(valid_face))
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: MeshBufferLimits = MeshBufferLimits {
        positions: 3,
        triangles: 1,
        edges: 2,
        stations: 3,
        faces: 8,
        unowned_face: None,
    };

    fn fixture() -> Mesh {
        Mesh {
            positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            normals: vec![Vec3::Z; 3],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![7],
            edges: vec![vec![Vec3::ZERO, Vec3::X], vec![Vec3::Y]],
            seams: vec![true],
            smooth_edges: vec![false, true],
            sharp_edges: vec![true],
            edge_faces: vec![vec![6, 7], vec![7]],
            ..Mesh::default()
        }
    }

    #[test]
    fn display_buffer_limits_check_each_count_and_preserve_optional_metadata() {
        let mesh = fixture();
        assert!(mesh.buffers_within(LIMITS));
        for limits in [
            MeshBufferLimits {
                positions: 2,
                ..LIMITS
            },
            MeshBufferLimits {
                triangles: 0,
                ..LIMITS
            },
            MeshBufferLimits { edges: 1, ..LIMITS },
            MeshBufferLimits {
                stations: 2,
                ..LIMITS
            },
            MeshBufferLimits { faces: 7, ..LIMITS },
        ] {
            assert!(!mesh.buffers_within(limits));
        }
        let mut optional = mesh.clone();
        optional.seams.clear();
        optional.smooth_edges.clear();
        optional.sharp_edges.clear();
        optional.edge_faces.clear();
        assert!(optional.buffers_within(LIMITS));
        assert!(Mesh::default().buffers_within(MeshBufferLimits {
            positions: 0,
            triangles: 0,
            edges: 0,
            stations: 0,
            faces: 0,
            unowned_face: None
        }));
        optional.triangles[0] = [0, 0, 0];
        assert!(
            optional.buffers_within(LIMITS),
            "geometric regularity is not a buffer property"
        );
    }

    #[test]
    fn display_buffer_limits_refuse_nonfinite_indices_and_malformed_metadata() {
        for defect in 0..13 {
            let mut mesh = fixture();
            match defect {
                0 => mesh.positions[1].x = f64::NAN,
                1 => mesh.normals[1].y = f64::INFINITY,
                2 => mesh.edges[0][1].z = f64::NEG_INFINITY,
                3 => {
                    mesh.normals.pop();
                }
                4 => mesh.triangles[0][2] = 3,
                5 => {
                    mesh.tri_face.clear();
                }
                6 => mesh.tri_face[0] = 8,
                7 => mesh.seams.extend([false, false]),
                8 => mesh.smooth_edges.push(false),
                9 => mesh.sharp_edges.extend([false, false]),
                10 => mesh.edge_faces.push(vec![]),
                11 => mesh.edge_faces[0].push(6),
                _ => mesh.edge_faces[0][1] = 8,
            }
            assert!(!mesh.buffers_within(LIMITS), "defect {defect}");
        }
    }

    #[test]
    fn display_buffer_limits_require_explicit_whole_surface_label_permission() {
        let mut mesh = fixture();
        mesh.tri_face[0] = u32::MAX;
        mesh.edge_faces[0][1] = u32::MAX;
        assert!(!mesh.buffers_within(LIMITS));
        let permitted = MeshBufferLimits {
            unowned_face: Some(u32::MAX),
            ..LIMITS
        };
        assert!(mesh.buffers_within(permitted));
        mesh.tri_face[0] = 8;
        assert!(!mesh.buffers_within(permitted));
        mesh.tri_face[0] = u32::MAX;
        mesh.edge_faces[0][1] = 8;
        assert!(!mesh.buffers_within(permitted));
    }
}
