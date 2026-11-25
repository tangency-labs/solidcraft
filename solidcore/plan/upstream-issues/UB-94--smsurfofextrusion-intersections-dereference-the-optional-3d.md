#### UB-94. `SmSurfOfExtrusion` intersections dereference the optional 3D curve array
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:2506` (`IntersectWithPlane`), `:3272` (`IntersectWithCylinder`), `:3400` (`IntersectWithSurfOfExtrusion`).
- **What happens:** `pOpt3DCurves` is documented as "NULL to ignore", but the closing loops call `pOpt3DCurves->GetSize()` unconditionally (and `sm_IntersectGenerators` does the same for range solutions).
- **Trigger:** Any of the three with `pOpt3DCurves = NULL` that reaches its closing loop (e.g. `IntersectWithPlane` of a plane parallel to the extrusion).
- **How found:** read during porting (`surface.surf_of_extrusion_p2`).
- **Confirmed:** no. The shim always passes every output array.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Always produces every output.
