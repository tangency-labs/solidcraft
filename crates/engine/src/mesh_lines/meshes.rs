//! Export-only mesh instances. Keep one geometry buffer per unambiguous definition, while
//! retaining the current placed body's edge choices (tangent classification can vary after
//! placement). Never modify the document, fuse parts, or share across separate exports.
use std::collections::BTreeMap;
use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use solidcraft_doc::{IDENTITY, Mat, ModelBody, mat_inverse, mat_mul};
use solidcraft_geom::{Aabb3, Mesh, Vec3};
use solidcraft_kernel::{KernelError, map_with_local_pools};

use super::progress::Stage;
use crate::params::bad;
use crate::{Result, Session};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const WORKERS: usize = 8;
#[cfg(target_arch = "wasm32")]
pub(crate) const WORKERS: usize = 1;
const MESH_BYTES: usize = 512 * 1024 * 1024;
const MAX_BODIES: usize = 100_000;

pub(crate) struct Placement {
    pub mesh: usize,
    pub transform: Mat,
    /// Per edge: whether it is drawn.
    pub visibility: Vec<bool>,
    /// Per edge, when sources are asked for: whether it is a tangent edge.
    pub tangent: Vec<bool>,
    /// The pairs of faces every edge between which is drawn, sorted.
    pub joins: Vec<[u32; 2]>,
}
pub(crate) struct Scene {
    pub meshes: Vec<Arc<Mesh>>,
    pub placements: Vec<Placement>,
    pub triangles: usize,
    pub bytes: usize,
    /// Whether each line keeps its source ([`Ask::sources`]).
    pub sources: bool,
}

/// What a caller asks of the meshes besides the chord. The view DXF asks for neither.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ask {
    /// Draw the edges where faces meet smoothly too.
    pub tangent_edges: bool,
    /// Each line keeps its placement and its edge or silhouette face.
    pub sources: bool,
}

struct Group {
    first: usize,
    inverse: Mat,
}
struct Item<'a> {
    body: &'a ModelBody,
    group: usize,
    keep: bool,
    transform: Mat,
}
struct Prepared {
    mesh: Option<Arc<Mesh>>,
    visibility: Vec<bool>,
    tangent: Vec<bool>,
    joins: Vec<[u32; 2]>,
    triangles: usize,
}

pub(super) fn claim(counter: &AtomicUsize, amount: usize, limit: usize) -> bool {
    let mut old = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = old.checked_add(amount).filter(|n| *n <= limit) else { return false };
        match counter.compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(current) => old = current,
        }
    }
}

pub(super) fn buffer_bytes(m: &Mesh) -> usize {
    m.positions.capacity() * size_of::<Vec3>()
        + m.normals.capacity() * size_of::<Vec3>()
        + m.triangles.capacity() * size_of::<[u32; 3]>()
        + m.tri_face.capacity() * size_of::<u32>()
        + m.edges.capacity() * size_of::<Vec<Vec3>>()
        + m.edges.iter().map(|e| e.capacity() * size_of::<Vec3>()).sum::<usize>()
        + m.seams.capacity()
        + m.smooth_edges.capacity()
        + m.sharp_edges.capacity()
        + m.edge_faces.capacity() * size_of::<Vec<u32>>()
        + m.edge_faces.iter().map(|e| e.capacity() * size_of::<u32>()).sum::<usize>()
}

fn rigid(m: &Mat) -> bool {
    if m.iter().flatten().any(|v| !v.is_finite()) || m.iter().zip([0., 0., 0., 1.]).any(|(c, w)| (c[3] - w).abs() > 1e-10) {
        return false;
    }
    let axes = [Vec3::new(m[0][0], m[0][1], m[0][2]), Vec3::new(m[1][0], m[1][1], m[1][2]), Vec3::new(m[2][0], m[2][1], m[2][2])];
    let [x, y, z] = axes;
    (x.len2() - 1.).abs() < 1e-9
        && (y.len2() - 1.).abs() < 1e-9
        && (z.len2() - 1.).abs() < 1e-9
        && x.dot(y).abs() < 1e-9
        && x.dot(z).abs() < 1e-9
        && y.dot(z).abs() < 1e-9
        && (x.cross(y).dot(z) - 1.).abs() < 1e-9
}

