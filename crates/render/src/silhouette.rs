//! Camera-dependent display outlines of a tessellation. These lines never define
//! CAD topology, selection, or any modeling decision.
use std::collections::{BTreeMap, btree_map::Entry};

use solidcore_record::Vec3;
use solidcraft_geom::{Mesh, prelude::*};

/// A hard memory ceiling, in addition to the caller's scene-wide budget.
const MAX_EDGES: usize = 250_000;
/// Unit face normals and max-component-scaled view directions keep dot products
/// bounded. Classify roundoff at tangency consistently, including tiny normal
/// components introduced when computing triangles on a vertical curved face.
const FACING_ROUNDOFF: f64 = 64.0 * f64::EPSILON;

/// Facing uses a constant direction in orthographic views and the actual eye in
/// perspective views. Both vectors are in the mesh's coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SilhouetteView {
    Orthographic { toward_eye: Vec3 },
    Perspective { eye: Vec3 },
}

#[derive(Clone, Copy, Debug)]
pub struct SilhouetteEdge {
    endpoints: [Vec3; 2],
    normals: [Vec3; 2],
}

impl SilhouetteEdge {
    /// The shared triangle edge when the two incident triangles face opposite
    /// sides of the camera. A shared endpoint also anchors both triangle planes.
    pub fn segment(&self, view: SilhouetteView) -> Option<[Vec3; 2]> {
        let direction = match view {
            SilhouetteView::Orthographic { toward_eye } => toward_eye,
            SilhouetteView::Perspective { eye } => eye - self.endpoints[0],
        };
        let direction = scaled(direction)?;
        let a = self.normals[0].dot(direction);
        let b = self.normals[1].dot(direction);
        ((a >= -FACING_ROUNDOFF) != (b >= -FACING_ROUNDOFF)).then_some(self.endpoints)
    }
}

#[derive(Debug, Default)]
pub struct PreparedSilhouettes {
    candidates: Vec<SilhouetteEdge>,
    budget_limited: bool,
}

impl PreparedSilhouettes {
    /// Iterate in bounded batches; no adjacency work is needed when the camera moves.
    pub fn candidates(&self) -> &[SilhouetteEdge] {
        &self.candidates
    }

    /// A capped mesh contributes only retained edges. All triangles still count
    /// toward their adjacency, so an omitted triangle cannot create an outline.
    pub fn budget_limited(&self) -> bool {
        self.budget_limited
    }
}

type PointKey = [u64; 3];
type EdgeKey = [PointKey; 2];

#[derive(Debug)]
struct Incident {
    edge: SilhouetteEdge,
    count: u8,
}

/// Incremental adjacency preparation for the UI thread, including wasm. One
/// budget unit processes a triangle (three bounded tree operations) or removes
/// one adjacency entry. The tree avoids whole-table rehashes during a frame.
#[derive(Debug)]
pub struct SilhouetteBuilder {
    adjacency: BTreeMap<EdgeKey, Incident>,
    discarded: bool,
    next_triangle: usize,
    triangles_done: bool,
    max_edges: usize,
    prepared: PreparedSilhouettes,
}

impl SilhouetteBuilder {
    /// Retain at most `max_edges` adjacency entries, clamped to 250,000. The
    /// resulting candidate count cannot exceed the same allocation ceiling.
    pub fn new(max_edges: usize) -> Self {
        Self {
            adjacency: BTreeMap::new(),
            discarded: false,
            next_triangle: 0,
            triangles_done: false,
            max_edges: max_edges.min(MAX_EDGES),
            prepared: PreparedSilhouettes::default(),
        }
    }

    /// Continue using the same immutable mesh until this returns true. Work is
    /// charged to a caller-owned budget shared by every mesh in the frame.
    pub fn advance(&mut self, mesh: &Mesh, remaining_work: &mut usize) -> bool {
        if self.discarded {
            return self.discard(remaining_work);
        }
        while *remaining_work > 0 {
            if !self.triangles_done {
                let Some(triangle) = mesh.triangles.get(self.next_triangle) else {
                    self.triangles_done = true;
                    continue;
                };
                *remaining_work -= 1;
                self.next_triangle += 1;
                if let Some((points, normal)) = triangle_normal(mesh, *triangle) {
                    for (a, b) in [(points[0], points[1]), (points[1], points[2]), (points[2], points[0])] {
                        let mut key = [point_key(a), point_key(b)];
                        key.sort_unstable();
                        let full = self.adjacency.len() >= self.max_edges;
                        match self.adjacency.entry(key) {
                            Entry::Occupied(mut entry) => {
                                let incident = entry.get_mut();
                                incident.count = incident.count.saturating_add(1);
                                incident.edge.normals[1] = normal;
                            }
                            Entry::Vacant(entry) if !full => {
                                entry.insert(Incident { edge: SilhouetteEdge { endpoints: [a, b], normals: [normal; 2] }, count: 1 });
                            }
                            Entry::Vacant(_) => self.prepared.budget_limited = true,
                        }
                    }
                }
            } else if let Some((_, incident)) = self.adjacency.pop_first() {
                *remaining_work -= 1;
                // Boundaries already have authored display edges. Nonmanifold
                // adjacency is ambiguous; coplanar diagonals are never outlines.
                let [a, b] = incident.edge.normals;
                if incident.count == 2 && a.cross(b).len() > 1e-12 {
                    self.prepared.candidates.push(incident.edge);
                }
            } else {
                return true;
            }
        }
        self.triangles_done && self.adjacency.is_empty()
    }

    /// Cancel without dropping a large tree on the UI thread. Each discarded
    /// adjacency entry costs one work unit; no further mesh triangles are read.
    /// Once the tree is empty, release the contiguous candidate allocation and
    /// return true. Cancellation is permanent, including if `advance` is called.
    pub fn discard(&mut self, remaining_work: &mut usize) -> bool {
        self.discarded = true;
        while *remaining_work > 0 && !self.adjacency.is_empty() {
            let _ = self.adjacency.pop_first();
            *remaining_work -= 1;
        }
        if self.adjacency.is_empty() {
            self.prepared = PreparedSilhouettes::default();
            true
        } else {
            false
        }
    }

    /// Available only after all budgeted preparation has completed, and never
    /// after cancellation.
    pub fn finish(self) -> Option<PreparedSilhouettes> {
        (!self.discarded && self.triangles_done && self.adjacency.is_empty()).then_some(self.prepared)
    }
}

fn point_key(p: Vec3) -> PointKey {
    // Rendering vertices split by normals still share a geometric endpoint.
    // Canonical zero also joins seams with +0 and -0 coordinates.
    [p.x, p.y, p.z].map(|x| if x == 0.0 { 0 } else { x.to_bits() })
}

fn scaled(v: Vec3) -> Option<Vec3> {
    if ![v.x, v.y, v.z].iter().all(|x| x.is_finite()) {
        return None;
    }
    let scale = v.x.abs().max(v.y.abs()).max(v.z.abs());
    (scale > 0.0).then(|| v / scale)
}

fn triangle_normal(mesh: &Mesh, indices: [u32; 3]) -> Option<([Vec3; 3], Vec3)> {
    let [a, b, c] = indices;
    let points = [*mesh.positions.get(a as usize)?, *mesh.positions.get(b as usize)?, *mesh.positions.get(c as usize)?];
    if points.iter().any(|p| ![p.x, p.y, p.z].iter().all(|x| x.is_finite())) {
        return None;
    }
    let u = scaled(points[1] - points[0])?;
    let v = scaled(points[2] - points[0])?;
    let normal = scaled(u.cross(v))?;
    Some((points, normal / normal.len()))
}
