# Machine budget and code style

Moved from AGENTS.md (§2, §3) unchanged; AGENTS.md points here.

## Machine budget

8 cores / 16 threads shared by every agent.

```sh
export CARGO_BUILD_JOBS=2 RUSTC_WRAPPER=sccache      # sccache is installed in ~/.cargo/bin
nice -n 10 cargo test -p solidcore                     # every cargo/C++ command under nice
```

- C++ reference: `scripts/fetch-cpp.sh` downloads the prebuilt libraries for the current
  harness key (no compiling); it falls back to `nice -n 10 harness/cpp/build.sh --jobs 6`
  when no prebuilt matches. Only refetch/rebuild if `bin/cpp/lib` is missing or the
  harness changed; one shared `bin/cpp` serves all worktrees (below). With
  `RUSTC_WRAPPER=sccache` the shims solidcore-ffi compiles are cached by sccache too.
- Run it from the main tree; worktrees reuse that build by exporting
  `SOLIDCORE_CPP_DIR=$HOME/github/solidcore/bin/cpp` (the CMake build dir is keyed by
  source tree, so a worktree run gets its own build dir but installs to the same `lib/`).
- Before claiming a unit, check load: `cut -d' ' -f1 /proc/loadavg`. If > 24, do not
  claim; finish or wait.
- After merging, delete your worktree's `bin/` (each worktree has its own cargo target).
- Unset `RUSTC_WRAPPER` if sccache is missing; the repo never requires it.

## Code style

**Numerics: ported code keeps upstream's, new code chooses for correctness.** Code that is
ported and unchanged keeps upstream numerics: same formulas, same operation order, same
iteration order, same branch structure for tolerance tests and degenerate cases. Do not
"simplify" `a*b + c` into `mul_add`, reorder sums, swap in `hypot`/`midpoint`, or replace
loops with library calls that change rounding there; the C++ reference is built with
`-ffp-contract=off`, so matching order gives matching bits. Code that fixes or replaces a
construction uses the numerics its correctness needs (exact predicates, interval enclosures,
certified rounding bounds) and says so in its doc comment. Everything else is Rust-first:

- **Data model.** Topology lives in `topology::Brep` arenas addressed by typed ids
  (`FaceId`, `EdgeId`, `VertexId`, `LoopId`, `ShellId`, `BrepId`); no pointers, `Rc`,
  or `RefCell` graphs. Geometry is concrete `enum`s (`Curve`, `Surface`) over structs,
  not trait objects. Points/vectors are `math::Vec3` (`Point3` is an alias, as upstream).
- **Kernel.** Tolerances, RNG and caches are fields of `kernel::Kernel` (upstream
  `SmContext`). Take `&Kernel` (or `&mut Kernel` when using rng/caches); it must stay
  `Send`.
- **APIs.** Return values, not out-params (tuples or small structs for multiple
  outputs). Fallible fns return `solidcore::Result<T>`; build `SolidError` with the
  `operation` path (e.g. `"nurbs::knots::insert"`) and entity id. Many optional
  arguments become an options struct with `Default`. Upstream `SmStatus` codes with no
  matching variant map to `SolidError::Upstream { code, .. }`.
- **Names.** Drop `NL_`/`Sm`/`SM_`/`smgu_`/Hungarian prefixes; snake_case fns,
  CamelCase types (`SmBSplineCurve` -> `BSplineCurve`, `NL_KnotInsert` -> `knot_insert`).
- **Organisation.** A unit's `rust_module` (`nurbs::knots`) lives at
  `crates/solidcore/src/nurbs/knots.rs`, declared with `pub mod knots;` in `nurbs.rs`
  (no `mod.rs`). Keep `mod` lines sorted, one per line, so parallel merges conflict
  cleanly. Unit tests go in the same file (`#[cfg(test)] mod tests`).
- **Docs.** Every public item has a doc comment (`missing_docs` warns; CI uses
  `-D warnings`). Lints are in root `Cargo.toml` `[workspace.lints]`; do not add
  `#[allow]` for denied lints in library code.

### File header

Every ported file starts with:

```rust
//! One-line summary of what the module does.
//!
//! Upstream: `source/SMLib/src/NL_KnotVec.cpp`, `source/SMLib/inc/NL_KnotVec.h`.
//!
//! | Rust                    | Upstream                         |
//! |-------------------------|----------------------------------|
//! | [`KnotVector`]          | `NL_KNOTVECTOR`                  |
//! | [`KnotVector::insert`]  | `NL_KnotInsert`                  |
//! | `find_span` (private)   | `nl_FindSpan`                    |
//!
//! Deviations: <anything not 1:1, e.g. out-param -> return value, rand() -> kernel.rng>.
```
