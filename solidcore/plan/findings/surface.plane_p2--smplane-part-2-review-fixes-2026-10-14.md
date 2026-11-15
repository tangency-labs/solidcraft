# surface.plane_p2 (`SmPlane`, part 2, review fixes 2026-10-14)

- **Difftest runners must convert a raw tolerance the way the shim does.** The shims read
  `IntersectWithPlane` / `GlobalSurfaceIntersect` tolerances into an `SmApproxTol3d`, whose
  `double` constructor stores `fabs(tol)` and maps `SM_USE_DEFAULT` (9876543.21) to
  `s_ZoneTol3d / 2` (`SmTol.cpp`). The library functions take an already-converted
  `SmApproxTol3d` (non-negative by type), so the runner applies
  `geom::tol::approx_tol3d(k, t)` first. Before this fix, plane z=0 against plane x=1 over
  [-5,5]x[-3,3] with tol=-1 gave the C++ crossing line (44 values) but no curve in Rust
  (7 values: `TrimPlaneLineToPlaneDomain` saw a negative tolUV); tol=9876543.21 gave 44 vs
  34 values. The generator only produced positive tolerances. The same conversion is now
  applied in the `surface.surf_of_revolution_p1` runner, whose FFI guard used to reject a
  negative plane tolerance (now accepts any finite value, as the C++ does).
- **`MakeNewPlaneEllipse` with a zero radius** appends the degenerate line curve to
  `rCrvs` even when it is `NULL`; the port drops the `None` (documented inline). No input
  in the difftest reaches it (unreachable in practice).
