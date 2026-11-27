//! Clearance (`inspect.clearance`): in one call, which bodies overlap and by how much, which
//! come closer than a clearance and where, and each body's nearest neighbours. Pairs are pruned
//! by bounding boxes grown by the clearance, then by the bodies' meshes (which lie within their
//! chord tolerance of the faces); distances are measured on the exact faces and edges
//! ([`solidcraft_kernel::body_distance`]) and overlaps by intersecting the bodies. The pairs
//! that are left run in parallel. A pair whose bodies have not changed or moved since a call
//! with the same settings measured it is not measured again ([`PairCache`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde_json::Value;
use solidcraft_geom::mesh_distance::{TriangleSet, box_gap};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Aabb3, Mesh};
use solidcraft_kernel::{BodyDistance, DistanceShape, KernelFormMask};

use super::CommandSpec;
use crate::params::{bad, num, str_, string_list};
use crate::schema::clearance::{
    Clearance as ClearanceReply, ClearanceParams, MAX_CLEARANCE, MAX_NEAREST, NearPair, Neighbour, Overlap, OverlapBounds, Unchecked,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<ClearanceParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.clearance", "Clearance", crate::checked!("inspect.clearance", clearance))
    .icon("interference")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<ClearanceReply>)
    .params(
    "bodies?: [names] (default: every solid body, in world placement, but those in `against`; mesh bodies are not measured); against?: [names] (only pairs with one body in `bodies` and the other here); \
     min_volume?: mm³ (default 0.01: smaller overlaps count as contact); clearance?: mm (default 0: report pairs closer than this); \
     nearest?: n (each body of `bodies` gets its n nearest others); skip_within?: \"occurrence\" (pairs of bodies of one occurrence) | \
     \"component\" (pairs of bodies inside one placed component, its subcomponents included) | null; envelopes?: bool (default false: component envelopes, solid.bounding_solid with `component`, are left out unless named; true: each is checked instead of the bodies inside it, and a body named inside one means it; an envelope is never paired with a body inside it); tolerance?: mm (default 1e-6: a gap \
     below it is contact, 0) → overlaps: [{a, b, volume_mm3, area_mm2, bbox, center}]; near: [{a, b, distance_mm, from, to, overlap_mm3?: \
     a contact's shared volume below min_volume, exact?: false when measured on the meshes}], nearest by body (the same for pairs that overlap: 0); \
     nearest: {body: [{body, distance_mm}]} (omits pairs in unchecked); unchecked: [{a, b, reason, distance_mm?, exact?}] (pairs whose overlap could not be worked out, with non-boolean minimum distance when computable and exact: false when measured on the meshes; each mesh body, STL/3MF/OBJ import, is not measured and is listed as {a: name, b: null, reason: \"mesh body: not checked\"}, also when named in `bodies` or `against`; the result's `bodies` counts the solid bodies only); pairs_checked; \
     pairs_reused (of those, measured by an earlier call: neither body changed or moved since); pairs_pruned (ruled out by their boxes); ms. Distances are measured on the exact faces and edges, the meshes only rule pairs out",
)];

/// Most bodies one call takes.
const MAX_BODIES: usize = 20_000;
/// Most pairs whose grown boxes meet that one call measures.
const MAX_PAIRS: usize = 200_000;
/// Maximum concurrent workers during `inspect.clearance` mesh preparation and pair checks.
/// Caps simultaneous kernel boolean workspaces so high-core machines do not spike commit memory.
const MAX_CLEARANCE_THREADS: usize = 8;
/// Default number of pairs measured per chunk before evicting finished bodies' distance shapes
/// and transient kernel forms.
const DEFAULT_CHUNK_PAIRS: usize = 64;
/// Upper target for live `DistanceShape` triangle hierarchies kept across chunks (256 MiB).
const MAX_SHAPE_CACHE_BYTES: usize = 256 << 20;
/// Lower bound when scaling the live shape working-set budget under tight memory (16 MiB).
const MIN_SHAPE_CACHE_BYTES: usize = 16 << 20;
/// Minimum host memory / commit headroom required to start or continue a clearance batch
/// (256 MiB). Below this floor, `inspect.clearance` returns a structured error rather than risking
/// an uncatchable allocation abort that terminates a headless session.
const MIN_HEADROOM_BYTES: u64 = 256 << 20;

/// Shared definition display mesh and optional rigid world placement (`None`: identity), so
/// placed occurrences never deep-clone transformed meshes into `ModelBody` caches.
type PlacedSource = (Arc<Mesh>, Option<solidcraft_doc::Mat>);

/// Borrowed view of a body's shared mesh, rigid world placement, and uninflated mesh bounds for
/// containment checks without allocating a moved mesh.
struct PlacedMeshRef<'a> {
    mesh: &'a Mesh,
    transform: Option<solidcraft_doc::Mat>,
    mesh_bounds: Aabb3,
}

/// Releases transient SMLib / solidcore-ops kernel forms created during `inspect.clearance` when
/// the command finishes or exits early, preserving any forms that were already cached beforehand.
struct KernelFormGuard<'a> {
    bodies: &'a [&'a solidcraft_doc::ModelBody],
    initial: &'a [KernelFormMask],
}

impl Drop for KernelFormGuard<'_> {
    fn drop(&mut self) {
        for (body, &mask) in self.bodies.iter().zip(self.initial) {
            body.body.release_kernel_forms_since(mask);
        }
    }
}

/// Worker thread cap for `inspect.clearance`, scaled down when host memory headroom is tight.
fn clearance_worker_limit(items: usize, avail_bytes: Option<u64>) -> usize {
    let cap = match avail_bytes {
        Some(avail) if avail < (2u64 << 30) => 1,
        Some(avail) if avail < (4u64 << 30) => 2,
        Some(avail) if avail < (8u64 << 30) => 4,
        _ => MAX_CLEARANCE_THREADS,
    };
    items.clamp(1, cap)
}

/// Pair chunk size for `inspect.clearance`, shrinking under tight memory so finished bodies'
/// distance hierarchies and transient kernel forms are freed sooner.
fn chunk_size_for_memory(avail_bytes: Option<u64>) -> usize {
    match avail_bytes {
        Some(avail) if avail < (2u64 << 30) => 16,
        Some(avail) if avail < (4u64 << 30) => 32,
        _ => DEFAULT_CHUNK_PAIRS,
    }
}

