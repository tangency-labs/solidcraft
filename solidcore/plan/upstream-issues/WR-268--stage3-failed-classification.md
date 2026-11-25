#### WR-268. `SetProps_Stage3` consumes failed loop-classification outputs
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmFaceProps.cpp`, `SetProps_Stage3`, the unchecked `ClassifyLoops` status before its classification arrays are read.
- **What happens:** failed classification output can be consumed or an empty classification can be finalized as successful properties.
- **Confirmed:** no native reproduction; source and #521; a Rust pre-fix regression on a plane with no loops demonstrates lost `ClassifyLoops` status 1001. A live C++ empty-face probe was not run.
- **Rust port:** restores the owned tree/problem type, then propagates the original classification error before reading output arrays. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md

**Opt-in (2026-10-26):** the fix above is now the opt-in `Kernel::fixes.heal_stage3_failures_propagate` (default `false`, upstream behaviour: the status is ignored as SMLib does), following the opt-in `UpstreamFixes` precedent for result-changing fixes. With the default the unit compares as SMLib with no known mismatches; the `issue_521_*` tests run with the fix on, and a default-behaviour test pins the upstream result.
