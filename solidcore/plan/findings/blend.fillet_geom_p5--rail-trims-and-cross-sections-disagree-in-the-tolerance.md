# blend.fillet_geom_p5: rail trims and cross sections disagree in the tolerance-dominated regime (resolved)

**Resolved in round 2 (2026-10-20): a port bug in this unit's calls, not in the curve
units.** Instrumenting both sides on `gen0024` (rail 1, `app_tol 1e-9`, end point
`3.6e-7` off a rail of size `1e-3`) showed the drop of the start vertex agree bit for bit
(`t = 0.58501075132934788`) and `SmCurve::GlobalPointSolve(.., SM_SR_ALL)` differ: upstream
returns one solution (`0.035778619102039888`, value `3.6387038570803368e-7`), the port
four within `5e-8` of each other in parameter and `1e-15` in value, and
`TrimRailCurves` takes the first (`0.035778566244631806`). Upstream merges them in
`SmGlobalSolver::AddSortedSolution` -> `IdenticalSolutions` (SmGlobalSolver.cpp:1910-1931),
whose parameter tolerance is `max(100 SM_EFF_ZERO (1 + |t|), 1e-4 * length)` with the
length from `SmSolution::GetParameterInterval`, i.e. the natural interval of the solution's
object, the curve. The port called `global_point_solve` / `global_curve_self_intersect`
with no solution object (`NoObject`, `None` handles), so the interval was missing, the
tolerance `~1e-9` and the near-duplicates stayed. Fix: the rail solves in
`TrimRailCurves` / `TrimCliffRails` (and the self-intersection) name the rail curve with
its natural interval (`RailCurveHandle`, as `query.edge` does), and `sm_CreateUVCurve`'s
surface solves in `blend.fillet_geom_p4` name the surface with its natural UV domain
(`UvSurfaceHandle`, SmFilletGeom.cpp:4461-4504). After it: 300 generated cases 287 pass,
0 fail, max_rel_err 0.

