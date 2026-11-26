Updates: WR-40

#### WR-40 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): SmBSplineSurface::TrimWithDomain (SmBSplineSurfaceNLib.cpp), the V upper-boundary snapping now sets idxMax (not idxMin), and the V lower branch sets idxMin = i-1 when it snaps down to the previous knot. Verification: nurbs.b_spline_surface_n_lib_p2 TrimWithDomain cases, pending the pin101 difftest run.
