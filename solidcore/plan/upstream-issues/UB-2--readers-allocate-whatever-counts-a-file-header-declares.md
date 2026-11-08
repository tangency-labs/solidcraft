#### UB-2. Readers allocate whatever counts a file header declares [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `SmBSplineSurface.cpp:6367` `SmBSplineSurface::ReadFromDB`, `SmBSplineCurve.cpp:7237` `SmBSplineCurve::ReadFromDB` and the other `ReadFromDB` readers. Also `SmCompositeCurve.cpp:4169` `SmCompositeCurve::ReadFromDB`: an unreadable ASCII segment count leaves `SM_UNDEF_ULONG` (123456789), which is then `calloc`'d and read with uninitialised locals.
- **What happens:** Counts from the header go straight to `malloc`/`calloc` and are not checked against the data left. `n + p + 1` can overflow. ASCII readers keep going with stale values after a missing field.
- **Trigger:** A `.smc`/`.sms` record whose header declares e.g. `n = 2^40` control points followed by a few bytes.
- **How found:** read during porting (commit security review of `nurbs.b_spline_surface_p1`).
- **Confirmed:** no.
- **Severity:** crash/UB (memory exhaustion, integer overflow).
- **Rust port:** Checks each count against the bytes left (`DatabaseReader::ensure_available`) after upstream's own validation, and returns `InvalidInput`.
