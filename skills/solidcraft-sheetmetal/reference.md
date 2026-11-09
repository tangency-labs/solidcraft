# SolidCraft sheet metal reference for agents

Command shapes for sheet metal, the segment contract, the bend arithmetic the engine uses, the
rule values a shop supplies, the inference recipes for FR-17 to FR-20, the traps and one worked
example. The example runs in CI (`sheetmetal_skill_example_builds_as_documented` in
`crates/mcp/src/tests.rs`), so the numbers here stay honest.

Lengths are mm, angles are degrees in expressions (`"90 deg"`). Any numeric field that takes an
expression can name the design's parameters; `segments[].from` and `segments[].to` are plain
numbers (fractions 0…1), so an expression there is refused.

## Commands

### `sheet.manage_rules`
```json
{"command":"sheet.manage_rules","params":{"name":"AL2","thickness":"2 mm","k_factor":0.5,
 "bend_radius":"1 mm","relief_width":"2 mm","relief_depth":"1 mm","gap":"0.5 mm","active":true}}
```
Keys: `name` (new or existing), `thickness`, `k_factor`, `bend_radius`, `relief_width`,
`relief_depth`, `corner_relief`, `hem_gap`, `gap` (expressions; `Thickness` inside a rule means
that rule's thickness), `corner_relief_shape` (`notch`|`round`|`rectangular`|`tear`),
`angle_display` (`bend`|`included`), `active`, `delete`.

- `k_factor` is validated to 0…1, `bend_radius` to at least 0.
- The default rule is Steel (mm), 2.5 mm thick, K 0.44, bend radius = thickness, relief width =
  thickness, relief depth = thickness × 0.5.
- A flange refuses `thickness`, `k_factor` and `bend_radius`: they belong to the rule.
- `sheet.rules` lists the rules and the active one.

### `sheet.flange`
Base: `sketch`, `profiles?`, `rule?`, `flip?`, `name?`, `body_name?`.
Contour: `sketch`, `curves` (an open chain of lines), `distance`, `rule?`, `flip?`, `reverse?`.
Edge: `edges`, `height`, `angle?` (default 90°), `radius?` (default: the rule's), `height_from?`
(`outer`|`inner`|`tangent`), `position?` (`inside`|`outside`|`middle`|`material_outside`|
`tangent`), `flip?`, `body?`, `corner_relief_shape?`, `corner_relief?`, `then?`, `name?`,
`body_name?`.

An `edges` entry is `[x,y,z]` on a top or bottom edge, `{x,y,z}`, or
`{point:[x,y,z], angle?, height?, segments?}` for an edge with its own values. `then` lists up to
16 further bends `{angle?, height, flip?}`, each on the far edge of the one before.

```json
{"command":"sheet.flange","params":{"type":"edge","edges":[{"point":[100,0,2],"segments":[
 {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm"},
 {"from":0.5,"to":0.75,"angle":"90 deg","height":"15 mm"},
 {"from":0.75,"to":1,"angle":"45 deg","height":"15 mm","flip":false}]}],
 "height":"20 mm","angle":"90 deg","position":"inside"}}
```

### `sheet.path_flange`, `sheet.hem`, `sheet.fold`
- `sheet.path_flange`: `edge` or `sketch`+`curves` (a chain of lines across the edge, each line one
  flange and each corner one bend), `radius?`, `body?`.
- `sheet.hem`: `edges` (1…1000 points), `length` (to the outer face), `gap?`, `flip?`, `body?`.
- `sheet.fold`: `line` (sketch + curve) or `points` (`[[x,y,z],[x,y,z]]`), `angle?`, `radius?`,
  `position?` (`centerline`|`start`|`end`|`mould`), `fixed?`, `flip?`, `body?`.

### `sheet.unfold`, `sheet.refold`, `sheet.convert`, `sheet.perforate`
- `sheet.unfold {"body":"..."}` unfolds every bend; `sheet.refold` folds it back.
- `sheet.convert`: `body`, `face` (a point on the large flat face or its name), `rule?`, `bends?`.
- `sheet.perforate`: `diameter`, `pitch`, `margin?`, `pattern?` (`staggered`|`square`), `body?`,
  `sketch?`+`profiles?`; holes go in the flat pattern, the DXF and the volume, not the B-rep.

### `sheet.flat_pattern`, `sheet.export_dxf`, `sheet.export_bend_table`
- `sheet.flat_pattern {"body":"..."}` answers `flat_size_mm` (`[x,y,thickness]`), `outline`,
  `cutouts`, `cutout_shapes`, `pockets`, `bends` (`bend`, `angle_deg`, `included_angle_deg`,
  `radius`, `direction`, `allowance`, `length`, `start`, `end` in flat coordinates),
  `bend_zones`, `perforations`, `bores` (holes drilled along a panel into its rim) and the unfolded
  body.
- `sheet.export_dxf`: `body?`, `path?`, `curves?` (`exact`|`arcs`), `text?` (`cut`|`etch`),
  `bend_notes?`, `note_height?`, `bend_lines?`, `layers?` (`roles`|`single`), `per_body?`.
  Layers: `OUTLINE`, `CUT`, `TEXT`/`ETCH`, `BEND_UP`/`BEND_DOWN`, `BEND_NOTES`,
  `POCKET_<BOTTOM|TOP|INTERNAL>_Z<from>_TO_<to>_MM`, `PERFORATION`.
- `sheet.export_bend_table`: `body?`, `format?` (`csv`|`json`), `path?`. Each row gives the bend's
  angle, direction, radius, allowance, length and its flat centre line, so an agent can check
  where a bend landed.

### Inspection
- `inspect.measure`: `bodies?`, `names_only?`, or `items` (faces and edges) for lengths, areas,
  volumes, diameters, distances, centre distances and angles between two faces or edges.
- `inspect.interference {"bodies":[...]}`: pairs that overlap, with volume, area and bbox.
- `inspect.clearance {"bodies":[...],"against":[...],"clearance":...}`: gaps and overlaps on exact
  faces.
- `inspect.parameter_sweep {"parameter":"H","from":15,"to":30,"steps":10}`: rebuilds in a copy of
  the design and reports what fails or warns at each value; the design is not changed.

## The segment contract

`segments` splits one picked edge's bend into stretches. Each is
`{from, to, angle?, height?, flip?}`; no other key. `from` and `to` are unit-less fractions of the
stretch the sheet still has there after cut-outs, `0` one end and `1` the other. A value left out
falls back to the edge's own value, then the feature's.

- **Partition only:** the first `from` is 0, the last `to` is 1, each `from` equals the `to` of the
  one before, and each `from` < `to`. The segments tile the edge; leaving material out is a
  set-back (FR-14), not a segment.
- **Junction tolerance:** "equals" means within `SEGMENT_JOIN_EPS` = 1e-9 (`crates/doc/src/sheet.rs`),
  the same number the build merges matching stretches across and the same number it snaps a first
  `from` or a last `to` inside to the edge's ends. One number on both sides, so every list the
  command accepts tiles the edge with no gap and no overlap: stretches that match in angle, height
  and side build the bend a whole-edge flange builds, and stretches that differ build one bend
  each. A wider gap is refused.
- **Every segment bends:** each stretch becomes a bend with its own angle, height, setback and
  side, so a bend that changes angle, height or side along one edge is one feature. Consecutive
  stretches that match in angle, height and side merge into one bend. There is no flat stretch
  between segments.
- **Junctions are snapped shut:** a junction within `SEGMENT_JOIN_EPS` is snapped to the stretch
  before it whatever the two stretches' angle, height or side are, so no interior gap survives into
  trimming or the flat pattern; only the 1e-12 attribute comparison decides whether they are one
  bend. A list 5e-10 apart with different heights builds the same part as the exact partition.
- **One straight line, whole:** a split edge's fractions run along a straight line of the outline
  and the whole stretch must lie on it. A cut-out that shortened the line is fine (the stretch left
  is still one line); a notch or a step between two collinear pieces is not, and is refused.
- **Direction:** fractions run from the end of the edge where the sheet's outline enters it, along
  the outline's own direction (counter-clockwise in the flat pattern's coordinates). An
  asymmetric partition therefore depends on it: check `sheet.export_bend_table`'s flat centre
  lines, or keep the partition symmetric, and swap the fractions if the bends land mirrored.
- **Junctions:** no bend relief is cut where two segments meet and no corner is closed there; the
  two bends butt along the junction line. Only the outermost segments take part in corner closing
  and end relief. Where the two stretches bend to opposite sides the blank is cut along their
  junction, from the inner bend line out to the nearer stretch's far edge, in the flat pattern and
  the DXF: a blank joined there cannot fold, one stretch up and the next down. Stretches at
  different angles can have different setbacks, depending on the rule's position (`inside` sets
  the bend line back, `outside` does not), so their bend lines can be offset; the cut starts at the
  inner of the two and so covers the strip between them, over which the folded part separates, and
  its outer end is the nearer far edge, past which only the longer stretch's material remains.
  Where a cut-out took that
  far edge away, the cut is clipped to the material that is left: it runs from the bend line to the
  nearest boundary the junction line meets (a notch's edge, for one), so the blank is parted as far
  as its remaining material allows; a line and a circular arc are met exactly, so a round notch ends
  the cut on the arc itself, which the DXF splits there. Only the stretch of the line from the bend
  line out to the junction's own tip belongs to that junction: past the tip the line runs on through
  material the bend never reaches (another arm of a C-shaped blank, for one), and that material keeps
  no cut. A junction with no material left outward of
  the bend line has nothing to part and is left alone. The slit is zero-width, so the DXF carries
  two coincident outline lines there: a cutter may follow both, so check the toolpath before
  cutting and tell the shop. Same-side stretches stay
  one piece. A junction the command accepts within `SEGMENT_JOIN_EPS` is snapped shut, and a
  stretch narrower than that tolerance is no bend of its own: the run before it takes that stretch
  (its angle and height do not build), so the bends still tile the edge with nothing left uncovered,
  every span the build makes runs forwards, and none reaches the flat pattern reversed. The same
  holds for stretches at the very start, with no run before them yet: they are absorbed by the run
  that follows them, and that run still starts at the edge's start (0), so the first bend covers the
  edge from 0 and the last ends at 1 however many stretches were absorbed. A junction that overlaps,
  not just one that gaps, is snapped the same way: a stretch that starts below the covered end starts
  where the edge is already covered, so the runs are sorted, contiguous and never overlap and the
  part is the partition's.
- **Refused:** `segments` on a base or contour flange; a split edge whose stretch leaves one
  straight line of the outline (a cut-out that shortened the line is fine, a notch or a step across
  it is not); `then` with `segments` (the Flange dialog refuses it and names the edge, rather than
  dropping the chain); more than 64 segments (`MAX_SEGMENTS = 64`). Each bend costs two flat pieces
  and the flat budget is 4096 (`MAX_FLAT_PIECES`), so a design holds at most 2047 bends.

The refusals are the contract; quote them when a call fails:

| Wrong | Message |
|---|---|
| `segments` on a base or contour flange | `segments is only for edge flanges (a base or contour flange has no edge to divide)` |
| `segments` beside `edges` | `` `segments` belongs inside an `edges` item: edges [{point, segments}] `` |
| not a list | `edges[i].segments must list segments {from, to, angle?, height?, flip?}` |
| count | `edges[i].segments must list 1…64 segments` |
| unknown key | `edges[i].segments[j]: unknown key k (a segment takes from, to, angle, height and flip)` |
| fraction missing or out of 0…1 | `edges[i].segments[j].from must be a fraction from 0 to 1` |
| `to` not above `from` | `edges[i].segments[j].to must be more than its from` |
| first `from` not 0 | `edges[i].segments[0].from must be 0: the segments cover the whole edge` |
| last `to` not 1 | `edges[i].segments[n].to must be 1: the segments cover the whole edge` |
| gap or overlap | `edges[i].segments[j].from must equal the to of the segment before it (the segments must meet, with no gap or overlap)` |
| `flip` not a bool | `edges[i].segments[j].flip must be true or false` |
| angle outside 0…180° | `edges[i].segments[j]: the flange angle must be between 0 and 180°` |
| height below the bend's minimum | `edges[i].segments[j]: the flange height must be at least X mm for this bend` |
| `then` with `segments` | `not supported yet: then on a segmented edge (bend each segment's far edge in its own feature)` |
| the Flange dialog's `then` with a split edge | `edge N is split into segments: a split edge takes no `then`; bend each segment's far edge in its own feature` |
| a split curved edge | `not supported yet: segments on a curved edge (pick a straight edge of the sheet)` |

## Bend arithmetic

With bend angle θ (radians), inner radius R, thickness T and K-factor k:

- bend allowance: `θ · (R + k · T)` (`allowance` in the doc layer).
- setback from the edge for `position` `inside` (default): `(R + T) · tan(θ/2)`. `outside`: 0;
  `middle`: half of inside; `material_outside`: `R · tan(θ/2)`; `tangent`: `R + T` at 90° and
  above, `(R + T) · sin θ` below.
- leg (the flat panel past the bend): the height less what sits before it, which is
  `(R + T) · tan(θ/2)` for `height_from` `outer`, `R · tan(θ/2)` for `inner`, and `R + T` for
  `tangent` past 90°.
- flat size: the base less its setback, plus the allowance, plus the leg. The folded volume is the
  flat area times the thickness.

For the AL2 rule above (T 2, k 0.5, R 1) at 90°: allowance π = 3.141592653589793, setback 3, so a
20 mm flange has a 17 mm leg. At 45°: allowance π/2, setback 3·tan(22.5°) =
1.2426406871192852, and a 15 mm flange has a 13.757359312880714 mm leg.

The worked numbers here and the volume check (flat area times thickness) assume the rule's K-factor
is 0.5, as the example rule's is: with another K the allowance, the flat area and the volume it
implies all change.

## Rule values and the unfold method

Ask the shop for the rule: thickness, K-factor (or a 90° deduction or allowance), bend radius,
relief width and depth, the gap between flanges that meet at a corner, and the corner relief. Never
invent a K-factor: it depends on the material, the tooling and the grain direction.

Today's build unfolds every bend from the rule's K-factor. OPEN DRAFT PR #9911 adds
`unfold_method` (`KFactor`|`Deduction`|`Allowance`) and a 90° deduction or allowance value per
rule, and derives K = (2·BA90/π − R)/T; it is not in the build from this branch. When it lands, a
rule can take a shop's chart value instead of a K-factor, and the skill's arithmetic above stays
the same because the engine still unfolds from one K.

## Corners and relief

- Bends of one feature that meet at a corner of the base are closed toward each other, with the
  rule's `gap` between them, and the corner relief the rule (or the feature's
  `corner_relief_shape`/`corner_relief`) gives.
- Two bends of one split edge never close toward each other at a junction.
- Bends of separate features do not close toward each other.
- A bend that ends beside more of the base gets the rule's bend relief there; a bend narrower than
  the relief width gets none, with a warning.

## Flat pattern and exports

Before handing a part to a shop, check:

1. `sheet.flat_pattern`: the flat size fits the stock; the outline and cut-outs are what you
   planned; each bend's length, angle and direction match the plan.
2. `sheet.export_bend_table {"format":"json"}`: one row per bend, with its flat centre line; the
   allowances follow from the rule.
3. `inspect.interference` and `inspect.clearance` against the mating bodies: zero overlap, the
   rule's gap or more.
4. `sheet.export_dxf`: layers and bend notes; `bend_lines: false` when the cutter would cut them.

## Inference recipes (FR-17 to FR-20)

### Segment inference (FR-17)
1. Take the intent: what the part must do at that edge (stiffener positions, clearances, mating
   faces, where the load is).
2. Decide how many bends the edge carries and where their boundaries fall, in mm along the usable
   stretch. Convert to fractions from the direction rule above.
3. State the assumption, the confidence (high when the intent gives dimensions, medium when it
   gives a count or a spacing rule, low when inferred) and one alternative partition with its
   reason.
4. Check each stretch's length and each height against the bend's minimum, then build one flange
   feature and verify the bend table's lengths and angles.

### Angle inference (FR-18)
1. Propose one angle with a confidence: high when the mating geometry fixes it (measure the faces
   or the gap), medium for a standard shop angle (90°, 45°, 135°), low when inferred.
2. Give at least one alternative and why it is worse (a taller leg, a clash, a non-standard tool).
3. Refuse when the geometry contradicts the words: say what you measured (for example the mating
   face is at 45° from the base, so a 90° bend cannot sit flush) and ask, rather than bending.
4. Verify with `inspect.clearance`/`inspect.interference` and the bend table.

### Natural language (FR-19)
| Phrase | Parameters |
|---|---|
| "90 degree flange 20 mm high on the long edge" | `{"type":"edge","edges":[[100,0,2]],"angle":"90 deg","height":"20 mm"}` on the long edge's midpoint |
| "fold the back up 20 mm at 90° except the last 50 mm, which stays at 45°" | `segments` tiling the edge: the last stretch at `"45 deg"`, height as stated or asked |
| "two stiffeners side by side on that edge, 15 mm high" | two segments with `height":"15 mm"` and the split where the intent puts it |
| "make it from 1.5 mm aluminium with K 0.44" | `sheet.manage_rules` with `thickness` and `k_factor`, then the flange |
| "hem the top edge 8 mm" | `sheet.hem {"edges":[[...]],"length":"8 mm"}` |
| "unfold it and give me the flat" | `sheet.unfold`, then `sheet.flat_pattern` |

Read the numbers back to the user when the phrase is ambiguous (a height with no datum, a direction,
which edge is "the long edge" on a near-square part) before building.

### Multi-flange optimisation (FR-20)
1. State the objective: zero `inspect.interference` volume between the sheet and the mating bodies,
   `inspect.clearance` at least the rule's `gap`, the flat within the stock, every radius at or
   above the rule's.
2. Variables: segment fractions, angles, heights and sides. At most 8 candidate sets; use
   `inspect.parameter_sweep` for one parameter over a range, and `undo` to roll back a build.
3. Score every candidate and keep the numbers; stop when the objective is met or the 8th candidate
   is scored.
4. Report the winner, its margin over the runner-up and every iteration's objective values. Never
   claim an improvement without the numbers.

## Traps

- **Fractions need the direction:** an asymmetric partition can land mirrored. Check the bend
  table's centre lines against the plan, or keep the partition symmetric.
- **Segments are not set-backs:** they tile the edge; a gap is refused.
- **Deleting a stretch in the Flange dialog keeps the edge whole:** the gap the removed stretch
  leaves is bridged from the stretches beside it, never from the removed row (its fields may be
  mid-edit and empty): the stretch before it reaches to where the stretch after it starts, or to
  the edge's end when none follows. A boundary the user did not touch does not move, and removing
  the only stretch left makes the whole edge. The dialog never submits a list with a gap.
- **Per-edge versus per-segment:** `angle` and `height` at the edge entry are that edge's values; a
  segment's own values override them; the feature's values are the last fallback.
- **A named stretch keeps its name with its material:** redefining a split edge after a stretch is
  deleted or merged keeps the surviving stretch's own model parameter name (the name on the value it
  still has; failing that, the name whose span it still covers), and the deleted stretch's name goes
  away. Expressions that read the survivor's name still read its height; a deleted stretch's name
  never lands on a stretch whose value came from elsewhere. A segment input is keyed by its edge's
  pick point, the physical edge, not by the edge's place in the list: reordering the `edges` list
  keeps every name on its own edge's stretch. A pick point that moves by a hair still names the same
  edge: two picks within the document's pick tolerance (`appearance::pick_tolerance`, 1e-6 mm where
  no body applies) are the same edge, so redefining with a nudged pick keeps the edge's and its
  stretches' names; a pick that moved further still takes the nearest edge point on the same feature
  within a sane bound, and a genuinely different edge keeps its own name.
