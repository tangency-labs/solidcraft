#### WR-20. `SmLocalSolveNd::SolveIt` reports a failed first evaluation as found
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `SmLocalSolveNd.cpp:90`, `SmLocalSolveNd::SolveIt`.
- **What happens:** If the first `Evaluate` fails, it returns `SM_SUCCESS` with `rbFoundSolution = TRUE` (the guess) and `SM_TR_UNABLE_TO_CONVERGE`. `bFoundAnswer` is read even when the evaluator never writes it, and the constructor does not set `m_lCurrentIteration`.
- **Trigger:** An evaluator that fails on the first call.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is (state initialised).
