#### WR-788. `HasInternalPole` takes a full-break knot for a pole without comparing control points
- **Severity:** Wrong result (validation: false and missed internal poles)
- **Area:** Surfaces (B-spline validity)
- **Location:** `source/SMLib/src/SmBSplineSurface.cpp`, `SmBSplineSurface::HasInternalPole`: both knot walks' patch loop runs `rowCnt = degree - (mult - 1)` rows.
- **What happens:** at a knot of multiplicity `degree + 1` (a full break) `rowCnt` is 0. The patch loop runs zero times and the knot passes the coincidence test vacuously. The U walk then decides on `IsSingularity` alone, and the V walk, which has no singularity check, always reports a pole there. A full break has two net rows at the knot, the left piece's end row and the right piece's start row, and a row collapsed to one point there (a real pole of that piece) is never compared.
- **Trigger:** degree 2 x 2 nets with a full-break knot at 0.5 (bendavis's table on #17028). A plain grid broken in V reports a pole; a collapsed left end row is missed.
- **How found:** #17028 (filed from #16962's review); measured by bendavis on `main`.
- **Confirmed:** by the port's tests on those nets. No C++ run.
- **Rust port:** at a full break both walks test the two rows at the knot and report a pole when the `degree_other + 1` points of some span of either row coincide; that piece's iso line at the knot is then one point over the span. The V walk also takes the U walk's singularity check for its other knots. Tests: `a_full_break_on_a_plain_grid_is_not_a_pole`, `equal_rows_at_a_full_break_are_not_a_pole`, `a_collapsed_left_end_row_at_a_full_break_is_a_pole` and `a_collapsed_right_start_row_at_a_full_break_is_a_pole` in `surface/surface/assert_valid.rs`.
