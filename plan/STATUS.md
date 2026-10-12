# solidcore status

Last updated: end of Wave 0 (2026-10-12). Update a summary line here when your unit
changes it; per-unit state lives in `plan/modules.toml`.

## Progress

| | |
|---|---|
| Units planned | 260 (65 layers), 387,903 upstream function-body lines assigned |
| Green | **1**: `nlib.knots` (768 lines) |
| Claimed / blocked | 0 / 0 |
| Todo | 259 |

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

## Ready set for Wave 1 (todo units whose `depends_on` are all green)

| Layer | Unit | Upstream lines | Rust module |
|---|---|---|---|
| 0 | `nlib.math` | 2971 | `nurbs::math` (NL_Math, NL_SrfPoly, NL_Util) |
| 0 | `geom.extent3d` | 1122 | `geom::extent3d` |
| 0 | `topology.database_io` | 363 | `topology::database_io` |
| 0 | `geom.vector2d` | 339 | `geom::vector2d` |

These four units unlock the next set of layer-1 units: `nlib.geom` (needs `nlib.math`),
`geom.tol` (needs `geom.extent3d` and `geom.vector2d`), `geom.tree` (needs
`geom.extent3d`) and `geom.attribute` (needs `topology.database_io`). `nlib.math` is on
the critical path (`nlib.math` -> `nlib.geom` -> ... -> blend). Start it first.

Note: `geom.extent3d` and `geom.vector2d` sit in the 220-unit dependency cycle. Their
back-edges call into `geom.vector3d`, `geom.tol` and others (listed in each unit's
`notes`). Port the parts that don't need those first, or co-port them.

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

- **`--perturb` covers floats only.** It perturbs only the first nonzero float, so
  2092/2953 `nlib.knots` cases go unperturbed: these return only ints or an error (span,
  mult, valid, error codes). Exact int and error comparison is covered by
  `compare::tests::structure_mismatches`. Still, `--perturb` should also flip one int or
  error string per case, so that sensitivity is proven for integer/topology-only units.
- **`geom.vector2d` / `geom.vector3d` vs `math.rs`.** The plan puts these units at
  `geom::vector2d` / `geom::vector3d`, but `crates/solidcore/src/math.rs` already defines
  `Vec2`/`Vec3`, mapped to `SmVector2d`/`SmVector3d`. The agent porting those units should
  extend `math.rs`, or re-export from it, rather than create duplicate types, and update
  `rust_module` to match.
