# intersect.topology_solver_p1

- Upstream issues UB-85 (`MayContainAnswer` reads uninitialised `IntersectLine`
  results for a ray shorter than `SM_EFF_ZERO`), UB-86 (edge solvers dereference a free
  curve's missing edge) and WR-62 (`SmTLSPointVertex::SolveIt` ray firing falls through
  into the 3D signed case; still unconfirmed against the C++: even single-call fixtures
  are deferred because `add_sorted_solution` reaches the `SmTol::GetZoneTol3d` stub of
  `topology.brep_p1` for vertex objects). The port keeps the WR-62 fall-through as
  written. (Renumbered when other units took the numbers on `main` first: filed as
  UB-73, UB-74 and WR-48, then UB-77, UB-78 and WR-52, then UB-78, UB-79 and WR-55,
  then UB-80, UB-81 and WR-59, merged as UB-80, UB-81 and WR-61 while
  `surface.surf_of_extrusion_p1` held those numbers, then UB-85, UB-86 and WR-62.)
- The FFI guard refuses an option vector shorter than `1e-6` only where upstream is
  undefined or unverified: the ray of `MayContainAnswer` (`Op::May` and tree solves with
  `SM_SO_RAYFIRE`) and every vector the curve solvers hand to
  `SmCurve::GlobalPointSolve` / `GlobalCurveSolve`. Zero and tiny vectors for the vertex
  solvers and the other `MayContainAnswer` branches are defined (`smgu_LineClosestPoint`
  gives `0`, `smgu_PointProjectToPlane` NaN) and match bit for bit
  (`zero_vectors.json`, 156 cases).
- Wiring notes for later porters: the `curve_global_curve_solve` stub
  (`UNPORTED(curve.curve_p9)`) receives both solvers' intervals (`m_vInterval`,
  `m_vInterval2`, swapped under `m_bSwapOrder`) and the operation, tolerance, best
  answer, option vectors and `SM_SR_ALL` exactly as upstream passes them; the
  `face_point_classify` stub must return the class even when `PointClassify` fails
  (every caller ignores its status); `face_silhouettes` calls `surface_cache`
  (`surface.surface_p2`) through `TopologyObjects::surface`, and resolving the cache
  id to `SurfaceCache::has_cached_curves` is left for `surface.surface_p1`.
- Untested until their callees land: `SmTLSPointFace`, `sm_GetFaceSilhouettes` and
  every tree case reaching `SmTopologySolver::LocalSolve` (`surface.surface_p4`,
  `surface.surface_p8`, `intersect.topology_solver_p2`); the upstream tests only reach
  the curve and vertex paths.
