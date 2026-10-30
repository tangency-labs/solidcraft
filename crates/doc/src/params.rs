//! Parameters: user parameters, model parameters of sketch dimensions, and feature inputs
//! (extrude distance, fillet radius…) which also appear as model parameters `dN` without being
//! stored twice: the feature keeps its expression and the parameter name refers to it. Every
//! parameter can be referenced by name from any expression; the dependency graph and cycles
//! come from the expression references.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::expr::{self, Expr, Kind, ParamDef, Value};
use crate::walk::Walk;
use crate::{DocError, Document, FeatureKind, MAX_PARAMS, Result};

/// Whether an expression of a feature is one of its inputs, which become model parameters named
/// in walk order (`dN`), or a coordinate of a primitive's placement point, which gets no name and
/// keeps bare numbers in mm whatever the design's units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExprRole {
    Input,
    Placement,
}

impl FeatureKind {
    /// The feature's numeric inputs: (label, expression, kind), in a fixed order.
    pub fn inputs_mut(&mut self) -> Vec<(&'static str, &mut Expr, Kind)> {
        let mut v = Vec::new();
        self.walk_exprs_mut(&mut |(label, e, kind, role)| {
            if role == ExprRole::Input {
                v.push((label, e, kind));
            }
        });
        v
    }

    /// Read-only view of [`FeatureKind::inputs_mut`]: (label, expression, kind).
    pub fn inputs(&self) -> Vec<(&'static str, String, Kind)> {
        let mut v = Vec::new();
        self.walk_exprs(&mut |(label, e, kind, role)| {
            if role == ExprRole::Input {
                v.push((label, e.to_string(), kind));
            }
        });
        v
    }

    /// The coordinates of the points that place the feature: a primitive's position, an
    /// explicit plane's origin. They are not inputs (no parameter names), but follow
    /// parameters as inputs do.
    pub fn placement_exprs_mut(&mut self) -> Vec<&mut Expr> {
        let mut v = Vec::new();
        self.walk_exprs_mut(&mut |(_, e, _, role)| {
            if role == ExprRole::Placement {
                v.push(e);
            }
        });
        v
    }

    /// Read-only view of [`FeatureKind::placement_exprs_mut`].
    pub fn placement_exprs(&self) -> Vec<&str> {
        let mut v = Vec::new();
        self.walk_exprs(&mut |(_, e, _, role)| {
            if role == ExprRole::Placement {
                v.push(e.as_str());
            }
        });
        v
    }

    /// Every expression the feature uses: its inputs, then a primitive's placement coordinates.
    pub fn expressions(&self) -> Vec<&str> {
        let (mut inputs, mut placement) = (Vec::new(), Vec::new());
        self.walk_exprs(&mut |(_, e, _, role)| match role {
            ExprRole::Input => inputs.push(e.as_str()),
            ExprRole::Placement => placement.push(e.as_str()),
        });
        inputs.extend(placement);
        if let FeatureKind::Sketch { sketch, .. } = self {
            inputs.extend(sketch.patterns.iter().filter_map(|p| p.count_expression.as_deref()));
            inputs.extend(sketch.patterns.iter().flat_map(|p| p.expressions.values().map(String::as_str)));
        }
        inputs
    }
}

/// One row of the parameters table.
#[derive(Clone, Debug, Serialize)]
pub struct ParamRow {
    pub name: String,
    pub expression: String,
    pub unit: String,
    pub value: Option<f64>,
    pub error: Option<String>,
    pub comment: String,
    /// "user", "sketch" (a dimension) or "feature".
    pub source: &'static str,
    /// For feature inputs: the feature and the input's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<&'static str>,
    pub favorite: bool,
    /// Names this one's expression uses, and names whose expressions use this one.
    pub uses: Vec<String>,
    pub used_by: Vec<String>,
    /// A text parameter's text (its unit is `text`, its value none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

fn unit_of(k: Kind) -> &'static str {
    match k {
        Kind::Length => "mm",
        Kind::Angle => "deg",
        Kind::Unitless => "",
    }
}

impl Document {
    /// Every (name, expression, kind) the expressions can refer to: stored parameters and the
    /// named feature inputs.
    pub fn all_param_exprs(&self) -> Vec<(String, String, Kind)> {
        let mut v: Vec<(String, String, Kind)> = self.params.iter().map(|p| (p.name.clone(), p.expr.clone(), p.kind())).collect();
        for f in &self.features {
            for (name, (_, e, k)) in f.param_names.iter().zip(f.kind.inputs()) {
                if !name.is_empty() {
                    v.push((name.clone(), e, k));
                }
            }
        }
        v
    }

    pub(crate) fn name_taken(&self, n: &str) -> bool {
        self.param(n).is_some() || self.text_param(n).is_some() || self.features.iter().any(|f| f.param_names.iter().any(|x| x == n))
    }

    /// The next free `dN` name.
    pub fn next_d_name(&self) -> String {
        // Every taken name at once: asking `name_taken` for d1, d2, … searched the whole
        // design once per number, quadratic in the number of named inputs.
        // Their numbers, so trying d1, d2 … writes no names: a design with thousands of named
        // inputs wrote that many for every new one.
        let names = self.params.iter().map(|p| p.name.as_str()).chain(self.features.iter().flat_map(|f| f.param_names.iter().map(String::as_str)));
        let taken = numbers_after("d", names);
        let n = (1..).find(|n| !taken.contains(n)).unwrap_or(1);
        format!("d{n}")
    }

    /// Give a feature's inputs parameter names (keeping existing names in place).
    pub fn name_feature_inputs(&mut self, id: u64) {
        let Some(idx) = self.feature_index(id) else { return };
        let n = self.features.get(idx).map(|f| f.kind.inputs().len()).unwrap_or(0);
        for k in 0..n {
            let have = self.features.get(idx).and_then(|f| f.param_names.get(k)).is_some_and(|x| !x.is_empty());
            if have {
                continue;
            }
            let name = self.next_d_name();
            if let Some(f) = self.features.get_mut(idx) {
                while f.param_names.len() <= k {
                    f.param_names.push(String::new());
                }
                if let Some(slot) = f.param_names.get_mut(k) {
                    *slot = name;
                }
            }
        }
        if let Some(f) = self.features.get_mut(idx) {
            f.param_names.truncate(n);
        }
    }

