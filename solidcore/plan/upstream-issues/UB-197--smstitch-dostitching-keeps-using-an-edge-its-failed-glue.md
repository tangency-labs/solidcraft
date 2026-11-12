#### UB-197. `SmStitch::DoStitching` keeps using an edge its failed glue deleted
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmStitch.cpp:2182`-`2213` (`DoStitching`, keep-`pTestE` branch of the edge gluing).
- **What happens:** With `m_bIgnoreProblems`, a failing `GlueEdgesGeneral(pTestE, .., pE)` that nevertheless deleted `pE` sets `sVEdges[jj]` to `NULL`, but the local `pE` is not reset: it is added to `sCandidateEdges` and compared with the remaining `sVEdges[kk]` (`GetOtherVertex`, `GetCurve`, `GetInterval`), all through a stale pointer.
- **Trigger:** `m_bIgnoreProblems` (e.g. `StitchIntoShell` with `bShellIsWellFormed == FALSE`), a coincident edge pair keeping the second edge, and a glue that fails after deleting the first.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection; `GlueEdgesGeneral` is unported, so no difftest case reaches it). Not file-reachable: `m_bIgnoreProblems` defaults to FALSE (`SmStitch.h:99`) and is set only by an API caller choosing `StitchIntoShell(bShellIsWellFormed=FALSE)` (`SmOffsetExecutive` passes TRUE), so no [PSIRT] tag.
- **Severity:** crash / use after free.
- **Rust port:** Reproduces the control flow; queries on the deleted edge id return `NotFound`.
