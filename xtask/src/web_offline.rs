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
/// Files the app fetches only when it needs them (Japanese ~4.7 MB and the Chinese and Korean
/// subsets under `fonts/` in apps/solidcraft-web): not kept for offline use, so not every user
/// downloads them. The worker leaves them to the network.
const ON_DEMAND: &[&str] = &["fonts/"];
/// The version that makes sw.js retire (the template's own).
const RETIRE_VERSION: &str = "dev";
const COMPUTE_FILES: [&str; 3] = ["solidcraft-compute-worker_loader.js", "solidcraft-compute-worker.js", "solidcraft-compute-worker_bg.wasm"];

pub fn run(root: &Path, dir: Option<&str>) -> Result<(), String> {
    let dir = match dir {
        Some(d) => PathBuf::from(d),
        None => {
            let dir = PathBuf::from(
                std::env::var("TRUNK_STAGING_DIR").map_err(|_| "web-offline: give the built site's directory (or run as Trunk's hook)")?,
            );
            if std::env::var("TRUNK_PROFILE").is_ok_and(|p| p != "release") {
                prepare_compute_bundle(&dir)?;
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
    prepare_compute_bundle(dir)?;
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
    std::fs::write(dir.join(SW), sw).map_err(|e| format!("{}: {e}", dir.join(SW).display()))?;
    let build = json!({ "version": version, "files": manifest }).to_string();
    std::fs::write(dir.join(BUILD), build).map_err(|e| format!("{}: {e}", dir.join(BUILD).display()))?;
    Ok((version, files.len()))
}

/// Trunk 0.21.14 names worker artifacts without a content hash. Pin the three
/// generated files together, before the offline manifest hashes index.html.
/// Paths inside the generated module and loader stay relative to this bundle.
fn prepare_compute_bundle(dir: &Path) -> Result<(), String> {
    if !COMPUTE_FILES.iter().any(|file| dir.join(file).exists()) {
        return Ok(()); // fixture or an already packaged build
    }
    let mut digest = Sha256::new();
    for file in COMPUTE_FILES {
        let bytes = std::fs::read(dir.join(file)).map_err(|e| format!("incomplete browser worker bundle ({file}): {e}"))?;
        digest.update(Sha256::digest(bytes));
    }
    let identity: String = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let relative = format!("workers/{identity}");
    let index = dir.join("index.html");
    let html = std::fs::read_to_string(&index).map_err(|e| format!("{}: {e}", index.display()))?;
    let (head, tail) = html.split_once("</head>").ok_or("browser worker bundle has no index head")?;
    if head.contains("solidcraft-compute-worker") {
        return Err("index already contains a browser worker loader".into());
    }
    let loader = COMPUTE_FILES[0];
    let html = format!("{head}<meta name=\"solidcraft-compute-worker\" content=\"./{relative}/{loader}\">\n</head>{tail}");
    let destination = dir.join(relative);
    std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
    for file in COMPUTE_FILES {
        std::fs::rename(dir.join(file), destination.join(file)).map_err(|e| format!("could not package browser worker {file}: {e}"))?;
    }
    std::fs::write(&index, html).map_err(|e| format!("{}: {e}", index.display()))
}

/// Write `dir/sw.js` as the worker that keeps nothing and retires a kept release build.
pub fn retire(root: &Path, dir: &Path) -> Result<(), String> {
    let template = template(root)?;
    let sw = render(&template, RETIRE_VERSION, &json!([]))?;
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
    fn compute_bundle_identity_covers_loader_glue_and_wasm_and_generation_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("solidcraft-compute-bundle-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let package = |variant: &str, changed: Option<&str>| {
            let stage = dir.join(variant);
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("index.html"), "<html><head></head><body></body></html>").unwrap();
            for file in COMPUTE_FILES {
                let bytes = if Some(file) == changed { "new worker asset" } else { file };
                std::fs::write(stage.join(file), bytes).unwrap();
            }
            let version = generate(&crate::root(), &stage).unwrap();
            let index = std::fs::read_to_string(stage.join("index.html")).unwrap();
            let loader = index.split_once("content=\"./").unwrap().1.split_once('"').unwrap().0;
            let bundle = Path::new(loader).parent().unwrap();
            for file in COMPUTE_FILES {
                assert!(!stage.join(file).exists());
                assert!(stage.join(bundle).join(file).is_file());
            }
            let manifest = std::fs::read(stage.join(BUILD)).unwrap();
            assert_eq!(version, generate(&crate::root(), &stage).unwrap());
            assert_eq!(manifest, std::fs::read(stage.join(BUILD)).unwrap());
            assert_eq!(version.1, 4, "index plus the three exact worker files");
            loader.to_owned()
        };
        let a = package("a", None);
        assert_eq!(a, package("same", None));
        for file in COMPUTE_FILES {
            assert_ne!(a, package(file, Some(file)));
        }
        let incomplete = dir.join("incomplete");
        std::fs::create_dir_all(&incomplete).unwrap();
        std::fs::write(incomplete.join(COMPUTE_FILES[0]), "loader only").unwrap();
        assert!(generate(&crate::root(), &incomplete).is_err());
        assert!(incomplete.join(COMPUTE_FILES[0]).is_file(), "verify completeness before moving files");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
