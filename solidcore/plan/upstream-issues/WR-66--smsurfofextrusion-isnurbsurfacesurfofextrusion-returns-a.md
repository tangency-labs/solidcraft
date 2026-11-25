#### WR-66. `SmSurfOfExtrusion::IsNurbSurfaceSurfOfExtrusion` returns a failing status as `TRUE`
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:3520`, `:3534`, `:3539` (`SER` in `IsNurbSurfaceSurfOfExtrusion`, def. `:3447`, which returns `SmBoolean`).
- **What happens:** `SER` returns the failing `SmStatus` (a non-zero value) from a function returning `SmBoolean`, so a failed iso curve or control polygon reports "is an extrusion" with `rpSurfOfExtrusion == NULL`.
- **Trigger:** A test surface whose iso curve extraction fails.
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Returns the error (`Err`); the shim reports the raw value.
