# topology.brep_constructor: port, difftest and the STEP corpus built through SmBrepConstructor (2026-10-25)

PR #1000.

## Unit

`SmBrepConstructor` (SmBrepData.cpp lines 78-1189) is in `topology::brep_constructor`. The
difftest cases are constructor scripts (`step_import::script`). Both kernels run them:
the C++ side in a forked child, so an upstream null dereference becomes a crash marker
that is compared with the port's constructor `InvalidInput`. Each case compares every
call's status and a full dump of the B-rep that was built.

First result (2026-10-25): `PARTIAL topology.brep_constructor cases=185 passed=125
failed=0 deferred=60 max_rel_err=0e0`, all 60 deferrals at `EndShell(TRUE)`.

Upstream `EndShell(TRUE)` calls `SmBrep::ValidatePointers` first and ignores its
status, but that call creates the missing UV trim curves of every face that already
has one (`plan/upstream-issues/WR-446--brepconstructor-endshell-validatepointers-makes-uv-trim-curves.md`).
`ValidatePointers`, with `sm_CheckEdgeuseGeometry`, is ported on main as
`topology::brep::validation::validate_pointers` (unit `topology.brep_validation`).
When this PR was refreshed on main (2026-10-30), `EndShell(TRUE)` was wired to it with
upstream's defaults (no report list, `SM_LEVEL_0`, `SM_NO_WALK`; status ignored), so
those cases are compared rather than deferred. Current counts: see the PR difftest
summary.

## The importer recipe (`step_import::script::from_body`)

Two recipe bugs caused the first constructor run's 30 area DIFFs. Both kernels built
the same wrong topology and then disagreed on it; the ported area code was not at fault.
1. **Vertex order.** A new edge's start and end vertices must be given in the curve's
   direction. `StartEdge` gives the start vertex to the edge use that runs with the
   curve, whatever its comment says ("relative to the loop orientation"); smTopoTest
   passes them in curve order too. Passing them in loop order put vertex uses at the
   wrong ends of every edge traversed against its curve.
2. **UV domains.** Faces were bound over their surface's natural domain. Faces on
   periodic surfaces that straddle the seam, such as half-cylinder bores, came out
   wrong. The recipe now calls `AdjustSTEPUVDomain` with the face's STEP parameter box
   (script op 20, `step_uv_domain`): angles unwrapped along each loop, a full turn as
   one period, ranges kept within [-360, 360], cones clamped at their apex.

Check: `step_face_area` integrates the analytic area element over the STEP loops by
Green's theorem, independently of either kernel. It agrees with the constructor-built
faces on 1233 of 1241 turbine faces; the other 8 are within that estimate's own
polygon error.

## Corpus, `--jobs 4`, both recipes on this branch (main merged)

| build | MATCH | HEAL-DIFF | DIFF | DEFERRED | CRASH | SKIPPED |
|---|---|---|---|---|---|---|
| faces + stitch | 177 | 101 | 8 | 13 | 3 | 4 |
| SmBrepConstructor | 215 | 77 | 8 | 2 | 0 | 4 |

The 8 constructor DIFFs:
- **C++ crashes, Rust completes (2 bodies):** `1797609in` 0 and `nasty_cheese` 0
  (`construct` stage). Rust is the better side.
- **Rust fails, C++ succeeds (1 body):** `st203-bapl` 0, `area` stage: Rust `SM_ERR`,
  C++ succeeds. The body has two failed recipe calls (an `AdjustSTEPUVDomain` that
  returns `SM_ERR_INVALID_INPUT` on both sides). Rust is the worse side; not
  investigated yet.
- **Small value differences (5 bodies):** `moon_buggy` 0 (area, 2.7e-9 relative),
  `rear` 4 (volume, 7e-9), and `rear` 12, 15, 20 (area, 1e-9 to 2.8e-8). All are well
  inside the 1e-6 integration accuracy both kernels were asked for. These bodies have
  B-spline faces with no independent estimate, so the right side cannot be named.

Rust hangs on two bodies where C++ crashes: `io1-ec-214` 0 and `53711_74563f01_na` 0.
The run before the domain fixes timed out after 600 s in `CreateUVTrimCurves` ->
`DropCurve` on a cone face whose `AdjustSTEPUVDomain` had failed. With the final recipe
both no longer reach that state.

## The stitch recipe's faces (`face_input` -> `CreateFacesWithCurves`)

- On piston face 1 the face is built with its loop reversed, and three of its four
  edge uses carry UV trims that lift onto a different edge, 25.02 away (the arc's
  diameter). The area is still right.
- On turbine, 547 of 1241 face areas are off by 0.1-2%: face 901 gives 45.99 against an
  exact 47.00.
- `face_input` passes the same bounded curves and orientations as the constructor
  recipe, and both kernels agree on the stitch-path areas. So this is what upstream
  `CreateFacesWithCurves` does with that input, not a recipe bug.
- The STEP mapping is sound: every edge's curve at `t_start`/`t_end` lands on its
  vertices (piston 138 edges, turbine 3703 edges, 0 off).

Diagnostics on the branch: `crates/difftest/examples/step_edge_check.rs`,
`step_face_trims.rs`, `constructor_face_areas.rs`, `step_body_area.rs`,
`constructor_script_ops.rs`.
