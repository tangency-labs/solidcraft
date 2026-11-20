# Extrusion/plane patches retain the generator's normal offset

The in-plane certificate accepts a generator whose distance from the plane is
small but nonzero. Copying that generator at the two ends of a parallel sweep
preserves its normal offset. The final output loop previously replaced the
certificate with deviation zero.

## Closed-form regression

In `slow_departure_extends_the_range_or_declines`, shape 0 has control points
`(0,0,0), (1,0,0), (2,0,d), (3,0,d)`, with degree 1 and unit knot spans.
For `d = 1e-12`, its height is 0 on `[0,1]`, `d(t-1)` on `[1,2]`, and d on
`[2,3]`. Sweeping along Y changes no height. Thus the maximum distance from
z = 0 is d, including on both translated boundary copies. Shapes 1 and 2 also
attain maximum absolute height d at knots. A reported zero cannot bound them.

The rational regression uses positive weights `(1, 1/2, 1)` with every
Euclidean control point at z = d. Its height is exactly
`sum(N_i w_i d) / sum(N_i w_i) = d` at every parameter. After clipping, all
boundary sides can lie at z = 0, yet remain distance d from the extrusion.
The same identity holds at scales `1e-3`, 1 and `1e3`, with `d = 1e-12 scale`.
A check of only the output curves' distance to the plane misses this case.

## Bound

For each positive-weight net, let n be the plane normal, O its origin, P_i its
Euclidean control points, and e_i their split/conversion error bounds from
`net_points`. The existing `functional_err(n, P_i, O, e_i)` encloses the error
of the affine normal functional. By the positive rational basis's partition
of unity and nonnegativity, its maximum absolute value on the entire curve
is bounded by the largest control value plus its error.

Use `m = max(abs(n.x), abs(n.y), abs(n.z))`. Since `m <= ||n||_2`, division
by m bounds Euclidean distance without assuming a rounded unit normal has
length exactly 1. The bound is

`max_i ((abs(n dot (P_i-O)) + functional_err(n,P_i,O,e_i)) / m) (1+gamma(16))`.

For shape 0's finished copies at y = +/-1, the largest point 1-norm is
`4+d` and the degree is 1, so `net_points` uses `gamma(10)` and the maximum
bound is `(d + gamma(10)(8+2d) + gamma(8)(4+d))(1+gamma(16))`, about
`1.0124344978758066e-12`. This follows from the input control points, without
using a measured result as an expectation.

The inflation encloses the positive sums/products used to compute the error
expression, the numerator addition and division, and the inflation's rounding.
Reject a nonpositive/nonfinite m or any nonfinite per-point bound before taking
maxima. The existing in-plane acceptance certificate and tolerances stay fixed.

Retain the maximum over the certified generator ranges, then include the
finished boundary nets after trimming and translation. Apply that maximum to
the patch's output suffix, keeping preceding point-solution outputs aligned.
This addresses the generator's normal offset; sweep-direction certification
is unchanged.

The construction follows the convex-hull property in Piegl and Tiller,
*The NURBS Book*, and the gamma arithmetic model in Higham,
*Accuracy and Stability of Numerical Algorithms*, as recorded in
`docs/definitions-and-certificates.md` sections 4.2 and 4.4.
