# blend.fillet_solver_p5 — PR #208 maintainer review (2026-10-19)

Integrated the current #147 (`b396cdd3`) and #192 (`a88c62af`) heads and canonical
main `28cd7d9a`, preserving both sides of the findings history. The offset wrapper
is the real `surface.offset_surface::OffsetSurface` plus its base identity.

- `ProcessRollOver` calls `IsConvexRadialSector(10, FALSE, 0.0)`, matching the
  omitted default in `SmEdgeuse.h:356`, rather than stepping off by 20 edge
  tolerances. Continuation regressions assert the arguments at the callee boundary.
- Tangent rollover constructs the real offset with `SS_UNKNOWN`, as upstream's
  two-argument constructor does; `SetAllowExtension(TRUE)` follows construction.
  Offset copying calls the real `OffsetSurface::copy(k)`, preserving base identity.
  The existing output/status adapter still preserves a usable copy when an ignored
  failing status carries one. Regression covers a real copy and its options.
- `EvaluateBinormal` is wired by the current dependency heads. The target B-rep is
  mutable because the real evaluator can create UV trim curves. No p5/p1 binormal
  or offset-copy stub remains.
- `sm_SplitRail` and `sm_ComputeSupportEdge` now have one implementation each in
  `blend::fillet_solver::p1::rails`, re-exported as p1's public typed functions.
  p5 delegates through those implementations. They use `FilletBrep`, geometry ids
  and `RailOwnership` rather than unresolved opaque topology references. The
  public-entry regression reaches only the genuinely missing `MakeFilletEdge`
  (fillet_geom_p5) / `sm_SrfSrfIntersection` (fillet_geom_p2); the continuation
  regressions exercise successful splits/support curves with scripted callees.
  Split tolerance uses the real SetTolerance defaults, including vertex cascades.
- The C++ guard rejects missing rail curves only on the multi-interval path after
  a non-null fillet surface. Earlier blend, missing-classification, 1x1 and
  null-surface returns retain their defined behavior. A standalone guard regression
  distinguishes the null-surface and blend/1x1 returns from the unsafe copy path.
- Native calls now use `posix_spawn` to execute a separately built worker image.
  SMLib allocation/destruction never runs in a forked copy of the multithreaded
  Rust host. Bounded requests/results use private CLOEXEC pipes; the entire small
  request is written before spawning, avoiding parent SIGPIPE. Signals/aborts and
  stalled/failed workers remain `SC_ERR_EXCEPTION`, distinct from guard rejection.
  The worker is built with the same compiler flags and existing SMLib libraries
  by `solidcore-ffi/build.rs`; no CI workflow changes are required.
- `TestRollOver`'s four-argument constructor now calls the real `reset` with
  upstream defaults and the caller's source tolerance; `ConnectIntervals` is also
  wired to its real p2 port. No actual tangent/cliff repair parity is claimed. Added 84 committed adversarial fixtures: all class pairs at huge/tiny
  intervals, eight-interval rails across every geometry kind with zero, inverted,
  huge, tiny, narrow and offset domains, and both TestRollOver operations.

Native safety verification with the spawn worker: 1200 allocation-failure probes,
818 successes / 382 contained native failures, zero host crashes; SIGSEGV and
SIGABRT contained. Four direct ASan/LSan calls executed fixture destruction with
no reported errors/leaks (shim/test instrumented, normal reference libraries).

Review regression results: p5 170 cases / 164 pass / 4 deferred on
`topology.face_p4::SmFace::GetEdges` / 2 rejected / 0 failed, all compared values
bit-identical. The original five binormal deferrals now compare. Current dependency
runs: solver_p1 343/303 pass/40 deferred, geom_p1 480/446 pass/21 deferred/13
rejected, custom_solver 448/401 pass/47 deferred; no failures. Kernel 1420 tests
pass, including 18 rollover tests. Native guard/parser/capacity/concurrent-worker
tests: 4 pass. Fmt, explicit runner/FFI rustfmt, Clippy and wasm32 pass.

### Worker completion and late global finalizers

The fresh worker must not run SMLib global destructors after sending its reply:
that would put C++ work outside the parent's protocol-read timeout, before the
blocking reap. It now calls `std::_Exit` after `rollover_worker_main` returns.
All per-call fixture destructors have already executed inside the in-process
functions, before the reply is produced; direct ASan/LSan still exercises normal
destruction. A standalone regression arms a global destructor that aborts and
requires a successful worker response: post-response global finalization must
not run. Rechecked: 1200 allocation probes (818 success / 382 contained errors),
SIGSEGV/SIGABRT containment, the new exit regression, four direct ASan/LSan calls
and four checked FFI tests all pass. No host crash or reported leak/error.

Full local `--green --no-report` after integration (`e28e6dbf`): 214 units,
114 passed, 100 partial, 0 failed. The worker-exit follow-up changes only the native
process completion; the affected p5 native comparisons are rechecked separately.

Canonical main `5040b0b2` (including #130, topology.brep_p10) was subsequently
merged, retaining both sides of the findings history. Post-integration fmt,
plain workspace Clippy, 1426 kernel tests and wasm32 check pass. The 214-unit
full-sweep result above is scoped to the earlier integrated candidate; PR #208
records the refreshed differential sweep and exact-head CI results.

### Fresh vertex tolerances below the system zone tolerance

A final constructor audit found that the typed rail split and cliff support
creation used `Brep::add_vertex`, whose convenience default is `ZONE_TOL_3D`.
Upstream `SmFilletVertex()` inherits `SmVertex()` (`SmVertex.h:117`), which stores
tolerance `0.0`; a native probe of `new (&pseudo) SmFilletVertex()` confirmed zero.
With a rail tolerance of `1e-12`, the old Rust split kept the new vertex at `1e-5`
because `SetTolerance` only increases tolerances. The regression failed before
the fix (`1e-5` vs `1e-12`). Both fresh-vertex sites now use `add_vertex_record`
with `Vertex::default()`, preserving the zero constructor tolerance. The split
then cascades the rail's actual `1e-12`; cliff support creation is checked for
zero tolerance at its `MakeFilletEdge` call boundary. All 19 rollover tests and
1427 kernel tests pass; fmt, plain Clippy and wasm32 also pass.
This is a Rust port correction, not an upstream defect; native repair paths
remain dependency-limited as described above.
