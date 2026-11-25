Updates: UB-189

#### UB-189 (update, 2026-10-26). Partly fixed in v1.0.1

Partly fixed in v1.0.1 (3fd5b95): `SmFace::ComputeLoopOrientation`'s 3D path (`bDo3DLoopComputation` TRUE, or the fallback after the UV path) returns `SM_SUCCESS` with `SM_OT_UNKNOWN` for an edge loop with no edge uses (`SmFace.cpp:1990`). The UV path (`bDo3DLoopComputation` FALSE) still calls `sEUs.GetLast()` on the empty array (`SmFace.cpp:1960`).
