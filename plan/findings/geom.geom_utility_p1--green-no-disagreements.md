# geom.geom_utility_p1 (green; no disagreements)

4185 default cases (185 fixture + 4000 generated) agree bit for bit with upstream
(`max_rel_err = 0`); `--perturb` catches all of them. Wiring the back-edges made
`geom.axis2_placement` (`AreCoaxial`; 4048 cases) and `geom.geom_utility_p2` (its private
stubs became calls into `p1`; 3270 cases) green with 0 deferred. `p2::PointSetType` is now a
re-export of `p1::PointSetType`. Upstream behaviour worth knowing:

- **Testing file-static helpers.** `sm_AddPoint` and `sm_AddClosestPoint` are `static` in
  `SmGeomUtility.cpp`. `harness/cpp/shim/geom_geom_utility_p1_static.cpp` compiles the
  upstream `.cpp` inside a namespace, so the shim can call them. It includes every header
  the file uses at global scope first, but not `SmGeomUtility.h`, which then lands in the
  namespace together with the definitions. That avoids clashes with the library's global
  `smgu_*` (ADL would make a global declaration ambiguous). A `using ::SmMatrix;` makes
  the header's `class SmMatrix;` forward declaration refer to the real class. Other units
  with static helpers can reuse the trick.
- **`smgu_BestFitLineToPoints` takes `aEigenVectors[2]` of `C = del*I - M`.** The code
  comment calls this the eigenvector of the largest eigenvalue, but the least-squares line
  direction minimises `v^T C v`. In practice the vector it returns depends on the Jacobi
  solver and the sort: for axis-aligned collinear points it is perpendicular to the points
  (so they classify as `SM_PST_SCATTERED`), while for points along `(1,1,1)` it is along
  the line (`SM_PST_LINEAR`). Ported as is. `smgu_BestFitPlaneToPoints` (p2) uses the
  same vector as a plane normal.
- **`smgu_CircleCenterFrom3Points` returns `FALSE` (0)** for (nearly) coincident points.
  0 is not a valid `SmStatus` and is not `SM_SUCCESS`, and the centre is left unset. The
  port returns `SolidError::Upstream { code: 0 }`.
- **`smgu_CircleFindSilhouettePoints`, perspective case**, returns the rotated unit
  directions times the radius, without adding the circle centre (the parallel case does
  add it). This looks like an upstream bug; ported as is.
- **`smgu_CircleFrom3Points`** returns `SM_ERR_WARNING` (1023) from
  `SmMatrix::SolveLinearSystem` when a pivot is near zero (`SER_MSG` passes any status that
  is not `SM_SUCCESS`), leaving every output unset.
- **`smgu_PlanePointDistance`** rejects a normal only when `|n|^2 - 1 > SM_EFF_ZERO`, so
  too-short normals are accepted.
- **`smgu_PolygonArea` with no points** reads `sPoints[-1]`. The port returns
  `InvalidInput`, and the ffi wrapper only passes at least one point.
- **`SmAxis2Placement::AreCoaxial`** takes `SmApproxTol3d`. The port uses `|tol3d|` but
  does not apply the `SM_USE_DEFAULT` substitution, because `are_coaxial` has no kernel.
