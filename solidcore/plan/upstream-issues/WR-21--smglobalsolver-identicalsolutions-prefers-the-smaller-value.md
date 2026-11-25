#### WR-21. `SmGlobalSolver::IdenticalSolutions` prefers the smaller value when maximising
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `SmGlobalSolver.cpp:1865`, `SmGlobalSolver::IdenticalSolutions`.
- **What happens:** An `SM_SO_MAXIMIZE` solve can replace an interior maximum by a nearby span-boundary point with a smaller value.
- **Trigger:** Maximum distance on a closed Hermite curve: `0.42572...` replaced by `0.42578125`.
- **How found:** generated difftest case.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
