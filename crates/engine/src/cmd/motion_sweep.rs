//! Sweeping a joint until something touches: `joint.sweep` drives one joint value from `from`
//! toward `to` on a copy of the design (closed loops following on their branch), measures the
//! gaps between parts at each sample and bisects the first contact down to `refine`, so a hinge,
//! door or linkage answers how far it opens, what stops it and how much room each part has.
//! `joint.find_limits` sweeps each value both ways from where it is, for the joint dialog's Find
//! Limits.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::Document;
use solidcraft_doc::joints::JointKind;

use super::analysis::{MAX_BUDGET_S, checked_pairs, part_place};
use super::contact_sweep::{ContactGeometry, Gap, ran_out};
use super::{drive_with_contact, joint_id, value};
use crate::params::{bad, bool_, num};
use crate::schema::joint_sweep::{NearestGap, SampleGap, SweepResult, SweepSample};
use crate::schema::motion::{FindLimitsResult, FoundLimits, Overlap, PartPlace, Stop, Why};
use crate::{EngineError, Result, Session};

pub const SWEEP_DOC: &str = "joint: id|name; from, to: its value the sweep runs between (degrees for angles, mm for distances, \
     or expressions); index? (which value, default 0); step? (between samples, in the joint's unit; default: fine enough that \
     no part passes through another between two); refine?: mm (the first contact is bisected until no part moves more than \
     this between the last clear value and the touching one; default 0.01); occurrences?: [ids or names] (checked against \
     each other; default every occurrence), with against?: [ids or names] (the parts that stay: each of `occurrences` only \
     against these), or contact_sets?: true (the pairs of the design's contact sets); exclude_pairs?: [[a, b]] (occurrences by id or name: \
     pairs of bodies under a and b are not checked, such as a pin in its hole); budget_s?: seconds (at most 86400; past them \
     the sweep stops with what it found; its work is then not capped. Without it the work caps, 4096 samples, 64 bisection \
     steps and 500 million triangle tests, stop it the same way); report_gaps?: bool (the nearest gap at \
     every sample, default false); apply?: bool (leave the design where the sweep stopped; default false: the design is not \
     changed) → stopped_at (the last value clear of contact, or where a loop or limit stopped it; null: it reached `to`), \
     stopped_by: {a, b, at: [x,y,z] (where they touch, world), a_part, b_part: {component, path: [occurrence names], path_ids} \
     (where each touching body sits)} | \"loop\" | \"limit\" | \"budget\" (stopped_at: the last value checked clear) | null, \
     samples: [{value, min_gap_mm?, pair?, parts?}], min_gap? (report_gaps: {part: {mm, at_value, to}}, the nearest each part came to another), unit, gap_method \
     (\"mesh\": distances between the tessellations), gap_tolerance_mm (how far those can be off: 0 when every face is flat), \
     pairs (checked), touching_at_start: [[a, b]] (pairs already touching at `from`, such as parts joined face to face or a door \
     on its frame: these stop it only once they pass into each other deeper than they start), touching_parts (where their \
     bodies sit), budget_s, budget_exceeded, budget_ran_out: \"time\" | \"samples\" | \"bisections\" (a contact reported \
     less refined than `refine`) | \"intersections\" | \"work\" | null, applied. The parts are tested at the samples and through the bisection; the motion is on a copy (closed loops \
     follow, contact sets do not stop it)";

pub const FIND_LIMITS_DOC: &str = "joint: id|name; index? (one value; default every value); refine?, step?, occurrences?, \
     against?, contact_sets?, exclude_pairs? (as joint.sweep) → values: [{index, dof, unit, value (now), min, max, min_by, max_by, range}], \
     each value swept both ways from where it is, its own limits ignored, closed loops following; each side stops at the \
     first contact (bisected to `refine`), where a loop cannot close, or at another limit (another value's, or another joint's through a motion link): min_by/max_by {a, b, at} | \
     \"loop\" | \"limit\" | \"budget\" (its work ran out: the last value checked clear) | \"turn\" (an angle with no contact within a turn: a turn from the other side, or ±180° from \
     now when nothing is met at all) | \"range\" (a distance with no contact within the parts' reach: min/max null); \
     touching_at_start, overlapping_at_start: [{a, b, depth_mm}] (already passing into each other: they stop it only going \
     deeper), refine_mm, gap_method, gap_tolerance_mm, pairs (0: no two parts to check, so nothing is swept). The design is \
     not changed";

