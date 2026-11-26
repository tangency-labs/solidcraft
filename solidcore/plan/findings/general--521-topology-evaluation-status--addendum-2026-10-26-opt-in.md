# general: #521 failure propagation made opt-in (2026-10-26)
Updates: plan/findings/general--521-topology-evaluation-status.md

The #700 behaviour changes are result-changing fixes of upstream defects (SMLib ignores
these statuses; none is a crash, undefined behaviour or hang in the C++), so they are
now opt-in `Kernel::fixes` fields, default `false` (SMLib behaviour):

- `edge_reversal_keeps_failed_edge` (WR-260): `SmEdge::ReverseOrientation`.
- `topology_failed_evaluations_propagate` (WR-262, WR-263, WR-264):
  `GetUVCornerBoundaries`, `GetStartPointUVLoopGap`, `sm_CalcDistAndDev`.
- `heal_stage3_failures_propagate` (WR-268, WR-266): `SetProps_Stage3`'s
  `ClassifyLoops` status and its in-progress marker on failure.

With the defaults, `heal.face_props` compares as SMLib again (the eight WR-266
stage-marker cases, C++ 5 vs Rust 4, match) and its known-mismatch file is removed; the
unit returns to green. The `issue_521_*` unit tests set the fields and keep #700's
regressions; default-behaviour tests pin the upstream results. The vertex fixtures
(WR-261 scope after #704) are unchanged parity coverage.