- **`flip` is per segment:** it replaces the feature's `flip` outright for that stretch: `false` is
  the default direction and `true` the other one, whatever the feature's `flip` is. It is not a turn
  relative to the feature.
- **`then` is refused with `segments`:** bend each segment's far edge in its own feature. The
  Flange dialog says the same, naming the edge: `edge N is split into segments: a split edge takes
  no `then`; bend each segment's far edge in its own feature`.
- **Edge picks** are plain numbers on a top or bottom face at the sheet's thickness; a pick on the
  bottom face flips `top`. The pick snaps to the nearest edge.
- **`segments` on a base or contour flange** is refused: there is no edge to divide.
- **The flat budget:** 4096 flat pieces, two per bend, so at most 2047 bends.
- **A bend narrower than the relief width** gets no relief and a warning.
- **The flat pattern is refused** once a feature that is not sheet metal changed the body after
  its sheet metal features: remove or suppress it, or model that shape with sheet metal features.
- **`Thickness` inside a rule** is that rule's thickness, not the design's.

## Worked example: a request turned into one flange feature

Request: "Make it from 2 mm steel, K 0.5, bend radius 1 mm. Base 200 × 100 mm. Fold the back edge
up 20 mm at 90°, except the last 50 mm, which steps down to 45° at 15 mm."

