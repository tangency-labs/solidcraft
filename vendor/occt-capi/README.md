# occt-capi: Open CASCADE behind a small C ABI

The native module behind `solidcraft-kernel`'s opt-in `backend-occt` feature, the Open CASCADE
(OCCT) adapter to SolidCraft's kernel interface (`docs/kernel-support.md`). It is an experiment
that shows the interface fits a second kernel. No build we ship, no release, no wasm build and no
CI job turns the feature on, so nothing here is compiled or linked into SolidCraft as shipped.

## Where it comes from

The build files (`build.rs`, `capi/CMakeLists.txt`, `capi/build.cmake`) and the C++ shim
(`capi/src/internal.hpp`, `capi/src/base.cpp`) are from bherbruck/text-to-cad-rs
(`crates/occt-capi-sys`), MIT OR Apache-2.0, by the same author. `capi/src/solidcraft.cpp` and
`src/lib.rs` are written for SolidCraft on the same ABI rules. Changes from upstream: the wasm build
is refused (`build.rs`), and only the two upstream shim files above are taken.

## Building

`cargo build -p solidcraft-kernel --features backend-occt` runs `build.rs`, which either

- copies a prebuilt `libocct_capi` from `OCCT_CAPI_LIB_DIR`, or
- runs `capi/build.cmake`: it downloads OCCT 8.0.1's source (pinned by SHA-256), builds the
  modelling and STEP toolkits as static libraries and links them into one shared library,
  `libocct_capi.so` (`.dylib`, `occt_capi.dll`). It needs CMake 3.21+ and a C++17 compiler and
  takes about 8 minutes the first time. `OCCT_SOURCE_DIR` points it at a source tree already on
  disk; `OCCT_BUILD_JOBS` sets its parallel jobs.

Cargo puts the module on the loader path for `cargo run` and `cargo test`. Anything run outside
Cargo needs `LD_LIBRARY_PATH` (or the platform's equivalent) set to the directory the build
reports as `DEP_OCCT_CAPI_LIB_DIR`.

## Licence

This crate's own files are MIT OR Apache-2.0. OCCT is not: it is LGPL-2.1-only WITH
OCCT-exception-1.0 (both texts are in `licenses/`), downloaded at build time and never stored in
this repository.

- Rust calls OCCT only through the module's plain C ABI and never includes an OCCT header. The
  module holds OCCT and is linked dynamically, so it stays a separate, replaceable library, which
  is what LGPL-2.1 section 6 asks for: a user can rebuild it from modified OCCT sources and swap it
  in without relinking SolidCraft.
- OCCT is never linked statically into a SolidCraft binary, and there is no wasm build of it.
- A build with the feature is a developer measurement build and is not distributed. If one ever
  were, it would ship the module as its own file, with `licenses/`, a notice that it uses Open
  CASCADE Technology, and the OCCT source and these build files for the exact version (or a
  written offer), and would not forbid modifying or reverse engineering the module.

This software makes use of Open CASCADE Technology (https://dev.opencascade.org) when built with
the `backend-occt` feature.
