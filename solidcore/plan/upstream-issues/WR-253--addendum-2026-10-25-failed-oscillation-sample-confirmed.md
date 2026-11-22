Updates: WR-253

#### Issue #481 / WR-253 follow-up. Failed oscillation sample confirmed as a false root
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers

- **Location:** `source/SMLib/src/SmLocalSolve1d.cpp:470-495`, the sign test after the interior search.
- **Trigger:** `F=t^3-2t+2` on `[-3,3]`, guess zero, with an evaluator that zeroes its outputs and fails at the final interior sample `0.94564800000197`.
- **Confirmed:** yes, now reproduced through the unchanged native solver in the differential harness. C++ reports that failed sample as a converged root with F=0, although its actual F is about 0.95; the corrected Rust search reports no root.
- **Rust port:** the correction is opt-in through `Kernel::fixes.oscillating_skips_failed_guesses`. PR #668 retains main's default-off solver and curve-property path, threads the same switch through all other production `LocalSolve1d` callers (curve point/extrema/cache, surface tangent/silhouette, clipped fillet radius), and adds three direct opt-in regressions. The zero-on-failure fixture now agrees with pinned C++ under the default policy; no known mismatch is needed.
