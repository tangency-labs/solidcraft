# Narrow curved tips: distance-angle chamfer refusal without a fit

Issue: SolidCraft #19108. Diagnosis only; this record does not fix the issue.

## Exact geometry

The reported extrusion is 10 mm high. The selected curved top edge ends tangent to
the bottom outline across an unselected terminal segment 0.5806010928961749 mm long.
Both adjacent top rims are selected. The first distance is along the upward-facing
top face (`crates/kernel/src/blend_smlib.rs`, `sides`). At a right wedge, the second
distance is `d tan(angle)` (`surface/procedural_blend/chamfer.rs`, `setbacks`).

At d = 1 mm and 80 degrees the vertical setback is 5.671281819617707 mm, less than
the wall height. The two opposing top setbacks occupy 2 mm across the terminal strip,
which is only 0.5806010928961749 mm wide. At d = 0.5 mm they occupy 1 mm; at d = 0.2 mm
they occupy 0.4 mm. The limiting face is the planar top, not the vertical wall.
The terminal contact event alone bounds the ideal equal top inset by half that width,
0.29030054644808745 mm. This is a local upper bound, not a proof of the maximum over
all 34 selected edges. The reporter's design remains outside the repository.

Overlapping chamfer bands are a refusal of the current blend construction
(`docs/generic-blend.md`, section 3.3). Producing the requested scalloped surface
would require constructing and merging intersecting bands. Removing the refusal is
not a geometry fix.

## Independent synthetic reproduction

The following outline has the same narrow terminal strip, with original round numbers.
The upper quadratic has control points (10, 0.6), (5, 0.6), (0, 5), all weights 1.
Its terminal tangent is horizontal. At that end its inward normal opposes the bottom
line's, so two equal top offsets first touch locally at 0.6 / 2 = 0.3 mm.
The terminal top edge is deliberately unselected.

```json
{"commands":[
  {"command":"sketch.create","params":{"plane":"XY"}},
  {"command":"sketch.line","params":{"points":[[0,0],[10,0],[10,0.6]]}},
  {"command":"sketch.conic","params":{"start":[10,0.6],"apex":[5,0.6],"end":[0,5],"rho":0.5,"connect":false}},
  {"command":"sketch.line","params":{"points":[[0,5],[0,0]]}},
  {"command":"sketch.finish","params":{}},
  {"command":"solid.extrude","params":{"distance":10}},
  {"command":"solid.chamfer","params":{"edges":[[5,0,10],[5,1.7,10]],"distance":1,"angle":"80 deg","tangent_chain":false}}
]}
```

Observed with the installed SolidCraft 0.44.1 CLI, `run SCRIPT --validation off --results`:

| Distance / angle | Exit | Result |
|---|---|---|
| 1 mm / 80 degrees | 1 | `not supported yet: the chamfer is larger than the faces beside its edges allow` |
| 1 mm / 45 degrees | 1 | overlapping bands; edge (5, 0, 10); largest distance 0.300523 mm |
| 0.2 mm / 80 degrees | 0 | builds |

The original design with the original 34 picks also reproduces the bare 80 degree
refusal in that CLI. It warns that its reader supports format 58 while the design is
format 60; the synthetic reproduction has no saved-format dependency.

Targeted external checks against the three synthetic scripts:

- `test_distance_angle_refusal_names_edge_distance_and_face`: FAIL, the 80 degree
  refusal has no edge, distance or limiting face.
- `test_equal_setback_limit_does_not_exceed_terminal_contact`: FAIL, the named
  0.300523 mm exceeds the ideal terminal contact value 0.3 mm. It must not be treated
  as an analytic maximum or reused to justify a new distance-angle fit.
- `test_small_chamfer_builds`: PASS at 0.2 mm / 80 degrees.

These checks ran against the released CLI, not a fresh build of this worktree.
Renditions of the synthetic input and the 0.2 mm control were captured with
`solidcraft-cli snapshot`, 1280 x 720, isometric, without axes or grid, and inspected.
No renderer output or reporter design is committed.

## Why the diagnostic loses its size

`session/fit_size.rs::bands_meet_at` requires both solvers to roll a constant ball:
equal positive offsets. An 80 degree chamfer has unequal offsets, so it returns None.
`session.rs::band_scale` only sizes chamfers between parallel straight edges of a
plane. Its `line` classification rejects the curved edge, so it also returns None.
`p3.rs::refuse_overlapping_bands` consequently records no fit. Ops maps this to
`Refusal::NoRoom`, and the application displays the unsized message.

The 45 degree path is not a certified replacement for the missing scale construction:

- `bands_meet_at` uses 32 stations followed by a local golden-section minimum, not a
  global enclosure of all contact events.
- `ball_through_at` excludes contacts whose centre projects onto an edge endpoint.
  This terminal contact is precisely such an event.
- `creation_bands_meet` considers pairs sharing a vertex and the original edge
  intervals; the band checker uses kept rail intervals and excludes adjacent pairs.

The section equations can be generalized to a common scale of each solver's original
offset pair, without rebuilding bodies. That alone does not prove the global least
scale, endpoint events, the kept intervals, or the geometric margin. Replacing the
parallel-line construction with the existing sampled search would weaken its stated
exactness guarantee. Removing the line restriction and scaling distances measured at
one rail position would be wrong: curved rails move their feet as the size changes.

## Remaining feature work

A correct diagnostic needs one contact-event construction on the exact support
definitions, covering interior and endpoint contact over the kept intervals, with a
global bound and the fit margin derived in geometric units. It must retain the
existing exact parallel-line event and avoid building candidate solids to search.
For distance-angle input, the named scalar is the first distance at the fixed angle;
for two-distance input it is a scale of both distances.

The common limiting face can use `ErrorInfo.side_face` and `FilletSession::error_site`.
`record_no_room` sets `no_room_without_site`, so a genuine site must clear it after
recording the refusal. Ops' `SessionRefusal` / `Refusal::TooLarge` currently discard
the face, and the application's `TooLarge` error has no face field. That metadata
must reach the user-facing refusal too; setting the kernel field alone is insufficient.

No production fix or failing Rust test is retained. No tolerances, expectations,
goldens, locale catalogs, commands or design formats changed. A prototype Rust
regression was queued through buildq, then cancelled while waiting (52 jobs ahead)
when the investigation established this construction gap. There are no changed Rust
crates requiring test, formatting or clippy verification in this diagnosis-only change.
