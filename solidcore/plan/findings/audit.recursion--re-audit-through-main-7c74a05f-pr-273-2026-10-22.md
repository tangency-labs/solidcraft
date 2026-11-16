# audit.recursion: re-audit through main `7c74a05f` (PR #273, 2026-10-22)

The merge brought 18 PRs and 122 changed kernel files (about 17,000 lines). New ports:
#439 `blend.fillet_standard_solver_p2`, #236 `blend.fillet_solver_p3`, #213
`primitive.primitive_creation_p1`, #242 `blend.fillet_geom_p3`, and
`topology.brep_p3`/`p8` (via the shepherd batch). #436 adds `SurfaceKind::Sphere` and
`SurfaceKind::Torus`, and #462 splits the plan into `plan/units/<id>.toml`. This
branch's notes moved there with `scripts/units-migrate-branch.sh`, and the `--green`
exemption for `audit.recursion` now runs on the per-unit loader.
- **Sphere and torus.** Both wrap a `SurfOfRevolution`, whose generator must be a
  B-spline curve (`gen_base` rejects anything else), so neither can nest. The
  shared nesting walker follows their generator anyway.
- **Screen.** No new self-call in the new or changed functions. The two call cycles
  found are bounded as before: the heal cascade (`update_target_lists` ->
  `heal_targets` -> the `fix_*` methods) by `MAX_TARGET_UPDATE_DEPTH` = 32, and the
  advanced intersector's `solve_branch` <-> `solve_branch_at_depth` by its solve
  counter. No unchecked install of nested geometry, no new nesting reader and no new
  boxed geometry field. The merged code compiled without changes against the
  checked constructors, so it uses none of the removed or now-fallible APIs unchecked.
- **Checks.** fmt, workspace clippy, 1827 solidcore tests, 39 difftest tests, the wasm
  check and `harness/plan/units.py check` (261 units) pass locally. C++-feature clippy
  passes, and difftest of the 9 units the walker reaches (`surface.sphere`,
  `surface.torus`, `surface.surface_p2`, `surface.offset_surface`, `curve.iso_curve`,
  `surface.surf_of_revolution_p1`/`p3`, `surface.curve_bounded_surface`,
  `surface.srf_in_volume`) gives 7 passed, 2 partial, 0 failed, `max_rel_err` 0.
