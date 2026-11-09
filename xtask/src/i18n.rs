//! `cargo xtask i18n`: the message catalogs' coverage report, and a way to repair it.
//!
//! The catalogs under `assets/locales` are plain JSON maps from a stable message id
//! to one translation. The English catalog is also the lookup table for `Locale::label()`, which
//! translates source strings coming from the engine (command labels, toolbar tabs and panels) by
//! matching their English text. A label the English catalog does not carry therefore stays English
//! in *every* locale, silently — nothing fails, the row just looks untranslated.
//!
//! That is the failure this command exists to make visible. `--check` turns the report into a
//! non-zero exit for CI; `--add-missing` writes the uncovered command labels into the English
//! catalog under `cmd.<command id>` so a translator only has to fill in the other catalogs.
//!
//! German and Korean predate the coverage guards and are still partial, so their gaps are
//! reported but do not fail `--check`; the catalogs delivered complete (`COMPLETE`) may not
//! regress.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

/// Where the catalogs live, and the file the English one must lead.
const DIR: &str = "assets/locales";

/// Catalogs delivered complete: a missing message here fails `--check`. English is the source
/// catalog and is checked implicitly; keep this in step with `COMPLETE` in `ui-egui::i18n`.
const COMPLETE: &[&str] = &["ar", "he", "ja", "vi", "zh"];

/// What `run` found.
#[derive(Default)]
struct Report {
    /// Locale id → the English message ids it has not translated.
    missing: BTreeMap<String, Vec<String>>,
    /// Command label → the command ids that use it, for labels English does not carry.
    untranslated: BTreeMap<String, Vec<String>>,
}

impl Report {
    /// Gaps that must be repaired (complete catalogs only; partial catalogs are reported).
    fn problems(&self) -> usize {
        self.missing.iter().filter(|(id, _)| COMPLETE.contains(&id.as_str())).map(|(_, m)| m.len()).sum::<usize>() + self.untranslated.len()
    }

    /// Gaps in catalogs that are still allowed to be partial.
    fn reported_only(&self) -> usize {
        self.missing.iter().filter(|(id, _)| !COMPLETE.contains(&id.as_str())).map(|(_, m)| m.len()).sum()
    }
}

/// The registry flags. CI splits the check so the fast repository job never builds the app:
/// that job passes `--no-registry` (catalog coverage only), and the job that already built
/// `solidcraft-cli` passes `--cli <binary>` (everything, command labels included). With neither
/// the registry is read through `cargo run`, as before. A registry that cannot be read still
/// fails the check; only an explicit `--no-registry` skips it.
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let cli = match args.iter().position(|a| *a == "--cli") {
        Some(i) => Some(args.get(i + 1).ok_or("--cli needs the path of a built solidcraft-cli")?.to_string()),
        None => None,
    };
    if args.contains(&"--no-registry") {
        if cli.is_some() || args.contains(&"--add-missing") {
            return Err("--no-registry cannot be combined with --cli or --add-missing".into());
        }
        println!("i18n: catalog coverage only; command labels are checked where solidcraft-cli is built (--cli)");
        return run_with_registry(root, args, || Ok(Vec::new()));
    }
    run_with_registry(root, args, || registry(cli.as_deref()))
}

/// Resolve the command registry before reporting success or writing any catalog.
fn run_with_registry(root: &Path, args: &[&str], registry: impl FnOnce() -> Result<Vec<Value>, String>) -> Result<(), String> {
    let english = read(root, "en")?;
    let mut catalogs: Vec<(String, Map<String, Value>)> = Vec::new();
    for id in locales(root)? {
        let messages = read(root, &id)?;
        catalogs.push((id, messages));
    }

    let mut report = Report::default();
    for (id, messages) in &catalogs {
        if id == "en" {
            continue;
        }
        let missing: Vec<String> = english.keys().filter(|k| !messages.contains_key(*k)).cloned().collect();
        if !missing.is_empty() {
            report.missing.insert(id.clone(), missing);
        }
    }

    let known: Vec<&str> = english.values().filter_map(Value::as_str).collect();
    for (label, ids) in labels_needing_translation(&known, registry()?) {
        report.untranslated.insert(label, ids);
    }

    if args.contains(&"--add-missing") {
        let added = add_missing(root, &mut report)?;
        println!("i18n: wrote {added} new English message(s) into {DIR}/en.json");
        return Ok(());
    }

    let labels = !args.contains(&"--no-registry");
    print_report(&report, labels);

    if args.contains(&"--check") {
        if report.problems() > 0 {
            return Err(format!(
                "{} message(s) missing from a complete catalog and {} command label(s) with no English entry \
                 (see above); `cargo xtask i18n --add-missing` writes the labels into en.json",
                report.missing.iter().filter(|(id, _)| COMPLETE.contains(&id.as_str())).map(|(_, m)| m.len()).sum::<usize>(),
                report.untranslated.len()
            ));
        }
        if report.reported_only() > 0 {
            println!(
                "i18n: every complete catalog ({}) carries every English message; {}; \
                 {} message(s) are still missing from the partial catalogs (de, ko)",
                COMPLETE.join(", "),
                if labels { "every command label is translatable" } else { "command labels not checked here" },
                report.reported_only()
            );
        } else {
            println!(
                "i18n: every catalog carries every English message; {}",
                if labels { "every command label is translatable" } else { "command labels not checked here" }
            );
        }
    }
    Ok(())
}

