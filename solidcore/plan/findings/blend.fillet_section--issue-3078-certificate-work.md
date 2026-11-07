# #3078: variable-round certificate work

Unit: blend.fillet_section. Diagnostic only; no certificate arithmetic, target,
acceptance test, station solve, or fitted net is changed.

## Reproduction and evidence

`variable_round_certificate_work` in `wr685_law_corners.rs` is an opt-in,
profile-feature test. It constructs the four box/edge/law inputs from
`solidcore-ops/tests/blend.rs::a_variable_round_with_a_corner_in_its_law_is_its_balls_union`
through the kernel executive, with all fixes enabled. Each construction uses a
fresh kernel. It then runs the existing two-piece `(2, 4, 2)` curve-only helper.
That helper invokes `CircularCrossSectionFsg::create_surface`, not a support-face
procedural definition: it has **zero certificate spans and zero enclosures**.
It is a zero-work control, not an example of level-zero certification.

Run the following on a build host (the operator's `myy-cargo` wrapper is required
on the laptop):

```sh
cargo test --profile goldens -p solidcore --features profile \
  --test wr685_law_corners variable_round_certificate_work -- --ignored --nocapture
```

The profiler records notes only during an explicit capture. Without `profile`
the instrumentation expands to nothing. A capture is bounded, with an explicit
overflow marker; the measuring test rejects overflow and compares its enclosure
notes to the independent call-path profiler total. The original 100,000-note
capture truncated the two longer cases; those partial counts are not evidence
for the completed table. The final capture uses a 1,000,000-note limit.

## Counts

The accompanying `blend.fillet_section--issue-3078-shorter-cases.csv` records
every attempted span and every reached level for the **two complete shorter
captures only**. The longer two captures were truncated and are deliberately
excluded. The table includes rejected parent spans later bisected by the caller. `pieces` is the
level's planned partition size, not the number necessarily visited: a failed
first piece can end a level after just two enclosure calls. `enclosures` counts
actual calls. Repeated `[s0,s1]` attempts are retained in execution order.
`span_bound` is the best returned bound; an accepted attempt is one at or below
its recorded target. The CSV retains the failed arc construction's interval inputs, minimum rational
denominator and its numerator/weight terms, finite upper/lower bounds and excess
over target, and maximum station/UV fourth-derivative bounds. The new diagnostic
prints the circle's failed positivity interval directly, but that version has
not returned a remote verdict. No sampling manufactures bounds.

| Input | Span attempts | Accepted / rejected parents | Enclosures | Last-level counts L3 / L4 / L5 / L6 |
| --- | ---: | ---: | ---: | ---: |
| peak | 124 | 64 / 60 | 20,808 | 16 / 24 / 17 / 67 |
| dip | 104 | 54 / 50 | 18,774 | 16 / 12 / 12 / 64 |
| dip-peak | incomplete capture | unknown | profiler total 29,368 | unknown |
| reversed dip-peak | incomplete capture | unknown | profiler total 29,198 | unknown |
| curve-only helper | 0 | 0 / 0 | 0 | none |

The independent profiler totals for the longer inputs are complete, but the
per-span notes are not. Do not use their truncated per-span counts. Peak level
starts are `124,124,124,124,108,84,67`; dip's are
`104,104,104,104,88,76,64` (L0 through L6). Peak/dip have 136/104 failed
arc-row levels and 28/22 non-finite-bound levels. The extractor verifies every
span has an end and enclosure counts equal the independent profiler total;
1,399 level records cover all 228 shorter-case span attempts.

## What forces the work

The expensive refinement is observable without timings. In the first peak-case
span `[-4.999899999999967, 0]`, target `4.000000000000001e-6`, levels 0 through 4
fail arc-circle positivity after the first piece. Both station fourth-derivative
bounds remain around `2.2e-29` to `2.5e-29`, not large geometric curvature.
At level 5 all 32 pieces are enclosed (64 calls), but the middle homogeneous row
has

```text
w_lo = 0.5795065387500216
(h^4/384) * w4 = 17.410143434327928
w_lo - (h^4/384) * w4 = -16.830636895577907
```

Level 6 encloses all 64 pieces (128 calls), yet its denominator is still
`0.6519574090667212 - 0.7782375059267068 = -0.12628009685998565`.
The returned bound is infinity; this one rejected parent already cost 202 calls.
Halving that span gives `[-4.999899999999967, -2.4999499999999837]`:
level 5's finite bound is `1.1381794509220917`, level 6's is
`0.1959090577823233`, still about 48,977 times the original `4e-6` target.
Its station and UV fourth bounds remain about `2.4e-29`.