    /// Repair a loaded timeline in one pass. Reserve even later features' names, just as
    /// `name_feature_inputs` does, without restarting a document scan for every `dN` candidate.
    pub(crate) fn name_all_feature_inputs(&mut self) {
        let mut taken: BTreeMap<String, usize> = BTreeMap::new();
        for name in self.params.iter().map(|p| &p.name).chain(self.features.iter().flat_map(|f| &f.param_names)) {
            *taken.entry(name.clone()).or_default() += 1;
        }
        let mut next = 1usize;
        for f in &mut self.features {
            let n = f.kind.inputs().len();
            for k in 0..n {
                if f.param_names.get(k).is_some_and(|name| !name.is_empty()) {
                    continue;
                }
                let name = loop {
                    let name = format!("d{next}");
                    next += 1;
                    if !taken.contains_key(&name) {
                        break name;
                    }
                };
                if f.param_names.len() <= k {
                    f.param_names.resize(k + 1, String::new());
                }
                // Empty slots cannot collide with a generated name.
                if let Some(slot) = f.param_names.get_mut(k) {
                    *slot = name.clone();
                }
                taken.insert(name, 1);
            }
            // Match the single-feature repair: surplus names become available only after
            // this feature has been repaired. Counts preserve names shared with parameters.
            for name in f.param_names.drain(n..) {
                if let Some(count) = taken.get_mut(&name) {
                    *count -= 1;
                    if *count == 0 {
                        taken.remove(&name);
                        if let Some(i) = name.strip_prefix('d').and_then(|s| s.parse::<usize>().ok())
                            && name == format!("d{i}")
                            && i > 0
                        {
                            next = next.min(i);
                        }
                    }
                }
            }
        }
    }

    /// Change a parameter: a stored one, or a feature input by its name.
    /// A typed length that is a bare number means the design's units: it gets that unit written
    /// in (feature inputs are stored in mm otherwise). Anything else is returned unchanged.
    pub fn with_design_unit(&self, e: &str, kind: Kind) -> String {
        // A definition's value reads as the value would (#4071): `w = 2` is `w = 2 in`.
        if let Some((name, rhs)) = expr::definition(e)
            && expr::is_bare_number(rhs)
        {
            return format!("{name} = {}", self.with_design_unit(rhs, kind));
        }
        let u = self.units.trim();
        let bare = expr::is_bare_number(e);
        if kind == Kind::Length && bare && !u.is_empty() && u != "mm" && expr::unit_info(u).is_some_and(|(k, _)| k == Kind::Length) {
            format!("{} {u}", e.trim())
        } else {
            e.to_string()
        }
    }

    /// A measured length (mm) written as a value in the design's units, with its unit: `1 in`
    /// for 25.4 mm in an inch design. Kept to the micrometre, as a millimetre value would be, so
    /// a dimension that restates the geometry moves nothing. Returns (unit, expression).
    pub fn design_length(&self, mm: f64) -> (String, String) {
        let u = self.units.trim();
        let per_unit = match expr::unit_info(u) {
            Some((Kind::Length, s)) if u != "mm" && s.is_finite() && s > 0.0 => s,
            _ => return ("mm".into(), format!("{} mm", (mm * 1e6).round() / 1e6)),
        };
        let decimals = (-(1e-6 / per_unit).log10()).ceil().clamp(0.0, 12.0) as usize;
        let text = format!("{:.*}", decimals, mm / per_unit);
        let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
        let text = if text == "-0" { "0" } else { text };
        (u.into(), format!("{text} {u}"))
    }

    /// Every length input of every feature as the geometry reads it (mm; a unit-less result is
    /// in the design's units), for checking that an edit moves nothing.
    pub fn length_inputs(&self) -> Vec<Option<f64>> {
        let (vals, _) = self.param_values();
        let scale = expr::unit_info(self.units.trim()).filter(|(k, _)| *k == Kind::Length).map(|(_, s)| s).unwrap_or(1.0);
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        self.features
            .iter()
            .flat_map(|f| f.kind.inputs())
            .filter(|(_, _, k)| *k == Kind::Length)
            .map(|(_, e, _)| {
                let d = if expr::is_literal(&e) { expr::Defaults::default() } else { expr::Defaults { len: scale, ..expr::Defaults::default() } };
                expr::eval_with_defaults(&e, &look, d).and_then(|v| v.to_kind_in(Kind::Length, d)).ok()
            })
            .collect()
    }

