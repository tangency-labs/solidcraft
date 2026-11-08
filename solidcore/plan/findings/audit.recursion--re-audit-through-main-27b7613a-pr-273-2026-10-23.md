# audit.recursion: re-audit through main `27b7613a` (PR #273, 2026-10-23)

Two merges: 21 PRs through `422f7132` (111 changed kernel files), including #441
`topology.brep_p4`, #463 `intersect.surface_drop_lib_p3`, #244
`blend.fillet_executive_p3`, #280 `topology.brep_p7` and #554 (CrvOnSurf over any
surface kind), then #572 (offset face-cache solves).
- **Shared bound.** Main's new depth checks used 64 levels. `CrvOnSurf` now holds a
  general `Surface`, and its constructors and `set_uv_curve` check both children through
  `Surface::check_*_evaluation_copy_depth`. That check now wraps `check_geometry_nesting`,
  which follows a curve-on-surface's surface as well as its UV curve. The leaf
  B-spline/plane fast path keeps the walker's boundary (`<= MAX_GEOMETRY_NESTING`).
  `Volume::transform_simple`'s depth-aware append refused at 63 and now refuses at
  `MAX_GEOMETRY_NESTING` (32). Main's new tests that built 63-level chains now build to
  the shared bound.
- **Screen.** The `boolean::merge::p5` stage `delete_edge`/`delete_vertex` hits are
  trait methods forwarding to free functions. The `surface_intersector` cycle
  (`do_point_intersection_body` <-> `join_from_singular_point`) is held to depth 1 by
  the `inside_me` flag. The `CrvOnSurf` `Clone`, `CompositeCurve::clone_exact_bounded`
  and the evaluation copies copy geometry that was checked when it was built. There is
  no new nesting reader, and no unchecked boxed geometry field or install
  (`PendingExtension` predates this merge and holds a leaf extension). #572 adds
  `Surface::domain_with_continuity_from_cache`, which only forwards. No new recursion.
- **Checks.** fmt, workspace clippy, the wasm check, 45 difftest tests and
  `harness/plan/units.py check` (261 units) pass locally. 2040 of 2041 solidcore tests
  pass. The exception, `primitive::api_primitives::tests::solids_run_primitive_creation_until_the_next_unported_call`,
  fails only on Windows: it fails on a clean `main` there too (stopping at
  `topology.edge` `SmEdge::GetTolerance` instead of `geom.tol`) and passes on the Linux
  runner. On the runner, C++-feature clippy passes, and difftest of `curve.crv_on_surf`,
  `curve.crv_in_volume`, `curve.curve_p2`, `surface.surface_p2`, `surface.volume_p1`,
  `surface.volume_p2`, `surface.curve_bounded_surface`, `surface.srf_in_volume` and
  `surface.surf_of_extrusion_p1` gives 7 passed, 2 partial, 0 failed, with
  `max_rel_err` 0.
