//! Interference (`inspect.interference`): the pairs of bodies that share volume, and how much.
//!
//! Pairs are pruned in three steps before anything exact runs. Their boxes first (sort and
//! sweep), then their display meshes: a mesh lies within its chord tolerance of the faces, so
//! two meshes farther apart than a margin over both tolerances belong to bodies whose boundaries
//! don't meet, and unless one body sits inside the other (a point of each face of one tested
//! against the other's mesh) the pair is clear without a boolean. The pairs left are intersected
//! exactly and measured, in parallel on desktop (one after another in the browser), the slowest
//! first, under a time budget: pairs not finished when it runs out are listed as not checked
//! rather than holding the call. The shared part is measured as `solidcraft_kernel::measure`
//! measures it (its mass properties alone).

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_geom::mesh_distance::TriangleSet;
use solidcraft_geom::{Aabb3, Mesh, Vec3};
use solidcraft_kernel::Body;
use solidcraft_kernel::profile::KernelTimes;

use crate::params::{bad, num, string_list};
use crate::{Result, Session};

/// Most pairs with overlapping bounding boxes one call intersects.
pub(crate) const MAX_INTERFERENCE_PAIRS: usize = 20_000;
/// The time budget when the call gives none (s): under the 300 s an MCP client waits for a tool.
pub(crate) const DEFAULT_BUDGET_S: f64 = 240.0;
/// The longest time budget a call may ask for (s).
const MAX_BUDGET_S: f64 = 86_400.0;
/// How many chord tolerances apart (of both meshes together) two meshes must be to clear a pair
/// without intersecting it: twice what the tolerances allow, for meshers that sag a little more.
const MESH_MARGIN: f64 = 4.0;
/// Why a mesh body is listed under `unchecked` (here and in `inspect.clearance`).
pub(super) const MESH_NOT_CHECKED: &str = "mesh body: not checked";
/// Most pairs listed under `slowest`.
const SLOWEST: usize = 8;

/// One pair to intersect exactly: the bodies (cheap clones sharing the solids) and their names.
struct Job {
    pair: usize,
    a: Body,
    b: Body,
    names: (String, String),
    /// Faces of one times faces of the other: the slowest pairs start first.
    cost: usize,
}

/// What an exact check of one pair came to.
enum Outcome {
    Hit(Value),
    Clear,
    Unchecked(String),
}

/// One pair's answer and where its time went.
struct Done {
    outcome: Outcome,
    /// Unverified boolean results are listed as unchecked and still reported as a hit.
    note: Option<String>,
    ms: f64,
    kernel: KernelTimes,
}

