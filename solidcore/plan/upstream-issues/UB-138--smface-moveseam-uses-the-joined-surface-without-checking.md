#### UB-138. `SmFace::MoveSeam` uses the joined surface without checking `JoinSurface`
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFace.cpp:17539-17607` `SmFace::MoveSeam`.
- **What happens:** `eStat = pRightSurface->JoinSurface(.., pJoinedSurface)` is never checked. When the join fails, `pJoinedSurface` is still `NULL` (first direction) and is dereferenced by `CopyAndAddAnalytics` for an analytic surface or handed to `SmBrep::ReplaceSurface` / `SetSurface`; in the second direction it still points to the first direction's surface, which the face already owns.
- **Trigger:** a seam move whose split halves cannot be joined within the surface's zone tolerance (for example face properties that report a seam problem in a direction the surface is not closed in). Not reproduced: `MoveSeam` without split ran crash-free on the 15 `pt_TestFiles` B-reps of the `topology.face_p1` difftest (PR #148 review).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** crash.
- **Rust port:** Returns the join's error.
