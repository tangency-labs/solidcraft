# Porting a unit, back-edges and the merge lock

Moved from AGENTS.md (§4, §4a) unchanged. Every SMLib unit now has a Rust counterpart, so this is the
workflow for the porting and wiring work that remains; AGENTS.md §4 has the pull-request rules for
all other work.

## 4. Porting a unit (PR workflow)

Unit list and states: `plan/units/<unit id>.toml`, one file per unit (format:
`plan/modules.schema.md`, order: `plan/order.md`). **The [checklist](https://tangency-labs.github.io/solidcore/PROGRESS.md)** is generated
from `main` by the Pages workflow (`harness/plan/progress.py`; not committed). **Any `todo` unit or unclaimed completion task can be claimed**, after the work §0 lists. A claim reserves
its stated scope, not every future change to the unit. Missing functions, back-edge
wiring, exact runner imports, fixtures and demonstrated bugs remain useful work in
`partial` and `green` units; inspect the source and the goldens, not status alone.
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

**One-time setup** (per machine): clone this repo, and clone upstream at the pinned commit
into `upstream/` (see README): it is the reading reference, and many cases read its
`TestFiles`. Nothing builds it.

1. **Claim = a scoped draft PR.** Search open PR titles and bodies for the unit,
   symbols and affected paths, including `wire:`, `fix:` and `goldens:` work; a title
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
   `harness/callgraph/units.json`) into its `rust_module`, reading upstream's source, and
   extend its runner and fixtures (`crates/goldens/README.md`). Inputs: the unit's
   upstream `tests`, plus fixed-seed generated and edge cases (degenerate, boundary,
   tolerance limits). Expected values come from closed forms, geometric checks and
   invariants, and from upstream's tests where they state them.
3. **Wire back-edges** (section 4a): replace `UNPORTED(<unit id>)` stubs elsewhere with
   real calls and check the goldens of every unit you touched.
