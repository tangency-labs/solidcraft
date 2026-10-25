#### WR-264. `sm_CalcDistAndDev` ignores a failed surface derivative evaluation
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:1065`, `sm_CalcDistAndDev` before its normal and distance calculations.
- **What happens:** a failed `Evaluate1stDerivatives` supplies no valid surface point or derivatives, but the helper continues with their defaults.
- **Confirmed:** no native reproduction; source and #521; the safe Rust empty-surface regression previously returned successful `(-1,-1)` and now preserves the derivative error. The unsafe empty native surface was not evaluated.
- **Rust port:** requires successful derivatives; normal fallback for a successfully evaluated singular point and distance-only requests keep their existing paths. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md
