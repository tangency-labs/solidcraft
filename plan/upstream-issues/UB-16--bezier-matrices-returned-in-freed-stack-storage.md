#### UB-16. Bezier matrices returned in freed stack storage
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_Bezier.cpp:301` `N_BezElevateDegree` (matrix built at `:319` on local stacks freed at `:353`). The same pattern appears in `N_BezSrfElevateDegree` and `N_BezFuncMultiplyBez*`.
- **What happens:** When the caller passes a NULL `NL_RMATRIX`, the matrix is built on a local `NL_STACKS` that `N_EndNurbs` frees before returning. The caller is left holding dangling storage.
- **Trigger:** Call `N_BezElevateDegree` with a NULL `dm`, then reuse `dm`.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash/UB (use after free).
- **Rust port:** Builds a local matrix when the caller's is null. Callers wanting reuse build it first.
