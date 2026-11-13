//! Pending files: one file per change for what many pull requests add to, folded on `main`.
//!
//! - `assets/locales/pending/<slug>.json`: `{"<locale>": {"<id>": "<text>"}}`, folded into the
//!   catalogs (`assets/locales/<locale>.json`, sorted as `cargo xtask lists` expects).
//! - `attribution/pending/<slug>.md`: rows of the ATTRIBUTION.md files table, appended to it.
//!
//! `cargo xtask fold-pending` folds and deletes them (the Generated files workflow runs it on
//! `main`); `check_*` is what `cargo xtask lists`, `i18n` and `assets` run on every pull request.
//! Both read `README.md` in each folder as documentation, never as a pending file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const LOCALES: &str = "assets/locales";
const LOCALES_PENDING: &str = "assets/locales/pending";
const ATTRIBUTION_PENDING: &str = "attribution/pending";

/// One pending locale file: its path and, per locale, the messages it adds.
type LocalePending = (PathBuf, BTreeMap<String, BTreeMap<String, String>>);

fn files_with(dir: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(Vec::new()) };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext) && p.file_name().is_some_and(|n| n != "README.md"))
        .collect();
    files.sort();
    Ok(files)
}

fn show(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn read_locale_pending(root: &Path) -> Result<Vec<LocalePending>, String> {
    let mut out = Vec::new();
    for path in files_with(&root.join(LOCALES_PENDING), "json")? {
        let name = show(root, &path);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
        let map: BTreeMap<String, BTreeMap<String, String>> =
            serde_json::from_str(&text).map_err(|e| format!("{name}: expected {{\"<locale>\": {{\"<id>\": \"<text>\"}}}}: {e}"))?;
        out.push((path, map));
    }
    Ok(out)
}

fn read_catalog(root: &Path, locale: &str) -> Result<Map<String, Value>, String> {
    let path = root.join(LOCALES).join(format!("{locale}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", show(root, &path)))?;
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("{}: a catalog must be a JSON object", show(root, &path))),
        Err(e) => Err(format!("{}: {e}", show(root, &path))),
    }
}

/// The messages the pending files add to one locale, for the tools that read the catalogs.
pub fn locale_messages(root: &Path, locale: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (_, map) in read_locale_pending(root)? {
        if let Some(messages) = map.get(locale) {
            out.extend(messages.clone());
        }
    }
    Ok(out)
}

/// Fails on a pending file the fold could not apply: a locale without a catalog, an id the catalog
/// already has, or an id two pending files both add. Returns nothing else.
pub fn check_locales(root: &Path) -> Result<(), String> {
    plan_locales(root).map(|_| ())
}

type Plan = (BTreeMap<String, Map<String, Value>>, Vec<PathBuf>);

/// Locale → catalog with the pending messages folded in, plus the pending files that fed them.
fn plan_locales(root: &Path) -> Result<Plan, String> {
    let pending = read_locale_pending(root)?;
    let mut catalogs: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut added_by: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut problems = Vec::new();
    for (path, map) in &pending {
        let name = show(root, path);
        for (locale, messages) in map {
            if !root.join(LOCALES).join(format!("{locale}.json")).is_file() {
                problems.push(format!("{name}: `{locale}` has no catalog assets/locales/{locale}.json"));
                continue;
            }
            if !catalogs.contains_key(locale) {
                catalogs.insert(locale.clone(), read_catalog(root, locale)?);
            }
            for (id, text) in messages {
                if let Some(other) = added_by.get(&(locale.clone(), id.clone())) {
                    problems.push(format!("{name}: `{id}` ({locale}) is also added by {other}"));
                } else if catalogs[locale].contains_key(id) {
                    problems.push(format!("{name}: `{id}` is already in {locale}.json (rename the new id, or edit the catalog)"));
                } else {
                    added_by.insert((locale.clone(), id.clone()), name.clone());
                    if let Some(catalog) = catalogs.get_mut(locale) {
                        catalog.insert(id.clone(), Value::String(text.clone()));
                    }
                }
            }
        }
    }
    if problems.is_empty() { Ok((catalogs, pending.into_iter().map(|(p, _)| p).collect())) } else { Err(problems.join("\n")) }
}

