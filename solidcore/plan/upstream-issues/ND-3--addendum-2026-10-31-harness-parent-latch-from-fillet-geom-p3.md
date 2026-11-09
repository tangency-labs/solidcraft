Updates: ND-3

#### ND-3 (update, 2026-10-31). The harness's own C++ calls latched the static in the difftest process (`blend.fillet_geom_p3`)
- **What happened:** under `difftest --green --jobs 1` (every unit in one process), `blend.fillet_intersector` and `blend.fillet_solver_p4` failed on `values.fillet_point_step` with cpp `1e-5`, rust `1e-3`, and passed alone (#2064, reported by begsaquib). Their C++ cases run in `solidcore_ffi::isolate::fresh_process`, a `fork()` of the difftest process. A forked child inherits the parent's C++ statics, so `sdStaticStep` was already latched when the case started.
- **Poisoner:** `blend.fillet_geom_p3`. Its shim's `SmFilletEdge::CalcFilletIntFillet` / `SmFilletGeom::ReCalcFilletGeom` reach `SmFilletSolver::ComputePointValues`, and its runner called them in the difftest process itself, latching the static with `1e-5`.
- **Evidence:** a bisect over the `--green` order (probe batch 53l):
  - Each target alone: clean.
  - The 18 units up to `blend.fillet_solver_p4`: 10 `fillet_point_step` mismatches (5 per target).
  - Shortest poisoning prefix: the first 10 units, ending at `blend.fillet_geom_p3`. The 9-unit prefix is clean.
  - `blend.fillet_geom_p3` + `blend.fillet_intersector`: 5 mismatches (gen0005, gen0311, gen0314, ...: cpp `1e-5`, rust `1e-3`). `blend.fillet_geom_p3` + `blend.fillet_solver_p4`: 5 (gen0015, gen0019, gen0021, ...).
  - The Rust side of `blend.fillet_intersector` is unchanged (`--digest-dir` digests equal alone and after the poisoner). The C++ side moved.
- **Not a Rust state leak:** the Rust side runs every case on a fresh `Kernel` (the latch is a `Kernel` field, ND-3's port). CI is unaffected: `--jobs 0` runs every unit in its own process.
- **Harness fix:**
  - `blend.fillet_geom_p3`'s C++ calls now run in `isolate::fresh_process`, the source of the latch.
  - The three units whose C++ cases run in `with_fresh_static_step` (`blend.fillet_intersector`, `blend.fillet_solver_p4`, and `blend.fillet_executive_p3`'s point-step cases) check once before their cases and once after that the static is still unlatched in the parent (`check_parent_static_step`, through the runner's new `before_cases` / `after_cases` hooks). A latch before is another unit's C++ call, a latch after is the unit's own, and either is that unit's harness error, so the next poisoner fails loudly.
  - The check never latches the parent. In two fresh children, the existing probe (`sc_blend_fillet_solver_p4_static_step`, which latches 1e-5 when unlatched) and a new one that first makes a tangent-point `ComputePointValues` (`sc_blend_fillet_solver_p4_static_step_after_tangent`, which latches 1e-3 when unlatched) read `1e-5` and `1e-3` only when the parent latched nothing.
- **Confirmed:** yes, with our C++ build (the pair runs above).
