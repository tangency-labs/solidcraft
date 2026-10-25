# geom.pseudo_box (green; no disagreements)

3011 difftest cases (11 fixtures + 3000 generated) are bit-identical. Upstream
behaviour worth knowing:

- **`SmExtent3d::Circumscribe` of an axis-aligned pseudo box mixes up the axis map.**
  `IsAxisAligned` fills `alAxisMap[i]` with the axis that *basis* `i` is parallel to,
  but `Circumscribe` reads `alAxisMap[i]` as the basis parallel to *axis* `i`. Swaps and
  sign flips come out right, but a 3-cycle does not: basis `(y, z, x)` with intervals
  `[1,2], [3,5], [-7,-6]` gives `min (3, -7, 1)` instead of `(-7, 1, 3)`. Ported as is
  (fixture `circumscribe_axis_cycle_quirk`). `geom.extent3d` is green now that this is
  wired.
- **`DistanceToPoint`** returns `SM_ERR` (1001.0) as a distance when the point is outside
  more than three planes. That only happens for inside-out boxes (`Init()` state). When
  `smgu_IntersectTwoPlanes`/`ThreePlanes` fail it ignores the status and uses the
  uninitialised (`SM_UNDEF_DOUBLE`) line or vertex.
- **`ProjectToPlane`** projects the corners *parallel* onto the plane for every projection
  type (the perspective type only changes the basis vectors). It writes into a
  caller-provided box and keeps that box's basis on the rare `SetBasis` failure. The
  `MakeUnitOrthoVectors` branch writes `m_aBasis` directly without updating
  `m_bOrthogonalBasis`. When that branch fails (seen only for an uninitialised basis with
  huge intervals), basis vector 0 is left as `(0,0,1)`. All three are kept in
  `project_to_plane_into`.
- The `SM_IS_BETWEEN_TO_TOL(s, 0, 1, tol)` test in the rotation branch has an
  always-false reversed-interval half; the port keeps only the live half.
- **Wired `SmTransform::EvaluateBoundingBoxSimple`** (unit `geom.transform`). A requested
  output without `pOptParamPseudoBox` (an upstream null dereference) is `InvalidInput` in
  the port. The unaligned-and-bounded branch still needs `SmVolume::EvaluatePoint` and
  returns `Unported(surface.volume_p2)`. The 113 `geom.transform` cases deferred on
  `geom.pseudo_box` now pass. 256 cases are still deferred, on `surface.volume_p1`/`_p2`.
