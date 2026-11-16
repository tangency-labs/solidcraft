#### WR-166. `N_SrfIsClosedSmooth` reports an error after storing its result
- **Severity:** Wrong result
- **Area:** NLib
- **Location:** `source/SMLib/src/NL_SrfNurb.cpp:5178` (and the v-closure loop), `N_SrfIsClosedSmooth` (def. `:5058`).
- **What happens:** A failed `N_VectorsAngle` (a zero-length tangent, as at a pole side) only breaks out of the span loop: `error` stays `NL_YES`, the closed directions are still stored in `*gnflg`, and the function returns the error. `N_SrfEvalPtDerivsUnbounded` then fails its first call for such a surface with `esur` holding the copy, and callers that retry (`sm_ExtendSurface`, `EvaluatePointOnExtendedSurface`) extend that copy, `sm_ExtendSurface` at the unpushed parameter.
- **Trigger:** An offset extension over a B-spline with a pole side.
- **How found:** generated difftest case (`surface.offset_surface`: the Rust port, which dropped the flag on that error, diverged from the C++).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** `Surface::is_closed_smooth_flag` returns both; `eval_pt_derivs_unbounded` keeps the copy and the flag, as upstream.
