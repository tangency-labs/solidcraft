# blend.fillet_solver_p5 — Linux-only native worker is unavailable on macOS (2026-10-24)

The platform mechanism is explicit, not a numerical comparison failure:
`harness/cpp/shim/sc_rollover_worker.h:52-96` implements `isolated` only
under `__linux__ && SC_ROLLOVER_WORKER_PATH`; the other branch immediately
returns `SC_ERR_EXCEPTION`. No worker is spawned on macOS. Both the native
evaluation and Rust's native fixture import can therefore fail with
`cpp_exception`; the runner correctly treats these harness failures as failures,
not matching operation-error passes. Repeating them or reducing `--jobs` cannot
enable the missing branch. This is distinct from the out-of-range conversion
skips proposed in #773 and the deform skips landed in #793 (#449, UB-251).

What would settle it: implement and separately validate a macOS fresh-process
containment path, or define an explicit unsupported-platform policy without
turning exceptions into passing comparisons. Neither change belongs to this
findings-only lane. No `upstream-issues.md` entry: the observed limitation is
in this repository's harness, not evidence of a new upstream kernel defect.

Fresh full run: 100 cases, 0 passed, 100 failed, 0 deferred, 0
`skipped(platform)`, 0 rejected, 0 known mismatches; `max_rel_err=0` because
no value comparison was reached. Samples
`review_huge_tiny_pair_100_100`, `review_huge_tiny_pair_100_101` and
`review_huge_tiny_pair_100_102` all report
`cpp: cpp_exception; rust: cpp_exception`. They use rails spanning
`[-1e300, 1e300]` and `[0, 1e-300]`, with the second interval's classification
100, 101 and 102 respectively. Their extreme scales are not evidence of a
numerical failure: the platform guard returns before executing the fixture.
