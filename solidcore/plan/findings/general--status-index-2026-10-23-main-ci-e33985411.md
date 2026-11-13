# Status index (2026-10-23, main CI `e33985411`)

This file is append-only, so older headings keep the status they had when written. This
index records what has changed since; check here before acting on an older entry. Unit
statuses are from the difftest reports of main CI run 40029966086 (`e33985411`).

**Headings that say "partial" for units that are now green (39).** By line number at
`e33985411`; each of these units now passes every compared case with nothing deferred:
299 `geom.extent3d`, 402 `geom.tol`, 472 `geom.tree`, 582 `geom.attribute`, 840
`geom.object`, 1055 `geom.grid`, 1139 `nlib.crv_basic`, 1171 `geom.transform`, 1189
`geom.a_object`, 1262 `curve.hermite_curve`, 1281 `nlib.crv_tool_p1`, 3665
`surface.surface_p6`, 4190 `topology.edgeuse_p1`, 4549 `intersect.curve_class_p2`, 4710
`intersect.surface_intersector_p5`, 4777 `topology.proto_topology_p1`, 5300
`topology.vertexuse`, 5434 `topology.loopuse`, 5499 `intersect.surface_drop_lib_p1`,
5586 `topology.vertex`, 5655 `surface.surface_p10`, 5743 `heal.gap`, 5828
`intersect.surface_drop_curve`, 5968 `topology.brep_cache`, 6017
`topology.trimming_tools`, 6148 `topology.edgeuse_p3`, 6228
`sweep.rotational_sweep_geometry`, 6459 `heal.heal_data_p2`, 6580
`intersect.curve_class_p2`, 7346 `surface.surface_p11`, 8774
`blend.fillet_standard_solver_p1`, 8915 `blend.fillet_geom_p1`, 11246 `query.face_p1`,
12060 `heal.stitch_p1`, 12099 `blend.fillet_standard_solver_p2`, 12346
`sweep.topology_sweep`, 12440 `primitive.primitive_creation_p3`, 13179 `heal.stitch_p2`,
16001 `blend.fillet_solver_p3`. (`intersect.curve_class_p2` has two such headings.)
The 30 other "partial" headings belong to units that are still partial; see
`plan/STATUS.md` for their blockers.

**Entries that are resolved but not marked as such in their heading:**
- "PR #463: 27 Rust-vs-C++ mismatches newly reachable once `SmSurface::DropCurve` is
  wired": all 27 resolved. The 23 `topology.edgeuse_p3` `EvaluateBinormal` cases by
  PR #581, `blend.fillet_solver_p4` `gen0512` by PR #584, and the 3 nested UV-trim-curve
  cases (`topology.face_p3` `cyl_bspline_uv_trim_*`, `boolean.merge_p2`
  `make_faceuse_sphere`) by PR #586. None is in `crates/difftest/known_mismatches.toml`
  any more.
- "topology.brep_p4 / topology.brep_p5: StitchAndOrient's second pass, and the glue
  stub that hides 36 heal.stitch_p2 differences": resolved by the PR #441 follow-up
  (`GlueEdgesGeneral` now takes the glued edge off `Brep::edge_list`); see the later
  `primitive.api_primitives` entry of 2026-10-22. `heal.stitch_p2` is green (556/556).
- "surface.surface_p11 / surface.surface_p1: surface cache history decides
  `LiftCurve`'s C1 domain off the surface domain (upstream:cache-history)": resolved by
  PR #571 (issue #568), which models the lazy surface cache tree; the
  `upstream:cache-history` deferral is gone (see "surface cache history modelled").
- "query.face_p3 `bisector`: `LoopObjectCurves` has no `vertex_face_gap`" and
  "query.face_p1 `gen0160`": both bodies say "Resolved"; the headings do not.
  `query.face_p1` is green.

**Wording.** The entry "intersect.topology_solver_p2 static face solves: answering
`GetNaturalUVDomain` exposes 9 mismatches" (still open) names two internal tracking
labels. Read the first as the proposal to give the runner's `StaticHandle` a
`natural_uv_domain`, and the second as the missing face-aware `SurfaceSolveContext`
(global solves on a face's surface should use that face's trimmed surface cache).