pub(super) fn interference(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.interference";
    let started = web_time::Instant::now();
    let budget_s = match p.get("budget_s") {
        None | Some(Value::Null) => DEFAULT_BUDGET_S,
        Some(_) => match num(p, "budget_s") {
            Some(v) if v > 0.0 && v <= MAX_BUDGET_S => v,
            _ => return Err(bad(cmd, format!("`budget_s` must be a number of seconds above 0, at most {MAX_BUDGET_S}"))),
        },
    };
    let budget = std::time::Duration::from_secs_f64(budget_s);
    let want = string_list(cmd, p, "bodies")?;
    let st = s.world_state();
    let (meshes_left, bodies): (Vec<&solidcraft_doc::ModelBody>, Vec<&solidcraft_doc::ModelBody>) =
        st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).partition(|b| b.body.is_mesh());

    // The bodies' display meshes, made once (in parallel) and kept on the bodies: a design open
    // in the app has them already.
    let meshes: Vec<Arc<Mesh>> = {
        let made = solidcraft_kernel::map_with_local_pools(&bodies, usize::MAX, |b| Ok(b.mesh()));
        made.into_iter().map(|m| m.unwrap_or_default()).collect()
    };
    // Prune by bounding boxes first (sort and sweep), so the cost follows the pairs that can
    // touch rather than the square of the body count.
    // Each mesh lies within its chord of the faces, inside a round one: grown by that, the boxes
    // of bodies a hundredth of a millimetre into each other still meet (an arm's round end into a
    // rounded rim, its facets clear of the rim's).
    let chords: Vec<f64> = bodies.iter().map(|b| b.display_tol()).collect();
    let boxes: Vec<Aabb3> = meshes
        .iter()
        .zip(&chords)
        .map(|(m, &chord)| {
            let (bx, g) = (m.bounds(), 2.0 * chord);
            if bx.is_empty() { bx } else { Aabb3 { min: bx.min - Vec3::new(g, g, g), max: bx.max + Vec3::new(g, g, g) } }
        })
        .collect();
    let (pairs, skipped) = super::joints::overlapping_pairs(&boxes, MAX_INTERFERENCE_PAIRS);

    // Then by the meshes: pairs whose meshes stay apart (and neither holds the other) are clear.
    let used: Vec<bool> = {
        let mut u = vec![false; bodies.len()];
        for &(i, j) in &pairs {
            for k in [i, j] {
                if let Some(x) = u.get_mut(k) {
                    *x = true;
                }
            }
        }
        u
    };
    let sets: Vec<Option<TriangleSet>> = {
        let idx: Vec<usize> = (0..bodies.len()).filter(|&i| used.get(i).copied().unwrap_or(false)).collect();
        let made = solidcraft_kernel::map_with_local_pools(&idx, usize::MAX, |&i| Ok(meshes.get(i).map(|m| TriangleSet::new(m))));
        let mut out: Vec<Option<TriangleSet>> = std::iter::repeat_with(|| None).take(bodies.len()).collect();
        for (&i, set) in idx.iter().zip(made) {
            if let (Some(slot), Ok(set)) = (out.get_mut(i), set) {
                *slot = set;
            }
        }
        out
    };
    let need_exact = solidcraft_kernel::map_with_local_pools(&pairs, usize::MAX, |&(i, j)| {
        let (Some(Some(sa)), Some(Some(sb)), Some(ma), Some(mb)) = (sets.get(i), sets.get(j), meshes.get(i), meshes.get(j)) else {
            return Ok(true);
        };
        let margin = MESH_MARGIN * (chords.get(i).copied().unwrap_or(0.0) + chords.get(j).copied().unwrap_or(0.0));
        Ok(!meshes_apart(ma, sa, mb, sb, margin))
    });
    let mut jobs = Vec::new();
    let mut cleared = 0usize;
    for (k, (&(i, j), exact)) in pairs.iter().zip(need_exact).enumerate() {
        if !exact.unwrap_or(true) {
            cleared += 1;
            continue;
        }
        let (Some(a), Some(b)) = (bodies.get(i), bodies.get(j)) else { continue };
        jobs.push(Job {
            pair: k,
            a: a.body.clone(),
            b: b.body.clone(),
            names: (a.name.clone(), b.name.clone()),
            cost: a.body.face_count().saturating_mul(b.body.face_count()),
        });
    }
    let intersected = jobs.len();
    jobs.sort_by(|x, y| y.cost.cmp(&x.cost).then(x.pair.cmp(&y.pair)));

    let left = budget.saturating_sub(started.elapsed());
    let waited = web_time::Instant::now();
    let (done, threads) = run_jobs(jobs, left);
    let waited_ms = waited.elapsed().as_secs_f64() * 1000.0;

    // Mesh bodies are not intersected: say so, rather than let `count: 0` read as "no overlap".
    let mut unchecked: Vec<Value> = meshes_left.iter().map(|b| json!({"a": b.name, "b": null, "reason": MESH_NOT_CHECKED})).collect();
    let (mut hits, mut timed) = (Vec::new(), Vec::new());
    let (mut not_checked, mut boolean_ms, mut tessellate_ms) = (0usize, 0.0, 0.0);
    for ((a, b), d) in done {
        let Some(d) = d else {
            not_checked += 1;
            unchecked.push(json!({"a": a, "b": b, "reason": format!("not checked: the time budget of {budget_s} s ran out (budget_s)")}));
            continue;
        };
        boolean_ms += d.kernel.boolean_ms;
        tessellate_ms += d.kernel.tessellate_ms;
        timed.push((
            d.ms,
            json!({"a": a, "b": b, "ms": round_ms(d.ms), "boolean_ms": round_ms(d.kernel.boolean_ms),
            "tessellate_ms": round_ms(d.kernel.tessellate_ms)}),
        ));
        if let Some(reason) = d.note {
            unchecked.push(json!({"a": a, "b": b, "reason": reason}));
        }
        match d.outcome {
            Outcome::Hit(v) => hits.push(v),
            Outcome::Clear => {}
            Outcome::Unchecked(reason) => unchecked.push(json!({"a": a, "b": b, "reason": reason})),
        }
    }
    // A timing profile of the command (`--profile`) counts the kernel time the workers spent,
    // shared out over the time this thread waited for them (on the calling thread it is counted
    // already).
    if threads > 0 && boolean_ms + tessellate_ms > 0.0 {
        let share = (waited_ms / (boolean_ms + tessellate_ms)).min(1.0);
        solidcraft_kernel::profile::credit(KernelTimes { boolean_ms: boolean_ms * share, tessellate_ms: tessellate_ms * share });
    }
    timed.sort_by(|x, y| y.0.total_cmp(&x.0));
    let slowest: Vec<Value> = timed.into_iter().take(SLOWEST).map(|t| t.1).collect();
    Ok(json!({
        "interferences": hits, "count": hits.len(), "checked": bodies.len(), "unchecked": unchecked,
        "truncated": skipped > 0, "skipped_pairs": skipped,
        "pairs_candidate": pairs.len(), "pairs_cleared": cleared, "pairs_intersected": intersected,
        "pairs_not_checked": not_checked, "budget_s": budget_s, "budget_exceeded": not_checked > 0,
        "threads": threads, "boolean_ms": round_ms(boolean_ms), "tessellate_ms": round_ms(tessellate_ms),
        "slowest": slowest, "ms": round_ms(started.elapsed().as_secs_f64() * 1000.0),
    }))
}

