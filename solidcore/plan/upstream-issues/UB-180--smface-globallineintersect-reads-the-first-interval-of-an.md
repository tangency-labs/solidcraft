#### UB-180. `SmFace::GlobalLineIntersect` reads the first interval of an empty UV line classification
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFace.cpp:11769`, `SmFace::GlobalLineIntersect` (def. `:11692`).
- **What happens:** For a range solution of the surface, `sm_FaceClassifyUVLine` classifies the UV line and, when it returns at most one interval, `sCC[0].m_vMid` is read without checking the size. An empty classification with `SM_SUCCESS` reads past the interval array.
- **Trigger:** A line lying in the face's surface (a range solution) whose `sm_FaceClassifyUVLine` succeeds without intervals.
- **How found:** read during porting (`topology.face_p4`).
- **Confirmed:** no.
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** Returns `InvalidInput` for an empty classification.
