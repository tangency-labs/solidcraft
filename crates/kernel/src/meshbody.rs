//! Mesh bodies: triangles without a B-rep (imported 3MF/STL). They render, measure, move and
//! export; B-rep features refuse them.

use solidcraft_geom::prelude::*;
use std::collections::{HashMap, hash_map::Entry};
use std::hash::{Hash, Hasher};

use solidcore_record::Vec3;
use solidcraft_geom::Mesh;

use crate::body::Body;
use crate::{KernelError, Result};

/// Most triangles in one mesh body.
pub const MAX_TRIANGLES: usize = 20_000_000;

/// Indexed triangles with shared vertices, wound counter-clockwise seen from outside.
#[derive(Clone, Debug, Default)]
pub struct TriMesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub corner_normals: Vec<[Vec3; 3]>,
    /// Each side has exactly two incident triangles, running in opposite directions.
    pub(crate) closed: bool,
}

impl TriMesh {
    /// Display/measure mesh: corners split so authored seams stay independent.
    pub(crate) fn to_mesh(&self) -> Mesh {
        let mut out = Mesh::default();
        out.positions.reserve(self.triangles.len() * 3);
        for (index, t) in self.triangles.iter().enumerate() {
            let (Some(a), Some(b), Some(c)) =
                (self.positions.get(t[0] as usize), self.positions.get(t[1] as usize), self.positions.get(t[2] as usize))
            else {
                continue;
            };
            let n = (*b - *a).cross(*c - *a).normalized().unwrap_or(Vec3::Z);
            let base = u32::try_from(out.positions.len()).unwrap_or(u32::MAX);
            out.positions.extend([*a, *b, *c]);
            out.normals.extend(self.corner_normals.get(index).copied().unwrap_or([n; 3]).map(|v| {
                let v = v.normalized().unwrap_or(n);
                match v.dot(n).partial_cmp(&0.0) {
                    Some(std::cmp::Ordering::Greater) => v,
                    Some(std::cmp::Ordering::Less) => -v,
                    _ => n,
                }
            }));
            out.triangles.push([base, base.saturating_add(1), base.saturating_add(2)]);
            out.tri_face.push(0);
        }
        out
    }

    /// Measured properties of a mesh body.
    pub(crate) fn measure(&self) -> (solidcraft_geom::MeshMeasure, solidcraft_geom::Aabb3) {
        let m = self.to_mesh();
        (m.measure(), m.bounds())
    }

    /// Triangle count.
    pub(crate) fn face_count(&self) -> usize {
        self.triangles.len()
    }

    /// Vertex count.
    pub(crate) fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Distinct undirected triangle sides.
    pub(crate) fn edge_count(&self) -> usize {
        let mut set = std::collections::HashSet::new();
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                set.insert(MeshEdge::new(a, b));
            }
        }
        set.len()
    }

    /// Signed volume ×6.
    fn volume6(&self) -> f64 {
        self.triangles
            .iter()
            .filter_map(|t| Some((self.positions.get(t[0] as usize)?, self.positions.get(t[1] as usize)?, self.positions.get(t[2] as usize)?)))
            .map(|(a, b, c)| a.dot(b.cross(*c)))
            .sum()
    }
}

#[derive(PartialEq, Eq)]
struct MeshEdge(u32, u32);

impl MeshEdge {
    fn new(a: u32, b: u32) -> Self {
        Self(a.min(b), a.max(b))
    }
}

impl Hash for MeshEdge {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash both indices in one write while keeping the table entries aligned to u32.
        state.write_u64((u64::from(self.0) << 32) | u64::from(self.1));
    }
}

/// A mesh body from triangles (`positions` in mm). Coincident vertices are merged, degenerate
/// triangles dropped, and a closed mesh with inward winding is turned outward.
pub fn mesh_body(positions: &[Vec3], triangles: &[[u32; 3]]) -> Result<Body> {
    mesh_body_with_normals(positions, triangles, &[])
}

