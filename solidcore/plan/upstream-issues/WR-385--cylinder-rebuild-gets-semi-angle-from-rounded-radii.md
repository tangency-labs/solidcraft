#### WR-385. `SmCone::RebuildSTEPFromNURBParameters` gives an `SmCylinder` a semi-angle from rounded radii
- **Severity:** Wrong result (a cylinder with a non-zero semi-angle, which `SmCylinder::AssertValid` refuses: "Test #0 Zero SemiAngle")
- **Area:** Surfaces (cones and cylinders)
- **Location:** `source/SMLib/src/SmCone.cpp`, `SmCone::RebuildSTEPFromNURBParameters`: the top and bottom radii read back from the NURB are made equal only within `SM_EFF_ZERO * (1 + r)`; otherwise `m_dSemiAngleDeg = atan2(radius difference, height)`.
- **What happens:** a cylinder's NURB edited by an exact-in-theory operation (e.g. `SmFace::SwapUV` → `SmBSplineSurface::SetCanonical`) reads back radii a few 1e-11 apart; the rebuild turns that rounding into a semi-angle.
- **Trigger:** tangency-labs/solidcore#713: a ring r10/r9 drawn as 256 arcs per circle, extruded 3 (`SmApiCreateLinearSweep`): radii 9.0000000000985 and 9.0000000000757 after the swap, and the level-2 check reports 26 failures.
- **How found:** tangency-labs/solidcore#713 (traced with lldb on the fork).
- **Confirmed:** on the fork's C++ build (tangency-labs/smlib@f06b48a).
- **Fix:** for an `SmCylinder`, radii within `SM_EFF_ZERO_SQRT * (1 + r)` are its one radius (their mean); a NURB modified further than that is refused (`SM_ERR`) as no longer a cylinder.
- **Rust port:** opt-in `Kernel::fixes.cylinder_rebuild_mean_radius` (default `false`, upstream's rule) in `Cone::rebuild_step_from_nurb_parameters`, `crates/solidcore/src/surface/cone/p2.rs` (only for `SmCylinder`s, `Cone::cylinder`). Test `issue_713_cylinder_rebuild_keeps_zero_semi_angle` (a radius-9 cylinder with one end scaled by 1 + 2.5e-12). With the default, `surface.cone_p2` and the units reaching the rebuild compare as before. Fixes tangency-labs/solidcore#713.
