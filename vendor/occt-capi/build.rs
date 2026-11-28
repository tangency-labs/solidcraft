//! Provides the occt_capi module: a prebuilt one from `OCCT_CAPI_LIB_DIR`, or one
//! built with `capi/build.cmake` (which downloads OCCT's source and builds it into the module).
//! From bherbruck/text-to-cad-rs (crates/occt-capi-sys/build.rs), MIT OR Apache-2.0.

use std::env;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=capi");
    for var in ["OCCT_CAPI_LIB_DIR", "OCCT_SOURCE_DIR", "OCCT_BUILD_JOBS", "CMAKE_TOOLCHAIN_FILE"] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        panic!("occt-capi has no wasm build in SolidCraft: the backend-occt feature is native only");
    }

    let lib_dir = match env::var_os("OCCT_CAPI_LIB_DIR") {
        Some(dir) => stage_prebuilt(PathBuf::from(dir)),
        None => build_module(),
    };
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=occt_capi");
    println!("cargo:lib_dir={}", lib_dir.display());
}

fn build_module() -> PathBuf {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("occt_capi");
    let script = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR")).join("capi").join("build.cmake");

    let mut cmake = Command::new("cmake");
    cmake.arg(format!("-DOUT_DIR={}", out.display()));
    if let Some(dir) = env::var_os("OCCT_SOURCE_DIR") {
        cmake.arg(format!("-DOCCT_SOURCE_DIR={}", PathBuf::from(dir).display()));
    }
    if let Some(file) = env::var_os("CMAKE_TOOLCHAIN_FILE") {
        cmake.arg(format!("-DTOOLCHAIN={}", PathBuf::from(file).display()));
    }
    cmake.arg("-P").arg(&script);
    // Cargo parses a build script's stdout, so CMake's output goes to stderr.
    cmake.stdout(Stdio::from(io::stderr()));

    match cmake.status() {
        Ok(status) if status.success() => out.join("lib"),
        Ok(status) => panic!("{} failed with {status}", script.display()),
        Err(e) if e.kind() == ErrorKind::NotFound => panic!(
            "cmake was not found on PATH. occt-capi-sys needs CMake 3.21+ and a C++17 compiler \
             to build the occt_capi module, or set OCCT_CAPI_LIB_DIR to a prebuilt one"
        ),
        Err(e) => panic!("could not run cmake: {e}"),
    }
}

/// Copies a prebuilt module into OUT_DIR, so `cargo run` and `cargo test` find it the same way
/// they find a module this script built (cargo puts OUT_DIR link paths on the loader path).
fn stage_prebuilt(dir: PathBuf) -> PathBuf {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("prebuilt");
    std::fs::create_dir_all(&out).expect("create OUT_DIR/prebuilt");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "OCCT_CAPI_LIB_DIR {}: {e}\n\n\
             The OCCT module is not in git. Either:\n\
             - put a prebuilt libocct_capi there: build it once with\n\
             \x20   cmake -DOUT_DIR=/tmp/occt -P vendor/occt-capi/capi/build.cmake\n\
             \x20 and copy /tmp/occt/lib/*occt_capi* into it (or set OCCT_CAPI_LIB_DIR to where one is); or\n\
             - unset OCCT_CAPI_LIB_DIR, and this build compiles OCCT itself (CMake 3.21+, a C++17\n\
             \x20 compiler, about 8 minutes).\n\
             See vendor/occt-capi/README.md.",
            dir.display()
        )
    });
    for entry in entries {
        let path = entry.expect("read OCCT_CAPI_LIB_DIR").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        if name.contains("occt_capi") {
            let dest = out.join(&name);
            let stale = std::fs::metadata(&dest).and_then(|d| Ok((d.len(), d.modified()?))).ok()
                != std::fs::metadata(&path).and_then(|s| Ok((s.len(), s.modified()?))).ok();
            if stale {
                std::fs::copy(&path, &dest).expect("copy prebuilt occt_capi");
                if let Ok(t) = std::fs::metadata(&path).and_then(|m| m.modified()) {
                    let _ = std::fs::File::options().write(true).open(&dest).and_then(|f| f.set_modified(t));
                }
            }
        }
    }
    out
}
