#### WR-186. `SmStitch::DoStitching` never resets its candidate flags while splitting edges
- **Severity:** Wrong result
- **Area:** Numerical integration
- **Location:** `source/SMLib/src/SmStitch.cpp:1423`-`1500` (`DoStitching`, edge splitting at vertices).
- **What happens:** `bCandidateVtx` and `bCandidateEdge` start `TRUE` before the vertex loop and are set `FALSE` for the first vertex (edge) not on the candidate list, but never set back. Every later vertex and edge of the pass is treated as a non-candidate: pairs where both are later are skipped although they may be candidates, and the split vertex and edge are re-added to the candidate lists.
- **Trigger:** `DoStitching` with a candidate vertex or edge list and `m_bSplitEdgesWithVertices`.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection; every `DoStitching` case defers at `SmBrep::Notify`).
- **Severity:** wrong result.
- **Rust port:** Reproduces it.
