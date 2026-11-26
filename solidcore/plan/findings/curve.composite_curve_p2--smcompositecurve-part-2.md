# curve.composite_curve_p2 (`SmCompositeCurve`, part 2)

- **`sm_CreateCorner` chamfer crash reaches the offsetters.** `CreateOffsetsOfManyCurves`
  (and `CreateTrimmedOffsets`) with `SM_OC_LINEAR_CHAMFER` segfault in upstream
  `sm_CreateCorner` (`sSolutions[0]` of an empty intersection array, see
  `curve.composite_curve_p1`) whenever the chamfer line misses an extension line, which
  random planar polygons hit often. The port returns `InvalidInput` there; the ffi sends
  the offsetters fillet and linear-extension corners only.
- **`sm_CheckBowTies` indexes `crOffsets` by the original curve count** (`crOffsets[0]`,
  `crOffsets[lNumOrig-1]`) and `crOrigCurves[0]` without checks; `IntersectAndTrimOffsets`
  ignores its status. Fewer raw offsets than original curves (degenerate offsets dropped
  with one-sided offsetting) read past the array. The port returns `InvalidInput` and the
  trimmer goes on without bow-tie points; the ffi requires `1 <= originals <= raws` for
  `IntersectAndTrimOffsets` (the shim's copy of `sm_CheckBowTies` returns
  `SM_ERR_INVALID_INPUT` at those points).
- **`SmCompositeCurve::ReadFromDB` ignores the segment statuses.** A segment without a
  curve, or whose curve fails to read, keeps a `NULL` curve (every later use
  dereferences it); the port returns `InvalidInput`, the shim reports such a composite as
  `SM_ERR_INVALID_INPUT`. An unreadable ASCII segment count leaves `SM_UNDEF_ULONG`
  (123456789) segments to `calloc` and read with uninitialised locals; the port rejects
  counts the remaining input cannot hold.
- **`MakeCompositeNurb` is `const` but `CreateByJoining` raises the segments' degree in
  place**; the port takes `&mut self` and writes the raised B-spline segments back
  (analytic segments derived from `SmBSplineCurve` join through a copy).
- **Deferred cases** (unit `curve.curve_p5`): most trimmed offsets reach
  `SmCCIntersectENFO::Evaluate` (curve/curve intersection), `SmCurve::IsArc` (offset
  approximation) or `SmCurve::LocalPropertyAnalysis` (`sm_FixBowTie`, classification).
