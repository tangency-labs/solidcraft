//! `solidcraft-cli esc`: the .esc text design format's `check`, `fmt`, `edit` and `rename`
//! (crates/esc).

use std::path::{Path, PathBuf};

use serde_json::json;
use solidcraft_esc::check::check as check_file;
use solidcraft_esc::edit::{Intent, apply};
use solidcraft_esc::fmt::format;
use solidcraft_esc::project::{Project, list};
use solidcraft_esc::{Diagnostic, Located};

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        Some("fmt") => fmt(&args[1..]),
        Some("edit") => edit(&args[1..]),
        Some("rename") => rename(&args[1..]),
        Some(other) => Err(format!("esc: unknown action `{other}`\n{}", crate::command_usage("esc", &[]))),
        None => Err(crate::command_usage("esc", &[])),
    }
}

/// The words that are not options or an option's value.
fn words<'a>(args: &'a [String], valued: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if valued.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a.as_str());
        }
        i += 1;
    }
    out
}

fn absolute(path: &str) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|e| format!("{path}: {e}"))
}

/// `path` as the user would type it from here.
fn shown(path: &Path) -> String {
    let here = std::env::current_dir().ok();
    here.and_then(|h| path.strip_prefix(h).ok().map(|p| p.display().to_string())).unwrap_or_else(|| path.display().to_string())
}

