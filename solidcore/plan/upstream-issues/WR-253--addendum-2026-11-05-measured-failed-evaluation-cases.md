Updates: WR-253

#### Updates: WR-253 (2026-11-05). Measured failed-evaluation and minimum-radius cases for PR #2637

- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers; curves
- **Location:** `source/SMLib/src/SmLocalSolve1d.cpp:431,473,495,799,1083`, `SolveOscillating` / `SolveIt`; `source/SMLib/src/SmCurve.cpp:14919,15402,15993-16012`, `SmCurvePropertyEFO::Evaluate` / `SmCurve::LocalPropertyAnalysis`.
- **Confirmed:** yes, the named cases and measurements below are from the supplied pinned-C++ difftest run on 2026-11-04 at head `2148ea293`.
- **Scope:** adds case-specific evidence to the 2026-10-25 confirmation and the 2026-11-04 description of the unconditional fix. It does not edit or replace the merged records.

### Measurement and assignment

Source directory:
`F:/Dev/solidcore/runs/2026-11-04-2574-straight-cusp/difftest/`.
The JSON files are `geom.local_solve1d.json`, `curve.curve_p4.json`,
`curve.curve_p5.json`, and `nurbs.b_spline_curve_p3.json`; `summary.txt`
identifies the run head `2148ea293`. The task identifies rebased head
`0c0a3512b` as containing the same fix. No new difftest was run for this record.

Measured totals are 3318 / 78 failures, 3260 / 5 failures, 3140 / 3 failures,
and 2460 / 0 failures respectively. This addendum supports 61 entries:
53 solver cases with an interval, five direct property evaluations, and
three local property searches. The 25 interval-less solver cases all fail
their initial evaluation and belong to WR-750 instead, including the base
evaluator and failure-range triggers without positive `fail_first`.

For the 53 solver cases, a failed evaluation's outputs must not replace an
iteration value: partial writes and zeroed outputs are not valid function
values. This applies both to failed attempts at an interval start and to
failed samples during iteration. The change retains only successfully
evaluated values; the old sign/bracket and stopping decisions could read
the failed sample's outputs. Differences in state or trace are consequences
of that contract, not additional claimed geometric roots.

All eight curve cases use property 13 (`MinimizeRadiusOfCurvature`) on a
line or a degree-one B-spline. On each regular straight span,
`C'' = 0` and the closed form `|C'|^3 / |C' x C''|` has unbounded radius.
The five `property` cases measure C++'s zeroed `values.out[0] = 0e0` and
error status `1001`, versus solidcore's `1e20` (`BIG_DOUBLE`) and status `0`.
They exercise the changed objective directly, not a solver's initial
fall-through. They belong to the curve-objective part of WR-253's
2026-11-04 addendum.

The three `lpa` cases measure C++ status `1001` and no output values,
versus solidcore status `0` and one output value. The runner and C++ shim
(`crates/difftest/src/units/curve_curve_p5.rs`, `OpIn::Lpa`, and
`harness/cpp/shim/curve_curve_p5.cpp`, `OP_LPA`) show the meaning of that
length: a successful search emits the found flag, and emits solution data
only when found. Thus these Rust results are successful searches with no
solution, not finite minima or radii. The objective's unbounded-radius
sentinel avoids the C++ evaluation error; the new no-solution rule rejects
that sentinel as a minimum. Both parts were described in the 2026-11-04
addendum. The report gives no differing parameter or finite-radius value
for these cases, and none is invented here.

The tables reproduce every supplied `mismatches[]` field and C++/Rust string.
`1.23456e-61` is the solver's undefined sentinel, not a small residual.
No cases of `nurbs.b_spline_curve_p3` need entries. None of the measured
86 discrepancies remains unclassified.

### `geom.local_solve1d` / `gen0027`

Source: `generated:seed=1894831246230430276`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `2.8389032904265794e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0110`

Source: `generated:seed=1894831246230430359`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `3.821041993215646e-3` | `3.5192682940260295e-1` |


### `geom.local_solve1d` / `gen0165`

Source: `generated:seed=1894831246230430414`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.890415994813361e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0187`

Source: `generated:seed=1894831246230430436`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.461272811129354e0` | `-2.824181323262327e0` |


### `geom.local_solve1d` / `gen0404`

Source: `generated:seed=1894831246230430653`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.869202440958756e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0531`

Source: `generated:seed=1894831246230430780`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.9878594536081273e-1` | `1.9943861419631725e-1` |
| `values.state_f[23]` | `1.9860258370873457e-1` | `1.990503941985298e-1` |


### `geom.local_solve1d` / `gen0557`

