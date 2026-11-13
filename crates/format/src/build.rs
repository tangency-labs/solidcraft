//! For a build script: the generated lists of a family's changes. Each change is one `.rs` file in
//! the family's folder, in `v<N>/` once a release fixed its number, else in `pending/`; the lists
//! are generated so that a change adds a file and never edits a line another change also edits.
//!
//! The generated code, for the app to `include!`, is per family a `FOLDED` list (each folded
//! format and its parts) and a `PENDING` list, both prefixed by the family's `list_prefix`, then a
//! module per change file. It names the app's `Part` type, which the including module has in scope
//! (`type Part = solidcraft_format::Part<AppError>;`), and a change file's step as `step` (a
//! `pub fn step(v: &mut Value) -> Result<(), AppError>`, when the file has one).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// A family's change files: their folder (relative to the crate's manifest), the last format
/// before them, and the prefix of the family's lists.
pub struct Family {
    pub folder: &'static str,
    pub base: u32,
    pub list_prefix: &'static str,
}

/// What [`lists`] made: the code to write, and the folders the build script reruns on.
pub struct Lists {
    pub code: String,
    pub watch: Vec<PathBuf>,
}

/// The `FOLDED` and `PENDING` lists of every family, and a module for each change file. Refuses
/// versions that are not contiguous from a family's base, a change file not named with lowercase
/// letters, digits and dashes, and one that doesn't start with a `//!` note.
pub fn lists(manifest: &Path, families: &[Family]) -> Result<Lists, String> {
    let mut code = String::new();
    let mut mods = String::new();
    let mut watch = Vec::new();
    for family in families {
        let root = manifest.join(family.folder);
        family_lists(family, &root, &mut code, &mut mods)?;
        watch.push(root);
    }
    code.push_str(&mods);
    Ok(Lists { code, watch })
}

/// A family's `FOLDED` and `PENDING` lists; its folded formats follow on from its base.
fn family_lists(family: &Family, root: &Path, out: &mut String, mods: &mut String) -> Result<(), String> {
    let Family { folder, base, list_prefix } = family;
    let mut folded: Vec<(u32, PathBuf)> = std::fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let version = path.file_name()?.to_str()?.strip_prefix('v')?.parse().ok()?;
            path.is_dir().then_some((version, path))
        })
        .collect();
    folded.sort();
    for (i, (version, _)) in folded.iter().enumerate() {
        let expected = base + 1 + i as u32;
        if *version != expected {
            return Err(format!("{folder}/: expected v{expected}/ next, found v{version}/ (versions are contiguous)"));
        }
    }
    // Module names start with the family's (`drawing_pending_…`), so two families' changes of one
    // name never clash.
    let module = list_prefix.to_ascii_lowercase();
    let _ = writeln!(
        out,
        "/// The changes of each format after the base, oldest first (`{folder}/v<N>/`).\nconst {list_prefix}FOLDED: &[(u32, &[Part])] = &["
    );
    for (version, dir) in &folded {
        let _ = writeln!(out, "    ({version}, &[");
        parts(dir, &format!("{module}v{version}"), out, mods)?;
        out.push_str("    ]),\n");
    }
    let _ = writeln!(out, "];\n\n/// The changes no release has a number for yet (`{folder}/pending/`).\nconst {list_prefix}PENDING: &[Part] = &[");
    parts(&root.join("pending"), &format!("{module}pending"), out, mods)?;
    out.push_str("];\n\n");
    Ok(())
}

/// The `Part`s of the change files in `dir`, in file-name order, and a module for each.
fn parts(dir: &Path, prefix: &str, out: &mut String, mods: &mut String) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    files.sort();
    for path in files {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(format!("{}: name a format change file with lowercase letters, digits and dashes", path.display()));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !text.trim_start().starts_with("//!") {
            return Err(format!("{}: start a format change with a `//!` note", path.display()));
        }
        let module = format!("{prefix}_{}", name.replace('-', "_"));
        let step = if text.contains("fn step(") { format!("Some({module}::step)") } else { "None".into() };
        let _ = writeln!(out, "        Part {{ name: {name:?}, step: {step} }},");
        let _ = writeln!(mods, "#[path = {:?}]\nmod {module};", path.display().to_string());
    }
    Ok(())
}
