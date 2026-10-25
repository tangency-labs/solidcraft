# sweep.api_brep (`SmApiBrep.cpp` sweep entry points, PR #110, 2026-10-15)

- **Partial, no disagreements.** Every entry point validates and copies its inputs as
  upstream, then stubs the `SmPrimitiveCreation` sweep it wraps
  (`primitive.primitive_creation_p2` / `_p3`); the compared cases are the API-level
  rejections and copy failures.
- **Kernel crashes fenced off by the FFI** (`plan/upstream-issues.md`): UB-111 (`Draft`
  with no curves), UB-112 (`Draft` / `Taper` on a profile curve below about
  `2 * SM_ZONE_TOL_3D`), HANG-13 (negative-distance linear sweep of a full circle),
  UB-113 (`PlanarPath` with a Hermite curve in either list: the failed profile `Copy`
  and the unchecked `SmBSplineCurve*` cast), UB-114 (`CurveSweep` whose profile holds no
  B-spline curve reads `sNurbs[0]` of an empty list unless `bMoveProfileCenterToPath`).
  Each guard fires only where the kernel is reached: a list holding a `Null` or
  `Hermite` curve fails in `SmApiCopyCurvesForConsumingSweep` first, and a capped
  `LinearReps` with other than one repetition is refused by `CreateLinearSweep` with
  `SM_ERR` before it sweeps. The porters of `primitive.primitive_creation_p2` / `_p3`
  must return an error in those UB cases instead of crashing.
- **For the p3 porter:** `SmApiCreateCurveSweep` calls `CreateCurveSweep` with the
  default `bTestContinuity = TRUE`.
- **Review fixtures** (`fixtures/sweep.api_brep/review.json`, 132 cases): empty lists for
  plain `Linear` / `Rotational`, guard edges (`-0.0` distance, 359.9999 degree arc, `Null`
  / `Hermite` before a full circle or tiny curve, capped reps 0 / 2), and the 96
  `CurveSweep` profiles without a B-spline curve with `bMoveProfileCenterToPath` set (all
  other flags, scaled and unscaled), which run without crashing in C++.
