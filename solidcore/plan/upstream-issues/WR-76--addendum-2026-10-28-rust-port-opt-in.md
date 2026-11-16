#### Updates: WR-76 (2026-10-28). Ported as an opt-in fix

- **Cause traced (tangency-labs/solidcore#392):** three paths in
  `SmSurfaceIntersector.cpp` trace one open tangent curve more than once.
  1. Where the surfaces touch along a curve that crosses a boundary, the
     boundary/surface intersection is within tolerance over a stretch of the boundary,
     so `FindBoundaryStartPoints` returns a cluster of tangent start points there
     (about 1e-5 to 1e-3 apart). `AddStartPoint` keeps them all and `DoIntersection`
     traces each: the curve again, plus short spans snapped from one cluster point to
     the next (the 3.4e-5 deviation of `y23_t6_ex1_tangent_line`).
  2. `TraceCurve` runs its "second point already on a found curve" test only after a
     regular step; a first step that `TestPoint` ends (snapped onto the start point at
     the curve's other end) is added without it, so a curve one span long (the tangent
     line of `c2n_cylN_on_planeN`) comes back once from each end.
  3. Tangential tracing can stop part way; the trace from the other end then runs on
     over the first curve.
- **Rust port:** `Kernel::fixes.ssi_trace_tangent_curves_once` (`UpstreamFixes`, off by
  default), the SolidCraft fork's tangency-labs/smlib@0ce09f2:
  `AddStartPoint` keeps one start point per tangent cluster
  (`SurfaceIntersector::is_same_tangent_cluster`: same boundary side on both surfaces,
  within `10 sqrt(tol (1 + size))`, surfaces within `tol` at three boundary points in
  between; the tangent-curve point with the smallest deviation represents it); a
  traced curve removes the start points clustered with its ends before `FlushCurve`
  (`DoIntersection`); `TraceCurve` applies the already-traced test to snapped first
  steps (`span_on_found_curve`) and stops a near-tangent trace (normals within 1 degree)
  that passes a found curve's free end, shortening its last span to that end
  (`reaches_found_curve_end`). Files: `crates/solidcore/src/intersect/surface_intersector/p1.rs`,
  `p4.rs`, `p5.rs`.
- **Not ported:** the fork's follow-up tangency-labs/smlib@1499681 changes
  `SmTangentCurveTracer.cpp`, a tracer the fork added that v1.0.1 does not have.
- **Kind:** a result-changing fix (a wrong result, no crash, undefined behaviour, hang
  or nondeterminism), so it follows the opt-in rule; off by default the port matches
  the pinned C++.
- **Test:** `crates/solidcore/tests/issue_392_tangent_twice.rs` (`c2n_cylN_on_planeN`:
  two overlapping tangent curves with the flag off, one with it on).
