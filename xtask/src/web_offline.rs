//! `cargo xtask web-offline [DIR]`: let a web build work offline and update itself.
//!
//! Runs as Trunk's post_build hook (apps/solidcraft-web/Trunk.toml) on the staging directory
//! (`TRUNK_STAGING_DIR`), or on DIR. It writes `sw.js` from apps/solidcraft-web/sw.js with this
//! build's version and files (each with its SHA-256, which the worker checks every download
//! against, so a deployment that changes mid-download never mixes into a kept build), and the
//! same as `build.json` (which launches check to find a newly deployed build). The version is the
//! workspace version plus a hash of all that, so any change to the build changes `sw.js`, which
//! is what makes browsers install the update.
//! Debug builds (`trunk serve`) get the unbuilt template instead, a worker that keeps nothing and
//! retires one a release build left on the same origin.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The template line the build is written into.
const MARKER: &str = "/* @build */";
const SW: &str = "sw.js";
const BUILD: &str = "build.json";
/// Files the app fetches only when it needs them (the CJK font, ~4.7 MB: `JAPANESE_FONT_PATH`
/// in apps/solidcraft-web/src/main.rs): not kept for offline use, so not every user downloads
/// them. The worker leaves them to the network.
const ON_DEMAND: &[&str] = &["fonts/"];
/// The version that makes sw.js retire (the template's own).
const RETIRE_VERSION: &str = "dev";

pub fn run(root: &Path, dir: Option<&str>) -> Result<(), String> {
    let dir = match dir {
        Some(d) => PathBuf::from(d),
        None => {
            let dir = PathBuf::from(
                std::env::var("TRUNK_STAGING_DIR").map_err(|_| "web-offline: give the built site's directory (or run as Trunk's hook)")?,
            );
            if std::env::var("TRUNK_PROFILE").is_ok_and(|p| p != "release") {
                retire(root, &dir)?;
                println!("web-offline: debug build, {SW} retires a kept release build");
                return Ok(());
            }
            dir
        }
    };
    let (version, n) = generate(root, &dir)?;
    println!("web-offline: {SW} keeps {n} files (version {version})");
    Ok(())
}

/// Write `dir/sw.js` and `dir/build.json` for the site in `dir`; returns its version and how many
/// files it keeps.
pub fn generate(root: &Path, dir: &Path) -> Result<(String, usize), String> {
    let files = list(dir)?;
    if !files.iter().any(|(name, _)| name == "index.html") {
        return Err(format!("web-offline: {} has no index.html", dir.display()));
    }
    let manifest: Vec<Value> = files.iter().map(|(name, bytes)| json!({ "file": name, "sha256": sha256(bytes) })).collect();
    let manifest = Value::Array(manifest);
    let digest = sha256(manifest.to_string().as_bytes());
    let version = format!("{}-{}", env!("CARGO_PKG_VERSION"), digest.get(..16).unwrap_or(&digest));
    let template = template(root)?;
    let sw = render(&template, &version, &manifest)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join(SW), sw).map_err(|e| format!("{}: {e}", dir.join(SW).display()))?;
    let build = json!({ "version": version, "files": manifest }).to_string();
    std::fs::write(dir.join(BUILD), build).map_err(|e| format!("{}: {e}", dir.join(BUILD).display()))?;
    Ok((version, files.len()))
}

/// Write `dir/sw.js` as the worker that keeps nothing and retires a kept release build.
pub fn retire(root: &Path, dir: &Path) -> Result<(), String> {
    let template = template(root)?;
    let sw = render(&template, RETIRE_VERSION, &json!([]))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join(SW), sw).map_err(|e| format!("{}: {e}", dir.join(SW).display()))
}

fn template(root: &Path) -> Result<String, String> {
    let path = root.join("apps/solidcraft-web").join(SW);
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every file under `dir` (but `sw.js`, `build.json` and ON_DEMAND ones) as (URL path relative to `dir`, contents), sorted.
fn list(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
        let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for e in rd {
            let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            let p = e.path();
            if p.is_dir() {
                walk(&p, &format!("{name}/"), out)?;
            } else if name != SW && name != BUILD && !ON_DEMAND.iter().any(|p| name.starts_with(p)) {
                out.push((name, std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Lower-case hex SHA-256, as the worker's `crypto.subtle.digest` check computes it.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The template with its marker line replaced by the version and files.
fn render(template: &str, version: &str, files: &Value) -> Result<String, String> {
    let build = format!("{MARKER} const VERSION = {}, FILES = {files};", Value::from(version));
    let mut found = false;
    let lines: Vec<&str> = template
        .lines()
        .map(|l| {
            if l.starts_with(MARKER) && !found {
                found = true;
                build.as_str()
            } else {
                l
            }
        })
        .collect();
    if !found {
        return Err(format!("web-offline: the {SW} template has no `{MARKER}` line"));
    }
    Ok(lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_replaces_the_marker_line_only() {
        let t = "// head\n/* @build */ const VERSION = \"dev\", FILES = [];\nconst X = 1;\n";
        let files = json!([{ "file": "a \"b\".js", "sha256": "00" }]);
        let out = render(t, "0.3.2-00ff", &files).unwrap_or_default();
        assert_eq!(
            out,
            "// head\n/* @build */ const VERSION = \"0.3.2-00ff\", FILES = [{\"file\":\"a \\\"b\\\".js\",\"sha256\":\"00\"}];\nconst X = 1;\n"
        );
        assert!(render("no marker\n", "v", &json!([])).is_err());
    }

    #[test]
    fn the_template_has_the_marker() {
        let t = std::fs::read_to_string(crate::root().join("apps/solidcraft-web").join(SW)).unwrap_or_default();
        assert!(render(&t, "v", &json!(["x"])).is_ok_and(|s| s.contains("FILES = [\"x\"]")));
    }

    #[test]
    fn sha256_is_lower_case_hex() {
        // FIPS 180-2's "abc" example.
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn generate_lists_every_file_with_its_digest() {
        let dir = std::env::temp_dir().join(format!("solidcraft-web-offline-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ok = std::fs::create_dir_all(dir.join("snippets"))
            .and_then(|()| std::fs::create_dir_all(dir.join("fonts")))
            .and_then(|()| std::fs::write(dir.join("fonts/big.ttf"), "on demand"))
            .and_then(|()| std::fs::write(dir.join("index.html"), "<html>"))
            .and_then(|()| std::fs::write(dir.join("snippets/a.js"), "abc"))
            .and_then(|()| std::fs::write(dir.join(SW), "old"));
        assert!(ok.is_ok());
        let (version, n) = generate(&crate::root(), &dir).unwrap_or_default();
        let build: Value = std::fs::read(dir.join(BUILD)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(n, 2, "on-demand files aren't listed");
        assert_eq!(build["version"], version.as_str());
        assert_eq!(build["files"][1], json!({ "file": "snippets/a.js", "sha256": sha256(b"abc") }));
        assert_eq!(build["files"][0]["file"], "index.html");
    }
}
