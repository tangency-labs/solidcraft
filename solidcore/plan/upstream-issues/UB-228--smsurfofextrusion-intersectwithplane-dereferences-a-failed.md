#### UB-228. `SmSurfOfExtrusion::IntersectWithPlane` dereferences a failed plane projection of the generator
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:2245` (`m_pGenCurve->CreatePlaneProjection(..., m_vExtrusionVec, pProjCurve)`, status ignored) and `:2276` (`pProjCurve->CalculateBoundingBox`) in `SmSurfOfExtrusion::IntersectWithPlane` (def. `:1870`).
- **What happens:** `N_CrvProjectOntoPlane` fails for an extrusion vector whose squared length overflows, `pProjCurve` stays NULL and is dereferenced.
- **Trigger:** a canonical extrusion of a B-spline with vector `(1e300, 0, 0)`, or of a parabola with vector `(1e-9, 0, 1e300)`, intersected with a non-parallel plane (`IntersectWithPlane` directly, or through the global surface intersection).
- **How found:** crash in the C++ reference run (`difftest --fuzz 2000`, `surface.surf_of_extrusion_p2` `fuzz00363`, `fuzz01373`).
- **Confirmed:** yes.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns the projection error. The `surface.surf_of_extrusion_p2` FFI rejects plane intersections (`Plane`, `Global` with a plane) of a surface whose extrusion vector length is not finite.
