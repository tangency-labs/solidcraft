//! What a crash draft may show. The report on disk keeps every path and name so it can be
//! replayed here. The draft that is copied or filed replaces the personal parts (#12736).
//!
//! Numbers, booleans, units, command ids, plane names and the app's own default names stay.
//! Another string becomes a numbered placeholder. The same text in one report keeps one number.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// The parts of a report a shared draft prints.
pub struct Shared {
    pub message: String,
    pub location: String,
    pub backtrace: Vec<String>,
    pub command: Value,
    /// Empty when nothing was replaced. Otherwise `Replaced before sharing: 1 path, 2 names`.
    pub replaced: String,
}

/// Redact `report` for a draft. `report` itself is not changed.
pub fn for_draft(report: &Value) -> Shared {
    let mut share = Share::new();
    let message = share.prose(report.get("message").and_then(Value::as_str).unwrap_or(""));
    let location = share.prose(report.get("location").and_then(Value::as_str).unwrap_or(""));
    let backtrace = report
        .get("backtrace")
        .and_then(Value::as_array)
        .map(|frames| frames.iter().filter_map(Value::as_str).take(25).map(|frame| share.prose(frame)).collect())
        .unwrap_or_default();
    let command = report.get("command").map(|command| share.command(command)).unwrap_or(Value::Null);
    Shared { message, location, backtrace, command, replaced: share.summary() }
}

struct Share {
    homes: Vec<String>,
    paths: u32,
    names: u32,
    texts: u32,
    params: u32,
    seen: BTreeMap<String, String>,
}

impl Share {
    fn new() -> Self {
        let mut homes = Vec::new();
        for key in ["USERPROFILE", "HOME"] {
            if let Ok(home) = std::env::var(key)
                && home.len() > 3
            {
                let forward = home.replace('\\', "/");
                if forward != home {
                    homes.push(forward);
                }
                homes.push(home);
            }
        }
        homes.sort_by_key(|home| std::cmp::Reverse(home.len()));
        homes.dedup();
        Self { homes, paths: 0, names: 0, texts: 0, params: 0, seen: BTreeMap::new() }
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (n, one, many) in
            [(self.paths, "path", "paths"), (self.names, "name", "names"), (self.texts, "text", "texts"), (self.params, "param", "params")]
        {
            if n == 1 {
                parts.push(format!("1 {one}"));
            } else if n > 1 {
                parts.push(format!("{n} {many}"));
            }
        }
        if parts.is_empty() { String::new() } else { format!("Replaced before sharing: {}", parts.join(", ")) }
    }

    fn placeholder(&mut self, kind: Kind, original: &str, extra: &str) -> String {
        if let Some(have) = self.seen.get(original) {
            return have.clone();
        }
        let n = match kind {
            Kind::Path => {
                self.paths += 1;
                self.paths
            }
            Kind::Name => {
                self.names += 1;
                self.names
            }
            Kind::Text => {
                self.texts += 1;
                self.texts
            }
            Kind::Param => {
                self.params += 1;
                self.params
            }
        };
        let text = match kind {
            Kind::Path if !extra.is_empty() => format!("<path #{n} .{extra}>"),
            Kind::Path => format!("<path #{n}>"),
            Kind::Name => format!("<name #{n}>"),
            Kind::Text => format!("<text #{n}, {extra} chars>"),
            Kind::Param => format!("<param #{n}>"),
        };
        self.seen.insert(original.to_string(), text.clone());
        text
    }

    /// A sentence: home folders become `~`, a Cargo path keeps the crate onward, a loaded
    /// module keeps its file name and where it came from, and a path into someone's files
    /// becomes a placeholder. The rest of the sentence stays.
    fn prose(&mut self, text: &str) -> String {
        let folded = self.fold_home(text);
        let mut out = String::with_capacity(folded.len());
        let mut rest = folded.as_str();
        while !rest.is_empty() {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (first, mut after) = rest.split_at(end);
            // `Program Files` is two words. A path keeps its following words until the file or the load offset.
            let mut joined = String::new();
            let token = if path_token(first) {
                joined.push_str(first);
                while let Some((next, rest_after)) = extend_path(&joined, after) {
                    joined = next;
                    after = rest_after;
                }
                joined.as_str()
            } else {
                first
            };
            let (bare, punct) = trim_punct(token);
            if bare.is_empty() {
                out.push_str(token);
            } else if let Some(short) = shorten_cargo(bare) {
                out.push_str(&short);
                out.push_str(punct);
            } else if let Some(module) = native_module(bare) {
                out.push_str(&module);
                out.push_str(punct);
            } else if user_path(bare) {
                let ext = path_ext(bare);
                out.push_str(&self.placeholder(Kind::Path, bare, ext));
                out.push_str(punct);
            } else {
                out.push_str(token);
            }
            if after.is_empty() {
                break;
            }
            let space = after.find(|c: char| !c.is_whitespace()).unwrap_or(after.len());
            out.push_str(after.get(..space).unwrap_or_default());
            rest = after.get(space..).unwrap_or_default();
        }
        out
    }

