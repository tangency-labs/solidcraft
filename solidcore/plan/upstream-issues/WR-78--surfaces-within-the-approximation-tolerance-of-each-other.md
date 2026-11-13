#### WR-78. Surfaces within the approximation tolerance of each other come back as coincident regions
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface::GlobalSurfaceIntersect` (plane against a NURBS plane through the general path; the `SmSphere` override for two spheres). Cause not traced.
- **What happens:** Two planes at an angle of 1e-6 to 1e-8 rad, and two planes or spheres 1e-7 apart, are reported as coincident: four `COINCIDENT` region boundary curves along the domain edges. The crossing line of the tilted planes is missed (half of it covered by chance), and the parallel pairs, which do not intersect at all, get a spurious region. Run at `SmApproxTol3d(1e-6)`: the surfaces stay within that tolerance of each other, so this may be the intended tolerance semantics. It is listed because the exact answer differs.
- **Trigger (ssi-corpus cases):** `c7_angle_planes_1e-6`, `c7_angle_planes_1e-7`, `c7_angle_planes_1e-8` (verdict `INCOMPLETE`). `c7_offset_planes_1e-7`, `c7_offset_spheres_1e-7` (verdict `WRONG TOPOLOGY`). The same pairs at 1e-3 to 1e-5 pass.
- **How found:** ssi-corpus, exact ground truth.
- **Confirmed:** yes, by the C++ build.
- **Severity:** wrong result (near-degenerate input).
- **Rust port:** not reached yet (`intersect.surface_intersector_p5`, `surface.sphere`).
