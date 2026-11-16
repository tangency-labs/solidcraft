Updates: plan/findings/blend.fillet_geom_p6--own-reader-face-owned-surface-cache-deferrals.md

# blend.fillet_geom_p6: face-handle context wired for the own-reader targets (2026-10-28)

Unit: `blend.fillet_geom_p6` (the stub site belongs to `topology.face_p2`, which is green).

Where the 37 cases stopped: every one runs `SmFilletEdge::CalcBlendingRail`
(`FilletBrep::calc_blending_rail`). 27 reached `SurfaceCache::face` through
`Surface::lift_curve` (its continuity cache) and 10 through `Surface::global_point_solve`
(the end-point drop of a non-mate fillet vertex). Both called `get_surface_cache` with
`NoObject` on the face's own surface, so the trimmed cache's `build_tree` asked for the
face (`SmSurface::GetFace`) and had no handle.

Fix: `calc_blending_rail` (and `calc_blending_geom` / `calc_blending_geom_of_solver`, which
call it and lift the resampled rail on the same face) take the target as `&mut Brep`. They
run the face surface's `GlobalPointSolve`, `LiftCurve` and the bilinear branch's
`DropCurve` through `BrepFaceServices` with the handle `CacheObject::Surface(face)`, as
`blend.fillet_corner_p1`'s `drop_end_rib_for_global_merge` already did. The cache is then
the face's trimmed cache, made on the B-rep as upstream's `smsurf_GetSurfaceCache` makes it
through `GetFace()`. The surface is evaluated as a copy (`clone_for_evaluation`), so the
B-rep can be borrowed mutably for the services.

Result: the runner reads its targets with the own reader. 400 cases: 400 pass, 0 failed,
0 deferred, max_rel_err=0. No values disagree.

Still unthreaded (not reached by any current case): `blend/fillet_geom/p2.rs`'s other
`surface_drop_curve` calls on face surfaces (`calc_rail_rail_interpolation_with_solvers`, upstream `CalcRailRailInterpolation`) still use
`NoObject`. If an own-reader case reaches them, they would land at the same
`SurfaceCache::face` stub, which now carries an `UNPORTED(topology.face_p2)` tag.