/// Most samples a sweep takes before its bisection: each is a solve.
const MAX_SAMPLES: usize = 4096;
/// Most bisection steps: each halves the bracket around the first contact.
const MAX_BISECTIONS: usize = 64;
/// Most gap work over a whole sweep (see `ContactGeometry::gaps`).
const MAX_WORK: usize = 500_000_000;
/// Most gap work each side of a limit search takes: a whole turn of curved pins passing into each
/// other costs about 20 M a sample, some 500 samples a turn (a few seconds). A 32-bit build, the
/// web, takes half what its `usize` holds, so the work it adds up can still pass it.
const FIND_WORK: usize = {
    let most = MAX_WORK.saturating_mul(40);
    if most > usize::MAX / 2 { usize::MAX / 2 } else { most }
};

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
    /// When it stops, done or not (`budget_s`).
    deadline: Option<web_time::Instant>,
    /// The value's own limit left out (the copy only).
    free: bool,
}

/// What a sweep found.
struct Swept {
    /// The last value clear of contact (or where a loop or limit stopped it), and what stopped
    /// it; `None`: it reached `to`.
    stop: Option<(f64, Stop)>,
    samples: Vec<SweepSample>,
    nearest: BTreeMap<String, Nearest>,
    touching: Vec<[String; 2]>,
    touching_parts: Vec<[PartPlace; 2]>,
    /// The budget that ran out (`time`, `samples`, `bisections`, `intersections`, `work`).
    ran_out: Option<&'static str>,
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
    let budget_s = match p.get("budget_s") {
        None | Some(Value::Null) => None,
        Some(_) => match num(p, "budget_s") {
            Some(v) if v > 0.0 && v <= MAX_BUDGET_S => Some(v),
            _ => return Err(bad(cmd, format!("`budget_s` must be a number of seconds above 0, at most {MAX_BUDGET_S}"))),
        },
    };
    // Given a time, the work is not capped: the time is looked at before each sample and each
    // bisection step, as `motion.interference` does (#7855).
    let deadline = budget_s.and_then(|v| std::time::Duration::try_from_secs_f64(v).ok()).and_then(|d| web_time::Instant::now().checked_add(d));
    let budget = if deadline.is_some() { usize::MAX } else { MAX_WORK };
    let geometry = pairs_to_check(s, p, cmd)?;
    let sweep = Sweep { id, index, from, to, step, refine, report_gaps, budget, deadline, free: false };
    let r = run(s, geometry, sweep, cmd)?;

