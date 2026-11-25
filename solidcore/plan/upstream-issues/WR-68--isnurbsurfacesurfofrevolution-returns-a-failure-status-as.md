#### WR-68. `IsNurbSurfaceSurfOfRevolution` returns a failure status as `TRUE` with a NULL surface
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurfOfRevolution.cpp:4659`, `:4676` (`SER(sm_TestForIsoCircles(..))` in a function returning `SmBoolean`). Same family as WR-52.
- **What happens:** `SER` returns the status (`SM_ERR` = 1001), which is truthy, so a failed iso-circle test is reported as "found" while `rpSurfOfRevolution` stays `NULL`. Callers that test the return value dereference or trust a missing surface.
- **Trigger:** a rational net whose generator lies on the rotation axis.
- **How found:** PR #51 review.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result.
- **Rust port:** returns `Err(SM_ERR)` from `test_for_iso_circles`; the difftest shim records the C++ return value as is.
