# sweep.rotational_sweep_geometry (`SmRotationalSweepGeometry`, 2026-10-15; partial, no disagreements)

- **Harness.** Edges are wire edges of a real `SmBrep` (`MakeShellVertex` + `MakeWireEdgeVertex`,
  or `MakeWireEdge` to the same vertex for a closed edge) on real `SmBSplineCurve`s, B-spline and
  analytic `SmLine` segments, `SmCircle`s, `SmEllipse`s and `SmHermiteCurve`s, with set edge,
  vertex and B-rep tolerances; the Rust side builds the same records (the shim reports the start
  / primary vertex, closed flag, points and tolerances, which are compared too). Results are
  compared as type codes, natural intervals / domains and evaluations at fixed fractions, so
  B-spline and analytic results are both covered. Edge intervals are in the curve's natural
  (NURB) interval: `[0, 1]` for circles and ellipses, not degrees.
- **Out-of-range read** in `sm_CheckEdgeOnAxis` (`sSolutions[1]` with one solution): the port
  reads a zero-filled slot; upstream-issues UB-130. The C++ agrees on every case reaching it.
- **`SM_SWEEP_CHECKS` is not defined**, so `CheckCurveSweep` only detects rotation about an
  interior point of the curve, and only through `IntersectWithLine` (no solutions for a general
  B-spline, which upstream then sweeps even when it crosses the axis); the port does the same.
- **`SmLine` generators**: `CreateSurfOfRevolution` reparametrises the sweep circle with arc
  length for a line parallel to the axis (`GetType() == SmLine_TYPE`); the port passes the line
  vector (`create_surf_of_revolution_with_line_vector`). Without it the cylinder cases disagreed
  in the circular parameter range only.
- **Deferred**: `CopyAndAddAnalytics` of a non-planar swept B-spline surface or face surface
  reaches `SmCone::IsNurbSurfaceCone` (unit `surface.cone_p2`).
