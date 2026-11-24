//! Export-only mesh instances. Keep one geometry buffer per unambiguous definition, while
//! retaining the current placed body's edge choices (tangent classification can vary after
//! placement). Never modify the document, fuse parts, or share across separate exports.
use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use solidcore_record::Vec3;
use solidcraft_doc::{IDENTITY, Mat, ModelBody, ModelState, apply_point, mat_inverse, mat_mul};
use solidcraft_geom::{Aabb3, Mesh};
use solidcraft_kernel::{KernelError, map_with_local_pools};

use super::Refused;
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
    /// A reference part: its lines merge apart from the model's, as both are drawn.
    pub reference: bool,
}
pub(crate) struct Scene {
    pub meshes: Vec<Arc<Mesh>>,
    pub placements: Vec<Placement>,
    pub triangles: usize,
    pub bytes: usize,
    /// Whether each line keeps its source ([`Ask::sources`]).
    pub sources: bool,
    /// Whether reference parts cover the model's lines, as every part does unless told otherwise.
    /// Their own lines are always tested against everything.
    pub reference_hides_model: bool,
}

/// What a caller asks of the meshes besides the chord. The view DXF asks for neither.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ask {
    /// Draw the edges where faces meet smoothly too.
    pub tangent_edges: bool,
    /// Each line keeps its placement and its edge or silhouette face.
    pub sources: bool,
}

impl Scene {
    /// How many triangles the drawing's meshes hold, for the export's reply.
    pub(crate) fn triangle_count(&self) -> usize {
        self.triangles
    }
}

struct Group {
    first: usize,
    inverse: Mat,
}

/// A shown body as a view places it ([`placed`]).
pub(crate) struct Placed<'a> {
    pub body: &'a ModelBody,
    /// The placements that share one copy of the geometry: their group's first body's, at identity.
    pub group: usize,
    /// It is its group's first body, whose geometry the group shares.
    pub first: bool,
    /// Where it is from its group's first body: identity for the first, a rigid motion for the rest.
    pub transform: Mat,
}
struct Prepared {
    visibility: Vec<bool>,
    tangent: Vec<bool>,
    joins: Vec<[u32; 2]>,
    triangle_count: usize,
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

/// The bytes a mesh's buffers hold, for budgets that copy or keep meshes.
pub(crate) fn buffer_bytes(m: &Mesh) -> usize {
    m.positions.capacity() * size_of::<Vec3>()
        + m.normals.capacity() * size_of::<Vec3>()
        + m.triangles.capacity() * size_of::<[u32; 3]>()
        + m.tri_face.capacity() * size_of::<u32>()
        + polyline_bytes(&m.edges)
        + m.seams.capacity()
        + m.smooth_edges.capacity()
        + m.sharp_edges.capacity()
        + adjacency_bytes(&m.edge_faces)
}

fn polyline_bytes(edges: &Vec<Vec<Vec3>>) -> usize {
    edges.capacity() * size_of::<Vec<Vec3>>() + edges.iter().map(|line| line.capacity() * size_of::<Vec3>()).sum::<usize>()
}

fn adjacency_bytes(adjacency: &Vec<Vec<u32>>) -> usize {
    adjacency.capacity() * size_of::<Vec<u32>>() + adjacency.iter().map(|faces| faces.capacity() * size_of::<u32>()).sum::<usize>()
}

struct SharedEdges {
    adjacency: Vec<Vec<u32>>,
    workspace: usize,
}

// The mesh buffers required by projection take precedence over optional edge reuse.
struct MeshBudget {
    limit: usize,
    geometry: usize,
    adjacency: usize,
    shared: BTreeMap<usize, SharedEdges>,
}

impl MeshBudget {
    fn new(limit: usize) -> Self {
        Self { limit, geometry: 0, adjacency: 0, shared: BTreeMap::new() }
    }

    fn retain(&mut self, bytes: usize, group: usize, shared: Option<(usize, SharedEdges)>) -> bool {
        let Some(geometry) = self.geometry.checked_add(bytes).filter(|&n| n <= self.limit) else { return false };
        self.geometry = geometry;
        if self.adjacency > self.limit - geometry {
            self.shared.clear();
            self.adjacency = 0;
        }
        if let Some((bytes, edges)) = shared
            && bytes <= self.remaining()
        {
            self.shared.insert(group, edges);
            self.adjacency += bytes;
        }
        true
    }

