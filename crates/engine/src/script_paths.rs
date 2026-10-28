//! The file paths in a script file, resolved against the script's folder (#8195).
//!
//! A script run from a file (`solidcraft-cli run r.json`, `eval`, `snapshot`, `mcp --in`) names
//! the files it reads and writes the way a repository keeps them: next to the script. A
//! relative path there means "beside the script", wherever the program was started, so a script
//! and its STEP files can be kept together and run from anywhere. [`relative_to`] rewrites the
//! script before it runs: each relative file path a command takes is joined to the script's
//! folder. Absolute paths, and every parameter that is not a file path, stay as written.
//!
//! Commands take paths under several names, and the same name means other things elsewhere
//! (`path` is a curve id in `sketch.text` and an occurrence path in `browser.visibility`), so
//! [`FILE_PARAMS`] lists each command's file parameters by hand. File paths nested in a render
//! setup (an environment's HDR, texture maps) are not in it yet and stay relative to the working
//! folder.

use std::path::Path;

use serde_json::Value;

/// Each command's parameters that name a file or folder on disk, read or written.
pub const FILE_PARAMS: &[(&str, &[&str])] = &[
    ("assembly.bom", &["path"]),
    ("canvas.decal", &["path"]),
    ("canvas.insert", &["path"]),
    ("component.extract", &["path"]),
    ("component.insert_design", &["path"]),
    ("component.relink", &["path"]),
    ("component.replace_with_link", &["path"]),
    ("doc.open", &["path"]),
    ("doc.recover", &["dir"]),
    ("doc.recovery_list", &["dir"]),
    ("file.export", &["path"]),
    ("file.export_view_dxf", &["path"]),
    ("file.insert_mesh", &["path"]),
    ("file.insert_step", &["path"]),
    ("file.save", &["path"]),
    ("file.save_as", &["path"]),
    ("file.save_mesh", &["path"]),
    ("library.apply", &["path"]),
    ("library.export", &["path"]),
    ("library.inspect", &["path"]),
    ("motion.export_animation", &["path"]),
    ("parameters.export", &["path"]),
    ("parameters.import", &["path"]),
    ("render.export", &["path"]),
    ("render.hdr.inspect", &["path"]),
    ("render.settings.capture_hdr", &["path"]),
    ("sheet.export_dxf", &["path"]),
    ("sketch.drawing_bounds", &["path"]),
    ("sketch.edit_text", &["font"]),
    ("sketch.export_dxf", &["path"]),
    ("sketch.insert_dxf", &["path"]),
    ("sketch.insert_svg", &["path"]),
    ("sketch.text", &["font"]),
    ("timeline.export_script", &["path"]),
];

/// `script` (a JSON array of `{command, params}`, or `{commands: [...]}`) with each relative file
/// path its commands take joined to `dir`. Steps it can't read are left for the script runner
/// to report.
pub fn relative_to(script: &Value, dir: &Path) -> Value {
    let mut out = script.clone();
    let steps = match &mut out {
        Value::Array(a) => Some(a),
        Value::Object(o) => o.get_mut("commands").and_then(Value::as_array_mut),
        _ => None,
    };
    for step in steps.into_iter().flatten() {
        let Some(id) = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str) else { continue };
        // Command ids match as `Session::execute` matches them, case and all.
        let Some(id) = crate::cmd::find_command(id).map(|c| c.id) else { continue };
        let Some((_, keys)) = FILE_PARAMS.iter().find(|(c, _)| *c == id) else { continue };
        let Some(params) = step.get_mut("params").and_then(Value::as_object_mut) else { continue };
        for key in *keys {
            if let Some(Value::String(path)) = params.get_mut(*key)
                && let Some(joined) = joined(dir, path, key)
            {
                *path = joined;
            }
        }
    }
    out
}

/// `path` under `dir`, when it is a relative file path; `None` leaves it as written.
fn joined(dir: &Path, path: &str, key: &str) -> Option<String> {
    let p = Path::new(path);
    if path.trim().is_empty() || !p.is_relative() || path.contains("://") {
        return None;
    }
    // A font is a file only when it looks like one: otherwise it is an installed family's name.
    // `~` and `%VAR%` expand to a folder of their own.
    if key == "font" {
        let lower = path.to_ascii_lowercase();
        let file = path.contains(['/', '\\']) || [".ttf", ".otf", ".ttc", ".otc"].iter().any(|e| lower.ends_with(e));
        if !file || path.starts_with('~') || path.contains('%') {
            return None;
        }
    }
    Some(dir.join(p).to_string_lossy().into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_listed_command_exists() {
        for (id, _) in FILE_PARAMS {
            assert_eq!(crate::cmd::find_command(id).map(|c| c.id), Some(*id), "{id} is not a command");
        }
    }

    #[test]
    fn relative_file_paths_join_the_script_folder_and_nothing_else_changes() {
        let dir = std::env::temp_dir().join("scripts");
        let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let absolute = std::env::temp_dir().join("elsewhere").join("part.step").to_string_lossy().into_owned();
        let script = json!({"commands": [
            {"command": "file.insert_step", "params": {"path": "cap.step", "name": "cap"}},
            {"command": "FILE.INSERT_MESH", "params": {"path": "parts/m.stl"}},
            {"command": "file.export", "params": {"path": absolute}},
            {"id": "file.save_as", "params": {"path": "../out/part.sc"}},
            {"command": "sketch.text", "params": {"text": "A", "font": "fonts/a.ttf", "path": "c1"}},
            {"command": "sketch.text", "params": {"text": "A", "font": "DejaVu Sans"}},
            {"command": "sketch.text", "params": {"text": "A", "font": "~/fonts/a.ttf"}},
            {"command": "browser.visibility", "params": {"visible": true, "path": "1"}},
            {"command": "file.save", "params": {}},
            {"command": "no.such_command", "params": {"path": "x.step"}},
            "not a step"
        ]});
        let out = relative_to(&script, &dir);
        let p = |i: usize, k: &str| out["commands"][i]["params"][k].clone();
        assert_eq!(p(0, "path"), json!(at("cap.step")));
        assert_eq!(p(0, "name"), json!("cap"));
        assert_eq!(p(1, "path"), json!(dir.join("parts/m.stl").to_string_lossy()));
        assert_eq!(p(2, "path"), json!(absolute));
        assert_eq!(p(3, "path"), json!(dir.join("../out/part.sc").to_string_lossy()));
        // The font file moves with the script; the baseline curve `path` is not a file.
        assert_eq!(p(4, "font"), json!(dir.join("fonts/a.ttf").to_string_lossy()));
        assert_eq!(p(4, "path"), json!("c1"));
        assert_eq!(p(5, "font"), json!("DejaVu Sans"));
        assert_eq!(p(6, "font"), json!("~/fonts/a.ttf"));
        assert_eq!(p(7, "path"), json!("1"));
        assert_eq!(out["commands"][8], script["commands"][8]);
        assert_eq!(out["commands"][9], script["commands"][9]);
        assert_eq!(out["commands"][10], json!("not a step"));
        // A bare array is a script too.
        let bare = relative_to(&json!([{"command": "doc.open", "params": {"path": "a.sc"}}]), &dir);
        assert_eq!(bare[0]["params"]["path"], json!(at("a.sc")));
    }
}
