//! Original median-split BVH. World geometry is rebased around the camera target for f32.
#[path = "planar_patches.rs"]
mod planar_patches;
use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_render::{
    ProductScene,
    job::{CancelToken, ErrorCode, JobError, Result},
    surface::{OpaqueSurface, srgb8_to_linear},
    transmission::Transmission,
};

pub(super) struct Triangle {
    pub p: [Vec3; 3],
    pub indices: [u32; 3],
    pub surface: OpaqueSurface,
    pub transmission: Option<Transmission>,
    pub medium: u32,
}
type MediumEdge = (usize, usize, i32);
pub(super) fn medium_preparation_bytes(scene: &ProductScene) -> u64 {
    // Validation consumes one body's scratch at a time. Opaque/thin-sheet bodies
    // allocate none of the weld table: do not charge their triangles for it.
    let closed = |t: Option<Transmission>| t.is_some_and(|v| v.thin_wall_thickness_mm().is_none());
    let mut total = 0u64;
    let mut largest_medium = 0u64;
    for body in &scene.meshes {
        let triangles = body.mesh.triangles.len() as u64;
        total = total.saturating_add(triangles);
        if closed(body.transmission) || body.faces.iter().any(|face| closed(face.transmission)) {
            largest_medium = largest_medium.max(triangles);
        }
    }
    // Three representatives per triangle at most. The 320 includes positions,
    // full eight-slot hash entries, control bytes and worst-case table rounding.
    let scratch = 3 * (std::mem::size_of::<MediumEdge>() + 320) + std::mem::size_of::<usize>();
    total.saturating_mul(std::mem::size_of::<Triangle>() as u64).saturating_add(largest_medium.saturating_mul(scratch as u64))
}

pub(super) fn planar_preparation_bytes_per_triangle() -> usize {
    let common = std::mem::size_of::<Triangle>() + 2 * std::mem::size_of::<Node>();
    (common + planar_patches::temporary_bytes_per_triangle()).max(common + 100 + 80 + 4)
}

impl Triangle {
    fn centre(&self, axis: usize) -> f64 {
        self.p.iter().map(|p| [p.x, p.y, p.z].get(axis).copied().unwrap_or(0.0)).sum::<f64>() / 3.0
    }
}

#[derive(Default)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    first: u32,
    count: u32,
}

pub(super) struct Upload {
    pub geometry: Vec<u8>,
    pub nodes: Vec<u8>,
    pub epsilon: f32,
}

fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
pub(super) fn reserved<T>(count: usize) -> Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "path tracer allocation failed"))?;
    Ok(v)
}

