#### Updates: WR-253 (2026-10-25). Opt-in fix for issue #481

- **Fix:** `SolveOscillating` takes a sign change only from an interior guess that evaluated;
  a failed guess is skipped (as its `continue` intends) and its leftover `F` is not tested
  after the loop.
- **Rust port:** opt-in per solver, `LocalSolve1d::set_failed_guess_no_sign_change` (default
  `false`, upstream's test), in `crates/solidcore/src/geom/local_solve1d.rs`. The curve
  property solve (`curve::curve::p5`, `LocalPropertyAnalysis`, which `GlobalPropertyAnalysis`
  and `SmBSplineCurve::AssertValid`'s radius test use) sets it from
  `Kernel::fixes.oscillating_skips_failed_guesses` (default `false`). Turn it on with WR-382's
  `Kernel::fixes.bspline_check_min_radius` (tangency-labs/solidcore#475), which otherwise reports a
  radius of 0 on straight pieces.
- **Test:** `issue_481_failed_guess_is_no_sign_change` in
  `crates/solidcore/src/geom/local_solve1d.rs` (the same `F = t^3 - 2t + 2` with the failing
  last guess now finds nothing, as without the failure).
- **Difftest:** with the default, `geom.local_solve1d` and the curve property units compare
  as before.
- Fixes tangency-labs/solidcore#481.