/// Fold every pending locale file into the catalogs and delete it. Returns how many messages.
pub fn fold_locales(root: &Path) -> Result<usize, String> {
    let (catalogs, files) = plan_locales(root)?;
    if files.is_empty() {
        return Ok(0);
    }
    let mut count = 0;
    for (locale, catalog) in &catalogs {
        let before = read_catalog(root, locale)?.len();
        count += catalog.len() - before;
        let text = crate::lists::canonical(&serde_json::to_string(catalog).map_err(|e| e.to_string())?)?;
        let path = root.join(LOCALES).join(format!("{locale}.json"));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", show(root, &path)))?;
    }
    for file in files {
        std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", show(root, &file)))?;
    }
    Ok(count)
}

/// The first cell of a table row (the file path), when the line is a data row of the files table.
fn row_file(line: &str) -> Option<&str> {
    let line = line.trim();
    if !line.starts_with('|') {
        return None;
    }
    let cell = line.split('|').nth(1)?.trim();
    (!cell.is_empty() && cell != "File" && !cell.starts_with("---") && !cell.starts_with(":-")).then_some(cell)
}

/// The rows of every pending attribution file: (file name, row line). Anything else in a pending
/// file besides the table header and blank lines is an error, so a stray note is not lost.
pub fn attribution_rows(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut rows = Vec::new();
    for path in files_with(&root.join(ATTRIBUTION_PENDING), "md")? {
        let name = show(root, &path);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
        for line in text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
            if row_file(line).is_some() {
                rows.push((name.clone(), line.to_string()));
            } else if !line.trim_start().starts_with('|') {
                return Err(format!("{name}: only ATTRIBUTION.md table rows (`| File | Author | Source | Licence |`) belong here, not `{line}`"));
            }
        }
    }
    Ok(rows)
}