#[cfg(test)]
pub(super) fn flatten(scene: &ProductScene, origin: Vec3, cancel: &CancelToken) -> Result<Upload> {
    flatten_with_media(scene, origin, cancel, false)
}
pub(super) fn flatten_with_media(scene: &ProductScene, origin: Vec3, cancel: &CancelToken, nested: bool) -> Result<Upload> {
    let count = scene.meshes.iter().map(|m| m.mesh.triangles.len()).sum::<usize>();
    let mut triangles = reserved(count)?;
    let vertex_count = scene.meshes.iter().map(|m| m.mesh.positions.len()).sum::<usize>();
    let mut vertices = reserved(vertex_count * 32)?;
    let mut vertex_offset = 0u32;
    let mut media_bounds = reserved::<super::media_hierarchy::Region>(solidcraft_render::job::MAX_GPU_DIELECTRICS)?;
    for (body_index, body) in scene.meshes.iter().enumerate() {
        cancel.check()?;
        let body_start = triangles.len();
        let mut faces = reserved(body.faces.len())?;
        faces.extend(body.faces.iter());
        faces.sort_unstable_by_key(|f| f.face);
        if faces.windows(2).any(|f| f[0].face == f[1].face) {
            return Err(invalid("duplicate face appearance"));
        }
        let mesh = &body.mesh;
        if mesh.normals.len() != mesh.positions.len() || mesh.tri_face.len() != mesh.triangles.len() {
            return Err(invalid("path tracer requires normals and a face ID for every triangle"));
        }
        for (index, (position, normal)) in mesh.positions.iter().zip(&mesh.normals).enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            let position = *position - origin;
            if !position.is_finite() || [position.x, position.y, position.z].iter().any(|x| x.abs() > 1e7) {
                return Err(invalid("path geometry must be finite and within 1e7 mm of the camera target"));
            }
            let normal = solidcraft_render::surface::unit_direction(*normal).map_err(|_| invalid("invalid mesh normal"))?;
            vector(&mut vertices, position, 0.0);
            vector(&mut vertices, normal, 0.0);
        }
        for (index, indices) in mesh.triangles.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            let face = mesh.tri_face.get(index).copied().ok_or_else(|| invalid("missing triangle face"))?;
            let look = faces.binary_search_by_key(&face, |f| f.face).ok().and_then(|i| faces.get(i));
            let opacity = look.map_or(body.opacity, |f| f.opacity);
            let transmission = look.map_or(body.transmission, |f| f.transmission);
            if opacity != 1.0 && transmission.is_none() {
                return Err(invalid("gpu-path requires explicit transmission for nonopaque preview appearances"));
            }
            let color = look.map_or(body.color, |f| f.color);
            let surface = look
                .map_or(body.surface, |f| f.surface)
                .map(Ok)
                .unwrap_or_else(|| {
                    OpaqueSurface::new(Vec3::new(srgb8_to_linear(color[0]), srgb8_to_linear(color[1]), srgb8_to_linear(color[2])), 0.0, 0.5, 1.5)
                })
                .map_err(|e| invalid(&e.to_string()))?;
            let mut p = [Vec3::ZERO; 3];
            let mut global_indices = [0u32; 3];
            for (corner, &vertex) in indices.iter().enumerate() {
                let position = *mesh.positions.get(vertex as usize).ok_or_else(|| invalid("triangle index out of bounds"))? - origin;
                if !position.is_finite() || [position.x, position.y, position.z].iter().any(|x| x.abs() > 1e7) {
                    return Err(invalid("path geometry must be finite and within 1e7 mm of the camera target"));
                }
                if let Some(v) = p.get_mut(corner) {
                    *v = position;
                }
                if let Some(v) = global_indices.get_mut(corner) {
                    *v = vertex.checked_add(vertex_offset).ok_or_else(|| invalid("vertex offset overflow"))?;
                }
            }
            if (p[1] - p[0]).cross(p[2] - p[0]).len() <= 1e-12 {
                continue;
            }
            if let Some(t) = transmission
                && let Some(scatter) = t.subsurface()
            {
                t.with_subsurface(scatter).map_err(|e| invalid(&e.to_string()))?;
            }
            if transmission.is_some() && surface.clear_coat().is_some() {
                return Err(invalid("clear coat and transmission cannot share a surface"));
            }
            if transmission.is_some() && surface.metallic() != 0.0 {
                return Err(invalid("transmission requires a nonmetallic surface"));
            }
            triangles.push(Triangle { p, indices: global_indices, surface, transmission, medium: body_index as u32 + 1 });
        }
        validate_medium(&triangles[body_start..], cancel)?;
        let body_triangles = &triangles[body_start..];
        if body_triangles.iter().any(|t| t.transmission.is_some()) {
            if media_bounds.len() >= solidcraft_render::job::MAX_GPU_DIELECTRICS {
                return Err(invalid("at most 32 disjoint dielectric bodies are supported"));
            }
            let closed = body_triangles.iter().any(|t| t.transmission.is_some_and(|v| v.thin_wall_thickness_mm().is_none()));
            let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
            let mut hi = -lo;
            for t in body_triangles {
                for p in t.p {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            media_bounds.push(super::media_hierarchy::Region {
                lo,
                hi,
                closed,
                triangles: body_start..triangles.len(),
                medium: body_index as u32 + 1,
            });
        }
        vertex_offset = vertex_offset.checked_add(mesh.positions.len() as u32).ok_or_else(|| invalid("vertex offset overflow"))?;
    }
    let parents = super::media_hierarchy::parents(&media_bounds, &triangles, nested, cancel)?;
    let mut nodes = reserved(triangles.len().saturating_mul(2).max(1))?;
    nodes.push(Node::default());
    build(&mut triangles, 0, &mut nodes, 0, 0, cancel)?;
    let mut representatives = std::collections::BTreeMap::new();
    for (index, t) in triangles.iter().enumerate() {
        if t.transmission.is_some_and(|m| m.thin_wall_thickness_mm().is_none()) {
            representatives.entry(t.medium).or_insert(index as u32 + 1);
        }
    }
    let patches = planar_patches::ids(&triangles, cancel)?;
    let extent = nodes.first().map_or(1.0, |n| n.lo.iter().zip(n.hi).map(|(a, b)| b - a).fold(0.0f32, f32::max));
    let mut geometry = reserved(16 + triangles.len().max(1) * 100 + vertices.len())?;
    let mut materials = reserved(triangles.len().max(1) * 80)?;
    let vertex_word = 4 + triangles.len().max(1) * 4;
    let material_word = vertex_word + vertices.len() / 4;
    let patch_word = material_word + triangles.len().max(1) * 20;
    for word in [triangles.len() as u32, vertex_word as u32, material_word as u32, patch_word as u32] {
        geometry.extend_from_slice(&word.to_le_bytes());
    }
    for (material, t) in triangles.into_iter().enumerate() {
        if material % 1024 == 0 {
            cancel.check()?;
        }
        for index in t.indices {
            geometry.extend_from_slice(&index.to_le_bytes());
        }
        geometry.extend_from_slice(&(material as u32).to_le_bytes());
        vector(&mut materials, t.surface.base_color(), if t.transmission.is_some() { 1.0 } else { 0.0 });
        vector(
            &mut materials,
            Vec3::new(t.surface.metallic(), t.surface.roughness(), t.surface.dielectric_ior()),
            t.transmission.and_then(|t| t.thin_wall_thickness_mm()).unwrap_or(-1.0) as f32,
        );
        vector(&mut materials, t.transmission.map_or(Vec3::ZERO, |t| t.extinction()), t.medium as f32);
        let coat = t.surface.clear_coat().map_or(Vec3::new(0.0, 0.5, 1.5), |c| Vec3::new(c.weight(), c.roughness(), c.ior()));
        vector(&mut materials, coat, if t.transmission.is_some_and(|t| t.shadow_transmission()) { 1.0 } else { 0.0 });
        let scatter = t.transmission.and_then(|m| m.subsurface());
        let parent = parents.iter().find(|(id, _)| *id == t.medium).map_or(0, |(_, id)| *id);
        let parent_triangle =
            if parent == 0 { 0 } else { *representatives.get(&parent).ok_or_else(|| invalid("missing certified parent material"))? };
        if scatter.is_some_and(|m| m.active()) {
            let scale = t.p.iter().map(|p| p.x.abs().max(p.y.abs()).max(p.z.abs())).fold(1.0, f64::max);
            if scatter.is_some_and(|m| m.scattering_per_mm.recip() < scale / 1_048_576.0) {
                return Err(invalid("subsurface mean free path falls below camera-relative GPU position precision"));
            }
        }
        vector(
            &mut materials,
            Vec3::new(
                scatter.map_or(0.0, |s| s.scattering_per_mm),
                scatter.map_or(0.0, |s| s.anisotropy),
                if scene.meshes.get(t.medium.saturating_sub(1) as usize).is_some_and(|body| body.name.starts_with("__render_stage__/")) {
                    1.0
                } else {
                    0.0
                },
            ),
            parent_triangle as f32,
        );
    }
    if geometry.len() == 16 {
        geometry.resize(32, 0);
        materials.resize(80, 0);
    }
    geometry.extend_from_slice(&vertices);
    drop(vertices);
    geometry.extend_from_slice(&materials);
    drop(materials);
    if patches.is_empty() {
        geometry.extend_from_slice(&0u32.to_le_bytes());
    } else {
        for (index, patch) in patches.into_iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            geometry.extend_from_slice(&patch.to_le_bytes());
        }
    }
    let mut node_bytes = reserved(nodes.len() * 32)?;
    for node in nodes {
        for x in node.lo {
            node_bytes.extend_from_slice(&x.to_le_bytes());
        }
        node_bytes.extend_from_slice(&node.first.to_le_bytes());
        for x in node.hi {
            node_bytes.extend_from_slice(&x.to_le_bytes());
        }
        node_bytes.extend_from_slice(&node.count.to_le_bytes());
    }
    Ok(Upload { geometry, nodes: node_bytes, epsilon: (extent * 1e-7).max(1e-5) })
}

