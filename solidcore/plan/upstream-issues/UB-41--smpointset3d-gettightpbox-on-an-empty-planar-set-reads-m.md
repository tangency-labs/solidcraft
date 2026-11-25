#### UB-41. `SmPointSet3d::GetTightPBox` on an empty planar set reads `m_pData[0]`
- **Severity:** Crash / undefined behaviour
- **Area:** Spatial structures and small geometry
- **Location:** `source/SMLib/inc/SmPointSet.h:181` `GetTightPBox`, via `SmPointSet3d::SetUpTightPseudoBox` (`SmPointSet.cpp:631`).
- **What happens:** An empty set built with a normal is classified planar, and the planar extra work reads the first point.
- **Trigger:** An empty `SmPointSet3d` with a normal, then `GetTightPBox()`.
- **How found:** crash in the C++ reference run (segfault).
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** Skips the extra work and uses `(0,0,0)` for missing points.
