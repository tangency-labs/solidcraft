//! `cargo xtask plugin-examples`: the plugin guide's examples (docs/book/src/dev/plugins.md) are
//! real crates and packages in docs/book/plugin-examples, and this keeps the two the same and
//! builds each crate for wasm32-unknown-unknown as the guide does, against this checkout's WIT.
//!
//! The crates are workspaces of their own (a plugin author's crate is not part of SolidCraft),
//! with their own lockfiles. `--run` also wraps each module as a component and runs
//! `solidcraft-cli plugin test --require-cases` on its package, as the guide's last steps do.

use std::path::{Path, PathBuf};

/// The examples: the package folder and the crate that builds its component.
const EXAMPLES: &[(&str, &str)] = &[("plate-maker", "plate-maker-src"), ("box-importer", "box-importer-src")];

/// The files the guide shows whole, each as one fenced block of its language.
const SHOWN: &[(&str, &str)] = &[
    ("plate-maker/plugin.json", "json"),
    ("plate-maker/tests/default-plate.json", "json"),
    ("plate-maker-src/src/lib.rs", "rust"),
    ("box-importer/plugin.json", "json"),
    ("box-importer/tests/small.json", "json"),
    ("box-importer-src/src/lib.rs", "rust"),
];

const GUIDE: &str = "docs/book/src/dev/plugins.md";
const EXAMPLES_DIR: &str = "docs/book/plugin-examples";
const WIT: &str = "crates/craft-plugin/wit/plugin.wit";

pub fn run(root: &Path, run_tests: bool) -> Result<(), String> {
    let guide = read(&root.join(GUIDE))?;
    let blocks = fenced_blocks(&guide);
    let dir = root.join(EXAMPLES_DIR);
    let mut stale = Vec::new();
    for (file, language) in SHOWN {
        let text = read(&dir.join(file))?;
        if !blocks.iter().any(|(l, b)| l == language && *b == text) {
            stale.push(format!("{EXAMPLES_DIR}/{file} is not a ```{language} block of {GUIDE}"));
        }
    }
    // The guide shows only the part of Cargo.toml a reader adds after `cargo new`.
    let toml_blocks: Vec<&String> = blocks.iter().filter(|(l, _)| l == "toml").map(|(_, b)| b).collect();
    for (_, krate) in EXAMPLES {
        let manifest = read(&dir.join(krate).join("Cargo.toml"))?;
        if !toml_blocks.iter().any(|b| manifest.contains(b.as_str())) {
            stale.push(format!("{EXAMPLES_DIR}/{krate}/Cargo.toml does not hold the guide's ```toml block"));
        }
    }
    if !stale.is_empty() {
        return Err(format!("the plugin guide and its examples differ; change both:\n  {}", stale.join("\n  ")));
    }
    let wit = read(&root.join(WIT))?;
    let target = root.join("target").join("plugin-examples");
    for (package, krate) in EXAMPLES {
        let krate = dir.join(krate);
        // As the guide says: the crate builds against a copy of the app's WIT in its wit folder.
        std::fs::create_dir_all(krate.join("wit")).map_err(|e| format!("{}: {e}", krate.display()))?;
        std::fs::write(krate.join("wit").join("plugin.wit"), &wit).map_err(|e| format!("{}: {e}", krate.display()))?;
        let mut c = crate::cargo();
        c.args(["build", "--release", "--locked", "--target", "wasm32-unknown-unknown", "--manifest-path"])
            .arg(krate.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target);
        crate::run(c, &format!("cargo build {package} for wasm32-unknown-unknown"))?;
        if run_tests {
            let module = module_path(&target, &krate)?;
            let component = wrap(&read_bytes(&module)?)?;
            let package = dir.join(package);
            let installed = package.join(module_file(&package)?);
            with_component(&installed, &component, || {
                let mut c = crate::cargo();
                c.args(["run", "--profile", "ci", "-p", "solidcraft-cli", "--", "plugin", "test", "--require-cases"]).arg(&package);
                crate::run(c, &format!("solidcraft-cli plugin test {}", package.display()))
            })?;
        }
    }
    println!("plugin-examples: {} examples match the guide and build", EXAMPLES.len());
    Ok(())
}

