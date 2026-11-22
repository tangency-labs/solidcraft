# surface.surface_p6 (`SmSurface`, part 6; partial, no disagreements)

- **Coverage.** Every non-deferred case agrees bit for bit (`max_rel_err 0`). Most of the
  unit sits behind `surface.surface_p4`/`_p5`: `GetSampleValues` defers at `DropPoint`
  once its input checks pass, `LocalCurveIntersect` always defers
  (`SmCSIntersectENFO::Evaluate`), `SimpleCoincidenceChecker` compares only disjoint boxes
  (`GlobalPointSolve` next), `ClassifyIntersectingRangeSolution` only point solutions
  (`LocalPointSolve` next), `SmSSGlobalSolver::IdenticalSolutions` defers at the
  singularity test of non-plane surfaces, and `sm_SCLocalSolve` compares the boundary
  curve/curve solves with `bSkipNormalize` (the point solves and the curve/surface solves
  defer). `CalculateContinuities` and `CreateNaturalUVTrimCurves` compare fully on
  B-spline, plane and revolution surfaces (also true cones with their apex on the axis).
- **`CreateIsoParametricCurve` dispatch.** The base body is `surface.surface_p5`; the
  B-spline, plane (`SmLine`) and revolution (`SmCircle`) overrides are ported and are what
  both functions call, so the iso curves keep their own `IsDegenerate`, `GetKnots` and
  `CalculateContinuities`.
- **`sm_SCLocalSolve` shim.** The verbatim copy takes the evaluated surface, the patch
  surface and the node boxes instead of the tree nodes. PR #106 replaces its former
  `-2` stop marker with the real `sm_SpLocalSolve` reference shared with the p5 shim.
- **Upstream quirks kept.** `SmSSGlobalSolver::IdenticalSolutions` reads the surface
  parameters from `m_vStart[1..4]` (the solutions hold them in `[0..3]`);
  `CalculateContinuities` keeps the knots of a replaced degenerate iso curve;
  `GetSampleValues`' degenerate-tangent test scales `SM_EFF_ZERO` by `1.0 *` (not `1.0 +`)
  the position's largest coordinate.
- **Upstream crash.** UB-100: `GetSampleValues` with `SM_TT_DIR_TO_REFERENCE_CURVE` and no
  reference curve dereferences `NULL` (found by a generated case); the shim refuses it.
- **Wiring.** Surface caches now build (their tree needs `CalculateContinuities`), so
  `intersect.surface_tracer`'s runner runs `DoTrace` on the cache instead of a placeholder
  harness error, and `curve.iso_curve` has no deferred case left (green). The two
  remaining `UNPORTED(surface.surface_p6)` markers in `surface_cache::p1` and
  `surface::p7::is_planar` stood for the base `CalculateBoundingBox` of curve-bounded and
  STEP surfaces, which needs a kernel those signatures do not take: now DISPATCH GAP notes
  naming `surface.surface_p1`. Once the kernel is plumbed, the next blocker there is
  `surface.surface_p5`: the cache build's `CalculateContinuities` calls the base
  `CreateIsoParametricCurve` for these kinds (the comments at both sites say so).
- **Review fixes (PR #92, 2026-10-15).**
  - *FFI guard on revolutions.* The guard refused any generator with an endpoint within
    `1e-3` of the axis, which kept true cones (apex on the axis) away from the C++, and with
    them the degenerate-side branches of `CreateNaturalUVTrimCurves` (remove degenerate),
    `CalculateContinuities` (degenerate retry) and the revolution's degenerate-circle iso
    curve. It now refuses only a generator with both ends on the axis (no surface swept).
    The C++ neither crashes nor disagrees on the apex cases: 12 cone-apex
    `cont`/`trim` fixtures (also with the generator reversed) are bit-identical, and a
    reviewer's 4000-seed run of every op with apex generators enabled gave 4107 cases, 0
    failed. Even a generator lying entirely on the axis did not crash the C++, but it sweeps
    no surface, so it stays refused.
  - *`sm_SCLocalSolve` iso-curve dispatch gap.* `boundary_iso_curve` (`surface::surface::p6`)
    builds `SmIsoCurve` only over a B-spline; for plane, revolution, curve-bounded and STEP
    surfaces it returns `Unported` naming `curve.iso_curve` (green), because the ported
    `IsoCurve` holds an `SmBSplineSurface` and would drop the kind's evaluation overrides.
    The FFI `scls` guard ("this op needs a B-spline surface") keeps every non-B-spline
    surface from the C++, so the gap is never exercised. A plane with `process = false` or
    `skip = true` would already be comparable and is still refused. Whoever generalises
    `IsoCurve` over `Surface` should wire this site and relax the guard.
  - *`sc_local_solve` result.* It builds a fresh `Solution` from `LocalCurveSolve`'s result
    instead of copying upstream's whole `sSolution` (end values, objects). The runner
    compares type, `nvars`, value and t/u/v only, and the path defers today
    (`surface.surface_p4`): recheck it when p4 lands.
  - *`surface.plane_p1` (pre-existing, not this unit).* `SmPlane.cpp:596/600` ignores the
    status of `CreateNaturalUVTrimCurves` and goes on with whatever curves were appended;
    `plane::p1` propagates it with `?`. This differs only on SM errors (e.g. a degenerate
    line), not on `Unported`; a faithful fix needs the partial curve lists on error.
