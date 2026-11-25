#### WR-264. `sm_CalcDistAndDev` ignores a failed surface derivative evaluation
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:1065`, `sm_CalcDistAndDev` before its normal and distance calculations.
- **What happens:** a failed `Evaluate1stDerivatives` supplies no valid surface point or derivatives, but the helper continues with their defaults.
- **Confirmed:** no native reproduction; source and #521; the safe Rust empty-surface regression previously returned successful `(-1,-1)` and now preserves the derivative error. The unsafe empty native surface was not evaluated.
- **Rust port:** requires successful derivatives; normal fallback for a successfully evaluated singular point and distance-only requests keep their existing paths. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md

**Opt-in (2026-10-26):** the fix above is now the opt-in `Kernel::fixes.topology_failed_evaluations_propagate` (default `false`, upstream behaviour: the status is ignored as SMLib does), following the opt-in `UpstreamFixes` precedent for result-changing fixes. With the default the unit compares as SMLib with no known mismatches; the `issue_521_*` tests run with the fix on, and a default-behaviour test pins the upstream result.