/// Every fenced block of `markdown`, with its language.
fn fenced_blocks(markdown: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in markdown.lines() {
        match open.as_mut() {
            Some(_) if line.trim_end() == "```" => blocks.extend(open.take()),
            Some((_, body)) => {
                body.push_str(line);
                body.push('\n');
            }
            None => {
                if let Some(language) = line.strip_prefix("```") {
                    open = Some((language.trim().to_string(), String::new()));
                }
            }
        }
    }
    blocks
}

/// The core module cargo built for the crate at `krate`.
fn module_path(target: &Path, krate: &Path) -> Result<PathBuf, String> {
    let manifest: toml::Value = toml::from_str(&read(&krate.join("Cargo.toml"))?).map_err(|e| format!("{}: {e}", krate.display()))?;
    let name = manifest.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str).ok_or("a crate without a name")?;
    Ok(target.join("wasm32-unknown-unknown").join("release").join(format!("{}.wasm", name.replace('-', "_"))))
}

/// The package's component file, as its manifest names it.
fn module_file(package: &Path) -> Result<String, String> {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(&package.join("plugin.json"))?).map_err(|e| format!("{}: {e}", package.display()))?;
    manifest["runtime"]["module"]
        .as_str()
        .filter(|m| m.ends_with(".wasm") && !m.contains(['/', '\\']))
        .map(String::from)
        .ok_or_else(|| format!("{}: plugin.json names no runtime.module", package.display()))
}

/// The module as a component: what `wasm-tools component new` does, from the type information
/// wit-bindgen put in the module.
fn wrap(module: &[u8]) -> Result<Vec<u8>, String> {
    wit_component::ComponentEncoder::default()
        .module(module)
        .and_then(|e| e.validate(true).encode())
        .map_err(|e| format!("wrapping the module as a component: {e:#}"))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map(|text| text.replace("\r\n", "\n")).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Install the test component temporarily, restoring a developer's component on either result.
fn with_component(path: &Path, component: &[u8], run: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let original = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let result = std::fs::write(path, component).map_err(|error| format!("{}: {error}", path.display())).and_then(|()| run());
    let restored = match original {
        Some(bytes) => std::fs::write(path, bytes),
        None => std::fs::remove_file(path),
    };
    restored.map_err(|error| format!("restoring {}: {error}", path.display()))?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!("solidcraft-plugin-examples-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).expect("create scratch directory");
        path
    }

    #[test]
    fn crlf_examples_and_manifests_match_guide_blocks() {
        let dir = scratch();
        let guide = dir.join("guide.md");
        let example = dir.join("lib.rs");
        let manifest = dir.join("Cargo.toml");
        std::fs::write(&guide, "```rust\r\nfn main() {}\r\n```\r\n```toml\r\n[dependencies]\r\n```\r\n").unwrap();
        std::fs::write(&example, "fn main() {}\r\n").unwrap();
        std::fs::write(&manifest, "[package]\r\nname = \"example\"\r\n[dependencies]\r\n").unwrap();
        let blocks = fenced_blocks(&read(&guide).unwrap());
        assert!(blocks.iter().any(|(language, body)| language == "rust" && *body == read(&example).unwrap()));
        assert!(blocks.iter().any(|(language, body)| language == "toml" && read(&manifest).unwrap().contains(body)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn component_is_restored_after_success_and_failure() {
        let dir = scratch();
        let path = dir.join("example.wasm");
        for succeeds in [true, false] {
            std::fs::write(&path, b"developer component").unwrap();
            let result = with_component(&path, b"test component", || {
                assert_eq!(std::fs::read(&path).unwrap(), b"test component");
                if succeeds { Ok(()) } else { Err("test failed".into()) }
            });
            assert_eq!(result, if succeeds { Ok(()) } else { Err("test failed".into()) });
            assert_eq!(std::fs::read(&path).unwrap(), b"developer component");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_component_is_removed_after_success_and_failure() {
        let dir = scratch();
        let path = dir.join("example.wasm");
        for succeeds in [true, false] {
            let result = with_component(&path, b"test component", || {
                assert_eq!(std::fs::read(&path).unwrap(), b"test component");
                if succeeds { Ok(()) } else { Err("test failed".into()) }
            });
            assert_eq!(result.is_ok(), succeeds);
            assert!(!path.exists());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
