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
    let (text, crates) = generate(root)?;
    let copyleft: Vec<&Crate> = crates.iter().filter(|c| !allows_permissive_use(&c.license)).collect();
    if !copyleft.is_empty() {
        let mut report = String::new();
        report
            .push_str("licences: a dependency with a copyleft licence is in the tree (AGENTS.md: no GPL, LGPL or AGPL code, not even reading it):\n");
        for c in &copyleft {
            report.push_str(&format!("  {} {} ({})\n", c.name, c.version, c.license));
        }
        if check {
            return Err(report);
        }
        eprint!("{report}");
    }
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

/// Can the project use a crate with this licence expression without taking on a copyleft
/// obligation?
///
/// The licences the project refuses are GPL, LGPL and AGPL (AGENTS.md). A crate that offers a
/// permissive alternative is not one of them: the project uses it under that alternative. So the
/// answer is about the expression, not about one identifier inside it:
///
/// * `MIT` -> yes.
/// * `MIT OR Apache-2.0 OR LGPL-2.1-or-later` -> yes (the project takes MIT).
/// * `Apache-2.0 OR GPL-2.0-only` -> yes (the project takes Apache-2.0).
/// * `GPL-3.0-only` -> no.
/// * `MIT AND GPL-3.0` -> no (the project obeys both, and one of them is copyleft).
///
/// An expression that cannot be read (an empty licence, or one that is not SPDX) is not a
/// failure: the check does not know, and a licence nobody can parse is not a copyleft one. The
/// catalogue still lists the crate and its licence file.
fn allows_permissive_use(expression: &str) -> bool {
    let tokens = tokenize(expression);
    let mut parser = ExprParser { tokens: &tokens, pos: 0 };
    // `None` is "cannot be read"; that is not a copyleft licence.
    parser.or_expr().unwrap_or(true)
}

/// Is this licence identifier one of the copyleft ones?
fn is_copyleft(identifier: &str) -> bool {
    let id = identifier.trim().to_ascii_uppercase();
    ["GPL", "LGPL", "AGPL"].iter().any(|prefix| id == *prefix || id.starts_with(&format!("{prefix}-")))
}

/// The licence expression, split into words and parentheses.
fn tokenize(expression: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    for ch in expression.chars() {
        if ch == '(' || ch == ')' {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
            tokens.push(ch.to_string());
        } else if ch.is_alphanumeric() || ch == '-' || ch == '.' || ch == '+' {
            word.push(ch);
        } else if ch.is_whitespace() {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
        } else if !word.is_empty() {
            // A character that is not part of a licence identifier (a slash, for example):
            // it ends the word it was in.
            tokens.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    tokens
}

/// A reader of the SPDX licence expressions: `OR` means the project picks one alternative, `AND`
/// means it obeys all of them.
struct ExprParser<'a> {
    tokens: &'a [String],
    pos: usize,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        if self.peek().is_some_and(|t| t.eq_ignore_ascii_case(keyword)) {
            self.pos += 1;
            return true;
        }
        false
    }

    /// `or := and ('OR' and)*`: permissive when ANY branch is permissive.
    fn or_expr(&mut self) -> Option<bool> {
        let mut permissive = self.and_expr()?;
        while self.eat_keyword("OR") {
            let branch = self.and_expr()?;
            permissive = permissive || branch;
        }
        Some(permissive)
    }

    /// `and := primary ('AND' primary)*`: permissive when EVERY part is permissive.
    fn and_expr(&mut self) -> Option<bool> {
        let mut permissive = self.primary()?;
        while self.eat_keyword("AND") {
            let part = self.primary()?;
            permissive = permissive && part;
        }
        Some(permissive)
    }

    /// `primary := '(' or ')' | identifier ['WITH' identifier]`.
    fn primary(&mut self) -> Option<bool> {
        if self.peek() == Some("(") {
            self.pos += 1;
            let inner = self.or_expr()?;
            if self.peek() != Some(")") {
                return None;
            }
            self.pos += 1;
            return Some(inner);
        }
        let identifier = self.tokens.get(self.pos)?.clone();
        self.pos += 1;
        // "GPL-2.0-only WITH Classpath-exception-2.0" is still copyleft: the exception does not
        // make the licence permissive, so only the base counts.
        if self.eat_keyword("WITH") {
            self.tokens.get(self.pos)?;
            self.pos += 1;
        }
        Some(!is_copyleft(&identifier))
    }
}

struct Crate {
    name: String,
    version: String,
    license: String,
    repository: String,
    /// (file name, text) of the licence files shipped in the crate.
    files: Vec<(String, String)>,
}

