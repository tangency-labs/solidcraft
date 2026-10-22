//! Clearance (`inspect.clearance`): in one call, which bodies overlap and by how much, which
//! come closer than a clearance and where, and each body's nearest neighbours. Pairs are pruned
//! by bounding boxes grown by the clearance, then by the bodies' meshes (which lie within their
//! chord tolerance of the faces); distances are measured on the exact faces and edges
//! ([`solidcraft_kernel::body_distance`]) and overlaps by intersecting the bodies. The pairs
//! that are left run in parallel. A pair whose bodies have not changed or moved since a call
//! with the same settings measured it is not measured again ([`PairCache`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::mesh_distance::box_gap;
use solidcraft_geom::{Aabb3, Mesh};
use solidcraft_kernel::{BodyDistance, DistanceShape};

use super::CommandSpec;
use crate::params::{bad, num, str_, string_list};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.clearance", "Clearance", clearance).icon("interference").noundo().params(
    "bodies?: [names] (default: every solid body, in world placement, but those in `against`; mesh bodies are not measured); against?: [names] (only pairs with one body in `bodies` and the other here); \
     min_volume?: mm³ (default 0.01: smaller overlaps count as contact); clearance?: mm (default 0: report pairs closer than this); \
     nearest?: n (each body of `bodies` gets its n nearest others); skip_within?: \"occurrence\" (pairs of bodies of one occurrence) | \
     \"component\" (pairs of bodies inside one placed component, its subcomponents included) | null; tolerance?: mm (default 1e-6: a gap \
     below it is contact, 0) → overlaps: [{a, b, volume_mm3, area_mm2, bbox, center}]; near: [{a, b, distance_mm, from, to, overlap_mm3?: \
     a contact's shared volume below min_volume, exact?: false when measured on the meshes}], nearest by body (the same for pairs that overlap: 0); \
     nearest: {body: [{body, distance_mm}]}; unchecked: [{a, b, reason}] (pairs whose overlap could not be worked out; each mesh body, STL/3MF/OBJ import, is not measured and is listed as {a: name, b: null, reason: \"mesh body: not checked\"}, also when named in `bodies` or `against`; the result's `bodies` counts the solid bodies only); pairs_checked; \
     pairs_reused (of those, measured by an earlier call: neither body changed or moved since); pairs_pruned (ruled out by their boxes); ms. Distances are measured on the exact faces and edges, the meshes only rule pairs out",
)];

/// Most bodies one call takes.
const MAX_BODIES: usize = 20_000;
/// Most pairs whose grown boxes meet that one call measures.
const MAX_PAIRS: usize = 200_000;
/// Most neighbours a body's `nearest` list holds.
const MAX_NEAREST: usize = 1000;
/// Largest clearance (mm).
const MAX_CLEARANCE: f64 = 1e6;

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

/// What one pair came to.
#[derive(Clone, Default)]
struct PairResult {
    /// Shared volume, area, box and centre.
    overlap: Option<(f64, f64, Aabb3, solidcraft_geom::Vec3)>,
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

    let st = s.world_state()?;
    let (mesh_bodies, solid): (Vec<&solidcraft_doc::ModelBody>, Vec<&solidcraft_doc::ModelBody>) = st.bodies.iter().partition(|b| b.body.is_mesh());
    // Mesh bodies are not measured. A mesh body named in `bodies` or `against` is left out of the
    // pairs and listed as unchecked, rather than refused as "no solid body".
    let named = |key: &str| -> Result<Vec<usize>> {
        let names = string_list(p, key);
        let mut out = Vec::new();
        for n in &names {
            let Some(i) = solid.iter().position(|b| &b.name == n) else {
                if mesh_bodies.iter().any(|b| &b.name == n) {
                    continue;
                }
                return Err(bad(cmd, format!("no solid body `{n}`")));
            };
            if !out.contains(&i) {
                out.push(i);
            }
        }
        Ok(out)
    };
    let against = if p.get("against").is_none_or(Value::is_null) { None } else { Some(named("against")?) };
    // Without `bodies`: every body (every body not in `against`, when it is given).
    let subjects = if p.get("bodies").is_none_or(Value::is_null) {
        (0..solid.len()).filter(|i| against.as_ref().is_none_or(|a| !a.contains(i))).collect()
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
        let (a, b) = (path_of(i), path_of(j));
        match skip {
            SkipWithin::Nothing => true,
            SkipWithin::Occurrence => a.is_empty() || a != b,
            SkipWithin::Component => a.is_empty() || a.first() != b.first(),
        }
    };

