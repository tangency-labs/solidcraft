# audit.recursion: re-audit through main `6d3bc74f` (PR #273, 2026-10-23)

The merge brought 18 PRs and 101 changed kernel files (about 17,000 lines), including
#466 `blend.fillet_solver_p2`, the #531 fillet batch (`blend.fillet_corner_p1`) and
#535 (`topology.brep_p6` closure split), plus new files for the sweep store and the
surface-cache face context.
- **One walker, one bound.** Main added its own non-recursive evaluation-copy walker
  (`check_owned_evaluation_copy_depth`, 64 levels) with curve and volume entry points.
  Those entry points now wrap this audit's `check_geometry_nesting`, so construction and
  evaluation copies share `MAX_GEOMETRY_NESTING` = 32. Construction already refuses
  anything deeper, so no buildable geometry changes behaviour. Main's two new tests that
  built 62-level chains now build until a constructor refuses. Its composite-segment
  test uses the checked `set_segment` with an exact `clone_for_evaluation` copy, because
  `Clone` resets the owner and applies the knot fix-up.
- **Screen.** No new self-recursion: `query::face::p2::point_classify` forwards to the
  free function, `topology_traverser`'s trait methods call `Marks`' inherent methods,
  and the sweep store's `object` forwards to its provider. The only cycle,
  `trim_srf_cache` `implant_edgeuse` <-> `subdivide_node`, is limited to depth 2 as
  before. The new composite `clone_for_evaluation_depth` builds copies of curves already
  checked at entry. No new nesting reader, boxed geometry field or unchecked install.
- **Checks.** fmt, workspace clippy, 1959 solidcore tests, 41 difftest tests, the wasm
  check and `harness/plan/units.py check` (261 units) pass locally. C++-feature clippy
  passes, and difftest of the 13 units that make evaluation copies or hold the
  changed tests (`curve.curve_p2`, `curve.composite_curve_p1`, `curve.crv_in_volume`,
  `curve.crv_on_surf`, `curve.iso_curve`, `curve.offset_curve`, `curve.projected_curve`,
  `surface.srf_in_volume`, `surface.surface_p2`, `surface.surface_point_solve_p3`,
  `surface.surf_of_extrusion_p1`, `surface.surf_of_revolution_p1`,
  `blend.fillet_corner_p3`) gives 9 passed, 4 partial, 0 failed, `max_rel_err` 0.
