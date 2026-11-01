//! Motion analysis: a study played sample by sample on a copy of the design, the way
//! `motion.play` places it (closed loops following on their branch). `motion.plot` answers the
//! joint values and a point's path with their velocities and accelerations; `motion.interference`
//! answers the pairs of parts that pass through each other along the way.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::joints;
use solidcraft_doc::motion::MotionStudy;
use solidcraft_doc::{Document, Mat, apply_point, mat_mul};
use solidcraft_geom::{Aabb3, Vec3};

use super::contact_sweep::{ContactGeometry, Member};
use super::{drive_with_contact, joint_id, keyed, occurrences, path_world, study_index, traced_point, values_at};
use crate::params::{bad, bool_, num};
use crate::{EngineError, Result, Session};

pub const PLOT_DOC: &str = "study: id|name; joints?: [id|name] (default: every joint that moves); occurrence? and point? (a point \
     of a part to follow, as motion.trace takes them); samples? (3…1000, default the study's steps + 1); duration?: seconds the \
     whole study takes, 0.001 or more (velocities and accelerations then per second, else per step) → step: [the study step of each sample], \
     time?: [s], joints: [{joint, name, index, dof, unit, value, velocity, acceleration, velocity_unit, acceleration_unit}] \
     (degrees for angles, mm for distances; angles unwrapped), point?: {occurrence, position: [[x,y,z]…], velocity, \
     acceleration (vectors), speed, acceleration_norm, unit, velocity_unit, acceleration_unit}, samples (taken), stopped_at, \
     stopped_by_loop, stopped_by_contact. The study plays as motion.play plays it, each sample driven from the one before, on \
     a copy (the design is not changed); velocities and accelerations are finite differences between samples (central, \
     one-sided at the ends)";

pub const INTERFERENCE_DOC: &str = "study: id|name; occurrences?: [ids or names] (two or more, checked against each other; \
     default every occurrence), with against?: [ids or names] (then each of `occurrences` is checked only against these, such \
     as an arm against the parts it must miss), or contact_sets?: true (the pairs of the design's contact sets); samples? \
     (2…1000, default the study's steps + 1); budget_s?: seconds (above 0, at most 86400; default a fixed work budget): the \
     time the check may take instead, looked at between samples, for a mechanism such as a gear train that needs longer → collisions: [{a, b, first_step, last_step, steps: [[from, to]…] (the step ranges where they overlap), \
     volume_mm3 (the most volume they share at a sample, worked out exactly from the bodies), deepest_step (the first sample \
     sharing that volume), not_measured (null, or why the volume was not worked out at every sample they overlap, such as \
     a budget it ran past: then volume_mm3 and deepest_step are null, never an estimate), depth_mm (how far into each \
     other at most: an estimate from points of the bodies' contact meshes, not exact)}], clear (every sample checked and none collide; never while truncated), \
     pairs (checked), samples (taken), truncated (the work budget, or budget_s, ran out; checked_to: the last step checked), budget_s \
     (when given), stopped_at, \
     stopped_by_loop. The study plays as motion.play plays it but contact sets do not stop it, on a copy (the design is not \
     changed). Parts are tested at the samples only: one that passes wholly through another between two samples is missed";

/// Most samples a plot or an interference check takes: each is a solve.
const MAX_SAMPLES: usize = 1000;
/// Most joint values one plot answers.
const MAX_SERIES: usize = 256;
/// Most collision work over a whole study (see `ContactGeometry::hits`).
const MAX_WORK: usize = 500_000_000;
/// Longest time an interference check may be given (`budget_s`): a day, as `inspect.interference`.
const MAX_BUDGET_S: f64 = 86_400.0;
/// Most colliding pairs whose overlap volume is worked out, and most body booleans that takes
/// over the whole study (one per pair of overlapping bodies at each sample a pair collides).
const MAX_VOLUMES: usize = 16;
const MAX_BOOLEANS: usize = 256;

/// How many samples `p` asks for (`min`…`MAX_SAMPLES`, default the study's steps + 1).
fn samples(p: &Value, m: &MotionStudy, min: usize, cmd: &str) -> Result<usize> {
    if m.steps == 0 {
        return Err(bad(cmd, "the study has no steps"));
    }
    match p.get("samples") {
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite() && *n >= min as f64 && *n <= MAX_SAMPLES as f64)
            .map(|n| n.round() as usize)
            .ok_or_else(|| bad(cmd, format!("`samples` must be {min}…{MAX_SAMPLES}"))),
        None => Ok((m.steps as usize).saturating_add(1).clamp(min, MAX_SAMPLES)),
    }
}

