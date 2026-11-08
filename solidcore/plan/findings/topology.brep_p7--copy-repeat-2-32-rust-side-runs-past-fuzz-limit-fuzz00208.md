# topology.brep_p7: a `copy` with `repeat` 2^32 kept the Rust side copying faces where the FFI refuses the C++ (nightly fuzz00208) (2026-10-30)

- **Unit:** `topology.brep_p7` (`SmBrep::CopyFaces` through the runner's `copy` op).
- **Inputs:** nightly fuzz case `topology.brep_p7/fuzz00208` of run 41040730716
  (2026-10-30): base `copy_plane_square.smb_1`, mutations `/merge = false`,
  `/merge = true`, `/repeat = 4294967296`; op `copy` with `n: 2`, `merge: true` on
  `plane_square.smb` (`rebuild: false`). Verdict: `timeout`, `rust: no result after 20 s`.
- **Both sides:**
  - C++: not run. The FFI (`solidcore-ffi` `topology_brep_p7::eval`) refuses the input
    before calling in: `invalid argument: bad index/count 4294967296`, the `repeat`
    read with `index(MAX_COPIES)`. The runner reports it as the harness error `c++:
    invalid argument: ...`, so the fuzz driver runs the Rust side on the input on purpose.
  - Rust: the runner's `rust_record` loops `for _ in 0..repeat` around
    `topology::brep::p7::copy_faces`, so it copied the B-rep's face 2^32 times into one
    destination B-rep (each copy a fresh notification environment) until the driver
    killed it at 20 s (and would have run out of memory long before finishing).
- **Cause:** the runner, not the port: `repeat` is a harness parameter (how many times the
  shim and the runner call `CopyFaces`), not an upstream argument. `copy_faces` itself
  copies the faces it is given once; nothing in the library loops on `repeat`.
- **Fix (harness, PR #502's convention, as #1358 for `topology.brep_p3`):** the checks of
  `topology_brep_p7::eval` are split out (`check_input`: everything but a B-rep op's block
  and arguments; `check_args`: the arguments against the B-rep's counts) and exposed as
  `validate(op, path, input, counts)`, which checks the B-rep block with the pure-Rust
  checks of `topology.vertexuse` instead of exporting it; `eval`'s behaviour is
  identical. The runner's Rust side applies `validate` to the same input vector the C++
  side gets (after the UB-165 rejection, as `run_cpp` orders it), with its own B-rep's
  counts, so the case is a harness error on both sides and the Rust side never runs it. A
  raw-knot `merge_self` builds its plane face inside `rust_record` (no B-rep at that
  point), so only its arguments' count checks are skipped there; `resolve` still limits it
  to face 0 of a plane fixture.
- **Regression:** `crates/difftest/tests/fuzz_corpus/topology.brep_p7.json`
  (`fuzz00208_copy_repeat_2_32`) and the runner tests
  `huge_copy_repeat_is_refused_on_both_sides` and `max_copy_repeat_runs_on_both_sides`. A
  gate fixture is not possible: a harness error always fails a case.
