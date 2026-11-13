# General: the twisted disc's side rounds miss their radius by up to 1.8e-3; its top round leaves vertex gaps and fails at its named fit (2026-11-04)

Unit: the generic blend (hub #2243); `blend::fillet_exact`, the fillet trace's surface. Issue
#2652 (solidcraft#11848), PR #2659.

Inputs: the twisted slotted disc of solidcraft#11848 rebuilt from the issue's script (2 slots,
radius 30, height 15, 30 degree twist; #2649's `tests/issue_2648_twisted_disc_top_round.rs`
builder), its 4 twisted side edges rounded 2 mm, then its 12 top outline edges rounded.
SolidCraft's fix list (`tests/kernel_cases/fixes.json`). Rust only (the fixes are not in
the pinned C++).

## 1. Two of the four side rounds are not 2 mm rounds of their walls

For a point x of a side round with unit normal n, the ball's centre x + 2n should be 2 from
both walls the round joins. On side round `FaceId(28)` (and its mirror image `FaceId(30)`)
the error against wall `FaceId(12)` grows to 1.8e-3 at u = 23.6 (of its domain
[8.82, 31.52]), v = 0.9, z = 10.4, and is 5e-4 near the top (u = 27, z = 12.9); against the
other wall it stays below 1e-5. Elsewhere along the round (u = 11..18) both errors are
below 5e-5. Its rail-to-rail section has 3 control points, 106 along u. The other two side
rounds (`FaceId(29)`, `FaceId(31)`) put the top round's ball 2 from their walls to 1.4e-6.

Effect at the top round R2: over each side round's top edge the ball is as wide as the side
round, so its centre should be fixed (the spine crossing z = 13). Over the two accurate
side rounds it is (their end centres agree to 1e-12). Over `FaceId(28)`'s top edge the
round's radius of curvature toward the ball runs from 1.9988 to 2.0002, crossing 2: the face
is as wide as the ball along part of the edge only. Under the rule of #2659 (jyviko: a full
match only; a partial match is refused naming the smallest size that fits) the round is
refused before the trace (`OffsetCollapses`, fit 1.95861259706246 = 1.9988 less the margin
0.0402). Before #2659 it was traced through the nearly fixed centre: the centres at the
edge's ends are 1.1e-3 apart (solver tolerance 1.6e-4; a collapsed offset's centre is
ill-conditioned), the rail folded and the round failed with 1001 (#2652's diagnosis).

Suspected cause: the side round's surface between its traced sections (the error is small
at the section rows and large between them, with alternating sign), so its section spacing
or fit tolerance along the twist. Not yet compared with the fork or the C++.

## 2. The top round leaves vertex gaps above the vertex tolerance

The top outline rounded 1 mm builds a manifold solid, but `check_brep` Level2 reports
`SmVertex::Test #1 Bad Vertex Gap` (offset 2.4957061e-05, tolerance 2e-05). At the bands'
fit, 1.101826756514697 (the size named by the BandsOverlap refusal at 1.5 mm), it builds
with the same test failing at offsets up to 5.5762783e-05. With a ball smaller than the side
rounds no offset collapses, so these gaps are not #2652's event; whether they come from
the side rounds of part 1 is not yet known.

## 3. The named fit does not build

At the fit 1.95861259706246 the top round runs 877 s (release) and fails with
`Upstream { operation: "blend::fillet_executive::session::crossing_rails", code: 1001,
entity: Edge(EdgeId(3)) }`, with no fit of its own; 1.9 fails the same way (a slot wall's
top edge). At 1.5 it is refused `BandsOverlap` naming 1.1018, which builds with the gaps of
part 2. So after #2659 the user is shown 1.9586 at 2 mm, and that size does not build.
