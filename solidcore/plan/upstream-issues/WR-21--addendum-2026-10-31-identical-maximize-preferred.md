Updates: WR-21

#### WR-21 addendum (2026-10-31): opt-in larger-value preference in `SmGlobalSolver::IdenticalSolutions` when maximising

`Kernel::fixes.global_solver_identical_prefers_larger_when_maximizing` (#2155) opts into
preferring the solution with the larger `start.solution_value` in
`SmGlobalSolver::IdenticalSolutions` (`GlobalSolver::identical_solutions_in`) when the
solver operation is a plain maximisation (`SM_SO_MAXIMIZE`, `SM_SO_DIRECTED_MAXIMIZE`,
`SM_SO_PROJECTED_MAXIMIZE`). It is `false` by default, preserving pinned C++ v1.0.1
parity across all callers and difftests.

1. **Root cause (`SmGlobalSolver.cpp:1865-1942`).**
   After confirming that `crSol1` and `crSol2` match in object count, variable count,
   solution type, object pointers, solution value within `SM_EFF_ZERO_SQRT` (`1e-6`), and
   parameters within `dScaledZero` (`max(100 * SM_EFF_ZERO * (1 + |param|), 1e-4 * ivl_len)`),
   lines 1936-1938 unconditionally select
   `pPreferredSol = (crSol1.m_vStart.m_dSolutionValue < crSol2.m_vStart.m_dSolutionValue) ? &crSol1 : &crSol2`.
   While smaller `m_dSolutionValue` is tighter/better for intersections, minimisations,
   and signed residual solves (including `SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE`, which
   negates the direction vector and stores a non-negative residual), the three
   `OperationCategory::Maximize` operations (`SM_SO_MAXIMIZE`, `SM_SO_DIRECTED_MAXIMIZE`,
   `SM_SO_PROJECTED_MAXIMIZE`) store positive distances where larger is better. Near a
   smooth interior maximum `t*`, a span boundary within `1e-4` of the domain length has a
   value deficit `O((t* - t_span)^2) << SM_EFF_ZERO_SQRT`, so `AddSortedSolution` treats
   the two candidates as identical and replaces or blocks the true interior maximum in
   favour of the smaller span-boundary value.
2. **Opt-in behaviour and `Kernel::fixes` propagation.**
   With `global_solver_identical_prefers_larger_when_maximizing` enabled and
   `operation.is_plain_maximize()`, `GlobalSolver::identical_solutions_in` selects
   `PreferredSolution::First` when `sol1.start.solution_value > sol2.start.solution_value`
   and `PreferredSolution::Second` otherwise (retaining the stored solution on exact
   ties, mirroring upstream's strict inequality). All other operations keep `<`.
   `GlobalSolver::new_with_kernel`, `with_tree_and_kernel`, and `with_trees_and_kernel`
   propagate `k.fixes` into `GlobalSolver::fixes` across curve, surface, revolution, and
   topology global solves.

- **Code:** `crates/solidcore/src/kernel.rs`
  (`global_solver_identical_prefers_larger_when_maximizing`),
  `crates/solidcore/src/geom/global_solver.rs` (`GlobalSolver::identical_solutions_in`,
  `new_with_kernel`, `with_tree_and_kernel`, `with_trees_and_kernel`), plus `k.fixes`
  propagation in `curve/curve/p1.rs`, `curve/curve/p7.rs`, `surface/surface/p1.rs`,
  `surface/surf_of_revolution/p3.rs`, and `intersect/topology_solver/p1.rs`.
- **Tests:** `crates/solidcore/src/geom/global_solver.rs::tests::wr21_*`
  (`wr21_identical_solutions_default_vs_opt_in_maximize`,
  `wr21_add_sorted_solution_keeps_true_interior_maximum`,
  `wr21_minimize_and_intersect_unchanged_and_flag_lookup`).
