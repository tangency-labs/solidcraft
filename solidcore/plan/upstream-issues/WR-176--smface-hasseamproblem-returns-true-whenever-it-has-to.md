#### WR-176. `SmFace::HasSeamProblem` returns TRUE whenever it has to compute the face properties
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmFace.cpp:15650`, `SmFace::HasSeamProblem` (def. `:15540`).
- **What happens:** When `pOptFaceProps` is NULL or below `SM_PROPSTAGE_2a`, the function runs `SetProps_Stage2`, copies its seam classifications and enums to the outputs, and returns `SM_SUCCESS` (`1000`) from a function returning `SmBoolean`. The result is always TRUE ("seam problem"), even for a face whose seams are fine and whose output enums say `SM_SP_NEITHER`.
- **Trigger:** `HasSeamProblem` without face properties on a face of a periodic surface (e.g. faces of `TestFiles/pt_TestFiles/Mouse.smb`).
- **How found:** generated difftest case (`topology.face_p4`: a port returning FALSE there disagreed with the C++).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Returns `has_problem = true` there, as upstream.