/// Create a reduced copy using SolidCore's checked connectivity, without import-time welding or
/// winding repair. Returns the mesh and its achieved triangle count; it may exceed the target.
pub fn reduce_mesh(body: &Body, target: usize) -> Result<(Body, usize)> {
    let (positions, triangles) = body.triangle_mesh().ok_or_else(|| KernelError::Invalid("select a mesh body".into()))?;
    let points: Vec<[f64; 3]> = positions.iter().map(|p| [p.x, p.y, p.z]).collect();
    let normals: Vec<[f64; 3]> = body.triangle_corner_normals().unwrap_or_default().iter().flatten().map(|n| [n.x, n.y, n.z]).collect();
    let run = || solidcore_ops::reduce_mesh(&points, triangles.as_flattened(), (!normals.is_empty()).then_some(normals.as_slice()), target);
    let reduction = match solidcraft_smlib::stop::token() {
        Some(token) => solidcore_ops::cancel::within(&token, run),
        None => run(),
    }
    .map_err(|e| match e {
        e @ solidcore_ops::Error::Refused { .. } => KernelError::Invalid(format!("mesh reduction failed: {e}")),
        solidcore_ops::Error::Internal(m) => KernelError::Internal(format!("mesh reduction: {m}")),
        e => KernelError::Failed(format!("mesh reduction failed: {e}")),
    })?;
    let mesh = TriMesh {
        positions: reduction.positions.into_iter().map(Vec3::from).collect(),
        triangles: reduction.indices.as_chunks::<3>().0.to_vec(),
        corner_normals: reduction.corner_normals.as_deref().unwrap_or_default().as_chunks::<3>().0.iter().map(|n| n.map(Vec3::from)).collect(),
        // Reduction preserves the boundary and edge incidence.
        closed: body.is_closed_mesh(),
    };
    Ok((Body::of_mesh(mesh).with_color(body.color()), reduction.reduced_triangles))
}

/// A mesh with optional independent, finite nonzero shading normals per triangle corner.
/// Welding and winding repair affect geometric indices; seams never affect connectivity.
pub fn mesh_body_with_normals(positions: &[Vec3], triangles: &[[u32; 3]], normals: &[[Vec3; 3]]) -> Result<Body> {
    build_mesh(positions.iter().copied(), triangles, normals.iter().copied())
}

fn build_mesh(
    positions: impl ExactSizeIterator<Item = Vec3> + Clone,
    triangles: &[[u32; 3]],
    normals: impl ExactSizeIterator<Item = [Vec3; 3]> + Clone,
) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "mesh_body");
    weld_mesh(positions, triangles, normals).map(finish_mesh)
}

