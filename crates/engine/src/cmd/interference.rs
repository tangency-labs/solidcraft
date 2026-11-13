//! Interference (`inspect.interference`): the pairs of bodies that share volume, and how much.
//!
//! Pairs are pruned before anything exact runs. Their exact boxes first (sort and sweep), then
//! their exact contact: bodies whose boundaries stay apart and neither encloses the other are
//! clear without a boolean. The pairs left are intersected exactly and measured, in parallel on
//! desktop (one after another in the browser), the slowest first, under a time budget: pairs not
//! finished when it runs out are listed as not checked rather than holding the call. The shared
//! part is measured as `solidcraft_kernel::measure` measures it (its mass properties alone).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use solidcraft_geom::Aabb3;
use solidcraft_kernel::Body;
use solidcraft_kernel::profile::KernelTimes;

use crate::params::{bad, num, string_list};
use crate::schema::clearance::{Overlap, OverlapBounds, Unchecked};
use crate::schema::interference::{InterferenceResult, PairTime, PlacedOverlap, PlacedUnchecked};
use crate::{Result, Session};

/// Most pairs with overlapping bounding boxes one call intersects.
pub(crate) const MAX_INTERFERENCE_PAIRS: usize = 20_000;
/// The time budget when the call gives none (s): under the 300 s an MCP client waits for a tool.
pub(crate) const DEFAULT_BUDGET_S: f64 = 240.0;
/// The longest time budget a call may ask for (s).
const MAX_BUDGET_S: f64 = 86_400.0;
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
    Hit(Overlap),
    Clear,
    Unchecked(String),
}

