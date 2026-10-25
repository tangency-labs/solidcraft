#### HANG-42. `N_SrfRemoveAllKnots` / `N_SrfRemoveKnotsConstraints` loop forever once a removal error bound is NaN
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/NL_SrfTool.cpp:3114` `N_SrfRemoveAllKnots` and `:11424` `N_SrfRemoveKnotsConstraints` (`dir = NL_UVDIR` direction choice).
- **What happens:** With a control point near `1e300` the u error bounds are NaN, so `bu < bv` is false and every `NL_UVDIR` step goes to v. Once v has nothing removable, each step removes nothing and leaves the selection state unchanged, and the loop never ends.
- **Trigger:** `N_SrfRemoveAllKnots(sur, tol = 0, NL_UVDIR)` on a generated surface with one net coordinate `1e300` (`nlib.srf_tool_p3` `fuzz00246`, `fuzz00619`, `fuzz00633`, `fuzz01530`, `fuzz01724`); `N_SrfRemoveKnotsConstraints(.., tol = 1, NL_UVDIR)` with a net coordinate `-1e300` (`nlib.srf_tool_p4` `fuzz00396`, `fuzz00456`, `fuzz01087`, `fuzz01339`, `fuzz01923`).
- **How found:** generated difftest case (`difftest --fuzz 2000`: C++ gave no result within 20 s).
- **Confirmed:** yes (timeout, 10 cases).
- **Severity:** hang.
- **Rust port:** `nurbs::srf_tool::p3::remove_all_knots` detects a step that changes nothing and returns `NotConverged`. The `nlib.srf_tool_p3`/`p4` FFIs only remove knots of nets with homogeneous coordinates within `MAX_REMOVAL_COORD = 1e60` and weights within `[1e-30, 1e30]` (conservative, not an exact test).
