//! `find_commands` and `call_command`: the whole command registry behind two small tools, so an
//! agent loads a short tool list and looks up only the commands it needs. `find_commands` ranks
//! the registry by the query's words and gives each match's parameters as a short signature
//! read from its input schema; `call_command` checks the parameters against that schema and
//! then runs the command as `execute` does.

use serde_json::{Map, Value, json};
use solidcraft_engine::CommandSpec;

/// Matches `find_commands` gives when the call names no `limit`.
pub(crate) const DEFAULT_LIMIT: usize = 5;
/// Most matches one `find_commands` call gives.
pub(crate) const MAX_LIMIT: usize = 25;
/// How much of a command's parameter notes a match carries unless `detail` asks for all of it.
const DOC_CHARS: usize = 400;
/// Longest query read; the rest is ignored.
const MAX_QUERY_CHARS: usize = 200;
/// Most issues one refused `call_command` names.
const MAX_ISSUES: usize = 5;

/// The lower-case words of `s`: its runs of ASCII letters and digits.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_ascii_lowercase).collect()
}

/// How well `spec` matches the query `query` (its words `q`); 0 is no match. A word in the id
/// counts most, then the label, the toolbar tab and panel, and the parameter notes least; the
/// exact id wins outright, and a planned command (not built yet) sinks below working ones.
fn score(spec: &CommandSpec, query: &str, q: &[String]) -> f64 {
    let id = words(spec.id);
    let label = words(spec.label);
    let place = words(&format!("{} {}", spec.tab, spec.panel));
    let doc = words(spec.params);
    let mut s: f64 = 0.0;
    for w in q {
        if id.contains(w) {
            s += 4.0;
        } else if id.iter().any(|t| t.starts_with(w.as_str())) {
            s += 3.0;
        }
        if label.contains(w) {
            s += 2.0;
        }
        if place.contains(w) {
            s += 1.0;
        }
        if doc.contains(w) {
            s += 0.5;
        }
    }
    if s > 0.0 && spec.id.eq_ignore_ascii_case(query.trim()) {
        s += 100.0;
    }
    if s > 0.0 && spec.planned.is_some() {
        s -= 3.0;
    }
    s.max(0.0)
}

/// `find_commands {query, limit?, detail?}`: the best matches in `specs`, each
/// `{id, label, planned?, input?, types?, doc?}`, and how many commands matched at all.
pub(crate) fn find_commands(specs: &[&CommandSpec], a: &Map<String, Value>) -> Result<Value, String> {
    let query: String = a
        .get("query")
        .and_then(Value::as_str)
        .ok_or("`query` is required: words to look for, e.g. \"fillet\"")?
        .chars()
        .take(MAX_QUERY_CHARS)
        .collect();
    let q = words(&query);
    if q.is_empty() {
        return Err("`query` needs at least one word, e.g. \"fillet\" or \"sketch circle\"".into());
    }
    let limit = a.get("limit").and_then(Value::as_u64).map_or(DEFAULT_LIMIT, |n| (n as usize).clamp(1, MAX_LIMIT));
    let detail = a.get("detail").and_then(Value::as_bool).unwrap_or(false);
    let mut ranked: Vec<(f64, &CommandSpec)> = specs.iter().map(|s| (score(s, &query, &q), *s)).filter(|(s, _)| *s > 0.0).collect();
    // Best first; among equals the shorter id (the plainer command), then the id itself, so the
    // same query always lists the same commands in the same order.
    ranked.sort_by(|(sa, a), (sb, b)| sb.total_cmp(sa).then(a.id.len().cmp(&b.id.len())).then(a.id.cmp(b.id)));
    let total = ranked.len();
    let matches: Vec<Value> = ranked.iter().take(limit).map(|(_, spec)| entry(spec, detail)).collect();
    let mut reply = json!({"matches": matches, "total": total});
    if total == 0 {
        reply["hint"] = json!("no command matched: try other words (an id part such as \"extrude\", a label, a toolbar panel)");
    }
    Ok(reply)
}

