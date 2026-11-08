#### UB-102. `SmSrfInVolume::Evaluate` with orders (0,0) and `bOnlyUpperHalf = FALSE` writes `Duv` past the output array
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSrfInVolume.cpp:598`-`601` (the `lTempNumDerivsToEval = 2` branch), `:~720` onward (the `Duv` store), `SmSrfInVolume::Evaluate`.
- **What happens:** With `lHighestUDeriv == lHighestVDeriv <= 1` and `bOnlyUpperHalf == FALSE` the second-derivative branch is taken, and `Duv` is stored at an index one past the single entry a (0,0) request sizes the caller's array to.
- **Trigger:** `Evaluate(uv, 0, 0, D, FALSE)` with a one-entry `D`.
- **How found:** crash in the C++ reference run (the shim's exit 139 during `surface.srf_in_volume` difftest development).
- **Confirmed:** yes (segfault in the shim; the shim now sizes the array to avoid it).
- **Severity:** crash/UB (out-of-bounds write).
- **Rust port:** Skips the out-of-range `Duv` write and returns success with `D`; documented and tested.
