//! `cargo xtask shared`: prove each shared crate (`layers::Class::Shared`) would build after
//! extraction from this repository.
//!
//! For every shared crate it first checks the rules that keep a crate movable: its manifest uses
//! the workspace lints (`[lints] workspace = true`) and states a licence, a README.md sits beside
//! it, and no `include_str!`/`include_bytes!` reaches outside its own directory. Then it copies
//! the crate and the shared crates it depends on (transitively) into a scratch workspace under
//! `target/xtask-shared/<crate>/`, whose root manifest keeps this workspace's package fields,
//! lints, profiles, patches (made absolute) and external dependencies but none of its path
//! dependencies except the copied shared crates, and runs `cargo test --workspace` there with the
//! repository's Cargo.lock. A dependency on any other workspace crate is then simply missing, so
//! the build fails, which is the point.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};

use crate::layers::{self, Class};

/// Scratch workspaces, one per shared crate, under the target directory.
const SCRATCH: &str = "target/xtask-shared";
/// One target directory for every scratch workspace, so their builds share artefacts.
const SCRATCH_TARGET: &str = "target/xtask-shared-target";

pub fn run(root: &Path) -> Result<(), String> {
    let meta = crate::metadata()?;
    let crates = layers::from_metadata(&meta, None)?;
    let dirs = manifest_dirs(&meta)?;
    let shared: BTreeSet<&str> = crates.iter().filter(|c| layers::classify(&c.name) == Some(Class::Shared)).map(|c| c.name.as_str()).collect();
    if shared.is_empty() {
        println!("shared: no shared crates (layers::Class::Shared) to check");
        return Ok(());
    }
    let dir_of = |name: &str| dirs.get(name).cloned().ok_or_else(|| format!("{name}: no manifest_path in cargo metadata"));

    let mut problems = Vec::new();
    for &name in &shared {
        problems.extend(static_rules(name, &dir_of(name)?)?);
    }
    if !problems.is_empty() {
        println!("{} shared-crate rule violation(s):", problems.len());
        for p in &problems {
            println!("  - {p}");
        }
        return Err(format!("{} shared-crate rule violation(s)", problems.len()));
    }

    let root_manifest = read_toml(&root.join("Cargo.toml"))?;
    let shared_dirs: BTreeMap<PathBuf, &str> = shared.iter().map(|&n| Ok((canonical(&dir_of(n)?)?, n))).collect::<Result<_, String>>()?;
    for &name in &shared {
        let closure = shared_closure(name, &crates);
        let scratch = root.join(SCRATCH).join(layers::short_name(name));
        if scratch.exists() {
            std::fs::remove_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        }
        std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        let mut members = Vec::new();
        for member in &closure {
            let dest = layers::short_name(member).to_string();
            copy_dir(&dir_of(member.as_str())?, &scratch.join(&dest))?;
            members.push(dest);
        }
        let manifest = scratch_manifest(root, &root_manifest, &members, &shared_dirs)?;
        let text = toml::to_string(&manifest).map_err(|e| format!("{name}: scratch Cargo.toml: {e}"))?;
        write(&scratch.join("Cargo.toml"), &text)?;
        let lock = root.join("Cargo.lock");
        if lock.is_file() {
            std::fs::copy(&lock, scratch.join("Cargo.lock")).map_err(|e| format!("{}: {e}", lock.display()))?;
        }
        let mut c = crate::cargo();
        c.current_dir(&scratch).env("CARGO_TARGET_DIR", root.join(SCRATCH_TARGET)).args(["test", "--workspace"]);
        crate::run(c, &format!("cargo test --workspace (shared crate {name} on its own: {})", members.join(", ")))
            .map_err(|e| format!("shared crate {name} does not build and test on its own: {e}"))?;
    }
    println!("shared: {} shared crate(s) build and test on their own", shared.len());
    Ok(())
}

