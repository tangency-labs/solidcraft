//! Associative trace definitions and dependency checks. Sampling uses the existing motion
//! driver on a scratch session; only a changed generated wire is handed to the evaluator.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::joints::{self, JointOrigin, Snap};
use solidcraft_doc::motion::{MotionTrace, TraceMotion};
use solidcraft_doc::{IDENTITY, apply_point};
use solidcraft_geom::Vec3;

use crate::params::{bad, bool_};
use crate::{Result, Session};

const CMD: &str = "motion.trace.edit";

#[derive(Clone, Debug, Default)]
pub(crate) struct Cache {
    entries: Arc<BTreeMap<u64, CacheEntry>>,
    history: Arc<BTreeMap<u64, Vec<(Value, Vec<Vec3>)>>>,
    #[cfg(test)]
    pub(crate) recomputations: usize,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    inputs: std::result::Result<Value, String>,
    error: Option<String>,
}

fn motion(s: &Session, p: &Value, cmd: &str) -> Result<TraceMotion> {
    let _ = super::motion_plan(s, p, cmd)?;
    if p.get("study").is_some() {
        let study = s.doc.assembly.studies.get(super::study_index(s, p, cmd)?).ok_or_else(|| bad(cmd, "study is missing"))?;
        Ok(TraceMotion::Study { study: study.id })
    } else {
        Ok(TraceMotion::Joint {
            joint: super::joint_id(s, p.get("joint"), cmd)?,
            index: p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
            from: p.get("from").cloned().unwrap_or(Value::Null),
            to: p.get("to").cloned().unwrap_or(Value::Null),
        })
    }
}

pub(super) fn source(s: &Session, p: &Value, cmd: &str) -> Result<MotionTrace> {
    if s.doc.assembly.traces.len() >= super::MAX_ITEMS {
        return Err(bad(cmd, "too many motion traces"));
    }
    let (occurrence, local) = super::traced_point(s, p, cmd)?;
    let point = match p.get("point_ref") {
        Some(v) => {
            let mut reference = v.clone();
            // The origin parser uses an occurrence's first placement. A trace can name a
            // different nested instance; convert its world pick to that canonical frame.
            let id = occurrence.last().copied().unwrap_or(0);
            let placed = placed_world(s, &occurrence)?;
            let inverse = solidcraft_doc::mat_inverse(&placed).ok_or_else(|| bad(cmd, "invalid occurrence placement"))?;
            let canonical = joints::occurrence_world(&s.doc, id);
            for key in ["face", "circle", "edge", "point"] {
                if let Some(at) = reference.get(key).and_then(crate::params::vec3) {
                    let at = apply_point(&canonical, apply_point(&inverse, at));
                    reference[key] = json!([at.x, at.y, at.z]);
                }
            }
            let o = super::super::joints::origin_param(s, Some(&reference), cmd)?;
            if o.occurrence != occurrence.last().copied().unwrap_or(0) {
                return Err(bad(cmd, "point_ref must belong to the traced occurrence"));
            }
            o
        }
        None => JointOrigin { occurrence: occurrence.last().copied().unwrap_or(0), snap: Snap::Point { point: local, z: None, x: None } },
    };
    let referenced = apply_point(&joints::resolve_origin(&s.doc, &s.model.state(), &point)?, Vec3::ZERO);
    if referenced.dist(local) > 1e-4 {
        return Err(bad(cmd, "point_ref must resolve to the traced point"));
    }
    let motion = motion(s, p, cmd)?;
    let samples = p.get("samples").map(|_| super::sample_count(p, 100, super::MAX_TRACE, cmd)).transpose()?;
    Ok(MotionTrace {
        sketch: 0,
        wire: String::new(),
        occurrence,
        point,
        motion,
        samples,
        seeds: s.doc.assembly.joints.iter().map(|j| (j.id, j.values.clone())).collect(),
    })
}

