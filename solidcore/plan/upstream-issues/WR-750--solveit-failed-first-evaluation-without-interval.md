#### WR-750. `SmLocalSolve1d::SolveIt` without an interval iterates from a failed first evaluation
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmLocalSolve1d.cpp:799-846`, `SmLocalSolve1d::SolveIt`: the first `Evaluate` writes directly into `m_vCurr` (`:799`); its failure branch (`:806`) handles only `m_bUseInterval` (`:818-845`). Without an interval there is no `else`, so control falls through at `:846` into the serendipity check and Newton iteration.
- **What happens:** the solver continues with whatever the failed evaluator left behind, although no function value was obtained. `SmCurvePropertyEFO::Evaluate` zeroes its outputs before failing (`source/SMLib/src/SmCurve.cpp:14919`); that leftover `F = 0` can be accepted as a converged root. Partial writes can instead drive further iterations, and the base evaluator can leave the undefined-value sentinel looking like a root. The shim defines the found flag on failure, so these measurements do not rely on an uninitialised flag.
- **Trigger:** the constructor without an interval and failure of the evaluation at the initial guess. The measured cases below trigger this through `func.fail_first`, the always-failing base evaluator (`func.kind = 99`), or an initial guess within the configured failure range.
- **How found:** source reading while fixing solidcore#2574 in PR #2637, followed by the supplied generated/fixture difftest run against the pinned C++ reference.
- **Confirmed:** yes, `bad_first_eval_no_interval`, `base_class_evaluator_no_interval` and the generated cases below in `geom.local_solve1d`, measured on 2026-11-04 at head `2148ea293`.
- **Rust port:** `LocalSolve1d::eval_into` does not commit outputs from a failed evaluation, and `solve_it` returns the initial evaluator error when there is no interval. An unevaluated value cannot establish a root or seed Newton iteration. This is unconditional in PR #2637.
- **Relationship:** WR-253 records failed values consumed later in the search and its curve-property consequences; its 2026-10-31 addendum also describes this initial fall-through. This separate record isolates that mechanism with measured case evidence. WR-19 concerns the separate interval-less Newton-step clamp. The minimum-radius caller in `local_property_analysis` constructs a solver with an interval; its measured curve cases are therefore documented under WR-253, not claimed as interval-less reproductions here.

### Measurement and assignment (PR #2637)

Source: `F:/Dev/solidcore/runs/2026-11-04-2574-straight-cusp/difftest/geom.local_solve1d.json`
and `summary.txt`, run started 2026-11-04 at `2148ea293`. The task identifies
rebased head `0c0a3512b` as containing the same fix. These are the supplied
measurements, not a new run on that rebased head.

Of 3318 solver cases, 78 failed comparison. All 25 failing cases without an
interval have a failed initial evaluation and are assigned here: 16 have
`fail_first > 0`, eight additional cases use `kind = 99`, and `gen2715` starts
inside its failure range. Both a missing `interval` and JSON `null` mean no
interval in the runner. This classification uses the evaluator's failure
predicate in `crates/difftest/src/units/geom_local_solve1d.rs` and
`harness/cpp/shim/geom_local_solve1d.cpp`, not case-name patterns alone.
In particular, `kind = 99` delegates to the failing base implementation before
the `fail_first`/failure-range test.

The other 53 solver discrepancies have an interval and are recorded in the
WR-253 measurement addendum. The field names and C++/Rust strings below are
copied from each case's `mismatches[]`. `1.23456e-61` is the solver's undefined
sentinel, not a measured small residual. The JSON retains at most 20 mismatch
details per case: when `mismatch_count` is larger, only the supplied details
are reproduced; no missing values are inferred.

### `geom.local_solve1d` / `bad_first_eval_no_interval`

Source: `fixture:fixtures/geom.local_solve1d/cases.json#16 (upstream source/SMLib/src/SmLocalSolve1d.cpp:806)`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=true`.
Reported mismatch count: 21; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `1.88492229492188e21` | `1.23456e-61` |
| `values.state_f[3]` | `1.0000000000000003e-1` | `1e20` |
| `values.state_f[4]` | `4e-1` | `1.23456e-61` |
| `values.state_f[5]` | `1.88492229492188e21` | `4e-1` |
| `values.state_f[6]` | `1.88492229492188e21` | `1.23456e-61` |
| `values.state_f[7]` | `1e0` | `1.23456e-61` |
| `values.state_f[8]` | `1.933593750011904e15` | `0e0` |
| `values.state_f[9]` | `1.884921328125005e21` | `1.23456e-61` |
| `values.state_f[10]` | `1.884921328125005e21` | `1.23456e-61` |
| `values.state_f[11]` | `1e0` | `1.23456e-61` |
| `values.state_f[12]` | `1.933593750011904e15` | `1.23456e-61` |
| `values.state_f[13]` | `1.884919394531255e21` | `1.23456e-61` |
| `values.state_f[14]` | `1.884919394531255e21` | `1.23456e-61` |
| `values.state_f[15]` | `1e0` | `1.23456e-61` |
| `values.state_f[16]` | `1.933593750011904e15` | `1.23456e-61` |
| `values.t[0]` | `1.88492229492188e21` | `0e0` |
| `values.trace_t.len` | `361` | `1` |
| `ints.state_i[3]` | `100` | `0` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `base_class_evaluator_no_interval`

