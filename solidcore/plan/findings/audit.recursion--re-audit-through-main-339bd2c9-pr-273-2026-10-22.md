# audit.recursion: re-audit through main `339bd2c9` (PR #273, 2026-10-22)

The merge brought 24 PRs and 108 changed kernel files (about 8,900 lines). New ports:
#208 `blend.fillet_solver_p5` and #211 `boolean.topology_intersector_p3`. Others add
`SurfaceKind::SurfOfExtrusion` (#490), a crate-wide allocation ceiling (#505/#513,
`limits.rs`) and loop bounds (#514).
- **Solver.** #514 caps the total work of `SolveBranch` (`branches_visited` <=
  `MAX_SOLVE_BRANCHES` = 2^20 per solve). That is a work bound, not a depth bound,
  so the merge keeps both it and this audit's nesting counter (`solve_depth` <=
  `MAX_SOLVE_DEPTH + 1`). The topology solver's state rebuild carries both fields.
- **Extrusion.** Its generator must be a B-spline-based curve (`gen_base`: B-spline,
  ellipse, line, hyperbola, parabola), so it cannot nest; the shared walker follows
  it anyway.
- **Screen.** No new self-recursion: `Sphere`/`Torus::convert_uv_from_nurbs_to_step`
  forward to free functions, and the `OffsetSurface` literal in
  `blend::fillet_solver::p5::recovery` is the fillet solver's record type. No new
  call cycle, unchecked nested-geometry install, nesting reader or boxed geometry field.
- **Checks.** fmt, workspace clippy, 1887 solidcore tests, 39 difftest tests, the wasm
  check and `harness/plan/units.py check` (261 units) pass locally. C++-feature clippy
  passes, and difftest of the 8 touched units (`geom.global_solver`,
  `intersect.adv_surface_intersector`, `intersect.topology_solver_p1`, `query.tess`,
  `surface.surf_of_extrusion_p1`/`p2`, `surface.surface_p2`, `surface.offset_surface`)
  gives 3 passed, 5 partial, 0 failed, `max_rel_err` 0.
- **Follow-up merge through `16192513`.** #517 wires `Surface::point_based_surface_intersect`
  to `SurfaceIntersector::do_point_intersection` (three kernel files). That path was
  audited before: its re-entry through `join_from_singular_point` is limited to depth
  2. No new self-call, cycle or nested-geometry install. fmt, clippy, 1887 solidcore
  tests, 39 difftest tests and the wasm check pass.
