# nurbs.b_spline_surface_n_lib_p2 (`Evaluate`, `TrimWithDomain`, `CreateSkinnedSurface`, `CreateCornerBlend`)

Partial, no disagreement (654 cases, 26 deferred; 20k-seed run clean). Upstream behaviour
worth knowing:

- **`TrimWithDomain` v snapping uses `idxMin` for both boundaries.** In the v loop the upper
  boundary branch writes `idxMin` (not `idxMax`), and `idxMax` stays `-1`, so
  `sm_CheckTrimBoundaries` never widens `vr`; the lower-boundary "previous knot" branch also
  leaves `idxMin` at `i`. The u loop's upper "slightly above the knot" branch does not set
  `idxMax`. Ported as is. Only the u boundaries are checked for being too close
  (`SM_ERR_INVALID_INPUT`); the v check is commented out upstream.
- **`CreateSkinnedSurface`** treats a skinning call as failed only when NLib returns exactly
  `NL_YES`; it retries without the knot vector, then (with derivative surfaces) without
  them, on the cross sections / rails / derivative surfaces the failed calls already changed
  in place. The knot vector is only used with derivative surfaces (it is reset to `NULL`
  otherwise). It casts away the rails' constness: two rails are reparametrised by
  `SyncronizeKnotsOfCurves` (the port takes them `&mut`). The spine branch evaluates
  `pRail1` (not the degree-elevated NLib copy) through the `SmBSplineCurve.h` overload, so
  `bNonZeroTangents = FALSE`.
- **`CreateCornerBlend`**: the `NER` check of missing UV curves / surfaces fires only at the
  entry where the null count first exceeds the limit (a four-sided G1 blend tolerates one
  missing pair, used as a degenerate triangle side). A surface without UV curve, a rational
  3D curve with a surface (`SmSurface::DropCurve` crashes on a standalone surface) and
  n-sided bilinear blends with missing UV curves / surfaces dereference `NULL` upstream; the
  shim rejects them, the port returns `InvalidInput`. `SM_OT_UNKNOWN` behaves as "not
  `SM_OT_SAME`" for four curves and as "not `SM_OT_OPPOSITE`" for more. G1 blends run
  `SmTangentField::Create` (its end-point checks are compared) and are then deferred on
  `SmSurface::GetNaturalUVDomain` (`surface.surface_p2`), `SmCrvOnSurf::ApproximateCurve`
  (`curve.crv_on_surf`) or, for n-sided blends, `SmSurface::EvaluateNormal`
  (`surface.surface_p7`).
- **`Evaluate`**'s `SmSurface::GetDerivativeLimit` (`10 * SM_EFF_ZERO * (1 + max|P|)`) is
  co-ported (`CO-PORT(surface.surface_p2)`). With fewer than two derivatives in either
  direction a zero tangent is fixed by recursing with `2, 2` upper-half (from the
  unclamped parameter); otherwise from `Suv` / `Suu` / `Svv`, then by sampling up to five
  nearby points for steps `1e-5 .. 0.1`; a surface collapsed to a point returns `SM_ERR`.
