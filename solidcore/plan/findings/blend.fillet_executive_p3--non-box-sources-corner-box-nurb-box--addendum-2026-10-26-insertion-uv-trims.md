Updates: plan/findings/blend.fillet_executive_p3--non-box-sources-corner-box-nurb-box.md

# Addendum (2026-10-26): four cases fixed; corner_box edges 9/13 diverge in InsertIntersectionTopology

Fixed (now pass at 1e-9): corner_box every edge, nurb_box every edge, nurb_box edges 0 and 5.
Causes, all in the fillet solver path:

1. `is_bspline_surface` (`blend/fillet_solver/p4.rs`) now accepts every kind derived from
   `SmBSplineSurface` (SurfOfExtrusion, Cone/Cylinder, Sphere, Torus as well).
2. `LoadInitialValues` under the trace never saw the target B-rep: `StepSolve`,
   `ComputePointValues` and `PointOnPlaneSolve` ran with `brep = None`, so a solver with
   edge uses failed with "edge uses without their B-rep" and the traced fillet had no
   surface. Upstream reaches the edge uses through its `GetEdgeuse` pointers; the port
   now passes the target B-rep through `IntersectorSolver`, the p4 `StepSolve` /
   `ComputePointValues` wrappers, `sm_FindFilletTsectPnt` and the
   `FilletSolverOps::point_on_plane_solve` virtual.
3. The executive's constant-radius solver behaved as an `SmConstantRadiusAssistedFS`
   (`GetSolverType` = `SM_FS_CONST_RADIUS_ASSISTED`, the assisted `PointOnPlaneSolve`),
   so `StepSolve` ran the assisted plane refinement (SmFilletSolver.cpp:5355). Upstream
   `SetFilletParameters` (SmFilletExecutive.cpp:805) builds a plain `SmConstantRadiusFS`.
   `ConstantRadiusAssistedFs::plain` now selects `SmConstantRadiusFS`'s `GetType`,
   `IsKindOf`, `GetSolverType` and the base `PointOnPlaneSolve`; the session sets it.

Still disagreeing (kept in known_mismatches): corner_box edge 9 (area 364.0502109116303
C++ vs 364.05026992193945 Rust) and edge 13 (364.2382213623975 vs 364.2370067454). Every
executive checkpoint up to MergedFilletFaces agrees except the target body from
InsertedIntersectionTopology (checkpoint 16) on: two target edges' UV trim curves differ
by ~1e-8 in their interior (end points identical; e.g. edge-9 case, a curve sample
0.9461316620477862 C++ vs 0.9461316463673117 Rust). In the port these curves are created
lazily by `get_or_create_uv_trim_curve` (`CreateUVTrimCurve` -> `DropCurve` at the edge
tolerance) from `classify_face_point` (`sm_ClassifyFacePoint` ->
`CalculateUVDomainFromUVTrimCurves`) in `UpdateEdgeSplitFace` / `UpdateGeomSplitFace`
during `InsertIntersectionTopology`, which is also where upstream creates them. Suspected
cause: a different input to that DropCurve (the 3D edge curve's parameterisation/knots,
the face domain, or the face surface after the intersection split), not the approximation
itself; not localised further. The unfilleted bodies (import) agree exactly, UV trims
included.
