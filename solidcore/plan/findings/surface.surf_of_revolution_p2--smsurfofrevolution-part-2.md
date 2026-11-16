# surface.surf_of_revolution_p2 (`SmSurfOfRevolution`, part 2)

- `CreateOffsetSurface` merges the near-duplicate axis intersections through the solutions' curve objects (their natural intervals), so the port names the two curves with handles that carry their intervals ([`CurveHandle`]); without them 5 point solutions stay unmerged and the offset generator is not trimmed at the axis.
- `BSplineSurface::create_surf_of_revolution` with a given sweep circle ignores the status of the circle's `Transform` (it refuses `|scale| < SM_EFF_ZERO` before changing anything, so the untransformed copy is used).
- Upstream quirks recorded in `plan/upstream-issues.md`: WR-67 (`GetControlPolygon` zeroes the weights a polar conversion then reads) and WR-68 (`IsNurbSurfaceSurfOfRevolution` returns a status as `TRUE`).
- `Surface::global_surface_intersect` for a surface of revolution dispatches to `SurfOfRevolution::global_surface_intersect`, which falls back to the free `base_global_surface_intersect(this, ..)` (shared with `surface.plane_p2`, PR #40).