    // Meshes (made once, in parallel) and the distance shapes over them.
    let idx: Vec<usize> = (0..n).filter(|&i| flag(&used, i)).collect();
    let meshes: Vec<Option<Arc<Mesh>>> = {
        let made = solidcraft_kernel::map_with_local_pools(&idx, usize::MAX, |&i| Ok(solid.get(i).map(|b| b.mesh())));
        let mut out = vec![None; n];
        for (&i, m) in idx.iter().zip(made) {
            if let (Some(slot), Ok(m)) = (out.get_mut(i), m) {
                *slot = m;
            }
        }
        out
    };
    // The distance shapes are made only for bodies a pair or a nearest list measures; the boxes
    // come from the meshes, grown by the chord tolerance as a shape's are.
    let shapes: Vec<std::sync::OnceLock<Option<DistanceShape>>> = (0..n).map(|_| std::sync::OnceLock::new()).collect();
    let shape = |i: usize| {
        shapes
            .get(i)?
            .get_or_init(|| match (solid.get(i), meshes.get(i).and_then(Option::as_ref)) {
                (Some(b), Some(m)) if flag(&used, i) => Some(DistanceShape::new(&b.body, m, b.display_tol())),
                _ => None,
            })
            .as_ref()
    };
    let boxes: Vec<Aabb3> = (0..n)
        .map(|i| match (solid.get(i), meshes.get(i).and_then(Option::as_ref)) {
            (Some(b), Some(m)) if flag(&used, i) => {
                // Over the triangles with finite corners, as a shape's set of them is.
                let mut bx = Aabb3::EMPTY;
                for p in m.triangles.iter().filter_map(|t| m.tri(t)).filter(|p| p.iter().all(|v| v.is_finite())) {
                    p.into_iter().for_each(|v| bx.add(v));
                }
                let chord = b.display_tol();
                let g = solidcraft_geom::Vec3::new(1.0, 1.0, 1.0) * if chord.is_finite() && chord > 0.0 { chord } else { 0.0 };
                if bx.is_empty() { bx } else { Aabb3 { min: bx.min - g, max: bx.max + g } }
            }
            _ => Aabb3::EMPTY,
        })
        .collect();

    // Pairs whose boxes, grown by half the clearance each, meet.
    let half = solidcraft_geom::Vec3::new(1.0, 1.0, 1.0) * (gap / 2.0);
    let grown: Vec<Aabb3> = boxes.iter().map(|b| if b.is_empty() { *b } else { Aabb3 { min: b.min - half, max: b.max + half } }).collect();
    let (close, truncated) = super::joints::overlapping_pairs(&grown, MAX_PAIRS);
    let has_mesh = |i: usize| meshes.get(i).is_some_and(Option::is_some);
    let close: Vec<(usize, usize)> = close.into_iter().filter(|&(i, j)| allowed(i, j) && has_mesh(i) && has_mesh(j)).collect();
    let mut total = 0usize;
    for i in 0..n {
        for j in i + 1..n {
            if allowed(i, j) {
                total += 1;
            }
        }
    }

