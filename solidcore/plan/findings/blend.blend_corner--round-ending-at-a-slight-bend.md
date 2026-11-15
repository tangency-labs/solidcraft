# blend.blend_corner: a round ending where its edge bends slightly and the part widens (2026-11-15)

Units: `blend.blend_corner` (`blend_corner/end.rs`, `run_on_leaves_the_part`).
Test: `crates/solidcore-ops/tests/round_ending_at_a_slight_bend.rs`.

## The case

A disc (R 55) with a handle, extruded 16. The handle's top edge runs tangentially into a
concave arc (R 49), which meets the disc's rim at the vertex V 6.05 degrees off tangent
(SolidCraft's #15603). The two circles cross at V and again 5.5 mm on; between the crossings
the disc stands 0.144 across the arc continued. A round (r 2) of the handle's top and the arc
was refused with status 1001.

## The cause

The corner's tangency tolerance is 10 degrees, so the rim continues the arc at V, and the
question at V is `run_on_leaves_the_part`: does the round run on past V and out through the
disc's wall, or end at V on a cap square to the arc. It compared the two edges' second and
third orders at the far end of the shorter edge, where the rim has turned towards the round,
and answered "runs out". But the bend is a first order: just past V the rim leaves to the far
side of the arc continued. The round's surface and the wall then meet in no curve from the
wall rail's end at V; their curve starts at the second crossing. The End asked the
intersection for an end rib from V and got none.

## The rule

The bend is judged first. With the continuing edge standing `first x + second x^2` over the
blended edge continued (to second order), a negative `first` takes it `first^2 / (4 second)`
under before the curvatures bring it back. Deeper than the shared face's zone tolerance, the
part stands across the run-on and the round ends at V, capped. Otherwise the turn decides,
as before.

## What stays as it was

Measured on the test's part (the same circles, V at 50 degrees on the rim, an arc of 48
degrees) and its mirror image in the x axis, with and without the rule, on SolidCraft's main
at 5af8cbe38. Bends are positive where the rim leaves V away from the round. "Every fix but
two" is SolidCraft's set: all of `UpstreamFixes` but `fillet_refuse_rounds_breaking_through`
and `fillet_round_ends_on_outline`. "Runs out": the round runs on past V and ends on the
disc's wall.

| bend, degrees | default flags | every fix but two |
|---|---|---|
| 0.1, 6 | built, capped at V; without the rule 1001 | built, capped at V; without the rule 1001 |
| 0.02, 0.05 | 1001 | 1001 |
| 0.001, 0.003, 0.01 | 1001 | the part 1001; its mirror image runs out, at 0.01 as a body that fails its check |
| 0.000001 to 0.0001 | 1001 | runs out |
| 0, -6, -20, -45 | 1001 | runs out |
| 12 (past the tolerance: the End on the wall continued) | built; its mirror image `NotSolid` | built |

The rule changes the first row only (the test also takes 1 and 9 degrees). It fires in no
other: there the bend takes the rim no deeper than the tolerance, or is the other way. Without
it every entry under every fix but two was measured the same, and under the default flags
those at 0.01, 0.02, 0.05, 0, -6, -20 and 12.

Open, none of it touched by the rule:
- **Bends from about 0.001 to 0.05 degrees** take the rim under the arc continued by less
  than the zone tolerance (0.05 degrees: 1e-5), so the round still runs on. To second order
  the wall leaves the round's rail `first / second` past the rail's end (0.045 at 0.05
  degrees), not at it, and the run-on's end rib is refused or, on the mirror image, built
  from the rail's end all the same: at 0.01 degrees the body it returns fails its check
  (`Body::check` at `CheckLevel::All`). The run-on's End would have to start its rib where
  the wall leaves the rail, as a tangent end starts its own, or refuse.
- **Default flags, any bend that runs on**: 1001. At -45 degrees the rail's end on the rim
  was found 0.036 off the rail's circle of radius 51 (measured at c3b08915c), so no end rib
  passes through it.
- **Default flags, 12 degrees, the mirror image**: `NotSolid`, where the part itself builds.
