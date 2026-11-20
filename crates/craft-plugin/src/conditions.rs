//! Bounded, declarative sibling conditions shared by plugin forms and input validation.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::{Result, invalid};

pub const VISIBLE_WHEN: &str = "x-solidcraft-visible-when";
pub const ENABLED_WHEN: &str = "x-solidcraft-enabled-when";
pub const OPTION_WHEN: &str = "x-solidcraft-when";
pub const DEFAULTS: &str = "x-solidcraft-defaults";

/// A structured condition, compiled only after its references and values have been checked.
#[derive(Clone, Debug, PartialEq)]
enum Condition {
    Eq(String, Value),
    In(String, Vec<Value>),
    All(Vec<Self>),
    Any(Vec<Self>),
    Not(Box<Self>),
}
impl Condition {
    fn matches(&self, values: &Map<String, Value>) -> bool {
        match self {
            Self::Eq(field, value) => values.get(field).is_some_and(|v| scalar_equal(v, value)),
            Self::In(field, options) => values.get(field).is_some_and(|v| options.iter().any(|option| scalar_equal(v, option))),
            Self::All(conditions) => conditions.iter().all(|c| c.matches(values)),
            Self::Any(conditions) => conditions.iter().any(|c| c.matches(values)),
            Self::Not(condition) => !condition.matches(values),
        }
    }
}

// JSON numbers compare by mathematical value, including GUI f64s against integer literals.
// Comparing via f64 alone would conflate distinct integers above 2^53.
pub(crate) fn scalar_equal(a: &Value, b: &Value) -> bool {
    let integer = |v: &Value| v.as_i64().map(i128::from).or_else(|| v.as_u64().map(i128::from));
    match (integer(a), integer(b), a.as_f64(), b.as_f64()) {
        (Some(a), Some(b), _, _) => a == b,
        (Some(a), None, _, Some(b)) => b.is_finite() && b.fract() == 0.0 && b as i128 == a,
        (None, Some(b), Some(a), _) => a.is_finite() && a.fract() == 0.0 && a as i128 == b,
        _ => a == b,
    }
}

/// The conditions attached to a single field. Evaluation never calls plugin code.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldConditions {
    visible: Option<Condition>,
    enabled: Option<Condition>,
    options: Vec<Option<Condition>>,
    defaults: Vec<(Condition, Value)>,
    dependencies: BTreeSet<String>,
}
impl FieldConditions {
    pub fn visible(&self, values: &Map<String, Value>) -> bool {
        self.visible.as_ref().is_none_or(|c| c.matches(values))
    }
    pub fn enabled(&self, values: &Map<String, Value>) -> bool {
        self.enabled.as_ref().is_none_or(|c| c.matches(values))
    }
    pub(crate) fn has_choice_filter(&self) -> bool {
        self.options.iter().any(Option::is_some)
    }
    pub fn option_available(&self, index: usize, values: &Map<String, Value>) -> bool {
        self.options.get(index).and_then(Option::as_ref).is_none_or(|c| c.matches(values))
    }
    pub fn default_value(&self, values: &Map<String, Value>) -> Option<&Value> {
        self.defaults.iter().find(|(c, _)| c.matches(values)).map(|(_, value)| value)
    }
    pub fn dependencies(&self) -> &BTreeSet<String> {
        &self.dependencies
    }
}

/// Conditions on one object's properties, in dependency order (controllers first).
#[derive(Clone, Debug, Default)]
pub struct InputConditions {
    pub fields: BTreeMap<String, FieldConditions>,
    pub order: Vec<String>,
}
impl InputConditions {
    pub fn parse(schema: &Value) -> Result<Self> {
        Self::from_properties(schema.get("properties").and_then(Value::as_object))
    }

    pub(crate) fn from_properties(properties: Option<&Map<String, Value>>) -> Result<Self> {
        let Some(properties) = properties else { return Ok(Self::default()) };
        if properties.len() > 1024 {
            return Err(invalid("input_schema.properties", "too many conditional fields"));
        }
        let mut fields = BTreeMap::new();
        let mut budget = 4096_usize;
        for (name, property) in properties {
            let path = format!("input_schema.properties.{name}");
            let mut dependencies = BTreeSet::new();
            let mut parse = |value: &Value| parse_condition(value, properties, &path, &mut dependencies, 0, &mut budget);
            let visible = property.get(VISIBLE_WHEN).map(&mut parse).transpose()?;
            let enabled = property.get(ENABLED_WHEN).map(&mut parse).transpose()?;
            let mut options = Vec::new();
            if let Some(list) = property.get("oneOf").and_then(Value::as_array) {
                for option in list {
                    options.push(option.get(OPTION_WHEN).map(&mut parse).transpose()?);
                }
            }
            let mut defaults = Vec::new();
            if let Some(list) = property.get(DEFAULTS) {
                let list = list.as_array().ok_or_else(|| invalid(&path, "conditional defaults must be a list"))?;
                for rule in list {
                    let rule = rule.as_object().filter(|r| r.len() == 2).ok_or_else(|| invalid(&path, "a default needs when and value"))?;
                    let condition = parse(rule.get("when").ok_or_else(|| invalid(&path, "a default needs when"))?)?;
                    let value = rule.get("value").ok_or_else(|| invalid(&path, "a default needs value"))?;
                    crate::input::check_value(property, value).map_err(|_| invalid(&path, "conditional default does not fit the field"))?;
                    defaults.push((condition, value.clone()));
                }
            }
            fields.insert(name.clone(), FieldConditions { visible, enabled, options, defaults, dependencies });
        }
        // Kahn's algorithm: no recursive walk through an input-derived graph.
        let mut order = Vec::new();
        let mut remaining: BTreeSet<String> = fields.keys().cloned().collect();
        while !remaining.is_empty() {
            let ready: Vec<String> =
                remaining.iter().filter(|name| fields.get(*name).is_some_and(|f| f.dependencies.is_disjoint(&remaining))).cloned().collect();
            if ready.is_empty() {
                return Err(invalid("input_schema", format!("conditional field cycle: {}", remaining.into_iter().collect::<Vec<_>>().join(", "))));
            }
            for name in ready {
                remaining.remove(&name);
                order.push(name);
            }
        }
        Ok(Self { fields, order })
    }

