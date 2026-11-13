# SurfaceKind::SurfOfExtrusion and exact extrusion export (#76, follow-up to #313/#436, 2026-10-22)

- `SmSurfOfExtrusion::IntersectWithPlane` is declared without `virtual` but has the exact
  signature of the virtual `SmSurface::IntersectWithPlane`: it overrides it, so the
  `Surface::intersect_with_plane` dispatch runs the extrusion's special cases.
- `SmSurface::GlobalSurfaceIntersect` reaches `SmSurfaceIntersector::DoIntersection`,
  which `ReSet`s the caller's output arrays (`SmSurfaceIntersector.cpp`, start of
  `DoIntersection`): curves a special case added before asking for more are dropped.
  The extrusion port replaces them; `SurfOfRevolution::global_surface_intersect`
  appends the general curves after the special case's instead (not changed here; only
  differs when a revolution special case returns curves together with needs-more).
- Exact `SmCone`/`SmCylinder` export (adding them to `exact_type` in
  `sc_analytic_export.h`, importer via `Cone::set_out_of_bounds_enabled`) was tried and
  backed out: Mac `--green` gained ~400 passes (query.brep 994 -> 1033, edgeuse_p3
  3051 -> 3177, ...) but Linux `heal.gap` `gen0247` (box_with_large_fillet.smb, topo
  kind 1, `VertexFaceTrimCurveGap::CacheGap`) disagrees: length 8.130623401411884e-14
  (C++) vs 8.1111651133134e-14 (Rust), rel 2.4e-3, while the end points agree within
  1e-9: a last-bit difference of a point on the exact cone (the trim-curve end lifted
  through the cone's analytic evaluation), amplified by the near-zero distance. Needs
  the cone evaluation path compared bit for bit before the export can land. Also
  needed: `Surface::create_seam_curve` treats a cone as a CrvOnSurf dispatch gap (was
  an invalid-input harness failure in topology.face_p4 `cone.smb_*_op21..`; fixed here).
- Free-extrusion general intersection: running `SmAdvSurfaceIntersector` from
  `SurfOfExtrusion::global_surface_intersect` on ownerless `Surface` stand-ins passed
  before merging main 7c74a05f (its intersector parts were deferred), but afterwards
  `surface.surf_of_extrusion_p2` gen0413, gen0448, gen0732 (plane targets: bounded plane
  through a built extrusion, a parabola generator extruded by (0,0,1000), a cubic
  B-spline generator; tol -1e-6 / default) return no curves in Rust (record length 8)
  where C++ returns 230 / 734 / 1724 values. Not resolved: that path is kept a named
  `surface.surface_p2` dispatch gap; the `SmSurface` dispatch (real face surfaces) runs
  the general intersector (no B-rep unit disagreed on Linux/Mac `--green`).