/// The study step sample `k` of `n` is at.
fn step_of(m: &MotionStudy, k: usize, n: usize) -> f64 {
    f64::from(m.steps) * k as f64 / n.saturating_sub(1).max(1) as f64
}

/// Plays study `m` on a copy of the design in `n` samples, each driven from the one before (so
/// closed loops follow on their branch, as `motion.play` places them; contact sets stop it only
/// when `contact`). `each` sees the copy at every sample and answers whether to go on. Returns
/// where the motion stopped short (all null when it did not).
fn play(s: &Session, m: &MotionStudy, n: usize, contact: bool, mut each: impl FnMut(&Session, usize) -> Result<bool>) -> Result<Value> {
    let mut t = s.scratch();
    // Played from where the parts are placed: moves only shown are not where joints are solved.
    t.pending_moves.clear();
    if !contact {
        t.doc_mut().assembly.contact_enabled = false;
    }
    let given = keyed(m);
    for k in 0..n {
        let want = values_at(&t, m, step_of(m, k, n));
        let (r, _) = drive_with_contact(&mut t, &want, &given)?;
        let go = each(&t, k)?;
        if r["stopped_by_loop"].is_object() || r["stopped_by_contact"].is_object() {
            return Ok(json!({"stopped_at": k, "stopped_by_loop": r["stopped_by_loop"], "stopped_by_contact": r["stopped_by_contact"]}));
        }
        if !go {
            break;
        }
    }
    Ok(json!({"stopped_at": null, "stopped_by_loop": null, "stopped_by_contact": null}))
}

/// The derivative of samples `v` spaced `h` apart: central differences inside, second-order
/// one-sided at the ends (a plain difference with two samples, 0 with one).
fn derivative(v: &[f64], h: f64) -> Vec<f64> {
    let n = v.len();
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
    (0..n)
        .map(|k| match n {
            0 | 1 => 0.0,
            2 => (at(1) - at(0)) / h,
            _ if k == 0 => (-3.0 * at(0) + 4.0 * at(1) - at(2)) / (2.0 * h),
            _ if k + 1 == n => (3.0 * at(k) - 4.0 * at(k - 1) + at(k - 2)) / (2.0 * h),
            _ => (at(k + 1) - at(k - 1)) / (2.0 * h),
        })
        .collect()
}

/// The second derivative of samples `v` spaced `h` apart: central differences inside,
/// second-order one-sided at the ends (the one difference there is with three samples, 0 with
/// fewer).
fn second_derivative(v: &[f64], h: f64) -> Vec<f64> {
    let n = v.len();
    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
    let h2 = h * h;
    (0..n)
        .map(|k| match n {
            0..=2 => 0.0,
            3 => (at(0) - 2.0 * at(1) + at(2)) / h2,
            _ if k == 0 => (2.0 * at(0) - 5.0 * at(1) + 4.0 * at(2) - at(3)) / h2,
            _ if k + 1 == n => (2.0 * at(k) - 5.0 * at(k - 1) + 4.0 * at(k - 2) - at(k - 3)) / h2,
            _ => (at(k + 1) - 2.0 * at(k) + at(k - 1)) / h2,
        })
        .collect()
}

/// Angles in degrees made continuous: a jump of more than half a turn between samples is taken
/// as the angle going round (half a turn exactly is a real step).
fn unwrap_degrees(v: &mut [f64]) {
    let (mut turns, mut prev) = (0.0, None::<f64>);
    for x in v.iter_mut() {
        let raw = *x;
        if let Some(p) = prev
            && (raw - p).abs() > 180.0
        {
            turns -= 360.0 * ((raw - p) / 360.0).round();
        }
        prev = Some(raw);
        *x = raw + turns;
    }
}

