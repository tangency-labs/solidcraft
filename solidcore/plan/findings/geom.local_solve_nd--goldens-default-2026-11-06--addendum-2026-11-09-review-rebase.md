# geom.local_solve_nd: review controls after the SolidCraft rebase (2026-11-09)

Updates: `geom.local_solve_nd--goldens-default-2026-11-06.md`.

PR: tangency-labs/solidcraft#14102; issue: tangency-labs/solidcraft#13764.
The published migration head was `94ae5e1806a59af4322ec50fd81af232161a363f`.
The branch was rebased onto main `b0fba036bdc4d34335ed72b2b32d617f838094c4`.
The measured production head after that rebase is
`b1153884028b8168d43af6411c1fb9bc5c1cacca`.
The review follow-up adds tests, evidence and a known-failure classification; it changes no
production code or comparison tolerance beyond the rebased migration itself.

## Fresh controls

A freshly rebuilt golden-runner library replays all 18 previously reviewed inputs and the
three additional cases raised by CodeRabbit. All 21 inputs, integer outputs and raw floating-point
bits match their historical corrected captures. All 14,298 observed floating-point values are finite.
Every previously archived error, deferred, rejection, topology and diagnostic field also matches.
The old `curve.curve_p3/gen0607` capture omitted those metadata fields; its complete fresh
capture now records them, without claiming historical equality for absent data.
The [per-case comparison](geom.local_solve_nd--goldens-default-2026-11-06--addendum-2026-11-09-review-rebase/raw-replay-comparison.json) distinguishes those checks.

This is a fresh **rebased-after** replay. It does not claim a newly executed clean-main
`b0fba...` before control. The original before controls remain the explicitly identified
`f9ed9a90b453517d83e787169887c180e17656fd` measurements. Historical records and their
before pictures retain that provenance.

The 13 geometric review regressions also pass on the newly compiled test binary.
The three additional findings retain fresh raw captures, independently recomputed checks and
matching views:

- [Curve-on-surface cache](curve.crv_on_surf--gen0344-cache-geometry-2026-11-09.md):
  the unsuccessful drop is unchanged; exact polynomial identities confine cache changes outside
  the query. Its query-interval bound remains below `5e-6`. The inherited whole-cache limitation
  is not presented as a certificate at that default tolerance.
- [Projected curve-pair minimum](curve.curve_p3--gen0607-projected-boundary-2026-11-09.md):
  the caller still accepts a nonminimum. The fresh accepted pair has a feasible descent direction,
  proved by the exact derivative certificate. Its incorrect golden is removed and the case remains
  executed as known wrong under [#14238](https://github.com/tangency-labs/solidcraft/issues/14238).
- [Extrusion intersection](surface.surf_of_extrusion_p2--gen1140-finite-touching-2026-11-09.md):
  32 finite raw outputs are checked independently of overflowing sentinel sums. The huge implicit
  tolerance is retained as an explicit limit; no exact-contact or completeness claim is made.

## Golden accounting

The unchanged comparator against exact `b0fba...` expected files measures 1,393 serialized
updates across 31 units, of which 1,327 across 30 units exceed its original thresholds.
Six wrong golden lines are removed and their cases remain executed as known failures.
The original 135 base known-failure entries retain their classifications and reasons;
the review brings the registry from 140 to 141. The [per-unit delta](geom.local_solve_nd--goldens-default-2026-11-06--addendum-2026-11-09-review-rebase/golden-delta.txt)
records the counts. Main's independent directed-topology golden updates are preserved.

The complete validation status is reported in the PR description. The earlier full app
failure comparison and ignored-rig success belong to the earlier `f9ed...`/`94ae...` controls;
they are not relabeled as measurements of this new main.
