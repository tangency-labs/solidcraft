`SmSurface::LocalSurfaceIntersect`'s out-of-bounds boundary retries guard both
the minimum and maximum boundary tests of the second through fourth parameter
branches (`v` of `this`, `u` of `other`, `v` of `other`) with `!bFoundSolution`,
matching step 1, instead of binding `!bFoundSolution` only to the minimum test
via `!bFoundSolution && min || max` and re-running `sm_IntersectSrfBndryCrv` at
a maximum boundary where a failed solve overwrites an already-found boundary
solution (WR-99, #2210;
`plan/upstream-issues/WR-99--addendum-2026-11-01-guard-max-boundary-retry.md`).
Off by default for pinned C++ parity.