/// Package name to the directory holding its Cargo.toml.
fn manifest_dirs(meta: &serde_json::Value) -> Result<BTreeMap<String, PathBuf>, String> {
    let pkgs = meta["packages"].as_array().ok_or("metadata: no packages array")?;
    let mut out = BTreeMap::new();
    for p in pkgs {
        let (Some(name), Some(manifest)) = (p["name"].as_str(), p["manifest_path"].as_str()) else { continue };
        if let Some(dir) = Path::new(manifest).parent() {
            out.insert(name.to_string(), dir.to_path_buf());
        }
    }
    Ok(out)
}

/// The crate and every shared crate it reaches through workspace dependencies of any kind.
fn shared_closure(name: &str, crates: &[layers::Crate]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![name.to_string()];
    while let Some(n) = todo.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        let Some(c) = crates.iter().find(|c| c.name == n) else { continue };
        todo.extend(c.deps.iter().filter(|d| d.workspace && layers::classify(&d.name) == Some(Class::Shared)).map(|d| d.name.clone()));
    }
    seen
}

/// The scratch workspace's root manifest (see the module comment).
fn scratch_manifest(root: &Path, src: &toml::Table, members: &[String], shared_dirs: &BTreeMap<PathBuf, &str>) -> Result<toml::Table, String> {
    let empty = toml::Table::new();
    let ws = src.get("workspace").and_then(toml::Value::as_table).unwrap_or(&empty);

    let mut workspace = toml::Table::new();
    workspace.insert("resolver".into(), toml::Value::String("3".into()));
    workspace.insert("members".into(), toml::Value::Array(members.iter().cloned().map(toml::Value::String).collect()));
    for key in ["package", "lints"] {
        if let Some(v) = ws.get(key) {
            workspace.insert(key.into(), v.clone());
        }
    }
    let mut deps = toml::Table::new();
    for (key, value) in ws.get("dependencies").and_then(toml::Value::as_table).unwrap_or(&empty) {
        let Some(path) = path_of(value) else {
            deps.insert(key.clone(), value.clone());
            continue;
        };
        // Path dependencies stay only when they are shared crates, pointed at their copies.
        let Ok(dir) = canonical(&root.join(path)) else { continue };
        // Only the copied ones: a shared crate outside this closure has no directory here.
        if let Some(pkg) = shared_dirs.get(&dir).filter(|pkg| members.iter().any(|m| m == layers::short_name(pkg))) {
            let mut entry = value.as_table().cloned().unwrap_or_default();
            entry.insert("path".into(), toml::Value::String(layers::short_name(pkg).to_string()));
            deps.insert(key.clone(), toml::Value::Table(entry));
        }
    }
    workspace.insert("dependencies".into(), toml::Value::Table(deps));

    let mut out = toml::Table::new();
    out.insert("workspace".into(), toml::Value::Table(workspace));
    if let Some(profile) = src.get("profile") {
        out.insert("profile".into(), profile.clone());
    }
    if let Some(patches) = src.get("patch").and_then(toml::Value::as_table) {
        let mut patch = toml::Table::new();
        for (registry, entries) in patches {
            let mut table = toml::Table::new();
            for (key, value) in entries.as_table().unwrap_or(&empty) {
                let mut value = value.clone();
                if let (Some(path), Some(entry)) = (path_of(&value).map(str::to_string), value.as_table_mut()) {
                    let abs = root.join(&path);
                    let abs = abs.to_str().ok_or_else(|| format!("patch {registry}.{key}: path {} is not UTF-8", abs.display()))?;
                    entry.insert("path".into(), toml::Value::String(abs.to_string()));
                }
                table.insert(key.clone(), value);
            }
            patch.insert(registry.clone(), toml::Value::Table(table));
        }
        out.insert("patch".into(), toml::Value::Table(patch));
    }
    Ok(out)
}

fn path_of(value: &toml::Value) -> Option<&str> {
    value.as_table()?.get("path")?.as_str()
}

