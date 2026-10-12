# difftest — Rust port vs upstream C++, side by side

`difftest` runs every case of a unit through **upstream SMLib (C++, via
`solidcore-ffi`)** and the **solidcore Rust port** on identical inputs and
compares the results. A unit may be marked `status = "green"` in
`plan/modules.toml` only when `difftest --unit <id>` passes.

## Quick start

```sh
harness/cpp/build.sh                      # once: builds bin/cpp/lib/libSM_API.a, libSMLib.a (owner: agent A)
export CARGO_BUILD_JOBS=2                 # shared machine
nice -n 10 cargo run -p difftest --features cpp -- --list
nice -n 10 cargo run -p difftest --features cpp -- --unit harness.selftest
nice -n 10 cargo run -p difftest --features cpp -- --unit harness.selftest --perturb   # sensitivity check
nice -n 10 cargo run -p difftest --features cpp -- --green          # every status=green unit
nice -n 10 cargo test -p difftest --features cpp                     # harness self-tests
```

| flag | meaning |
|---|---|
| `--unit <id>` | run one unit (repeatable) |
| `--green` | all units with `status = "green"` in `plan/modules.toml` (a green unit without a runner FAILS) |
| `--all` | every registered runner |
| `--list` | registered runners, their plan status and case counts |
| `--seeds N` | generated cases per unit (default 16) |
| `--filter S` | only cases whose name contains `S` |
| `--perturb` | multiply one Rust value per case by `1+1e-8`; passes only if **every** perturbed case is caught. No report written. |
| `--report-dir D` / `--no-report` | report location (default `reports/difftest/`) / skip writing |

Exit status: `0` all pass, `1` any failure, `2` usage or setup error.
Output: one summary line per unit (`PASS|FAIL <unit> cases=… failed=… max_rel_err=…`),
the first few failing cases, then `reports/difftest/<unit>.json`.

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
#include <nurbs.h>            // or SmXxx.h / SmApi*.h — include dirs come from bin/cpp/compile_commands.json

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
kernel (taken from `bin/cpp/compile_commands.json`, or `bin/cpp/ffi.env` if
present — format in `crates/solidcore-ffi/build.rs`).

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
nice -n 10 cargo run -p difftest --features cpp -- --unit foo.bar
nice -n 10 cargo run -p difftest --features cpp -- --unit foo.bar --perturb   # must say SENSITIVE
cargo clippy -p difftest -p solidcore-ffi --features difftest/cpp --all-targets -- -D warnings
git add harness/cpp/shim/foo_bar.cpp crates/solidcore-ffi/src/units/foo_bar.rs \
        crates/difftest/src/units/foo_bar.rs fixtures/foo.bar reports/difftest/foo.bar.json
```

Commit the report produced with the default `--seeds` (it is deterministic, so
diffs in it are meaningful). Only then set `status = "green"`.

## Notes

* Cases run sequentially in one process: the C++ kernel has global state. A
  C++ crash (segfault) kills the run; use `--filter` to bisect.
* Rust panics in a runner are caught and reported as a failing case.
* `solidcore-ffi` without feature `cpp` is an empty crate that builds for
  `wasm32-unknown-unknown`; the difftest binary refuses to run without `cpp`.
* Debugging: the binary is `bin/debug/difftest`; set breakpoints with
  mcp-debugger (the C++ kernel is built with `-g`).
* Reports: `reports/difftest/<unit>.json` — sorted keys, no timestamps,
  shortest round-trip floats, non-finite floats as `"NaN"`/`"inf"`; failing
  cases include inputs and up to 20 mismatches.
