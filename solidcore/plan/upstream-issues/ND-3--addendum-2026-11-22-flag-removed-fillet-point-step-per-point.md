Updates: ND-3

#### ND-3 (update, 2026-11-22). `fillet_point_step_per_point` is removed; the step latch is gone

`ComputePointValues` (`blend/fillet_solver/p4.rs`) always takes the step of the point it evaluates: `1e-3` at a
tangent point, `1e-5` otherwise. Nothing on the kernel remembers a step, so a fillet's result no longer depends on
the fillets the kernel ran before it. Removed: the field and its doc file, the `Kernel::fillet_point_step` latch,
`Kernel::process_fillet_point_step`, `Kernel::latch_fillet_point_step` and `Kernel::latched_fillet_point_step`, the
process-wide step cell `solidcore-ops` handed to every pooled kernel, the latch reset in SolidCraft's kernel pool
(`crates/smlib/src/core.rs`), the copy of the latch into the Boolean merge's cache kernel, the `fillet_point_step`
key of the three goldens units that recorded the latch (`blend.fillet_solver_p4`, `blend.fillet_intersector`,
`blend.fillet_executive_p3`), the flag's entries in the `issue_18166`, `issue_1945`, `issue_2020`, `issue_2260`
and `wr650` flag lists, and the flag-off halves of the tests.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** 2736 rows move (`blend.fillet_executive_p3` 793, `blend.fillet_intersector` 1292,
  `blend.fillet_solver_p4` 651), each only by losing its `fillet_point_step` key; every other value of every row is
  main's. Run on one machine, each row's output with the change equals its output without it, the key taken out,
  bit for bit, in all three units.
- **Tests:** `the_step_is_the_points_own_whatever_the_kernel_evaluated_before` (p4.rs) evaluates a crossing and a
  tangent point on one kernel in both orders and on a second kernel in the other order: every crossing's first
  finite-difference guess is the same, and every tangent point's lies a hundred times further. It replaces
  `per_point_step_ignores_the_latch_with_the_fix`. The latch tests in `kernel.rs` and `solidcore-ops`' session tests
  (`a_latched_step_reaches_every_kernel`, `calls_side_by_side_latch_one_step`) go with the latch.
