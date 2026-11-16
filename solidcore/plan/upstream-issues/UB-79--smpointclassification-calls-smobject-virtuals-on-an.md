#### UB-79. `SmPointClassification` calls `SmObject` virtuals on an `SmPoint3d*`
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `SmPointClass.cpp:668` (`ComputePointParameters`: `SmTol::GetZoneTol3d(m_pObject)`), `:435` (`GetGap3d` with a surface, through `GetObjZoneTol3d`), `:493` (`IsConnectedTo`: `GetType`), `:917` (`MergeIntoObject`: `GetType`).
- **What happens:** `SM_PC_POINT` classifications store an `SmPoint3d*` cast to `SmObject*` (`SetClassObject`), and these paths call virtual `SmObject` members on it. `SmPoint3d` has no `SmObject` vtable, so the call is undefined behaviour.
- **Trigger:** `SetClassObject(SM_PC_POINT, (SmObject*)&point)` followed by `ComputePointParameters`, `GetGap3d(surface)`, `IsConnectedTo` or `MergeIntoObject`.
- **How found:** read during porting.
- **Confirmed:** no (the harness keeps these calls away from the C++).
- **Severity:** crash/UB.
- **Rust port:** Treats a point like any other non-topology object (system zone tolerance, not connected, not mergeable).
