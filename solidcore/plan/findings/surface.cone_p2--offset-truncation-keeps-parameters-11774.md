# The apex-truncated cone offset anchored its UV domain at the wrong end

Unit: `surface.cone_p2` (`Cone::create_offset_surface`, the one-radius-below-tolerance
branch). Found chasing tangency-labs/solidcraft#11774 (a shell on a cone with an apex
refused with `status 1001`).

## The defect

The offset of a cone whose radius passes zero is the cone truncated at its new apex,
made by `create_canonical` and reparameterised "to the matching part of this NURB
domain". The matching part kept the domain's minimum and scaled the maximum
(`min + new_height / height * max`), whatever end the radius collapsed at. When the
radius that passes zero is at the domain's minimum (the apex end of a revolved cone,
whose `bot_radius` is 0), the kept cone sits at the domain's *maximum* end: the
matching part is `[max - ratio * span, max]`, not `[min, min + ratio * span]`.

The offset of S is S + d n **at the same parameters** (offset-and-shell.md §1). With the
domain placed at the wrong end, corresponding points did not keep their parameters: the
rim's UV moved (40 to 35.006 in the #11774 cone), and every consumer that compares UV
points broke. In the shell that showed it: `map_edgeuses` (`offset_executive/p1.rs`)
pairs an original edge use with its offset copy by their UV midpoints and by dropping
the offset's UV point onto the original's UV trim curve; the rim pair matched neither,
so the rim edge use got no offset counterpart, `mate_edges` returned
`SM_ERR_NULL_POINTER` on its "can't be Null" guard (softly: the edge stayed unmated),
the rim kept its raw normal-offset circle instead of the mitre circle, and
`close_loop_gaps` died bridging the 0.75 mm step (`MakeVertexSplitEdge` refuses a split
at a curve's end). Every stage behaved to its own contract; the parameterisation
placement broke the one rule.

The mirror branch (the radius at the domain's maximum passing zero) was already
anchored at the minimum, which is why truncated cones and cylinders without an apex
shelled correctly and only the apex case failed.

## The fix

Anchor the matching part at the end that survives: the top's end when the bot radius
passes zero, the bot's end when the top radius does. `Cone::p2`
`offset_truncation_keeps_the_surviving_ends_parameters` pins both branches.

## What a later agent must know

- The span is scaled by `new_height / height`, which is correct only because the cone's
  height dimension of the natural UV domain has the height's length (v runs bot to top
  over `[0, height]`). A cone whose domain is not the natural one would need the
  ratio's span taken from the collapsed radius instead.
- The record rebuild and the copy paths (`analytic_copy_keeps_parameterization`,
  `copy_and_add_analytics`) preserve the domain, so a rebuilt cone shows the same
  behaviour as an in-kernel one; the #11774 body reached the shell through a record
  copy and reproduced without one.
- Upstream scales the domain maximum rather than its span (the branch's doc note): a
  separate defect, unfixed here (min is 0 in every recorded case, where the two agree).
