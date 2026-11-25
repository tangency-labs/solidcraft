Updates: WR-547

#### WR-547 (update, 2026-10-31). The side-angle measure makes the normal unit by its own length (fork 701da4b)

- **Location:** the fork's `sm_CoonsSideAngle` (`source/SMLib/src/SmBSplineSurfaceNLib.cpp`,
  tangency-labs/smlib@cae8618), not in the pinned SMLib.
- **What happens:** the station test skips a station without a normal, measured against the
  derivatives' own size (`|Du x Dv| <= SM_EFF_ZERO_SQRT |Du| |Dv|`), then made the normal unit
  with `SmVector3d::Unitize`, whose absolute floor (1e-12) refuses a normal the relative test
  had accepted. Near the collapsed side of a patch 0.01 across `|Du x Dv|` falls to 1e-14, and
  the corner fails.
- **Trigger:** the three edges at a corner of a box of 0.01 rounded together by a
  variable-radius round (here a constant law, r 0.001 or 2e-4): main fails (`SM_ERR`); the box
  of 10 at r 1 builds.
- **Fix:** tangency-labs/smlib@701da4b divides the normal by its own length. Ported under the
  same option, `Kernel::fixes.corner_blend_coons_patch_tangent_to_sides`
  (`coons_side_angle`, `nurbs/b_spline_surface_n_lib/p2.rs`). Regression test:
  `crates/solidcore/tests/fork_701da4b_small_box_variable_corner.rs` (10 faces at both sizes;
  the r 0.001 body removes the r 1 one's volume times 1e-9, to 1%).
- **Found with it (C++ parity, unconditional):** the port's `MakeAnalyticCornerSurface` read
  `GetSolverType` from the base solver, always `SM_FS_UNKNOWN`, so a variable-radius corner
  whose curves fit a sphere was built as one where the C++ builds the Coons patch; it now asks
  the concrete solver (`CornerSolverStore::solver_type`). Test:
  `crates/solidcore/tests/parity_variable_radius_corner_not_analytic.rs`.
