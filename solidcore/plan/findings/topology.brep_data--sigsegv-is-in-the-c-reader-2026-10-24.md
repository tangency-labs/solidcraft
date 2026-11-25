# topology.brep_data — SIGSEGV is in the C++ reader (2026-10-24)

Three fresh full unit runs terminate with signal 11 (Python subprocess return
code `-11`). No run finishes a unit report; passed/failed/deferred/skip/rejected
totals are therefore unavailable, not zero and not copied from a stale report.
This reproduces without parallel unit scheduling. Isolated
`--unit topology.brep_data --filter gen0110 --no-report` also terminates with
SIGSEGV (shell status 139).

LLDB on that isolated case shows `EXC_BAD_ACCESS`, address `0x58`, in
`SmShellData::ReadFromDB + 500`, with the stack:
`SmShellData::ReadFromDB` <- `SmBrepData::ReadFromDB` <-
`SmBrepData::ReadFromFile` <- `sc_topology_brep_data_run` <-
`solidcore_ffi::units::topology_brep_data::run` <-
`difftest::units::topology_brep_data::cpp` <- `Runner::run_cpp`.
Thus the crashing engine is upstream C++, not the Rust kernel. A Rust-only
`--filter gen0110 --digest-dir bin/macos-divergence/rust-only --digest-debug`
run completes (`cases=1`, `needs_cpp=0`, exit 0); that is attribution evidence,
not a passing C++ differential comparison.

A debugger breakpoint at the C shim preserved the full run's last input:
2777-byte ASCII `Version 33` stream, beginning with the color component
`4.595e-321`; SHA256
`95f52f5fe9381b17a1ae67ad33ba2938747c2fc470dcc2c4495890b4b79dd3bb`.
The isolated `gen0110` shim input has the identical length and SHA256.
This matches the already documented UB-6 platform trigger (libc++ extraction
of a subnormal double sets stream failure, later readers use unset locals),
including the previously identified crashing case `gen0110`; UB-75 records
the related unchecked reader counts. The current `skipped_platform` hook
covers mode `db` only, not this `ReadFromFile` path. Class: reproducible
platform/environment-dependent upstream reader crash, not worker instability.

Reproduce the backtrace without rebuilding the shared reference:

```sh
nice -n 10 lldb -b -o run -k 'thread backtrace -c 18' -k 'process kill' -- \
  bin/difftest/difftest --unit topology.brep_data --filter gen0110 --no-report
```

What would settle a fix: exercise the file-mode/subnormal path against the
existing libc++ oracle guard policy and verify the reader's extraction/status
handling in a separately owned implementation change. No Rust crash chase was
performed. No new `upstream-issues.md` entry: UB-6 already records this exact
case and C++ crash location; duplicating it would add no new defect.
