#### UB-171. `SmOffsetSurface::EvaluatePlainOrSimple` writes past `aDerivatives` with `bOnlyUpperHalf`
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1442` (and the `Duuv` / `Duvv` writes after it), `SmOffsetSurface::EvaluatePlainOrSimple` (def. `:960`), reached from `Evaluate` and `EvaluateSimple`.
- **What happens:** With `bOnlyUpperHalf` the mixed derivatives are computed whenever the larger order allows them, but are written at their indices in the full `(hu+1)(hv+1)` layout. With more `v` than `u` derivatives they land past the caller's array (`hu = 0, hv = 2`: `Duv` at index 4 of a 3-entry array; also `0, 3` and `1, 3`).
- **Trigger:** `Evaluate(uv, 0, 2, TRUE, TRUE, TRUE /*bOnlyUpperHalf*/, a /*3 entries*/)` on an offset with a non-zero distance.
- **How found:** read during porting (`surface.offset_surface`).
- **Confirmed:** no. The harness rejects these orders.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput` for a write past the array.