Source: `fixture:fixtures/geom.local_solve1d/cases.json#18 (upstream source/SMLib/inc/SmLocalSolve1d.h:86)`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `5e-1` | `1.23456e-61` |
| `values.t[0]` | `5e-1` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen0142`

Source: `generated:seed=1894831246230430391`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `7.488937073916664e-1` | `1.23456e-61` |
| `values.t[0]` | `7.488937073916664e-1` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen0206`

Source: `generated:seed=1894831246230430455`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=true`.
Reported mismatch count: 24; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `-1.4673399510531036e0` | `1.23456e-61` |
| `values.state_f[1]` | `-1.4673399510531036e0` | `-1e20` |
| `values.state_f[2]` | `9.13755882083108e-1` | `1.23456e-61` |
| `values.state_f[3]` | `6.634399172201346e9` | `1e20` |
| `values.state_f[4]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[6]` | `-1.4673399510531036e0` | `1.23456e-61` |
| `values.state_f[7]` | `-1.4808171223728816e-10` | `1.23456e-61` |
| `values.state_f[8]` | `-1.584e19` | `0e0` |
| `values.state_f[9]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[10]` | `6.634399172201346e9` | `1.23456e-61` |
| `values.state_f[11]` | `-2.0941916578540284e-10` | `1.23456e-61` |
| `values.state_f[12]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[13]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[14]` | `6.634399172201346e9` | `1.23456e-61` |
| `values.state_f[15]` | `-2.0941916578540284e-10` | `1.23456e-61` |
| `values.state_f[16]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[21]` | `-1.584e19` | `1.23456e-61` |
| `values.t[0]` | `9.13755882083108e-1` | `0e0` |
| `values.trace_t.len` | `384` | `362` |
| `ints.state_i[3]` | `1` | `0` |


### `geom.local_solve1d` / `gen0209`

Source: `generated:seed=1894831246230430458`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=true`.
Reported mismatch count: 23; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `3.299892458907485e0` | `1.23456e-61` |
| `values.state_f[1]` | `-1.9625758227861815e19` | `-1e20` |
| `values.state_f[2]` | `1.584e19` | `1.23456e-61` |
| `values.state_f[3]` | `3.299892458907485e0` | `1e20` |
| `values.state_f[4]` | `-9.25248822480051e-1` | `1.23456e-61` |
| `values.state_f[6]` | `3.299892458907485e0` | `1.23456e-61` |
| `values.state_f[7]` | `-1.2389998881225894e0` | `1.23456e-61` |
| `values.state_f[8]` | `1.584e19` | `0e0` |
| `values.state_f[9]` | `1.584e19` | `1.23456e-61` |
| `values.state_f[10]` | `-1.9625758227861815e19` | `1.23456e-61` |
| `values.state_f[11]` | `-1.2389998881225894e0` | `1.23456e-61` |
| `values.state_f[12]` | `1.584e19` | `1.23456e-61` |
| `values.state_f[13]` | `1.584e19` | `1.23456e-61` |
| `values.state_f[14]` | `-1.9625758227861815e19` | `1.23456e-61` |
| `values.state_f[15]` | `-1.2389998881225894e0` | `1.23456e-61` |
| `values.state_f[16]` | `1.584e19` | `1.23456e-61` |
| `values.t[0]` | `-9.25248822480051e-1` | `0e0` |
| `values.trace_t.len` | `23` | `1` |
| `ints.state_i[3]` | `1` | `0` |
| `ints.state_i[4]` | `1` | `0` |


### `geom.local_solve1d` / `gen0253`