fn round_ms(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

/// Are two bodies' meshes farther apart than `margin` with neither body inside the other? Then
/// their boundaries don't meet (each mesh is within its chord tolerance of its faces, which the
/// margin covers), so the bodies share no volume. Conservative: an empty mesh, nested boxes or
/// a point of one inside the other answer no.
fn meshes_apart(ma: &Mesh, sa: &TriangleSet, mb: &Mesh, sb: &TriangleSet, margin: f64) -> bool {
    if ma.triangles.is_empty() || mb.triangles.is_empty() || !margin.is_finite() {
        return false;
    }
    if sa.closest(sb, margin).is_some() {
        return false;
    }
    // Apart boundaries: either the bodies are apart, or one holds the other (or a piece of it).
    let (ba, bb) = (ma.bounds(), mb.bounds());
    if nested(&ba, &bb) || nested(&bb, &ba) {
        return false;
    }
    !(any_inside(&face_points(ma), mb) || any_inside(&face_points(mb), ma))
}

/// Is `inner` inside `outer`?
fn nested(inner: &Aabb3, outer: &Aabb3) -> bool {
    !inner.is_empty()
        && outer.min.x <= inner.min.x
        && outer.min.y <= inner.min.y
        && outer.min.z <= inner.min.z
        && inner.max.x <= outer.max.x
        && inner.max.y <= outer.max.y
        && inner.max.z <= outer.max.z
}

/// A corner of each face's first triangle (every piece of a body in several pieces has a face).
fn face_points(m: &Mesh) -> Vec<Vec3> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (k, t) in m.triangles.iter().enumerate() {
        let face = m.tri_face.get(k).copied().unwrap_or(0);
        if seen.insert(face)
            && let Some(p) = m.positions.get(t[0] as usize)
        {
            out.push(*p);
        }
    }
    out
}

/// Is any of `points` inside the closed mesh `outer`?
fn any_inside(points: &[Vec3], outer: &Mesh) -> bool {
    let b = outer.bounds();
    let within = |p: &&Vec3| b.min.x <= p.x && p.x <= b.max.x && b.min.y <= p.y && p.y <= b.max.y && b.min.z <= p.z && p.z <= b.max.z;
    let candidates: Vec<&Vec3> = points.iter().filter(within).collect();
    match candidates.len() {
        0 => false,
        1..=4 => candidates.iter().any(|p| outer.contains(**p)),
        _ => {
            let index = outer.inside_index();
            candidates.iter().any(|p| index.contains(**p))
        }
    }
}

