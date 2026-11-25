#### UB-73. `SmBSplineVolume::CalculateContinuitiesSimple` samples `sKnots2[ii]` past its end
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmBSplineVolumeNLib.cpp:602` and `:635` (`SmBSplineVolume::CalculateContinuitiesSimple`, def. `:456`); also `:667` (`rConts[kk]`).
- **What happens:** The inner sampling loop over `jj` reads `sKnots2[ii]` (the outer index) instead of `sKnots2[jj]`, so it samples one iso-curve per `ii` repeatedly and reads past the end of `sKnots2` (into its 256-entry stack buffer: stale or uninitialised values) whenever `sKnots1` has more samples than `sKnots2`. `rConts[kk]` likewise assumes every iso-curve has as many knots as the first one classified.
- **Trigger:** `CalculateContinuitiesSimple(.., bCalcGeometric = TRUE)` on a volume whose two other directions have different sample counts (e.g. 5 and 3), in a build where `EvaluateIsoParametricCurveSimple` succeeds (`USE_VOLUMES`).
- **How found:** read during porting.
- **Confirmed:** no (the reference build has no `USE_VOLUMES`, so the loop stops at the first sample; see WR-48).
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` where upstream would read past the arrays.