Mapping: the back edge is the 200 mm edge from (0,0) to (200,0); its midpoint is `[100,0,2]`; the
first 150 mm is one bend at 90°/20 mm, the last 50 mm another at 45°/15 mm, so two segments, with
the boundary at 0.75 of the stretch.

```json
{"commands":[
 {"command":"sheet.manage_rules","params":{"name":"AL2","thickness":"2 mm","k_factor":0.5,
  "bend_radius":"1 mm","active":true}},
 {"command":"sketch.create","params":{"plane":"XY","name":"Base"}},
 {"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[200,100]}},
 {"command":"sketch.finish","params":{}},
 {"command":"sheet.flange","params":{"sketch":"Base"}},
 {"command":"sheet.flange","params":{"type":"edge","edges":[{"point":[100,0,2],"segments":[
  {"from":0,"to":0.75,"angle":"90 deg","height":"20 mm"},
  {"from":0.75,"to":1,"angle":"45 deg","height":"15 mm"}]}],
  "height":"20 mm","angle":"90 deg","position":"inside"}}
]}
```

Expected, by hand:

- Flat area 23275.514645666284 mm²; folded volume 46551.02929133257 mm³.
- Flat size 200 × 117.1415926535898 × 2 mm; the 45° part's flat edge reaches 114.08551495255631 mm,
  and the base's flat edge steps by 1.7573593128807148 mm at the junction.
- `sheet.export_bend_table {"format":"json"}`: two bends, 150 mm at 90° and 50 mm at 45°, allowances
  π and π/2, both up; their flat centre lines show which end of the edge the fractions started
  from.
- `inspect.measure` must report 46551.03 mm³ within 0.01 %; anything else means the bend did not
  build as planned.

The 3-segment variant in the command section (90°/20 mm, 90°/15 mm, 45°/15 mm at 0.5 and 0.75)
gives three bends of 100, 50 and 50 mm and a volume of 46051.02929133257 mm³: the 15 mm stretch's
leg is 12 mm, not the 17 mm a 20 mm stretch has, so the area is 250 mm² smaller.