/// The files the words name, a folder standing for every .esc file under it.
fn files(words: &[&str]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for w in words {
        let path = absolute(w)?;
        if path.is_dir() {
            out.extend(list(&path).map_err(|e| format!("{w}: {e}"))?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    solidcraft_engine::io::write_atomic(path, text.as_bytes(), false).map_err(|e| format!("{}: {e}", shown(path)))
}

fn located(path: &Path, text: &str, diagnostics: &[Diagnostic]) -> Vec<Located> {
    let file = shown(path);
    diagnostics.iter().map(|d| Located::new(&file, text, d)).collect()
}

fn write_formatted(path: &Path, formatted: &str, json: bool) -> Result<(), String> {
    write(path, formatted)?;
    if json {
        solidcraft_engine::note!("formatted {}", shown(path));
    } else {
        println!("formatted {}", shown(path));
    }
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    crate::known_args("esc check", args, &[], &["--json", "--fix"], true)?;
    let targets = files(&words(args, &[]))?;
    if targets.is_empty() {
        return Err(crate::command_usage("esc", &[]));
    }
    let json = args.iter().any(|a| a == "--json");
    if args.iter().any(|a| a == "--fix") {
        let mut project = Project::default();
        for path in &targets {
            let file = project.load(path)?;
            if let Ok(formatted) = format(&file.text)
                && formatted != file.text
            {
                write_formatted(path, &formatted, json)?;
            }
        }
    }
    let mut project = Project::default();
    let mut found = Vec::new();
    for path in &targets {
        let checked = check_file(&mut project, path)?;
        let file = project.load(path)?;
        let here = located(path, &file.text, &checked.diagnostics);
        if !json {
            here.iter().for_each(|l| print!("{}", l.render(&file.text)));
        }
        found.extend(here);
    }
    if json {
        crate::print_json(&json!({"files": targets.len(), "diagnostics": found}), None)?;
    }
    match found.len() {
        0 => Ok(()),
        n => Err(format!("esc check: {n} problem(s) in {} file(s)", targets.len())),
    }
}

fn fmt(args: &[String]) -> Result<(), String> {
    crate::known_args("esc fmt", args, &[], &["--check"], true)?;
    let only_check = args.iter().any(|a| a == "--check");
    let targets = files(&words(args, &[]))?;
    if targets.is_empty() {
        return Err(crate::command_usage("esc", &[]));
    }
    let mut project = Project::default();
    let mut problems = 0;
    for path in &targets {
        let file = project.load(path)?;
        match format(&file.text) {
            Err(errors) => {
                located(path, &file.text, &errors).iter().for_each(|l| print!("{}", l.render(&file.text)));
                problems += 1;
            }
            Ok(formatted) if formatted == file.text => {}
            Ok(_) if only_check => {
                println!("{} is not in canonical layout", shown(path));
                problems += 1;
            }
            Ok(formatted) => write_formatted(path, &formatted, false)?,
        }
    }
    match problems {
        0 => Ok(()),
        n => Err(format!("esc fmt: {n} file(s) need attention")),
    }
}

/// Applies the intent and writes each file it changes. References are searched for in every
/// .esc file under `--root`, by default the current folder.
fn apply_and_write(args: &[String], path: &str, intent: &Intent) -> Result<(), String> {
    let root = absolute(crate::flag(args, "--root").unwrap_or("."))?;
    let all = list(&root).map_err(|e| format!("{}: {e}", shown(&root)))?;
    let mut project = Project::default();
    let changes = apply(&mut project, &all, &absolute(path)?, intent)?;
    write_changes(&mut project, &changes, write)?;
    for (changed, _) in &changes {
        println!("wrote {}", shown(changed));
    }
    Ok(())
}

// Rollback covers write errors, not process crashes.
fn write_changes(
    project: &mut Project,
    changes: &[(PathBuf, String)],
    mut write_file: impl FnMut(&Path, &str) -> Result<(), String>,
) -> Result<(), String> {
    let originals = changes.iter().map(|(path, _)| project.load(path)).collect::<Result<Vec<_>, _>>()?;
    for (i, (path, text)) in changes.iter().enumerate() {
        if let Err(mut error) = write_file(path, text) {
            for original in originals.iter().take(i).rev() {
                if let Err(restore_error) = write_file(&original.path, &original.text) {
                    error.push_str(&format!("\nrollback failed: {restore_error}"));
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

fn edit(args: &[String]) -> Result<(), String> {
    crate::known_args("esc edit", args, &["--root"], &[], true)?;
    let [path, intent] = words(args, &["--root"])[..] else { return Err(crate::command_usage("esc", &[])) };
    let intent: Intent = serde_json::from_str(intent).map_err(|e| format!("esc edit: the intent: {e}"))?;
    apply_and_write(args, path, &intent)
}

fn rename(args: &[String]) -> Result<(), String> {
    crate::known_args("esc rename", args, &["--root"], &[], true)?;
    let [path, from, to] = words(args, &["--root"])[..] else { return Err(crate::command_usage("esc", &[])) };
    apply_and_write(args, path, &Intent::Rename { from: from.to_owned(), to: to.to_owned() })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!("solidcraft-esc-cli-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("a.esc"), "esc 1\npart\n\nwidth = param(2 mm)\n").unwrap();
            for name in ["b.esc", "c.esc"] {
                std::fs::write(root.join(name), "esc 1\nassembly\n\npart = import(\"a\", width: 3 mm)\n").unwrap();
            }
            Self(root)
        }

        fn changes(&self, project: &mut Project) -> Vec<(PathBuf, String)> {
            apply(project, &list(&self.0).unwrap(), &self.0.join("a.esc"), &Intent::Rename { from: "width".into(), to: "plate_width".into() })
                .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn edit_and_rename_write_all_changed_files() {
        for action in ["edit", "rename"] {
            let dir = Scratch::new();
            let mut args = vec![action.to_owned(), dir.0.join("a.esc").to_string_lossy().into_owned()];
            if action == "edit" {
                args.push(r#"{"intent":"rename","from":"width","to":"plate_width"}"#.into());
            } else {
                args.extend(["width".into(), "plate_width".into()]);
            }
            args.extend(["--root".into(), dir.0.to_string_lossy().into_owned()]);
            run(&args).unwrap();
            for path in list(&dir.0).unwrap() {
                let text = std::fs::read_to_string(path).unwrap();
                assert!(text.contains("plate_width"), "{text}");
            }
        }
    }

    #[test]
    fn failed_write_restores_completed_files() {
        for fail_at in 0..3 {
            let dir = Scratch::new();
            let mut project = Project::default();
            let changes = dir.changes(&mut project);
            assert_eq!(changes.len(), 3);
            let originals: Vec<_> = changes.iter().map(|(path, _)| std::fs::read(path).unwrap()).collect();
            let mut calls = 0;
            let error = write_changes(&mut project, &changes, |path, text| {
                let call = calls;
                calls += 1;
                if call == fail_at { Err(format!("{}: injected write failure", shown(path))) } else { write(path, text) }
            })
            .unwrap_err();
            assert_eq!(error, format!("{}: injected write failure", shown(&changes[fail_at].0)));
            assert_eq!(calls, fail_at * 2 + 1);
            for ((path, _), original) in changes.iter().zip(originals) {
                assert_eq!(std::fs::read(path).unwrap(), original);
            }
        }
    }

    #[test]
    fn rollback_reports_failures_and_keeps_restoring_other_files() {
        let dir = Scratch::new();
        let mut project = Project::default();
        let changes = dir.changes(&mut project);
        let original = std::fs::read(&changes[0].0).unwrap();
        let mut calls = 0;
        let error = write_changes(&mut project, &changes, |path, text| {
            calls += 1;
            match calls {
                3 => Err(format!("{}: injected write failure", shown(path))),
                4 => Err(format!("{}: injected restore failure", shown(path))),
                _ => write(path, text),
            }
        })
        .unwrap_err();
        assert_eq!(
            error,
            format!("{}: injected write failure\nrollback failed: {}: injected restore failure", shown(&changes[2].0), shown(&changes[1].0))
        );
        assert_eq!(calls, 5);
        assert_eq!(std::fs::read(&changes[0].0).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&changes[1].0).unwrap(), changes[1].1);
    }
}