Source: `generated:seed=1894831246230430806`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.217841766598926e-2` | `9.126831591040969e-1` |


### `geom.local_solve1d` / `gen0586`

Source: `generated:seed=1894831246230430835`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-8.806524451662676e-1` | `-8.836366172842304e-1` |


### `geom.local_solve1d` / `gen0589`

Source: `generated:seed=1894831246230430838`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.6767527725042437e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0634`

Source: `generated:seed=1894831246230430883`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `4.025472927405268e1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0853`

Source: `generated:seed=1894831246230431102`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-6.417883732738405e-2` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0884`

Source: `generated:seed=1894831246230431133`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `7.540159532754998e-2` | `1.23456e-61` |


### `geom.local_solve1d` / `gen0918`

Source: `generated:seed=1894831246230431167`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.3424301110309134e-1` | `1.2842605297235804e-1` |


### `geom.local_solve1d` / `gen0919`

Source: `generated:seed=1894831246230431168`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `2.4170732640389048e1` | `4.1929967407561854e1` |


### `geom.local_solve1d` / `gen1125`

Source: `generated:seed=1894831246230431374`.
Input: `op=solve_it`; interval present; fail_first=1; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-3.01888410120933e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1147`

Source: `generated:seed=1894831246230431396`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.8328704033617915e0` | `-2.399609162293568e0` |


### `geom.local_solve1d` / `gen1185`

Source: `generated:seed=1894831246230431434`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `2.0774209258216323e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1252`

Source: `generated:seed=1894831246230431501`.
Input: `op=solve_it`; interval present; fail_first=1; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.490201007460869e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1260`

Source: `generated:seed=1894831246230431509`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.0055876014617615e-1` | `-3.1269678623032604e-1` |


### `geom.local_solve1d` / `gen1336`

Source: `generated:seed=1894831246230431585`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.2025072957177727e0` | `-2.215640203364982e0` |


### `geom.local_solve1d` / `gen1338`

Source: `generated:seed=1894831246230431587`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-3.6473275832557844e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1346`

Source: `generated:seed=1894831246230431595`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.300086321459427e-2` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1376`

Source: `generated:seed=1894831246230431625`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `4.605693027661192e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1514`

Source: `generated:seed=1894831246230431763`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-2.077579883501285e-2` | `-3.158713703559761e-1` |


### `geom.local_solve1d` / `gen1532`

Source: `generated:seed=1894831246230431781`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.167133114066233e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1561`

Source: `generated:seed=1894831246230431810`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `2.011999833713121e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1572`

Source: `generated:seed=1894831246230431821`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.8917427291284135e0` | `2.2769498746578143e0` |


### `geom.local_solve1d` / `gen1606`

Source: `generated:seed=1894831246230431855`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.1273045673512727e-4` | `1.417278475247515e-1` |


### `geom.local_solve1d` / `gen1629`

Source: `generated:seed=1894831246230431878`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `4.109581109550633e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1655`

Source: `generated:seed=1894831246230431904`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.2401478676641333e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1665`

Source: `generated:seed=1894831246230431914`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `9.772916784465124e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1692`

Source: `generated:seed=1894831246230431941`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-1.624833804374245e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1717`

Source: `generated:seed=1894831246230431966`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-9.974705199807263e-1` | `-4.045404056138241e0` |


### `geom.local_solve1d` / `gen1721`

Source: `generated:seed=1894831246230431970`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `4.6450391392428236e-2` | `3.4781980179806404e-1` |


### `geom.local_solve1d` / `gen1734`

Source: `generated:seed=1894831246230431983`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.0923407216361443e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1898`

Source: `generated:seed=1894831246230432147`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.2258806639982742e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen1989`

Source: `generated:seed=1894831246230432238`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-5.614405246143255e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2036`

Source: `generated:seed=1894831246230432285`.
Input: `op=solve_it`; interval present; fail_first=1; `partial=true`.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-6.705709091213901e-1` | `-6.738735062739905e-1` |
| `values.state_f[23]` | `-6.702689148489069e-1` | `-6.737032039254e-1` |


### `geom.local_solve1d` / `gen2192`

Source: `generated:seed=1894831246230432441`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-3.9156277401025275e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2289`

Source: `generated:seed=1894831246230432538`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `6.863446723037189e-1` | `-8.156226548372908e-1` |
| `values.state_f[23]` | `8.20794036862956e-1` | `-8.156226548372908e-1` |


### `geom.local_solve1d` / `gen2383`

Source: `generated:seed=1894831246230432632`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `6.509230576591065e-1` | `7.696496214233891e-1` |


### `geom.local_solve1d` / `gen2591`

