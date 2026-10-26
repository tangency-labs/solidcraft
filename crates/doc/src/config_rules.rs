//! Bounded typed configuration rules. The saved row remains the editable source;
//! ordered rules derive an effective design without overwriting its cells.
use super::Column;
use crate::{DocError, Document, Result, expr};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Most configuration rules a design keeps. A rule drives one feature's suppression, so an
/// assembly that switches a few hundred parts on and off, by two rules each, needs hundreds.
pub const MAX_RULES: usize = 2048;
const MAX_NODES: usize = 64;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRule {
    pub id: String,
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Unit context for feature-length expressions, fixed when authoring a rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_unit: Option<String>,
    pub condition: RuleCondition,
    pub effect: RuleEffect,
}
fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleCondition {
    Always,
    All { conditions: Vec<RuleCondition> },
    Any { conditions: Vec<RuleCondition> },
    Not { condition: Box<RuleCondition> },
    Parameter { parameter: String, operator: RuleOperator, expression: String },
    Suppression { feature: u64, value: bool },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOperator {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleEffect {
    DriveParameter { parameter: String, expression: String },
    DriveSuppression { feature: u64, value: bool },
    ParameterRange { parameter: String, minimum: String, maximum: String },
    AllowParameterValues { parameter: String, expressions: Vec<String> },
}

#[derive(Clone, Debug, Serialize)]
pub struct RuleTrace {
    pub id: String,
    pub name: String,
    pub matched: bool,
    pub effect: RuleEffect,
}

/// Check saved rule structure before recursive serde decoding. Absence is backward
/// compatible; malformed present data is refused. Runtime evaluation checks it again.
pub fn validate_rules_json(document: &Value) -> Result<()> {
    let Some(rules) = document.get("configs").and_then(|configs| configs.get("rules")) else { return Ok(()) };
    let rules = rules.as_array().ok_or_else(|| invalid("rules must be an array"))?;
    if rules.len() > MAX_RULES {
        return Err(invalid("too many rules"));
    }
    for rule in rules {
        let rule = rule.as_object().ok_or_else(|| invalid("rule must be an object"))?;
        for (key, limit) in [("id", 64), ("name", 128)] {
            let value = rule.get(key).and_then(Value::as_str).ok_or_else(|| invalid("rule id and name are required"))?;
            if value.trim().is_empty() || value.chars().count() > limit {
                return Err(invalid("rule id or name is empty or too long"));
            }
        }
        if let Some(unit) = rule.get("length_unit") {
            let unit =
                unit.as_str().filter(|unit| matches!(*unit, "mm" | "cm" | "m" | "in" | "ft")).ok_or_else(|| invalid("invalid rule length unit"))?;
            let _ = unit;
        }
        let mut pending = vec![(rule.get("condition").ok_or_else(|| invalid("condition required"))?, 1usize)];
        let mut count = 0;
        while let Some((condition, depth)) = pending.pop() {
            count += 1;
            if depth > MAX_DEPTH || count > MAX_NODES {
                return Err(invalid("condition nesting or node limit exceeded"));
            }
            let kind = condition.get("type").and_then(Value::as_str).ok_or_else(|| invalid("condition type required"))?;
            match kind {
                "all" | "any" => {
                    let children = condition.get("conditions").and_then(Value::as_array).ok_or_else(|| invalid("conditions must be an array"))?;
                    if children.is_empty() || children.len() > MAX_NODES {
                        return Err(invalid("condition group is empty or too large"));
                    }
                    pending.extend(children.iter().map(|child| (child, depth + 1)));
                }
                "not" => pending.push((condition.get("condition").ok_or_else(|| invalid("nested condition required"))?, depth + 1)),
                "parameter" => {
                    check_parameter(condition.get("parameter"))?;
                    check_expression(condition.get("expression"))?;
                }
                "always" | "suppression" => {}
                _ => return Err(invalid("unknown condition type")),
            }
        }
        let effect = rule.get("effect").ok_or_else(|| invalid("effect required"))?;
        if effect.get("type").and_then(Value::as_str) != Some("drive_suppression") {
            check_parameter(effect.get("parameter"))?;
        }
        match effect.get("type").and_then(Value::as_str) {
            Some("drive_parameter") => check_expression(effect.get("expression"))?,
            Some("parameter_range") => {
                check_expression(effect.get("minimum"))?;
                check_expression(effect.get("maximum"))?;
            }
            Some("allow_parameter_values") => {
                let expressions = effect.get("expressions").and_then(Value::as_array).ok_or_else(|| invalid("allowed values must be an array"))?;
                if expressions.is_empty() || expressions.len() > 128 {
                    return Err(invalid("allowed values are empty or too many"));
                }
                for expression in expressions {
                    check_expression(Some(expression))?;
                }
            }
            Some("drive_suppression") => {}
            _ => return Err(invalid("unknown effect type")),
        }
    }
    Ok(())
}
fn check_expression(value: Option<&Value>) -> Result<()> {
    let text = value.and_then(Value::as_str).ok_or_else(|| invalid("expression must be text"))?;
    if text.trim().is_empty() || text.len() > 4096 {
        return Err(invalid("expression is empty or too long"));
    }
    Ok(())
}
/// A rule's parameter is name-shaped and short. Whether the name is reserved is not checked here:
/// an older design's parameter can be named after what is now a unit, and is renamed only after
/// these checks, as the design opens. A rule naming no parameter fails when it is evaluated.
fn check_parameter(value: Option<&Value>) -> Result<()> {
    let name = value.and_then(Value::as_str).ok_or_else(|| invalid("parameter name required"))?;
    if name.len() > 64 || !expr::is_name_shaped(name) {
        return Err(invalid("invalid parameter name"));
    }
    Ok(())
}
fn invalid(message: &str) -> DocError {
    DocError::Invalid(format!("configuration rules: {message}"))
}

fn operands(
    doc: &Document,
    values: &BTreeMap<String, expr::Value>,
    parameter: &str,
    expression: &str,
    length_unit: Option<&str>,
) -> Result<(f64, f64)> {
    let lhs = values.get(parameter).copied().ok_or_else(|| invalid(&format!("parameter `{parameter}` is missing or invalid")))?;
    let unit = doc
        .param(parameter)
        .map(|p| p.unit.as_str())
        .or_else(|| lhs.kind().map(|kind| if kind == expr::Kind::Length { length_unit.unwrap_or(doc.units.as_str()) } else { kind.default_unit() }))
        .unwrap_or("");
    let defaults = expr::Defaults::for_unit(unit, expr::unit_info(&doc.units).map(|(_, scale)| scale).unwrap_or(1.0));
    let rhs = expr::eval_with_defaults(
        expression,
        &|name| values.get(name).copied().ok_or_else(|| invalid(&format!("unknown parameter `{name}`"))),
        defaults,
    )?;
    let kind = lhs.kind().ok_or_else(|| invalid("comparison requires a length, angle, mass, volume, area or unit-less value"))?;
    let right = rhs.to_kind_in(kind, defaults)?;
    if !lhs.v.is_finite() || !right.is_finite() {
        return Err(invalid("comparison requires finite values"));
    }
    Ok((lhs.v, right))
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}
fn validate_drive_graph(graph: &BTreeMap<String, Vec<String>>, parameter: &str, expression: &str) -> Result<()> {
    let mut pending = expr::references(expression);
    let mut visited = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if name == parameter {
            return Err(invalid(&format!("drive for `{parameter}` creates a parameter cycle")));
        }
        if visited.insert(name.clone())
            && let Some(references) = graph.get(&name)
        {
            pending.extend(references.iter().cloned());
        }
    }
    Ok(())
}
fn condition(doc: &Document, values: &BTreeMap<String, expr::Value>, value: &RuleCondition, length_unit: Option<&str>) -> Result<bool> {
    match value {
        RuleCondition::Always => Ok(true),
        RuleCondition::All { conditions } => {
            for c in conditions {
                if !condition(doc, values, c, length_unit)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        RuleCondition::Any { conditions } => {
            for c in conditions {
                if condition(doc, values, c, length_unit)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        RuleCondition::Not { condition: c } => Ok(!condition(doc, values, c, length_unit)?),
        RuleCondition::Suppression { feature, value } => {
            Ok(doc.feature(*feature).ok_or_else(|| invalid("condition feature is gone"))?.suppressed == *value)
        }
        RuleCondition::Parameter { parameter, operator, expression } => {
            let (a, b) = operands(doc, values, parameter, expression, length_unit)?;
            Ok(match operator {
                RuleOperator::Eq => close(a, b),
                RuleOperator::Ne => !close(a, b),
                RuleOperator::Lt => a < b,
                RuleOperator::Le => a <= b || close(a, b),
                RuleOperator::Gt => a > b,
                RuleOperator::Ge => a >= b || close(a, b),
            })
        }
    }
}

/// The features whose suppression a condition reads.
fn suppression_reads(c: &RuleCondition) -> Vec<u64> {
    match c {
        RuleCondition::Always | RuleCondition::Parameter { .. } => Vec::new(),
        RuleCondition::All { conditions } | RuleCondition::Any { conditions } => conditions.iter().flat_map(suppression_reads).collect(),
        RuleCondition::Not { condition } => suppression_reads(condition),
        RuleCondition::Suppression { feature, .. } => vec![*feature],
    }
}

impl Document {
    /// After a plain parameter edit (`before`: the parameter values before it): the enabled rules
    /// that switch a feature's suppression and whose condition reads a parameter whose value
    /// changed, or the suppression of a feature switched earlier in this pass, applied in their
    /// order as activation applies them. A switch parameter alone so switches its features.
    /// Driven parameters and restrictions still wait for a configuration edit or activation, and
    /// a rule whose condition reads nothing that changed leaves its feature as it is. A rule that
    /// cannot be evaluated (a missing parameter) is skipped: it never blocks the edit.
    /// Returns the features switched, with their new suppression.
    pub fn apply_suppression_rules(&mut self, before: &BTreeMap<String, expr::Value>) -> Vec<(u64, bool)> {
        if !self.configs.rules.iter().any(|r| r.enabled && matches!(r.effect, RuleEffect::DriveSuppression { .. })) {
            return Vec::new();
        }
        let (values, _) = self.param_values();
        let changed: BTreeSet<&String> = values
            .iter()
            .filter(|(name, v)| before.get(*name).is_none_or(|b| !close(b.v, v.v) || b.len != v.len || b.ang != v.ang))
            .map(|(name, _)| name)
            .chain(before.keys().filter(|name| !values.contains_key(*name)))
            .collect();
        if changed.is_empty() {
            return Vec::new();
        }
        let mut switched: Vec<(u64, bool)> = Vec::new();
        for rule in self.configs.rules.clone() {
            let RuleEffect::DriveSuppression { feature, value } = rule.effect else { continue };
            if !rule.enabled {
                continue;
            }
            let reads = rule.parameter_references().iter().any(|n| changed.contains(n))
                || suppression_reads(&rule.condition).iter().any(|f| switched.iter().any(|(s, _)| s == f));
            if !reads || !condition(self, &values, &rule.condition, rule.length_unit.as_deref()).unwrap_or(false) {
                continue;
            }
            if let Some(f) = self.feature_mut(feature)
                && f.suppressed != value
            {
                f.suppressed = value;
                switched.retain(|(s, _)| *s != feature);
                switched.push((feature, value));
            }
        }
        switched
    }

    /// Apply ordered drives, then enforce all matched restrictions on the final state.
    /// Running on a scratch document makes a failing rule fully atomic for callers.
    pub(super) fn evaluate_configuration_rules(&mut self) -> Result<Vec<RuleTrace>> {
        validate_rules_json(&serde_json::json!({"configs":{"rules":self.configs.rules}}))?;
        let mut ids = BTreeSet::new();
        let mut trace = Vec::new();
        let mut restrictions = Vec::new();
        let (mut values, _) = self.param_values();
        let parameter_names: BTreeSet<String> = self.all_param_exprs().into_iter().map(|(name, _, _)| name).collect();
        let mut dependency_graph: BTreeMap<String, Vec<String>> =
            self.all_param_exprs().into_iter().map(|(name, expression, _)| (name, expr::references(&expression))).collect();
        for rule in self.configs.rules.clone() {
            if !ids.insert(rule.id.clone()) {
                return Err(invalid("duplicate rule id"));
            }
            if let Some(name) = rule.parameter_references().iter().find(|name| !parameter_names.contains(*name)) {
                return Err(invalid(&format!("rule `{}` references missing parameter `{name}`", rule.name)));
            }
            // Targets must remain configured, including disabled rules, so dormant rules
            // cannot silently become invalid and surprise a subsequent activation.
            let target = match &rule.effect {
                RuleEffect::DriveParameter { parameter, .. }
                | RuleEffect::ParameterRange { parameter, .. }
                | RuleEffect::AllowParameterValues { parameter, .. } => Column::Param(parameter.clone()),
                RuleEffect::DriveSuppression { feature, .. } => Column::Suppress(*feature),
            };
            if !self.configs.columns.contains(&target) {
                return Err(invalid(&format!("rule `{}` target is not configured", rule.name)));
            }
            let matched = rule.enabled
                && condition(self, &values, &rule.condition, rule.length_unit.as_deref())
                    .map_err(|e| invalid(&format!("rule `{}`: {e}", rule.name)))?;
            trace.push(RuleTrace { id: rule.id, name: rule.name.clone(), matched, effect: rule.effect.clone() });
            if !matched {
                continue;
            }
            match rule.effect {
                RuleEffect::DriveParameter { parameter, mut expression } => {
                    validate_drive_graph(&dependency_graph, &parameter, &expression)?;
                    dependency_graph.insert(parameter.clone(), expr::references(&expression));
                    if self.param(&parameter).is_none()
                        && rule.length_unit.as_deref().is_some_and(|unit| unit != self.units)
                        && values.get(&parameter).is_some_and(|value| value.kind() == Some(expr::Kind::Length))
                    {
                        let (_, value) = operands(self, &values, &parameter, &expression, rule.length_unit.as_deref())?;
                        expression = format!("{value} mm");
                    }
                    self.change_params_atomic(&[(parameter, expression)]).map_err(|e| invalid(&format!("rule `{}`: {e}", rule.name)))?;
                    values = self.param_values().0;
                }
                RuleEffect::DriveSuppression { feature, value } => {
                    self.feature_mut(feature).ok_or_else(|| invalid("rule feature is gone"))?.suppressed = value
                }
                effect => restrictions.push((rule.name, effect, rule.length_unit)),
            }
        }
        for (name, effect, length_unit) in restrictions {
            let valid = match effect {
                RuleEffect::ParameterRange { parameter, minimum, maximum } => {
                    let (value, min) = operands(self, &values, &parameter, &minimum, length_unit.as_deref())?;
                    let (_, max) = operands(self, &values, &parameter, &maximum, length_unit.as_deref())?;
                    min <= max && (value >= min || close(value, min)) && (value <= max || close(value, max))
                }
                RuleEffect::AllowParameterValues { parameter, expressions } => {
                    let mut valid = false;
                    for expression in expressions {
                        let (a, b) = operands(self, &values, &parameter, &expression, length_unit.as_deref())?;
                        valid |= close(a, b);
                    }
                    valid
                }
                _ => true,
            };
            if !valid {
                return Err(invalid(&format!("rule `{name}` restricts the final value")));
            }
        }
        Ok(trace)
    }
}

impl ConfigRule {
    pub(super) fn preserve_length_units(&mut self, parameters: &BTreeSet<String>, unit: &str) {
        if self.length_unit.is_none() {
            self.length_unit = Some(unit.to_string());
        }
        fn annotate(parameter: &str, expression: &mut String, parameters: &BTreeSet<String>, unit: &str) {
            if parameters.contains(parameter) && expression.trim().parse::<f64>().is_ok() {
                *expression = format!("{} {unit}", expression.trim());
            }
        }
        fn walk(condition: &mut RuleCondition, parameters: &BTreeSet<String>, unit: &str) {
            match condition {
                RuleCondition::Parameter { parameter, expression, .. } => annotate(parameter, expression, parameters, unit),
                RuleCondition::All { conditions } | RuleCondition::Any { conditions } => {
                    for condition in conditions {
                        walk(condition, parameters, unit);
                    }
                }
                RuleCondition::Not { condition } => walk(condition, parameters, unit),
                _ => {}
            }
        }
        walk(&mut self.condition, parameters, unit);
        match &mut self.effect {
            RuleEffect::DriveParameter { parameter, expression } => annotate(parameter, expression, parameters, unit),
            RuleEffect::ParameterRange { parameter, minimum, maximum } => {
                annotate(parameter, minimum, parameters, unit);
                annotate(parameter, maximum, parameters, unit);
            }
            RuleEffect::AllowParameterValues { parameter, expressions } => {
                for expression in expressions {
                    annotate(parameter, expression, parameters, unit);
                }
            }
            _ => {}
        }
    }

    /// Parameter references include targets and condition/effect expressions.
    pub fn parameter_references(&self) -> Vec<String> {
        let mut copy = self.clone();
        let mut references = Vec::new();
        copy.visit_parameters(&mut |text, expression| {
            if expression {
                references.extend(expr::references(text));
            } else {
                references.push(text.clone());
            }
        });
        references
    }
    /// Rename parameter `old` to `new`, renaming it in expressions with `rename`
    /// (`expr::rename_reference`, or `expr::rename_word` as an older build read them).
    pub fn rename_parameter(&mut self, old: &str, new: &str, rename: fn(&str, &str, &str) -> String) {
        self.visit_parameters(&mut |text, expression| {
            if expression {
                *text = rename(text, old, new);
            } else if text == old {
                *text = new.to_string();
            }
        });
    }
    pub(super) fn clone_for_targets(&self, maps: &super::ConfigCloneMaps) -> Option<Self> {
        let mapped = match &self.effect {
            RuleEffect::DriveParameter { parameter, .. }
            | RuleEffect::ParameterRange { parameter, .. }
            | RuleEffect::AllowParameterValues { parameter, .. } => maps.parameters.contains_key(parameter),
            RuleEffect::DriveSuppression { feature, .. } => maps.features.contains_key(feature),
        };
        if !mapped {
            return None;
        }
        let mut copy = self.clone();
        copy.visit_parameters(&mut |text, expression| {
            *text = if expression {
                super::clone::remap_expression(text, &maps.parameters)
            } else {
                maps.parameters.get(text).cloned().unwrap_or_else(|| text.clone())
            };
        });
        fn remap(condition: &mut RuleCondition, maps: &super::ConfigCloneMaps) {
            match condition {
                RuleCondition::Suppression { feature, .. } => {
                    *feature = maps.features.get(feature).copied().unwrap_or(*feature);
                }
                RuleCondition::All { conditions } | RuleCondition::Any { conditions } => {
                    for condition in conditions {
                        remap(condition, maps);
                    }
                }
                RuleCondition::Not { condition } => remap(condition, maps),
                _ => {}
            }
        }
        remap(&mut copy.condition, maps);
        if let RuleEffect::DriveSuppression { feature, .. } = &mut copy.effect {
            *feature = maps.features.get(feature).copied().unwrap_or(*feature);
        }
        Some(copy)
    }
    fn visit_parameters(&mut self, visit: &mut impl FnMut(&mut String, bool)) {
        fn walk(c: &mut RuleCondition, visit: &mut impl FnMut(&mut String, bool)) {
            match c {
                RuleCondition::All { conditions } | RuleCondition::Any { conditions } => {
                    for c in conditions {
                        walk(c, visit);
                    }
                }
                RuleCondition::Not { condition } => walk(condition, visit),
                RuleCondition::Parameter { parameter, expression, .. } => {
                    visit(parameter, false);
                    visit(expression, true);
                }
                _ => {}
            }
        }
        walk(&mut self.condition, visit);
        match &mut self.effect {
            RuleEffect::DriveParameter { parameter, expression } => {
                visit(parameter, false);
                visit(expression, true);
            }
            RuleEffect::ParameterRange { parameter, minimum, maximum } => {
                visit(parameter, false);
                visit(minimum, true);
                visit(maximum, true);
            }
            RuleEffect::AllowParameterValues { parameter, expressions } => {
                visit(parameter, false);
                for e in expressions {
                    visit(e, true);
                }
            }
            _ => {}
        }
    }
    pub fn references_features(&self, ids: &[u64]) -> bool {
        fn condition(c: &RuleCondition, ids: &[u64]) -> bool {
            match c {
                RuleCondition::Suppression { feature, .. } => ids.contains(feature),
                RuleCondition::All { conditions } | RuleCondition::Any { conditions } => conditions.iter().any(|c| condition(c, ids)),
                RuleCondition::Not { condition: c } => condition(c, ids),
                _ => false,
            }
        }
        condition(&self.condition, ids) || matches!(&self.effect,RuleEffect::DriveSuppression {feature,..} if ids.contains(feature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigRow;
    fn design() -> Document {
        let mut doc = Document::new("Rules");
        doc.set_param("width", "10 mm", None, None).unwrap();
        doc.set_param("depth", "5 mm", None, None).unwrap();
        doc.configs.columns = vec![Column::Param("width".into()), Column::Param("depth".into())];
        doc.configs.rows.push(ConfigRow { name: "Default".into(), cells: vec!["10 mm".into(), "5 mm".into()] });
        doc
    }
    fn rule(id: &str, condition: RuleCondition, effect: RuleEffect) -> ConfigRule {
        ConfigRule { id: id.into(), name: id.into(), enabled: true, length_unit: None, condition, effect }
    }
    #[test]
    fn ordered_rules_derive_values_without_overwriting_rows() {
        let mut doc = design();
        doc.configs.rules = vec![
            rule("first", RuleCondition::Always, RuleEffect::DriveParameter { parameter: "width".into(), expression: "1 in".into() }),
            rule(
                "second",
                RuleCondition::Parameter { parameter: "width".into(), operator: RuleOperator::Gt, expression: "20 mm".into() },
                RuleEffect::DriveParameter { parameter: "depth".into(), expression: "width / 2".into() },
            ),
        ];
        let source = doc.configs.rows.clone();
        let trace = doc.apply_configuration_with_trace("Default").unwrap();
        assert!(trace.iter().all(|entry| entry.matched));
        assert!((doc.param_values().0["depth"].v - 12.7).abs() < 1e-9);
        assert_eq!(doc.configs.rows, source);
        assert!(doc.configuration_matches());
        doc.configs.rules.swap(0, 1);
        let trace = doc.apply_configuration_with_trace("Default").unwrap();
        assert!(!trace[0].matched);
        assert_eq!(doc.param_values().0["depth"].v, 5.0);
    }
    #[test]
    fn restrictions_intersect_and_fail_atomically_with_named_rule() {
        let mut doc = design();
        doc.configs.rules = vec![
            rule(
                "range",
                RuleCondition::Always,
                RuleEffect::ParameterRange { parameter: "width".into(), minimum: "5 mm".into(), maximum: "1 in".into() },
            ),
            rule(
                "choices",
                RuleCondition::Always,
                RuleEffect::AllowParameterValues { parameter: "width".into(), expressions: vec!["1 cm".into(), "20 mm".into()] },
            ),
        ];
        doc.apply_configuration("Default").unwrap();
        doc.configs.rows[0].cells[0] = "15 mm".into();
        let before = doc.clone();
        assert!(doc.apply_configuration("Default").unwrap_err().to_string().contains("choices"));
        assert_eq!(doc, before);
        doc.configs.rules[0].effect = RuleEffect::ParameterRange { parameter: "width".into(), minimum: "1 deg".into(), maximum: "90 deg".into() };
        assert!(doc.apply_configuration("Default").is_err());
    }
    #[test]
    fn rule_references_follow_rename_and_prevent_delete() {
        let mut doc = design();
        doc.configs.rules.push(rule(
            "derive",
            RuleCondition::Parameter { parameter: "width".into(), operator: RuleOperator::Ge, expression: "depth".into() },
            RuleEffect::DriveParameter { parameter: "depth".into(), expression: "width / 2".into() },
        ));
        assert!(doc.remove_param("width").is_err());
        doc.rename_param("width", "span").unwrap();
        let refs = doc.configs.rules[0].parameter_references();
        assert!(refs.contains(&"span".into()));
        assert!(!refs.contains(&"width".into()));
        doc.apply_configuration("Default").unwrap();
        assert_eq!(doc.param_values().0["depth"].v, 5.0);
    }
    #[test]
    fn angle_and_unitless_comparisons_keep_their_dimensions() {
        let mut doc = design();
        doc.set_param("angle", "45 deg", Some("deg"), None).unwrap();
        doc.set_param("count", "1", Some(""), None).unwrap();
        doc.configs.columns.push(Column::Param("count".into()));
        doc.configs.rows[0].cells.push("1".into());
        doc.configs.rules.push(rule(
            "angle-drive",
            RuleCondition::Parameter { parameter: "angle".into(), operator: RuleOperator::Gt, expression: "1".into() },
            RuleEffect::DriveParameter { parameter: "count".into(), expression: "2".into() },
        ));
        let trace = doc.apply_configuration_with_trace("Default").unwrap();
        assert!(trace[0].matched);
        assert_eq!(doc.param_values().0["count"].v, 2.0);
        doc.configs.rules[0].condition =
            RuleCondition::Parameter { parameter: "count".into(), operator: RuleOperator::Eq, expression: "1 deg".into() };
        let before = doc.clone();
        assert!(doc.apply_configuration("Default").is_err());
        assert_eq!(doc, before);
    }

    #[test]
    fn saved_rule_budgets_are_checked_before_deserialization() {
        let valid = serde_json::to_value(rule(
            "r",
            RuleCondition::Always,
            RuleEffect::DriveParameter { parameter: "width".into(), expression: "2 mm".into() },
        ))
        .unwrap();
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":[valid.clone()]}})).is_ok());
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":null}})).is_err());
        // An assembly that switches a few hundred parts on and off, by two rules each, keeps them.
        let many: Vec<Value> = (0..600)
            .map(|i| {
                let mut r = valid.clone();
                r["id"] = serde_json::json!(format!("r{i}"));
                r
            })
            .collect();
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":many}})).is_ok());
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":vec![valid.clone();MAX_RULES + 1]}})).is_err());
        let mut condition = serde_json::json!({"type":"always"});
        for _ in 0..8 {
            condition = serde_json::json!({"type":"not","condition":condition});
        }
        let mut deep = valid.clone();
        deep["condition"] = condition;
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":[deep]}})).is_err());
        let mut wide = valid.clone();
        wide["condition"] = serde_json::json!({"type":"all","conditions":vec![serde_json::json!({"type":"always"});64]});
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":[wide]}})).is_err());
        let mut long = valid.clone();
        long["effect"]["expression"] = serde_json::json!("a".repeat(4097));
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":[long]}})).is_err());
        let mut malformed = valid.clone();
        malformed["condition"] = serde_json::json!({"type":"any","conditions":{}});
        assert!(validate_rules_json(&serde_json::json!({"configs":{"rules":[malformed]}})).is_err());
        // An older design's parameter can be named after what is now a unit (#4811): the saved
        // rule's name passes, so the design opens and renames it. A misshapen name doesn't.
        for (name, ok) in [("g", true), ("mm", true), ("2w", false), ("", false), ("a b", false)] {
            let mut named = valid.clone();
            named["effect"]["parameter"] = serde_json::json!(name);
            assert_eq!(validate_rules_json(&serde_json::json!({"configs":{"rules":[named]}})).is_ok(), ok, "{name:?}");
        }
    }

    #[test]
    fn compound_conditions_short_circuit_guarded_numeric_expressions() {
        let mut doc = design();
        doc.set_param("count", "0", Some(""), None).unwrap();
        let guard = RuleCondition::Parameter { parameter: "count".into(), operator: RuleOperator::Gt, expression: "0".into() };
        let invalid_when_zero = RuleCondition::Parameter { parameter: "count".into(), operator: RuleOperator::Gt, expression: "1 / count".into() };
        doc.configs.rules = vec![rule(
            "guarded",
            RuleCondition::All { conditions: vec![guard, invalid_when_zero.clone()] },
            RuleEffect::DriveParameter { parameter: "width".into(), expression: "20 mm".into() },
        )];
        assert!(!doc.apply_configuration_with_trace("Default").unwrap()[0].matched);
        assert_eq!(doc.param_values().0["width"].v, 10.0);
        doc.configs.rules[0].condition = RuleCondition::Any {
            conditions: vec![
                RuleCondition::Parameter { parameter: "count".into(), operator: RuleOperator::Eq, expression: "0".into() },
                invalid_when_zero,
            ],
        };
        assert!(doc.apply_configuration_with_trace("Default").unwrap()[0].matched);
        assert_eq!(doc.param_values().0["width"].v, 20.0);
    }

    #[test]
    fn saved_documents_reject_malformed_deep_and_oversized_rules_before_typed_decode() {
        let valid = serde_json::to_value(rule(
            "r",
            RuleCondition::Always,
            RuleEffect::DriveParameter { parameter: "width".into(), expression: "2 mm".into() },
        ))
        .unwrap();
        let mut deep = valid.clone();
        let mut condition = serde_json::json!({"type":"always"});
        for _ in 0..8 {
            condition = serde_json::json!({"type":"not","condition":condition});
        }
        deep["condition"] = condition;
        let mut wide = valid.clone();
        wide["condition"] = serde_json::json!({"type":"all","conditions":vec![serde_json::json!({"type":"always"});64]});
        let mut invalid_context = valid.clone();
        invalid_context["length_unit"] = serde_json::json!("mile".repeat(1000));
        for invalid in [
            Value::Null,
            serde_json::json!([deep]),
            serde_json::json!([wide]),
            serde_json::json!(vec![valid; MAX_RULES + 1]),
            serde_json::json!([invalid_context]),
        ] {
            let mut saved = serde_json::to_value(design()).unwrap();
            saved["configs"]["rules"] = invalid;
            let error = crate::format::read(&saved.to_string()).unwrap_err().to_string();
            assert!(error.contains("configuration rules"), "saved boundary must report bounded rule validation: {error}");
        }
    }

    #[test]
    fn cross_unit_effective_literals_keep_declared_drive_cycle_checks() {
        let mut doc = Document::new("Cross-unit drive graph");
        doc.add_feature(
            crate::FeatureKind::Box {
                corner: ["0".into(), "0".into(), "0".into()],
                length: "10 mm".into(),
                width: "5 mm".into(),
                height: "10 mm".into(),
                operation: crate::Operation::NewBody,
            },
            None,
        )
        .unwrap();
        let width = doc.features[0].param_names[0].clone();
        let depth = doc.features[0].param_names[1].clone();
        doc.rename_param(&width, "width").unwrap();
        doc.rename_param(&depth, "depth").unwrap();
        doc.configs.columns = vec![Column::Param("width".into()), Column::Param("depth".into())];
        doc.configs.rows.push(ConfigRow { name: "Default".into(), cells: vec!["10 mm".into(), "5 mm".into()] });
        doc.units = "in".into();
        let mut first =
            rule("first", RuleCondition::Always, RuleEffect::DriveParameter { parameter: "width".into(), expression: "depth + 1".into() });
        first.length_unit = Some("mm".into());
        let mut second =
            rule("second", RuleCondition::Always, RuleEffect::DriveParameter { parameter: "depth".into(), expression: "width + 1".into() });
        second.length_unit = Some("mm".into());
        doc.configs.rules = vec![first];
        doc.apply_configuration("Default").unwrap();
        assert_eq!(doc.config_current(&Column::Param("width".into())).as_deref(), Some("6 mm"));
        doc.configs.rules.push(second);
        let before = doc.clone();
        assert!(doc.apply_configuration("Default").is_err());
        assert_eq!(doc, before);
    }
}
