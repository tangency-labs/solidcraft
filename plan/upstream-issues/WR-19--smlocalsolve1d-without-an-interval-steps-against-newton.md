#### WR-19. `SmLocalSolve1d` without an interval steps against Newton
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/inc/SmLocalSolve1d.h:290` (constructor without interval); `SmLocalSolve1d.cpp:751` `SmLocalSolve1d::SolveIt`.
- **What happens:** The interval stays `SmExtent1d()` (`[1e20, -1e20]`), so the step limit `GetLength() * 0.08 + SM_EFF_ZERO` is negative and every step becomes `±1.584e19` against the Newton direction. With no interval and a failed first evaluation, `bFoundAnswer` is read uninitialised.
- **Trigger:** `SmLocalSolve1d(evaluator)` then `SolveIt`.
- **How found:** adversarial / edge-case fixtures `no_interval_*`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is (`bFoundAnswer` starts `false`).
