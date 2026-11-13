# topology.edgeuse_p2 (SmEdgeuse part 2: sector tests, step-off binormal, UV trim curve checks, Notify)

- **Shared B-rep harness.** The B-rep construction/export helpers of the
  `topology.edgeuse_p1` shim moved to `harness/cpp/shim/sc_edgeuse_graph.h` (both shims
  include it). The export now also carries each face's `GetUVDomain()`,
  `GetTolerance()` and surface type code, the B-rep's `GetTolerance()`, and an `SmPlane`
  face surface as `1 O N` (it used to go out as its B-spline part); the
  `topology.edgeuse_p1` runner imports them (`Face::uv_domain`, `Face::tolerance`,
  `Brep::tolerance`, a `Plane` rebuilt from origin and normal, not bit-exact: only its
  kind is observed off deferred paths).
- **Inexact geometry is deferred, not compared.** The import holds an analytic
  `SmBSplineSurface` subclass (SmCone/SmCylinder 15012/13, SmSphere 15014, SmTorus,
  SmSurfOfExtrusion 15030, SmSurfOfRevolution ...) and an analytic `SmBSplineCurve`
  subclass only as their B-spline parts. `CheckAndSetUVTrimCurve` evaluates both, and on
  `cone.smb` the B-spline part of the `SmCone` differs from `SmCone::EvaluatePoint` by
  ~1e-15 (distances 6.2e-15 vs 4.0e-15). The runner therefore defers such cases (on the
  unit of the missing `SurfaceKind`, as `Surface::read_from_db` does, or on
  `topology.brep_p7` `SmBrep::ReadFromFile` where only an exact Rust B-rep reader can
  hold the geometry; relabelled to `topology.brep_p10` `MakeTopologyFromData` by PR #155). Most upstream `.smb` faces are analytic; compared
  `CheckAndSetUVTrimCurve` cases come from `Endcap.smb`, `ellips_ts2.smb`, `Mouse.smb`
  B-spline faces and `CreateFaceFromSurface` B-spline faces.
- **Deferred everywhere else.** `IsTangentSector` (non-lamina), `IsConvexRadialSector`
  (with samples), `EvaluateBinormalStepOff` and `RebuildUVTrimCurve` reach
  `SmEdgeuse::EvaluateBinormal` / `CreateUVTrimCurve` (`topology.edgeuse_p3`); the shim
  skips ("not run", `-3`) those it cannot run safely (wire edge uses, missing geometry),
  which the Rust side always defers. `SM_NO_REG_PROPAGATION` is not sent to an edge use
  (the region-attribute handler would dereference the caller as a region).
