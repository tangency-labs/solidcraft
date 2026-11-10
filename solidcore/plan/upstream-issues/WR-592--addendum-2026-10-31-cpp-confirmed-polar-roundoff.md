Updates: WR-592

#### WR-592 addendum (2026-10-31). Confirmed with the C++; the fixed result can still fail SmSurfOfRevolution #7 by roundoff
- **Severity:** Wrong result
- **Area:** Blend (fillet solver)
- **What changed:** **Confirmed:** yes. The pinned C++ build (`3fd5b95`) ran on the
  test's bar (`crates/solidcore/tests/wr592_torus_rail_on_axis.rs`, built with
  `SmApiCreateLinearSweep` and rounded with `SmApiFilletEdges`). It leaves the
  collapsed rail off the axis: the capi check sequence reports a `SmTrimmingTools #2`
  loop gap, and `AssertValid` reports `SmSurface #5`. On the fork's own input
  (`capi/tests/data/corner-after-round.txt` at tangency-labs/smlib@994f129) it reports
  `SmSurface #5` only. The pinned build with the 994f129 loop added makes the fork's
  input sound. On the port's rebuilt bar the same build still fails
  `SmSurfOfRevolution #7`: the corner surface's polar circle starts 1.19e-10 degrees
  past the sweep's 0, against a tolerance of 9.1e-11. The cause is roundoff between
  the surface frame's x axis and the NURB's u-min point, measured by
  `CreateIsoParametricCurve`. The Rust
  port gives the same values bit for bit with the flag off and on. Details:
  `plan/findings/blend.fillet_solver_p2--wr592-revolution-polar-start-angle.md`.