    let applied = apply && !Arc::ptr_eq(&r.doc, &s.doc);
    if applied {
        let mut doc = (*r.doc).clone();
        doc.assembly.contact_enabled = s.doc.assembly.contact_enabled;
        *s.doc_mut() = doc;
    }
    let (stopped_at, stopped_by) = r.stop.map(|(v, stop)| (shown(v), stop)).unzip();
    let min_gap = report_gaps.then(|| r.nearest.into_iter().map(|(part, g)| (part, NearestGap { mm: g.mm, at_value: g.at, to: g.to })).collect());
    let reply = SweepResult {
        joint: id,
        name,
        index,
        unit: if angle { "deg" } else { "mm" },
        from: shown(from),
        to: shown(to),
        stopped_at,
        stopped_by,
        samples: r.samples,
        refine_mm: refine,
        gap_method: "mesh",
        gap_tolerance_mm: r.chord,
        pairs: r.pairs,
        touching_at_start: r.touching,
        touching_parts: r.touching_parts,
        budget_s,
        budget_exceeded: r.ran_out.is_some(),
        budget_ran_out: r.ran_out,
        applied,
        min_gap,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

pub(super) fn find_limits(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "joint.find_limits";
    // Every search is on a copy: the design is only read.
    let s: &Session = s;
    let (id, name, kind) = swept_joint(s, p, cmd)?;
    let indices: Vec<usize> = match value_index(p, kind.dofs().len(), cmd)? {
        Some(i) => vec![i],
        None => (0..kind.dofs().len()).collect(),
    };
    let refine = positive(p, "refine", 1e3, cmd)?.unwrap_or(0.01);
    let step = positive(p, "step", 1e9, cmd)?;
    // No two parts to check (the joint turns the only part with bodies): nothing can be met, but
    // a closed loop or a limit can still stop an angle, swept a degree at a time; a distance has
    // nothing to search over. Parts the call names that make no pair are refused, as
    // `joint.sweep` refuses them.
    let named = p.get("occurrences").is_some_and(|v| !v.is_null()) || bool_(p, "contact_sets").unwrap_or(false);
    let geometry = if named { pairs_to_check(s, p, cmd)? } else { checked_pairs(s, p, cmd)? };
    let paired = geometry.has_pairs();
    let now = s.doc.assembly.joints.iter().find(|j| j.id == id).map(|j| j.values.clone()).unwrap_or_default();
    // A distance is searched as far as the parts reach: past it, a straight move meets nothing new.
    let reach = if paired { geometry.reach(s)? } else { 0.0 };
    if paired && (!reach.is_finite() || reach <= 0.0) {
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
        let step = match step {
            Some(x) => Some(if angle { x.to_radians() } else { x }),
            None if !paired => Some(1f64.to_radians()),
            None => None,
        };
        let side = |to: f64| {
            if !angle && !paired {
                return Ok(None);
            }
            let sweep = Sweep { id, index, from: c, to, step, refine, report_gaps: false, budget: FIND_WORK, deadline: None, free: true };
            Ok::<_, EngineError>(Some(run(s, geometry.clone(), sweep, cmd)?))
        };
        // (value, what stopped it) for each side; `None` value: no limit that side. An angle is
        // searched a turn up, then down to a turn below where that stopped: a span of at most a
        // turn, each side checked (a motion link can make the way down differ from the way up).
        let up = side(c + range)?;
        let up_stop = up.as_ref().and_then(|u| u.stop.clone());
        let down = side(if angle { up_stop.as_ref().map_or(c, |(u, _)| *u) - turn } else { c - range })?.and_then(|d| d.stop);
        let turned = |v: f64| (Some(v), Stop::Other(Why::Turn));
        let (min, max) = match (angle, up_stop, down) {
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
        if first.is_none() {
            first = up;
        }
    }
    let reply = FindLimitsResult {
        joint: id,
        name,
        values,
        refine_mm: refine,
        gap_method: "mesh".into(),
        gap_tolerance_mm: first.as_ref().map_or(0.0, |f| f.chord),
        pairs: first.as_ref().map_or(0, |f| f.pairs),
        touching_at_start: first.as_ref().map(|f| f.touching.clone()).unwrap_or_default(),
        overlapping_at_start: first
            .map(|f| f.overlapping)
            .unwrap_or_default()
            .into_iter()
            .map(|(a, b, depth_mm)| Overlap { a, b, depth_mm })
            .collect(),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Drive the value `sw` names from `sw.from` toward `sw.to` on a copy of `s`, checking the pairs
/// of `geometry` at each sample, and bisect the first contact.
fn run(s: &Session, mut geometry: ContactGeometry, sw: Sweep, cmd: &str) -> Result<Swept> {
    let Sweep { id, index, from, to, step, refine, report_gaps, budget, deadline, free } = sw;
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
    // Pairs touching where the sweep starts (parts joined face to face, a door on its frame)
    // stop it only when they pass into each other, deeper than they start (by more than their
    // meshes can be off): a door resting on its frame cannot sweep through it. Each pair of their
    // bodies is held to its own depth: pins already in each other don't hide arms of the same
    // parts meeting later, and plates that only come flush (parts end to end) never stop it.
    // A work cap stops the sweep with what it found (#5963): `ran` names the one that ran out.
    let mut ran: Option<&'static str> = None;
    let place = |g: &ContactGeometry, i: usize| part_place(&s.doc, g.body_path(i));
    let Some(start) = within(geometry.gaps(&t, touch, &mut work, budget), &mut ran)? else {
        return Ok(unchecked(from, ran, &geometry, t.doc.clone()));
    };
    let touching: BTreeSet<(u64, u64)> = start.iter().map(|g| key(g.a.0, g.b.0)).collect();
    let touching_names: Vec<[String; 2]> = start.iter().map(|g| [g.a.1.clone(), g.b.1.clone()]).collect();
    let touching_parts: Vec<[PartPlace; 2]> = start.iter().map(|g| [place(&geometry, g.bodies.0), place(&geometry, g.bodies.1)]).collect();
    let resting = geometry.take_pairs(|x, y| touching.contains(&key(x, y)));
    let rest_depth: Vec<f64> = if resting.has_pairs() {
        match within(resting.depths(&t, &mut work, budget), &mut ran)? {
            Some(d) => d,
            None => return Ok(unchecked(from, ran, &geometry, t.doc.clone())),
        }
    } else {
        Vec::new()
    };
    // A pair of bodies' own margin: flat faces against flat faces are exact.
    let margin = |i: usize| touch.max(2.0 * resting.pair_chord(i));
    // Parts already passing into each other, deeper than a touch: the deepest of their bodies.
    let mut overlapping: BTreeMap<(u64, u64), (String, String, f64)> = BTreeMap::new();
    for (i, depth) in rest_depth.iter().enumerate() {
        if *depth > margin(i)
            && let Some((a, b)) = resting.members(i)
        {
            let entry = overlapping.entry(key(a.0, b.0)).or_insert((a.1, b.1, 0.0));
            entry.2 = entry.2.max(*depth);
        }
    }

    // The samples run to where the joint gets: a limit or a loop can stop it short of `to`, and
    // the default count is for the motion up to there. One more drive toward `to` then finds
    // what stops it.
    // Past the sample cap (or, without a `step`, the transform budget) the sweep stops at the
    // last sample it took (#5963).
    let (n, sample_to, cap, cut_by) = match step {
        Some(h) => {
            let n = ((to - from).abs() / h).ceil();
            if !n.is_finite() {
                return Err(bad(cmd, format!("`step` makes more than {MAX_SAMPLES} samples: take a larger step")));
            }
            (n.min(usize::MAX as f64) as usize, to, MAX_SAMPLES, "samples")
        }
        None => {
            let mut end = t.scratch();
            drive_to(&mut end, to)?;
            let (n, allowed) = geometry.samples_within(&t, &end).map_err(|e| match e {
                EngineError::BadParams { msg, .. } => bad(cmd, format!("{msg} (or give a `step`)")),
                e => e,
            })?;
            let got = reached(&end);
            (n, if (got - to).abs() > 1e-9 && got.is_finite() { got } else { to }, allowed, if n > MAX_SAMPLES { "samples" } else { "work" })
        }
    };
    let n = n.max(1);
    let cut = n > cap;
    let targets: Vec<f64> = (1..=n.min(cap))
        .map(|k| from + (sample_to - from) * k as f64 / n as f64)
        .chain((!cut && (sample_to - to).abs() > 1e-9).then_some(to))
        .collect();

    let mut samples: Vec<SweepSample> = Vec::new();
    let mut nearest: BTreeMap<String, Nearest> = BTreeMap::new();
    let mut record = |t: &Session, v: f64, work: &mut usize, geometry: &ContactGeometry| -> Result<()> {
        if !report_gaps {
            samples.push(SweepSample { value: shown(v), gap: None });
            return Ok(());
        }
        let gaps = geometry.gaps(t, f64::INFINITY, work, budget)?;
        let gap = match gaps.first() {
            Some(g) => SampleGap {
                min_gap_mm: Some(g.mm),
                pair: Some([g.a.1.clone(), g.b.1.clone()]),
                parts: Some([place(geometry, g.bodies.0), place(geometry, g.bodies.1)]),
            },
            None => SampleGap { min_gap_mm: None, pair: None, parts: None },
        };
        samples.push(SweepSample { value: shown(v), gap: Some(gap) });
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
        let deeper = depths.iter().enumerate().find(|(i, d)| **d > rest_depth.get(*i).copied().unwrap_or(0.0) + margin(*i)).map(|(i, _)| i);
        let Some((i, (a, b))) = deeper.and_then(|i| resting.members(i).map(|m| (i, m))) else { return Ok(None) };
        // Where: the nearest points of those bodies (passing into each other, at no gap).
        let at = resting.only(i).gaps(t, touch, work, budget)?.into_iter().next().map_or((Vec3::ZERO, Vec3::ZERO), |g| g.at);
        Ok(Some(Gap { a, b, mm: 0.0, at, bodies: resting.pair_bodies(i).unwrap_or_default() }))
    };

    let (mut lo_doc, mut lo_v): (Arc<Document>, f64) = (t.doc.clone(), from);
    let mut stop: Option<(f64, Stop)> = None;
    let budget_stop = |v: f64| Some((v, Stop::Other(Why::Budget)));
    let out_of_time = || deadline.is_some_and(|d| web_time::Instant::now() >= d);
    'sweep: {
        if within(record(&t, from, &mut work, &geometry), &mut ran)?.is_none() {
            stop = budget_stop(from);
            break 'sweep;
        }
        for target in targets {
            if out_of_time() {
                (ran, stop) = (Some("time"), budget_stop(lo_v));
                t.doc = lo_doc.clone();
                break 'sweep;
            }
            let r = drive_to(&mut t, target)?;
            let v = reached(&t);
            let Some(found) = within(contact(&t, &mut work, &geometry), &mut ran)? else {
                stop = budget_stop(lo_v);
                t.doc = lo_doc.clone();
                break 'sweep;
            };
            if let Some(mut hit) = found {
                // Halve the bracket from the last clear value until no part moves more than
                // `refine` across it; each try is driven from the clear side, so loops keep their
                // branch. A budget running out here leaves the contact found, less refined.
                let (mut hi_doc, mut hi_v) = (t.doc.clone(), v);
                for _ in 0..MAX_BISECTIONS {
                    if geometry.travel(&lo_doc, &hi_doc)? <= refine {
                        break;
                    }
                    if out_of_time() {
                        ran = Some("time");
                        break;
                    }
                    let mid = (lo_v + hi_v) / 2.0;
                    t.doc = lo_doc.clone();
                    drive_to(&mut t, mid)?;
                    match within(contact(&t, &mut work, &geometry), &mut ran)? {
                        Some(Some(h)) => (hi_doc, hi_v, hit) = (t.doc.clone(), mid, h),
                        Some(None) => (lo_doc, lo_v) = (t.doc.clone(), mid),
                        None => break,
                    }
                }
                if ran.is_none() && geometry.travel(&lo_doc, &hi_doc)? > refine {
                    ran = Some("bisections");
                }
                t.doc = lo_doc.clone();
                let at = (hit.at.0 + hit.at.1) * 0.5;
                let (a_part, b_part) = (place(&geometry, hit.bodies.0), place(&geometry, hit.bodies.1));
                stop = Some((
                    lo_v,
                    Stop::Contact { a: hit.a.1, b: hit.b.1, at: [at.x, at.y, at.z], a_part: Box::new(a_part), b_part: Box::new(b_part) },
                ));
                break 'sweep;
            }
            // (The drive on toward `to` past a limit stays where the last sample was.) Clear
            // here, so a budget running out measuring its gaps stops the sweep here.
            if (v - lo_v).abs() > 1e-12 && within(record(&t, v, &mut work, &geometry), &mut ran)?.is_none() {
                stop = budget_stop(v);
                break 'sweep;
            }
            if r["stopped_by_loop"].is_object() {
                stop = Some((v, Stop::Other(Why::Loop)));
                break 'sweep;
            }
            if (v - target).abs() > 1e-9 {
                stop = Some((v, Stop::Other(Why::Limit)));
                break 'sweep;
            }
            (lo_doc, lo_v) = (t.doc.clone(), v);
        }
        if cut {
            (ran, stop) = (Some(cut_by), budget_stop(lo_v));
        }
    }
    let pairs = geometry.pair_count() + resting.pair_count();
    Ok(Swept {
        stop,
        samples,
        nearest,
        touching: touching_names,
        touching_parts,
        ran_out: ran,
        overlapping: overlapping.into_values().collect(),
        pairs,
        chord: geometry.chord(),
        doc: t.doc,
    })
}

/// `Ok(Some(x))`, or `Ok(None)` where a work cap ran out: `ran` then names it.
fn within<T>(r: Result<T>, ran: &mut Option<&'static str>) -> Result<Option<T>> {
    match r {
        Ok(x) => Ok(Some(x)),
        Err(e) => match ran_out(&e) {
            Some(budget) => {
                *ran = Some(budget);
                Ok(None)
            }
            None => Err(e),
        },
    }
}

/// A sweep a work cap stopped before it checked anything: at `from`.
fn unchecked(from: f64, ran: Option<&'static str>, geometry: &ContactGeometry, doc: Arc<Document>) -> Swept {
    Swept {
        stop: Some((from, Stop::Other(Why::Budget))),
        samples: Vec::new(),
        nearest: BTreeMap::new(),
        touching: Vec::new(),
        touching_parts: Vec::new(),
        ran_out: ran,
        overlapping: Vec::new(),
        pairs: geometry.pair_count(),
        chord: geometry.chord(),
        doc,
    }
}