/// The joint values a plot follows: (joint id, value index).
fn plotted(s: &Session, p: &Value, cmd: &str) -> Result<Vec<(u64, usize)>> {
    let joints = &s.doc.assembly.joints;
    let ids: Vec<u64> = match p.get("joints") {
        None | Some(Value::Null) => joints.iter().filter(|j| !j.suppressed).map(|j| j.id).collect(),
        Some(Value::Array(list)) if list.len() <= MAX_SERIES => list.iter().map(|v| joint_id(s, Some(v), cmd)).collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, format!("`joints` must list at most {MAX_SERIES} joints"))),
    };
    let mut out = Vec::new();
    for id in ids {
        let dofs = joints.iter().find(|j| j.id == id).map_or(0, |j| j.kind.dofs().len());
        for i in 0..dofs {
            if !out.contains(&(id, i)) {
                out.push((id, i));
            }
        }
    }
    if out.len() > MAX_SERIES {
        return Err(bad(cmd, format!("more than {MAX_SERIES} joint values: list the `joints` to plot")));
    }
    Ok(out)
}

pub(super) fn plot(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.plot";
    let m = s.doc.assembly.studies.get(study_index(s, p, cmd)?).cloned().ok_or_else(|| bad(cmd, "study"))?;
    let n = samples(p, &m, 3, cmd)?;
    let duration = match p.get("duration") {
        None | Some(Value::Null) => None,
        Some(v) => {
            // At least a millisecond: shorter sample spacings square to nothing.
            Some(v.as_f64().filter(|d| (1e-3..=1e9).contains(d)).ok_or_else(|| bad(cmd, "`duration` must be seconds, 0.001 or more"))?)
        }
    };
    let series = plotted(s, p, cmd)?;
    let given = keyed(&m);
    let point = if p.get("point").is_some() || p.get("occurrence").is_some() { Some(traced_point(s, p, cmd)?) } else { None };
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); series.len()];
    let mut path: Vec<Vec3> = Vec::new();
    let mut taken = 0;
    let stop = play(s, &m, n, true, |t, _| {
        taken += 1;
        let now = joints::joint_values(&t.doc.assembly);
        for ((id, i), out) in series.iter().zip(values.iter_mut()) {
            out.push(now.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0));
        }
        if let Some((occ, local)) = &point {
            path.push(apply_point(&path_world(t, occ, cmd)?, *local));
        }
        Ok(true)
    })?;
    // Sample spacing in steps, or in seconds when the study has a duration.
    let seconds_per_step = duration.map(|d| d / f64::from(m.steps));
    let h = f64::from(m.steps) / (n - 1) as f64 * seconds_per_step.unwrap_or(1.0);
    let per = if duration.is_some() { "s" } else { "step" };
    let steps: Vec<f64> = (0..taken).map(|k| step_of(&m, k, n)).collect();
    let mut out = json!({"study": m.name, "steps": m.steps, "samples": taken, "step": steps});
    if let Some(sps) = seconds_per_step {
        out["time"] = steps.iter().map(|st| st * sps).collect();
    }
    let joints_out: Vec<Value> = series
        .iter()
        .zip(values.iter_mut())
        .map(|((id, i), v)| {
            let j = s.doc.assembly.joints.iter().find(|j| j.id == *id);
            let angle = j.is_some_and(|j| j.kind.is_angle(*i));
            let unit = if angle { "deg" } else { "mm" };
            if angle {
                v.iter_mut().for_each(|x| *x = x.to_degrees());
                // Keyed values are as the study sets them, and values beyond half a turn
                // (through motion links) already run on; a loop can hand back a follower's
                // angle within half a turn, a turn round.
                if !given.contains(&(*id, *i)) && v.iter().all(|x| x.abs() <= 180.0 + 1e-9) {
                    unwrap_degrees(v);
                }
            }
            json!({"joint": id, "name": j.map(|j| j.name.clone()), "index": i, "dof": j.and_then(|j| j.kind.dofs().get(*i).copied()),
                "unit": unit, "velocity_unit": format!("{unit}/{per}"), "acceleration_unit": format!("{unit}/{per}²"),
                "value": v, "velocity": derivative(v, h), "acceleration": second_derivative(v, h)})
        })
        .collect();
    out["joints"] = json!(joints_out);
    if let Some((occ, _)) = &point {
        let axis = |c: usize| -> Vec<f64> { path.iter().map(|q| [q.x, q.y, q.z].get(c).copied().unwrap_or(0.0)).collect() };
        let d: Vec<(Vec<f64>, Vec<f64>)> = (0..3).map(|c| (derivative(&axis(c), h), second_derivative(&axis(c), h))).collect();
        let vec_at = |k: usize, second: bool| -> Vec3 {
            let c = |i: usize| d.get(i).and_then(|(v, a)| if second { a.get(k) } else { v.get(k) }).copied().unwrap_or(0.0);
            Vec3::new(c(0), c(1), c(2))
        };
        let (vel, acc): (Vec<Vec3>, Vec<Vec3>) = (0..path.len()).map(|k| (vec_at(k, false), vec_at(k, true))).unzip();
        let xyz = |list: &[Vec3]| -> Value { list.iter().map(|v| json!([v.x, v.y, v.z])).collect() };
        let name = s.doc.occurrences.iter().find(|o| Some(&o.id) == occ.last()).map_or_else(|| "root".to_string(), |o| o.name.clone());
        out["point"] = json!({"occurrence": name, "unit": "mm", "velocity_unit": format!("mm/{per}"), "acceleration_unit": format!("mm/{per}²"),
            "position": xyz(&path), "velocity": xyz(&vel), "acceleration": xyz(&acc),
            "speed": vel.iter().map(|v| v.len()).collect::<Vec<_>>(), "acceleration_norm": acc.iter().map(|v| v.len()).collect::<Vec<_>>()});
    }
    for k in ["stopped_at", "stopped_by_loop", "stopped_by_contact"] {
        out[k] = stop[k].clone();
    }
    Ok(out)
}