const MEDIUM_WELD_MM: f64 = 1e-6;

#[derive(Default)]
struct MediumBucket {
    representatives: [usize; 8],
    len: usize,
}

/// Only topology uses these representatives; every uploaded vertex stays unchanged.
/// Checking neighboring cells avoids rounding-boundary cracks in curved CAD seams.
struct MediumWeld {
    cells: HashMap<[i64; 3], MediumBucket>,
    positions: Vec<Vec3>,
    limit: usize,
}

impl MediumWeld {
    fn new(limit: usize) -> Result<Self> {
        let mut cells = HashMap::new();
        cells.try_reserve(limit).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "glass topology allocation failed"))?;
        Ok(Self { cells, positions: reserved(limit)?, limit })
    }

    fn key(&mut self, point: Vec3) -> Result<usize> {
        let xyz = [point.x, point.y, point.z];
        // Leave room for neighboring cells and refuse saturated integer casts.
        if !xyz.into_iter().all(|v| v.is_finite() && v.abs() <= 1e9) {
            return Err(invalid("glass topology position exceeds finite coordinate bounds"));
        }
        let cell = xyz.map(|v| (v / MEDIUM_WELD_MM).floor() as i64);
        let mut found: Option<usize> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let neighbor = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    let Some(bucket) = self.cells.get(&neighbor) else { continue };
                    for &index in bucket.representatives.iter().take(bucket.len) {
                        let other = self.positions.get(index).ok_or_else(|| invalid("missing glass topology representative"))?;
                        let delta = point - *other;
                        if delta.dot(delta) <= MEDIUM_WELD_MM * MEDIUM_WELD_MM && found.is_none_or(|old| index < old) {
                            found = Some(index);
                        }
                    }
                }
            }
        }
        if let Some(index) = found {
            return Ok(index);
        }
        if self.positions.len() >= self.limit {
            return Err(invalid("glass topology representative budget exceeded"));
        }
        let index = self.positions.len();
        let bucket = self.cells.entry(cell).or_default();
        // Split a unit cell into eight half-size cubes: points sharing one are
        // at most sqrt(3)/2 tolerances apart, so a cell has at most eight reps.
        let slot = bucket.representatives.get_mut(bucket.len).ok_or_else(|| invalid("glass topology cell budget exceeded"))?;
        *slot = index;
        bucket.len += 1;
        self.positions.push(point);
        Ok(index)
    }
}