/// One match: the id, the label, the parameters as a signature with the named types it uses,
/// and the start of the parameter notes (all of them with `detail`).
fn entry(spec: &CommandSpec, detail: bool) -> Value {
    let mut e = json!({"id": spec.id, "label": spec.label});
    if spec.planned.is_some() {
        e["planned"] = json!(true);
    }
    if let Some(contract) = spec.contract {
        let schema = contract.input_schema().to_value();
        let mut sig = Signature::new(schema.get("$defs"));
        e["input"] = json!(sig.of(&schema));
        if !sig.types.is_empty() {
            e["types"] = Value::Object(sig.types.into_iter().map(|(k, v)| (k, Value::String(v))).collect());
        }
    }
    if !spec.params.is_empty() {
        e["doc"] = json!(if detail { spec.params.to_string() } else { clip(spec.params, DOC_CHARS) });
    }
    e
}

/// `s` cut to at most `n` characters, with an ellipsis when cut.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).chain(['…']).collect() }
}

/// A JSON Schema written as a short signature: `{length: LengthExpr, name?: str}`, `a|b` for the
/// choices of an enum, `T[]` for an array, and each named type (`$defs`) written once in
/// `types`. Descriptions are left out: the command's notes carry them.
struct Signature<'a> {
    defs: Option<&'a Value>,
    /// The named types used so far, in the order first met, with their signatures.
    types: Vec<(String, String)>,
}

impl<'a> Signature<'a> {
    fn new(defs: Option<&'a Value>) -> Self {
        Self { defs, types: Vec::new() }
    }

    fn named(&mut self, reference: &str) -> String {
        let name = reference.rsplit('/').next().unwrap_or(reference).to_string();
        if !self.types.iter().any(|(n, _)| *n == name) {
            // Placeholder first, so a type that refers to itself ends.
            self.types.push((name.clone(), String::new()));
            let def = self.defs.and_then(|d| d.get(&name)).cloned().unwrap_or(Value::Null);
            let sig = self.of(&def);
            if let Some(slot) = self.types.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = sig;
            }
        }
        name
    }

    fn of(&mut self, s: &Value) -> String {
        let Some(o) = s.as_object() else { return "any".into() };
        if let Some(r) = o.get("$ref").and_then(Value::as_str) {
            return self.named(r);
        }
        if let Some(choices) = o.get("enum").and_then(Value::as_array) {
            return choices.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_string)).collect::<Vec<_>>().join("|");
        }
        if let Some(c) = o.get("const") {
            return c.to_string();
        }
        // An object whose alternatives only say which of its fields it needs
        // (`anyOf: [{required: [line]}, {required: [points]}]`): its fields, then the choice.
        if o.contains_key("properties")
            && let Some(alts) = o.get("anyOf").or_else(|| o.get("oneOf")).and_then(Value::as_array)
            && alts.iter().all(|a| a.as_object().is_some_and(|a| a.keys().all(|k| k == "required")))
        {
            let needs: Vec<String> = alts
                .iter()
                .map(|a| a["required"].as_array().map(|r| r.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" + ")).unwrap_or_default())
                .collect();
            return format!("{} (needs one of: {})", self.object(o), needs.join(" | "));
        }
        if let Some(alts) = o.get("anyOf").or_else(|| o.get("oneOf")).and_then(Value::as_array) {
            let mut parts: Vec<String> = Vec::new();
            for alt in alts {
                let t = self.of(alt);
                if !parts.contains(&t) {
                    parts.push(t);
                }
            }
            // `T | null` is an optional field's way of saying it may be left out.
            if parts.len() > 1 {
                parts.retain(|p| p != "null");
            }
            return parts.join(" | ");
        }
        match o.get("type") {
            Some(Value::Array(types)) => {
                let mut parts: Vec<String> = Vec::new();
                for t in types {
                    let mut one = o.clone();
                    one.insert("type".into(), t.clone());
                    let sig = self.of(&Value::Object(one));
                    if !parts.contains(&sig) {
                        parts.push(sig);
                    }
                }
                if parts.len() > 1 {
                    parts.retain(|p| p != "null");
                }
                parts.join(" | ")
            }
            Some(Value::String(t)) => match t.as_str() {
                "array" => {
                    let item = o.get("items").map_or_else(|| "any".to_string(), |i| self.of(i));
                    if item.contains(' ') || item.contains('|') { format!("({item})[]") } else { format!("{item}[]") }
                }
                "object" => self.object(o),
                "integer" => "int".into(),
                "number" => "num".into(),
                "string" => "str".into(),
                "boolean" => "bool".into(),
                other => other.to_string(),
            },
            _ if o.contains_key("properties") => self.object(o),
            _ => "any".into(),
        }
    }

    /// `{required: T, optional?: T}`: the required fields first, each group in schema order.
    fn object(&mut self, o: &Map<String, Value>) -> String {
        let Some(props) = o.get("properties").and_then(Value::as_object).filter(|p| !p.is_empty()) else { return "object".into() };
        let required: Vec<&str> =
            o.get("required").and_then(Value::as_array).map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let mut fields: Vec<(bool, &String, &Value)> = props.iter().map(|(k, v)| (!required.contains(&k.as_str()), k, v)).collect();
        fields.sort_by_key(|(optional, _, _)| *optional);
        let parts: Vec<String> =
            fields.into_iter().map(|(optional, k, v)| format!("{k}{}: {}", if optional { "?" } else { "" }, self.of(v))).collect();
        format!("{{{}}}", parts.join(", "))
    }
}

