# topology.face_p2 (`SmFace` part 2, 2026-10-15; partial, no disagreements)

- **Harness.** As `topology.vertexuse`, the Rust side imports the C++ B-rep export. The
  export now carries each face's raw `m_pFU`, `m_vUVDomain`, `m_bRectangularTrim`,
  `m_sZoneTol3d` and whether its surface is owned by it, real `SmPlane`s (origin, axes,
  scales, analytic domain; rebuilt with `Plane::new`) and the vertex/face gap's face UV.
  The `topology.vertexuse` shim and runner were extended the same way when its
  `SmFace::GetUVDomain` / `GetTolerance` stubs were wired (its plane faces used to be
  imported as B-splines; its deferrals on `topology.face_p2` are gone, 0 failures).
- **Inexact geometry.** Several upstream `.smb` files (`Mouse`, `box_with_large_fillet`,
  `cone`, `sphere`, `surf_extrude`, `surf_rev`, `surf_sphere150`, `torus`, `toruswvy`,
  `vase_rev`) have face surfaces that are `SmBSplineSurface` subclasses other than
  `SmPlane`, and some edges are `SmEllipse`s; the export carries their B-spline part only.
  A first run showed what that costs: `GetPointsInFace` on `sphere.smb` returned a normal
  `z` of `7.5e-17` in C++ and `3.0e-16` in Rust, while the same B-spline surface as a
  standalone `SmBSplineSurface` gives `3.0e-16` in both (the subclass evaluates
  differently). Ops that evaluate such a face surface or (`sm_FindBestUVCurve`) edge curve
  therefore report `inexact_geometry` on both sides (both runners); they compare once the
  surface subclasses can be rebuilt in Rust.
- **`sm_FindBestUVCurve` on planes** is deferred (`curve.crv_on_surf`): the ported
  `CrvOnSurf` holds an `SmBSplineSurface` only.
- **Opaque handles.** `UNPORTED(topology.face_p2)` sites remain where the caller holds a
  face only as a generic handle or an `ObjectRef` (no `Brep` to look the face up in):
  `geom::object` (extent of faces), `geom::solution_array` (`face_surface`,
  `face_uv_domain`), `geom::tol` (comments for `GetScaledZero` / `UpdateObjectTolerance`),
  `intersect::point_class` (4), `intersect::curve_class::p1::compute_face_point_class`,
  `intersect::topology_solver::p1::face_silhouettes` (`CreateCurvesByTrimming`),
  `surface::plane::p2` / `surface::surface::p1` / `surface::surface::p7` (the owning face's
  tolerance), `surface::surface_cache::p1` (the surface's face handle). They need a way
  from those handles to the B-rep before they can call `topology::face::p2`.
- **Upstream defects:** WR-86 (`GetPointsInFace` samples a rectangular face from `0`, not
  from its domain), WR-87 (`sm_CheckSeamCrossing` returns `SM_SUCCESS` as `TRUE`).
