Updates: WR-39

#### WR-39 (update, 2026-11-16). `surface_parallel_to_vector_checks` is removed

`BSplineSurface::is_parallel_to_vector` and `Plane::is_parallel_to_vector` are always `false` for a vector with no direction, and they test the control net's rows against the chord direction. The fix is now unconditional and the flag is gone. SolidCraft already ran with it on. Removed: the field `UpstreamFixes::surface_parallel_to_vector_checks`, the `_opt_in` and `_with_kernel` variants, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it (#14246, group B surface-generic).

- **Results:** 28 goldens rows move.
  - 27 of them ask with the zero vector, where "parallel" has no direction to test. The answer is now `false`.
  - `b_spline_surface_p1` gen0926 asks with (0, 0, 1), and the answer is now `true`. An exact rational check shows that every u-row of the net is collinear and parallel to z.
- **Tests:** `nurbs::b_spline_surface::p1` tests `wr39_zero_vector_is_never_parallel` and `wr39_surface_dispatch` (was `..._and_flag_mut`) and the chord-direction cases run on a default kernel. The bounding-box zero-vector assertion now expects `false`.
