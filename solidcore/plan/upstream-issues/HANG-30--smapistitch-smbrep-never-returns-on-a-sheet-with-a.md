#### HANG-30. `SmApiStitch(SmBrep*)` never returns on a sheet with a coordinate near the top of the double range
- **Severity:** Wrong result
- **Area:** Healing
- **Location:** `source/SM_API/src/SmApiBrep.cpp:1128`-`1151` (`SmApiStitch(pBrep)`) -> `SmBrep::StitchAndOrient` (unit `topology.brep_p5`); the looping routine is not located.
- **What happens:** A one-face sheet whose biquadratic surface has one control point at `-1e300` (the others about `1e-3`) makes `StitchAndOrient` run past 20 s without returning, also when run alone; the same class as HANG-20 (box arithmetic overflowing on a coordinate in the top decades of the double range).
- **Trigger:** `SmApiStitch` on such a sheet (difftest fuzz program `fuzz01923` of `heal.stitch_p1`).
- **How found:** difftest fuzzing of `heal.stitch_p1` (PR #196).
- **Confirmed:** yes (C++ reference run, reproduced alone by the fuzz harness).
- **Severity:** hang.
- **Rust port:** `heal::stitch::p1::api_stitch` defers at `SmBrep::StitchAndOrient` (`topology.brep_p5`); the difftest FFI bounds sheet values at 1e150 for the full-stitch API entry points.
