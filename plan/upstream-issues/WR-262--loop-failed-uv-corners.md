#### WR-262. `GetUVCornerBoundaries` reuses failed endpoint evaluations
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmLoop.cpp:386`, the trim endpoint evaluations and 3D point-drop fallback in `GetUVCornerBoundaries`.
- **What happens:** failed evaluations/drops leave unset or preceding-edgeuse endpoints available for boundary classification. The trim pointer acquisition status is also ignored upstream.
- **Confirmed:** no native reproduction; source and the #521 report; Rust regressions with an empty trim fail before the fix at both the first and a later edgeuse. Empty-NURB C++ calls were not executed because their reference behavior is unsafe.
- **Rust port:** trim acquisition already propagated failure; endpoint evaluation and fallback drop failures now propagate too, with fresh locals per edgeuse. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md
