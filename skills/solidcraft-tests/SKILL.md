---
name: solidcraft-tests
description: "Use when writing, placing or running tests in SolidCraft: a regression test for a bug, an engine test for a command, a UI scenario, a reference (oracle) test, goldens, or when CI fails a test you did not touch."
---

# Where tests go and how to run them

The long form is `docs/book/src/dev/testing.md`.

## Placement
- A regression test goes into its area's existing test file (booleans, blends, shell, STEP, naming, ...), never a new file named after the bug (#3226).
- Engine tests sit next to the code or in `*_tests.rs` files, measured against a value you can derive.
- UI scenarios are JSON in `crates/ui-egui/tests/scenarios/*.json`. Put `{"pending": "why"}` in a scenario that records a bug before it is fixed, and remove it in the fixing PR.
- Reference tests are `tests/oracle/<case>/recipe.json` with an `expected.json` beside it, run by `cargo xtask oracle`, expected values in closed form (`tools/oracle-analytic/`) or from FreeCAD as a separate program (`tools/freecad-oracle/`). A known failure names its issue (`"known_failure": "#123"`). Never change an expected value to match SolidCraft's output, and never take one from a commercial CAD product.
- Crash fixes: a regression test from the report's `design.solidcraft` and `replay.json` (`skills/solidcraft-crash-safety/SKILL.md`).

## Unit test or rig case
- Unit tests assert the code's logic: which edges a chain picks, name resolution, history labels, refusal wording, hostile input, parsing and saving (`a_corner_round_wider_than_its_faces_overflows_and_takes_the_corners_other_edges`: per-edge radii, which no command takes).
- The rig (`tests/kernel-cases/`) asserts geometry: exact volumes, shapes, seam continuity, geometric refusals, through commands with a hand-derived answer, so it survives a kernel swap (`fillets/190-fillet-disc-rim-past-its-2mm-wall`). A test doing both keeps only its logic.

## A failing test needs its fix
A failing test cannot merge without its fix (it turns `main` red): include it, or list the test and cause in `tests/KNOWN_FAILING_TESTS` (`cargo xtask known-failing`); the fix drops the line.

## Running
- `cargo xtask test` builds with the `ci` profile and runs every test binary at once; arguments after `--` go to each binary (`cargo xtask test -- shell`). It builds several times faster than `--release`; a test failing only in a debug build: say so in the PR.
- `SOLIDCRAFT_TEST_JOBS=N` bounds concurrent commands. `cargo xtask ci` is the gate before a commit (fmt, lints, clippy `-D warnings`, tests, wasm).
- Tests run in their own processes: no shared state, no wall-clock timing.

## Goldens, matrices and skipped tests
- The kernel's reference is the ladder (`tests/kernel-cases/ladder/`, `cargo xtask kernel-cases --ladder`, `docs/kernel-ladder.md`).
- Kernel PRs run `kernel-rig.yml` (areas by `PATHS` against `KNOWN_FAILING`, the ladder against its golden). In the same PR a fix drops its line, a new failing case adds one, a changed ladder result is re-recorded (`--record "why"`), a new command gets a case or an `EXEMPT` line (`tests/kernel-cases/README.md`).
- Maintainers' PRs run native, GPU and reference tests on affected packages; others wait for `approved-for-ci`. `release-gate.yml` runs all before develop replaces main.
- The action matrix is a ratchet: bless a newly passing action (`cargo xtask action-matrix --bless`, `docs/action-matrix.md`).
- PRs skip a few heavy tests, run in the merge queue, nightly and on `main`. Check them after merging (`.config/nextest.toml`).
- Mutation testing: `docs/mutation-testing.md`.
