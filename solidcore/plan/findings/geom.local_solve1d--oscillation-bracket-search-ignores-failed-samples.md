# geom.local_solve1d: failed oscillation samples, confirmation and opt-in callers (2026-10-26)

Unit: `geom.local_solve1d`; WR-253 / #481; PR #668.

Pinned `SolveOscillating` reads the final attempted sample's F even when that
sample failed. The zero-on-failure evaluator mode added by this PR independently
reproduces the curvature evaluator's behavior in Rust and C++. The 14-value test
ABI is unchanged: mode 0 keeps outputs, 1 writes F, and 2 zeroes F and F'.

The native reproduction is `oscillation_failed_last_guess_zeroed_outputs`:
F=t^3-2t+2 on [-3,3], initial guess zero, and an evaluation failure exactly at
0.94564800000197. C++ reports that value as a converged root with F=0, although
its actual F is about 0.95. Main's default Rust policy deliberately agrees.
With `Kernel::fixes.oscillating_skips_failed_guesses`, the solver instead reports
no root, t=0.8316000000017324 and actual F approximately 0.9119000984961294.
The earlier native comparison confirmed WR-253; its addendum records that evidence.

Main already contains the default-off solver correction and the curve-property
caller. This PR adds three direct opt-in regressions: compare a failed final
sample with the clean no-root solve, reject all eight failed samples after they
write zero or an opposite sign, and accept a later valid bracket after an earlier
sample fails (root 0.85). Main's default false-root regression remains intact.

The same switch is now forwarded by every other production `LocalSolve1d`
constructor: curve point/extrema/cache, surface tangent/silhouette and clipped
fillet radius solves. Direct solver users retain the existing explicit setter.
Successful samples, iteration/formula order and unported-error propagation are
unchanged. The differential fixture runs the default policy, so no known mismatch,
status downgrade, comparison omission or widened tolerance is needed.
