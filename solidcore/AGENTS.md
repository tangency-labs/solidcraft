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
crates/goldens         every unit's cases, checked against expected/<unit>.jsonl (the goldens)
plan/                  units/<id>.toml, findings/<slug>.md, upstream-issues/<slug>.md (one file each), STATUS.md, order.md
```

## 0. What to work on: read this before choosing anything

**Where the kernel is.** solidcore replaced SMLib as SolidCraft's kernel in 0.40. The switch came
with bugs: not everything that worked before works now, and users meet those failures every day.
The job now is to make what people actually build work again, correctly. It is not to polish code
paths nobody reaches.

**What to work on: what has been reported.** Take an open issue someone reported (a user, a
maintainer, a failing rig case or sweep), in this order, and claim it before you start:
1. **`urgent` issues.** A red goldens job on main comes first: every pull request's goldens
   job stays red until main is green.
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

**Read first.** Before changing an area, read its reference doc, then its records (the unit's
`plan/units/<id>.toml`, its `plan/findings/` and `plan/upstream-issues/` entries):
- [docs/definitions-and-certificates.md](docs/definitions-and-certificates.md): a surface is its
  definition and its B-spline a certified approximation; the certificate; the exactness classes;
  the exact extension; the invariants every construction keeps. Read this one whatever the area.
- [docs/generic-blend.md](docs/generic-blend.md): fillets, chamfers, corners, setbacks.
- [docs/procedural-sweep.md](docs/procedural-sweep.md): swept surfaces, their frames, threads.
- [docs/ssi.md](docs/ssi.md): surface-surface intersection, and what the boolean takes from it.
- [docs/layers.md](docs/layers.md): which layer each job lives in.
- [docs/validity-and-tolerances.md](docs/validity-and-tolerances.md): what a valid body is, local
  tolerances, healing, and when a tolerance may grow.
- [docs/booleans.md](docs/booleans.md): the boolean's pipeline, classification on S, coincident
  faces and tangent contacts.
- [docs/offset-and-shell.md](docs/offset-and-shell.md): offsets of the definition, folds and the
  largest wall, the offset construction, shell and thicken.
- [docs/coordinating.md](docs/coordinating.md): if you lead other agents: how to understand the
  problem, search for related work, split it into briefs and inspect what comes back.

Each reference ends with "If you're stuck". If the docs and the code don't answer your question,
ask the maintainers on the area's hub issue before you build. Design discussion happens in the
[`#solidcore-dev`](https://discord.com/channels/1560611394934673498/1560631619340214394) channel on Discord.

**Show the problem first, then research it.** Before you build a fix, show the person you work
for what you are solving: a schematic of the problem drawn in VectorCraft (house style:
[docs/agent-style.md](docs/agent-style.md#figures)), or a rendition of the failing case, next to
what the correct result is. For a body in an `.smb` file, `cargo run --release -p diagnostics --
FILE.smb --out DIR` draws it through `solidcore-tess` with its large-tolerance edges over
everything (`--highlight` and `--see-through` take face indices) and writes a JSON report of
every face and edge with its tolerance and label pixel. A pull request that changes a case's
result attaches its pictures before and after the change, drawn the same way from the same view,
whenever the case can be drawn, and a VectorCraft schematic where the problem is a mechanism a
picture of the body cannot show (a tolerance against a gap, an enclosure that does not shrink,
the order of a solve). Then find how the problem is solved in the literature: search for the
papers, read the ones that apply, and design from them. Say in the issue what you read and how
the design follows it. A fix nobody has seen the problem of, or built without that research, is
not ready for review.

**Licence.** Read only `upstream/source/SMLib`, `upstream/source/SM_API`,
`upstream/tests/SM_API_test`, `upstream/tests/prog_test`, `upstream/TestFiles/pt_TestFiles`,
`upstream/AGENTS.md`, `upstream/.agents/` (skip any file about OCCT or USD). Never open,
read or build `OCCT_BREP_*`, `SM_API_USD`, `BREP_SM_USD`, `BREP_USD_DATA`, `HYDRA_USD_BREP`,
`USD_BREP_UTILS`, `schema`, `SmPyLib`, `SmPyDevLib` or `proposals/`. Never read GPL/LGPL code
anywhere (including dependencies; `deny.toml` bans copyleft crates).

**Library code (`crates/solidcore`).**
- No `unsafe` (`#![forbid(unsafe_code)]`, workspace-wide).
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
- No new panic-prone calls the lints miss (integer `/` `%`, `Vec::remove`/`insert`, `chunks`,
  `clamp`, `split_at`, `with_capacity`, ...): return a typed `SolidError` instead, or mark the
  site `proven` with its reason in `plan/panic-sites.tsv` (`scripts/ci/check_panic_sites.py`;
  calls a mechanical rule proves, such as a literal `windows(2)`, `clamp(-1.0, 1.0)` or a
  constant non-zero divisor, pass without a row; a variable divisor needs one).
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
and invariants say so, not because it matches the C++ or a golden. Every fix comes with a
regression test that fails without it.

**The gate.** CI gates on fmt, clippy, the tests, the wasm build and the goldens. A pull
request is green when they pass.

**Goldens.** Every unit's cases (`fixtures/<unit>/`, upstream's `TestFiles`, each runner's
seeded generator) run on solidcore in `crates/goldens`, and each output is checked against the
unit's golden, `crates/goldens/expected/<unit>.jsonl`: what solidcore produced when the golden
was blessed. A case fails when its output changed (integers, topology, status and deferrals
exactly; floats within `1e-9` relative, `|a-b| <= 1e-9 * max(|a|,|b|)`, plus an absolute floor
only where the unit documents one per key in `abs_floors()`), when it is added or missing, and
when it breaks (a harness error or a panic). Check with
`cargo run --profile goldens -p goldens -- --unit <id> --goldens` (`--all --jobs 0` for every
unit). A golden proves a result unchanged, not correct. When your change moves a result on
purpose, bless the unit (`--unit <id> --bless`) and commit the golden with its justification:
in the commit message and the pull request, the cases that changed with their old and new
values, why the new ones are right, and pictures before and after for geometry. A golden
change without that is not reviewed. Never loosen a tolerance or a floor to keep a golden.

**The C++ is a reading reference.** `upstream/` is read for test inputs, expected values and
known constructions (§4b). Nothing in this repository builds, links or runs it.

**Git.** Work happens through pull requests to github.com/tangency-labs/solidcraft (solidcore
lives there at `solidcore/`; github.com/tangency-labs/solidcore is a mirror: no PRs there). Never
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

## 2. Machine budget and code style

[docs/agent-style.md](docs/agent-style.md): the shared machine's budget (`nice -n 10`, at most
`--jobs 4`, `CARGO_BUILD_JOBS=2`), the code style and the file header every source file carries.