    /// Scalar controller values in dependency order. Hidden controllers cannot affect another
    /// row. Only scalars are copied: an arbitrary, unchecked nested input is never cloned here.
    pub fn visible_values(&self, values: &Map<String, Value>) -> Map<String, Value> {
        let mut values: Map<String, Value> =
            values.iter().filter(|(_, v)| !v.is_object() && !v.is_array()).map(|(k, v)| (k.clone(), v.clone())).collect();
        for name in &self.order {
            if self.fields.get(name).is_some_and(|f| !f.visible(&values)) {
                values.remove(name);
            }
        }
        values
    }
}

fn parse_condition(
    value: &Value,
    properties: &Map<String, Value>,
    path: &str,
    dependencies: &mut BTreeSet<String>,
    depth: usize,
    budget: &mut usize,
) -> Result<Condition> {
    if depth > 32 || *budget == 0 {
        return Err(invalid(path, "condition is too large or deeply nested"));
    }
    *budget -= 1;
    let object = value.as_object().ok_or_else(|| invalid(path, "condition must be an object"))?;
    if object.len() == 1 {
        for key in ["all", "any"] {
            if let Some(children) = object.get(key) {
                let children = children.as_array().ok_or_else(|| invalid(path, "all/any needs a list of conditions"))?;
                let children =
                    children.iter().map(|c| parse_condition(c, properties, path, dependencies, depth + 1, budget)).collect::<Result<Vec<_>>>()?;
                return Ok(if key == "all" { Condition::All(children) } else { Condition::Any(children) });
            }
        }
        if let Some(child) = object.get("not") {
            return Ok(Condition::Not(Box::new(parse_condition(child, properties, path, dependencies, depth + 1, budget)?)));
        }
    }
    if object.len() != 2 {
        return Err(invalid(path, "condition needs field and eq/in, or all/any/not"));
    }
    let field = object.get("field").and_then(Value::as_str).ok_or_else(|| invalid(path, "condition needs a sibling field name"))?;
    let property = properties.get(field).ok_or_else(|| invalid(path, format!("condition names missing field {field}")))?;
    if !matches!(property.get("type").and_then(Value::as_str), Some("string" | "boolean" | "number" | "integer" | "null")) {
        return Err(invalid(path, format!("condition field {field} must be scalar")));
    }
    let possible = |v: &Value| -> Result<()> {
        if v.is_array() || v.is_object() || crate::input::check_value(property, v).is_err() {
            return Err(invalid(path, format!("impossible condition value for {field}")));
        }
        Ok(())
    };
    dependencies.insert(field.to_owned());
    if let Some(value) = object.get("eq") {
        possible(value)?;
        return Ok(Condition::Eq(field.to_owned(), value.clone()));
    }
    if let Some(values) = object.get("in").and_then(Value::as_array) {
        if values.len() > *budget {
            return Err(invalid(path, "condition is too large"));
        }
        *budget -= values.len();
        for value in values {
            possible(value)?;
        }
        return Ok(Condition::In(field.to_owned(), values.clone()));
    }
    Err(invalid(path, "condition needs eq or an in list"))
}

/// Whether a schema opts into conditional input handling. Schemas without these annotations
/// retain the existing direct-call pass-through contract. A schema exceeding the walk limit
/// is treated as conditional so validation reports it rather than bypassing the check.
pub fn has_conditions(schema: &Value) -> bool {
    fn visit(schema: &Value, depth: usize, budget: &mut usize) -> bool {
        if depth > 32 || *budget == 0 {
            return true;
        }
        *budget -= 1;
        if [VISIBLE_WHEN, ENABLED_WHEN, OPTION_WHEN, DEFAULTS].iter().any(|key| schema.get(*key).is_some()) {
            return true;
        }
        if schema.get("properties").and_then(Value::as_object).is_some_and(|p| p.values().any(|v| visit(v, depth + 1, budget))) {
            return true;
        }
        for key in ["items", "additionalProperties"] {
            if schema.get(key).is_some_and(|v| visit(v, depth + 1, budget)) {
                return true;
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if schema.get(key).and_then(Value::as_array).is_some_and(|options| options.iter().any(|v| visit(v, depth + 1, budget))) {
                return true;
            }
        }
        false
    }
    visit(schema, 0, &mut 100_000)
}

/// Validate every object scope at load, including schemas the GUI does not draw.
pub fn validate_conditions(schema: &Value) -> Result<()> {
    fn visit(schema: &Value, depth: usize, budget: &mut usize) -> Result<()> {
        if depth > 32 || *budget == 0 {
            return Err(invalid("input_schema", "schema is too large or deeply nested"));
        }
        *budget -= 1;
        InputConditions::parse(schema)?;
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for property in properties.values() {
                visit(property, depth + 1, budget)?;
            }
        }
        for key in ["items", "additionalProperties"] {
            if let Some(child) = schema.get(key) {
                visit(child, depth + 1, budget)?;
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(options) = schema.get(key).and_then(Value::as_array) {
                for option in options {
                    visit(option, depth + 1, budget)?;
                }
            }
        }
        Ok(())
    }
    visit(schema, 0, &mut 100_000)
}
