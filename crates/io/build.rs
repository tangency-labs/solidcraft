//! What the model cache is keyed by. The digest itself is [`crate::evaluation`]'s, so the build
//! script and the tests that prove its scope are one implementation: this file only hands it to
//! the crate as `SOLIDCRAFT_EVALUATION_SOURCE` (and the commit as `SOLIDCRAFT_SOURCE_COMMIT`).

#[path = "src/evaluation.rs"]
mod evaluation;

use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = crate_dir.join("../..");
    let commit = evaluation::git(&crate_dir, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_SOURCE_COMMIT={commit}");
    let source = evaluation::source(&root).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_SOURCE={source}");
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_CRATES={}", evaluation::EVALUATION_CRATES.join(","));
    // A new commit in this checkout (or worktree) restamps the build.
    if let Some(dir) = evaluation::git(&crate_dir, &["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={}/logs/HEAD", dir.trim());
    }
    // An edit to what computes models restamps it too: the digest carries uncommitted changes.
    for path in evaluation::watched() {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
