# Fuzz sweep over all green/partial units (2026-10-20)

`difftest --unit U --fuzz 2000` over every green/partial unit (233). Before -> after per unit, rerun on the
fixed tree: 148 units re-fuzzed (all 113 FUZZ-FAIL units, the 5 units new on main, the touched ones), FUZZ-FAIL 113 -> 26 (nlib.basis_basic left to #262; the rest under "Open" in each area). Fixes are in
PRs fuzz/geom, fuzz/curve, fuzz/nurbs-surface, fuzz/nlib-curve, fuzz/topology-query; nlib.basis_basic is left to #262.
Upstream defects: UB-221..UB-237, HANG-40..HANG-45 and "Fuzz sweep trigger" lines in plan/upstream-issues.md.

### Geometry

`difftest --unit U --fuzz 2000` on `spike/fuzz-sweep` vs main `bb137d7d`. Case files:
`fuzz-out/n2000/<unit>/`, 10000-case runs in `fuzz-out/geom/<unit>/`. After-fix rerun:
`fuzz-verify.log`, `fuzz-out/verify2000/`. All 20 units FUZZ-PASS after the fixes; `--green` unchanged
except for added fixtures.

**Findings** (unit; case; minimised input; C++; Rust before; fix; status)

- geom.tree; n2000 `fuzz00296`, `fuzz00352`, `fuzz01815` +11; `/nodes element 5 duplicated`, `/nodes/28/c1 = -1`;
  C++ SIGSEGV (11 cases) or stack overflow (3 cases) in the neighbour and side searches; Rust ok. Fix: FFI
  `well_formed` rejects one-child nodes, mismatched parent/child links and cyclic parent chains
  (`solidcore-ffi/src/units/geom_tree.rs:152`). Status: fixed (harness guard). Upstream UB-221.
- geom.global_solver; `fuzz00308` (`/ops/1/nodes/1/c1 = 0`), `fuzz01871` (`/ops/1/nodes element 0 duplicated`) +6;
  the C++ shim already rejects child links that are not greater than the node's own index; Rust
  `SolveBranch` recursed through a child link back to the top until the stack overflowed. Fix: library
  `GlobalSolver::check_solve_depth` in `solve_it` (cycle check plus `MAX_SOLVE_DEPTH`). Status: fixed;
  bound needs decision (below).
- geom.grid; `fuzz00013` (`/grid/size/0 = 2147483648`) +42; FFI `MAX_AXIS` 16 rejects it; Rust aborted
  allocating `nx*ny*nz` voxels (up to 6.6 TB). Fix: `Grid::new` `try_reserve_exact`, failure is `SM_ERR`
  as upstream's failed `smos_Calloc` (`geom/grid.rs:482`). Status: fixed.
- geom.grid; 10000-run `fuzz09254` (`/grid/bbox/3 = 1e+300`), `fuzz08036`; FFI `MAX_COORD` 1e12
  rejects it; Rust `Step3DDDA` looped forever (all next-plane parameters at `SM_BIG_DOUBLE`, the chosen axis has
  step 0). Fix: step bound (below), unit test `stalled_walk_is_an_error`. Status: fixed. Upstream HANG-40.
- geom.matrix; `fuzz00021` (`/j = 9223372036854775807`), `fuzz00008` (`/i = 2147483648`) +191; C++ never
  reached. The FFI wrapper sized its output buffer from `i`/`j` for every op although only `Resize`
  validates them: an overflow panic (83 cases, `geom_matrix.rs:117`/`118`) or an allocation abort of up to 515 GB (108).
  Rust `Matrix::set_size` aborted or hit capacity overflow (9: `fuzz00059`, `fuzz00922`). Fix: FFI sizes
  the output from `i`/`j` only for `Resize` (`solidcore-ffi/src/units/geom_matrix.rs:117`); `Matrix::try_new`, fallible
  `set_size` (`geom/matrix.rs:167`, `:279`). Status: fixed. Upstream UB-222.
- geom.solution_array; `fuzz01906` (`/ops/17/count = 4294967296`), `fuzz00154` (`/ops/8/idx = 4294967296`),
  `fuzz01580`/`fuzz01720` (`/ops/N/n = 2147483648`); FFI `MAX_SIZE`/`MAX_COUNT` reject them; Rust aborted
  (up to 1.5 TB). Fix: `try_set_size`, `try_insert_at`, `try_with_capacity` (`geom/solution_array.rs:1243`,
  `:1359`, `:1187`). Status: fixed. Upstream UB-222.
- geom.local_solve1d; `fuzz00027` (`/size = 2147483648`, `/steps emptied`) +5; FFI rejects it; Rust
  `IterQueue1d::new` aborted (64 GB). Fix: `IterQueue1d::try_new` (`geom/local_solve1d.rs:228`). Status:
  fixed. Upstream UB-222.
- blend.fillet_custom_solver; `fuzz01010` (`/jac/0 = 4294967296`); FFI rejects it; Rust runner
  `Matrix::new` aborted (343 GB). Fix: runner mirrors `ffi::MAX_ARRAY` and uses `Matrix::try_new`
  (`difftest/src/units/blend_fillet_custom_solver.rs:662`). Status: fixed.
- blend.fillet_custom_solver; `fuzz00885` (`/solver/b/net/1/2/1 = -1e+300`), `fuzz01617`, `fuzz01638`;
  C++ `surface2 = 0`/`1`, Rust `-1`. `LoadInitialValues` (`SmFilletCustomSolver.cpp:62`, `:903`) assigns
  `rpSurface2 = GetSurface(1 - rail)` before the evaluation that fails. The Rust API returns it only on
  success. Fix: the runner reports `1 - rail` on error (`:742`); documented in the module header
  (`blend/fillet_custom_solver.rs:55`). Status: fixed.
- geom.a_object; `fuzz00210` (`/ops/5/2 = -9223372036854775808`) +5; C++ shim computes `-ref - 1`
  (signed wrap); Rust runner panicked on negate overflow. Fix: `checked_neg` (`geom_a_object.rs:520`).
  Status: fixed (harness). The shim's signed overflow is UB in the shim itself, not upstream.
- geom.tol; `fuzz00181` (`/n1 = 9223372036854775807`) +3; FFI rejects it; Rust runner aborted (4 GB).
  Fix: runner mirrors the FFI bound `0..=10_000` (`geom_tol.rs:404`). `fuzz00769` (`/small = -1`) +1: the runner
  parsed `small` for every op, while the C++ only uses it in the zone ops. Fix: parsed lazily (`:272`). Status: fixed (harness).
- geom.transform; `fuzz00224` (`/n = 2147483648`) +10; FFI rejects it; Rust runner aborted (4.8 GB). Fix: runner
  bound `0..=4` (`geom_transform.rs:335`). Status: fixed (harness).
- geom.tools; `fuzz00395` (`/n_edges = 4294967296`) +14; Rust runner aborted (6 GB of placeholder edges).
  Fix: `MAX_PLACEHOLDER_EDGES` (`geom_tools.rs:217`). `fuzz01267` (`/vertices/2/3 = -1.0`), `fuzz00731`:
  C++ `found = 1`, Rust `0`. The shim assigns the vertex tolerance to an `SmZoneTol3d` (`fabs`). Fix: runner uses
  `tol3d(t)` (`:195`). Status: fixed (harness).
- topology.brep_data; `fuzz00030` (`/pre = 4294967296`) +80; FFI `pre > 64` rejects it; Rust runner
  built attributes until the allocation aborted (4.8 GB). Fix: runner mirrors `pre > 64` (`topology_brep_data.rs:462`); `Arena::insert`
  and `AttributeStore::insert` reserve fallibly (`topology.rs:194`, `geom/attribute.rs:855`). `fuzz00153`
  (`/dim = 9223372036854775807`) +3: Rust runner refused the manifold flag, while the shim truncates
  `(SmBoolean)dim` to its low 32 bits. Fix: `as i32` (`:427`). Status: fixed.
- geom.axis2_placement / polar_box / pseudo_box / point_set; `axis2_placement fuzz01575`
  (`/a/v/0/1 = 1e+300`), `polar_box fuzz00004` (`/prog/0/v/2 = -1e+300`), `pseudo_box fuzz00509`
  (`/prog/12/proj/vec/2 = 1e+300`); C++ `0`/`-0`, Rust `SM_UNDEF_DOUBLE` or stale axes. When `|v|^2` overflows,
  `MakeUnitOrthoVectors` writes the unit x (zero) and z = x × Z into the out-params before `SER(SM_ERR)`.
  Fix: `Vec3::unit_ortho_vectors_on_failure` (`geom/vector3d.rs:679`), used in `axis2_placement.rs:135`,
  `point_set.rs:163`, `polar_box.rs:208`, `pseudo_box.rs:1058`. Status: fixed; fixtures `fuzz_regressions.json`.
- geom.point_set; `fuzz01283` (`/tol = -1e-300`), `fuzz01385` (`/tol = -1.0`) +6; C++ treats the tolerance as
  `|tol|` (`SmTol3d` member). Rust used it signed and defaulted its axes to zero, not `SM_UNDEF_DOUBLE`. Fix:
  `point_set.rs:188`, `:192`. Status: fixed; fixture.
- geom.attribute (136 differs, e.g. `fuzz00083` `/ops/11/2 = 4294967296`), geom.assert_array (12, `fuzz00025`
  `/ops/8/add/owner_ts = 17`), geom.object (5, `fuzz00913` `/faces emptied`): harness asymmetries (shim
  `SC_ERR_ARG` not treated as a rejection, op kind checked after the object, type-string indices looked up for
  NULL strings, B-rep source with no selection). Fix: runners (`geom_attribute.rs:370`, `:807`,
  `geom_assert_array.rs:300`, `geom_object.rs:183`). Status: fixed (harness).

**Library resource bounds added**

- `MAX_SOLVE_DEPTH = 1000` (`crates/solidcore/src/geom/global_solver.rs:93`, checked at `:1650`): sum of
  the start trees' heights, plus a cycle check, before `SolveBranch`. Why: `SolveBranch` recursion depth is
  input-driven (wasm has no guard page, AGENTS section 1). Needs maintainer decision (documented Deviation).
- `Step3DDDA` step bound `1 + nx + ny + nz` per call (`crates/solidcore/src/geom/grid.rs:872`). Why: an
  advancing walk exits within that many steps; a stalled one never ends (HANG-40).
- Fallible allocation (no numeric bound, `InvalidInput`/`SM_ERR` on failure): `Matrix::try_new` and
  `set_size`, `SolutionArray::try_*`, `IterQueue1d::try_new`, `Grid::new`, `Arena::insert`,
  `AttributeStore::insert`. Why: caller-chosen sizes aborted the process; upstream fails softly or
  writes through NULL (UB-222).

**Open items**

- No unit in this group is still FUZZ-FAIL in `compare.txt`.
- geom.point_set: 2 differs remain (FUZZ-PASS 2/0/0/0): `fuzz-out/verify2000/geom.point_set/fuzz00533.json`,
  `fuzz01968.json` (+12 in `fuzz-out/geom/geom.point_set/`). Coordinates near `1e300` make the best-fit
  scatter matrix NaN. C++ ignores the Jacobi failure and uses uninitialised eigenvectors (UB-21). Rust returns
  `SM_ERR`. Needs decision: the shim could refuse non-finite scatter matrices.
- geom.grid: the stalled-walk inputs (`fuzz09254`, `fuzz08036`) are covered by a unit test only; the FFI
  rejects them, so no fixture compares them.
- geom.global_solver: the depth check walks every start tree on each `solve_it` (O(nodes)). Not measured.

### Curves

`difftest --unit U --fuzz 2000` over the 25 `curve.*` units. Case files under
`fuzz-out/n2000/<unit>/` (before), `fuzz-out/verify2000/<unit>/` (after),
`fuzz-out/verify3/<unit>/` (rerun of curve_p2/p4/p5/p6, crv_on_surf after the sample bounds).
Counts are differs/crashes/panics/timeouts. All timeouts below are Rust-side
(`rust: no result after 20 s`); no C++ hang was seen in this group.

**Findings**

- **Derivative order sizes the output allocation** (conic, crv_in_volume, crv_on_surf,
  curve_p2, hermite_curve, offset_curve, projected_curve). Cases: `curve.conic/fuzz00361`
  (`/n = 9223372036854775807`), `fuzz00567` (`/n = 2147483648`);
  `curve.crv_in_volume/fuzz00294`; `curve.crv_on_surf/fuzz01688` (`/ops/0/n = 2147483648`);
  `curve.curve_p2/fuzz01494`, `fuzz01819`, `fuzz01833`; `curve.hermite_curve/fuzz00395`,
  `fuzz00749`, `fuzz00975`, `fuzz01287`; `curve.offset_curve/fuzz00193`, `fuzz00299`,
  `fuzz00643`, `fuzz00690`, `fuzz00777`, `fuzz00845`, `fuzz00871`, `fuzz01010`;
  `curve.projected_curve/fuzz01303`. C++: fills a caller array (FFI bounds the order).
  Rust before: `capacity overflow` panic or abort allocating 51-103 GB. Fix: `MAX_EVAL_DERIVS`
  (below); `Conic::evaluate` sizes its output `min(n, 2) + 1` (`n > 2` is `SM_ERR` anyway,
  `conic.rs:323`); Hermite and line kinds checked in the `Curve::evaluate` dispatcher
  (`curve/curve/p2.rs:1094`, `:1116`); hermite runner rejects orders above the bound
  (`curve_hermite_curve.rs:230`). Status: fixed (bound needs decision).
- **`Line::tessellate` allocation abort** (curve_p1). `curve.curve_p1/fuzz01447`
  (`/minseg = 9223372036854775807`, `/chord = 1.0`). C++: FFI caps `minseg` at 64 (upstream
  loops: HANG-41). Rust before: abort allocating 4 GB. Fix: `Line::tessellate` is now fallible,
  reserves `min_segments + 1` per requested array with `try_reserve_exact`, `InvalidInput`
  on failure (`line.rs:988`, `:999`); `Curve::tessellate` propagates (`curve/curve/p1.rs`),
  runner reports the status (`curve_line.rs:901`). Status: fixed (documented Deviation).
- **`Line::assert_valid` skipped the base-class checks' NURB creation** (line).
  `curve.line/fuzz00926` (`/ops/1/given = false`), `fuzz00846`, `fuzz01178`, `fuzz01915`.
  C++: `SmBSplineCurve::AssertValid` calls the virtual `MakeNurb` first, then checks with the
  line's virtual `degree`/`IsDegenerate`/... Rust before: base checks on the missing NURB,
  12-15 fewer record values. Fix: make the NURB (status ignored) and call
  `assert_valid_with(.., DerivedOverrides { .. })`; `assert_valid` takes `&mut self`
  (`line.rs:1995`). Status: fixed.
- **Hermite pseudo box with an overflowing chord** (hermite_curve). `curve.hermite_curve/fuzz00381`
  (`/a/v/0/2 = -1e+300`), `fuzz01731` (`/a/v/1/2 = -1e+300`), `fuzz01740`. C++: ignores the
  `MakeUnitOrthoVectors` failure; `SetBasis` then fails and the box keeps its basis.
  Rust before: propagated the failure (`status_1001`) or a different basis. Fix: ignore the
  status as upstream (`hermite_curve.rs:262`). Status: fixed. (The header says upstream passes
  an "uninitialised" second axis; `SmVector3d` members default to `SM_UNDEF_DOUBLE`, so it is
  `SM_UNDEF_DOUBLE`, not uninitialised; wording only.)
- **`Ellipse::convert_to_2d` with one root** (ellipse_p1). `curve.ellipse_p1/fuzz01889`
  (`/build/read_db/record/head/11 = -1e-300`). C++: degenerate radius makes the quadratic
  linear, one root; only the unused `dThetaYRad` reads the uninitialised second root.
  Rust before: `InvalidInput` unless two roots. Fix: require one root
  (`ellipse/p1.rs:1278`). Status: fixed.
- **Ellipse polar table shorter than the angle table** (ellipse_p1).
  `curve.ellipse_p1/fuzz00284` (`/build/read_db/record/exact emptied`). C++: SIGSEGV
  (`SmEllipse.cpp:555`, UB-57). Rust before: `InvalidInput`. Fix: FFI `check_record` rejects
  a usable converter with no angles or fewer than `angles - 1` flags
  (`solidcore-ffi/src/units/curve_ellipse_p1.rs:391`). Status: fixed.
- **Ellipse record with a decreasing knot** (ellipse_p1). `curve.ellipse_p1/fuzz01692`
  (`/build/read_db/record/vals/41 = -1.0`). C++: `GlobalPointSolve` fails (no crash).
  Rust before: 104 vs 99 record values. Fix: FFI rejects non-monotone record knots
  (`curve_ellipse_p1.rs:421`). Status: needs decision (guard is broader than a crash: it hides a
  differs on invalid input rather than UB).
- **Composite approximation join of a vanishing knot span** (composite_curve_p1).
  `curve.composite_curve_p1/fuzz00767` (`/comp/curves/1/bspline/oob = true`,
  `/comp/curves/2/bspline/u/4 = 1e-300`, op `approx`). C++: SIGSEGV (`SmCompositeCurve.cpp:676`,
  UB-59). Rust before: join error. Fix: FFI `check_approx` replays the join's knot shift and
  rejects a span that rounds away (`curve_composite_curve_p1.rs:338`, message at `:393`).
  Status: fixed.
- **Sense/gap arrays not matching the curve count** (composite_curve_p1, composite_curve_p2).
  `curve.composite_curve_p1/fuzz00574` (`/comp/closed = false`,
  `/comp/curves/1/bspline/cpts/0/3 = 0.0`, `/comp/curves truncated to 1`): cpp `1e20`, rust `0`.
  C++ shim reads one entry per curve; Rust read the extra gaps. Fix: runners pass exactly
  `curves.len()` senses/gaps and reject shorter arrays (`curve_composite_curve_p1.rs:323`,
  `curve_composite_curve_p2.rs:379`; upstream UB-224). Status: fixed.
- **Parabola `GlobalPointSolveSTEP` without a NURB** (parabola). `curve.parabola/fuzz01135`
  (`/a/y/2 = 1e+300`, op `gps_step`). C++: SIGSEGV (`SmBSplineCurve.cpp:3300`, UB-46).
  Rust before: `SM_ERR`. Fix: FFI replays `Adjust`/`MakeNurb` and refuses when no NURB can be
  made (`curve_parabola.rs:131`). Status: fixed.
- **Parabola runner flag parsing** (parabola). `curve.parabola/fuzz00671` (`/args/0 = 1e-300`),
  `fuzz01627` (`/args element 5 duplicated`). C++ shim: `(int64_t)x != 0`; Rust runner used
  `x != 0.0`, and read `dim`/`version` without the shim's `(ULONG)(int64_t)` cast. Fix:
  `flag_arg` (`curve_parabola.rs:248`) and matching casts. Status: fixed.
- **Hyperbola shim rejection read as a value** (hyperbola). `curve.hyperbola/fuzz01441`
  (`/this/b = 0.0`): cpp `-1002` (`SC_ERR_ARG`, shim refuses `gps` without a NURB, UB-46),
  rust `1006`. Fix: runner maps a leading `-1002` to a rejection (`curve_hyperbola.rs:748`).
  Status: fixed.
- **Out-of-range enum values in readers** (iso_curve, projected_curve).
  `curve.iso_curve/fuzz00479` (`/db/sp = 2147483648`), `fuzz01108`, `fuzz01374`
  (`/db/form = 9223372036854775807`), `fuzz01475`; `curve.projected_curve/fuzz00412`
  (`/ptype = 2147483648`), `fuzz00488` (`/ptype = 4`), `fuzz00517`, `fuzz01057`, `fuzz01107`,
  `fuzz01228`, `fuzz01349`, `fuzz01403`, `fuzz01515`, `fuzz01640`. C++: stores the raw value
  (UB, UB-223). Rust: `UNKNOWN`. Fix: runners keep `sp`/`form > 15` and `ptype > 3` away from
  the C++ side (`curve_iso_curve.rs:353`, `curve_projected_curve.rs:365`). Status: documented.
- **Runner loops sized by fuzzed counts** (offset_curve, curve_p2/p4/p5/p6, crv_on_surf).
  `curve.offset_curve/fuzz00067` (`/spec/nrev = 2147483648`) and 28 more `nrev` cases;
  `curve.curve_p2/fuzz00911` (`/samples = 9223372036854775807`), `fuzz00010`, `fuzz01668`,
  `fuzz01868`, `fuzz01898`; `curve.curve_p4/fuzz00562` (`/count = 4294967296`) and 11 more;
  `curve.curve_p5/fuzz00892`, `fuzz01537` (`/n = 9223372036854775807`);
  `curve.curve_p6/fuzz00286` (`/nsamp = 9223372036854775807`) and 4 more;
  `curve.crv_on_surf/fuzz00321` (`/ops/1/samples = 4294967296`). C++: FFI rejects the count.
  Rust before: ran the full loop (timeout). Fix: runner bounds mirroring the FFI limits
  (`curve_curve_p2.rs:958` 400, `curve_curve_p4.rs:776` 401, `curve_curve_p5.rs:775`/`:790` 401,
  `curve_curve_p6.rs:739` 64, `curve_crv_on_surf.rs:946` 8, `curve_offset_curve.rs:371`
  `MAX_RUNNER_REVERSALS = 4`). Status: fixed (verify3: curve_p2/p4/p5/p6 FUZZ-PASS 0/0/0/0).

**Library resource bounds added**

- `MAX_EVAL_DERIVS = 64`, `crates/solidcore/src/curve.rs:43`, checked by `check_eval_derivs`
  (`:50`) in `CrvInVolume::evaluate`, `CrvOnSurf::evaluate`, `OffsetCurve::evaluate` and the
  `Curve::evaluate` Hermite/line arms. Why: the port returns a `Vec` of `num_derivs + 1`
  vectors, so a caller order sized an unbounded allocation; upstream fills a caller array and
  callers ask for at most 5 (`NL_MAXDER`). Documented Deviation in each header; needs
  maintainer decision.
- `Line::tessellate`: `try_reserve_exact(min_segments + 1)` per requested array
  (`crates/solidcore/src/curve/line.rs:999`). Why: caller count sized the arrays; fails
  cleanly instead of aborting. Documented Deviation.
- `Conic::evaluate`: output sized `min(num_derivs, 2) + 1` (`conic.rs:323`); no behaviour
  change (higher orders are `SM_ERR`).

**Open items** (FUZZ-FAIL after the fix in `compare.txt`)

- `curve.composite_curve_p2` 4/0/0/9. Timeouts (Rust): `verify2000/curve.composite_curve_p2/`
  `fuzz00041`, `fuzz01111` (offset/B-spline weight `+-1e-300`), `fuzz00198`, `fuzz00430`,
  `fuzz01865` (canonical point `+-1e300`), `fuzz00435`, `fuzz01007`, `fuzz01099`, `fuzz01988`:
  extreme weights/coordinates make Rust approximation or join loops run long; C++ finishes.
  Differs `fuzz00145` (`/version = 0`), `fuzz00405`, `fuzz01109`, `fuzz01386` (`/bytes
  truncated`): C++ reads a result from a truncated/old-version stream, Rust errors.
- `curve.crv_on_surf` 5/0/0/3 (verify3: 5/0/0/2). Differs `fuzz00330`, `fuzz00496`,
  `fuzz01164`, `fuzz01858`, `fuzz01999` (read_db with emptied/truncated bytes): cpp `SM_ERR`
  (1001), rust `SM_ERR_INVALID_INPUT` (1007), status mapping of a short read. Timeouts
  `fuzz00526` (surface weight `-1e-300`, cache break `-1e300`), `fuzz01785`
  (`/ops/1/breaks/1 = 1e+300`).
- `curve.curve_cache` 0/0/0/1: `fuzz01735` (`/opts/ratio = -1e-300`, Hermite `dim = 2^63-1`,
  `p1/2 = -1e300`).
- `curve.curve_p1` 0/1/0/1: `fuzz01255` (`/breaks/3 = 1e+300`, duplicated break) now aborts
  allocating 6 GB (was a timeout): break-driven sample count; `fuzz01951` (`/breaks/1 =
  -1e+300`) timeout.
- `curve.curve_p8` 0/0/0/5: `fuzz00388`, `fuzz00886`, `fuzz01456`, `fuzz01620`, `fuzz01856`
  (B-spline/Hermite coordinates `+-1e300`).
- `curve.curve_p9` 0/0/0/3: `fuzz00144` (weight `0.0`), `fuzz01043` (`d2 = 1e300`),
  `fuzz01222` (weight `0.0`, knot `1e-300`).
- `curve.curve_p2`, `curve_p4`, `curve_p5`, `curve_p6`: FUZZ-FAIL in `compare.txt`
  (verify2000 ran before the sample bounds); verify3 FUZZ-PASS 0/0/0/0.
- Remaining differs in FUZZ-PASS units: `curve.ellipse_p1/fuzz01971`
  (`/build/ellipse/nurb_from/inside_out = true`, 173 vs 115 record values from
  `polar_assert`/`assert_valid`); `curve.parabola/fuzz00925` (read_db `args/2 = -1.0`:
  version `ULONG_MAX`, cpp `4.29674712e8` looks uninitialised); `curve.iso_curve/fuzz00681`,
  `fuzz00793`, `fuzz01691` (malformed nested surface: C++ reads 8 values, Rust rejects);
  `curve.projected_curve/fuzz00292`, `fuzz00975`, `fuzz01301`, `fuzz01429` (malformed
  B-spline: C++ result, Rust none); `curve.offset_curve` 15 differs (read_db `version` 0/3,
  truncated or emptied bytes; status 1001 vs 1007).
- No `fixtures/curve.*/fuzz_regressions.json` were added; the library fixes are covered by
  unit tests only (`huge_derivative_orders_are_rejected`,
  `dispatcher_rejects_huge_derivative_orders`, `tessellate_rejects_unallocatable_segment_counts`,
  `overflowing_chord_keeps_the_pseudo_box_basis`).
- `curve.curve_p7/fuzz01115` (`/vecs/0/0 = 1e+300`, timeout) passes after the branch with no
  curve_p7 change; presumably fixed by the `nurbs.b_spline_curve_*` edits (other group), not
  verified.

### NURBS classes and surfaces

Units: `nurbs.b_spline_curve*`, `nurbs.b_spline_surface*`, `nurbs.nurbs_crv`, `nurbs.nurbs_srf`,
`primitive.api_curves`, `nlib.srf_*`, `surface.{cone_p1, plane_p2, sphere, torus,
surf_of_extrusion_p1/p2, surf_of_revolution_p1/p2/p3, surface_cache_p1, trim_srf_cache,
surface_p5, surface_p6, b_spline_volume_n_lib}`, `sweep.*`. Run: `difftest --unit U --fuzz 2000`.
Case files: `fuzz-out/n2000/<unit>/fuzzNNNNN.json` (before), `fuzz-out/verify2000/<unit>/`
and `fuzz-out/verify3/<unit>/` (after). "Mutations" is `cases[-1].fuzz.mutations`.
Counts are differs/crashes/panics/timeouts, before -> after (`verify-work/compare.txt`).

**C++ defects (upstream entries UB-225..UB-233, HANG-42, HANG-43; triggers added to UB-32, UB-68, UB-112, HANG-5, HANG-6, HANG-23).**

- `nurbs.b_spline_surface_n_lib_p1` (0/23/12/3 -> 0/0/0/0). `fuzz00080`: `/tol = 1e-300` on `curve0.smc`, `non_rat`: C++ segfault; `fuzz00651`: `/curve/cpts/2/2 = 1e+300`: heap corruption (`realloc(): invalid next size`, reported on the Rust side of the shared process); `fuzz00902`: double free 50 cases later. Cause `N_ApproxNurbsWithNonRatCrv` sample count overflow (UB-225). Fix: FFI `NonRat` of a rational curve needs `1e-7 <= tol <= 1` and data within `MAX_COORD = 1e6` (`crates/solidcore-ffi/src/units/nurbs_b_spline_surface_n_lib_p1.rs:381`). `fuzz00530` (`sweep`, `/tol = 1e-300`): C++ timeout (HANG-5); fix: `MIN_SWEEP_TOL_FRACTION` (`:357`). `fuzz01411`, `fuzz01639` (`extend`, net coordinate `1e300`): C++ timeout (HANG-6); fix: `MAX_COORD` on control points (`:240`). Status: fixed (guarded).
- `nurbs.b_spline_surface_n_lib_p2` (0/1/0/0 -> 0). `fuzz01761`: `/curves/3/cpts/1/0 = 1e+300`, `corner`: C++ segfault in `CreateCornerBlend` (UB-226, dereference not located). Fix: `MAX_COORD = 1e6` (`nurbs_b_spline_surface_n_lib_p2.rs:186`). Status: fixed (guarded).
- `nlib.srf_adv_p1` (1/0/10/5 -> 0), `nlib.srf_adv_p2` (4/0/0/27 -> 0). Timeouts: `/dist` `0.0`, `-0.0`, `±1e-300` with `cont` G1/G2 (`p1` `fuzz00349`, `fuzz01120`, `fuzz01972`, `fuzz01998`; `p2` `fuzz00123`, `fuzz00438`, ...), net coordinate `±1e300` / `2^63` (`p1` `fuzz01548`; `p2` `fuzz00006`, `fuzz00184`, ...): C++ hang (HANG-6). Differs `/dist = -1e-300` (`p1` `fuzz01328`, `p2` `fuzz01299`, `fuzz01467`): C++ surface, Rust `NotConverged`. Fix: `g2_extension_ok` (`nlib_srf_adv_p1.rs:256`, `nlib_srf_adv_p2.rs:204`): non-zero `dist` and estimated step `range * dist / iso-polygon bound >= MIN_G2_STEP_EPS (1024) * eps * max|knot|`. Differs `/sur/net/3/7/3 = 0.0` (`p2` `fuzz01654`), `/sur/net/1/0/3 = -0.0` (`p2` `fuzz00394`): zero weight, C++ reads past its reflection scratch rows (UB-32), Rust errors. Fix: weights all `NOW` or all positive (`nlib_srf_adv_p1.rs:166`, `p2.rs:129`, `p3.rs:156`). Status: fixed; **needs decision**: `g2_extension_ok` also rejects small negative distances that upstream may handle.
- `nlib.srf_adv_p4` (4/18/10/0 -> 0). Differs `fuzz00511` (`/cur/cpts/1/3 = 1e-300`), `fuzz00921`, `fuzz01762` (`cpts` `1e300`), `fuzz01441` (`/ds/0 = -1e+300`): C++ curve, Rust angle error; `N_ApproxCrossBoundaryDerivs` reads an uninitialised angle / knot (UB-32). Fix: `approx_angles_ok` replays `N_VectorsAngle` in upstream order (`nlib_srf_adv_p4.rs:224`), curve weights all `NOW` or positive (`:185`). Status: fixed (guarded).
- `nlib.srf_adv_p6` (13/22/14/9 -> 0). Timeouts `/e = 1e-300`, trajectory control point `±1e300` / `-2^63` (`fuzz00381`, `fuzz00486`, `fuzz01019`, `fuzz01105`, `fuzz01236`, `fuzz01549`, `fuzz01582`, `fuzz01615`, `fuzz01772`): C++ hang in `N_CreateSweepScale` -> `N_ApproxCrvWithPolyline` (HANG-5). Fix: `E >= MIN_SWEEP_TOL_FRACTION (1e-6) * trajectory control polygon length` (`nlib_srf_adv_p6.rs:168`). Differs `status cpp=1000 rust=-1` (`fill`, curve coordinate `±1e300`; `fuzz00198`, `fuzz01035`, `fuzz01174`, `fuzz01191`, `fuzz01326`, `fuzz01597`, `fuzz01774`, `fuzz01818`, `fuzz01937`, `fuzz01947`): upstream's bare error flag from `ST_BezCalcArcLength`'s recursion limit vs Rust `NotConverged`. Fix: runner maps `NotConverged` of `nurbs::bezier::calc_arc_length` to `1000` (`crates/difftest/src/units/nlib_srf_adv_p6.rs:341`); fixtures `fixtures/nlib.srf_adv_p6/fuzz_regressions.json` (`fuzz_regress_fill_arclen_limit_00198`, `_01035`, `_01326`). Differs `status cpp=14 rust=17` (`fill`; `fuzz00484` `/der/0/cpts/0/2 = 1e+300`, `fuzz00813`, `fuzz01797` duplicated point): C++ `NL_INP_ERR`, Rust `NL_GEO_ERR`; not reproduced in `verify2000`, but no fill change in this branch explains it. Status: hang and 1000-flag fixed; 14/17 **needs a replay**.
- `nlib.srf_tool_p3` (0/39/21/5 -> 0), `nlib.srf_tool_p4` (0/52/47/5 -> 0). Timeouts: net coordinate `±1e300` (`p3` `fuzz00246`, `fuzz00619`, `fuzz00633`, `fuzz01530`, `fuzz01724`; `p4` `fuzz00396`, `fuzz00456`, `fuzz01087`, `fuzz01339`, `fuzz01923`): C++ hang in `N_SrfRemoveAllKnots` / `N_SrfRemoveKnotsConstraints` (NaN error bound, HANG-42). Fix: library `remove_all_knots` returns `NotConverged` on a step that changes nothing (`crates/solidcore/src/nurbs/srf_tool/p3.rs:407`); FFI removal only for coordinates `<= MAX_REMOVAL_COORD (1e60)` and weights in `[1e-30, 1e30]` (`nlib_srf_tool_p3.rs:105`, `nlib_srf_tool_p4.rs:105`). Status: fixed.
- `nurbs.b_spline_curve_p2` (2/2/0/0 -> 0). Differs `/senses emptied` (`fuzz00360`, `fuzz01002`): C++ joined curve, Rust error; `CreateByJoining` reads `cpOptSenses` past the array (UB-233). Fix: runner keeps fewer senses than segments from C++ (`crates/difftest/src/units/nurbs_b_spline_curve_p2.rs:488`). Status: fixed (guarded).
- `surface.cone_p1` (1/4/0/0 -> 1/0/0/0). `fuzz00168`, `fuzz00494`, `fuzz00916`, `fuzz01891`: `/cone/mode/bot = ±1e+300` on a near-zero semi-angle, unbounded: C++ stack overflow (UB-230). Fix: `apex_overflows` (`surface_cone_p1.rs:398`), exempting `|semi| <= SM_EFF_ZERO` (cylinder; verifier narrowed it). Status: fixed (guarded).
- `surface.plane_p2` (13/4/0/0 -> 12/1/0/0). `fuzz00149`, `fuzz01393` (`intersect`, inverted/empty bounded domain), `fuzz00896`, `fuzz01926` (`transform`, `/a/make/domain/0 = 2147483648`, `/domain/3 = -0.0`): C++ NULL-NURB segfault (UB-68). Fix: `nurb_less` / `bounded_nurb_less` (`surface_plane_p2.rs:342`, `:532`, `:565`). Status: fixed; the remaining crash and 12 differs are open (below).
- `surface.sphere` (1/1/0/0 -> 1/0), `surface.torus` (1/3/0/0 -> 1/0), `surface.surf_of_revolution_p1` (34/5/0/0 -> 34/0), `surface.surf_of_revolution_p2` (2/8/0/0 -> 2/0), `surface.surf_of_extrusion_p1` (1/7/0/2 -> 0/0/0/2), `surface.surf_of_extrusion_p2` (1/17/0/0 -> 1/0). 37 C++ segfaults: generator circle axis component `±1e300` (`sphere` `fuzz00721`, `torus` `fuzz00120`, `rev_p1` `fuzz00820`, `ext_p1` `fuzz01152`, ...), hyperbola/parabola centre or size `1e300` (`ext_p1` `fuzz00827`, `fuzz00834`, `fuzz01228`, `ext_p2` `fuzz00874`): generator gets no NURB and the constructor evaluates it (UB-227). Fix: axes with non-finite length rejected (`surface_sphere.rs:377`, `surface_torus.rs:201`, `surface_surf_of_revolution_p1.rs:264`, `surface_surf_of_extrusion_p1.rs:343`), conic centre/size `<= MAX_CONIC = 1e12` (`surface_surf_of_extrusion_p1.rs:455`). `ext_p2` `fuzz00363`, `fuzz01373` (`/surf/build/vec/* = 1e+300`, plane intersection): NULL projected curve (UB-228); fix `surface_surf_of_extrusion_p2.rs:507`. `rev_p2` `fuzz01805`, `fuzz01926` (axis point `±2^63`, `Offset`): NULL axis segment (UB-229); fix `far_axis`, `MAX_AXIS_POINT = 1e12` (`surface_surf_of_revolution_p2.rs:297`). Status: fixed (guarded); `MAX_CONIC` and `MAX_AXIS_POINT` are heuristics (**follow-up**).
- `nlib.srf_fit` (0/35/12/0 -> 0). Not a C++ defect: `/p` or `/q = 2^31..2^63` overflowed or sized the harness's Rust knot computation (`cpp runner: attempt to add with overflow at crates/solidcore/src/nurbs/crv_fit/p1.rs:264`, allocation aborts of 16-34 GB; e.g. `fuzz00024`, `fuzz00160`). Fixed by the `nlib.crv_fit_p1` hunks (`add` / fallible `alloc` in `crates/solidcore/src/nurbs/crv_fit/p1.rs`, `fit_arrays.rs`; owned by the NLib-fit group).

**Rust-only failures (panics, allocation aborts, Rust timeouts) and their fixes.** Inputs are absurd counts or degrees (`2^31`, `2^32`, `2^63-1`) that upstream either cannot express (`NL_DEGREE` is a `short`) or fails softly on (`N_Malloc` / `smos_Calloc` NULL).

- `nlib.srf_adv_p1` (`fuzz00200` `/sur/q = 2^63-1`: add overflow `srf_nurb.rs:1107`; `fuzz00596`, `fuzz01129` `/subdiv = 2^63-1`: capacity / multiply overflow `srf_adv/p1.rs:548`), `nlib.srf_nurb` (`fuzz01067`). Fix: `succ` (`crates/solidcore/src/nurbs/srf_nurb.rs:1343`), checked `k * (subdiv + 1)` (`srf_adv/p1.rs:552`). Fixed.
- `nlib.srf_adv_p3`/`p4`/`p5`/`p6`, `sweep.api_surfaces` (degree `2^31`/`2^63-1`: `fuzz00112`, `fuzz00356`, `fuzz00067`, `fuzz00189`, `fuzz00415`, `fuzz00976`, `fuzz00016`, `fuzz00101`, `fuzz00394`, `sweep.api_surfaces` `fuzz00011`, `fuzz01176`, `fuzz01392`; aborts up to 206 GB, overflow at `srf_adv/p4.rs:582`, `crv_nurb.rs:884`, `srf_tool/p2.rs:267`, `srf_tool/p3.rs:272`). Fix: `DEGREE_RANGE_MAX = 32767` (`srf_adv.rs:42`, `srf_tool/p1.rs:148`), fallible `filled`/`grid` returning `NL_MEM_ERR`. Fixed.
- `nlib.srf_tool_p1`/`p2`/`p3`/`p4` (`/sur/p|q = 2^32` aborts, `2^63-1` overflow at `srf_tool/p2.rs:495`, `:705`, `srf_tool/p3.rs:272`). Same fix (`srf_tool/p1.rs:152`, `succ`, `twice`). Fixed.
- `nlib.srf_basic` (0/3/2/0 -> 0 in `verify3`). `fuzz00159`, `fuzz00830` (`/idx/0 = 2^32`), `fuzz00933` (`/idx/0 = 2^63-1`): allocation abort / capacity overflow. Fix: `size_arrays` is fallible, `NlibCode::Memory` (`crates/solidcore/src/nurbs/srf_basic.rs:1008`); runner reports the status (`crates/difftest/src/units/nlib_srf_basic.rs:477`). Fixed (verifier commit `7ea7083b`).
- `nlib.srf_offset` (0/10/2 Rust -> 0). `/qq = 2^32`, `/pp = 2^63-1` (`fuzz00419`, `fuzz00954`): fixed by the `crv_fit/p1.rs` hunks; `(NL_INDEX)del` cast made explicit (`srf_offset.rs:69`). Fixed.
- `nurbs.b_spline_curve_p1` (`fuzz00528` `/mults/1 = 2^31`), `nurbs.b_spline_curve_p2` (`fuzz00409`), `primitive.api_curves` (`fuzz00168` `/mults/1 = 2^32`): `CreateCanonical` expanded the knots by caller multiplicities. Fix: implicit `ExpandedKnots` (`crates/solidcore/src/nurbs/b_spline_curve/p1.rs:1734`), shared by `b_spline_surface/p1.rs:1485` (`nurbs.b_spline_surface_p1` `fuzz00115`, `fuzz00156`; `_p2` `fuzz00007`, `fuzz00065`). Fixed.
- `nurbs.b_spline_curve_p2` / `nurbs.b_spline_surface_p2` expert control-point output sized by a caller stride: fallible (`b_spline_curve/p2.rs:1388`, `b_spline_surface/p2.rs:1161`). Fixed.
- `nurbs.b_spline_curve_n_lib` (0/1/3/2 -> 0). `fuzz00889`, `fuzz01344` (`/mult = 2^63-1`): `DampenKnotSpacing` inserted knots one by one (Rust timeout). Fix: fallible splice (`b_spline_curve_n_lib.rs:846`). `fuzz00819` (`/this/crv/p = 2^63-1`, overflow `crv_adv/p1.rs:1351`), `fuzz00880` (`/deg = 2^31`): fixed by the NLib-curve group's `crv_adv` hunks. Fixed.
- `nurbs.b_spline_curve_p3` (0/0/0/2 -> 0/0/0/1). `fuzz00318` (`/ns = 2^63-1`, `IsSegmentLine`): Rust ran for hours. Fix: `MAX_SEGMENT_SAMPLES` (`b_spline_curve/p3.rs:106`). `fuzz00919` open (below).
- `nurbs.nurbs_crv` (0/33/13/0 -> 0; `fuzz00041` `/der = 2^32`, `fuzz00094`), `nurbs.nurbs_srf` (0/3/1/0 -> 0; `fuzz00748`, `fuzz01665` `/dims`): derivative rows and surface storage sized by caller counts. Fix: fallible `filled` (`nurbs_crv.rs:268`), one fallible reservation before any row (`nurbs_srf.rs:713`). Fixed.
- `nurbs.b_spline_surface_n_lib_p1` Rust side (`fuzz00045` `/n = 2^31`, `fuzz00082` `/p = 2^63-1`): `make_knots` `n + p + 2` checked (`b_spline_surface_n_lib/p1.rs:714`), runner bounds its knot buffer (`crates/difftest/src/units/nurbs_b_spline_surface_n_lib_p1.rs:1109`). Fixed.
- `primitive.api_curves` (0/23/0/0 -> 0). `fuzz00060` `/sides = 2^32`: `create_regular_polygon` grew segment by segment until OOM. Fix: fallible reservation up front (`primitive/api_curves.rs:485`). Fixed.
- `surface.surface_cache_p1` (0/2/0/0 -> 0). `fuzz00864` `/tree/4/c1 = 0`: a cyclic child link; `FindUnmarkedLeafNode` stack grew without bound. Fix: pop bound (`surface/surface_cache/p1.rs:2201`) and `FindUVNodes` visit bound (`surface_cache/p3.rs:280`), `InvalidInput`. Upstream builds no cyclic trees; not an upstream entry. Fixed.
- `surface.trim_srf_cache` (0/0/17/0 -> 0). `fuzz00171` `/srf/srf/u emptied`: runner `len() - 1` underflow. Fix: `checked_sub` (`crates/difftest/src/units/surface_trim_srf_cache.rs:275`). Fixed (harness).
- `surface.surface_p5` (0/0/0/1 -> 0 in `verify3`), `surface.surface_p6` (timeout `fuzz01197` `/op/n = 2^32` -> 0): Rust sampled every requested count. Fix: runners mirror the FFI limits (`surface_surface_p5.rs:708`, 32; `surface_surface_p6.rs:749`, `ffi::MAX_SAMPLES = 64`). Fixed (harness).
- `surface.b_spline_volume_n_lib` (0/1/0/0 -> 0; `fuzz00437` `/curve/p = 2^31`), `sweep.trans_sweep_geometry` (0/0/0/1 -> 0; `fuzz01409` weights `1e-150`, `/points/7/2 = 2^63-1`, Rust timeout): fixed by shared NLib degree / allocation bounds; not traced to one hunk.
- `nlib.srf_adv_p2` `fuzz00017` (`/sur/v/8 = 1e+300`, Rust timeout): no longer run: one of the new `nlib.srf_adv_p2` FFI guards rejects it, and fuzz mode does not run Rust on a rejected case. The Rust extension itself was not shown to stop quickly on that input (**follow-up**).

**Library resource bounds added (documented Deviations; maintainer decision pending, see brief).**

| Name | Value | Location | Why |
|---|---|---|---|
| `MAX_SEGMENT_SAMPLES` | `1 << 20` | `crates/solidcore/src/nurbs/b_spline_curve/p3.rs:106` | `IsSegmentLine` evaluates any caller count; `2^63-1` ran for hours. `InvalidInput` above it. |
| `DEGREE_RANGE_MAX` | `32767` | `nurbs/srf_adv.rs:42`, `nurbs/srf_tool/p1.rs:148` | Degrees upstream's `short` `NL_DEGREE` cannot hold; bounds index arithmetic and scratch sizes. A range check, not a tuning bound. |
| `remove_all_knots` stuck-step detection | one unchanged step | `nurbs/srf_tool/p3.rs:407` | HANG-42; `NotConverged` instead of an endless loop. |
| surface cache walk bounds | `(stack + 1) * (nodes + 1)` pops; `2 * nodes + 1` visits | `surface/surface_cache/p1.rs:2201`, `surface_cache/p3.rs:280` | Cyclic child links (input-built trees) loop forever. |
| Fallible allocation (no numeric bound) | `try_reserve` -> `InvalidInput` / `NL_MEM_ERR` | `ExpandedKnots` (`b_spline_curve/p1.rs:1734`), `size_arrays` (`srf_basic.rs:1008`), `nurbs_crv.rs:268`, `nurbs_srf.rs:713`, `api_curves.rs:485`, `b_spline_curve_n_lib.rs:846`, `b_spline_curve/p2.rs:1388`, `b_spline_surface/p2.rs:1161`, `srf_adv.rs`, `srf_tool/p1.rs` | Caller counts must not abort the process (upstream: `N_Malloc` / `smos_Calloc` return NULL). |

FFI bounds added (harness only, upstream-hazard driven): `MAX_COORD = 1e6` (`nurbs_b_spline_surface_n_lib_p1.rs:76`, `_p2.rs:58`), `APPROX_MIN_TOL = 1e-7`, `MIN_SWEEP_TOL_FRACTION = 1e-6` (`nlib_srf_adv_p6.rs:34`, `nurbs_b_spline_surface_n_lib_p1.rs:79`), `MIN_G2_STEP_EPS = 1024` (`nlib_srf_adv_p1.rs:193`, `_p2.rs:141`), `MAX_REMOVAL_COORD = 1e60` / `MAX_REMOVAL_WEIGHT = 1e30` (`nlib_srf_tool_p3.rs:96`, `_p4.rs:96`), `MAX_CONIC = 1e12` (`surface_surf_of_extrusion_p1.rs:448`), `MAX_AXIS_POINT = 1e12` (`surface_surf_of_revolution_p2.rs:290`). `MAX_CONIC` / `MAX_AXIS_POINT` are heuristic (**follow-up**: derive the real thresholds).

**Open items (still FUZZ-FAIL, or PASS with residual differs).**

- `surface.surface_p6` (FUZZ-FAIL 0/2/0/0 in `verify3`): `verify3/surface.surface_p6/fuzz01069.json` (`/op/dom/2 = 1e-300`, `trim` of a plane), `fuzz01289.json` (`/op/dom/0 = -1e-300`, `trim` of `offset_nurb.sms`): C++ segfault in `CreateNaturalUVTrimCurves` (UB-231); no guard yet.
- `sweep.api_brep` (FUZZ-FAIL 0/5/0/2, unchanged): `verify2000/sweep.api_brep/fuzz00538`, `fuzz01054`, `fuzz01493` (line endpoint z `±2^63`, taper/draft), `fuzz01538` (two identical circles, taper): C++ segfault (UB-112 triggers); `fuzz01378` (pipe path point `1e300`): `SIGFPE` (UB-232); `fuzz01218`, `fuzz01458` (`from_faces`, path point `1e300`): C++ timeout (HANG-43). No guards yet; the Rust side is a stub.
- `sweep.api_surfaces`: HANG-5 guard applied in `crates/solidcore-ffi/src/units/sweep_api_surfaces.rs` (path control polygon length bound). Status: guarded.
- `surface.surf_of_extrusion_p1`: HANG-23 guard applied in `surface_surf_of_extrusion_p1.rs` (`surface_op` Adjust u domain). Status: guarded.
- `surface.surf_of_revolution_p3` (FUZZ-FAIL 1/0/0/3): `fuzz00476`, `fuzz01129`, `fuzz01853` (v domain bound `±2^63` / `-1e300` degrees): C++ timeout (HANG-23 / HANG-1); `fuzz01771` (`/surf/build/domain/3 = 2147483648`): build `cpp=0 rust=1001`.
- `surface.plane_p2` (FUZZ-FAIL 12/1/0/0): `verify2000/surface.plane_p2/fuzz00842` (`/header emptied`, C++ signal 11 only after 42 preceding cases: heap state from an earlier case, not minimised); 12 differs `rec.len cpp=N rust=1` (`fuzz00179`, `fuzz00303`, `fuzz00647`, `fuzz00703`, `fuzz00751`, `fuzz00761`, `fuzz01112`, `fuzz01365`, `fuzz01705`, `fuzz01788`, `fuzz01794`, `fuzz01845`): C++ writes/reads a DB record for a malformed surface (emptied/truncated knots, `p = 0`, `p > n`, huge `form`/`kt`) that Rust rejects; pre-existing (13 before).
- `nurbs.b_spline_curve_p3` (FUZZ-FAIL 0/0/0/1): `verify2000/nurbs.b_spline_curve_p3/fuzz00919` (`assert`, `/this/crv/cpts/0/0 = 1e+300`, out-of-bounds enabled): Rust timeout; not traced.
- Residual differs in FUZZ-PASS units (fuzz mode records differs as notes, not failures): `surface.surf_of_revolution_p1` 34 (`build cpp=1007 rust=-1`: SM status vs non-status Rust error for `±1e300` generator data, e.g. `fuzz00191`), `surface.surf_of_revolution_p2` 2 (`fuzz00336`, `fuzz00597`), `surface.cone_p1` `fuzz01279` (point `1e300`: `cpp=0 rust=3` outputs), `surface.sphere` `fuzz01732` / `surface.torus` `fuzz01921` (`/tol = -1e+300`: `cpp=1 rust=356/525` outputs), `surface.surf_of_extrusion_p2` `fuzz01030` (`/rec/orient = 2^63-1`: build `cpp=0 rust=1007`), `nlib.srf_offset` `fuzz01097` (`/sur/net/4/2/3 = 1e-300` weight: `status cpp=1000 rust=-1`, likely the bare-flag mapping of `nlib.srf_adv_p6`).
- Outside this group's file list but `surface.*` and still FUZZ-FAIL (timeouts, no change in this branch): `surface.surface_p8` (2), `surface.surface_point_solve_p1` (2), `_p2` (3), `_p3` (2).

### NLib curves

Units: nlib.bezier, nlib.crv_adv_p1/p2, nlib.crv_approx, nlib.crv_fit_p1..p3, nlib.crv_nurb,
nlib.crv_offset, nlib.crv_tool_p1/p2, nlib.funcs_adv, nlib.iges, nlib.math
(`difftest --unit U --fuzz 2000`). Before -> after counts are differs/crashes/panics/timeouts
(`fuzz-out/verify-work/compare.txt`). Failing cases before the fix: `fuzz-out/n2000/<unit>/`.
After the fix: `fuzz-out/verify2000/<unit>/`. No C++ crash or hang occurred. Every `crash`
verdict was a Rust `memory allocation of N bytes failed` abort, either in the Rust process or
in the runner side of the `cpp` process (input building, not upstream code). No
`fuzz_regressions.json` fixtures were added for these units. The regressions are unit tests
in each file.

**Findings**

1. **Degree beyond upstream's `short` `NL_DEGREE`** (bezier, crv_adv_p1/p2, crv_nurb, crv_offset,
   crv_tool_p1/p2, funcs_adv, iges). Inputs: `/p`, `/crv/p`, `/f/p`, `/deg`, `/deg/0` set to
   `2147483648`, `4294967296` or `9223372036854775807`. Examples: `nlib.bezier` fuzz00029
   (`reduce_coefs`, abort allocating 34 GB) and fuzz00168 (capacity overflow);
   `nlib.crv_tool_p2` fuzz00032 (`remove_knots`, `2*p` overflow `crv_tool/p2.rs:229`);
   `nlib.crv_nurb` fuzz01058 (`linear_segs`, `crv_nurb.rs:884`); `nlib.funcs_adv` fuzz00001
   (`deriv`, `funcs_adv.rs:175`); `nlib.iges` fuzz00006 (`clamp_crv`, 137 GB abort) and
   fuzz00008 (`correct_crv`, `iges.rs:361`). C++: never reached (FFI caps degrees; `short`
   upstream). Rust before: overflow panic or allocation abort. Fix: `check_short_degree`
   (`bezier.rs:125`) returns `InvalidInput` for `p > MAX_SHORT_DEGREE` at the entry of every
   routine that does index arithmetic or allocates from the degree.
   `degree_reduce_coefs` now returns `Result`. `funcs_adv::multiply` uses `checked_add`
   for `p + q` (`NL_DEG_ERR` on overflow, `funcs_adv.rs:340`). `remove_knots_deriv_constraints`
   saturates `p + der` (`crv_adv/p2.rs:391`). Status: fixed.