/// Why `params` don't fit the command `id`'s schema, in the engine's words (as a strict
/// parameter check refuses them); `None` when they fit, the command has no schema yet, or the
/// registry lacks the id (the command itself then answers).
pub(crate) fn refusal(id: &str, params: &Value) -> Option<String> {
    let issues = solidcraft_engine::schema::issues(id, params);
    let first = issues.first()?;
    let mut text = format!("invalid parameters for `{id}`: {}", first.message);
    let more: Vec<&str> = issues.iter().skip(1).take(MAX_ISSUES - 1).map(|i| i.message.as_str()).collect();
    if !more.is_empty() {
        text.push_str(&format!("; also {}", more.join("; ")));
    }
    text.push_str(&format!(" (find_commands {{\"query\": \"{id}\", \"detail\": true}} gives its parameters)"));
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs() -> Vec<&'static CommandSpec> {
        solidcraft_engine::command_specs()
    }

    fn find(a: Value) -> Value {
        find_commands(&specs(), a.as_object().unwrap()).unwrap()
    }

    fn ids(reply: &Value) -> Vec<String> {
        reply["matches"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn finds_commands_by_their_words() {
        assert_eq!(ids(&find(json!({"query": "solid.box"})))[0], "solid.box", "the exact id comes first");
        assert!(ids(&find(json!({"query": "fillet"}))).contains(&"solid.fillet".to_string()));
        assert_eq!(ids(&find(json!({"query": "revolute joint"})))[0], "joint.create");
        assert!(ids(&find(json!({"query": "sketch polygon"}))).iter().all(|id| id.starts_with("sketch.")));
        let five = find(json!({"query": "sketch"}));
        assert_eq!(five["matches"].as_array().map(Vec::len), Some(DEFAULT_LIMIT));
        assert!(five["total"].as_u64().unwrap() > 50, "{five}");
        assert_eq!(find(json!({"query": "sketch", "limit": 1000}))["matches"].as_array().map(Vec::len), Some(MAX_LIMIT));
        assert_eq!(find(json!({"query": "fillet"})), find(json!({"query": "fillet"})), "the same query, the same reply");
        let none = find(json!({"query": "zzqqxx"}));
        assert_eq!(none["total"], 0);
        assert!(none["hint"].is_string());
        assert!(find_commands(&specs(), json!({"query": " .. "}).as_object().unwrap()).is_err());
        assert!(find_commands(&specs(), json!({}).as_object().unwrap()).is_err());
    }

    #[test]
    fn a_match_carries_its_parameters_compactly() {
        let reply = find(json!({"query": "solid.box", "limit": 1}));
        let m = &reply["matches"][0];
        let input = m["input"].as_str().unwrap();
        // Required fields first, optional ones marked, and `| null` left off optional fields.
        assert!(input.starts_with("{height: LengthExpr, length: LengthExpr, width: LengthExpr"), "{input}");
        assert!(input.contains("name?: str,") || input.contains("name?: str}"), "{input}");
        assert!(!input.contains("null"), "{input}");
        assert_eq!(m["types"]["LengthExpr"], "num | str", "{m}");
        assert!(m["types"]["Operation"].as_str().unwrap().starts_with("new|join|cut|intersect"), "{m}");
        // Far smaller than the full description.
        let full = serde_json::to_string(&solidcraft_engine::find_command("solid.box").unwrap().contract.unwrap().input_schema().to_value()).unwrap();
        assert!(m.to_string().len() * 3 < full.len(), "{} vs {}", m.to_string().len(), full.len());
        // The notes are cut unless asked for in full.
        let fillet = solidcraft_engine::find_command("solid.fillet").unwrap();
        assert!(fillet.params.chars().count() > DOC_CHARS);
        let short = find(json!({"query": "solid.fillet", "limit": 1}));
        assert_eq!(short["matches"][0]["doc"].as_str().unwrap().chars().count(), DOC_CHARS);
        let long = find(json!({"query": "solid.fillet", "limit": 1, "detail": true}));
        assert_eq!(long["matches"][0]["doc"], fillet.params);
    }

    #[test]
    fn every_schema_has_a_signature() {
        for spec in specs() {
            let e = entry(spec, false);
            if spec.contract.is_some() {
                let input = e["input"].as_str().unwrap_or_default();
                assert!(!input.is_empty() && input != "any", "{}: {input}", spec.id);
                for (name, sig) in e["types"].as_object().into_iter().flatten() {
                    assert!(!sig.as_str().unwrap_or_default().is_empty(), "{}: type {name} is empty", spec.id);
                }
            }
        }
    }

    #[test]
    fn fields_that_need_one_of_several_keys() {
        let schema = json!({"type": "object", "anyOf": [{"required": ["line"]}, {"required": ["points"]}],
            "properties": {"line": {"type": "string"}, "points": {"type": "array", "items": {"type": "string"}}}});
        assert_eq!(Signature::new(None).of(&schema), "{line?: str, points?: str[]} (needs one of: line | points)");
    }

    #[test]
    fn a_self_referring_type_ends() {
        let schema = json!({"type": "object", "properties": {"tree": {"$ref": "#/$defs/Node"}}, "required": ["tree"],
            "$defs": {"Node": {"type": "object", "properties": {"kids": {"type": "array", "items": {"$ref": "#/$defs/Node"}}}}}});
        let mut sig = Signature::new(schema.get("$defs"));
        assert_eq!(sig.of(&schema), "{tree: Node}");
        assert_eq!(sig.types, vec![("Node".to_string(), "{kids?: Node[]}".to_string())]);
    }

    #[test]
    fn refusal_names_the_field() {
        let why = refusal("solid.box", &json!({"lenght": 10, "width": 20, "height": 30})).unwrap();
        assert!(why.starts_with("invalid parameters for `solid.box`:") && why.contains("lenght"), "{why}");
        assert!(refusal("solid.box", &json!({"length": 10, "width": 20, "height": 30})).is_none());
        assert!(refusal("no.such_command", &json!({})).is_none(), "the command itself says it is unknown");
    }
}
