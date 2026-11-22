# Small fixture rejections explained: fillet_geom_p3, merge_p4, offset_executive_p2 (PR #692, 2026-10-23)

These rejections had no reason recorded in `plan/`. None is a wrong guard, so their
behaviour is unchanged.

- **blend.fillet_geom_p3 `regress_corner_edge_on_edge` (1 case).** The shim refuses
  (`SC_ERR_ARG`) `CalcCornerEdgeGeom` on an edge whose type is computed by another
  unit's calculator (`Model::other_unit_edge`: cross section, fillet x side face, on
  edge, on extended edge, fillet end, setback rail, rail extension). This fixture is an
  `SM_FE_ON_EDGE` edge on the shim's `network` model: `CalcEdgeOnEdge`
  (`blend.fillet_geom_p6`) crashed natively on it in fuzzing, since the shim does not
  build the solver data that calculator reads. The guard covers every other-unit edge
  type, not only the crashing one; those calculators are tested in their own units.
  The other five `regress_*` rejections are documented in `plan/upstream-issues.md`
  (UB-211 `regress_on_vertex_unclassified`, UB-185 `regress_mate_cycle`, UB-208
  `regress_recalc_no_dir_all_*`, UB-210 `regress_fif_no_curve_no_corner`).
- **blend.fillet_geom_p3 `regress_recalc_analytic_default` (1 case).** `ReCalcFilletGeom`
  with `analytic = TRUE` on a geometry that is not `SM_FG_TANGENT_ROLLOVER` runs
  `CalcConeFilletGeom`/`CalcTorusFilletGeom` (`blend.fillet_solver_p2`), which read
  solver data the shim's base `SmFilletSolver` does not build; it crashed natively. The
  shim refuses every analytic recalculation except on a tangent rollover.
- **boolean.merge_p4 `plane-domain-constructor-rejection`, `gen0008`, `gen0013` (3
  cases).** An operand of kind 3 is a `SmPlane(point, normal)`, whose natural UV domain
  is `±SM_INFINITE_PARAMETER` (1234567). The shim makes its face with
  `CreateFaceFromSurface` on `[0, u] x [0, v]`; when `u` or `v` exceeds 1234567 (2e6 in
  the fixture; `v` = 1889531 and 1242370 in the generated `other` operands at the 1e6
  scale), upstream's domain-containment check returns `SM_ERR`. The shim then returns
  `[-1]`. The Rust operands are imported from the native export, so there is no Rust
  operand to merge and nothing to compare.
- **offset.offset_executive_p2 `review_sheet_*` (9 cases).** The native plane-sheet
  fixture (a unit square made with `CreateFaceFromSurface` on a B-rep whose tolerance
  is 0.9 or 1000) is refused with `SM_ERR` by `representable_topology`: with a
  tolerance that large compared with the edges, upstream merges or drops boundary
  vertices and edges, so the result is not 1 face, 1 loop, 4 edges, 8 edge uses and 4
  vertices. The Rust fixture builds that 4-edge sheet by hand and cannot represent the
  collapsed topology, so the offset operation is never run on either side. The runner
  test `incomplete_native_sheets_are_rejected_without_hiding_valid_controls` pins the
  9 rejections and the 4 compared controls (tolerance 0.5, and 1000 on a 1e4-scale
  sheet).
