# A tangent-end rib's `[0, t*]`: bands about the fillet UV curve (#19140)

![Two plots of alpha against t near a tangent end. Left: the ring about alpha0 spans the whole of 0 to t*, from A to 2A off alpha0 on both sides, A twice the root's offset at t*. Right: two bands follow the root, one over t*/2 to t* and one over t*/4 to t*/2, each h to 2h off the curve's alpha on both sides; below t*/4 a smaller ring about alpha0 takes over.](blend.fillet_geom_p5--issue-19140-tangent-end-bands.svg)

*Look at how far each enclosure sits from the root: the ring about `alpha0` must clear the
root's offset `a(t*)` and its own width, the bands only a fraction of the offset there.*

## The cause

`Rib::near_bound` (`blend/fillet_geom/tangent_end_rib/certificate.rs`) bounds the rib next to the
tangent end, on `[0, t*]`, where Krawczyk's test of the graded pieces stops. Its ring about
`alpha0` proves `|alpha − alpha0| < A` there, `A` twice the root's offset at `t*`, and the curves
are within their distance from `S(sigma, alpha0)` plus `|S_alpha| A`. The root itself moves
`a(t*)` from `alpha0`, so the bound is about `3 |S_alpha| a(t*)` whatever the rounding. On large
rounds `|S_alpha| a(t*)` is a sizeable part of `approx_tol`.

## The construction

Where that ring's bound is over the tolerance, the same exclusion test (the separation of the
end surface from the section keeps one sign over a box, interval enclosures cut until they
decide) runs about the fillet UV curve's own `alpha_c(t)`, a known function, on bands graded
toward the tangent end so that the offset falls by at most half across one. A band's ring is
`h <= |alpha − alpha_c(t)| <= 2h`. With the root at `a` from `alpha0` and its mirror at `−a`,
the separation is about `½ Δκ ((alpha − alpha0)² − a²)`; at the inner ring's edges, `−h` and `−2h`
from the centre, it is `½ Δκ h (2a − h)` and `2 Δκ h (a − h)`, equal for `h = 2a/3`
(`4/9 Δκ a²`), and the outer ring's least is twice that. Below the bands the ring about `alpha0`
covers the rest, started at the root's farthest alpha there.

This is the exclusion test of interval analysis (definitions-and-certificates.md §4.3, [9, 10]
there) run along a predicted path, as interval path following does (R. B. Kearfott, Z. Xing,
"An interval step control for continuation methods", SIAM J. Numer. Anal. 31 (1994) 892–914).
The design is the maintainer's sketch on the issue's parent pull request, with the band's
radius derived above in place of the sketch's `|alpha_c − alpha0| / 4`.

## Measured

At fae7a1594e plus the change, with a probe that printed each rib's outcome (not committed),
the bands switched off (main's certificate) and on:

| test target | ribs on enclosed faces | open on main | open now |
|---|---|---|---|
| `issue_2360_mixed_corner` | 10 | 10 | 0 |
| `wr603_trace_from_tangent_point` | 1 | 1 | 0 |
| `issue_2489_blend_faces_are_definitions` | 1 | 1 | 0 |
| `issue_2089_tangent_tracer_crease_round` | 1 | 1 | 1 |

`issue_2307_peg_chamfers` and `issue_1945_fork_1207_step4` build no rib on enclosed faces at
that commit. The rib in `issue_2089` is on an order-1 definition: its separation's enclosure
does not narrow below about 3.5e-12 (the second cause in the issue), not touched here.

`wr603`'s rib (`|S_alpha|` 27, `approx_tol` 4e-5, `t*` 3.8e-6): the ring about `alpha0` over
`[0, t*]` gives 4.6e-5; the band `[t*/2, t*]` 3.8e-6; the ring below it 2.5e-5. The separation at
the band's corners is at least 2.4e-12 off zero; at a point its enclosure is 5.7e-13 wide.

The test surfaces `tangent_end_surfaces(r)` (a quarter cylinder of radius `r` and a biquadratic
end surface, tolerance 1e-5) stop their graded pieces at `t*` = 2.4e-7, where a point's
enclosure of the separation is about `1.1e-14 r` wide. With the band radius `a/2` the inner
ring missed by a few 1e-14 at `r` = 20 and 40. With `2a/3`, radius 40 is bounded (9.97e-6) and
radius 160 is not, in the unit tests at this change; an earlier probe of the same radius rule
also bounded 20 and 80 (the latter at tolerance 4e-5) and not 320.

## Not covered

The bands are exercised by contact order 2 (`m = 1`) only; orders 3 and up grade them by
`2^(−1/2^j)` (`2^j >= m`) and have no test of their own. A rib's existence between `t*` and the
tangent end is argued, not checked, for the bands as for the ring about `alpha0`.
