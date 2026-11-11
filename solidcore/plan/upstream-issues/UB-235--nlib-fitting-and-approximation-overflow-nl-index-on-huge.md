#### UB-235. NLib fitting and approximation overflow `NL_INDEX` on huge caller counts
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: curves, fitting, surfaces
- **Location:** `NL_CrvApprox.cpp:2274` `N_ApproxCircArcWithCrvData` (`nc + p + 1` knots, `np = 4 * nc` at `:2444`, `:2526`); `NL_CrvFit.cpp:2000` `N_FitCrvCalcKnotVector`, `:2458` `N_FitCalcKnotVectorEndDerivs`, `:2562` `N_FitCalcKnotVectorDeriv`, `:4023` `N_FitCalcKnotVectorCrvApprox` (`n + p + 1`), `:2273` `N_FitCrvInterpGivenParams` (`n + p + 1` compared with the knot count), `:13320` `N_FitCrvWeightedLstSqKnots` (`mc + n`), `:14621` `N_FitCrvApproxClosedConditions` (`p + 1`, `k` derivatives allocated).
- **What happens:** Index sums and products on caller counts are signed `long` arithmetic with no overflow check. Near `LONG_MAX` they overflow (undefined behaviour), and a wrapped sum then sizes `N_Malloc` or indexes the knot array.
- **Trigger:** `N_ApproxCircArcWithCrvData(..., deg = 4, nc = 9223372036854775807, ...)` (fuzz `nlib.crv_approx` fuzz00475). `N_FitCrvInterp` with `p = 9223372036854775807` (`nlib.crv_fit_p1` fuzz00023). The bad argument has to come from the calling code. No file input reaches these calls.
- **How found:** fuzz sweep (Rust overflow panics at the mirrored lines).
- **Confirmed:** no. The FFI caps `nc` (`MAX_NC` = 400), counts and degrees.
- **Severity:** crash/UB.
- **Rust port:** `fit_arrays::add` / `mul` return `InvalidInput` on overflow. Where the sum is only compared with the knot count, an overflowing sum counts as a mismatch (`NL_INP_ERR`). `alloc` reserves fallibly and returns `NL_MEM_ERR`, as a failed `N_Malloc` does.
