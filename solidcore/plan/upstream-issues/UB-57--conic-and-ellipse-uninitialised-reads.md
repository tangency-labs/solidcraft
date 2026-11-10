#### UB-57. Conic and ellipse uninitialised reads
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:**
  - `SmConic.cpp:461` `SmConic::IntersectImplicitConic`: solves a quartic of uninitialised coefficients for `SM_CT_UNKNOWN`.
  - `SmCircle.cpp` three-point constructor: ignores `smgu_CircleFrom3Points`' status, so collinear points leave the outputs unset.
  - `SmEllipse.cpp:1674` `SmEllipse::ConvertTo2D`: reads `adSols[1]` with fewer than two roots.
  - `SmEllipse.cpp:467` `SmPolarConversion::ConvertToNURBSParameter` / `ConvertToPolarParameter` / `IsCurrent`: index empty or short tables. `SmEllipse::AssertValid` (`:4093`) uses an uninitialised `dNurbT` when no table entry is written.
- **What happens:** Uninitialised reads, out-of-bounds table reads.
- **Trigger:** `SmCircle(P0, P1, P2)` with three collinear points.
- **How found:** read during porting.
- **Confirmed:** the circle failed-solve precondition is confirmed by the compiled native preflight in PR #213; the undefined constructor execution and the other subcases have not been run.
- **Severity:** crash/UB.
- **Rust port:** Returns `InvalidInput`.
- **Fuzz sweep trigger (2026-10-20):** `SmPolarConversion::ReadFromDB` (`SmEllipse.cpp:1488`) accepts an exactness table shorter than `angles - 1` (here empty, `curve.ellipse_p1` fuzz `fuzz00284`: `/build/read_db/record/exact emptied`); `ConvertToNURBSParameter` then reads `m_vExactConversion[ii-1]` past the array (`SmEllipse.cpp:555`). SIGSEGV in the C++ reference run: change **Confirmed:** to yes and add **[PSIRT]** to the title (reachable from a crafted file). Also refine the `ConvertTo2D` bullet: the quadratic is at `SmEllipse.cpp:1732`; with one root only `dThetaYRad` (`:1735`) reads the uninitialised `adSols[1]`, and it is unused; only a zero-root solve makes the used `adSols[0]` uninitialised (fuzz `fuzz01889`, `/build/read_db/record/head/11 = -1e-300`, degenerate radius).

- **PR #213 witness:** `cylindrical_full_span_preflight` in `fixtures/primitive.primitive_creation_p1/primitives.json` (length 2, inner radius 1, outer radius 3, angles 0..360, identity frame). The shim calls the same three-point-circle solve before `CreateCylindricalBox`; a non-success result rejects the fixture before `SmCircle` reads the unset radius. This is a rejected fixture, not a parity pass or a claim that the constructor fails gracefully.