/// The exact check of one pair: a pin a hair off the axis of its hole by the walls' crescent,
/// otherwise the intersection measured.
fn check(job: &Job) -> Done {
    let started = web_time::Instant::now();
    let ((outcome, note), kernel) = solidcraft_kernel::profile::capture(|| check_pair(&job.a, &job.b, &job.names));
    Done { outcome, note, ms: started.elapsed().as_secs_f64() * 1000.0, kernel }
}

fn check_pair(a: &Body, b: &Body, (na, nb): &(String, String)) -> (Outcome, Option<String>) {
    let threshold = 1e-9 * a.size().max(1.0).powi(3);
    // A pin a hair off the axis of its hole: the crescent the walls share, which the boolean
    // can't intersect.
    if let Some(o) = solidcraft_kernel::coaxial_overlap(a, b) {
        if o.volume > threshold {
            let hit = json!({"a": na, "b": nb, "volume_mm3": o.volume, "area_mm2": o.area, "center": o.centroid,
                "bbox": {"min": o.bbox.min, "max": o.bbox.max}, "near_coaxial": {"offset_mm": o.offset, "length_mm": o.length}});
            return (Outcome::Hit(hit), None);
        }
        return (Outcome::Clear, None);
    }
    // A pair whose shared part can't be worked out is reported, not passed as clear.
    let (shared, report) = solidcraft_kernel::capture_boolean_diagnostics(|| solidcraft_kernel::boolean(a, b, solidcraft_kernel::BoolOp::Intersect));
    let shared = match shared {
        Ok(o) => o,
        Err(e) => return (Outcome::Unchecked(e.to_string()), None),
    };
    let mut note = report.unverified();
    let pieces = match shared {
        Some(o) => vec![o],
        // A body in several pieces, one of them wholly inside the other body: the whole bodies'
        // boolean can come out empty, so the pieces are intersected one by one.
        None if a.shell_count() > 1 || b.shell_count() > 1 => match by_pieces(a, b) {
            Ok((pieces, unverified)) => {
                note = note.or(unverified);
                pieces
            }
            Err(e) => return (Outcome::Unchecked(e), note),
        },
        None => return (Outcome::Clear, note),
    };
    // The shared parts measured together: volumes and areas add, the centre is the volumes'.
    let mut measured = Vec::new();
    for o in &pieces {
        match solidcraft_kernel::measure_mass(o) {
            Ok(m) => measured.push(m),
            Err(e) => return (Outcome::Unchecked(e.to_string()), note),
        }
    }
    let volume: f64 = measured.iter().map(|m| m.volume).sum();
    if volume > threshold {
        let area: f64 = measured.iter().map(|m| m.area).sum();
        let bbox = measured.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.bbox));
        let center = match measured.as_slice() {
            [m] => m.centroid,
            _ => measured.iter().fold(Vec3::ZERO, |c, m| c + m.centroid * m.volume) / volume,
        };
        let hit = json!({"a": na, "b": nb, "volume_mm3": volume, "area_mm2": area, "center": center, "bbox": {"min": bbox.min, "max": bbox.max}});
        return (Outcome::Hit(hit), note);
    }
    (Outcome::Clear, note)
}

/// Most pairs of pieces [`by_pieces`] intersects.
const MAX_PIECE_PAIRS: usize = 256;

