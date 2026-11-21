# Shell loop-gap failures keep the check that stopped them

Issue: #18802. The synthetic stress-corpus recipe S0790 makes a frustum with bottom radius
25, top radius 10 and height 50; subtracts a radius-1.2 blind cylinder at (8.79998, 0, 40),
height 10, and a radius-1 cylinder at (0, 0, 42), height 8; then shells inward by 1.2 with
no face removed. At the top the first hole leaves 10 - 8.79998 - 1.2 = 0.00002 of material.

The application regression initially failed with `SMLib failed (status 1001)`. Retaining
solidcore's statement exposed a second loss of information: `offset::offset_executive::step_gaps:
stage_failed`, without a cause. The application wrapper had discarded named offset failures,
and the loop-gap closer's shared error helper gave none of its callers' conditions.

With both diagnostics retained, S0790 reports: `the offset is refused:
offset::offset_executive::step_gaps: stage_failed; caused by
offset::offset_executive::step_gaps: the joined offset edge could not be split at the gap
bridge joint`. This names the existing `split == false` guard that stops this recipe.

The gap closer now retains each failed check as the stage failure's cause:

| Check | Cause named |
| --- | --- |
| The per-face search passes exceed `MAX_GAP_PASSES` | The loop-gap search exhausted its pass limit. |
| The two gap ends do not identify exactly one end within the existing tolerance of their shared vertex | The gap has no unique end at its shared vertex. |
| The closest-point solve returns no joint on the joined edge | The gap bridge joint could not be located on the joined offset edge. |
| Splitting the joined edge at that joint returns `split == false` | The joined offset edge could not be split at the gap bridge joint. |
| Splitting an edge at a step corner returns `split == false` | The offset edge could not be split at a step corner. |

These are the existing guards, with no changed predicate, tolerance, geometry or construction.
The outer reason remains `OffsetFailure::StageFailed`, its status remains `SM_ERR` (1001),
and its defect classification is retained. Its cause is `OffsetFailure::Failed` with the
check's statement. The existing gap-search unit test verifies those properties and that the
cause reaches the display text. The application wrapper carries the complete diagnostic,
including its entity and cause chain, while preserving the existing specialised refusals.

The S0790 regression lives in the existing application offset test file. It rejects numeric
statuses, raw upstream-status text, internal errors, generic unsupported errors and a
`stage_failed` without a cause. Other tests in that file check the transport of phrase classes,
entities and nested causes through offset and Boolean stages.