/// The manifest, README and include rules for one shared crate.
fn static_rules(name: &str, dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let manifest = read_toml(&dir.join("Cargo.toml"))?;
    let lints_from_workspace = manifest.get("lints").and_then(|l| l.get("workspace")).and_then(toml::Value::as_bool) == Some(true);
    if !lints_from_workspace {
        out.push(format!("{name}: Cargo.toml must use the workspace lints (`[lints] workspace = true`)"));
    }
    let licence = manifest.get("package").and_then(|p| p.get("license"));
    let has_licence = match licence {
        Some(toml::Value::String(s)) => !s.trim().is_empty(),
        Some(v) => v.get("workspace").and_then(toml::Value::as_bool) == Some(true),
        None => false,
    };
    if !has_licence {
        out.push(format!("{name}: Cargo.toml must state a licence (`license = \"...\"` or `license.workspace = true`)"));
    }
    if !dir.join("README.md").is_file() {
        out.push(format!("{name}: needs a README.md beside its Cargo.toml stating its public API"));
    }
    let crate_dir = canonical(dir)?;
    let mut files = Vec::new();
    rust_files(dir, &mut files)?;
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let tokens: TokenStream = text.parse().map_err(|e| format!("{}: does not tokenise: {e}", file.display()))?;
        let base = canonical(file.parent().unwrap_or(dir))?;
        let shown = file.strip_prefix(dir).unwrap_or(&file).display().to_string();
        for (mac, target) in includes(tokens, &base, &crate_dir) {
            let inside = match &target {
                Included::Relative(p) => normalise(p).starts_with(&crate_dir),
                Included::Absolute => false,
            };
            if !inside {
                out.push(format!("{name}: {shown}: `{mac}!` reads outside the crate's own directory; move the file into the crate"));
            }
        }
    }
    Ok(out)
}

enum Included {
    /// The resolved path (not yet normalised).
    Relative(PathBuf),
    /// An absolute literal path, which never travels with the crate.
    Absolute,
}

/// Every `include_str!`/`include_bytes!` whose path is a string literal, or
/// `concat!(env!("CARGO_MANIFEST_DIR"), "...")`, with the file it names. Paths built any other way
/// cannot be resolved here and are left alone.
fn includes(tokens: TokenStream, base: &Path, crate_dir: &Path) -> Vec<(String, Included)> {
    let mut out = Vec::new();
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Ident(id) if *id == "include_str" || *id == "include_bytes" => {
                let (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(args))) = (trees.get(i + 1), trees.get(i + 2)) else { continue };
                if bang.as_char() != '!' {
                    continue;
                }
                if let Some(target) = include_target(args.stream(), base, crate_dir) {
                    out.push((id.to_string(), target));
                }
            }
            TokenTree::Group(g) => out.extend(includes(g.stream(), base, crate_dir)),
            _ => {}
        }
    }
    out
}

fn include_target(args: TokenStream, base: &Path, crate_dir: &Path) -> Option<Included> {
    let trees: Vec<TokenTree> = args.into_iter().collect();
    match &trees[..] {
        [TokenTree::Literal(lit)] => {
            let path = string_literal(lit)?;
            if Path::new(&path).is_absolute() { Some(Included::Absolute) } else { Some(Included::Relative(base.join(path))) }
        }
        [TokenTree::Ident(concat), TokenTree::Punct(bang), TokenTree::Group(inner)] if *concat == "concat" && bang.as_char() == '!' => {
            let parts: Vec<TokenTree> = inner.stream().into_iter().collect();
            let [TokenTree::Ident(env), TokenTree::Punct(b), TokenTree::Group(var), rest @ ..] = &parts[..] else { return None };
            let var: Vec<TokenTree> = var.stream().into_iter().collect();
            let from_manifest_dir = matches!(&var[..], [TokenTree::Literal(l)] if string_literal(l).as_deref() == Some("CARGO_MANIFEST_DIR"));
            if *env != "env" || b.as_char() != '!' || !from_manifest_dir {
                return None;
            }
            let mut tail = String::new();
            for t in rest {
                match t {
                    TokenTree::Punct(p) if p.as_char() == ',' => {}
                    TokenTree::Literal(l) => tail.push_str(&string_literal(l)?),
                    _ => return None,
                }
            }
            Some(Included::Relative(crate_dir.join(tail.trim_start_matches(['/', '\\']))))
        }
        _ => None,
    }
}

fn string_literal(lit: &proc_macro2::Literal) -> Option<String> {
    syn::parse2::<syn::LitStr>(TokenStream::from(TokenTree::Literal(lit.clone()))).ok().map(|s| s.value())
}

