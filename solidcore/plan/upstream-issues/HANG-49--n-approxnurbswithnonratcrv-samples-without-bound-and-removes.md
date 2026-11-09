#### HANG-49. `N_ApproxNurbsWithNonRatCrv` samples without bound and removes knots in quadratic time
- **Severity:** Hang
- **Area:**
- **Location:** `NL_CrvApprox.cpp:2780` (`N_ApproxNurbsWithNonRatCrv`, def. `:2698`): the sample count `mu` is the sum of the per-segment counts from the second-derivative bound.
- **What happens:** a weight of `-1` (a pole) asks for about ten million samples; all are interpolated and `N_CrvRemoveKnotsDerivConstraints` then shifts its arrays once per removed knot, quadratic in the count.
- **Trigger:** `fuzz-out/nlib-fit10k/nlib.crv_approx/fuzz05147.json`.
- **How found:** fuzz-corpus replay of the Rust side alone (issue #504).
- **Confirmed:** no (the FFI refuses non-positive weights).
- **Severity:** hang (quadratic time in an input-derived count).
- **Rust port:** `InvalidInput` past `MAX_APPROX_SAMPLES = 2^17` samples (`nurbs/crv_approx.rs`).
