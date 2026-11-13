# query.face_p2 (`SmFace::PointClassify`, `CurveOnClassify`, `sm_ClassifyParamOnFaces`, `SmProtoVertex` containment, 2026-10-15; partial, no disagreements)

- **Harness.** The shim builds the B-reps of unit `topology.face_p2` (upstream `.smb` files,
  `CreateFaceFromSurface` over B-spline surfaces and real `SmPlane`s) and exports them in that
  unit's layout; the runner imports them with that unit's importer. Edits overwrite stored
  face/edge/vertex tolerances (tolerance-limit cases) and build missing UV trim curves
  (`GetOrCreateUVTrimCurve`, which the port cannot do yet: unit `topology.edgeuse_p3`).
- **Trimmed surface caches.** `SmBrep::ReadFromFile` (notably with `RebuildUVTrimCurves`)
  leaves an `SmTrimSrfCache` on some face surfaces, and `PointClassify` then answers from its
  leaf nodes first. B-rep surfaces have no object cache in the port (`SmCacheMgr` has no
  counterpart), so the shim deletes the cache of every face it classifies against: both sides
  classify through the vertices and loops. The cache branch is ported (the caller passes the
  cache) but not compared; with the caches left in place, the 7 affected generated cases agreed
  on the classification anyway.
- **Inexact faces.** Faces of surfaces of revolution / extrusion, cones, spheres and tori (and
  their edge curves) are exported as their B-spline parts only; such cases report
  `inexact_geometry` on both sides and are excluded from the fixtures (`Mouse.smb` faces 0 and
  1, `box_with_large_fillet.smb` face 6, and the single faces of `cone`, `sphere`,
  `surf_extrude`, `surf_rev`, `surf_sphere150`, `torus`, `toruswvy`, `vase_rev`).
- **`SM_ERR` returns** (a loop that cannot classify the point) do not occur on these B-reps;
  the status is compared on every case.
- **Not wired**, still `UNPORTED(query.face_p2)`: `surface::trim_srf_cache`,
  `surface::surface::p4` (`GlobalPointSolve` on a face-owned surface), `intersect::ray_tracer` and `intersect::topology_solver::p1` call `PointClassify` on opaque
  face handles without a `Brep`; `topology::face::p2` and `intersect::topology_solver::p2`
  reach `CurveOnClassify` only after `UNPORTED(intersect.curve_class_p4)`.
