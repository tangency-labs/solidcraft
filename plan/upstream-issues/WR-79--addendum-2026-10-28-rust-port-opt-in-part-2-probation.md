#### Updates: WR-79 (2026-10-28). Part 2 (NURBS start points on probation) ported as an opt-in fix

- **Cause traced (NURBS path, tangency-labs/solidcore#396 part 2):**
  `SmAdvSurfaceIntersector::FindInteriorCurves` skips a node pair whose surface normals
  are within 5 degrees at the solver's guess, or within 2 degrees at the local
  intersection's solution. On a small closed loop between nearly tangent surfaces the
  normals differ by about the loop radius over the curvature radius (2e-4 rad for the
  NURBS paraboloid `z = x^2 + y^2` cut by `z = 1e-8`), so every start point on the loop
  is dropped and the loop is missed (`c7_tiny_loop_parab_1e-4`, `_1e-5`: no output).
- **Rust port:** `Kernel::fixes.ssi_low_angle_start_on_probation` (`UpstreamFixes`, off
  by default; `crates/solidcore/src/intersect/adv_surface_intersector.rs`). With it on,
  such a start point is traced on probation: the result is kept only when the trace is
  closed, has more than two points, the diagonal of their box exceeds `2 tol`, and the
  flushed curve is not `TANGENT`, `COINCIDENT` or `TOUCHING`. A start point whose
  solution angle is below `2 tol / modelSize` radians (model size: the larger box
  diagonal of the two surfaces over their domains) is still skipped, and a rejected
  trace and its start point (a degenerate curve) are remembered so later low-angle
  start points on them are not traced again. This is the SolidCraft fork's
  tangency-labs/smlib@30d25bf (`SmAdvSurfaceIntersector.cpp` part), without the fork's
  earlier tangent-seed bookkeeping, which v1.0.1 does not have.
- **Kind:** a result-changing fix (a missed intersection; no crash, undefined behaviour,
  hang or nondeterminism), so it follows the opt-in rule; off by default the port
  matches the pinned C++.
- **Test:** `crates/solidcore/tests/issue_396_probation_start_points.rs` (the corpus'
  paraboloid against `z = r^2` for `r = 1e-4, 1e-5` at tolerance 1e-6: no curve with the
  flag off; with it on one closed loop typed near tangent, within 1e-6 of the true
  circle; a transversal cut at `r = 0.5, 0.8` bit-identical with the flag off and on).
- **Scope:** part 1 (sphere/plane and sphere/sphere circles collapsed to a touching
  point, `c7_tiny_loop_sphere_*`) is the analytic path and a separate flag
  (`sphere_small_circle_not_touching`, addendum
  `WR-79--addendum-2026-10-28-rust-port-opt-in.md`); this flag does not reach those
  cases. **Limit (as in the fork):** the angle cutoff assumes curvature at model scale;
  a curvature-aware loop test would be the general fix.
