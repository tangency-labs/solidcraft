# The notch must preserve the complete torus meridian

Review of SolidCraft #18804: a valid solid and the correct volume alone do not
exclude an unwanted seam split. The existing regression
`a_notch_beside_a_half_torus_seam_keeps_its_boundary_on_the_seam` now also checks
the boundary topology and the complete retained meridian, at both scales and
both offsets, with and without its preparatory holes or closed void.

## Topology and the existing contact rule

The upper half torus has two faces (the curved surface and the flat annulus),
three edges (the two equatorial circles and the seam meridian), and two
vertices (the ends of that meridian).

The notch's footprint meets the outer equator twice, away from the meridian.
It leaves the original curved face connected and adds one cylindrical face.
Its top is above the torus throughout the overlap, so no planar top face is
kept. Its own cylindrical seam is on the outward side, outside the kept side
wall, so that seam is not retained. With distinct corners, the two new
intersection vertices divide the original outer-equator circle into three
pieces; removing the notch leaves two pieces and the original meridian
endpoint. The two intersection edges would then give `[3, 6, 4]`.

That zero-tolerance incidence is not this fixture's existing contact rule.
Write b=major+tube, r=notch radius and alpha=(r+d)/b. The near equator
intersection has longitude beta=alpha-2 asin(r/(2b)). For these inputs beta
is positive: the correction `2b(asin(x)-x)`, x=r/(2b), is bounded above by
`r^3 / (24 b^2 (1-x^2)^(3/2))`, which is less than 2.5e-6 at the small
scale and 4e-11 at the large scale, both below the smallest d=1e-5.
The exact chord gap to the original seam corner is
`D=2b sin(beta/2) < b beta < d <= 2e-5`. Both corner tolerances are at least
the existing 1e-5 zone tolerance. Their contact regions therefore overlap;
`docs/booleans.md`, "One point or two", makes them one proto vertex.
The near crossing shares the original meridian corner, removing one vertex
and one short outer-equator edge from the distinct-corner incidence above.
The expected result is therefore `[faces, edges, vertices] = [3, 5, 3]`.
The far crossing has longitude alpha+2 asin(r/(2b)); its chord distance
from the seam corner is greater than r, at least 0.06, beyond the existing
0.01 capture search. Only the near crossing shares the corner. No tolerance
is introduced or enlarged by the test.

The same separation is verified for the actual stored binary64 cylinder
centres, including their trigonometric construction rounding, without treating
ideal sine values as exact inputs. Treat b, cx, cy, r and d as exact binary
rationals. The point at chord distance h from P=(b,0) on the positive outer
circle has

```
x = b-h^2/(2b), y = sqrt(h^2*(1-h^2/(4b^2))).
```

For all four scale/offset pairs, integer rational arithmetic proves P is
outside the notch, and that both h=d and h=r give points strictly inside it.
The radical is enclosed by adjacent multiples of 2^-256 using integer square
root; squaring both interval ends verifies the enclosure, and the maximum of
the two squared distances bounds the whole interval. Thus the near crossing
lies strictly before chord d and the far crossing strictly after chord r.
The two-circle intersection has at most two crossings. The lane validation
artifact `notch-topology-proof.py` records these exact comparisons; the
2^-256 radical enclosure is arithmetic precision, not a geometric tolerance.

The first validation used the distinct-corner count and failed with actual
`[3, 5, 3]` at the small plain case. This exposed the omitted contact rule;
the corrected count follows the inequalities above, rather than being copied
from that measured total. The separate complete-meridian assertion still
requires the corner's original coordinates and curve to survive exactly.

Each of the three disjoint blind holes adds a cylindrical wall and a planar
floor, with two circular edges and one cylindrical seam. The seam has two
vertices. It removes an interior disk from the existing annular face without
splitting that face: its addition is `[2, 3, 2]`. Three holes therefore give
`[9, 14, 9]`. This preparation is exercised only at the small scale, as in the
original regression.

The closed toroidal void adds its own curved surface and flat annular cap,
two circular edges and a meridian edge, and that meridian's two endpoints.
Its addition is `[2, 3, 2]`, hence the notched hollow body has `[5, 8, 5]`.
The void is strictly inside the material and disjoint from the notch, as
derived in `boolean-seam-crossing-tolerance-contacts-18804.md`.

Neither a positive change in notch longitude between the two offsets nor the
change in torus scale changes any of these incidences. These counts follow the contact rule and incidences above; they are not
golden values.

## Exact meridian invariant

Before the notch, the regression identifies the outer torus meridian as the
edge with two uses on one face and an endpoint at the outer equator on +x.
It checks the entire source curve and its native form lie in the plane y=0:
for an analytic circle or ellipse, its centre and both frame axes have zero y; for a
spline, every control y coefficient is zero; for a rational spline, every
homogeneous y coefficient is zero and every weight is positive. These are
whole-curve identities, without a distance tolerance or sampled certificate.
The fixture's generating profile supplies the circle of tube radius centred
at `(major, 0, 0)` in that plane.

The result must retain exactly one edge with the identical complete source
curve, native form, parameter interval, and endpoint coordinates. The edge
must still have two uses on the same face. This checks that the original
meridian survives intact: a displaced curve, shortened interval, added seam
split, changed endpoint, or loss of the seam boundary fails independently of
the existing removed-material volume assertion.

## Preserve the existing judges

The original `half_torus` helper, including its below box, matches origin/main
exactly; it is compared read-only. The new cases use the separate radii helper.
The boolean file differs from origin/main only by added lines. Its existing
corpus loader already calls `solidcraft_fix_names`, which this older branch's
shared test support did not define. The support now exposes the same recorded
fix names to both the existing preset builder and the session-based caller,
without changing the existing boolean test loader or importing files from main.

## Final targeted validation

The corrected notch regression compiled and passed (exit 0; 1 passed,
0 failed, 69 filtered out). It exercises all ten scale/preparation/offset
combinations, including the exact complete-meridian assertion and the
separate volume assertion. The existing half-torus boss judge also passed
in the preceding two-test run; that run failed only the initial distinct-
corner count, corrected above by the existing contact rule.

```
TMPDIR=/home/ray/builds/lane-18804/tmp buildq run solidcraft . -- cargo --config 'env.TMPDIR.value="/home/ray/builds/lane-18804/tmp"' --config env.TMPDIR.force=true test --manifest-path /home/ray/builds/worktrees/lane-18804/solidcore/Cargo.toml --profile goldens -p solidcore-ops --test boolean -- a_notch_beside_a_half_torus_seam_keeps_its_boundary_on_the_seam --nocapture
```

Both workspace formatting commands passed through buildq. Clippy was canceled
while queued after 1914 seconds (exit 130; no lint checks ran). The wider
boolean-file job was narrowed before execution; those remaining tests are
left for CI under the instruction to stop once this regression passes.
