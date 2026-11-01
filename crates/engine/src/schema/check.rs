//! Contracts, the unknown-key check and the adapter that runs a converted command.

use std::any::TypeId;
use std::collections::{BTreeSet, HashMap};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, OnceLock};

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::record;
use crate::cmd::Run;
use crate::params::bad;
use crate::{Result, Session};

/// How a session treats calls that break a command's contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParamCheck {
    /// Nothing is checked; commands behave as they always have.
    #[default]
    Off,
    /// The call runs, and a successful object result gains `param_warnings`.
    Warn,
    /// The call is refused, and nothing runs.
    Strict,
}

impl ParamCheck {
    /// Every level, least checking first.
    pub const ALL: [ParamCheck; 3] = [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict];

    pub fn parse(s: &str) -> Option<ParamCheck> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Some(ParamCheck::Off),
            "warn" => Some(ParamCheck::Warn),
            "strict" => Some(ParamCheck::Strict),
            _ => None,
        }
    }

    /// The level's word, as [`ParamCheck::parse`] reads it (the control channel's `param_check`).
    pub fn name(self) -> &'static str {
        match self {
            ParamCheck::Off => "off",
            ParamCheck::Warn => "warn",
            ParamCheck::Strict => "strict",
        }
    }
}

/// The JSON Schema (2020-12) of what deserializes into `T`: a command's parameters.
pub fn input_schema_for<T: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12().for_deserialize().into_generator().into_root_schema_for::<T>()
}

/// The JSON Schema (2020-12) of what `T` serializes to: a command's result.
pub fn output_schema_for<T: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12().for_serialize().into_generator().into_root_schema_for::<T>()
}

/// One way a call breaks a contract.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Issue {
    /// Where: a top-level key (each field type reads its whole value); empty for the whole call.
    pub path: String,
    /// The message, in the engine's style (without the `invalid parameters for …` prefix).
    pub message: String,
    /// For an unknown key, the closest key the command takes.
    pub suggestion: Option<String>,
}

/// A command's typed parameter contract.
pub trait Contract: Sync {
    /// The parameters' JSON Schema.
    fn input_schema(&self) -> Schema;
    /// The top-level keys the command takes.
    fn allowed_keys(&self) -> Arc<BTreeSet<String>>;
    /// Unknown keys and badly shaped parameters in `params`, without running anything.
    fn check(&self, params: &Value) -> Vec<Issue>;
}

/// The contract "the parameters deserialize into `P`". Stateless; `fn() -> P` keeps it
/// `Send + Sync` whatever `P` is.
pub struct TypedContract<P>(PhantomData<fn() -> P>);

impl<P> TypedContract<P> {
    pub const fn new() -> Self {
        TypedContract(PhantomData)
    }
}

impl<P> Default for TypedContract<P> {
    fn default() -> Self {
        Self::new()
    }
}

/// Allowed keys per contract type, worked out once from its schema.
static KEYS: OnceLock<Mutex<HashMap<TypeId, Arc<BTreeSet<String>>>>> = OnceLock::new();

impl<P: DeserializeOwned + JsonSchema + 'static> Contract for TypedContract<P> {
    fn input_schema(&self) -> Schema {
        input_schema_for::<P>()
    }

    fn allowed_keys(&self) -> Arc<BTreeSet<String>> {
        let keys = KEYS.get_or_init(Default::default);
        let mut keys = keys.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        keys.entry(TypeId::of::<P>()).or_insert_with(|| Arc::new(schema_keys(input_schema_for::<P>().as_value()))).clone()
    }

    fn check(&self, params: &Value) -> Vec<Issue> {
        let object = match params {
            // `Session::execute` reads `null` as `{}`.
            Value::Null => return Vec::new(),
            Value::Object(o) => o,
            _ => return vec![Issue { path: String::new(), message: "parameters must be an object".into(), suggestion: None }],
        };
        let mut issues = unknown_keys(object, &self.allowed_keys());
        if let Err(e) = serde_path_to_error::deserialize::<_, P>(params) {
            issues.push(parse_issue(&e));
        }
        issues
    }
}

/// The most keys of one call that are looked at, and the longest key given a suggestion.
pub(crate) const MAX_KEYS: usize = 64;
const MAX_KEY_LEN: usize = 64;
const MAX_MESSAGE_LEN: usize = 300;
const MAX_WHOLE_LEN: usize = 4096;