fn weld_mesh(
    positions: impl ExactSizeIterator<Item = Vec3> + Clone,
    triangles: &[[u32; 3]],
    mut normals: impl ExactSizeIterator<Item = [Vec3; 3]> + Clone,
) -> Result<TriMesh> {
    if triangles.len() > MAX_TRIANGLES {
        return Err(KernelError::Invalid(format!("mesh with {} triangles (limit {MAX_TRIANGLES})", triangles.len())));
    }
    let normal_count = normals.len();
    if normal_count != 0 && (normal_count != triangles.len() || normals.clone().flatten().any(|n| !n.is_finite() || n.normalized().is_none())) {
        return Err(KernelError::Invalid("mesh shading normals must contain three finite nonzero vectors per triangle".into()));
    }
    if positions.clone().any(|p| !(p.is_finite() && p.x.abs() < 1e9 && p.y.abs() < 1e9 && p.z.abs() < 1e9)) {
        return Err(KernelError::Invalid("mesh with non-finite or huge coordinates".into()));
    }
    // Merge vertices that are exactly equal (bit pattern of the coordinates).
    let mut map: HashMap<[u64; 3], u32> = HashMap::new();
    let mut remap = Vec::new();
    remap.try_reserve_exact(positions.len()).map_err(|_| KernelError::Invalid("could not allocate mesh vertex remapping".into()))?;
    let mut out = TriMesh::default();
    for p in positions {
        let key = [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
        let next = u32::try_from(out.positions.len()).map_err(|_| KernelError::Invalid("too many vertices".into()))?;
        let i = *map.entry(key).or_insert_with(|| {
            out.positions.push(p);
            next
        });
        remap.push(i);
    }
    drop(map);
    for &[a, b, c] in triangles {
        let (Some(&a), Some(&b), Some(&c)) = (remap.get(a as usize), remap.get(b as usize), remap.get(c as usize)) else {
            return Err(KernelError::Invalid("mesh triangle refers to a missing vertex".into()));
        };
        let normal = normals.next();
        if a == b || b == c || c == a {
            continue;
        }
        out.triangles.push([a, b, c]);
        if let Some(n) = normal {
            out.corner_normals.push(n);
        }
    }
    drop(remap);
    if out.triangles.is_empty() {
        return Err(KernelError::Invalid("mesh has no triangles".into()));
    }
    Ok(out)
}

fn finish_mesh(mut out: TriMesh) -> Body {
    out.closed = unify_winding(&mut out);
    if out.closed && out.volume6() < 0.0 {
        for (i, t) in out.triangles.iter_mut().enumerate() {
            t.swap(1, 2);
            if let Some(n) = out.corner_normals.get_mut(i) {
                n.swap(1, 2);
            }
        }
    }
    Body::of_mesh(out)
}

/// Wind every connected piece one way: a triangle that runs a side in the same direction as
/// its neighbour is turned over (files with stray inverted facets). Sides shared by more than
/// two triangles are left alone. Returns whether every side has two opposite runs after repair.
fn unify_winding(m: &mut TriMesh) -> bool {
    const UNPAIRED: u32 = u32::MAX;
    // A side fits in a u32 because construction caps the mesh at MAX_TRIANGLES.
    const _: () = assert!(MAX_TRIANGLES < (u32::MAX / 3) as usize);
    let mut links = vec![[UNPAIRED; 3]; m.triangles.len()];
    let side_links = links.as_flattened_mut();
    let mut first_sides = HashMap::new();
    for (triangle, &[a, b, c]) in (0u32..).zip(&m.triangles) {
        for (side, (a, b)) in (0u32..).zip([(a, b), (b, c), (c, a)]) {
            let current = triangle * 3 + side;
            let first = match first_sides.entry(MeshEdge::new(a, b)) {
                Entry::Vacant(entry) => {
                    entry.insert(current);
                    continue;
                }
                Entry::Occupied(entry) => entry.into_mut(),
            };
            if *first == UNPAIRED {
                continue;
            }
            let Some(previous) = side_links.get_mut(*first as usize) else { continue };
            if *previous == UNPAIRED {
                *previous = current;
                if let Some(link) = side_links.get_mut(current as usize) {
                    *link = *first;
                }
            } else {
                // The third use disconnects the first two; further uses stay disconnected.
                let second = std::mem::replace(previous, UNPAIRED);
                if let Some(link) = side_links.get_mut(second as usize) {
                    *link = UNPAIRED;
                }
                *first = UNPAIRED;
            }
        }
    }
    // The walk follows contiguous per-triangle links without visiting the edge table again.
    drop(first_sides);
    let runs = |&[u, v, w]: &[u32; 3], a, b| [(u, v), (v, w), (w, u)].contains(&(a, b));
    let mut done = vec![false; m.triangles.len()];
    let mut stack = Vec::new();
    let mut closed = true;
    for start in 0..m.triangles.len() {
        if done[start] {
            continue;
        }
        done[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let Some(&[a, b, c]) = m.triangles.get(i) else { continue };
            let Some(&sides) = links.get(i) else { continue };
            for ((a, b), side) in [(a, b), (b, c), (c, a)].into_iter().zip(sides) {
                if side == UNPAIRED {
                    closed = false;
                    continue;
                }
                let neighbor = (side / 3) as usize;
                let Some(visited) = done.get_mut(neighbor) else { continue };
                if *visited {
                    // A visited triangle has its final winding. Two equal runs here mean
                    // the component cannot be oriented consistently.
                    closed = closed && m.triangles.get(neighbor).is_some_and(|triangle| !runs(triangle, a, b));
                    continue;
                }
                *visited = true;
                if let Some(triangle) = m.triangles.get_mut(neighbor)
                    && runs(triangle, a, b)
                {
                    triangle.swap(1, 2);
                    if let Some(links) = links.get_mut(neighbor) {
                        // [a,b], [b,c], [c,a] become the reversed sides in reverse order.
                        links.swap(0, 2);
                    }
                    if let Some(normals) = m.corner_normals.get_mut(neighbor) {
                        normals.swap(1, 2);
                    }
                }
                stack.push(neighbor);
            }
        }
    }
    closed
}

/// Where the ray from `o` along unit `d` first meets mesh body `m` (its triangles are its exact
/// shape): the distance along the ray and the hit triangle's own unit normal. A closed mesh is
/// wound outward, so its normal is the outward one, also where a ray from inside leaves it; an
/// open mesh has no outside, so its normal is turned against the ray. `None` where it meets
/// nothing.
pub(crate) fn mesh_ray(m: &TriMesh, o: Vec3, d: Vec3) -> Option<(f64, Vec3)> {
    let mesh = m.to_mesh();
    let (t, ti) = mesh.raycast(o, d)?;
    let [a, b, c] = mesh.tri(mesh.triangles.get(ti)?)?;
    let n = (b - a).cross(c - a).normalized()?;
    Some((t, if !m.closed && n.dot(d) > 0.0 { n * -1.0 } else { n }))
}

/// Where the line through `o` along unit `d` meets a mesh body's triangles, the meeting nearest
/// `o` on either side, as a signed distance along `d`.
pub(crate) fn mesh_line_hit(m: &TriMesh, o: Vec3, d: Vec3) -> Option<f64> {
    let mesh = m.to_mesh();
    let ahead = mesh.raycast(o, d).map(|(t, _)| t);
    let behind = mesh.raycast(o, d * -1.0).map(|(t, _)| -t);
    match (ahead, behind) {
        (Some(a), Some(b)) => Some(if b.abs() < a.abs() { b } else { a }),
        (a, b) => a.or(b),
    }
}

impl Body {
    /// Build from the packed coordinates and corner normals stored in a design.
    pub fn from_packed_mesh(positions: &[[f64; 3]], triangles: &[[u32; 3]], normals: &[[f64; 9]]) -> Result<Self> {
        let positions = positions.iter().copied().map(Vec3::from);
        let normals = normals.iter().map(|&[ax, ay, az, bx, by, bz, cx, cy, cz]| [[ax, ay, az], [bx, by, bz], [cx, cy, cz]].map(Vec3::from));
        build_mesh(positions, triangles, normals)
    }

    /// Make a flat imported body and the same source with shared vertex indices. The source
    /// retains its facet order and winding, so it can be rebuilt or shaded the usual way.
    pub fn from_packed_flat_mesh_indexed(positions: std::sync::Arc<[f64]>, triangles: std::sync::Arc<[u32]>) -> Result<(Self, Vec<f64>, Vec<u32>)> {
        let _span = solidcraft_profile::span("kernel", "mesh_body");
        let mesh = weld_mesh(positions.as_chunks::<3>().0.iter().copied().map(Vec3::from), triangles.as_chunks::<3>().0, std::iter::empty())?;
        drop(positions);
        drop(triangles);
        let points = mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
        let facets = mesh.triangles.iter().flatten().copied().collect();
        Ok((finish_mesh(mesh), points, facets))
    }

    /// Shared-vertex triangles of a mesh body.
    pub fn triangle_mesh(&self) -> Option<(&[Vec3], &[[u32; 3]])> {
        self.mesh.as_ref().map(|m| (m.positions.as_slice(), m.triangles.as_slice()))
    }
    /// Shading metadata independent of the mesh body's certified geometric triangles.
    pub fn triangle_corner_normals(&self) -> Option<&[[Vec3; 3]]> {
        self.mesh.as_ref().map(|m| m.corner_normals.as_slice())
    }
    /// Does a mesh body enclose a volume (every side shared by two triangles)?
    pub fn is_closed_mesh(&self) -> bool {
        self.mesh.as_ref().is_some_and(|m| m.closed)
    }

    /// A mesh body moved by a column-major affine matrix (reflections keep outward winding).
    pub(crate) fn mesh_transformed(&self, m: &[[f64; 4]; 4]) -> Option<Body> {
        let tm = self.mesh.as_ref()?;
        let f = |p: Vec3| {
            Vec3::new(
                m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
                m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
                m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
            )
        };
        let det = m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2]) - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
            + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2]);
        // Cofactors have the inverse-transpose direction after the determinant's
        // sign is removed. Avoid division by a tiny/zero determinant; display
        // conversion normalizes and falls back to geometry for a collapsed vector.
        let a = Vec3::new(m[0][0], m[0][1], m[0][2]);
        let b = Vec3::new(m[1][0], m[1][1], m[1][2]);
        let c = Vec3::new(m[2][0], m[2][1], m[2][2]);
        let normal = |n: Vec3| {
            let n = (b.cross(c) * n.x + c.cross(a) * n.y + a.cross(b) * n.z) * if det < 0.0 { -1.0 } else { 1.0 };
            let scale = n.x.abs().max(n.y.abs()).max(n.z.abs());
            // Normal directions are independent of the scale's magnitude. Rescale
            // before normalization so a small valid mesh keeps its authored normals.
            if scale > 0.0 && scale.is_finite() { (n / scale).normalized().unwrap_or(n) } else { n }
        };
        let mut out = TriMesh {
            positions: tm.positions.iter().map(|p| f(*p)).collect(),
            triangles: tm.triangles.clone(),
            corner_normals: tm.corner_normals.iter().map(|n| n.map(normal)).collect(),
            closed: tm.closed,
        };
        if det < 0.0 {
            for (i, t) in out.triangles.iter_mut().enumerate() {
                t.swap(1, 2);
                if let Some(n) = out.corner_normals.get_mut(i) {
                    n.swap(1, 2);
                }
            }
        }
        let mut placed = Body::of_mesh(out);
        placed.color = self.color;
        placed.paint = self.paint.clone();
        placed.splits = self.splits.clone();
        placed.regularization_protected = self.regularization_protected;
        Some(placed)
    }
}

#[cfg(test)]
mod reduction_tests {
    use super::*;

    #[test]
    fn stopped_reduction_is_refused_and_scope_is_restored() {
        let points =
            [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 1.0, 0.0)];
        let body = mesh_body(&points, &[[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]).unwrap();
        let stop = crate::Stop::new();
        stop.stop();
        let typed = stop.within(|| {
            let token = solidcraft_smlib::stop::token().unwrap();
            solidcore_ops::cancel::within(&token, || {
                solidcore_ops::reduce_mesh(
                    &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0], [1.0, 1.0, 0.0]],
                    &[0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4],
                    None,
                    2,
                )
            })
        });
        assert!(matches!(typed, Err(solidcore_ops::Error::Cancelled)));
        let error = stop.within(|| reduce_mesh(&body, 2)).unwrap_err();
        assert!(matches!(&error, KernelError::Failed(_)) && error.to_string().to_lowercase().contains("cancel"));
        assert!(reduce_mesh(&body, 2).is_ok_and(|(reduced, count)| count < 4 && !reduced.is_closed_mesh()));
    }
}
