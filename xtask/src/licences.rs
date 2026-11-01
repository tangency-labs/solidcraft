//! `cargo xtask licences [--check]`: THIRD-PARTY-LICENSES.txt, the licences of every crate built
//! into the shipped programs (solidcraft, solidcraft-cli, solidcraft-web), read from
//! `cargo metadata` and the crates' own licence files. The programs embed it (Help ▸ About ▸
//! Licences, `solidcraft-cli licences`), so a release download needs no licence files beside it.
//! `--check` (part of `cargo xtask ci`) generates it without writing it: it fails when the
//! dependency graph can't be read, and only reports a file that is out of date, since pull
//! requests leave it to the Generated files workflow (`.github/workflows/generated.yml`), which
//! rewrites it on main after each merge.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde_json::Value;

use crate::cargo;

pub const FILE: &str = "THIRD-PARTY-LICENSES.txt";
const ROOTS: &[&str] = &["solidcraft", "solidcraft-cli", "solidcraft-web"];

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let text = generate(root)?;
    let path = root.join(FILE);
    if check {
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        if old.replace("\r\n", "\n") != text {
            // Not an error: pull requests don't edit the file (two that did would always conflict);
            // the Generated files workflow rewrites it on main after the merge.
            println!("licences: {FILE} will change when this lands (the Generated files workflow rewrites it on main)");
            return Ok(());
        }
        println!("licences: {FILE} is up to date");
        return Ok(());
    }
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("licences: wrote {FILE} ({} KB)", text.len() / 1024);
    Ok(())
}

struct Crate {
    name: String,
    version: String,
    license: String,
    repository: String,
    /// (file name, text) of the licence files shipped in the crate.
    files: Vec<(String, String)>,
}

fn generate(root: &Path) -> Result<String, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--locked"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata JSON: {e}"))?;
    let packages: BTreeMap<&str, &Value> = meta["packages"].as_array().into_iter().flatten().filter_map(|p| Some((p["id"].as_str()?, p))).collect();
    let nodes: BTreeMap<&str, &Value> =
        meta["resolve"]["nodes"].as_array().into_iter().flatten().filter_map(|n| Some((n["id"].as_str()?, n))).collect();
    let members: BTreeSet<&str> = meta["workspace_members"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();

    // Everything reachable from the shipped programs through normal (not dev) dependencies, on any
    // platform.
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<&str> = packages
        .iter()
        .filter(|(id, p)| members.contains(*id) && ROOTS.contains(&p["name"].as_str().unwrap_or_default()))
        .map(|(id, _)| *id)
        .collect();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        for d in nodes.get(id).and_then(|n| n["deps"].as_array()).into_iter().flatten() {
            let normal = d["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
            if normal && let Some(pkg) = d["pkg"].as_str() {
                queue.push_back(pkg);
            }
        }
    }

    let mut crates = Vec::new();
    for id in seen {
        let Some(p) = packages.get(id) else { continue };
        if members.contains(id) {
            continue;
        }
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let dir = Path::new(p["manifest_path"].as_str().unwrap_or_default()).parent().map(Path::to_path_buf).unwrap_or_default();
        crates.push(Crate {
            name: s("name"),
            version: s("version"),
            license: s("license"),
            repository: s("repository"),
            files: licence_files(&dir, p),
        });
    }
    crates.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    Ok(render(root, &crates))
}

/// The licence and notice files at the top of a crate's package (and its `license-file`).
fn licence_files(dir: &Path, p: &Value) -> Vec<(String, String)> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let u = n.to_ascii_uppercase();
            if e.path().is_file() && ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"].iter().any(|k| u.starts_with(k)) {
                names.insert(n);
            }
        }
    }
    if let Some(f) = p["license_file"].as_str() {
        names.insert(f.to_string());
    }
    names
        .into_iter()
        .filter_map(|n| {
            let text = std::fs::read_to_string(dir.join(&n)).ok()?;
            let text = text.replace("\r\n", "\n").trim().to_string();
            (!text.is_empty()).then(|| (Path::new(&n).file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or(n), text))
        })
        .collect()
}