/// One pair of parts that collides along a study.
struct Collision {
    names: (String, String),
    /// The samples where they overlap, in order.
    samples: Vec<usize>,
    /// The most sampled mesh depth.
    depth: f64,
    /// The most exact shared volume and the first sample sharing it, or why it is not measured
    /// (a sample's volume was not worked out).
    most_volume: std::result::Result<(f64, usize), String>,
}

impl Collision {
    /// Takes the exact shared volume at sample `k`, or why it was not worked out. A volume
    /// larger by no more than the booleans' rounding is a tie, which keeps the earlier sample.
    fn take_volume(&mut self, k: usize, volume: std::result::Result<f64, String>) {
        if let Ok((best, at)) = self.most_volume {
            self.most_volume = volume.map(|v| if v > best + 1e-9 * best.max(1.0) { (v, k) } else { (best, at) });
        }
    }
}

/// The volume the parts under occurrences `a` and `b` share, placed as in `doc`, or why it was
/// not worked out. Only bodies whose boxes overlap are intersected, at most `booleans` of them.
fn shared_volume(s: &Session, doc: &Document, a: u64, b: u64, booleans: &mut usize) -> std::result::Result<f64, String> {
    let st = s.model.state();
    // The bodies under each (and not under the other), with their world frames and boxes.
    let mut sides: [Vec<(Mat, Aabb3, &solidcraft_doc::ModelBody)>; 2] = Default::default();
    for (comp, path, m) in doc.placements() {
        // A body belongs to the innermost of the two on its path, as contact sets pair them.
        let side = match path.iter().rev().find(|o| **o == a || **o == b) {
            Some(o) if *o == a => 0,
            Some(_) => 1,
            None => continue,
        };
        for body in st.bodies.iter().filter(|x| doc.model_body_component(x) == comp && !x.body.is_mesh()) {
            let frame = doc.model_body_offset(body).map(|o| mat_mul(&m, o)).unwrap_or(m);
            let local = body.mesh().bounds();
            let mut world = Aabb3::EMPTY;
            for c in 0..8 {
                let pick = |bit: usize, lo: f64, hi: f64| if c & bit == 0 { lo } else { hi };
                let corner = Vec3::new(pick(1, local.min.x, local.max.x), pick(2, local.min.y, local.max.y), pick(4, local.min.z, local.max.z));
                world.add(apply_point(&frame, corner));
            }
            if let Some(list) = sides.get_mut(side) {
                list.push((frame, world, body));
            }
        }
    }
    let [first, second] = &sides;
    let mut total = 0.0;
    for (fa, xa, ba) in first {
        for (fb, xb, bb) in second {
            if xa.max.x < xb.min.x || xb.max.x < xa.min.x || xa.max.y < xb.min.y || xb.max.y < xa.min.y || xa.max.z < xb.min.z || xb.max.z < xa.min.z
            {
                continue;
            }
            *booleans = booleans.checked_sub(1).ok_or_else(|| format!("the check's {MAX_BOOLEANS} body intersections ran out"))?;
            let failed = |e: &dyn std::fmt::Display| format!("the intersection of {} and {} could not be made ({e})", ba.name, bb.name);
            let (pa, pb) = (
                solidcraft_kernel::transform_matrix(&ba.body, *fa).map_err(|e| failed(&e))?,
                solidcraft_kernel::transform_matrix(&bb.body, *fb).map_err(|e| failed(&e))?,
            );
            if let Some(shared) = solidcraft_kernel::boolean(&pa, &pb, solidcraft_kernel::BoolOp::Intersect).map_err(|e| failed(&e))? {
                total += solidcraft_kernel::measure(&shared).map_err(|e| failed(&e))?.volume;
            }
        }
    }
    Ok(total)
}