/// ATTRIBUTION.md with the pending rows appended to its files table; an error when a row's file
/// already has one (in the table or in another pending file).
fn merged_attribution(root: &Path) -> Result<Option<String>, String> {
    let rows = attribution_rows(root)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let path = root.join("ATTRIBUTION.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    let mut lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.trim() == "## Files").ok_or("ATTRIBUTION.md has no `## Files` section")?;
    let end = lines[start..].iter().position(|l| l.starts_with("## ") && l.trim() != "## Files").map_or(lines.len(), |i| start + i);
    let last_row = (start..end).rev().find(|&i| lines[i].trim_start().starts_with('|')).ok_or("ATTRIBUTION.md: no files table under `## Files`")?;
    let mut seen: BTreeMap<String, String> =
        lines[start..end].iter().filter_map(|l| row_file(l)).map(|f| (f.to_string(), "ATTRIBUTION.md".into())).collect();
    let mut problems = Vec::new();
    for (name, row) in &rows {
        let Some(file) = row_file(row) else { continue };
        if let Some(other) = seen.get(file) {
            problems.push(format!("{name}: `{file}` already has a row in {other}"));
        } else {
            seen.insert(file.to_string(), name.clone());
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    let new: Vec<&str> = rows.iter().map(|(_, r)| r.as_str()).collect();
    lines.splice(last_row + 1..last_row + 1, new);
    Ok(Some(lines.join("\n") + "\n"))
}

pub fn check_attribution(root: &Path) -> Result<(), String> {
    merged_attribution(root).map(|_| ())
}

/// Append the pending rows to ATTRIBUTION.md and delete the pending files. Returns the row count.
pub fn fold_attribution(root: &Path) -> Result<usize, String> {
    let Some(text) = merged_attribution(root)? else { return Ok(0) };
    let count = attribution_rows(root)?.len();
    std::fs::write(root.join("ATTRIBUTION.md"), text).map_err(|e| format!("ATTRIBUTION.md: {e}"))?;
    for file in files_with(&root.join(ATTRIBUTION_PENDING), "md")? {
        std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", show(root, &file)))?;
    }
    Ok(count)
}

/// `cargo xtask fold-pending`.
pub fn run(root: &Path) -> Result<(), String> {
    let messages = fold_locales(root)?;
    let rows = fold_attribution(root)?;
    println!("fold-pending: {messages} message(s) into assets/locales, {rows} row(s) into ATTRIBUTION.md");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(catalogs: &[(&str, &str)]) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("xtask-pending-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(LOCALES_PENDING)).unwrap();
        std::fs::create_dir_all(root.join(ATTRIBUTION_PENDING)).unwrap();
        for (locale, text) in catalogs {
            std::fs::write(root.join(LOCALES).join(format!("{locale}.json")), text).unwrap();
        }
        std::fs::write(
            root.join("ATTRIBUTION.md"),
            "# Attribution\n\n## Files\n\n| File | Author | Source | Licence |\n|---|---|---|---|\n| a.png | me | own | MIT |\n\n## Other\n\ntext\n",
        )
        .unwrap();
        root
    }

    fn put(root: &Path, rel: &str, text: &str) {
        std::fs::write(root.join(rel), text).unwrap();
    }

    #[test]
    fn locale_files_fold_sorted_and_are_removed() {
        let root = tree(&[("en", "{\n  \"b\": \"B\"\n}\n"), ("ja", "{\n  \"b\": \"ビ\"\n}\n")]);
        put(&root, "assets/locales/pending/one.json", r#"{"en": {"z": "Z", "a": "A"}, "ja": {"a": "ア"}}"#);
        put(&root, "assets/locales/pending/two.json", r#"{"en": {"m": "M"}}"#);
        put(&root, "assets/locales/pending/README.md", "docs, never folded");
        assert_eq!(locale_messages(&root, "en").unwrap().len(), 3);
        assert_eq!(fold_locales(&root).unwrap(), 4);
        let en = std::fs::read_to_string(root.join("assets/locales/en.json")).unwrap();
        assert_eq!(en, "{\n  \"a\": \"A\",\n  \"b\": \"B\",\n  \"m\": \"M\",\n  \"z\": \"Z\"\n}\n");
        assert_eq!(en, crate::lists::canonical(&en).unwrap());
        assert!(std::fs::read_to_string(root.join("assets/locales/ja.json")).unwrap().contains("\"a\": \"ア\""));
        assert!(!root.join("assets/locales/pending/one.json").exists() && !root.join("assets/locales/pending/two.json").exists());
        assert!(root.join("assets/locales/pending/README.md").exists());
        assert!(locale_messages(&root, "en").unwrap().is_empty());
    }

    #[test]
    fn a_key_the_catalog_has_or_two_files_add_fails_and_nothing_is_changed() {
        let root = tree(&[("en", "{\n  \"b\": \"B\"\n}\n")]);
        put(&root, "assets/locales/pending/one.json", r#"{"en": {"b": "again"}}"#);
        let err = fold_locales(&root).unwrap_err();
        assert!(err.contains("`b` is already in en.json"), "{err}");
        assert!(root.join("assets/locales/pending/one.json").exists());
        assert_eq!(std::fs::read_to_string(root.join("assets/locales/en.json")).unwrap(), "{\n  \"b\": \"B\"\n}\n");

        put(&root, "assets/locales/pending/one.json", r#"{"en": {"c": "C"}}"#);
        put(&root, "assets/locales/pending/two.json", r#"{"en": {"c": "C2"}}"#);
        assert!(check_locales(&root).unwrap_err().contains("is also added by"));
        put(&root, "assets/locales/pending/two.json", r#"{"xx": {"d": "D"}}"#);
        assert!(check_locales(&root).unwrap_err().contains("no catalog"));
        put(&root, "assets/locales/pending/two.json", "[]");
        assert!(check_locales(&root).is_err());
    }

    #[test]
    fn attribution_rows_fold_into_the_files_table_and_a_repeat_fails() {
        let root = tree(&[]);
        put(&root, "attribution/pending/x.md", "| File | Author | Source | Licence |\n|---|---|---|---|\n| b.png | me | own | MIT |\n");
        put(&root, "attribution/pending/y.md", "| c.png | me | own | MIT |\n");
        assert_eq!(attribution_rows(&root).unwrap().len(), 2);
        assert_eq!(fold_attribution(&root).unwrap(), 2);
        let text = std::fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap();
        assert!(text.contains("| a.png | me | own | MIT |\n| b.png | me | own | MIT |\n| c.png | me | own | MIT |\n\n## Other"), "{text}");
        assert!(!root.join("attribution/pending/x.md").exists());

        put(&root, "attribution/pending/z.md", "| a.png | you | own | MIT |\n");
        assert!(check_attribution(&root).unwrap_err().contains("already has a row"));
        put(&root, "attribution/pending/z.md", "a stray note\n");
        assert!(check_attribution(&root).is_err());
    }
}