Source: `generated:seed=1894831246230432840`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[23]` | `1.343350178899689e0` | `1.4817860163211778e0` |


### `geom.local_solve1d` / `gen2708`

Source: `generated:seed=1894831246230432957`.
Input: `op=solve_it`; interval present; fail_first=1; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-8.556045914669821e-1` | `-9.566450982976977e-1` |


### `geom.local_solve1d` / `gen2718`

Source: `generated:seed=1894831246230432967`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.5684929555273457e0` | `1.7121592321406331e0` |


### `geom.local_solve1d` / `gen2735`

Source: `generated:seed=1894831246230432984`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `3.149148394364893e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2766`

Source: `generated:seed=1894831246230433015`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-6.602255801437036e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2776`

Source: `generated:seed=1894831246230433025`.
Input: `op=solve_it`; interval present; fail_first=2; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `6.38339756047165e-2` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2797`

Source: `generated:seed=1894831246230433046`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `8.639370850691806e0` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2903`

Source: `generated:seed=1894831246230433152`.
Input: `op=solve_it`; interval present; initial guess in failure range; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-5.779559365936585e-1` | `1.23456e-61` |


### `geom.local_solve1d` / `gen2928`

Source: `generated:seed=1894831246230433177`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `1.4580167195632023e0` | `3.5546769110868515e0` |


### `geom.local_solve1d` / `gen2943`

Source: `generated:seed=1894831246230433192`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 1; supplied details: 1.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `-4.043802873955552e-1` | `-4.1645873165379665e-1` |


### `geom.local_solve1d` / `gen2949`

Source: `generated:seed=1894831246230433198`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=true`.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[6]` | `4.534848147194793e-1` | `5.889546133131618e-1` |
| `values.state_f[23]` | `5.235121647426776e-1` | `6.395743513773237e-1` |


### `geom.local_solve1d` / `oscillation_failed_last_guess_zeroed_outputs`

Source: `fixture:fixtures/geom.local_solve1d/cases.json#317`.
Input: `op=solve_it`; interval present; no configured initial failure at the supplied guess; `partial=false`.
Reported mismatch count: 12; supplied details: 12.

| Field | C++ | Rust |
|---|---|---|
| `values.state_f[0]` | `0e0` | `1.23456e-61` |
| `values.state_f[1]` | `0e0` | `-1e20` |
| `values.state_f[2]` | `9.4564800000197e-1` | `1.23456e-61` |
| `values.state_f[5]` | `9.4564800000197e-1` | `8.316000000017324e-1` |
| `values.state_f[6]` | `0e0` | `9.119000984961294e-1` |
| `values.state_f[7]` | `7.097804800112903e-1` | `7.467568000864389e-2` |
| `values.t[0]` | `9.4564800000197e-1` | `8.316000000017324e-1` |
| `values.trace_t.len` | `36` | `21` |
| `ints.found[0]` | `1` | `0` |
| `ints.state_i[0]` | `1` | `5` |
| `ints.state_i[4]` | `1` | `0` |
| `ints.trace_flags.len` | `36` | `21` |


### `curve.curve_p4` / `gen0593`

Source: `generated:seed=7577093924984375023`.
Input: `op=property`; `prop=13`; degree-one B-spline.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out[0]` | `0e0` | `1e20` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p4` / `gen1006`

Source: `generated:seed=7577093924984375436`.
Input: `op=property`; `prop=13`; degree-one B-spline.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out[0]` | `0e0` | `1e20` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p4` / `gen1304`

Source: `generated:seed=7577093924984375734`.
Input: `op=property`; `prop=13`; line.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out[0]` | `0e0` | `1e20` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p4` / `gen1585`

Source: `generated:seed=7577093924984376015`.
Input: `op=property`; `prop=13`; line.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out[0]` | `0e0` | `1e20` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p4` / `gen2353`

Source: `generated:seed=7577093924984376783`.
Input: `op=property`; `prop=13`; line.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out[0]` | `0e0` | `1e20` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p5` / `gen0508`

Source: `generated:seed=7577095024496003149`.
Input: `op=lpa`; `prop=13`; line.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out.len` | `0` | `1` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p5` / `gen1302`

Source: `generated:seed=7577095024496003943`.
Input: `op=lpa`; `prop=13`; line.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out.len` | `0` | `1` |
| `ints.status[0]` | `1001` | `0` |


### `curve.curve_p5` / `gen2745`

Source: `generated:seed=7577095024496005386`.
Input: `op=lpa`; `prop=13`; degree-one B-spline.
Reported mismatch count: 2; supplied details: 2.

| Field | C++ | Rust |
|---|---|---|
| `values.out.len` | `0` | `1` |
| `ints.status[0]` | `1001` | `0` |
