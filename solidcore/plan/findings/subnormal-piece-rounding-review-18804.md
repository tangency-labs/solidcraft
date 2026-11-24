# Subnormal rounding in native piece hulls

Review of SolidCraft #18804: a relative error allowance cannot certify a
rational piece when splitting loses a subnormal homogeneous coordinate.

## Exact counterexample

A degree-one curve has equal weights `w = 2^-1070` and homogeneous controls
`(0,0,0,w)` and `(w,w/16,0,w)`. Its exact definition is `(u,u/16,0)`.
At `u=1/2`, the split multiplies the second Y coordinate `2^-1074` by
one half. Rounding to nearest, ties to even, produces zero. The Euclidean
split control is then `(1/2,0,0)`, missing the exact endpoint's Y coordinate
`1/32`. The old two-level allowance is `32 eps`, approximately `7.1e-15`.
This is a failed enclosure, independently of any geometric tolerance.
The horizontal segment from `(3/10,1/40,0)` to `(9/20,1/40,0)` crosses the
exact curve at `(2/5,1/40,0)` in the left piece. In the binary64
regression, the stored horizontal Y coordinate is `fl(1/40)`; its exact
intersection has X=`16Y`, still strictly inside both stored intervals.

The regression in the existing `curve/proximity.rs` test module requires any
returned piece box to contain the exact endpoint with zero tolerance, and
requires separation of these crossing pieces to remain unproved. It also
checks normal-minimum and unit weights.

## Bound including gradual underflow

Use the standard rounded-operation model extended by its additive term:
`|fl(z)-z| <= u |z| + eta`, where `u=2^-53` and `eta=2^-1074` is a
conservative whole subnormal unit (the nearest-rounding error is at most
half that unit). This is Higham's rounding model used by the kernel, with
the underflow term retained instead of assumed absent. The shared piece bound
also serves curve and surface covers; every caller now excludes a non-finite
total knot span, so the coefficient subtractions cannot silently overflow.

For stored binary64 knots, a subtraction whose result is subnormal is exact:
both operands and their difference are integer multiples of eta, and every
such subnormal multiple is representable. Normal differences use the usual
relative bound; the finite total-span checks exclude overflow. The split
coefficient is in [0,1]. A division that underflows therefore adds at most eta
to that coefficient's absolute error, which is already smaller than one u
in the conservative coefficient budget. Its contribution after multiplication
by a homogeneous coordinate is absorbed in the relative H term, not lost.

For L convex-combination levels, budget N=`16L+2` operations per dependency
path. A level forms its coefficient by two knot subtractions and a quotient,
forms its complement, then two products and a sum for each homogeneous
coordinate. The coefficient error affects both terms; sixteen unit roundoffs
cover these contributions. N also covers the initial division used to bound
the Euclidean controls. Non-finite knot differences cannot use this model.

Let H be the largest absolute homogeneous spatial coordinate, W the largest
weight, w the smallest positive weight, and M an upper bound on the exact
Euclidean control coordinates. With g=`gamma(N)` and A=`N eta (1+g)`, the
homogeneous spatial and weight error bounds are respectively
`dx=g H+A` and `dw=g W+A`. Convex combinations do not amplify exact inherited
errors; `(1+g)` covers their rounded propagation. All arithmetic forming the
bounds is rounded outward, including the division defining M.

If `w-dw` is not strictly positive, the piece is uncertified. Otherwise the
Euclidean error before the last division's rounding is bounded by

```
(dx + M dw) / (w-dw).
```

This follows by subtracting `(H+deltaH)/(W+deltaW)` from `H/W`; the
computed denominator is at least `w-dw`. Add the last division's rounding,
`gamma(1) (M+error)+eta`, again outward. Any non-finite bound leaves the
piece uncertified. No distance tolerance is introduced or changed.

For the reported `w=16 eta`, the conservative weight-error budget exceeds
w. The corrected construction therefore leaves that piece, and hence its
separation from the crossing segment, unproved.


## Reproduction

Before changing the bound, the targeted regression ran through buildq and
failed (exit 101; 0 passed, 1 failed). Its reported piece-box upper Y bound
was `7.105427357601003e-15`, excluding the exact endpoint `1/32`. This
confirms the enclosure failure independently of any boolean result or golden.
The corrected-run results are recorded with the review validation report.


After the bound change, all nine `curve::proximity::tests` passed in the
consolidated targeted run, including
`subnormal_split_rounding_cannot_prove_crossing_curves_disjoint`. The same
run intentionally included a separate seam-authority diagnostic, whose
failure is recorded in `heal-face-props-seam-review-18804.md`; it was not a
failure of this rounding regression.