/// The shared parts of two bodies, a piece of each at a time, and why any of them is unverified.
fn by_pieces(a: &Body, b: &Body) -> std::result::Result<(Vec<Body>, Option<String>), String> {
    let (la, lb) = (a.lumps().map_err(|e| e.to_string())?, b.lumps().map_err(|e| e.to_string())?);
    if la.len() < 2 && lb.len() < 2 {
        return Ok((Vec::new(), None));
    }
    if la.len().saturating_mul(lb.len()) > MAX_PIECE_PAIRS {
        return Err(format!("not checked: {} by {} pieces are more than {MAX_PIECE_PAIRS} pairs of pieces", la.len(), lb.len()));
    }
    let (mut out, mut note) = (Vec::new(), None);
    for x in &la {
        for y in &lb {
            let (r, report) =
                solidcraft_kernel::capture_boolean_diagnostics(|| solidcraft_kernel::boolean(x, y, solidcraft_kernel::BoolOp::Intersect));
            note = note.or(report.unverified());
            if let Some(o) = r.map_err(|e| e.to_string())? {
                out.push(o);
            }
        }
    }
    Ok((out, note))
}

/// Each job's answer (in pair order, with the pair's names; `None`: not finished within
/// `budget`) and how many worker threads ran them (0: this thread did).
fn run_jobs(jobs: Vec<Job>, budget: std::time::Duration) -> (Vec<((String, String), Option<Done>)>, usize) {
    let mut order: Vec<(usize, usize, (String, String))> = jobs.iter().enumerate().map(|(k, j)| (j.pair, k, j.names.clone())).collect();
    order.sort_unstable_by_key(|o| o.0);
    let (mut results, threads) = run_ordered(jobs, budget);
    let out = order.into_iter().map(|(_, k, names)| (names, results.get_mut(k).and_then(Option::take))).collect();
    (out, threads)
}

/// Each job's answer in job order, and the threads used. On desktop the jobs run on worker
/// threads of their own, which the call does not wait for once `budget` is spent: a pair still
/// running then finishes in the background and is dropped (a kernel boolean can't be stopped
/// part way).
#[cfg(not(target_arch = "wasm32"))]
fn run_ordered(jobs: Vec<Job>, budget: std::time::Duration) -> (Vec<Option<Done>>, usize) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;

    let n = jobs.len();
    let mut out: Vec<Option<Done>> = std::iter::repeat_with(|| None).take(n).collect();
    if n == 0 || budget.is_zero() {
        return (out, 0);
    }
    let deadline = web_time::Instant::now() + budget;
    // The workers outlive the call when the budget runs out, so they own what they work on.
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<(usize, Done)>();
    let want = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(n).max(1);
    let mut threads = 0;
    for _ in 0..want {
        let (jobs, next, stop, tx) = (jobs.clone(), next.clone(), stop.clone(), tx.clone());
        let spawned = std::thread::Builder::new().name("solidcraft-interference".into()).spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(k) else { break };
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(job))).unwrap_or_else(|_| Done {
                    outcome: Outcome::Unchecked("internal error while checking this pair".into()),
                    note: None,
                    ms: 0.0,
                    kernel: KernelTimes::default(),
                });
                if tx.send((k, done)).is_err() {
                    break;
                }
            }
        });
        if spawned.is_ok() {
            threads += 1;
        }
    }
    drop(tx);
    if threads == 0 {
        // No thread could be started: one pair after another here.
        return (run_here(&jobs, deadline), 0);
    }
    let mut received = 0;
    while received < n {
        let left = deadline.saturating_duration_since(web_time::Instant::now());
        match rx.recv_timeout(left) {
            Ok((k, done)) => {
                if let Some(slot) = out.get_mut(k) {
                    *slot = Some(done);
                }
                received += 1;
            }
            // Out of time, or every worker gone (none is left with work then).
            Err(_) => break,
        }
    }
    stop.store(true, Ordering::Relaxed);
    (out, threads)
}

/// The browser has no threads: one pair after another, none started once the budget is spent.
#[cfg(target_arch = "wasm32")]
fn run_ordered(jobs: Vec<Job>, budget: std::time::Duration) -> (Vec<Option<Done>>, usize) {
    (run_here(&jobs, web_time::Instant::now() + budget), 0)
}

/// The jobs one after another on this thread, none started past `deadline`.
fn run_here(jobs: &[Job], deadline: web_time::Instant) -> Vec<Option<Done>> {
    jobs.iter().map(|job| (web_time::Instant::now() < deadline).then(|| check(job))).collect()
}

#[cfg(test)]
#[path = "interference_tests.rs"]
mod tests;