/// Contiguous runs of samples, as study step ranges.
fn ranges(m: &MotionStudy, samples: &[usize], n: usize) -> Vec<[f64; 2]> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &k in samples {
        match out.last_mut() {
            Some(run) if run.1 + 1 == k => run.1 = k,
            _ => out.push((k, k)),
        }
    }
    out.into_iter().map(|(a, b)| [step_of(m, a, n), step_of(m, b, n)]).collect()
}

/// The pairs of parts a check looks at, as `motion.interference` and `joint.sweep` take them:
/// `occurrences` against each other (or each only `against` others), the design's contact sets
/// with `contact_sets`, else every occurrence.
pub(super) fn checked_pairs(s: &Session, p: &Value, cmd: &str) -> Result<ContactGeometry> {
    let everything = || vec![s.doc.occurrences.iter().map(|o| o.id).collect::<Vec<u64>>()];
    // Each of `occurrences` only against these.
    let against = match p.get("against") {
        None | Some(Value::Null) => None,
        Some(_) if p.get("occurrences").is_none() => return Err(bad(cmd, "`against` checks the `occurrences` against other parts: list them too")),
        Some(v) => Some(occurrences(s, &json!({"occurrences": v}), cmd)?),
    };
    let sets: Vec<Vec<u64>> = match (p.get("occurrences"), bool_(p, "contact_sets").unwrap_or(false)) {
        (Some(_), true) => return Err(bad(cmd, "give `occurrences` or `contact_sets`, not both")),
        (Some(_), false) => {
            let mut list = occurrences(s, p, cmd)?;
            if list.len() + against.as_ref().map_or(0, Vec::len) < 2 {
                return Err(bad(cmd, "list two or more occurrences to check against each other"));
            }
            for o in against.iter().flatten() {
                if !list.contains(o) {
                    list.push(*o);
                }
            }
            vec![list]
        }
        (None, true) => {
            let a = &s.doc.assembly;
            let sets: Vec<Vec<u64>> =
                if a.contact_all { everything() } else { a.contact_sets.iter().filter(|c| !c.suppressed).map(|c| c.occurrences.clone()).collect() };
            if sets.is_empty() {
                return Err(bad(cmd, "the design has no contact sets"));
            }
            sets
        }
        (None, false) => everything(),
    };
    let mut geometry = ContactGeometry::of_sets(s, &sets).map_err(|e| match e {
        EngineError::BadParams { msg, .. } => bad(cmd, format!("{msg} (check fewer parts: list the `occurrences`)")),
        e => e,
    })?;
    if let Some(b) = &against {
        let a = occurrences(s, p, cmd)?;
        geometry.keep_pairs(|x, y| (a.contains(&x) && b.contains(&y)) || (a.contains(&y) && b.contains(&x)));
    }
    Ok(geometry)
}

pub(super) fn interference(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.interference";
    let seconds = match p.get("budget_s") {
        None | Some(Value::Null) => None,
        Some(_) => match num(p, "budget_s") {
            Some(v) if v > 0.0 && v <= MAX_BUDGET_S => Some(v),
            _ => return Err(bad(cmd, format!("`budget_s` must be a number of seconds above 0, at most {MAX_BUDGET_S}"))),
        },
    };
    interference_within(s, p, MAX_WORK, seconds)
}

