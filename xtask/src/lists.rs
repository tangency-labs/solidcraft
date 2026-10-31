//! `cargo xtask lists [--fix]`: files that many pull requests add to are kept in a canonical,
//! sorted form, so two changes that each add an item meet only when their items are neighbours
//! (CONTRIBUTING.md, "Lists many changes add to"). Checked by `cargo xtask ci` and the Lists
//! workflow; `--fix` rewrites them.
//!
//! - The message catalogs (`assets/locales/*.json`): one message per line, sorted by id, no
//!   duplicate ids, as `serde_json` pretty-prints them.
//! - `tests/KNOWN_FAILING_TESTS` and `.smlib`: one test a line with its cause, sorted, each test once
//!   (checked only; `cargo xtask known-failing` reads it).

use std::path::Path;

use serde_json::{Map, Value};

const LOCALES: &str = "assets/locales";

pub fn run(root: &Path, fix: bool) -> Result<(), String> {
    let dir = root.join(LOCALES);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let mut stale = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let canonical = canonical(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if canonical == text.replace("\r\n", "\n") {
            continue;
        }
        if fix {
            std::fs::write(&path, &canonical).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("lists: sorted {}", path.display());
        } else {
            stale.push(path.strip_prefix(root).unwrap_or(&path).display().to_string());
        }
    }
    if !stale.is_empty() {
        return Err(format!(
            "not sorted one message per line (or an id appears twice): {}\nrun `cargo xtask lists --fix` and commit the result",
            stale.join(", ")
        ));
    }
    crate::pending::check_locales(root)?;
    crate::pending::check_attribution(root)?;
    crate::known_failing::check_list(root)?;
    println!("lists: message catalogs and the known-failing tests are sorted; pending locale and attribution files fold cleanly");
    Ok(())
}

/// A catalog as it should be written: its messages sorted by id, one per line. A repeated id
/// keeps its last message, the one the app reads.
pub fn canonical(text: &str) -> Result<String, String> {
    let map: Map<String, Value> = match serde_json::from_str(text).map_err(|e| e.to_string())? {
        Value::Object(map) => map,
        _ => return Err("a catalog must be a JSON object".into()),
    };
    if let Some((id, _)) = map.iter().find(|(_, v)| !v.is_string()) {
        return Err(format!("message `{id}` is not a string"));
    }
    Ok(serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())? + "\n")
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn catalogs_are_sorted_one_per_line_and_a_repeated_id_keeps_its_last_message() {
        let text = "{\n  \"b\": \"B\",\n  \"a\": \"A\",\n  \"b\": \"B2\"\n}";
        assert_eq!(canonical(text).unwrap(), "{\n  \"a\": \"A\",\n  \"b\": \"B2\"\n}\n");
        assert!(canonical("[]").is_err());
        assert!(canonical("{\"a\": 1}").is_err());
    }
}
