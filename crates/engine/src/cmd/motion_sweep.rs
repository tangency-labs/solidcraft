//! Sweeping a joint until something touches: `joint.sweep` drives one joint value from `from`
//! toward `to` on a copy of the design (closed loops following on their branch), measures the
//! gaps between parts at each sample and bisects the first contact down to `refine`, so a hinge,
//! door or linkage answers how far it opens, what stops it and how much room each part has.
//! `joint.find_limits` sweeps each value both ways from where it is, for the joint dialog's Find
//! Limits.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_doc::Document;
use solidcraft_doc::joints::JointKind;
use solidcraft_geom::Vec3;

use super::analysis::checked_pairs;
use super::contact_sweep::{ContactGeometry, Gap};
use super::{drive_with_contact, joint_id, value};
use crate::params::{bad, bool_};
use crate::schema::motion::{FindLimitsResult, FoundLimits, Overlap, Stop, Why};
use crate::{EngineError, Result, Session};

pub const SWEEP_DOC: &str = "joint: id|name; from, to: its value the sweep runs between (degrees for angles, mm for distances, \
     or expressions); index? (which value, default 0); step? (between samples, in the joint's unit; default: fine enough that \
     no part passes through another between two); refine?: mm (the first contact is bisected until no part moves more than \
     this between the last clear value and the touching one; default 0.01); occurrences?: [ids or names] (checked against \
     each other; default every occurrence), with against?: [ids or names] (the parts that stay: each of `occurrences` only \
     against these), or contact_sets?: true (the pairs of the design's contact sets); report_gaps?: bool (the nearest gap at \
     every sample, default false); apply?: bool (leave the design where the sweep stopped; default false: the design is not \
     changed) → stopped_at (the last value clear of contact, or where a loop or limit stopped it; null: it reached `to`), \
     stopped_by: {a, b, at: [x,y,z] (where they touch, world)} | \"loop\" | \"limit\" | null, samples: [{value, min_gap_mm?, \
     pair?}], min_gap? (report_gaps: {part: {mm, at_value, to}}, the nearest each part came to another), unit, gap_method \
     (\"mesh\": distances between the tessellations), gap_tolerance_mm (how far those can be off: 0 when every face is flat), \
     pairs (checked), touching_at_start: [[a, b]] (pairs of parts with bodies already touching at `from`, such as parts joined \
     face to face or a door on its frame: those bodies stop it only once they pass into each other deeper than they start; \
     the parts' other bodies stop it as they touch), applied. The parts are tested at the samples and through the bisection; the motion is on a copy (closed loops \
     follow, contact sets do not stop it)";

pub const FIND_LIMITS_DOC: &str = "joint: id|name; index? (one value; default every value); refine?, step?, occurrences?, \
     against?, contact_sets? (as joint.sweep) → values: [{index, dof, unit, value (now), min, max, min_by, max_by, range}], \
     each value swept both ways from where it is, its own limits ignored, closed loops following; each side stops at the \
     first contact (bisected to `refine`), where a loop cannot close, or at another joint's limit: min_by/max_by {a, b, at} | \
     \"loop\" | \"limit\" | \"turn\" (an angle with no contact within a turn: a turn from the other side, or ±180° from \
     now when nothing is met at all) | \"range\" (a distance with no contact within the parts' reach: min/max null); \
     touching_at_start, overlapping_at_start: [{a, b, depth_mm}] (already passing into each other: they stop it only going \
     deeper), refine_mm, gap_method, gap_tolerance_mm, pairs. The design is not changed";

/// Most samples a sweep takes before its bisection: each is a solve.
const MAX_SAMPLES: usize = 4096;
/// Most bisection steps: each halves the bracket around the first contact.
const MAX_BISECTIONS: usize = 64;
/// Most gap work over a whole sweep (see `ContactGeometry::gaps`).
const MAX_WORK: usize = 500_000_000;
/// Most gap work each side of a limit search takes: a whole turn, where curved pins resting in
/// each other cost about 20 M a sample (the hinge sample's pins). (A 32-bit build, the web,
/// stops at what its `usize` holds.)
const FIND_WORK: usize = MAX_WORK.saturating_mul(20);

/// The nearest a part came to another along the sweep.
struct Nearest {
    mm: f64,
    at: f64,
    to: String,
}