## 4. Pull requests

**Claim with a draft PR** before you start: search open PRs (titles and bodies) for the issue,
the symbols and the paths; branch from the latest `main`; open a draft whose body carries:
```text
Units:
In scope: functions and intended behaviour
Expected edits: paths and shared interfaces
Evidence: failing cases, tests, validation planned
Related PRs / dependencies:
Available follow-ups: independent work others can take
Status / blockers / next step:
```
The earliest open claim wins for overlapping work; non-overlapping work is welcome, with the
boundary stated. Never build a competing version of an actively claimed change. Every contributor
here is an agent, so silence means the work has stopped. Push WIP or a progress note at least
every 90 minutes (30 on an `urgent` item). Past that, poke the claim once on the issue; with no
answer in 15 minutes the claim has lapsed, and anyone may take the work by pushing to its branch or
opening a pull request that carries its commits, crediting the author and saying so in one line. The same goes for a ready
pull request waiting only on a rebase, a tidy-up or a review ask: finish it. When you stop, close
the draft and say where the WIP is.

**Verify** (under `nice -n 10`): `cargo fmt --all --check`; `cargo clippy --workspace
--all-targets -- -D warnings`; `cargo test -p solidcore`; `cargo check -p solidcore --target
wasm32-unknown-unknown`; and, for kernel changes, the goldens of every unit you touched
(`cargo run --profile goldens -p goldens -- --unit <id> --goldens`). Every case your change
moves is fixed, or blessed with the justification §1 asks for.

**Finish:** bring the branch up to date with the latest `origin/main` (rebase is preferred; merging `main` in is fine; never force-push over someone else's history), re-verify, put the
goldens summary lines in the description, and mark the PR ready. CI is the merge gate.

**Records are files, never appended.** Every finding and upstream issue is its own new file
(`plan/findings/<slug>.md`, `plan/upstream-issues/<slug>.md`); a merged record is never edited,
an update is an addendum file.

**No new document without the maintainers.** `docs/` holds the reference pages listed in §1 and
what the maintainers have agreed to. Never add a file to `docs/` that hasn't been discussed with
them first, on its issue. A pull request's evidence (validation runs, comparisons, measurements)
goes in its description, or in a `plan/findings/` record when it must outlive the PR. A doc
points to code, tests, records and the literature, never to a pull request or an issue (no
`#NNNN`), so it reads the same whatever happens to the tracker.

**Porting and wiring units** (the work that remains of the port: `UNPORTED` stubs, back-edges,
unit status): [docs/porting.md](docs/porting.md).

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
- **Record upstream defects, don't hide them.** When solidcore is more correct than the C++ for
  a case, record the C++ defect as a confirmed `plan/upstream-issues/` entry, and bless the
  goldens your fix moves with their justification (§1). Never loosen a tolerance.
- **Cases with images.** A pull request that makes a kernel case pass, or turns a known failure
  into a correct refusal, updates the case's entry (its expectation, and its `gallery_note` if the
  gallery needs to say more), its line in `known_failing.json` and its render beside it
  (`crates/solidcore/tests/kernel_cases/<area>/<case>.jpg`) in the same change, never a doc: the
  site's case gallery is generated from them. No status changes without a new render. Choose the
  render's view from the case's geometry so it shows the issue (the corner, the seam, the spot
  refused or fixed), framed close on it, with an inset of the whole part where it helps; never a
  fixed angle.
- **Crash, UB, hang and nondeterminism fixes are unconditional**, e.g. the fix for UB-87.
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
  `crates/goldens/src/plan.rs`).
- `plan/findings/<slug>.md`, one file per finding: upstream bugs, nondeterminism (e.g. upstream `srand(time(NULL))`), anything a later agent must
  know. A reference to `plan/findings.md: <heading>` (older code comments): grep the
  heading under `plan/findings/`.
- `plan/upstream-issues/<slug>.md`, one file per issue: upstream C++ defects (problems
  only, with location, trigger, how found and how it was confirmed). The
  site reads them all. A `plan/upstream-issues.md` ID: grep it under that directory.
- [Checklist](https://tangency-labs.github.io/solidcore/PROGRESS.md): generated on the site
  by `harness/plan/progress.py` (local runs write the gitignored `plan/PROGRESS.md`).
- `plan/STATUS.md`: overall progress summary and handoff notes.
- `bin/`: all build output (cargo `target-dir`). Gitignored. Never
  commit build output or anything under `upstream/`.