Source: `generated:seed=1894831246230430502`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=true`.
Reported mismatch count: 21; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `1.9745104877610695e64` | `1.23456e-61` |
| `values.state_f[3]` | `1.0747348048856828e0` | `1e20` |
| `values.state_f[4]` | `3.685989842692763e-1` | `1.23456e-61` |
| `values.state_f[5]` | `1.88492229492188e21` | `3.685989842692763e-1` |
| `values.state_f[6]` | `1.9745104877610695e64` | `1.23456e-61` |
| `values.state_f[7]` | `3.1425865560833147e43` | `1.23456e-61` |
| `values.state_f[8]` | `1.933593750011904e15` | `0e0` |
| `values.state_f[9]` | `1.884921328125005e21` | `1.23456e-61` |
| `values.state_f[10]` | `1.9745074495197663e64` | `1.23456e-61` |
| `values.state_f[11]` | `3.142583332351397e43` | `1.23456e-61` |
| `values.state_f[12]` | `1.933593750011904e15` | `1.23456e-61` |
| `values.state_f[13]` | `1.884919394531255e21` | `1.23456e-61` |
| `values.state_f[14]` | `1.9745013730465092e64` | `1.23456e-61` |
| `values.state_f[15]` | `3.142576884892521e43` | `1.23456e-61` |
| `values.state_f[16]` | `1.933593750011904e15` | `1.23456e-61` |
| `values.t[0]` | `1.88492229492188e21` | `0e0` |
| `values.trace_t.len` | `361` | `1` |
| `ints.state_i[3]` | `100` | `0` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen0284`

Source: `generated:seed=1894831246230430533`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.735324273482498e0` | `1.23456e-61` |
| `values.t[0]` | `-2.735324273482498e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen0317`

Source: `generated:seed=1894831246230430566`.
Input: `op=solve_it`; interval absent/null; fail_first=2; `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.120096617685072e0` | `1.23456e-61` |
| `values.t[0]` | `-2.120096617685072e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen0405`

Source: `generated:seed=1894831246230430654`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=true`.
Reported mismatch count: 21; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `-4.691066678774453e-1` | `1.23456e-61` |
| `values.state_f[1]` | `-2.1309805387724962e-1` | `-1e20` |
| `values.state_f[2]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[5]` | `1.85324229492188e21` | `1.889997180682303e0` |
| `values.state_f[6]` | `-4.691066678774453e-1` | `1.23456e-61` |
| `values.state_f[7]` | `-5.40096762043265e-46` | `1.23456e-61` |
| `values.state_f[8]` | `1.933593750011904e15` | `0e0` |
| `values.state_f[9]` | `1.853241328125005e21` | `1.23456e-61` |
| `values.state_f[10]` | `-4.691066678774453e-1` | `1.23456e-61` |
| `values.state_f[11]` | `-1e0` | `1.23456e-61` |
| `values.state_f[12]` | `1.933593750011904e15` | `1.23456e-61` |
| `values.state_f[13]` | `1.853239394531255e21` | `1.23456e-61` |
| `values.state_f[14]` | `-4.691066678774453e-1` | `1.23456e-61` |
| `values.state_f[15]` | `-1e0` | `1.23456e-61` |
| `values.state_f[16]` | `1.933593750011904e15` | `1.23456e-61` |
| `values.t[0]` | `1.85324229492188e21` | `0e0` |
| `values.trace_t.len` | `361` | `1` |
| `ints.state_i[3]` | `100` | `0` |
| `ints.state_i[4]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen0431`

Source: `generated:seed=1894831246230430680`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `4.0793522862772313e-1` | `1.23456e-61` |
| `values.t[0]` | `4.0793522862772313e-1` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen0667`

Source: `generated:seed=1894831246230430916`.
Input: `op=solve_it`; interval absent/null; fail_first=2; `partial=false`.
Reported mismatch count: 14; supplied details: 14.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `1.6184337406439075e0` | `1.23456e-61` |
| `values.state_f[20]` | `1.23456e-61` | `1e20` |
| `values.state_f[21]` | `-2.3763536800970533e0` | `1.23456e-61` |
| `values.t[0]` | `1.6184337406439075e0` | `0e0` |
| `values.t[1]` | `-2.3763536800970533e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.found[1]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.state_i[6]` | `1` | `5` |
| `ints.state_i[11]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |
| `ints.status[1]` | `0` | `1007` |


### `geom.local_solve1d` / `gen0820`

