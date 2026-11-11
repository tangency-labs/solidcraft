#### UB-234. `N_CrvOffsetPtSampling` casts an unchecked sample count to `NL_INDEX`
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `source/SMLib/src/NL_CrvOffset.cpp:1380` `nu[i] = (NL_INDEX)del;` and `:1381` `mu += nu[i];` in `N_CrvOffsetPtSampling` (def. `:1217`), then `:1386` `N_AllocReal1dArray( mu, &SL )`. The same pattern is at `:297`/`:298` (`N_CrvOffsetFuncVariableDir`) and `:674`/`:675` (`N_CrvOffsetFuncConstantDir`).
- **What happens:** `del = ceil(f1 * gro)` comes from `N_CrvOffsetGetMax2ndDeriv` per Bezier segment. A NaN or infinite `Muu`, or a `del` above `LONG_MAX`, makes the double-to-integer conversion undefined. The per-segment counts are summed without an overflow check and the sum sizes the parameter and point arrays.
- **Trigger:** A rational curve with a non-positive weight (a pole: `Muu` non-finite or huge), or a tolerance near `0`. Fuzz cases `nlib.crv_offset` fuzz00366 / fuzz01037 / fuzz01299 (`/crv/cpts/k/3 = -1.0`) and fuzz00468 (`/tol = 1e+300`, `/deg = 4294967296`, control points at `+-1.8e308`).
- **How found:** read during porting (the `nlib.crv_offset` shim recomputes the count before the call); fuzz sweep (`difftest --fuzz 2000`) inputs reach it.
- **Confirmed:** no. The shim returns `SC_ERR_ARG` for a non-finite count or a total above `MAX_SAMPLES` (4000), and the FFI rejects non-positive weights.
- **Severity:** crash/UB.
- **Rust port:** Sums with `checked_add` and returns `InvalidInput` for an unrepresentable count or more than `MAX_OFFSET_SAMPLES` (16 384) samples.
