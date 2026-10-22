# difftest — Rust port vs upstream C++, side by side

`difftest` runs every case of a unit through **upstream SMLib (C++, via
`solidcore-ffi`)** and the **solidcore Rust port** on identical inputs and
compares the results. A unit may be marked `status = "green"` in
`plan/units/<id>.toml` only when `difftest --status-of <id>` prints `green`, and
`status = "partial"` only when it prints `partial` (AGENTS.md section 4a).

## Quick start

```sh
scripts/fetch-cpp.sh                      # once: bin/cpp/lib/libSM_API.a, libSMLib.a (prebuilt; else runs harness/cpp/build.sh)
export CARGO_BUILD_JOBS=2                 # shared machine
export RUSTC_WRAPPER=sccache              # optional: also caches the ~200 shim objects
# --profile difftest: optimised build with overflow checks; same results as debug, ~20x faster.
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --list
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit harness.selftest
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit harness.selftest --perturb   # sensitivity check
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --green          # every status=green|partial unit (~35 min on 16 threads; C++ topology units dominate)
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --green --jobs 4 # same, 4 units at a time (below)
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --status-of nlib.knots   # prints green|partial|fail
nice -n 10 cargo test -p difftest --features cpp                     # harness self-tests
```

| flag | meaning |
|---|---|
| `--unit <id>` | run one unit (repeatable; `*` and `?` match several registered runners, e.g. `--unit 'nlib.*'`) |
| `--jobs N` / `-j N` | run N units at a time, each in its own child process (default `1`: one process, in order; `0`: every core). Output, reports and exit status are the same as with `1` (below) |
| `--green` | all units with `status = "green"` or `"partial"` in `plan/units/*.toml` (one without a runner FAILS; a `green` unit that comes out partial FAILS) |
| `--status-of <id>` | run the unit (no report) and print only `green`, `partial` or `fail`; exit `1` on `fail` (also when no runner is registered) |
| `--all` | every registered runner |
| `--list` | registered runners, their plan status and case counts |
| `--seeds N` | generated cases per unit (default 16) |
| `--filter S` | only cases whose name contains `S` |
| `--perturb` | change one item of each case's Rust output (below); passes only if **every** perturbed case is caught. No report written. |
| `--report-dir D` / `--no-report` | report location (default `reports/difftest/`) / skip writing |
| `--fuzz N` | with `--unit`: probe the unit's FFI guard with N mutated cases, each run in a child process (below). No report written. |
| `--fuzz-out D` | where `--fuzz` writes failing inputs (default `bin/fuzz/`; files go to `D/<unit>/`) |
| `--ssi-corpus [--tol T] [--jobs N] [glob]` | score both engines against the SSI corpus (see [below](#--ssi-corpus-exact-ground-truth-ssi-corpus)); not a gate |
| `--solidcraft-cases [glob]` | score the Rust port on SolidCraft's whole-operation regression cases (see [below](#--solidcraft-cases-solidcraft-regression-cases)); not a gate, no `cpp` feature needed |

Exit status: `0` no failures (deferred cases are not failures), `1` any
failure, `2` usage or setup error. Output: one summary line per unit,

```
PASS|PARTIAL|FAIL <unit> cases=… passed=… failed=… deferred=… max_rel_err=… with_values=… no_values=… [blocked_by=<unit>(<n>),…]
```

then the first few failing cases and `reports/difftest/<unit>.json`.
`with_values` / `no_values` split the passing cases into those that compared
at least one float, integer or topology entry and those that compared nothing
(both sides returned the same error, or empty outputs). A unit whose passes are
mostly `no_values` proves little: make the generator reach the success path.

**Timing** (stderr, one line per unit; diagnostic only, not in the report):

```
TIME <unit> n=<cases> cases=<s> cpp=<s> rust=<s> compare=<s> ratio=<rust/cpp> slowest_rust=<case>:<s>
```

**Output digest** (opt-in): with `DIFFTEST_RUST_DIGEST=<dir>` set, each unit
writes `<dir>/<unit>.digest` with one `<case>\t<fnv1a64>` line per case, the
hash of the Rust output's `Debug` form (f64 `Debug` is the shortest round-trip
form). Equal digest files from two builds mean bit-identical Rust outputs, which
is how a refactor or optimisation proves it changed no value.

**Rust side only / wasm32** (`--digest-dir <dir>`, plus `--digest-debug` for the
full outputs in `<dir>/<unit>.debug`): runs only `run_rust` and writes the same
digest lines, so the Rust kernel can be compared between targets. The binary also
builds for `wasm32-wasip1` with feature `cpp`: the C++ shims are then not linked but
left as imports, which `scripts/wasm_stub.py` serves with stubs returning
`SC_ERR_UNLINKED`; a case whose Rust side reaches C++ (fixture exports) is written as
`needs-cpp`. `scripts/wasm-digest.sh [--jobs N] [selection]` (default `--green`)
builds both targets, runs native and under `wasmtime`, and lists differing cases.

**Profiling without perf/sudo**: `scripts/perf/sampler.py` (ptrace sampler,
frame-pointer unwinding) and `scripts/perf/analyze.py` (self/inclusive tables):

```sh
CARGO_TARGET_DIR=bin/prof CARGO_PROFILE_DIFFTEST_DEBUG=line-tables-only \
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile difftest -p difftest --features cpp
python3 scripts/perf/sampler.py out.json 2 -- bin/prof/difftest/difftest --unit <u> --no-report
python3 scripts/perf/analyze.py out.json --focus solidcore --libc
```

**Perturbation** (`--perturb`): per case, in order of preference: the error
string if the Rust side errored (gets `~perturbed` appended); else the first
nonzero finite float (× `1+1e-8`); else the first integer (`+1`); else the
first topology id (`+1`, or an empty adjacency entry gains a neighbour); else
the first float of an all-zero / non-finite array (`0`→`1e-8`, NaN→`0`,
±inf→∓inf). The line reports `perturbed`, `detected`, `unperturbed` (cases
with an empty output) and `deferred` (never perturbed).

## Parallel runs (`--jobs`)

```sh
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --green --jobs 0   # CI (16 vCPU)
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --green --jobs 4   # shared machine
```

With `--jobs N` and more than one unit (`--green`, `--all`, several or a glob of
`--unit`), every unit runs in its own child process (the same binary run as `--unit
<id>` with the other options; the C++ kernel keeps global state, so units never share a
process or run on threads). At most N children run at once; the queue is ordered longest
first by the `cases_total` of each unit's previous report (in `--report-dir`, else
`reports/difftest/`; a unit with no report is estimated from the size of its
`fixtures/<id>/`, about 512 bytes a case), and the next unit starts as soon as a child exits. The parent
prints each unit's lines in the serial order and the usual summary line, so stdout,
`reports/difftest/*.json` and the exit status are identical to a serial run's. Timing
goes to stderr only: `difftest --jobs N: wall … s, sum of unit times … s, longest unit
<id> … s`.

