# AGENTS.md — solidcore

solidcore is a B-rep kernel in idiomatic safe Rust. It started as a port of NVIDIA's SMLib (C++,
Apache-2.0) and has detached from it: the aim is the most correct result, by construction, not
SMLib's. SMLib and the SolidCraft fork of it are references to consult (test inputs, expected
values, known constructions), never targets to match.
Upstream is cloned (gitignored) at `upstream/`, pinned to v1.0.1, commit `3fd5b95`
(`scripts/upstream-pin.env` is the single source of truth for scripts, CI and the site). Many
agents work here at once: read this whole file before your first change. What solidcore builds is its own
design wherever that is more correct than upstream's (§4b). People: [CONTRIBUTING.md](CONTRIBUTING.md) has the same principles in short.

```
crates/solidcore       the kernel (pure Rust, no unsafe, wasm-ok)        owner: unit porters
crates/solidcore-ffi   extern "C" bindings to the C++ kernel (feature cpp)
crates/difftest        side-by-side Rust vs C++ runner
harness/cpp            C++ reference build (build.sh -> bin/cpp) + shim/<unit>.cpp
plan/                  units/<id>.toml, findings/<slug>.md, upstream-issues/<slug>.md (one file each), STATUS.md, order.md
```

## 0. What to work on: read this before choosing anything

**Where the kernel is.** solidcore replaced SMLib as SolidCraft's kernel in 0.40. The switch came
with bugs: not everything that worked before works now, and users meet those failures every day.
The job now is to make what people actually build work again, correctly. It is not to polish code
paths nobody reaches.

**What to work on: what has been reported.** Take an open issue someone reported (a user, a
maintainer, a failing rig case or sweep), in this order, and claim it before you start:
1. **`urgent` issues.** Main's red difftest sweep comes first (#2794 lists each red unit and the
   cases): every pull request's difftest stays red until main is green.
2. **`help wanted` issues**, oldest first among equals.
3. **The rig's failures and unported operations** with an issue: the case names its operation
   and the result it expects (`cargo test -p solidcore-ops --test kernel_cases -- --ignored`).

Work already taken is visible: an issue's assignee, its `in progress` label, or an open pull
request that names it. Check all three, and build on the existing work rather than next to it.

**What not to work on.**
- **Defects you think you found** in code no user reaches: an overflow from an impossible input,
  a branch only a fuzzer finds, an "improvement" to a solver or an integrator nobody asked for.
  File an issue with a reproduction from a real use (a rig case, a STEP file, a SolidCraft
  design), and leave it for triage. Don't open a pull request for it unless a maintainer asks.
  Crashes and hangs from real inputs are still taken: they belong to the list above.
- **Speed tricks.** A speed-up is taken only with its root cause, shown as a count (evaluations,
  subdivisions, enclosures), never a timing, on a case that is slow for users or for CI. Making a
  slow path a constant factor faster, or making a slow test cheaper, is not taken.
- **Anything not reported** waits. A pull request names, in its first lines, the issue it fixes;
  one that fixes no reported issue is reviewed after all the ones that do.

## 1. Hard rules

**Read first.** Before changing an area, read its design doc and records: `docs/generic-blend.md`
for fillets and chamfers, the unit's `plan/units/<id>.toml`, and its `plan/findings/` and
`plan/upstream-issues/` entries. If the docs and the code don't answer your question, ask the
maintainers on the area's hub issue before you build.

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
- Must build for `wasm32-unknown-unknown`: no filesystem or `std::time`, and no threads in
  library code for now (§4c).
- Never allocate from an input-declared size unchecked (file headers, counts in data):
  use checked arithmetic and call `DatabaseReader::ensure_available(count, min_bytes,
  op)` before allocating, as `BSplineSurface::read_from_db` does. Upstream `malloc`
  fails softly; Rust aborts.
  Never pre-reserve (`Vec::with_capacity`, `vec![x; n]`) from a count read out of a
  file: an in-memory item is often far larger than its minimum on-disk size. Grow
  the collection as items are actually read.