fn params(t: &MotionTrace, point: Vec3) -> Value {
    let mut p = match &t.motion {
        TraceMotion::Joint { joint, index, from, to } => json!({"joint":joint,"index":index,"from":from,"to":to}),
        TraceMotion::Study { study } => json!({"study":study}),
    };
    p["occurrence"] = json!(t.occurrence);
    p["point"] = json!([point.x, point.y, point.z]);
    if let Some(n) = t.samples {
        p["samples"] = json!(n);
    }
    p
}

fn local(s: &Session, t: &MotionTrace) -> Result<Vec3> {
    if t.occurrence.len() > 64
        || t.seeds.len() > joints::MAX_JOINTS
        || t.seeds.iter().any(|(_, q)| q.len() > 6 || q.iter().any(|v| !v.is_finite()))
        || t.point.occurrence != t.occurrence.last().copied().unwrap_or(0)
    {
        return Err(bad(CMD, "invalid saved trace inputs"));
    }
    let m = joints::resolve_origin(&s.doc, &s.model.state(), &t.point)?;
    let p = apply_point(&m, Vec3::ZERO);
    if !p.len().is_finite() || p.len() >= 1e7 {
        return Err(bad(CMD, "invalid trace point"));
    }
    Ok(p)
}

fn placed_world(s: &Session, path: &[u64]) -> Result<solidcraft_doc::Mat> {
    s.doc.path_placement(path).map(|(_, world)| world).ok_or_else(|| bad(CMD, "the trace occurrence path is missing"))
}

/// Only the moving graph connected to the trace/driver is followed. Grounded anchors stop
/// traversal, so another mechanism attached to the same ground does not invalidate this one.
fn graph(s: &Session, t: &MotionTrace) -> (BTreeSet<u64>, BTreeSet<u64>) {
    let a = &s.doc.assembly;
    // With contact enabled another mechanism can be an obstacle. Its joint frames and
    // stationary placements are then dependencies too, even without a joint to this trace.
    if a.contact_enabled {
        return (s.doc.occurrences.iter().map(|o| o.id).collect(), a.joints.iter().map(|j| j.id).collect());
    }
    let anchor = |id: u64| id == 0 || s.doc.occurrences.iter().any(|o| o.id == id && o.grounded);
    let mut occurrences: BTreeSet<u64> = t.occurrence.iter().copied().collect();
    let mut ids: BTreeSet<u64> = match &t.motion {
        TraceMotion::Joint { joint, .. } => BTreeSet::from([*joint]),
        TraceMotion::Study { study } => a.studies.iter().find(|m| m.id == *study).into_iter().flat_map(|m| m.keys.iter().map(|k| k.joint)).collect(),
    };
    loop {
        let before = (occurrences.len(), ids.len());
        for j in a.joints.iter().take(joints::MAX_JOINTS) {
            if ids.contains(&j.id) || (!j.suppressed && [j.a.occurrence, j.b.occurrence].iter().any(|o| !anchor(*o) && occurrences.contains(o))) {
                ids.insert(j.id);
                occurrences.extend([j.a.occurrence, j.b.occurrence]);
            }
        }
        for l in a.links.iter().take(joints::MAX_JOINTS) {
            if ids.contains(&l.a) || ids.contains(&l.b) {
                ids.extend([l.a, l.b]);
            }
        }
        let parents: Vec<_> = s.doc.occurrences.iter().filter(|o| occurrences.contains(&o.id)).flat_map(|o| s.doc.first_path(o.parent)).collect();
        occurrences.extend(parents);
        if before == (occurrences.len(), ids.len()) {
            break;
        }
    }
    (occurrences, ids)
}