A child that dies (signal, e.g. a C++ segfault, or no summary line) fails only its own
unit: `FAIL <unit> (worker process killed by signal 11)` followed by the tail of its
stderr; the other units still run and the run exits `1`. Ctrl-C reaches every child
(same process group); a parent that is killed closes the children's stdin, and they exit
too, so none outlive it.

Machine budget (AGENTS.md section 2): `--jobs 0` uses every core; on a shared machine
use `--jobs 4` or less. Memory is per child: most units peak under 0.6 GiB, the heaviest
at 1.5-5.4 GiB (`blend.hcr` 5.4, `topology.edgeuse_p2`/`_p3` 3.0, `topology.loopuse`,
`query.edge`, `query.api_queries` 2-2.4); `--green --jobs 8` peaked at 8.7 GiB in total.

## Fuzzing the FFI guard (`--fuzz`)

```sh
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit foo.bar --fuzz 1000
```

The FFI wrapper is the only thing between bad inputs and undefined behaviour in
upstream, so `--fuzz N` throws N mutated inputs at it. Fuzz case `i` takes one
of the unit's normal cases (fixtures and `--seeds` generated cases, `--filter`
applies) and changes one to three leaves or arrays of its inputs: floats to
`0`, `-0`, `±1`, `±1e-300`, `±1e300` (and `"NaN"`, `"inf"`, `"-inf"` if the
unit's inputs already use those strings), integers to `-1`, `0`, `2^31`,
`2^32`, `i64::MIN`, `i64::MAX` or the length of an input array; arrays are
emptied, truncated or get an element duplicated; flags are flipped. Strings are
left alone. The seed is `fnv1a64("fuzz:" + unit) + i`, so a run is
reproducible and `--fuzz 300` is a prefix of `--fuzz 1000`.

Cases run in batches of 100 in a child process (`difftest --fuzz-exec`, hidden)
with a 4 GiB address-space limit, so a segfault, abort or hang ends the batch
instead of the run; the case that died is re-run alone to confirm it. Each
case gets one verdict:

| verdict | meaning |
|---|---|
| `rejected` | the C++ runner returned a harness error (the guard refused the input) or its fixture constructor rejected the case; Rust is not run on it (fine) |
| `malformed` | the mutated inputs no longer parse into the runner's input type (fine; not a guard probe) |
| `ran` | both sides ran (fine). `differs` counts those where they disagree: a note only (not a failure), since fuzz inputs may be outside the generator's domain; each is still written as a reproduction file, like a failure |
| `crash` | the child died from a signal or an abnormal exit (C++ crash, Rust stack overflow or failed allocation): **FAIL** |
| `panic` | a runner panicked on either side: **FAIL** |
| `timeout` | no result within 20 s, and again when the case runs alone: **FAIL**. A timeout that does not reproduce alone is load-dependent: the case counts with the verdict it gets alone and a `note (timeout)` line records it |

```
FUZZ-PASS|FUZZ-FAIL <unit> cases=… rejected=… malformed=… ran=… differs=… crashes=… panics=… timeouts=…
```

Each failure is printed with its detail (side, signal or panic location) and
its inputs are written as a fixture file, `bin/fuzz/<unit>/fuzzNNNNN.json`
(`{"cases": [...], "fuzz_failure": {...}}`; a crash that only happens after
earlier cases of its batch keeps those cases, in order). Fix the guard (or the
Rust port), then copy the case into `fixtures/<unit>/` as a regression case
(fixture paths from `$upstream_file` appear expanded). Exit status `1` on any
failure. A `differs` case is printed as a `note (differs)` line with its file and
is written the same way (so it can be re-run), but does not fail the run.

## Fuzzing the `.smb` readers (`examples/reader_fuzz.rs`)

A mutation fuzzer for the Rust database readers only (no C++, not part of the
gate). Build and run it with the difftest profile, so arithmetic overflow
panics and is reported. Unix-only (the code is in `examples/reader_fuzz/fuzz.rs`;
on other platforms the example only prints a notice and exits 2):

```sh
X="nice -n 10 cargo run --profile difftest -p difftest --example reader_fuzz --"
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
* Binary and persisted-attribute seeds must be written by upstream
  (`SmBrepData::WriteToFile(SM_BINARY)`, `WriteToDB(.., bPersistAttribs)`).
  The tool that does this is not in the repo; it would go in `harness/cpp/`
  as a small program (or shim functions) linked against the reference build.

## Deferred cases, `partial` units

A unit's Rust code may call into a unit that is not ported yet; it then returns
`SolidError::Unported { upstream, unit }` (AGENTS.md section 4a). In
`run_rust`, map that to `Output::deferred(unit, symbol)`, most simply:

```rust
Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))),
```

A case whose Rust side is deferred is **neither pass nor fail**: nothing is
compared (a harness error on the C++ side still fails it). Unit status:

| status | condition |
|---|---|
| `green` | ≥1 case, every case passes, 0 deferred |
| `partial` | no failing case, ≥1 deferred |
| `fail` | any failing case, harness error, or no cases |

The report records `status`, `cases_passed`/`cases_failed`/`cases_deferred`,
`deferred_by_unit` (`{"<blocking unit>": <cases>}`, sorted), and per case
`outcome` (`pass|fail|deferred`) plus `deferred: {unit, symbol}`. When the
blocking unit lands and its stubs are wired, re-run: the deferred count drops
and the unit can move from `partial` to `green`.

## Rejected native fixtures

When a native fixture constructor rejects geometry before the operation under test
runs, its runner may return `Output::rejected(reason)` from `run_cpp`. The harness
records the inputs and reason with `outcome: "rejected"`, skips the Rust call and
excludes the case from parity passes, failures and sensitivity counts. Reports and
summaries include a rejected count when nonzero. A run containing only rejected
fixtures fails the gate: rejection cannot establish parity. Layout errors, native
exceptions and errors from the operation itself must not be classified this way.

## Pass criteria (fixed; never weaken)

* **Topology / integers / error strings**: identical, same counts and order.
* **Floats**: `a == b`, or `|a-b| <= 1e-9 * max(|a|,|b|)`. Both NaN passes;
  one NaN fails; unequal infinities fail. `0` vs `1e-300` **fails**.
* **Absolute floor** only where the runner documents one via
  `abs_floors()` (key + value + reason, written into the report). Do not add a
  floor to make a case pass; if the C++ and Rust genuinely disagree, record it in
  `plan/findings.md` and leave the unit non-green.
* Key sets and array lengths must match. A case with a harness error (bad
  input, panic) fails. A unit with zero cases fails.

## Adding a runner for unit `foo.bar` (4 new files, no shared edits)

Use `_` for `.` in file names: unit `nlib.knots` → `nlib_knots`.

| file | what |
|---|---|
| `harness/cpp/shim/foo_bar.cpp` | `extern "C"` shims calling upstream |
| `crates/solidcore-ffi/src/units/foo_bar.rs` | `unsafe` extern block + safe wrappers |
| `crates/difftest/src/units/foo_bar.rs` | `pub struct Runner;` implementing `UnitDiff` |
| `fixtures/foo.bar/*.json` | data cases (upstream tests re-expressed as data) |

All four are discovered automatically: `build.rs` globs `shim/*.cpp`,
`solidcore-ffi/src/units/*.rs` (→ `solidcore_ffi::units::foo_bar`) and
`difftest/src/units/*.rs` (→ registry). The reference example for every step
is `harness.selftest` (`harness_selftest.*`).

### 1. C++ shim — `harness/cpp/shim/foo_bar.cpp`

```cpp
// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `foo.bar`. Wraps upstream <Fn> (source/SMLib/src/<File>.cpp).
#include "sc_shim.h"
#include <nurbs.h>            // or SmXxx.h / SmApi*.h — include dirs come from bin/cpp/ffi.env

SC_EXPORT int32_t sc_foo_bar_eval(const double* pts, int64_t n_pts, double t,
                                  double* out, int64_t cap, int64_t* out_len)
{
    if (n_pts < 1) return SC_ERR_ARG;
    SC_TRY
    std::vector<double> res = /* call upstream */;
    return sc_copy_out(res.data(), (int64_t)res.size(), out, cap, out_len);
    SC_CATCH
}
```

Rules: name `sc_<unit_with_underscores>_<fn>`; only plain C data (scalars,
caller-owned flat arrays + lengths); return `int32_t` status (`0` ok, upstream
error flag, or `SC_ERR_*`); never let exceptions or C++-allocated memory cross
the boundary (free upstream objects inside the shim); flatten points as
`[x0,y0,z0,x1,…]`. Shims are compiled with the same `-D`/`-I`/`-f` flags as the
kernel (taken from `bin/cpp/ffi.env`, or `bin/cpp/compile_commands.json` if
it is missing — format in `crates/solidcore-ffi/build.rs`).

`SC_TRY` also acquires the single native-call mutex: upstream's API context,
callbacks and system tolerances are process-global. Handwritten `try/catch`
entrypoints must acquire `SC_NATIVE_LOCK;` before touching native state, retaining
it through native object destruction and any catch-path cleanup. Pure forwarding
entrypoints may rely on the guarded callee. The mutex is recursive for nested
shim calls; it serializes individual exports, not multi-call retained-result
protocols such as SSI's `intersect`/`take` pair.

Topology from C++: walk upstream lists in their native order and emit
**traversal-order indices** (map each pointer to its first-seen index), never
pointers.

### 2. Safe wrapper — `crates/solidcore-ffi/src/units/foo_bar.rs`

```rust
//! FFI wrappers for unit `foo.bar`. Shim: harness/cpp/shim/foo_bar.cpp. Upstream: <file> (<Fn>).
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_foo_bar_eval(pts: *const f64, n_pts: i64, t: f64,
                               out: *mut f64, cap: i64, out_len: *mut i64) -> i32;
    }
}

