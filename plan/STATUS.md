# solidcore status

Last updated: 2026-10-12, Wave 1 in progress (after style review batch #8). Update a
summary line here when your unit changes it; per-unit state lives in `plan/modules.toml`.

## Progress

Counts are from `plan/modules.toml`. Line counts are the upstream function-body lines
(`est_lines`) assigned to each unit.

| | |
|---|---|
| Units planned | 260 (65 layers), 387,903 upstream lines |
| Green | **53** (59,299 lines): `nlib.knots`, `geom.vector2d`, `topology.database_io`, `nlib.math`, `geom.periodic_extent1d`, `geom.matrix`, `geom.assert_array`, `nlib.geom`, `geom.vector3d`, `geom.tol`, `geom.context`, `geom.extent1d`, `geom.polynomial`, `nlib.srf_basic`, `geom.extent2d`, `nlib.basis_basic`, `geom.hash_table`, `geom.local_solve_nd`, `geom.local_solve1d`, `geom.polar_box`, `nlib.bezier`, `nlib.funcs_basic`, `nlib.basis_adv`, `geom.geom_utility_p1`, `geom.axis2_placement`, `geom.geom_utility_p2`, `geom.extent3d`, `geom.pseudo_box`, `nlib.iges`, `nlib.srf_tool_p1`, `nlib.crv_fit_p1`, `nlib.funcs_adv`, `nlib.crv_tool_p1`, `geom.point_set`, `nlib.srf_fit`, `nlib.crv_fit_p2`, `topology.topology`, `nlib.crv_nurb`, `nlib.crv_geom`, `nlib.crv_tool_p2`, `nlib.crv_basic`, `nurbs.nurbs_crv`, `nlib.crv_fit_p3`, `nlib.crv_common`, `nlib.crv_adv_p1`, `nlib.spiral`, `nlib.srf_common`, `nlib.srf_nurb`, `nlib.crv_adv_p2`, `nlib.srf_tool_p2`, `nlib.crv_offset`, `nurbs.nurbs_srf`, `nlib.crv_approx` |
| Partial | **11** (11,041 lines): `geom.tree`, `geom.attribute`, `geom.object`, `geom.grid`, `geom.solution_array`, `geom.tools`, `geom.transform`, `geom.global_solver`, `curve.hermite_curve`, `geom.a_object`, `nurbs.b_spline_curve_p1` |
| Ported (green + partial) | 64 units, 70,340 upstream lines (18.1%); about 73,500 Rust lines in `geom/`, `nurbs/`, `topology/` and `curve/` |
| Claimed | 4 (7,500 lines): `nlib.srf_adv_p2`, `nlib.srf_adv_p3`, `nlib.srf_tool_p3`, `nurbs.b_spline_surface_p1` |
| Blocked | 0 |
| Todo | 192 (310,063 lines) |

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

## Partial units

Every non-deferred difftest case passes; deferred cases wait on the listed unit (never reported as green).

| Unit | Rust module | difftest | Deferred on |
|---|---|---|---|
| `geom.tree` | `geom::tree` | 618 cases (18 fixture + 600 generated), 611 bit-identical; a 20,018-case run was also clean | 7 (`nurbs.b_spline_surface_p1` 6, `surface.surface_cache_p1` 1) |
| `geom.attribute` | `geom::attribute` | 2029 cases (29 fixture + 2000 generated), 1991 bit-identical; a 200,029-case run was also clean | 38 (`topology.shell`: merge-region Merge) |
| `geom.object` | `geom::object` | 624 cases (24 fixture + 600 generated), 593 bit-identical | 31 (every non-empty `SmShape`: `topology.face_p2` 11, `topology.brep_p1` 10, `topology.vertex` 6, `topology.edge` 4) |
| `geom.grid` | `geom::grid` | 615 cases (15 fixture + 600 generated), 613 bit-identical; a 30,015-case run was also clean | 2 (`surface.surface_cache_p1` 1: `SmSurfaceGridElement::Init`; `surface.surface_p2` 1: `ExpandHierarchy`) |
| `geom.solution_array` | `geom::solution_array` | 810 cases (10 fixture + 800 generated), 754 bit-identical | 56 (object geometry: `curve.curve_p2` 16, `topology.face_p2` 14, `topology.edge` 12, `surface.surface_p2` 9, `topology.vertex` 5) |
| `geom.a_object` | `geom::a_object` | 2040 cases (40 fixture + 2000 generated), 1872 bit-identical; a 50,040-case run was also clean | 168 (`topology.brep_cache` 131: new `SmBrepCache`; `topology.shell` 37: `SmRegion::IsVoid`) |
| `geom.tools` | `geom::tools` | 913 cases, 721 bit-identical | 192 (`curve.curve_p2` 68, `topology.brep_p1` 64, `topology.vertex` 60) |
| `geom.transform` | `geom::transform` | 3042 cases, 2786 bit-identical | 256 (`surface.volume_p1` / `_p2`) |
| `geom.global_solver` | `geom::global_solver` | 620 cases, 611 bit-identical | 9 (`curve.curve_p2`) |
| `curve.hermite_curve` | `curve::hermite_curve` | 2131 cases (131 fixture + 2000 generated), 2038 bit-identical | 93 (`curve.curve_p2`: `SmCurve::IsKindOf`) |
| `nurbs.b_spline_curve_p1` | `nurbs::b_spline_curve::p1` | 2216 cases (216 fixture on the six pt_TestFiles .smc curves, smSweepTest lines/circles, edge cases + 2000 generated), 2156 bit-identical; a 30,216-case run was also clean | 60 (`curve.curve_p2` 59: `EvaluateContinuity` / `IsKindOf` / `ApproximateLength`; `curve.curve_p1` 1: `TessellateByBisection`) |

## Ready set

Ready (todo, every dependency green/partial): none; both former ready units
(`nlib.crv_adv_p1`, `nlib.crv_common`) are claimed. Waiting on a claimed unit:

| Layer | Unit | Upstream lines | Waiting on (claimed) |
|---|---|---|---|
| 8 | `nlib.spiral` | 392 | `nlib.crv_fit_p3` |
| 9 | `nlib.srf_common` | 661 | `nlib.crv_common` |
| 9 | `nurbs.b_spline_curve_p1` | 1894 | `nlib.crv_common`, `nurbs.nurbs_crv` |

## Recent findings (details in `plan/findings.md`)

- No Rust-vs-C++ disagreement so far: every non-deferred case of the 51 ported
  units is bit-identical (`max_rel_err = 0`).
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

## Resume note (2026-10-12, after style review batch #8)

- `main` is pushed to `origin` (github.com/bherbruck/solidcore). Four units are
  claimed (`nlib.crv_fit_p3`, `nurbs.nurbs_crv`, `nlib.crv_adv_p1`,
  `nlib.crv_common`); check `git worktree list` before you claim. Nothing is ready
  until one of them lands (see Ready set).
- For worktrees branched before batch #8: `SolidError::nlib`, `nurbs::index` and
  `nurbs::fit_arrays` exist on `main`; prefer them to new private copies
  (`nlib.crv_fit_p3` should import `nurbs::fit_arrays` rather than copy p2's former
  helpers). No public API changed in batch #8. Earlier API changes still apply:
  `Entity126` / `Entity128`, `extract_patch(.., Rectangle)`, `BooleanOperation`,
  `EndClass`, `SolveRequest`, `Axis2Placement::from_to`, `SurfaceIndices`,
  `nurbs::geom::Frame`, the `geom_utility_p2` options structs.
- Use `SolidError::sm_err` / `error::SM_ERR` for upstream `SM_ERR` and
  `SolidError::nlib` for NLib errors in new code; older files still carry private
  copies (`grep -rn "const SM_ERR\|fn sm_err\|^fn nlib(" crates/solidcore/src`).
- Remaining partial waits: `geom.tree` on `nurbs.b_spline_surface_p1`/
  `surface.surface_cache_p1`, `geom.attribute` on `topology.shell`, `geom.object` on the
  topology units, `geom.grid` on the surface caches, `geom.solution_array` and
  `geom.tools` on curve/surface/edge/face/vertex geometry, `geom.transform` on
  `surface.volume_p1`/`_p2`, `geom.global_solver` and `curve.hermite_curve` on
  `curve.curve_p2`, `geom.a_object` on `topology.brep_cache`/`topology.shell`.
- Style review batch #9: `nlib.crv_geom`, `nlib.crv_tool_p2` and the units that merge
  next (`nlib.crv_fit_p3`, `nurbs.nurbs_crv`, `nlib.crv_adv_p1`, `nlib.crv_common`);
  optionally migrate `crv_tool/p1.rs`, `srf_tool/p1.rs`, `iges.rs` to `nurbs::index`
  and the remaining private `nlib` / `SM_ERR` copies.

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
