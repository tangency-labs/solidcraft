---
name: solidcraft-tests
description: "Use when writing, placing or running tests in SolidCraft: a regression test for a bug, an engine test for a command, a UI scenario, a reference (oracle) test, goldens, or when CI fails a test you did not touch."
---

# Where tests go and how to run them

The long form is `docs/book/src/dev/testing.md`.

## Placement
- A regression test goes into the existing test file for its area (booleans, blends, shell, sweep, STEP, naming, ...), never a new file named after the bug (#3226).
- Engine tests sit next to the code or in `*_tests.rs` files; they measure the result (volume, area, face/edge/vertex counts) against a value you can derive.
- UI scenarios are JSON in `crates/ui-egui/tests/scenarios/*.json`; a new file is picked up automatically. Put `{"pending": "why"}` in a scenario that records a bug before it is fixed, and remove it in the fixing PR.
- Reference tests are `tests/oracle/<case>/recipe.json` with an `expected.json` beside it, run by `cargo xtask oracle`. Expected values are worked out in closed form (`tools/oracle-analytic/`) or computed by FreeCAD run as a separate program (`tools/freecad-oracle/`). A case that fails on a known bug names its issue (`"known_failure": "#123"`). Never change an expected value to match SolidCraft's output, and never take one from a commercial CAD product.
- Crash fixes: a regression test, started from the crash report's `design.solidcraft` and `replay.json` (`skills/solidcraft-crash-safety/SKILL.md`).

## A failing test needs its fix
A failing test without its fix cannot merge: it turns `main` red for everyone. Include the fix in the same PR, or mark the test `#[ignore = "fails until #123 is fixed"]` and remove that line in the PR that fixes it.

## Running
- `cargo xtask test` builds with the `ci` profile and runs every test binary at once; arguments after `--` go to each binary (`cargo xtask test -- shell`). Use the `ci` profile: it builds several times faster than `--release`, and debug assertions are off because truck's reject valid geometry. If a test fails only in a plain debug build, say so in the PR.
- `SOLIDCRAFT_TEST_JOBS=N` bounds concurrent commands on a shared machine.
- `cargo xtask ci` is the gate before a commit: fmt, assets, icons, licences, lists, layers, ui-containers, clippy `-D warnings`, tests and the wasm check.
- Every test runs in its own process: do not rely on state left by another test, or on wall-clock timing.

## Goldens, matrices and skipped tests
- The reference for what the kernel builds is the kernel-cases ladder (`tests/kernel-cases/ladder/`, `cargo xtask kernel-cases --ladder`, `docs/kernel-ladder.md`).
- Kernel PRs run the kernel rig gate (`kernel-rig.yml`): each area against its `KNOWN_FAILING`, the ladder against its golden. In the same PR, a fix removes its line, a new failing case adds one, a changed ladder result is re-recorded (`--ladder --record "why"`); see `tests/kernel-cases/README.md`.
- Maintainers' PRs run the native, GPU path and reference tests on the affected packages; others wait for `approved-for-ci`. `release-gate.yml` runs everything in full before develop replaces main.
- The action matrix is a ratchet: record a newly passing action with `cargo xtask action-matrix --bless` (`docs/action-matrix.md`).
- Pull requests skip a few heavy tests, which run in the merge queue, nightly and on `main`. Check that validation after merging. The list is in `.config/nextest.toml` and the testing page.
- Mutation testing finds behaviour the suites miss: `docs/mutation-testing.md`.
