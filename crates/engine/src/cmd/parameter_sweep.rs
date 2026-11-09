//! Sweep a parameter and check that the design holds together at every value: that each feature
//! still builds, that every joint still finds its origins and none turned over, and that no
//! sketch plane flipped or sketch solve folded. A template driven by its parameters (one
//! design, many sizes) can be checked across its whole range before it is trusted, and the
//! report says what moved where, so a lost face or a flipped joint is found before a part is.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcore_record::Vec3;

use super::CommandSpec;
use crate::params::{bad, num, str_};
use crate::schema::parameter_sweep::{
    FeatureFailed, FeatureWarned, JointFlipped, JointLost, JointSide, MAX_VALUES, ParameterSweep, ParameterSweepParams, SketchFailed, SketchFlipped,
    SweepChanges, SweepResult, SweepSummary,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<ParameterSweepParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("inspect.parameter_sweep", "Parameter Sweep", crate::checked!("inspect.parameter_sweep", sweep))
    .icon("params")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<ParameterSweep>)
    .params(
    "parameter: name; values: [number | expression] (at most 200) | from, to: numbers, steps? (default 10, at most 199: steps + 1 values) → \
     for each value, in a copy of the design (the design itself is not changed): {value, ms (the rebuild), ok, failed: [{feature, name, error}], warned: [{feature, name, warning}] \
     (features that build at the current value and fail or warn at this one), joints_lost: [{joint, name, origin?: a|b, why}], joints_flipped: [{joint, name, origin: a|b, degrees}] \
     (an origin's z turned more than 90° on its own part from where it is now), sketches_flipped: [{sketch, name, degrees}] (the sketch plane's normal turned more than 90°), \
     sketches_failed: [{sketch, name, failing, folded}] (a solve that fails, or keeps a triangle's turn or an offset's side only by flipping it)} \
     (or {value, ms, ok: false, error} where the parameter takes no such value); \
     summary: {values, ok, slowest_ms, failed, joints_lost, joints_flipped, sketches_flipped, sketches_failed} (how many values show each)",
)];

/// A frame's origin and z axis, in its occurrence's coordinates.
type Frame = (Vec3, Vec3);

/// What the design looks like at one value.
struct Look {
    errors: BTreeMap<u64, (String, String)>,
    warnings: BTreeMap<u64, (String, String)>,
    /// Per joint: its name, its two origins (none: not found, with why).
    joints: BTreeMap<u64, (String, std::result::Result<Frame, String>, std::result::Result<Frame, String>)>,
    /// Joints the solve could not apply.
    unsolved: BTreeMap<u64, String>,
    /// Per sketch: its name, its plane's normal and whether its solve is sound (failing and
    /// folded constraints).
    sketches: BTreeMap<u64, (String, Vec3, Vec<String>, Vec<String>)>,
}

fn look(s: &Session) -> Look {
    let st = s.model.state();
    let mut errors = BTreeMap::new();
    let mut warnings = BTreeMap::new();
    for r in &s.model.results {
        if let Some(e) = &r.error {
            errors.insert(r.id, (r.name.clone(), e.clone()));
        }
        if let Some(w) = &r.warning {
            warnings.insert(r.id, (r.name.clone(), w.clone()));
        }
    }
    // In its own part's frame: an origin that turns over on its part is a lost reference; a part
    // the joints place anew is only its consequence.
    let frame = |o: &solidcraft_doc::joints::JointOrigin| -> std::result::Result<Frame, String> {
        let f = solidcraft_doc::joints::resolve_origin(&s.doc, &st, o).map_err(|e| e.to_string())?;
        Ok((Vec3::new(f[3][0], f[3][1], f[3][2]), Vec3::new(f[2][0], f[2][1], f[2][2])))
    };
    let joints = s.doc.assembly.joints.iter().filter(|j| !j.suppressed).map(|j| (j.id, (j.name.clone(), frame(&j.a), frame(&j.b)))).collect();
    let unsolved = if s.doc.assembly.joints.is_empty() {
        BTreeMap::new()
    } else {
        solidcraft_doc::joints::solve(&s.doc, &st).errors.into_iter().map(|(id, _, why)| (id, why)).collect()
    };
    let sketches = st
        .sketches
        .iter()
        .map(|k| {
            let failing = if k.report.status == solidcraft_sketch::SolveStatus::Failed { k.report.failing.clone() } else { Vec::new() };
            (k.feature, (k.name.clone(), k.plane.normal(), failing, k.report.folded.clone()))
        })
        .collect();
    Look { errors, warnings, joints, unsolved, sketches }
}

/// The angle between two directions, in degrees (none if either has no length).
fn degrees(a: Vec3, b: Vec3) -> Option<f64> {
    let (a, b) = (a.normalized()?, b.normalized()?);
    Some(a.dot(b).clamp(-1.0, 1.0).acos().to_degrees())
}