    pub fn change_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        let hit = self.features.iter().enumerate().find_map(|(i, f)| f.param_names.iter().position(|n| n == name).map(|k| (i, k)));
        let Some((i, k)) = hit else { return self.set_param(name, expr_s, unit, comment) };
        if expr_s.len() > 4096 {
            return Err(DocError::Expr("expression too long".into()));
        }
        let mut next = self.clone();
        let kind = self.features.get(i).and_then(|f| f.kind.inputs().get(k).map(|x| x.2)).unwrap_or(Kind::Unitless);
        let expr_s = self.with_design_unit(expr_s, kind);
        if let Some(f) = next.features.get_mut(i)
            && let Some((_, e, _)) = f.kind.inputs_mut().into_iter().nth(k)
        {
            *e = expr_s.clone().into();
        }
        if let Some(c) = comment {
            next.param_comments.insert(name.to_string(), c.to_string());
        }
        let (_, errors) = next.param_values();
        if let Some(e) = errors.get(name) {
            return Err(DocError::Expr(e.clone()));
        }
        *self = next;
        Ok(())
    }

    /// Change existing expressions against their final dependency graph, independent of
    /// column order. Unlike `change_param`, this never creates a missing parameter.
    pub fn change_params_atomic(&mut self, changes: &[(String, String)]) -> Result<()> {
        if changes.len() > crate::config::MAX_COLUMNS {
            return Err(DocError::Invalid("too many parameter changes".into()));
        }
        let mut names = BTreeSet::new();
        let mut changed = BTreeSet::new();
        let (_, before_errors) = self.param_values();
        let mut next = self.clone();
        for (name, expression) in changes {
            if !names.insert(name.clone()) {
                return Err(DocError::Invalid(format!("duplicate parameter `{name}`")));
            }
            if expression.len() > 4096 || expression.trim().is_empty() {
                return Err(DocError::Expr("expression is empty or too long".into()));
            }
            if let Some(p) = next.text_params.iter_mut().find(|p| p.name == *name) {
                let text = crate::text_params::unquote(expression);
                if p.text != text {
                    changed.insert(name.clone());
                }
                p.text = text;
                continue;
            }
            if let Some(p) = next.params.iter_mut().find(|p| p.name == *name) {
                if p.expr != expression.trim() {
                    changed.insert(name.clone());
                }
                p.expr = expression.trim().to_string();
                continue;
            }
            let hit = next.features.iter().enumerate().find_map(|(i, f)| f.param_names.iter().position(|n| n == name).map(|k| (i, k)));
            let Some((i, k)) = hit else { return Err(DocError::Unknown(format!("parameter `{name}`"))) };
            let kind = next.features.get(i).and_then(|f| f.kind.inputs().get(k).map(|x| x.2)).unwrap_or(Kind::Unitless);
            let normalized = next.with_design_unit(expression, kind);
            let Some(feature) = next.features.get_mut(i) else { return Err(DocError::Unknown(format!("parameter `{name}`"))) };
            let Some((_, target, _)) = feature.kind.inputs_mut().into_iter().nth(k) else {
                return Err(DocError::Unknown(format!("parameter `{name}`")));
            };
            if *target != normalized {
                changed.insert(name.clone());
            }
            *target = normalized.into();
        }
        let (_, errors) = next.param_values();
        for (name, error) in errors {
            if changed.contains(&name) || !before_errors.contains_key(&name) {
                return Err(DocError::Expr(format!("parameter `{name}`: {error}")));
            }
        }
        *self = next;
        Ok(())
    }

    /// The parameters table (user, sketch dimensions, feature inputs).
    pub fn param_rows(&self) -> Vec<ParamRow> {
        let (vals, errs) = self.param_values();
        let all = self.all_param_exprs();
        let refs: BTreeMap<String, Vec<String>> = all.iter().map(|(n, e, _)| (n.clone(), expr::references(e))).collect();
        // Who uses each name, in name order: one pass over the references (a design an agent
        // built names every feature input, so a search per row was quadratic).
        let mut users: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for (k, r) in &refs {
            for x in r {
                users.entry(x.as_str()).or_default().push(k.clone());
            }
        }
        let used_by = |n: &str| -> Vec<String> { users.get(n).cloned().unwrap_or_default() };
        let value = |n: &str, k: Kind| vals.get(n).and_then(|v| v.to_kind(k).ok()).map(|x| if k == Kind::Angle { x.to_degrees() } else { x });
        let mut rows = Vec::new();
        for p in &self.params {
            rows.push(ParamRow {
                name: p.name.clone(),
                expression: p.expr.clone(),
                unit: p.unit.clone(),
                value: self.param_display_value(&vals, &p.name),
                error: errs.get(&p.name).cloned(),
                comment: p.comment.clone(),
                source: if p.model { "sketch" } else { "user" },
                feature: None,
                input: None,
                favorite: self.favorites.contains(&p.name),
                uses: refs.get(&p.name).cloned().unwrap_or_default(),
                used_by: used_by(&p.name),
                text: None,
            });
        }
        for f in &self.features {
            for (name, (label, e, k)) in f.param_names.iter().zip(f.kind.inputs()) {
                if name.is_empty() {
                    continue;
                }
                rows.push(ParamRow {
                    name: name.clone(),
                    expression: e,
                    unit: unit_of(k).into(),
                    value: value(name, k),
                    error: errs.get(name).cloned(),
                    comment: self.param_comments.get(name).cloned().unwrap_or_default(),
                    source: "feature",
                    feature: Some(f.name.clone()),
                    input: Some(label),
                    favorite: self.favorites.contains(name),
                    uses: refs.get(name).cloned().unwrap_or_default(),
                    used_by: used_by(name),
                    text: None,
                });
            }
        }
        for p in &self.text_params {
            let readers = self.features_reading_texts(&BTreeSet::from([p.name.clone()]));
            rows.push(ParamRow {
                name: p.name.clone(),
                expression: p.text.clone(),
                unit: crate::text_params::TEXT_UNIT.into(),
                value: None,
                error: None,
                comment: p.comment.clone(),
                source: "user",
                feature: None,
                input: None,
                favorite: self.favorites.contains(&p.name),
                uses: Vec::new(),
                used_by: readers.iter().filter_map(|id| self.feature(*id)).map(|f| format!("Feature:{}", f.name)).collect(),
                text: Some(p.text.clone()),
            });
        }
        rows
    }

    /// Move user parameter `name` to just before (or with `after`, just after) user parameter
    /// `target`: the order the Parameters dialog lists them in. Nothing else changes.
    pub fn move_user_param(&mut self, name: &str, target: &str, after: bool) -> Result<()> {
        fn user(params: &[crate::Param], n: &str) -> Option<usize> {
            params.iter().position(|p| p.name == n && !p.model)
        }
        let (Some(from), Some(_)) = (user(&self.params, name), user(&self.params, target)) else {
            let missing = if user(&self.params, name).is_none() { name } else { target };
            return Err(DocError::Unknown(format!("user parameter `{missing}`")));
        };
        if name == target {
            return Err(DocError::Invalid("a parameter cannot move next to itself".into()));
        }
        let moved = self.params.remove(from);
        let at = user(&self.params, target).map_or(self.params.len(), |i| if after { i + 1 } else { i });
        self.params.insert(at, moved);
        Ok(())
    }

    pub fn set_favorite(&mut self, name: &str, on: bool) -> Result<()> {
        if !self.name_taken(name) {
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        if on {
            self.favorites.insert(name.to_string());
        } else {
            self.favorites.remove(name);
        }
        Ok(())
    }

    /// User parameters as CSV (`name,unit,expression,value,comment`).
    pub fn params_csv(&self) -> String {
        let q = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
        let mut out = String::from("name,unit,expression,value,comment,favorite\n");
        for r in self.param_rows().into_iter().filter(|r| r.source == "user") {
            out += &format!(
                "{},{},{},{},{},{}\n",
                q(&r.name),
                q(&r.unit),
                q(&r.expression),
                r.value.map(|v| format!("{v}")).unwrap_or_default(),
                q(&r.comment),
                r.favorite
            );
        }
        out
    }

    /// Import user parameters (CSV with a header, or JSON `[{name, expression, unit?, comment?}]`):
    /// existing ones are updated, new ones added, all checked before anything changes.
    pub fn import_params(&mut self, text: &str) -> Result<Vec<String>> {
        let rows: Vec<(String, String, Option<String>, Option<String>)> = if text.trim_start().starts_with('[') {
            let v: serde_json::Value = serde_json::from_str(text).map_err(|e| DocError::Invalid(format!("parameters JSON: {e}")))?;
            v.as_array()
                .ok_or_else(|| DocError::Invalid("parameters JSON must be a list".into()))?
                .iter()
                .map(|r| {
                    let s = |k: &str| r.get(k).and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_f64().map(|n| n.to_string())));
                    Ok((
                        s("name").ok_or_else(|| DocError::Invalid("a parameter without `name`".into()))?,
                        s("expression").or_else(|| s("value")).unwrap_or_default(),
                        s("unit"),
                        s("comment"),
                    ))
                })
                .collect::<Result<_>>()?
        } else {
            let mut lines = text.lines();
            let header: Vec<String> = parse_csv_line(lines.next().unwrap_or_default()).into_iter().map(|h| h.trim().to_ascii_lowercase()).collect();
            let col = |n: &str| header.iter().position(|h| h == n);
            let (Some(ni), Some(ei)) = (col("name"), col("expression").or(col("value"))) else {
                return Err(DocError::Invalid("the CSV needs `name` and `expression` columns".into()));
            };
            let (ui, ci) = (col("unit"), col("comment"));
            lines
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    let c = parse_csv_line(l);
                    let get = |i: Option<usize>| i.and_then(|i| c.get(i)).map(|s| s.trim().to_string());
                    (get(Some(ni)).unwrap_or_default(), get(Some(ei)).unwrap_or_default(), get(ui), get(ci))
                })
                .collect()
        };
        if rows.len() > 10_000 {
            return Err(DocError::Invalid("too many parameters".into()));
        }
        // Each row is checked and applied as `change_param` would, but the parameters are
        // evaluated once for all of them, not once per row (thousands of rows took minutes).
        let mut next = self.clone();
        let (mut vals, _) = next.param_values();
        for (n, e, u, c) in &rows {
            next.import_row(n, e, u.as_deref(), c.as_deref(), &mut vals)?;
        }
        let (_, errors) = next.param_values();
        if let Some(e) = rows.iter().find_map(|(n, ..)| errors.get(n)) {
            return Err(DocError::Expr(e.clone()));
        }
        *self = next;
        Ok(rows.into_iter().map(|(n, ..)| n).collect())
    }

    /// One imported row: the checks and change of `change_param` (a feature's parameter) or
    /// `set_param` (a user parameter, a new one taking the unit of its value) without evaluating
    /// every parameter. `vals` are the values so far, for the units of the rows after it.
    fn import_row(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>, vals: &mut BTreeMap<String, Value>) -> Result<()> {
        if expr_s.len() > 4096 {
            return Err(DocError::Expr("expression too long".into()));
        }
        let hit = self.features.iter().enumerate().find_map(|(i, f)| f.param_names.iter().position(|n| n == name).map(|k| (i, k)));
        if let Some((i, k)) = hit {
            let kind = self.features.get(i).and_then(|f| f.kind.inputs().get(k).map(|x| x.2)).unwrap_or(Kind::Unitless);
            let expr_s = self.with_design_unit(expr_s, kind);
            if let Some(f) = self.features.get_mut(i)
                && let Some((_, e, _)) = f.kind.inputs_mut().into_iter().nth(k)
            {
                *e = expr_s.into();
            }
            if let Some(c) = comment {
                self.param_comments.insert(name.to_string(), c.to_string());
            }
            return Ok(());
        }
        if unit.map_or(self.text_param(name).is_some(), crate::text_params::is_text_unit) {
            return self.set_text_param(name, expr_s, comment);
        }
        if let Some(p) = Self::param_name_problem(name) {
            return Err(DocError::Invalid(p));
        }
        if expr_s.trim().is_empty() {
            return Err(DocError::Expr("empty expression".into()));
        }
        if let Some(u) = unit
            && expr::unit_info(u).is_none()
        {
            return Err(DocError::Expr(format!("unknown unit `{u}` (mm, cm, m, in, ft, deg, rad or none)")));
        }
        if comment.is_some_and(|c| c.len() > 4096) {
            return Err(DocError::Invalid("comment too long".into()));
        }
        match self.params.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.expr = expr_s.trim().to_string();
                if let Some(u) = unit {
                    p.unit = u.trim().to_string();
                }
                if let Some(c) = comment {
                    p.comment = c.to_string();
                }
            }
            None => {
                if self.params.len() >= MAX_PARAMS {
                    return Err(DocError::Invalid("too many parameters".into()));
                }
                let unit = match unit {
                    Some(u) => u.trim().to_string(),
                    None => Self::infer_unit_in(vals, expr_s, &self.units),
                };
                self.params.push(crate::Param {
                    name: name.into(),
                    expr: expr_s.trim().into(),
                    unit,
                    comment: comment.unwrap_or_default().into(),
                    model: false,
                });
            }
        }
        let def = self.params.iter().find(|p| p.name == name).map(|p| ParamDef { name: p.name.clone(), expr: p.expr.clone(), unit: p.unit.clone() });
        let value = def.and_then(|d| {
            expr::eval_def(&d, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))), doc_len(&self.units)).ok()
        });
        match value {
            Some(v) => vals.insert(name.to_string(), v),
            None => vals.remove(name),
        };
        Ok(())
    }
}

