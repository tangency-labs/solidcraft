# topology.face_p1 — UB-137 crash guard replaced by a fork trap (PR #467)

#294 row 13. The shim's UB-137 guard (above, under the unit's **C++ crash guard**) is now
a trap: the same predicate picks the seam calls that can reach upstream's `SplitAtSeam`
crash, and the shim runs each of them in a forked child (`fp1_trapped`) instead of
skipping it. The child sends the op record back through a pipe. A child that dies on a
signal, runs past its 60 s alarm or sends no whole record answers `-4`. Where the shim
cannot fork (Windows) or the pipe or the fork fails, the call is skipped (`-3`) as
before. The child marks itself non-dumpable: a piped `core_pattern` (WSL's
`wsl-capture-crash`, systemd-coredump) ignores `RLIMIT_CORE`, and capturing each
expected crash made the unit about 4x slower.

- **Runner.** The Rust side runs every case (its copy of the guard is gone). `run_cpp`
  reports a `-3`/`-4` record as `Output::deferred("cpp-crash-guard:UB-137",
  "SmFace::SplitAtSeam")`. The generic runner (`crates/difftest/src/runner.rs`) now
  defers a case whose C++ side is deferred, and that deferral wins over a Rust-side one:
  a case the C++ cannot run stays uncomparable once the Rust side's blocker lands.
- **Result** (Linux, main `1054aeee`): of the 98 guarded cases, 44 pass bit-identical,
  37 crash (SIGSEGV), 11 defer to `topology.face_p3` and 6 to `topology.face_p4`; none
  fail. The 37 are the 36 review crash reproducers (`guard_vase_rev_*`, `vase_rev.smb`
  face 0, `MoveSeam` with split and fabricated crossed-BOTH verdicts) and
  `vase_rev.smb_1_f5_op9` (the UB-137 trigger). The other 61 run without a crash.
- **Not done.** The trap still needs the guard's predicate to choose calls (forking every
  seam call would also work, at the cost of a fork each). The crash cases are not
  comparable on either platform.
