# surface.surf_of_extrusion_p2: free-extrusion general intersection resolved (2026-10-23)

- **Cause of gen0413 / gen0448 / gen0732 (entry "Free-extrusion general intersection" above).**
  All three pass `tol = -1e-6`. Upstream's parameter is `const SmApproxTol3d*`; the shim
  builds it from the double and `SmTol3d(double)` stores `fabs` (`SmTol.h`, `SmTol.cpp:109`), so
  the C++ intersector runs with `+1e-6`. The Rust special cases converted the tolerance
  (`approx_tol3d`), but the general intersection got the raw `-1e-6`: `ExpandAbsolute` then
  shrinks the zero-width boundary-line boxes, `GlobalCurveIntersect` of the plane with the
  extrusion's boundary lines finds no solution, and no start point is traced. Both extrusion
  entries now pass `approx_tol3d(k, t)` to `base_global_surface_intersect_in`; the three cases
  agree and the unit is green (1373/1373). It was not an ownerless-surface problem.
- **Possible same gap elsewhere (not checked by a difftest yet).** The other `Surface`
  dispatchers (plane, cone, sphere, torus, surface of revolution) also forward the caller's
  `approx_tol_3d` to the general intersector without the `SmApproxTol3d` conversion. Only a
  negative (or `SM_USE_DEFAULT`) tolerance is affected; a generated negative-tolerance case
  in those runners would show whether they diverge.
