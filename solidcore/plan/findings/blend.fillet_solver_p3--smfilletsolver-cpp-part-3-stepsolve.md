# blend.fillet_solver_p3 (`SmFilletSolver.cpp` part 3: `StepSolve`, `SplitAndTrimFilletSurface`, `ProcessFullRollOver`, ...; PR #236, 2026-10-18; partial, no disagreements)

Built on the open dependency PRs #147 (`blend.fillet_geom_p1`, head `b396cdd3`) and #192
(`blend.fillet_solver_p1`, head `a88c62af`), merged into the branch; #192 carries its own
reconciliation with main's `surface.offset_surface`. With both, the affected difftests have
0 failures (`fillet_solver_p1` 303 passed / 40 deferred, `fillet_geom_p1` 446/21,
`fillet_custom_solver` 401/47).

- **`StepSolve` difftest.** Every library solver belongs to an unported unit, so the shim
  derives a deterministic `SmFilletSolver` (the offset/offset intersection as
  `LoadJacobian`, a configurable `GetSolverType`, `SetupOffsetValues` from a radius, an
  optional extra unknown, a failing `LoadInitialValues`), mirrored by the runner. Cases step
  along floor/wall fillets (20 to 160 degrees, scales `1e-3`..`1e3`, flat and bumped
  B-splines) inside the domains, across boundaries (iso-curve intersection, clipped radius
  solve of a constant-distance solver), from boundaries, with zero/tiny/reversed steps,
  extended domains and offsets on the wrong side. The constant-radius-assisted clipped
  branch defers at `PointOnPlaneSolve` (`blend.fillet_solver_p2`). The variable-radius
  branch casts to `SmVariableRadiusFS`: the FFI guard rejects that solver type and the
  Rust side returns `Unported(blend.fillet_standard_solver_p1)`.
- **New virtuals.** `FilletSolverOps` gains `solver_type` (`GetSolverType`, pure virtual
  upstream; default `SM_FS_UNKNOWN`, the custom solvers' `SmConstantRadiusAssistedFS`
  override is `Unported(blend.fillet_standard_solver_p1)`) and `constant_distance` (the
  `SmConstantDistanceFS::GetDistance` read of `SmFindClippedRadiusEFO`, default `Unported`).
  `FilletBrep::set_vertex_point` (`SmVertex::SetPoint` on a fillet vertex) is public.
- **Kernel-tested only.** `GetOtherFilletCorner` (needs `GetFilletCorner`, p2),
  `sm_FindFilletTsectPnt` (defers at `PointOnPlaneSolve` after both drops),
  `SplitAndTrimFilletSurface` (no-overlap deletion compares; an overlap reaches
  `SplitFilletGeom`, whose geometry calls are p1 stubs), `sm_RefineRailCurveClassification`
  and `sm_CalcRailXEdge` (defer at `RailEdgeuseIntersect`, p2) and `ProcessFullRollOver`
  (defers at `sm_SplitRail`'s fillet geometry calls, then `CreateCliffRollOverSideFG`, p2,
  and `TrimCliffRails`, `blend.fillet_geom_p5`; `MakeFilletEdge` is wired to #264's
  `FilletBrep::make_fillet_edge`). A native fixture for them needs the
  executive's pseudo B-rep and real fillet geometries; once p2 lands they should get
  difftest cases. Upstream index defects found while porting: UB-248, UB-249.
- **For #208 (`blend.fillet_solver_p5`).** Its `ProcessRollOver` calls
  `is_convex_radial_sector(.., 10, true, 0.0)`; upstream calls `IsConvexRadialSector(10)`,
  whose `bOptStepOff` defaults to `FALSE` (`SmEdgeuse.h:356`, `SmFilletSolver.cpp:7260`).
