# surface.surface_cache_p3 (`SmSurfaceCache`, part 3: tree build and subdivision, 2026-10-14)

- **Patch boxes of ownerless surfaces.** The construction Bezier patches get their
  boxes from `SmBSplineSurface(pBezier, TRUE, ..)` surfaces that have no face or brep,
  so `CalculateBoundingBox` expands them by `SM_ZONE_TOL_3D`. The B-spline port cannot
  ask `SmSurface::GetFace` (unported) and would defer every case; p3 computes the boxes
  unexpanded and expands them by `ZONE_TOL_3D` itself (`patch_boxes`). When
  `surface.surface_p1` ports `GetFace`, this can go back to the plain call.
- **Planes are B-splines.** `IsKindOf(SmBSplineSurface_TYPE)` holds for `SmPlane`;
  `BuildTreeBase` takes the plane's NURB (`Plane::base_with_nurb`), and `bspline_of`
  (part 1) answers the kind test for both. Treating planes as non-B-spline gave
  bicubic approximations where upstream splits the plane's own NURB.
- **Shim crash.** `SubdivideToTolerances` on an aux-data start node without a surface
  dereferences a null `mBA_pSurface` in `TestAgainstTolerances` (segfault); the FFI
  rejects such start nodes (aux type 6 needs the surface flag and a B-spline).
- **Recursion.** `SubdivideToTolerances` recurses into both children without a bound;
  the port stops past `MAX_SUBDIVISION_DEPTH` (512) with `InvalidInput`. Generated
  tolerances are size-relative (chord >= 1e-3 size, side >= 1e-2 size, angle >= 0.05)
  so the C++ side terminates too.
- **`BuildTree` flags.** `BuildTree` turns point testing off and marks face
  containment as under construction for the build (against recursion back into
  `BuildTree`), restoring both afterwards; `build_tree` does the same around its body.
- **Wired back-edges:** `trim_srf_cache` (`BuildTree`, `FindUVNodes`, `SubdivideNode`)
  and part 1's `GetTree` (the `existing_tree` stand-in is gone; `mark_face_containment`
  calls `get_tree`).
- **Not wired (needs a design):** `geom.tree` `GetFirstBezierPatch`
  (`SmBezierPatch::GetBezierPtr`, `SmBSplineSurface::GetGwNurbPointer`), `geom.grid`
  `SmSurfaceGridElement::Init` (`GetPseudoBox`) and `surface.surface_p2`
  (`GetPolarBox`) read patch data from a bare `Tree`, but the patches live in the
  surface cache (`BezierPatchRef` is an empty enum). A hand-over like the curve cache's
  `set_curve_data` would close them. The callees are ported (`surface.surface_cache_p1`,
  `nurbs.b_spline_surface_p1`), so the call sites carry `DISPATCH GAP` comments and their
  `Unported` errors name those owning units (review of PR #39).
- **Boundary edge uses on a split (review of PR #39).** `SubdivideNode` moves a boundary
  parent's edge uses into the children through the virtual `ImplantEdgeuse`; the port
  called the base class's (always `SM_ERR`, ignored) from the trimmed surface cache too,
  so the edge uses were dropped. The trim runner printed only the input nodes, so CI did
  not see it. Now `SurfaceCache::subdivide_node_split` does the split and
  `TrimSrfCache` re-implants each edge use with its own `implant_edgeuse` (into one
  child, no subdivision); that needs `SmEdgeuse::GetUVTrimCurve`, so those cases defer
  on `topology.edgeuse_p1` (poly edges on `query.poly`). The trim runner prints every
  node of the tree; fixture `implant_then_split_boundary_leaf` covers it.
- **Build/Get not compared yet.** Every `Build` / `Get` case defers on
  `CalculateContinuities` (`surface.surface_p6`), so the first-generation node loop of
  `BuildTreeBase`, the `BuildTree` cleanup and `GetTree` have no C++ comparison until
  that unit lands.
- **Dispatch gaps.** `GetTree` dispatches `BuildTree()` virtually (the trimmed surface
  cache's on an `SmTrimSrfCache`); `SurfaceCache::get_tree` builds the base tree, and
  `subdivide_node` does not repeat upstream's `GetTree()` call (it needs a built tree).
- **Planes in the tolerance test.** An `SM_AD_AUX_DATA` node holding a plane reads the
  plane's NURB (`GetGwNurbPointer` on an `SmBSplineSurface`); the port's plane makes its
  NURB lazily, so the test uses `Plane::base_for_query`. Found once the generator put
  planes on aux start nodes (the FFI had rejected them; upstream handles them).
