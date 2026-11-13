# intersect.topology_solver_p2 static face solves: answering `GetNaturalUVDomain` exposes 9 mismatches (open, PR #599, 2026-10-23)

Known-issue group G3, item 6, proposed giving the runner's `StaticHandle` a
`natural_uv_domain`. The handle's surfaces 2002/2003 are the shim's two `SmPlane`s,
at z=0 and z=1 over [0,1]^2. That change un-defers the 12
`face_included_without_shape_limit_*` fixture cases, which are deferred on
`SmSurface::GetNaturalUVDomain` (surface.surface_p2). Three of them move on to other
blockers. The other 9 then **fail**: entry 6 (`SmTLSCurveFace`) `_6_3_0`, `_6_4_0` and
`_6_4_1`; entry 7 `_7_4_0` and `_7_4_1`; entry 8 (`SmTLSFaceFace`) `_8_3_0`,
`_8_3_1`, `_8_4_0` and `_8_4_1`.

The values from `static-shell-vertices.json#132` (entry 6, operation 3, request 0, tol
1e-5, no swap, included) are below. Each solution is `[type, nvars, nobjs, obj0,
obj1, value, params...]`.
- C++: `3, 2, 999, -2, 0, 1, 1, 0`. Curve t=1, plane uv (1, 0).
- Rust: `3, 2, 999, -2, 0, 0.5, 0, 0`. Curve t=0.5, plane uv (0, 0).

Both are valid zero-distance points of the overlap x in [0, 1], but they are
different picks. Entry 8 with the same inputs (parallel planes, distance 1
everywhere) gives:
- C++: `4, 2, -2, -2, 1, 0.5, 0.5, 0.5, 0.5`.
- Rust: `4, 2, -2, -2, 1, 0, 0, 0, 0`.

Suspected cause: the face-owned surface cache. Upstream `pSurface1->GlobalSurfaceSolve`
(and the curve/face global solve) uses the surface's own cache, which for a
face's surface is that face's `SmTrimSrfCache`. Rust `Surface::global_surface_solve`
builds a fresh plain cache (`solve_surface_cache`), so the subdivision tree differs,
and so does the order in which equal-valued solutions arrive. This is the
known-issue G8 class, a face-aware `SurfaceSolveContext`. It is not a handle
one-liner.

What PR #599 does: it leaves the handle without `natural_uv_domain`, so these 12
cases stay deferred. It adds only `StaticScene::object_zone_tol_3d`, which turns the
2 `static_operation_matrix_0_8_*` `GetZoneTol3d` deferrals into passes.