Source: `generated:seed=1894831246230431069`.
Input: `op=solve_it`; interval absent/null; fail_first=2; `partial=true`.
Reported mismatch count: 36; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `3.300455560772656e-1` | `1e20` |
| `values.state_f[4]` | `9.624372732773168e-3` | `1.23456e-61` |
| `values.state_f[5]` | `1.584e19` | `9.624372732773168e-3` |
| `values.state_f[6]` | `3.218097113987879e-1` | `1.23456e-61` |
| `values.state_f[7]` | `1e0` | `1.23456e-61` |
| `values.state_f[9]` | `9.624372732773168e-3` | `1.23456e-61` |
| `values.state_f[10]` | `3.300455560772656e-1` | `1.23456e-61` |
| `values.state_f[11]` | `1e0` | `1.23456e-61` |
| `values.state_f[12]` | `0e0` | `1.23456e-61` |
| `values.state_f[13]` | `9.624372732773168e-3` | `1.23456e-61` |
| `values.state_f[14]` | `3.300455560772656e-1` | `1.23456e-61` |
| `values.state_f[15]` | `1e0` | `1.23456e-61` |
| `values.state_f[16]` | `0e0` | `1.23456e-61` |
| `values.state_f[17]` | `3.218097113987879e-1` | `1.23456e-61` |
| `values.state_f[20]` | `3.218097113987879e-1` | `1e20` |
| `values.state_f[21]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[22]` | `1.85324229492188e21` | `1.6788940898593188e0` |
| `values.state_f[23]` | `3.218097113987879e-1` | `1.23456e-61` |
| `values.state_f[24]` | `-0e0` | `1.23456e-61` |
| `values.state_f[25]` | `1.933593750011904e15` | `0e0` |


### `geom.local_solve1d` / `gen0917`

Source: `generated:seed=1894831246230431166`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=true`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-1.1629520762535466e0` | `1.23456e-61` |
| `values.t[0]` | `-1.1629520762535466e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen1015`

Source: `generated:seed=1894831246230431264`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.2067871959893806e0` | `1.23456e-61` |
| `values.t[0]` | `-2.2067871959893806e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen1062`

Source: `generated:seed=1894831246230431311`.
Input: `op=solve_it`; interval absent/null; fail_first=2; `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `2.9601997574811145e0` | `1.23456e-61` |
| `values.t[0]` | `2.9601997574811145e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen1087`

Source: `generated:seed=1894831246230431336`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=true`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-1.8934542383538262e0` | `1.23456e-61` |
| `values.t[0]` | `-1.8934542383538262e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen1196`

Source: `generated:seed=1894831246230431445`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=true`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.9221738217264903e-1` | `1.23456e-61` |
| `values.t[0]` | `-2.9221738217264903e-1` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen1523`

Source: `generated:seed=1894831246230431772`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.096825360599385e0` | `1.23456e-61` |
| `values.t[0]` | `-2.096825360599385e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen1635`

