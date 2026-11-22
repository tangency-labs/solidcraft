# Planar chamfer bands that only touch are not an overlap

Issue: SolidCraft #20450. Addendum to
`blend.fillet_executive--parallel-band-retained-interval.md` ("The closed event also
decides overlap").

## What went wrong

The parallel-line certificate refused a pair of bands when the exact squared separation
of their original-edge feet was below the squared sum of their widths,
`|Q - P|^2 < (a + b)^2`, with no allowance at all. Two chamfers whose setbacks add up to
exactly the width of the face they share (two 1 mm chamfers on the long edges of a 2 mm
face) meet in its middle. The rounding of the edges' line data or of a setback can put that
sum one or two ulps above the separation. The certificate then took bands that only touch
for overlapping bands and refused them, naming the open-margin scale (0.999 mm). Whether it
did depended on how the edges' lines happened to round, so the same design built at some
block lengths and not at others.

The band rail check beside it refuses a rail only when it lies inside the other band by more
than the solvers' tolerance `tol` (`from_rail > tol`). Before the certificate, that was the
only threshold, and touching chamfers built.

## Construction

The certificate keeps its exact arithmetic and now uses the rail check's threshold: bands
overlap when they overlap by more than `tol`,

    |Q - P| < a + b - tol,   that is   a + b - tol > 0  and  |Q - P|^2 < (a + b - tol)^2.

`a + b - tol` and both squares are formed as error-free expansions, as before; no rounded
value enters the comparison and no new tolerance is introduced. An overlap by more than
`tol` is refused exactly as before, so the 30 x 0.6 x 10 case of the earlier addendum (widths
2 against a gap 0.6) is unchanged. The fit named by a refusal is unchanged.

## Regression

`chamfers_that_exactly_use_up_their_shared_face_build` in `tests/chamfer_band_scale.rs`
chamfers the two top long edges of a 30 x 2 x 10 block by (1, 1). With the block's near side
at y = 0.3 the far edge lies at the binary64 sum `0.3 + 2`, so the edges are `2 - 2^-52`
apart and the widths exceed that by 2.2e-16 mm. At y = 0 the edges are exactly 2 apart.
Both must build with volume `600 - 2 * (1 * 1 / 2) * 30 = 570` mm^3, and chamfers of 1.01
mm, which overlap by 0.02 mm, must still be refused.
