# SurfaceKind::Sphere / Torus (#76 follow-up to #313, 2026-10-21)

- `SmSphere` and `SmTorus` are `SurfaceKind` variants; `Surface::read_from_db` and the
  B-spline recogniser chain return them, and the shared graph exports
  (`sc_analytic_export.h`, from #304) carry sphere and torus face surfaces as their
  WriteToDB record, so the importers rebuild real spheres/tori.
- Still named dispatch gaps (`surface.surface_p2` unless noted): sphere/torus
  `TrimWithDomain` (the inherited `SmSurfOfRevolution` body calls the derived class's
  `ConvertUVFromNURBSToSTEP`, `MakeNurb`, `Reparameterize`; 24 `topology.face_p1` cases
  that passed on the flattened B-spline now defer here), torus `IntersectWithPlane` and the
  inherited `GlobalSurfaceIntersect` special cases with a torus `this` (virtual iso curves
  and conversions), non-planar torus `STEPInversion`, sphere/torus iso storage
  (`surface.surface_p9`) and `SmCrvOnSurf` seams (`curve.crv_on_surf`).
- `DropPointFast` of spheres and tori runs `SurfOfRevolution::drop_point_fast_using`
  with the derived class's `EvaluateSTEPPoint` / `LocalPointSolve`.
- `topology.vertexuse` +151 deferrals on `SmFace::GetTolerance` (`topology.face_p2`):
  exact sphere/torus faces now reach the face tolerance, as #304's revolution faces did.
- `plane::p2::global_surface_intersect`'s `Cone` arm (from #313) calls the cone's
  `SmSurfOfRevolution::IntersectWithPlane`, while upstream calls
  `SmCone::IntersectWithPlane` (a different, conic-section body): confirmed by
  difftest (2 failures with real cones) and fixed separately in #440.
- `SurfOfRevolution::is_nurb_surface_surf_of_revolution` returns `None` for a cone,
  sphere or torus; upstream's `IsKindOf(SmSurfOfRevolution_TYPE)` copies them.
- `topology.vertex` has its own fixture export (`support/topology_fixture.h`), which does
  not use `sc_analytic_export.h`: its 104 sphere and torus deferrals remain.
- Query face boxes (`query.face` p1/p3) of cones, spheres and tori now take the
  `SmBSplineSurface::CalculateBoundingBox` path (face-tolerance expansion), as upstream.
