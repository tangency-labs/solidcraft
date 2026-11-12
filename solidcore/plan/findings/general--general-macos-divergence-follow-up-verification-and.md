# General — macOS divergence follow-up verification and remaining gates (2026-10-24)

Fetched authoritative `upstream/main` again and merged it into
`plan/macos-divergence-findings` before final verification; the merge was
already up to date at `2ca17d8e9963483d46ab53e7e52743a6dfc4345c`.
Thus all unit runs above and the post-merge checks use the same kernel,
harness, fixtures and known-mismatch configuration. Only this findings file
is changed against upstream; `plan/upstream-issues.md` and unit statuses are
unchanged. Existing upstream defect entries are cited instead of duplicated.

The assigned standard suite was run twice, including after that final merge,
with Rust 1.98.1 and the environment recorded above. Every command exited 0:

```sh
nice -n 10 rustup run 1.98.1 cargo fmt --all --check
nice -n 10 rustup run 1.98.1 cargo clippy --workspace --all-targets -- -D warnings
nice -n 10 rustup run 1.98.1 cargo test -p solidcore
nice -n 10 rustup run 1.98.1 cargo check -p solidcore --target wasm32-unknown-unknown
```

Kernel tests: 2109 unit tests passed; integration groups passed 3, 1, 4 and 2
tests respectively (one integration test ignored); doctests had no failures.
Final logs are `bin/macos-divergence/final-{fmt,clippy,test,wasm}.log`.
The native difftest link also emits Apple's nonfatal `__eh_frame section too
large` unwind-table warning; the binary builds and executes, and that warning
does not explain any of the attributed failures.

Unresolved product findings: eight selected units have case/harness failures
(`geom.hash_table`, three healer parts, `blend.fillet_solver_p5`,
`geom.local_solve1d`, `query.face_p4`, `topology.brep_p10`),
`topology.database_io` fails the independent zero-value coverage gate, and
`topology.brep_data` crashes in C++. The three passing commands are
`surface.srf_in_volume`, `curve.curve_p6` and
`nurbs.b_spline_curve_n_lib`, with their skips explicitly separate from
passes above. A full local `--green` sweep was not added to the assigned
validation; no local all-green or platform-fix claim is made. These diagnosed
pre-existing defects are the content of the completed findings report, not
prerequisites to merge this documentation-only PR. Canonical CI run
[40125789957](https://github.com/tangency-labs/solidcore/actions/runs/40125789957)
passed all eight checks on exact report head
`33bb6841920092836a382e01265886f5c43cd361`. Readiness of the documentation
correction is gated by its own exact-head canonical CI, independently of the
unresolved product findings. No tolerance, waiver, input, plan status or
existing PR (#773) was changed to obtain green.
