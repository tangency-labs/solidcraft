#### UB-93. `SmSurfOfExtrusion::IntersectWithCylinder` makes the iso circle of an unbounded cylinder from unset values
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfOfExtrusion.cpp:3223` `SmSurfOfExtrusion::IntersectWithCylinder` (def. `:3164`), then `:3255` `sm_IntersectGenerators(*this, dTol, *pOtherGenCurve, ...)`.
- **What happens:** The other generator is `CreateIsoCircleFromPoint` at the bottom of the cylinder's height interval, whose status is ignored. For an unbounded cylinder that point lies at `-SM_INFINITE_PARAMETER` on the axis, where the cone's STEP inversion leaves its outputs unset and `CreateIsoCircleFromPoint` reads them (UB-80); a tolerance of `0` does the same for bounded cylinders. When the circle stays `NULL` and the extrusion is parallel to the axis, `sm_IntersectGenerators` dereferences it.
- **Trigger:** `IntersectWithCylinder` with `SmCylinder::CreateCanonical(O, 0.6)`, or with a bounded `SmCone` of equal radii and `pdOptApproxTol3d = 0`.
- **How found:** generated difftest cases (`surface.surf_of_extrusion_p2`: the shim's replay of the inversion rejected the generated unbounded cylinders and zero tolerances; the generator now avoids them).
- **Confirmed:** yes (the replayed `STEPInversion` left its outputs unset).
- **Severity:** crash/UB (uninitialised read, null dereference).
- **Rust port:** A failed iso circle is `None`; the generators' intersection returns `SM_ERR_NULL_POINTER` where upstream dereferences it.
