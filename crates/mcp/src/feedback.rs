//! What a tool call did to the design, and what changed in it between two calls, told back to the
//! agent with the call's result (docs/mcp.md, "Design feedback").
//!
//! The server reads `model.health` before and after each tool call. Comparing the two says which
//! features the call broke or mended and which warnings it raised; comparing
//! the reading before a call with the one after the previous call says whether someone else (the
//! person at the app, or another client) changed the design in between, and how. When there is
//! anything to tell, the result gets one more text block: `Design feedback:` and a JSON object.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::backend::Backend;

/// The first words of the feedback block, so an agent (and a test) can find it.
pub const PREFIX: &str = "Design feedback: ";

/// Most features listed under one heading (the count says how many there are).
const MAX_LISTED: usize = 20;

/// One `model.health` reading.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Health {
    features: u64,
    /// By feature id: (name, message).
    errors: BTreeMap<u64, (String, String)>,
    warnings: BTreeMap<u64, (String, String)>,
    parameter_errors: BTreeMap<String, String>,
    undo: Vec<String>,
    undo_depth: u64,
    redo_depth: u64,
    path: Option<String>,
}

impl Health {
    fn read(b: &mut dyn Backend) -> Option<Health> {
        let v = b.call("engine.execute", json!({"command": "model.health", "params": {}})).ok()?;
        let messages = |key: &str, field: &str| -> BTreeMap<u64, (String, String)> {
            v[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| {
                            let id = e["feature"].as_u64()?;
                            Some((id, (e["name"].as_str().unwrap_or_default().to_string(), e[field].as_str().unwrap_or_default().to_string())))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Health {
            features: v["features"].as_u64()?,
            errors: messages("errors", "error"),
            warnings: messages("warnings", "warning"),
            parameter_errors: v["parameter_errors"]
                .as_object()
                .map(|o| o.iter().map(|(k, w)| (k.clone(), w.as_str().unwrap_or_default().to_string())).collect())
                .unwrap_or_default(),
            undo: v["undo"].as_array().map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect()).unwrap_or_default(),
            undo_depth: v["undo_depth"].as_u64().unwrap_or(0),
            redo_depth: v["redo_depth"].as_u64().unwrap_or(0),
            path: v["path"].as_str().map(str::to_string),
        })
    }
}

/// Features as `[{feature, name, error|warning}]`, at most [`MAX_LISTED`].
fn listed(m: &BTreeMap<u64, (String, String)>, ids: impl Iterator<Item = u64>, field: &str) -> Value {
    Value::Array(ids.filter_map(|id| m.get(&id).map(|(name, msg)| json!({"feature": id, "name": name, field: msg}))).take(MAX_LISTED).collect())
}

impl Health {
    /// The design itself changed between the two readings: its history moved, another design
    /// was opened, or features came, went, broke or mended. (`model.health`'s `revision` also
    /// moves when only the view changes, a body hidden say, so it is not used.)
    fn differs(&self, b: &Health) -> bool {
        let Health { features, errors, warnings, parameter_errors, undo, undo_depth, redo_depth, path } = self;
        *features != b.features
            || *errors != b.errors
            || *warnings != b.warnings
            || *parameter_errors != b.parameter_errors
            || *undo != b.undo
            || *undo_depth != b.undo_depth
            || *redo_depth != b.redo_depth
            || *path != b.path
    }
}

/// What changed from `a` to `b` in the features' errors and warnings and in the parameters.
fn changes(a: &Health, b: &Health, out: &mut Map<String, Value>) {
    let new = |x: &BTreeMap<u64, (String, String)>, y: &BTreeMap<u64, (String, String)>| -> Vec<u64> {
        y.iter().filter(|(id, m)| x.get(id) != Some(m)).map(|(id, _)| *id).collect()
    };
    let new_errors = new(&a.errors, &b.errors);
    if !new_errors.is_empty() {
        out.insert("new_errors".into(), listed(&b.errors, new_errors.into_iter(), "error"));
    }
    let fixed: Vec<Value> = a.errors.iter().filter(|(id, _)| !b.errors.contains_key(id)).take(MAX_LISTED).map(|(_, (name, _))| json!(name)).collect();
    if !fixed.is_empty() {
        out.insert("fixed".into(), Value::Array(fixed));
    }
    let new_warnings = new(&a.warnings, &b.warnings);
    if !new_warnings.is_empty() {
        out.insert("new_warnings".into(), listed(&b.warnings, new_warnings.into_iter(), "warning"));
    }
    let params: Map<String, Value> =
        b.parameter_errors.iter().filter(|(k, w)| a.parameter_errors.get(*k) != Some(*w)).map(|(k, w)| (k.clone(), json!(w))).collect();
    if !params.is_empty() {
        out.insert("parameter_errors".into(), Value::Object(params));
    }
}

/// The feedback for one call, from the readings after the previous call (`last`), before this
/// one (`pre`) and after it (`post`). `None` when there is nothing to tell.
pub(crate) fn report(last: Option<&Health>, pre: &Health, post: &Health) -> Option<Value> {
    let mut out = Map::new();
    match last {
        // Someone else changed the design since the agent's last call.
        Some(last) if last.differs(pre) => {
            let mut outside = Map::new();
            if last.path != pre.path {
                outside.insert("opened".into(), json!(pre.path));
            }
            if pre.undo_depth > last.undo_depth {
                let n = usize::try_from(pre.undo_depth - last.undo_depth).unwrap_or(usize::MAX).min(pre.undo.len());
                outside.insert("done".into(), json!(pre.undo.iter().skip(pre.undo.len() - n).collect::<Vec<_>>()));
            } else if pre.undo_depth < last.undo_depth {
                outside.insert("undone".into(), json!(last.undo_depth - pre.undo_depth));
            }
            if pre.redo_depth < last.redo_depth && pre.undo_depth > last.undo_depth {
                outside.insert("redone".into(), json!(last.redo_depth - pre.redo_depth));
            }
            changes(last, pre, &mut outside);
            outside.insert(
                "note".into(),
                json!("the design changed outside your calls (in the app, or by another client) since your last call; inspect it before relying on what you knew"),
            );
            out.insert("changed_since_your_last_call".into(), Value::Object(outside));
        }
        Some(_) => {}
        // The first call: say what is already broken.
        None if !pre.errors.is_empty() => {
            out.insert("failing_features".into(), listed(&pre.errors, pre.errors.keys().copied(), "error"));
        }
        None => {}
    }
    if pre.differs(post) {
        out.insert("features".into(), json!(post.features));
        changes(pre, post, &mut out);
        if !post.errors.is_empty() {
            out.insert("failing".into(), json!(post.errors.len()));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// The readings kept between calls.
#[derive(Default)]
pub struct Feedback {
    last: Option<Health>,
    /// On: tool results carry the feedback block. Off by default (`Server::with_design_feedback`).
    pub on: bool,
}

impl Feedback {
    /// Run a tool call (`call` returns its MCP result) and add the feedback block to its result.
    /// A backend without `model.health` (an older app), or one that cannot be reached, gets the
    /// result alone.
    pub fn around(&mut self, b: &mut dyn Backend, call: impl FnOnce(&mut dyn Backend) -> Value) -> Value {
        if !self.on {
            return call(b);
        }
        let Some(pre) = Health::read(b) else {
            self.last = None;
            return call(b);
        };
        let mut result = call(b);
        let Some(post) = Health::read(b) else {
            self.last = None;
            return result;
        };
        if let (Some(fb), Some(content)) = (report(self.last.as_ref(), &pre, &post), result.get_mut("content").and_then(Value::as_array_mut)) {
            content.push(json!({"type": "text", "text": format!("{PREFIX}{fb}")}));
        }
        self.last = Some(post);
        result
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// A reading with `steps` undo steps.
    fn health(steps: u64) -> Health {
        Health { features: 3, undo_depth: steps, ..Default::default() }
    }

    #[test]
    fn nothing_to_tell_when_nothing_changed() {
        let h = health(4);
        assert_eq!(report(Some(&h), &h, &h), None);
        assert_eq!(report(None, &h, &h), None);
    }

    #[test]
    fn a_change_reports_what_it_broke_and_mended() {
        let mut pre = health(4);
        pre.errors.insert(2, ("Fillet1".into(), "the radius is too big".into()));
        let mut post = health(5);
        post.errors.insert(3, ("Shell1".into(), "the wall is too thick".into()));
        post.warnings.insert(1, ("Extrude1".into(), "the profile is open".into()));
        let r = report(Some(&pre), &pre, &post).unwrap();
        assert_eq!(r["features"], 3);
        assert_eq!(r["new_errors"], json!([{"feature": 3, "name": "Shell1", "error": "the wall is too thick"}]));
        assert_eq!(r["fixed"], json!(["Fillet1"]));
        assert_eq!(r["new_warnings"][0]["name"], "Extrude1");
        assert_eq!(r["failing"], 1);
        assert!(r.get("changed_since_your_last_call").is_none(), "{r}");
    }

    #[test]
    fn a_change_between_calls_is_reported_as_someone_elses() {
        let mut last = health(1);
        last.undo = vec!["Box".into()];
        let mut pre = health(3);
        pre.undo = vec!["Box".into(), "Extrude".into(), "Fillet".into()];
        pre.errors.insert(7, ("Fillet1".into(), "no edge".into()));
        let r = report(Some(&last), &pre, &pre).unwrap();
        let outside = &r["changed_since_your_last_call"];
        assert_eq!(outside["done"], json!(["Extrude", "Fillet"]));
        assert_eq!(outside["new_errors"][0]["name"], "Fillet1");
        assert!(r.get("features").is_none(), "this call changed nothing: {r}");
        // Undone in the app.
        let undone = health(0);
        assert_eq!(report(Some(&last), &undone, &undone).unwrap()["changed_since_your_last_call"]["undone"], 1);
    }

    #[test]
    fn the_first_call_names_the_features_already_failing() {
        let mut pre = health(1);
        pre.errors.insert(5, ("Loft1".into(), "the sections cross".into()));
        let r = report(None, &pre, &pre).unwrap();
        assert_eq!(r["failing_features"][0]["name"], "Loft1");
    }
}