/// One sweep of a joint value.
#[derive(Clone, Copy)]
struct Sweep {
    id: u64,
    index: usize,
    from: f64,
    to: f64,
    step: Option<f64>,
    refine: f64,
    report_gaps: bool,
    /// The gap work it may take.
    budget: usize,
    /// The value's own limit left out (the copy only).
    free: bool,
}

/// What a sweep found.
struct Swept {
    /// The last value clear of contact (or where a loop or limit stopped it), and what stopped
    /// it; `None`: it reached `to`.
    stop: Option<(f64, Stop)>,
    samples: Vec<Value>,
    nearest: BTreeMap<String, Nearest>,
    touching: Vec<[String; 2]>,
    /// Pairs of parts passing into each other at the start deeper than a touch: (a, b, mm).
    overlapping: Vec<(String, String, f64)>,
    pairs: usize,
    chord: f64,
    /// The design where it stopped.
    doc: Arc<Document>,
}

/// The joint a command sweeps: its id, name and kind (a rigid joint is refused).
fn swept_joint(s: &Session, p: &Value, cmd: &str) -> Result<(u64, String, JointKind)> {
    let id = joint_id(s, p.get("joint"), cmd)?;
    let joint = s.doc.assembly.joints.iter().find(|j| j.id == id).ok_or_else(|| bad(cmd, "joint"))?;
    if joint.kind.dofs().is_empty() {
        return Err(bad(cmd, "a rigid joint does not move"));
    }
    Ok((id, joint.name.clone(), joint.kind))
}

/// `index`, below `dofs` (`None`: not given).
fn value_index(p: &Value, dofs: usize, cmd: &str) -> Result<Option<usize>> {
    match p.get("index") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .filter(|i| *i < dofs)
            .map(Some)
            .ok_or_else(|| bad(cmd, format!("`index` must be below {dofs}, the joint's value count"))),
    }
}

/// A number above 0, at most `max` (`None`: not given).
fn positive(p: &Value, key: &str, max: f64, cmd: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && *x > 0.0 && *x <= max)
            .map(Some)
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a number above 0, at most {max}"))),
    }
}

/// The parts the command checks, refused when there are no two to check against each other.
fn pairs_to_check(s: &Session, p: &Value, cmd: &str) -> Result<ContactGeometry> {
    let geometry = checked_pairs(s, p, cmd)?;
    if !geometry.has_pairs() {
        return Err(bad(cmd, "no pairs of parts to check: list two or more `occurrences` with bodies, or give `against`"));
    }
    Ok(geometry)
}

pub(super) fn sweep(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.sweep";
    let (id, name, kind) = swept_joint(s, p, cmd)?;
    let index = value_index(p, kind.dofs().len(), cmd)?.unwrap_or(0);
    let angle = kind.is_angle(index);
    // Shown in degrees for angles, mm for distances.
    let shown = |v: f64| if angle { v.to_degrees() } else { v };
    let end = |key: &str| match p.get(key) {
        Some(v) => value(s, v, angle, cmd),
        None => Err(bad(cmd, format!("give `from` and `to`: the joint's values the sweep runs between (no `{key}`)"))),
    };
    let (from, to) = (end("from")?, end("to")?);
    let refine = positive(p, "refine", 1e3, cmd)?.unwrap_or(0.01);
    let step = positive(p, "step", 1e9, cmd)?.map(|x| if angle { x.to_radians() } else { x });
    let report_gaps = bool_(p, "report_gaps").unwrap_or(false);
    let apply = bool_(p, "apply").unwrap_or(false);
    let geometry = pairs_to_check(s, p, cmd)?;
    let sweep = Sweep { id, index, from, to, step, refine, report_gaps, budget: MAX_WORK, free: false };
    let r = run(s, geometry, sweep, cmd)?;

    let applied = apply && !Arc::ptr_eq(&r.doc, &s.doc);
    if applied {
        let mut doc = (*r.doc).clone();
        doc.assembly.contact_enabled = s.doc.assembly.contact_enabled;
        *s.doc_mut() = doc;
    }
    let by = |stop: &Stop| serde_json::to_value(stop).map_err(|e| EngineError::Other(e.to_string()));
    let mut out = json!({
        "joint": id, "name": name, "index": index, "unit": if angle { "deg" } else { "mm" },
        "from": shown(from), "to": shown(to),
        "stopped_at": r.stop.as_ref().map(|(v, _)| shown(*v)),
        "stopped_by": r.stop.as_ref().map(|(_, stop)| by(stop)).transpose()?,
        "samples": r.samples,
        "refine_mm": refine,
        "gap_method": "mesh", "gap_tolerance_mm": r.chord,
        "pairs": r.pairs,
        "touching_at_start": r.touching,
        "applied": applied,
    });
    if report_gaps {
        out["min_gap"] =
            r.nearest.into_iter().map(|(k, g)| (k, json!({"mm": g.mm, "at_value": g.at, "to": g.to}))).collect::<serde_json::Map<_, _>>().into();
    }
    Ok(out)
}

