#### WR-260. `SmEdge::ReverseOrientation` installs an unset interval after a failed curve reversal
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmEdge.cpp:3640`, the unchecked `ReverseParameterization` call before `m_vInterval = sNewIvl` and the edgeuse loop.
- **Trigger:** a wire edge over `[0,1]` on a valid `SmHermiteCurve`, whose inherited reversal returns `SM_ERR` (1001).
- **What happens:** the interval becomes `[1e20,-1e20]`, orientations flip and UV trims are deleted despite the failed curve edit.
- **Confirmed:** yes, unchanged pinned C++ on macOS arm64: wire creation returns 1000, direct reversal returns 1001, interval `[0,1] -> [1e20,-1e20]`, primary orientation `1 -> 2`.
- **Rust port:** propagates the curve error before downstream topology edits; the existing edge test module verifies interval/uses/trims remain intact for this failure. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md