/// Upstream `<Fn>` on `pts` (flattened xyz) at `t`.
pub fn eval(pts: &[f64], t: f64) -> Result<Vec<f64>, FfiError> {
    if pts.is_empty() || pts.len() % 3 != 0 {
        return Err(FfiError::InvalidArgument("pts".into()));
    }
    call_with_buffer(64, |buf, len| {
        // SAFETY: pts valid for pts.len() reads; buf valid for buf.len() writes; len valid.
        unsafe { sys::sc_foo_bar_eval(pts.as_ptr(), (pts.len() / 3) as i64, t,
                                      buf.as_mut_ptr(), buf.len() as i64, &raw mut *len) }
    })
}
```

Validate **everything** the C++ would trust (lengths, degrees, indices, flags)
before the call: the wrapper is the only thing between fuzzed inputs and UB in
upstream. Fixed-size outputs can skip `call_with_buffer` (see
`harness_selftest.rs`). Use `check(status)` to turn a status into `Result`.

### 3. Runner — `crates/difftest/src/units/foo_bar.rs`

```rust
//! Runner for unit `foo.bar`. C++: solidcore_ffi::units::foo_bar. Rust: solidcore::<path>.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore_ffi::units::foo_bar as ffi;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

pub struct Runner;

#[derive(Serialize, Deserialize)]
struct Inputs { pts: Vec<[f64; 3]>, t: f64 }

