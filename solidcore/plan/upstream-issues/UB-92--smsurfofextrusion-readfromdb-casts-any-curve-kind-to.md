#### UB-92. `SmSurfOfExtrusion::ReadFromDB` casts any curve kind to `SmBSplineCurve` [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:4562`-`4565` `SmSurfOfExtrusion::ReadFromDB`.
- **What happens:** The generator read by `SmCurve::ReadFromDB` is only checked with `SM_ASSERT_MSG(pGenCurve->IsKindOf(SmBSplineCurve_TYPE))` (compiled out in release) and then stored as `SmBSplineCurve*`. A composite (or other non-B-spline) curve record makes every later generator call use the wrong class layout.
- **Trigger:** An `SmSurfOfExtrusion` record whose generator is an `SmCompositeCurve` record.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no. The FFI guard only passes `SmBSplineCurve` generator records; the shim also refuses a read generator that is not one.
- **Severity:** crash/UB (type confusion).
- **Rust port:** Returns `InvalidInput`.