**Round 3 (review of #264, 2026-10-20): the same pattern in the other fillet-geometry
solves.** The object-less solves whose solutions are counted or chosen (a `== 2` or
`!= 1` test on the solution count, a pick of the first solution) are the ones that can
disagree. Fixed here, as co-ports in units that are already merged (this PR edits their
files; their difftests were re-run):
- `blend.fillet_geom_p1` `two_pnts_intersection` (`sm_TwoPntsIntersection`, the four
  `sm_SurfaceDropPoint` calls; `== 2` seam test, cpp:286): the drops now name their
  surface (`SurfaceHandle`, p1.rs), as upstream's `GlobalPointSolve` does (cpp:161). The
  p1 runner's `SurfaceDropPoint` op passes the same handle. This is on `calc_fillet_int_side_face`
  / `calc_edge_on_extended_edge` / the cliff rails / `calc_fillet_int_fillet` path of this
  unit (all through `srf_srf_intersection` with skip = `false`). Seam fixtures at scale
  `1e-3` / `1e-6` with the tolerance at the surface's extent were added to
  `fixtures/blend.fillet_geom_p1/cases.json` (`drop_point_cylinder_seam_1e-*_tol_extent`,
  `two_pnts_cylinder_seam_1e-*_tol_extent`); they agree with and without the handle (no
  generated or fixture p1 input was found, up to 5000 seeds, where the handle changes a
  result), so the change is a fidelity fix without a reproducing case.
- `blend.fillet_geom_p2` `calc_set_back_vert` (`SmFilletVertex::CalcSetBackVert`, cpp:3366
  / 3370, `solutions.len() != 1 -> SM_ERR`; the earlier text of this section misplaced it
  "in the `sm_TwoPntsIntersection` area"): the two `GlobalPointSolve`s name their offset
  surface (`SurfaceHandle`).
- `blend.fillet_geom_p4` `calc_fillet_int_2_fillets` (`GlobalCurveIntersect`, cpp:2806,
  `len() != 1 -> SM_ERR`): the intersection names the two curves and their analytic
  replacements (`CurveObjectHandle`, p1.rs, shared with p5's rail handles). The p2 and p4
  vertex calculators are reached by no difftest case (the corner object model is
  `blend.fillet_corner_p1`), so those two changes are faithful by construction, not
  by a comparison; @bherbruck owns both units.

Left as is: `p1.rs` `find_fillet_brep_edge` (`SR_SINGLE`, only the first solution's
value is read), `find_stepoff_uv` (`SR_SINGLE`, tests emptiness and takes the best),
`normalized_drop` (`surface.drop_point::<NoObject>`), `p4.rs` `calc_rail_int_extended_edgeuse`
(only tests emptiness): none depends on how many near-duplicates survive.

**Cross-cutting (for the orchestrator).** `NoObject` (`surface/surface/p4.rs`,
`ObjectKind::Surface` with no natural domain) is used in about 30 files crate-wide (99
mentions outside its definition: the surface, intersect, topology, query, heal and blend
units). Every solve there whose solutions are counted, compared with a size or chosen by
index is exposed to the same near-duplicate difference at small parameter domains or
large tolerances; a sweep is a task of its own (audit each `::<NoObject>` call against the
upstream call's `this`; `IdenticalSolutions` merges within `1e-4` of the object's natural
interval or domain, SmGlobalSolver.cpp:1910-1931, `SmSolution::GetParameterInterval`,
SmSolutionArray.cpp:730-828).

Original report (round 1):

PR #264, difftest `--unit blend.fillet_geom_p5` (300 generated cases: 283 pass, 4 fail,
13 deferred; `--seeds 2000`: 1896 pass, 22 fail, 82 deferred). Every failure has a model
at scale `1e-3` or `1e-6` (`scale_of` in the p1 generator) with a solver tolerance
`1e-9 .. 1e-3`, i.e. a tolerance comparable to or larger than the rail; ordinary scales
agree bit for bit. Three kinds, all inside callees of ported units:

1. `TrimRailCurves` / `TrimCliffRails` (17 of the 2000-seed failures, mostly cliff
   rollover geometries): the trimmed rail's control points and the vertex uses' UV
   points differ at `1e-9 .. 1.5e-6` relative (e.g. `gen0024`: the end vertex's
   `GlobalPointSolve` parameter, cpp `3.577861910203989e-2`, rust
   `3.5778566244631806e-2`; `gen0162`: cpp `5.9835695910636895e-5`, rust
   `5.9835698254225096e-5`). The calls are `SmCurve::DropPoint` (MINIMIZE, growing
   tolerances) and `SmCurve::GlobalPointSolve(ivl, SM_SO_MINIMIZE, p, tol, NULL, NULL,
   SM_SR_ALL)` on the same rail curve, then `Copy`/`Trim`; the Rust side passes the same
   arguments (`curve.curve_p6` / `curve.curve_p1`). Suspected: upstream keeps the curve's
   cache on the curve object across the drop and the solve (`SmCacheMgr`), the port
   rebuilds it per call; or a solver branch of those units that only differs when the
   tolerance exceeds the curve's extent. Not isolated yet.
2. `gen1042` (`TrimRailCurves`, rational degree-1 rail at scale `1e-3`, tolerance
   `1e-3`, coincident end points): upstream's `GlobalCurveSelfIntersect(ivl, 1e-2)` (unit
   `curve.curve_p9`) returns exactly one solution (the rail is trimmed to its interior
   knots `[1.1280950573855266, 2.243313725505451]`), the port's does not (no trim).
3. `gen0677` (`CalcCrossSection` on a fillet surface at scale `1e-3`): upstream succeeds
   with a lifted 3D curve and a UV curve; the port returns `SM_ERR_NULL_POINTER` (no UV
   curve to lift), so `sm_CreateUVCurve` (`blend.fillet_geom_p4`, PR #252) or `LiftCurve`
   (`surface.surface_p11`) differs there.

Not fixed here: the differing code is in the callees, which are shared with other units.
The unit stays `blocked` until these are isolated (start with a curve-level difftest of
`DropPoint` + `GlobalPointSolve` on one curve object at scale `1e-3` with tolerance
`1e-9 .. 1e-3`). No tolerance was loosened and no generator scale was narrowed.
