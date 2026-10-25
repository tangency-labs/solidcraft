# surface.surf_of_revolution_p3 merge with surface.torus, surface.cone_p1, surface.surface_p1 (2026-10-14)

- **Back-edges wired.** `SmTorus::Transform` / `SmCone::Transform` finish in
  `SurfOfRevolution::transform_using`, whose polar-converter rebuild takes the derived
  class's iso-curve maker (upstream's virtual `CreateIsoParametricCurve`). This follows
  #23's `SweepIsoFn` API. `SmTorus::ReadFromDB` and `SmCone::ReadFromDB` read the parent
  record with `SurfOfRevolution::read_from_db`. `Cone::transform` now takes `&Kernel`.
- **Dispatch gap.** `Surface::read_from_db` reads `SmCone` / `SmCylinder` / `SmTorus`
  records in full but still returns `Unported` naming `surface.cone_p1` / `surface.torus`,
  because neither kind is a `SurfaceKind` variant yet. Whoever adds those variants (with
  their `SmSurface` overrides) should return the surface there.
- **`SurfaceKind::SurfOfRevolution` in surface_p1's dispatch.** `EvaluateSTEP(Point)`
  dispatches to the override. `EvaluateSimple`, `CalculateBoundingBox`,
  `IsDegeneratePoint/Curve` and `sm_CrossedSeamOrPole` use the inherited
  `SmBSplineSurface` code on the NURB. `Surface::convert_domain_*` now take `&Kernel` and
  return the conversion errors (`SER` upstream). `ConvertDomainFromNURBSToSTEP` has one
  body (`surface::surface::p1::convert_domain_from_nurbs_to_step_with`), shared with
  `SurfOfRevolution`. A reversed `u` interval is `InvalidInput`
  (`SmExtent2d::SetMinMax` returns `SM_ERR_INVALID_INPUT`), where surface_p1 had mapped
  it to `SM_ERR`.
- **Zone tolerance.** With surface_p1's bounding box, `IntersectWithSurfOfRevolution`
  without approximation tolerance no longer defers. The NURB has no owner, so no face
  tolerance applies.