/// Live `DistanceShape` byte budget, scaled down to at most one eighth of available headroom.
fn shape_cache_budget(avail_bytes: Option<u64>) -> usize {
    match avail_bytes {
        Some(avail) => usize::try_from(avail / 8).unwrap_or(MAX_SHAPE_CACHE_BYTES).clamp(MIN_SHAPE_CACHE_BYTES, MAX_SHAPE_CACHE_BYTES),
        None => MAX_SHAPE_CACHE_BYTES,
    }
}

/// Refuses to start or continue a clearance batch when host memory or commit headroom has fallen
/// below [`MIN_HEADROOM_BYTES`], returning an actionable error instead of letting a subsequent
/// allocation abort the server process.
fn check_memory_headroom(cmd: &str, avail_bytes: Option<u64>, remaining_pairs: usize, total_pairs: usize) -> Result<()> {
    if let Some(avail) = avail_bytes
        && avail < MIN_HEADROOM_BYTES
    {
        let avail_mib = avail / (1024 * 1024);
        let need_mib = MIN_HEADROOM_BYTES / (1024 * 1024);
        let detail = if total_pairs > 0 { format!(" ({remaining_pairs} of {total_pairs} pairs remaining)") } else { String::new() };
        return Err(bad(
            cmd,
            format!(
                "not enough available memory ({avail_mib} MiB available, need at least {need_mib} MiB) to continue inspect.clearance{detail}; \
                 reduce the number of bodies/pairs or close other sessions"
            ),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SkipWithin {
    Nothing,
    Occurrence,
    Component,
}

/// Most pairs [`PairCache`] keeps; it starts over when full.
const MAX_CACHED: usize = 2 * MAX_PAIRS;

/// Pairs measured before, by the two bodies' geometry where they stood
/// ([`solidcraft_doc::ModelBody::geometry_key`]) and the settings that shape the result
/// (clearance, smallest overlap, tolerance).
#[derive(Default)]
pub(crate) struct PairCache(HashMap<(u64, u64, [u64; 3]), PairResult>);

impl PairCache {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// What one pair came to.
#[derive(Clone, Default)]
struct PairResult {
    /// Shared volume, area, box and centre.
    overlap: Option<(f64, f64, Aabb3, solidcore_record::Vec3)>,
    distance: Option<BodyDistance>,
    unchecked: Option<String>,
}

fn clearance(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.clearance";
    let started = web_time::Instant::now();
    let param = |k: &str, default: f64, max: f64| -> Result<f64> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(default),
            Some(_) => match num(p, k) {
                Some(v) if (0.0..=max).contains(&v) => Ok(v),
                _ => Err(bad(cmd, format!("`{k}` must be a number from 0 to {max}"))),
            },
        }
    };
    let min_volume = param("min_volume", 0.01, f64::MAX)?;
    let gap = param("clearance", 0.0, MAX_CLEARANCE)?;
    let tolerance = param("tolerance", 1e-6, 1.0)?;
    let nearest = param("nearest", 0.0, MAX_NEAREST as f64)?.floor() as usize;
    let skip = match p.get("skip_within") {
        None | Some(Value::Null) => SkipWithin::Nothing,
        Some(_) => match str_(p, "skip_within") {
            Some("occurrence") => SkipWithin::Occurrence,
            Some("component") => SkipWithin::Component,
            _ => return Err(bad(cmd, "`skip_within` must be \"occurrence\", \"component\" or null")),
        },
    };

    let use_envelopes = super::envelopes::use_envelopes(p, cmd)?;
    let st = s.world_state()?;
    let envelopes = super::envelopes::Envelopes::of(s, &st);
    let (mesh_bodies, solid): (Vec<&solidcraft_doc::ModelBody>, Vec<&solidcraft_doc::ModelBody>) = st.bodies.iter().partition(|b| b.body.is_mesh());
    // Mesh bodies are not measured. A mesh body named in `bodies` or `against` is left out of the
    // pairs and listed as unchecked, rather than refused as "no solid body". With `envelopes`, a
    // body inside a component envelope is checked as that envelope.
    let solid_by_name: HashMap<&str, usize> = solid.iter().enumerate().map(|(i, b)| (b.name.as_str(), i)).collect();
    let mesh_names: HashSet<&str> = mesh_bodies.iter().map(|b| b.name.as_str()).collect();
    let named = |key: &str| -> Result<Vec<usize>> {
        let names = string_list(cmd, p, key)?;
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for n in &names {
            let n = if use_envelopes { envelopes.standing_in(n) } else { n.as_str() };
            let Some(&i) = solid_by_name.get(n) else {
                if mesh_names.contains(n) {
                    continue;
                }
                return Err(bad(cmd, format!("no solid body `{n}`")));
            };
            if seen.insert(i) {
                out.push(i);
            }
        }
        Ok(out)
    };
    let against = if p.get("against").is_none_or(Value::is_null) { None } else { Some(named("against")?) };
    // Without `bodies`: every body (every body not in `against`, when it is given), the
    // envelopes left out, or with `envelopes` standing in for the bodies inside them.
    let subjects = if p.get("bodies").is_none_or(Value::is_null) {
        (0..solid.len())
            .filter(|i| against.as_ref().is_none_or(|a| !a.contains(i)))
            .filter(|&i| solid.get(i).is_some_and(|b| envelopes.by_default(&b.name, use_envelopes)))
            .collect()
    } else {
        named("bodies")?
    };
    if solid.len() > MAX_BODIES {
        return Err(bad(cmd, format!("at most {MAX_BODIES} bodies")));
    }
    let n = solid.len();
    let mut in_subjects = vec![false; n];
    for &i in &subjects {
        if let Some(x) = in_subjects.get_mut(i) {
            *x = true;
        }
    }
    let mut in_against = vec![false; n];
    for &i in against.iter().flatten() {
        if let Some(x) = in_against.get_mut(i) {
            *x = true;
        }
    }
    let used: Vec<bool> = (0..n).map(|i| in_subjects.get(i).copied().unwrap_or(false) || in_against.get(i).copied().unwrap_or(false)).collect();

    // The occurrence path of each body, for `skip_within`.
    let paths: HashMap<String, Vec<u64>> = s.doc.body_frames(&s.model.state()).into_iter().map(|(name, _, path, _)| (name, path)).collect();
    let path_by_body: Vec<&[u64]> = solid.iter().map(|b| paths.get(&b.name).map(Vec::as_slice).unwrap_or(&[])).collect();
    let path_of = |i: usize| path_by_body.get(i).copied().unwrap_or(&[]);
    let flag = |v: &[bool], i: usize| v.get(i).copied().unwrap_or(false);
    let allowed = |i: usize, j: usize| -> bool {
        if i == j {
            return false;
        }
        let paired = match &against {
            None => flag(&in_subjects, i) && flag(&in_subjects, j),
            Some(_) => (flag(&in_subjects, i) && flag(&in_against, j)) || (flag(&in_subjects, j) && flag(&in_against, i)),
        };
        if !paired {
            return false;
        }
        // An envelope around the other body is no finding.
        if let (Some(x), Some(y)) = (solid.get(i), solid.get(j))
            && envelopes.encloses(&x.name, &y.name)
        {
            return false;
        }
        let (a, b) = (path_of(i), path_of(j));
        match skip {
            SkipWithin::Nothing => true,
            SkipWithin::Occurrence => a.is_empty() || a != b,
            SkipWithin::Component => a.is_empty() || a.first() != b.first(),
        }
    };

    // Snapshot pre-existing kernel forms so any SMLib / solidcore-ops forms created during this
    // call are released both between chunks and on normal or error exit.
    let initial_forms: Vec<KernelFormMask> = solid.iter().map(|b| b.body.kernel_forms_snapshot()).collect();
    let _form_guard = KernelFormGuard { bodies: &solid, initial: &initial_forms };

    // Shared definition meshes + rigid placements (never deep-cloning moved meshes into
    // `ModelBody.mesh` caches), prepared with bounded concurrency.
    let idx: Vec<usize> = (0..n).filter(|&i| flag(&used, i)).collect();
    let avail_start = solidcraft_kernel::available_memory_bytes();
    if !idx.is_empty() {
        check_memory_headroom(cmd, avail_start, 0, 0)?;
    }
    let placed_meshes: Vec<Option<PlacedSource>> = {
        let threads = clearance_worker_limit(idx.len(), avail_start);
        let made = solidcraft_kernel::map_with_local_pools(&idx, threads, |&i| {
            Ok(solid.get(i).map(|b| {
                let (m, mat) = b.mesh_in_place();
                let transform = (!solidcraft_doc::is_identity(&mat)).then_some(mat);
                (m, transform)
            }))
        });
        let mut out = vec![None; n];
        for (&i, m) in idx.iter().zip(made) {
            if let (Some(slot), Ok(m)) = (out.get_mut(i), m) {
                *slot = m;
            }
        }
        out
    };
    let mut incomplete = vec![false; n];
    let boxes: Vec<Aabb3> = (0..n)
        .map(|i| match (solid.get(i), placed_meshes.get(i).and_then(Option::as_ref)) {
            (Some(b), Some((m, transform))) if flag(&used, i) => {
                // Over the triangles with finite corners, as a shape's set of them is. Also track
                // whether any face was left unmeshed so an incomplete or empty display mesh never
                // silently prunes a body from overlap/clearance checks.
                let mut bx = Aabb3::EMPTY;
                let mut seen_faces = vec![false; b.body.face_count()];
                for (k, t) in m.triangles.iter().enumerate() {
                    let Some(p) = m.tri(t) else { continue };
                    let p = match transform {
                        Some(mat) => p.map(|v| solidcraft_doc::apply_point(mat, v)),
                        None => p,
                    };
                    if !p.iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    p.into_iter().for_each(|v| bx.add(v));
                    if let Some(seen) = m.tri_face.get(k).and_then(|&fi| seen_faces.get_mut(fi as usize)) {
                        *seen = true;
                    }
                }
                let chord = b.display_tol();
                let g = solidcore_record::Vec3::new(1.0, 1.0, 1.0) * if chord.is_finite() && chord > 0.0 { chord } else { 0.0 };
                let mesh_box = if bx.is_empty() { bx } else { Aabb3 { min: bx.min - g, max: bx.max + g } };
                if bx.is_empty() || seen_faces.iter().any(|&seen| !seen) {
                    if let Some(slot) = incomplete.get_mut(i) {
                        *slot = true;
                    }
                    let exact = b.body.exact_bounds().ok().unwrap_or_else(|| b.outline_bounds()).inflated(2.0 * b.body.record().tolerance);
                    if let Some(&mask) = initial_forms.get(i) {
                        b.body.release_kernel_forms_since(mask);
                    }
                    mesh_box.union(&exact)
                } else {
                    mesh_box
                }
            }
            _ => Aabb3::EMPTY,
        })
        .collect();

    // Pairs whose boxes, grown by half the clearance each, meet.
    let half = solidcore_record::Vec3::new(1.0, 1.0, 1.0) * (gap / 2.0);
    let grown: Vec<Aabb3> = boxes.iter().map(|b| if b.is_empty() { *b } else { Aabb3 { min: b.min - half, max: b.max + half } }).collect();
    let (close, truncated) = super::joints::overlapping_pairs(&grown, MAX_PAIRS);
    let has_mesh = |i: usize| placed_meshes.get(i).is_some_and(Option::is_some);
    let close: Vec<(usize, usize)> = close.into_iter().filter(|&(i, j)| allowed(i, j) && has_mesh(i) && has_mesh(j)).collect();
    let mut total = 0usize;
    for i in 0..n {
        for j in i + 1..n {
            if allowed(i, j) {
                total += 1;
            }
        }
    }

    // Pairs measured before come from the cache; the rest are measured in memory-bounded chunks.
    let settings = [gap.to_bits(), min_volume.to_bits(), tolerance.to_bits()];
    let cache_key = |i: usize, j: usize| Some((solid.get(i)?.geometry_key()?, solid.get(j)?.geometry_key()?, settings));
    let mut results: Vec<Option<solidcraft_kernel::Result<PairResult>>> = Vec::new();
    results
        .try_reserve_exact(close.len())
        .map_err(|_| bad(cmd, "not enough memory to allocate pair results for inspect.clearance; reduce the number of bodies/pairs"))?;
    {
        let cache = s.clearance_cache.lock().ok();
        for &(i, j) in &close {
            let hit = cache.as_ref().and_then(|c| c.0.get(&cache_key(i, j)?)).cloned().map(Ok);
            results.push(hit);
        }
    }
    let todo: Vec<usize> = (0..close.len()).filter(|&k| results.get(k).is_some_and(Option::is_none)).collect();
    let reused = close.len() - todo.len();

    // Distance shapes are built fallibly on demand per chunk, and evicted together with transient
    // kernel forms once a body's remaining pairs finish or the live working-set budget is reached.
    let mut shapes: Vec<Option<DistanceShape<'_>>> = (0..n).map(|_| None).collect();
    let mut active_shape_bytes: usize = 0;
    let evict_body = |i: usize, shapes: &mut [Option<DistanceShape<'_>>], active_bytes: &mut usize| {
        if let Some(slot) = shapes.get_mut(i)
            && let Some(old) = slot.take()
        {
            *active_bytes = active_bytes.saturating_sub(old.estimated_bytes());
        }
        if let (Some(b), Some(&mask)) = (solid.get(i), initial_forms.get(i)) {
            b.body.release_kernel_forms_since(mask);
        }
    };
    let trim_shapes = |keep: &HashSet<usize>, budget: usize, shapes: &mut [Option<DistanceShape<'_>>], active_bytes: &mut usize| {
        if *active_bytes <= budget {
            return;
        }
        let mut victims: Vec<(bool, usize, usize)> =
            shapes.iter().enumerate().filter_map(|(i, slot)| slot.as_ref().map(|sh| (keep.contains(&i), sh.estimated_bytes(), i))).collect();
        // Evict bodies not needed by the next chunk first, largest hierarchies first.
        victims.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        for (_, _, i) in victims {
            if *active_bytes <= budget {
                break;
            }
            evict_body(i, shapes, active_bytes);
        }
    };

    let mut remaining_uses = vec![0usize; n];
    for &k in &todo {
        if let Some(&(i, j)) = close.get(k) {
            if let Some(u) = remaining_uses.get_mut(i) {
                *u += 1;
            }
            if let Some(u) = remaining_uses.get_mut(j) {
                *u += 1;
            }
        }
    }

    let mut offset = 0usize;
    while offset < todo.len() {
        let avail = solidcraft_kernel::available_memory_bytes();
        check_memory_headroom(cmd, avail, todo.len() - offset, todo.len())?;
        let chunk_len = chunk_size_for_memory(avail);
        let end = (offset + chunk_len).min(todo.len());
        let chunk = todo.get(offset..end).unwrap_or(&[]);
        let threads = clearance_worker_limit(chunk.len(), avail);

        // Build missing `DistanceShape`s for this chunk's bodies (fallibly, without touching B-rep
        // records or allocating moved display meshes).
        let mut chunk_bodies: Vec<usize> = Vec::new();
        let mut chunk_seen: HashSet<usize> = HashSet::new();
        for &k in chunk {
            if let Some(&(i, j)) = close.get(k) {
                if chunk_seen.insert(i) {
                    chunk_bodies.push(i);
                }
                if chunk_seen.insert(j) {
                    chunk_bodies.push(j);
                }
            }
        }
        let missing: Vec<usize> = chunk_bodies.iter().copied().filter(|&i| shapes.get(i).is_some_and(Option::is_none)).collect();
        if !missing.is_empty() {
            let build_threads = clearance_worker_limit(missing.len(), avail);
            let built = solidcraft_kernel::map_on_threads(&missing, build_threads, |&i| -> Result<Option<DistanceShape<'_>>> {
                let (Some(b), Some((m, mat))) = (solid.get(i), placed_meshes.get(i).and_then(Option::as_ref)) else {
                    return Ok(None);
                };
                let set = TriangleSet::try_from_indexed(&m.positions, &m.triangles, &m.tri_face, mat.as_ref()).map_err(|_| {
                    bad(cmd, format!("not enough memory to index body `{}` for inspect.clearance; reduce the number of bodies/pairs", b.name))
                })?;
                Ok(Some(DistanceShape::with_triangle_set(&b.body, m, *mat, set, b.display_tol())))
            });
            for (&i, item) in missing.iter().zip(built) {
                let shape = item.ok_or_else(|| bad(cmd, "worker failed while indexing body for inspect.clearance"))??;
                if let (Some(slot), Some(shape)) = (shapes.get_mut(i), shape) {
                    active_shape_bytes = active_shape_bytes.saturating_add(shape.estimated_bytes());
                    *slot = Some(shape);
                }
            }
        }

        let measured = solidcraft_kernel::map_with_local_pools(chunk, threads, |&k| {
            let Some(&(i, j)) = close.get(k) else { return Ok(PairResult::default()) };
            let (Some(a), Some(b)) = (shapes.get(i).and_then(Option::as_ref), shapes.get(j).and_then(Option::as_ref)) else {
                return Ok(PairResult::default());
            };
            let (Some(ba), Some(bb)) = (solid.get(i), solid.get(j)) else { return Ok(PairResult::default()) };
            let (Some((ma, ta)), Some((mb, tb))) = (placed_meshes.get(i).and_then(Option::as_ref), placed_meshes.get(j).and_then(Option::as_ref))
            else {
                return Ok(PairResult::default());
            };
            let pa = PlacedMeshRef { mesh: ma, transform: *ta, mesh_bounds: a.mesh_bounds() };
            let pb = PlacedMeshRef { mesh: mb, transform: *tb, mesh_bounds: b.mesh_bounds() };
            let incomplete_pair = flag(&incomplete, i) || flag(&incomplete, j);
            Ok(measure_pair((&ba.body, &pa), a, (&bb.body, &pb), b, gap, min_volume, tolerance, incomplete_pair))
        });
        if let Ok(mut cache) = s.clearance_cache.lock() {
            for (&k, r) in chunk.iter().zip(&measured) {
                // A pair that could not be worked out is tried again next time.
                let (Ok(r), Some(&(i, j))) = (r, close.get(k)) else { continue };
                if r.unchecked.is_some() {
                    continue;
                }
                let Some(key) = cache_key(i, j) else { continue };
                if cache.0.len() >= MAX_CACHED {
                    cache.0.clear();
                }
                cache.0.insert(key, r.clone());
            }
        }
        for (&k, r) in chunk.iter().zip(measured) {
            if let Some(slot) = results.get_mut(k) {
                *slot = Some(r);
            }
            if let Some(&(i, j)) = close.get(k) {
                if let Some(u) = remaining_uses.get_mut(i) {
                    *u = u.saturating_sub(1);
                }
                if let Some(u) = remaining_uses.get_mut(j) {
                    *u = u.saturating_sub(1);
                }
            }
        }

        // Release finished bodies immediately when `nearest == 0`, and trim live shapes to budget.
        if nearest == 0 {
            for &b in &chunk_bodies {
                if remaining_uses.get(b).copied().unwrap_or(0) == 0 {
                    evict_body(b, &mut shapes, &mut active_shape_bytes);
                }
            }
        }
        let budget = shape_cache_budget(avail);
        if active_shape_bytes > budget {
            let next_end = (end + chunk_len).min(todo.len());
            let mut keep_next: HashSet<usize> = HashSet::new();
            for &k in todo.get(end..next_end).unwrap_or(&[]) {
                if let Some(&(i, j)) = close.get(k) {
                    keep_next.insert(i);
                    keep_next.insert(j);
                }
            }
            trim_shapes(&keep_next, budget, &mut shapes, &mut active_shape_bytes);
        }
        offset = end;
    }

    if nearest == 0 {
        for i in 0..n {
            evict_body(i, &mut shapes, &mut active_shape_bytes);
        }
    }
    let results = results.into_iter().map(|r| r.unwrap_or_else(|| Ok(PairResult::default())));

    // The mesh bodies the call was about (every one without `bodies`, else those named in
    // `bodies` or `against`) come first, so "nothing found" is not read as "no overlap".
    let mut asked: Vec<String> = Vec::new();
    for k in ["bodies", "against"] {
        asked.extend(string_list(cmd, p, k)?);
    }
    let mut unchecked: Vec<Unchecked> = mesh_bodies
        .iter()
        .filter(|b| p.get("bodies").is_none_or(Value::is_null) || asked.contains(&b.name))
        .map(|b| Unchecked { a: b.name.clone(), b: None, reason: super::interference::MESH_NOT_CHECKED.to_string(), distance_mm: None, exact: None })
        .collect();
    let names: Vec<&str> = solid.iter().map(|b| b.name.as_str()).collect();
    let ctx = PairContext { names: &names, subjects: &subjects, boxes: &boxes, allowed: &allowed, gap, min_volume, nearest };
    let mut nearest_oom: Option<crate::EngineError> = None;
    let summary = collect_pair_results(&ctx, &close, results, &mut unchecked, |pairs| {
        if nearest_oom.is_some() {
            return Vec::new();
        }
        let mut out: Vec<solidcraft_kernel::Result<Option<f64>>> = Vec::new();
        if out.try_reserve_exact(pairs.len()).is_err() {
            nearest_oom = Some(bad(cmd, "not enough memory to compute nearest neighbours in inspect.clearance; reduce the number of bodies"));
            return Vec::new();
        }
        let mut pos = 0usize;
        while pos < pairs.len() {
            let avail = solidcraft_kernel::available_memory_bytes();
            if let Err(err) = check_memory_headroom(cmd, avail, pairs.len() - pos, pairs.len()) {
                nearest_oom = Some(err);
                return Vec::new();
            }
            let chunk_len = chunk_size_for_memory(avail);
            let end = (pos + chunk_len).min(pairs.len());
            let chunk = pairs.get(pos..end).unwrap_or(&[]);
            let mut missing: Vec<usize> = Vec::new();
            let mut seen: HashSet<usize> = HashSet::new();
            for &(i, j) in chunk {
                for b in [i, j] {
                    if shapes.get(b).is_some_and(Option::is_none) && seen.insert(b) {
                        missing.push(b);
                    }
                }
            }
            if !missing.is_empty() {
                let build_threads = clearance_worker_limit(missing.len(), avail);
                let built = solidcraft_kernel::map_on_threads(&missing, build_threads, |&i| -> Result<Option<DistanceShape<'_>>> {
                    let (Some(b), Some((m, mat))) = (solid.get(i), placed_meshes.get(i).and_then(Option::as_ref)) else {
                        return Ok(None);
                    };
                    let set = TriangleSet::try_from_indexed(&m.positions, &m.triangles, &m.tri_face, mat.as_ref()).map_err(|_| {
                        bad(cmd, format!("not enough memory to index body `{}` for inspect.clearance; reduce the number of bodies/pairs", b.name))
                    })?;
                    Ok(Some(DistanceShape::with_triangle_set(&b.body, m, *mat, set, b.display_tol())))
                });
                for (&i, item) in missing.iter().zip(built) {
                    let shape = match item {
                        Some(Ok(s)) => s,
                        Some(Err(err)) => {
                            nearest_oom = Some(err);
                            return Vec::new();
                        }
                        None => {
                            nearest_oom = Some(bad(cmd, "worker failed while indexing body for inspect.clearance"));
                            return Vec::new();
                        }
                    };
                    if let (Some(slot), Some(shape)) = (shapes.get_mut(i), shape) {
                        active_shape_bytes = active_shape_bytes.saturating_add(shape.estimated_bytes());
                        *slot = Some(shape);
                    }
                }
            }
            let threads = clearance_worker_limit(chunk.len(), avail);
            out.extend(solidcraft_kernel::map_with_local_pools(chunk, threads, |&(i, j)| {
                let (Some(a), Some(b)) = (shapes.get(i).and_then(Option::as_ref), shapes.get(j).and_then(Option::as_ref)) else {
                    return Ok(None);
                };
                Ok(solidcraft_kernel::body_distance(a, b, f64::INFINITY)?.map(|d| if d.distance < tolerance { 0.0 } else { d.distance }))
            }));
            let budget = shape_cache_budget(avail);
            if active_shape_bytes > budget {
                let next_end = (end + chunk_len).min(pairs.len());
                let mut keep_next: HashSet<usize> = HashSet::new();
                for &(i, j) in pairs.get(end..next_end).unwrap_or(&[]) {
                    keep_next.insert(i);
                    keep_next.insert(j);
                }
                trim_shapes(&keep_next, budget, &mut shapes, &mut active_shape_bytes);
            }
            pos = end;
        }
        out
    });
    if let Some(err) = nearest_oom {
        return Err(err);
    }

    let reply = ClearanceReply {
        overlaps: summary.overlaps,
        near: summary.near,
        nearest: summary.nearest,
        unchecked,
        bodies: subjects.len(),
        pairs_checked: close.len(),
        pairs_reused: reused,
        pairs_pruned: total.saturating_sub(close.len()),
        truncated: truncated > 0,
        ms: started.elapsed().as_secs_f64() * 1000.0,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

struct PairContext<'a> {
    names: &'a [&'a str],
    subjects: &'a [usize],
    boxes: &'a [Aabb3],
    allowed: &'a dyn Fn(usize, usize) -> bool,
    gap: f64,
    min_volume: f64,
    nearest: usize,
}

struct PairSummary {
    overlaps: Vec<Overlap>,
    near: Vec<NearPair>,
    nearest: BTreeMap<String, Vec<Neighbour>>,
}

/// Turns measured close pairs into `overlaps`, `near`, `unchecked` and (when asked) `nearest`.
/// Pairs whose measurement failed (`Err` or `r.unchecked`) are excluded from `nearest` so
/// `body_distance` cannot fall back to reporting an unchecked collision as `0.0` mm contact.
fn collect_pair_results(
    ctx: &PairContext<'_>,
    close: &[(usize, usize)],
    results: impl IntoIterator<Item = solidcraft_kernel::Result<PairResult>>,
    unchecked: &mut Vec<Unchecked>,
    mut measure_distance: impl FnMut(&[(usize, usize)]) -> Vec<solidcraft_kernel::Result<Option<f64>>>,
) -> PairSummary {
    let mut known: HashMap<(usize, usize), f64> = HashMap::new();
    let mut unchecked_pairs: HashSet<(usize, usize)> = HashSet::new();
    let track_nearest = ctx.nearest > 0;
    let (mut overlaps, mut near) = (Vec::new(), Vec::new());
    let name = |i: usize| ctx.names.get(i).copied().unwrap_or("");
    let pair = |i: usize, j: usize, reason: String, distance: Option<BodyDistance>| Unchecked {
        a: name(i).to_string(),
        b: Some(name(j).to_string()),
        reason,
        distance_mm: distance.map(|d| d.distance),
        exact: distance.and_then(|d| (!d.exact).then_some(false)),
    };
    let key = |i: usize, j: usize| (i.min(j), i.max(j));
    for (&(i, j), r) in close.iter().zip(results) {
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                unchecked.push(pair(i, j, e.to_string(), None));
                if track_nearest {
                    unchecked_pairs.insert(key(i, j));
                }
                continue;
            }
        };
        if let Some(reason) = r.unchecked {
            unchecked.push(pair(i, j, reason, r.distance));
            if track_nearest {
                unchecked_pairs.insert(key(i, j));
            }
            continue;
        }
        if let Some((volume, area, bbox, center)) = r.overlap
            && volume >= ctx.min_volume
        {
            overlaps.push(Overlap {
                a: name(i).to_string(),
                b: name(j).to_string(),
                volume_mm3: volume,
                area_mm2: area,
                bbox: OverlapBounds { min: bbox.min.into(), max: bbox.max.into() },
                center: center.into(),
            });
            if track_nearest {
                known.insert(key(i, j), 0.0);
            }
            continue;
        }
        let Some(d) = r.distance else { continue };
        if track_nearest {
            known.insert(key(i, j), d.distance);
        }
        if d.distance < ctx.gap {
            near.push(NearPair {
                a: name(i).to_string(),
                b: name(j).to_string(),
                distance_mm: d.distance,
                from: d.from.into(),
                to: d.to.into(),
                overlap_mm3: r.overlap.filter(|o| o.0 > 0.0).map(|o| o.0),
                exact: (!d.exact).then_some(false),
            });
        }
    }
    near.sort_by(|x, y| x.distance_mm.total_cmp(&y.distance_mm));

    let mut nearest_out = BTreeMap::new();
    if ctx.nearest > 0 {
        let nearest_allowed = |i: usize, j: usize| (ctx.allowed)(i, j) && !unchecked_pairs.contains(&key(i, j));
        let lists = nearest_lists(ctx.subjects, ctx.names.len(), ctx.nearest, ctx.boxes, &nearest_allowed, &mut known, &mut measure_distance);
        for (i, list) in lists {
            let rows = list.iter().map(|&(j, d)| Neighbour { body: name(j).to_string(), distance_mm: d }).collect();
            nearest_out.insert(name(i).to_string(), rows);
        }
    }
    PairSummary { overlaps, near, nearest: nearest_out }
}

