# General

- **Nondeterministic upstream paths.** `SmFace::GetPointsInFace` (asked for more than one
  point) and `SmShell.cpp` call `srand(time(NULL))`. Those paths can't be reproduced bit for
  bit. Difftests must avoid them, or compare only results that don't depend on the random
  draw. The Rust port draws from `kernel.rng` instead. (Reported by Wave 0 agents B and C;
  see `plan/closure.md` for the other hazards: pointer-keyed maps, global tolerances, and
  statics set by the first call.)
- **NLib global error record.** NLib routines return a flag and store the error number in
  the process-wide `NL_ERROR.eno` (`N_ErrSet`). Shims must call `N_ErrClear()` before the
  call and read `NL_ERROR.eno` after it. On the Rust side this is
  `SolidError::Nlib { code: NlibCode, .. }` (added to `crates/solidcore/src/error.rs` with
  `nlib.knots`). Normalise both sides to `nlib_<number>` in runners.
- **JSON can't hold NaN or inf.** difftest inputs are JSON, so generated cases can't contain
  non-finite parameters (serde turns NaN into `null`). Test robustness to non-finite input in
  Rust unit tests. Outputs may still be non-finite.
- **Shims: one argument-reader call per statement.** C++ leaves the evaluation order of
  function and constructor arguments unspecified: `SmPoint3d p(ad.num(), ad.num(),
  ad.num())` reads `(x, y, z)` on arm64 g++ but `(z, y, x)` on x86-64 g++. The
  `topology.face_p4` line op passed on arm64 and failed 326 cases on x86-64 CI (lines through
  permuted points). Read each value into a named local first (`double x = ad.num(), y =
  ad.num();` is sequenced).
