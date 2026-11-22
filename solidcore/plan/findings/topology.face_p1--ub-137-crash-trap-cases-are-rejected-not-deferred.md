# topology.face_p1 UB-137 crash-trap cases are rejected, not deferred

Follows the fork crash trap of PRs #467/#696 (entry "UB-137 crash guard replaced by a fork
trap" above). Difftest must not decide deferrals from C++ output, so that entry's runner
semantics are reverted:

- **face_p1.** A seam call whose forked child crashed (`-4`: signal, alarm or no whole
  record), or that the shim skipped because it could not fork (`-3`), is an input
  undefined in C++ (`plan/upstream-issues.md` UB-137; AGENTS.md section 4, step 5.6). The
  runner's `run_cpp` now returns `Output::rejected` with a reason naming UB-137 and the
  trap, the same mechanism other units' FFI guards use: the harness skips the Rust side
  and compares nothing. Seam calls the trap runs to completion compare as before.
- **Runner.** `crates/difftest/src/runner.rs` no longer lets a C++-side deferral win: a
  `deferred` output from `run_cpp` is a harness error (fails the case). The runner test
  `cpp_side_deferral_is_a_harness_error_and_crash_traps_reject` covers both. No other
  unit's `run_cpp` returned a deferral (every other `Output::deferred` is on a Rust path).
- **Result** (Linux): 2315 cases, 2276 pass, 0 failed, 0 deferred, 37 rejected (the 36
  `guard_vase_rev_*` review reproducers and `vase_rev.smb_1_f5_op9`), 2 known mismatches
  (`cone.smb_1_f5_op9`, `torus.smb_1_f5_op9`); was 37 deferred to
  `cpp-crash-guard:UB-137`. The unit stays partial because of the known mismatches.
