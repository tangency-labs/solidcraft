//! Builds SMLib, SM_API and their C API (tangency-labs/smlib, at the commit in `SMLIB_REV`) into
//! static libraries, when the `native` feature is on. The sources come from
//! `SOLIDCRAFT_SMLIB_DIR`, or from `target/smlib-<rev>` as `scripts/fetch-smlib.sh` leaves them.
//!
//! The file lists, defines and include order follow the fork's `capi/CMakeLists.txt`: SMLib/inc
//! and SM_API/inc each have a StdAfx.h, so each part must find its own first.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=SMLIB_REV");
    println!("cargo:rerun-if-env-changed=SOLIDCRAFT_SMLIB_DIR");
    if std::env::var_os("CARGO_FEATURE_NATIVE").is_none() {
        return;
    }
    if let Err(e) = build() {
        // A build script has no other way to fail the build.
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn build() -> Result<(), String> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let rev = std::fs::read_to_string(manifest.join("SMLIB_REV")).map_err(|e| format!("SMLIB_REV: {e}"))?.trim().to_string();
    let root = match std::env::var_os("SOLIDCRAFT_SMLIB_DIR") {
        Some(d) => PathBuf::from(d),
        None => manifest.join("../../target").join(format!("smlib-{}", rev.get(..12).unwrap_or(&rev))),
    };
    if !root.join("capi/include/smlib.h").is_file() {
        return Err(format!(
            "the SMLib sources are not at {} (feature `native`): run scripts/fetch-smlib.sh, or set SOLIDCRAFT_SMLIB_DIR to a tangency-labs/smlib checkout at {rev}",
            root.display()
        ));
    }
    println!("cargo:rerun-if-changed={}", root.join("capi").display());
    let src = root.join("source");
    let core = sources(&src.join("SMLib/src"), &["StdAfx.cpp", "SmDllMain.cpp"])?;
    let api = sources(&src.join("SM_API/src"), &["StdAfx.cpp"])?;
    let target = std::env::var("TARGET").unwrap_or_default();
    let msvc = target.contains("msvc");

    let base = || {
        let mut b = cc::Build::new();
        b.cpp(true).std("c++17").define("NDEBUG", None).define("NOMINMAX", None).define("SM_STATIC", None).define("SM_RAND_SEED", "20240101u");
        // Rust links the MSVC runtime as a DLL; the cc crate's default (/MD) matches.
        if msvc {
            b.flag("/EHsc").flag("/bigobj");
        }
        b
    };
    let quiet = |b: &mut cc::Build| {
        b.warnings(false);
        if msvc {
            b.flag("/w");
        } else {
            b.flag("-w");
        }
    };

    // Link order: the C API needs SM_API, which needs SMLib.
    let mut c = base();
    c.file(root.join("capi/src/smlib_c.cpp")).include(root.join("capi/include")).include(src.join("SM_API/inc")).include(src.join("SMLib/inc"));
    quiet(&mut c);
    c.try_compile("smlib_capi").map_err(|e| e.to_string())?;

    let mut a = base();
    a.files(&api).include(src.join("SM_API/inc")).include(src.join("SMLib/inc"));
    quiet(&mut a);
    a.try_compile("smlib_api").map_err(|e| e.to_string())?;

    let mut k = base();
    k.files(&core).include(src.join("SMLib/inc"));
    quiet(&mut k);
    k.try_compile("smlib_core").map_err(|e| e.to_string())?;
    Ok(())
}

/// The `.cpp` files in `dir`, sorted, without `skip`.
fn sources(dir: &Path, skip: &[&str]) -> Result<Vec<PathBuf>, String> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "cpp") && !p.file_name().is_some_and(|n| skip.iter().any(|s| n == *s)))
        .collect();
    v.sort();
    if v.is_empty() {
        return Err(format!("no sources in {}", dir.display()));
    }
    Ok(v)
}
