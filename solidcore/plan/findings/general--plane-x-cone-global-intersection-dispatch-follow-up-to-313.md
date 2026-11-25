# Plane x cone global intersection dispatch (follow-up to #313, 2026-10-21)

- `SmPlane::GlobalSurfaceIntersect` calls `rCone.IntersectWithPlane` for
  `SmCone_TYPE` / `SmCylinder_TYPE` with `rCone` an `SmCone&`: `SmCone` declares its own
  `IntersectWithPlane` (`SmCone.cpp:3219`, conic sections: lines, circles, ellipses,
  ~1000 lines), hiding `SmSurfOfRevolution::IntersectWithPlane` (generator-based). The
  port's `plane::p2::global_surface_intersect` `Cone` arm (from #313) called the
  surface of revolution's method on the cone's base.
- Shown by `fixtures/surface.plane_p2/cone.json` (real `SmCone` / `SmCylinder` from
  `CreateCanonical` through the plane GSI op): before the fix
  `global_cone_plane_through_axis` (record length 60 vs 80) and
  `global_cylinder_plane_perp` (rel err 0.92) failed and 4 cases deferred on
  `SmPlane::CreateIsoParametricCurve`; after it 9 of 10 pass bit-identically.
