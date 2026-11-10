#### WR-23. `sm_CrvTrim` returns an error flag that callers read as success
- **Severity:** Wrong result
- **Area:** NLib
- **Location:** `SmNurbsCrv.cpp:729`, `sm_CrvTrim`.
- **What happens:** It returns the flag value `NL_INP_ERR` (14), not `NL_YES`, for `ur <= ul`, without `N_ErrSet`. Callers comparing with `NL_YES` treat it as success.
- **Trigger:** `sm_CrvTrim` with `ur == ul`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Returns `InvalidInput`.
