#### UB-258. RemoveExcessFaces leaves removed faces in correspondence maps
- **Severity:** Crash / undefined behaviour
- **Area:** Offsets
- **Location:** `source/SMLib/src/SmOffsetExecutive.cpp`, `RemoveExcessFaces`, immediately before `pResult->RemoveFaces(sDeleteFaces)`.
- **What happens:** stale face pointers are read after removal (reported under ASan in #368).
- **Trigger:** an excess offset face still named by face-use, edge, vertex or blend correspondence, subsequently walked by `PiecewiseMerge`.
- **How found:** ASan report in #368.
- **Confirmed:** no local C++ rerun; direct Rust relation regression, native shell trigger not rerun.
- **Rust port:** Fixed in tangency-labs/smlib 5fe723c; the port carries it. All incoming face pairs and deleted blends' outgoing pairs are forgotten before removal. See `plan/findings/offset.offset_executive_p1--remove-excess-faces-correspondence-maps-issue-368.md`.
