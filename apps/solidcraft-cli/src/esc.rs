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

fn write_formatted(path: &Path, formatted: &str) -> Result<(), String> {
    write(path, formatted)?;
    println!("formatted {}", shown(path));
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
                write_formatted(path, &formatted)?;
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
            Ok(formatted) => write_formatted(path, &formatted)?,
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
    for (changed, text) in apply(&mut project, &all, &absolute(path)?, intent)? {
        write(&changed, &text)?;
        println!("wrote {}", shown(&changed));
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
