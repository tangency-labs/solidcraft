# query.face_p4: a UV line ending at v = 1e300 kept the Rust side classifying loops where the FFI refuses the C++ (nightly fuzz00209) (2026-10-31)

- **Unit:** `query.face_p4` (`ClassifyLoops` on a `MakeFaceWithCurves` face).
- **Inputs:** nightly fuzz case `query.face_p4/fuzz00209` of run 41040730716
  (2026-10-30): base `gen0117`, mutation `/brep/loops/0/0/d = 1e+300`; a cylinder face
  (`a` 1.7697804728715845, `c` 4.514774976811207) with two one-line loops, the first line
  from (0, 0.354) to (1, 1e300); face 18, props 1, loops 0, `init_gaps`. Verdict:
  `timeout`, `rust: no result after 20 s`.
- **Both sides:**
  - C++: not run. The FFI (`solidcore-ffi` `query_face_p4::eval`, `face_block`,
    `crates/solidcore-ffi/src/units/query_face_p4.rs` line 359) refuses a
    line end outside the unit face domain before calling in: `invalid argument: line: end
    outside the domain`. The runner reports it as the harness error `c++: invalid
    argument: ...`, so the fuzz driver runs the Rust side on the input on purpose.
  - Rust: the runner built the face with the 1e300 trim and ran `classify_loops`; it
    finishes, but after 48 s (replayed alone, difftest profile), almost all in the curve
    coincidence search (`Curve::global_coincidence_checker` recursing through
    `global_point_solve_cached` / `GlobalSolver::solve_branch` under
    `CurveClassification::edge_solutions`) along the 1e300-long line. The recursion is
    bounded (it returns), so this is not a library non-termination; it is an input the
    C++ reference is never given, so there is nothing to compare it against.
- **Cause:** the runner, not the port: the Rust side did not apply the FFI's input
  checks. No upstream branch, cap or NaN exit is missing from the port on this path that
  could be shown against the C++ (the C++ never runs this input).
- **Fix (harness, PR #502's convention, as #1358 and #1861):** the checks of
  `query_face_p4::eval` are split out as `validate(path, input)` (`eval` calls it; its
  behaviour is identical), and the runner's Rust side applies it to the same input vector
  `run_ffi` passes, so the case is a harness error on both sides.
- **Regression:** `crates/difftest/tests/fuzz_corpus/query.face_p4.json`
  (`fuzz00209_line_end_v_1e300`) and the runner test
  `line_end_outside_the_domain_is_refused_on_both_sides`. A gate fixture is not
  possible: a harness error always fails a case.
