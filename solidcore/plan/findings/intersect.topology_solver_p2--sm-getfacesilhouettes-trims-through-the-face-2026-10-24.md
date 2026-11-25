# intersect.topology_solver_p2: sm_GetFaceSilhouettes trims through the face (2026-10-24)

- `face_silhouettes` (`sm_GetFaceSilhouettes`) now calls the ported
  `topology::face::p2::create_curves_by_trimming` on the face that
  `TopologyObjects::face_entity` resolves, then `AddCurvesToCache` /
  `HasCachedCurves` on the per-call surface cache as upstream. The static runner's
  adapter now holds real `CreateFaceFromSurface` faces (each in its own B-rep, as the
  shim does) and resolves `face_entity` for them. The 8 `SmFace::CreateCurvesByTrimming`
  cases (operation 14) and the 4 `SmFace::PointClassify` cases now pass; the planes seen
  along their normal have no silhouette, so the projected cases compare status and an
  empty solution list.
- **Vertex/face maximize over a face-owned plane (same class as the GetNaturalUVDomain
  entry above).** With `face_entity` answered, `face_included_without_shape_limit_5_4_0`
  and `_5_4_1` (static-shell-vertices.json #124/#125: entry 5 `SmTLSVertexFace`,
  operation 4 `SM_SO_MAXIMIZE`, request 0, tol 1e-5, vertex (-0.7, 0, 0), plane z=0
  over [0,1]^2, included) reach `GlobalPointSolve` in the non-projected branch.
  C++: success, 0 solutions. Rust: success, 1 solution, value 1.972308292331602 at
  uv (1, 1). A free (not face-owned) `SmPlane` in C++ returns the same single solution
  as Rust, so the difference is the face's `SmTrimSrfCache`: the adapter's planes are
  standalone copies, and handing it the face-owned surfaces instead makes Rust stop at
  `SmFace handle of a surface's face` (no trimmed-face binding for the adapter's
  handles). The runner defers exactly these inputs (entry 5, maximize, included) under
  that symbol until a face-aware `SurfaceSolveContext` exists for the static adapter.