- **Graphs after deletions.** An arena cannot free a record, so a Rust op that deletes
  topology resets it, while the C++ side exports a fresh walk. `boolean_merge_p1::graph`
  numbers every arena record (unreachable ones last), so its counts and lists differ
  after a deletion; `graph_reachable` (and `Canon::reachable`) compares only the
  topology reachable from the regions and the edge list. Also, the shared importer
  leaves the Rust-only caches `Face::loops` and `Loop::edges` empty: a runner whose
  port fills them (`topology.brep_p9`'s `sync_face_cache`) empties them before
  comparing. Both surfaced in `topology.brep_p9` once `topology.face_p4` let its
  MakeVertexLoop / MakeEdgeInFace paths run.
- **difftest report `seeds` field.** It records the `--seeds` CLI value, not the number of
  generated cases a runner actually produced. `nlib.knots` overrides `cases()` to always
  generate at least 1200.
- **The C++ reference is an x86-64 oracle where upstream casts out-of-range doubles to
  integers.** C++ leaves `(ULONG)d` and `(long)d` undefined when the truncated value does
  not fit (negative for unsigned, NaN, infinite, too large). x86-64 g++ emits `cvttsd2si`:
  negative values wrap, NaN and out-of-range values give `0x8000000000000000`. aarch64
  g++ (Apple Silicon, ARM Linux) emits `fcvtzu` / `fcvtzs`, which saturate: negative
  values and NaN give 0, `+inf` gives the maximum. The port reproduces x86-64, the CI
  reference, in three places: `geom::hash_table::ulong_from_f64` (`MapPointToVoxel`,
  UB-35), `heal::heal_data::p2::edge_len_histogram_index` (`SM_HISTOGRAM_EDGELEN_INDEX`,
  UB-194) and `surface::twist_volume::trunc_long` (no current case reaches the last one:
  `surface.twist_volume` passes on aarch64). On an aarch64 host the native
  reference therefore disagrees by design: `geom.hash_table` fails 435 of 619 cases,
  `heal.heal_data_p1` 2, `heal.heal_data_p2` 41 and `heal.heal_data_p3` 2. With the
  first two emulations switched to aarch64 semantics (`v as u64`), all four units agree
  bit for bit (619 of 619; 265, 347 and 267 passed, none failed). Checked on 2026-10-17
  on macOS 27, arm64, GCC 13.4.
  Judge these units on x86-64 only. Keep the Rust emulation as it is: the library must
  return the same result on every target, wasm included.
- **Math libraries differ by platform (2026-10-17).** difftest proves solidcore matches
  the C++ bit for bit only on Linux x86_64 with glibc. There, Rust's `f64::sin` and the
  C++ `sin` both call glibc, and so does every other measured function, with one
  exception: `cbrt` (below). Elsewhere Rust calls a different libm. On 200,000
  kernel-like inputs per function, against glibc 2.39 x86_64 with FMA,
  `harness/mathprobe/run.sh` measured (differing results, max ulp). Columns are glibc on
  arm64 (what the C++ gets there), then Rust on macOS (Apple libm), on
  `wasm32-unknown-unknown` (Rust's libm) and in a pure-Rust Linux x86_64 binary:

  | fn (solidcore call sites) | glibc arm64 | Rust macOS | Rust wasm32 | Rust Linux x86_64 |
  |---|---|---|---|---|
  | `cos` (83) / `sin` (74) | 0 / 0 | 3.4% / 3.2%, 1 | 2.4% / 2.5%, 1 | 0 |
  | `atan2` (14) | 0 | 0.4%, 1 | 17.1%, 1 | 0 |
  | `powf` (12) | 0 | 0.2%, 1 | 6.2%, 1 | 0 |
  | `tan` (9) | 0 | 39.4%, 3 | 2.9%, 1 | 0 |
  | `sinh` / `cosh` (4 each) | 0 / 0 | 20.7% / 19.1%, 2 | 2.0% / 9.6%, 2 | 0 |
  | `log10` (3) | 0 | 9.1%, 2 | 9.1%, 2 | 0 |
  | `acos` (2), `asin`, `atan`, `ln` (1 each) | 0 | 0.02–12.3%, 1 | 1.1–6.2%, 1 | 0 |
  | `cbrt` (1) | 19.3%, 4 | 47.5%, 3 | 47.5%, 3 | **47.5%, 3** |
  | `powi` (6) | | (reference) | 0 | |

  **Which `cbrt` Rust gets on Linux depends on link order** (found by PR #225's
  review, then checked with `nm` and a mixed Rust/C++ binary on Linux x86_64).
  `compiler_builtins` defines `cbrt` itself, and the call binds to that copy unless
  libm is linked before it. A pure-Rust binary (the probe, or an application using
  solidcore) gets `compiler_builtins`' `cbrt` (`nm`: local `t cbrt`; every other
  function is an undefined glibc import). That copy is identical to the wasm column and
  1–3 ulp off glibc in 47.5% of cases. A binary that links `-lm` ahead of it, as
  `solidcore-ffi`'s `build.rs` does (`stdc++`, `m`), imports glibc's `cbrt`
  (`U cbrt`). Its Rust and C++ sides then agree on all 200,000 inputs and match
  `libm.so.6`. On macOS, Rust imports `_cbrt` from libSystem (Apple's). The one caller
  is `smos_CubeRoot` in the cubic solver (`geom/vector3d.rs` `cbrt`, used at
  `geom/polynomial.rs:155,170-171`; upstream `SmMath.cpp` `sm_cbrt` = `std::cbrt`).

  `sqrt` is correctly rounded everywhere, so it is not measured. Consequences:
  - The wasm build computes slightly different results than the C++ reference that CI
    proved parity against, and nothing tests the wasm build's values (CI only runs
    `cargo check` for wasm). Single-call differences are about 1e-16 relative, far
    inside difftest's 1e-9, but they can compound in iterative solvers or flip a
    branch that sits exactly at a tolerance. Whether that ever happens is not
    measured yet. The next step is to replay the Rust side on wasm against Linux results.
  - `geom.polynomial`'s difftest is bit-identical (`max_rel_err = 0`) only because the
    difftest binary links glibc's `cbrt` into both sides. A pure-Rust build of solidcore
    on Linux x86_64, which is what users run, solves cubics with `compiler_builtins`'
    `cbrt`. Its roots can then differ from the C++ by an ulp, well within 1e-9, but not
    bit-identical, and difftest cannot see it. Making the port call glibc's `cbrt`
    explicitly, or one fixed implementation on both sides, would close that gap.
  - Running difftest on macOS (once the harness builds there) or on Linux arm64
    (`cbrt`) shows libm disagreements that are not port bugs. Compare there only
    against references produced on the same platform.
  - Options, undecided: document "bit-identical on Linux x86_64 glibc, within a few
    ulp elsewhere"; route every transcendental through one pure-Rust libm (native and
    wasm agree, but no longer bit-match the glibc C++); or correctly rounded math on
    both sides (identical everywhere).
