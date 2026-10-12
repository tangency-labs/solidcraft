# AGENTS.md — solidcore

solidcore is a port of NVIDIA's SMLib B-rep kernel (C++, Apache-2.0) to idiomatic safe
Rust. Upstream is cloned (gitignored) at `upstream/`, pinned to commit `c9c5979`. Many
agents work here at once: read this whole file before your first change.

```
crates/solidcore       the kernel (pure Rust, no unsafe, wasm-ok)        owner: unit porters
crates/solidcore-ffi   extern "C" bindings to the C++ kernel (feature cpp)
crates/difftest        side-by-side Rust vs C++ runner
harness/cpp            C++ reference build (build.sh -> bin/cpp) + shim/<unit>.cpp
plan/                  modules.toml (units, claims), STATUS.md, findings.md, order.md
```

## 1. Hard rules

**Licence.** Read only `upstream/source/SMLib`, `upstream/source/SM_API`,
`upstream/tests/SM_API_test`, `upstream/tests/prog_test`, `upstream/TestFiles/pt_TestFiles`,
`upstream/AGENTS.md`, `upstream/.agents/` (skip any file about OCCT or USD). Never open,
read or build `OCCT_BREP_*`, `SM_API_USD`, `BREP_SM_USD`, `BREP_USD_DATA`, `HYDRA_USD_BREP`,
`USD_BREP_UTILS`, `schema`, `SmPyLib`, `SmPyDevLib` or `proposals/`. Never read GPL/LGPL code
anywhere (including dependencies; `deny.toml` bans copyleft crates).

**Library code (`crates/solidcore`).**
- No `unsafe` (`#![forbid(unsafe_code)]`). Only `solidcore-ffi` may use it.
- No `unwrap`/`expect`/`panic!`/`todo!`/unchecked `[i]` indexing (denied by lints; tests
  are exempt). Use `.get()`, iterators, `?` and `SolidError`.
- Deterministic: no clock, no hashing of addresses, no `HashMap` iteration order in
  results (use `Vec`, `BTreeMap`, or sort), randomness only from `kernel.rng`.
- No global or `static mut`/`thread_local` state. Everything lives in `Kernel`.
- Must build for `wasm32-unknown-unknown`: no threads, filesystem or `std::time`.

**Correctness gate.** A unit is **green** only when difftest runs the Rust and the C++
(via FFI) on the same inputs and topology is identical and every value agrees within
`1e-9` relative (`|a-b| <= 1e-9 * max(|a|,|b|,1)` unless the unit's harness documents a
stricter form). Never loosen a tolerance to get green. A disagreement you cannot fix
goes in `plan/findings.md` (unit, inputs, both values, suspected cause) and the unit
stays `blocked`.

**Git.** Commit locally only; never push, never rewrite `main`'s history, never
`reset`/`checkout`/`restore` files you did not write. Stage explicit paths only (never
`git add -A`/`git add .`), commit immediately after staging. If `.git/index.lock`
exists, wait a few seconds and retry; never delete it. End every commit message with:

```
Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
```

**Processes.** Kill only processes you started, by PID. Never `pkill`/`pgrep -f`.

## 2. Machine budget

8 cores / 16 threads shared by every agent.

```sh
export CARGO_BUILD_JOBS=2 RUSTC_WRAPPER=sccache      # sccache is installed in ~/.cargo/bin
nice -n 10 cargo test -p solidcore                     # every cargo/C++ command under nice
```

- C++ builds: `nice -n 10 harness/cpp/build.sh --jobs 6`. Only rebuild if `bin/cpp/lib`
  is missing or the harness changed; one shared build serves all worktrees (below).
- Before claiming a unit, check load: `cut -d' ' -f1 /proc/loadavg`. If > 24, do not
  claim; finish or wait.
- After merging, delete your worktree's `bin/` (each worktree has its own cargo target).
- Unset `RUSTC_WRAPPER` if sccache is missing; the repo never requires it.

## 3. Code style

**Port faithfully, write idiomatically.** Keep upstream numerics: same formulas, same
operation order, same iteration order, same branch structure for tolerance tests and
degenerate cases. Do not "simplify" `a*b + c` into `mul_add`, reorder sums, swap in
`hypot`/`midpoint`, or replace loops with library calls that change rounding. The C++
reference is built with `-ffp-contract=off`, so matching order gives matching bits.
Everything else is Rust-first:

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

## 4. Swarm workflow

Unit list and states: `plan/modules.toml` (format: `plan/modules.schema.md`, order:
`plan/order.md`). Pick a `todo` unit whose `depends_on` are all `green`, lowest `layer`
first.

1. **Claim** (in the main tree, on `main`): re-read `plan/modules.toml`; set
   `status = "claimed"`, `claimed_by = "<your agent name>"`, `claimed_at = "<UTC ISO-8601>"`
   for one unit; then `git add plan/modules.toml && git commit -m "claim: <unit id>"`.
   One unit per commit. **First claim in history wins**: check `git log -p -1 --
   plan/modules.toml`; if someone else's claim of that unit landed first, pick another.
2. **Worktree**: `git worktree add ../solidcore-wt/<unit id> -b unit/<unit id> main`, then
   in it `ln -s ../../solidcore/upstream upstream` and
   `export SOLIDCORE_CPP_DIR=$HOME/github/solidcore/bin/cpp` (shared C++ build).
   Work only there.
3. **Port** the upstream files listed for the unit into its `rust_module`, plus the C++
   shim (`harness/cpp/shim/<unit_id_with_underscores>.cpp`, functions `sc_<unit>_<fn>`,
   plain C data only), its `solidcore-ffi` binding, and its difftest cases, following
   the conventions documented in `crates/difftest` and `crates/solidcore-ffi`. Use the
   upstream tests/fixtures listed in the unit's `tests` as difftest inputs, plus edge
   cases (degenerate, boundary, tolerance-limit inputs).
4. **Verify** in the worktree:
   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test -p solidcore
   cargo check -p solidcore --target wasm32-unknown-unknown
   cargo run -p difftest --features cpp -- <unit id>   # flags: ... -- --help
   ```
5. **Merge** when green: in the worktree `git merge main` (resolve; never rebase
   shared history), re-run step 4, then in the main tree
   `git merge --no-ff unit/<unit id> -m "merge: <unit id>"`.
6. **Mark green**: in the main tree set the unit's `status = "green"` (record the
   difftest case count in `notes`), commit `plan/modules.toml` alone. Update
   `plan/STATUS.md` if your unit changes a summary line.
7. **Clean up**: `rm -rf ../solidcore-wt/<unit id>/bin`,
   `git worktree remove ../solidcore-wt/<unit id>`, `git branch -d unit/<unit id>`.

Blocked (dependency missing, upstream behaviour unclear, gate cannot be met): set
`status = "blocked"` with a reason in `notes`, write details to `plan/findings.md`, commit
both, and move on. Never leave a unit `claimed` when you stop working on it.

## 5. Where things go

- `plan/findings.md`: Rust-vs-C++ disagreements, upstream bugs, nondeterminism (e.g.
  upstream `srand(time(NULL))`), anything a later agent must know.
- `plan/STATUS.md`: overall progress summary.
- `bin/`: all build output (cargo `target-dir`, `bin/cpp/` for C++). Gitignored. Never
  commit build output or anything under `upstream/`.
