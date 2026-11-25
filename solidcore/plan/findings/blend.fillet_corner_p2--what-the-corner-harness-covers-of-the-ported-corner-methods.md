# blend.fillet_corner_p2: what the corner harness covers of the ported corner methods (PR #641)

- **Compared with values:** `SmFilletNx2Corner::AdjustConcaveCornerTopology` and
  `SmFilletNx2Corner::AdjustTopology` to the end (the new fillet vertices and edges, the
  deleted corner edge, mates, the extended side surface's registration is not exported);
  `MakeAnalyticCornerSurface` on sphere octants (the B-spline sphere of
  `CreateSphereFromArcs`), chamfer corners (an `SmPlane` or a bilinear patch, and a
  rail-rail arc recomputed as the corner plane's intersection with its original face),
  four-sided rail-rail corners (the B-spline surface of revolution) and its early
  `FALSE` / `TRUE` exits (WR-254), 120 built-in cases (half with real `SmLine` / `SmCircle`
  edges): 97 pass with values, 23 defer on
  `MakeConstantRadiusFillet` (unit `blend.fillet_corner_p1`, not ported).
- **Not compared (refused or not reached):** the corner harness's pseudo-B-rep fillet
  geometries have no fillet solver (`blend.fillet_geom_p1`'s model), and a solver is
  dereferenced by `SmFilletConvexNxNCorner::AdjustTopology` past its first cross section
  (`FindIndexOfRailOnFace`, `GetEdgeuse(0)`), by both tangent-corner splits
  (`SplitFilletGeomAtSideEdge`, then `ReCalcFilletGeom`) and by
  `MakeVariableRadiusFillet` past its first cross section (surface generator, centre
  line, the temporary `SmVariableRadiusFS`). Those cases are refused (`-2`). Their
  early exits run and match (the `NxN` loop's vertex-type `SER`, vertices off a face,
  geometries missing on both edge uses; the closed Nx1 test's evaluations, corner points
  and `LineClosestPoint`; the variable-radius fillet's count, type and `NER` returns).
  Covering the rest needs corners with real fillet solvers (a fillet executive run), so it
  waits for `blend.fillet_executive_p1`'s production adapter. The Nx2 tangent corner's code
  after `CalcCornerVertGeom` is ported for the same reason without a comparison; in Rust
  `CalcCornerVertGeom` itself still returns `Unported` (its
  `SmFilletVertex::CalcCornerVertGeom` stub, unit `blend.fillet_geom_p3`, in
  `blend/fillet_corner/p1.rs`).
- **Harness changes:** the corner shim appends the fillet vertices an op makes to the
  model's export in creation order (the Rust model lists every live vertex), exports the
  corner under test (`this`, stored by new vertices and edges) as corner id 1000000 on both
  sides, and appends the corner's surfaces to the record. To tell a default corner patch
  that reaches `MakeFaceBrep` (deferred) from one that returns a status before it, the
  pre-check runs `MakeAnalyticCornerSurface`'s fits (`sm_CreatePlanarSurface` and
  `sm_CreateSrfOfRevolution` through C++ wrappers in `blend_fillet_corner_p2_static.inc`,
  the translation unit that compiles `SmFilletCorner.cpp`; `CreateSphereFromArcs`; the
  chamfer edges' `sm_SrfSrfIntersection`) and the Coons patch's `CreateCornerBlend` on the
  same inputs, and deletes their results.
