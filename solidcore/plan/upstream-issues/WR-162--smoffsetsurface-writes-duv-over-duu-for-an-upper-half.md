#### WR-162. `SmOffsetSurface` writes `Duv` over `Duu` for an upper half without `v` derivatives
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1442`, `SmOffsetSurface::EvaluatePlainOrSimple`.
- **What happens:** With `bOnlyUpperHalf`, `hu = 2` and `hv = 0`, the mixed derivative is computed (the larger order is 2) and written at index `hv + 2 = 2`, the slot of `Duu`.
- **Trigger:** `Evaluate(uv, 2, 0, .., TRUE /*bOnlyUpperHalf*/, ..)` with a non-zero distance.
- **How found:** read during porting.
- **Confirmed:** yes (fixture `upper_half_slot_overwrite`).
- **Severity:** wrong result.
- **Rust port:** Kept.
