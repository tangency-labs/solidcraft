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
    fn fields<'a>(&'a self, fields: &mut BTreeSet<&'a str>) {
        match self {
            Self::Eq(field, _) | Self::In(field, _) => {
                fields.insert(field);
            }
            Self::All(conditions) | Self::Any(conditions) => {
                for condition in conditions {
                    condition.fields(fields);
                }
            }
            Self::Not(condition) => condition.fields(fields),
        }
    }
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
        let annotated = properties.values().any(|property| {
            [VISIBLE_WHEN, ENABLED_WHEN, DEFAULTS].iter().any(|key| property.get(*key).is_some())
                || property.get("oneOf").and_then(Value::as_array).is_some_and(|options| options.iter().any(|o| o.get(OPTION_WHEN).is_some()))
        });
        if !annotated {
            return Ok(Self::default());
        }
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

    /// Scalar controller values after all of this scope's visibility rules have been applied.
    pub fn visible_values(&self, values: &Map<String, Value>) -> Result<Map<String, Value>> {
        visible_values(values, self.fields.iter().map(|(name, rules)| (name.as_str(), rules)))
    }
}

/// Evaluate controllers across all applicable scopes, with each controller resolved before its
/// dependents. A field hidden by any scope cannot control a field in another scope. Only scalars
/// are copied, so unchecked nested input is never cloned here.
pub fn visible_values<'a>(
    values: &Map<String, Value>,
    fields: impl IntoIterator<Item = (&'a str, &'a FieldConditions)>,
) -> Result<Map<String, Value>> {
    let mut values: Map<String, Value> =
        values.iter().filter(|(_, v)| !v.is_object() && !v.is_array()).map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut remaining: BTreeMap<&str, Vec<&FieldConditions>> = BTreeMap::new();
    for (name, rules) in fields {
        remaining.entry(name).or_default().push(rules);
    }
    let mut dependents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut counts = BTreeMap::new();
    let mut ready = BTreeSet::new();
    for (name, rules) in &remaining {
        let mut dependencies = BTreeSet::new();
        for rule in rules {
            if let Some(condition) = &rule.visible {
                condition.fields(&mut dependencies);
            }
        }
        dependencies.retain(|dependency| remaining.contains_key(dependency));
        counts.insert(*name, dependencies.len());
        if dependencies.is_empty() {
            ready.insert(*name);
        }
        for dependency in dependencies {
            dependents.entry(dependency).or_default().insert(*name);
        }
    }
    while let Some(name) = ready.pop_first() {
        if let Some(rules) = remaining.remove(name)
            && rules.iter().any(|rule| !rule.visible(&values))
        {
            values.remove(name);
        }
        if let Some(dependents) = dependents.get(name) {
            for dependent in dependents {
                if let Some(count) = counts.get_mut(dependent) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.insert(*dependent);
                    }
                }
            }
        }
    }
    if !remaining.is_empty() {
        return Err(invalid("input_schema", format!("conditional field cycle: {}", remaining.keys().copied().collect::<Vec<_>>().join(", "))));
    }
    Ok(values)
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
    contains_conditions(schema, true)
}