fn inputs(s: &Session, t: &MotionTrace) -> Result<Value> {
    let at = local(s, t)?;
    // The plan reads only driving inputs, not the picked point or displayed placement.
    let p = params(t, at);
    placed_world(s, &t.occurrence)?;
    let (default, given, plan) = super::motion_plan(s, &p, CMD)?;
    let n = super::sample_count(&p, default.min(super::MAX_TRACE), super::MAX_TRACE, CMD)?;
    let (occurrences, ids) = graph(s, t);
    let st = s.model.state();
    let seeds: BTreeMap<u64, &Vec<f64>> = t.seeds.iter().map(|(id, q)| (*id, q)).collect();
    let mut joints = Vec::new();
    for j in s.doc.assembly.joints.iter().filter(|j| ids.contains(&j.id)) {
        let mut definition = j.clone();
        definition.name.clear();
        definition.values = seeds.get(&j.id).map(|q| (*q).clone()).unwrap_or_default();
        let (offset, angle, q) = joints::expressed(&s.doc, &definition)?;
        definition.offset = offset;
        definition.angle = angle;
        definition.values = q;
        definition.exprs = Default::default();
        let fa = joints::resolve_origin(&s.doc, &st, &j.a)?;
        let fb = joints::resolve_origin(&s.doc, &st, &j.b)?;
        joints.push(json!([definition, fa, fb]));
    }
    // Poses of driven parts are outputs. Only anchors and placements no joint places are
    // inputs. Include ancestors of nested paths to notice an enclosing assembly's move.
    let placed: BTreeSet<u64> = s.doc.assembly.joints.iter().filter(|j| !j.suppressed).flat_map(|j| [j.a.occurrence, j.b.occurrence]).collect();
    let poses: Vec<Value> = s
        .doc
        .occurrences
        .iter()
        .filter(|o| occurrences.contains(&o.id))
        .map(|o| json!([o.id, o.parent, o.component, o.grounded, if o.grounded || !placed.contains(&o.id) { o.transform } else { IDENTITY }]))
        .collect();
    let links: Vec<_> = s
        .doc
        .assembly
        .links
        .iter()
        .filter(|l| ids.contains(&l.a) || ids.contains(&l.b))
        .map(|l| {
            let mut l = l.clone();
            l.coupling = None;
            l
        })
        .collect();
    let study = match &t.motion {
        TraceMotion::Study { study } => s.doc.assembly.studies.iter().find(|m| m.id == *study).map(|m| json!([m.steps, m.keys])),
        _ => None,
    };
    let contact = if s.doc.assembly.contact_enabled {
        // Contact reads geometry, not just the joint frames. Content keys avoid meshing or
        // hashing triangles here; unkeyed bodies conservatively invalidate on a new revision.
        let bodies: Vec<_> = st.bodies.iter().map(|b| json!([b.name, b.component, b.geometry_key().unwrap_or(s.revision)])).collect();
        json!([s.doc.assembly.contact_all, s.doc.assembly.contact_sets, bodies])
    } else {
        Value::Null
    };
    // A plan fills unkeyed coordinates from the current pose. They belong to the retained
    // branch seed instead; normalizing the small value vectors avoids copying the document
    // merely to check whether a cached trace is still valid.
    let mut ends = [plan(s, 0, n), plan(s, n - 1, n)];
    for end in &mut ends {
        for (id, values) in end {
            if let Some(seed) = seeds.get(id) {
                for (i, v) in values.iter_mut().enumerate() {
                    if !given.contains(&(*id, i)) {
                        *v = seed.get(i).copied().unwrap_or(0.0);
                    }
                }
            }
        }
    }
    Ok(json!([t.sketch, t.wire, t.occurrence, at, n, given, ends, joints, poses, links, study, contact]))
}

fn seeded(s: &Session, t: &MotionTrace) -> Session {
    let mut scratch = s.scratch();
    let seeds: BTreeMap<_, _> = t.seeds.iter().map(|(id, q)| (*id, q)).collect();
    for j in &mut scratch.doc_mut().assembly.joints {
        if let Some(q) = seeds.get(&j.id) {
            j.values = (*q).clone();
        }
    }
    scratch
}