impl UnitDiff for Runner {
    fn id(&self) -> &'static str { "foo.bar" }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let flat: Vec<f64> = i.pts.iter().flatten().copied().collect();
        match ffi::eval(&flat, i.t) {
            Ok(v) => Ok(Output::new().values("p", v)),
            Err(solidcore_ffi::FfiError::Status(s)) => Ok(Output::err(format!("status_{s}"))),
            Err(e) => Err(HarnessError::new(e.to_string())),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match solidcore::foo::bar(&i.pts, i.t) {
            Ok(p) => Ok(Output::new().points("p", &[p])),
            Err(_) => Ok(Output::err("status_1")),   // normalise to the same string as C++
        }
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
`Output::err("normalised error")`.

Shared conversions live in `crate::util` (`v3`, `p2`, `flag`): import them
instead of redefining them in the runner.

* `Ok(Output::err(..))` = the *operation* failed (both sides must fail with the
  same string). `Err(HarnessError)` = the *harness* failed (bad inputs) and
  always fails the case.
* Optional overrides: `generate` (seeded `Rng`, `None` = no generated cases),
  `abs_floors` (documented per-key absolute floors), `cases` (add cases built
  in code; start from `crate::runner::default_cases(self, ctx)`).
* Generated case `i` of unit `u` uses seed `fnv1a64(u) + i`, recorded in the
  report as `generated:seed=…` — reproduce with `--seeds` ≥ i+1 and
  `--filter gen00ii`.

### 4. Fixtures — `fixtures/foo.bar/*.json`

```json
{ "cases": [
  { "name": "fit_12_points",
    "upstream_ref": "tests/prog_test/src/smCurveTest.cpp:123",
    "inputs": { "pts": { "$pts": "FitCrvPoints.pts" }, "t": 0.5 } }
] }
```

Files load in name order; names must be unique per unit. Re-express upstream
tests (`upstream/tests/prog_test`, `upstream/tests/SM_API_test`) as data and
cite them in `upstream_ref`. In `inputs`, `{"$pts": "<file>"}` expands to the
`[[x,y,z],…]` of `upstream/TestFiles/pt_TestFiles/<file>`, and
`{"$upstream_file": "<file>"}` to that file's absolute path (for C++-side
readers of `.smb/.smc/.sms/.smp`; the Rust-side readers are ports in their own
units).

### 5. Run, then go green

```sh
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit foo.bar
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --unit foo.bar --perturb   # must say SENSITIVE
cargo clippy -p difftest -p solidcore-ffi --features difftest/cpp --all-targets -- -D warnings
git add harness/cpp/shim/foo_bar.cpp crates/solidcore-ffi/src/units/foo_bar.rs \
        crates/difftest/src/units/foo_bar.rs fixtures/foo.bar plan/units/foo.bar.toml
```

Do not commit `reports/difftest/foo.bar.json` (gitignored; CI regenerates every
green/partial unit's report and publishes main's set). Quote its summary line in the
PR instead. Then set `status` in `plan/units/foo.bar.toml` to what
`--status-of foo.bar` prints (`green` or `partial`; never on `fail`).

## `--ssi-corpus`: exact ground-truth SSI corpus

A correctness report, separate from the gate: it never changes a unit's status, and
CI's gating `difftest` job does not run it. It grades **both** engines' surface/surface
intersection against [jyviko's SSI corpus](https://github.com/jyviko/ssi-corpus) (commit
`b04c8a6`, MIT OR Apache-2.0; issue #17): 119 cases (plane, quadrics, torus, B-spline and
NURBS pairs, 25 of them from Yang, Jia & Yan, ACM TOG 42(6) 211, 2023), each with the dense
exact intersection and the expected topology.

```sh
scripts/fetch-ssi-corpus.sh               # clones the corpus into corpus/ssi/ (gitignored, idempotent, full-SHA pin)
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --ssi-corpus             # all cases, tol 1e-6
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --ssi-corpus 'c3_*'      # case ids matching a glob (`*`, `?`)
nice -n 10 cargo run --profile difftest -p difftest --features cpp -- --ssi-corpus --tol default --jobs 4
```

Without `corpus/ssi/` it prints how to fetch it and exits `0`. The corpus is data only:
the mode reads `cases/data/*.json` and never runs anything from `corpus/`. The fetch script
pins the full commit SHA and checks the checkout's origin and `HEAD` against it. Exit `2` only for harness
errors (unreadable case, report not writable); verdicts never fail the run.

* **Engines.** Each case runs in a child process per engine (`difftest --ssi-case <case>
  <tol> <cpp|rust> <out.json>`), so a C++ segfault or a 300 s timeout is a `CRASH`
  verdict, as with the corpus' runner. `--jobs N` (default 2) children run at once.
  * `cpp`: shim `harness/cpp/shim/ssi_corpus.cpp` mirrors the corpus'
    `runner/ssi_runner.cpp` in its `gsi` mode: real `SmPlane`, `SmCylinder`, `SmCone`
    (`CreateCanonical`), `SmSphere`, `SmTorus`, and `SmBSplineSurface` over an NLib net
    filled directly; `SmApiGetOrCreateContext()`; natural UV domains; edges on;
    `SmApproxTol3d(tol)` (or `NULL` for `default`); `SmSurface::GlobalSurfaceIntersect`.
  * `rust`: the same surfaces with solidcore's types (`Plane`, `Cone` as cylinder and
    `create_canonical` cone, `Sphere`, `Torus`, each with its NURB made as the runner's
    `GetNaturalUVDomain()` does, and `BSplineSurface`) and
    `Surface::global_surface_intersect`. Any `SolidError::Unported` makes the case
    `DEFERRED`, never a faked result.
  * Both sample every 3D and UV curve at 4001 uniform parameters of its natural interval
    and evaluate each surface at its UV samples, and write the runner's JSON
    (`{xyz, uv1, uv2, s1_at_uv, s2_at_uv, type}` per curve) to
    `bin/ssi-corpus/raw/<engine>_<tol>/<id>.json`.