- **`blend.fillet_geom_p5` back-edges (after #264 merged, 2026-10-21).**
  `ProcessFullRollOver`'s `SmFilletEdge::MakeFilletEdge` calls #264's
  `FilletBrep::make_fillet_edge` (start `pEndVert`, end `pNewV`, no corner), and the
  `SetFilletEdgeType(SM_FE_ON_EXTEND_EDGE)` after it, unreachable behind the stub before,
  is ported. `SmFilletGeom::TrimCliffRails` stays a justified stub:
  `FilletBrep::trim_cliff_rails` finds the geometry's solver by its `FilletSolverId` in a
  solver table (`&[FilletSolver]`), while `ProcessFullRollOver` holds only `this` solver
  (`&mut self`); a one-entry table would answer for id 0 only. Wiring it needs the
  solver table, or the solver itself, passed to `trim_cliff_rails` (the same handle
  question as #192's `FilletGeomRef` / `FilletBrep` id note); no compared path reaches it
  (`ProcessFullRollOver` defers earlier at `CreateCliffRollOverSideFG`, p2).
- **Solver dispatch (#236 review round 4, 2026-10-20).** The constant-radius solver's
  `FilletSolverOps` overrides (`GetJacobianSize`, `SetupOffsetValues`,
  `OffsetRadiiCanChange`, `LoadJacobian`, `GetType`, `GetSolverType`) call
  `blend.fillet_standard_solver_p1` (#168); they answer for `SmConstantRadiusFS`, the
  class the intersector shim builds (one Rust type stands for both it and
  `SmConstantRadiusAssistedFS`), so `GetSolverType` is `SM_FS_CONST_RADIUS` and
  `GetType` `SmConstantRadiusFS_TYPE`. `PointOnPlaneSolve` is a `FilletSolverOps` hook
  (virtual upstream, overridden by `SmConstantRadiusAssistedFS` in
  `blend.fillet_standard_solver_p2`); its default is the base's stub
  (`blend.fillet_solver_p2`). Two stubs stay, justified: `StepSolve`'s variable-radius
  cast (`SmVariableRadiusFS::GetOriginalCurve`) and `SmFindClippedRadiusEFO`'s
  `SmConstantDistanceFS::GetDistance` name `blend.fillet_standard_solver_p1`, which
  ports `CurveBasedFs::original_curve` and `ConstantDistanceFs::distance`, but neither
  solver implements `FilletSolverOps` yet, so no `StepSolve` call reaches them. Wiring
  them needs those solvers on the trait (with `original_curve` and `constant_distance`
  overrides) and, for the variable-radius branch, the rest of it after the cast (the
  radius law, `blend.fillet_executive_p1`).
- **`allow_ext` results are not bit-identical (owner: `surface.offset_surface`; found by
  the #236 review, 2026-10-19).** With `allow_ext = false` every `StepSolve` case agrees
  bit for bit; with `allow_ext = true` the committed generated cases differ by up to
  `2e-15` relative (inside the gate), and degenerate inputs on bumped degree-2/3 floor/wall
  patches stepping into the extended domain exceed it: in a local 160-input probe (40
  bumped patch pairs, each as is, with radius `1e-12`, radius `0`, and coincident surfaces
  `s1 = s0` with solver type 6), 69 fail with `allow_ext = true` (radius `1e-12`: 22, up to
  `2.4e-4`; radius `0`: 17, up to `1.26` on a value near `1e-17` on both sides, round-off;
  coincident: 30, up to `6.4e-3`) and all 160 agree bit for bit with `allow_ext = false`.
  The difference is in the extension the offset surface evaluates through
  (`OffsetSurface::select_surface` -> `extend_surface`, `surface/offset_surface.rs`), not in
  `StepSolve`, which only steps into the extended domain. Reproducer (a `fixtures/blend.fillet_solver_p3/`
  file; `values.flat[5]`, the next point's `uv0.x`: cpp `-4.9125900745644524e-1`, rust
  `-4.912594392781986e-1`, rel `8.8e-7`):
  ```json
  {"cases":[{"name":"allow_ext_coincident_t6","inputs":{"op":"step","ty":6,"tol":1e-06,"radius":0.24711325320699934,"orient":[1.0,1.0],"distance":0.3971157594662729,"extra":false,"init_fail":false,"allow_ext":true,"s0":{"bspline":{"oob":false,"form":0,"net":[[[0.0,0.0,0.0,-1.7976931348623157e+308],[0.0,0.3333333333333333,0.0,-1.7976931348623157e+308],[0.0,1.0,0.0,-1.7976931348623157e+308],[0.0,1.6666666666666665,0.0,-1.7976931348623157e+308],[0.0,2.0,0.0,-1.7976931348623157e+308]],[[0.3333333333333333,0.0,0.0,-1.7976931348623157e+308],[0.3333333333333333,0.3333333333333333,-0.00386486079810279,-1.7976931348623157e+308],[0.3333333333333333,1.0,-0.0496387620540373,-1.7976931348623157e+308],[0.3333333333333333,1.6666666666666665,-0.042903240427989445,-1.7976931348623157e+308],[0.3333333333333333,2.0,0.0,-1.7976931348623157e+308]],[[1.0,0.0,0.0,-1.7976931348623157e+308],[1.0,0.3333333333333333,0.006920316939206098,-1.7976931348623157e+308],[1.0,1.0,0.04497275994504303,-1.7976931348623157e+308],[1.0,1.6666666666666665,-0.005616450429799948,-1.7976931348623157e+308],[1.0,2.0,0.0,-1.7976931348623157e+308]],[[1.6666666666666665,0.0,0.0,-1.7976931348623157e+308],[1.6666666666666665,0.3333333333333333,-0.03901164828741922,-1.7976931348623157e+308],[1.6666666666666665,1.0,0.052278298644664226,-1.7976931348623157e+308],[1.6666666666666665,1.6666666666666665,-0.04916828327950224,-1.7976931348623157e+308],[1.6666666666666665,2.0,0.0,-1.7976931348623157e+308]],[[2.0,0.0,0.0,-1.7976931348623157e+308],[2.0,0.3333333333333333,0.0,-1.7976931348623157e+308],[2.0,1.0,0.0,-1.7976931348623157e+308],[2.0,1.6666666666666665,0.0,-1.7976931348623157e+308],[2.0,2.0,0.0,-1.7976931348623157e+308]]],"p":2,"q":2,"u":[0.0,0.0,0.0,0.3333333333333333,0.6666666666666666,1.0,1.0,1.0],"v":[0.0,0.0,0.0,0.3333333333333333,0.6666666666666666,1.0,1.0,1.0]}},"s1":{"bspline":{"oob":false,"form":0,"net":[[[0.0,0.0,0.0,-1.7976931348623157e+308],[0.0,0.3333333333333333,0.0,-1.7976931348623157e+308],[0.0,1.0,0.0,-1.7976931348623157e+308],[0.0,1.6666666666666665,0.0,-1.7976931348623157e+308],[0.0,2.0,0.0,-1.7976931348623157e+308]],[[0.3333333333333333,0.0,0.0,-1.7976931348623157e+308],[0.3333333333333333,0.3333333333333333,-0.00386486079810279,-1.7976931348623157e+308],[0.3333333333333333,1.0,-0.0496387620540373,-1.7976931348623157e+308],[0.3333333333333333,1.6666666666666665,-0.042903240427989445,-1.7976931348623157e+308],[0.3333333333333333,2.0,0.0,-1.7976931348623157e+308]],[[1.0,0.0,0.0,-1.7976931348623157e+308],[1.0,0.3333333333333333,0.006920316939206098,-1.7976931348623157e+308],[1.0,1.0,0.04497275994504303,-1.7976931348623157e+308],[1.0,1.6666666666666665,-0.005616450429799948,-1.7976931348623157e+308],[1.0,2.0,0.0,-1.7976931348623157e+308]],[[1.6666666666666665,0.0,0.0,-1.7976931348623157e+308],[1.6666666666666665,0.3333333333333333,-0.03901164828741922,-1.7976931348623157e+308],[1.6666666666666665,1.0,0.052278298644664226,-1.7976931348623157e+308],[1.6666666666666665,1.6666666666666665,-0.04916828327950224,-1.7976931348623157e+308],[1.6666666666666665,2.0,0.0,-1.7976931348623157e+308]],[[2.0,0.0,0.0,-1.7976931348623157e+308],[2.0,0.3333333333333333,0.0,-1.7976931348623157e+308],[2.0,1.0,0.0,-1.7976931348623157e+308],[2.0,1.6666666666666665,0.0,-1.7976931348623157e+308],[2.0,2.0,0.0,-1.7976931348623157e+308]]],"p":2,"q":2,"u":[0.0,0.0,0.0,0.3333333333333333,0.6666666666666666,1.0,1.0,1.0],"v":[0.0,0.0,0.0,0.3333333333333333,0.6666666666666666,1.0,1.0,1.0]}},"uv0":[0.16677358603977566,0.7353018686739239],"uv1":[0.7353018686739239,0.16677358603977568],"duv0":[0.0,0.5],"duv1":[0.5,0.0],"crv_pos":[0.3335471720795513,1.4706037373478478,0.24711325320699934],"crv_deriv":[0.0,1.0,0.0],"step":1.1314549619233154,"dom1":[-0.5,-0.5,1.5,1.5],"dom2":[-0.5,-0.5,1.5,1.5],"next_uv0":[0.0,0.0],"next_uv1":[0.0,0.0],"next_ud":[0.0,0.0,0.0,0.0]}}]}
  ```
  Not committed as a fixture (it fails the unit); tracking it down is a follow-up in
  `surface.offset_surface`.
