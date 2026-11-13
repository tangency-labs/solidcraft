# geom.global_solver: the tiny-scale torus booleans were never a fair "C++ builds, Rust doesn't" comparison (2026-10-26)

**Unit:** `geom.global_solver`: the port's solve work bound `MAX_SOLVE_BRANCHES` (not upstream; module Deviations, "Work bound"). It is seen through `boolean.api_brep`.

**Inputs:** the box (5e-5, -5e-5, -1e-4) of size 1e-4 x 1e-4 x 2e-4, combined with a torus at the origin of major radius 1e-4 and minor 3e-5, as intersection (operation 2) and difference (operation 3). These were `torus_guard_tiny_scale_box_{intersection,difference}` in `fixtures/boolean.api_brep/torus_guards.json`.

**What it looked like:** the C++ finished both booleans. The port refused them with its own `InvalidInput`, "solve visits more than MAX_SOLVE_BRANCHES branches", at the old bound `2^20`. That read as a legitimate input failing on a guard of ours.

**Measured:** a measurement build raised the bound to `2^28` and logged each solve's visits. Each boolean runs four large global solves:
- 1,415,622, 3,106,603, 3,327,629 and 3,408,795 branches; the largest is about `2^21.7`.
- Both booleans then finish `Ok` in the Rust kernel.
- The regression `max_solve_branches_tiny_torus_booleans_finish` (`crates/solidcore/tests/max_solve_branches_tiny_torus.rs`, `#[ignore]`) takes about 178 s in release.

`MAX_SOLVE_BRANCHES` is now `2^23` (8,388,608): 2.46 times the largest count, while a runaway solve stays at about a minute at ~8 µs a branch.

**Why the comparison was never fair:**
- The C++ only "finished" because it ran on past UB-175. In that defect, `~SmProtoTopologyManager` frees a proto edge that a proto vertex still lists, and `~SmProtoVertex` then reads it (`plan/upstream-issues/UB-175--smprototopologymanager-frees-proto-edges-before-the-proto.md`; tiny Booleans reach that teardown through the public API, `UB-175--tiny-booleans-reach-the-teardown-through-the-public-api.md`).
- Before the raise, the port stopped at its bound before reaching that teardown. So the runner's UB-175 pre-check (`crates/difftest/src/units/boolean_api_brep.rs`: `cpp()` calls `ub_175()`, which runs the Rust call first and rejects the case when `k.upstream_ub_passed` names UB-175) did not fire. The C++ side was run and appeared to succeed.
- With `2^23` the port gets far enough to pass UB-175, so the pre-check now rejects both cases on the C++ side.
- The `DIFFTEST_OPT_IN_FIXTURES` run shows that: `FAIL boolean.api_brep cases=2 passed=0 failed=0 deferred=0 ... rejected=2 (no comparable cases)`, `TIME boolean.api_brep n=2 cases=0.030498 cpp=225.371746 rust=0.000000`.
- The 225.4 s in the cpp column is mostly the pre-check's own Rust run of the two booleans, not the C++.

**Consequence:**
- The two cases can never be compared with the C++. They are dropped from the fixtures: they are no longer in `torus_guards.json`, and the opt-in file that briefly held them is deleted.
- The only guard is the Rust-only `#[ignore]` regression above. It asserts only that both booleans finish at the default bound, not their geometry.
- A correct reference for these results would need an independent check, such as a volume or area estimate, since the C++ result passed through undefined behaviour.

**PR:** the `MAX_SOLVE_BRANCHES` raise (`crates/solidcore/src/geom/global_solver.rs`; `crates/difftest/src/units/curve_curve_p9.rs`, `unculled_solves_stop_at_the_branch_bound` now expecting 139 solutions; this finding).