// At composition boundaries, scalar choice-option annotations belong to the enclosing field,
// not to an object branch. Conditions below properties/items still need branch projection.
pub(crate) fn contains_conditions(schema: &Value, choice_filters: bool) -> bool {
    fn visit(schema: &Value, depth: usize, budget: &mut usize, choice_filters: bool) -> bool {
        if depth > 32 || *budget == 0 {
            return true;
        }
        *budget -= 1;
        if [VISIBLE_WHEN, ENABLED_WHEN, DEFAULTS].iter().any(|key| schema.get(*key).is_some())
            || (choice_filters && schema.get(OPTION_WHEN).is_some())
        {
            return true;
        }
        if schema.get("properties").and_then(Value::as_object).is_some_and(|p| p.values().any(|v| visit(v, depth + 1, budget, true))) {
            return true;
        }
        for key in ["items", "additionalProperties"] {
            if schema.get(key).is_some_and(|v| visit(v, depth + 1, budget, true)) {
                return true;
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if schema.get(key).and_then(Value::as_array).is_some_and(|options| options.iter().any(|v| visit(v, depth + 1, budget, choice_filters))) {
                return true;
            }
        }
        false
    }
    visit(schema, 0, &mut 100_000, choice_filters)
}

// A required, unconditional finite scalar discriminator proves that two branches cannot both
// match. The same numeric comparison as input checking avoids rounding integer discriminators.
fn disjoint_branches(a: &Value, b: &Value, objects_only: bool, path: &str, budget: &mut usize, input: Option<&Value>) -> Result<bool> {
    fn domain(property: &Value) -> Option<&[Value]> {
        let values = property.get("const").map(std::slice::from_ref).or_else(|| property.get("enum").and_then(Value::as_array).map(Vec::as_slice))?;
        (!values.is_empty() && values.len() <= 256 && values.iter().all(|v| !v.is_object() && !v.is_array())).then_some(values)
    }
    let object_branches = a.get("type").and_then(Value::as_str) == Some("object") && b.get("type").and_then(Value::as_str) == Some("object");
    if !(objects_only || object_branches) {
        return Ok(false);
    }
    let (Some(required), Some(other)) = (a.get("required").and_then(Value::as_array), b.get("required").and_then(Value::as_array)) else {
        return Ok(false);
    };
    if required.len() > 1024 || other.len() > 1024 {
        return Err(invalid(path, "conditional composition is too large"));
    }
    let other: BTreeSet<&str> = other.iter().filter_map(Value::as_str).collect();
    for name in required.iter().filter_map(Value::as_str) {
        *budget = budget.checked_sub(1).ok_or_else(|| invalid(path, "conditional composition is too large"))?;
        if !other.contains(name) {
            continue;
        }
        let (Some(a), Some(b)) = (a.get("properties").and_then(|p| p.get(name)), b.get("properties").and_then(|p| p.get(name))) else { continue };
        if has_conditions(a) || has_conditions(b) {
            continue;
        }
        let (Some(a), Some(b)) = (domain(a), domain(b)) else { continue };
        let mut overlap = false;
        for left in a {
            for right in b {
                *budget = budget.checked_sub(1).ok_or_else(|| invalid(path, "conditional composition is too large"))?;
                if scalar_equal(left, right) {
                    overlap = true;
                    break;
                }
            }
            if overlap {
                break;
            }
        }
        if !overlap && input.is_none_or(|input| input.get(name).is_some_and(|value| a.iter().any(|option| scalar_equal(value, option)))) {
            return Ok(true);
        }
    }
    Ok(false)
}

// Use only the discriminators that prove branches disjoint, rather than validating fields
// that another scope may hide. Single branches need no discriminator.
pub(crate) fn selected_branch<'a>(options: &'a [Value], value: &Value, budget: &mut usize) -> Result<Option<&'a Value>> {
    if options.len() == 1 {
        return Ok(options.first());
    }
    for (index, option) in options.iter().take(256).enumerate() {
        let mut matches = true;
        for (other_index, other) in options.iter().take(256).enumerate() {
            if index != other_index && !disjoint_branches(option, other, true, "input_schema", budget, Some(value))? {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(Some(option));
        }
    }
    Ok(None)
}