/// What changed from `base` (the design as it is) to `at` (at one value).
fn compare(base: &Look, at: &Look) -> SweepChanges {
    let failed = at
        .errors
        .iter()
        .filter(|(id, _)| !base.errors.contains_key(id))
        .map(|(id, (name, e))| FeatureFailed { feature: *id, name: name.clone(), error: e.clone() })
        .collect();
    let warned = at
        .warnings
        .iter()
        .filter(|(id, (_, w))| base.warnings.get(id).is_none_or(|(_, b)| b != w) && !at.errors.contains_key(id))
        .map(|(id, (name, w))| FeatureWarned { feature: *id, name: name.clone(), warning: w.clone() })
        .collect();
    let mut joints_lost = Vec::new();
    let mut joints_flipped = Vec::new();
    for (id, (name, a, b)) in &at.joints {
        let Some((_, a0, b0)) = base.joints.get(id) else { continue };
        for (side, now, was) in [(JointSide::A, a, a0), (JointSide::B, b, b0)] {
            match (now, was) {
                (Err(why), Ok(_)) => joints_lost.push(JointLost { joint: *id, name: name.clone(), origin: Some(side), why: why.clone() }),
                (Ok(f), Ok(f0)) => {
                    if let Some(d) = degrees(f.1, f0.1).filter(|d| *d > 90.0) {
                        joints_flipped.push(JointFlipped { joint: *id, name: name.clone(), origin: side, degrees: (d * 10.0).round() / 10.0 });
                    }
                }
                _ => {}
            }
        }
        if let (Some(why), None) = (at.unsolved.get(id), base.unsolved.get(id))
            && a.is_ok()
            && b.is_ok()
        {
            joints_lost.push(JointLost { joint: *id, name: name.clone(), origin: None, why: why.clone() });
        }
    }
    let mut sketches_flipped = Vec::new();
    let mut sketches_failed = Vec::new();
    for (id, (name, n, failing, folded)) in &at.sketches {
        let Some((_, n0, failing0, folded0)) = base.sketches.get(id) else { continue };
        if let Some(d) = degrees(*n, *n0).filter(|d| *d > 90.0) {
            sketches_flipped.push(SketchFlipped { sketch: *id, name: name.clone(), degrees: (d * 10.0).round() / 10.0 });
        }
        if (!failing.is_empty() && failing0.is_empty()) || (!folded.is_empty() && folded0.is_empty()) {
            sketches_failed.push(SketchFailed { sketch: *id, name: name.clone(), failing: failing.clone(), folded: folded.clone() });
        }
    }
    SweepChanges { failed, warned, joints_lost, joints_flipped, sketches_flipped, sketches_failed }
}

/// The values to sweep.
fn values(p: &Value, cmd: &str) -> Result<Vec<Value>> {
    match (p.get("values"), p.get("from"), p.get("to")) {
        (Some(Value::Array(list)), None, None) => {
            if list.is_empty() || list.len() > MAX_VALUES {
                return Err(bad(cmd, format!("`values` must list 1…{MAX_VALUES} values")));
            }
            list.iter()
                .map(|v| match v {
                    Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => Ok(v.clone()),
                    Value::String(x) if !x.trim().is_empty() && x.len() <= 1000 => Ok(v.clone()),
                    _ => Err(bad(cmd, "each value is a number or an expression")),
                })
                .collect()
        }
        (None, Some(_), Some(_)) => {
            let finite = |k: &str| num(p, k).filter(|x| x.is_finite() && x.abs() <= 1e9).ok_or_else(|| bad(cmd, format!("`{k}` must be a number")));
            let (from, to) = (finite("from")?, finite("to")?);
            let steps = match p.get("steps") {
                None | Some(Value::Null) => 10,
                Some(_) => num(p, "steps")
                    .filter(|k| k.fract() == 0.0 && *k >= 1.0 && *k < MAX_VALUES as f64)
                    .ok_or_else(|| bad(cmd, format!("`steps` must be a whole number 1…{}", MAX_VALUES - 1)))? as usize,
            };
            Ok((0..=steps).map(|i| json!(from + (to - from) * i as f64 / steps as f64)).collect())
        }
        _ => Err(bad(cmd, "give `values`, or `from` and `to`")),
    }
}

fn sweep(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.parameter_sweep";
    let name = str_(p, "parameter").ok_or_else(|| bad(cmd, "`parameter` is required (parameters.list)"))?.to_string();
    if !super::parameters::known(s, &name) {
        return Err(bad(cmd, format!("no parameter `{name}` (parameters.list)")));
    }
    let values = values(p, cmd)?;
    let base = look(s);
    let mut results = Vec::new();
    let mut summary =
        SweepSummary { values: 0, ok: 0, slowest_ms: 0.0, failed: 0, joints_lost: 0, joints_flipped: 0, sketches_flipped: 0, sketches_failed: 0 };
    for value in values {
        let mut x = s.scratch();
        let t0 = std::time::Instant::now();
        let changed = x.execute("parameters.change", &json!({"name": name, "expression": value}));
        let ms = (t0.elapsed().as_secs_f64() * 1e4).round() / 10.0;
        summary.slowest_ms = summary.slowest_ms.max(ms);
        let r = match changed {
            Ok(_) => {
                let c = compare(&base, &look(&x));
                summary.failed += usize::from(!c.failed.is_empty());
                summary.joints_lost += usize::from(!c.joints_lost.is_empty());
                summary.joints_flipped += usize::from(!c.joints_flipped.is_empty());
                summary.sketches_flipped += usize::from(!c.sketches_flipped.is_empty());
                summary.sketches_failed += usize::from(!c.sketches_failed.is_empty());
                let ok = c.failed.is_empty()
                    && c.joints_lost.is_empty()
                    && c.joints_flipped.is_empty()
                    && c.sketches_flipped.is_empty()
                    && c.sketches_failed.is_empty();
                SweepResult { value, ms, ok, error: None, changes: Some(c) }
            }
            Err(e) => SweepResult { value, ms, ok: false, error: Some(e.to_string()), changes: None },
        };
        summary.ok += usize::from(r.ok);
        results.push(r);
    }
    summary.values = results.len();
    serde_json::to_value(ParameterSweep { parameter: name, results, summary, param_warnings: Vec::new() }).map_err(|e| bad(cmd, e.to_string()))
}

#[cfg(test)]
#[path = "parameter_sweep_tests.rs"]
mod tests;