/// `motion.interference` with at most `budget` collision work (see `ContactGeometry::hits`), or,
/// given `seconds`, with that time instead: the work is then not counted against a budget, and
/// the time is looked at before each sample, so the last one may run past it (#7855).
fn interference_within(s: &mut Session, p: &Value, budget: usize, seconds: Option<f64>) -> Result<Value> {
    let cmd = "motion.interference";
    let deadline = seconds.and_then(|v| std::time::Duration::try_from_secs_f64(v).ok()).and_then(|d| web_time::Instant::now().checked_add(d));
    let budget = if deadline.is_some() { usize::MAX } else { budget };
    let m = s.doc.assembly.studies.get(study_index(s, p, cmd)?).cloned().ok_or_else(|| bad(cmd, "study"))?;
    let n = samples(p, &m, 2, cmd)?;
    let geometry = checked_pairs(s, p, cmd)?;
    let mut found: BTreeMap<(u64, u64), Collision> = BTreeMap::new();
    let (mut work, mut checked, mut truncated) = (0usize, 0usize, false);
    let mut booleans = MAX_BOOLEANS;
    let stop = play(s, &m, n, false, |t, k| {
        if deadline.is_some_and(|d| web_time::Instant::now() >= d) {
            truncated = true;
            return Ok(false);
        }
        // Placing the meshes is work too, also where nothing comes near.
        work = work.saturating_add(geometry.vertex_count());
        if work > budget {
            truncated = true;
            return Ok(false);
        }
        let hits: Vec<(Member, Member, f64)> = match geometry.hits(t, &mut work, budget, true) {
            Ok(h) => h,
            Err(_) if work > budget => {
                truncated = true;
                return Ok(false);
            }
            Err(e) => return Err(e),
        };
        checked = k + 1;
        for ((a, an), (b, bn), depth) in hits {
            // Each pair once, the older occurrence (the lower id) first.
            let ((a, an), (b, bn)) = if a <= b { ((a, an), (b, bn)) } else { ((b, bn), (a, an)) };
            // The first MAX_VOLUMES pairs found have their shared volume worked out.
            let slots = found.len() < MAX_VOLUMES;
            let c = found.entry((a, b)).or_insert_with(|| Collision {
                names: (an, bn),
                samples: Vec::new(),
                depth: 0.0,
                most_volume: if slots {
                    Ok((0.0, k))
                } else {
                    Err(format!("more than {MAX_VOLUMES} pairs collide: only the first {MAX_VOLUMES} found have their volumes worked out"))
                },
            });
            if c.samples.last() == Some(&k) {
                continue;
            }
            c.samples.push(k);
            c.depth = c.depth.max(depth);
            if c.most_volume.is_ok() {
                c.take_volume(k, shared_volume(s, &t.doc, a, b, &mut booleans));
            }
        }
        Ok(true)
    })?;
    // Nothing checked is not clear.
    if truncated && checked == 0 {
        return Err(bad(
            cmd,
            match seconds {
                Some(_) => "the check ran out of its budget_s before its first step: give it more time or check fewer parts (occurrences, against)",
                None => "the check ran out of its work budget before its first step: check fewer parts (occurrences, against) or give it a budget_s",
            },
        ));
    }
    let mut list: Vec<(&(u64, u64), &Collision)> = found.iter().collect();
    list.sort_by_key(|(_, c)| c.samples.first().copied().unwrap_or(usize::MAX));
    let collisions: Vec<Value> = list
        .iter()
        .map(|(_, c)| {
            let (volume, deepest, not_measured) = match &c.most_volume {
                Ok((v, at)) => (Some(*v), Some(step_of(&m, *at, n)), None),
                Err(why) => (None, None, Some(why.clone())),
            };
            let first = c.samples.first().map(|k| step_of(&m, *k, n));
            let last = c.samples.last().map(|k| step_of(&m, *k, n));
            json!({"a": c.names.0, "b": c.names.1, "first_step": first, "last_step": last, "steps": ranges(&m, &c.samples, n),
                "depth_mm": c.depth, "deepest_step": deepest, "volume_mm3": volume, "not_measured": not_measured})
        })
        .collect();
    // A check cut short says nothing about the steps it did not reach, so it is not clear (#7855).
    let clear = collisions.is_empty() && !truncated;
    let mut out = json!({"study": m.name, "steps": m.steps, "clear": clear, "collisions": collisions,
        "pairs": geometry.pair_count(), "samples": checked, "truncated": truncated});
    if truncated {
        out["checked_to"] = json!(checked.checked_sub(1).map(|k| step_of(&m, k, n)));
    }
    if let Some(v) = seconds {
        out["budget_s"] = json!(v);
    }
    out["stopped_at"] = stop["stopped_at"].clone();
    out["stopped_by_loop"] = stop["stopped_by_loop"].clone();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// Cube B (x 20…30) slides from 10 mm away until it touches A (x 0…10) over the 15 steps of
    /// study "Push": nothing ever overlaps.
    fn touching() -> Session {
        let mut s = Session::default();
        for (name, x) in [("A", 0), ("B", 20)] {
            run(&mut s, "component.activate", json!({"component": "root"}));
            run(&mut s, "component.create", json!({"name": name}));
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
        }
        run(&mut s, "component.activate", json!({"component": "root"}));
        let a = s.doc.find_component("A").and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
        run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
        run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
        run(&mut s, "motion.study", json!({"name": "Push", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, 0]]}]}));
        s
    }

    /// #7855: a check its work budget cuts short is not clear, though nothing collided in the
    /// steps it reached.
    #[test]
    fn a_check_cut_short_is_not_clear() {
        let mut s = touching();
        let p = json!({"study": "Push"});
        let full = run(&mut s, "motion.interference", p.clone());
        assert!(full["clear"] == true && full["truncated"] == false && full["samples"] == 16, "{full}");
        assert!(full.get("budget_s").is_none(), "{full}");
        // The least work that checks a first step stops long before the last.
        let cut = (0..31).find_map(|i| interference_within(&mut s, &p, 1usize << i, None).ok()).unwrap_or_default();
        assert!(cut["truncated"] == true && cut["samples"].as_u64().is_some_and(|n| (1..16).contains(&n)), "{cut}");
        assert!(cut["collisions"] == json!([]) && cut["clear"] == false, "{cut}");
    }

    /// #7855: given `budget_s`, the time is the budget, not the work: the smallest work budget,
    /// which stops a check before its first step, no longer stops it.
    #[test]
    fn a_time_budget_takes_the_place_of_the_work_budget() {
        let mut s = touching();
        let p = json!({"study": "Push"});
        assert!(interference_within(&mut s, &p, 1, None).is_err(), "one unit of work checks no step");
        let timed = interference_within(&mut s, &p, 1, Some(600.0)).unwrap_or_default();
        assert!(timed["clear"] == true && timed["truncated"] == false && timed["samples"] == 16, "{timed}");
        let r = run(&mut s, "motion.interference", json!({"study": "Push", "budget_s": 600}));
        assert!(r["clear"] == true && r["samples"] == 16 && r["budget_s"] == 600.0, "{r}");
    }

    /// The differences are exact for a quadratic: its velocity and acceleration everywhere,
    /// the ends too.
    #[test]
    fn finite_differences_follow_a_polynomial() {
        let h = 0.5;
        let f = |x: f64| 2.0 * x * x - 3.0 * x + 1.0;
        let v: Vec<f64> = (0..7).map(|k| f(k as f64 * h)).collect();
        let d = derivative(&v, h);
        let a = second_derivative(&v, h);
        for (k, (dk, ak)) in d.iter().zip(&a).enumerate() {
            let x = k as f64 * h;
            assert!((dk - (4.0 * x - 3.0)).abs() < 1e-9, "{k}: {dk}");
            assert!((ak - 4.0).abs() < 1e-9, "{k}: {ak}");
        }
        assert_eq!(derivative(&[1.0, 3.0], 2.0), vec![1.0, 1.0]);
        assert_eq!(second_derivative(&[1.0, 3.0], 2.0), vec![0.0, 0.0]);
        assert_eq!(derivative(&[5.0], 1.0), vec![0.0]);
        assert!(derivative(&[], 1.0).is_empty());
    }

    #[test]
    fn angles_unwrap_across_half_turns() {
        let mut v = vec![170.0, 179.0, -172.0, -160.0, 175.0];
        unwrap_degrees(&mut v);
        assert_eq!(v, vec![170.0, 179.0, 188.0, 200.0, 175.0]);
        // Half a turn a sample is a step, not a wrap.
        let mut v = vec![0.0, 180.0, 360.0];
        unwrap_degrees(&mut v);
        assert_eq!(v, vec![0.0, 180.0, 360.0]);
    }
}