fn render(root: &Path, crates: &[Crate]) -> String {
    let rule = "=".repeat(80);
    let mut out = format!(
        "Third-party software in SolidCraft\n\nSolidCraft (the desktop app, solidcraft-cli and the web app) is built from the {} crates below.\n\
         Generated by `cargo xtask licences` from Cargo.lock and the crates' own licence files.\n\
         SolidCraft's own licence is in LICENSE-MIT and LICENSE-APACHE, and NOTICE.\n\n",
        crates.len()
    );
    for c in crates {
        out += &format!("{} {}  ({})\n", c.name, c.version, if c.license.is_empty() { "see its licence file" } else { &c.license });
    }
    // One copy of each distinct licence text, with the crates it covers.
    let mut by_text: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut without: Vec<&Crate> = Vec::new();
    for c in crates {
        if c.files.is_empty() {
            without.push(c);
        }
        for (_, text) in &c.files {
            by_text.entry(text.as_str()).or_default().push(format!("{} {}", c.name, c.version));
        }
    }
    let mut sections: Vec<(Vec<String>, &str)> = by_text.into_iter().map(|(t, v)| (v, t)).collect();
    sections.sort();
    for (names, text) in sections {
        out += &format!("\n{rule}\n{}\n{rule}\n\n{text}\n", wrap(&names.join(", ")));
    }
    if !without.is_empty() {
        out += &format!(
            "\n{rule}\nCrates whose package carries no licence file (their licence is named in Cargo.toml; the MIT and Apache-2.0\ntexts are as in SolidCraft's own LICENSE-MIT and LICENSE-APACHE)\n{rule}\n\n"
        );
        for c in without {
            out += &format!(
                "{} {}: {}{}\n",
                c.name,
                c.version,
                c.license,
                if c.repository.is_empty() { String::new() } else { format!(" ({})", c.repository) }
            );
        }
    }

    // Bundled font licences (SIL Open Font License, Version 1.1)
    let font_rule = "=".repeat(80);
    out += &format!(
        "\n{font_rule}\nBundled fonts (SIL Open Font License, Version 1.1)\n{font_rule}\n\n\
         - LiberationSans-Regular.ttf (assets/fonts/liberation-sans/LiberationSans-Regular.ttf)\n  \
           Copyright 2012 Red Hat, Inc. (Liberation Fonts 2.x; digitized data Google)\n  \
           Source: https://github.com/liberationfonts\n\n\
         - BIZUDPGothic-Regular.ttf (assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf)\n  \
           Copyright 2022 The BIZ UDGothic Project Authors (https://github.com/googlefonts/morisawa-biz-ud-mincho)\n  \
           Source: https://github.com/googlefonts/morisawa-biz-ud-gothic\n\n\
         - CraftUI-SC.otf (assets/fonts/craft-ui-sc/CraftUI-SC.otf)\n  \
           Copyright 2014-2021 Adobe / Google Inc. (Noto Sans SC subset for SolidCraft web zh)\n  \
           Source: https://github.com/notofonts/noto-cjk (Sans SubsetOTF/SC)\n\n\
         - CraftUI-TC.otf (assets/fonts/craft-ui-tc/CraftUI-TC.otf)\n  \
           Copyright 2014-2021 Adobe / Google Inc. (Noto Sans TC subset for SolidCraft web zh-TW)\n  \
           Source: https://github.com/notofonts/noto-cjk (Sans SubsetOTF/TC)\n\n\
         - IBMPlexSans-Variable.ttf (assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf)\n  \
           Copyright 2017 IBM Corp. with Reserved Font Name \"Plex\"\n  \
           Source: https://github.com/IBM/plex\n\n\
         - IBMPlexMono-Regular.ttf (assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf)\n  \
           Copyright 2017 IBM Corp. with Reserved Font Name \"Plex\"\n  \
           Source: https://github.com/IBM/plex\n\n"
    );
    let ofl_path = root.join("assets/fonts/biz-udp-gothic/OFL.txt");
    let fallback_path = root.join("assets/fonts/liberation-sans/OFL.txt");
    let ofl_text = std::fs::read_to_string(&ofl_path).or_else(|_| std::fs::read_to_string(&fallback_path)).unwrap_or_default();
    let ofl_clean = ofl_text.replace("\r\n", "\n").trim().to_string();
    if !ofl_clean.is_empty() {
        out += &format!("{font_rule}\nSIL OPEN FONT LICENSE Version 1.1\n{font_rule}\n\n{ofl_clean}\n");
    }

    out
}

/// Wrap a long list of crate names at 100 columns.
fn wrap(s: &str) -> String {
    let mut out = String::new();
    let mut col = 0;
    for w in s.split(' ') {
        if col > 0 && col + w.len() + 1 > 100 {
            out.push('\n');
            col = 0;
        } else if col > 0 {
            out.push(' ');
            col += 1;
        }
        out += w;
        col += w.len();
    }
    out
}
