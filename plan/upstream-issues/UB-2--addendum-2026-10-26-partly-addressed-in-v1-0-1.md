Updates: UB-2

#### UB-2 (update, 2026-10-26). Partly addressed in v1.0.1

Partly addressed in v1.0.1 (3fd5b95): SmBSplineCurve::ReadFromDB and SmBSplineSurface::ReadFromDB now reject a degree outside 1..NL_MAXDEG, n < p (and m < q), a rational flag other than 0/1, and counts whose control-point byte size would overflow ULONG. They still allocate whatever in-range counts the header declares before reading the data, and the other readers (including SmCompositeCurve::ReadFromDB) are unchanged.
