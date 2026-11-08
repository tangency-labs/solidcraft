Updates: WR-383

PR: https://github.com/tangency-labs/solidcore/pull/662

The five `curve.curve_p1` helper fixtures cover all four parameter directions and
an end solve jumping to the start. The saved `curve.curve_p8` production case
`range_end_preserves_noncollapsed_tail` returns two solutions by default and three
with `Kernel::fixes.refine_cc_range_ends`. The third solution is an approximate
range: eleven sampled point pairs remain within its requested tolerance.
No extra guard is required: measuring the end shift from one, already on main,
rejects the start jump and preserves that tail. Default differential comparisons
remain faithful to the pinned C++; the direct regression exercises both policies.
This supersedes this PR's unmerged duplicate WR-260 record.