/// Locale ids other than English: every catalog file that sits beside `en.json`, so a newly
/// added locale is picked up without editing this command. A regional English (`en-GB.json`)
/// holds only the messages it spells differently, so it has no gaps to report.
fn locales(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join(DIR);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut ids: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .filter(|id| id != "en" && !id.starts_with("en-"))
        .collect();
    ids.sort();
    Ok(ids)
}

fn read(root: &Path, locale: &str) -> Result<Map<String, Value>, String> {
    let path = root.join(DIR).join(format!("{locale}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Messages still in assets/locales/pending/ count: they are folded on main.
    let pending = crate::pending::locale_messages(root, locale)?;
    match serde_json::from_str(&text) {
        Ok(Value::Object(mut map)) => {
            for (id, text) in pending {
                map.entry(id).or_insert(Value::String(text));
            }
            Ok(map)
        }
        Ok(_) => Err(format!("{}: a catalog must be a JSON object", path.display())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Command labels the English catalog cannot translate, mapped to the commands that use them.
///
/// The registry is read through `solidcraft-cli commands` so the report comes from the real
/// command table, never from a copy of it.
fn labels_needing_translation(known: &[&str], commands: Vec<Value>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for command in commands {
        let (Some(id), Some(label)) = (command["id"].as_str(), command["label"].as_str()) else {
            continue;
        };
        if known.contains(&label) {
            continue;
        }
        let ids = out.entry(label.to_string()).or_default();
        if !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    }
    out
}

fn registry(cli: Option<&str>) -> Result<Vec<Value>, String> {
    let mut command = match cli {
        Some(path) => {
            let mut c = std::process::Command::new(path);
            c.arg("commands");
            c
        }
        None => {
            let mut c = crate::cargo();
            c.args(["run", "-q", "--release", "-p", "solidcraft-cli", "--", "commands"]);
            c
        }
    };
    let out = command.output().map_err(|e| format!("solidcraft-cli: {e}"))?;
    if !out.status.success() {
        return Err(format!("solidcraft-cli commands failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("commands JSON: {e}"))
}

/// Write one `cmd.<command id>` entry per still-untranslated label into the English catalog and
/// drop the newly covered labels from the report.
fn add_missing(root: &Path, report: &mut Report) -> Result<usize, String> {
    if report.untranslated.is_empty() {
        return Ok(0);
    }
    let path = root.join(DIR).join("en.json");
    let mut english = read(root, "en")?;
    // One key per label, so two commands sharing a label cannot collide in `label()`'s
    // English-text → id table. The lowest command id wins, keeping the entry stable.
    let mut chosen: BTreeMap<String, String> = BTreeMap::new();
    for (label, ids) in &report.untranslated {
        if let Some(id) = ids.iter().min() {
            chosen.insert(label.clone(), format!("cmd.{id}"));
        }
    }
    for (label, key) in &chosen {
        if english.contains_key(key) {
            continue;
        }
        english.insert(key.clone(), Value::String(label.clone()));
    }
    english.sort_keys();
    std::fs::write(&path, serde_json::to_string_pretty(&Value::Object(english)).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let added = chosen.len();
    report.untranslated.clear();
    Ok(added)
}

fn print_report(report: &Report, labels: bool) {
    for (locale, missing) in &report.missing {
        println!("i18n: {locale} is missing {} English message(s):", missing.len());
        for id in missing {
            println!("      {id}");
        }
    }
    if !report.untranslated.is_empty() {
        println!("i18n: {} command label(s) have no English catalog entry, so they stay English in every locale:", report.untranslated.len());
        for (label, ids) in &report.untranslated {
            println!("      {label:<42} {}", ids.join(", "));
        }
        println!("i18n: run `cargo xtask i18n --add-missing` to add them to en.json, then translate them.");
    }
    if report.problems() == 0 {
        if labels {
            println!("i18n: catalogs complete; every command label is translatable");
        } else {
            println!("i18n: catalog coverage ok (command labels not checked here)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_root() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("solidcraft-i18n-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(root.join(DIR)).unwrap();
        std::fs::write(root.join(DIR).join("en.json"), "{\"cmd.known\":\"Known\"}\n").unwrap();
        root
    }

    #[test]
    fn registry_errors_fail_every_mode_without_writing_catalogs() {
        let root = catalog_root();
        let english = root.join(DIR).join("en.json");
        let before = std::fs::read(&english).unwrap();
        let errors = [
            format!("solidcraft-cli: {}", std::io::Error::from(std::io::ErrorKind::NotFound)),
            "solidcraft-cli commands failed:\ncompiler failed".to_owned(),
            format!("commands JSON: {}", serde_json::from_slice::<Vec<Value>>(b"not JSON").unwrap_err()),
        ];
        for mode in [None, Some("--check"), Some("--add-missing")] {
            let args: Vec<&str> = mode.into_iter().collect();
            for error in &errors {
                let result = run_with_registry(&root, &args, || Err(error.clone()));
                assert_eq!(result, Err(error.clone()), "mode {mode:?} must retain the registry failure");
                assert_eq!(std::fs::read(&english).unwrap(), before, "a registry failure must not rewrite the catalog");
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn known_labels_and_repeated_command_ids_stay_deduplicated() {
        let commands = vec![
            serde_json::json!({"id":"known","label":"Known"}),
            serde_json::json!({"id":"zeta","label":"New"}),
            serde_json::json!({"id":"alpha","label":"New"}),
            serde_json::json!({"id":"zeta","label":"New"}),
        ];
        assert_eq!(labels_needing_translation(&["Known"], commands), BTreeMap::from([("New".into(), vec!["zeta".into(), "alpha".into()])]));
    }

    #[test]
    fn a_valid_registry_still_checks_and_adds_missing_labels() {
        let root = catalog_root();
        let commands = vec![serde_json::json!({"id":"zeta","label":"New"}), serde_json::json!({"id":"alpha","label":"New"})];
        let error = run_with_registry(&root, &["--check"], || Ok(commands.clone())).unwrap_err();
        assert!(error.contains("1 command label(s) with no English entry"), "{error}");
        run_with_registry(&root, &["--add-missing"], || Ok(commands.clone())).unwrap();
        let english: Value = serde_json::from_slice(&std::fs::read(root.join(DIR).join("en.json")).unwrap()).unwrap();
        assert_eq!(english["cmd.known"], "Known");
        assert_eq!(english["cmd.alpha"], "New", "the lowest command id still names a shared label");
        assert!(english.get("cmd.zeta").is_none());
        run_with_registry(&root, &["--check"], || Ok(commands)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn report_counts_both_kinds_of_gap() {
        let report = Report {
            missing: BTreeMap::from([("zh".into(), vec!["a".into(), "b".into()])]),
            untranslated: BTreeMap::from([("Flange".into(), vec!["sheet.flange".into()])]),
        };
        assert_eq!(report.problems(), 3);
    }

    #[test]
    fn an_empty_report_has_no_problems() {
        assert_eq!(Report::default().problems(), 0);
    }

    #[test]
    fn no_registry_skips_label_discovery_and_conflicts_are_refused() {
        let root = catalog_root();
        run(&root, &["--check", "--no-registry"]).unwrap();
        assert!(run(&root, &["--check", "--no-registry", "--cli", "x"]).is_err());
        assert!(run(&root, &["--add-missing", "--no-registry"]).is_err());
    }

    #[test]
    fn an_unreadable_cli_fails_the_check() {
        let root = catalog_root();
        let error = run(&root, &["--check", "--cli", "/nonexistent/solidcraft-cli"]).unwrap_err();
        assert!(error.starts_with("solidcraft-cli:"), "{error}");
    }
}
