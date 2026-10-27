Updates: WR-38

#### WR-38 (update, 2026-10-26). Fixed in v1.0.1

Fixed in v1.0.1 (3fd5b95): SmBSplineSurface data constructor (SmBSplineSurface.cpp:87), the v-knot smos_MemCpy now passes the v knot count (s+1) as the destination size, so the v knots are copied for every s (no longer left 0 when s > r). The port's BSplineSurface::from_data copies them unconditionally. Verification: nurbs.b_spline_surface_p1 data-constructor cases (gen0022 etc., 125 cases that mismatched on values.this_v against C++ 1.0.1), pending the pin101 rerun.
