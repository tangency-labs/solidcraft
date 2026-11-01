//! Lists the format changes of each family of files versioned by `src/format.rs`: designs in
//! `src/format/`, drawings in `src/drawing/format/` (#3890). One file per change, in `v<N>/` once a
//! release fixed its number, else in `pending/` (see `src/format/pending/README.md`). The lists are
//! generated so that a change adds a file and never edits a line another change also edits.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// A family's change files: their folder, the last format before them, and the prefix of the
/// family's lists in `format.rs`.
struct Family {
    folder: &'static str,
    base: u32,
    list_prefix: &'static str,
}

/// Every family, each read by `scripts/fold-format-pending.sh` too. A design's upgrade steps are
/// written out in `format.rs` up to 22 (`JSON_STEPS`); a drawing's first format is 1.
const FAMILIES: [Family; 2] =
    [Family { folder: "src/format", base: 22, list_prefix: "" }, Family { folder: "src/drawing/format", base: 1, list_prefix: "DRAWING_" }];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut out = String::new();
    let mut mods = String::new();
    for family in &FAMILIES {
        lists(family, &manifest, &mut out, &mut mods);
    }
    out.push_str(&mods);
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("format_parts.rs");
    std::fs::write(dest, out).unwrap();
}

/// A family's `FOLDED` and `PENDING` lists; its folded formats follow on from its base.
fn lists(family: &Family, manifest: &Path, out: &mut String, mods: &mut String) {
    let Family { folder, base, list_prefix } = family;
    let root = manifest.join(folder);
    println!("cargo:rerun-if-changed={}", root.display());
    let mut folded: Vec<(u32, PathBuf)> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let version = path.file_name()?.to_str()?.strip_prefix('v')?.parse().ok()?;
            path.is_dir().then_some((version, path))
        })
        .collect();
    folded.sort();
    for (i, (version, _)) in folded.iter().enumerate() {
        let expected = base + 1 + i as u32;
        assert_eq!(*version, expected, "{folder}/: expected v{expected}/ next, found v{version}/ (versions are contiguous)");
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
        parts(dir, &format!("{module}v{version}"), out, mods);
        out.push_str("    ]),\n");
    }
    let _ = writeln!(out, "];\n\n/// The changes no release has a number for yet (`{folder}/pending/`).\nconst {list_prefix}PENDING: &[Part] = &[");
    parts(&root.join("pending"), &format!("{module}pending"), out, mods);
    out.push_str("];\n\n");
}

/// The `Part`s of the change files in `dir`, in file-name order, and a module for each.
fn parts(dir: &Path, prefix: &str, out: &mut String, mods: &mut String) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    files.sort();
    for path in files {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        assert!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{}: name a format change file with lowercase letters, digits and dashes",
            path.display()
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.trim_start().starts_with("//!"), "{}: start a format change with a `//!` note", path.display());
        let module = format!("{prefix}_{}", name.replace('-', "_"));
        let step = if text.contains("fn step(") { format!("Some({module}::step)") } else { "None".into() };
        let _ = writeln!(out, "        Part {{ name: {name:?}, step: {step} }},");
        let _ = writeln!(mods, "#[path = {:?}]\nmod {module};", path.display().to_string());
    }
}
