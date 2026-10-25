#### UB-43. Small object-model NULL dereferences
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `source/SMLib/inc/SmVector2d.h:541` `SmVector2d::operator[]` (non-const, index > 1); `source/SMLib/inc/SmObject.h:331`-`332` `SmTemporaryChangeValue::UseOrigValue` / `UseTempValue` on a default-constructed guard.
- **What happens:** Both dereference NULL.
- **Trigger:** `v[2] = 1.0` on an `SmVector2d`; `SmTemporaryChangeValue<double> g; g.UseOrigValue();`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB.
- **Rust port:** Returns `SolidError::Upstream { code: 1001 }`, or does nothing.
