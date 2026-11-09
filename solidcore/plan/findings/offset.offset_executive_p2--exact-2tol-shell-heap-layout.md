# offset.offset_executive_p2: exact-2tol shell vertex bounds follow native heap layout (2026-10-25)

- **Symptom.** `shell_boundary_negative_exactly_two_tol`
  (`fixtures/offset.offset_executive_p2/tolerance_boundaries.json#3`: plane sheet at
  z = 1, 2 x 3, tolerance 1e-6, static shell by -2e-6 with extend/merge/create-solid)
  failed in some CI runs only, always with the same value:
  `values.result.geometry[6]` (result vertex bounds, max z) C++ `9.999985000000001e-1`,
  Rust `9.99999e-1` (rel 5e-7). Locally and on main's push CI both sides gave
  `0.999999`, bit for bit on all 34 values.
- **Not the CPU, libm or rand.** Failing and passing sweeps ran on the same Blacksmith
  host kinds (AuthenticAMD gen 2 and GenuineIntel gen 1 both passed and failed) with the
  same prebuilt C++ reference (`cpp-ref-v1-c9c5979f04ce-726643fb17a52b7d`). Merge-queue
  runs of stacked heads flip back and forth: pr-921 failed, pr-916 (921 plus a change
  to `crates/difftest/src/plan.rs` only) passed, pr-914 failed. The case calls
  `acos`/`asin` only, with identical results with
  `GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2,-FMA`. It calls no `rand` (the unit's other
  cases do, `srand(time(NULL))`, and vary at the 1e-15 level; seeding `srand` with
  1..12 changes nothing for this case).
- **Cause: heap layout.** An `LD_PRELOAD` constructor that mallocs 2N chunks of S bytes
  and frees every other one (holes in the free lists before the run) reproduces the CI
  value exactly: S = 80, N = 64 with `--filter shell_boundary_negative_exactly_two_tol`;
  in full-unit runs 8 of 100 (S, N) layouts (S 16..512, N 2..128) fail, all with this
  case only and this same value. ASLR, `glibc.malloc.perturb` (uninitialised heap) and
  single-chunk shifts of 8..4080 bytes change nothing. So the C++ result depends on the
  relative addresses of objects, which change whenever the Rust code or harness of the
  binary allocates differently before the case: that is why any PR could flip it.
- **Upstream site.** `SmRelation` stores `SmMapPtrToPtrs`, a `std::unordered_map`
  keyed by object address (`SmMapPtrToPtr.h:115`, identity `std::hash<T*>`).
  libstdc++ iterates it in bucket order, so whether two keys share a bucket (their
  address difference modulo the bucket count) changes `GetAllFirsts`/`GetAllKeys`
  order. `SmOffsetExecutive::DoSolidOffset` builds the side walls and caps from
  `m_vEUToEU.GetAllFirsts(sOffsetEdgeuses)` (`SmOffsetExecutive.cpp:1419`). At exactly
  2*tol the side walls are degenerate and their vertices are tolerance-merged by
  midpoint averaging; the order of the merges decides the final z (Rust before the
  stitch: 0.999998625, 0.999998625, 0.9999985, 0.999999; the other native order gives
  0.9999985 for the last one). The Rust `geom::tol::Relation` iterates sorted handle
  order (deterministic by design, `offset::offset_executive::p1` Deviations), which
  matches the native order of the common layout. Per-site attribution to line 1419
  (rather than another `SmRelation` walk in `DoSolidOffset`) is from reading, not
  instrumented.
- **Difftest.** The case keeps everything else (status, ownership, counts, sorted
  face/edge kinds, tolerance, input projections, purity) and sets the fixture input
  `heap_ordered_vertices: true`, which drops only the returned B-rep's vertex bounds on
  both sides (`Inputs::heap_ordered_vertices` in the runner). It is an input flag on
  that fixture case, chosen from the inputs (degenerate exactly-2tol side walls), not
  from C++ output; no tolerance changed. The 100-layout sweep shows no other case of
  the unit moving with heap layout. Recorded in `plan/upstream-issues.md`.
