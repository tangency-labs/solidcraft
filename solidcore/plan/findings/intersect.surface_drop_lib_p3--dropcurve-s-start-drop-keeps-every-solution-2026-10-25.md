# intersect.surface_drop_lib_p3: DropCurve's start drop keeps every solution (2026-10-25)

The SolidCraft case `a52-wire-harness/harness_bplus_planar_run` (a circle swept along a
5-segment planar path, `SmApiCreateSweepAlongPlanarPath`; runner step and fixture in #883)
deferred at `query.face_p2: SmFace::PointClassify`, reached from the sweep's internal
boolean: the cone/face intersect -> `AdvSurfaceIntersector::AddBoundaryCurve` ->
`DropAndTrimCurve` -> `DoTrace` -> `TraceCurve2` -> `SmSurface::DropCurve` ->
`GlobalPointSolve` on a face-owned surface whose caller passes neither a face test nor a
solve context.

`SmSurface::DropCurve` (`SmSurfaceDropLib.cpp:4310-4316`) sets the surface cache's
`m_bPointTestEnabled` to FALSE for its whole body, so its start-point `GlobalPointSolve`
(`:4389`) skips the fast drop's `pFace->PointClassify` trim test (`SmSurface.cpp:20069`).
`Surface::drop_curve_base_in` now calls `global_point_solve_point_test_off` when the
caller gives no face test or context, instead of the `global_point_solve` that stubbed the
trim test. With #883's step the case passes (volume 989.2761449134 vs 989.2758217705,
tolerance 0.01; 7/12/7 faces/edges/vertices): SolidCraft cases PASS 39 -> 40, FAIL 0,
ERROR 4 (the known 3665 cases). 23 touched units compared with C++: identical to main.

Not changed: the face-tested (#816) and context (#875) paths of the start drop still
classify on the face when its cache has point testing on; by the same C++ lines that test
is off inside `DropCurve`, so those paths may drop solutions upstream keeps. Unmeasured.
