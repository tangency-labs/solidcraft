# nurbs.b_spline_surface_n_lib_p2: TrimWithDomain v snapping fixed in v1.0.1 (2026-10-26)

Updates: plan/findings/nurbs.b_spline_surface_n_lib_p2--evaluate-trimwithdomain-createskinnedsurface.md (first bullet, "TrimWithDomain v snapping uses idxMin for both boundaries")

Fixed in v1.0.1 (3fd5b95): SmBSplineSurface::TrimWithDomain, V upper snapping now writes idxMax and the V lower branch sets idxMin = i-1 when snapping to the previous knot (see WR-40). The other bullets are unchanged.
