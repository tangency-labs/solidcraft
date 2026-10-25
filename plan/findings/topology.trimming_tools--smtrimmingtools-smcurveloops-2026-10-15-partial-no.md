# topology.trimming_tools (`SmTrimmingTools`, `SmCurveLoops`, 2026-10-15; partial, no disagreements)

- **What is compared.** Geometry ops on real `SmBSplineCurve`, `SmLine`, `SmEllipse`
  (full ellipses as one-curve loops) and quadratic circle arcs, and on `SmBSplineSurface`
  (open, closed in u or v, pinched, C0 kinks, rational), `SmPlane` and
  `SmCurveBoundedSurface` (no boundary curves): `OrderCurvesIntoLoops` followed by
  `FindOuterLoops` (ordered indices, loop counts, orientations, plane point and normal,
  box, outer loop counts; the upstream tests `TestSmOrderCurves`,
  `TestSmCreatePlanarFaces`, `TestSmCreatePlanarCircle` re-expressed as data),
  `FindOuterLoops` on given loops, `MirrorCurve` (the resulting NURB), both `IsIsoCurve`s,
  `IsIsoCurveOnSeam`, `SurfaceContinuityCheck`, `sm_IsLoopUVGapWithinTolerance` (copied
  verbatim into the shim), `Min`, the null-region error of
  `TrimSurfaceWithModelSpaceCurves`, and scripts of `SmCurveLoops` /
  `SmIsoXSectSegment` / `SmCurveIntersections` operations (new iso-curve copies compared
  by their interval and five samples). All bit-identical (max_rel_err 0).
- **Conic UV trim curves** (review of PR #124). `SmEllipse`, `SmHyperbola` and `SmParabola`
  are `SmBSplineCurve`s: `MirrorCurve`, both `IsIsoCurve`s, `IsIsoCurveOnSeam` and the
  `EditEndPoint` repairs run the inherited `SmBSplineCurve` methods on them. The port
  first rejected them and the shim's guard hid `SmLine` / `SmEllipse` from the UV curve
  ops; `rv_ellipse.json` (ellipses incl. inside-out arcs and 1e-6 radii, lines) now
  passes bit-identical.
- **B-rep ops** use the `topology.vertexuse` B-reps (the 15 `.smb` files of that runner
  and generated faces) with the same geometry normalisation as `heal.gap` (analytic face
  surfaces and edge curves replaced by plain B-spline copies before the op):
  `AreUVCurvesPresent`, `GetEdgeUVExtremes`, `CheckLoop` (closure, status and the assert
  reports), `CheckFace` up to its first loop orientation, `FixTrimLoopTinyParameterSpaceGaps`,
  `EnsureLastEdgeuseUVCorrect`, `FixBackwardsOuterLoopOnSeam` (every edge use's UV trim
  curve compared afterwards). An optional pre-step mirrors one UV trim curve of the loop
  across the face surface's domain with `MirrorCurve` on both sides, so that loops get
  seam jumps and large UV gaps: it makes `FixBackwardsOuterLoopOnSeam` repair loops and
  `CheckLoop` fail with closure 2 or 1.
- **Shim guards.** The shim skips (reports `-2`) what upstream would dereference null:
  loops without loop use, edge uses with neither a UV trim curve nor vertex uses, faces
  without surface, and in `FixTrimLoopTinyParameterSpaceGaps` a gap to close at an edge
  use without UV trim curve (found by a dry run of the gap test; the edits of a pair
  touch only that pair's curve ends). `FindOuterLoops` reads past its arrays when a loop
  has zero area (UB-119): the shim detects the dropped curves and reports `-2`, the port
  returns `InvalidInput`.
- **`sm_IsLoopUVGapWithinTolerance` on a plane** extracts from `GetOrCreateGwNurbPointer`:
  the port uses `Plane::base_with_nurb` (a plane made by `Plane::from_normal` has no NURB
  until asked; with the raw base the certification was `false` where the C++ said
  `true`).
- **Deferred:** everything past `SmFace::ComputeLoopOrientation` (`topology.face_p3`):
  `CheckFace` after its first `CheckLoop`, `FixTrimLoopOrientation`, `FixTrimLoopOrder`
  (whose `SmEdge::GetUVTrimCurveOfSurface` also defers at
  `SmEdgeuse::GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`); `SplitAtSeams`,
  `ClearUVTrimCurves`, `CreateUVTrimCurves`; the trim of model-space curves
  (`SmBrep::SetTolerance`, `topology.brep_p2`); vertex-use UV points of edge uses without UV
  trim curve (`ComputeUVPoint`'s deferrals). Counts after merging main (PR #52,
  `surface.surface_p3`): 3263 cases, 2842 pass, 421 deferred (`topology.edgeuse_p3` 193,
  `topology.face_p3` 219, `topology.face_p4` 9); the 265 `CheckLoop` cases that deferred at
  `GetSingularities` now pass bit-identical.
- **Back-edges wired:** `SmSurface::CreatePlaneWith3DCurves` (`surface.surface_p2`) now
  calls the ported `OrderCurvesIntoLoops` / `FindOuterLoops` (it takes the kernel); its
  difftest's one `topology.trimming_tools` deferral passes.
- **Upstream bugs** ported as is: WR-96 (`GetLoopStartEnd` start index), WR-97
  (`IsIsoCurve` returns a failed `GetCanonical` as `TRUE`), WR-98 (`MirrorCurve` divides
  rational points by their weights twice); UB-119, UB-120 (null curves) are rejected.