// Expand alternatives independently: combining all alternatives would invent cycles between
// mutually exclusive branches. Children inherit the same combination of enclosing scopes.
fn validate_visibility_combinations(schemas: &[&Value], path: &str, depth: usize, budget: &mut usize) -> Result<()> {
    #[derive(Clone, Copy)]
    enum Pending<'a> {
        Schema(&'a Value),
        Choice(&'a [Value]),
    }
    fn spend(budget: &mut usize, amount: usize, path: &str) -> Result<()> {
        *budget = budget.checked_sub(amount).ok_or_else(|| invalid(path, "conditional scope combinations are too large"))?;
        Ok(())
    }
    if depth > 32 {
        return Err(invalid(path, "schema is too deeply nested"));
    }
    let mut combinations = vec![(schemas.iter().copied().map(Pending::Schema).collect::<Vec<_>>(), Vec::new())];
    'combinations: while let Some((mut pending, mut applied)) = combinations.pop() {
        while let Some(next) = pending.pop() {
            spend(budget, 1, path)?;
            match next {
                Pending::Schema(schema) => {
                    applied.push(schema);
                    for key in ["oneOf", "anyOf"] {
                        if let Some(options) = schema.get(key).and_then(Value::as_array)
                            && options.iter().any(|option| contains_conditions(option, false))
                        {
                            pending.push(Pending::Choice(options));
                        }
                    }
                }
                Pending::Choice(options) => {
                    for option in options {
                        spend(budget, pending.len() + applied.len() + 1, path)?;
                        let mut branch = pending.clone();
                        branch.push(Pending::Schema(option));
                        combinations.push((branch, applied.clone()));
                    }
                    continue 'combinations;
                }
            }
        }
        let mut conditions = Vec::new();
        for schema in &applied {
            let rules = InputConditions::parse(schema)?;
            spend(budget, rules.fields.len() + rules.fields.values().map(|field| field.dependencies.len()).sum::<usize>(), path)?;
            conditions.push(rules);
        }
        visible_values(&Map::new(), conditions.iter().flat_map(|scope| scope.fields.iter().map(|(name, rules)| (name.as_str(), rules))))?;
        let mut names = BTreeSet::new();
        for schema in &applied {
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                spend(budget, properties.len(), path)?;
                names.extend(properties.keys().map(String::as_str));
            }
        }
        for name in names {
            let children: Vec<_> = applied
                .iter()
                .filter_map(|schema| {
                    schema.get("properties").and_then(|properties| properties.get(name)).or_else(|| schema.get("additionalProperties"))
                })
                .collect();
            if children.iter().any(|child| contains_conditions(child, false)) {
                validate_visibility_combinations(&children, &format!("{path}.properties.{name}"), depth + 1, budget)?;
            }
        }
        for key in ["items", "additionalProperties"] {
            let children: Vec<_> = applied.iter().filter_map(|schema| schema.get(key)).collect();
            if children.iter().any(|child| contains_conditions(child, false)) {
                validate_visibility_combinations(&children, &format!("{path}.{key}"), depth + 1, budget)?;
            }
        }
    }
    Ok(())
}

/// Validate every object scope at load, including schemas the GUI does not draw.
pub fn validate_conditions(schema: &Value) -> Result<()> {
    fn visit(schema: &Value, path: &str, depth: usize, budget: &mut usize) -> Result<()> {
        if depth > 32 || *budget == 0 {
            return Err(invalid("input_schema", "schema is too large or deeply nested"));
        }
        *budget -= 1;
        InputConditions::parse(schema)?;
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                visit(property, &format!("{path}.properties.{name}"), depth + 1, budget)?;
            }
        }
        for key in ["items", "additionalProperties"] {
            if let Some(child) = schema.get(key) {
                visit(child, &format!("{path}.{key}"), depth + 1, budget)?;
            }
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(options) = schema.get(key).and_then(Value::as_array) {
                let location = format!("{path}.{key}");
                if options.iter().any(|option| contains_conditions(option, false)) {
                    if key == "allOf" || options.len() > 256 {
                        return Err(invalid(
                            &location,
                            "conditional object rules require a supported oneOf/anyOf composition with at most 256 branches",
                        ));
                    }
                    for (i, a) in options.iter().enumerate() {
                        for b in options.iter().skip(i + 1) {
                            *budget = budget.checked_sub(1).ok_or_else(|| invalid(&location, "conditional composition is too large"))?;
                            if !disjoint_branches(a, b, schema.get("type").and_then(Value::as_str) == Some("object"), &location, budget, None)? {
                                return Err(invalid(&location, "conditional branches need disjoint required scalar const/enum discriminators"));
                            }
                        }
                    }
                }
                for (i, option) in options.iter().enumerate() {
                    visit(option, &format!("{location}[{i}]"), depth + 1, budget)?;
                }
            }
        }
        Ok(())
    }
    let mut budget = 100_000;
    visit(schema, "input_schema", 0, &mut budget)?;
    if contains_conditions(schema, false) {
        validate_visibility_combinations(&[schema], "input_schema", 0, &mut budget)?;
    }
    Ok(())
}
