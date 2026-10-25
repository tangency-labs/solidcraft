# intersect.surface_silhouette (`SmSurfaceSilhouette`, PR #45 review, 2026-10-14)

No Rust-vs-C++ disagreement: 540 of 540 non-deferred difftest cases bit-identical (709
cases, 169 deferred; `SmSurface::EvaluateNormal` and `DropVectors` now call the
`surface.surface_p7` port).

- **Planes outside their extent.** `SmPlane` evaluates linearly everywhere, so
  `ComputePointValues` / `DoesPointLieOnCurve` points and tracer domains outside the
  plane's `SmExtent2d` are valid in C++. The ffi guard only bounds them to the natural
  domain for B-splines (upstream evaluates those unclamped); six `advx_plane_outside_*`
  fixtures (parallel, perspective, eye above) cover it and match exactly.
- **Plane prev-point singularity.** `adv_plane_prev_*` fixtures view along or near a
  plane axis with `use_prev`, and `adv_plane_persp_*` put the eye in the plane or exactly
  on the surface point (`Unitize` of a zero vector gives `(0,0,1)`); all match. Tiny
  (1e-6) and huge (1e5) planes, a parabolic cylinder viewed along its rulings (parabolic
  branch, "hit singularity directly" recursion) and saddle chains with angle tolerance
  None/1e-3/1.5/0 are covered too; 3 of the 31 defer on `surface.surface_p3`.
- **`UNPORTED(surface.plane_p1)` stub (pre-existing on `main`).**
  `surface/surface_cache/p1.rs` (~line 272) stubs `SmBSplineSurface::CalculateBoundingBox`
  on a plane as `UNPORTED(surface.plane_p1)`, although `surface.plane_p1` is already
  `partial`: the inherited B-spline body re-dispatches to plane overrides the port has no
  hook for yet. It causes this unit's 6 `surface.plane_p1` deferrals; whoever wires it
  should re-run this difftest.
- **`SurfaceCache::polar_box_unstored`** rebuilds the polar box from the nearest stored
  ancestor on every query instead of storing the chain as `SmBezierAux2d::GetPolarBox`
  does. Same values (checked against `surface.surface_cache_p1` and
  `intersect.surface_tracer` difftests); only repeated work differs.