    fn fold_home(&self, text: &str) -> String {
        let mut out = text.to_string();
        for home in &self.homes {
            if out.contains(home) {
                out = out.replace(home, "~");
            }
        }
        out
    }

    fn command(&mut self, command: &Value) -> Value {
        let Some(object) = command.as_object() else { return self.value(command, None, None) };
        let id = object.get("command").and_then(Value::as_str).unwrap_or("");
        if object.get("params_omitted").and_then(Value::as_bool) == Some(true) || object.get("params").is_none() {
            return json_command(id);
        }
        let schema = crate::find_command(id).and_then(|spec| spec.contract).map(|contract| contract.input_schema().as_value().clone());
        let params = object.get("params").map(|params| self.value(params, schema.as_ref(), schema.as_ref())).unwrap_or(Value::Null);
        serde_json::json!({"command": id, "params": params})
    }

    fn value(&mut self, value: &Value, schema: Option<&Value>, root: Option<&Value>) -> Value {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
            Value::String(text) => Value::String(self.param_string(text, schema, root)),
            Value::Array(items) => {
                let item_schema = schema.and_then(|schema| resolve(root, schema).get("items"));
                Value::Array(items.iter().take(256).map(|item| self.value(item, item_schema, root)).collect())
            }
            Value::Object(object) => Value::Object(self.object(object, schema, root)),
        }
    }

    fn object(&mut self, object: &Map<String, Value>, schema: Option<&Value>, root: Option<&Value>) -> Map<String, Value> {
        let resolved = schema.map(|schema| resolve(root, schema));
        let properties = resolved.and_then(|schema| schema.get("properties"));
        let mut out = Map::new();
        for (key, value) in object.iter().take(256) {
            let child = properties.and_then(|properties| properties.get(key));
            out.insert(key.clone(), self.value(value, child, root.or(schema)));
        }
        out
    }

    fn param_string(&mut self, text: &str, schema: Option<&Value>, root: Option<&Value>) -> String {
        let schema = schema.map(|schema| resolve(root, schema));
        if schema.is_some_and(|schema| enum_has(schema, text) || alt_enum_has(schema, root, text))
            || kept_word(text)
            || crate::doc::expr::is_literal(text)
        {
            return text.to_string();
        }
        if let Some(expression) = self.expression(text) {
            return expression;
        }
        let folded = self.fold_home(text);
        if is_path(&folded) || user_path(&folded) {
            return self.placeholder(Kind::Path, text, path_ext(&folded));
        }
        let chars = text.chars().count();
        if chars > 24 || text.chars().any(char::is_whitespace) {
            self.placeholder(Kind::Text, text, &chars.to_string())
        } else {
            self.placeholder(Kind::Name, text, "")
        }
    }

    fn expression(&mut self, text: &str) -> Option<String> {
        if !text.chars().any(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '*' | '/')) {
            return None;
        }
        let refs = crate::doc::expr::references(text);
        if refs.is_empty() {
            return None;
        }
        let mut out = text.to_string();
        for name in refs {
            let placeholder = self.placeholder(Kind::Param, &name, "");
            out = crate::doc::expr::rename_reference(&out, &name, &placeholder);
        }
        Some(out)
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Path,
    Name,
    Text,
    Param,
}

fn json_command(id: &str) -> Value {
    serde_json::json!({"command": id})
}

fn kept_word(text: &str) -> bool {
    matches!(text, "XY" | "XZ" | "YZ" | "YX" | "ZX" | "ZY" | "Top" | "Front" | "Right" | "Untitled")
        || crate::find_command(text).is_some()
        || default_name(text)
}

