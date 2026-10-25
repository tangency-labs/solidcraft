#### WR-43. `SmCurveBoundedSurface::ReadFromDB` reads UV boundaries with the 3D type
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmCurveBoundedSurface.cpp:709`, `SmCurveBoundedSurface::ReadFromDB`.
- **What happens:** The UV boundaries are read with the type of the last 3D boundary (`l3DType`, `0` when there is none, so every UV boundary then fails with `SM_ERR`). The implicit-outer flag that was read is dropped.
- **Trigger:** A record with UV boundaries but no 3D boundaries.
- **How found:** generated difftest case (`WriteToDB`-layout ASCII records).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