- Polymorphic readers that can nest (`Curve::read_from_db`, and the surface/topology
  dispatchers when ported) call `db.enter_nested(op)?` before dispatching and
  `db.leave_nested()` after, so crafted files cannot recurse without bound.
- **Recursion.** wasm32 has no stack guard page: a stack overflow there silently
  corrupts memory instead of aborting. Any recursion whose depth depends on input
  (subdivision, refinement, tree/topology walks, nested geometry) carries an explicit
  depth parameter or counter and returns `SolidError` past a documented bound that is
  well above anything upstream fixtures reach. Converting to a loop with an explicit
  stack is allowed only if it visits in exactly the same order. Note each bound in the
  file header's Deviations line. `audit.recursion` (last unit) sweeps the whole crate.

**Correctness is proved directly.** A result is right because closed forms, geometric checks
and invariants say so, not because it matches the C++. Every fix comes with a regression test
that fails without it.

**The gate.** solidcore's CI gates on its own checks: fmt, clippy, its tests and the wasm build.
A pull request is green when they pass. The difftest against the C++ reference is a tool, not
the gate: maintainers run it on a pull request that needs it, from the CI workflow's Run workflow
with `difftest` set (`.github/workflows/ci.yml`), and the reference it builds is set in
`scripts/upstream-pin.env`. Pushes to main still run the sweep and publish main's reports,
which are the exact base a requested run checks against. When it runs, it holds the rules below.

**Difftest.** The difftest against the pinned C++ is a diagnostic for code that is ported and
unchanged, not a goal: where solidcore is more correct and differs, the C++ is wrong for that
case, recorded as below. The rules for a port: a unit is **green** only when difftest runs
the Rust and the C++
(via FFI) on the same inputs and topology is identical and every value agrees within
`1e-9` relative (`|a-b| <= 1e-9 * max(|a|,|b|)`; an absolute floor only where a unit
documents one per key in `abs_floors()`, recorded in its report). Never loosen a tolerance to get green. The harness never feeds C++ outputs or C++ process state into the Rust side (`crates/difftest/README.md`, No C++ state reaches the Rust side). A disagreement you cannot fix
goes in a new `plan/findings/<slug>.md` (unit, inputs, both values, suspected cause) and the unit
stays `blocked`. Sole exception: a case where the C++ reference is verified wrong (a confirmed
`plan/upstream-issues/` entry) is listed with `cpp_defect = "<id>"` in its known-mismatch entry
and is parked: not run in the gate, not counted as a pass, and does not block green; removing
the marker runs it again (`crates/difftest/README.md`); never use it for a Rust divergence.

**Git.** Work happens through pull requests to github.com/tangency-labs/solidcore. Never
push to `main` directly, never force-push shared branches, never rewrite `main`'s
history, never `reset`/`checkout`/`restore` files you did not write. Stage explicit
paths only (never `git add -A`/`git add .`). End every commit message with your
tool's attribution, e.g. for Claude Code:

```
Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
```

**Processes.** Kill only processes you started, by PID. Never `pkill`/`pgrep -f`.

**Deleting.** Delete directories and build output with `cargo xtask rm <path>...` (and
worktrees with `cargo xtask worktree-rm <dir>`), never `rm -rf`: agent harnesses hold `rm -rf`
for a human's approval even with permissions bypassed, which stalls unattended runs. The
xtask only deletes inside the worktree root, `bin/`, `reports/` and the temp dir.

## 2. Machine budget

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

## 3. Code style

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

## 4. Porting a unit (PR workflow)