/// Something that uses a parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamUser {
    /// Another parameter's expression (a stored one or a feature input).
    Parameter { name: String },
    /// A feature input (`input` is its label, e.g. `Distance`).
    Feature { feature: u64, name: String, input: String },
    /// A sketch dimension (`constraint` is its id).
    Dimension { feature: u64, name: String, constraint: String },
    /// A configured column or an expression saved in a configuration row.
    Configuration { row: Option<String>, column: String },
    /// A joint's offset, angle or value, or a motion link's gear, rack or screw length (`input`
    /// says which; a link is named by the joint it drives).
    Joint { joint: u64, name: String, input: String },
    /// A sheet metal or plastic rule's value (`kind` says which, `field` which value).
    Rule { kind: String, name: String, field: String },
    /// The plane a canvas lies on (`input` is its label, e.g. `Offset`).
    Canvas { canvas: u64, name: String, input: String },
}

impl ParamUser {
    pub fn describe(&self) -> String {
        match self {
            ParamUser::Parameter { name } => format!("parameter {name}"),
            ParamUser::Feature { name, input, .. } => format!("{name} ({input})"),
            ParamUser::Dimension { name, constraint, .. } => format!("{name} dimension {constraint}"),
            ParamUser::Configuration { row, column } => match row {
                Some(row) => format!("configuration `{row}` ({column})"),
                None => format!("configuration column {column}"),
            },
            ParamUser::Joint { name, input, .. } => format!("joint {name} ({input})"),
            ParamUser::Rule { kind, name, field } => format!("{kind} rule {name} ({field})"),
            ParamUser::Canvas { name, input, .. } => format!("canvas {name} ({input})"),
        }
    }
}

