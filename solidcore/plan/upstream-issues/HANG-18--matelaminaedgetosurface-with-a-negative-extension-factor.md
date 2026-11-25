#### HANG-18. `MateLaminaEdgeToSurface` with a negative extension factor never returns
- **Severity:** Hang
- **Area:**
- **Location:** `NL_CrvAdv.cpp:640` `ST_curxcdr` (the G2 mirror-segment searches, `while( NL_YES )` at `:748` and `:839`), through `N_CrvExtendByDist` <- `ST_surxsdr` <- `N_SrfExtendByDist` <- `SmBSplineSurface::CreateExtendedSurface` <- `SmBrep::MateLaminaEdgeToSurface` (`SmBrep.cpp:22248`, `CreateExtendedSurface(ctx, dMaxDist*dSurfaceExtensionFactor, SM_CT_G1R, ..)`).
- **What happens:** Nothing rejects a negative distance. With `ldst < 0` the parameter step `du = (U[m]-U[0])*(ldst/d1)` is negative, so the search walks the parameter off the knot range (only one side is clamped) and its exits compare arc lengths against the negative `ldst` (`fabs(ldst - dr) < ldst * 0.05` is never true). Distinct from the long positive extensions of the offset hang (swarm registry HANG-17), which split and recurse.
- **Trigger:** `MateLaminaEdgeToSurface` with `dSurfaceExtensionFactor = -1` on a lamina edge whose points are off the mating surface: `Endcap.smb` edge 0, and edges 0 and 1 of a `10 x 10` plane face, mating plane `z = 0.5`, `dUVExtensionFactor = 10`, editing enabled. Factors `0`, `1.5` (the only upstream use) and `1e6` return normally.
- **How found:** `topology.brep_p2` pre-review guard fuzz (PR #149).
- **Confirmed:** yes (gdb backtrace inside `ST_curxcdr` <- `N_CrvExtendByDist` after 90 s; killed after 40 min in the sweep). The loop mechanism is from source inspection.
- **Severity:** hang.
- **Rust port:** `topology::brep::p2::mate_lamina_edge_to_surface` returns `InvalidInput` for a negative extension distance before `create_extended_surface`; the difftest FFI rejects a negative factor (`topology.brep_p2` runner test `mate_rejects_a_negative_extension_factor`).