fn generate(root: &Path) -> Result<(String, Vec<Crate>), String> {
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
    Ok((render(root, &crates), crates))
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
         SolidCraft's own licence is in LICENSE-MIT and LICENSE-APACHE, and NOTICE. Its brand and interface (the UI kit,\n\
         the look, the theme, the name, logo and app icon) are under LICENSE-BRAND: using them outside SolidCraft needs\n\
         the maintainers' written permission.\n\n",
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
         - LiberationSans-Regular.ttf (crates/sketch/assets/fonts/liberation-sans/LiberationSans-Regular.ttf)\n  \
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
         - CraftUI-KR.otf (assets/fonts/craft-ui-kr/CraftUI-KR.otf)\n  \
           Copyright 2014-2021 Adobe / Google Inc. (Noto Sans KR subset for SolidCraft web ko)\n  \
           Source: https://github.com/notofonts/noto-cjk (Sans SubsetOTF/KR)\n\n\
         - IBMPlexSans-Variable.ttf (crates/look/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf)\n  \
           Copyright 2017 IBM Corp. with Reserved Font Name \"Plex\"\n  \
           Source: https://github.com/IBM/plex\n\n\
         - IBMPlexMono-Regular.ttf (crates/look/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf)\n  \
           Copyright 2017 IBM Corp. with Reserved Font Name \"Plex\"\n  \
           Source: https://github.com/IBM/plex\n\n\
         - NotoSansArabic-Regular.ttf (assets/fonts/noto-sans-arabic/NotoSansArabic-Regular.ttf)\n  \
           Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic)\n  \
           Source: https://github.com/notofonts/notofonts.github.io\n\n\
         - NotoSansKR-Regular.otf, NotoSansSC-Regular.otf and NotoSansTC-Regular.otf (assets/fonts/noto-sans-kr, -sc and -tc)\n  \
           Copyright 2014-2021 Adobe (http://www.adobe.com/)\n  \
           Source: https://github.com/notofonts/noto-cjk\n\n"
    );
    let ofl_path = root.join("assets/fonts/biz-udp-gothic/OFL.txt");
    let fallback_path = root.join("crates/sketch/assets/fonts/liberation-sans/OFL.txt");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permissive_licence_can_be_used() {
        assert!(allows_permissive_use("MIT"));
        assert!(allows_permissive_use("Apache-2.0"));
        assert!(allows_permissive_use("Apache-2.0 WITH LLVM-exception"));
    }

    #[test]
    fn an_or_with_a_permissive_alternative_can_be_used() {
        // The two the tree really has: the project takes the permissive one.
        assert!(allows_permissive_use("MIT OR Apache-2.0 OR LGPL-2.1-or-later"));
        assert!(allows_permissive_use("Apache-2.0 OR GPL-2.0-only"));
    }

    #[test]
    fn a_copyleft_licence_alone_cannot_be_used() {
        assert!(!allows_permissive_use("GPL-3.0-only"));
        assert!(!allows_permissive_use("LGPL-2.1"));
        assert!(!allows_permissive_use("AGPL-3.0-or-later"));
        assert!(!allows_permissive_use("gpl-3.0"));
    }

    #[test]
    fn an_and_with_a_copyleft_part_cannot_be_used() {
        assert!(!allows_permissive_use("MIT AND GPL-3.0"));
        assert!(!allows_permissive_use("(MIT OR Apache-2.0) AND GPL-3.0"));
    }

    #[test]
    fn an_exception_does_not_make_a_copyleft_licence_permissive() {
        assert!(!allows_permissive_use("GPL-2.0-only WITH Classpath-exception-2.0"));
    }

    #[test]
    fn a_licence_that_cannot_be_read_is_not_a_failure() {
        // The check does not know, and what it does not know is not copyleft.
        assert!(allows_permissive_use(""));
        assert!(allows_permissive_use("MIT / Apache-2.0"));
        assert!(allows_permissive_use("SEE LICENSE IN"));
    }

    #[test]
    fn the_copyleft_identifiers_are_the_three_families() {
        assert!(is_copyleft("GPL-2.0-only"));
        assert!(is_copyleft("LGPL-2.1-or-later"));
        assert!(is_copyleft("AGPL-3.0"));
        assert!(!is_copyleft("MIT"));
        assert!(!is_copyleft("Apache-2.0"));
        assert!(!is_copyleft("Unicode-3.0"));
        assert!(!is_copyleft("BSL-1.0"));
    }
}