/// Length unit of the document in mm (bare numbers of unit-less parameters next to lengths).
fn doc_len(units: &str) -> f64 {
    match expr::unit_info(units) {
        Some((Kind::Length, s)) => s,
        _ => 1.0,
    }
}

impl Document {
    /// Every parameter with its unit, for evaluation: stored parameters and named feature
    /// inputs (mm / deg / none by their kind).
    pub fn param_defs(&self) -> Vec<ParamDef> {
        let mut v: Vec<ParamDef> =
            self.params.iter().map(|p| ParamDef { name: p.name.clone(), expr: p.expr.clone(), unit: p.unit.clone() }).collect();
        for f in &self.features {
            for (name, (_, e, k)) in f.param_names.iter().zip(f.kind.inputs()) {
                if !name.is_empty() {
                    // A bare number stored in an input is mm (the design's units are written in
                    // when it is typed); anything else that comes out unit-less (a unit-less
                    // parameter used as a length) is in the design's units.
                    let unit = match k {
                        Kind::Length if !expr::is_literal(&e) && expr::unit_info(self.units.trim()).is_some_and(|(k, _)| k == Kind::Length) => {
                            self.units.trim().to_string()
                        }
                        _ => unit_of(k).to_string(),
                    };
                    v.push(ParamDef { name: name.clone(), expr: e, unit });
                }
            }
        }
        v
    }

