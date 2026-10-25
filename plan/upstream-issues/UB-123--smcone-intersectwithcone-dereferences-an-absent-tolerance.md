#### UB-123. `SmCone::IntersectWithCone` dereferences an absent tolerance for coincident cylinders
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmCone.cpp:2832,2860`, both `IntersectSurfaceBoundaries` calls in the `bCoincidence` branch of `SmCone::IntersectWithCone`.
- **What happens:** The routine computes a default local `dTol` but dereferences `pdOptApproxTol3d` instead when passing the tolerance to the boundary calls.
- **Trigger:** Distinct objects representing coincident, overlapping cylinders with `pdOptApproxTol3d == NULL`, after the bounding-box, angular, radial and axial early exits.
- **How found:** read during porting (`surface.cone_p3`, PR #131).
- **Confirmed:** no. Native guard tests reject this path and preserve defined default-tolerance calls (same object, disjoint heights, nested radii); an intentional undefined null dereference was not executed.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Represents optional tolerances with `Option`; coincident-boundary calls currently defer to the missing Cone/Cylinder adapter in `surface.surface_p2` (#76/#133); the boundary algorithm itself merged in #120. The FFI adapter rejects exactly the native undefined path before its boundary calls.
