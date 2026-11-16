Updates: WR-253

#### Updates: WR-253 (2026-10-31). Completes d2757e1 (the N-D part) under the same flag (#1945)

- **Upstream:** `source/SMLib/src/SmLocalSolveNd.cpp`, `SmLocalSolveNd::SolveIt`, the clamped-step
  block (~454-582) and the re-evaluation at the best guess (~809). It sums the equations of a
  failed clamped evaluation, can take a failed scaled evaluation as the better step, and when it
  takes the clamped-and-scaled step it copies that point's equation values but keeps the unscaled
  point's Jacobian (`sJacobian`, not `sJacobian2`) and found flag, so the next Newton step uses the
  Jacobian of another point. The status of the re-evaluation at the best guess is ignored.
- **Fork fix:** tangency-labs/smlib@d2757e1 ("the local solvers take no value from a failed
  evaluation"), whose 1-D part is this record's earlier addendum (#481). Its N-D part: no sum of a
  failed evaluation's equations; the scaled step only `if (!bSecondFailed && ...)`, with
  `sJacobian = sJacobian2; bFoundAnswer = bFoundAnswer2; bFirstFailed = FALSE;`; the line search
  stops when the clamped step failed and no scaled one was taken; a best guess that cannot be
  re-evaluated is no solution (`SM_TR_UNABLE_TO_CONVERGE`).
- **Rust port:** completes d2757e1 under the same opt-in flag as its 1-D part,
  `Kernel::fixes.oscillating_skips_failed_guesses` (default `false`, upstream), passed to each N-D
  solve by `LocalSolveNd::set_skips_failed_evaluations` in `crates/solidcore/src/geom/local_solve_nd.rs`.
  Every `LocalSolveNd` built with a `Kernel` in reach sets it; the volume point solve
  (`surface::volume::p2`, `SmVolume::LocalPointSolveSimple`) has no kernel and keeps upstream's solve.
  Callers that already turned the flag on for the 1-D fix get the N-D fix too, as the fork has both.
- **Effect (#1945):** with `fillet_through_extended_rail_end` and `fillet_trace_reaches_rails`, a
  near-tangent surface/surface intersection (`ComputePointValues`' `SM_SO_NORMALIZE` local surface
  solve) takes a clamped-and-scaled step at the round's v = 1 bound; with the stale Jacobian its end
  lands 1.6e-8 off, the corner vertex drops 3.1e-8 from the rail's knot, `Trim` does not snap, and
  the rail's UV trim keeps a 3.1e-8 span (check tests #15, #3, #11). With the flag the trim is clean.
  Root cause traced by tdamsma on #1945 against an instrumented fork build.
- **Test:** `crates/solidcore/tests/issue_1945_rail_uv_trim.rs` (flag on: check clean, removal inside
  the analytic bracket; flag off: test #15 as upstream).
