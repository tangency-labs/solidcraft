Updates: WR-456

#### WR-456 (update, 2026-11-05). Bigger step is the default; flag retired

`UpstreamFixes::fillet_point_bigger_step_retry` is retired (#2338). In
`c6_compute_point_values`, the retry after a failed `1e-5` forward step always
solves with `1e-3`; the bigger step is the only path. The test is now
`bigger_step_retry_uses_the_bigger_step` and uses the default kernel. It checks
that the retry's first guess lies 100 times as far from the point as the first
solve's: at the test commit it failed with ratio 1; with the fix it passes.

The full solidcore suite at `7ad798bf5` (same patch, base `7f6cf9efe`), run with
`cargo test --profile difftest -p solidcore --no-fail-fast`, reported 5075 passed,
0 failed, 44 ignored. A temporary, uncommitted reach probe in the suite logged
425 retry entries in about 50 tests; the test count is approximate because
concurrent writes interleaved. All pass.

At `e97c6c2cb` (base `8cc5c16b8`),
`cargo run --profile difftest -p difftest --features cpp -- --green --jobs 3`
with the same probe reported 280 units: 263 passed, 12 partial, 5 failed. The
retry was reached only in `blend.fillet_executive_p3` (157 entries) and
`blend.fillet_solver_p4` (61 entries), without changing those cases' outcomes
or compared values. Against main's CI reports (run `42014925714`, main
`b5569dae5`), the solver still has 651/651 passes and `max_rel_err 0`; the
executive still has 629 passed, 1 failed, 154 known mismatches, 2 parked. Its
`source-file-corner_box-edge9` failure has identical values on main.

The case-by-case comparison across all 280 units found the same outcomes and
mismatch lists except `geom.hash_table`'s `growth_after_all_removed`: it fails
on main and passes here, but the retry is not on its path and the probe never
reached it. No `cpp_defect` marker is needed for this retirement.

Rebased onto main `3ed619475` (goldens replace the difftest), at `c9ab79f98`:
`cargo run --profile goldens -p goldens -- --unit blend.fillet_solver_p4 --unit
blend.fillet_executive_p3 --goldens` passes both units with `changed=0` (651 and 788
cases), and `--all --goldens` passes 283 of 283 units. No golden is blessed: the
retirement moves no golden. The suite (`cargo test --profile goldens -p solidcore
--no-fail-fast`) reports 5084 passed, 0 failed, 44 ignored.
