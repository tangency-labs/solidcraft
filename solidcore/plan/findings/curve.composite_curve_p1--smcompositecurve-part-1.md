# curve.composite_curve_p1 (`SmCompositeCurve`, part 1)

- **`GetKnots` reads past the knot array.** After the first segment, `lLastIndex =
  rKnots.GetSize()` (not `GetSize() - 1`), so the first transition reads
  `rKnots.GetAt(GetSize())` and writes `pKnotMultiplicities->SetAt(GetSize(), ..)`, one
  past the end (`SM_ASSERT_BREAK` is compiled out): any composite of two or more segments.
  With `pOptIvl` the trim loop `for(ii=GetSize()-1; ii>=0; ii--)` runs on an unsigned
  `ULONG` and never ends (it reads `rKnots[ULONG_MAX]`). The port returns `InvalidInput`
  in both cases (after the segment `GetKnots` calls upstream makes first); difftest only
  sends composites of at most one segment, without an interval.
- **`ApproximateCurve` dereferences `NULL`** when `CreateByJoining` fails before making a
  curve (unconnected segments, no segments): `rpNewBSplineCurve->AssertValid()`. The port
  returns the join's error; the ffi only sends connected 3D B-spline / Hermite chains.
  `AssertValid` of the joined curve reaches `GlobalPropertyAnalysis`
  (`LocalPropertyAnalysis`, unit `curve.curve_p5`) for every non-degenerate, non-analytic
  join, so these cases are deferred.
- **`CalculateBoundingBox` shifts the segment intervals from `0`**, not from the first
  segment's start (as `GetNaturalInterval` does), and passes the *shifted* interval to the
  segment's own `CalculateBoundingBox` (the B-spline override clamps it to its range).
  Ported as written. `*pBBox` is written unconditionally (`NULL` crashes); the port always
  computes the axis aligned box and rejects a request without it.
- **`sm_CreateCorner` (chamfer) reads `sSolutions[0]`** of an empty intersection array
  when the chamfer line misses an extension line. The port returns `InvalidInput`; the
  shim copy of the static returns `SM_ERR_INVALID_INPUT` at that point (its only change).
- **Copy constructor** ignores each segment's `Copy` status and leaves a `NULL` segment
  curve (Hermite curves have no `Copy`), which later calls dereference; the port returns
  the error, the ffi sends no Hermite segments to `Copy`.
- **`sm_FlushCrv`** is listed as unused by the call graph (another file has a static
  function of the same name) but is reached from `BuildCompositesFromCurves`; it is ported
  in this unit.
