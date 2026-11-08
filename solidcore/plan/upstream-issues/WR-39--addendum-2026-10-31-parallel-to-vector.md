Updates: WR-39

#### WR-39 (update, 2026-10-31). Opt-in zero-vector and chord-direction checks

`SmBSplineSurface::IsParallelToVector` (`source/SMLib/src/SmBSplineSurface.cpp:3146`-`3240`)
has two defects retained at the `v1.0.1` pin:

1. `SER(sTestVec.Unitize())` (`:3151`, before the `SmPlane` cast and `GetOrCreateGwNurbPointer`)
   returns `SM_ERR_INVALID_INPUT` (`1004`) from an `SmBoolean` function, so a zero or
   sub-`SM_EFF_ZERO_SQ` vector reads as `TRUE`.
2. Both control-net loops (`:3192` for constant-U rows and `:3227` for constant-V columns)
   call `smgu_LinePointDistance(sLinePnt, sLineEnd, sPnt, dDistance)`, whose second
   parameter is `const SmVector3d& crLineVec` (a tangent direction, not an end point).
   Passing `sLineEnd` instead of the already unitised `sLineVec` tests `i >= 2` control
   points against `sLinePnt + t * sLineEnd`, causing both false negatives on translated
   extruded surfaces and false positives on bent surfaces whose `i >= 2` control points
   happen to lie along `sLinePnt + t * sLineEnd`.

- **Rust port (#2145):** `Kernel::fixes.surface_parallel_to_vector_checks` (default
  `false` for pinned C++ parity) returns `false` when `vector.unitize()` fails (in both
  `BSplineSurface` and `Plane`) and passes the unitised chord direction `line_vec` instead
  of `line_end` to `line_point_distance` in both control-net loops.
  `BSplineSurface::is_parallel_to_vector` and `Plane::is_parallel_to_vector` keep their
  existing signatures and default behaviour; `is_parallel_to_vector_opt_in` and
  `is_parallel_to_vector_with_kernel` expose the fix, and `Surface::is_parallel_to_vector`
  forwards its `&Kernel` to every `SmBSplineSurface`-derived kind (`BSpline`, `Plane`,
  `Cone`, `SurfOfRevolution`, `SurfOfExtrusion`, `Sphere`, `Torus`).
- **Regression:** `wr39_*` unit tests in `crates/solidcore/src/nurbs/b_spline_surface/p1.rs`
  cover zero/degenerate vectors on `BSplineSurface`, `Plane` and `Surface`, translated
  quadratic (`3x3`) extruded surfaces in both constant-U and constant-V directions (plus a
  ruling with `line_end` at the origin), bent `3x3` surfaces that falsely pass the default
  `line_end` direction test in both loops, and `Surface::is_parallel_to_vector` dispatch
  across `Plane`, `BSpline` and `SurfOfExtrusion` together with
  `UpstreamFixes::flag_mut("surface_parallel_to_vector_checks")`.
