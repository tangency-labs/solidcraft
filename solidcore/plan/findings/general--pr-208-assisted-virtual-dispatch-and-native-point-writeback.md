# PR #208 — assisted virtual dispatch and native point writeback (2026-10-21)

The five `ConstantRadiusAssistedFs` virtuals owned by the already partial
`blend.fillet_standard_solver_p1` now delegate to its inherent methods. Jacobian
offset reference arguments are copied back even when a later evaluation fails;
`IsKindOf` and `LoadInitialValues` retain their unavailable p2 calls. Three Rust
trait-object regressions check queries, oriented offsets, both rail-index mappings,
nonzero Jacobian row/column offsets and partial writes on an invalid later UV.

Enabling `SetupOffsetValues` exposed 33 previously deferred native delta cases.
The Rust difftest runner cloned both arena points but discarded their mutations,
so e.g. `gen0047` compared native point type 2 against the old Rust type 0 and
native surface position values against zero. `Delta` now writes both points back
before status propagation; `Values` also retains its caller's point mutations.
Five concrete failing inputs and a derived zero-chord case are fixtures in
`fixtures/blend.fillet_intersector/review4_virtual_dispatch.json`; the latter
compares the surface writes retained before `SM_ERR`.

The regenerated native intersector report has 1292 cases: 914 pass with values,
378 deferred, 0 failed, maximum relative error 0. All 33 former standard-solver p1
deferrals now compare. The remaining deferrals belong to solver p2/p3/p4 and the
plane `SmCrvOnSurf` dispatch boundary. This does not certify actual rollover repair
or previously unavailable solver/corner construction. No tolerance was relaxed.

The merge retains both findings append sets, existing co-port attributions and
`test_analytic_surface`'s failing-iso-status-as-TRUE behavior verified against
`SmCone.cpp`'s `SER` returns and `SmFilletSolver.cpp`'s boolean test.