- **difftest is x86-64-only where upstream converts out-of-range doubles to integers.**
  Upstream relies on `(ULONG)double` / `(long)double` conversions that are UB for negative,
  NaN or out-of-range values, and the port reproduces what x86-64 g++ emits for them
  (`cvttsd2si`: a signed truncation, so `(ULONG)-3.0 = 2^64 - 3`, NaN `= 2^63`). On arm64
  (AArch64 `fcvtzu` / `fcvtzs`, with GCC 13 or Apple clang) the same C++ saturates:
  negatives and NaN give `0`, `+inf` gives `ULONG_MAX`. So arm64 hosts disagree with the
  port wherever such a value is reached; macOS arm64 and Linux arm64 fail with identical
  counts, while CI (x86-64) passes. Investigated on main `e1ae2640` in an Ubuntu 24.04 arm64
  container (GCC 13.3), 2026-10-18:
  - `heal.heal_data_p1` 3 failed, `heal.heal_data_p2` 104 failed, `heal.heal_data_p3`
    7 failed. With the two Rust conversions temporarily switched to Rust's saturating
    `as u64` (what `fcvtzu` does), all three units pass with 0 failed and unchanged
    deferred counts (267 / 441 / 279 passed). Switching only one site at a time: the voxel
    mapping alone (UB-35, `geom::hash_table::ulong_from_f64`) fixes 0 / 74 / 5 of the
    failures, the edge-length histogram alone (UB-194,
    `heal::heal_data::p2::edge_len_histogram_index`) fixes 2 / 26 / 1, and the remaining
    1 / 4 / 1 cases need both.
  - Example, `heal.heal_data_p2` fixture `wire_edge_degenerate` (`out.len` cpp 2818, rust
    2833): `Cache_CoinVertices` sizes a 12x12x12 voxel model over the box of the wire's
    single vertex, so the box has zero length and `(x - min) / len` is `0 / 0 = NaN` on all
    three axes. x86-64 maps NaN to `2^63`, which clamps to voxel `(11, 11, 11)`. The point
    then lies on that voxel's low boundary, and `Add` files 3 neighbour copies (4 point
    items, 15 more output values in the voxel item dump). arm64 maps NaN to `0`, voxel
    `(0, 0, 0)` has no lower neighbours, and C++ keeps 1 item.
  - Example of the histogram: `SM_HISTOGRAM_EDGELEN_INDEX(0.01)` is
    `(ULONG)floor(log10(0.01)) + 6`. That is bucket 4 on x86-64 (`2^64 - 2 + 6` wraps)
    and bucket 6 on arm64 (`0 + 6`), so every edge in `[1e-5, 1)` lands in bucket 6
    (for example `heal.heal_data_p1` `gen0086`: `out[3588]` cpp 0, rust 4).
  - `geom.hash_table` (status green) fails the same way on arm64: 70-76 of every 100
    generated cases fail. A run of all 619 cases segfaults in the C++
    `SmObjsInVoxels::FindCoincidentPairs` (gdb backtrace through `sc_geom_hash_table_floats`),
    but with the Rust conversion switched to saturating all 619 pass and nothing crashes.
    The shim's guards were checked only on x86-64, so arm64's voxel choice probably reaches
    a bucket read they never meant to allow. Not investigated further.
  - Not a port bug: x86-64 Linux is the reference platform (`plan/upstream-issues.md`,
    *Build*), and making the port follow the host would make Rust results depend on the
    platform (wasm32 included). Use x86-64 (CI, or an x86-64 Linux VM/container) for any
    difftest that reaches these conversions. Other x86-64 emulations of the same kind
    (the `(long)` of a NaN quotient in `EvaluateBoundingBoxSimple`, under
    `surface.twist_volume`) will disagree on arm64 in the same way.
