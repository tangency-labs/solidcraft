#### HANG-44. `N_CrvOffsetPtSampling` time grows quadratically with an unbounded sample count
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/NL_CrvOffset.cpp:1380`-`1381` (sample count per segment, no upper bound), `:1525` `N_CrvRemoveKnotsDerivConstraints( curQ, tol, NL_BOTH, der, curQ, SG )` on the interpolant through all `mu` samples.
- **What happens:** The count is `ceil((1/tol)^0.34 * max(2 deg, sqrt(Muu / 8)))` per segment, with no cap. The interpolant has `mu` control points, and the knot removal that follows takes time quadratic in `mu`. The Rust port, which runs the same algorithm, needs about 5 s for 23 000 samples. A pole makes `Muu` huge (727 541 samples for the case below), and that run did not finish.
- **Trigger:** `nlib.crv_offset` fuzz00366: a cubic, 8 control points, weight of control point 5 set to `-1.0` (`/crv/cpts/5/3 = -1.0`), `B = (0,0,1)`, `d = 0`, `deg = 3`, `NL_INHERITED`, `tol = 0.11799393559359235` (Rust: no result after 20 s). The same happens for valid positive-weight curves with a tiny `tol`.
- **How found:** fuzz sweep (Rust timeout on the faithful port).
- **Confirmed:** no. The shim refuses more than `MAX_SAMPLES` (4000) samples and the FFI refuses non-positive weights, so the C++ never ran this input.
- **Severity:** hang.
- **Rust port:** `InvalidInput` ("too many sample points") above `MAX_OFFSET_SAMPLES` = 16 384 (`crates/solidcore/src/nurbs/crv_offset.rs:52`, check at `:214`), a documented deviation. The bound needs a maintainer decision.