    /// Evaluate all parameters: values (mm / rad / unit-less) and errors by name. Errors name
    /// the problem (`circular reference: a → b → a`, `cannot add a length and an angle`).
    pub fn param_values(&self) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
        expr::eval_params(&self.param_defs(), doc_len(&self.units))
    }

    /// A parameter's value in its own unit (e.g. 2 for a 50.8 mm parameter in inches).
    pub fn param_display_value(&self, vals: &BTreeMap<String, Value>, name: &str) -> Option<f64> {
        let v = vals.get(name)?;
        let unit = match self.param(name) {
            Some(p) => p.unit.clone(),
            None => v.kind().map(|k| unit_of(k).to_string()).unwrap_or_default(),
        };
        Some(expr::value_in_unit(*v, &unit))
    }

    /// The unit for a new parameter with this expression: that of its value; a bare number is
    /// a length in the design's units, as a typed length reads it (#8865).
    pub(crate) fn infer_unit(&self, e: &str) -> String {
        Self::infer_unit_in(&self.param_values().0, e, &self.units)
    }

    /// [`Document::infer_unit`] against parameter values already worked out, a length taking
    /// `units` (the design's, or `mm` where a bare number reads as millimetres).
    pub(crate) fn infer_unit_in(vals: &BTreeMap<String, Value>, e: &str, units: &str) -> String {
        let v = expr::eval_with(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))));
        match v.ok().and_then(|v| v.kind()) {
            Some(Kind::Unitless) if !expr::references(e).is_empty() => String::new(),
            Some(Kind::Angle) => "deg".into(),
            _ => expr::length_unit_or_mm(units).to_string(),
        }
    }

    /// Everything that uses a parameter: other parameters (and feature inputs) whose
    /// expressions refer to it, and the sketch dimensions it drives.
    pub fn param_users(&self, name: &str) -> Vec<ParamUser> {
        let mut out = Vec::new();
        for rule in &self.configs.rules {
            if rule.parameter_references().iter().any(|reference| reference == name) {
                out.push(ParamUser::Configuration { row: None, column: format!("rule `{}`", rule.name) });
            }
        }
        for (index, column) in self.configs.columns.iter().enumerate() {
            if let crate::config::Column::Param(target) = column {
                let key = self.config_key(column);
                if target == name {
                    out.push(ParamUser::Configuration { row: None, column: key.clone() });
                }
                for row in &self.configs.rows {
                    if row.cells.get(index).is_some_and(|value| expr::references(value).iter().any(|reference| reference == name)) {
                        out.push(ParamUser::Configuration { row: Some(row.name.clone()), column: key.clone() });
                    }
                }
            }
        }
        for p in &self.params {
            if p.name != name && expr::references(&p.expr).iter().any(|r| r == name) {
                out.push(ParamUser::Parameter { name: p.name.clone() });
            }
        }
        for f in &self.features {
            for (k, (label, e, _)) in f.kind.inputs().into_iter().enumerate() {
                if expr::references(&e).iter().any(|r| r == name) {
                    match f.param_names.get(k).filter(|n| !n.is_empty()) {
                        Some(n) => out.push(ParamUser::Parameter { name: n.clone() }),
                        None => out.push(ParamUser::Feature { feature: f.id, name: f.name.clone(), input: label.to_string() }),
                    }
                }
            }
            if f.kind.placement_exprs().iter().any(|e| expr::references(e).iter().any(|r| r == name)) {
                out.push(ParamUser::Feature { feature: f.id, name: f.name.clone(), input: "Position".into() });
            }
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                for c in &sketch.constraints {
                    if c.param.as_deref() == Some(name) {
                        out.push(ParamUser::Dimension { feature: f.id, name: f.name.clone(), constraint: c.id.clone() });
                    }
                }
                for pattern in &sketch.patterns {
                    if pattern.count_expression.as_ref().is_some_and(|e| expr::references(e).iter().any(|r| r == name)) {
                        out.push(ParamUser::Feature { feature: f.id, name: f.name.clone(), input: format!("{} Quantity", pattern.id) });
                    }
                }
                for (what, e) in
                    sketch.text_exprs().into_iter().chain(sketch.patterns.iter().flat_map(|p| p.expressions.iter().map(|(k, e)| (k.as_str(), e))))
                {
                    if expr::references(e).iter().any(|r| r == name) {
                        out.push(ParamUser::Feature { feature: f.id, name: f.name.clone(), input: what.to_string() });
                    }
                }
                if sketch.text_word_exprs().iter().any(|e| crate::text_params::references(e).iter().any(|r| r == name)) {
                    out.push(ParamUser::Feature { feature: f.id, name: f.name.clone(), input: "Text".into() });
                }
            }
        }
        for (joint, input, e) in self.assembly.exprs() {
            if expr::references(e).iter().any(|r| r == name) {
                let jname = self.assembly.joints.iter().find(|j| j.id == joint).map(|j| j.name.clone()).unwrap_or_else(|| joint.to_string());
                out.push(ParamUser::Joint { joint, name: jname, input });
            }
        }
        let uses = |e: &str| expr::references(e).iter().any(|r| r == name);
        for r in &self.sheet.rules {
            for (field, _, _) in r.fields().into_iter().filter(|(_, e, _)| uses(e)) {
                out.push(ParamUser::Rule { kind: "sheet metal".into(), name: r.name.clone(), field: field.into() });
            }
        }
        for r in &self.plastic.rules {
            for (field, _, _) in r.fields().into_iter().filter(|(_, e, _)| uses(e)) {
                out.push(ParamUser::Rule { kind: "plastic".into(), name: r.name.clone(), field: field.into() });
            }
        }
        for c in &self.canvases {
            c.plane.walk_exprs(&mut |(label, e, _, _)| {
                if uses(e) {
                    out.push(ParamUser::Canvas { canvas: c.id, name: c.name.clone(), input: label.into() });
                }
            });
        }
        out
    }

    /// Remove a stored parameter; refused while something uses it (the error lists the users).
    pub fn remove_param(&mut self, name: &str) -> Result<()> {
        if self.text_param(name).is_some() {
            return self.remove_text_param(name);
        }
        if self.param(name).is_none() {
            if self.features.iter().any(|f| f.param_names.iter().any(|n| n == name)) {
                return Err(DocError::Invalid(format!("`{name}` is a feature's input: delete or edit the feature instead")));
            }
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        let users = self.param_users(name);
        if !users.is_empty() {
            let list: Vec<String> = users.iter().map(ParamUser::describe).collect();
            return Err(DocError::Invalid(format!("`{name}` is used by {}", list.join(", "))));
        }
        self.params.retain(|p| p.name != name);
        self.favorites.remove(name);
        Ok(())
    }

    /// Rename a parameter (stored or a feature input); every expression, feature input,
    /// dimension, favourite and comment that refers to it follows. Returns how many
    /// references were updated.
    pub fn rename_param(&mut self, old: &str, new: &str) -> Result<usize> {
        let new = new.trim();
        if !self.name_taken(old) {
            return Err(DocError::Unknown(format!("parameter `{old}`")));
        }
        if old == new {
            return Ok(0);
        }
        if let Some(p) = Self::param_name_problem(new) {
            return Err(DocError::Invalid(p));
        }
        if self.name_taken(new) {
            return Err(DocError::Invalid(format!("there is already a parameter `{new}`")));
        }
        let mut n = 0;
        let text = self.text_param(old).is_some();
        for p in &mut self.text_params {
            if p.name == old {
                p.name = new.to_string();
            }
        }
        for p in &mut self.params {
            if p.name == old {
                p.name = new.to_string();
            }
            let r = expr::rename_reference(&p.expr, old, new);
            if r != p.expr {
                p.expr = r;
                n += 1;
            }
        }
        for f in &mut self.features {
            for x in &mut f.param_names {
                if x == old {
                    *x = new.to_string();
                }
            }
            for e in f.kind.placement_exprs_mut() {
                let r = expr::rename_reference(e, old, new);
                if r != *e {
                    *e = r.into();
                    n += 1;
                }
            }
            for (_, e, _) in f.kind.inputs_mut() {
                let r = expr::rename_reference(e, old, new);
                if r != *e {
                    *e = r.into();
                    n += 1;
                }
            }
            if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                for c in &mut sketch.constraints {
                    if c.param.as_deref() == Some(old) {
                        c.param = Some(new.to_string());
                        n += 1;
                    }
                }
                for e in sketch.patterns.iter_mut().flat_map(|p| p.count_expression.iter_mut().chain(p.expressions.values_mut())) {
                    let r = expr::rename_reference(e, old, new);
                    if r != *e {
                        *e = r;
                        n += 1;
                    }
                }
                for e in sketch.text_exprs_mut() {
                    let r = expr::rename_reference(e, old, new);
                    if r != *e {
                        *e = r;
                        n += 1;
                    }
                }
                // An equation curve's variable hides a parameter of its name: such a parameter is
                // not what its formulas refer to.
                for (variable, e) in sketch.equation_exprs_mut() {
                    if variable == old {
                        continue;
                    }
                    let r = expr::rename_reference(e, old, new);
                    if r != *e {
                        *e = r;
                        n += 1;
                    }
                }
                for e in sketch.text_word_exprs_mut() {
                    let r = crate::text_params::rename_reference(e, old, new);
                    if r != *e {
                        *e = r;
                        n += 1;
                    }
                }
            }
        }
        if self.favorites.remove(old) {
            self.favorites.insert(new.to_string());
        }
        for c in &mut self.configs.columns {
            if *c == crate::config::Column::Param(old.to_string()) {
                *c = crate::config::Column::Param(new.to_string());
            }
        }
        // Configuration cells refer to parameters too.
        for rule in &mut self.configs.rules {
            rule.rename_parameter(old, new);
        }
        // A text parameter's cells are texts, not expressions.
        let texts: BTreeSet<String> = self.text_params.iter().map(|p| p.name.clone()).collect();
        for r in &mut self.configs.rows {
            for (column, cell) in self.configs.columns.iter().zip(&mut r.cells) {
                if !text && matches!(column, crate::config::Column::Param(c) if !texts.contains(c)) {
                    *cell = expr::rename_reference(cell, old, new);
                }
            }
        }
        for e in self.assembly.exprs_mut() {
            let r = expr::rename_reference(e, old, new);
            if r != *e {
                *e = r;
                n += 1;
            }
        }
        // Sheet metal and plastic rules and canvas planes hold expressions too (#4420).
        let rules = self.sheet.rules.iter_mut().flat_map(|r| r.fields_mut()).chain(self.plastic.rules.iter_mut().flat_map(|r| r.fields_mut()));
        for (_, e) in rules {
            let r = expr::rename_reference(e, old, new);
            if r != *e {
                *e = r;
                n += 1;
            }
        }
        for c in &mut self.canvases {
            c.plane.walk_exprs_mut(&mut |(_, e, _, _)| {
                let r = expr::rename_reference(e, old, new);
                if r != *e {
                    *e = r.into();
                    n += 1;
                }
            });
        }
        if let Some(c) = self.param_comments.remove(old) {
            self.param_comments.insert(new.to_string(), c);
        }
        Ok(n)
    }

    /// Set a parameter's comment (stored parameters keep it, feature inputs in `param_comments`).
    pub fn set_param_comment(&mut self, name: &str, comment: &str) -> Result<()> {
        if comment.len() > 4096 {
            return Err(DocError::Invalid("comment too long".into()));
        }
        if let Some(p) = self.params.iter_mut().find(|p| p.name == name) {
            p.comment = comment.to_string();
            return Ok(());
        }
        if let Some(p) = self.text_params.iter_mut().find(|p| p.name == name) {
            p.comment = comment.to_string();
            return Ok(());
        }
        if !self.name_taken(name) {
            return Err(DocError::Unknown(format!("parameter `{name}`")));
        }
        if comment.is_empty() {
            self.param_comments.remove(name);
        } else {
            self.param_comments.insert(name.to_string(), comment.to_string());
        }
        Ok(())
    }

    /// Dependency edges: (user, parameter used). Users are parameter names, or
    /// `Feature:<name>` for a feature whose inputs or dimensions use the parameter.
    pub fn param_graph(&self) -> Vec<(String, String)> {
        let mut out = expr::param_edges(&self.param_defs());
        for f in &self.features {
            let mut names: BTreeSet<String> = f.param_names.iter().filter(|n| !n.is_empty()).cloned().collect();
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                names.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
                names.extend(sketch.patterns.iter().flat_map(|p| p.expressions.values()).flat_map(|e| expr::references(e)));
                names.extend(sketch.text_exprs().into_iter().flat_map(|(_, e)| expr::references(e)));
                names.extend(sketch.patterns.iter().filter_map(|p| p.count_expression.as_deref()).flat_map(expr::references));
                names.extend(sketch.text_word_exprs().into_iter().flat_map(|e| crate::text_params::references(e)));
            }
            // Sheet metal and plastic features rebuild when their rules' parameters change (#4420).
            if f.kind.is_sheet() {
                names.extend(self.sheet.rules.iter().flat_map(|r| r.fields()).flat_map(|(_, e, _)| expr::references(e)));
            }
            if f.kind.is_plastic() {
                names.extend(self.plastic.rules.iter().flat_map(|r| r.fields()).flat_map(|(_, e, _)| expr::references(e)));
            }
            out.extend(names.into_iter().map(|n| (format!("Feature:{}", f.name), n)));
        }
        out
    }

    /// Features whose result depends on a parameter (directly or through other parameters).
    pub fn features_using_param(&self, name: &str) -> Vec<u64> {
        let defs = self.param_defs();
        let mut affected: BTreeSet<String> = BTreeSet::from([name.to_string()]);
        for _ in 0..defs.len().min(MAX_PARAMS * 2) {
            let before = affected.len();
            for d in &defs {
                if expr::references(&d.expr).iter().any(|r| affected.contains(r)) {
                    affected.insert(d.name.clone());
                }
            }
            if affected.len() == before {
                break;
            }
        }
        // A rule that uses it changes every sheet metal (or plastic) feature: the evaluation
        // hashes the rules into their fingerprint.
        let uses = |e: &String| expr::references(e).iter().any(|r| affected.contains(r));
        let sheet = self.sheet.rules.iter().any(|r| r.fields().iter().any(|(_, e, _)| uses(e)));
        let plastic = self.plastic.rules.iter().any(|r| r.fields().iter().any(|(_, e, _)| uses(e)));
        let mut out = Vec::new();
        for f in &self.features {
            let mut hit = f.param_names.iter().any(|n| affected.contains(n));
            hit |= (sheet && f.kind.is_sheet()) || (plastic && f.kind.is_plastic());
            hit |= f.kind.expressions().iter().any(|e| expr::references(e).iter().any(|r| affected.contains(r)));
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                hit |= sketch.constraints.iter().any(|c| c.param.as_ref().is_some_and(|p| affected.contains(p)));
                hit |= sketch.patterns.iter().flat_map(|p| p.expressions.values()).any(|e| expr::references(e).iter().any(|r| affected.contains(r)));
                hit |= sketch.text_exprs().iter().any(|(_, e)| expr::references(e).iter().any(|r| affected.contains(r)));
                hit |= sketch.text_word_exprs().iter().any(|e| crate::text_params::references(e).iter().any(|r| affected.contains(r)));
            }
            if hit {
                out.push(f.id);
            }
        }
        out
    }

    /// Add a user parameter (refused when the name exists).
    pub fn add_user_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        if self.name_taken(name.trim()) {
            return Err(DocError::Invalid(format!("there is already a parameter `{}`", name.trim())));
        }
        if self.params.len() >= MAX_PARAMS {
            return Err(DocError::Invalid("too many parameters".into()));
        }
        self.set_param(name.trim(), expr_s, unit, comment)
    }
}

