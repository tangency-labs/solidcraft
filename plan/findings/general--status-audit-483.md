# General: operation-status audit for #483 (2026-10-31)

PR: https://github.com/tangency-labs/solidcore/pull/1775
Issue: https://github.com/tangency-labs/solidcore/issues/483

Units: `boolean.merge_p1`, `primitive.primitive_creation_p1/p2/p3`,
`blend.fillet_corner_p1/p3`, `blend.fillet_geom_p1`, `blend.fillet_solver_p1/p2`,
`offset.offset_geometry_creation`, `intersect.adv_surface_intersector`,
`intersect.surface_intersector_p1/p2` and their callers.

The pinned source is v1.0.1 (`3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`).
This is an independent audit of the Rust counterparts of the concrete defects named
in #483. The fork's `r-no-ack` diff is unavailable to this contributor (404); its
reported 1060-site classification is not a coverage count for this Rust change.
`SmProtoTopology.cpp`, the remaining `SmSurfaceIntersector.cpp` methods and shared
core status policy remain outside this issue's stated classified paths. No global
change is made to `ignored`, `ignore_status` or native failure recovery.

All result-changing fixes are off by default in `Kernel::fixes`. Original public
pivot/extension signatures keep their behavior; production kernel-aware callers use
new variants. Checked mode returns the first mandatory failure before consuming its
output. Successful prefix mutations remain; these are not transactions. The Boolean
scope's established inverse-error precedence remains unchanged.

## Call inventory

Rows group repeated calls only when they have identical policy. `?` means an existing
propagation was audited and retained. Test names below are searchable in source.