Source: `generated:seed=1894831246230431884`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.6958215234034943e0` | `1.23456e-61` |
| `values.t[0]` | `-2.6958215234034943e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen2410`

Source: `generated:seed=1894831246230432659`.
Input: `op=solve_it`; interval absent/null; fail_first=2; `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-2.671615227194796e0` | `1.23456e-61` |
| `values.t[0]` | `-2.671615227194796e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1007` |


### `geom.local_solve1d` / `gen2715`

Source: `generated:seed=1894831246230432964`.
Input: `op=solve_it`; interval absent/null; initial guess in failure range; `partial=true`.
Reported mismatch count: 23; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `-6.473868273673362e-1` | `1.23456e-61` |
| `values.state_f[1]` | `-6.473868273673362e-1` | `-1e20` |
| `values.state_f[2]` | `6.269035470834909e-1` | `1.23456e-61` |
| `values.state_f[3]` | `3.0978664346350045e6` | `1e20` |
| `values.state_f[4]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[6]` | `-6.473868273673362e-1` | `1.23456e-61` |
| `values.state_f[7]` | `-4.106761979892074e-14` | `1.23456e-61` |
| `values.state_f[8]` | `-1.584e19` | `0e0` |
| `values.state_f[9]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[10]` | `3.0978664346350045e6` | `1.23456e-61` |
| `values.state_f[11]` | `-1e0` | `1.23456e-61` |
| `values.state_f[12]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[13]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[14]` | `3.0978664346350045e6` | `1.23456e-61` |
| `values.state_f[15]` | `-1e0` | `1.23456e-61` |
| `values.state_f[16]` | `-1.584e19` | `1.23456e-61` |
| `values.t[0]` | `6.269035470834909e-1` | `0e0` |
| `values.trace_t.len` | `23` | `1` |
| `ints.state_i[3]` | `1` | `0` |
| `ints.state_i[4]` | `1` | `0` |


### `geom.local_solve1d` / `gen2759`

Source: `generated:seed=1894831246230433008`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=false`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `-1.7237329531990448e0` | `1.23456e-61` |
| `values.t[0]` | `-1.7237329531990448e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |


### `geom.local_solve1d` / `gen2811`

Source: `generated:seed=1894831246230433060`.
Input: `op=solve_it`; interval absent/null; fail_first=2; `partial=true`.
Reported mismatch count: 37; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[1]` | `-5.8095764715159275e0` | `-1e20` |
| `values.state_f[2]` | `-2.930571398736296e0` | `1.23456e-61` |
| `values.state_f[5]` | `-1.584e19` | `-2.930571398736296e0` |
| `values.state_f[6]` | `-2.7133631206949847e19` | `1.23456e-61` |
| `values.state_f[7]` | `1e0` | `1.23456e-61` |
| `values.state_f[9]` | `-2.930571398736296e0` | `1.23456e-61` |
| `values.state_f[10]` | `-5.8095764715159275e0` | `1.23456e-61` |
| `values.state_f[11]` | `1e0` | `1.23456e-61` |
| `values.state_f[12]` | `0e0` | `1.23456e-61` |
| `values.state_f[13]` | `-2.930571398736296e0` | `1.23456e-61` |
| `values.state_f[14]` | `-5.8095764715159275e0` | `1.23456e-61` |
| `values.state_f[15]` | `1e0` | `1.23456e-61` |
| `values.state_f[16]` | `0e0` | `1.23456e-61` |
| `values.state_f[17]` | `3.2288375255156465e21` | `1.23456e-61` |
| `values.state_f[19]` | `-2.930571398736296e0` | `1.23456e-61` |
| `values.state_f[20]` | `5.890299096744427e-1` | `1e20` |
| `values.state_f[21]` | `8.047902380834477e-1` | `1.23456e-61` |
| `values.state_f[22]` | `1.88492229492188e21` | `8.047902380834477e-1` |
| `values.state_f[23]` | `3.2288375255156465e21` | `1.23456e-61` |
| `values.state_f[24]` | `1.7129817681155206e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2813`

Source: `generated:seed=1894831246230433062`.
Input: `op=solve_it`; interval absent/null; fail_first=1; `partial=true`.
Reported mismatch count: 26; supplied details: 20.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `-2.1064196320611206e-1` | `1.23456e-61` |
| `values.state_f[1]` | `-1.330829767459598e-1` | `-1e20` |
| `values.state_f[2]` | `0e0` | `1.23456e-61` |
| `values.state_f[3]` | `1.6671932429366199e0` | `1e20` |
| `values.state_f[4]` | `1.584e19` | `1.23456e-61` |
| `values.state_f[5]` | `3.890152914917542e17` | `-9.09718352176208e-1` |
| `values.state_f[6]` | `-2.1064196320611206e-1` | `1.23456e-61` |
| `values.state_f[7]` | `-2.0263410453626203e0` | `1.23456e-61` |
| `values.state_f[8]` | `-1.584e19` | `0e0` |
| `values.state_f[9]` | `0e0` | `1.23456e-61` |
| `values.state_f[10]` | `-1.330829767459598e-1` | `1.23456e-61` |
| `values.state_f[11]` | `-1.1605714018514084e0` | `1.23456e-61` |
| `values.state_f[12]` | `-1.584e19` | `1.23456e-61` |
| `values.state_f[13]` | `1.584e19` | `1.23456e-61` |
| `values.state_f[14]` | `1.6671932429366199e0` | `1.23456e-61` |
| `values.state_f[15]` | `-2.0263410453626203e0` | `1.23456e-61` |
| `values.state_f[16]` | `1.584e19` | `1.23456e-61` |
| `values.t[0]` | `3.890152914917542e17` | `0e0` |
| `values.trace_t.len` | `56` | `16` |
| `ints.found[0]` | `1` | `0` |


### `geom.local_solve1d` / `gen2923`

Source: `generated:seed=1894831246230433172`.
Input: `op=solve_it`; interval absent/null; base evaluator (kind=99); `partial=true`.
Reported mismatch count: 7; supplied details: 7.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[3]` | `1.23456e-61` | `1e20` |
| `values.state_f[4]` | `1.891103870210955e0` | `1.23456e-61` |
| `values.t[0]` | `1.891103870210955e0` | `0e0` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[5]` | `1` | `0` |
| `ints.status[0]` | `0` | `1001` |
