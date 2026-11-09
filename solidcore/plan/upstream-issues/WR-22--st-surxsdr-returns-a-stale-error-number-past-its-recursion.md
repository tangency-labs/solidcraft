#### WR-22. `ST_surxsdr` returns a stale error number past its recursion limit
- **Severity:** Wrong result
- **Area:** NLib
- **Location:** `NL_SrfAdv.cpp:696`, `ST_surxsdr` (`depth > 10`).
- **What happens:** It returns `1` without `N_ErrSet`, so `NL_ERROR` holds whatever was recorded last. That can be an ignored iso-curve extension error, `NL_GEO_ERR`, or `0` after a callee's `N_ErrClear`. `ST_BezCalcArcLength` (`NL_Bezier.cpp:4713`) and `ST_curxcdr` / `ST_curxcrr` also return bare flags.
- **Trigger:** `DropCurveSurface.sms` extended 900 units in v: `eno = 17` at one end, `0` at the other.
- **How found:** adversarial / edge-case fixture.
- **Confirmed:** yes.
- **Severity:** wrong result (error reporting).
- **Rust port:** Returns `NotConverged`.