fn default_name(text: &str) -> bool {
    for prefix in ["Body", "Sketch", "Component", "Feature", "Plane", "Group"] {
        if let Some(rest) = text.strip_prefix(prefix)
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn resolve<'a>(root: Option<&'a Value>, schema: &'a Value) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else { return schema };
    let Some(name) = reference.strip_prefix("#/$defs/") else { return schema };
    root.and_then(|root| root.get("$defs")).and_then(|defs| defs.get(name)).unwrap_or(schema)
}

fn enum_has(schema: &Value, text: &str) -> bool {
    schema.get("enum").and_then(Value::as_array).is_some_and(|values| values.iter().any(|value| value.as_str() == Some(text)))
}

fn alt_enum_has(schema: &Value, root: Option<&Value>, text: &str) -> bool {
    let Some(alts) = schema.get("anyOf").or_else(|| schema.get("oneOf")).and_then(Value::as_array) else { return false };
    alts.iter().any(|alt| enum_has(resolve(root, alt), text))
}

fn path_token(text: &str) -> bool {
    text.contains('/') || text.contains('\\') || text.starts_with('~') || (text.len() > 2 && text.as_bytes().get(1) == Some(&b':'))
}

/// The next word still belongs to the path (`Files\SolidCraft\solidcraft.exe+0x10`).
fn extend_path<'a>(token: &str, after: &'a str) -> Option<(String, &'a str)> {
    let trimmed = after.trim_start();
    if trimmed.is_empty() || path_finished(token) {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let next = trimmed.get(..end)?;
    if !next.contains('/') && !next.contains('\\') && !next.contains("+0x") && !next.contains("+0X") && path_ext(next).is_empty() {
        return None;
    }
    let space = after.len() - trimmed.len();
    let mut joined = String::with_capacity(token.len() + space + end);
    joined.push_str(token);
    joined.push_str(after.get(..space + end).unwrap_or_default());
    Some((joined, after.get(space + end..).unwrap_or_default()))
}

fn path_finished(token: &str) -> bool {
    let (bare, _) = trim_punct(token);
    split_load_offset(&bare.replace('\\', "/")).is_some() || !path_ext(bare).is_empty()
}

fn is_path(text: &str) -> bool {
    text.contains('/') || text.contains('\\') || (text.len() > 2 && text.as_bytes().get(1) == Some(&b':'))
}

fn user_path(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('~') || text.starts_with("/home/") || text.starts_with("/Users/") || text.contains("\\Users\\") || text.contains("/Users/")
}

/// `C:\Windows\System32\ntdll.dll+0x1234` becomes `ntdll.dll (system)+0x1234`. A bare
/// `driver.dll+0x1234` is already a file name and stays as it was recorded.
fn native_module(token: &str) -> Option<String> {
    let forward = token.replace('\\', "/");
    if !forward.contains('/') {
        return None;
    }
    let (path, offset) = split_load_offset(&forward)?;
    let name = path.rsplit('/').next().filter(|name| !name.is_empty())?;
    let origin = if user_path(token) || user_path(&forward) || forward.starts_with("~/") {
        "user folder"
    } else if system_path(&forward) {
        "system"
    } else {
        "app folder"
    };
    Some(format!("{name} ({origin}){offset}"))
}

fn split_load_offset(path: &str) -> Option<(&str, &str)> {
    let at = path.rfind("+0x").or_else(|| path.rfind("+0X"))?;
    let offset = path.get(at..)?;
    let hex = offset.get(3..)?;
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((path.get(..at)?, offset))
}

fn system_path(forward: &str) -> bool {
    let lower = forward.to_ascii_lowercase();
    lower.contains("/windows/")
        || lower.contains("/system32/")
        || lower.contains("/syswow64/")
        || lower.contains("/winsxs/")
        || lower.starts_with("/usr/")
        || lower.starts_with("/lib/")
        || lower.starts_with("/lib64/")
        || lower.starts_with("/system/")
        || lower.starts_with("/library/")
}

fn path_ext(text: &str) -> &'static str {
    let name = text.rsplit(['/', '\\']).next().unwrap_or(text);
    let Some(dot) = name.rfind('.') else { return "" };
    let ext = name.get(dot + 1..).unwrap_or_default();
    let ext = ext.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
    match ext {
        "sc" | "solidcraft" | "step" | "stp" | "stl" | "json" | "3mf" | "obj" | "dxf" | "iges" | "igs" => ext_static(ext),
        _ => "",
    }
}

fn ext_static(ext: &str) -> &'static str {
    match ext {
        "sc" => "sc",
        "solidcraft" => "solidcraft",
        "step" => "step",
        "stp" => "stp",
        "stl" => "stl",
        "json" => "json",
        "3mf" => "3mf",
        "obj" => "obj",
        "dxf" => "dxf",
        "iges" => "iges",
        "igs" => "igs",
        _ => "",
    }
}