| C++ symbol / call | Rust counterpart / disposition | Policy / evidence |
|---|---|---|
| `SmTemporaryBrepTransform`: both `CalculateBoundingBox` calls | `temporary_brep_transform`: reject either failure before selecting a shift | `boolean_temporary_transform_statuses`; `audit_483_boxes_fail_before_temporary_translation`, both operand orders |
| constructor's first and second `Transform` | `TemporaryBrepShift::apply`: first failure stops; second failure restores the successful first operand | same flag; existing `checked_temporary_translation_*` tests |
| destructor's `Transform` on surviving operands | `TemporaryBrepTransform::restore/run`: try every survivor; first inverse failure overrides the body error | same flag; existing restore/survivor tests; `fixtures/boolean.merge_p1/temporary-transform.json` |
| `sm_FindPivotPoint`: four `Evaluate` calls in original short-circuit order | `find_pivot_point_with_kernel` / private implementation: no failed endpoint is compared or returned | `primitive_path_curve_statuses`; `audit_483_pivot_and_extensions_stop_at_failed_evaluations` |
| `sm_CreateRuledSurfaceFromOffsetCurve` pivot call | `ruled_surface_from_offset`: `?` through kernel-aware pivot; nearest-corner override also checks all endpoint evaluations | same flag, independently of `taper_pivot_nearest_corner`; p3 native fixtures |
| `ExtendPathCurves`: previous-end/current-start/current-end/next-start `Evaluate` | `extend_path_curves_with_kernel`: error before parallelism test; input curves unchanged | same flag; pivot/extension regression and native p1 helpers |
| `ExtendPathCurves`: each start/end `CreateExtendedCurve` | private `extend`: error before replacing local curve; successful `None` means no replacement | same flag; sweep's checked mode also applies this policy |
| `PositionProfileCurves`: each `CalculateBoundingBox` | `position_profile_curves_impl`: error before accumulating bounds or appending this curve | `planar_path_profile_placement_statuses`; `audit_483_placement_rejects_failed_box_before_appending_copy` |
| same: each `Copy` | already `?` (a failed copy becomes NULL and is dereferenced upstream) | unchanged unconditional safety guard |
| same: translation `SetCanonical`, each translation `Transform` | existing WR-632 policy retained | same placement flag; existing four overflow regressions and normal-success test |
| same: `ComputeCrvsNormal`, `AngleBetween` | propagate before normal/angle use; keep copies and translations already made | same flag; `audit_483_placement_read_failures_keep_the_successful_prefix` |
| same: identity orientation `SetCanonical` | elided identity assignment; cannot fail for constant unit axes | invariant, no fallible output |
| same: orientation `Transform`, path `Evaluate`, `MakeUnitOrthoVectors`, placement `SetCanonical`, final `Transform` | propagate before using partial axes or failed output; old WR-632 transform handling retained | same flag; placement regressions; called in checked sweep even when only sweep flag is enabled |
| same: `RotateAboutAxis` | Rust operation is infallible; retains upstream unitization fallback | invariant, no Result discarded |
| `CreateSweepAlongPlanarPath`: first/last/start/end `Evaluate` | `evaluate_keep`: propagate before closure, tangent, reference-point or mitre tests | `planar_path_sweep_statuses` |
| same: profile boxes, `ExtendPathCurves`, `PositionProfileCurves` | all mandatory outputs checked, including transitive helper policy | same flag; real straight-sweep integration with flag alone and all audited flags |
| same: `ComputeCrvsNormal` for path | checked lazily at first mitre, before any consumer; legacy remains eager | same flag; straight path needs no normal and succeeds (`ComputeCrvsNormal` explicitly refuses one linear curve) |
| `SweepProfilesAlongPath`: `Copy` of path | B-spline copy is infallible owned cloning | no status to drop |
| same: `CreateCurveSweep`, both `RemoveFaces` | reject failed sweep before cap removal; reject first failed removal before next stage | same flag; `audit_483_planar_failures_release_owned_bodies_and_stop` |
| mitre and closing-join `CCWAngleBetween` | check before reusing angle accumulator | same flag |
| mitre and closing-join `CreateLineSegment`, every `CreateExtendedCurve` | `line_into` / `extend_into`: error before old output slot can be consumed | same flag; line dimension is fixed 3, ordinary line construction is otherwise infallible |
| curved-join `BrepIntersect` | intentional recovery: discard failed search; use independently constructed temporary cutting line | `audit_483_optional_mitre_search_uses_independent_cutting_line`; no failed intersection output is read; internal port errors still propagate in checked mode |
| curved-join `GlobalCurveSolve` | intentional recovery: skip failed candidate search; successful candidates or independent cutting line remain available | unchanged optional search; no failed solution array is consumed; internal port errors still propagate in checked mode |
| accepted solution `GetAt`, both `Param` and point `Evaluate` calls | `GetAt` already `?`; checked parameters/evaluations propagate before distance ranking | sweep flag; failed accepted values cannot become UNDEF candidates |
| both `ProjectAndTrim` calls | `trim_both`: return first failure before trimming next operand | sweep flag; `audit_483_planar_trim_stops_before_second_operand_after_failure` |
| closed single-piece `StitchFaces`, split `ProjectAndTrim`, `RemoveFaces`, topological removal | mandatory statuses checked; topology queries already `?` | sweep flag |
| postfix union `BooleanTrees`, final stitch, manifold/region classification and region queries | existing `?` retained; both final topological removal attempts now checked | sweep flag; faults at union, stitch, both removals and manifold query |
| sweep scratch allocation/destruction | register ID immediately after `new_brep`, before sweep; on any checked error delete all owned slots; never register caller MAIN; keep returned result on success | sweep flag; real-model cleanup regressions, including multi-piece union failure. Current `PrimitiveModel::delete_brep` is infallible and tolerates already-consumed slots |
| `sm_GetFilletRadius`: `Unitize` and `CCWAngleBetween` | `fillet_radius`: already `?` for angle; unitize has defined `(0,0,1)` fallback | no new flag; existing quarter-turn test; projected-length guard in `CCWAngleBetween` exceeds the subsequent zero-vector threshold |
| `SmFilletGeom::Trim`: rail trims/intersections, center-line trim, surface-domain intersection and surface trim | already corrected by PR #674 (`WR-604`), including successful-prefix behavior | existing `fillet_trim_status`; existing `trim_geom_*` tests and nine native regression fixtures retained |
| `sm_TestG1Continuity`: both tangent `Evaluate`, `AngleBetween` | `evaluate_tangent` / `test_g1_continuity`: propagate before using UNDEF or a default angle | `fillet_corner_statuses`; real box with failed curve regression; successful zero tangents remain `(false,None)` |
| related corner tangency/concavity `EvaluateBinormal` | `evaluate_binormal`: same checked policy before normal/binormal use | same flag; existing corner fixtures |
| Nx1 degeneracy binormal statuses through `SmBoolean` | p1 already returns error; p3 selector now preserves it instead of converting native failure to `true` | same flag; extended real Nx1 evaluator-status regression checks both wrapper and `FilletCorner::create` |
| `CheckAndFixDegeneracy`: matching original vertex-use `ComputeUVPoint` | error before installing UV or setting `degenerate`; earlier topological edits retained | same flag; existing `degeneracy_fix_ignores_a_failed_uv_projection` now tests off/on, seeded UV retained on failure |
| `sm_CheckEdgeCurveDomains`: `Copy`, `Trim` | error before returning copy as trimmed; original edge unchanged; p2 solver callers already use `?` | `fillet_edge_curve_domain_statuses`; collapsed-domain failure and valid subdomain regression |
| `HaveTangentGaps`: each `NormalizedEvaluate`, `ClosestPoint` | `tangent_gap_samples`: first failure propagates before next query/sample | `offset_tangent_gap_statuses`; service-boundary failure injection checks call order |
| same: closest-point success without solution | intentional `Ok(None)` result, not an error and not evidence of a gap | regression covers no-solution, sample parameters and real distance threshold |
| `ALIGN_DIR_WITH_PREVIOUS_POINT`: `AngleBetween` at all 11 expansion sites | `align_with_previous -> Result<()>`, every call uses `?`; policy captured by `SurfaceIntersector::new` | `ssi_alignment_statuses`; zero/tiny/huge/absent/opposite direction tests plus production singularity path and captured-policy regression |
| preceding projection `Unitize` | intentional recovery: a failed projected direction retains original previous derivative; zero original derivative is rejected by checked alignment | no failed projected output is read |
| SSI public `ComputePointValues` singularity call | already returns `SM_ERR` on ordinary singularity failures; retains this documented normalization, never treats failure as success | existing normalization retained; `Unported` stays distinct |

