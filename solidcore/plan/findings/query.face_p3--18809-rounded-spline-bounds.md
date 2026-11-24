# Rounded-spline measurement: bound and seed extrema by cell (2026-11-18)

Issue #18809 reports 60–400 seconds for a cylinder or box boolean immediately after
rounding or chamfering a spline body. The installed-build reproduction locates the
large cost in the final measurement's tight bounds, rather than the boolean itself.
A kernel reproduction isolates the repeated work on the trimmed procedural blend face.

## Inputs and builds

The linked [synthetic stress corpus](https://github.com/dutchman-cad/stress-corpus)
is Apache-2.0. A local copy at `6d90043` supplies `items.jsonl` and its SolidCraft
script generator, `scripts.py`. Only the public synthetic inputs and SolidCraft
generator were used; no other CAD implementation was read or run.

`scripts.script_for(item)` supplies the command script. Each run below stops after
the first command following the initial round, except S0560, which stops after its
first cylinder (its notch precedes the round). This isolates the commands named in
the timing report from the later patterns and transforms.

The installed executable identifies itself as SolidCraft 0.44.1. Its headless MCP
`version` response reports source commit
`e151d5ea6866fb055e011379153fb1cae0566f22`. That commit is unavailable in this
checkout's git objects. These observations therefore do **not** establish the
performance of checkout `27fafb007`, or a speed-up relative to reported baseline
`374359bed`.

Contrary to the issue's description of joins, the generated commands for S0312,
S0560, S0565, S0639 and S0642 have `operation: "cut"`. S0687 has
`operation: "join"`. The historical generated scripts used for the report were
not supplied, so equivalence to those exact scripts is unconfirmed.

## Installed-build observations

Runs used `nice -n 10 timeout 180 solidcraft-cli run SCRIPT --profile
--profile-out TRACE`, with the normal validation setting. S0565's isolated run
used a 300-second cap. These are single observations on a busy shared machine,
not comparative benchmarks. Times below are seconds, rounded to three decimals.
`SmApiBoolean` is the backend span, distinct from the whole feature command and
the enclosing `kernel boolean` span, which can include restoring round definitions.

| Item | Command after round | Command time | Backend `SmApiBoolean` | Result |
|---|---|---:|---:|---|
| S0312 | cylinder cut | 4.289 | 2.419 | exit 0 |
| S0560 | cylinder cut | 3.005 | 1.417 | exit 0 |
| S0565 | box cut | 3.310 | 1.288 | exit 0 |
| S0639 | cylinder cut | 8.745 | 3.001 | exit 1: face test 29, boundary crosses a seam |
| S0642 | cylinder cut | 28.067 | 4.726 | exit 0 |
| S0687 | box join | 24.868 | 3.104 | exit 0 |

S0642 spends 22.162 seconds in `blend_definition::adopt`; S0687 spends 20.575
seconds there. This is a separate measured cost, not evidence of the reported
60–400-second backend boolean. S0639's seam validation failure is also distinct
from that performance report.

The complete S0565 script timed out at 120 seconds (exit 124). Its prefix through
the box cut with `--profile` completed successfully in 8.683 seconds. Its prefix through the extrusion completed
in 0.017 seconds, and through the round in 3.049 seconds in separate runs.

The corpus checker's `hang_step` bisects prefixes with `run --param-check strict`
and no timing option. `apps/solidcraft-cli/src/main.rs`'s normal `run` branch measures the
result to print its summary; `--timings` / `--profile` skip that summary. Therefore
the feature's apparent time in that bisection includes measurement.

S0565's saved notched result, measured by `eval --profile`, completed in 128.077
seconds. The backend boolean took 1.195 seconds, while `inspect.measure` took
122.721 seconds: 64.419 in bounds and 58.224 in mass properties. The estimated
mass-properties implementation itself calculates tight bounds before integration
(`mass/integrator/properties.rs`), so that construction runs twice. Measuring the
same result with a 60-second cap timed out (exit 124).

Measured volume falls from 18.713987076159185 before the cut to
18.672666772528093 after it. These are observations, not derived expectations.
Images of both operands were captured with `solidcraft-cli snapshot` and inspected:
the notch is visible. Review media and generated scripts remain outside the repository.

## Related history and limits

`d37045d26` memoizes unchanged procedural-round certificates on record import
(#17640/#17642), but already belongs to reported baseline `374359bed`.
`c765fd1ba` bounds trim-image certification at surface knot ends (#18724), but
that is a validation construction; no evidence ties it to these boolean costs.
Neither is established as this issue's cause or fix.

A correct performance change needs a failing construction and a work-count
comparison against its unchanged baseline. No tolerance, fallback, golden or
kernel-case expectation was changed.

## Kernel reproduction and construction

The eight S0565 fit points define a chord-parameterized periodic C2 cubic. The
kernel test constructs that spline by solving its cyclic interpolation equations
independently with ideal chord-scaled knots. It also tests the app's exact stored
representation: 25 polynomial control points and 29 knots, captured from the
original extrusion's bottom edge in the installed-build S0565 design. The rounded
body's bottom edge matches that original edge and interval bit-for-bit. This
second input preserves the app's floating-point interpolation, including its
parameterization; no net or definition from another CAD program is used. Both
inputs are extruded by 2, rounded at the top by 0.12, and cut with the reported
0.29 square notch from z = 1 through the top.

On the unchanged checkout, the diagnostic boolean completed; every edge tight
box took less than a millisecond in that run. The body's tight-box search read
735,000 control points. Of these, 729,012 came from one trimmed blend face, with
16,522 definition evaluations and 4,599 station solves for that face alone. This
is the same repeated face-cell search exposed by the installed CLI's bounds span;
its wall time is sensitive to the interpolation's floating-point representation.
No wall-clock threshold belongs in the regression.

The construction is the existing certified enclosure, used before solving a
procedural-blend cell, rather than only at a search limit. Let h be the greatest
control point coordinate of the cell's extracted net, r the extraction rounding
bound from `sub_patch_rounding`, and epsilon the net's same-parameter certificate.
For each of the six unit-axis directions d,

`S(p) dot d <= h + r + epsilon`.

The sums are rounded upward. A cell whose bound is at most the current best can
be set aside before a global solve or point classification. This proof does not
depend on whether the surface is closed. A cell's net must cover the entire cell;
`cell_patch` already rejects snapped-short patches. Every retained cell still
uses the original surface definition and the existing exact solve/subdivision.
The definition map is preserved; there is no net-as-surface substitution.

Retained cells also seed the existing global solver from their extracted cell
net, rather than the whole blend net. `SurfPtGlobalSolver::with_seed_net` changes
only the net read by polygon/Greville guesses, control-net seam-index choices and
grid-refinement neighbourhoods.
The solver's surface remains the original procedural blend: objective values,
derivatives, local solves, closure, stationary-point and solution seam tests are
unchanged.
Its original stand-in surface handle and natural domain are retained too. Thus
the cell net selects starting parameters; it never supplies a model decision as
though it were S. Extraction rounding belongs in the enclosure above, but a seed
needs no tightened certificate. No blend copy, map change or domain clamp is
introduced. The unit test `objective_and_guesses` verifies that changing the
seed's height changes a guess's distance while leaving the objective's distance
to the original surface unchanged.

References are the repository's established constructions in
`docs/definitions-and-certificates.md` sections 4.1, 4.2 and 4.4 (same-parameter
certificate, positive-weight convex hull, and counted extraction rounding), and
the `sub_patch_rounding` derivation in `query/face/p3.rs`. They apply the NURBS
convex-hull property and Higham's gamma bound already used by this query. No new
numerical tolerance is introduced.

The regression compares the resulting box with the extrusion's box: the round
and notch remove material, leave the bottom outline untouched, and leave part of
the top plane at z = 2. Thus all six extents remain, within the body's existing
modeling tolerance. Its work budget follows the existing subpatch regression's
50-whole-net budget and rejects the hundreds of whole nets read before pruning.

## Red check

The focused `a_notched_spline_round_bounds_cells_before_solving_them` regression
failed on unchanged production code (exit 101): 735,000 solve-guess control points
against a budget of 50 x 1,970 = 98,500. Both box-containment assertions passed
before the work-count assertion failed. Definition evaluations were 16,757 and
station solves 4,686 for the body's bounds query. This demonstrates the defect
without a wall-clock assertion.

An intermediate check with certified pre-pruning alone still failed: 494,208
control points, 11,339 definition evaluations and 3,269 station solves. The six
existing tests in the file passed. This isolated the remaining repeated seed
searches; the work assertion and geometric tolerance were kept unchanged.

## Fixed construction

The complete existing integration file passes: 7 passed, 0 failed (5.57 seconds
for both new inputs and the six existing tests). Both spline representations
preserve the expected six body extents and read 15,480 solve-guess control points
against the unchanged budget of 98,500.

| Bounds query | Solve-guess control points | Definition evaluations | Station solves |
|---|---:|---:|---:|
| Independent cyclic solve, unchanged production code | 735,000 | 16,757 | 4,686 |
| Independent cyclic solve, fixed construction | 15,480 | 11,343 | 3,266 |
| App's stored profile, fixed construction | 15,480 | 11,346 | 3,270 |

The cyclic input therefore reads about 47 times fewer control points for solve
guesses. Definition evaluations and station solves are also reduced, by about a
third; the control-point ratio is not a claim about total runtime. The stored
profile has no unchanged-checkout work-count measurement here, and the installed
CLI timing above is not a matched-build speed comparison.

The red and green commands, from the repository root, are:

```text
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml --profile goldens -p solidcore --features profile --test issue_1799_tight_box_divides_at_knots a_notched_spline_round_bounds_cells_before_solving_them -- --nocapture
  unchanged production code: exit 101, the work assertion fails
buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml --profile goldens -p solidcore --features profile --test issue_1799_tight_box_divides_at_knots -- --nocapture
  fixed construction: exit 0, 7 passed
RUST_BACKTRACE=1 buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml --profile goldens -p solidcore --features profile --lib -- query::face::p3::tests surface::surface_point_solve::p1::tests --nocapture
  exit 0, 45 passed; includes a_procedural_blends_hull_is_raised_by_its_nets_bound and objective_and_guesses
RUST_BACKTRACE=1 buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/.local/share/buildq/tmp/lane-18809"' --config env.TMPDIR.force=true clippy --manifest-path solidcore/Cargo.toml --profile goldens -p solidcore --features profile --all-targets -- -D warnings
  exit 0, all targets clean
buildq run solidcraft . -- cargo fmt --all
  exit 0
buildq run solidcraft . -- cargo fmt --all --manifest-path solidcore/Cargo.toml
  exit 0
git --no-optional-locks diff --check
  exit 0
```

`goldens` here names the optimized compiler profile, not a golden-suite run.
No golden or kernel-case expectation was changed or blessed. Three remote checks
failed before compilation with `Disk quota exceeded` (exit 2); successful test
retries ran locally. Normal dispatch was retried after other remote builds
compiled, but clippy hit the quota again. Its final retry uses local dispatch and
disk-backed temporary storage.

The first crate clippy check exited 101 on two new test-helper lints:
`needless_pass_by_value` and `default_trait_access`. The helper now borrows its
curve and names `BlendCounts::default()` explicitly; the construction, assertions
and work budget are unchanged.

A local temporary-directory override initially used an inline-table `--config`
argument, which Cargo rejected before linting (exit 101). Separate
`env.TMPDIR.value` and `env.TMPDIR.force` keys were then validated with an immediate
buildq formatting run (exit 0).

All targeted checks are complete. Workspace-wide tests, goldens, wasm and the
repository audits are left to CI after the owner's push, per this lane's rules.
Changes remain unstaged; no commit or push was made.