* **Scorer** (`src/ssi/score.rs`, `polyset.rs`, `geom.rs`): a port of the corpus'
  `scoring/score.py` and the distance helpers of `ssi_geom.py` (not the truth
  generators). Accuracy tolerance `10 * tol`. Verdicts, most severe first: `CRASH`,
  `ERROR` (non-success status), `INCOMPLETE` (an expected branch, isolated point or
  region boundary not covered within `cov_tol`), `WRONG TOPOLOGY` (spurious output,
  wrong component count or free ends, duplicated curves, a curve crossing a singular
  point instead of ending there), `INACCURATE` (output farther than the accuracy
  tolerance from an exact surface), `PASS`.
* **Parity.** Where both engines answered, their outputs are compared with the gate's
  rule (status, curve count, types, every sample within `1e-9` relative); mismatches are
  listed per case. A faithful port should also score the same verdicts as the C++.
* **Output.** One line per case (`cpp` verdict, `rust` verdict, parity, the C++ flags),
  then

  ```
  SSI-CORPUS cpp: PASS=… INACCURATE=… WRONG_TOPOLOGY=… INCOMPLETE=… ERROR=… CRASH=… (cases=119)
  SSI-CORPUS rust: PASS=… DEFERRED=… (cases=119)
  SSI-CORPUS parity: compared=… identical=… mismatched=… same_verdict=…
  ```

  and `reports/ssi-corpus.json` (`reports/ssi-corpus_<tol>_<glob>.json` for other
  tolerances or a glob): per case both grades with their flags, distances and topology
  signatures, the deferral reason and the parity result. No timings, so it is
  deterministic.
