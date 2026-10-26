# Test harnesses

`cargo xtask ci` runs everything a pull request must pass: fmt, `xtask assets`,
`xtask licences --check`, `xtask layers`, clippy with `-D warnings`, the tests with the `ci`
profile and the wasm build check.

| Harness | Where | What it catches |
|---|---|---|
| Unit and engine tests | next to the code, `*_tests.rs` | measured results: volume, area, topology counts against derived values |
| Hostile input | `hostile_params_never_panic`, `crates/engine/src/tests.rs` | every command × every documented parameter × junk values: errors, never panics |
| Sketch fuzz | `crates/engine/src/cmd/sketch_fuzz_tests.rs` | random sketches through the solver and profile finder |
| UI scenarios | `crates/ui-egui/tests/scenarios/*.json` | the real app, headless: clicks, keys, drags, assertions |
| Layout tests | `crates/ui-egui/tests/ui_layout.rs`, `window_widths.rs` | panels and dialogs at every window width |
| Design-file corpus | `apps/solidcraft-cli/tests/corpus` | every saved file version still opens and evaluates the same |
| Kill mid-save | `apps/solidcraft-cli/tests/kill_mid_save.rs` | a save killed at any moment leaves a readable file |
| Reference tests | `cargo xtask oracle` → [docs/oracle.md](oracle.md) | parts rebuilt from `tests/oracle/*/recipe.json`, compared with analytic or FreeCAD expected values |

## Dependency advisories

Scheduled and manually dispatched CI runs have a separate **Dependency advisories** job;
new advisories do not fail unrelated pull requests. To run the same scan locally:

```sh
cargo install cargo-deny --version 0.20.2 --locked
cargo deny --locked check --warn unmaintained advisories bans sources
```

The checked-in `deny.toml` rejects vulnerabilities and unknown registries/git sources.
Unmaintained crates, yanked versions and duplicate versions warn; licences remain covered
by `cargo xtask licences --check`. Review advisory failures and update dependencies rather
than adding broad ignores. The first scan prompted an update to quick-xml 0.41.0 for
[RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194) and
[RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195).

Coverage excludes the version matching of local path patches: `truck-shapeops`,
`truck-stepio`, `truck-meshalgo`, `matext4cgmath` and `egui-wgpu` in `vendor/`. Their checked-in
code and patches still need comparison with their permissively licensed upstreams; a green
scan does not establish their security or incorporate an upstream fix automatically.

## UI scenarios

A scenario is a JSON list of steps run against the real app with a headless egui context:

```json
[
  {"start": "sketch.create"},
  {"click": {"plane": "XY"}},
  {"start": "sketch.rectangle.two_point"},
  {"click": {"sketch": [0, 0]}},
  {"click": {"sketch": [40, 30]}},
  {"expect": {"errors": 0}}
]
```

A new file in `crates/ui-egui/tests/scenarios/` is picked up automatically. To record a bug
before it's fixed, add a `{"pending": "why"}` step; its test is then ignored with that reason, and
the fix removes the step.

## CI profile

CI runs the tests with `--profile ci`: light optimisation, line tables, no LTO and debug
assertions off. Some truck debug assertions fail on valid geometry; the `ci` profile disables
them while building faster than `--release`. If a test fails only in a plain debug build, say
so in the PR.

Run `cargo xtask test` to build with this profile and run the test binaries concurrently,
with doc tests alongside. The binaries run a pool at a time (a quarter of the cores, or what
`SOLIDCRAFT_TEST_JOBS` says), each with a share of the cores as `RUST_TEST_THREADS`, so the
suite keeps the machine busy without oversubscribing it. A `RUST_TEST_THREADS` you set yourself
wins: the pool leaves it alone and the binaries inherit it. Arguments after `--` go to each test
binary, for example `cargo xtask test -- shell`. To run the test binaries sequentially instead,
use `cargo test --workspace --profile ci`.

## Test-level scheduling for full local CI

Install [cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/), then set
`SOLIDCRAFT_TEST_RUNNER=nextest` when running `cargo xtask ci`. For example, on PowerShell:

```powershell
$env:SOLIDCRAFT_TEST_RUNNER = 'nextest'
cargo xtask ci
```

This optional backend builds the workspace tests once, then schedules individual tests using
the `local-ci` nextest profile, which inherits `ci`. The action matrix runs first, then the longest geometry
cases start before the other costly tests, so the ordinary queue does not stall behind an
all-slot reservation. It **ignores the default filter**,
so the heavy tests below still run, does not stop after a test failure, and does not retry failures.
Tests marked `#[ignore]` remain ignored, as with the ordinary full local CI.
The default concurrency is at most four tests; `NEXTEST_TEST_THREADS` overrides it. The action
matrix's child workers default to the smaller of that budget and the host CPU count;
`ACTION_MATRIX_JOBS` explicitly overrides this worker count. The local profile reserves CPU
slots for the matrix and keeps its scheduling changes separate from GitHub CI. On Windows,
the `solidcraft-gpu` and app packages, plus UI offscreen/GPU tests, use a serial group. The Wasm check and doc tests still run alongside,
and the web-offline gate still follows a successful native/Wasm/doc run.

The backend keeps Cargo and binary metadata under `target/xtask-nextest/` and reuses it for the
native run, freeing Cargo to run the side gates. Nextest prints each test's result and duration
as it finishes. `SOLIDCRAFT_TEST_RUNNER=libtest` (or leaving it unset) retains the original runner;
`cargo xtask test` and its Cargo/libtest argument forwarding are unchanged. Unknown runner names
fail explicitly.

## CI sharding and heavy tests

On GitHub, one job builds the tests into a [cargo-nextest](https://nexte.st/) archive and four
jobs run a quarter of the tests each (two halves on a pull request; `--partition slice:N/4`,
settings in `.config/nextest.toml`), the slowest tests first; doc tests run in their own job.
Pull requests skip a few heavy tests (each takes one to twelve minutes); they run on every push to
`main`, nightly, and in `cargo xtask test`:

- `solidcraft-engine`: `tests::hostile_params_never_panic`, everything in `tests::robustness` and
  the seeds of `tests::robustness_goldens` (`seed_00` … `seed_95`; pull requests that change the kernel, truck or
  the goldens run all but the slowest, `SLOW_SEEDS` in `ci.yml`),
  `pattern_suppress_tests::enclosure_arc_extinction_keeps_a_real_sharp_inner_lip_after_resize_and_reopen`,
  `pattern_suppress_tests::enclosure_datums_follow_wall_height_and_board_depth_with_material_proofs`,
  `cmd::plastic::tests::enclosure_snap_fit_follows_resized_front_wall`, the `atomic_commands` contract tests
- `solidcraft-kernel`: `tests::boolean_identity_on_tangent_primitives`,
  `tspline::tests::hostile_tsplines_never_panic`, and the `boolean_props` property tests
  (these run only in the nightly "Property tests" job: their inputs are random, so main skips them too)
- `solidcraft-ui-egui` scenarios: `acceptance_sensor_enclosure`

The **action matrix** (`apps/solidcraft-cli/tests/action_matrix.rs`) tries every ordinary action
(sketch, offset plane, press pull, hole, draft, shell, fillet, chamfer, construction geometry, move,
mirror, pattern) on every face, edge, vertex and body of a corpus of designs and compares with a
checked-in baseline: an action that passed there and fails now fails the test. Every run checks a
fixed sample; the nightly "Action matrix" job runs it all. After a fix, record what passes now with
`cargo xtask action-matrix --bless`. The results and the causes of the failures are in
[the action matrix page](https://github.com/bherbruck/solidcraft/blob/main/docs/action-matrix.md).

To run exactly what a pull request runs: `cargo nextest run --cargo-profile ci --profile ci`;
add `--ignore-default-filter` for the full set. A new test that takes over a minute on CI can
join the list (the `default-filter` in `.config/nextest.toml`) if it guards robustness rather
than everyday behaviour.
