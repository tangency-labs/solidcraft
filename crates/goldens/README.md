# goldens — every unit's cases against their expected outputs

`goldens` runs every case of every unit on solidcore and checks each output against the
unit's **golden**, `expected/<unit>.jsonl`: the output solidcore produced for that case
when the golden was blessed. A unit passes when every case it produces is in its golden,
every case in its golden is produced, no case breaks (a harness error or a panic) and
every output agrees with the golden (below). CI's `goldens` job runs every unit on every
pull request.

A golden proves that a result is unchanged, not that it is correct: correctness is proved
by closed forms, geometric checks and invariants (`docs/definitions-and-certificates.md`).
A pull request that changes a golden says why, with numbers and, for geometry, pictures.

The cases come from upstream SMLib's tests re-expressed as data (`fixtures/<unit>/`),
from upstream's `TestFiles` (read from the `upstream/` checkout, see
`scripts/upstream-pin.env`) and from each runner's seeded generator. The runners were
ported against upstream's C++, which stays the reading reference: it is never built,
linked or run here.

## Quick start

```sh
export CARGO_BUILD_JOBS=2                     # shared machine
G="nice -n 10 cargo run --profile goldens -p goldens --"
$G --list                                     # registered units and their case counts
$G --unit nlib.knots --goldens                # one unit against its golden
$G --unit 'nlib.*' --goldens                  # globs (`*`, `?`) over the registered units
$G --all --goldens --jobs 0                   # every unit, one child process per core (CI)
$G --unit nlib.knots --bless                  # rewrite the unit's golden from this run
nice -n 10 cargo test -p goldens              # the harness's and the runners' own tests
```

`--profile goldens` is an optimised build with overflow checks and debug assertions
(Cargo.toml `[profile.goldens]`); results are the same as a debug build's.

