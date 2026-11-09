#### WR-70. `SmAdvSurfaceIntersector::SolveBranch` never tests Gauss map separation
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmAdvSurfaceIntersector.cpp:1383` and `:1401` `SmAdvSurfaceIntersector::SolveBranch`.
- **What happens:** The method sets `m_bSeperateGaussMaps = TRUE` on entry and then calls `BranchIsGaussSeparate` only `if (!m_bSeperateGaussMaps)`, so the polar box test and the whole "separable" branch (one start point per separable sub-tree pair, the documented purpose of the class) are dead code: every intersecting leaf pair gives its own candidate.
- **Trigger:** Any `SolveIt(SM_SO_INTERSECT, ..)` on an `SmAdvSurfaceIntersector`.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.adv_surface_intersector` difftest: `solve` and `solve_branch` ops agree on one solution per intersecting leaf pair).
- **Severity:** wrong result (performance and duplicate start points).
- **Rust port:** Faithful.