pub(super) fn remember(s: &mut Session, t: &MotionTrace) {
    let key = inputs(s, t).map_err(|e| e.to_string());
    if let Ok(key) = &key
        && let Some(points) = s.doc.sketch(t.sketch).ok().and_then(|sk| sk.wires.iter().find(|w| w.id == t.wire)).map(|w| w.fit.clone())
    {
        remember_points(s, t.sketch, key.clone(), points);
    }
    Arc::make_mut(&mut s.motion_traces.entries).insert(t.sketch, CacheEntry { inputs: key, error: None });
}

fn remember_points(s: &mut Session, id: u64, key: Value, points: Vec<Vec3>) {
    let history = Arc::make_mut(&mut s.motion_traces.history).entry(id).or_default();
    if history.iter().all(|(old, _)| *old != key) {
        if history.len() >= 8 {
            history.remove(0);
        }
        history.push((key, points));
    }
}

fn same_points(a: &[Vec3], b: &[Vec3]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.dist(*b) < 1e-9)
}

fn sample(s: &Session, t: &MotionTrace) -> Result<Vec<Vec3>> {
    let at = local(s, t)?;
    let mut scratch = seeded(s, t);
    scratch.pending_moves.clear();
    let world = placed_world(&scratch, &t.occurrence)?;
    let p = params(t, apply_point(&world, at));
    let (mut points, _) = super::sample_trace(&scratch, &p, CMD)?;
    super::finish_trace(&mut points);
    if points.len() < 2 {
        return Err(bad(CMD, "the traced point no longer moves"));
    }
    Ok(points)
}

/// Refresh generated curves once after the input model and joint origins are current. A
/// failed dependency retains the last curve and publishes a warning instead of a stale success.
pub(crate) fn refresh(s: &mut Session) {
    // Designs without associations pay no dependency-walk or geometry-check cost.
    if s.doc.assembly.traces.is_empty() {
        s.motion_traces.entries = Default::default();
        s.motion_traces.history = Default::default();
        return;
    }
    let existing: BTreeSet<u64> = s.doc.features.iter().map(|f| f.id).collect();
    if s.doc.assembly.traces.iter().any(|t| !existing.contains(&t.sketch)) {
        s.doc_mut().assembly.traces.retain(|t| existing.contains(&t.sketch));
    }
    if s.doc.assembly.traces.is_empty() {
        s.motion_traces.entries = Default::default();
        s.motion_traces.history = Default::default();
        return;
    }
    let traces: Vec<_> = s.doc.assembly.traces.iter().take(super::MAX_ITEMS).cloned().collect();
    for r in &mut s.model.results {
        if traces.iter().any(|t| t.sketch == r.id) && r.warning.as_ref().is_some_and(|w| w.starts_with("Motion Trace: ")) {
            r.warning = None;
        }
    }
    if s.motion_traces.entries.keys().any(|id| !traces.iter().any(|t| t.sketch == *id)) {
        Arc::make_mut(&mut s.motion_traces.entries).retain(|id, _| traces.iter().any(|t| t.sketch == *id));
        Arc::make_mut(&mut s.motion_traces.history).retain(|id, _| traces.iter().any(|t| t.sketch == *id));
    }
    let mut changed = false;
    for t in &traces {
        if s.doc.feature(t.sketch).is_none_or(|f| f.suppressed) {
            continue;
        }
        let key = inputs(s, t).map_err(|e| e.to_string());
        let cached = key
            .as_ref()
            .ok()
            .and_then(|key| s.motion_traces.history.get(&t.sketch).and_then(|h| h.iter().find(|(old, _)| old == key)).map(|(_, p)| p.clone()));
        let unchanged = s.motion_traces.entries.get(&t.sketch).is_some_and(|old| old.inputs == key);
        if unchanged
            && cached.as_ref().is_none_or(|points| {
                s.doc.sketch(t.sketch).ok().and_then(|sk| sk.wires.iter().find(|w| w.id == t.wire)).is_some_and(|w| same_points(&w.fit, points))
            })
        {
            continue;
        }
        let result = match &key {
            Ok(_) => match cached {
                Some(points) => Ok(points),
                None => {
                    #[cfg(test)]
                    {
                        s.motion_traces.recomputations += 1;
                    }
                    sample(s, t).map_err(|e| e.to_string())
                }
            },
            Err(e) => Err(e.clone()),
        };
        if let (Ok(key), Ok(points)) = (&key, &result) {
            remember_points(s, t.sketch, key.clone(), points.clone());
        }
        let error = match result {
            Ok(points) => match s.doc.sketch(t.sketch).ok().and_then(|sk| sk.wires.iter().find(|w| w.id == t.wire)) {
                Some(wire) if same_points(&wire.fit, &points) => None,
                Some(_) => {
                    if let Ok(sk) = s.doc_mut().sketch_mut(t.sketch)
                        && let Some(wire) = sk.wires.iter_mut().find(|w| w.id == t.wire)
                    {
                        wire.pts = solidcraft_sketch::fit_polyline(&points);
                        wire.fit = points;
                        changed = true;
                    }
                    None
                }
                None => Some("the generated trace curve is missing; detach or recreate the trace".to_string()),
            },
            Err(e) => Some(e),
        };
        Arc::make_mut(&mut s.motion_traces.entries).insert(t.sketch, CacheEntry { inputs: key, error });
    }
    if changed {
        s.model.evaluate(&s.doc);
        s.revision += 1;
    }
    for t in &traces {
        if let Some(error) = s.motion_traces.entries.get(&t.sketch).and_then(|e| e.error.as_ref())
            && let Some(r) = s.model.results.iter_mut().find(|r| r.id == t.sketch)
        {
            r.warning = Some(format!("Motion Trace: {error}; showing its last successful curve"));
        }
    }
}

