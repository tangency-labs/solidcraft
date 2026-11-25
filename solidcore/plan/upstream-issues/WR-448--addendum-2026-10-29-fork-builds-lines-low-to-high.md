Updates: WR-448

The fork commit is public now: tangency-labs/smlib 9853e54 ("a revolved curve is its
analytic surface, per profile curve", cherry-pick of 0b143f7's change).
Its `SmRotationalSweepGeometry::AnalyticSurface` builds a line's cylinder or cone from
the low end of the line along the axis upwards with `CreateCanonical`. It is never
inside out. The port laid out a line falling along the axis as an inside-out
cylinder or cone, v running from the curve's start. `IsNurbSurfaceCylinder` then
recognised that cylinder's NURB with its axis reversed (WR-496, tangency-labs/solidcore#1463).

With `fixes.revolve_analytic_profile_faces`, `sweep::analytic_revolution` now builds a
falling line's cylinder or cone from its low end, not inside out. It is laid out as
the B-spline surface of revolution of the reversed line, and checked against that
B-spline. `RotationalSweepGeometry::edge_sweep_higher` swaps the analytic surface in
before taking the normal, as the fork does (`AnalyticSurface` replaces
`sm_CreateRotSrf` there). The loop then comes from the analytic surface's boundary,
and the edge-orientation test handles the reversed side.
Spheres and tori keep the port's layout, which still uses `inside_out` for a meridian
that falls. `IsNurbSurfaceSphere` and `IsNurbSurfaceTorus` have no reported defect for
those.