/// Marks a deserializer's message as complete as it is (a handler's own wording, such as
/// ``unknown operation `bogus` (…)``), so [`parse_issue`] doesn't put the key in front.
const WHOLE: char = '\u{1}';

/// `message`, marked as complete (see [`WHOLE`]).
pub(crate) fn whole(message: impl std::fmt::Display) -> String {
    format!("{WHOLE}{message}")
}

/// Marks a deserializer's message that the handler gives after the key and a colon, as it is
/// (serde's own wording for a saved reference, such as "missing field `at`", which
/// [`parse_issue`] would otherwise reword).
const KEYED: char = '\u{3}';

/// `message`, to follow the key and a colon as it is (see [`KEYED`]).
pub(crate) fn keyed(message: impl std::fmt::Display) -> String {
    format!("{KEYED}{message}")
}

/// Marks a deserializer's message that carries its own path: an error from a flattened
/// `FeatureCommon`, which serde can't locate. The path (one of its field names) and the error
/// follow as JSON.
const LOCATED: char = '\u{2}';

/// `e`, an error deserializing a flattened `FeatureCommon`, as a deserializer's message that
/// keeps its path, for [`parse_issue`].
pub(crate) fn located(e: &serde_path_to_error::Error<serde_json::Error>) -> String {
    let located = serde_json::to_string(&(error_path(e), e.inner().to_string())).unwrap_or_default();
    format!("{LOCATED}{located}")
}

fn error_path(e: &serde_path_to_error::Error<serde_json::Error>) -> String {
    let path = e.path().to_string();
    if path == "." { String::new() } else { path }
}

/// The top-level property names of a schema, following `allOf`/`anyOf`/`oneOf` and `$ref`s
/// into the root's `$defs`.
pub(crate) fn schema_keys(root: &Value) -> BTreeSet<String> {
    fn walk(root: &Value, node: &Value, out: &mut BTreeSet<String>, depth: usize) {
        if depth > 16 {
            return;
        }
        if let Some(props) = node.get("properties").and_then(Value::as_object) {
            out.extend(props.keys().cloned());
        }
        for k in ["allOf", "anyOf", "oneOf"] {
            for branch in node.get(k).and_then(Value::as_array).into_iter().flatten() {
                walk(root, branch, out, depth + 1);
            }
        }
        if let Some(name) = node.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/"))
            && let Some(target) = root.get("$defs").and_then(|d| d.get(name))
        {
            walk(root, target, out, depth + 1);
        }
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out, 0);
    out
}

/// The keys of `params` that `allowed` doesn't list, each with the closest allowed key.
fn unknown_keys(params: &Map<String, Value>, allowed: &BTreeSet<String>) -> Vec<Issue> {
    let mut issues: Vec<Issue> = params
        .keys()
        .take(MAX_KEYS)
        .filter(|k| !allowed.contains(*k))
        .map(|k| {
            let suggestion = nearest(k, allowed);
            // A hostile key is cut, so errors and warnings stay small.
            let k = truncate(k, MAX_KEY_LEN);
            let message = match &suggestion {
                Some(s) => format!("unknown key `{k}`; did you mean `{s}`?"),
                None => format!("unknown key `{k}`"),
            };
            Issue { path: k, message, suggestion }
        })
        .collect();
    if params.len() > MAX_KEYS {
        issues.push(Issue { path: String::new(), message: format!("too many parameters ({}; at most {MAX_KEYS})", params.len()), suggestion: None });
    }
    issues
}

/// `s` cut to at most `max` bytes, on a character boundary, with `…` when cut.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", s.get(..end).unwrap_or_default())
}

/// The allowed key closest to `key`, when it is close enough to be a likely typo. Keys are
/// short, so short keys may be one edit away at most (`x` is not a typo of `to`).
fn nearest(key: &str, allowed: &BTreeSet<String>) -> Option<String> {
    if key.len() > MAX_KEY_LEN {
        return None;
    }
    let key = key.to_ascii_lowercase();
    let near = (key.chars().count() / 3).max(1);
    allowed
        .iter()
        .map(|a| (crate::cmd::edit_distance(&key, &a.to_ascii_lowercase()), a))
        .filter(|(d, _)| *d <= near)
        .min_by_key(|(d, _)| *d)
        .map(|(_, a)| a.clone())
}