    fn remaining(&self) -> usize {
        self.limit - self.geometry - self.adjacency
    }
}

fn trim_mesh(mesh: &mut Mesh, share: bool) -> Option<(usize, SharedEdges)> {
    mesh.seams = Vec::new();
    mesh.smooth_edges = Vec::new();
    mesh.sharp_edges = Vec::new();
    let adjacency = std::mem::take(&mut mesh.edge_faces);
    (share && adjacency.len() == mesh.edges.len()).then(|| {
        let bytes = adjacency_bytes(&adjacency);
        // A worker moves the edge polylines, copies their adjacency and flags each edge.
        let workspace = polyline_bytes(&mesh.edges) + bytes + 3 * mesh.edges.len();
        (bytes, SharedEdges { adjacency, workspace })
    })
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

/// The export meshes of the bodies of `world` that `shown` keeps, at the chord `tolerance`, with
/// what `ask` asks for. Errors name the command `cmd`; a refusal says what `says` makes of it, and
/// a body the kernel can't mesh comes back as the kernel's error.
pub(crate) fn prepare(
    s: &Session,
    world: &ModelState,
    cmd: &str,
    tolerance: f64,
    ask: Ask,
    shown: impl Fn(&ModelBody) -> bool,
    says: fn(Refused) -> String,
) -> Result<Scene> {
    let items = placed(s, world, shown, cmd)?;
    prepare_placed(&items, cmd, tolerance, ask, says, MESH_BYTES, tessellate)
}

fn prepare_placed(
    items: &[Placed<'_>],
    cmd: &str,
    tolerance: f64,
    ask: Ask,
    says: fn(Refused) -> String,
    limit: usize,
    mesh_body: impl Fn(&Placed<'_>, &solidcraft_kernel::MeshTolerance) -> solidcraft_kernel::Result<Mesh> + Sync,
) -> Result<Scene> {
    let progress = Stage::new("dxf_meshes", items.len());
    let mut firsts = Vec::new();
    let mut followers = Vec::new();
    let mut mesh_jobs = Vec::new();
    let mut keeps_adjacency = vec![false; items.len()];
    for (index, item) in items.iter().enumerate() {
        if item.first {
            firsts.push(index);
        } else if let Some(&first) = firsts.get(item.group)
            && let Some(source) = items.get(first)
            && shares_edge_order(source.body, item.body)
        {
            keeps_adjacency[first] = true;
            followers.push(index);
            continue;
        }
        mesh_jobs.push(index);
    }

    let bytes = AtomicUsize::new(0);
    // Scenes with no eligible copies only need the atomic geometry reservation.
    let budget = (!followers.is_empty()).then(|| Mutex::new(MeshBudget::new(limit)));
    let tolerance = solidcraft_kernel::MeshTolerance { chord: tolerance, angle: Some(solidcraft_kernel::DISPLAY_ANGLE) };
    let mut prepared: Vec<Option<Prepared>> = (0..items.len()).map(|_| None).collect();
    let mut meshes: Vec<Option<Mesh>> = (0..firsts.len()).map(|_| None).collect();
    for (index, result) in mesh_jobs.iter().copied().zip(map_with_local_pools(&mesh_jobs, WORKERS, |&index| {
        if let Err(refused) = progress.check() {
            return Ok(Err(refused));
        }
        let Some(item) = items.get(index) else { return Ok(Err(Refused::Invalid("missing export body"))) };
        let mut mesh = mesh_body(item, &tolerance)?;
        let edges = prepared_edges(&mesh, mesh.triangle_count(), ask);
        let geometry = if item.first {
            let shared = trim_mesh(&mut mesh, keeps_adjacency.get(index) == Some(&true));
            let geometry = buffer_bytes(&mesh);
            let retained = if let Some(budget) = &budget {
                let Ok(mut budget) = budget.lock() else { return Ok(Err(Refused::Invalid("mesh budget unavailable"))) };
                budget.retain(geometry, item.group, shared)
            } else {
                claim(&bytes, geometry, limit)
            };
            if !retained {
                return Ok(Err(Refused::Memory));
            }
            Some(mesh)
        } else {
            None
        };
        if let Err(refused) = progress.advance(1) {
            return Ok(Err(refused));
        }
        Ok(Ok((edges, geometry)))
    })) {
        let (edges, geometry) = result?.map_err(|e| bad(cmd, says(e)))?;
        if let Some(mesh) = geometry
            && let Some(item) = items.get(index)
            && let Some(slot) = meshes.get_mut(item.group)
        {
            *slot = Some(mesh);
        }
        if let Some(slot) = prepared.get_mut(index) {
            *slot = Some(edges);
        }
    }
    let budget = budget.map(Mutex::into_inner).transpose().map_err(|_| bad(cmd, "mesh budget unavailable"))?;
    let workers = WORKERS.min(followers.len()).max(1);
    let workspace_per_worker = budget.as_ref().map_or(0, |b| b.remaining() / workers);
    for (index, result) in followers.iter().copied().zip(map_with_local_pools(&followers, workers, |&index| {
        if let Err(refused) = progress.check() {
            return Ok(Err(refused));
        }
        let Some(item) = items.get(index) else { return Ok(Err(Refused::Invalid("missing export body"))) };
        let Some(source) = meshes.get(item.group).and_then(Option::as_ref) else {
            return Ok(Err(Refused::Invalid("missing export geometry")));
        };
        let reuse = budget.as_ref().and_then(|b| b.shared.get(&item.group)).filter(|edges| edges.workspace <= workspace_per_worker);
        let edges = if let Some(reuse) = reuse {
            // The group's triangles already represent this placement in the projection.
            // Reclassify only its edges: seams and authored splits are evaluated in place.
            let mut mesh = Mesh {
                edges: source.edges.iter().map(|line| line.iter().map(|point| apply_point(&item.transform, *point)).collect()).collect(),
                edge_faces: reuse.adjacency.clone(),
                ..Mesh::default()
            };
            item.body.body.flag_mesh_edges(&mut mesh);
            prepared_edges(&mesh, source.triangle_count(), ask)
        } else {
            let mesh = mesh_body(item, &tolerance)?;
            prepared_edges(&mesh, mesh.triangle_count(), ask)
        };
        if let Err(refused) = progress.advance(1) {
            return Ok(Err(refused));
        }
        Ok(Ok(edges))
    })) {
        let edges = result?.map_err(|e| bad(cmd, says(e)))?;
        if let Some(slot) = prepared.get_mut(index) {
            *slot = Some(edges);
        }
    }
    let meshes = meshes.into_iter().map(|m| m.map(Arc::new).ok_or_else(|| bad(cmd, "missing export geometry"))).collect::<Result<Vec<_>>>()?;
    let mut placements = Vec::with_capacity(items.len());
    let mut triangles = 0usize;
    for (item, p) in items.iter().zip(prepared) {
        let p = p.ok_or_else(|| bad(cmd, "missing export edge flags"))?;
        if meshes.get(item.group).is_none_or(|m| m.edges.len() != p.visibility.len()) {
            return Err(bad(cmd, "placed edge identity changed"));
        }
        triangles = triangles.saturating_add(p.triangle_count);
        placements.push(Placement {
            mesh: item.group,
            transform: item.transform,
            visibility: p.visibility,
            tangent: p.tangent,
            joins: p.joins,
            reference: false,
        });
    }
    let bytes = budget.map_or_else(|| bytes.load(Ordering::Relaxed), |b| b.geometry);
    Ok(Scene { meshes, placements, triangles, bytes, sources: ask.sources, reference_hides_model: true })
}

fn tessellate(item: &Placed<'_>, tolerance: &solidcraft_kernel::MeshTolerance) -> solidcraft_kernel::Result<Mesh> {
    let mesh = item.body.body.mesh(tolerance)?;
    if mesh.triangle_count() == 0 {
        return Err(KernelError::Invalid(format!("{} has no surface to mesh", item.body.name)));
    }
    Ok(mesh)
}

/// An occurrence's display cells are its definition's. Do not infer edge order from
/// matching names or equal-looking solids; the two bodies must share that definition.
fn shares_edge_order(source: &ModelBody, follower: &ModelBody) -> bool {
    let definition = |body: &ModelBody| body.placed_definition().unwrap_or_else(|| Arc::as_ptr(&body.display_identity()) as *const () as usize);
    source.source_key() != 0
        && source.source_key() == follower.source_key()
        && definition(source) == definition(follower)
        && !source.body.is_mesh()
        && !follower.body.is_mesh()
        && source.body.display_share().is_none()
        && follower.body.display_share().is_none()
}

fn prepared_edges(mesh: &Mesh, triangle_count: usize, ask: Ask) -> Prepared {
    let (visibility, tangent) = drawn_edges(mesh, ask);
    let joins = drawn_joins(mesh, &visibility);
    Prepared { visibility, tangent, joins, triangle_count }
}

/// The bodies of `world` that `shown` keeps, in its order, each placed in a group that shares one
/// copy of its geometry. A definition the model state holds once shares its first placement's
/// geometry with its later ones whose motion from it is rigid; any other placement (a mirror, a
/// scale, a definition held twice, a body with no geometry key such as a section view's cut) is a
/// group of its own, at identity. Errors name `cmd`.
pub(crate) fn placed<'a>(s: &Session, world: &'a ModelState, shown: impl Fn(&ModelBody) -> bool, cmd: &str) -> Result<Vec<Placed<'a>>> {
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
            return Err(bad(cmd, "too many placed bodies"));
        }
        let key = (body.component, body.definition_name(), body.feature);
        // A body with no geometry key may not be its definition's geometry: a section's cut isn't.
        let unambiguous = definitions.get(&key) == Some(&1) && body.geometry_key().is_some();
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
        let first = groups.get(group).is_some_and(|g| g.first == index);
        items.push(Placed { body, group, first, transform });
    }
    Ok(items)
}