/// One pair's answer and where its time went.
struct Done {
    outcome: Outcome,
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
    let st = s.world_state()?;
    // A name that is no body is refused, as inspect.clearance refuses it. Dropped, a misspelled
    // body was never checked and the answer read as no interference (#9740).
    if let Some(n) = want.iter().find(|n| st.body(n).is_none()) {
        return Err(bad(cmd, super::no_body(n, st.bodies.iter().map(|b| b.name.as_str()))));
    }
    let (meshes_left, bodies): (Vec<&solidcraft_doc::ModelBody>, Vec<&solidcraft_doc::ModelBody>) =
        st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).partition(|b| b.body.is_mesh());

    // Prune by bounding boxes first (sort and sweep), so the cost follows the pairs that can
    // touch rather than the square of the body count.
    let boxes: Vec<Aabb3> =
        bodies.iter().map(|b| b.body.exact_bounds().map(|bb| bb.inflated(2.0 * b.body.record().tolerance)).unwrap_or(Aabb3::EMPTY)).collect();
    let (pairs, skipped) = super::joints::overlapping_pairs(&boxes, MAX_INTERFERENCE_PAIRS);
    // Where each body sits (#5963): its occurrence path, for `exclude_pairs` and the reply.
    let paths: HashMap<String, Vec<u64>> = s.doc.body_frames(&s.model.state()).into_iter().map(|(name, _, path, _)| (name, path)).collect();
    let path_of = |name: &str| paths.get(name).map_or(&[][..], Vec::as_slice);
    let excluded = excluded_pairs(s, p, &st, cmd)?;
    let matches = |side: &Side, name: &str| match side {
        Side::Body(n) => n == name,
        Side::Occurrence(o) => path_of(name).contains(o),
    };
    let left_out = |i: usize, j: usize| {
        let (Some(a), Some(b)) = (bodies.get(i), bodies.get(j)) else { return false };
        excluded.iter().any(|(x, y)| (matches(x, &a.name) && matches(y, &b.name)) || (matches(x, &b.name) && matches(y, &a.name)))
    };
    let before = pairs.len();
    let pairs: Vec<(usize, usize)> = pairs.into_iter().filter(|&(i, j)| !left_out(i, j)).collect();
    let pairs_excluded = before - pairs.len();
    let place = |name: &str| super::motion::part_place(&s.doc, path_of(name));
    let placed = |u: Unchecked| PlacedUnchecked { a_part: place(&u.a), b_part: u.b.as_deref().map(place), unchecked: u };

    // Then prune by contact: pairs whose boundaries stay apart and neither encloses the other
    // are clear without a boolean.
    let need_exact = solidcraft_kernel::map_with_local_pools(&pairs, usize::MAX, |&(i, j)| {
        let (Some(a), Some(b)) = (bodies.get(i), bodies.get(j)) else {
            return Ok(true);
        };
        if solidcraft_kernel::apart(&a.body, &b.body).unwrap_or(false) {
            return Ok(false);
        }
        a.body.meets(&b.body)
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
    let mut unchecked: Vec<PlacedUnchecked> =
        meshes_left.iter().map(|b| placed(Unchecked { a: b.name.clone(), b: None, reason: MESH_NOT_CHECKED.to_string() })).collect();
    let (mut hits, mut timed) = (Vec::new(), Vec::new());
    let (mut not_checked, mut boolean_ms, mut tessellate_ms) = (0usize, 0.0, 0.0);
    for ((a, b), d) in done {
        let Some(d) = d else {
            not_checked += 1;
            unchecked.push(placed(Unchecked { a, b: Some(b), reason: format!("not checked: the time budget of {budget_s} s ran out (budget_s)") }));
            continue;
        };
        boolean_ms += d.kernel.boolean_ms;
        tessellate_ms += d.kernel.tessellate_ms;
        let time = PairTime {
            a: a.clone(),
            b: b.clone(),
            ms: round_ms(d.ms),
            boolean_ms: round_ms(d.kernel.boolean_ms),
            tessellate_ms: round_ms(d.kernel.tessellate_ms),
        };
        timed.push((d.ms, time));
        match d.outcome {
            Outcome::Hit(overlap) => hits.push(PlacedOverlap { a_part: place(&overlap.a), b_part: place(&overlap.b), overlap }),
            Outcome::Clear => {}
            Outcome::Unchecked(reason) => unchecked.push(placed(Unchecked { a, b: Some(b), reason })),
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
    let slowest: Vec<PairTime> = timed.into_iter().take(SLOWEST).map(|t| t.1).collect();
    let reply = InterferenceResult {
        count: hits.len(),
        interferences: hits,
        checked: bodies.len(),
        unchecked,
        truncated: skipped > 0,
        skipped_pairs: skipped,
        pairs_candidate: pairs.len(),
        pairs_cleared: cleared,
        pairs_intersected: intersected,
        pairs_not_checked: not_checked,
        pairs_excluded,
        budget_s,
        budget_exceeded: not_checked > 0,
        threads,
        boolean_ms: round_ms(boolean_ms),
        tessellate_ms: round_ms(tessellate_ms),
        slowest,
        ms: round_ms(started.elapsed().as_secs_f64() * 1000.0),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn round_ms(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

/// The exact check of one pair: the bodies' intersection, measured.
fn check(job: &Job) -> Done {
    let started = web_time::Instant::now();
    let (outcome, kernel) = solidcraft_kernel::profile::capture(|| check_pair(&job.a, &job.b, &job.names));
    Done { outcome, ms: started.elapsed().as_secs_f64() * 1000.0, kernel }
}

fn check_pair(a: &Body, b: &Body, (na, nb): &(String, String)) -> Outcome {
    let threshold = 1e-9 * a.size().max(1.0).powi(3);
    // A pair whose shared part can't be worked out is reported, not passed as clear.
    let shared = match solidcraft_kernel::boolean(a, b, solidcraft_kernel::BoolOp::Intersect) {
        Ok(Some(o)) => o,
        Ok(None) => return Outcome::Clear,
        Err(e) => return Outcome::Unchecked(e.to_string()),
    };
    let m = match solidcraft_kernel::measure_mass(&shared) {
        Ok(m) => m,
        Err(e) => return Outcome::Unchecked(e.to_string()),
    };
    if m.volume > threshold {
        return Outcome::Hit(Overlap {
            a: na.clone(),
            b: nb.clone(),
            volume_mm3: m.volume,
            area_mm2: m.area,
            bbox: OverlapBounds { min: m.bbox.min.into(), max: m.bbox.max.into() },
            center: m.centroid.into(),
        });
    }
    Outcome::Clear
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
        let spawned = solidcraft_kernel::kernel_thread("solidcraft-interference").spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(k) else { break };
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(job))).unwrap_or_else(|_| Done {
                    outcome: Outcome::Unchecked("internal error while checking this pair".into()),
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

/// One side of an `exclude_pairs` pair: a body by its world name, or every body under an
/// occurrence.
enum Side {
    Body(String),
    Occurrence(u64),
}

/// Most pairs `exclude_pairs` may list.
const MAX_EXCLUDED: usize = 10_000;

/// `exclude_pairs`: `[[a, b]…]`, each a body's world name or an occurrence's id or name (#5963).
fn excluded_pairs(s: &Session, p: &Value, st: &solidcraft_doc::ModelState, cmd: &str) -> Result<Vec<(Side, Side)>> {
    let list = match p.get("exclude_pairs") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) if list.len() <= MAX_EXCLUDED => list,
        Some(Value::Array(_)) => return Err(bad(cmd, format!("`exclude_pairs` lists more than {MAX_EXCLUDED} pairs"))),
        Some(_) => return Err(bad(cmd, "`exclude_pairs` must list pairs: [[a, b]…]")),
    };
    let side = |v: &Value| -> Result<Side> {
        if let Some(n) = v.as_str()
            && st.body(n).is_some()
        {
            return Ok(Side::Body(n.to_string()));
        }
        match super::joints::occurrence_id(s, Some(v), cmd) {
            Ok(0) => Err(bad(cmd, "the root is not an occurrence")),
            Ok(id) => Ok(Side::Occurrence(id)),
            Err(_) => Err(bad(cmd, format!("`exclude_pairs`: {v} is neither a body nor an occurrence"))),
        }
    };
    list.iter()
        .map(|pair| match pair.as_array().map(Vec::as_slice) {
            Some([a, b]) => Ok((side(a)?, side(b)?)),
            _ => Err(bad(cmd, "each of `exclude_pairs` is a pair: [a, b]")),
        })
        .collect()
}
