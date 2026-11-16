# curve.curve_p4 (`SmCurve`, part 4)

Difftest: 3259 cases (259 fixture + 3000 generated), all non-deferred cases bit-identical
(max_rel_err 0); 30,259-case run clean; `--perturb` SENSITIVE. Upstream behaviour worth
knowing:

- **`SmCurve::FixChainCurvesList` with `bDeleteDuplicates`** deletes a duplicate link
  (`delete pBSp1; pBSp1 = NULL;`, `SmCurve.cpp:946`, 968, 989, 1010) but
  leaves the dangling pointer in `r3DCurves[j]`: the next scan (`goto REDOLOOP`) and the
  outer chain-start loop dereference the freed curve (use after free), a later branch of
  the same scan can `delete` it again (double free: a reversed duplicate matches both the
  "end at end" and "end at start" tests), and a later branch can pick the `NULL` local as
  the next link (`pNextCrv = pBSp1`), which cases 2 / 4 dereference. Not difftestable: the
  generator drops every case whose Rust run deletes a duplicate. The port removes the
  duplicate from the list once, and a link choice that is the deleted curve is
  `InvalidInput`. Without duplicates (or with `bDeleteDuplicates = FALSE`) both sides
  agree.
- **`sm_FilterSolution`** passes `SmVector3d sPV1[2]` / `sPV2[2]` to `sm_UnitizeTangent`,
  which reads `rPVV[2]` when the (non-zero-replaced) first derivative is not longer than
  `SM_EFF_ZERO`: an out-of-bounds stack read. The port reads `Vec3::UNDEF` there; only a
  curve whose first and second derivatives both vanish reaches it.
- **`sm_GetProjDist`** (a `double` function) returns the failing `SmStatus` of
  `EvaluatePoint` through `SER` as the distance (e.g. `1001.0`). The port returns the
  error; the difftest maps it back to the status value.
- **`SmCurvePropertyEFO::Evaluate`**: `SM_CP_PROJECTED_POINT_*` read `m_cpOptVectors`
  without a `NULL` check, `SM_CP_DISTANCE_TO_POINT` / `SM_CP_CYLINDER_INTERSECTION`
  read `m_cpdPropertyValue` without one (port: `InvalidInput`; the ffi never sends
  these); `SM_CP_MAXIMIZE_RADIUS_OF_CURVATURE` with a zero curvature sets
  `rbFoundAnswer` and falls through into the division by the zero numerator (NaN `F'`,
  kept); `SM_CP_PLANE_CLASSIFY` has no case (`SM_ERR`). Values written before a failing
  `return SM_ERR` (e.g. `SM_CP_X_NORMAL`) are part of the comparison.
- `SmFindCurveDistanceExtremaEFO::Evaluate` was co-ported by `curve.curve_cache`; this
  unit only difftests it. `HasPlaneCrossings` with planes is deferred on
  `surface.surface_p8` (`SmSurface::GlobalCurveIntersect`); ellipse operands of
  `AnalyticCurveIntersect` defer on `curve.curve_p1` (`GlobalPointSolve`).