/// One CSV line with quoted fields.
fn parse_csv_line(l: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = l.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            (c, _) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Parameters whose definitions depend on each other in a loop.
pub fn cycles(list: &[(String, String, Kind)]) -> BTreeSet<String> {
    let refs: BTreeMap<&str, Vec<String>> = list.iter().map(|(n, e, _)| (n.as_str(), expr::references(e))).collect();
    let mut bad = BTreeSet::new();
    for (start, _, _) in list {
        // Depth-first from `start`; reaching it again is a cycle.
        let mut stack: Vec<(String, usize)> = refs.get(start.as_str()).map(|r| r.iter().map(|x| (x.clone(), 1)).collect()).unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some((n, d)) = stack.pop() {
            if &n == start {
                bad.insert(start.clone());
                break;
            }
            if d > 10_000 || !seen.insert(n.clone()) {
                continue;
            }
            if let Some(r) = refs.get(n.as_str()) {
                stack.extend(r.iter().map(|x| (x.clone(), d + 1)));
            }
        }
    }
    bad
}

/// The numbers `n` for which `{base}{n}` is among `names` (written as `n` is: no sign or
/// leading zeros).
pub(crate) fn numbers_after<'a>(base: &str, names: impl Iterator<Item = &'a str>) -> std::collections::HashSet<u64> {
    names
        .filter_map(|name| name.strip_prefix(base))
        .filter(|digits| !digits.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|digits| digits.parse().ok())
        .collect()
}