    // Pairs measured before come from the cache; the rest are measured and kept.
    let settings = [gap.to_bits(), min_volume.to_bits(), tolerance.to_bits()];
    let cache_key = |i: usize, j: usize| Some((solid.get(i)?.geometry_key()?, solid.get(j)?.geometry_key()?, settings));
    let mut results: Vec<Option<solidcraft_kernel::Result<PairResult>>> = {
        let cache = s.clearance_cache.lock().ok();
        close.iter().map(|&(i, j)| cache.as_ref().and_then(|c| c.0.get(&cache_key(i, j)?)).cloned().map(Ok)).collect()
    };
    let todo: Vec<usize> = (0..close.len()).filter(|&k| results.get(k).is_some_and(Option::is_none)).collect();
    let reused = close.len() - todo.len();
    // A body in many pairs is cut down to each pair's region from the same mesh of it.
    let kept: Vec<std::sync::OnceLock<Option<solidcraft_kernel::Body>>> = (0..n).map(|_| std::sync::OnceLock::new()).collect();
    let kept_body = |i: usize| kept.get(i)?.get_or_init(|| solid.get(i).map(|b| b.body.keeping_meshes())).as_ref();
    let measured = solidcraft_kernel::map_with_local_pools(&todo, usize::MAX, |&k| {
        let Some(&(i, j)) = close.get(k) else { return Ok(PairResult::default()) };
        let (Some(a), Some(b)) = (shape(i), shape(j)) else { return Ok(PairResult::default()) };
        let (Some(ba), Some(bb)) = (solid.get(i), solid.get(j)) else { return Ok(PairResult::default()) };
        let (Some(ka), Some(kb)) = (kept_body(i), kept_body(j)) else { return Ok(PairResult::default()) };
        Ok(measure_pair((ba, ka), a, (bb, kb), b, gap, min_volume, tolerance))
    });
    if let Ok(mut cache) = s.clearance_cache.lock() {
        for (&k, r) in todo.iter().zip(&measured) {
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
    for (k, r) in todo.into_iter().zip(measured) {
        if let Some(slot) = results.get_mut(k) {
            *slot = Some(r);
        }
    }
    let results = results.into_iter().map(|r| r.unwrap_or_else(|| Ok(PairResult::default())));

    let mut known: HashMap<(usize, usize), f64> = HashMap::new();
    let (mut overlaps, mut near) = (Vec::new(), Vec::new());
    // The mesh bodies the call was about (every one without `bodies`, else those named in
    // `bodies` or `against`) come first, so "nothing found" is not read as "no overlap".
    let asked: Vec<String> = ["bodies", "against"].iter().flat_map(|k| string_list(p, k)).collect();
    let mut unchecked: Vec<Value> = mesh_bodies
        .iter()
        .filter(|b| p.get("bodies").is_none_or(Value::is_null) || asked.contains(&b.name))
        .map(|b| json!({"a": b.name, "b": null, "reason": super::interference::MESH_NOT_CHECKED}))
        .collect();
    let name = |i: usize| solid.get(i).map_or("", |b| b.name.as_str());
    for (&(i, j), r) in close.iter().zip(results) {
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                unchecked.push(json!({"a": name(i), "b": name(j), "reason": e.to_string()}));
                continue;
            }
        };
        if let Some(reason) = r.unchecked {
            unchecked.push(json!({"a": name(i), "b": name(j), "reason": reason}));
            continue;
        }
        if let Some((volume, area, bbox, center)) = r.overlap
            && volume >= min_volume
        {
            overlaps.push(json!({"a": name(i), "b": name(j), "volume_mm3": volume, "area_mm2": area,
                "bbox": {"min": bbox.min, "max": bbox.max}, "center": center}));
            known.insert((i, j), 0.0);
            continue;
        }
        let Some(d) = r.distance else { continue };
        known.insert((i, j), d.distance);
        if d.distance < gap {
            let mut e = json!({"a": name(i), "b": name(j), "distance_mm": d.distance, "from": d.from, "to": d.to});
            if let Some((v, ..)) = r.overlap.filter(|o| o.0 > 0.0) {
                e["overlap_mm3"] = json!(v);
            }
            if !d.exact {
                e["exact"] = json!(false);
            }
            near.push(e);
        }
    }
    near.sort_by(|x, y| x["distance_mm"].as_f64().unwrap_or(0.0).total_cmp(&y["distance_mm"].as_f64().unwrap_or(0.0)));

