# heal.gap (`SmGap`, `SmGapArray`, `SmGapSample`, `SmGapFunction`, 2026-10-15; partial, no disagreements)

- **What is compared.** Gap functions (curve or surface to point, curve or surface)
  over real `SmBSplineCurve`, `SmHermiteCurve`, `SmLine`, `SmEllipse`, `SmCircle`,
  `SmCrvOnSurf` curves and `SmBSplineSurface`, `SmPlane`, `SmCurveBoundedSurface`
  surfaces: status, adjusted sample counts, sample state, extreme indices, all-normal
  flag, other-parameter ranges and every sample (drop type, both positions, tolerance
  point, parameters, tolerance, in-tolerance flag), through `GetMaxGapSample` and
  `GetSampleSet`, with and without stopping at the first big gap, after `SetSampleCnt` /
  `SetXSectTol3d`; single `Evaluate` calls with and without drop guesses;
  `sm_AddUniqueGetOrderedIndex`; `SmGapSample::SetCanonical`; `SmGapArray`; and the six
  gap records set on real B-reps (constructor = `Set`, every accessor,
  `GetGapEndPoints`, `CacheGap`). All bit-identical (max_rel_err 0).
- **Topology cases** build the B-rep exactly as the `topology.vertexuse` shim (the code is
  copied into `harness/cpp/shim/heal_gap.cpp`), and the Rust side imports the same
  export with that runner's importer (made `pub(crate)`). The importer rebuilds every
  face surface as a plain B-spline of its NURB (an `SmPlane` as a plane, since the
  topology.face_p2 merge) and every edge curve as a B-spline (or `SmLine`), so the shim
  replaces the other analytic face surfaces and non-line edge curves of the C++ B-rep by
  plain B-spline copies of their NURBs before the op (the originals are leaked). Without that, gaps between nearly coincident points disagreed at ULP level
  (e.g. `0` vs `8.9e-16` on `disk.smb`): the analytic and NURB evaluations differ in the
  last bits, which a distance of near-coincident points amplifies. Parameters are given
  as fractions of the selected edge's interval / the face surface's domain, resolved on
  the imported graph by both sides.
- **Deferred:** curve drops onto an `SmCrvOnSurf`
  (`SmCurveClassification` construction, `intersect.curve_class_p4`), and trim-curve gaps
  on edge uses without a UV trim curve (creating one in
  `SmEdgeuse::GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`; the existing-curve branch
  is read in place).
- **Back-edges wired:** `SmVertexEdgeGap::Set` / `SmVertexFaceGap::Set` in
  `topology::vertexuse` (`CalcVertexEdgeGap`, `CalcVertexFaceGap`) and
  `SmEdgeEdgeGap::SetEdgeEdgeGap` / `SmCrvSrfGapFunction` in `topology::edgeuse::p1`
  (`CalcEdgeCCWEdgeGap`, `CalcMaxEdgeFaceGap`, which now take the kernel). Their
  runners compare only the cache states of those gaps, so the gap values are compared
  here. `CalcMaxEdgeFaceGap` uses the context-less `SmTol::GetXSectTol3d(NULL)` (the
  records carry no context). PR #106 wires the surface drop for both gap functions.
- **Upstream bugs** ported as is: `plan/upstream-issues.md` WR-88 (non-B-spline `v`
  samples spaced by array index), WR-89 (B-spline spans walked from the first knot),
  WR-90 (`SmGapArray::Append` keeps the old longest gap), UB-104 (null dereferences and
  zero counts; the port returns `InvalidInput`).
- `SmBrep::ReadFromFile` crashes on `SmallBox.smb` (UB-1), so topology fixtures use the
  `topology.vertexuse` file list.

- Full-suite integration regressions exposed two existing dispatch/output gaps.
  `BSplineSurface::STEPInversion`'s guessed local solve now uses the original
  concrete surface (and caller kernel), retaining `SmPlane`'s virtual evaluator.
  `SmSurfaceEval` now retains the B-spline position/derivative matrix written
  before zero-tangent correction returns `SM_ERR`. C++ accessors ignore that
  failure and read those values; discarding them caused false convergence on
  collapsed surfaces. Native regressions cover the initial norms, next step,
  local driver and callers (`surface.surface_p4/p5`).
- `SurfPtSolveStepper::Normal` now ignores ordinary failed-normal status and
  reads the cached normal, like C++ (missing dependencies still propagate).
  A source audit also corrected `SurfPtGlobalSolver::Solve` to retain a failed
  `RefineGuessByGrid` status internally rather than returning it from `Solve`.
- The collapsed Normalize regression has no usable rough guess: both engines
  return `SM_ERR` with an empty solution array. The p3 and existing p4 runners
  now retain that array count on error, matching their C++ wire format; p3 also
  compares restoration of out-of-bounds evaluation on this failure path.