## Input and output evidence

Default Rust behavior is compared to the same pinned C++ inputs by the existing
native unit fixtures, including the three far-origin fixtures added earlier in
this PR and the already-merged nine fillet-trim fixtures. Checked behavior has a
different expected status, so it is verified on Rust with exact originating errors,
service call order, retained prefix edits, valid topology and ownership assertions.
No undefined C++ input is introduced merely to manufacture an error. No tolerance,
case guard, mismatch waiver or report baseline is loosened.

Representative differences: a failed box no longer selects a shift from a default
extent; a collapsed `[0.5,0.5]` edge trim no longer returns the full `[0,1]` copy;
failed UV projection retains the seeded `(4,5)` and leaves the corner unfinished;
a failed tangent-gap evaluation never calls ClosestPoint; a zero previous SSI
direction returns its angle error instead of a successful zero-angle alignment.

## Validation

Implementation revision: `d54dfd46219b2aa441ba6355894bce22855846ac`, including merge
of main `1fcfee382a96fe7c4c7f45768867df3406ccf69b`. The subsequent evidence/header
commit changes documentation only (module comments, records and unit notes).

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- The same clippy gate with `--features cpp`: passed.
- `cargo check -p solidcore --target wasm32-unknown-unknown`: passed.
- `cargo test -p solidcore --profile difftest`: 3358 passed, 0 failed, 22 ignored.
- `cargo test -p difftest --features cpp --profile difftest`: 787 passed, 0 failed,
  37 ignored.
- Unfiltered `--green --jobs 8 --baseline bin/483-exact-base/reports`: 280 units,
  280 green, 0 partial, 0 failed; 591113 cases, 590398 pass, 0 failed/deferred,
  697 existing guard rejections and 18 existing parked C++ defects. No vacuous pass.
- Baseline: exact-base trusted push/main CI run `41178204113`, artifact
  `13034146295`, SHA-256
  `0a095d80ccf40ec5757487a39de02d21cd80b80dc0ad97dd7761b7be2fb4866b`.
  The repository, revision, workflow, digest and complete unit set were validated
  by `scripts/ci/difftest_baseline.py`; the native run passed its coverage ratchet.
- `--perturb --jobs 4` on all 15 touched/reviewed units and necessary callers:
  13887 perturbed cases, 13887 detected, 0 missed, 0 failed units.

The C++ key remains `v1-3fd5b9548d19-726643fb17a52b7d`; no reference rebuild,
tolerance change, new rejection, new parked case or coverage waiver was needed.
Historical reports under `bin/issue-1781-*` are not reused as current-head evidence.


## CI integration after the SSI API changed

The automatic PR merge tested by CI run `41180451358` included PR #1938's
`adv_compute_singularity_point` signature extension (`fix_dupin_bottom_axis`).
The #483 captured-policy regression still supplied three arguments, so both the
workspace test build and all-targets clippy failed with `E0061`; the native sweep,
perturbation and coverage jobs passed. The earlier local results above belong to
implementation revision `d54dfd462`, before that main change.

Merged main `f5475831163a4d3b15d37338fb715249a21c6a0b` and updated the regression to
forward `k.fixes.dupin_bottom_hyperbola_axis`, as the production caller does. It
now checks the four combinations of the alignment-status and Dupin-axis policies.
The focused regression passes on the merged source. This correction changes test
code only; the library changes in the merge are those already accepted on main.
Updated CI verification is recorded in PR #1775.