This localizes the excessive refinement to the interval **touch-circle / rational
arc-row composition**, not failure to prove the station root or a large rail
fourth derivative. On the complete peak/dip traces every one of the 20,808 / 18,774
root proofs succeeds in one Krawczyk iteration. Their contraction factors range
from `1.24e-13` to `2.47e-13` / `9.00e-14` to `3.04e-13`, far below the rejection
threshold of one. In `arc_row_terms_over`, each rail and the centre is first
centred independently over the whole piece. The touch-circle projection, relative
radius vectors, squared lengths, normalized dot product, square root, and
homogeneous middle row then compose these dependent intervals. Their common
motion and scale correlations are no longer available. Subdividing the enclosure
domain reduces this overestimation, but the remainder multiplier is the original
span's `h^4/384`, correctly not the piece's. The resulting excessive row fourth
bounds force deeper enclosure levels and eventually smaller interpolation spans.

For the ideal plane-plane geometry with an affine centre and linear radius on
one smooth law segment, the touch-circle radius vectors are fixed directions
times the radius: their normalized dot product and weight are constant, and the
homogeneous rows are affine. This explains the cancellation that the interval
composition misses. The tiny *numerically enclosed* station fourth derivatives
are not an algebraic proof that the stored definition's derivatives are zero;
none is discarded. Law kinks already delimit smooth pieces, and the quoted
failure occurs within a single piece, so removing or smoothing a kink is not a
sound remedy. The target is unchanged and not intrinsically incompatible with
the geometry; it exposes the arc-row overestimation.

## Scope and next proof obligation

No performance fix is included. The measured cause is repeated high-level
certification work; this does not measure machine copies or establish their
share of elapsed time. Series temporaries may make each enclosure expensive,
but the add-of-scale/in-place work of #2728 is deliberately untouched.

A general fix needs a dependency-preserving enclosure of the relative circle
vectors and rational rows, with a proof that all required derivative intervals
and denominator lower bounds remain enclosing, including varying-speed centres
and non-planar supports. An affine-only numerical-zero shortcut, sampled bound,
smaller multiplier, relaxed target, or skipped positivity test would not supply
that proof. The diagnostic is retained to compare every span before/after such
a change. There are no claimed after-count reductions or golden movements.

## Validation

- Earlier remote job 31148 returned wrapper exit 0 with no log/copy-back; the
  operator confirmed it had been deliberately cancelled and nothing ran.
- Remote job 31215, through `myy-cargo`: `cargo fmt --all` and the opt-in trace
  test passed (1 passed, 15 filtered out). That run used the original capture
  instrumentation at `44e13e89a17` plus formatting, not the subsequent larger
  capture / numeric-positivity diagnostics / profiler unit tests.
- The longer captures in 31215 hit the old note limit. Shorter-case extraction
  and its completeness/count assertions pass (228 spans, 1,399 levels).
- Remote job **31233**, name `sc-gpt-1107-141314-2-edit`, was submitted through
  `myy-cargo -t 120` for the complete trace and formatting. The local background
  waiter reached its maximum 7,200-second tool limit and was killed, with **no
  remote exit verdict and no copied-back files**. The Slurm job's state is
  unknown; it has not been cancelled or restarted. Do not resubmit into that
  build tree until the operator resolves its state. The wrapper reported its
  remote log as
  `/home/aapo/scratch/solidcore/logs/slurm-31233-gpt-1107-141314-2-edit.out`.
- The latest instrumentation changes and added profile tests are therefore
  uncompiled/unverified. Its formatting has not returned either.
- `git diff --check` passed locally. No cargo/rustc/build/test ran on the laptop.
- `reports/issue-3078-validate.sh` records the remaining validation commands:
  section/profile lib tests, full ops blend binary, all selected variable/fillet
  integration binaries, fmt, workspace and profile clippy, wasm, panic-site
  check, and blend-unit goldens. **None of those gates has run on the latest
  source.** No golden movement/no-change comparison is claimed.

This is a partial investigation, not a ready performance-fix branch. Remaining
work is the complete four-case per-span capture, direct numerical circle
positivity diagnostics, compilation of the final instrumentation/tests, and all
requested gates/goldens. No certificate-preserving optimization is claimed.
