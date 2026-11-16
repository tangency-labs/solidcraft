# Remaining native-guard cases are rejected, not deferred

Follows "topology.face_p1 UB-137 crash-trap cases are rejected, not deferred" above: an
input that crashes or hangs the C++ reference is rejected (AGENTS.md section 4, step
5.6), decided from the inputs or the Rust side, never from C++ output. The last three
units whose guards still deferred such inputs now reject them in `run_cpp`, so neither
side runs and nothing is compared:

- **offset.offset_geometry_creation (HANG-26, HANG-27).** `run_cpp` first follows the
  Rust script; when it stops at a non-local `FaceuseOffset` of an exact
  `SmSurfOfRevolution` face with self-intersection (HANG-26), or at an `EdgeuseOffset`
  beyond the `1e6` distance bound (HANG-27), the case is rejected. The shim's `-6` stop
  at those ops stays as a safety net (it also covers the unsupported-surface stop, where
  the Rust side still defers under `harness:analytic-geometry-export`). A script the Rust
  side cannot follow that far (an unported callee or the cached gap refill first) runs as
  before. 16 rejected (HANG-26), 0 HANG-27 (the generator's distances stay below the
  bound); was 16 deferred to `upstream:HANG-26`.
- **primitive.api_primitives (UB-119).** `run_cpp` runs the Rust `create_pyramid` and
  rejects the case when it returns `find_outer_loops`' `InvalidInput` "index out of
  range", which happens exactly where upstream's count translation reads past
  `sOutLoops` (a zero-area loop no outer loop contains). The C++ no longer runs these
  inputs (it used to run them with the uninitialised read and the Rust side deferred).
  The match is narrower than the old one (any `InvalidInput` of `topology::trimming_tools`)
  and selects the same 5 generated cases; was 5 deferred to `cpp-guard:ub-119`.
- **topology.brep_p7 (UB-165).** `run_cpp` rejects a `MakeFaceWithCurves` with
  `pbOptCrvOnSurf` TRUE, 3D curves and no UV curves past its own input checks (the
  shim's `-2` predicate, with the Rust degeneracy tests), and the Rust side no longer
  defers it. 3 rejected (`face_curves_cos_no_uv_{0,1,2}_2`, the SIGSEGV reproducers);
  was 3 deferred to `cpp-guard:ub-165`. Still wider than the trigger in principle: an
  input upstream refuses after those checks but before `MakeFace` succeeds
  (`SmBrep.cpp:16178-16511`: vertex search, `MakeShellVertex`, the edge makers) would not
  crash, but neither the shim nor the runner can tell it apart without running it. No
  current case is such an input (no generated case meets the predicate).
- **Result.** Each unit's rejected cases are exactly its former guard deferrals; no
  other outcome changed. All three units stay partial on their other deferrals.
  `topology.brep_p7`'s `cpp-guard:null-curve` and `cpp-guard:null-surface` (`-2` guards
  whose output both sides would only echo) and `topology.brep_p8`'s `cpp-guard:ub-119`
  are still Rust-side deferrals; no current case reaches them.
