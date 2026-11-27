#### WR-799. `SmLSIGlobalSolver::LocalSolve` finds one of two separated roots in a leaf
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurface.cpp:18075` (`SmLSIGlobalSolver::LocalSolve`) at v1.0.1 (`3fd5b95`), as the unit's fixture cites it (`fixtures/surface.surface_p10/cases.json`, `upstream_ref`). In Rust: `crates/solidcore/src/surface/surface/p10.rs`, `LsiGlobalSolver::local_solve_branch`.
- **What happens:** A line leaf seeds one Newton guess at the middle of its interval and looks for further roots only through the near-tangent neighbour, and only when the polar box allows more. A second root well apart in the same leaf is lost.
- **Trigger:** A ray crossing a convex rational patch twice inside one cache leaf (#17356: face 34, roots at t 43.03 and 54.118).
- **How found:** #17356 probes.
- **Confirmed:** no (no C++ run). The port's leaf solve follows the pinned C++ (`3fd5b95`) here, and before the certified pass the unit test `line_crossing_a_rational_patch_twice_gets_both_roots` got one root of two.
- **Rust port:** fixed (deviation) by the certified pass that runs after the leaf guesses (`crates/solidcore/src/surface/surface/certified_line.rs`), which adds every certified crossing no guess stands for; the leaf solve itself is unchanged.
