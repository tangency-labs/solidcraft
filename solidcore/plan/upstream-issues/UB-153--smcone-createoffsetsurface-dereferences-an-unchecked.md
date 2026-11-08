#### UB-153. `SmCone::CreateOffsetSurface` dereferences an unchecked `CreateCanonical` result
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmCone.cpp:624`-`641`, the apex-truncation branch of `SmCone::CreateOffsetSurface` (def. `:533`).
- **What happens:** When one new radius falls below the tolerance, the truncated cone is made by `SmCone::CreateCanonical`, whose status is ignored; `pOffCone->GetSTEPUVDomain()` and the following calls then dereference it. The new height is `dNewRadius * dHeight / dOldRadius`: an old radius of `0` gives an infinite or NaN height, for which the bounded constructor's NURB construction recurses without bound; a failed `MakeNurb` leaves `pOffCone` `NULL`.
- **Trigger:** A bounded cone with bottom radius `0` and a negative top radius, offset so that the new bottom radius is at least the tolerance and the new top radius below it.
- **How found:** read during porting (`surface.cone_p2`, PR #153).
- **Confirmed:** no. The shim tries the same `CreateCanonical` first (with the same formulas) and refuses the input when it fails or the height is not finite.
- **Severity:** crash/UB (null dereference, unbounded recursion).
- **Rust port:** `create_canonical`'s error is returned; no recursion.
