//! Settings profiles (#6237): named sets of the input and interface preferences, kept by the
//! app. The engine holds no preferences, so these commands only check and echo their
//! parameters; the desktop and web apps act on the echo (`settings_profiles` in the UI crate).
//! Run without an app (CLI, scripts), they do nothing else.

use serde_json::{Map, Value, json};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

/// Most characters in a profile's name.
pub const MAX_PROFILE_NAME: usize = 64;
/// Most bytes in a path given to Export or Import.
const MAX_PATH: usize = 4096;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("settings.profile.list", "Settings Profiles", list)
        .icon("list")
        .noundo()
        .doc_only()
        .params("→ profiles: [name], active: name|null; handled by the app"),
    CommandSpec::new("settings.profile.save", "Save Settings Profile", save)
        .icon("save")
        .noundo()
        .doc_only()
        .params("name: 1..64 characters → name; keeps the current input and interface settings under `name` (replacing a profile of that name) and makes it the active one"),
    CommandSpec::new("settings.profile.apply", "Apply Settings Profile", apply)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("name → name, applied, skipped: [preference]; switches every setting the profile holds at once"),
    CommandSpec::new("settings.profile.rename", "Rename Settings Profile", rename)
        .icon("rename")
        .noundo()
        .doc_only()
        .params("name, to: 1..64 characters → name"),
    CommandSpec::new("settings.profile.delete", "Delete Settings Profile", delete)
        .icon("delete")
        .noundo()
        .doc_only()
        .params("name → deleted"),
    CommandSpec::new("settings.profile.export", "Export Settings Profile", export)
        .icon("export")
        .noundo()
        .doc_only()
        .params("name, path?: file → profile (a `solidcraft-profile/1` document), written to `path` when given; never opens a dialog"),
    CommandSpec::new("settings.profile.import", "Import Settings Profile", import)
        .icon("import")
        .noundo()
        .doc_only()
        .params("path: file | profile: `solidcraft-profile/1` document, name?: 1..64 characters → name; adds the profile without switching to it"),
];

/// A profile name as given: trimmed, 1 to [`MAX_PROFILE_NAME`] characters, no control characters.
pub fn profile_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_PROFILE_NAME && !name.chars().any(char::is_control)).then_some(name)
}

fn name_param<'a>(cmd: &str, p: &'a Value, key: &str) -> Result<&'a str> {
    str_(p, key)
        .and_then(profile_name)
        .ok_or_else(|| bad(cmd, format!("`{key}` must be 1 to {MAX_PROFILE_NAME} characters, without control characters")))
}

fn path_param<'a>(cmd: &str, p: &'a Value) -> Result<Option<&'a str>> {
    match p.get("path") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(path)) if !path.trim().is_empty() && path.len() <= MAX_PATH => Ok(Some(path)),
        Some(_) => Err(bad(cmd, format!("`path` must be a file path of at most {MAX_PATH} bytes"))),
    }
}

fn list(_s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({}))
}

fn save(_s: &mut Session, p: &Value) -> Result<Value> {
    Ok(json!({ "name": name_param("settings.profile.save", p, "name")? }))
}

fn apply(_s: &mut Session, p: &Value) -> Result<Value> {
    Ok(json!({ "name": name_param("settings.profile.apply", p, "name")? }))
}

fn rename(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "settings.profile.rename";
    Ok(json!({ "name": name_param(cmd, p, "name")?, "to": name_param(cmd, p, "to")? }))
}

fn delete(_s: &mut Session, p: &Value) -> Result<Value> {
    Ok(json!({ "name": name_param("settings.profile.delete", p, "name")? }))
}

fn export(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "settings.profile.export";
    let mut out = Map::new();
    out.insert("name".into(), json!(name_param(cmd, p, "name")?));
    if let Some(path) = path_param(cmd, p)? {
        out.insert("path".into(), json!(path));
    }
    Ok(Value::Object(out))
}

fn import(_s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "settings.profile.import";
    let mut out = Map::new();
    let (key, value) = match (path_param(cmd, p)?, p.get("profile")) {
        (Some(path), None) => ("path", json!(path)),
        (None, Some(profile)) if profile.is_object() => ("profile", profile.clone()),
        _ => return Err(bad(cmd, "give either `path` or `profile` (a `solidcraft-profile/1` document)")),
    };
    out.insert(key.into(), value);
    if p.get("name").is_some_and(|name| !name.is_null()) {
        out.insert("name".into(), json!(name_param(cmd, p, "name")?));
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, p: Value) -> std::result::Result<Value, String> {
        let mut s = Session::default();
        s.execute(id, &p).map_err(|e| e.to_string())
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(run("settings.profile.save", json!({"name":"  Laptop "})).unwrap(), json!({"name":"Laptop"}));
        for name in [json!(""), json!("   "), json!("a\nb"), json!("x".repeat(65)), json!(3), Value::Null] {
            assert!(run("settings.profile.save", json!({ "name": name })).is_err(), "{name}");
        }
        assert!(run("settings.profile.save", json!({"name":"é".repeat(64)})).is_ok());
        assert!(run("settings.profile.rename", json!({"name":"A"})).is_err());
    }

    #[test]
    fn import_takes_a_path_or_a_document_but_not_both() {
        assert_eq!(run("settings.profile.import", json!({"path":"/tmp/p.json"})).unwrap(), json!({"path":"/tmp/p.json"}));
        let doc = json!({"format":"solidcraft-profile/1"});
        assert_eq!(run("settings.profile.import", json!({"profile":doc, "name":"B"})).unwrap(), json!({"profile":doc, "name":"B"}));
        assert!(run("settings.profile.import", json!({"path":"/tmp/p.json", "profile":doc})).is_err());
        assert!(run("settings.profile.import", json!({"profile":"text"})).is_err());
        assert!(run("settings.profile.import", json!({})).is_err());
        assert!(run("settings.profile.export", json!({"name":"A", "path":"x".repeat(5000)})).is_err());
    }

    #[test]
    fn profile_commands_leave_the_design_and_its_history_alone() {
        let mut s = Session::default();
        let doc = s.doc.to_json();
        for (id, p) in [
            ("settings.profile.save", json!({"name":"A"})),
            ("settings.profile.apply", json!({"name":"A"})),
            ("settings.profile.list", json!({})),
            ("settings.profile.delete", json!({"name":"A"})),
        ] {
            s.execute(id, &p).unwrap();
        }
        assert_eq!(s.doc.to_json(), doc);
        assert!(s.history.is_empty());
    }
}