2. **Fitting / approximation index arithmetic and allocations on huge counts** (crv_approx,
   crv_fit_p1..p3). Inputs: `nlib.crv_approx` fuzz00475 `/nc = 9223372036854775807`
   (`crv_approx.rs:240` overflow); fuzz00003 `/deg = 2147483648` (111 GB abort);
   `nlib.crv_fit_p1` fuzz00015 `/p = 4294967296` (`end_derivs_knots`, 34 GB abort),
   fuzz00023 `/p = 9223372036854775807` (`crv_fit/p1.rs:264`); `nlib.crv_fit_p2` fuzz00210
   (`crv_fit/p2.rs:723`); `nlib.crv_fit_p3` fuzz00040 (`crv_fit/p3.rs:663`), fuzz00307
   (capacity overflow, `vec![_; k]` of derivatives). C++: not reached (FFI count caps).
   Fix: `fit_arrays::alloc` reserves with `try_reserve_exact` and returns `NlibCode::Memory`
   (`NL_MEM_ERR`, as upstream's failed `N_Malloc`, `fit_arrays.rs:123`). `fit_arrays::add` / `mul`
   (`:131`, `:137`) return `InvalidInput` on overflow. Knot-count comparisons treat an
   overflowing `n + p + 1` as a mismatch (`NL_INP_ERR`, `crv_fit/p1.rs:498`, `crv_fit/p2.rs:448`,
   `:455`). Sums whose degree check comes later are now formed after that check.
   `approx_closed_conditions` allocates `Ds` / `De` through `alloc` (`crv_fit/p3.rs:828`).
   Upstream entry UB-235. Status: fixed.
3. **`NL_G2` extension by a vanishing distance** (crv_adv_p1 fuzz00548, fuzz01037: `/dist = 1e-300`
   on `closedCurve.smc` / `crvTrim1.smc`, relative, start). C++: not reached (FFI
   `MIN_G2_FRACTION`). Rust before: timeout (each step changes the parameter, but about
   `arc length / dist` steps are needed). Fix: `MAX_G2_STEPS` (below). Upstream: add trigger to
   HANG-6. Status: fixed (documented deviation).
4. **`NL_G2` extension of a curve with a zero end weight** (crv_adv_p1 fuzz01836:
   `/crv/cpts/0/3 = 0`, rational quadratic, relative `dist = 3`, end). NaN arc length; the
   search never meets its exit. C++: not reached (FFI rejects non-positive weights). Rust
   before: timeout. Fix: `MAX_G2_STEPS`. Upstream: add trigger to HANG-22. Status: fixed.
5. **Offset by point sampling with a pole** (crv_offset fuzz00366, fuzz01037, fuzz01299:
   one weight set to `-1.0`). `Muu` explodes, giving 727 541 samples and a knot removal
   quadratic in that count. C++: not reached (shim `MAX_SAMPLES` = 4000, FFI positive weights).
   Rust before: timeout. Fix: `MAX_OFFSET_SAMPLES` (below). Also fuzz00468 (`/deg =
   4294967296`, `tol = 1e300`, 34 GB abort), fixed by `check_short_degree(deg)`
   (`crv_offset.rs:126`). fuzz00143 (`crv_nurb.rs:1011` via `get_type`), fixed by finding 1.
   Upstream: UB-234 (unchecked `(NL_INDEX)del`), HANG-44. Status: fixed (documented deviation).
6. **`nlib.math` triangular transpose with a huge logical size** (fuzz00662: `transpose`,
   in place, `upper_right`, `n = 0`, `m = 9223372036854775807`, storage `[[-3.03]]`). Rust before:
   timeout walking `2^63` outer iterations with empty inner loops. Fix: the outer loops run to
   `n.min(m)` / `m.min(n)`, skipping only empty iterations, so the order is unchanged
   (`math.rs:1288`, `:1296`, `:1365`, `:1372`). Status: fixed.
7. **`nlib.math` runner sent ragged arrays to the C++** (fuzz00449, 00887, 01197, 01438, 01497:
   `realloc1d` with `/items/k emptied` or truncated). The C++ got the flattened, shifted
   data and the Rust got the rows (`block[0]: cpp=5.435 rust=0`). Harness bug. Fix: the
   runner rejects items of the wrong width, ragged `realloc2d` rows and ragged matrix storage
   rows (`nlib_math.rs:368`, `:543`, `:566`). Status: fixed.
8. **Runner-only panics and aborts.**
   - `nlib_bezier.rs`: output buffers sized `l + 1` / `max(e, p+q) + 1` from fuzzed indices.
     Now capped by `buf_len` / `MAX_BUF` (`:586`, `:591`), and the library reports the
     out-of-range write.
   - `nlib_crv_fit_p1.rs`:
     - `first_deriv_knots` allocated `n + p + 2` knots for `p = 2^31`; this was fuzz00035,
       the "cpp: killed" verdict. It is now a harness error above `MAX_DERIVED_DEGREE`
       (`:209`, `:250`).
     - Degree-1 interpolation indexed `u[0]` of an empty parameter list (fuzz01196). It now
       uses `first()` / `last()` (`:284`).
   - `nlib_crv_fit_p1.rs:719` / `nlib_crv_fit_p2.rs:714`: divide by zero for a
     zero-column banded matrix (`/matrix/bw = 0`, fuzz01080, fuzz00678).
   - `nlib_crv_fit_p3.rs:306`: an existing curve above `ffi::MAX_COUNT` is rejected before
     sizing a placeholder.
   - `nlib_iges.rs:287`, `:325`: `checked_mul` for the net sizes (fuzz00020 `/size/1 = i64::MAX`,
     fuzz00062).

   Status: fixed.
9. **`nlib.crv_fit_p3` `cubic_spline` with a +-1e300 coordinate among the four end points**
   (fuzz00441 `/pts/0/2 = -1e+300`, `ds = 0`; also fuzz00796, 00906, 00980, 01046, 01314,
   01403, each with a zero end derivative and the huge coordinate in `P[0..=3]` or
   `P[k-3..=k]`). C++ returns control points (`cpts=present`); Rust returns an error
   (`missing`). Cause: the zero derivative is estimated by a chord-length cubic through the
   four end points; the chord length overflows to infinity and the estimate fails.
   Upstream ignores the errors of that fit and `N_CrvDerivs` and keeps going with the
   garbage; the port returns them. This is the deviation already recorded under
   "nlib.crv_fit_p3" above. Still 7 differs after the fix; the fuzz mode counts them as
   notes, so the unit is FUZZ-PASS. Status: documented.

**Library resource bounds added** (all documented in the file headers' Deviations lines)

| Name | Value | Location | Why |
|------|-------|----------|-----|
| `MAX_SHORT_DEGREE` | 32 767 | `crates/solidcore/src/nurbs/bezier.rs:97` (`check_short_degree` `:125`) | Upstream `NL_DEGREE` is a C `short`; larger degrees cannot occur upstream and overflowed index arithmetic / sized huge allocations. |
| `MAX_G2_STEPS` | 100 000 | `crates/solidcore/src/nurbs/crv_adv/p1.rs:92` (checks `:700`, `:758`) | `ST_curxcdr` `NL_G2` search takes about `arc length / extension` steps (HANG-6, HANG-22). Returns `NotConverged`. Needs maintainer decision. |
| `MAX_OFFSET_SAMPLES` | 16 384 | `crates/solidcore/src/nurbs/crv_offset.rs:52` (check `:214`) | Unbounded sample count, quadratic knot removal (HANG-44). FFI shim allows 4000. Needs maintainer decision. |
| `fit_arrays::alloc` fallible | n/a | `crates/solidcore/src/nurbs/fit_arrays.rs:117` | Count-sized work arrays return `NL_MEM_ERR` instead of aborting (wasm and native). |

Runner-only bounds: `MAX_BUF = 1 << 16` (`crates/difftest/src/units/nlib_bezier.rs:586`),
`MAX_DERIVED_DEGREE = 1 << 16` (`crates/difftest/src/units/nlib_crv_fit_p1.rs:250`).

**Open items**

- `nlib.basis_basic`: FUZZ-FAIL 13/0/7/0 before and after. The sweep's edits were dropped
  because open PR #262 covers them; deferred to PR #262.
- `nlib.crv_fit_p3`: 7 differs remain (finding 9, `fuzz-out/verify2000/nlib.crv_fit_p3/`).
  Accepted deviation. Making the ffi reject non-finite chord lengths would turn them into
  rejected cases.
- `MAX_G2_STEPS` and `MAX_OFFSET_SAMPLES` change results on inputs upstream finishes
  (slowly), so a maintainer has to accept them.
- All other units in this group: FUZZ-PASS 0/0/0/0.

### Topology, query, intersect

`difftest --unit U --fuzz 2000`, default seeds. Case files: `fuzz-out/n2000/<unit>/` (sweep), `fuzz-out/verify2000/<unit>/` (after the fixes). Counts are differs/crashes/panics/timeouts, before -> after (`fuzz-out/verify-work/compare.txt`). Mutations are shown as the minimised input. No fixtures were added for this group.

**Findings**

- `topology.brep_p10` (30/4/0/0 -> 0/0/0/0).
  - 30 differs `rust: manifold` (e.g. `fuzz00045.json`, `/patches/0/manifold = -9223372036854775808`; `fuzz00238.json`, `= 2147483648`). The C++ shim converts with `(SmBoolean)rd.i()` (keeps the low 32 bits); the runner's `i32::try_from` failed instead. Harness bug. Fix: `v as i32` (`crates/difftest/src/units/topology_brep_p10.rs:382`). Status: fixed.
  - 4 C++ SIGSEGV (`fuzz00353.json`, `fuzz00790.json`, `fuzz01898.json`, `fuzz01957.json`; `/options/5 = true` on the IGES-patched spine fixtures). UB-237. Fix: runner `heal_guard` returns `Output::rejected` for healing after an IGES patch that leaves a face use outside every shell, and after any `Set` patch (UB-164) (`topology_brep_p10.rs:182`, `:219`). Status: fixed. The `Set` arm is broader than the crash: 17 generated cases moved to rejected (9 had passed with 0 values, 8 were deferred). Needs decision.
- `topology.edgeuse_p2` (1/0/0/11 -> 1/0/0/6).
  - 5 Rust timeouts with `/n = 2^31..2^32` (`fuzz00649.json`, `fuzz01131.json`, `fuzz01389.json`, `fuzz01750.json`, `fuzz01970.json`): the runner sampled `n` times while the FFI bounds `MAX_SAMPLES = 64`. Fix: mirror the bound (`crates/difftest/src/units/topology_edgeuse_p2.rs:406`). Status: fixed.
  - 6 C++ timeouts, surface net coordinate `+-1e300` (`fuzz00450.json`, `fuzz00530.json`, `fuzz00863.json`, `fuzz01071.json`, `fuzz01284.json`, `fuzz01619.json`). HANG-29 regime. FFI `check_block_magnitudes` (`MAX_BLOCK_MAGNITUDE = 1e150`, `crates/solidcore-ffi/src/units/topology_edgeuse_p2.rs:40`, `:164`) was added but has no effect: the runner's `setup` calls the `topology.edgeuse_p1` `Export` first (`topology_edgeuse_p2.rs:124`), and that hangs. Status: open.
  - 1 differ (`fuzz00597.json`, `/frac = 1e-300; /eu = -1; /eu = 9223372036854775807`): `state[2]` C++ `-1.0756e-31`, Rust `0`. Not investigated. Status: open.
- `topology.edgeuse_p3` (0/0/0/8 -> 0/0/0/8). C++ timeouts, surface net `+-1e300` (`fuzz00576.json`, `fuzz00779.json`, `fuzz00845.json`, `fuzz00949.json`, `fuzz01243.json`, `fuzz01435.json`, `fuzz01764.json`, `fuzz01940.json`). The block guard was dropped (no effect: the hang is in the runner's `edgeuse_p1` `Export`, `crates/difftest/src/units/topology_edgeuse_p3.rs:104`). Status: open.
- `topology.face_p2` (0/1/11/3 -> 0/0/0/0).
  - 10 runner panics `attempt to subtract with overflow` (`topology_face_p2.rs:306/307`) on emptied `u`/`v` knots (e.g. `fuzz00431.json`, `/brep/surface/bspline/u emptied`). Fix: `checked_sub` -> harness error (`crates/difftest/src/units/topology_face_p2.rs:306`). Status: fixed.
  - Rust `capacity overflow` / 32 GB abort with `/n = 2^63-1` or `2^32` (`fuzz01481.json`, `fuzz01354.json`). Fix: mirror the FFI `MAX_DYDX = 64` (`topology_face_p2.rs:1936`). Status: fixed.
  - 3 C++ timeouts with a B-rep net or UV curve coordinate of `+-1e300` (`fuzz00474.json`, `fuzz00646.json`, `fuzz00882.json`). HANG-29 regime. Fix: FFI `MAX_COORD` lowered from `1e300` to `1e150` (`crates/solidcore-ffi/src/units/topology_face_p2.rs:66`). Status: fixed.
- `topology.face_p3` (0/0/0/3 -> 0/0/0/1). C++ timeouts, net `+-1e300` (`fuzz01109.json`, `fuzz01124.json`, `fuzz01302.json`). Fix: FFI block guard (`crates/solidcore-ffi/src/units/topology_face_p3.rs:210`); 2 of the 3 are now rejected. `fuzz01302.json` (`net/0/0/1 = -9223372036854775808; net/1/0/2 = -1e+300; /init_gaps = true`) still hangs: the runner's fixture `Export` runs first (`crates/difftest/src/units/topology_face_p3.rs:190`). Status: open.
- `topology.loopuse` (0/0/0/1 -> 0/0/0/1). C++ timeout `fuzz00839.json` (`/brep/surface/bspline/net/1/0/1 = -1e+300`). The guard was dropped (no effect: the runner's `edgeuse_p1` `Export` comes first, `crates/difftest/src/units/topology_loopuse.rs:295`). Status: open.
- `topology.proto_topology_p1` (0/1/0/1 -> 0/0/0/0).
  - C++ SIGSEGV `fuzz00350.json` (`/objects/1/curve/conic/x/1 = -1e+300`): conic axis with an overflowing squared length has no NURB (UB-46). Fix: FFI `axis3` (`crates/solidcore-ffi/src/units/topology_proto_topology_p1.rs:557`). Status: fixed.
  - C++ timeout `fuzz00000.json` (`/objects/17/curve/conic/domain/1 = -1e+300`): HANG-4. Fix: domain ends within `MAX_CONIC_ANGLE = 1e6` (`:539`), under the Rust 2^20-turn bound. Status: fixed.
- `topology.shell` (2/0/0/0 -> 1/0/0/0, FUZZ-PASS).
  - `fuzz01687.json` (`/transfer/split_faceuse = true; /transfer/recipient = 4294967296`): the Rust runner reported a harness failure where the shim returns `SC_ERR_ARG`. Fix: `Output::err("rejected_input")` (`crates/difftest/src/units/topology_shell.rs:620`). Status: fixed.
  - `fuzz00814.json` (`/ray/radius = 1e+300`): the Rust `may_contain_answer` refuses a failed ray/box intersection, which the C++ reads uninitialised (UB-85). Status: documented (open differ).
- `query.api_queries` (48/0/0/1 -> 2/0/0/0, FUZZ-PASS).
  - 47 differs with `/op = 0` (or `op` mutated to `0`): op 0 and 26 are the shim's graph exports, with no Rust counterpart (`geometry.len` C++ 20..2301, Rust 0). Harness. Fix: `compared_op` refuses ops outside `1..=25` on both sides (`crates/difftest/src/units/query_api_queries.rs:77`). Status: fixed.
  - C++ timeout `fuzz00500.json` (base `rational_kind9_1e-05_north`, `/op = 0`): HANG-45. Fix: FFI requires radius `>= 1e-3` for kind-9 B-rep ops (`crates/solidcore-ffi/src/units/query_api_queries.rs:83`). Status: fixed.
  - 2 differs remain (`fuzz00024.json`, `fuzz01274.json`, `/data/16 = 1.0`): Rust "null pointers have no typed Rust equivalent", the C++ answers. Should be classified rejected/deferred by the runner. Status: open.
- `query.loop` (1/4/0/0 -> 1/0/0/0, FUZZ-PASS).
  - 3 C++ SIGSEGV with an edge curve tail knot changed (`fuzz00183.json` `/edges/1/curve/u/2 = 0.0`, `fuzz00293.json`, `fuzz00592.json`): UB-236. Fix: FFI rejects unclamped end knots (`crates/solidcore-ffi/src/units/query_loop.rs:400`). Status: fixed.
  - C++ SIGSEGV `fuzz01585.json` (`/edges/0/curve/y/1 = 1e+300`): circle with no NURB (UB-46). Fix: `circle_axes_ok` (`query_loop.rs:389`). Status: fixed.
  - Differ `fuzz00992.json` (`/edgeuses/4/trim/u/5 = 1e+300`): C++ succeeds, Rust `BSplineCurve::evaluate` returns `upstream status 1001`. Status: open.
- `query.poly` (0/11/3/0 -> 0/0/0/0).
  - 8 Rust aborts allocating 2-4 GB for `/brep/regions/N = 2^31..2^63` (e.g. `fuzz00647.json`); 3 C++ SIGSEGV for `regions/0 = 9223372036854775807` (`fuzz00556.json`, `fuzz01265.json`, `fuzz01442.json`): the shim's `(int64_t)` of `2^63` wraps negative, so no shells are allocated and later shell indices read past the shim's array (a harness defect, not upstream). Fix: FFI and runner `MAX_SHELLS_PER_REGION = 65536` (`crates/solidcore-ffi/src/units/query_poly.rs:227`, `crates/difftest/src/units/query_poly.rs:408`). Status: fixed.
  - 3 runner panics `subtract with overflow` (`query_poly.rs:431`) for an emptied loop (`fuzz01197.json`, `fuzz01866.json`, `fuzz01936.json`). Fix: mirror the FFI loop check (`query_poly.rs:437`). Status: fixed.
- `query.tess` (1/11/11/2 -> 1/0/0/2).
  - 11 runner panics on emptied knots (`query_tess.rs:307`, e.g. `fuzz00034.json` `/setup/srf/srf/v emptied`). Fix: `checked_sub` (`crates/difftest/src/units/query_tess.rs:307`). Status: fixed.
  - 11 Rust aborts allocating 2-4 GB for a tree whose child link points back up (`/setup/tree/N/c1 = 0`, e.g. `fuzz00143.json`, `fuzz00770.json`): `FindUnmarkedLeafNode` / `FindUVNodes` walk the cycle with a growing stack. The FFI already refuses such trees. Fix: library visit bounds (below). Status: fixed.
  - 2 Rust timeouts remain (`fuzz00241.json`: `/setup/tree/5/c2 = 4; /setup/tree/4/c1 = 4; .../pts/2/3 = 1e-300`; `fuzz00353.json`: `tree element 0 duplicated; /setup/tree/9/c2 = 4294967296`): malformed trees the FFI refuses but the Rust side still runs; probably another cyclic walk not covered by the bounds. Status: open.
  - Differ `fuzz00567.json` (`/setup/opts/2 = 1e-300`): `node_geom.len` C++ 650, Rust 150. A tiny tessellation tolerance; probably a Rust subdivision bound (`MAX_SUBDIVISION_NODES` or a depth bound). Status: open.
- `intersect.curve_class_p5` (0/2/0/0 -> 0/0/0/0). C++ SIGSEGV `fuzz00653.json`, `fuzz01268.json` (`/bounds truncated to 1`): UB-109. Fix: FFI `op == 6 && n == 0 && flag1 != 0` (`crates/solidcore-ffi/src/units/intersect_curve_class_p5.rs:90`, narrowed by the verifier). Status: fixed.
- `intersect.ray_tracer` (0/58/9/7 -> 0/0/0/0). Rust only, no C++ defect.
  - 39 aborts allocating 0.2-17 TB for `/grid/size/K = 2^31..2^32` (e.g. `fuzz00042.json`). Fix: `Grid::new` reserves fallibly -> `SM_ERR`, as upstream's failed `smos_Calloc` (`crates/solidcore/src/geom/grid.rs:482`). Status: fixed.
  - 19 aborts (34-68 GB) and 8 panics `multiply with overflow` (`intersect_ray_tracer.rs:338`) for a Bezier degree `p`/`q` of `2^31..2^63` (e.g. `fuzz00273.json`, `fuzz00416.json`). Fix: the runner refuses a degree not below the net size before sizing knots (`crates/difftest/src/units/intersect_ray_tracer.rs:338`). Status: fixed.
  - 7 Rust timeouts with renderer `nx`/`ny = 2^31..2^63` (e.g. `fuzz00479.json`). Fix: mirror the FFI renderer bound (`1..=8`, rate `1..=3`) (`intersect_ray_tracer.rs:593`). Status: fixed.
  - 1 `capacity overflow` panic `fuzz01589.json` (`/grid/size/2 = 9223372036854775807`): covered by the grid fix.
- `intersect.surface_drop_lib_p2` (1/6/7/0 -> 0/0/0/0).
  - 13 runner aborts/panics building a chain of `/n = 2^31..2^63` spans (`fuzz00210.json`, `fuzz00510.json`, ...). Fix: `expand` refuses `n >= ffi::MAX_ITEMS` (20000) (`crates/difftest/src/units/intersect_surface_drop_lib_p2.rs:264`). Status: fixed.
  - Differ `fuzz01583.json` (`/surface/domain/2 = -1.0`): plane with an inverted domain; Rust `create_canonical` fails, the C++ continues with a NULL NURB (UB-68). Fix: FFI rejects a non-positive domain extent (`crates/solidcore-ffi/src/units/intersect_surface_drop_lib_p2.rs:387`). Status: fixed.
- `intersect.surface_intersector_p5` (0/1/0/0 -> 0/0/0/0). C++ SIGSEGV `fuzz00950.json` (`/a/domain/2 = -1.0`): UB-68 through `sm_CreateBezierPatchBlock`. Fix: FFI domain check (`crates/solidcore-ffi/src/units/intersect_surface_intersector_p5.rs:238`). Status: fixed.
- `offset.offset_executive_p1` (2/14/0/5 -> 2/0/0/5).
  - 14 Rust aborts (1.5 GB) building fixture topology for `original_edges` / `offset_edges` / `face_count` of `2^31..2^63` (e.g. `fuzz00290.json`). Fix: the runner mirrors the FFI `MAX_ITEMS = 4096` (`crates/difftest/src/units/offset_offset_executive_p1.rs:970`). Status: fixed.
  - 2 C++ timeouts, surface control point `-1e300` (`fuzz00315.json`, `fuzz01104.json`). HANG-29 regime, call not isolated. Status: open.
  - 3 Rust timeouts, `/offset_distance = -1.0` or `-1e300` (`fuzz00948.json`, `fuzz01324.json`, `fuzz01699.json`). Cause not established. Status: open.
  - 2 differs at roundoff: `fuzz01232.json` (`edgeuse.18.geometry[7]` C++ `2.22e-16`, Rust `0`), `fuzz01631.json` (`edgeuse.2.geometry[8]` C++ `-4.44e-16`, Rust `0`). Needs a look at operation order or a per-key abs floor. Status: open.
- `heal.heal_data_p1` (0/0/0/4 -> 0/0/0/2).
  - 4 C++ timeouts, surface net `+-1e300` (`fuzz00232.json`, `fuzz00523.json`, `fuzz00977.json`, `fuzz01666.json`). HANG-29 regime. Fix: runner `run_cpp` refuses script values above `1e150` (`crates/difftest/src/units/heal_heal_data_p1.rs:75`, `:94`). Status: fixed for the C++ side.
  - `fuzz00977.json` and `fuzz01666.json` now time out on the Rust side: the guard is in `run_cpp` only, and the faithful Rust heal hangs on the same input. Status: open (mirror the guard in `run_rust`, or bound the library).
- `mass.integrator` (10/0/0/0 -> 4/0/0/0, FUZZ-PASS).
  - 6 differs `status` C++ 1007, Rust 1001 (e.g. `fuzz00069.json` `/geometry/parameters/0 = -1e+300`): the C++ face construction failed, so the Rust side had no trims to import. Harness. Fix: classify as deferred (`Unported`, `crates/difftest/src/units/mass_integrator.rs:420`). Status: fixed.
  - 4 differs remain (`fuzz00051.json` `out[0]` C++ `-inf`, Rust `0`; `fuzz00923.json`, `fuzz01272.json`, `fuzz01487.json` `out.len` C++ 26-27, Rust 0) with `+-1e300` controls. Possibly WR-94-related. Status: open.
- `blend.hcr` (0/0/0/9 -> 0/0/0/0). 7 C++ and 2 Rust timeouts with a curve or surface coordinate `+-1e300` (`fuzz00041.json`, ..., `fuzz00700.json`, `fuzz01870.json`): HANG-40. Fix: FFI `MAX_SCENE_MAGNITUDE = 1e15` on scene and op-7 point (`crates/solidcore-ffi/src/units/blend_hcr.rs:262`, `:178`). Rust: the `Step3DDDA` bound. Status: fixed. The bound is broader than the measured onset (1e18 returns), and generated case `gen0155` lost its values. Needs decision.

**Library resource bounds** exercised by this group (`b7dc0007`, `9d19e0ec`; each recorded in its file header's Deviations; the `Arena` and `Grid` items are also in the geom group's list)

- `Arena::insert` reserves fallibly (`InvalidInput` "arena cannot grow"), `crates/solidcore/src/topology.rs:191`. Turns an allocation abort while growing a topology arena into an error.
- `Grid::new` voxel array `try_reserve_exact` -> `SM_ERR`, `crates/solidcore/src/geom/grid.rs:482`. Upstream `smos_Calloc` fails softly; Rust aborted (ray_tracer).
- `Grid::step_3d_dda` at most `1 + nx + ny + nz` steps per call, `crates/solidcore/src/geom/grid.rs:872`. An advancing walk leaves the grid sooner; a stalled one is HANG-40.
- `SurfaceCache` `FindUnmarkedLeafNode` visit limit `(stack.len() + 1) * (nodes + 1)`, `crates/solidcore/src/surface/surface_cache/p1.rs:2202`; `FindUVNodes` visit limit `2 * nodes + 1`, `crates/solidcore/src/surface/surface_cache/p3.rs:281`. Cyclic child links; an acyclic tree never reaches either limit (query.tess).

**Open items** (FUZZ-FAIL in `compare.txt` after the fixes)

- `topology.edgeuse_p2` (1/0/0/6), `topology.edgeuse_p3` (0/0/0/8), `topology.face_p3` (0/0/0/1), `topology.loopuse` (0/0/0/1): C++ hangs at `+-1e300` net coordinates inside the shared fixture `Export` (`topology.edgeuse_p1` / `topology.vertexuse`), which the runners call before the guarded op. Case files in `fuzz-out/verify2000/<unit>/`. A guard belongs in the shared export, with the valid-input boundary established first (see `topology.vertexuse`, PR #248). Plus the edgeuse_p2 `fuzz00597` sign-of-tiny differ.
- `query.tess` (1/0/0/2): Rust timeouts `fuzz00241.json`, `fuzz00353.json` (malformed trees the FFI refuses); differ `fuzz00567.json` (tolerance `1e-300`, 650 vs 150 nodes).
- `offset.offset_executive_p1` (2/0/0/5): C++ timeouts `fuzz00315.json`, `fuzz01104.json`; Rust timeouts `fuzz00948.json`, `fuzz01324.json`, `fuzz01699.json`; roundoff differs `fuzz01232.json`, `fuzz01631.json`.
- `heal.heal_data_p1` (0/0/0/2): Rust timeouts `fuzz00977.json`, `fuzz01666.json` (guard only on the C++ side).

### blend.fillet_executive_p3 — dependency and coverage boundaries (PR #244)

All three executive control-flow bodies are present, including the continuation
following every missing call. Dependency objects remain in adapters, following the
existing topology-intersector handle model; metadata views never clone a B-rep.
The p1 executive/error-info data needed by these methods is co-ported in
`blend::fillet_executive::p3::{FilletExecutive, ErrorInfo}`; p1/p2 algorithms and
claimed solver/corner/geometry records are not copied.

Native parity currently covers explicit control states over **empty B-reps**.
Passing cases are precondition failures with status/state/tolerance observations;
there are **zero successful geometric fillets**. Construction paths defer at
`SmFilletExecutive::ProcessSmoothEdgeSequences` (executive p2), and initialized
`DoFillet` defers at `CreateFilletCorners` (executive p1). The upstream API fillet
examples listed for this unit require those setup/corner/solver owners and are
not represented as passing geometry cases. Control-flow unit tests with scripted
dependencies check ignored statuses, deferral propagation, both temporary-flag
restorations, selective error-info assignment, and the five stitching attempts;
these are not substitutes for native geometry parity.

Callgraph ownership gap: `SmFilletCorner::MakeCornerTopology` is pure virtual;
its concrete overrides are listed in `unused_functions.txt`, not in units.json.
`ExecutiveAccess::make_corner_topology` is a required virtual-dispatch adapter
method, with no fabricated UNPORTED owner. The future corner adapters must supply
that dispatch when their types can first be constructed.

C++ `SmFilletErrorInfo::operator=` does not copy comment, entity name, location,
side face, fillet-vertex pointers, or fillet curves. Rust retains this exact subset.
The native fixture always explicitly initializes `m_bIsInitialized` (UB-185), and
clears intersector references before freeing a leftover preview B-rep.

The seven fixed examples from `tests/SM_API_test/src/TestSmFillets.cpp` are also
re-expressed as native API fixtures: circular/chamfer on 5×10×15 boxes,
first-four-edge and per-edge-radius fillets, variable radius on 10³ boxes, and
round/bevel-corner variants. Rust defers these fixture entrypoints explicitly to
executive p1; they do not contribute direct p3 passes. Their native outputs are
status and final entity counts, not complete topology parity. Full geometric and
topological parity remains pending the setup and geometry owners.

Metadata getters are intentionally lazy: non-blending/non-tangent corners never
request solver metadata, and corner names/type are queried only on error.
`SmFace::Notify` is a required executive adapter to `topology::face::p3::notify`,
with `PreEdit`, the face, null and null forwarded as separate arguments.
`SM_NO_GET_BREP(face)` expands to a null object (`SmTypes.h:659`), not `GetBrep()`.

Current UNPORTED audit for #244 (main e5f3e441 integrated; 33 symbols / 12 owners):

- `blend.fillet_corner_p1` (`todo`): `SmFilletCorner::GetFilletSolvers`, `SmFillet4x3Corner::AdjustTopology`, `SmFilletNxNCorner::AdjustTopology`, `SmFilletCorner::AdjustTangentCorner`, `SmFilletCorner::AdjustTopology`, `SmFilletCorner::CalcCornerEdgeGeom`, `SmFilletCorner::CalcCornerVertGeom`, `SmFilletCorner::CheckAndFixDegeneracy`, `SmFilletCorner::InsertIntersectionTopology`, `SmFilletCorner::ReInitializeCornerEdges`.
- `blend.fillet_corner_p2` (`todo`): `SmFilletConvexNxNCorner::AdjustTopology`, `SmFilletNx1ClosedCorner::AdjustTangentCorner`.
- `blend.fillet_corner_p3`: none since it became `partial` on main (#256). `SmFilletCorner::CalcCornerGeom` is the required adapter `ExecutiveAccess::calc_corner_geom`, documented as delegating to `FilletCorner::calc_corner_geom`. Nx2 `AdjustTopology` / `AdjustTangentCorner` dispatch to corner p3's `FilletCorner::adjust_topology` / `adjust_tangent_corner`.
- `blend.fillet_executive_p1` (`todo`): `SmApiChamferFillet`, `SmApiCircularFillet`, `SmApiFilletEdges`, `SmApiFilletEdgesPerEdge`, `SmApiSetBevelCorners`, `SmApiVariableRadiusFillet`, `SmFilletExecutive::CreateFilletCorners`, `SmFilletExecutive::DoPiecewiseMerge`, `SmFilletExecutive::RemoveTopoEdges`.
- `blend.fillet_executive_p2` (`todo`): `SmFilletExecutive::ProcessSmoothEdgeSequences`, `SmFilletExecutive::SetFilletBrepEdgeMap`.
- `blend.fillet_geom_p4`: none since it merged (#252). `SmFilletGeom::MakeFaceBrep` and `SmFilletGeom::InsertIntersectionTopology` are the required adapters `ExecutiveAccess::make_face_brep` and `insert_geom_topology`, documented as delegating to `FilletBrep::make_face_brep` and `FilletBrep::insert_intersection_topology`. Status handling is unchanged: the `MakeFaceBrep` status is ignored per solver (`:2519`), and `InsertIntersectionTopology` propagates (`SER`, `:3113`).
- `blend.fillet_geom_p6` (`todo`): `SmFilletGeom::TrimOriginalSurfaces`.
- `blend.fillet_solver_p1`: none since it became `partial` on main (#192). `SmFilletSolver::TrimRails` is the required adapter `ExecutiveAccess::trim_rails`, documented as delegating to `FilletSolver::trim_rails`.
- `blend.fillet_solver_p4` (`todo`): `SmFilletSolver::CalcFilletGeom`.
- `blend.fillet_standard_solver_p2` (`todo`): `SmSurfaceSurfaceFS::CalcFilletGeom`.
- `blend.fillet_solver_p5` (`todo`): `SmFilletSolver::TestRollOver`.
- `heal.stitch_p2`: none since it became `partial` on main (#205). `SmStitch::DoStitching` is the required adapter `ExecutiveAccess::do_stitching`, documented as delegating to `heal::stitch::p2::Stitch::do_stitching`.
- `topology.brep_p2`: none since it became `partial` on main (#149). `SmBrep::SetTolerance` is the required adapter `ExecutiveAccess::set_brep_tolerance`, delegating to `topology::brep::p2::set_tolerance`, and its status is ignored (`void` upstream).
- `topology.brep_p3` (`todo`): `SmBrep::MergeBrep`, `SmBrep::OrientTrimmedSurfaces`.
- `topology.brep_p4` (`todo`): `SmBrep::StitchFaces`.

Direct-state output also compares both full comment byte sequences (not merely
lengths). Fixture intersector setup preserves the executive constructor's
`ignore_tangency = true` and `SM_DEG2RAD(30)` operation order. Passing operations
all return early error statuses; their observed post-error state is compared as
data rather than being discarded by error-string normalization.

Verification for the full-comment-byte candidate after merging main `05731370`:
fmt (including the included runner/FFI/test files), both workspace clippy modes,
1,389 kernel tests, four FFI guard tests and wasm pass. Direct parity remains
287 cases / 112 state-error passes / 0 failures / 175 deferred, with_values=112
and no_values=0, max_rel_err=0. Perturbation detects 112/112 changed cases.
Fuzz: 2,000 cases, rejected=1764, malformed=21, ran=215, differs=0, crashes=0,
panics=0, timeouts=0. Valgrind compares 40 C++/Rust programs with their baseline:
matching protocol, zero errors, zero definitely/indirectly/possibly lost bytes.
These checks establish no successful geometric or topological parity.
The final parallel green/partial matrix passes all 210 units; a separate per-unit
log audit confirms zero failures and zero units declared green with deferrals.


### blend.fillet_executive_p3 — response to bb84f1a5 pre-review (PR #244)

- Removed all three inactive `cbi_MAP_TOPO_EDGES` experiments: both intersection
  collection sites and the extra mate-attribute scan. The pinned C++ macro is 0.
  The regression supplies populated real mate maps and no original topo candidates,
  checks unchanged intersection arrays with trimming off/on, and checks that no
  extra candidates or keep attributes are introduced.
- All parent-linked corner/solver/geometry stage hooks now borrow the live
  `FilletExecutive`, including topology creation, degeneracy, adjustment,
  vertex/edge/face geometry, rail trimming, rollover, insertion and surface trimming.
  This carries options, topo edges, error info and the existing intersector through
  every stage; adapters must not keep a duplicate executive/intersector. Insertion
  regressions verify mate and SSI writes across hooks and into the subsequent mark
  and trim phases; other regressions check global-merge enablement and candidate
  deletion before the attribute scan.
- Hook audit: `GetFilletSolvers` resolves edgeuses through the parent solver list
  (`SmFilletCorner.cpp:4073`), so it now receives the executive read-only; error-only
  metadata also receives it. Solver/geometry error notifications can reach parent
  `NoteFilletError` (`SmFilletSolver.cpp:7943`, `SmFilletGeom.cpp:13220`), covered by
  the mutable stage contract. Standalone B-rep, stitch and attribute hooks receive
  their explicit objects/context; intersector operations already borrow the actual
  intersector. The remaining corner predicates/point/tolerance getters and the
  nonvirtual `SetCalcAsConstRad` are local record accesses, not parent mutations.
- The intersector-p1 change is a cfg(test)-only mate-map fixture initializer; it
  changes no production API/algorithm. It avoids a second, fake intersector in the
  regression world. The full matrix rechecks intersector p1 as well as p3.
- Native and Rust fixture setup now both construct a zone tolerance: the positive
  `SM_USE_DEFAULT` sentinel converts before `fabs`, and finite negatives are
  accepted and absolutized. Fixed paired cases include the positive sentinel,
  -0.25, a tiny negative, and the negative sentinel magnitude (which must not be
  confused with the positive default sentinel).
- Temporary keep attributes now survive orientation and are unlinked afterwards,
  including on failures and deferrals. Tests observe live attributes in orientation
  and cleanup both there and when merge exits earlier.
- Face construction and first-geometry queries now use indexed live access;
  snapshot copies remain only where C++ explicitly copies its array. A regression
  appends geometry during face creation and verifies two visits with zero array
  snapshots, preserving live upstream iteration rather than capturing a stale list.

Validation of this revision: fmt (including included files), both clippy modes,
1,401 core tests (18 executive tests), four FFI tests and wasm pass. Paired parity:
291 cases / 116 state-error passes / 0 failures / 175 deferred (p1=46, p2=129),
with_values=116, no_values=0, max_rel_err=0. All four new constructor-domain cases
match C++. Perturb detects 116/116 changes. Fuzz 2000: rejected=1720, malformed=16,
ran=264, differs=0, crashes=0, panics=0, timeouts=0. Valgrind: 52 valid programs
ran on both engines, baseline protocol equal, zero errors and zero lost bytes
(definite/indirect/possible), no suppressions. The final 211-unit matrix passes;
its per-unit log audit confirms zero failures and green_but_deferred=0.
Coverage still establishes zero successful geometric/topological comparisons.


### blend.fillet_executive_p3 — response to cbfc8dcc Opus review (PR #244)

- Corrected `SmFace::Notify(PreEdit, face, NULL, NULL)` and its scripted
  regression. `SM_NO_GET_BREP(face)` is the null-object macro at `SmTypes.h:659`;
  the alternative `GetBrep()` macro is commented out. The earlier note above now
  states the actual payload. No native passing fixture reaches this stub today.
- `AdjustTopology`, `AdjustTangentCorner` and `CalcFilletGeom` are now required
  virtual-dispatch adapters, with no inherited base-owner default deferral.
  Adapters must resolve the actual concrete type and either call its implementation
  or return `Unported` for that implementation's owner. Source tags list every
  owner: base/4x3/NxN topology adjustment in corner p1, ConvexNxN adjustment and
  Nx1Closed tangent adjustment in corner p2, Nx2 adjustment/tangent adjustment
  in corner p3, base fillet geometry in solver p4 and SurfaceSurfaceFS geometry
  in standard-solver p2. This prevents future base wiring from silently replacing
  a missing derived implementation. The empty-B-rep runner constructs no corners
  or solvers and rejects nonexistent handles in these required adapters.
- `corner_solvers` is now a default `UNPORTED(blend.fillet_corner_p1)` stub for
  `SmFilletCorner::GetFilletSolvers`. This is real owned logic, not a getter:
  `SmFilletCorner.cpp:4067-4075` resets the result, loops over the edgeuse count
  and **always looks up edgeuse [0]**, preserving repeated entries of the first
  solver. The future port must keep that source quirk, including executive p1's
  `GetFilletSolverOfEdgeuse` lookup. It has not been executed or established as
  an upstream defect, so no WR id is reserved. The scripted tangent regression
  now returns repeated first-solver entries; it is not a port of that algorithm.
- Extended ownership audit: the tags now cover **33 distinct symbols / 12 owners**,
  all `todo` in the merged plan; the complete list above includes all new tags.
  Added GetFilletSolvers; added seven derived symbols and two new owners; removed
  no dependency owners. Required-but-owned-elsewhere corner accessors are also
  explicit: `IsBlendingCorner`, virtual `IsTangentCorner` (all current overrides
  in corner p1), `SetCalcAsConstRad`, `GetFilletedVertex`, and
  corner `GetThisApproxTol3d` are inline corner-p1 record accesses; solver
  `GetThisApproxTol3d` is an inline `blend.fillet_solver_p1` record access. They must use those
  owners' records and concrete dispatch when wired, never supply fake metadata.
  `GetCornerType` is pure virtual, with inline derived constants in
  `SmFilletCorner.h`; those symbols are omitted from units.json. Error metadata
  must use the actual derived type, without inventing a callgraph owner.
  Error-only `GetName` uses the object's real name. These required reads do not
  have default `Unported` implementations and are separate from the 41 tags.
  `MakeCornerTopology` retains its previously documented callgraph ownership gap.
- Converted the solver's raw approximate tolerance with existing `zone_tol3d`
  at the `SetTolerance` call site, matching the implicit `SmZoneTol3d(double)`
  conversion in `SmBrep.h:700`. A scripted regression covers the default sentinel
  in this p3 call (distinct from the paired fixture-constructor sentinel case).
- Coverage is **three distinct p3 control paths, all precondition exits**:
  uninitialized DoFillet; null-target CreateFilletBrep; and DoFilleting reaching
  that null-target check then restoring temporary flags. The paired tolerance
  values only read back fixture setup; the four constructor-domain fixtures
  check harness setup, not a p3 tolerance mutation. No geometric success is paired.
- FFI docs now distinguish exact Boolean encodings / finite-only tolerances as
  fixture-encoding choices, not UB guards. Fuzz over empty B-reps does not establish
  crash safety for eventual geometric paths.
- Open dependencies include solver p1 (#192, `TrimRails`) and brep p3 (#204,
  `MergeBrep`/`OrientTrimmedSurfaces`); `todo` is their merged-plan status, not
  absence of claimed work. Solver p1 uses owned solver objects, so wiring the
  handle adapter will require a shim that mutates the same executive-backed object.
  Likewise executive p1 must allocate the actual `m_pAttributeE` on construction;
  the runner's current `edge_attribute: None` is unreachable before deferral.

Validation of this revision on sc-builder-7: fmt (including include!-ed files),
both workspace clippy configurations, 1,414 core tests (19 executive regressions),
four FFI guard tests and wasm pass. Paired results: 291 cases / 116 passes /
0 failures / 175 deferrals (executive p1=46, p2=129), with_values=116,
no_values=0, max_rel_err=0. Perturb detects 116/116 changes. Fuzz 2000:
rejected=1720, malformed=16, ran=264, differs=0, crashes=0, panics=0,
timeouts=0. Valgrind: all 52 valid programs actually ran on both engines,
protocol equal to baseline, 0 errors and 0 definite/indirect/possible lost bytes,
no suppressions. The final 213-unit green/partial matrix passes; the strict
per-unit log audit confirms zero declared-green units with deferrals.
The three paired control paths remain precondition exits, with no geometric
success comparisons. Latest main 0cf3e290 is merged; unrelated plan entries are
unchanged from main (only executive p1 co-port notes and this unit differ).


### blend.fillet_executive_p3 — clean re-check and readiness integration (PR #244)

The Opus re-check of 612c6631 is CLEAN. Its runner-error nit is folded in:
`adjust_topology`, `adjust_tangent_corner` and `calc_fillet_geom` now return
`SolidError::Unported` for the base corner-p1 / solver-p4 symbols instead of
`InvalidInput` through `missing()`. This empty-B-rep fixture constructs no concrete
corner or solver; future fixtures must dispatch real derived objects to their
actual owners. Explicit UNPORTED tags sit on each runner stub body, and the
complete override-owner tags remain discoverable at the required trait methods.
The audit counts 33 distinct upstream symbols / 12 owners, including required
virtual-dispatch targets, not 33 independently implemented stub bodies.

Merged origin/main 46ddc4ff (#179 blend.fillet_intersector), preserving both
`fillet_executive` and `fillet_intersector` exports in the sorted blend module list.
None of this unit's 41 deferred callee symbols is among #179's 12 owned functions
or its solver `SetSurfacesToZeroOffset` / `ReloadSurfaceOffsets` co-ports; no
executive-p3 stub points at fillet_intersector. No new call can therefore be wired
here. The intersector's internal solver/geometry integration remains with those
owners. The main merge retains the incoming fillet intersector/custom-solver code
and all unrelated plan entries unchanged.

The subsequent main 8e79624a update changes only generated PROGRESS.md and is
also merged; the tested kernel and 214-unit plan are unchanged from 46ddc4ff.

Readiness validation after this merge: fmt (including include!-ed files), both
workspace clippy configurations, 1,422 core tests (19 executive regressions),
four FFI tests and wasm pass. Paired results remain 291 cases / 116 passes /
0 failures / 175 deferrals (executive p1=46, p2=129), with_values=116,
no_values=0, max_rel_err=0. Perturb detects 116/116 changes. Fuzz 2000:
rejected=1720, malformed=16, ran=264, differs=0, crashes=0, panics=0,
timeouts=0. Valgrind: all 52 valid programs actually ran on both engines,
protocol equal to baseline, 0 errors and 0 definite/indirect/possible lost bytes,
no suppressions. The final 214-unit matrix passes, with zero declared-green
units carrying deferrals in the strict per-unit log audit. Coverage remains three
paired p3 precondition paths and zero geometric-success comparisons.


### blend.fillet_executive_p3 — shared corner metadata (#244 / #247)

Per brett19's coordination request, `CornerView::corner_type` now reuses
`blend::fillet_geom::p1::FilletCornerType`. The error-info record carries the same
strongly typed value, including `Unknown` as its existing zero default, so the
kernel propagates the enum without integer conversion. The scripted corner-error
regression uses `NByTwo` (upstream discriminant 9). The runner and native fixture
explicitly encode the two error-info corner kinds at their wire boundary; paired
precondition fixtures observe `Unknown`, not geometric corner classification.
Existing numeric values and the three precondition-only control paths are retained.
The requested read-only `CornerView` co-port note is appended to corner-p1 notes.

The canonical enum is not on main c5c8395a yet. This branch therefore builds on
#147 (fillet_geom_p1 head 4a987837), merged as-is per AGENTS.md rather than copying
the claimed enum. The p3 dependency on geom-p1 is now hard because its type is
required. Incoming #147 code, co-port notes and regenerated reports are preserved;
no geometry-p1 algorithm is reimplemented. Validation before the #259 batch merge: fmt/includes, clippy x2, core 1431/1431
(19 executive tests), FFI 4/4 and wasm pass. Paired 291/116/0/175,
with_values=116 no_values=0 max_rel_err=0; perturb detects 116/116 changes.
Fuzz 2000: rejected=1720 malformed=16 ran=264 differs=0 crashes=0 panics=0
timeouts=0. Valgrind: 52 actual programs, both engines, baseline equal, zero
errors and zero definite/indirect/possible lost bytes, no suppressions. Main then
merged #259 during verification; its integration and final gates are pending.


### blend.fillet_executive_p3 — main batch #259 compatibility

Merged main 4db3d6602, retaining #147's canonical enum and cache changes and both
sides' upstream issue entries. The existing mate-map test initializer remains
cfg(test)-only. The now-ported `SmFace::Notify`,
`SmTopologyIntersector::DeleteFaces` and `IntersectInsertRelate` no longer have
`UNPORTED` defaults: like the existing ported BooleanPostProcess hook, these are
required storage adapters. Their contracts name the canonical APIs and require
the live records, attributes, cache and intersector world/identity. No dependency
algorithm is copied. The empty-B-rep runner cannot supply those nonexistent face
handles or a geometry/intersector world and explicitly rejects them, as its
other topology accessors do; no passing native fixture reaches these adapters.
Scripted tests override them only to exercise p3 continuation order. A concrete
executive p1/p2 object adapter is still needed for geometric integration; this
change does not claim a new geometric pass. The current UNPORTED inventory is
33 symbols across 12 owners; all remaining tagged callees are still unported.
Final integrated-tree checks pending.

### blend.fillet_executive_p3 — closed-manifold check after `DoFillet` (issue #422, WR-212)

- **Deviation from upstream.** `FilletExecutive::do_fillet` now ends with
  `check_closed_manifold`: it walks the target's edges in `GetEdges` order and
  refuses the first one that is not `SmEdge::IsManifold`
  (`topology::edge::is_manifold`: four edge uses, the first a loop edge use). The
  refusal sets the error info to `SM_FILERR_BAD_MERGE` (7), `SM_FIL_FAILURE`, that
  edge and a comment, and returns `SolidError::Upstream { code: 1001 }` with the
  edge's `EntityRef` (from `ExecutiveAccess::edge_entity`, `None` for adapters
  whose handles have no typed id). Upstream returns success with the body.
  `do_filleting` is unchanged, so its difftest cases still compare as before.
- **Why.** SolidCraft's rig case `fillets/324-fillet-over-hole-overlap` (an R4 round
  on the top long edge of a 40x20x10 block, where a through hole of r=3 at (20, 14.5)
  reaches into the round) comes back from the C++ as success with a body that is not
  a closed manifold solid. The C++ that ran was the tangency-labs/smlib fork, called
  through `smlib_blend`. `trim_and_merge` passes `bMaybeNotSolid` to
  `OrientTrimmedSurfaces` and ignores it, as upstream does, so nothing on that path
  refuses the result. The defect that breaks the shell is not found: the Rust port
  can't run this fillet yet (`blend.fillet_executive_p1`, `fillet_geom_p1`,
  `fillet_corner_p1` and others are unported), and no C++ reproduction was built
  in this harness.
- **Parity.** No difftest case changes. The p3 runner's `do_fillet` op is always
  deferred (`CreateFilletCorners` is unported), and the p2 and p3 runners' new
  `edge_is_manifold` adapters return "fixture has no such object", as their other
  unused edge queries do. Once `blend.fillet_executive_p1` lands, a `do_fillet` case
  whose C++ result is not manifold will fail by design: classify it against this
  entry, don't remove the check.
- **Tests.** `p3_tests.rs` hand-builds a tetrahedron with full use topology (mated
  face uses, loop uses, four loop edge uses per edge). The closed body passes. An
  open body (one face missing) is refused at its first laminar edge, and a body
  with a fin (a fifth face on one edge, six uses) is refused at that edge.
  `do_filleting` alone still accepts the open body.