Unit list and states: `plan/units/<unit id>.toml`, one file per unit (format:
`plan/modules.schema.md`, order: `plan/order.md`). **The [checklist](https://tangency-labs.github.io/solidcore/PROGRESS.md)** is generated
from `main` by the Pages workflow (`harness/plan/progress.py`; not committed). **Any `todo` unit or unclaimed completion task can be claimed**, after the work §0 lists. A claim reserves
its stated scope, not every future change to the unit. Missing functions, back-edge
wiring, exact runner imports, fixtures and demonstrated parity bugs remain useful
work in `partial` and `green` units; inspect source and reports, not status alone.
A unit is *ready* when its `depends_on` are all
`green` or `partial` (`soft_depends_on` never blocks; see section 4a); ready units are
the easiest, lowest `layer` first. Claiming a unit that is not ready is fine:
- Calls into a missing dependency are stubbed (`UNPORTED(<unit>)`, cases deferred; 4a).
- If you need a **type** (struct, enum variant, base class) from a missing dependency,
  port the minimum of it yourself and record it in that unit's `notes` in
  `plan/units/<that unit>.toml` as `CO-PORTED in <your unit> (PR #n): <C++ symbols> -> <Rust path>`
  so its eventual porter extends your code instead of duplicating it.
- If that dependency has an open `port:` PR, do not port the same code twice: build on
  that PR's branch (merge it into yours) or agree on the split in that PR's comments.

**One-time setup** (per machine): clone this repo, clone upstream at the pinned commit
into `upstream/` (see README), then run `scripts/fetch-cpp.sh`, or
`nice -n 10 harness/cpp/build.sh --jobs 6` to build from source.

1. **Claim = a scoped draft PR.** Search open PR titles and bodies for the unit,
   symbols and affected paths, including `wire:`, `fix:` and `difftest:` work; a title
   search for `port:` alone misses completion work. Branch from the latest `main`.
   Do not edit `plan/units/` to claim.
   Initial whole-unit ports keep the title **`port: <unit id>`**. For completion work,
   use a descriptive title such as **`wire: <unit id> — <function group>`** and name
   the unit in the body. An empty claim commit is sufficient to open a draft.

   Include this scope in the PR body (update it as the work changes):
   ```text
   Units:
   In scope: upstream symbols, Rust functions and intended behavior
   Expected edits: paths and shared interfaces
   Evidence: failing/deferred case IDs or missing branches; validation planned
   Related PRs / dependencies:
   Available follow-ups: independent work others can take
   Status / blockers / next step:
   ```
   For overlapping whole-unit `port:` claims, the lowest-numbered open PR wins.
   For completion tasks, the earliest open claim wins only for overlapping work.
   Non-overlapping work in
   the same unit is welcome. Link the related PR and state the boundary before
   starting; independent work need not wait for a whole-unit owner to reply.
   If work changes the same function or shared interface, agree on a split in the
   existing PR first, or build on its branch and document the dependency. If the
   boundary is unclear, ask there and take a different independent task meanwhile.
   Never implement a competing version of an actively claimed change.

   Push WIP or post a concrete progress update at least daily while implementing.
   After 24 h without either, comment with the exact scope you propose to take over
   and allow 12 h for a response; then continue from published WIP, crediting its
   author. Do not take over a PR merely because it awaits review or CI: offer review,
   conflict resolution or a coordinated follow-up instead. A blocker update should
   identify what is blocked and release independent work, rather than reserve the
   whole unit indefinitely. When pausing, release the scope and link the WIP.
   Existing PR authors retain their active scope; this is not a reset of claims.
   If the unit's notes mention a WIP branch, inspect it before starting.

2. **Port** the upstream functions the unit owns (listed in
   `harness/callgraph/units.json`) into its `rust_module`, plus the C++ shim
   (`harness/cpp/shim/<unit_id_with_underscores>.cpp`, functions `sc_<unit>_<fn>`, plain
   C data only), its `solidcore-ffi` binding, and its difftest runner and fixtures,
   following `crates/difftest/README.md`. Inputs: the unit's upstream `tests`, plus
   fixed-seed generated and edge cases (degenerate, boundary, tolerance limits).
3. **Wire back-edges** (section 4a): replace `UNPORTED(<unit id>)` stubs elsewhere with
   real calls and re-run the difftest of every unit you touched.
4. **Verify** (`export CARGO_BUILD_JOBS=2 RUSTC_WRAPPER=sccache`, everything under
   `nice -n 10`):
   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test -p solidcore
   cargo check -p solidcore --target wasm32-unknown-unknown
   cargo run --profile difftest -p difftest --features cpp -- --unit <unit id>   # and every unit you touched
   cargo run --profile difftest -p difftest --features cpp -- --green --jobs 4   # what CI runs (with --jobs 0)
   ```
   `--jobs N` runs N units at a time, each in its own process (`0` = every core; the
   output and reports are the same as a serial run). On a shared machine stay within the
   section 2 budget: `--jobs 4` at most, fewer when the load is high.
   CI also enforces the 75% value-share floor and exact-base artifact ratchet
   (`crates/difftest/README.md`, Regression gate). Missing/expired baseline artifacts
   fail closed; do not extract historical committed reports or waive lost coverage
   to make a run pass. Local `--baseline` runs must use matching seeds and no filter.
5. **Before you mark a PR ready**, check each item. Earlier PRs passed CI yet still
   disagreed with the C++ because of these:
   1. **Default arguments** match upstream (e.g. `IsDegenerate()` defaults to
      `SM_EFF_ZERO`, not a zone tolerance).
   2. **No parameter dropped or hard-coded** (e.g. `0.0` passed instead of the
      caller's value).
   3. **Every upstream special-case branch ported** (e.g. cylinder reparametrisation
      for a line parallel to the axis).
   4. **Statuses upstream relies on are honoured** (e.g. `Unitize()` turning a zero
      vector into `(0,0,1)`).
   5. **The shim builds every geometry kind the code branches on** (real
      `SmEllipse`/`SmCircle`/`SmLine`/..., not B-spline stand-ins), or those branches
      are never compared.
   6. **FFI guards reject exactly the inputs that are undefined or crash in C++**: no
      more (valid inputs stay covered), no fewer (no garbage or segfaults).
   7. **Generated cases cover** degenerate, boundary, tolerance-limit, huge/tiny-scale
      and sub-domain inputs, and most cases compare real values, not just an error
      status.
   8. **No panics on extreme caller input**: bound derivative orders and counts; file
      readers bound header counts; input-driven recursion is bounded (section 1).
   9. **`UNPORTED` stubs only for calls into units that are not `green`/`partial`**,
      tagged with the owning unit from `harness/callgraph/units.json`; co-ports are
      recorded as `CO-PORTED` in that unit's `plan/units/<id>.toml` notes.
6. **Finish the PR**: rebase your branch onto the latest `origin/main` (never merge main
   into it: history stays linear) and force-push your own PR branch with
   `--force-with-lease`; never rewrite a branch someone else builds on, or `main`. Re-verify,
   set the unit's `status` to `"green"` or `"partial"` in
   `plan/units/<unit id>.toml` with case/deferred counts in `notes` (plus the files of
   any units you promoted), commit, push, and mark the PR ready. Put the difftest summary lines in
   the PR description. CI runs fmt, clippy, tests, wasm, difftest on every green or
   partial unit, and the difftest runner tests. **CI is the merge gate.** Once CI is
   green, a maintainer adds the PR to the merge queue after a quick check (stub tags,
   no loosened tolerance, no case dropped), and the queue re-runs CI on the PR merged
   with the latest `main`. Deep adversarial review happens after merge; anything it
   finds comes back as a follow-up issue or PR, so keep the step-5 checklist honest.

**Shared files (avoid merge conflicts).** Every open PR touches the same plan, so:
- Edit only `plan/units/<id>.toml` of your unit (and of units you promote or co-port
  into). There is no shared `plan/modules.toml` any more.
- Never commit `reports/difftest/` (gitignored). difftest still writes your reports
  there locally; CI regenerates them, uploads main's set as the `difftest-reports`
  artifact, and the site and its checklist read that
  (`scripts/fetch-reports.sh` downloads it). Quote your summary lines in the PR instead.
- **Never append to a shared file.** Every finding and every upstream issue is its own
  new file: `plan/findings/<slug>.md` and `plan/upstream-issues/<slug>.md` (formats in
  each directory's `README.md`). Records are never edited after they merge (like ADRs):
  an update is an addendum file. The old shared `plan/findings.md` and
  `plan/upstream-issues.md` were split into those directories; CI fails if either comes
  back. Two PRs that each add a file never conflict.
  Known difftest mismatches likewise live in `crates/difftest/known_mismatches/<unit id>.toml`
  (one file per unit; see `crates/difftest/src/known.rs`), and SolidCraft cases in
  `fixtures/solidcraft/<issue>-<slug>.json` (one file per issue).
  The same goes for anything new you design: a registry or log is a directory of
  per-entry files that tools read in sorted order, never one file everybody appends to.
- **Branches from before the per-unit split** (they edit `plan/modules.toml` or commit
  reports): merge main, then run the migration, which moves your `modules.toml` edits
  into `plan/units/*.toml` and untracks your reports. This one merge is the only exception
  to rebasing (§4 step 6): the script reads the conflicted merge (`MERGE_HEAD`) to find
  your edits. After it, rebase as usual:
  ```sh
  git fetch origin && git merge origin/main; scripts/units-migrate-branch.sh && git commit --no-edit
  ```
  It reports a `CONFLICT` for a field main also changed (it keeps yours; check it) and
  lists any other conflicted files for you to resolve before committing.

**Blocked** (gate cannot be met, upstream behaviour unclear): record details in
`plan/findings/<slug>.md`, set `status = "blocked"` with a reason in `notes`, and open the PR
with just that so others can see it. Never sit on a draft PR you have stopped working
on: close it, and say where the WIP is.

### Completion evidence

For new ports and completion changes, include the [completion evidence checklist](docs/completion-evidence.md)
in the PR description. The evidence table is required for `wire:` PRs that remove
`UNPORTED` stubs, including when existing fixtures do not reach the continuation. Identify the owned functions and branches implemented, which
operations actually complete successfully against C++, and which remain unreachable.
A passing status or empty result is evidence for that case, not proof that the
operation's successful construction or mutation path works.

A dependency stub replaces the missing call only. Port the upstream logic following
it as well, preserving control flow and operation order; do not omit the continuation
and present the function as complete. If an incremental PR deliberately leaves an
owned body or continuation missing, list the exact symbols and remaining work.
Report that limitation even if existing fixtures never reach it. Unit status follows
the existing gate; it does not certify complete function or branch coverage.

When a dependency is wired, rerun the affected callers and add cases that reach the
previously blocked continuations. Keep newly exposed mismatches visible. Review
runner skips, stand-in geometry and absent-entity cases before claiming coverage.
Record independent harness follow-ups with reproducible evidence and check existing
PRs before implementing them. This checklist supplements the existing correctness
gate; it does not change tolerances, outcomes or status accounting.

## 4a. Cycles, back-edges and the merge lock

The kernel core is one big call cycle, so the plan dropped ~2,300 calls as *back-edges*
(listed in each unit's `notes`: `cycle back-edges (...): <unit>(<calls>)`). Rules:

- **Soft dependencies.** `soft_depends_on` lists dependencies a unit calls at most twice
  and needs no type from (generated by `harness/plan/soften.py` from the call graph).
  They do not block claiming: treat every call into a soft dependency that is not
  `green`/`partial` yet exactly like a back-edge below. Only `depends_on` blocks.
- **Calling unported code.** A call into a unit that is not `green`/`partial` yet goes
  through `Err(SolidError::Unported { upstream: "<C++ symbol>", unit: "<unit id>" })`.
  Never panic, never fake a result. Every such site carries the comment
  `// UNPORTED(<unit id>): <C++ symbol>` so the target's porter can find it with grep.
- **Difftest with back-edges.** Cases whose C++ path reaches an unported callee are
  classified `deferred` (the Rust side returns `Unported`); they are neither pass nor
  fail. A unit with all non-deferred cases passing and >=1 deferred case is
  **`partial`**. With zero deferred cases it is **`green`**. `partial` satisfies
  `depends_on` for scheduling only; it is never reported as green.
- **Wiring back-edges when you port a unit.** After your unit passes, grep
  `UNPORTED(<your unit id>)` across `crates/solidcore`, replace each stub with the real
  call, and re-run difftest for every unit you touched. Any unit whose deferred count
  drops to 0 and passes becomes `green`; record this in the same commit.
- **Local swarms only:** when several agents share one clone, everything that writes
  that clone's `main` runs under `flock .git/swarm.lock -c '<commands>'`.

## 4b. Fixing defects and building constructions

Every SMLib unit has a Rust counterpart; the work now is fixing defects and building solidcore's
own constructions. The policy (owner, 2026-11-04; supersedes 2026-11-02 and #1018):

- **The most correct solution, in solidcore.** Don't match SMLib or the fork by default. Use them,
  pinned or fork, for test inputs, expected values and known constructions; where their approach
  is the most correct one, use it, and never copy their bugs. The redesigned constructions (the
  generic blend, `docs/generic-blend.md`) are designed on their solidcore issue and built here.
- **Which baseline a test keeps.** Code ported unchanged from the pinned C++ or the fork keeps their
  results bit for bit. Code written or changed in solidcore sets its own baseline: closed forms,
  geometric checks and invariants grade it, and its measured values are the reference from then on,
  with the fork's numbers at most a cross-check.
- **No compatibility flags.** A fix changes the default result, with a regression test that fails
  without it. No new `Kernel::fixes` (`UpstreamFixes`) field, no old path kept behind a switch,
  no preference for the old behaviour: a saved design rebuilds with today's correct result. Each PR
  says what result changes and shows the evidence (for example: every case that built before is
  unchanged bit for bit, or the cases that change and why the new result is right).
- **Retire the existing flags.** The `UpstreamFixes` fields that exist are left from the port.
  Turning one on by default, deleting its off path and the solidcore-only `WR-` patches made to
  it, is welcome work: a few flags per PR, each with the cases whose results change.
- **Record differences from the C++, don't hide them.** When solidcore is more correct and
  differs, the C++ is wrong for that case: record it as a confirmed `plan/upstream-issues/` entry
  and a `cpp_defect` known-mismatch marker (§1). Never loosen a tolerance.
- **Cases with images.** A pull request that makes a SolidCraft rig case pass, or turns a known
  failure into a correct refusal, updates its row in `docs/generic-blend.md` §7 and its render
  (`docs/generic-blend/cases/<case>.jpg`) in the same change. No row changes without its image.
  Choose the render's view from the case's geometry so it shows the issue (the corner, the seam,
  the spot refused or fixed), framed close on it, with an inset of the whole part where it helps;
  never a fixed angle.
- **Don't extend code that is going away.** Improve the construction that replaces it; an old
  path is deleted when its replacement lands, never kept as a fallback.
- **Crash, UB, hang and nondeterminism fixes are unconditional**, e.g. #1160 for UB-87.
- **Check that the issue is still open work before you claim it.** Many issues were
  fixed by a PR that did not close them (see #1206). Search merged PRs for the issue
  number, the upstream symbol and the `WR-`/`UB-`/`HANG-` record id, and grep
  `plan/upstream-issues/` and the `UpstreamFixes` docs. Constructions SMLib never had
  (the screw sweep, procedural definitions, STEP import) are in scope like any other.
- A fixing PR uses a closing keyword (`Fixes #n`) so the issue closes when it merges.

**Root causes, not point solutions.**
- **A fallback is a code smell.** If a fix ends up as try-then-fallback, a retry on failure, a
  chain of methods tried in turn, or a branch for one case, stop: that patches a symptom.
  Find the construction that is wrong for the case and fix it as a whole.
- **Fillets and chamfers.** These follow the generic rolling-ball blend that is replacing
  SMLib's fillet code step by step (hub: #2243). A failing case means some part of that one
  construction is wrong for it: the spine, the rails, the section, an end, a corner, a fold
  check or the overflow. Never add a special case for it.
- **Literature first.** Before designing a geometry fix, search the literature for the
  problem; most kernel failures are known problems with known constructions. Reading the papers
  is most often the right way to solve a case; code invented to make one case pass is not.
- **More than one issue.** Before fixing, look for the other open issues and failing cases
  with the same cause, and fix the cause for all of them. A fix that closes one case and opens
  another, or a growing set of per-case branches, means the cause is elsewhere.
- **Events, not try/catch trains.** Decide from the geometry which case you are in (a tangent
  end, a fold, a corner type, an empty offset) and build that one construction; never attempt
  one construction, catch its failure and try the next. The generic blend is written this way:
  its cases are events of the trace (`docs/generic-blend.md` §3).
- **No code smells.** No patches, fallbacks, retries, magic tolerances, special-cased inputs,
  dead flags or commented-out code left behind.
- **Ask before you build.** Check open PRs and issues, and the hub issue for the area, for
  contributors working on something similar. Ask there, and build on their work rather than
  next to it. When the docs and the code can't answer a design question, ask the maintainers
  there and wait for the answer rather than guessing.

## 4c. Concurrency and parallelism

- **The concurrency unit is the `Kernel`.** A `Kernel` is `Send` and holds no global state
  (`crates/solidcore/src/kernel.rs`), so independent kernels run on separate threads or workers
  without interfering: one `Kernel` per thread. Upstream SMLib's unit is the process, because
  its default `SmContext` holds unlocked mutable state (the fork's
  `.agents/operations/concurrency.md`); keeping all state in `Kernel` is what removes that limit,
  so never add global, `static` or `thread_local` state.
- **Inside an operation: parallel structure, serial for now.** Chains, spines, stations, spans and
  corners are pure functions of immutable inputs (`docs/generic-blend.md` §5), so they can be spread
  over threads later with identical results. Library code starts no threads today; wasm32 always
  runs the same code serially.
- **Same result, any thread count.** A parallel run must equal the serial one bit for bit: no
  shared mutable state, no order-dependent reductions, no lazily filled caches without
  synchronisation. State that latches on first use (ports of C++ function statics kept in
  `Kernel`) makes a result depend on earlier calls; it goes when its construction is replaced, and
  nothing new may be added like it.

## 5. Where things go

- **Which layer a job belongs to:** [docs/layers.md](docs/layers.md). Each job lives in one layer: kernel, record, ops, tess, or the applications. The kernel computes; ops decides and sequences, and never computes a geometric quantity itself. Two implementations of one job are a bug.
- `crates/solidcore/tests/kernel_cases/<area>/<case>.json`: a kernel-operation case (an entry, its operand records and exact expected values), one file per case; new ones come here, not to SolidCraft's rig (its README, "Adding a case").
- `plan/units/<id>.toml`: one unit's plan entry (status, claims, notes). Edit only
  your unit's file. Every tool reads them through `harness/plan/units.py` (Rust:
  `crates/difftest/src/plan.rs`).
- `plan/findings/<slug>.md`, one file per finding: Rust-vs-C++ disagreements, upstream
  bugs, nondeterminism (e.g. upstream `srand(time(NULL))`), anything a later agent must
  know. A reference to `plan/findings.md: <heading>` (older code comments): grep the
  heading under `plan/findings/`.
- `plan/upstream-issues/<slug>.md`, one file per issue: upstream C++ defects (problems
  only, with location, trigger, how found and whether the C++ build confirmed it). The
  site reads them all. A `plan/upstream-issues.md` ID: grep it under that directory.
- `reports/difftest/<unit>.json`: difftest output. Not committed: CI uploads main's set
  as an artifact; `scripts/fetch-reports.sh` downloads it.
- [Checklist](https://tangency-labs.github.io/solidcore/PROGRESS.md): generated on the site
  by `harness/plan/progress.py` (local runs write the gitignored `plan/PROGRESS.md`).
- `plan/STATUS.md`: overall progress summary and handoff notes.
- `bin/`: all build output (cargo `target-dir`, `bin/cpp/` for C++). Gitignored. Never
  commit build output or anything under `upstream/`.