#[cfg(test)]
mod design_length_tests {
    use crate::Document;

    fn written(units: &str, mm: f64) -> (String, String) {
        let mut doc = Document::new("Units");
        doc.units = units.into();
        doc.design_length(mm)
    }

    #[test]
    fn a_measured_length_is_written_in_the_design_units() {
        let w = |u: &str, e: &str| (u.to_string(), e.to_string());
        assert_eq!(written("in", 25.4), w("in", "1 in"));
        assert_eq!(written("in", 12.7), w("in", "0.5 in"));
        assert_eq!(written("in", 10.0), w("in", "0.39370079 in"));
        assert_eq!(written("cm", 12.34), w("cm", "1.234 cm"));
        assert_eq!(written("mm", 12.3456789), w("mm", "12.345679 mm"));
        assert_eq!(written("in", -25.4), w("in", "-1 in"));
        assert_eq!(written("in", -1e-12), w("in", "0 in"));
        // Unknown or non-length units keep millimetres.
        assert_eq!(written("parsec", 5.0), w("mm", "5 mm"));
        assert_eq!(written("deg", 5.0), w("mm", "5 mm"));
        assert_eq!(written("", 5.0), w("mm", "5 mm"));
    }

    /// A bare number typed as a formula (`=5`) means the design's units like `5` does.
    #[test]
    fn a_formula_that_is_a_bare_number_gets_the_design_unit() {
        let mut doc = Document::new("Units");
        doc.units = "in".into();
        let length = crate::expr::Kind::Length;
        assert_eq!(doc.with_design_unit("5", length), "5 in");
        assert_eq!(doc.with_design_unit("=5", length), "=5 in");
        assert_eq!(doc.with_design_unit("=5 mm", length), "=5 mm");
        let inches = doc.eval(&doc.with_design_unit("=5", length), length).unwrap();
        assert!((inches - 127.0).abs() < 1e-9, "{inches}");
    }

    #[test]
    fn the_written_value_reads_back_within_a_micrometre() {
        for units in ["mm", "cm", "m", "in", "ft"] {
            let mut doc = Document::new("Units");
            doc.units = units.into();
            for mm in [0.001, 1.0, 12.345_678_9, 25.4, 999.999, 12_345.678] {
                let (_, e) = doc.design_length(mm);
                let back = doc.eval(&e, crate::expr::Kind::Length).unwrap();
                assert!((back - mm).abs() <= 1e-6, "{units} {mm}: {e} reads {back}");
            }
        }
    }
}