// Volume transmission needs a consistently oriented closed two-manifold. CAD
// tessellation duplicates face-seam vertices; compare their actual Euclidean
// separation at the existing one-nanometre-in-mm tolerance, without modifying
// uploaded geometry or merging chains beyond a representative's tolerance.
fn validate_medium(triangles: &[Triangle], cancel: &CancelToken) -> Result<()> {
    let Some(first) = triangles.iter().find(|t| t.transmission.is_some_and(|v| v.thin_wall_thickness_mm().is_none())) else {
        return Ok(());
    };
    if triangles.iter().any(|t| t.transmission != first.transmission || t.surface.dielectric_ior() != first.surface.dielectric_ior()) {
        return Err(invalid("a closed glass body must use one homogeneous transmission medium; use separate bodies or thin walls"));
    }
    let vertex_count = triangles.len().checked_mul(3).ok_or_else(|| invalid("glass topology vertex count overflow"))?;
    let mut edges = reserved::<MediumEdge>(vertex_count)?;
    let mut weld = MediumWeld::new(vertex_count)?;
    let mut parents = reserved::<usize>(triangles.len())?;
    parents.extend(0..triangles.len());
    let origin = first.p[0];
    let mut six_volume = 0.0;
    for (index, triangle) in triangles.iter().enumerate() {
        if index % 1024 == 0 {
            cancel.check()?;
        }
        let keys = [weld.key(triangle.p[0])?, weld.key(triangle.p[1])?, weld.key(triangle.p[2])?];
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            if keys[a] == keys[b] {
                return Err(invalid("glass triangle collapses at the 1e-6 mm topology tolerance"));
            }
            let ordinal = i32::try_from(index + 1).map_err(|_| invalid("too many glass triangles"))?;
            edges.push(if keys[a] < keys[b] { (keys[a], keys[b], ordinal) } else { (keys[b], keys[a], -ordinal) });
        }
        six_volume += (triangle.p[0] - origin).dot((triangle.p[1] - origin).cross(triangle.p[2] - origin));
    }
    edges.sort_unstable();
    for edge in edges.chunks(2) {
        let [a, b] = edge else {
            return Err(invalid("volume glass requires a closed mesh; use thin_wall_thickness_mm for sheets"));
        };
        if a.0 != b.0 || a.1 != b.1 || a.2.signum() + b.2.signum() != 0 {
            return Err(invalid("volume glass requires a closed, consistently outward-oriented mesh"));
        }
        let first = medium_root(&mut parents, a.2.unsigned_abs() as usize - 1)?;
        let second = medium_root(&mut parents, b.2.unsigned_abs() as usize - 1)?;
        *parents.get_mut(first).ok_or_else(|| invalid("invalid glass component"))? = second;
    }
    // Groups with four or more coincident directed edges cannot be a two-manifold.
    if edges.windows(3).any(|e| e[0].0 == e[2].0 && e[0].1 == e[2].1) {
        return Err(invalid("nonmanifold glass edge"));
    }
    let component = medium_root(&mut parents, 0)?;
    for index in 0..parents.len() {
        if index % 1024 == 0 {
            cancel.check()?;
        }
        if medium_root(&mut parents, index)? != component {
            return Err(invalid("closed dielectric body must contain exactly one connected shell"));
        }
    }
    if six_volume <= 0.0 || !six_volume.is_finite() {
        return Err(invalid("glass mesh must enclose positive outward-oriented volume"));
    }
    cancel.check()
}

