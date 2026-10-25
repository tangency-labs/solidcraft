#### WR-60. `SmBendVolume::TrimParamBoundingBoxSimple` keeps stale output for boxes crossing the period end
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBendVolume.cpp:2180`-`:2184`.
- **What happens:** Each `v` end is clamped to the primary period separately with `SM_CLAMP_TO_PERIOD`, so a box crossing `v = pi d` gets `vMin > vMax`. The final `SetMinMax` then fails, its status is ignored, and the function returns `SM_SUCCESS` with the caller's output box unchanged (whatever it held before).
- **Trigger:** Bend with neutral distance `1`, parameter box `v` in `[2, 4.5]`.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes (fixture `trim_unit_across_period` of `surface.bend_volume`).
- **Severity:** wrong result.
- **Rust port:** Ported as written.