/// A deserialization failure as an issue, in the engine's style.
pub(crate) fn parse_issue(e: &serde_path_to_error::Error<serde_json::Error>) -> Issue {
    let path = error_path(e);
    let inner = e.inner().to_string();
    // An error from a flattened `FeatureCommon`: its field continues the path.
    let (path, inner) = match inner.strip_prefix(LOCATED).and_then(|r| serde_json::from_str::<(String, String)>(r).ok()) {
        Some((field, inner)) if path.is_empty() => (field, inner),
        Some((field, inner)) => (format!("{path}.{field}"), inner),
        None => (path, inner),
    };
    if let Some(after) = inner.strip_prefix(KEYED) {
        return Issue { path: path.clone(), message: truncate(&format!("`{path}`: {after}"), MAX_WHOLE_LEN), suggestion: None };
    }
    if let Some(complete) = inner.strip_prefix(WHOLE) {
        // The handler's own message, which names the caller's value: in full up to a generous
        // cap, since the contract also checks keys the handler ignores (`side` without `thin`).
        return Issue { path, message: truncate(complete, MAX_WHOLE_LEN), suggestion: None };
    }
    let message = if let Some(field) = inner.strip_prefix("missing field `").and_then(|r| r.split('`').next()) {
        let at = if path.is_empty() { field.to_string() } else { format!("{path}.{field}") };
        return Issue { path: at.clone(), message: truncate(&format!("`{at}` is required"), MAX_MESSAGE_LEN), suggestion: None };
    } else if path.is_empty() {
        inner
    } else if inner.starts_with("must ") {
        format!("`{path}` {inner}")
    } else if let Some((_, expected)) = inner.strip_prefix("invalid type: ").and_then(|r| r.rsplit_once(", expected ")) {
        // serde's wording for the plain types, in the engine's (`thin` must be true or false).
        let expected = match expected {
            "a boolean" => "true or false",
            "a string" => "text",
            other => other,
        };
        format!("`{path}` must be {expected}")
    } else {
        format!("`{path}`: {inner}")
    };
    // serde quotes the caller's value, which may be long.
    Issue { path, message: truncate(&message, MAX_MESSAGE_LEN), suggestion: None }
}

/// What is wrong with `params` for the command `id` by its typed contract: the issues a call gets
/// at [`ParamCheck::Warn`] (the first is the refusal at [`ParamCheck::Strict`]). None for a
/// command without a contract, or an id the registry lacks (a runner in another process may be
/// another version).
pub fn issues(id: &str, params: &Value) -> Vec<Issue> {
    crate::cmd::find_command(id).and_then(|c| c.contract).map(|c| c.check(params)).unwrap_or_default()
}

/// Run `handler` as the command `id`, checking the call against the contract `id` publishes
/// (its [`CommandSpec::contract`](crate::cmd::CommandSpec::contract)), so the contract enforced is
/// always the one described. With the level off and no audit recorder, this is exactly
/// `handler(s, p)`, without looking anything up; a command without a contract just runs.
pub fn run_checked_as(s: &mut Session, p: &Value, id: &str, handler: Run) -> Result<Value> {
    if s.param_check == ParamCheck::Off && !record::enabled() {
        return handler(s, p);
    }
    match crate::cmd::find_command(id).and_then(|c| c.contract) {
        Some(contract) => run_checked(s, p, id, contract, handler),
        None => handler(s, p),
    }
}

/// Run `handler` as the command `id`, checking the call against `contract` as the session's
/// [`ParamCheck`] level asks (see the module docs). With the level off and no audit recorder,
/// this is exactly `handler(s, p)`.
pub fn run_checked(s: &mut Session, p: &Value, id: &str, contract: &dyn Contract, handler: Run) -> Result<Value> {
    let level = s.param_check;
    let recording = record::enabled();
    if level == ParamCheck::Off && !recording {
        return handler(s, p);
    }
    let issues = contract.check(p);
    if level == ParamCheck::Strict
        && let Some(first) = issues.first()
    {
        if recording {
            record::note(id, &issues, None, p);
        }
        return Err(bad(id, first.message.clone()));
    }
    let result = handler(s, p);
    if recording && !issues.is_empty() {
        record::note(id, &issues, Some(&result), p);
    }
    match result {
        Ok(Value::Object(mut o)) if level == ParamCheck::Warn && !issues.is_empty() => {
            o.insert("param_warnings".into(), json!(issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>()));
            Ok(Value::Object(o))
        }
        other => other,
    }
}
