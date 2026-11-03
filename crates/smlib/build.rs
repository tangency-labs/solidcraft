//! Links SMLib, SM_API and their C API (tangency-labs/smlib, at the commit in `SMLIB_REV`) as
//! static libraries, when the `native` feature is on. In order:
//! - `SOLIDCRAFT_SMLIB_DIR`: a checkout, compiled here;
//! - the prebuilt library for the target, as `scripts/fetch-smlib.sh` leaves it in
//!   `target/smlib-<rev>-prebuilt/<target>`, when its SHA-256 is the one in `SMLIB_PREBUILT`;
//! - the sources in `target/smlib-<rev>` (`scripts/fetch-smlib.sh --source`), compiled here.
//!
//! The prebuilt libraries come from the fork's `capi/CMakeLists.txt`. The compiled path follows
//! it: the same file lists, defines and include order (SMLib/inc and SM_API/inc each have a
//! StdAfx.h, so each part must find its own first).

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=SMLIB_REV");
    println!("cargo:rerun-if-changed=SMLIB_PREBUILT");
    println!("cargo:rerun-if-env-changed=SOLIDCRAFT_SMLIB_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
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
    let rev12 = rev.get(..12).unwrap_or(&rev);
    let target = std::env::var("TARGET").unwrap_or_default();
    let fetched = manifest.join("../../target");
    let root = match std::env::var_os("SOLIDCRAFT_SMLIB_DIR") {
        Some(d) => PathBuf::from(d),
        None => {
            let prebuilt = fetched.join(format!("smlib-{rev12}-prebuilt")).join(&target);
            if let Some(sha) = pinned(&manifest, &target)? {
                println!("cargo:rerun-if-changed={}", prebuilt.join(".sha256").display());
                let have = std::fs::read_to_string(prebuilt.join(".sha256")).unwrap_or_default();
                if have.trim() == sha {
                    return link_prebuilt(&prebuilt, &target);
                }
            }
            fetched.join(format!("smlib-{rev12}"))
        }
    };
    if !root.join("capi/include/smlib.h").is_file() {
        return Err(format!(
            "SMLib {rev} for {target} is not fetched (feature `native`): run scripts/fetch-smlib.sh {target}, or set SOLIDCRAFT_SMLIB_DIR to a tangency-labs/smlib checkout at that commit"
        ));
    }
    println!("cargo:rerun-if-changed={}", root.join("capi").display());
    // SMLib's own sources too: a fix to the kernel must rebuild it.
    println!("cargo:rerun-if-changed={}", root.join("source").display());
    let src = root.join("source");
    let core = sources(&src.join("SMLib/src"), &["StdAfx.cpp", "SmDllMain.cpp"])?;
    let api = sources(&src.join("SM_API/src"), &["StdAfx.cpp"])?;
    let msvc = target.contains("msvc");
    let wasi = if target == "wasm32-unknown-unknown" { Some(wasi_sdk()?) } else { None };

    let base = || {
        let mut b = cc::Build::new();
        b.cpp(true).std("c++17").define("NDEBUG", None).define("NOMINMAX", None).define("SM_STATIC", None).define("SM_RAND_SEED", "20240101u");
        // Rust links the MSVC runtime as a DLL; the cc crate's default (/MD) matches.
        if msvc {
            b.flag("/EHsc").flag("/bigobj");
        } else {
            // No fused multiply-add, as the prebuilt libraries (capi/CMakeLists.txt): SMLib's
            // tangent cases decide on the last bits. MSVC does not contract by default.
            b.flag("-ffp-contract=off");
        }
        if let Some(sdk) = &wasi {
            b.compiler(sdk.join("bin/clang++")).archiver(sdk.join("bin/llvm-ar")).target(WASI_TARGET);
            b.flag(format!("--sysroot={}", sdk.join("share/wasi-sysroot").display())).flag("-fwasm-exceptions");
            // The exnref encoding, as wasi-sdk's libc++ and Rust use: a module may not mix it with
            // the legacy one clang still defaults to.
            b.flag("-mllvm").flag("-wasm-use-legacy-eh=false");
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
    let capi = sources(&root.join("capi/src"), &[])?;
    let mut c = base();
    c.files(&capi).include(root.join("capi/include")).include(src.join("SM_API/inc")).include(src.join("SMLib/inc"));
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

    // The C++ runtime and the C library SMLib needs, from wasi-sdk: the exception-handling build of
    // libc++. The few WASI calls libc makes are answered by src/wasi.rs.
    if let Some(sdk) = &wasi {
        let lib = sdk.join("share/wasi-sysroot/lib").join(WASI_TARGET);
        println!("cargo:rustc-link-search=native={}", lib.join("eh").display());
        println!("cargo:rustc-link-search=native={}", lib.display());
        for l in ["c++", "c++abi", "unwind", "c"] {
            println!("cargo:rustc-link-lib=static={l}");
        }
    }
    Ok(())
}

/// The SHA-256 `SMLIB_PREBUILT` pins for `target`'s prebuilt archive, if there is one.
fn pinned(manifest: &Path, target: &str) -> Result<Option<String>, String> {
    let pins = std::fs::read_to_string(manifest.join("SMLIB_PREBUILT")).map_err(|e| format!("SMLIB_PREBUILT: {e}"))?;
    Ok(pins.lines().filter(|l| !l.starts_with('#')).find_map(|l| {
        let mut f = l.split_whitespace();
        (f.next() == Some(target)).then(|| f.next().map(str::to_string)).flatten()
    }))
}

/// Links the prebuilt C API in `dir` (one archive, `smlib`) and the C++ runtime it was compiled
/// against: the system's, or for wasm wasi-sdk's, which the archive carries.
fn link_prebuilt(dir: &Path, target: &str) -> Result<(), String> {
    let lib = dir.join("lib");
    if !lib.is_dir() {
        return Err(format!("{} has no lib folder: run scripts/fetch-smlib.sh {target} again", dir.display()));
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=smlib");
    if target == "wasm32-unknown-unknown" {
        for l in ["c++", "c++abi", "unwind", "c"] {
            println!("cargo:rustc-link-lib=static={l}");
        }
    } else if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if !target.contains("msvc") {
        println!("cargo:rustc-link-lib=stdc++");
    }
    Ok(())
}

/// The C target SMLib is compiled for inside a `wasm32-unknown-unknown` build: wasi-sdk's, the
/// one with a C++ standard library.
const WASI_TARGET: &str = "wasm32-wasip1";

/// wasi-sdk, from `WASI_SDK_PATH` (github.com/WebAssembly/wasi-sdk, release 34 or later).
fn wasi_sdk() -> Result<PathBuf, String> {
    let sdk =
        std::env::var_os("WASI_SDK_PATH").map(PathBuf::from).ok_or("building SMLib for wasm32-unknown-unknown needs wasi-sdk: set WASI_SDK_PATH")?;
    if !sdk.join("bin/clang++").is_file() {
        return Err(format!("WASI_SDK_PATH={} has no bin/clang++", sdk.display()));
    }
    Ok(sdk)
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
