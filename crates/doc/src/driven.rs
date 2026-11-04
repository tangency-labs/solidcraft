//! Driven (reference) dimensions as read-only parameters (#5582). A driven dimension keeps a
//! model parameter `dN`, as a driving one does, so expressions can use it. Its value is what the
//! dimension measures once its sketch is solved, and nothing can set it.
//!
//! The order of evaluation: a feature that comes before the driven dimension's sketch in the
//! timeline cannot use it (it would read a value worked out later). Within the sketch itself a
//! driving dimension may use it: the sketch is solved again with the measured value until the
//! value settles. A dimension that would depend on itself that way (the driven dimension
//! measures geometry the dimension moves) is refused.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_sketch::{Analysis, Hold, Sketch, SolveReport};

use crate::eval::param_names;
use crate::expr::{self, ParamDef, Value};
use crate::{DocError, Document, FeatureKind, Result};

/// Solves of one sketch, the first one included, before a driven value that dimensions of the
/// same sketch use must have settled.
const MAX_PASSES: usize = 8;
/// Names followed through parameter expressions at most: a bound on a design's chains.
const MAX_REACH: usize = 1 << 16;
/// Words in every refusal that a driven dimension causes, so a command can tell them from other
/// errors of the features it rebuilds.
const MARK: &str = "driven dimension";

/// A driven dimension's parameter: the sketch it is in and what it measures (mm or radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrivenParam {
    /// The sketch feature.
    pub feature: u64,
    /// The sketch's place in the timeline.
    pub index: usize,
    pub value: Value,
}

/// The named driven dimensions of a sketch: (constraint index, name, measured value).
fn driven_in(sk: &Sketch) -> impl Iterator<Item = (usize, &str, Value)> {
    sk.constraints.iter().enumerate().filter(|(_, c)| c.driven).filter_map(|(i, c)| {
        let name = c.param.as_deref()?;
        let v = c.kind.value().filter(|v| v.is_finite())?;
        Some((i, name, if c.kind.is_angle() { Value::angle(v) } else { Value::length(v) }))
    })
}

