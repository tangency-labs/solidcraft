#### UB-121. `SmSurface::IntersectSurfaceBoundaries` dereferences an optional NULL curve array
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmSurface.cpp:21903` `SmSurface::IntersectSurfaceBoundaries` (`pOpt3DCurves->Add( pCrvCopy )` in the segment loop).
- **What happens:** `pOpt3DCurves` is declared like the other optional outputs (the header calls it `pOpt...`, the comment "required") and is reset only when non-NULL, but a kept boundary segment is added through it unconditionally: a NULL array is dereferenced.
- **Trigger:** `pOpt3DCurves == NULL` and two coincident surfaces with a boundary piece of the other surface inside this one.
- **How found:** read during porting (unit `surface.surface_p11`).
- **Confirmed:** no (the difftest shim always passes the array).
- **Severity:** crash/UB.
- **Rust port:** `Surface::intersect_surface_boundaries` always returns the curves.
