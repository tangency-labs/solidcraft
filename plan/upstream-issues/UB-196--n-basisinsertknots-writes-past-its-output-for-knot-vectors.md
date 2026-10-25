#### UB-196. `N_BasisInsertKnots` writes past its output for knot vectors shorter than `2p+2`
- **Severity:** Crash / undefined behaviour
- **Area:** NLib: knots and basis functions
- **Location:** `NL_BasisAdv.cpp:253`, `N_BasisInsertKnots` (copies at `:275-276` and `:302-303`).
- **What happens:** The routine always copies `R[0..=p]` and `R[mr-p..=mr]` into `S`. With fewer than `2p+2` knots these ranges overlap, so it writes `2p+2+mx+1` knots instead of the documented `mr+mx+2` and overruns a caller buffer of that size (heap corruption).
- **Trigger:** `mr < 2p+1` with `X[0] > R[p]` and `X[mx] < R[mr-p]`, which needs an unsorted `R` (e.g. `p = 1`, `R = [4294967296, 0]`, `X = [1e300, 1.5, 2.5, 3.5]`). The range check rejects such `R` when it is sorted.
- **How found:** crash in the C++ reference run (`difftest --fuzz 30000` on `nlib.basis_adv`, issue #182: two cases aborted in `malloc` with a corrupted heap).
- **Confirmed:** yes, by those two aborts (fixtures `fuzz_insert_short_unsorted_r`, `fuzz_insert_short_r_four_new_knots` in `fixtures/nlib.basis_adv/fuzz_regressions.json`). The shim now sizes `S` for every knot written, so the reference runs them and the results are compared.
- **Severity:** crash/UB.
- **Rust port:** Returns all `2p+2+mx+1` knots (a `Vec` grown as written), as upstream computes them.
