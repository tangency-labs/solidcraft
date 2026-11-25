#### UB-90. Volume inverse maps reach UB-42 through `MapPseudoBox` and `EvaluateBoundingBox`
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `SmVolume.cpp:3907` `InvMapPoint` -> `:2058` `MapPseudoBox` (trim branch: `EvaluateBoundingBoxSimple(sParamBox, NULL, ...)`); `:3411` `InvEvaluatePoint` and `:4512` `GlobalPointSolve` -> `:1692` `EvaluateBoundingBox(*pOptParamDomain, NULL, ...)`.
- **What happens:** With a search domain and compounding, an unoriented `SmTransform` map with next maps (or, for `EvaluateBoundingBox`, any transform map, and the orient map for in-space boxes) receives a NULL parameter pseudo box, and `SmTransform::EvaluateBoundingBoxSimple` dereferences it (UB-42).
- **Trigger:** `InvMapPoint(P, NULL, ok, pts, gaps, &domain, TRUE)` on `SmTransform` -> `SmTwistVolume`.
- **How found:** crash in the C++ reference run (segfault in `SmPseudoBox::AreAligned` under `InvMapPoint` -> `MapPseudoBox`).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Returns `SolidError::Unsupported`; the FFI guard of `surface.volume_p2` models these call paths and rejects them.
