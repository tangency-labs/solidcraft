# curve.curve_p1: range endpoint regression coverage (2026-10-26)

Units: `curve.curve_p1`, `curve.curve_p8`; WR-383 / #527; PR #662.

Main's `Kernel::fixes.refine_cc_range_ends` already implements the complete
correction. The oriented fraction is measured from the range start; the end's
shift must be judged relative to one. No additional production guard is required.
The default remains faithful to the pinned C++.

Five saved helper fixtures cover four combinations of curve parameter direction
and a local end solve that converges near the range start. Direct opt-in tests
check polishing of both analytic endpoints and rejection of that end-to-start
jump. The fixture comparisons use the default policy against C++.

The saved production case `curve.curve_p8/range_end_preserves_noncollapsed_tail`
(former generated case gen2160) demonstrates the practical effect. The regression
asserts two solutions by default and three with the switch enabled. The additional
solution is an approximate tail that would otherwise collapse onto its start and
be discarded. Its parameter ranges exceed 0.2 and 0.07 respectively; eleven
sampled curve-point pairs all remain within the caller's requested tolerance.

The runner uses a policy-taking internal helper for the direct regression, while
its ordinary differential entry point always uses default fixes. No known mismatch
or status downgrade is needed and no tolerance or fixture is relaxed. The unmerged
WR-260 record is replaced by a WR-383 addendum to avoid reusing an occupied ID.