* **Cross-check with the corpus' own scorer.** `scripts/ssi-corpus-runner.sh` is an
  `SSI_RUNNER` for `score.py` (Python 3, numpy):

  ```sh
  SSI_SC_RAW=$PWD/bin/ssi-corpus/raw/cpp_1e-6 SSI_RUNNER=$PWD/scripts/ssi-corpus-runner.sh \
    SSI_ENGINE=solidcore-cpp python3 -I corpus/ssi/scoring/score.py gsi 1e-6
  ```

  grades the same raw files (without `SSI_SC_RAW` it reruns the shim per case).

## `--solidcraft-cases`: SolidCraft regression cases

A correctness report, separate from the gate like `--ssi-corpus`: it never changes a
unit's status and verdicts never fail the run. `fixtures/solidcraft/` holds one file per
SolidCraft issue that was found and fixed on SolidCraft's earlier kernel (the issue, the
fixing PR and the test the cases come from). Each case is a list of `SM_API`-level steps
and the values its `result` body must have, known in closed form, so the bugs stay
fixed once the operations run end to end here. Geometry and tolerances are those of the
SolidCraft PR's own test wherever it has one.

```sh
nice -n 10 cargo run -p difftest -- --solidcraft-cases                 # all cases (Rust only)
nice -n 10 cargo run -p difftest -- --solidcraft-cases '3526-*'        # ids (<file stem>/<name>) matching a glob
```

