# intersect.ray_tracer: `surface_leaf` vs upstream's `SE` on degenerate patches (fixed)

- **Unit / code:** `intersect.ray_tracer`, `surface_leaf` (`SmSurfaceGridElement::Init`,
  `SmGrid.cpp:252-290`).
- **Inputs:** (a) a bilinear Bezier patch with all four control points at (5,5,5);
  (b) a patch with `P00 == P10` and `P01 == P11` (du == 0 everywhere). Fixtures
  `degenerate_patch_all_corners_equal` and `degenerate_patch_du_zero`.
- **C++:** `SE(Evaluate)`, `SE(Unitize)` and `SE(EvaluateNormal)` signal the error and
  keep going, so `FireRay` returns a normal record (no error).
- **Cause:** asking for (1,1) derivatives makes `SmBSplineSurface::Evaluate` take its
  "not enough derivatives" branch, `SER(Evaluate(crUV, 2, 2, ...))`
  (`SmBSplineSurfaceNLib.cpp:8335`), which returns before the fixed-up tangents are
  written back, so `sEval` keeps the plain NLib derivatives. That is what the ported
  `evaluate` gives with `non_zero_tangents: false`. (The first version of this entry
  wrongly said the match had to wait for `surface.surface_p8`.)
- **Fix:** `surface_leaf` retries a failed `evaluate` with `non_zero_tangents: false` and
  keeps the cross product when `evaluate_normal` fails; `Unported` still propagates.
  The leaf's mid values are not compared directly by difftest; only the ray record is.
- **No remaining deviation:** upstream `SmSurface::EvaluateNormal` sets `(0,0,0)` and
  returns `SM_SUCCESS` when every attempt fails (`SmSurface.cpp:11459-11462`), and the
  port's `base_evaluate_normal` does the same, so the leaf's failure arm is effectively
  unreachable (only `Unported` errors).