fn medium_root(parents: &mut [usize], mut index: usize) -> Result<usize> {
    loop {
        let parent = *parents.get(index).ok_or_else(|| invalid("invalid glass component index"))?;
        if parent == index {
            return Ok(index);
        }
        let grandparent = *parents.get(parent).ok_or_else(|| invalid("invalid glass component parent"))?;
        *parents.get_mut(index).ok_or_else(|| invalid("invalid glass component index"))? = grandparent;
        index = grandparent;
    }
}

// The shader's traversal stack holds 32 nodes and needs at most depth + 1.
const MAX_BVH_DEPTH: usize = 30;
const SAH_BINS: usize = 16;
type Bounds = ([f32; 3], [f32; 3]);
const EMPTY_BOUNDS: Bounds = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);

fn grow(bounds: &mut Bounds, t: &Triangle) {
    for p in t.p {
        for (axis, value) in [p.x as f32, p.y as f32, p.z as f32].into_iter().enumerate() {
            bounds.0[axis] = bounds.0[axis].min(value);
            bounds.1[axis] = bounds.1[axis].max(value);
        }
    }
}

fn merge(bounds: &mut Bounds, other: &Bounds) {
    for axis in 0..3 {
        bounds.0[axis] = bounds.0[axis].min(other.0[axis]);
        bounds.1[axis] = bounds.1[axis].max(other.1[axis]);
    }
}

fn half_area(bounds: &Bounds) -> f64 {
    let [x, y, z] = [0, 1, 2].map(|axis| f64::from((bounds.1[axis] - bounds.0[axis]).max(0.0)));
    x * y + y * z + z * x
}

// Depth a median split adds below a node of `count` triangles.
fn median_levels(mut count: usize) -> usize {
    let mut levels = 0;
    while count > 4 {
        count = count.div_ceil(2);
        levels += 1;
    }
    levels
}

