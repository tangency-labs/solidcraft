#### WR-62. `SmTLSPointVertex::SolveIt` ray firing falls through into the 3D signed case
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `SmTopologySolver.cpp:206-262`, `SmTLSPointVertex::SolveIt` (def. `:116`).
- **What happens:** The `SM_SO_RAYFIRE` case has no `break`, so the `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE` case runs after it: it recomputes the ray parameter without the rayfire snap to the ray start (`dParam < 0.0 -> 0.0`), overwrites `dDistSq`, and sets the sign from the unsnapped parameter (negated again when the object order is swapped). A vertex just behind the ray start (within `SM_EFF_ZERO` of it) gets a small negative ray distance instead of `0`, and a swapped call returns negated ray distances.
- **Trigger:** `SM_SO_RAYFIRE` point/vertex solve from `(1, 0, 0)` along `(1, 0, 0)` with a vertex at `(1 - 1e-13, 0, 0)` (fixture `point_vertex_rayfire_behind_start`).
- **How found:** read during porting (`intersect.topology_solver_p1`).
- **Confirmed:** no (the C++ runs the fixture, but the Rust side defers it: the solution comparison needs vertex zone tolerances). The review's single-call fixtures `adv_rayfire_behind_start_single_*` are deferred the same way (`add_sorted_solution` reaches the `SmTol::GetZoneTol3d` stub of `topology.brep_p1` for vertex objects).
- **Severity:** wrong result.
- **Rust port:** Ported as is (the fall-through kept).