* **Steps** (`"op"`): `box`, `sphere`, `cylinder` (`axis`, default +Z), `union`,
  `difference` (optional `pattern`: the tool cut once per grid position, one cut after
  the other), `intersection`, `fillet` (edges by a point on each), `offset_faces` (faces
  by a point on each). Each writes the body `id`; the last writes `result`.
* **Expectations** (`expect`): `lumps` (bounded regions; `0` = nothing left), `volume`
  (`value`, absolute `tol`, the `formula` it came from), `cylinder_faces_max`.
* **Verdicts**: `PASS`, `FAIL`, `UNCHECKED` (no check failed but one cannot be measured
  yet: `cylinder_faces_max` until solidcore has a cylinder surface kind), `ERROR` (a step
  returned an error other than `Unported`), `DEFERRED`. A step that reaches unported code
  defers the case on the unit in its `SolidError::Unported`; `fillet` has no Rust
  `SmApiFilletEdges` yet and defers on `blend.fillet_executive_p1`; `offset_faces` defers
  on `no SM_API entry point`, since `SM_API` only offsets or shells whole bodies
  (`SmApiOffsetBrepFull`, `SmApiShellBrepFull`).
* **Engines.** Rust only for now. A C++ engine (a shim running the same steps through
  `SM_API`) would show which of these bugs upstream SMLib has.
* **Output.** One line per case, then

  ```
  SOLIDCRAFT-CASES rust: PASS=… FAIL=… UNCHECKED=… ERROR=… DEFERRED=… (cases=…)
  SOLIDCRAFT-CASES rust: blocked_by=<unit>(<cases>),…
  ```

  and `reports/solidcraft-cases.json` (`reports/solidcraft-cases_<glob>.json` for a
  glob): per case its source links, verdict, deferral, measured values and checks.
* `cargo test -p difftest --lib solidcraft` checks that every case file loads, every
  case runs to a verdict, and every deferral names a plan unit.

## Notes

* Cases run sequentially in one process: the C++ kernel has global state. A
  C++ crash (segfault) kills the run; use `--filter` to bisect (`--fuzz` runs
  its cases in child processes instead).
* Rust panics in a runner are caught and reported as a failing case.
* `solidcore-ffi` without feature `cpp` is an empty crate that builds for
  `wasm32-unknown-unknown`; the difftest binary refuses to run without `cpp`, except for `--solidcraft-cases` (Rust only).
* Debugging: the binary is `bin/debug/difftest`; set breakpoints with
  mcp-debugger (the C++ kernel is built with `-g`).
* Reports: `reports/difftest/<unit>.json` — not committed (gitignored): CI uploads
  main's set as the `difftest-reports` artifact, `scripts/fetch-reports.sh`
  downloads it. Sorted keys, no timestamps,
  shortest round-trip floats, non-finite floats as `"NaN"`/`"inf"`; failing
  cases include inputs and up to 20 mismatches.
* Reference platform: x86-64 Linux (as CI). Upstream converts out-of-range
  doubles to integers in a few places (UB), and the port reproduces the
  x86-64 result. On arm64 (macOS or Linux) the C++ saturates instead, so
  `geom.hash_table` and `heal.heal_data_p1`-`p3` fail there (see
  `plan/findings.md`, General). Use an x86-64 host for those units.
