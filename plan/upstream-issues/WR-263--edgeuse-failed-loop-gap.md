#### WR-263. `GetStartPointUVLoopGap` computes a gap from failed evaluations or drops
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmEdgeuse.cpp`, `GetStartPointUVLoopGap`, both trim evaluations and the fallback 3D evaluation/drop sequence.
- **What happens:** unwritten/default endpoints, or the first drop's guess retained by the second failed drop, become a reported gap.
- **Confirmed:** no native reproduction; source and #521; Rust-only regressions exercise both trim endpoints and both 3D endpoints, with a healthy zero-gap control. Empty-NURB native calls were not executed.
- **Rust port:** propagates evaluation/drop statuses and refuses unsuccessful drops before reading their UV results. All 4402 defined native edgeuse_p1 cases still match. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md
