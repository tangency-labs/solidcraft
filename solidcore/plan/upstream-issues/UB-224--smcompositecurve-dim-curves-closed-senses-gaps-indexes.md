#### UB-224. `SmCompositeCurve(dim, curves, closed, senses, gaps)` indexes senses and gaps by the curve count
- **Severity:** Crash / undefined behaviour
- **Area:** Curves
- **Location:** `source/SMLib/src/SmCompositeCurve.cpp:1073` (`(*cpSenses)[i]`), `:1090` (`(*cpGaps)[m_lNumSegments-1]`), `:1097` (`(*cpGaps)[i]`); constructor def. `:1050`.
- **What happens:** The optional sense and gap arrays are read once per curve with unchecked `operator[]` (the size check is an `SM_ASSERT_BREAK_MSG`, compiled out). A shorter array is read past its end; a closed composite with no curves and a gap array reads `cpGaps[ULONG_MAX]`.
- **Trigger:** Two curves with a one-entry `cpSenses` or `cpGaps`; `bIsClosed = TRUE`, no curves, `cpGaps` non-NULL.
- **How found:** generated difftest case (fuzz sweep: mutated sense/gap arrays of a different length than the curve list; the runners now pass exactly one entry per curve).
- **Confirmed:** no. The harness keeps these inputs away from the C++ side.
- **Severity:** crash/UB.
- **Rust port:** `CompositeCurve::new` returns `InvalidInput` for fewer senses or gaps than curves and for gaps of a closed composite without curves.
