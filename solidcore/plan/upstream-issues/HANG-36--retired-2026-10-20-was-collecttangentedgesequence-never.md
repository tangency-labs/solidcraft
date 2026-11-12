#### HANG-36. RETIRED 2026-10-20 (was: `CollectTangentEdgeSequence` never returns on a tangent cycle without its first solver)
- **Severity:** Hang
- **Area:**
- **Reason:** the trigger can't occur. The four `GetTangentFilletSolver` overrides (`SmFilletCorner.cpp:4448`, `:4573`, `:5141`, `:6869`) pair a corner's two solvers symmetrically, or return the solver itself for a closed edge, so every walk ends at NULL or back at `pFS`. A foreign corner makes `GetOtherFilletCorner` (`SmFilletSolver.cpp:235`) return NULL, but the walk only ever passes a corner the solver owns, so that NULL is not reached either. The port keeps its walk bound as a defence against adapter input.
