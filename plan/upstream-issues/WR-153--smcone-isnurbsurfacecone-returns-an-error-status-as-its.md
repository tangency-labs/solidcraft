#### WR-153. `SmCone::IsNurbSurfaceCone` returns an error status as its `SmBoolean`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmCone.cpp:4406`, `:4415`, `:4494` (`SER` around `CreateIsoParametricCurve`), `SmCone::IsNurbSurfaceCone` (def. `:4305`).
- **What happens:** `SER` returns the failing `SmStatus` (e.g. `SM_ERR` = 1001) from a function returning `SmBoolean`, so callers such as `SmBSplineSurface::CopyAndAddAnalytics` read it as `TRUE` with a `NULL` cone and return success without a surface.
- **Trigger:** An iso-curve extraction failure on a candidate cone NURB (e.g. a knot multiplicity above the degree).
- **How found:** read during porting (`surface.cone_p2`, PR #153).
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Returns the error.
