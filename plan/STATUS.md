# solidcore status

Last updated: 2026-10-12, Wave 1 in progress (after style review batch #1). Update a
summary line here when your unit changes it; per-unit state lives in `plan/modules.toml`.

## Progress

Counts are from `plan/modules.toml`. Line counts are the upstream function-body lines
(`est_lines`) assigned to each unit.

| | |
|---|---|
| Units planned | 260 (65 layers), 387,903 upstream lines |
| Green | **13** (11,063 lines): `nlib.knots`, `geom.vector2d`, `topology.database_io`, `nlib.math`, `geom.periodic_extent1d`, `geom.matrix`, `geom.assert_array`, `nlib.geom`, `geom.vector3d`, `geom.tol`, `geom.context`, `geom.extent1d`, `geom.polynomial` |
| Partial | **3** (3,301 lines): `geom.extent3d`, `geom.tree`, `geom.attribute` |
| Ported (green + partial) | 16 units, 14,364 upstream lines (3.7%); about 13,400 Rust lines in `geom/`, `nurbs/` and `topology/` |
| Claimed | 5 (4,335 lines): `nlib.basis_basic`, `geom.axis2_placement`, `geom.geom_utility_p2`, `geom.hash_table`, `geom.local_solve_nd` |
| Blocked | 0 |
| Todo | 239 |

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

## Partial units

Every non-deferred difftest case passes; deferred cases wait on the listed unit (never reported as green).

| Unit | Rust module | difftest | Deferred on |
|---|---|---|---|
| `geom.extent3d` | `geom::extent3d` | 3086 cases, 3082 bit-identical | 4 (`geom.pseudo_box`) |
| `geom.tree` | `geom::tree` | 618 cases (18 fixture + 600 generated), 611 bit-identical; a 20,018-case run was also clean | 7 (`nurbs.b_spline_surface_p1` 6, `surface.surface_cache_p1` 1) |
| `geom.attribute` | `geom::attribute` | 2029 cases (29 fixture + 2000 generated), 1991 bit-identical; a 200,029-case run was also clean | 38 (`topology.shell`: merge-region Merge) |

## Ready set

No `todo` unit is ready: every unit whose `depends_on` are all green or partial has been
claimed. These units become ready as soon as the listed claimed units land:

| Layer | Unit | Upstream lines | Waiting on (claimed) |
|---|---|---|---|
| 2 | `nlib.basis_basic` | 786 | `nlib.geom` |
| 2 | `nlib.srf_basic` | 1088 | `nlib.geom` |
| 3 | `geom.axis2_placement` | 479 | `geom.vector3d` |
| 3 | `geom.extent2d` | 766 | ready (deps green/partial) |
| 3 | `geom.geom_utility_p2` | 1617 | `geom.matrix`, `geom.vector3d` |
| 3 | `geom.hash_table` | 593 | `geom.vector3d` |
| 3 | `geom.local_solve1d` | 1294 | ready (deps green) |
| 3 | `geom.local_solve_nd` | 860 | `geom.matrix` |
| 3 | `geom.object` | 673 | `geom.attribute`, `geom.context` |
| 3 | `geom.polynomial` | 386 | `geom.vector3d` |

`nlib.geom` is on the critical path (`nlib.geom` -> `nlib.basis_basic`/`nlib.srf_basic`
-> ... -> blend). `geom.vector3d` unblocks five units.

## Recent findings (details in `plan/findings.md`)

- No Rust-vs-C++ disagreement so far: every non-deferred case of the seven ported
  units is bit-identical (`max_rel_err = 0`).
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

## Resume note (2026-10-12)

- `main` is pushed to `origin` (github.com/bherbruck/solidcore). The eight claimed units
  above have worktrees under `../solidcore-wt/`; check `git worktree list` before you
  claim anything.
- Next: merge the claimed units as they go green, then wire their back-edges
  (`grep -rn "UNPORTED(<unit>)"`). `geom.vector3d` should also fold in the co-port
  duplicates listed above. Re-run difftest for `geom.tol`: its 221 deferred cases wait
  only on `geom.vector3d` and should turn it green. `geom.extent3d` waits on
  `geom.extent1d` and `geom.pseudo_box`, and `geom.tree` on the surface cache /
  B-spline surface units.
- Run style review batch #2 once the current claims merge.

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