- **Difftest parity depends on GCC and LLVM fusing the same `sin`/`cos` pairs into
  glibc `sincos` (issue #357).** At `-O2` both compilers merge `sin(x)` and `cos(x)` of one
  argument into a single `sincos()` call when both sit in one basic block (`libSMLib.a`:
  32 `sincos` references; the solidcore rlib: 25). glibc 2.35's `sincos` is not
  bit-identical to `sin`: for x = -0.2914567944778674 (`0xbfd2a73a661eaf0b`) a C repro
  (gcc 11.4) gives `0xbfd263e862c542da` at `-O2` (fused) and `0xbfd263e862c542d9` with
  `-O2 -fno-builtin-sin -fno-builtin-cos` (separate `sin`); `cos` is
  `0x3feea6834f48c4b7` both ways. So a site where only one compiler fuses can differ by
  1 ulp, and only on machines/libms where that argument rounds differently, which is why
  `geom.polar_box`'s `arc_to_region` showed `max_rel_err` 2.14e-16 on one machine and 0 on
  another. Disabling fusion in the C++ build is not a fix: with `-fno-builtin-sin/-cos` 27
  green reports change and 4 units fail, because the sites where both sides fuse then
  disagree. Fix per site instead: `PolarBox::evaluate_normalized` (`SM_PS_REGION`) is fused
  by GCC, but LLVM sinks `sin` past the 90-degree early return and calls `sin`/`cos`
  separately; `core::hint::black_box(x.sin_cos())` keeps each pair in one block so LLVM
  emits `sincos` too (`sin_cos` alone produces the same IR as separate calls). To check a
  suspect site: `objdump -d -r` the function in `bin/cpp/lib/libSMLib.a` and in the
  solidcore rlib and compare `sin`/`cos`/`sincos` relocations.

- **The shared B-rep import does not keep vertex-use order.** `topology_vertexuse::import`
  (used by every runner that imports a real B-rep from `sc_topology_fixture.h`) lists each
  vertex's uses in arena order, because the export does not carry the order. So
  `topology::vertex::edges` / `vertexuses` on an imported B-rep can return a different order
  from upstream's `SmVertex::GetEdges` / `GetVertexuses`. For `plane_square.smb`, vertices
  1–3 come out reversed. Results that take "the first" edge or use of a vertex can disagree
  for that reason alone. The `boolean.topology_intersector_p3` runner hit this in
  `sm_MakeConsistentClassifications`; its shim exports each vertex's list order
  (`VertexUses` op) and the runner reorders the imported lists. Exporting the order in the
  shared fixture would fix every runner at once.
- **The `heal.heal_data_p1/p2/p3` and `geom.hash_table` failures seen in a Debian
  bookworm container on an Apple Silicon host are the arm64 conversion difference above,
  not GCC 12.** Docker there runs `linux/arm64` (`uname -m`: `aarch64`). On `main`
  `bb137d7d` the container gives 3 / 104 / 7 / 435 failed. With the two Rust conversions
  switched to saturating `as u64` for the experiment, `heal.heal_data_p2` passes every case
  it runs (452 on the `boolean.topology_intersector_p3` branch, 0 failed), including 4
  generated cases (`gen0070`, `gen0211`, `gen0544`, `gen0549`) that `main` defers and that
  branch reaches through `intersect.curve_class_p4`'s wired part-2 calls. An earlier note
  here blamed the compiler; that was wrong. (2026-10-20.)