/// One pair: its overlap when the meshes meet or one holds the other, otherwise its distance
/// when it may be closer than `gap`.
#[allow(clippy::too_many_arguments)]
fn measure_pair(
    (ka, pa): (&solidcraft_kernel::Body, &PlacedMeshRef<'_>),
    a: &DistanceShape,
    (kb, pb): (&solidcraft_kernel::Body, &PlacedMeshRef<'_>),
    b: &DistanceShape,
    gap: f64,
    min_volume: f64,
    tolerance: f64,
    incomplete_pair: bool,
) -> PairResult {
    let mut out = PairResult::default();
    let slack = 2.0 * (a.chord() + b.chord());
    let mesh = a.mesh_closest(b, gap + slack);
    let holds = |outer: &PlacedMeshRef<'_>, inner: &PlacedMeshRef<'_>| {
        let (ob, ib) = (outer.mesh_bounds, inner.mesh_bounds);
        let nested = ob.min.x <= ib.min.x
            && ob.min.y <= ib.min.y
            && ob.min.z <= ib.min.z
            && ib.max.x <= ob.max.x
            && ib.max.y <= ob.max.y
            && ib.max.z <= ob.max.z;
        if !nested {
            return false;
        }
        let Some(&p) = inner.mesh.positions.first() else { return false };
        let world_p = match inner.transform {
            Some(m) => solidcraft_doc::apply_point(&m, p),
            None => p,
        };
        let local_p = match outer.transform {
            Some(m) => {
                let Some(inv) = solidcraft_doc::mat_inverse(&m) else { return false };
                solidcraft_doc::apply_point(&inv, world_p)
            }
            None => world_p,
        };
        outer.mesh.contains(local_p)
    };
    let touching = mesh.is_some_and(|m| m.distance <= slack);
    let may_overlap = touching || holds(pa, pb) || holds(pb, pa) || (incomplete_pair && !solidcraft_kernel::apart(ka, kb).unwrap_or(false));
    if may_overlap {
        intersect_pair((ka, a), (kb, b), min_volume, tolerance, incomplete_pair, &mut out);
        if out.unchecked.is_some() {
            return out;
        }
        if out.overlap.is_some_and(|o| o.0 >= min_volume) {
            return out;
        }
        if out.overlap.is_some_and(|o| o.0 > 0.0) {
            // Contact with a sliver of shared volume.
            let at = out.overlap.map(|o| o.3).unwrap_or_default();
            out.distance = Some(BodyDistance { distance: 0.0, from: at, to: at, exact: true });
            return out;
        }
    }
    if mesh.is_none() {
        if incomplete_pair && gap > 0.0 {
            out.unchecked = Some("mesh incomplete: clearance could not be verified on unmeshed faces".into());
        }
        return out;
    }
    match solidcraft_kernel::body_distance(a, b, gap) {
        Ok(Some(mut d)) => {
            if d.distance < tolerance {
                d.distance = 0.0;
            }
            if incomplete_pair && (d.distance > 0.0 || !d.exact) {
                out.unchecked = Some("mesh incomplete: clearance could not be verified on unmeshed faces".into());
            } else {
                out.distance = Some(d);
            }
        }
        Ok(None) => {
            if incomplete_pair && gap > 0.0 {
                out.unchecked = Some("mesh incomplete: clearance could not be verified on unmeshed faces".into());
            }
        }
        Err(e) => out.unchecked = Some(e.to_string()),
    }
    out
}

/// Runs the exact intersection boolean for one pair under `Validation::EachStep`, retrying in
/// reversed operand order, checking certified disjointness (`interiors_disjoint`), retrying
/// shifted near the shared region on kernel failure, and falling back to a sanity-checked
/// `Validation::Off` measurement when SMLib's check alone refused the temporary overlap solid
/// while both operands passed; verifies an empty boolean result against exact interior point
/// witnesses so a real overlap is never silently reported as clear; and attaches a non-boolean
/// fallback distance when a pair still lands in `unchecked`.
fn intersect_pair(
    (ka, a): (&solidcraft_kernel::Body, &DistanceShape),
    (kb, b): (&solidcraft_kernel::Body, &DistanceShape),
    min_volume: f64,
    tolerance: f64,
    incomplete_pair: bool,
    out: &mut PairResult,
) {
    let at = shared_center(a, b, ka, kb);
    let is_cancelled = || solidcraft_kernel::watch::current().is_some_and(|w| w.cancelled());
    if is_cancelled() {
        out.unchecked = Some("cancelled".into());
        return;
    }
    let fallback_distance = |out: &mut PairResult| {
        if is_cancelled() {
            return;
        }
        if let Ok(Some(mut d)) = solidcraft_kernel::body_distance(a, b, f64::INFINITY) {
            if d.distance < tolerance {
                d.distance = 0.0;
            }
            if !incomplete_pair || (d.distance == 0.0 && d.exact) {
                out.distance = Some(d);
            }
        }
    };
    let record_overlap = |m: solidcraft_kernel::MassMeasure, out: &mut PairResult| {
        if m.volume > 0.0
            && let Some(why) = super::interference::impossible_overlap(m.volume, &m.bbox, ka, kb)
        {
            out.unchecked = Some(why);
            return;
        }
        out.overlap = Some((m.volume, m.area, m.bbox, m.centroid));
    };
    let verify_empty = |out: &mut PairResult| {
        if is_cancelled() {
            out.unchecked = Some("cancelled".into());
            return;
        }
        match solidcraft_kernel::overlap_witness(a, b) {
            Ok(None) => {}
            Err(e) => {
                out.unchecked = Some(e.to_string());
                fallback_distance(out);
            }
            Ok(Some(witness)) => {
                // Exact point classification proved interior overlap despite the empty boolean:
                // retry shifted directly to the witness point (both operand orders) before
                // recording the pair as unchecked.
                let recovered = if is_cancelled() {
                    None
                } else if let Some(m) = intersect_shifted(ka, kb, witness, false) {
                    Some(m)
                } else if !is_cancelled() {
                    intersect_shifted(ka, kb, witness, true)
                } else {
                    None
                };
                if let Some(m) = recovered {
                    record_overlap(m, out);
                } else {
                    out.unchecked = Some(format!(
                        "overlap at [{:.3}, {:.3}, {:.3}] could not be intersected by the boolean kernel",
                        witness.x, witness.y, witness.z
                    ));
                    fallback_distance(out);
                }
            }
        }
    };

    // A pair whose shared part can't be worked out is reported, not passed as clear. SMLib's
    // check of the shared part runs here: a part it refuses is reported unless a sanity-checked
    // measurement-only fallback recovers a valid overlap when both operands passed SMLib's check.
    match run_intersect(ka, kb) {
        Ok(Some(o)) => match measure_near_origin(&o, at) {
            Ok(m) => record_overlap(m, out),
            Err(e) => {
                out.unchecked = Some(e.to_string());
                fallback_distance(out);
            }
        },
        Err(first_err) => {
            if is_cancelled() {
                out.unchecked = Some(first_err.to_string());
                return;
            }
            match run_intersect(kb, ka) {
                Ok(Some(o)) => match measure_near_origin(&o, at) {
                    Ok(m) => record_overlap(m, out),
                    Err(e) => {
                        out.unchecked = Some(e.to_string());
                        fallback_distance(out);
                    }
                },
                Ok(None) => verify_empty(out),
                Err(second_err) => {
                    if is_cancelled() {
                        out.unchecked = Some(first_err.to_string());
                        return;
                    }
                    if solidcraft_kernel::interiors_disjoint(ka, kb) == Some(true) {
                        return;
                    }
                    if !is_cancelled()
                        && let Some(m) = retry_shifted_intersect(ka, kb, at, &is_cancelled)
                    {
                        record_overlap(m, out);
                        return;
                    }
                    match super::interference::fallback_unchecked_overlap(
                        ka,
                        kb,
                        (&first_err, &second_err),
                        |o| measure_near_origin(o, at),
                        |v| v > 0.0 && v >= min_volume,
                        is_cancelled,
                    ) {
                        super::interference::UncheckedFallback::Measured(m) => {
                            out.overlap = Some((m.volume, m.area, m.bbox, m.centroid));
                        }
                        super::interference::UncheckedFallback::Cancelled => {
                            out.unchecked = Some(first_err.to_string());
                        }
                        super::interference::UncheckedFallback::NotRecovered => {
                            out.unchecked = Some(first_err.to_string());
                            fallback_distance(out);
                        }
                    }
                }
            }
        }
        Ok(None) => verify_empty(out),
    }
}

fn run_intersect(a: &solidcraft_kernel::Body, b: &solidcraft_kernel::Body) -> solidcraft_kernel::Result<Option<solidcraft_kernel::Body>> {
    solidcraft_kernel::validating(solidcraft_kernel::Validation::EachStep, || solidcraft_kernel::boolean(a, b, solidcraft_kernel::BoolOp::Intersect))
        .0
}

/// Retries a failed intersection near the origin (when placed away from it) in both operand orders.
/// Only a validated non-empty solid result replaces the original error.
fn retry_shifted_intersect(
    ka: &solidcraft_kernel::Body,
    kb: &solidcraft_kernel::Body,
    at: solidcore_record::Vec3,
    is_cancelled: &impl Fn() -> bool,
) -> Option<solidcraft_kernel::MassMeasure> {
    let far = at.x.abs().max(at.y.abs()).max(at.z.abs()) > 1.0;
    if !far || is_cancelled() {
        return None;
    }
    if let Some(m) = intersect_shifted(ka, kb, at, false) {
        return Some(m);
    }
    if is_cancelled() {
        return None;
    }
    intersect_shifted(ka, kb, at, true)
}

/// Intersects `ka` and `kb` translated by `-at` (so the shared region sits at the origin) and
/// translates the measured overlap box and centroid back by `+at`.
fn intersect_shifted(
    ka: &solidcraft_kernel::Body,
    kb: &solidcraft_kernel::Body,
    at: solidcore_record::Vec3,
    swap: bool,
) -> Option<solidcraft_kernel::MassMeasure> {
    if !at.is_finite() {
        return None;
    }
    let ta = solidcraft_kernel::transform(ka, -at, solidcore_record::Vec3::ZERO, solidcore_record::Vec3::Z, 0.0).ok()?;
    let tb = solidcraft_kernel::transform(kb, -at, solidcore_record::Vec3::ZERO, solidcore_record::Vec3::Z, 0.0).ok()?;
    let (first, second) = if swap { (&tb, &ta) } else { (&ta, &tb) };
    let overlap = run_intersect(first, second).ok()??;
    let mut m = solidcraft_kernel::measure_mass(&overlap).ok()?;
    if !m.volume.is_finite() || m.volume <= 0.0 {
        return None;
    }
    m.centroid += at;
    m.bbox = Aabb3 { min: m.bbox.min + at, max: m.bbox.max + at };
    Some(m)
}

/// Center of the shared region of `a` and `b`, falling back to their exact bounding boxes when a
/// display mesh is empty or incomplete.
fn shared_center(a: &DistanceShape, b: &DistanceShape, ka: &solidcraft_kernel::Body, kb: &solidcraft_kernel::Body) -> solidcore_record::Vec3 {
    let region = shared_region(a, b);
    if !region.is_empty() {
        return region.center();
    }
    if let (Ok(ea), Ok(eb)) = (ka.exact_bounds(), kb.exact_bounds()) {
        let exact = ea.intersection(&eb);
        if !exact.is_empty() {
            return exact.center();
        }
    }
    solidcore_record::Vec3::ZERO
}

/// The box where two bodies may share volume: where their boxes meet, grown by a margin.
fn shared_region(a: &DistanceShape, b: &DistanceShape) -> Aabb3 {
    let (x, y) = (a.bounds(), b.bounds());
    let min = x.min.max(y.min);
    let max = x.max.min(y.max);
    if x.is_empty() || y.is_empty() || !(min.x <= max.x && min.y <= max.y && min.z <= max.z) {
        return Aabb3::EMPTY;
    }
    let grow = 1.0 + 0.05 * min.dist(max);
    let g = solidcore_record::Vec3::new(grow, grow, grow);
    Aabb3 { min: min - g, max: max + g }
}

/// [`solidcraft_kernel::measure_mass`] of `body` moved by `-at` and back: far from the origin, the
/// centre of a thin part loses its precision (its moments are large numbers that nearly cancel).
fn measure_near_origin(body: &solidcraft_kernel::Body, at: solidcore_record::Vec3) -> solidcraft_kernel::Result<solidcraft_kernel::MassMeasure> {
    let moved = solidcraft_kernel::transform(body, -at, solidcore_record::Vec3::ZERO, solidcore_record::Vec3::Z, 0.0)?;
    let mut m = solidcraft_kernel::measure_mass(&moved)?;
    m.centroid += at;
    m.bbox = Aabb3 { min: m.bbox.min + at, max: m.bbox.max + at };
    Ok(m)
}

/// Each subject's `count` nearest bodies, nearest first: pairs are measured in the order of their
/// boxes' gaps (a lower bound of their distance), in rounds, until the boxes of the next ones
/// are farther apart than the last neighbour kept. `known` holds distances already measured.
fn nearest_lists(
    subjects: &[usize],
    n: usize,
    count: usize,
    boxes: &[Aabb3],
    allowed: &dyn Fn(usize, usize) -> bool,
    known: &mut HashMap<(usize, usize), f64>,
    mut measure: impl FnMut(&[(usize, usize)]) -> Vec<solidcraft_kernel::Result<Option<f64>>>,
) -> BTreeMap<usize, Vec<(usize, f64)>> {
    let key = |i: usize, j: usize| (i.min(j), i.max(j));
    let gap_of = |i: usize, j: usize| match (boxes.get(i), boxes.get(j)) {
        (Some(a), Some(b)) => box_gap(a, b),
        _ => f64::INFINITY,
    };
    // Each subject's candidates, by box gap.
    let mut order: BTreeMap<usize, (Vec<(f64, usize)>, usize)> = BTreeMap::new();
    for &i in subjects {
        let mut c: Vec<(f64, usize)> = (0..n).filter(|&j| allowed(i, j)).map(|j| (gap_of(i, j), j)).filter(|(g, _)| g.is_finite()).collect();
        c.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        order.insert(i, (c, 0));
    }
    let mut failed: HashSet<(usize, usize)> = HashSet::new();
    let measured = |known: &HashMap<(usize, usize), f64>, i: usize, c: &[(f64, usize)]| -> Vec<(usize, f64)> {
        let mut l: Vec<(usize, f64)> = c.iter().filter_map(|&(_, j)| known.get(&key(i, j)).map(|d| (j, *d))).collect();
        l.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.0.cmp(&y.0)));
        l
    };
    // Each round measures, for every subject not settled yet, the next `count` candidates.
    for _ in 0..n.max(1) {
        let mut todo: Vec<(usize, usize)> = Vec::new();
        for (&i, (c, next)) in order.iter_mut() {
            let have = measured(known, i, c);
            let mut added = 0;
            while let Some(&(g, j)) = c.get(*next) {
                let bound = if have.len() >= count { have.get(count - 1).map_or(f64::INFINITY, |x| x.1) } else { f64::INFINITY };
                if g > bound || added >= count {
                    break;
                }
                *next += 1;
                let k = key(i, j);
                if !known.contains_key(&k) && !failed.contains(&k) && !todo.contains(&k) {
                    todo.push(k);
                    added += 1;
                }
            }
        }
        if todo.is_empty() {
            break;
        }
        for (k, r) in todo.iter().zip(measure(&todo)) {
            match r {
                Ok(Some(d)) => {
                    known.insert(*k, d);
                }
                _ => {
                    failed.insert(*k);
                }
            }
        }
    }
    order.iter().map(|(&i, (c, _))| (i, measured(known, i, c).into_iter().take(count).collect())).collect()
}

#[cfg(test)]
#[path = "clearance_tests.rs"]
mod tests;
