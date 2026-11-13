# Local goldens failures: one clean-base control, cause not established

Found: 2026-11-07, while re-verifying the #2981 flag-retirement lane locally.

## What was observed

A full goldens sweep (`cargo run --profile goldens -p goldens -- --all --jobs 4 --goldens`)
on the PR head a4e81596c, based on 7efbd5090, reported 84 of 283 units failing
on aarch64 macOS. This was not a full sweep of the clean base. One inspected
failure was in a compact integer-array summary
(`[sum, weighted_sum]` per chunk):
`topology.brep_constructor`'s `hexagon_prism_separate_stitched`, where only the
topology-int weighted sum moves (`383570` -> `383763`) while its other reported
fields are unchanged. Its array length stays the same. These summaries represent
actual topology entries; a difference is not harmless merely because it is compacted.

The clean base 7efbd5090 reproduced that unit's one changed case in a scratch
worktree. This establishes a pre-existing local failure for that case only. It
does not attribute the other 83 failing units to the baseline or establish a
cross-platform cause. Missing upstream fixtures, including `Platform.smp`, were
also reported by the PR-head sweep and need separate investigation.

## CI evidence and limits

Main's CI run 42399108643 at 8d7a0bf07 completed successfully, but it was a
`push` run whose `goldens`, `clippy`, and `wasm` jobs were **skipped**. It is not
evidence of a successful goldens comparison on another platform or on 7efbd5090.
No same-revision cross-platform comparison has been performed here.

The PR's four focused units passed locally against their committed expectations.
That establishes consistency with those expectations, not geometric correctness
or a successful full sweep. Remaining failures must be compared against a matching
clean base before being attributed or dismissed; canonical exact-head CI remains
unverified while its workflow requires approval.

## Related investigation

Issue #2929 concerns a QP attempt-budget difference on aarch64 Linux/GB10. It does
not establish the cause of these unrelated local goldens failures.