| flag | meaning |
|---|---|
| `--unit <id>` | one unit (repeatable; `*` and `?` match several registered units) |
| `--all` | every registered unit |
| `--goldens` | check each unit against `expected/<unit>.jsonl` |
| `--bless` | rewrite each unit's golden from this run (with `--filter`, only the cases it ran); a unit with a broken case is never blessed |
| `--reverse` | run each unit's cases in reverse order: a result that depends on what ran before shows as a change |
| `--goldens-dir <dir>` | read and write goldens there instead of `expected/` |
| `--filter S` | only cases whose name contains `S` |
| `--seeds N` | generated cases per unit (default 16; the goldens hold 16) |
| `--jobs N` / `-j N` | N units at a time, each in its own child process (default 1; 0 = every core); the output is the same as a serial run's |
| `--report-dir D` / `--no-report` | where `--jobs` keeps each unit's wall time (`times.tsv`, default `reports/goldens/`), which orders the next run's queue / keep none |
| `--digest-dir D` | Rust outputs as digests, no golden ([below](#digests-native-against-wasm32)) |
| `--list` | registered units and their case counts |
| `--ssi-corpus`, `--solidcraft-cases`, `--step-corpus` | correctness reports, not the gate ([below](#--ssi-corpus-exact-ground-truth-ssi-corpus)) |

Output: one line per unit,

```
PASS|FAIL|BLESSED <unit> cases=… changed=… added=… removed=… broken=… known_failing=… rust=<s>
```

then, for a unit that changed, its first ten changed, added, removed and broken cases,
each changed case with its first differences (`field: golden -> now`), and a final
`goldens: N unit(s), P passed, F failed`. Exit status: `0` every unit passed (or was
blessed), `1` any failure, `2` usage or setup error.

## Checking and blessing

* **Agreement.** Integers, topology, the operation's error status, deferrals and
  rejection reasons agree exactly. Each float agrees when `a == b`, both are NaN, or
  `|a-b| <= 1e-9 * max(|a|,|b|)`, plus an absolute floor only where the runner documents
  one for that key in `abs_floors()`. Never loosen either to make a case pass.
* **File format** (`src/golden.rs`): line 1 `{"unit": <id>, "format": 1}`, then one
  JSON object per case, sorted by name. Arrays of up to 16 entries are stored in full;
  longer ones as per-block sums (`[sum, abs_sum, weighted_sum]` per 256 entries), so a
  change in any block, or entries swapped within one, shows.
* **Blessing.** A change that moves a result on purpose blesses the units it moves
  (`--unit <id> --bless`) and commits the golden with its reason: the commit message and
  the pull request list the cases that changed with their old and new values, why the new
  ones are right, and, for geometry, pictures before and after drawn from the same view.
  A golden change without that justification is not reviewed. A bless never hides a
  broken case: a unit with one is refused.
* **Known failures.** A case whose result is wrong is never blessed. It is listed in
  `expected/known_failing.json` (`{"<unit>": {"<case>": "<issue>: <what is wrong>"}}`)
  and has no golden line. It still runs, so a harness error or panic still fails, and
  each run prints it as known failing with its reason. The fix removes the entry and
  blesses the correct result with its pictures. A listed case that has a golden line, or
  that the unit no longer produces, fails.
* **Status is recorded.** `Output::error` is the operation's status (`None` = success).
  A runner must never turn an `Err` into an empty or default output
  (`unwrap_or_default()`, `.ok()`, `Err(_) => vec![]`, ...): map it with
  `Output::from_result` (or `Output::unported(&e).unwrap_or_else(|| Output::err(..))`),
  or, for an op inside a larger record, push the status it maps to before any default.
  Otherwise "computed nothing" and "failed" look the same in the golden.
* **Deferred cases.** A Rust side that reaches code that is not ported returns
  `SolidError::Unported`; the runner maps it to `Output::deferred(unit, symbol)`, and
  the golden records the deferral like any other output.
* **Rejections are recorded.** A case whose fixture construction was refused before the
  operation ran stores its exact `Output::rejected` reason and compares it exactly, so a
  rejection is never an empty success and an empty reason is still a rejection. A
  non-rejected case omits the key, so its line is unchanged.

## Adding a runner for unit `foo.bar`

Use `_` for `.` in file names: unit `nlib.knots` → `nlib_knots`. Two kinds of new files,
no shared edits:

| file | what |
|---|---|
| `crates/goldens/src/units/foo_bar.rs` | `pub struct Runner;` implementing `UnitDiff` (registered by `build.rs`) |
| `fixtures/foo.bar/*.json` | data cases (upstream tests re-expressed as data) |

```rust
//! Runner for unit `foo.bar`. Rust: solidcore::<path>.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

pub struct Runner;

#[derive(Serialize, Deserialize)]
struct Inputs { pts: Vec<[f64; 3]>, t: f64 }

impl UnitDiff for Runner {
    fn id(&self) -> &'static str { "foo.bar" }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        Ok(Output::from_result(
            solidcore::foo::bar(&i.pts, i.t),
            |e| e.to_string(),
            |p| Output::new().points("p", &[p]),
        ))
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let n = 2 + rng.below(8) as usize;
        let pts: Vec<[f64; 3]> = (0..n).map(|_| rng.point3(-10.0, 10.0)).collect();
        serde_json::to_value(Inputs { pts, t: rng.unit() }).ok()
    }
}
```

`Output` builders: `.values(key, Vec<f64>)`, `.scalar(key, f64)`,
`.points(key, &[[f64; N]])`, `.ints(key, Vec<i64>)`, `.int(key, i64)`,
`.topology(Topology::new().count("faces", n).entities("edges", ids).adjacency("face_loops", adj))`,
`Output::err("normalised error")`. Shared conversions live in `crate::util` (`v3`, `p2`,
`flag`).

* `Ok(Output::err(..))` = the *operation* failed. `Err(HarnessError)` = the *harness*
  failed (bad inputs); the case is broken and never blessed.
* Optional overrides: `generate` (seeded `Rng`, `None` = no generated cases),
  `abs_floors` (documented per-key absolute floors), `cases` (add cases built in code;
  start from `crate::runner::default_cases(self, ctx)`).
* Generated case `i` of unit `u` uses seed `fnv1a64(u) + i`; reproduce it with
  `--seeds` ≥ i+1 and `--filter gen00ii`.

Fixtures, `fixtures/foo.bar/*.json`:

```json
{ "cases": [
  { "name": "fit_12_points",
    "upstream_ref": "tests/prog_test/src/smCurveTest.cpp:123",
    "inputs": { "pts": { "$pts": "FitCrvPoints.pts" }, "t": 0.5 } }
] }
```

Files load in name order; names must be unique per unit. Re-express upstream tests
(`upstream/tests/prog_test`, `upstream/tests/SM_API_test`) as data and cite them in
`upstream_ref`. In `inputs`, `{"$pts": "<file>"}` expands to the `[[x,y,z],…]` of
`upstream/TestFiles/pt_TestFiles/<file>`, `{"$upstream_file": "<file>"}` to that file's
absolute path, and `{"$repo_file": "fixtures/<unit>/data/<file>"}` to a file generated
for a case.

Then write the unit's first golden and commit it with the runner:

```sh
nice -n 10 cargo run --profile goldens -p goldens -- --unit foo.bar --bless
nice -n 10 cargo run --profile goldens -p goldens -- --unit foo.bar --goldens --reverse
git add crates/goldens/src/units/foo_bar.rs fixtures/foo.bar crates/goldens/expected/foo.bar.jsonl
```

## Hostile inputs (`tests/fuzz_corpus/`)

`tests/fuzz_corpus/<unit>.json` keeps inputs that once crashed, hung or panicked a
runner, reduced to the smallest form that still reaches the same outcome, with the
outcome the Rust side must still give (`tests/fuzz_corpus.rs` has the format). A panic
fails the test, so a change that reopens an input contract shows.

## Digests: native against wasm32

`--digest-dir <dir>` (plus `--digest-debug` for the full outputs in `<dir>/<unit>.debug`)
writes one `<case>\t<fnv1a64>` line per case, the hash of the output's `Debug` form, to
`<dir>/<unit>.digest`: equal digest files from two builds mean bit-identical outputs.
`scripts/wasm-digest.sh [--jobs N] [selection]` (default `--all`) builds the binary
natively and for `wasm32-wasip1`, runs both (the second under `wasmtime`) and lists the
cases that differ.

**Profiling without perf/sudo**: `scripts/perf/sampler.py` (ptrace sampler,
frame-pointer unwinding) and `scripts/perf/analyze.py` (self/inclusive tables):

```sh
CARGO_TARGET_DIR=bin/prof CARGO_PROFILE_GOLDENS_DEBUG=line-tables-only \
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile goldens -p goldens
python3 scripts/perf/sampler.py out.json 2 -- bin/prof/goldens/goldens --unit <u> --goldens
python3 scripts/perf/analyze.py out.json --focus solidcore --libc
```

## Fuzzing the `.smb` readers (`examples/reader_fuzz.rs`)

A mutation fuzzer for the Rust database readers (not part of the gate). Build
and run it with the goldens profile, so arithmetic overflow panics and is
reported. Unix-only (the code is in `examples/reader_fuzz/fuzz.rs`;
on other platforms the example only prints a notice and exits 2):

```sh
X="nice -n 10 cargo run --profile goldens -p goldens --example reader_fuzz --"
$X sweep --seeds-dir upstream/TestFiles/pt_TestFiles --cases 2000 --jobs 4 \
    --out reader-fuzz-out --tag read [--stage read|topo|full|persist] [--filter <substr>]
$X repro --seed <seed file> --case <i> [--write out.smb]     # rebuild one case
$X run --input f.smb [--binary] [--stage ..] [--stack-kb N]  # run one file in-process
$X minimize --input f.smb [--binary] --expect <verdict prefix> [--output f.min]
$X stats --seeds-dir <dir> --cases N [--stage ..]            # error-site histogram
```

* **Stages.** `read`: `BrepData::read_from_file`. `full`:
  `topology::brep::p7::read_from_file`. `topo`: read, then
  `p10::make_topology_from_data` without analytics, healing or UV checks.
  `persist`: `BrepData::read_from_db` with persisted attributes and a small
  attribute table. `selftest` checks that every bad verdict is detected.
* **Cases.** Case `i` of a seed applies 1 to 5 mutations (bit and byte flips,
  truncation, integer and float token swaps, line and section edits, type-code
  swaps, nesting wrappers, raw 8-byte overwrites for binary files) chosen by
  splitmix64 seeded from the seed's relative path and `i`, so `repro` rebuilds
  any case. A seed under a directory named `corpus-bin` is read as binary.
* **Isolation.** Batches (`--batch`, default 250) run in child processes under
  `ulimit -v` (`--mem-mb`, default 3072), one thread per case with a fixed
  stack (`--stack-kb`, default 8192), with a per-case timeout (`--timeout`,
  default 20 s).
* **Verdicts.** `ok`, `err`, `unported` are fine; `panic`, `oom`,
  `stack-overflow`, `crash` and `timeout` are bad. Each bad case is written to
  `<out>/crashes/` (mutated file plus a `.txt` with seed, case and mutations);
  each sweep writes `<tag>-summary.txt`, `<tag>-findings.tsv` and
  `<tag>-per-seed.tsv`. Exit status `1` on any bad verdict.
* `stats` runs in-process (use it only after a clean sweep) and counts the
  error site each case ended at, as a coverage proxy.
* Binary and persisted-attribute seeds are files upstream wrote
  (`SmBrepData::WriteToFile(SM_BINARY)`, `WriteToDB(.., bPersistAttribs)`); no tool in
  this repository writes them.

## `--ssi-corpus`: exact ground-truth SSI corpus

A correctness report, separate from the gate: it never changes a golden, and CI does not
run it. It grades three surface/surface intersection engines against
[jyviko's SSI corpus](https://github.com/jyviko/ssi-corpus) (commit `b04c8a6`, MIT OR
Apache-2.0): 119 cases (plane, quadrics, torus, B-spline and NURBS pairs, 25 of them from
Yang, Jia & Yan, ACM TOG 42(6) 211, 2023), each with the dense exact intersection and the
expected topology.

```sh
scripts/fetch-ssi-corpus.sh               # clones the corpus into corpus/ssi/ (gitignored, idempotent, full-SHA pin)
nice -n 10 cargo run --profile goldens -p goldens -- --ssi-corpus             # all cases, tol 1e-6
nice -n 10 cargo run --profile goldens -p goldens -- --ssi-corpus 'c3_*'      # case ids matching a glob
nice -n 10 cargo run --profile goldens -p goldens -- --ssi-corpus --tol default --jobs 4
```

Without `corpus/ssi/` it prints how to fetch it and exits `0`. The corpus is data only:
the mode reads `cases/data/*.json` and never runs anything from `corpus/`. Exit `2` only
for harness errors; verdicts never fail the run.

* **Engines.** Each case runs in a child process per engine (`goldens --ssi-case <case>
  <tol> <rust|ssi|solidcore-ssi> <out.json>`), so a crash or a 300 s timeout is a
  `CRASH` verdict, as with the corpus' runner. `--jobs N` (default 2) children run at once.
  * `rust` (`src/ssi/engine.rs`): the case's surfaces with solidcore's types and
    `Surface::global_surface_intersect`, after the input contract ported from the
    reference's corpus wrapper. Any `SolidError::Unported` makes the case `DEFERRED`.
  * `ssi` (`src/ssi/ssi_engine.rs`): the `ssi` crate (`crates/ssi`, a method of its own,
    not a port; see its README), as its own corpus runner runs it.
  * `solidcore-ssi` (`src/ssi/solidcore_ssi_engine.rs`): the `ssi` crate on solidcore's
    own surfaces, into the port's output form.
  * `rust` and `solidcore-ssi` sample every 3D and UV curve at 4001 uniform parameters of
    its natural interval and evaluate each surface at its UV samples, as the corpus'
    `runner/ssi_runner.cpp` does (`src/ssi/kernel.rs`); every engine writes the runner's
    JSON to `bin/ssi-corpus/raw/<engine>_<tol>/<id>.json`.
* **Scorer** (`src/ssi/score.rs`, `polyset.rs`, `geom.rs`): a port of the corpus'
  `scoring/score.py` and the distance helpers of `ssi_geom.py` (not the truth
  generators). Accuracy tolerance `10 * tol`. Verdicts, most severe first: `CRASH`,
  `ERROR` (non-success status), `INCOMPLETE`, `WRONG TOPOLOGY`, `INACCURATE`, `PASS`.
* **Output.** One line per case with each engine's verdict, a `SSI-CORPUS <engine>: …`
  summary line per engine, and `reports/ssi-corpus.json`
  (`reports/ssi-corpus_<tol>_<glob>.json` for other tolerances or a glob). No timings.
* **Cross-check with the corpus' own scorer.** `scripts/ssi-corpus-runner.sh` is an
  `SSI_RUNNER` for `score.py` (Python 3, numpy):

  ```sh
  SSI_SC_RAW=$PWD/bin/ssi-corpus/raw/rust_1e-6 SSI_RUNNER=$PWD/scripts/ssi-corpus-runner.sh \
    SSI_ENGINE=solidcore-rust python3 -I corpus/ssi/scoring/score.py gsi 1e-6
  ```

## `--solidcraft-cases`: SolidCraft regression cases

A correctness report, separate from the gate like `--ssi-corpus`: it never changes a
unit's status and verdicts never fail the run. `fixtures/solidcraft/` holds one file per
SolidCraft issue reproduced on SolidCraft's earlier kernel (the issue, the fixing PR if
any, and the test or issue reproduction the cases come from). Omit `pr` for an unresolved
issue; its report records `pr: null`. Each case is a list of `SM_API`-level steps
and the values its `result` body must have, known in closed form, so the bugs stay
fixed once the operations run end to end here. Geometry and tolerances are those of the
SolidCraft PR's own test wherever it has one.

```sh
nice -n 10 cargo run -p goldens -- --solidcraft-cases                 # all cases (Rust only)
nice -n 10 cargo run -p goldens -- --solidcraft-cases '3526-*'        # ids (<file stem>/<name>) matching a glob
```

* **Steps** (`"op"`): `box`, `sphere`, `cylinder` (`axis`, default +Z), `rotate` (a copy of a
  body turned about `point` + `axis` by `degrees`, `SmApiRotate`), `union`,
  `difference` (optional `pattern`: the tool cut once per grid position, one cut after
  the other), `intersection`, `extrude`, `revolve`, `pipe` (a circle swept along a path),
  `sweep` (a closed `profile` in `plane` at `origin` swept along one `path` arc or line in
  the XY plane, `SmApiCreateCurveSweep`), `loft` (`SmApiCreateSkinPrimitive` through
  `sections`, each one closed `profile` with its `plane` and `origin`; `degree` 1, ruled,
  by default; capped at both ends), `fillet` (edges by a point on each), `chamfer` (the same
  edge picking, `SmApiFilletEdges` with a linear cross section: `distance` is the radius of
  the rolling ball of the constant-radius solver, the leg on each face for a right-angle edge
  between planes (`distance * cot(α/2)` for an interior angle `α`, varying along curved
  faces); one distance for all edges; `SmApiChamferFillet` is not used, it chamfers every
  edge of a body), `offset_faces` (faces by a point on each, moved by the SolidCraft fork's
  `SmOffsetFacesExecutive`, `solidcore::offset::offset_faces`, the tangent blends on them
  following), `sweep_face` (the prism of a body's face: the face through the point `face`,
  the one whose surface holds it and which classifies it inside or on an edge bordering
  no other face, such as its own seam, has its loops' edge curves, trimmed to the edges,
  swept `distance` along `direction` with
  `SmApiCreateLinearSweep` and capped; for a cut by a face's own boundary, #968),
  `sheet_corner` (the deferred sheet-flange recipe below). Each
  writes the body `id`; the last writes `result`.
* **Fixes** (`fixes`, optional): `Kernel::fixes` (`UpstreamFixes`) fields the case turns
  on, by name, for a case that needs an opt-in fix of an upstream defect (for example
  `434-*`: `sweep_frame_on_planar_path_normal`, `cover_refuses_folded_continuation`).
* **Known refusal** (`refused_by`, optional): for a case the kernel refuses on purpose,
  the `issue` that tracks it, the `step` id that fails and text its `error` contains. Only
  that step failing with that error defers the case on the issue; any other error stays
  an `ERROR`, and the deferral keeps the full error text (for example
  `324-*/round_4_over_hole`, tangency-labs/solidcore#422).
  An empty or whitespace-only `error` never matches: the step failure stays an `ERROR`.
* **Expectations** (`expect`): `lumps` (bounded regions; `0` = nothing left; a sealed
  void is a region of its own), `volume` (`value`, absolute `tol`, the `formula` it came
  from), `removed` (the same with the earlier body `from`: its volume minus `result`'s,
  for a result without a closed-form volume of its own), `cylinder_faces_max`,
  `sheet_corner` (signed end distances and flat-pattern invariants), `manifold`
  (`result` must, or must not, be a closed manifold solid, `SmBrep::IsManifoldSolid`, so a
  blend that leaves an open body cannot pass on its volume alone), and `refused`
  (`step`, `error`: the kernel must refuse; the case passes when that step fails with an
  error containing `error`, fails when every step completes, and takes no other
  expectation). `refused` is the opposite of `refused_by`: there the refusal is a known
  defect and defers the case, here it is the right answer (a blend too large for its
  faces, for example `199-*/round_past_the_end_face_is_refused`). `refused.fit_size`
  (`value`, `tol`, `kind` `radius` (default), `scale` or `chord`, `source`) also requires the
  largest size that builds, which a refusal reports with `fillet_gn_section_generator`
  (`SolidError::FilletRefused`, WR-696): a refusal without a size fails it, and a refusal
  with one is still an `ERROR` in a case that expects none.
* **Verdicts**: `PASS`, `FAIL`, `UNCHECKED` (no expectations were supplied, or no check
  failed but one cannot be measured
  yet: `cylinder_faces_max` until solidcore has a cylinder surface kind), `ERROR` (a step
  returned an error other than `Unported`), `DEFERRED`. A step that reaches unported code
  defers the case on the unit in its `SolidError::Unported`; `fillet` has no Rust
  `SmApiFilletEdges` yet and defers on `blend.fillet_executive_p1`; `sheet_corner` defers
  on `no SM_API entry point` (sheet construction, corner joining and unfolding have no
  adapter).
* **Output.** One line per case, then

  ```
  SOLIDCRAFT-CASES rust: PASS=… FAIL=… UNCHECKED=… ERROR=… DEFERRED=… (cases=…)
  SOLIDCRAFT-CASES rust: blocked_by=<unit>(<cases>),…
  ```

  and `reports/solidcraft-cases.json` (`reports/solidcraft-cases_<glob>.json` for a
  glob): per case its source links, verdict, deferral, measured values and checks.
  With no matching cases, the report still contains an empty case list and zero
  verdict counts, replacing any earlier report at that path. `--no-report` writes nothing.
* `cargo test -p goldens --lib solidcraft` checks that every case file loads, every
  case runs to a verdict, and every deferral names a plan unit.
* **Adding cases:** add a new `fixtures/solidcraft/<issue>-<slug>.json`. Files load in
  sorted order and nothing counts them, so no Rust file changes unless the case needs
  a new step kind.

### Sheet flange corner overlap (SolidCraft #3170, PR #3472)

`fixtures/solidcraft/3170-sheet-corner-overlap.json` ports
`overlapping_corners_keep_surface_clearance_and_flat_material` from SolidCraft
PR #3472 (published revision `0420e385`). The 16 original combinations are signed
90/72 degree bends, Partial/Full overlap (1/2 mm), and front/left covering choices.
Each starts with a 120 x 90 mm XY base, thickness 2 mm, radius 1 mm, K factor 0.5,
gap 0.5 mm and four 38.6 mm inside flanges, in front/right/rear/left order. The
corner between flanges 0 and 3 is joined after the initial butt closure. Notch relief
is 2 x 1 mm, with corner relief 8 mm, matching the original aluminium rule.

The minimum signed distance of each end face (at both thickness faces) to its
neighbour's inner plane must be negative overlap for the covering flange and positive
gap for the covered flange, within the original `1e-9` mm absolute tolerance. The
folded solid must be valid, its flat pattern must be one non-overlapping region, its
flat area must change after joining, and
`abs(volume - flat_area * thickness) < volume * 1e-3` must hold. No mesh, saved B-rep
or value measured with another CAD product is used as ground truth.

**Current status: all 16 geometry cases are DEFERRED.** There is no sheet-flange,
corner-join and flat-pattern adapter in this runner. `sheet_corner` records the
original operation rather than substituting already-trimmed boxes, which would not
exercise the bug. It defers on `no SM_API entry point`. Even if a future adapter only
returns volume and lump count, the sheet expectations remain `UNCHECKED`, not `PASS`,
until it also measures and scores every sheet invariant. Passing the harness tests
proves that the cases are preserved and classified correctly, not that solidcore
already implements the sheet feature. Butt/standalone joins, per-corner expressions
and UI behavior are outside this original 16-case port.

```sh
nice -n 10 cargo test -p goldens --lib solidcraft
nice -n 10 cargo run -p goldens -- --solidcraft-cases '3170-*' --no-report
```

This case builds on the whole-operation harness in solidcore PR #345; it does not
change any unit status, kernel implementation or floating-point tolerance.

## `--step-corpus`: real-world STEP files

A correctness report on real CAD data, separate from the gate: verdicts never fail the
run and it is not part of CI. The files are STEP Tools' sample sets
(<https://steptools.com/docs/stpfiles/>, AP203 and AP214), "All Rights Reserved" and
copied only for informational, non-commercial use: they are never committed.

```sh
scripts/fetch-steptools-corpus.sh                                   # corpus/steptools/, SHA-256 pinned
nice -n 10 cargo run --profile goldens -p goldens -- --step-corpus --jobs 4
nice -n 10 cargo run --profile goldens -p goldens -- --step-corpus 'ap203/block*'
nice -n 10 cargo run --profile goldens -p goldens -- --step-corpus --constructor --jobs 4
```

* **Import.** `crates/step-import` (clean room, from the public ISO 10303-21 syntax and
  the Part 41/42 entity definitions; no other STEP implementation consulted) parses
  each file and maps its `MANIFOLD_SOLID_BREP`, `BREP_WITH_VOIDS`, `FACETED_BREP` and
  `SHELL_BASED_SURFACE_MODEL` bodies to a plain-data model in millimetres. A body that
  reaches an unsupported entity is skipped with `unsupported: <ENTITY>`. Assembly
  instancing is counted, not applied: each body is built once in its own coordinates.
* **Build.** Per face: the surface by `CreateCanonical`, the edge loops' 3D curves
  bounded to their edges, then `CreateFacesWithCurves`; then `SmApiStitchIntoSolid`,
  counts (regions, shells, faces, edges, vertices), `SmApiBrepIsManifoldSolid`,
  `SmApiBrepComputeArea` and `SmApiBrepComputeVolume` (accuracy `1e-6`), `SmApiHealBrep`,
  and counts, manifold test and volume again.
* **`--constructor`.** The body is built instead through `SmBrepConstructor` (unit
  `topology.brep_constructor`, `src/constructor.rs`) from the script
  `step_import::script::from_body`; the `construct` stage replaces `faces` and `stitch`.
* **Verdicts** (per body): `BUILT` (every stage ran, whatever its status), `DEFERRED`
  (a stage reached unported code; the unit is shown), `CRASH` (the child crashed, timed
  out or wrote no record), `SKIPPED` (no B-rep body in the file).
* Each body runs in its own child process (`--step-case <file> <body> <out.json>`).
  Output: one line per body, a `STEP-CORPUS` summary line, and
  `bin/step-corpus/report.json` (per body: source counts, stages, verdict).
* `cargo run -p step-import --example survey -- corpus/steptools` lists what the
  importer maps per file; `--example dump_body -- <file> <body> [--flat]` prints one body.

## Notes

* A unit's cases run in order in one process; Rust panics in a runner are caught and the
  case is broken. With `--jobs`, each unit runs in a child process of its own.
* The unit runners' docs say what each case records and which upstream routine it is
  ported from; `upstream/` is their reading reference.