    let mut nearest_out = serde_json::Map::new();
    if nearest > 0 {
        let lists = nearest_lists(&subjects, n, nearest, &boxes, &allowed, &mut known, |pairs| {
            solidcraft_kernel::map_with_local_pools(pairs, usize::MAX, |&(i, j)| {
                let (Some(a), Some(b)) = (shape(i), shape(j)) else { return Ok(None) };
                Ok(solidcraft_kernel::body_distance(a, b, f64::INFINITY)?.map(|d| if d.distance < tolerance { 0.0 } else { d.distance }))
            })
        });
        for (i, list) in lists {
            let rows: Vec<Value> = list.iter().map(|&(j, d)| json!({"body": name(j), "distance_mm": d})).collect();
            nearest_out.insert(name(i).to_string(), Value::Array(rows));
        }
    }

    Ok(json!({
        "overlaps": overlaps,
        "near": near,
        "nearest": nearest_out,
        "unchecked": unchecked,
        "bodies": subjects.len(),
        "pairs_checked": close.len(),
        "pairs_reused": reused,
        "pairs_pruned": total.saturating_sub(close.len()),
        "truncated": truncated > 0,
        "ms": started.elapsed().as_secs_f64() * 1000.0,
    }))
}

/// One pair: its overlap when the meshes meet or one holds the other, otherwise its distance
/// when it may be closer than `gap`.
fn measure_pair(
    (ba, ka): (&solidcraft_doc::ModelBody, &solidcraft_kernel::Body),
    a: &DistanceShape,
    (bb, kb): (&solidcraft_doc::ModelBody, &solidcraft_kernel::Body),
    b: &DistanceShape,
    gap: f64,
    min_volume: f64,
    tolerance: f64,
) -> PairResult {
    let mut out = PairResult::default();
    let slack = 2.0 * (a.chord() + b.chord());
    let mesh = a.mesh_closest(b, gap + slack);
    let holds = |outer: &solidcraft_doc::ModelBody, inner: &solidcraft_doc::ModelBody| {
        let (o, i) = (outer.mesh(), inner.mesh());
        let (ob, ib) = (o.bounds(), i.bounds());
        let nested = ob.min.x <= ib.min.x
            && ob.min.y <= ib.min.y
            && ob.min.z <= ib.min.z
            && ib.max.x <= ob.max.x
            && ib.max.y <= ob.max.y
            && ib.max.z <= ob.max.z;
        nested && i.positions.first().is_some_and(|p| o.contains(*p))
    };
    let touching = mesh.is_some_and(|m| m.distance <= slack);
    if touching || holds(ba, bb) || holds(bb, ba) {
        // A pair whose shared part can't be worked out is reported, not passed as clear.
        match solidcraft_kernel::boolean(ka, kb, solidcraft_kernel::BoolOp::Intersect) {
            Err(e) => {
                out.unchecked = Some(e.to_string());
                return out;
            }
            Ok(Some(o)) => match measure_near_origin(&o, shared_region(a, b).center()) {
                Ok(m) => out.overlap = Some((m.volume, m.area, m.bbox, m.centroid)),
                Err(e) => {
                    out.unchecked = Some(e.to_string());
                    return out;
                }
            },
            Ok(None) => {}
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
        return out;
    }
    match solidcraft_kernel::body_distance(a, b, gap) {
        Ok(Some(mut d)) => {
            if d.distance < tolerance {
                d.distance = 0.0;
            }
            out.distance = Some(d);
        }
        Ok(None) => {}
        Err(e) => out.unchecked = Some(e.to_string()),
    }
    out
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
    let g = solidcraft_geom::Vec3::new(grow, grow, grow);
    Aabb3 { min: min - g, max: max + g }
}

/// [`solidcraft_kernel::measure`] of `body` moved by `-at` and back: far from the origin, the
/// centre of a thin part loses its precision (its moments are large numbers that nearly cancel).
fn measure_near_origin(body: &solidcraft_kernel::Body, at: solidcraft_geom::Vec3) -> solidcraft_kernel::Result<solidcraft_kernel::BodyMeasure> {
    let moved = solidcraft_kernel::transform(body, -at, solidcraft_geom::Vec3::ZERO, solidcraft_geom::Vec3::Z, 0.0)?;
    let mut m = solidcraft_kernel::measure(&moved)?;
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
    measure: impl Fn(&[(usize, usize)]) -> Vec<solidcraft_kernel::Result<Option<f64>>>,
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
    let mut failed: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
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
