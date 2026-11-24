# WR-383 paired-retirement golden audit — 2026-11-15

All seven requested curve changes have direct geometric support. Their private proposed golden records exactly match the repository encoder applied to the fresh raw results. No tracked source, golden, index, or public state was changed by this audit. No Cargo build or test was run.

The captures are under `nov15/paired/golden-review/{main,nd-only,after}`. Here `main` is the fresh `be6ecb06420d501a8362d6de3d0de7ef324dafb9` kernel; latest main `c71236a40d` only changed generated catalogues. Every input is identical across these captures. For all seven cases, `main` and `nd-only` have identical output bits. All three stages report success, with no error or deferral. These seven changes therefore isolate the WR-383 retirement within the measured pair.

## Five direct endpoint-policy cases

The P1 output is `[count, start_deviation, start_t, start_u, end_deviation, end_t, end_u]`. It describes the refinement of a supplied candidate range; it does not certify that the entire supplied range is coincident.

For the four direction cases, the exact curves are

`L(t) = (t,0,0)` or `(1-t,0,0)`, and
`H(u) = (u,0.1u(1-u),0)` or `(1-u,0.1u(1-u),0)`.

Their only exact common points occur at the two ends. In every direction, the unchanged start refinement reaches its exact intersection. Main leaves the other end at its approximate parameter and deviation `9.999e-6`. The new default reaches its exact intersection with zero measured deviation:

| Case | Main end `(t,u)` | New end `(t,u)` | New start `(t,u)` |
| --- | --- | --- | --- |
| `refine_range_end_0_0` | `(0.9999,0.9999)` | `(1,1)` | `(0,0)` |
| `refine_range_end_0_1` | `(0.9999,0.00009999999999998899)` | `(1,0)` | `(0,1)` |
| `refine_range_end_1_0` | `(0.00009999999999998899,0.9999)` | `(0,1)` | `(1,0)` |
| `refine_range_end_1_1` | `(0.00009999999999998899,0.00009999999999998899)` | `(0,0)` | `(1,1)` |

For each parameter, the oriented fraction is `phi=(root-start)/(end-start)`. The correct end root has `phi≈1.000100020004001`. Main tests `|phi|`, rejecting it. The corrected end-relative test obtains `|1-phi|≈0.00010002000400078914`, below the unchanged `0.002` limit. This reasoning applies independently to increasing and decreasing parameters.

For `refine_range_end_rejects_start_jump`, `L(t)=(t,0,0)` and `C(u)=(u,0.1u,0)` intersect only at zero. Starting from the supplied end `(0.9,0.9)`, the local solve reaches approximately `(5.551115123125783e-16,5.551115123125783e-16)`. Its fraction is approximately `-0.00011112345816140115`: main incorrectly accepts the end jumping to the start. The corrected end-relative fraction is approximately `1.0001111234581614`, so the end remains exactly `(0.9,0.9)` with its supplied deviation `0.09`. The start result is unchanged. Retaining this candidate end is the helper's policy result, not a claim that these two crossing lines overlap over a range.

Inspection of the actual P1 diff confirms that the existing end-relative construction became unconditional. Desired precision `1e-10`, endpoint eligibility `1e-6`, shift limit `0.002`, parameter orientation, and input fixtures were preserved. Status propagation remains in its separately claimed scope.

## P8 saved tail and `gen2160`

`range_end_preserves_noncollapsed_tail` and `gen2160` have identical input objects and identical output bits at every stage. They exercise one geometry twice, not two independent constructions.

The operation is global curve/curve intersection with requested tolerance `0.5`, and the coincidence check enabled. The first curve is the supplied bounded line `L(t)=p+2t*unit(v)`. Its NURB and analytic domains are both `[1.7791547593931956,8.766899083602688]`, with no parameter reversal. The second is the supplied nonrational quadratic B-spline, with four control points and knots `[0,0,0,0.061999436959862465,1,1,1]`. Its `NOW` weight sentinel means ordinary unweighted control points.

Main returns one point and one range. The new default preserves those two complete payloads bit for bit and appends one `RangeOfValues`:

- Line interval: `[6.11037890900769,6.344696713196761]`.
- B-spline interval: `[0.4336618890164721,0.5053108900962493]`.
- Stored endpoint deviations: `7.377764055609925e-15` and `0.3323583728011054`.

The new start equals the previous range's end bit for bit. Both spans are positive, approximately `0.234317804189071` and `0.0716490010797772`. They lie wholly inside both requested intervals and both natural domains. The entire B-spline tail lies inside the single polynomial knot span `(0.061999436959862465,1)`.

### Independent whole-interval bound

Let `s∈[0,1]`, and interpolate each parameter affinely between its returned endpoints. The difference

`D(s)=L(t0+s(t1-t0))-B(u0+s(u1-u0))`

is a quadratic vector polynomial on this knot span. Its degree-two Bernstein controls satisfy

`D0=D(0)`, `D2=D(1)`, `D1=2D(1/2)-(D0+D2)/2`.

An exact-rational de Boor evaluation of the supplied binary64 B-spline data, together with rational interval bounds for the line constructor's binary64 normalization, yields control-norm upper bounds approximately

`1.104e-14`, `0.15137110751868907`, and `0.3323583728011098`.

The JSON certificate stores rational coordinate intervals and squared-norm upper bounds. Each squared bound is strictly below `(333/1000)^2`. Bernstein coefficients are nonnegative and sum to one throughout `[0,1]`; consequently

`||D(s)|| < 333/1000 = 0.333 < 0.5`

for the entire added range. This establishes a continuous affine correspondence below the caller's tolerance. It strengthens the existing eleven-sample regression without interpreting samples as a uniform proof.

The result is approximate coincidence at tolerance `0.5`. It is not exact curve equality, a completeness or uniqueness proof, a certificate for every floating-point evaluation, or a claim about any resulting B-rep. The stored endpoint deviations agree with this geometric bound.

## Evidence and historical records

The existing frozen records remain historical:

- `solidcore/plan/findings/curve.curve_p1--range-endpoint-refinement.md` (2026-10-26).
- `solidcore/plan/upstream-issues/WR-383--refine-cc-solutions-never-refines-a-forward-range-end.md`.
- `solidcore/plan/upstream-issues/WR-383--addendum-2026-10-26-saved-range-tail.md`.

Their default-off versus opt-in descriptions apply to that earlier source. The present audit supports retiring that switch; it does not edit or retroactively relabel those records.

Reproduce the certificate with `nice -n 10 python3 unbundled-validation/wr383-goldens-proof.py`. The report `wr383-goldens.json` includes all raw outputs, bit patterns, input objects, capture hashes, encoder comparisons, exact fractions and parameter fences. The encoder uses full arrays through length 16, then block length 256 with sequential sum, absolute sum and local weighted sum. It confirms the seven private proposed records, including the P8 length change from 20 to 31.

Archive note: the certificate helper named above is not bundled. Its path is a sanitized historical provenance identifier; all numerical certificate data are retained in [wr383-goldens.json](wr383-goldens.json).
