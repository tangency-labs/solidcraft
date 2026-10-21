# solidcore status

Last updated: 2026-10-16, PR #127 (`sweep.trans_sweep_geometry` partial: 719 bit-identical passes, 50 deferred on `surface.cone_p2`, 19 rejected native fixtures). Per-unit state and evidence are in `plan/modules.toml` and `reports/difftest/`.

Latest unit: `topology.face_p4` partial (port: topology.face_p4, taking over the #134 claim): 5506 cases, 3957 bit-identical passes, 0 failed, 1549 deferred (mostly the tolerance of face-owned surfaces: `SmTol::GetZoneTol3d` in the surface cache, tagged `geom.tol`, and `SmFace::GetTolerance`, `topology.face_p2`); 26 of its 44 back-edge stubs wired, including those of `heal.stitch_p1`, `topology.brep_p5`/`p7`/`p9`/`p10` and `topology.face_p3` (169 more `face_p3` passes); the rest take generic handles without a B-rep, or (one in `brep_p5`) no kernel; its own calls into `query.face_p3` (`sm_FaceClassifyUVLine`, `SectorClassifyCurveOn`) are wired since #132; `HasSeamProblem` returns `SM_SUCCESS` as TRUE upstream (`plan/upstream-issues.md` WR-176); 238 units (main after #229), 116 green and 122 partial.

Latest fuzz sweep (2026-10-20): `--fuzz 2000` over all 233 green/partial units; 148 re-fuzzed after fixes, FUZZ-FAIL 113 -> 26; PRs fuzz/geom, fuzz/curve, fuzz/nurbs-surface, fuzz/nlib-curve, fuzz/topology-query; findings in `plan/findings.md` ("Fuzz sweep over all green/partial units"), UB-221..UB-237 and HANG-40..HANG-45 in `plan/upstream-issues.md`.

Latest topology integration: `topology.brep_p6` partial (PR #169, after its dependency #154: 498 cases, 450 pass, 48 deferred, 0 failed; 206 units, 113 green and 93 partial); before: `topology.shell` partial (PR #154, continuing @0xlillith's PR #80: 192 ported units, 109 green and 83 partial); before: `surface.surface_p11` partial (PR #120: 176 ported units, 98 green and 78 partial; `intersect.surface_silhouette` promoted to green); before: `mass.integrator` partial and `curve.curve_p3` green (PR #119); PR #115 (`topology.brep_cache` partial). Update a summary line here when your unit changes it; per-unit state lives in `plan/modules.toml` and evidence in `reports/difftest/`.

Latest main integration: issue #76 kernel-aware Surface dispatch (PR #313, #133 revived, `SurfaceKind::Cone`): 234 units, 117 green / 117 partial, 0 failed; 534,436 cases, 510,383 pass, 23,933 deferred (before 493,865 / 24,694); `intersect.surface_intersector_p1`, `intersect.surface_intersector_p4`, `surface.surface_p11` promoted to green. Before: `surface.surface_p11` partial (PR #120: 176 ported units, 98 green and 78 partial; `intersect.surface_silhouette` promoted to green).

Latest unit: `boolean.merge_p2` ([PR #229](https://github.com/tangency-labs/solidcore/pull/229)) is partial: 45 differential cases, 30 bit-identical passes, 15 dependency deferrals, zero failures (`--seeds 1000`: 631 passes, 398 deferrals). It ports the `SmMerge` B-rep constructor, `MergeEdge`, `MakeFaceuse`, `MergeAddedFace` and `MergeAddedTopology`, with its stitch (`heal.stitch_p1` / `_p2`), intersector (`boolean.topology_intersector_p2`), `MakeFace` (`topology.brep_p7`), region and `topology.brep_p2` calls wired, and wires the `boolean.merge_p2` back-edges of `boolean.merge_p3`, `boolean.merge_p4`, `boolean.merge_p5`, `boolean.api_brep`, `heal.stitch_p1` and `topology.brep_p7`; its constructor builds `boolean.merge_p3`'s `MergeEngine` (#287), so the two-B-rep API booleans now run `ManifoldBoolean` (`boolean.merge_p4`) and stop at `query.brep`. It confirms UB-176 on the reference build (through `MergeAddedTopology`) and adds WR-197. The manifest has 235 ported units (114 green, 121 partial).

Latest unit: `surface.surf_of_revolution_p3` partial -> green (difftest runner builds the `SmCone` operand and runs `IntersectWithCone`: 26705 cases, 26705 bit-identical, 0 deferred; 233 ported units, 115 green and 118 partial). Before: `intersect.curve_class_p9` ([PR #151](https://github.com/bherbruck/solidcore/pull/151)) is partial on a standalone official-main base: review validation has 424 differential cases, 225 bit-identical passes, 197 dependency deferrals, 2 native setup rejections and zero failures. It ports edge and trim-boundary intersections with concrete query handle mappings. Pole lookup now matches the native FALSE default; collapsed caches, wider input ranges and positive endpoint classification are covered. Interior partition insertion awaits part 2. No open dependency PR is bundled. The manifest has 195 ported units (108 green, 87 partial); final integration results are recorded in the PR checks.


## Port review note (2026-10-18, issue #210)

Carry forward [issue #210](https://github.com/tangency-labs/solidcore/issues/210)
when extending or reviewing partial ports:

- Stub the missing call and retain the upstream logic after it. A function that stops
  at its first missing callee does not establish completion of its assigned body.
- Separate successful operations that change geometry/topology from refusals, empty
  results, no-ops and guard assertions. `partial`, `max_rel_err=0` and `with_values`
  alone do not certify the success paths; record the unexercised operations explicitly.
- Treat B-rep `Notify` integration as a critical dependency for meaningful topology
  coverage, and coordinate the canonical `Brep` fields and editing state across ports.
- Review defaults and unreachable tails against upstream before their dependencies
  land. Merge current main before readiness, wire stale stubs, and rerun affected
  topology units with expanded seeds when the shared dependencies become available.

These are review/handoff notes from the issue, not a claim that its proposed process
or reporting changes have already been implemented. For cone follow-up #207, the
rejection fixtures compare only the native guard (Rust runs on them but its result is
not compared) and the missing general Cone adapter remains an explicit dependency
deferral.


## Integration update (2026-10-15, PR #119)

The manifest now has 173 ported units: 97 green and 76 partial. `mass.integrator`
has 1520 passing cases and eight explicit dependency deferrals out of 1528, with
maximum relative error `4.979280456316938e-14` and no absolute floors. Review
regressions cover circular holes, curved B-spline surfaces, rational edges and nested area strips; each Cash–Karp derivative array
now retains its own values when a trim has zero U tangent. All three
integration back-edges are wired. `curve.curve_p3` is green (3076/3076 bit-identical),
and the 28 formerly deferred curved-length cases of `curve.curve_p5` now pass.
Precise edge/face/B-rep properties and their API control flow use explicit calls
for the remaining topology, bounding-box and offset dependencies. Detailed coverage
and the scalar wrapper's failure-masking behavior are in `plan/findings.md`.


Latest unit: `surface.cone_p3` ([PR #131](https://github.com/bherbruck/solidcore/pull/131)) adds analytic cone/cylinder plane sections and cone/cone intersections: 1308 differential cases, 1187 bit-identical passes, 121 dependency deferrals, zero failures. Ellipse trimming awaits `surface.cone_p2`; coincident boundaries await common Cone/Cylinder boundary adapters in #76/#133. The enum now retains both types, while analytic iso/boundary integration remains deferred. Shared cone/sphere enum dispatch remains coordinated in issue #76. Review follow-up narrows the native guard to exact cone dispatch, adds tolerance/constructor/flag regressions with native FFI safety tests validated locally.

## Handoff (2026-10-13): from one local swarm to pull requests

The original local swarm (one machine, up to 10 agents) was stopped here so that others
can contribute. From now on all work goes through PRs; see AGENTS.md section 4.

- **Where things get ticked off:** `plan/PROGRESS.md` (checklist, regenerated on `main`
  by `.github/workflows/progress.yml` from `plan/modules.toml` and open `port: <id>` PRs).
  The source of truth stays `plan/modules.toml`; evidence per unit is
  `reports/difftest/<unit>.json`.
- **Ready now:** `curve.curve_p9`, `surface.plane_p1`, `surface.surf_of_revolution_p1`,
  `surface.surface_p9`. `surface.trim_srf_cache` is being contributed in PR #4,
  resuming the interrupted `origin/unit/surface.trim_srf_cache` WIP.
- **Throughput limit:** the dependency chain, not people. Often only 2 to 5 units are
  ready at once. A useful non-port task: find `depends_on` links that are only 1 or 2
  calls and could become back-edges (section 4a), which would widen the ready set.
- **CI was red on `main` before this handoff** (newer clippy on GitHub; a test-only FFI
  buffer that needed ~37 GB on GitHub's runner). Fixed here: toolchain pinned to
  1.98.0, and `topology.database_io`'s FFI wrapper now sizes buffers exactly.
- **Last unit, after everything else:** `audit.recursion` (bounds input-driven recursion;
  wasm has no stack guard page).

## Progress

Counts are from `plan/modules.toml`. Line counts are the upstream function-body lines
(`est_lines`) assigned to each unit.

| | |
|---|---|
| Units planned | 261 (65 layers), 387,903 upstream lines |
| Green | **84** (105,920 lines): `geom.extent3d`, `geom.vector2d`, `nlib.knots`, `nlib.math`, `topology.database_io`, `geom.tol`, `nlib.geom`, `geom.assert_array`, `geom.context`, `geom.extent1d`, `geom.matrix`, `geom.periodic_extent1d`, `geom.vector3d`, `nlib.basis_basic`, `nlib.srf_basic`, `geom.axis2_placement`, `geom.extent2d`, `geom.geom_utility_p2`, `geom.hash_table`, `geom.local_solve1d`, `geom.local_solve_nd`, `geom.polynomial`, `nlib.basis_adv`, `nlib.bezier`, `nlib.funcs_basic`, `geom.geom_utility_p1`, `geom.polar_box`, `nlib.crv_basic`, `geom.pseudo_box`, `nlib.crv_fit_p1`, `nlib.crv_tool_p1`, `nlib.iges`, `nlib.srf_tool_p1`, `curve.hermite_curve`, `geom.point_set`, `nlib.crv_fit_p2`, `nlib.crv_nurb`, `nlib.funcs_adv`, `nlib.srf_fit`, `nlib.crv_fit_p3`, `nlib.crv_geom`, `nlib.crv_tool_p2`, `nurbs.nurbs_crv`, `topology.topology`, `nlib.crv_adv_p1`, `nlib.crv_common`, `nlib.spiral`, `nlib.crv_adv_p2`, `nlib.srf_common`, `nlib.crv_approx`, `nlib.crv_offset`, `nlib.srf_nurb`, `nlib.srf_tool_p2`, `nurbs.b_spline_curve_n_lib`, `nurbs.nurbs_srf`, `nlib.srf_adv_p2`, `nlib.srf_adv_p3`, `nlib.srf_tool_p3`, `nlib.srf_tool_p4`, `curve.conic`, `curve.projected_curve`, `nlib.srf_adv_p4`, `nlib.srf_adv_p1`, `nlib.srf_adv_p5`, `nlib.srf_adv_p6`, `nlib.srf_offset`, `curve.offset_curve`, `surface.b_spline_volume`, `nurbs.b_spline_curve_p1`, `curve.parabola`, `primitive.api_curves`, `nurbs.b_spline_curve_p2`, `curve.curve_p2`, `curve.curve_cache`, `curve.hyperbola`, `curve.line`, `curve.ellipse_p2`, `curve.curve_p6`, `curve.curve_p7`, `curve.ellipse_p1`, `curve.curve_p8`, `curve.composite_curve_p1`, `curve.composite_curve_p2`, `curve.curve_p9` |
| Partial | **31** (41,289 lines): `geom.attribute`, `geom.tree`, `geom.object`, `geom.grid`, `geom.solution_array`, `geom.global_solver`, `geom.tools`, `geom.transform`, `geom.a_object`, `nurbs.b_spline_surface_p1`, `nurbs.b_spline_curve_p3`, `curve.crv_in_volume`, `curve.tangent_field`, `nurbs.b_spline_surface_n_lib_p1`, `curve.iso_curve`, `nurbs.b_spline_surface_n_lib_p2`, `nurbs.b_spline_surface_p2`, `surface.volume_p1`, `curve.curve_p4`, `surface.twist_volume`, `curve.curve_p3`, `curve.curve_p1`, `curve.crv_on_surf`, `surface.surface_p2`, `surface.curve_bounded_surface`, `surface.surface_cache_p1`, `curve.curve_p5`, `surface.trim_srf_cache`, `surface.plane_p1`, `surface.unbend_volume`, `surface.bend_volume` |
| Ported (green + partial) | 115 units, 147,209 upstream lines (38.0%); about 150,000 Rust lines in `crates/solidcore/src` |
| Claimed | 0 (claims now use draft PRs) |
| Blocked | 0 |
| Todo | 146 (240,694 lines) |

## What Wave 0 produced

- **C++ reference kernel** (agent A). `harness/cpp/build.sh` builds upstream SMLib and
  SM_API at c9c5979 with CMake and Ninja into `bin/cpp/` (static libs in
  `bin/cpp/lib`, plus `compile_commands.json` and `ffi.env`). It uses no TBB and makes no
  upstream patches. `harness/cpp/run_tests.sh` runs the upstream suites. Details are in
  `plan/baseline.md`.
- **Port plan** (agent B). The plan is in `plan/modules.toml` (260 units),
  `plan/order.md` and `plan/closure.md`. The call-graph tooling is in `harness/callgraph/`
  (`run.sh`, `units.json`, `why.py`).
- **Scaffolding** (agent C). This covers LICENSE, NOTICE, ATTRIBUTION, README, AGENTS.md,
  workspace lints, CI and the `crates/solidcore` skeleton (`math`, `error`, `kernel`,
  `topology` arenas, and module stubs).
- **difftest harness** (agent D). It has three parts: `crates/solidcore-ffi` (shims built
  from `harness/cpp/shim/*.cpp`), `crates/difftest` (runner, comparison and reports) and
  the `harness.selftest` unit. To add a unit you create 4 new files and edit no shared
  file. See `crates/difftest/README.md`.
- **Wave 0 close** (this pass):
  - Integration check, all green: build.sh, selftest difftest, workspace
    fmt/clippy/tests and the wasm32 check.
  - Fixed the docs to use `difftest --unit <id>`.
  - Added `SolidError::Nlib` / `NlibCode`.
  - Ported the first unit and wrote `plan/findings.md`.

## Baseline (C++ reference, `plan/baseline.md`)

- **Build:** a clean build with `-j6` under `nice` takes 111 s (271 Ninja steps). An
  incremental build takes about 7 s, and a no-op build under 1 s.
- **SM_API_test_app:** PASS. It reports 115 calls; that is 112 real tests passing, 0
  failing and 2 skipped. The two skipped tests are stubbed because they compile
  `upstream/Examples`. Runtime is 50 s.
- **prog_test_app:** PROG_TEST SUCCESS (16 default suites, 46 s). All 17 suites pass when
  run one at a time, including `stitch`.

## Green units

| Unit | Rust module | difftest | Notes |
|---|---|---|---|
| `harness.selftest` | (harness only) | 28 cases, max_rel_err 0 | not a plan unit |
| `nlib.knots` | `nurbs::knots` | 2953 cases (1753 fixture + 1200 generated), max_rel_err 0 | bit-identical; a 51,753-case run was also clean |
| `geom.vector2d` | `geom::vector2d` | 4060 cases (60 fixture + 4000 generated), max_rel_err 0 | bit-identical; a 200,060-case run was also clean |
| `topology.database_io` | `topology::database_io` | 2059 cases (59 fixture + 2000 generated), max_rel_err 0 | stream state and position match per step; a 200,059-case run was also clean |
| `nlib.math` | `nurbs::math` | 3076 cases (76 fixture + 3000 generated), max_rel_err 0 | bit-identical; a 100,076-case run was also clean |
| `geom.periodic_extent1d` | `geom::periodic_extent1d` | 4054 cases (54 fixture + 4000 generated), max_rel_err 0 | bit-identical; upstream quirks in findings.md |
| `geom.assert_array` | `geom::assert_array` | 3734 cases (14 fixture + 720 label-table + 3000 generated), max_rel_err 0 | bit-identical; all 86 label tables ported here, lengths checked against libSMLib.a symbol sizes |
| `geom.matrix` | `geom::matrix` | 4044 cases (44 fixture + 4000 generated), max_rel_err 0 | bit-identical; a 100,044-case run was also clean; upstream quirks in findings.md |
| `geom.extent1d` | `geom::extent1d` | 3091 cases (91 fixture + 3000 generated), max_rel_err 0 | bit-identical; wired extent3d intervals and tol MapTo1d / GetScaledZero(SmExtent1d) |
| `geom.polynomial` | `geom::polynomial` | 6038 cases (38 fixture + 6000 generated), max_rel_err 0 | bit-identical; quartic UB with tol > 1 noted in findings.md |
| `nlib.srf_basic` | `nurbs::srf_basic` | 3220 cases (220 fixture incl. Surface.sms / offset_nurb.sms / DropCurveSurface.sms + 3000 generated), max_rel_err 0 | bit-identical; a 100,220-case run was also clean; NL_SURFACE as `Surface` (storage/index split), quirks in findings.md |
| `geom.extent2d` | `geom::extent2d` | 3069 cases (69 fixture + 3000 generated), max_rel_err 0 | bit-identical; a 30,069-case run was also clean; wired tol GetScaledZero(SmExtent2d); PeriodicWrap hang note in findings.md |
| `nlib.basis_basic` | `nurbs::basis_basic` | 2596 cases (1096 fixture incl. every pt_TestFiles .smc/.sms knot vector + 1500 generated), max_rel_err 0 | bit-identical; a 201,096-case run was also clean; upstream endless loops / OOB reads noted in findings.md |
| `geom.local_solve_nd` | `geom::local_solve_nd` | 3043 cases (43 fixture + 3000 generated), max_rel_err 0 | bit-identical (full Evaluate trace of a shared test equation set compared); a 30,043-case run was also clean; wired tol GetScaledZero(SmEvalNFunctionsObject); quirks in findings.md |
| `geom.hash_table` | `geom::hash_table` | 619 cases (19 fixture incl. SmHealData coincident-vertex replays + 600 generated), max_rel_err 0 | bit-identical; a 10,019-case run was also clean; upstream use-after-free and quirks in findings.md |
| `geom.local_solve1d` | `geom::local_solve1d` | 3316 cases (316 fixture + 3000 generated), max_rel_err 0 | bit-identical (full Evaluate trace of a shared test function, outputs and solver state compared); a 20,246-case run was also clean; interval-less constructor and other quirks in findings.md |
| `geom.polar_box` | `geom::polar_box` | 3030 cases (30 fixture programs incl. pt_TestFiles control-polygon edges + 3000 generated programs on 4 box registers), max_rel_err 0 | bit-identical; Union UB case (stale basis after ReSet), aliased Union and region-to-arc quirk in findings.md |
| `nlib.bezier` | `nurbs::bezier` | 2223 cases (223 fixture incl. SM_API_test rectangle/polygon segments, FitCrvPoints.pts, Points.pts, Surface.sms + 2000 generated), max_rel_err 0 | bit-identical; a 50,223-case run was also clean; dangling-matrix, NL_PascalTri bounds and recursion-limit quirks in findings.md |
| `nlib.funcs_basic` | `nurbs::funcs_basic` | 3130 cases (130 fixture + 3000 generated), max_rel_err 0 | bit-identical; a 50k-seed run was also clean; notes in findings.md |
| `nlib.basis_adv` | `nurbs::basis_adv` | 1744 cases (244 fixture + 1500 generated), max_rel_err 0 | bit-identical; a 100,244-case run was also clean; `N_BasisDerivsArray` unbounded span walk and `N_BasisInsertKnots` `X[mx+1]` read in findings.md |
| `geom.geom_utility_p1` | `geom::geom_utility::p1` | 4185 cases (185 fixture incl. SM_API_test rectangle/polygon, FitCrvPoints.pts, Points.pts, PointSet sections + 4000 generated), max_rel_err 0 | bit-identical; file-static helpers tested via a namespaced compile of SmGeomUtility.cpp; best-fit-line eigenvector, silhouette and FALSE-status quirks in findings.md |
| `geom.axis2_placement` | `geom::axis2_placement` | 4048 cases (48 fixture + 4000 generated), max_rel_err 0 | bit-identical; promoted from partial when `AreCoaxial` was wired to `geom.geom_utility_p1` |
| `geom.geom_utility_p2` | `geom::geom_utility::p2` | 3270 cases (70 fixture + 3200 generated), max_rel_err 0 | bit-identical; promoted from partial when its p1 back-edges were wired; `PointSetType` is p1's |
| `geom.extent3d` | `geom::extent3d` | 3086 cases (86 fixture + 3000 generated), max_rel_err 0 | bit-identical; promoted from partial when `geom.pseudo_box` wired `Circumscribe` (axis-map quirk in findings.md) |
| `geom.pseudo_box` | `geom::pseudo_box` | 3011 cases (11 fixture incl. my_test_pseudobox + 3000 generated programs on 4 box registers), max_rel_err 0 | bit-identical; wired extent3d Circumscribe, tol GetScaledZero(SmPseudoBox), transform EvaluateBoundingBoxSimple (aligned branch) |
| `nlib.funcs_adv` | `nurbs::funcs_adv` | 2080 cases (80 fixture incl. 6 pt_TestFiles .smc curves + 2000 generated), max_rel_err 0 | bit-identical; the three upstream products share one generic body; inputs rescaled in place as upstream |
| `nlib.crv_tool_p1` | `nurbs::crv_tool::p1` | 2143 cases (143 fixture on the six pt_TestFiles .smc curves, in-place and out-of-place C++ forms + 2000 generated), max_rel_err 0 | bit-identical; promoted from partial when `nlib.crv_nurb` wired `N_CrvGetMinMaxWeightsAndPts` (rational `N_CrvRemoveKnot`) |
| `nlib.srf_fit` | `nurbs::srf_fit` | 1556 cases (56 fixture on the TestSmCreateSurfaceFromPoints 5x7 grid and the 11 PointSet sections + 1500 generated), max_rel_err 0 | bit-identical; a 30,056-case run was also clean; N_FitSrfInterpParams always returns NL_NO, LstSqApprox ruling overwrites the caller grid (copy in Rust) |
| `geom.tol` | `geom::tol` | 3440 cases, max_rel_err 0 | bit-identical; `SmRelation` pointer-hash order sorted on both sides |
| `geom.context` | `geom::context` | 2013 cases (13 fixture scripts + 2000 generated), max_rel_err 0 | bit-identical; wired the `Option<&Context>` overloads of `geom.tol` |
| `nlib.geom` | `nurbs::geom` | 3173 cases (173 fixture + 3000 generated), max_rel_err 0 | bit-identical |
| `geom.vector3d` | `geom::vector3d` | 6114 cases (114 fixture from my_test_vector3d + edge cases, 6000 generated), max_rel_err 0 | bit-identical; `Vec3` lives here |
| `nlib.iges` | `nurbs::iges` | 2029 cases (29 fixture + 2000 generated), max_rel_err 0 | bit-identical; entity data passed as `Entity126` / `Entity128` |
| `nlib.srf_tool_p1` | `nurbs::srf_tool::p1` | 3132 cases (132 fixture + 3000 generated), max_rel_err 0 | bit-identical; `extract_patch` takes a `Rectangle` |
| `nlib.crv_fit_p1` | `nurbs::crv_fit::p1` | 2084 cases (84 fixture + 2000 generated), max_rel_err 0 | bit-identical; `M[2i+1][-1]` write ported as is (findings.md) |
| `nlib.crv_fit_p3` | `nurbs::crv_fit::p3` | 2242 cases (242 fixture on FitCrvPoints.pts / Points.pts and the upstream driver call shapes, edge cases + 2000 generated), max_rel_err 0 | bit-identical; errors upstream ignores in the end-derivative estimate are returned, `NL_INHERITED` reads the caller curve's knots (findings.md) |
| `nlib.spiral` | `nurbs::spiral` | 2032 cases (32 fixture from the helix tests and edge cases + 2000 generated), max_rel_err 0 | bit-identical; upstream crashes on its early input errors (ffi rejects them), span-limit `*NumPoints` quirk ported (findings.md) |
| `geom.point_set` | `geom::point_set` | 2029 cases (29 fixture + 2000 generated), max_rel_err 0 | bit-identical; unprojected tight-box angles and centroid block quirk kept (findings.md) |
| `topology.topology` | `topology::topology` | 657 cases (57 fixture + 600 generated scripts), max_rel_err 0 | bit-identical; `SmTopologyChangeCallback::Execute` unused upstream and not ported; dangling-callback UB on tracker destruction skipped (findings.md) |
| `nlib.crv_nurb` | `nurbs::crv_nurb` | 2871 cases (871 fixture on the six pt_TestFiles .smc curves, SM_API_test rectangle/polygon/circle and reversed-line curves, edge cases + 2000 generated), max_rel_err 0 | bit-identical; promoted from partial when `nlib.crv_geom` wired `N_CrvArcLength` (`N_CrvGetDegenSegs`) |
| `nlib.crv_geom` | `nurbs::crv_geom` | 1723 cases (223 fixture on the six pt_TestFiles .smc curves + edge cases, 1500 generated incl. circles, lines, degenerate and closed curves), max_rel_err 0 | bit-identical; closest-point best values on failure and polyline non-termination quirks in findings.md |
| `nlib.crv_tool_p2` | `nurbs::crv_tool::p2` | 2124 cases (124 fixture on the six pt_TestFiles .smc curves and curve pairs, in-place and out-of-place C++ forms, degenerate-segment edge cases + 2000 generated), max_rel_err 0 | bit-identical; RemoveKnots / ReduceDegree(Once) / MakeCompatible* / RemoveDegenSegs; wired N_CrvsMakeCompatible into crv_basic |
| `nurbs.nurbs_crv` | `nurbs::nurbs_crv` | 4089 cases (1089 fixture on the six pt_TestFiles .smc curves, SM_API_test rectangle/polygon edges and edge cases + 3000 generated), max_rel_err 0 | bit-identical; `sm_BasisEval` flags the domain ends, `sm_CrvTrim` returns `NL_INP_ERR` (findings.md) |
| `nlib.crv_basic` | `nurbs::crv_basic` | 3417 cases (417 fixture incl. the six pt_TestFiles .smc curves, .sms cross-boundary derivatives, SM_API_test rectangle/polygon segments, my_test_reversed_line_join lines + 3000 generated), max_rel_err 0 | bit-identical; promoted from partial when `nlib.crv_tool_p2` wired `N_CrvsMakeCompatible` (`N_CrvsAreCoincident` maps its NLib errors to false, as upstream) |
| `nlib.crv_adv_p1` | `nurbs::crv_adv::p1` | 2670 cases (670 fixture on the six pt_TestFiles .smc curves, SM_API_test rectangle/hexagon edges and edge cases + 2000 generated), max_rel_err 0; a 100,670-case run was also clean | bit-identical; `ST_curxcdr` / `ST_curxcrr` as `Curve::extend_by_*_at_depth`; in-place `ST_curxcrr` G2 recursion frees garbage upstream (findings.md) |
| `nlib.crv_adv_p2` | `nurbs::crv_adv::p2` | 2815 cases (815 fixture on the six pt_TestFiles .smc curves, SM_API_test rectangle/hexagon edges and edge cases + 2000 generated), max_rel_err 0; a 300,815-case run was also clean | bit-identical; remove_knots shares its body with N_CrvRemoveKnotsDerivConstraints; rational N_CrvSumDiffCrv over different ranges corrupts the heap upstream (findings.md) |
| `nlib.crv_approx` | `nurbs::crv_approx` | 1646 cases (146 fixture: sm_ApproxCircArc-style counts and arc fits, ApproximateRationalCurve-style approximations of the six pt_TestFiles .smc curves and exact NURBS circles, edge cases + 1500 generated), max_rel_err 0 | bit-identical; degree 2 parabolic arcs with parallel tangents and failed tangent projections use uninitialised points upstream (ffi rejects them; the port errors) |
| `curve.hermite_curve` | `curve::hermite_curve` | 2131 cases (131 fixture + 2000 generated), max_rel_err 0 | green once `curve.curve_p2` wired `SmCurve::IsKindOf` (93 deferred before) |
| `nurbs.b_spline_curve_n_lib` | `nurbs::b_spline_curve_n_lib` | 2282 cases (282 fixture + 2000 generated), max_rel_err 0; a 100,282-case run was also clean | green once `curve.curve_p2` wired `SmCurve::ApproximateLength` (57 deferred before) |
| `curve.conic` | `curve::conic` | 3240 cases (240 fixture: all 15 smCurveTest.cpp IntersectConic scenarios with their solution-count asserts, MakeDupin indicatrix conics, edge cases + 3000 generated), max_rel_err 0; `--perturb` SENSITIVE | bit-identical; first `CurveKind` added after Hermite; wired `SmCurve::ReadFromDB(SmConic_TYPE)` in curve.curve_p2; `Evaluate` zero-tangent fix reads an unwritten slot upstream (findings.md) |
| `curve.projected_curve` | `curve::projected_curve` | 2727 cases (227 fixture: the SmTransSweepGeometry parallel and SmRotationalSweepGeometry rotation projections of a FitCrvPoints cubic, perspective views, ownership-gated edits, MakeOrthoNormal edge cases, ReadFromDB streams v34/v36 ASCII and binary + 2500 generated), max_rel_err 0; `--perturb` SENSITIVE | bit-identical; `CurveKind::Projected`; wired `SmCurve::ReadFromDB(SmProjectedCurve_TYPE)` in curve.curve_p2; upstream MakeOrthoNormal only unitizes zero vectors and Transform needs UNSURE ownership (findings.md) |
| `nurbs.b_spline_curve_p1` | `nurbs::b_spline_curve::p1` | 2216 cases (216 fixture on the six pt_TestFiles .smc curves, smSweepTest lines/circles, edge cases + 2000 generated), max_rel_err 0; a 30,216-case run was also clean | green once `curve.curve_p1` wired `SmCurve::TessellateByBisection` (4 deferred before) |
| `curve.parabola` | `curve::parabola` | 3044 cases, max_rel_err 0, 0 deferred | bit-identical; `CurveKind::Parabola` runs the `SmBSplineCurve` methods on its base (as Hyperbola); `GlobalPointSolveSTEP` wired to `curve.curve_p1` at merge (206 deferred before) |
| `nurbs.b_spline_curve_p2` | `nurbs::b_spline_curve::p2` | 2328 cases (328 fixture: the six pt_TestFiles .smc curves through every op and via ReadFromDB, my_test_reversed_line_join lines, SM_API_test rectangle/polygon joins, arc/line joins, Bezier box cases, edge cases + 2000 generated), 2239 bit-identical; a 20,328-case run was also clean | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.curve_p2` | `curve::curve::p2` | 3021 cases (21 fixture: prog_test line and circle curves, `ApproximateLength(ivl, 20)`, circle seam continuity, Hermite base defaults, `ReadFromDB` dispatch incl. conics + 3000 generated over B-spline, Hermite and arc curves, static helpers, solver box tests), 3016 bit-identical; `--perturb` SENSITIVE | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.curve_cache` | `curve::curve_cache` | 638 cases (38 fixture: lines, cubics, C0 quadratic, closed, degenerate, 2D, rational arc and Hermite curves through `Tessellate` with every option, sub/beyond/disjoint/reversed intervals, `GetAnalyticCurve`, type queries + 600 generated), 636 bit-identical | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.hyperbola` | `curve::hyperbola` | 2062 cases (62 fixture: smCurveTest.cpp my_test_analytic_curves hyperbola (CreateCanonical, AdjustSTEPInterval(-1,2), evaluation, GlobalPointSolveSTEP, ReverseParameterization), the smSurfaceTest.cpp extrusion-generator hyperbola, edge cases, ReadFromDB records + 2000 generated), 2030 bit-identical; `--perturb` SENSITIVE | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.line` | `curve::line` | 2103 cases (103 fixture: my_test_reversed_line_join, smTopoTest canonical lines, my_test_analytic_curves / my_test_bsplinecurve_nlib lines, SM_API lines, edge cases + 2000 generated op programs), 2102 bit-identical; a 40,103-case run was also clean; `--perturb` SENSITIVE | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.ellipse_p2` | `curve::ellipse::p2` | 2102 cases (102 fixture: my_test_analytic_curves circle/ellipse, my_test_drop_curve_near_seam circles, smCurveTest circle/circle and line/circle configurations as SmEllipse, edge cases + 2000 generated op programs), 2005 bit-identical; a 30,102-case run was also clean; `--perturb` SENSITIVE; constructor co-ported from curve.ellipse_p1 | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.curve_p6` | `curve::curve::p6` | 3136 cases (136 fixture: SM_API_test rectangle / hexagon edges (boxes, planarity, coincidence, gaps, drops), my_test_reversed_line_join line pairs, smCurveTest DeformToEndPointTargets B-spline and line steps, circle boxes, edge cases + 3000 generated), 3048 bit-identical; 30k-seed run clean; `--perturb` SENSITIVE | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.curve_p7` | `curve::curve::p7` | 3027 cases (27 fixture: SM_API_test rectangle / hexagon edge pairs, my_test_reversed_line_join segments, arc plane intersection / classification with comb, edge cases: coincident, tolerance-limit gaps, degenerate Hermite, self-intersecting loop, projections + 3000 generated: whole `SolveIt` and single leaf-pair `LocalSolve`, `GlobalPropertyAnalysis` over all 29 properties), 1716 bit-identical; `--perturb` SENSITIVE; co-ported `sm_FindAdditionalCcTangentGuess` | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.ellipse_p1` | `curve::ellipse::p1` | 2028 cases (28 fixture: my_test_analytic_curves circle/ellipse, smSurfaceTest / smTopoTest / smCurveTest circles (CreateCanonical, AdjustSTEPInterval, EditParameterization, copy), edge cases + 2000 generated programs over the constructors, CreateCanonical and ReadFromDB records), 1896 bit-identical; `--perturb` SENSITIVE; `CurveKind::Ellipse` (ellipses and circles); `&Kernel` threaded through the Curve methods whose ellipse override rebuilds the polar converter | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.curve_p8` | `curve::curve::p8` | 3058 cases (58 fixture: SM_API_test rectangle / polygon edge pairs, my_test_reversed_line_join line pairs, with and without the coincidence check; edge cases: line and B-spline line crossings / overlaps / self intersections, vertex touch, parabola tangency, parallel gaps at the tolerance limit, coincident cubics, degenerate Hermite, looping self intersection, disjoint boxes + 3000 generated `GlobalCurveIntersect` calls), 1968 bit-identical; `--perturb` SENSITIVE; wired the `TrimGap` stub in nurbs.b_spline_curve_p3 | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.composite_curve_p1` | `curve::composite_curve::p1` | 3093 cases, all bit-identical | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |
| `curve.composite_curve_p2` | `curve::composite_curve::p2` | 661 cases, all bit-identical | promoted to green by `curve.curve_p5` (0 deferred after its stubs were wired) |

## Partial units

Every non-deferred difftest case passes; deferred cases wait on the listed unit (never reported as green).

| Unit | Rust module | difftest | Deferred on |
|---|---|---|---|
| `geom.tree` | `geom::tree` | 618 cases (18 fixture + 600 generated), 611 bit-identical; a 20,018-case run was also clean | 7 (`surface.surface_cache_p3`: `GetBezierPtr` and `GetGwNurbPointer` read the cache's patch data, which a bare tree reaches once the tree construction hands it over; `GetOwnerObject` of surface trees wired by surface.surface_cache_p1) |
| `geom.attribute` | `geom::attribute` | 2029 cases (29 fixture + 2000 generated), 1991 bit-identical; a 200,029-case run was also clean | 38 (`topology.shell`: merge-region Merge) |
| `geom.object` | `geom::object` | 624 cases (24 fixture + 600 generated), 593 bit-identical | 31 (every non-empty `SmShape`: `topology.face_p2` 11, `topology.brep_p1` 10, `topology.vertex` 6, `topology.edge` 4) |
| `geom.grid` | `geom::grid` | 615 cases (15 fixture + 600 generated), 613 bit-identical; a 30,015-case run was also clean | 2 (`surface.surface_cache_p3` 1: `SmSurfaceGridElement::Init` needs a bare tree node's patch data; `surface.surface_p1` 1: `ExpandHierarchy` via `smsurf_GetSurfaceCache` / `CacheMakeOrValidate`) |
| `geom.solution_array` | `geom::solution_array` | 810 cases (10 fixture + 800 generated), 754 bit-identical | 56 (object geometry: `curve.curve_p2` 16, `topology.face_p2` 14, `topology.edge` 12, `surface.surface_p2` 9, `topology.vertex` 5) |
| `geom.a_object` | `geom::a_object` | 2040 cases (40 fixture + 2000 generated), 1872 bit-identical; a 50,040-case run was also clean | 168 (`topology.brep_cache` 131: new `SmBrepCache`; `topology.shell` 37: `SmRegion::IsVoid`) |
| `geom.tools` | `geom::tools` | 913 cases, 721 bit-identical | 192 (`curve.curve_p2` 68, `topology.brep_p1` 64, `topology.vertex` 60) |
| `geom.transform` | `geom::transform` | 3042 cases, 3039 bit-identical | 3 (`surface.volume_p2`: `SmVolume::ReadFromDB` / `EvaluatePoint`); `surface.volume_p1` wired |
| `geom.global_solver` | `geom::global_solver` | 620 cases, 611 bit-identical | 9 (`curve.curve_p2`: opaque solution objects not yet resolvable to a `Curve`, see findings) |
| `nurbs.b_spline_surface_p1` | `nurbs::b_spline_surface::p1` | 2120 cases (120 fixture on the three pt_TestFiles .sms surfaces incl. ReadFromDB, prog_test cone/bilinear patches, canonical, sweep, edge cases + 2000 generated), 2058 bit-identical | 62 (`surface.surface_p1` 43: offset, degenerate-curve test, STEP inversion solves; `surface.surface_p7` 19: `EvaluateNormal` in polar boxes; `IsKindOf` wired by surface.surface_p2) |
| `nurbs.b_spline_curve_p3` | `nurbs::b_spline_curve::p3` | 2195 cases (195 fixture: the six pt_TestFiles .smc curves through every op, my_test_curve_interface JoinWith / GetControlPoint, my_test_reversed_line_join lines, SM_API_test rectangle/polygon joins and gaps, edge cases + 2000 generated), 2036 bit-identical | 159 (`curve.curve_p5` 159; `GlobalCurveIntersect` wired by curve.curve_p8, `GlobalPropertyAnalysis` by curve.curve_p7, `SmCurve::AssertValid` by curve.curve_p4, `SmCurve::IsLine` co-ported by curve.curve_cache) |
| `nurbs.b_spline_surface_n_lib_p1` | `nurbs::b_spline_surface_n_lib::p1` | 665 cases (65 fixture: my_test_bsplinesurface_nlib offset/sweep/swung, my_test_surface_interface ops on the surf1 net, surf1 Coons patches, the three pt_TestFiles .sms surfaces and 3D .smc curves through every op, edge cases + 600 generated), 638 bit-identical; `--perturb` SENSITIVE | 27 (`surface.surface_p5`: `SmSurface::IsSingularity` in `CreateExtendedSurface`) |
| `nurbs.b_spline_surface_n_lib_p2` | `nurbs::b_spline_surface_n_lib::p2` | 654 cases (54 fixture: my_test_surface_interface skinning of surf1 iso curves, smAdvancedTest TrimWithDomain fractions, the three pt_TestFiles .sms surfaces through Evaluate / TrimWithDomain / skinning / bilinear corner blends, degenerate-side zero-tangent fixes, edge cases + 600 generated), 628 bit-identical; `--perturb` SENSITIVE; 20k-seed run clean | 26 (`curve.crv_on_surf` 17: `ApproximateCurve` in G1 corner blends; `surface.surface_p7` 5: `EvaluateNormal` in n-sided blends; `curve.curve_p5` 4) |
| `nurbs.b_spline_surface_p2` | `nurbs::b_spline_surface::p2` | 763 cases (63 fixture: my_test_surface_interface surf1 through SwapUV / iso curves / SplitAt / CopySubPatch / AdjustSTEPUVDomain / control point ops, my_test_sp_analy CreateSurfOfRevolution, my_create_nurb_surf SetCanonical, the three pt_TestFiles .sms surfaces, revolutions of three .smc curves, edge cases + 700 generated), 718 bit-identical; `--perturb` SENSITIVE | 45 (`surface.surface_p1` 20: `Evaluate1stDerivatives` in non-rectangular trimmed planes, `SmSurface::IsDegeneratePoint`; `curve.curve_p5` 11: `IsArc` in spheres from three arcs; `surface.surface_p7` 9: `IsPlanar` in `GetMaxAnalyticDomain`; `surface.plane_p2` 5: `IsNurbSurfacePlane` in `CopyAndAddAnalytics`) |
| `curve.tangent_field` | `curve::tangent_field` | 655 cases, 511 bit-identical | 144 (`surface.surface_p7` 132, `surface.surface_p1` 12; `GetNaturalUVDomain` wired by surface.surface_p2, the runner now builds the shim bilinear surface); `SmCrvOnSurf::ReadFromDB` wired by curve.crv_on_surf |
| `curve.iso_curve` | `curve::iso_curve` | 2874 cases (874 fixture: iso curves of Surface.sms / offset_nurb.sms / DropCurveSurface.sms, bilinear and NURB-less edge cases, ReadFromDB streams + 2000 generated), 2754 bit-identical; `--perturb` SENSITIVE | 120 (`surface.surface_p6` 120; `SmSurface::ReadFromDB` wired by surface.surface_p2); the `SmBSplineSurface::Evaluate` back-edge was wired in style review batch #13, `SmLine` by curve.line |
| `curve.crv_in_volume` | `curve::crv_in_volume` | 2030 cases, 1822 bit-identical | 208 (`intersect.curve_class_p4` 175: uncached breaks; `surface.volume_p2` 33: `EvaluatePoint`, `ReadFromDB`); the real `Volume` replaced the stand-in (surface.volume_p1) |
| `surface.volume_p1` | `surface::volume::p1` | 2886 cases (886 fixture: plain / transform / oriented / compound chains through every op, TestSmTransform rotations and translations, lifts of every form and order, ConcatEvals, edge cases + 2000 generated), 2847 bit-identical; a 20,886-case run was also clean; `--perturb` SENSITIVE | 39 (`surface.unbend_volume`: its simple maps; twist wired) |
| `surface.twist_volume` | `surface::twist_volume` | 1572 cases (72 fixture: constructor repairs, evaluations n 0..5 with / without a next map, bounding boxes, inverse and solves, predicates, copies, equality, ASCII reads + 1500 generated), 1569 bit-identical; a 20,072-case run was also clean; `--perturb` SENSITIVE | 3 (`surface.volume_p2`: `SmVolume::ReadFromDB`) |
| `curve.curve_p4` | `curve::curve::p4` | 3259 cases (259 fixture: SM_API_test rectangle / polygon edges (also the far 1e-9 / 1.5e-6 ones), my_test_reversed_line_join line pairs, the my_test_drop_point unit circle through every curve property + 3000 generated: chains, closed / degenerate tests, polar boxes, AssertValid, solution filter, projected distance, Newton evaluators, analytic intersections), 3210 bit-identical; a 30,259-case run was also clean; `--perturb` SENSITIVE | 49 (`surface.surface_p8` 49: `SmSurface::GlobalCurveIntersect` in `HasPlaneCrossings`); ellipse intersections wired by curve.ellipse_p1 |
| `curve.curve_p3` | `curve::curve::p3` | 3076 cases (76 fixture: SM_API_test rectangle / hexagon loops (normals, gap fixing, linearity, plane intersections), smCurveTest line at-distance and circle drop-point / extrema solves + 3000 generated), 2334 bit-identical; `--perturb` SENSITIVE | 742 (`curve.curve_p5`: `LocalPropertyAnalysis` 385, `SmCCIntersectENFO::Evaluate` 136, base `SmCurve::Length` 120, `SmFindCCNormalENFO::Evaluate` 101) |
| `curve.curve_p1` | `curve::curve::p1` | 3083 cases (83 fixture: smCurveTest my_test_point_extrema / my_test_drop_point line and circle solves, the circle centre, SM_API_test rectangle / hexagon edges through every op, edge cases + 3000 generated), 2493 bit-identical; a 12,083-case run was also clean; `--perturb` SENSITIVE | 590 (`curve.curve_p5`: `LocalPropertyAnalysis` 310, `SmFindCCNormalENFO::Evaluate` 163, `SmCCIntersectENFO::Evaluate` 73, `IsArc` 44) |
| `curve.crv_on_surf` | `curve::crv_on_surf` | 2094 cases (94 fixture: smTopoTest loop UV-gap edges on the bulge patch, smMergeTest plane boundary via the UV-line constructor, face trim curves on Surface.sms / DropCurveSurface.sms + 2000 generated op programs incl. upstream WriteToDB records), 1598 bit-identical; `--perturb` SENSITIVE; `CurveKind::CrvOnSurf`; `ApproximateCurve` / `DropPoint` / `CalculateBoundingBox` dispatch to the override | 496 (`intersect.curve_class_p4` 294: uncached breaks; `surface.surface_p7` 147: `EvaluateNormal`; `surface.surface_p1` 50: `Evaluate2ndDerivatives`; `curve.curve_p5` 5; `SmSurface::ReadFromDB` wired by surface.surface_p2) |
| `surface.surface_p2` | `surface::surface::p2` | 869 cases (69 fixture: the three pt_TestFiles .sms surfaces through queries / Evaluate / knots / edits / copies / `SmSurfaceEval` / `ReadFromDB`, base `SmSurface()` defaults, TestSmPrincipalDirectionPairing + 800 generated), 756 bit-identical; `--perturb` SENSITIVE; introduces `Surface` / `SurfaceKind` (B-spline only) and `SurfaceEvaluate` | 113 (`surface.surface_p5` 36: extensions; `curve.curve_p5` 33: CC local solves and plane splits; `surface.surface_p7` 18: `GetMaxAnalyticDomain`; `surface.surface_p1` 13: STEP inversion solves; `surface.plane_p2` 11: `CopyAndAddAnalytics`; `intersect.adv_surface_intersector` 1; `topology.trimming_tools` 1) |
| `surface.curve_bounded_surface` | `surface::curve_bounded_surface` | 535 cases (135 fixture: Surface.sms / DropCurveSurface.sms / offset_nurb.sms bases with the pt_TestFiles .smc boundaries through state / queries / Evaluate / SmSurface defaults / operator== / SetContext, WriteToDB-layout ReadFromDB records incl. the dropped implicit flag and the UV-read-with-3D-type bug + 400 generated), 359 bit-identical; `--perturb` SENSITIVE; `SurfaceKind::CurveBounded` | 176 (`surface.surface_p1` 85: `EvaluateSimple` / `EvaluateSTEP` / `EvaluateSTEPPoint` / `operator==`; `surface.surface_p3` 56: `EvaluateGeometric` / `EvaluateNormalSection`; `surface.surface_p7` 25: `EvaluateNormal`; `surface.surface_p5` 10: `EvaluateNormalAtSingularity`) |
| `surface.surface_cache_p1` | `surface::surface_cache::p1` | 872 cases (72 fixture: the three pt_TestFiles .sms surfaces through boundary bounding planes, Bezier approximation, size tolerances, corners, iso boundary curves, knot continuity, BuildTreeTop over their knot spans and polar boxes, hand-built decompositions through FindUnmarkedLeafNode / GetConnectedRegion / MarkFaceContainment / GetFirstBezierPatch, alternate cache matching, static helper edge cases + 800 generated), 709 bit-identical; a 20,072-case run was also clean; `--perturb` SENSITIVE; `SurfaceCache` keeps the patch / aux arenas, tree nodes carry the arena slot (`SurfaceAux::slot`) | 163 (`surface.surface_p7` 107: `EvaluateNormal` in every polar box and at collinear boundary samples; `surface.surface_p5` 37: `IsSingularity` in every Bezier approximation; `surface.surface_p1` 19: `GetFace`) |
| `surface.surface_cache_p2` | `surface::surface_cache::p2` | 905 cases (105 fixture: the three pt_TestFiles .sms surfaces (and curve-bounded surfaces over them) through Bezier patch blocks, n-th bounding planes, TestAgainstTolerances on hand-built patch / aux-data / head nodes at the tolerance limits, SubdivideByBlock without surface, SM_API_test rectangle / hexagon planes, alternate cache eviction + 800 generated incl. 1e-4 / 1e4 scaled surfaces), 721 bit-identical; `--perturb` SENSITIVE; wired the `SmPlane::IsSingularity` override into the cache's Bezier approximation | 184 (`surface.plane_p1` 53: plane iso curves; `surface.surface_p1` 47: `GetFace` in the B-spline bounding box; `surface.surface_p5` 43: `IsSingularity`, curve-bounded iso curves; `surface.surface_p7` 28: `EvaluateNormal`; `surface.surface_p9` 13: `FindTessellationSplits` in every `SubdivideByBlock` with a surface) |
| `curve.curve_p5` | `curve::curve::p5` | 3097 cases (97 fixture: SM_API_test rectangle / hexagon edges through clean-up, duplicates, IsLine / IsLinear / IsArc / Length / IsDomainBoundary, my_test_reversed_line_join lines, closest-point inversions, Newton Jacobians, edge cases + 3000 generated over 16 ops), 3067 bit-identical; a 20,097-case run was also clean; `--perturb` SENSITIVE; wiring its 13 stubs promoted 12 units to green | 30 (`mass.integrator` 28: `SmIntegrator::IntegrateIt` in `Length` of curved spans; `curve.curve_p2` 2: `GetNaturalInterval` through a solution handle in `TrimGap`) |

## Ready set

2026-10-13 (after `surface.surface_cache_p1`): `surface.trim_srf_cache`
(`surface.surface_cache_p2` also waits for `curve.curve_p5` and `surface.surface_p9`,
`surface.surface_cache_p3` for both). `curve.curve_p5` has landed (partial), so `surface.surface_cache_p2` and
`surface.surface_cache_p3` now wait only on `surface.surface_p9`. New curve classes become `CurveKind`
variants of `curve::curve::Curve` (see `plan/findings.md`, curve.curve_p2); a merge that
adds a variant must also extend the exhaustive matches in `curve::curve_cache`.

## Recent findings (details in `plan/findings.md`)

- `curve.curve_p5` (`LocalPropertyAnalysis`, `IsArc`, `Length`, the curve/curve Newton
  equations, ...) wired its 13 stubs and promoted 12 units to green. Upstream
  `CrvsCleanup` crashes in `TrimGap` next to a closed curve, so those cases are not
  generated. The wiring also exposed gaps in dependent runners: solution handles
  without the curve's interval, the ellipse conversion's out-parameter, and an
  uninitialised branch slot of `SmGlobalSolver::SolveIt`. Each was fixed, or the case
  was excluded as undefined behaviour.

- No Rust-vs-C++ disagreement so far: every non-deferred case of the 110 ported
  units is bit-identical (`max_rel_err = 0`).
- `curve.curve_p7` ports the curve/curve intersection local solve and
  `GlobalPropertyAnalysis`; `CciGlobalSolver` now carries its curves and kernel
  (`with_curves`), and a pruned curve tree (aux data without full tree) is rejected (its
  leaves are not spans; upstream reads them as spans).
- `curve.curve_p6` bounds `GlobalCoincidenceChecker`'s recursion
  (`MAX_COINCIDENCE_DEPTH`) and retags the ellipse stub to `curve.ellipse_p1`;
  `curve.curve_p1` co-ports `IsLine` dispatch and `CacheMakeOrValidate`'s cache build.
- `curve.iso_curve`: `SmIsoCurve::Evaluate` writes the surface's `[nu + 1][nv + 1]`
  derivative array (one order zero) straight into the curve's array; the port flattens
  `BSplineSurface::evaluate`'s rows. The 355 cases deferred on
  `nurbs.b_spline_surface_n_lib_p2` now pass bit-identical (that unit's porter had found
  no stub because `curve.iso_curve` merged later).
- `curve.curve_cache` co-ported `SmFindCurveDistanceExtremaEFO::Evaluate`
  (`curve.curve_p4`) and `SmCurve::IsLine` (`curve::curve::p5::is_line_base`); porters of
  `curve.curve_p4` / `_p5` must not port them twice. `sm_ComputeConstants` stops at the
  first non-zero polygon angle with angle tolerance `0` (kept).
- `curve.hyperbola`: a NURB-less hyperbola (failed `MakeNurb`) segfaults upstream in
  `GlobalPointSolveSTEP`; `curve.curve_p1`'s porter decides the port (`SM_ERR`).
- Nested polymorphic reads (`SmCurve::ReadFromDB` -> projected/offset/iso curve ->
  `SmCurve::ReadFromDB`) have no depth limit upstream; the port bounds them at
  `MAX_READ_NESTING` = 64.
- `curve.tangent_field`: `SmTangentField` derives from `SmCrvOnSurf`; `SetAll`,
  `GetNaturalInterval`, `IsKindOf`, `operator==` are `CO-PORT(curve.crv_on_surf)`.
- `nlib.srf_adv_p5`: with rails and rational input the derivative curves share the
  derivative surfaces' knot arrays, so in-place knot writes reach `der` (ported with
  `make_compatible_knot_tol_traced`). `nlib.srf_adv_p1`: the periodic wrap of
  `N_SrfEvalPtDerivsUnbounded` can loop forever (port: `NotConverged`);
  `N_CreateSkinSpine` reads `pSpineParams[-1]` for one profile with end derivatives.
- `nurbs.b_spline_curve_p3`: `IsSegmentLine` returns a failed status as a "true"
  `SmBoolean`; `AssertValid` test 4 checks the start pair, and its radius-of-curvature
  tests never fire. `JoinWith` may degree-elevate the other curve in place.
- `nlib.srf_adv_p4`: `N_ApproxCrossBoundaryDerivs` ignores `N_VectorsAngle`'s status
  and can refine with an uninitialised knot; `N_MergeKnotVectors` loops forever on NaN
  knots; the Coons / skin-boundary routines modify their input curves (port returns
  errors for the UB paths, compares curves after the call).
- `nurbs.b_spline_curve_n_lib`: `DampenKnotSpacing` can loop forever (port stops after
  `DAMPEN_SPLIT_LIMIT` splits); `CreateHelixSegment` lets through inputs that crash
  `N_CrvApproxSpiral`. `nurbs.b_spline_surface_p1`: the data constructor drops the v
  knots when `s > r` (kept); file readers bound header-declared sizes.
- `nurbs.b_spline_curve_p1`: `SmBSplineCurve.h` binds `Evaluate`'s default
  `bNonZeroTangents = FALSE` (`SmCurve.h` says `TRUE`), so the zero-tangent fix only
  acts when two or more derivatives are asked; curves built with
  `SmBSplineCurve(lDim)` have a NULL NURB that nearly every query dereferences
  (port: `SM_ERR`).
- `nlib.crv_nurb`: `N_CrvGetMinMaxWeightsAndPts` always returns `pmax = 0`; the two
  `N_CrvReverse` codings (in place / new curve) agree only when `m = n + p + 1` (both
  kept as `reverse` / `reversed`); `N_CrvGetType` reports degree-1 non-line polylines
  as free-form.
- `geom.point_set`: `SetUpTightPseudoBox` discards its plane projections (angles use
  raw points); `SetUpCentroid` drops the last 50 points when the size is a multiple of
  50, so `GetCentroid` depends on call order (both kept).
- `curve.hermite_curve`: `Evaluate` leaves entry `num_derivs` unset for
  `num_derivs >= 4` (port and shim return zero there).
- `nlib.crv_tool_p1`: green since `nlib.crv_nurb` landed
  (`N_CrvGetMinMaxWeightsAndPts` always returns `pmax = 0`); `N_CrvRemoveKnot` can
  finish a removal and still return its error flag; the insert/split/refine routines
  do no input checks (port returns `InvalidInput`). Knot-valued fixture parameters use
  `{"knot": i}` because serde_json can parse decimals 1 ulp off.
- `geom.transform`: `EvaluateBoundingBoxSimple` dereferences a null
  `pOptParamPseudoBox` when an output is requested (port: `InvalidInput`).
- `nlib.basis_adv`: `N_BasisDerivsArray` walks spans forward without a bound (reads past
  `U[m]` for an inner parameter at or past the domain end) and has no zero-denominator
  guard; `N_BasisInsertKnots` reads `X[mx+1]` (port skips the no-op comparison).
- `nlib.bezier`: elevation/product matrices are built on a stack freed before return
  (dangling storage upstream); `NL_PascalTri` is `[18][18]` (degree sums above 17 read
  past it; port returns `InvalidInput`).
- `geom.polar_box`: `Union` loops over an unset count after `ReSet` (UB; port returns
  `InvalidInput`), and `p->Union(t, *p)` drops `p`'s own extent (kept).
- `geom.local_solve1d`: the interval-less constructor makes every Newton step a huge
  step *against* the Newton direction (kept); always pass an interval.
- `geom.hash_table`: `SmObjsInVoxels::IncMaxItemCount` leaves every bucket item pointing
  into the freed point-item array when a model with items grows (port uses indices; shim
  refuses it); `FindCoincidentPairs` only pairs items with the bucket head and overwrites
  stored points through a reference (both kept).
- `geom.attribute`: several upstream undefined-behaviour paths (double delete, NULL
  dereference in `SmMergeRegionAttribute::Merge`, writes past "other" lists) return
  `InvalidInput` in Rust and are skipped on both sides; shim liveness is predicted.
- `geom.polynomial`: `SolveQuarticEqn` reads an uninitialised cubic root when the
  tolerance is above 1 (port uses `0.0`; FFI rejects such inputs).
- `geom.tol`: `SmRelation` returns pointer-hash order upstream; the port sorts, and the
  shim sorts the C++ results before comparing. The small-topology zone tolerance is
  1000x *larger*, not smaller.
- `geom.extent3d`: upstream `ContainsLineSeg3d` ORs its two end-point tests (kept).
- `geom.tree`: a point tree (`SmTree(point, ...)`) divides by zero in
  `AddToSpatialTree` upstream (the ffi wrapper refuses it); `SmObjectList::FindObject`
  returns the last item as "previous" when not found (kept).
- `topology.database_io`: short binary reads are undefined behaviour upstream; the port
  returns `InvalidInput` with the same stream state.
- `nlib.math`: the `N_SrfPowerBasisEvalPt` back-edge into `nlib.srf_basic` was removed;
  the Rust function takes the control net directly.

## Style review batch #1 (2026-10-12)

Reviewed `geom.vector2d`, `geom.extent3d`, `topology.database_io`, `nlib.math`,
`geom.tol` and `geom.tree` against AGENTS.md. Numerics and operation order were not
changed, and every unit re-ran difftest with unchanged reports.

- `geom.tree`: the neighbour searches (`left/right/bot/top_neighbors`, `neighbors`) now
  return `Result<Vec<NodeId>>` instead of filling a caller out-param. The four
  `GetXNeighbors` recursions and the four `GetXSide` bodies are one direction-generic
  loop and one recursion. Cyclic parent/child links now return `InvalidInput` instead
  of looping forever or overflowing the stack. This applies to `find_node`,
  `propagate_to_parents`, `rm_from_spatial_tree`, `first_bezier_patch` and the
  neighbour searches.
- `geom.extent3d`: `approx_axis` names its near-centre tests (same comparisons).
- `geom.tol`: removed a `let _ = small;`.
- `topology.database_io`: error `operation` paths now include `DatabaseReader::`.
- No change was needed in `geom.vector2d` or `nlib.math`. `nlib.math` keeps a justified
  module-level `allow(clippy::if_not_else)` (a pedantic lint, not a denied one).
- Left for `geom.vector3d` (claimed): `sm_sqrt`, `sm_asin`, `sm_acos`,
  `SmVector3d::GetMaxDimension`/`DistanceBetween` and the `smos_Max`/`Min` family
  (`SmMath.h`) are duplicated as private `CO-PORT(geom.vector3d)` helpers in
  `geom/vector2d.rs`, `geom/extent3d.rs`, `geom/tol.rs` and `geom/tree.rs`. When that
  unit lands, replace them with the shared versions. `grep -rn "CO-PORT(geom.vector3d)"`
  finds every site.

## Style review batch #2 (2026-10-12)

Reviewed `geom.periodic_extent1d`, `geom.matrix`, `geom.assert_array`, `nlib.geom`,
`geom.vector3d` and `geom.context` against AGENTS.md. Numerics and operation order were
not changed; every touched unit re-ran difftest with the same counts and
`max_rel_err = 0` (also at `--seeds 4000`), plus fmt, clippy (with and without `cpp`),
tests and the wasm32 check.

- `geom.vector3d`: the last `CO-PORT(geom.vector3d)` copies (`sm_sqrt` in
  `geom/matrix.rs`; `sm_sqrt`, `DistanceBetween`, `GetMaxDimension` in `geom/tree.rs`)
  now call `vector3d::sqrt`, `Vec3::distance` and `Vec3::max_dimension`. `sqrt` lost a
  branch that duplicated its result; `length`/`closer_than` reuse
  `length_squared`/`distance_squared`.
- `geom.matrix`: `is_symmetric` is one short-circuiting iterator chain.
- `geom.periodic_extent1d`: `add_interval`'s integer `case_id` with a dead `_` arm is a
  private `SeamCase` enum; `complement`/`set_min_max` reuse `set_full`; error paths
  name `PeriodicExtent1d::`.
- `geom.context`: `Context::new` builds its fields directly; `MarkLock` errors name the
  failing method; the header lists `MarkLock::context2`.
- `geom.assert_array`: `AssertReport` equality is one `&&` chain.
- `nlib.geom`: **API change** `transform_matrix_from_axes(from: Frame, to: Frame)`
  replaces eight positional `Vec3`s (new `nurbs::geom::Frame`); the clamps in
  `vectors_cos_angle`/`vectors_angle` are single expressions; `ColinearDim::flag` is
  `const`.

## Style review batch #3 (2026-10-12)

Reviewed `geom.extent1d`, `geom.attribute`, `geom.polynomial`, `geom.axis2_placement`,
`nlib.srf_basic` and `geom.extent2d` against AGENTS.md. Numerics and operation order
were not changed. Every touched unit (also `geom.extent3d`, `geom.tol`,
`geom.geom_utility_p2`, `geom.matrix`, `geom.vector2d`, `geom.vector3d`) re-ran difftest
with unchanged reports and `max_rel_err = 0` (also at `--seeds 4000`), plus fmt, clippy
(with and without `cpp`), tests and the wasm32 check.

- **Shared `SmMath.h` helpers.** `math::{max2, min2, max3, min3, max4}` (`smos_Max`
  family) and `geom::tol::{is_zero, is_zero_squared, is_zero_to_tol, are_same,
  are_same_to_tol}` (`SM_IS_ZERO*`, `SM_ARE_SAME*`) replace 18 private copies in
  `extent1d`, `extent2d`, `extent3d`, `tol`, `geom_utility/p2`, `matrix`, `vector2d`,
  `vector3d` and `axis2_placement`. New code should import these instead of adding
  private `smax`/`is_zero` helpers.
- `geom.extent2d`/`geom.extent3d`: the private `lerp_exact`/`classify_coord` copies call
  `Extent1d::evaluate`/`classify_point`; extent2d error paths name `Extent2d::`.
- `geom.axis2_placement`: **API change** `Axis2Placement::from_to(from: &Self, to: &Self)`
  replaces six positional vectors (build each with `Axis2Placement::new`).
- `nlib.srf_basic`: **API change** `Surface::with_indices(net, u, v, idx)`,
  `set_size_indices(idx)` and `size_arrays(slot, idx)` take a new `SurfaceIndices
  { n, m, p, q, r, s }` (getter `Surface::indices`), removing an
  `allow(clippy::too_many_arguments)`; `is_flat_cheap` uses `combine2_pts`/`Vec3::length`
  instead of private `N_VectorCombine`/`N_VectorMagnitude` copies; `Direction::flag` /
  `from_flag` are `const`.
- `geom.attribute`: `region_type::{SOLID, VOID, INFINITE}` replace the 0/1/2 literals;
  the two accumulation loops of `SmMergeRegionAttribute::Merge` are one
  `append_new_regions`; `set_regions` bounds-checks once.
- No change was needed in `geom.polynomial`.

## Style review batch #4 (2026-10-12)

Reviewed `geom.geom_utility_p2`, `nlib.basis_basic`, `geom.hash_table`, `geom.object`,
`geom.local_solve_nd` and `geom.local_solve1d` against AGENTS.md. Numerics and operation
order were not changed; every unit re-ran difftest with unchanged counts and
`max_rel_err = 0` (`geom.local_solve1d` and `geom.geom_utility_p2` also at
`--seeds 4000`, `geom.local_solve1d` at `--seeds 200000`), plus fmt, clippy (with and
without `cpp`), tests and the wasm32 check.

- `geom.geom_utility_p2`: **API change** `segment_segment_perspective_intersect(.., view:
  &PerspectivePlane { point, unit_normal, eye }, tolerance)` and
  `line_coplanar_ellipse_intersect(line_pnt, line_vec, ellipse: &CoplanarEllipse {
  center, x_axis, x_radius, y_axis, y_radius }, tolerance)`. `inverse_4x4` builds each
  cofactor minor in a loop (`minor_3x3`) instead of 16 hand-unrolled nine-argument calls;
  the private `determinant_3x3` stub now takes `&[[f64; 3]; 3]`. No
  `allow(clippy::too_many_arguments)` is left in the file.
- `geom.local_solve1d`: `eval_into` evaluates into an `IterationValue` (no `t`/`f`/`f'`
  out-params); `SolveByBrent` has one bisection fallback instead of three copies; the
  Brent min/max checks of `SolveIt` and the bracket-end choice of `SolveOscillating` are
  one branch each (same comparisons); new `SolverAlgorithm::is_brent`; a discarded
  `contains_value` call was removed; `solve_by_bisection` takes `mut` parameters.
- `nlib.basis_basic`: operation paths inlined (no `OP_*` indirection); `Errors:` prose
  became `# Errors` sections.
- `geom.hash_table`, `geom.local_solve_nd`: `# Errors` sections on every fallible public
  fn. No other change was needed there or in `geom.object`.
- Outside the batch: `nurbs::geom`'s header used a redundant explicit intra-doc link
  that failed `cargo doc` with `-D warnings` (`nlib.geom` difftest re-run, unchanged).

## Style review batch #5 (2026-10-12)

Reviewed `geom.polar_box`, `nlib.bezier`, `nlib.funcs_basic`, `nlib.basis_adv`,
`geom.grid` and `geom.geom_utility_p1` against AGENTS.md. Numerics and operation order
were not changed; every unit re-ran difftest with unchanged counts and
`max_rel_err = 0` (`geom.grid` still 613 pass + 2 deferred), plus fmt, clippy, tests
and the wasm32 check. No public API changed.

- **Shared status codes.** `error::{SM_ERR, SM_ERR_WARNING}` and
  `SolidError::sm_err(operation)` replace the private copies in `polar_box`, `grid` and
  `geom_utility/p1`. Twelve other files still carry `const SM_ERR: i32 = 1001` and/or
  a private `fn sm_err` (`grep -rn "const SM_ERR\|fn sm_err" crates/solidcore/src`);
  new code should import the shared ones, and a later pass can migrate the rest.
- `nlib.bezier`: `N_BezGetDegreeElevationMatrix` and `N_BezFuncMultiplyBezCrvMatrix`
  run identical loops; both call one private `band_matrix_into`. `given_or_local`
  replaces the hand-rolled null-matrix fallbacks; `reduce_degree` shares its left/right
  loops between odd and even degrees.
- `geom.grid`: `Setup3DDDA`'s three per-axis blocks are one private `AxisStep::new`;
  `GetVoxelAddress`'s outside test is one expression.
- `geom.polar_box`: `PolarBoxState::is_unknown` (uninitialised or unbounded) serves
  `Cross`'s early exits and `compute_polar_coord`; redundant resets dropped.
- `geom.geom_utility_p1`: checks that return the same error are one `||` chain;
  `PointSetType::code` reads the discriminant.
- `nlib.funcs_basic`, `nlib.basis_adv`: removed `allow(clippy::too_many_arguments /
  too_many_lines)` (allowed workspace-wide); `CurveFunction::compact` truncates in
  place.

## Style review batch #6 (2026-10-12)

Reviewed `geom.solution_array`, `nlib.crv_basic`, `geom.tools`, `geom.transform`,
`geom.global_solver` and `geom.pseudo_box` against AGENTS.md. Numerics and operation
order were not changed; every unit (and `nlib.crv_tool_p1`, merged meanwhile) re-ran
difftest with unchanged counts and `max_rel_err = 0` (`geom.solution_array`,
`nlib.crv_basic`, `geom.global_solver`, `geom.tools` also at `--seeds 4000`), plus fmt,
clippy (with and without `cpp`), tests, `cargo doc -D warnings` and the wasm32 check.

- `geom.solution_array`: **API change** `Solution::classify_solution_end` returns a new
  `EndClass` enum (`BelowStart`, `AtStart`, `Within`, `AtEnd`, `AboveEnd`,
  `Inconsistent`; `code()` gives upstream's -2..3) instead of an `i64`.
  `# Errors` sections on every fallible public fn; `remove_at` uses `saturating_sub`.
- `geom.global_solver`: **API change** `GlobalSolve::solve_it(&SolveRequest { operation,
  solution_requested, tolerance_3d, best_answer_so_far_sq, opt_vectors }, solutions)`
  replaces five positional arguments (`SolveRequest::default()` is Intersect / All /
  0 / `SM_BIG_DOUBLE` / no vectors). `sm_CombineMinMax` matches on `EndClass`;
  `FindBestSubdivisionNode` returns its swap flag directly; the perspective radius
  loop iterates the four offsets; `# Errors` sections.
- `geom.transform`, `geom.pseudo_box`, `geom.global_solver`: the private `SM_ERR`
  consts / `sm_err` fns are replaced by `error::SM_ERR` / `SolidError::sm_err`. Removed
  `transform`'s unused `VOLUME_TYPE` const and its `let _` placeholders. Twelve files
  still carry a private copy (`grep -rn "const SM_ERR\|fn sm_err" crates/solidcore/src`).
- `geom.tools`: the `CurveMaxDistanceBetween` stub takes the sample count as a
  parameter (no `let _ = COINCIDENCE_SAMPLES`); the edge range is one expression.
- `nlib.crv_basic`: `OP_*` operation paths inlined; "Errors with" prose became
  `# Errors` sections; `equals` is one short-circuiting chain; `is_planar` skips
  non-normalisable corners with `let ... else` (same sums and comparisons).

## Style review batch #7 (2026-10-12)

Reviewed `nlib.iges`, `nlib.srf_tool_p1`, `nlib.crv_tool_p1`, `nlib.crv_fit_p1`,
`curve.hermite_curve` and `geom.a_object` against AGENTS.md. Numerics and operation
order were not changed; every unit re-ran difftest at `--seeds 2000` before and after
merging main with unchanged counts and `max_rel_err = 0` (`nlib.crv_tool_p1` is now
green with 2143 cases since `nlib.crv_nurb` landed), plus fmt, clippy (with `cpp`),
tests and the wasm32 check.

- `nlib.iges`: **API change** `iges_126_curve(&Entity126 { k, m, t, w, pts, v })` and
  `iges_128_surface(&Entity128 { k1, k2, m1, m2, s, t, w, pts, u, v })` replace six /
  ten positional arguments (the IGES entity data as named fields).
- `nlib.srf_tool_p1`: **API change** `extract_patch(sur, rect: Rectangle)` replaces
  four corner floats (the rectangle is also what the result is reparametrised to);
  `OP_*` paths inlined; `KnotVector` imported; `span_ul`/`span_ur`/`span_vl`/`span_vr`
  instead of `sul`/`sur_`/...
- `nlib.crv_tool_p1`, `nlib.crv_fit_p1`: `OP_*` operation paths inlined into each
  routine's local `OP`.
- `curve.hermite_curve`: `mut pseudo_box` parameter instead of a rebinding; direct
  dimension test in `approx_eq`.
- `geom.a_object`: **API change** the `boolean_op` const module became the
  `BooleanOperation` enum (all `SmBooleanOperationType` values, `from_code`); region
  propagation matches on it, the DIFFERENCE/SLICE/... comment now sits on the default
  arm. `SolidError::sm_err` instead of a hand-built `Upstream`; `assert_valid` is one
  expression; `Attribute` imported.
- Not changed (judged faithful structure, not transliteration): the u/v twin loops of
  the surface tools, `notify`'s seven positional arguments (same shape as
  `geom::object::notify`), the rational weight check of `remove_knot`.

## Style review batch #8 (2026-10-12)

Reviewed `geom.point_set`, `nlib.funcs_adv`, `nlib.crv_nurb`, `nlib.srf_fit`,
`nlib.crv_fit_p2` and `topology.topology` against AGENTS.md. Numerics and operation
order were not changed; every touched unit (also `nlib.crv_fit_p1`, and
`nlib.crv_geom` / `nlib.crv_tool_p2` after merging main) re-ran difftest with
unchanged reports, counts and `max_rel_err = 0`, plus fmt, clippy (with and without
`cpp`), tests and the wasm32 check. No public signature changed.

- **Shared NLib error constructor.** `SolidError::nlib(operation, code)` replaces the
  private `fn nlib` in the four NLib units of the batch (and `crv_fit::p1`). Thirteen
  older files still carry their own copy (`grep -rn "^fn nlib(" crates/solidcore/src`);
  new code should call `SolidError::nlib`.
- **Shared signed-index helpers.** Two crate-private modules hold the upstream
  `NL_INDEX` array access the ports keep: `nurbs::index` (`isize`: `get`, `set`,
  `get2`, `set2`, `storage`, `sx`, `ux`, `oob`; used by `funcs_adv`, `crv_nurb`) and
  `nurbs::fit_arrays` (`i64`: `at`, `put`, `at2`, `put2`, `alloc`, `alloc2`, `Block`,
  `mget`/`mset` for `RM[i][j]`, `banded`, `full`, `basis`, `copy_knots`, `curve`, ...;
  used by `crv_fit::p1`, `crv_fit::p2`, `srf_fit`), removing about 450 lines of
  copies. `crv_tool/p1.rs`, `srf_tool/p1.rs` and `iges.rs` still have private
  `isize` copies that can move to `nurbs::index`; new NLib ports should import these
  modules instead of copying helpers.
- `nlib.crv_nurb`: `pascal` returns the triangle (no `&mut` out-param), `get_tri` /
  `set_tri` folded into the generic `get2` / `set2`; `greville_abscissa` reads one
  checked knot window; `OP_*` paths inlined (`eval_tangent` and `reversed` now name
  themselves).
- `nlib.funcs_adv`, `nlib.crv_fit_p2`, `nlib.srf_fit`: `OP_*` paths inlined;
  `crv_fit_p2` dropped two `allow(clippy::too_many_lines)` the workspace grants.
- `nlib.srf_fit`: the `index` / `start` / `end` bookkeeping shared by the u and v
  passes of `lst_sq_approx` is one private `Band` value.
- `geom.point_set`: `unit_ortho` returns the new `[x, y, z]` axes instead of writing
  three `&mut` out-params; the containment count matches on ranges.
- `topology.topology`: `Notify`'s two per-target propagation blocks are one
  `propagate` helper (the copy mark reset still applies only to `pOther1`); the record
  lookups share one error; `stop_tracking` lost a dead store.
- Not changed (judged faithful structure): `topology::notify`'s eight positional
  arguments (same shape as `a_object::notify`), the per-direction loops of
  `crv_fit::p2::approx_lst_sq` (upstream's second copy of the band bookkeeping differs
  from `srf_fit`'s in its zeroing), `point_set`'s eight min/max angle updates.

## Style review batch #9 (2026-10-12)

Reviewed `nurbs.b_spline_curve_p1`, `nlib.srf_nurb`, `nlib.crv_adv_p2`,
`nlib.srf_tool_p2`, `nlib.crv_offset` and `nurbs.nurbs_srf` against AGENTS.md (earlier
in the day `nlib.crv_geom` and `nlib.crv_tool_p2` moved to `nurbs::index` /
`SolidError::nlib`, commits `ad631e4`, `3c02e52`). Numerics and operation order were
not changed; every touched unit (also `nurbs.nurbs_crv`, and `nlib.crv_approx` after
merging main) re-ran difftest with unchanged reports at the default seeds and a
2000-seed run, all `max_rel_err = 0`, plus fmt, clippy, tests and the wasm32 check.

- **Shared `TO_EUCLID`.** `nurbs::nurbs_crv::to_euclid` (crate-private; divides by
  `w`, unlike `CPoint::to_euclid`, which multiplies by the reciprocal as
  `N_CPtToPtEuclid`) replaces the copies in `b_spline_curve/p1.rs`, `nurbs_srf.rs` and
  the inline one in `crv_eval`.
- **Public API change:** `BSplineCurve::edit_parameterization(&Extent1d)` lost its
  unused `notify` flag (notifications are omitted per the module deviations). The
  difftest runner still passes `notify` to the C++ side.
- `SolidError::nlib` / `SolidError::sm_err` replace the private forwarding fns in
  `srf_nurb`, `crv_adv/p2`, `srf_tool/p2` and `b_spline_curve/p1`; `nurbs_srf` uses
  `nurbs::index::oob`. `NL_MAX` is `math::max2` in `srf_tool/p2` and `crv_offset`.
- `crv_adv/p2`: `OP_*` paths inlined; the end-constraint test of
  `remove_knots_deriv_constraints` is one boolean expression. `crv_offset`: one
  sample-count error constructor. `nurbs_srf` dropped an `if_not_else` allow that no
  longer applies.
- Not changed (judged faithful or out of scope): `srf_tool/p2` keeps importing
  `srf_tool/p1`'s private `Vec<Vec<_>>` index helpers (`at2` / `put2` have no
  `nurbs::index` counterpart for ragged nets); `b_spline_curve/p1` keeps its
  module-level `OP_*` constants (several are shared by more than one method);
  `TypeFinder`'s labelled-section state machine in `srf_nurb`.

## Style review batch #10 (2026-10-12)

Reviewed `nlib.crv_approx`, `nlib.srf_tool_p3`, `nlib.srf_adv_p3`, `nlib.srf_tool_p4`,
`nurbs.b_spline_surface_p1` and `nurbs.b_spline_curve_n_lib` against AGENTS.md (merge
`b458c36`). Numerics and operation order were not changed; every touched unit (also
`nurbs.b_spline_curve_p1`, `nurbs.nurbs_crv`, `nlib.crv_tool_p2`, `nlib.crv_nurb` and,
after merging main, `nlib.srf_adv_p2`) re-ran difftest with unchanged reports and
status, plus fmt, clippy, tests and the wasm32 check.

- **`SolidError::nl_ser(op)`** (crate-private): upstream `SER(SM_ERR)` after a failed
  NLib call, letting `Unported` through. Replaces the private `nl_err` copies in
  `b_spline_curve/p1.rs`, `b_spline_surface/p1.rs` and `b_spline_curve_n_lib.rs`; use
  it as `.map_err(SolidError::nl_ser(OP))` in new `Sm*` ports.
- **Shared `NL_BIGD`**: `nurbs::math::BIGD` is `pub(crate)`; the private copies in
  `nurbs_crv`, `crv_tool/p2`, `crv_nurb` and `srf_tool/p3` are gone.
- `b_spline_surface/p1` uses `nurbs_crv::to_euclid` (same branches and divisions) and
  lists `knot_type` / `is_nurb_borrowed` in its header table; it and
  `b_spline_curve_n_lib` call `SolidError::sm_err` directly.
- `b_spline_curve_n_lib`: one `extension_continuity` mapping (`SM_G1`/`G1G2`/
  `CINFINITY` -> `NL_*`) for both `CreateExtendedCurve` overloads.
- `srf_adv/p3`: `SolidError::nlib` instead of a private wrapper; `MAX_EXTEND_DEPTH` in
  the header table. `crv_approx`: `math::max2` for `NL_MAX`, `f64 * Vec3` for
  `N_VectorScale` (same products).
- Not changed (judged faithful or out of scope): `srf_adv/p3`'s local `g2`/`s2`/`net`
  ragged-net helpers (same shape as `srf_tool/p1`'s `at2`/`put2`, which are
  `pub(super)`); `n_lib`'s `knot_at`/`cpt_at`/`vec_at` (different error reasons);
  `BSplineCurve::approximate_arc`'s seven arguments (upstream signature, all required);
  `make_unit_ortho_into`'s out-params (upstream's partial writes on failure).

## Style review batch #11 (2026-10-12)

Reviewed `nlib.srf_adv_p1`, `nlib.srf_adv_p2`, `nlib.srf_adv_p4`, `nlib.srf_adv_p5`,
`nurbs.b_spline_curve_p2` and `nurbs.b_spline_curve_p3` against AGENTS.md (merge
`6472fe3`). Numerics and operation order were not changed; every touched unit (also
`nlib.srf_adv_p3`, `nurbs.b_spline_curve_p1` and, after merging main, `nlib.srf_offset`)
re-ran difftest with unchanged counts and status, plus fmt, clippy, tests and the
wasm32 check.

- **`nurbs::index::{at, put, at2, put2}`**: bounds-checked `usize` access (`a[i]`,
  `a[i][j]` of `Vec` rows) next to the signed `get`/`set`. `srf_adv/p1` and `p4` use
  them and `index::oob` instead of private copies; `p1`, `p2`, `p4` call
  `SolidError::nlib` directly; `p5`'s `oob` delegates to `index::oob`.
- **One `KN_TOL`**: `srf_adv/p2`'s duplicate `KNOT_SNAP_TOL` is gone; it uses
  `srf_adv::p3::KNOT_TOL` (same value, same upstream constant).
- `srf_adv/p5`: the `colinear` alias is gone (calls `cpts_are_colinear`); `srf_adv/p4`
  lists `Mat3` in its header table; `srf_adv/p1` drops a redundant
  `#[allow(clippy::too_many_lines)]` (allowed workspace-wide).
- `b_spline_curve/p2`: errors from `create_analytic_curve`,
  `has_repeated_end_control_points`, `is_line`, `number_of_unique_knots` and
  `match_weights` carry their own operation path instead of the shared `OP_NURB`; the
  private `nurb_p2`/`nurb_p2_mut` copies are replaced by `p1`'s `nurb_ref` /
  `nurb_ref_mut` (`pub(super)`).
- Not changed (judged faithful or out of scope): `p5`'s op-less `at`/`put` wrappers
  (one operation per file); the upstream-named fields of `SkinBoundaryOptions`
  (`der`, `cmp`, `cnt`, ...; FFI/difftest API); the per-file `invalid` helpers of
  `b_spline_curve/p1..p3`; the `AnalyticCurve` empty enum (until `curve.line` /
  `curve.ellipse_p1` land).

## Style review batch #12 (2026-10-12)

Workflow batch #4: reviewed `nlib.srf_offset`, `nlib.srf_adv_p6`, `curve.curve_p2`,
`curve.conic` and `nurbs.b_spline_surface_n_lib_p1` against AGENTS.md (merge
`bd22019`). `curve.parabola` was still claimed (not merged) and is left for a later
batch. Numerics and operation order were not changed; every touched unit (also
`nurbs.b_spline_surface_p1`, `topology.database_io` and, after merging main,
`curve.hyperbola` and `curve.projected_curve`) re-ran difftest with unchanged counts and
status, plus fmt, clippy, tests and the wasm32 check.

- **`error::SM_ERR_NULL_POINTER` + `SolidError::null_pointer(op)`** (upstream `NER` on a
  `NULL` pointer): replaces four private copies of the `1006` constant and the `ner` /
  `no_stream` / inline `Upstream { code: SM_ERR_NULL_POINTER, .. }` builders in
  `b_spline_surface_n_lib/p1`, `b_spline_surface/p1`, `curve/curve/p2` and
  `topology/database_io` (whose `pub const` is gone: import it from `crate::error`).
- `srf_adv/p6`: `nurbs::index::{at, put, oob}` and `SolidError::nlib` instead of private
  `oob` / `nlib` / `at` copies; `*mt(v, i, op)? = x` writes are `put(v, i, x, op)?`, and
  the rescaled derivative curves are rescaled through one slot borrow.
- `srf_offset`: `offset_patch_net` is a `Surface` method (was an associated fn taking
  the patch); `UNIT_SQUARE` listed in the header table.
- `conic`: `Conic::TYPE` is `curve::curve::p2::CONIC_TYPE` (no private
  `CURV_BASE_TYPE`); error operation paths name the methods
  (`curve::conic::Conic::evaluate`, ...).
- `curve/curve/p2`: `stack_look` destructures `last_chunk::<7>()` instead of indexing
  with an `unwrap_or(0.0)` fallback.
- Not changed (judged faithful or out of scope): `b_spline_surface_n_lib/p1`'s identical
  `build_nlib_curve` / `create_nlib_curve` (two upstream statics) and its `invalid`
  helper; `srf_adv/p6`'s `rf` / `mt` reference accessors (no shared equivalent yet);
  `curve/curve/p2`'s `u64 as usize` multiplicities of Hermite/conic knots; private
  `CURV_BASE_TYPE` copies in `hermite_curve`, `hyperbola` and `b_spline_curve/p1`
  (use `curve::curve::p2::*_TYPE` next time those files are touched).

## Style review batch #13 (2026-10-12)

Workflow batch #5: reviewed `curve.tangent_field`, `curve.hyperbola`,
`curve.projected_curve`, `curve.curve_cache`, `curve.iso_curve` and `curve.offset_curve`
against AGENTS.md (merges `16559c3`, `a0b635d`). Numerics and operation order were not
changed. Every touched unit (also `curve.hermite_curve`, `nurbs.b_spline_curve_p1`,
`curve.curve_p2`, `nurbs.b_spline_surface_n_lib_p2`) re-ran difftest with unchanged
counts and status, except `curve.iso_curve` (see below), plus fmt, clippy, tests and
the wasm32 check.

- **Back-edge wired:** `curve.iso_curve`'s `UNPORTED(nurbs.b_spline_surface_n_lib_p2)`
  stub for `SmBSplineSurface::Evaluate` now calls `BSplineSurface::evaluate`
  (`EvaluateFlags`, rows flattened). Deferred dropped from 640 to 285, still partial.
- **Type codes:** `Hyperbola::TYPE`, `HermiteCurve::TYPE` and `BSplineCurve::TYPE` use
  `curve::curve::p2::{HYPERBOLA,HERMITE_CURVE,BSPLINE_CURVE}_TYPE`; the hyperbola's
  owner-surface codes come from `geom::assert_array::labels`. No private
  `CURV_BASE_TYPE` remains outside `curve::curve::p2`.
- **Shared helpers:** `curve::curve::p2::at` (an evaluation entry or `UNDEF`) is
  `pub(crate)` and replaces copies in `curve_cache`, `hyperbola` and `tangent_field`
  (`crv_in_volume` still has one); `curve_cache` uses `SolidError::nl_ser` instead of its
  private `gw_ser`; `offset_curve` builds its null-base error with
  `SolidError::null_pointer`.
- **Enum codes:** `SurfParamType::from_code` and `ProjectionType::from_code` (returning
  `Option`) replace the ad hoc matches in `iso_curve` and `projected_curve`.
- Smaller fixes: `curve_cache` gets a `max_step` helper (was computed twice) and an
  `OP_PARENT` operation constant; redundant `#[allow(too_many_arguments /
  too_many_lines)]` attributes were removed (both lints are allowed workspace-wide);
  fully qualified paths became imports.
- Not changed (judged faithful or out of scope): `offset_curve`'s raw `SmBoolean`
  `i32` flags and no-op `set_context`; `tangent_field`'s generic surface `S`;
  `hyperbola`'s `global_point_solve_step` unused arguments (consumed once
  `curve.curve_p1` lands); the private `ignored` / `ignore_status` copies in
  `curve_cache`, `hyperbola`, `curve::curve::p2`, `b_spline_curve/p3` and
  `geom_utility/p2`. A shared `error` helper would remove them, but that touches
  units outside this batch.

## Style review batch #14 (2026-10-12)

Workflow batch #6: reviewed `nurbs.b_spline_surface_n_lib_p2`, `curve.crv_in_volume`,
`curve.line`, `nurbs.b_spline_surface_p2`, `curve.ellipse_p2` and `surface.volume_p1`
against AGENTS.md (merges `4be3702` and its follow-up). Numerics and operation order were
not changed. Every touched unit re-ran difftest with unchanged counts and status: the six
above plus `geom.solution_array`, `curve.curve_p2`, `curve.hyperbola`,
`curve.curve_cache`, `nurbs.b_spline_curve_p3` (partial) and `geom.geom_utility_p2`,
`geom.local_solve1d` (green). fmt, clippy, tests and the wasm32 check also passed.

- **Shared error helpers** in `crate::error`: `SolidError::invalid_input`,
  `SolidError::unported`, `SolidError::is_unported`, and the crate-level
  `ignored` (`Result<Option<T>>`, failure -> `None`) / `ignore_status` for calls whose
  status upstream ignores (`Unported` still propagates). These replace the private
  `bad` / `invalid` / `unported` / `is_unported` / `keep_unported` / `or_ignored`
  copies in this batch's units and the `ignored` / `ignore_status` / `ignore_failure`
  copies in `curve_cache`, `hyperbola`, `curve::curve::p2`, `b_spline_curve/p3`,
  `geom_utility/p2` and `local_solve1d`. That was the follow-up batch #13 asked for.
- **`nurbs.b_spline_surface_n_lib_p2`:** the private `SM_ERR_NULL_POINTER` / `ner`
  became `SolidError::null_pointer`, and its `get` became `nurbs::index::at`.
- **`curve.crv_in_volume`:** dropped the leftover pass-through wrappers from the stub era
  (`volume_natural_param_domain`, `volume_evaluate`, `volume_map`, `volume_map_point`,
  `volume_transform`, `volume_eq`). It now calls `Volume` directly; only the
  `surface.volume_p2` and `intersect.curve_class_*` stubs remain. It also uses
  `curve::curve::p2::at`, and the stale "unported `SmVolume::operator==`" docs were fixed.
- **Type codes:** `surface::volume::p1`'s `*_VOLUME_TYPE` constants re-export
  `geom::assert_array::labels`. `curve.line` and `curve.ellipse_p2` take
  `SURF_OF_*_TYPE` / `POLAR_CONVERSION_TYPE` from `labels`, and
  `nurbs.b_spline_surface_p2` takes `CRV_ON_SURF_TYPE` from `curve::curve::p2`. No private
  `CURV_BASE_TYPE` remains.
- **`Solution::new` / `zeroed` / `Default`** now need only `O: Copy`, so `curve.line`
  and `curve.ellipse_p2` no longer keep their own `new_solution`.
- Smaller fixes: `Line::join_with`'s `other_is_line` no longer makes a dead `is_line()`
  call, and `Volume::copy_from` uses `ignored` instead of a three-arm match.
- Not changed (judged faithful or out of scope): `Volume::trim_param_bounding_box_simple`
  and `has_discontinuities_simple` keep upstream's in/out argument, because a disjoint
  box leaves the caller's value and `Transform` shares that signature. The per-file
  `OP_*` operation-path constants and the `Err(SolidError::Unported { .. })` struct
  literals in other units still work and were left alone. The many other private
  `fn invalid(op, reason)` copies under `nurbs/` can switch to
  `SolidError::invalid_input` in a later batch.

## Style review batch #15 (2026-10-13)

Workflow batch #7: reviewed `curve.curve_p4`, `surface.twist_volume`,
`surface.b_spline_volume`, `curve.curve_p3`, `curve.curve_p1` and `curve.curve_p6`
against AGENTS.md (commits `90d5180`, `38d50f3`). Numerics and operation order were not
changed. difftest counts and status are unchanged: p1 3083 (590 deferred), p3 3076 (742),
p4 3259 (74), p6 3136 (88), twist_volume 1572 (3), volume_p1 2886 (39) partial;
b_spline_volume 2333 green; all `max_rel_err` 0. fmt, clippy, tests and wasm32 pass.

- **Shared helpers:** `curve::curve::{p1, p6}` dropped their `keep` aliases of
  `error::ignored`; `p1` / `p3` dropped private `invalid` for
  `SolidError::invalid_input`; `p3` / `p4` dropped private `is_unported` for the method;
  `p4`'s `point_or_undef`, `eval_into` and `reverse_ignored` use `ignored` /
  `ignore_status` instead of hand-written three-arm matches.
- **One `eval_n`:** `curve::curve::p3::eval_n` (`pub(super)`) replaces `p1`'s
  `eval_pv` and `p4`'s own `eval_n` (which was `p3`'s with `from_left = true`).
- **Type codes:** `p4` takes `EDGE_TYPE`, `EDGEUSE_TYPE`, `FACE_TYPE`, `SURF_OF_*_TYPE`
  from `geom::assert_array::labels` (its local copies are gone).
- **Lints:** removed `#[allow(clippy::too_many_arguments / too_many_lines)]` in `p4` /
  `p6`; both are allowed workspace-wide. `p4`'s `CurvePropertyEfo::evaluate` takes
  `_signal_errors` instead of `let _ =`.
- **`surface.twist_volume`:** `members_eq` no longer computes the dead period
  comparison (upstream overwrites it; documented); `read_from_db` takes `_version`.
- Judged fine and left alone: `surface.b_spline_volume` (the `USE_VOLUMES` `#else`
  branches are ported as always-`SM_ERR` stubs with documented signatures),
  `twist_volume`'s `Option<&mut _>` box outputs (documented deviation), the per-file
  `OP_*` constants. `curve::curve::p2` still has local `EDGE_TYPE` / `EDGEUSE_TYPE`
  copies (outside this batch), and the private `fn invalid` helpers under `nurbs/` remain.

## Style review batch #16 (2026-10-13)

Workflow batch #8: reviewed `curve.curve_p7`, `curve.crv_on_surf`, `curve.ellipse_p1`,
`curve.curve_p8`, `primitive.api_curves` and `curve.composite_curve_p1` against AGENTS.md
(commits `bff8a69`, `e06dbd7`, merged in `1d80f47`). Numerics and operation order were
not changed. difftest counts and status are unchanged: curve_p7 3027 (1311 deferred),
curve_p8 3058 (1090), ellipse_p1 2028 (132), composite_curve_p1 3093 (312), crv_on_surf
2094 (541) partial; api_curves 1629, parabola 3044, geom.tol 3440, geom_utility_p1 4185,
geom_utility_p2 3270 green; all `max_rel_err` 0. fmt, clippy, tests and wasm32 pass.

- **Shared `SmMath.h` helpers:** `math::rad_to_deg` (`SM_RAD2DEG`) and `math::min4`
  (`smos_4Min`) replace private copies in `curve::curve::{p7, p8}`, `geom::tol` and
  `geom::geom_utility::{p1, p2}`.
- **Shared crate helpers:** `curve::curve::p3::set_param`, `curve::curve::p4::point_or_undef`
  and `geom::vector3d::is_valid_double` are `pub(crate)`; the copies in `p7`, `p8` and
  `primitive::api_curves` are gone. `curve::curve::p2::at` replaces
  `.get(i).copied().unwrap_or(Vec3::UNDEF)` in `p7`, `p8` and `composite_curve::p1`;
  `ellipse::p1` reads its polar tables with `nurbs::index::at` (private `table` gone).
- **Type codes:** `curve::curve::p2::CURV_BASE_TYPE` is `pub(crate)`; `composite_curve::p1`
  and `parabola` dropped their private copies (`Parabola::TYPE` is `PARABOLA_TYPE`).
- **Unported propagation:** `ellipse::p1::read_from_db` and `p7`'s line/line guess point use
  `error::ignored` instead of hand-written three-arm matches.
- **Dead code:** `Ellipse::convert_to_2d` no longer computes the unused `theta_y_rad`
  (upstream computes it and uses ThetaX for both vectors; noted in a comment);
  `join_with` picks the common/end points directly instead of swapping and discarding.
- **Lints:** removed `#[allow(clippy::too_many_arguments / too_many_lines)]` in `p7`, `p8`
  and `composite_curve::p1` (allowed workspace-wide).
- Judged fine and left alone: `crv_on_surf` (back-edge stubs carry `UNPORTED` tags, lift
  formulas keep upstream order), `api_curves`' `#[allow(clippy::neg_cmp_op_on_partial_ord)]`
  (the negated test rejects NaN, as upstream), the remaining three-arm matches whose
  failure arm returns or `continue`s, and `let _ = extent.set_min_max(..)` (never
  `Unported`). `curve::hyperbola` and `ellipse::p1` still each have a private `extract`
  (`is >> double`); a shared `InputStream` method would remove both.

## Resume note (2026-10-13, after style review batch #16)

- `main` is pushed to `origin` (github.com/bherbruck/solidcore). Counts: 71 green,
  35 partial, 3 claimed (`curve.curve_p5`, `curve.composite_curve_p2`,
  `surface.curve_bounded_surface`), 152 todo, 0 blocked. Ready set:
  `surface.surface_cache_p1`.
- Worktrees branched before batch #16: private `rad_to_deg`, `min4`, `set_param`,
  `point_or_undef`, `is_valid_double` and `CURV_BASE_TYPE` copies are gone; use
  `crate::math::{rad_to_deg, min4}`, `curve::curve::p3::set_param`,
  `curve::curve::p4::point_or_undef`, `geom::vector3d::is_valid_double` and
  `curve::curve::p2::CURV_BASE_TYPE`.
- Next style review batch: `surface.surface_p2` and `curve.parabola`; also
  `curve::curve::p2`'s local `EDGE_TYPE` / `EDGEUSE_TYPE`, the `nurbs/` `fn invalid`
  copies and the two private `extract` helpers.

## Known problems

- **Long dependency chain.** There are 65 layers, a 116,850-line critical path, and one
  220-unit cycle broken by 800 dropped back-edges. A unit with back-edges can only be fully
  difftested once their targets are green (`plan/order.md`).
- **Nondeterministic upstream paths.** `srand(time(NULL))` in `SmFace::GetPointsInFace`
  (more than one point) and in `SmShell.cpp`. Pointer-ordered maps at 4 iteration sites.
  Process-wide tolerances, and statics set by the first call. See `plan/closure.md` and
  `plan/findings.md`.
- **Large debug library.** `libSMLib.a` is about 296 MB because it is built with `-g`. Use
  `build.sh --no-tests`, or drop `-g`, if disk gets tight.
- **prog_test granularity.** prog_test reports pass/fail per suite only. Its log names and
  timing lines depend on the clock, so strip them before diffing.
- **Approximate call graph.** Override matching (by signature) and template matching (by
  name) are approximate, so unit closures may over- or under-include a few functions.
  `query.poly` and `query.tess` are included only through over-broad virtual-call
  resolution; they are low priority.
- **NaN/inf in difftest inputs.** JSON can't represent NaN or inf, so non-finite inputs are
  covered by Rust unit tests only.
- **Misleading `seeds` field.** The difftest report's `seeds` field shows the CLI value, not
  the number of cases a runner actually generated.
- **Self-tagged iso curve stubs.** `surface/surface_cache/p1.rs` keeps
  `UNPORTED(surface.surface_cache_p1)` stubs for `SmPlane::CreateIsoParametricCurve` and
  `SmSurfOfRevolution::CreateIsoParametricCurve`, although that unit and the units that
  ported both overrides (`surface.plane_p1`, `surface.surf_of_revolution_p1`) are partial.
  The cache stores iso curves as `BSplineCurve`, so the `SmLine` / `SmCircle` results
  cannot be stored. Whoever changes `SurfaceCache` iso curve storage to `Curve` must wire
  them (grep the tag). No difftest case reaches them today.

## Wave 0 verification (independent check, 2026-10-12)

| Check | Result |
|---|---|
| Licence | PASS. LICENSE is byte-identical to upstream; NOTICE and ATTRIBUTION.md are present and correct. No tracked file comes from OCCT, USD, Hydra, SmPy or proposals. The CMake build and the call-graph extractor read only `source/SMLib`, `source/SM_API` and `tests/*`. Fixtures cite only `tests/prog_test`. Crate names carry no NVIDIA or SMLib branding. The README now says "a Rust port of NVIDIA's SMLib". |
| Gate | PASS. A clean `--green` run in a fresh target dir gave `nlib.knots` 2953/2953, max_rel_err 0. The C++ side really is upstream: `N_BasisEval` and the other upstream symbols are linked into the binary. REL_TOL is 1e-9 and there are no abs floors. Seeds are fixed (fnv1a(unit)+i). Two runs produced byte-identical reports, also identical to the committed report. `--perturb` caught 861/861 perturbed cases. |
| Rules | PASS. fmt, `clippy -D warnings` (with and without `cpp`), the wasm32 check and the tests are all clean. The library code has no unsafe (`forbid`), and its only unwraps are in tests. It has no statics, clock or HashMap. `Kernel: Send` is asserted. |
| Plan | PASS. 260 unique ids, all fields present, no dangling or self deps, no cycles. Layers equal the longest path, and every upstream path exists under SMLib/SM_API. The ready set (extent3d, vector2d, nlib.math, database_io) matches the table above. |

## Open issues

- **`geom.vector2d` / `geom.vector3d` vs `math.rs`.** The plan puts these units at
  `geom::vector2d` / `geom::vector3d`, but `crates/solidcore/src/math.rs` already defines
  `Vec2`/`Vec3`, mapped to `SmVector2d`/`SmVector3d`. The agent porting those units should
  extend `math.rs`, or re-export from it, rather than create duplicate types, and update
  `rust_module` to match. *Resolved for `Vec2`:* `geom.vector2d` moved `Vec2`/`Point2` into `geom::vector2d`
  and `math` re-exports them; `geom.vector3d` can do the same for `Vec3`.
- **`SurfOfExtrusion` as a `SurfaceKind` needs an owner.** `Surface::read_from_db` reads
  `SURF_OF_EXTRUSION_TYPE` records but defers them (a `DISPATCH GAP` whose error names the
  partial unit `surface.surf_of_extrusion_p2`), because adding the variant means threading a `Kernel` through the
  `Surface` dispatchers in `surface::surface::p2` and their callers (cross-unit refactor;
  `plan/findings.md`, `surface.surf_of_extrusion_p2`). The coordinator should assign it to
  a unit (or a refactor PR); 2 `topology.brep_data` cases wait on it.

## Surface tessellation and iso boundaries (2026-10-14, PR #6 review)

`surface.surface_p9` is partial: 778 cases (178 fixtures and 600 generated),
727 bit-identical, zero failed and 51 deferred (`intersect.curve_class_p4`: 50;
`surface.surface_p6`: 1), with no absolute floors. All 96 review regressions
and all 16 explicit analytic-plane cases without diagonals pass. The regression
corpus reproduced 32 failures before the degeneracy/max-side fixes.

The solver uses the existing surface_p1 `CsiGlobalSolver` and constructor;
surface/node/point helpers call the ported p1 implementations. The disjoint CSI
fixture now passes; crossing reaches `surface.surface_p6`. End-to-end CSI is
still unverified. `FindTessellationSplits` is wired into block subdivision:
cache p2 compares 808/905 cases (97 deferred), cache p3 450/600 (150 deferred).
The latest-main revolution surface also has 18 direct C++ comparisons (canonical
cylinder, virtual iso curves), all bit-identical. The committed reports give the
current deferral owners; earlier tables above
are historical snapshots.

## Topology traversal and public entrypoints (2026-10-14, PR #60)

`intersect.topology_solver_p2` is partial: 2,429 cases, 2,384 bit-identical,
zero failures and 45 explicit deferrals; no absolute floors. The shared part-1
runner now compares 3,248/3,532 cases, clearing all 87 previous part-2 deferrals.
`geom.tree` remains 611/618 with seven existing cache-data deferrals.

Coverage combines traversal, CurveVertex, real C++ shell-vertex B-rep queries,
empty shapes, face exclusion, operation rejection, all four result requests and
zero/tiny tolerances, plus 1,080 scale/subdivision fixtures with up to 40/30
vertices, coordinates scaled by 1e6 and tolerances 2/50. Forty face cases reach surface/query callees; the other
five deferrals are topology tolerance/edge access. Nonempty Shape construction,
full face solving and the native undefined EdgeVertex cast are not certified.
See the committed reports and findings for the exact owners and scope.

Final local gates pass with current official main's shared solver corrections: formatting, strict plain/C++ Clippy, workspace/native tests, wasm and
license/source checks. Sensitivity detects 2,384/2,384 part-2 cases,
3,248/3,248 part-1 cases 611/611 tree cases and 721/721 tools cases. Full differential validation:
142 units, 89 green, 53 partial, zero failed. The four default reports match
the full-validation reports byte for byte.

## B-rep spatial caches (2026-10-15, PR #115)

`topology.brep_cache` is partial: 663 cases, 662 bit-identical passes, zero
failures and one explicit `query.brep` deferral, with no absolute floors.
Polygon trees and exact vertex/line-edge resizing are compared against C++;
exact tree construction and face resizing await the dependencies documented
in `plan/findings.md`, including native B-rep list traversal. Cache factories
and topology-solver back-edges are wired; face picking uses caller-supplied
kernel, B-rep identity and owner mappings.
The four affected units keep their existing pass/deferred counts; 131
`geom.a_object` deferrals now correctly name `query.brep` instead of the cache.

## Public geometry and topology queries (2026-10-16, PR #125)

`query.api_queries` is partial: 1,203 cases (403 fixed and 800 fixed-seed
generated), 587 compared bit-identical, zero failed and 616 explicit deferrals.
The native oracle calls real `SmApi*` entrypoints on concrete analytic/NURBS
geometry, including a line-generated surface of revolution, a C0 B-spline corner,
vertex loops and holed faces. No absolute floors were added.

Coverage includes left tangents at an internal knot and its adjacent floats,
zero-edge loop counts, invalid inner-loop cleanup, subdomains, scales, poles and
seams. All 21 analytic revolution cases compare bit-identically, including
multiple UV representations. Relationship fixtures cover all five native classes,
edge/corner contact and reversed operands, including the upstream distance probes;
Rust relationship paths still defer to unfinished dependencies. Topology imports
retain exact concrete types, face-use links, trim flags and geometry owners.

The face UV query now calls `query.face_p2`. Face closest-point test adapters
validate the face before building their caches. `solution_point` follows
`SmSolution::GetPoint`'s parameter advances and ignored ordinary evaluation errors;
two core tests cover both behaviors, and the generic solution API points to this
adapter for future geometry wiring.

FFI guards cover the missing-curve solver/picker, face-query and curved tight-box
crashes found during review and isolated probes. After correction, all 396 probes
finish without a crash or timeout (192 guarded, 204 native calls completed).
Five FFI tests include the regressions and all 25 native null contracts; nulls
are excluded from the differential count because typed Rust has no equivalent.
Native defects are recorded in findings and upstream issues (UB-108).

Local checks pass: formatting, plain/C++ Clippy, 1,168 workspace/doc tests,
the five FFI tests, WASM, cargo-deny and locked publish dry-run. The full native
suite passes 173 units
(97 green, 76 partial), with zero failures; the final API rerun and touched-unit
perturbation also pass (6,463/6,463 detected). Committed reports total 435,272 cases:
422,654 compared, 12,618 deferred, zero failed.

## Independent curve-classification part 5

The p5 implementation and oracle remain independent of the open p2/p7 PRs.
Part 6 landed independently through PR #138 and is inherited from main; all four
p6 call sites are wired. Fresh validation on 9e495b12 compares 275/1,081 cases
exactly, with zero failures and 806 explicit deferrals (p2 493, p7 312,
curve.crv_on_surf 1). Perturbation detects all 275 compared cases. All nine affected
native runners, plain/native Clippy, 1,215 kernel tests, WASM and the focused FFI
regression pass. The eight other reports are unchanged.
The edge-target insertion and SSS blocks remain unported; nonempty large-gap
projection and tangent-flag behavior remain uncertified. Shared interfaces are
recorded as CO-PORTED. The proto-topology p3 extraction back-edge is wired and the
lazy-UV issue remains UB-134.

## Curve-classification interval integration (2026-10-16, PR #138)

`intersect.curve_class_p6` is partial: 4438 cases: 3927 bit-identical kernel comparisons,
6 expected FFI rejection checks, zero failures and 505 explicit deferrals, without absolute floors. Insertion,
homogenization and endpoint repair follow the upstream control flow, retaining
explicit dependency boundaries. `MergeInterval` implements its early exits;
its topology mutation remains deferred at the p2 boundary-tolerance lookup.
Nonempty homogenization/repair and 2D classification are not certified.

The oracle uses concrete native geometric types and real box topology, records
point-classification state and created-entity outputs, and compares actual curve
evaluations. Native FFI regressions include 27440 isolated combinations of
operations, geometric kinds, flags and classifications, plus 96 partition/SSS/boundary
probes and 660 mismatched fillet-partner probes. They run locally with the project CI workflow unchanged. The unit remains partial.

## Boolean wire/shell-vertex stage (2026-10-16, PR #161)

`boolean.merge_p5` is partial: 140 cases, 108 bit-identical comparisons, zero
failures, and 32 explicit deferrals on `query.brep::SmBrep::Point3DClassify`.
No absolute floors. The real `SmMerge::ProcessWiresAndShellVertices` oracle checks
native topology order, mate retention, complete arena connectivity, established
marks, geometry and output resetting across all 11 operations. Classification,
deletion and copying retain explicit dependency boundaries; their mutation paths
are not certified. No incoming back-edges or new dependency types were co-ported.

The FFI guard rejects part-container headers before the B-rep reader (UB-1,
reconfirmed with GDB); actual B-rep files supply the solid fixtures. Both shared
mark types and marked non-wire edges without mates remain valid and covered.

PR #161 review fixes: mate validation is limited to the four keep operations;
unmated marked wires pass for all seven other operations. Eighteen added fixtures
cover missing mates and nonidentity mate order. Both runners reject conflicting
`Relate` mappings, generated operands/marks/mates vary independently, and the
module header records the future need for mutable relation state when wiring copy
adapters.

Review verification: the extended 256-seed run has 380 cases, 291 bit-identical
passes, zero failures and 89 query deferrals. Perturbation detects every compared
case (108/108 default, 291/291 extended).