fn shorten_cargo(token: &str) -> Option<String> {
    let forward = token.replace('\\', "/");
    let marker = "registry/src/";
    let at = forward.find(marker)?;
    let rest = forward.get(at + marker.len()..)?;
    let slash = rest.find('/')?;
    rest.get(slash + 1..).filter(|rest| !rest.is_empty()).map(str::to_string)
}

fn trim_punct(token: &str) -> (&str, &str) {
    let bare = token.trim_end_matches([',', ';', ':', ')', ']', '}', '.']);
    (bare, token.get(bare.len()..).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::for_draft;

    const CANARY: &str = "zzCanarySecret";

    #[test]
    fn a_shared_draft_keeps_numbers_and_drops_names_and_paths() {
        let report = json!({
            "message": format!("could not read /home/ada/parts/{CANARY}.sc"),
            "location": "C:\\Users\\ada\\src\\crash.rs:12",
            "backtrace": ["solidcraft_engine::crash (C:\\Users\\ada\\.cargo\\registry\\src\\index\\solidcraft-engine-0.40.0\\src\\crash.rs:12)"],
            "command": {
                "command": "solid.extrude",
                "params": {
                    "distance": 20,
                    "name": CANARY,
                    "note": format!("made by {CANARY}"),
                    "profile": "Body3",
                    "plane": "XY",
                    "expression": "width + 2 mm",
                    "file": "C:\\Users\\ada\\bracket.sc"
                }
            }
        });
        let shared = for_draft(&report);
        let shown = format!("{} {} {:?} {}", shared.message, shared.location, shared.backtrace, shared.command);
        assert!(!shown.contains(CANARY), "{shown}");
        assert!(!shown.contains("ada"), "{shown}");
        assert_eq!(shared.command["params"]["distance"].as_i64(), Some(20), "{}", shared.command);
        assert_eq!(shared.command["params"]["profile"].as_str(), Some("Body3"));
        assert_eq!(shared.command["params"]["plane"].as_str(), Some("XY"));
        assert_eq!(shared.command["params"]["expression"].as_str(), Some("<param #1> + 2 mm"));
        assert!(shared.command["params"]["file"].as_str().unwrap_or("").starts_with("<path #"), "{}", shared.command);
        assert!(shared.replaced.contains("path") && shared.replaced.contains("name"), "{}", shared.replaced);
        assert!(shared.backtrace[0].contains("solidcraft-engine-0.40.0/src/crash.rs:12"), "{}", shared.backtrace[0]);
    }

    #[test]
    fn every_commands_string_parameter_keeps_a_canary_out_of_the_draft() {
        for spec in crate::command_specs() {
            let mut params = serde_json::Map::new();
            let keys: Vec<String> = spec.contract.map(|contract| contract.allowed_keys().iter().cloned().collect()).unwrap_or_default();
            let keys = if keys.is_empty() { vec!["name".to_string()] } else { keys };
            for key in &keys {
                params.insert(key.clone(), json!(format!("{CANARY}-{key}")));
            }
            let report = json!({"command": {"command": spec.id, "params": params}});
            let shared = for_draft(&report);
            let shown = shared.command.to_string();
            assert!(!shown.contains(CANARY), "{} leaked: {shown}", spec.id);
        }
    }

    #[test]
    fn a_native_module_keeps_its_file_name_and_drops_the_folder() {
        let report = json!({
            "backtrace": [
                "at C:\\Windows\\System32\\ntdll.dll+0x1234",
                "at C:\\Users\\ada\\AppData\\Local\\solidcraft\\driver.dll+0x20",
                "at C:\\Program Files\\SolidCraft\\solidcraft.exe+0x10",
                "driver.dll+0x1234"
            ]
        });
        let shared = for_draft(&report);
        assert_eq!(shared.backtrace[0], "at ntdll.dll (system)+0x1234");
        assert_eq!(shared.backtrace[1], "at driver.dll (user folder)+0x20");
        assert_eq!(shared.backtrace[2], "at solidcraft.exe (app folder)+0x10");
        assert_eq!(shared.backtrace[3], "driver.dll+0x1234");
        assert!(!shared.backtrace.join(" ").contains("ada"));
    }

    #[test]
    fn a_command_whose_parameters_were_not_recorded_shows_its_name_only() {
        let shared = for_draft(&json!({"command": {"command": "file.save", "params_omitted": true}}));
        assert_eq!(shared.command, json!({"command": "file.save"}));
        assert!(shared.replaced.is_empty());
    }
}