/// The expression a driven value is evaluated from: exact, in mm or radians.
pub(crate) fn measured_def(name: &str, v: Value) -> ParamDef {
    if v.ang != 0 {
        ParamDef { name: name.into(), expr: format!("{} rad", v.v), unit: "rad".into() }
    } else {
        ParamDef { name: name.into(), expr: format!("{} mm", v.v), unit: "mm".into() }
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

/// Whether an error message is one a driven dimension causes (it is used before its sketch, or a
/// dimension would depend on itself through it).
pub fn is_driven_problem(message: &str) -> bool {
    message.contains(MARK)
}

/// The driven dimensions of a design and the names every parameter expression refers to, for
/// following what a feature uses through other parameters.
pub struct DrivenIndex {
    pub driven: BTreeMap<String, DrivenParam>,
    refs: BTreeMap<String, Vec<String>>,
}

impl DrivenIndex {
    /// The driven dimensions `names` use, directly or through other parameters.
    pub fn reached<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut todo: Vec<&str> = names.into_iter().collect();
        let mut out = BTreeSet::new();
        while let Some(n) = todo.pop() {
            if seen.len() >= MAX_REACH || !seen.insert(n) {
                continue;
            }
            if self.driven.contains_key(n) {
                out.insert(n.to_string());
            }
            if let Some(r) = self.refs.get(n) {
                todo.extend(r.iter().map(String::as_str));
            }
        }
        out
    }

    /// Why feature `at` of `doc` cannot be built: it uses a driven dimension of a sketch that
    /// comes after it in the timeline. A sketch may use its own driven dimensions.
    pub fn order_problem(&self, doc: &Document, at: usize) -> Option<String> {
        let f = doc.features.get(at)?;
        let names = param_names(f);
        let later = self.reached(names.iter().map(String::as_str)).into_iter().find(|n| self.driven.get(n).is_some_and(|d| d.index > at))?;
        let sketch = self.driven.get(&later).and_then(|d| doc.features.get(d.index)).map(|g| g.name.clone()).unwrap_or_default();
        Some(format!(
            "`{later}` is a {MARK} of sketch `{sketch}`, which comes later in the timeline: only features after that sketch can use its value"
        ))
    }
}

impl Document {
    /// Every named driven dimension, by its parameter name.
    pub fn driven_params(&self) -> BTreeMap<String, DrivenParam> {
        let mut out = BTreeMap::new();
        for (index, f) in self.features.iter().enumerate() {
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                for (_, name, value) in driven_in(sketch) {
                    out.insert(name.to_string(), DrivenParam { feature: f.id, index, value });
                }
            }
        }
        out
    }

    /// Whether `name` is a driven dimension's parameter (read-only).
    pub fn is_driven_param(&self, name: &str) -> bool {
        self.features.iter().any(|f| match &f.kind {
            FeatureKind::Sketch { sketch, .. } => sketch.constraints.iter().any(|c| c.driven && c.param.as_deref() == Some(name)),
            _ => false,
        })
    }

    /// Refuse to set `name` when it is a driven dimension's parameter.
    pub(crate) fn refuse_driven(&self, name: &str) -> Result<()> {
        if self.is_driven_param(name) {
            return Err(DocError::Invalid(format!(
                "`{name}` is a {MARK}: its value is what it measures, so it cannot be set (make the dimension driving to give it a value)"
            )));
        }
        Ok(())
    }

    /// The driven dimensions and the parameter references to follow, or None when the design
    /// has no named driven dimension.
    pub fn driven_index(&self) -> Option<DrivenIndex> {
        let driven = self.driven_params();
        if driven.is_empty() {
            return None;
        }
        let refs = self.param_defs().into_iter().map(|d| (d.name, expr::references(&d.expr))).collect();
        Some(DrivenIndex { driven, refs })
    }

    /// For each feature that uses a driven dimension of a sketch after it: why it can't be built.
    pub fn driven_order_problems(&self) -> BTreeMap<u64, String> {
        let Some(index) = self.driven_index() else { return BTreeMap::new() };
        (0..self.features.len()).filter_map(|at| Some((self.features.get(at)?.id, index.order_problem(self, at)?))).collect()
    }

    /// `vals` with the driven values `measured` put in, and every parameter that uses them
    /// (directly or through others) worked out again from them. A parameter that no longer
    /// evaluates is left out, so whatever uses it fails naming it.
    pub fn with_driven_values(&self, vals: &BTreeMap<String, Value>, measured: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
        let mut out = vals.clone();
        out.extend(measured.iter().map(|(n, v)| (n.clone(), *v)));
        let defs = self.param_defs();
        let index: BTreeMap<&str, (&ParamDef, Vec<String>)> = defs.iter().map(|d| (d.name.as_str(), (d, expr::references(&d.expr)))).collect();
        // Everything that uses a measured value, through any chain of parameters.
        let mut dirty: BTreeSet<&str> = measured.keys().map(String::as_str).collect();
        for _ in 0..=defs.len() {
            let before = dirty.len();
            for (name, (_, refs)) in &index {
                if !dirty.contains(name) && refs.iter().any(|r| dirty.contains(r.as_str())) {
                    dirty.insert(name);
                }
            }
            if dirty.len() == before {
                break;
            }
        }
        let doc_len = crate::params::doc_len(&self.units);
        let mut done: BTreeSet<&str> = measured.keys().map(String::as_str).collect();
        // In dependency order: a pass works out each one whose dirty references are done.
        for _ in 0..=dirty.len() {
            let mut progress = false;
            for name in dirty.iter().copied().filter(|n| !done.contains(n)).collect::<Vec<_>>() {
                let Some((def, refs)) = index.get(name) else { continue };
                if refs.iter().any(|r| dirty.contains(r.as_str()) && !done.contains(r.as_str())) {
                    continue;
                }
                let look = |n: &str| out.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
                match expr::eval_def(def, &look, doc_len) {
                    Ok(v) => out.insert(name.to_string(), v),
                    Err(_) => out.remove(name),
                };
                done.insert(name);
                progress = true;
            }
            if !progress {
                break;
            }
        }
        // A loop of parameters (an error already) keeps no stale value.
        for name in dirty.iter().filter(|n| !done.contains(*n)) {
            out.remove(*name);
        }
        out
    }

    /// Write each driven dimension's measured value into its parameter's expression, in the
    /// parameter's unit, so the design file and the Parameters table read what it measures.
    pub fn sync_driven_params(&mut self) {
        let driven = self.driven_params();
        if driven.is_empty() {
            return;
        }
        let texts: BTreeMap<String, String> = driven.iter().map(|(n, d)| (n.clone(), self.measured_text(d.value))).collect();
        for p in self.params.iter_mut().filter(|p| p.model) {
            if let Some(text) = texts.get(&p.name)
                && p.expr != *text
            {
                p.expr.clone_from(text);
            }
        }
    }

    /// A measured value as a dimension's expression shows it: in the design's units, or degrees.
    pub fn measured_text(&self, v: Value) -> String {
        if v.ang != 0 { format!("{} deg", (v.v.to_degrees() * 1e6).round() / 1e6) } else { self.design_length(v.v).1 }
    }

    /// Solve a sketch whose own driving values use its own driven dimensions: after the first
    /// solve (`report`), solve again with what the driven dimensions measure until it settles,
    /// then refuse a dimension that depends on itself through one of them. `vals` are the values
    /// the first solve used.
    pub(crate) fn settle_own_driven(
        &self,
        vals: &BTreeMap<String, Value>,
        sk: &mut Sketch,
        mut report: SolveReport,
        analysis: Analysis,
    ) -> Result<SolveReport> {
        if !sk.constraints.iter().any(|c| c.driven && c.param.is_some()) {
            return Ok(report);
        }
        let Some(index) = self.driven_index() else { return Ok(report) };
        let users = own_users(sk, &index)?;
        if users.is_empty() {
            return Ok(report);
        }
        if !report.keeps_orientation() {
            return Ok(report);
        }
        // A loop is refused before any solve again, which could not settle it.
        self_dependence(sk, &users)?;
        let used: BTreeSet<&str> = users.iter().map(|(_, _, d)| d.as_str()).collect();
        let mut vals = vals.clone();
        for pass in 1..=MAX_PASSES {
            let measured: BTreeMap<String, Value> = driven_in(sk).filter(|(_, n, _)| used.contains(n)).map(|(_, n, v)| (n.to_string(), v)).collect();
            let stale = measured.iter().any(|(n, v)| vals.get(n).is_none_or(|w| !close(w.v, v.v)));
            if !stale || !report.keeps_orientation() {
                return Ok(report);
            }
            if pass == MAX_PASSES {
                break;
            }
            vals = self.with_driven_values(&vals, &measured);
            let here = sk.driving_values();
            report = self.solve_sketch_once(&vals, sk, &here, None, analysis)?;
        }
        let names: Vec<&str> = used.into_iter().collect();
        Err(DocError::Invalid(format!(
            "the {MARK} `{}` does not settle: the dimensions that use it keep changing what it measures",
            names.join("`, `")
        )))
    }
}

