# surface.cone_p3: requested axial domains after the midpoint fix (2026-10-27)

PR: https://github.com/tangency-labs/solidcore/pull/735.

Main `1d5602ab2` already includes #1077 and the opt-in
`Kernel::fixes.cone_parallel_line_midpoint`. Issue #711's original second-hole
Boolean succeeds with that option. The remaining limitation is documented in
`plan/upstream-issues/WR-259--addendum-2026-10-25-issue-711-overlapping-holes.md`:
testing a full line's middle neither clips it to the requested faces nor keeps
an overlap away from that middle.

Before changing the current algorithm, the five `issue711_*` tests from #735
were run against main's `surface/cone/p3.rs`, with the midpoint option enabled.
All five failed (`/tmp/pr735-main-probes.log`, reproducible by temporarily disabling
the new domain option and enabling the midpoint option in those tests):

- The reported radius-5, height-12 cylinders at `(6,0,-1)` and `(3,5.196,-1)`,
  first face limited to STEP V `[1,11]`, return two lines over world Z `[-1,11]`
  rather than `[0,10]`.
- Both faces trimmed to world Z `[1,9]` and `[3,10]` return lines over `[0,12]`
  rather than `[3,9]` (the test covers 32 UV/orientation/axis combinations).
- With an upper-only trim to Z `[0,4]`, the full line's midpoint is at Z 6 and
  no line is returned, instead of two crossing lines or one tangent line.
- Domains `[1,4]` and `[4,9]` lose their two point contacts at Z 4.
- A surface already trimmed to STEP V `[2,10]`, then requested only over `[3,9]`,
  against another requested over `[4,11]`, returns `[2,10]` instead of `[4,9]`.

The reference is now NVIDIA SMLib v1.0.1, `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`.
Its `SmCone.cpp:2796-2811` still computes the interval from whole cylinders and
tests its bottom end. The existing, unmodified native shim compares that default
behavior on the 48 `fixtures/surface.cone_p3/issue711.json` fixtures as well as the
previous fixtures and generated cases. They do not enable either Rust fix.

`Kernel::fixes.cone_parallel_line_domains`, independently opt-in and false by
default, converts both requested domains to STEP, projects the second cylinder's
trimmed endpoints into the first axis, and intersects those axial intervals before
the existing tolerance branches. It closes the gap without changing default
native parity or the midpoint option. Conversion errors propagate. No tolerance,
FFI guard, generated case or known-mismatch rule is changed.

The five analytic regression tests check line endpoints, radii and classifications
with the domain option. `issue_711_axial_domain_fix_cuts_below_the_tools_midpoint`
also uses the two actual Boolean cuts on a plate spanning Z `[0,4]`, below the
tools' midpoint at Z 5; it checks the analytic volume with the domain option alone
and combined with the midpoint option, and retains the midpoint-only failure as
a control. These corrected paths use independent geometric assertions, not native
parity against the unpatched reference.

The former unmerged WR-258 proposal in this branch describes the same defect as
the merged WR-259. Its historical blocked status and differential failures came
from enabling the fix unconditionally against `c9c5979`; that implementation and
its shared-archive entries are superseded by this opt-in version.

## Final verification (2026-10-28)

Tested source `4e7120570d7ae0a99c7f4bedeeb14387d1b8bcf4`, including main `e61b6296d0e983225e277179869d6e1ef6c59d64`.
Formatting, strict workspace Clippy with default and C++ features, wasm32, all
262 plan entries, record-ID checks and the frozen-archive guard pass. All 2,438
kernel unit tests and 103 integrations pass in the difftest profile (two unit
tests and five integrations remain ignored as before). The runner suite passes
393 unit tests and 338 integrations (36 unit tests and one integration remain
ignored). The earlier debug-profile kernel and integration run also passed before
the final main merge.

```text
PASS surface.cone_p3 cases=1490 passed=1490 failed=0 deferred=0 max_rel_err=0e0 with_values=1474 no_values=16 vacuous=0
SENSITIVE surface.cone_p3 perturbed=1490 detected=1490 unperturbed=0 deferred=0 (perturb mode: every perturbed case must FAIL)
difftest: 261 unit(s), 260 passed, 1 partial, 0 failed
difftest: compared 261 reports against the baseline: 0 unit(s) regressed
```

The full sweep enumerates 563,216 cases. Its only partial unit is the existing
`sweep.topology_sweep` (677 passes, two known mismatches, seven parked cases and
two rejected fixtures); none of its handling changes here. The exact-base
artifact is main CI run `40599566707`, artifact `12778245961`, validated with
provenance, digest and unit-set checks. All 1,442 cone baseline cases retain their
identities, outcomes and value coverage; the 48 added fixtures also pass with the
fixes off. No tolerance, absolute floor, native guard or mismatch waiver changes.
