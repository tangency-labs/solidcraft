# surface.surface_p11 (`LocalSurfaceIntersect`, `SmCSGlobalSolver` / `SmSSGlobalSolver::LocalSolve`, `IntersectSurfaceBoundaries`, `LiftCurve`, PR #120, 2026-10-15; partial)

- **Known disagreement (not generated): boundaries of an `SmPlane`.** In
  `IntersectSurfaceBoundaries` the other surface's boundary curves come from
  `CreateIsoBoundaries`; for an `SmPlane` upstream they are `SmLine`s (the plane's
  `CreateIsoParametricCurve` override), while the port's `Surface::create_iso_boundaries`
  (unit `surface.surface_p9`) returns `BSplineCurve`s. Copy, trim and evaluation then run
  as B-splines: plane/plane cases differ near zero coordinates (e.g. `cpp=0`,
  `rust=2.78e-17`, relative error 1; four of the first 600 generated cases with a plane as
  the other surface). The generator therefore uses patches for the other surface (this
  surface may be a plane). Fix belongs to `surface.surface_p9`: return the typed iso curve
  (`SmLine` / `SmCircle`) from `create_iso_boundaries`.
- **Global solves wired.** `Surface::global_curve_solve` / `global_surface_solve` (unit
  `surface.surface_p7`) now take the kernel and the solution handles, build the surface
  (and curve) caches for the solve (no cache store: `CacheMakeOrValidate`, unit
  `surface.surface_p1`) and run `SmCSGlobalSolver` / `SmSSGlobalSolver` with the leaf
  solves of this unit; the solvers borrow the caches (`CsGlobalSolver::curve` /
  `surface_cache`, `SsGlobalSolver::caches`), as `SmLSIGlobalSolver` does.
  `SmSSGlobalSolver::BranchMayContainAnswers` uses the caches' polar boxes when it has
  them. `global_surface_solve_with` also returns the solution nodes' boxes and patch
  domains (`SM_SR_NODES`, read by `SmSurfaceIntersector::FindInteriorCurves`, now wired).
- **Unported callees** (deferred): `sm_IntersectSrfBndryCrv` and `sm_ExtractIsoCurve`
  (`surface.surface_p8`; `LiftCurve` defers at its first call) and `LocalPointSolve`
  through `DropPoint` of a B-spline in `IntersectSurfaceBoundaries`
  (`surface.surface_point_solve_p3`). `SmSurface::LocalSurfaceSolve` is wired to unit
  `surface.surface_p3` (merged).
- **Dispatch gap: `LiftCurve` on a surface that is not a B-spline.** `SmCrvOnSurf` holds
  any `SmSurface` upstream; the ported `CrvOnSurf` (unit `curve.crv_on_surf`) holds an
  `SmBSplineSurface`, so `LiftCurve` of a plane, surface of revolution, curve-bounded,
  in-volume or STEP surface returns `Unported` naming `curve.crv_on_surf` past the iso
  curve test (as the `SmIsoCurve` gaps of parts 5, 6 and 9). Unreachable in the difftest
  until `sm_ExtractIsoCurve` (`surface.surface_p8`) lands.
- **Unbounded surface cache subdivision (fixed).** A difftest run of this unit with
  `surface.surface_p7` reached 3.5 GB RSS within 84 s and aborted ("memory allocation of
  3523215360 bytes failed": the `BezierPatch` array of a `SurfaceCache` growing past 2^21
  patches in `SubdivideToTolerances`). The `surface.surface_p7` generator ran the Rust
  side of each candidate case (now a real global solve) before its scale filter, so
  extreme-scale surfaces were subdivided to the default tolerances into millions of
  patches (upstream does the same, for minutes). Fixes: the generator checks both
  surfaces' scales before the Rust run, and `SubdivideToTolerances` (unit
  `surface.surface_cache_p3`) refuses to split once the tree holds
  `MAX_SUBDIVISION_NODES` (2^20) nodes, with `InvalidInput` (a generated
  `surface.surface_p1` `GetDomainWithContinuity` case at `1e-4` scale needs more than
  2^18; no fixture comes near 2^20). Peak RSS of the `surface.surface_p7` run is now
  about 80 MB.
