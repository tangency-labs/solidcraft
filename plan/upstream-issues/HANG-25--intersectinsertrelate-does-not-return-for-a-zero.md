#### HANG-25. `IntersectInsertRelate` does not return for a zero approximation tolerance
- **Severity:** Hang
- **Area:**
- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:1092-1270`, `SmTopologyIntersector::IntersectInsertRelate` (the full topology intersection at `m_dThisApproxTol3d`).
- **What happens:** With `m_dThisApproxTol3d` equal to `+0.0` or `-0.0`, the topology intersection did not finish within 180 s. Negative tolerances and tiny positive ones (down to the smallest subnormal) return within a second.
- **Trigger:** `SetThisApproxTol3d(0)`, then `IntersectInsertRelate` on two intersecting B-reps.
- **How found:** pre-review of PR #203 (the FFI's guard bypassed, timed C++ runs).
- **Confirmed:** yes (C++ run: no result after 180 s at `0` and `-0`; `-1e-5`, `-1e300`, `1e-12`, `1e-200` and `5e-324` return).
- **Severity:** hang (needs a bad argument from the calling code).
- **Rust port:** The callee loops are unported (`IntersectTopology` defers at `SmBrep::GetFaces`). The FFI refuses `IntersectInsertRelate` at a zero tolerance.