/// Resolve `.` and `..` without touching the file system (the file may not exist).
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Every .rs file under `dir`, skipping build output.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_dir() {
            if entry.file_name() != "target" {
                rust_files(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
    Ok(())
}

/// Copy `from` into `to`, skipping any `target` directory.
fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        let src = entry.path();
        let dest = to.join(entry.file_name());
        if src.is_dir() {
            if entry.file_name() != "target" {
                copy_dir(&src, &dest)?;
            }
        } else {
            std::fs::copy(&src, &dest).map_err(|e| format!("{} -> {}: {e}", src.display(), dest.display()))?;
        }
    }
    Ok(())
}

fn read_toml(path: &Path) -> Result<toml::Table, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(src: &str, base: &str, crate_dir: &str) -> Vec<(String, bool)> {
        let crate_dir = Path::new(crate_dir);
        includes(src.parse().unwrap(), Path::new(base), crate_dir)
            .into_iter()
            .map(|(m, t)| {
                let inside = match t {
                    Included::Relative(p) => normalise(&p).starts_with(crate_dir),
                    Included::Absolute => false,
                };
                (m, inside)
            })
            .collect()
    }

    #[test]
    fn includes_inside_the_crate_pass() {
        let src = r#"const A: &str = include_str!("../assets/a.txt"); fn f() { let b = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/b.bin")); }"#;
        assert_eq!(found(src, "/w/crates/x/src", "/w/crates/x"), [("include_str".to_string(), true), ("include_bytes".to_string(), true)]);
    }

    #[test]
    fn includes_outside_the_crate_flagged() {
        let src = r#"
            const A: &str = include_str!("../../../assets/a.txt");
            const B: &[u8] = include_bytes!("/etc/b.bin");
            const C: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../other/c.txt"));
            const D: &[&str] = &[include_str!(r"../../d.txt")];
        "#;
        let v = found(src, "/w/crates/x/src", "/w/crates/x");
        assert_eq!(v.len(), 4);
        assert!(v.iter().all(|(_, inside)| !inside), "{v:?}");
    }

    #[test]
    fn unresolvable_include_paths_are_left_alone() {
        assert!(found(r#"const A: &str = include_str!(env!("SOMEWHERE"));"#, "/w/x/src", "/w/x").is_empty());
    }

    #[test]
    fn scratch_manifest_keeps_external_and_shared_dependencies_only() {
        let root = std::env::temp_dir().join(format!("xtask-shared-test-{}", std::process::id()));
        let shared_dir = root.join("crates/ui-registry");
        let other_dir = root.join("crates/engine");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        let src: toml::Table = toml::from_str(
            r#"
            [workspace]
            members = ["crates/*"]
            [workspace.package]
            edition = "2024"
            [workspace.dependencies]
            serde = "1"
            solidcraft-ui-registry = { path = "crates/ui-registry" }
            solidcraft-engine = { path = "crates/engine" }
            [workspace.lints.rust]
            unsafe_code = "forbid"
            [profile.ci]
            inherits = "dev"
            [patch.crates-io]
            winit = { path = "vendor/winit" }
            "#,
        )
        .unwrap();
        let shared_dirs = BTreeMap::from([(std::fs::canonicalize(&shared_dir).unwrap(), "solidcraft-ui-registry")]);
        let m = scratch_manifest(&root, &src, &["ui-registry".into()], &shared_dirs).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let ws = m["workspace"].as_table().unwrap();
        assert_eq!(ws["resolver"].as_str(), Some("3"));
        let deps = ws["dependencies"].as_table().unwrap();
        assert!(deps.contains_key("serde") && !deps.contains_key("solidcraft-engine"));
        assert_eq!(path_of(&deps["solidcraft-ui-registry"]), Some("ui-registry"));
        assert!(ws.contains_key("package") && ws.contains_key("lints") && m.contains_key("profile"));
        let winit = path_of(&m["patch"]["crates-io"]["winit"]).unwrap();
        assert!(Path::new(winit).is_absolute() && winit.ends_with("vendor/winit"));
    }
}
