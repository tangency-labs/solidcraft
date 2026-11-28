# Chamfer ends meeting after an earlier chamfer (2026-11-18)

**Issue:** SolidCraft #17026. **Area:** the blend executive and corner topology.

## Reproduction

Make a 20 x 20 x 20 box. Chamfer its four vertical edges by 2, picked at
`(0,0,9)`, `(20,0,9)`, `(0,20,9)` and `(20,20,9)`. Chamfer only the four
original top edges by 2, picked at `(10,0,20)`, `(10,20,20)`, `(0,10,20)`
and `(20,10,20)`. The four short top edges of the earlier chamfers are not selected.

The installed SolidCraft CLI 0.44.1 (`e151d5ea6866fb055e011379153fb1cae0566f22`)
refuses the second chamfer with status 1001. Reversing the operations builds a
closed solid with 14 faces, 36 edges, 24 vertices and volume 7704 mm3.
Chamfering all eight original edges in one call also builds a closed solid with
those same counts and exactly 7704 mm3.
The failure was reproduced twice. Actual snapshots of the state before the
refused operation and of the successful reverse order were captured and inspected;
the generated media are review evidence, not repository assets.

Keeping the first distance at 2 and changing the second gives:

| top distance | CLI result | volume mm3 | edges | vertices |
|---|---|---|---|---|
| 0.5 | builds | 7831.833333333333 | 36 | 24 |
| 1 | builds | 7806.666666666668 | 32 | 20 |
| 1.5 | refuses: result is not a closed solid | -- | -- | -- |
| 2 | refuses: status 1001 | -- | -- | -- |

These controls are observations of 0.44.1. The worktree's targeted regression
`blend_chamfer_corner::top_and_vertical_chamfers_build_in_either_order` also
fails on the second chamfer, returning `Status(1001)` (0 passed, 1 failed).
It lives in the existing `solidcore-ops/tests/blend_chamfer_corner.rs` and is
explicitly ignored while the missing construction remains unresolved; running it
with `--ignored` replays the issue without changing the expected result.
The separate `top_chamfers_before_the_end_collision_match_the_octagon_slices`
test checks the successful 0.5 and 1 mm controls against the derivation below.
The targeted module run passed all four active tests (the control and three
existing tests), with the unresolved reproduction explicitly ignored.
`solidcore-ops` clippy with all targets and `-D warnings`, the root formatter,
and solidcore's own formatter passed. No production code or golden changed.

## Exact expected result

Near `(0,0,20)`, put `w = 20 - z`. The three cuts retain the half-spaces
`x + y >= 2`, `x + w >= 2`, `y + w >= 2`. They meet at `(1,1,19)`.
Their pairwise plane intersections close the corner without another face.
Intersection of half-spaces commutes, so either order and a simultaneous chamfer
must produce the same solid.

Each of the eight edge cuts removes a triangular prism of volume
`20 * 2^2 / 2 = 40`. At each of four top corners, three pairwise overlaps have
volume `2^3 / 3` each and their triple overlap has volume `2^3 / 4`.
Inclusion-exclusion gives
`8000 - 8 * 40 + 4 * (3 * 8/3 - 8/4) = 7704 mm3`.

There are 6 original faces and 8 chamfer faces. Their boundaries have
`8 + 4 + 4*4 + 4*5 + 4*6 = 72` edge incidences, so 36 edges and,
by Euler's formula, 24 vertices.

For a top setback `s` after vertical setback 2, the three planes meet at
`M = (1,1,21-s)`. Below `s=1` this is above the original top. At `s=1` the
short old top edge collapses. Above `s=1`, the two new chamfers require a new
shared edge from `M` to `(s,s,20)`, of length `sqrt(3) * (s-1)`.

For `s <= 1`, each horizontal slice removes four disjoint strips from the
vertically chamfered octagon. At strip width `t`, their total area is
`4 * (16*t + t^2)`. Integrating from 0 to `s` removes
`32*s^2 + 4*s^3/3` from the intermediate volume 7840. This gives
`46991/6` at `s=0.5` and `23420/3` at `s=1`, explaining the successful controls.
At `s=0.5`, the bottom and top are octagons, four walls are rectangles,
four vertical bevels are hexagons, and four top bevels are quadrilaterals:
`8 + 8 + 16 + 24 + 16 = 72` edge incidences, giving 36 edges and 24 vertices.
At `s=1`, the top becomes a square and the vertical bevels become pentagons,
reducing the incidence count to 64: 32 edges and 20 vertices. Both have 14 faces.

## Construction boundary

The documented exact chamfer mitre (docs/generic-blend.md, section 3.2) already
supplies the required plane geometry. Its current topology event starts at one
existing target vertex:

- `fillet_executive/p1.rs::create_fillet_corners_recording_vertex_errors` walks
  `filleted_vertices`, populated by the selected edges' input endpoints.
- `fillet_executive/session.rs::create_corners` creates those per-vertex corners.
  Its exact chamfer event requires `corner.chamfer` and `ConvexNByN`: all the
  incident edges at that existing vertex are chamfered.
- `fillet_executive/p3.rs::insert_intersection_topology` inserts the retained
  solver geometry and corner topology. It does not discover a new intersection
  between different blends' faces.
- The normal merge has `global_merge=false`, reset by
  `fillet_executive/p2.rs::reset_parameter_state`. Ordinary planar End ribs do
  not enable the multiple-end-face global merge.
- `session/trim_at_creases.rs::trim_sheet_into` intersects a blend sheet against
  target faces. It does not discover intersections between the new sheet faces.

The adjacent selected top edges in this reproduction have different input
vertices. Their end ribs meet inside the earlier vertical chamfer face, and their
rails cross on the original top. A new shared edge must be discovered, inserted,
and used to trim both new faces and the surviving original faces. Independent
per-input-vertex End events cannot supply that topology.

The overlapping-band guard is a refusal check, not that construction. It also
excludes projections onto the selected edges' endpoints; these chamfer ends
meet beyond the shortened edges' endpoints. Removing or loosening a guard would
not insert the shared edge. Selecting the four extra short top edges instead
changes the requested operation. Reordering saved features or introducing a
box-specific half-space path would bypass the missing event.

The existing `wr460_untrimmed_rails.rs` sequential case selects all eight edges
of the top loop after chamfering the verticals, then rounds them. It does not
cover the four-edge selection here.

A correct implementation needs the generic blend to discover and trim the
interaction between distinct End events. No production change or golden blessing
is justified by the evidence above alone.


## Resolution (2026-11-27)

The half-space argument above also gives a construction from existing events. Chamfered
one at a time, the same picks build: two passes, the edges at `y = 0` and `y = 20` first,
then the other two, give the closed 14-face solid of 7704 mm3 at `s = 2`, and match
`7840 - 32 s^2 - 4 s^3 / 3 + 8 (s - 1)^3 / 3` at `s = 1.2` and `1.5`. In each second-pass
End the chamfer plane meets the first pass's chamfer along their common line, which is the
missing shared edge. `blend_corner/crossing_ends.rs` plans such passes for straight convex
picks between planes whose bands reach past each other across an unchamfered edge joining
their End vertices (each running away from the other's band, so the overlap is the
corner's), and `solidcore-ops` runs the passes with the ordinary chamfer pass.
