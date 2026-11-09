#### HANG-9. `CreateExtendedSurface` with distance 0 never stops halving
- **Severity:** Hang
- **Area:**
- **Location:** `SmBSplineSurfaceNLib.cpp:4469` `SmBSplineSurface::CreateExtendedSurface`; halving tests `if (dExtDist < dDist/100.0)` at `:4574`, `:4636`, `:4703`, `:4764`. It also runs for `SmPlane`.
- **What happens:** After each failed extension the distance is halved until it drops below `dDist / 100`, which never happens for `dDist == 0` (or NaN).
- **Trigger:** `CreateExtendedSurface` with a distance of `0`. Also an infinite distance (`inf / 2` never drops below `inf / 100`): `SmBrep::MateLaminaEdgeToSurface` (`SmBrep.cpp:22248`) extends by `dMaxDist * dSurfaceExtensionFactor`, and `dMaxDist` (the largest `GlobalPointSolve` distance of 21 edge points, `:22216-22229`) is infinite once a point's squared distance to the mating surface overflows (a mating plane offset `1.4e154` from a `10 x 10` plane face or `surf_extrude.smb`; `1e154` returns), or the product overflows (`1e154 * 1e300`).
- **How found:** read during porting; the infinite case by `topology.brep_p2`'s `difftest --fuzz 2000` (PR #149: three timeouts, mating plane origin `z = +-1e300`).
- **Confirmed:** the infinite case yes, by bisecting the offset (C++ returns at `1e154` in 0.9 s and never at `1e160` .. `1e300`, killed after 30 s; no gdb on the pod). The Rust port on the same inputs, with `SmFace::Notify` stubbed out to reach the extension, stops in `create_extended_surface_dir` with `NotConverged` after 1100 halvings from `1.4e154` on, which places the loop here.
- **Severity:** hang.
- **Rust port:** Stops after 1100 halvings with `NotConverged`. The `topology.brep_p2` FFI refuses a `Mate` whose probed `dMaxDist * factor` is not finite.
