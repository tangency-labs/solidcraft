# intersect.topology_solver_p2: face-owned surfaces reach their face's cache in line intersections and drop traces, and edge tolerances in curve/surface point classification (2026-10-25)

Updates: plan/findings.md, "## intersect.topology_solver_p2 static face solves: answering `GetNaturalUVDomain` exposes 9 mismatches (open, PR #599, 2026-10-23)"

PR #890 (`wire/solve-handle-domains`).

Closes the open entry "intersect.topology_solver_p2 static face solves: answering
`GetNaturalUVDomain` exposes 9 mismatches" (PR #599). Its suspected cause was right: the
9 cases disagreed because the Rust solves ran over a fresh plain cache of a standalone
plane, while C++ solves over the face's `SmTrimSrfCache` (`smsurf_GetSurfaceCache`).
PR #890 first fixed it with a `BrepFaceServices` face service in the static adapter;
#896 landed the same fix first (its `StaticScene` prepares the faces' trimmed caches,
`global_point_solve_0_in` and `face_silhouettes` take the caller's cache, and the global
solvers get `SolveOperands`, with #945 for `SmCSIGlobalSolver::IdenticalSolutions`), so
#890 keeps those versions (plan/findings/intersect.topology_solver_p2--global-solver-operands-and-corner-blend.md)
and adds the rest below.

- **`sm_ClassifyCurveSurfaceXSectPoint`** reads `pCurveOwner->GetTolerance()` of an
  edge's curve through the new `FaceQueries::curve_edge_tolerance` (answered by the
  intersector's face-cache context), in the CSI leaf solve and in
  `sm_GlobalCurveIntersectCombineSolutions`' coincidence test.
- **`SmBrep::DeleteTopologicalVertex`**: the coincidence check's curve handles carry the
  curves' natural intervals (`SmCurve::GetNaturalInterval`, 3 cases).
- **Line intersections and drop traces.** `GlobalLineIntersect`'s coincident drop gets
  the face's cache from the context when the caller holds none; the `SmPlane` override's
  base solve runs on the owning surface (upstream calls `SmSurface::GlobalLineIntersect`
  on `this`; the port ran it on an ownerless copy, so a face's trimmed cache could not
  build its tree); `sm_GlobalCurveIntersectCombineSolutions`' `DropAndTrimCurve` traces on
  the face's cache; `SmSurfaceDropCurve`'s `DropCurve` runs with point testing off, as
  `DoTrace` leaves the cache (`SmSurfaceDropCurve.cpp:1629`), so its start drop keeps the
  fast-drop solutions unclassified.

Since merging main (#945, #896 and the `faces_to_delete` LiveBrepObjects change): the 5
planar-path cases that deferred in `SmBrep::ProjectAndTrim`'s ray
classification (`planar_path_two_lines`, `gen0069`, `gen0071`, `gen0167`, `gen0184`) run:
`primitive.primitive_creation_p2` 300 pass, 0 fail, 0 deferred, 8 known mismatches.
