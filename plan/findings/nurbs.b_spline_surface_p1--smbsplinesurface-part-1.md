# nurbs.b_spline_surface_p1 (SmBSplineSurface part 1)

No Rust-vs-C++ disagreement: 1985 of 1985 non-deferred difftest cases bit-identical (120
fixtures on the three `pt_TestFiles` `.sms` surfaces (also read back through `ReadFromDB`,
whose ASCII layout the `.sms` files share), the `smAdvancedTest.cpp` / `smTopoTest.cpp`
cone patches and bilinear patches, canonical, sweep and edge cases, plus 2000 generated).
135 cases are deferred: `surface.surface_p2` (`SmSurface::IsKindOf` for other types, which
`CalculateBoundingBox` and `IsParallelToVector` reach before anything else) and
`surface.surface_p1` (`CreateOffsetSurface`, `IsDegenerateCurve`, `STEPInversion`'s point
solves). Upstream behaviour worth knowing:

- The data constructor `SmBSplineSurface(n, m, p, q, r, s, Pw, U, V)` copies `V` with
  `smos_MemCpy(knv->U, V, (s+1) doubles, (r+1) doubles)`: the destination size is the *u*
  knot count, so when `s > r` the copy is refused (status only `SE`'d) and the v knots stay
  0 from `sm_AllocateNurbSurface`'s calloc, then `sm_FixupKnotVector` runs on zeros. The
  port reproduces this (caught by every generated `data` case with more v than u knots).
- `IsParallelToVector` returns `SER(sTestVec.Unitize())`'s `SM_ERR_INVALID_INPUT` as its
  `SmBoolean`, so a zero vector reads as TRUE; its line test calls
  `smgu_LinePointDistance(LinePnt, LineEnd, Pnt, d)`, which binds the (point, *direction*)
  overload, so the second net point is used as the line direction. Both kept.
- `CreateLinearSweep` passes the curve's own NURB to `N_CreateSrfExtrudeCrv`, which makes
  a 2D curve 3D in place (the `const` is cast away); difftest compares the curve after.
- `CalculateBoundingBox` only `SE`s (does not return) when the domain is outside the
  natural domain, and ignores `N_SrfIsFlatCheap`'s and `MakeUnitOrthoVectors`' status.
