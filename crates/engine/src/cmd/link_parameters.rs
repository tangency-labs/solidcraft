//! The values a design link builds its part with (`DesignLink::parameters`): which parameter
//! names a link may set, how they travel through the link code as one small `Copy` value, and
//! `component.set_link_parameters`, which changes them and builds the part again.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use serde_json::{Value, json};

use super::{SelectedConfiguration, component_key};
use crate::params::bad;
use crate::{Result, Session};

/// Parameters one link may set.
const MAX_LINK_PARAMETERS: usize = 64;
/// One expression, in bytes (the design's own bound).
const MAX_EXPRESSION_BYTES: usize = 4096;

/// A link's parameters, in name order. They are interned for the life of the process so a
/// [`SelectedConfiguration`] stays `Copy`: each distinct set is kept once, a few hundred bytes.
#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) struct LinkParameters(Vec<(String, String)>);

static INTERNED: Mutex<Option<HashSet<&'static LinkParameters>>> = Mutex::new(None);

impl LinkParameters {
    /// The shared copy of `map`; `None` for an empty map.
    pub(super) fn intern(map: &BTreeMap<String, String>) -> Option<&'static LinkParameters> {
        if map.is_empty() {
            return None;
        }
        let probe = LinkParameters(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
        let mut guard = INTERNED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let set = guard.get_or_insert_with(HashSet::new);
        if let Some(found) = set.get(&probe) {
            return Some(*found);
        }
        let kept: &'static LinkParameters = Box::leak(Box::new(probe));
        set.insert(kept);
        Some(kept)
    }

    pub(super) fn pairs(&self) -> &[(String, String)] {
        &self.0
    }
}

/// The parameters a selection carries, as the link stores them.
pub(super) fn parameters_map(c: Option<SelectedConfiguration>) -> BTreeMap<String, String> {
    c.and_then(|c| c.parameters).map(|p| p.pairs().iter().cloned().collect()).unwrap_or_default()
}

/// The same as a command parameter (`null` when there are none).
pub(super) fn parameters_json(c: Option<SelectedConfiguration>) -> Value {
    let map = parameters_map(c);
    if map.is_empty() { Value::Null } else { json!(map) }
}

/// `configuration` with `map` as its parameters; a selection of its own when only parameters are given.
pub(super) fn with_parameters(c: Option<SelectedConfiguration>, map: &BTreeMap<String, String>) -> Option<SelectedConfiguration> {
    let parameters = LinkParameters::intern(map);
    match c {
        Some(c) => Some(SelectedConfiguration { parameters, ..c }),
        None => parameters.map(|parameters| SelectedConfiguration { row: None, identity: None, parameters: Some(parameters) }),
    }
}

/// Read `parameters` from a command's parameters: omitted keeps `default`, `null` clears, an
/// object of names to expressions (or numbers, in the design's unit) is the new set.
pub(super) fn link_parameters(p: &Value, default: &BTreeMap<String, String>, cmd: &str) -> Result<BTreeMap<String, String>> {
    match p.get("parameters") {
        None => Ok(default.clone()),
        Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(map)) => {
            if map.len() > MAX_LINK_PARAMETERS {
                return Err(bad(cmd, format!("`parameters` sets at most {MAX_LINK_PARAMETERS} parameters")));
            }
            let mut out = BTreeMap::new();
            for (name, value) in map {
                if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
                    return Err(bad(cmd, format!("`{name}` is not a parameter name")));
                }
                let expression = match value {
                    Value::String(text) => text.trim().to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => return Err(bad(cmd, format!("the value of `{name}` must be an expression (text) or a number"))),
                };
                if expression.is_empty() || expression.len() > MAX_EXPRESSION_BYTES {
                    return Err(bad(cmd, format!("the expression for `{name}` is empty or longer than {MAX_EXPRESSION_BYTES} bytes")));
                }
                out.insert(name.clone(), expression);
            }
            Ok(out)
        }
        Some(_) => Err(bad(cmd, "`parameters` must be an object of parameter names to expressions, or null")),
    }
}

/// `component.set_link_parameters`: replace a linked component's parameters and build its part again.
pub(super) fn set_link_parameters(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.set_link_parameters";
    let component = component_key(s, p, "component", cmd)?;
    let link = s.doc.links.get(&component).cloned().ok_or_else(|| bad(cmd, "that component is not linked to a design"))?;
    if link.importer.is_some() || link.step_names.is_some() || link.path.is_empty() {
        return Err(bad(cmd, "only a component linked to a design file has parameters"));
    }
    if p.get("parameters").is_none() {
        return Err(bad(cmd, "`parameters` is required (an object, or null to clear them)"));
    }
    let values = link_parameters(p, &BTreeMap::new(), cmd)?;
    if let Some(stored) = s.doc_mut().links.get_mut(&component) {
        stored.parameters = values.clone();
    }
    // The update builds the part with the new values and refuses ones outside its limits; the
    // command is one edit, so a refusal puts the old values back.
    let updated = s.execute("component.update_links", &json!({"component": component}))?;
    Ok(json!({"component": component, "parameters": values, "updated": updated.get("updated").cloned().unwrap_or_else(|| json!([]))}))
}