4. **Verify** (`export CARGO_BUILD_JOBS=2 RUSTC_WRAPPER=sccache`, everything under
   `nice -n 10`):
   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test -p solidcore
   cargo check -p solidcore --target wasm32-unknown-unknown
   cargo run --profile goldens -p goldens -- --unit <unit id> --goldens   # and every unit you touched
   cargo run --profile goldens -p goldens -- --all --goldens --jobs 4     # what CI runs (with --jobs 0)
   ```
   `--jobs N` runs N units at a time, each in its own process (`0` = every core; the
   output is the same as a serial run's). On a shared machine stay within the section 2
   budget: `--jobs 4` at most, fewer when the load is high. A case your change moves is
   blessed (`--bless`) only with the justification AGENTS.md §1 asks for: the cases, their
   old and new values, why the new ones are right, pictures for geometry.
5. **Before you mark a PR ready**, check each item. Earlier ports passed CI yet still
   departed from upstream's behaviour because of these:
   1. **Default arguments** match upstream (e.g. `IsDegenerate()` defaults to
      `SM_EFF_ZERO`, not a zone tolerance).
   2. **No parameter dropped or hard-coded** (e.g. `0.0` passed instead of the
      caller's value).
   3. **Every upstream special-case branch ported** (e.g. cylinder reparametrisation
      for a line parallel to the axis).
   4. **Statuses upstream relies on are honoured** (e.g. `Unitize()` turning a zero
      vector into `(0,0,1)`).
   5. **The runner builds every geometry kind the code branches on** (real ellipses,
      circles, lines, ..., not B-spline stand-ins), or those branches are never checked.
   6. **Input contracts refuse exactly the inputs that are undefined upstream**: no more
      (valid inputs stay covered), no fewer.
   7. **Generated cases cover** degenerate, boundary, tolerance-limit, huge/tiny-scale
      and sub-domain inputs, and most cases record real values, not just an error
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
   any units you promoted), commit, push, and mark the PR ready. Put the goldens summary lines
   in the PR description. CI runs fmt, clippy, tests, wasm and every unit against its golden.
   **CI is the merge gate.** Once CI is
   green, a maintainer adds the PR to the merge queue after a quick check (stub tags,
   no loosened tolerance, no case dropped), and the queue re-runs CI on the PR merged
   with the latest `main`. Deep adversarial review happens after merge; anything it
   finds comes back as a follow-up issue or PR, so keep the step-5 checklist honest.

**Shared files (avoid merge conflicts).** Every open PR touches the same plan, so:
- Edit only `plan/units/<id>.toml` of your unit (and of units you promote or co-port
  into). There is no shared `plan/modules.toml` any more.
- **Never append to a shared file.** Every finding and every upstream issue is its own
  new file: `plan/findings/<slug>.md` and `plan/upstream-issues/<slug>.md` (formats in
  each directory's `README.md`). Records are never edited after they merge (like ADRs):
  an update is an addendum file. The old shared `plan/findings.md` and
  `plan/upstream-issues.md` were split into those directories; CI fails if either comes
  back. Two PRs that each add a file never conflict.
  Goldens likewise are one file per unit (`crates/goldens/expected/<unit id>.jsonl`), and
  SolidCraft cases one per issue (`fixtures/solidcraft/<issue>-<slug>.json`).
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

For new ports and completion changes, include the [completion evidence checklist](#completion-evidence-checklist)
in the PR description. The evidence table is required for `wire:` PRs that remove
`UNPORTED` stubs, including when existing fixtures do not reach the continuation. Identify the owned functions and branches implemented, which
operations actually complete successfully (with their checked results), and which remain
unreachable.
A passing status or empty result is evidence for that case, not proof that the
operation's successful construction or mutation path works.

A dependency stub replaces the missing call only. Port the upstream logic following
it as well, preserving control flow and operation order; do not omit the continuation
and present the function as complete. If an incremental PR deliberately leaves an
owned body or continuation missing, list the exact symbols and remaining work.
Report that limitation even if existing fixtures never reach it. Unit status follows
the existing gate; it does not certify complete function or branch coverage.

When a dependency is wired, rerun the affected callers and add cases that reach the
previously blocked continuations. Keep newly exposed failures visible. Review
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
- **Cases with back-edges.** A case whose path reaches an unported callee is `deferred`
  (the Rust side returns `Unported`), and its golden records the deferral. A unit with all non-deferred cases passing and >=1 deferred case is
  **`partial`**. With zero deferred cases it is **`green`**. `partial` satisfies
  `depends_on` for scheduling only; it is never reported as green.
- **Wiring back-edges when you port a unit.** After your unit passes, grep
  `UNPORTED(<your unit id>)` across `crates/solidcore`, replace each stub with the real
  call, and bless the goldens of every unit whose deferred cases now run, with the values
  they now produce and why they are right. Any unit whose deferred count drops to 0
  becomes `green`; record this in the same commit.
- **Local swarms only:** when several agents share one clone, everything that writes
  that clone's `main` runs under `flock .git/swarm.lock -c '<commands>'`.

## Completion evidence checklist

This checklist makes implementation gaps and useful follow-up tasks visible during
completion of the port. It complements AGENTS.md and the goldens. It does not add a second
status system or change acceptance tolerances.

### PR evidence

Use a short table for the operations affected by the PR. Group identical paths;
there is no need to inventory unrelated functions on every change.

| Operation / upstream symbols | Implementation | Successful operation checked | Remaining gap |
| --- | --- | --- | --- |
| `<operation and function group>` | Complete body, or exact missing branches | Case IDs and observable results, or none | Dependency, missing body, runner gap, or none |

Include the tested commit, goldens summary lines and affected callers. Distinguish
these forms of evidence:

- A successful operation whose result is checked: observable geometry, topology or state
  changes appropriate to that operation, against closed forms or invariants.
- A legitimate refusal, error or no-op, matching upstream's documented behaviour.
- A path blocked by an unported call or a harness limitation.

All are useful information. Only the first demonstrates the corresponding successful
operation. An operation that legitimately has no successful input needs an explanation,
not an invented success case. Status-only tests remain useful for status contracts;
use an observable result when claiming geometry or mutation coverage.

### Dependency stubs and missing continuations

An `Unported` call should occupy the place of the unavailable upstream call, with
the surrounding algorithm ported faithfully. Do not end a function at the first
missing dependency and silently omit the upstream continuation. A compiling function
with an early return is not proof that its complete body has been implemented.

Small incremental PRs remain welcome. If an owned body, branch or continuation is
still missing, name it in the evidence table and identify an independent follow-up
where possible. Do not infer implementation completeness from a green unit report
or count all of a unit's upstream functions as implemented without checking them.

Once a dependency becomes available:

1. Wire the actual service with the caller's real geometry, topology and context.
2. Exercise the previously blocked continuation and its relevant boundary branches.
3. Rerun affected callers, retaining existing case identities.
4. Diagnose newly reached failures; do not hide them by broadening guards, adding
   deferrals or loosening tolerances.

### Turning harness gaps into contributor tasks

Before debugging or taking a task, search current open PRs for its symbols and paths.

For each available task, provide this information in an issue or related PR body:

```text
Unit / operation / symbols:
Current main SHA and reproducing command / case IDs:
Gap: missing body, missing dependency wiring, runner, or generator
Expected edit boundary and related active PRs:
Evidence needed to finish:
```

Useful independent tasks include constructing real B-reps instead of opaque handles,
replacing stale runner skips with real calls, and generating existing entities so the
intended operation is exercised.
Each task needs a checkable result, not just fewer deferred cases.

Keep legitimate empty-input and refusal tests. Add meaningful successful cases rather
than deleting negative cases to improve a coverage percentage. An input contract must not
be narrowed without evidence for the exact inputs it protects.

For runner changes, show that the runner executes the intended operation. Keep the existing
cases and add operation-specific checks for the new results. A lower deferral count alone
does not establish correctness.

### Reporting scope

The table above is manual PR evidence today. Automated per-operation success-path
reporting would need a separate reviewed implementation defining operation identities,
success semantics and serialization for each runner; this document does not claim
that such a metric already exists.

The numeric gate and existing unit states remain unchanged. Any future change to
completion accounting or treatment of confirmed reference defects must be proposed
and implemented separately with auditable evidence.