/// The export meshes of the bodies `shown` keeps, at the chord `tolerance`, with what `ask` asks
/// for. Errors about the placements name the command `cmd`; a body with no exportable surface, the
/// memory budget and cancellation come back as the meshing stage's own errors.
pub(crate) fn prepare(s: &Session, cmd: &str, tolerance: f64, ask: Ask, shown: impl Fn(&ModelBody) -> bool) -> Result<Scene> {
    let world = s.world_state();
    let state = s.model.state();
    let mut definitions = BTreeMap::new();
    for body in &state.bodies {
        *definitions.entry((body.component, body.definition_name(), body.feature)).or_insert(0usize) += 1;
    }
    let mut by_definition = BTreeMap::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut items = Vec::new();
    for (index, body) in world.bodies.iter().enumerate().filter(|(_, b)| shown(b)) {
        if items.len() >= MAX_BODIES {
            return Err(bad(cmd, "too many placed bodies to export"));
        }
        let key = (body.component, body.definition_name(), body.feature);
        let unambiguous = definitions.get(&key) == Some(&1);
        let reused = unambiguous
            .then(|| by_definition.get(&key).copied())
            .flatten()
            .and_then(|g: usize| groups.get(g).map(|group| (g, mat_mul(&body.source_transform, &group.inverse))))
            .filter(|(_, m)| rigid(m));
        let (group, transform) = if let Some(reused) = reused {
            reused
        } else {
            let g = groups.len();
            groups.push(Group { first: index, inverse: mat_inverse(&body.source_transform).unwrap_or(IDENTITY) });
            if unambiguous && mat_inverse(&body.source_transform).is_some() {
                by_definition.entry(key).or_insert(g);
            }
            (g, IDENTITY)
        };
        let keep = groups.get(group).is_some_and(|g| g.first == index);
        items.push(Item { body, group, keep, transform });
    }
    let progress = Stage::new("dxf_meshes", items.len());
    let bytes = AtomicUsize::new(0);
    let results = map_with_local_pools(&items, WORKERS, |item| {
        progress.check().map_err(|e| KernelError::Failed(e.into()))?;
        let mut mesh = item.body.body.reference_display_mesh_angular(tolerance, solidcraft_kernel::DISPLAY_ANGLE)?;
        if mesh.triangles.is_empty() {
            return Err(KernelError::Invalid(format!("{} has no exportable surface", item.body.name)));
        }
        let drawn = |i: usize| if ask.tangent_edges { mesh.edge_pickable(i) } else { mesh.shaded_edge_visible(i) };
        let visibility: Vec<bool> = (0..mesh.edges.len()).map(drawn).collect();
        let tangent =
            if ask.sources { (0..mesh.edges.len()).map(|i| mesh.edge_pickable(i) && !mesh.shaded_edge_visible(i)).collect() } else { Vec::new() };
        let joins = drawn_joins(&mesh, &visibility);
        let triangles = mesh.triangles.len();
        let mesh = if item.keep {
            // These are owned export buffers, not the document's cached mesh. Picking and
            // shading data are unused by vector occlusion; edge choices were captured above.
            // The faces stay (4 bytes a triangle, counted below): they name a silhouette's face,
            // and tell a silhouette along a drawn edge from one across a face.
            mesh.normals = Vec::new();
            mesh.seams = Vec::new();
            mesh.smooth_edges = Vec::new();
            mesh.sharp_edges = Vec::new();
            mesh.edge_faces = Vec::new();
            if !claim(&bytes, buffer_bytes(&mesh), MESH_BYTES) {
                return Err(KernelError::Invalid("view exceeds export memory budget; hide bodies or increase tolerance".into()));
            }
            Some(Arc::new(mesh))
        } else {
            None
        };
        progress.advance(1).map_err(|e| KernelError::Failed(e.into()))?;
        Ok(Prepared { mesh, visibility, tangent, joins, triangles })
    });
    let mut prepared = results.into_iter().collect::<solidcraft_kernel::Result<Vec<_>>>()?;
    let mut meshes: Vec<Option<Arc<Mesh>>> = vec![None; groups.len()];
    for (item, p) in items.iter().zip(&mut prepared) {
        if let Some(mesh) = p.mesh.take()
            && let Some(slot) = meshes.get_mut(item.group)
        {
            *slot = Some(mesh);
        }
    }
    let meshes = meshes.into_iter().map(|m| m.ok_or_else(|| bad(cmd, "missing export geometry"))).collect::<Result<Vec<_>>>()?;
    let mut placements = Vec::with_capacity(items.len());
    let mut triangles = 0usize;
    for (item, p) in items.iter().zip(prepared) {
        if meshes.get(item.group).is_none_or(|m| m.edges.len() != p.visibility.len()) {
            return Err(bad(cmd, "placed edge identity changed"));
        }
        triangles = triangles.saturating_add(p.triangles);
        placements.push(Placement { mesh: item.group, transform: item.transform, visibility: p.visibility, tangent: p.tangent, joins: p.joins });
    }
    Ok(Scene { meshes, placements, triangles, bytes: bytes.load(Ordering::Relaxed), sources: ask.sources })
}

/// The pairs of faces (smaller first, sorted) every edge between which is drawn: a mesh side
/// between two of them lies on one of those edges.
fn drawn_joins(mesh: &Mesh, visibility: &[bool]) -> Vec<[u32; 2]> {
    let mut all_drawn: BTreeMap<[u32; 2], bool> = BTreeMap::new();
    for (faces, drawn) in mesh.edge_faces.iter().zip(visibility) {
        if let [a, b] = faces.as_slice()
            && a != b
        {
            *all_drawn.entry([*a.min(b), *a.max(b)]).or_insert(true) &= *drawn;
        }
    }
    all_drawn.into_iter().filter_map(|(pair, all)| all.then_some(pair)).collect()
}

pub(super) fn transformed_bounds(mesh: &Mesh, m: &Mat) -> Aabb3 {
    let b = mesh.bounds();
    let mut out = Aabb3::EMPTY;
    for x in [b.min.x, b.max.x] {
        for y in [b.min.y, b.max.y] {
            for z in [b.min.z, b.max.z] {
                out.add(solidcraft_doc::apply_point(m, Vec3::new(x, y, z)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_proper_rigid_placements_share_geometry() {
        assert!(rigid(&solidcraft_doc::rigid(Vec3::new(1., 2., 3.), Vec3::ZERO, Vec3::Z, 0.7)));
        let mut m = IDENTITY;
        m[0][0] = -1.;
        assert!(!rigid(&m));
        m[0][0] = 2.;
        assert!(!rigid(&m));
        m[0][0] = f64::NAN;
        assert!(!rigid(&m));
    }
}
