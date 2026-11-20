#### WR-17356. `SmLSIGlobalSolver::LocalSolve` finds one of two separated roots in a leaf
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurface.cpp:18075` (`SmLSIGlobalSolver::LocalSolve`) at v1.0.1 (`3fd5b95`), as the unit's fixture cites it (`fixtures/surface.surface_p10/cases.json`, `upstream_ref`). In Rust: `crates/solidcore/src/surface/surface/p10.rs`, `LsiGlobalSolver::local_solve_branch`.
- **What happens:** A line leaf seeds one Newton guess at the middle of its interval and looks for further roots only through the near-tangent neighbour, and only when the polar box allows more. A second root well apart in the same leaf is lost.
- **Trigger:** A ray crossing a convex rational patch twice inside one cache leaf (#17356: face 34, roots at t 43.03 and 54.118).
- **How found:** #17356 probes.
- **Confirmed:** no (no C++ run). The port follows the pinned C++ (`3fd5b95`) here, and on the port the unit test `line_crossing_a_rational_patch_twice_gets_both_roots` gets one root of two on main.
- **Rust port:** fixed (deviation): after a hit, both sides of it are searched, each on its own part of the line interval, within `MAX_GUESSES`, unless the line lies in the surface at the hit (normal within 0.05° of perpendicular to the line, the test `intersect_srf_bndry_crv` makes), so a line in the surface keeps upstream's result. Goldens: `surface.surface_p10` `adv_lsi_ray_in_plane` moved (n 18 -> 93) without the in-surface test; with it, `PASS surface.surface_p10 cases=867 changed=0` (8126c00b4).