- **Global solve comparison.** The `surface.surface_p7` runner's `GlobalCurveSolve` and
  `GlobalSurfaceSolve` cases compare the first 16 solutions (type, value, variables, end
  parameters of a range), not only their count. Its generator covers minimize, maximize,
  normalize and signed directed minimize curve solves and generated surface/surface solves
  (a bumpy patch lifted 0.05..2 above a B-spline surface's control hull, so minimizing
  never meets a continuum of zero distances; intersect for the `SM_SR_NODES` node pairs on
  crossing surfaces). `SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE` is not generated: it is not one
  of the operations `sm_SCLocalSolve` accepts (its `SM_ASSERT`; the Rust side returns
  `InvalidInput`, the release C++ carries on), and a trial of the generator with it gave
  13 of 3660 `surface.surface_p7` cases that disagreed for exactly that reason.
- **Upstream issues:** UB-121 (`IntersectSurfaceBoundaries` dereferences a NULL curve
  array), WR-99 (`LocalSurfaceIntersect` boundary retries: `!bFoundSolution` binds to
  the minimum test only; reproduced).


### PR #131 after #120 merged (official main `0d6885b2`)

Merged main and preserved both findings sections. `Surface::intersect_surface_boundaries`
is now available, but it accepts `Surface` operands and main still lacks
`SurfaceKind::Cone`. Wiring via the B-spline/revolution base would discard native
Cone/Cylinder virtual dispatch. The remaining adapter belongs to active issue #76 /
PR #133; no part of that claim is duplicated here. The cone boundary stub and its
30 report deferrals now name `surface.surface_p2`, not the already-ported p11 unit.
The p11 findings above also record a typed iso-boundary limitation; reassess that
coverage when the cone adapter is wired rather than certifying it in advance.

Fresh merged-tree checks: 1194 kernel tests, formatting, strict plain workspace
Clippy and wasm32 pass. Native comparison gives:

```
PARTIAL surface.cone_p3 cases=1308 passed=1187 failed=0 deferred=121 max_rel_err=0e0 blocked_by=surface.cone_p2(91),surface.surface_p2(30)
PARTIAL surface.surface_p2 cases=869 passed=858 failed=0 deferred=11 max_rel_err=0e0 blocked_by=surface.cone_p2(10),surface.surface_p8(1)
PARTIAL surface.surface_p11 cases=600 passed=498 failed=0 deferred=102 max_rel_err=0e0 blocked_by=surface.surface_p8(102)
PARTIAL topology.trimming_tools cases=3263 passed=2842 failed=0 deferred=421 max_rel_err=0e0 blocked_by=topology.edgeuse_p3(193),topology.face_p3(219),topology.face_p4(9)
SENSITIVE surface.cone_p3 perturbed=1187 detected=1187 unperturbed=0 deferred=121
PARTIAL surface.cone_p3 cases=10108 passed=9048 failed=0 deferred=1060 max_rel_err=0e0 blocked_by=surface.cone_p2(803),surface.surface_p2(257)
```

Only the 30 cone dependency records and their blocker summary change; compared
values and all three other reports reproduce committed bytes. No deferred path
is counted as a pass. Native validation used the separate disposable Mac checkout
and the excluded platform prerequisites. The first runner build exhausted local
disk space before comparisons ran. After clearing this thread's old build cache,
the retry used `--config 'profile.difftest.package.difftest.opt-level=0'` for the
runner only; kernel and C++ build settings are unchanged. No configuration change
is submitted. Full clean Linux/GNU certification remains outstanding; the known
independent Apple libc++ database abort was not rerun.

Strict C++-feature workspace Clippy also passes in the same disposable checkout
with the runner-only profile override. No platform prerequisites enter this PR.
