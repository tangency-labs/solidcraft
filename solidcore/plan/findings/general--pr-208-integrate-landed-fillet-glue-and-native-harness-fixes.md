# PR #208 — integrate landed fillet glue and native harness fixes (2026-10-22)

The landing-batch comment #6327780964 identified conflicts after #303 landed.
The merge keeps main's shared `FilletGeomId`, `FilletBrep` geometry storage,
constructor geometry/FinishConstruction wiring, common `FilStatus`, and the
intersector FlushCurve geometry writes. The p1 rail/support entry points retain
one typed implementation in `p1/rails.rs`; the duplicate opaque helper bodies
are removed. Existing assisted-solver virtual dispatch and partial point/Jacobian
writes remain in place. Both findings sets and p1 co-port attributions are kept.

Rollover now visits the snapshot of actual `FilletGeomId`s in the solver list,
and registers the actual id returned by a topology split. No separate opaque
handle/resolver or fabricated index is retained. All evaluation sites forward
the caller's Kernel. The p1 rail constructor delegates to the landed
`FilletBrep::make_fillet_edge`, whose unavailable infinite-region constructor
still reports `topology.brep_p3`. The rollover TrimRailCurves call delegates to
the landed geom p5 implementation through a single-live-solver entry using its
actual id; indexed-table callers retain their existing entry and share both the
normal and cliff trimming bodies. A missing or different solver is not replaced
by a cloned solver, default Kernel or index zero.

The public rail helper regression exercises the real builder with an explicit
infinite region and checks endpoint backlinks, rail kind, geometry id, curve
owner and notification. The trim regression uses a bounded plane with a rail
on its v boundary and solver id 37, observes its UV curve, and rejects solver id
0. These are Rust service regressions, not new native completed-rollover parity.

The native rollover worker also compiles `shim/sc_shim.cpp`, where
`sc_native_mutex` now lives. The p5 ClassGeometry adapter reads the actual vertex
use's face gap with the caller Kernel; it returns zero only when that real use
has no gap. These harness fixes adapt bherbruck's `b87e1aaa3` / `680654aff` and the
corresponding worker change on `batch/shepherd-1`; unrelated batch changes are
not imported. Numerical formulas, operation order, tolerances and fixtures'
comparison thresholds are unchanged. Completed tangent/cliff rollover repair
remains outside the documented native fixture coverage.
