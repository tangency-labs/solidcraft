#### WR-165. `SmOffsetSurface::STEPInversion` retries on the extended surface with the unwidened tolerance
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmOffsetSurface.cpp:1737`, `SmOffsetSurface::STEPInversion`.
- **What happens:** The base is inverted with the tolerance widened by the offset distance, but the retry on the extended surface uses `dDistanceTolerance`, so a point on the offset over the extension is not found.
- **Trigger:** `STEPInversion` of a point on the offset over its extended part.
- **How found:** read during porting.
- **Confirmed:** no (fixture `extension_then_step_inversion` waits on `surface.surface_p4`).
- **Severity:** wrong result.
- **Rust port:** Kept.
