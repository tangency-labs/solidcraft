# PR #208 — latest p1/main integration (2026-10-20)

Merged PR #192 at `3fe34880` and canonical main at `95b823ed` without rebasing.
The module lists retain every addition in sorted order. The intersector delegates
base operations to `FilletSolver`, uses `FilletGeomRef`, and keeps offsets in that
base instead of reintroducing `ConstantRadiusAssistedFs::saved_offsets`.
`RailUvCurveIntersectEnfo` keeps #192's `&Curve` dispatch. The geometry constructor
and accessors use the base solver's real offset slots and surface accessor.

`sm_SplitRail` and `sm_ComputeSupportEdge` still have exactly one typed body, in
`fillet_solver/p1/rails.rs`, publicly re-exported by p1 and called by p5. The duplicate
opaque bodies brought by main are removed. At #192 head `3fe34880`, its runner,
FFI and shims have no `split_rail` / `support_edge` operations to adapt. The existing
public-entry regressions in `p5/tests.rs` call the typed p1 API directly; they were
retained and extended.

Main also added `blend.fillet_geom_p2` as partial. Both newly available calls are
wired: support construction delegates to `srf_srf_intersection` with the caller's
kernel, actual vertex point, tolerances, null optional endpoints/directions and
`skip_two_pnts_intersection = false`; rollover corner recomputation delegates to
`calc_rail_int_edgeuse_with_solver`. The latter shares the complete existing
`CalcRailIntEdgeuse` body and accepts the caller's live solver with its actual id,
without renumbering geometry ids or allocating an input-sized table. Indexed
executive tables keep their existing API. Geometry-owned offsets take precedence;
a missing adjacent solver id is a resolver error. The corner getter still defers
on `blend.fillet_corner_p1`, so later solve branches are not natively compared.

Regressions check a nonzero solver id, retained live-solver mutations, unresolved
adjacent ids and geometry-owned offsets; the p5 adapter now reaches the real
corner dependency. A public support-helper regression uses two perpendicular
planes and completes the real intersection, final-surface intersection, vertex
update, curve trim, owner installation and notification. The earlier expectation
of an immediate `UNPORTED(blend.fillet_geom_p2)` was replaced by this successful
real-callee regression. These kernel tests do not claim C++ parity of a complete
rollover repair: native p5 fixtures still stop before the unavailable segment walk.
`fixtures/blend.fillet_geom_p2/review_pr208.json` separately compares the real
surface-intersection callee on the support helper's plane/plane, start-only,
null-directions input, including its 3D and UV curves (60 compared values, bit-identical).


The integrated targeted gate has 0 failures across `blend.fillet_solver_p5`,
`blend.fillet_solver_p1`, `blend.fillet_geom_p1`, `blend.fillet_geom_p2`,
`blend.fillet_custom_solver`, `blend.fillet_intersector` and
`offset.offset_geometry_creation`. P5 remains 170/164 pass/4 deferred/2 rejected;
p1 is 346/339 pass/7 deferred; geom p2 is 351/220 pass/131 deferred. Every compared
value is bit-identical. Perturbation detects 164/164, 339/339 and 220/220 cases in
p5, p1 and geom p2 respectively. All 1500 kernel tests and four native p5 FFI tests
pass. The complete sweep and CI status are recorded in the PR description.
