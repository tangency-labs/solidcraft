# surface.surface_p2 (`SmSurface` part 2: object, dispatch, `smsurf_*`, statics)

- **`smsurf_ComputeSurfacePointType` writes the v-side classification to the u-side
  output** (`if (pOptSurfSideDVV) *pOptSurfSideDUU = sSurfSideDVV`); the v output is never
  written and asking for the v side without the u side dereferences `NULL`. Ported as
  written (the v output is a flag); the ffi wrapper rejects v-without-u.
- **`sm_MergeDoubleArrays` stops when either array is exhausted**: the tail of the other
  array is dropped (the `while` needs both). Ported as written.
- **`SmDerivSurfDefinition::CreateCrossSectionCurves`** takes the Taylor third-derivative
  coefficient from the *second* derivatives (`sD = pOptStart2ndDerivs->GetAt(ii)`) when no
  end values are given. Ported as written.
- **`sm_SetUpBreaks`** reads `rBreaks[0]` of an empty array; `SmLSIGlobalSolver::
  BranchMayContainAnswers` reads an uninitialised hit count when `SmExtent3d::IntersectLine`
  fails (zero line vector). The port returns `InvalidInput` / the line error.
- **The `Surface` type** (`surface::surface::p2::Surface`, `SurfaceKind`) is introduced here
  with the B-spline as its only kind; later surface units add their kinds. Code generic over
  a surface (tangent fields, `SurfaceMaxDistanceBetween`, `SetIntersectionCurveThroughPoint`)
  now takes any `SurfaceEvaluate` (implemented by `Surface` and `BSplineSurface`). Solution
  objects are opaque handles, so `geom.solution_array`'s surface evaluations keep their
  `UNPORTED(surface.surface_p2)` markers until `SolutionObject` can reach a surface.
- **`smsurf_GetSurfaceCache` / `sm_GetOriginalSurface`** need the surface caches (no
  `SmCacheMgr` surface cache exists yet): `Unported(surface.surface_cache_p1)`; `geom.grid`'s
  `ExpandHierarchy` marker moved there.