/// The driving dimensions of `sk` that use one of its own driven dimensions: (constraint index,
/// the dimension's parameter, the driven dimension's parameter). Its other inputs (points
/// placed by expressions, patterns, texts, equation curves) may not use one: refused.
fn own_users(sk: &Sketch, index: &DrivenIndex) -> Result<Vec<(usize, String, String)>> {
    let own: BTreeSet<&str> = driven_in(sk).map(|(_, n, _)| n).collect();
    let mut users = Vec::new();
    for (i, c) in sk.constraints.iter().enumerate().filter(|(_, c)| !c.driven) {
        let Some(p) = c.param.as_deref() else { continue };
        for d in index.reached([p]).into_iter().filter(|d| own.contains(d.as_str())) {
            users.push((i, p.to_string(), d));
        }
    }
    let mut others: Vec<(String, &String)> = Vec::new();
    for p in &sk.points {
        if let Some(e) = p.exprs.as_deref() {
            others.extend(e.x.iter().chain(&e.y).map(|x| (format!("point {}", p.id), x)));
        }
    }
    for pattern in &sk.patterns {
        others.extend(pattern.count_expression.iter().chain(pattern.expressions.values()).map(|x| (format!("pattern {}", pattern.id), x)));
    }
    others.extend(sk.text_exprs().into_iter().map(|(what, x)| (format!("text {what}"), x)));
    for eq in &sk.equations {
        others.extend(eq.list().into_iter().map(|(_, x)| (format!("equation curve {}", eq.curve), x)));
    }
    for (what, e) in others {
        let refs = expr::references(e);
        if let Some(d) = index.reached(refs.iter().map(String::as_str)).into_iter().find(|d| own.contains(d.as_str())) {
            return Err(DocError::Invalid(format!("the {what} of this sketch cannot use `{d}`, a {MARK} of the same sketch")));
        }
    }
    Ok(users)
}

/// Refuse a dimension that depends on itself through a driven dimension of its sketch: nudged,
/// it changes what that driven dimension measures. `sk` is solved.
fn self_dependence(sk: &Sketch, users: &[(usize, String, String)]) -> Result<()> {
    let at = |s: &Sketch, d: &str| driven_in(s).find(|(_, n, _)| *n == d).map(|(_, _, v)| v.v);
    for (ci, p, d) in users {
        let (Some(v), Some(before)) = (sk.constraints.get(*ci).and_then(|c| c.kind.value()), at(sk, d)) else { continue };
        let step = 1e-3 * v.abs().max(1.0);
        let mut nudged = sk.clone();
        let Some(c) = nudged.constraints.get_mut(*ci) else { continue };
        c.kind.set_value(v + step);
        let kind = c.kind.clone();
        if nudged.validate(&kind).is_err() {
            continue;
        }
        let r = solidcraft_sketch::solve_with(&mut nudged, &Hold::default(), None, Analysis::StatusOnly, None);
        // A nudge the sketch can't take tells nothing; the passes above bound the solve.
        if !r.ok() {
            continue;
        }
        if let Some(after) = at(&nudged, d)
            && (after - before).abs() > 1e-4 * step
        {
            return Err(DocError::Invalid(format!("`{p}` would depend on itself: it uses `{d}`, a {MARK} that measures geometry `{p}` moves")));
        }
    }
    Ok(())
}
