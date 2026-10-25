# audit.recursion: merged snapshot and follow-up limits (PR #273, 2026-10-21)

The audit covers the code merged through main `67e321cd` plus the review fixes
in this PR. Wrapper constructors and setters check owned curves, surfaces and
volume chains with a non-recursive walk before
adding another ownership edge (maximum depth 64 at first, 32 since main's #308; root at depth 0). The base global
solver limits traversal to `MAX_SOLVE_DEPTH + 1` nested calls (main later set
`MAX_SOLVE_DEPTH` to 1000 and checks tree heights up front in `solve_it`; the counter
covers overrides and direct callers); the advanced intersector uses the same ceiling. Spatial-tree top-box rebuilds are limited to one nested rebuild,
and tessellation refuses a missing-tree build re-entry. Numeric formulas and
child visitation order are retained.

The readiness review found two ways to bypass the constructor checks:
`ProjectedCurve::set_canonical` accepted an arbitrarily nested replacement, and
`CompositeCurve::segments_mut` exposed unchecked segment replacement. The former
now checks before changing any field; the latter is replaced by the checked
`set_segment` method. Regression tests cover rejection without partial mutation,
valid replacement and solver depth-counter restoration after a recursive error.
The advanced intersector's guard now rejects before entering call 2049, matching
the base solver's ceiling; a regression test checks that no branch state is
changed by that rejection.
`audit.recursion` has no C++ counterpart or standalone difftest runner;
`--green` excludes exactly this audit id and continues to fail ordinary ports
without registered runners.

The follow-up review of main `0fb5bc7e..67e321cd` screened all 77 changed kernel
files for new self-calls and call cycles, then checked the new dispatch and
ownership-construction paths manually. This is a source review, not a formally
resolved call graph. The eight new ports are `blend.fillet_executive_p2`,
`blend.fillet_executive_p3`, `boolean.merge_p3`, `boolean.merge_p4`,
`query.face_p1`, `query.face_p3`, `query.face_p4` and `topology.brep_p2`.
No new unbounded recursion path was found. Boolean-tree evaluation uses an
explicit stack; fillet tangent-chain walks are iterative and bounded by the
solver count plus one; face-loop containment uses explicit stacks and the
existing depth-bounded `Looptree::find_item`. Wrapper construction still uses
the checked constructors. The solver state reconstruction retains `solve_depth`.
The merge keeps the new difftest wildcard and process-parallel execution options
alongside the exact-id audit exemption.

Validation before the main merges: at `726d2d60`, `difftest --green` completed
222 units (114 passed, 108 partial, 0 failed). Every unit summary (including case
counts and `max_rel_err`) matched the baseline; this compares summaries, not the
complete before/after numeric reports. After six more ports and the review fixes,
CI at `e8520b7c` passed all checks, including C++-feature clippy and
`difftest: 228 unit(s), 114 passed, 114 partial, 0 failed`
([run 39362924279](https://github.com/tangency-labs/solidcore/actions/runs/39362924279)).
The new merge through `67e321cd` selects 236 port runners (114 green, 122 partial
in the manifest); its full C++ differential run belongs to the fresh PR checks
and is not covered by that earlier run. Local merged-tree validation passes
1554 solidcore tests, 31 difftest unit tests and the JSON float-roundtrip
integration test. Formatting, workspace clippy and the wasm target check are
recorded in the PR description.

Re-audit through main `7b57d8de` (2026-10-21): the merge brought #282, #284, #285,
#286, #287, #289, #291, #293 and #298 (31 changed kernel files, no new port units).
Every function they add or change was screened for self-calls; each hit forwards
to a same-named function on another type or path (`SurfaceIntersector::flush_curve`,
the merge_p4 adapter, `topology::brep::p5`/`p9` helpers), and the new merge_p3 ->
adapter -> `merge::p4::manifold_boolean` chain cannot re-enter merge_p3 (p4 has no
reference to it). The NURBS evaluation rewrite (#298) is loop-based. No new curve,
surface or volume variant or wrapper constructor was added. No new bound was
needed; fmt, workspace clippy, 1556 solidcore tests and the wasm check pass.

Re-audit through main `ca3bbd2b` (2026-10-21): the merge brought two new ports,
#229 `boolean.merge_p2` and #239 `topology.face_p4`, plus follow-ups (28 changed
kernel files). Neither new file has an intra-file call cycle, and their same-name
calls forward to other modules (`topology::brep::p2`, `query::face::p3`, `loopuse`,
the classification adapter's `inner`). The one self-call, `face_p4::has_seam_problem`
-> `FaceProps::set_props_stage2_with` -> `has_seam_problem`, is depth 1: the stage is
set to `Stage2A` before the callback, and the nested call passes those props, so
it skips the re-run. No new curve, surface or volume variant or wrapper
constructor. No new bound needed; fmt, workspace clippy, 1562 solidcore tests and
the wasm check pass.

Re-audit through main `7e3f324e` (2026-10-22): 31 PRs, 202 changed kernel files,
including #267 `blend.fillet_executive_p2`, #363 `topology.brep_p6` (make face
topology), the #281/#317 co-ports and the #313 surface-dispatch refactor, which adds
`SurfaceKind::Cone` and threads `Kernel` through evaluation. Main independently
bounded `SolveBranch` with an up-front tree-height check (`MAX_SOLVE_DEPTH` = 1000).
The merge keeps it and keeps this audit's per-call counter (it also covers overrides
and direct callers), allowing `MAX_SOLVE_DEPTH + 1` nested calls so every tree set
the check accepts still runs. The advanced intersector's cap is now the same. The shared
nesting walker follows the cone's generator curve; cones are built from parameters
only, so no constructor check is needed. Main's deep-wrapper evaluation test, which
built 70-deep STEP and curve-bounded chains, now builds until a constructor refuses
(at most 64), because such chains can no longer be constructed. No new self-recursion
was found: the hits in new functions (`Curve::approximate_polar_box`,
`Curve::is_periodic`) forward to free functions. Validation: fmt, workspace clippy,
1625 solidcore tests, 38 difftest tests and the wasm check pass locally; C++-feature
clippy passes, and difftest of the 12 touched units (`geom.global_solver`,
`intersect.adv_surface_intersector`, `surface.offset_surface`, `surface.surface_p2`,
`surface.volume_p2`, `curve.offset_curve`, `surface.cone_p1`-`p3`, `geom.tree`,
`query.tess`, `intersect.topology_solver_p1`) gives 4 passed, 8 partial, 0 failed,
`max_rel_err` 0 throughout.

Re-audit through main `ea10fd3e` (2026-10-22): 19 PRs, 124 changed kernel files
(about 28,000 lines), with the new ports #201 `offset.offset_executive_p2`, #247
`blend.fillet_geom_p6`, #264 `blend.fillet_geom_p5`, #329 `query.brep`, #222
`sweep.topology_sweep` and #233/#250 `primitive.primitive_creation_p3`/`p2`.
- **Nesting bound to 32.** #308 measured that a 64-deep offset chain read from a
  file overflows the 1 MiB wasm32 stack unoptimised, and lowered `MAX_READ_NESTING`
  to 32. Geometry built in memory recurses through the same dispatch, so
  `MAX_GEOMETRY_NESTING` (and `MAX_SURFACE_NESTING`, tied to it) is now 32 as well.
- **Checked segment replacement.** `primitive.primitive_creation_p2`'s taper wrote
  B-spline generators back through `CompositeCurve::segments_mut`, which this audit
  removed. It now uses the checked `set_segment`; a B-spline cannot nest, so
  results are unchanged.
- **Screen.** The new functions' same-name calls forward to other modules or free
  functions (`fillet_intersector`, `topology_sweep::engine` local helpers). The one
  intra-module cycle, `fillet_geom::p3` `corner_vert_geom` <-> `mate_geom`, is
  already bounded by its port (`MAX_MATE_DEPTH` = 64, depth parameter, `SolidError`
  past it). The blend `OffsetSurface` struct literals are
  `blend::fillet_custom_solver::OffsetSurface`, a solver record rather than the
  wrapper surface. No new curve, surface or volume variant, no new nesting reader
  and no new boxed geometry field.
- **Checks.** fmt, workspace clippy, 1772 solidcore tests, 38 difftest tests and the
  wasm check pass locally. C++-feature clippy passes, and difftest of the 17 units
  holding a wrapper constructor or the taper change (`primitive.primitive_creation_p1`-`p3`,
  `curve.composite_curve_p1`/`p2`, `curve.crv_in_volume`, `curve.crv_on_surf`,
  `curve.iso_curve`, `curve.offset_curve`, `curve.projected_curve`,
  `surface.curve_bounded_surface`, `surface.offset_surface`, `surface.srf_in_volume`,
  `surface.surface_p1`/`p2`, `surface.volume_p1`/`p2`) gives 8 passed, 9 partial,
  0 failed, `max_rel_err` 0 throughout.

The green audit status applies to this merged snapshot. Direct audit dependency
`blend.fillet_corner_p2` remains unported (`offset.offset_executive_p2` landed in #201 and is covered by the re-audit through `ea10fd3e`);
their implementations, other open port PRs and future geometry variants need a
follow-up sweep when they land.
The current manifest's 122 partial units retain deferred dependency paths that
need review and differential testing when wired.
Native guard tests and a wasm compile check do not measure worst-case wasm stack
use at the solver ceiling; no runtime stack-exhaustion benchmark was run.
Constructor APIs that were infallible now return `Result`, and callers using
`segments_mut` must move to checked segment replacement.