/// Which of `mesh`'s edges a view draws, by index, and with `ask.sources` which of them are tangent
/// edges (else none). Every edge a pick can name is drawn when `ask.tangent_edges` asks for them;
/// otherwise only those the shaded view draws, so a smooth join is a tangent edge, and a fillet's
/// boundary, tangent but a crease in curvature, is drawn as any other edge.
pub(crate) fn drawn_edges(mesh: &Mesh, ask: Ask) -> (Vec<bool>, Vec<bool>) {
    let drawn = |i: usize| if ask.tangent_edges { mesh.edge_pickable(i) } else { mesh.shaded_edge_visible(i) };
    let visibility = (0..mesh.edges.len()).map(drawn).collect();
    let tangent =
        if ask.sources { (0..mesh.edges.len()).map(|i| mesh.edge_pickable(i) && !mesh.shaded_edge_visible(i)).collect() } else { Vec::new() };
    (visibility, tangent)
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

    #[test]
    fn optional_adjacency_yields_to_required_mesh_buffers() {
        let mut first = Mesh { edges: vec![vec![Vec3::ZERO, Vec3::X]], edge_faces: vec![vec![0, 1]], ..Mesh::default() };
        let extra = Mesh { positions: vec![Vec3::ZERO; 4], ..Mesh::default() };
        let shared = trim_mesh(&mut first, true);
        let geometry = buffer_bytes(&first) + buffer_bytes(&extra);
        let mut budget = MeshBudget::new(geometry);
        assert!(budget.retain(buffer_bytes(&first), 0, shared));
        assert!(!budget.shared.is_empty());
        assert!(budget.retain(buffer_bytes(&extra), 1, None));
        assert!(budget.shared.is_empty());
        assert_eq!(budget.geometry, geometry);
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn sharing_and_budget_fallback_produce_the_same_placed_edges() {
        let mut s = Session::default();
        let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap();
        let first = run(&mut s, "component.create", serde_json::json!({"name": "Part"}))["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.box", serde_json::json!({"length": 10, "width": 8, "height": 5, "operation": "new"}));
        run(&mut s, "occurrence.copy", serde_json::json!({"occurrence": first, "translate": [30, 0, 0]}));
        run(&mut s, "component.activate", serde_json::json!({"component": 0}));
        let world = s.world_state().unwrap();
        let items = placed(&s, &world, |_| true, "test").unwrap();
        let ask = Ask { tangent_edges: true, sources: true };
        let calls = AtomicUsize::new(0);
        let counted = |item: &Placed<'_>, tolerance: &solidcraft_kernel::MeshTolerance| {
            calls.fetch_add(1, Ordering::Relaxed);
            tessellate(item, tolerance)
        };
        let run_at_limit = |limit| prepare_placed(&items, "test", 0.02, ask, |why| format!("{why:?}"), limit, counted);
        let reuse = run_at_limit(MESH_BYTES).unwrap();
        assert_eq!(calls.swap(0, Ordering::Relaxed), 1);
        assert_eq!(reuse.placements.len(), 2);
        assert_eq!(reuse.meshes.len(), 1);

        let first = tessellate(&items[0], &solidcraft_kernel::MeshTolerance { chord: 0.02, angle: Some(solidcraft_kernel::DISPLAY_ANGLE) }).unwrap();
        let with_cache = reuse.bytes + adjacency_bytes(&first.edge_faces);
        for limit in [reuse.bytes, with_cache] {
            // The first limit excludes even the adjacency. The second leaves zero worker space.
            let fallback = run_at_limit(limit).unwrap();
            assert_eq!(calls.swap(0, Ordering::Relaxed), 2);
            assert_eq!(fallback.bytes, reuse.bytes);
            assert_eq!(fallback.triangles, reuse.triangles);
            assert_eq!(fallback.meshes.len(), 1);
            assert_eq!(fallback.placements.len(), 2);
            for (a, b) in fallback.placements.iter().zip(&reuse.placements) {
                assert_eq!(a.mesh, b.mesh);
                assert_eq!(a.transform, b.transform);
                assert_eq!(a.visibility, b.visibility);
                assert_eq!(a.tangent, b.tangent);
                assert_eq!(a.joins, b.joins);
            }
        }
        assert!(run_at_limit(reuse.bytes - 1).is_err());
    }

    /// Two placements of one part share its first placement's geometry, the second at the move
    /// from it; a body and its mirror image are two groups, each its own first, at identity; a
    /// body left out isn't placed.
    #[test]
    fn placements_share_geometry_and_a_mirror_image_has_its_own() {
        let mut s = Session::default();
        let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap();
        let first = run(&mut s, "component.create", serde_json::json!({"name": "Part"}))["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.box", serde_json::json!({"length": 10, "width": 10, "height": 5, "body_name": "Plate", "operation": "new"}));
        run(&mut s, "occurrence.copy", serde_json::json!({"occurrence": first, "translate": [30, 0, 0]}));
        run(&mut s, "component.activate", serde_json::json!({"component": 0}));
        run(
            &mut s,
            "solid.box",
            serde_json::json!({"length": 4, "width": 4, "height": 4, "corner": [0, 30, 0], "body_name": "Block", "operation": "new"}),
        );
        run(&mut s, "solid.mirror", serde_json::json!({"bodies": ["Block"], "plane": "XZ"}));
        let world = s.world_state().unwrap();
        let all = placed(&s, &world, |_| true, "test").unwrap();
        let by_group =
            |name: &str| all.iter().filter(|p| p.body.definition_name() == name).map(|p| (p.group, p.first, p.transform)).collect::<Vec<_>>();
        let plates = by_group("Plate");
        let [(g0, true, t0), (g1, false, t1)] = plates.as_slice() else { panic!("{plates:?}") };
        assert_eq!((g0, *t0), (g1, IDENTITY));
        assert_eq!([t1[3][0], t1[3][1], t1[3][2]], [30., 0., 0.]);
        assert!(rigid(t1));
        let others: Vec<_> = all.iter().filter(|p| p.body.definition_name() != "Plate").map(|p| (p.group, p.first, p.transform)).collect();
        let [(a, true, ta), (b, true, tb)] = others.as_slice() else { panic!("{others:?}") };
        assert!(a != b && a != g0 && b != g0 && *ta == IDENTITY && *tb == IDENTITY, "{others:?}");
        assert_eq!(placed(&s, &world, |b| b.definition_name() != "Plate", "test").unwrap().len(), 2);
    }
}