pub(super) fn find_limits(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.find_limits";
    let (id, name, kind) = swept_joint(s, p, cmd)?;
    let indices: Vec<usize> = match value_index(p, kind.dofs().len(), cmd)? {
        Some(i) => vec![i],
        None => (0..kind.dofs().len()).collect(),
    };
    let refine = positive(p, "refine", 1e3, cmd)?.unwrap_or(0.01);
    let step = positive(p, "step", 1e9, cmd)?;
    let geometry = pairs_to_check(s, p, cmd)?;
    let now = s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.values.clone()).unwrap_or_default();
    // A distance is searched as far as the parts reach: past it, a straight move meets nothing new.
    let reach = geometry.reach(s)?;
    if !reach.is_finite() || reach <= 0.0 {
        return Err(bad(cmd, "the parts have no size to search over"));
    }
    let turn = std::f64::consts::TAU;
    let mut values = Vec::with_capacity(indices.len());
    let mut first: Option<Swept> = None;
    for index in indices {
        let angle = kind.is_angle(index);
        let shown = |v: f64| if angle { v.to_degrees() } else { v };
        let c = now.get(index).copied().unwrap_or(0.0);
        if !c.is_finite() {
            return Err(bad(cmd, "the joint's value is not a number"));
        }
        let range = if angle { turn } else { reach };
        let step = step.map(|x| if angle { x.to_radians() } else { x });
        let side = |to: f64| {
            let sweep = Sweep { id, index, from: c, to, step, refine, report_gaps: false, budget: FIND_WORK, free: true };
            run(s, geometry.clone(), sweep, cmd)
        };
        // (value, what stopped it) for each side; `None` value: no limit that side. An angle is
        // searched a turn up, then down to a turn below where that stopped: a span of at most a
        // turn, each side checked (a motion link can make the way down differ from the way up).
        let up = side(c + range)?;
        let down = side(if angle { up.stop.as_ref().map_or(c, |(u, _)| *u) - turn } else { c - range })?.stop;
        let turned = |v: f64| (Some(v), Stop::Other(Why::Turn));
        let (min, max) = match (angle, up.stop.clone(), down) {
            (true, None, None) => (turned(c - turn / 2.0), turned(c + turn / 2.0)),
            (true, None, Some((d, by))) => ((Some(d), by), turned(d + turn)),
            (true, Some((u, by)), None) => (turned(u - turn), (Some(u), by)),
            (_, up, down) => {
                let found = |stop: Option<(f64, Stop)>| stop.map_or((None, Stop::Other(Why::Range)), |(v, by)| (Some(v), by));
                (found(down), found(up))
            }
        };
        values.push(FoundLimits {
            index,
            dof: kind.dofs().get(index).copied().unwrap_or("value").to_string(),
            unit: if angle { "deg" } else { "mm" }.to_string(),
            value: shown(c),
            min: min.0.map(shown),
            max: max.0.map(shown),
            min_by: min.1,
            max_by: max.1,
            range: shown(range),
        });
        first.get_or_insert(up);
    }
    let Some(start) = first else { return Err(bad(cmd, "the joint has no value to search")) };
    let reply = FindLimitsResult {
        joint: id,
        name,
        values,
        refine_mm: refine,
        gap_method: "mesh".into(),
        gap_tolerance_mm: start.chord,
        pairs: start.pairs,
        touching_at_start: start.touching,
        overlapping_at_start: start.overlapping.into_iter().map(|(a, b, depth_mm)| Overlap { a, b, depth_mm }).collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Drive the value `sw` names from `sw.from` toward `sw.to` on a copy of `s`, checking the pairs
/// of `geometry` at each sample, and bisect the first contact.
fn run(s: &Session, mut geometry: ContactGeometry, sw: Sweep, cmd: &str) -> Result<Swept> {
    let Sweep { id, index, from, to, step, refine, report_gaps, budget, free } = sw;
    let (dofs, angle) =
        s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| (j.kind.dofs().len(), j.kind.is_angle(index))).ok_or_else(|| bad(cmd, "joint"))?;
    let shown = |v: f64| if angle { v.to_degrees() } else { v };
    // On a copy, played from where the parts are placed; contact sets do not stop it.
    let mut t = s.scratch();
    t.pending_moves.clear();
    t.doc_mut().assembly.contact_enabled = false;
    if free && let Some(limit) = t.doc_mut().assembly.joints.iter_mut().find(|j| j.id == id).and_then(|j| j.limits.get_mut(index)) {
        *limit = None;
    }
    let given = BTreeSet::from([(id, index)]);
    let drive_to = |t: &mut Session, v: f64| -> Result<Value> {
        let mut q = t.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.values.clone()).unwrap_or_default();
        q.resize(q.len().max(dofs), 0.0);
        if let Some(x) = q.get_mut(index) {
            *x = v;
        }
        Ok(drive_with_contact(t, &[(id, q)], &given)?.0)
    };
    let reached = |t: &Session| t.doc.assembly.joints.iter().find(|j| j.id == id).and_then(|j| j.values.get(index).copied()).unwrap_or(0.0);

    let r = drive_to(&mut t, from)?;
    if r["stopped_by_loop"].is_object() {
        return Err(bad(cmd, "the joint cannot reach `from`: a closed loop it is in does not close there"));
    }
    if (reached(&t) - from).abs() > 1e-9 {
        return Err(bad(cmd, "`from` is outside the joint's limits"));
    }
    let scale = geometry.scale();
    // Nearer than this is touching (well below any `refine`).
    let touch = 1e-6 * scale;
    let mut work = 0usize;
    let key = |x: u64, y: u64| (x.min(y), x.max(y));
    // Bodies touching where the sweep starts (parts joined face to face, a door on its frame, a
    // leaf on its pin) stop it only when they pass into each other, deeper than they start (by
    // more than their meshes can be off): a door resting on its frame cannot sweep through it.
    // The parts' other bodies stop it as they touch.
    let start: Vec<Gap> = geometry.gaps(&t, touch, &mut work, budget)?;
    let touching: Vec<[String; 2]> = start.iter().map(|g| [g.a.1.clone(), g.b.1.clone()]).collect();
    let pairs = geometry.pair_count();
    let resting = geometry.take_touching(&t, touch, &mut work, budget)?;
    let rest_depth: Vec<f64> = if resting.has_pairs() { resting.depths(&t, &mut work, budget)? } else { Vec::new() };
    let margin = touch.max(2.0 * resting.chord());
    // Parts already passing into each other, deeper than a touch: the deepest of their bodies.
    let mut overlapping: BTreeMap<(u64, u64), (String, String, f64)> = BTreeMap::new();
    for (i, depth) in rest_depth.iter().enumerate() {
        if *depth > margin
            && let Some((a, b)) = resting.members(i)
        {
            let entry = overlapping.entry(key(a.0, b.0)).or_insert((a.1, b.1, 0.0));
            entry.2 = entry.2.max(*depth);
        }
    }

    // The samples run to where the joint gets: a limit or a loop can stop it short of `to`, and
    // the default count is for the motion up to there. One more drive toward `to` then finds
    // what stops it.
    let (n, sample_to) = match step {
        Some(h) => {
            let n = ((to - from).abs() / h).ceil();
            if !n.is_finite() || n > MAX_SAMPLES as f64 {
                return Err(bad(cmd, format!("`step` makes more than {MAX_SAMPLES} samples: take a larger step")));
            }
            (n as usize, to)
        }
        None => {
            let mut end = t.scratch();
            drive_to(&mut end, to)?;
            let n = geometry.samples(&t, &end).map_err(|e| match e {
                EngineError::BadParams { msg, .. } => bad(cmd, format!("{msg} (or give a `step`)")),
                e => e,
            })?;
            let got = reached(&end);
            (n, if (got - to).abs() > 1e-9 && got.is_finite() { got } else { to })
        }
    };
    let n = n.max(1);
    let targets: Vec<f64> =
        (1..=n).map(|k| from + (sample_to - from) * k as f64 / n as f64).chain(((sample_to - to).abs() > 1e-9).then_some(to)).collect();

    let mut samples: Vec<Value> = Vec::new();
    let mut nearest: BTreeMap<String, Nearest> = BTreeMap::new();
    let mut record = |t: &Session, v: f64, work: &mut usize, geometry: &ContactGeometry| -> Result<()> {
        if !report_gaps {
            samples.push(json!({"value": shown(v)}));
            return Ok(());
        }
        let gaps = geometry.gaps(t, f64::INFINITY, work, budget)?;
        match gaps.first() {
            Some(g) => samples.push(json!({"value": shown(v), "min_gap_mm": g.mm, "pair": [g.a.1, g.b.1]})),
            None => samples.push(json!({"value": shown(v), "min_gap_mm": null, "pair": null})),
        }
        for g in &gaps {
            for (me, other) in [(&g.a.1, &g.b.1), (&g.b.1, &g.a.1)] {
                if nearest.get(me).is_none_or(|x| g.mm < x.mm) {
                    nearest.insert(me.clone(), Nearest { mm: g.mm, at: shown(v), to: other.clone() });
                }
            }
        }
        Ok(())
    };
    let contact = |t: &Session, work: &mut usize, geometry: &ContactGeometry| -> Result<Option<Gap>> {
        if let Some(g) = geometry.gaps(t, touch, work, budget)?.into_iter().next() {
            return Ok(Some(g));
        }
        if !resting.has_pairs() {
            return Ok(None);
        }
        let depths = resting.depths(t, work, budget)?;
        let deeper = depths.iter().enumerate().find(|(i, d)| **d > rest_depth.get(*i).copied().unwrap_or(0.0) + margin).map(|(i, _)| i);
        let Some((i, (a, b))) = deeper.and_then(|i| resting.members(i).map(|m| (i, m))) else { return Ok(None) };
        // Where: the nearest points of those bodies (passing into each other, at no gap).
        let at = resting.only(i).gaps(t, touch, work, budget)?.into_iter().next().map_or((Vec3::ZERO, Vec3::ZERO), |g| g.at);
        Ok(Some(Gap { a, b, mm: 0.0, at }))
    };

    record(&t, from, &mut work, &geometry)?;
    let (mut lo_doc, mut lo_v): (Arc<Document>, f64) = (t.doc.clone(), from);
    let mut stop: Option<(f64, Stop)> = None;
    for target in targets {
        let r = drive_to(&mut t, target)?;
        let v = reached(&t);
        if let Some(mut hit) = contact(&t, &mut work, &geometry)? {
            // Halve the bracket from the last clear value until no part moves more than
            // `refine` across it; each try is driven from the clear side, so loops keep their branch.
            let (mut hi_doc, mut hi_v) = (t.doc.clone(), v);
            for _ in 0..MAX_BISECTIONS {
                if geometry.travel(&lo_doc, &hi_doc)? <= refine {
                    break;
                }
                let mid = (lo_v + hi_v) / 2.0;
                t.doc = lo_doc.clone();
                drive_to(&mut t, mid)?;
                match contact(&t, &mut work, &geometry)? {
                    Some(h) => (hi_doc, hi_v, hit) = (t.doc.clone(), mid, h),
                    None => (lo_doc, lo_v) = (t.doc.clone(), mid),
                }
            }
            t.doc = lo_doc.clone();
            let at = (hit.at.0 + hit.at.1) * 0.5;
            stop = Some((lo_v, Stop::Contact { a: hit.a.1, b: hit.b.1, at: [at.x, at.y, at.z] }));
            break;
        }
        // (The drive on toward `to` past a limit stays where the last sample was.)
        if (v - lo_v).abs() > 1e-12 {
            record(&t, v, &mut work, &geometry)?;
        }
        if r["stopped_by_loop"].is_object() {
            stop = Some((v, Stop::Other(Why::Loop)));
            break;
        }
        if (v - target).abs() > 1e-9 {
            stop = Some((v, Stop::Other(Why::Limit)));
            break;
        }
        (lo_doc, lo_v) = (t.doc.clone(), v);
    }
    Ok(Swept { stop, samples, nearest, touching, overlapping: overlapping.into_values().collect(), pairs, chord: geometry.chord(), doc: t.doc })
}
