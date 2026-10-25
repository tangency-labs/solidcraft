# geom.local_solve1d — reproducible near-threshold residual (2026-10-24)

Fresh full run: 3316 cases, 3315 passed, 1 failed, 0 deferred, 0
`skipped(platform)`, 0 rejected, 0 known mismatches;
`max_rel_err=1.588890061224939e-9`. There is only one failing case to sample:
`gen1231`, seed `1894831246230431480`, `solve_it`, algorithm 3,
function kind 2 with partial derivatives, guess `0.4249261703541882`, interval
`[-1.600249728224778,0.7935376232354012]`, acceptable accuracy
`1.9498269151442054e-12`. At `values.state_f[3]` and `[10]`, C++ returns
`3.493706235424554e-8`, Rust `3.493706240975669e-8`
(absolute difference about `5.55e-17`). A second full run and a standalone
`--filter gen1231 --no-report` reproduce the same values.

Class: floating-point residual, just outside the unchanged `1e-9` relative
gate. A platform/libm or operation-rounding cause is plausible, not established
by the error magnitude. This is not evidence for an architecture-conversion
skip or a looser tolerance. What would settle it: compare the same case's
evaluation/iteration values on Linux x86-64 and macOS, locating the first
rounding difference. No new upstream issue without such attribution.