pub(super) fn list(s: &Session) -> Vec<Value> {
    s.doc
        .assembly
        .traces
        .iter()
        .map(|t| {
            json!({"sketch":t.sketch,"motion":t.motion,"samples":t.samples,
        "error":s.motion_traces.entries.get(&t.sketch).and_then(|e| e.error.as_ref())})
        })
        .collect()
}

pub(super) fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let key = p.get("sketch").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).ok_or_else(|| bad(CMD, "give a sketch"))?;
    let id = s.doc.find_feature(&key).map(|f| f.id).ok_or_else(|| bad(CMD, "trace sketch is missing"))?;
    let i = s.doc.assembly.traces.iter().position(|t| t.sketch == id).ok_or_else(|| bad(CMD, "the sketch has no associative trace"))?;
    if bool_(p, "auto_update") == Some(false) {
        s.doc_mut().assembly.traces.remove(i);
        Arc::make_mut(&mut s.motion_traces.entries).remove(&id);
        Arc::make_mut(&mut s.motion_traces.history).remove(&id);
        return Ok(json!({"sketch":id,"auto_update":false}));
    }
    let mut t = s.doc.assembly.traces.get(i).cloned().ok_or_else(|| bad(CMD, "trace is missing"))?;
    if p.get("study").is_some() || p.get("joint").is_some() {
        t.motion = motion(s, p, CMD)?;
    } else if let TraceMotion::Joint { from, to, index, .. } = &mut t.motion {
        if let Some(v) = p.get("from") {
            *from = v.clone();
        }
        if let Some(v) = p.get("to") {
            *to = v.clone();
        }
        if let Some(v) = p.get("index") {
            *index = v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad(CMD, "invalid index"))?;
        }
    }
    if p.get("samples").is_some() {
        t.samples = Some(super::sample_count(p, 100, super::MAX_TRACE, CMD)?);
    }
    inputs(s, &t)?;
    // Validate before publishing an edited definition; execute can then undo the whole edit.
    if s.doc.assembly.traces.get(i) != Some(&t)
        && let Some(slot) = s.doc_mut().assembly.traces.get_mut(i)
    {
        *slot = t;
    }
    Ok(json!({"sketch":id,"auto_update":true}))
}
