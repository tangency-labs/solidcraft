# blend.fillet_solver_p5: one transient `cpp_exception` on `review_huge_tiny_pair_100_100` (2026-10-27)

- **Unit:** `blend.fillet_solver_p5`. **Case:** `review_huge_tiny_pair_100_100` (`fixtures/blend.fillet_solver_p5/review.json#0`; upstream `SmFilletSolver::ProcessRollOver`, `source/SMLib/src/SmFilletSolver.cpp:7161-7295`).
- **Observed:** a local `--green` run (the build of `fix/split-at-seam-vertex-resolve`, v1.0.1 reference) got `cpp_exception` on the C++ side of this case once. A rerun of the case passed 3 times out of 3, and the whole unit passed 100 of 100. The Rust side was not involved.
- **CI:** not seen. I searched the logs of all 390 failed CI runs from 2026-10-25T13:25Z to 2026-10-27 (`gh run view --log-failed`). Every one of the 52 `blend.fillet_solver_p5` results in them is `PASS cases=100 passed=100`. The case name appears only in timing lines (`slowest_rust=`), and no `cpp_exception` appears for this unit. Successful runs pass the unit by definition.
- **Input:**
  - op 0 (`ProcessRollOver`), fillet-geometry type 0 (`SM_FG_DEFAULT`), corner type 0 (`SM_FCR_UNKNOWN`), 16 adjusted corners.
  - One classification interval per rail, both with an `SM_PC_UNKNOWN` (100) midpoint: rail 0 over [−1e300, 1e300] and rail 1 over [0, 1e-300].
- **Upstream path reached:**
  - `ProcessRollOver` takes the both-rails-single-interval branch. Neither midpoint is classified, so it reads rail 0's vertex's corner (the shim's `Corner` with type `SM_FCR_UNKNOWN`).
  - That type is neither `SM_FCR_N_x_1_CLOSED` nor `SM_FCR_1_x_1`, so it returns `SM_ERR` through `SER_MSG` at `SmFilletSolver.cpp:7220` ("Unimplemented Roll-Over Case").
  - The huge and tiny interval bounds are only stored in the classification and echoed back by the shim. No arithmetic uses them, and no tolerance test is near a boundary. The path is short and has no input-dependent branching beyond the class and corner-type checks.
- **Why it is probably not the C++ kernel:**
  - `cpp_exception` is `SC_ERR_EXCEPTION`. For this unit the shim runs every case in a freshly spawned worker process (`harness/cpp/shim/sc_rollover_worker.h`, `sc_rollover::isolated`; worker binary `solidcore-rollover-worker` in solidcore-ffi's `OUT_DIR`, about 100 MB, statically linked).
  - `isolated` returns `SC_ERR_EXCEPTION` for every failure of that machinery, without saying which:
    - `pipe2` failing (e.g. `EMFILE`);
    - `posix_spawn` failing (e.g. `EAGAIN` under process limits, or `ETXTBSY`/`ENOENT` if a concurrent cargo build in the same `bin/` target directory is rewriting the worker binary);
    - no reply within the 30 s poll timeout (a cold start of the 100 MB worker on a loaded machine, and this is the unit's first case, so its first spawn);
    - a short read;
    - a non-zero worker exit.
  - Upstream code would show up the same way only through a crash or abort in the worker.
  - The upstream path for this input is a deterministic early return. A one-off fault on a shared, heavily loaded machine (local swarm `--green` run) therefore points at the containment layer, not at an uninitialised read in SMLib. This is not proven.
- **Memory hazard check (class and location only):** partly done.
  - The UB-175 ASan `--wide` run ran this case 3 times with no reports, so out-of-bounds and use-after-free on this path are unlikely.
  - ASan does not detect reads of uninitialised memory, so that class is still open. The candidates are the fixture objects the shim builds: the `SmFilletSolver` subclass, the `SmFilletCorner` subclass, the `SmFilletEdge` rails and the `SmCurveClassification`.
- **Next steps:**
  1. Harness follow-up (separate PR): make `sc_rollover::isolated` report which stage failed, with a distinct status per stage and the stage and errno on stderr, so a repeat is attributable.
  2. Uninitialised reads: a MemorySanitizer or Valgrind memcheck run of this case, if a repeat points at the worker itself.
- **Status:** unit stays green; no known mismatch is added.
