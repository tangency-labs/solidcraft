# General — macOS-arm64 divergence cluster, wave-2 follow-up (2026-10-24)

Diagnosis-only follow-up to #799, based on authoritative main `2ca17d8e9`
(after #793). The frozen sweep evidence is
`promote-zero-deferred-evidence.txt` and
`promote-zero-deferred-sweep-retry.log` under
`~/.local/share/opencode/`; the latter's SHA256 was rechecked as
`94cf2cc986a539fd97471c71e3b7a0e8f1d8f5e0f6348b13f8928e7e4d8d5923`.
Those historical outcomes are not substituted for fresh post-#793 runs below.

Environment: macOS 27.0.1 arm64, Rust 1.98.1, `CARGO_BUILD_JOBS=2`,
`RUSTC_WRAPPER` unset (sccache unavailable), and the existing shared reference
at `SOLIDCORE_CPP_DIR=/Users/altvnk/Projects/Personal/solidcore/bin/cpp`.
No reference rebuild, kernel/harness change, tolerance change or plan-status
promotion/demotion is part of this diagnosis. Commands use `nice -n 10`.

Reproduction command (one unit per process, default seeds, no case filter):

```sh
export CARGO_BUILD_JOBS=2
unset RUSTC_WRAPPER
export SOLIDCORE_CPP_DIR=/Users/altvnk/Projects/Personal/solidcore/bin/cpp
nice -n 10 rustup run 1.98.1 cargo run --profile difftest -p difftest --features cpp -- --unit <unit.id>
```

The lane's ignored `bin/macos-divergence/<unit.id>.log` and
`reports/difftest/<unit.id>.json` retain the fresh output and case inputs;
neither generated artifact is committed. A harness failure or worker signal
is not converted to an operation status, a rejection, a deferred case or a pass.

The overlapping, separately owned PR #773 proposes unit-level non-x86-64
oracle restrictions for `geom.hash_table`, `heal.heal_data_p1`/`p2`/`p3` and
`surface.twist_volume` (#448); it is not incorporated or modified here.
That architecture-dependent conversion cluster is distinct from the
allocator-dependent deform use-after-free (#449, UB-251), whose ten macOS
cases already have case-level `skipped(platform)` wiring in #793.