// Binned surface-area heuristic over triangle centres. A split is accepted only
// if a median split of its larger side still fits MAX_BVH_DEPTH, so the median
// fallback always remains available. Returns the left count after partitioning.
fn sah_split(triangles: &mut [Triangle], depth: usize) -> Option<usize> {
    let mut centre_lo = [f64::INFINITY; 3];
    let mut centre_hi = [f64::NEG_INFINITY; 3];
    for t in triangles.iter() {
        for axis in 0..3 {
            centre_lo[axis] = centre_lo[axis].min(t.centre(axis));
            centre_hi[axis] = centre_hi[axis].max(t.centre(axis));
        }
    }
    let bin = |t: &Triangle, axis: usize| {
        let scaled = (t.centre(axis) - centre_lo[axis]) / (centre_hi[axis] - centre_lo[axis]) * SAH_BINS as f64;
        (scaled as usize).min(SAH_BINS - 1)
    };
    let mut best: Option<(f64, usize, usize)> = None;
    for axis in 0..3 {
        if centre_hi[axis] - centre_lo[axis] <= 0.0 {
            continue;
        }
        let mut counts = [0usize; SAH_BINS];
        let mut boxes = [EMPTY_BOUNDS; SAH_BINS];
        for t in triangles.iter() {
            let b = bin(t, axis);
            counts[b] += 1;
            grow(&mut boxes[b], t);
        }
        let mut right_area = [0.0; SAH_BINS];
        let mut right_count = [0usize; SAH_BINS];
        let mut accumulated = EMPTY_BOUNDS;
        let mut count = 0;
        for b in (1..SAH_BINS).rev() {
            merge(&mut accumulated, &boxes[b]);
            count += counts[b];
            right_area[b] = half_area(&accumulated);
            right_count[b] = count;
        }
        accumulated = EMPTY_BOUNDS;
        count = 0;
        for b in 1..SAH_BINS {
            merge(&mut accumulated, &boxes[b - 1]);
            count += counts[b - 1];
            let right = right_count[b];
            if count == 0 || right == 0 || depth + 1 + median_levels(count.max(right)) > MAX_BVH_DEPTH {
                continue;
            }
            let cost = half_area(&accumulated) * count as f64 + right_area[b] * right as f64;
            if best.is_none_or(|(lowest, _, _)| cost < lowest) {
                best = Some((cost, axis, b));
            }
        }
    }
    let (_, axis, split) = best?;
    let mut middle = 0;
    for i in 0..triangles.len() {
        if bin(&triangles[i], axis) < split {
            triangles.swap(i, middle);
            middle += 1;
        }
    }
    Some(middle)
}

fn build(triangles: &mut [Triangle], offset: usize, nodes: &mut Vec<Node>, index: usize, depth: usize, cancel: &CancelToken) -> Result<()> {
    cancel.check()?;
    let mut bounds = EMPTY_BOUNDS;
    for t in triangles.iter() {
        grow(&mut bounds, t);
    }
    let (lo, hi) = bounds;
    let mut node = Node { lo, hi, first: offset as u32, count: triangles.len() as u32 };
    if triangles.is_empty() {
        node = Node { first: u32::MAX, ..Default::default() };
    }
    if triangles.len() > 4 {
        let middle = match sah_split(triangles, depth) {
            Some(middle) => middle,
            None => {
                let axis = (0..3).max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b]))).unwrap_or(0);
                let middle = triangles.len() / 2;
                triangles.select_nth_unstable_by(middle, |a, b| a.centre(axis).total_cmp(&b.centre(axis)));
                middle
            }
        };
        let first = nodes.len();
        nodes.push(Node::default());
        nodes.push(Node::default());
        node.first = first as u32;
        node.count = 0;
        let (left, right) = triangles.split_at_mut(middle);
        build(left, offset, nodes, first, depth + 1, cancel)?;
        build(right, offset + middle, nodes, first + 1, depth + 1, cancel)?;
    }
    *nodes.get_mut(index).ok_or_else(|| invalid("missing BVH node"))? = node;
    Ok(())
}

pub(super) fn vector(bytes: &mut Vec<u8>, v: Vec3, w: f32) {
    for value in [v.x as f32, v.y as f32, v.z as f32, w] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
#[path = "scene_weld_tests.rs"]
mod weld_tests;

#[cfg(test)]
#[path = "scene_bvh_tests.rs"]
mod bvh_tests;
