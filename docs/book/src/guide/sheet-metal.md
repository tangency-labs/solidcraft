# Sheet metal

MODEL → MODIFY offers Convert to Sheet Metal. The SHEET METAL tab is always available (it is highlighted
and selected when the active body is a sheet), and builds parts from a sheet of constant thickness under a **sheet metal rule**
(thickness, bend radius, K-factor), set in **Sheet Metal Rules** (`sheet.manage_rules`).

- [**Flange**](../reference/commands.md#command-sheet.flange): a base flange from a sketch profile, a contour flange from an open
  sketch of lines and circular arcs, or an edge flange from a part edge. A contour flange along an ellipse, a conic or a
  spline is not supported yet ([#15743](https://github.com/tangency-labs/solidcraft/issues/15743)): the free-form panel it
  would make has no closed-form flat, so the flange is refused, naming the curve.
- [**Hem**](../reference/commands.md#command-sheet.hem) returns a sheet edge back toward the sheet; no new sketch is required.
- **Fold** (`sheet.fold`) bends the sheet along a sketch line.
- **Unfold / Refold** (`sheet.unfold`, `sheet.refold`) for working on the flat part.
- **Convert** (`sheet.convert`) turns surfaces into sheet metal: the faces of a solid or of a
  surface body. Every face becomes a panel, however thin the body (a thin plate's six faces are
  six panels, parted by rips), with no face fixed; sharp edges are cut open but the ones picked
  as bends. The sheet is the rule's, as your tooling makes it: the rule's thickness, its bends at
  the rule's radius and its corners with the rule's reliefs, whatever a vendor's file has. A
  cylinder modeled between two flat faces is a bend, at the rule's radius, while its inside
  radius is short of 10^0.7 (about 5) of the rule's thicknesses, and a roll from there; other
  cylinders and cones are rolls. A roll keeps its own radius. The line between the two is where
  DIN 6935's correction of the neutral fiber, 0.65 + 0.5·lg(R/T), reaches 1; bends and rolls
  alike are developed with the rule's K-factor. A roll's flat pattern has its lines where it
  starts and ends and halfway between, and the DXF draws them on a `ROLL` layer. The sheet is
  every face of the body but the ones skipped (`skip`), or the faces picked (`face`, `faces`) and
  every face a bend joins to them. A surface body has no thickness of its own: its faces are the
  sheet's surface, with the rule's thickness on the side `direction` says. Panels whose material
  would overlap (a thin block's two sides, each the rule's thickness in) are refused, naming them.
  A free-form (B-spline) face is not supported yet
  ([#15743](https://github.com/tangency-labs/solidcraft/issues/15743)): its flat has no closed
  form, so it is refused, naming where it is. Skip it to leave it out.
- **Recognize Sheet Metal** (`sheet.recognize`) takes a body that is sheet metal already (a
  shelled part, a plate, a part from another system, of one even thickness) as one sheet, read
  from one of its sides (see **Recognize Sheet Metal** below).
- **Create Flat Pattern** (`sheet.flat_pattern`) shows the flat part with its bend lines and a
  bend table, and exports it as DXF or a bend-table CSV through the save picker.

Bends follow the rule's K-factor, so flat lengths match what a press brake will make. Tests
check flat lengths against hand-computed bend allowances.

## Step by step

A tray: a plate with a flange up each long side, and its flat pattern.

**1. Base flange.** Sketch an 80 × 50 mm rectangle on the Top plane and finish the sketch. On the
**SHEET METAL** tab, click **Flange**, then the profile, then **OK**: a plate at the rule's
thickness.

<video src="../media/sheet-metal/base.mp4" autoplay loop muted playsinline controls width="100%" aria-label="A base flange from a sketched rectangle: Flange, click the profile, OK"></video>

**2. Edge flanges.** Click **Flange** again, then the plate's two long top edges. Type the height
(20) and press Enter. Each flange bends up at the rule's radius.

<video src="../media/sheet-metal/flange.mp4" autoplay loop muted playsinline controls width="100%" aria-label="Edge flanges 20 mm high from the plate's two long top edges"></video>

**3. Flat pattern.** **Flat Pattern** shows the part laid flat, with a dashed line for each bend,
its flat size and a table of bends to export as DXF or CSV.

<video src="../media/sheet-metal/flat.mp4" autoplay loop muted playsinline controls width="100%" aria-label="Create Flat Pattern: the tray laid flat with its two bend lines, flat size and bend table"></video>

## Rules and tools

A rule is saved in its design, and a new design starts with only the default steel rule. To use
the same rules in every design, keep them as **personal rules**, saved in your profile: open
**Personal rules** at the top of Sheet Metal Rules.

- **Save new** keeps the rule the manager shows under a name: its thickness, K factor, bend
  radius, reliefs, gaps, corner relief shape and angle display. A bare number is saved with the
  design's unit, so the rule makes the same sheet in a millimetre or an inch design. `Thickness`
  and other expressions are kept as typed, and are worked out in the design the rule is used in.
- Picking a personal rule fills the manager with it: the design's own rule of that name, or else
  a new rule. **OK** adds it to the design (or updates that rule); picking alone changes nothing.
- The save icon beside the box saves the manager's values over the picked personal rule.
  **Rename** and **Delete** change the personal rule only, never the rules already in designs.

A design keeps its own copy of each rule it uses, so it builds the same on a computer without
your personal rules. The profile keeps up to 64 of them.

A copy of a sheet metal body is sheet metal too. **Mirror** (`solid.mirror` with `bodies`)
makes the opposite hand: its own flat pattern, DXF and bend table, the mirror image of the
original's flat, with the same thickness side, bends and flat lengths, and holes where the
mirror puts them. A copy from **Move** (`copy: true`) or a body pattern keeps the original's
flat pattern. Every copy takes further sheet metal features of its own. Mirroring with
`combine` joins the two halves into one solid, which has no single sheet model yet; mirroring
or patterning flange features themselves is not supported yet.

An edge flange's `height` runs along the flange to its far edge. Where it is measured from is
the feature's `height_from` (Height From in the Flange dialog): `outer` (the default, Outer
Sharp) starts at the outer virtual sharp, where the lines of the base's and the flange's outer
faces meet, so (R + T)·tan(θ/2) of it is bend before the flat leg starts (θ the bend angle, R
the inner radius, T the thickness); `inner` (Inner Sharp) starts at the inner virtual sharp,
R·tan(θ/2) before the leg; `tangent` is for bends past 90°, where the outer sharp runs far
out: it starts at the line square to the flange that touches the outside of the bend, R + T
before the leg. Up to 90° `tangent` is the outer sharp (at 90° the two are the same line, the
base's outer face, so the leg does not jump as the angle passes 90°). A height shorter than
what lies before the leg is refused with the smallest one that fits. With a 2 mm sheet and a
2 mm radius, a 90° flange 20 high has a leg of 16 from the outer sharp or the tangent and 18
from the inner sharp; at 120° it has a leg of 13.07 from the outer sharp, 16.54 from the inner
sharp and 16 from the tangent.

```json
{"command":"sheet.flange","params":{"edges":[[50,0,2]],"height":20,"height_from":"inner"}}
```

Where an edge flange's bend starts is its `position` (Bend Position in the Flange dialog): how
far inside the picked edge the base is cut back. `inside` (the default) puts the outer virtual
sharp on the edge, (R + T)·tan(θ/2) inside it, so at 90° the flange's outer face is flush with
the edge's face; `outside` (`sheet.flange` also takes `bend_outside` and saves it as `outside`) starts the bend at the edge, which
leaves the base whole and the bend wholly outside; `middle` cuts back half of `inside`;
`material_outside` puts the inner virtual sharp on the edge, R·tan(θ/2) inside it, so at 90°
the flange's inner face is flush with the edge's face and its material is outside; `tangent`
is for bends past 90°: it cuts back R + T, so the outside of the bend is tangent to the edge's
face, touching it a quarter turn into the bend with nothing of the bend past it. At 90° that
is `inside`. Below 90° the outside of the bend never turns parallel to the edge's face, so
`tangent` cuts back (R + T)·sin θ instead: the bend's end is on the edge's face and the leg
leans on out past it.

```json
{"command":"sheet.flange","params":{"edges":[[50,0,2]],"height":20,"angle":"120 deg","position":"tangent"}}
```

An edge flange takes its setback off its own edge only. Where that edge ends at an inside
corner of the base (the base runs on past it, like the leg of an L), the rest of the base
stays, and the rule's **bend relief** (`relief_width` and `relief_depth`; Relief Width and
Relief Depth in Sheet Metal Rules) is cut into it beside the bend's end: a slot the relief
width wide, from the relief depth before the bend line to the relief width past the flange's
far edge in the flat pattern. The bend and the flange are then free of the base, folded and
flat. A relief that can't be cut (a width under 0.01 mm, one as wide as the base beside the
bend, or a slot that would cut the base apart) is left out, and the flange feature reports a
warning saying where.

Where a cut-out has taken part of an edge away (a corner cut off the plate, a notch in the
middle of an edge), an edge flange picked on that edge runs along the stretch the cut-outs
leave nearest the pick, and takes its setback off that stretch only. The base on the other
side of a notch keeps its edge, and a pick there, in the same feature or a later one, makes
a flange of its own. Where the base is still left beside the bend's end at such a cut-out, it
gets the bend relief as above. An edge that cut-outs have taken whole is refused.

The setback is cut out of the base's outline as it was drawn: where the bend line crosses an
arc of the outline (a plate with rounded corners, bent inside or in the middle), the arc stays
an arc and ends there, in the flat pattern and the DXF. A free-form curve (an ellipse, a
spline) in an outline that a setback cuts is followed by tangent arcs within 0.001 mm.

Where two bends of one edge flange feature meet at a corner of the base, the rule's **corner
relief shape** (`corner_relief_shape`; Corner Relief Shape in Sheet Metal Rules) says what is
cut there. `notch`, the default, cuts nothing more: the bends end square at the corner, which
leaves a step in the flat pattern between each bend's end and its flange's closed end. `tear`
cuts no relief either, and no step: both bends run to the corner where the two bend lines
meet, and each bend's end is cut straight, in the flat pattern, from that corner to where its
flange's closed end starts. Folded, the two bends touch at the corner and open to the rule's
gap where the flanges start; the bend table lists a torn bend between the middles of its two
ends. A tear whose cuts would cross in the flat pattern, or that would run one bend into the
other folded (flanges at very different angles), is left out with a warning and the corner
keeps its notch. `round`
and `rectangular` cut a relief the size of the rule's **corner relief** across, centred on the corner where
the two bend lines meet: out of the base a round with half that size as its radius, or a square
with half that size as its side, and both bends stop where it begins, cut straight across. The
flange panels keep their ends. The relief is in the folded part, the flat pattern, the bend
table (the bends are shorter) and the DXF (a round relief is an arc). At a corner that isn't
square the round follows the angle of the base and the square becomes a kite, its two cuts
square to the edges. Changing the shape in a rule changes the parts made with that rule. A
relief that doesn't fit (under 0.01 mm across, leaving nothing of a bend, or reaching as far
from its corner as another edge of the base is) is left out, and the flange feature reports a
warning saying where. The reliefs at all the corners are cut together, so a corner's relief
does not depend on the order the corners are cut in; two of them that would overlap are both
left out, each with a warning saying which the other is.

```json
{"command":"sheet.manage_rules","params":{"name":"Sheet 2 mm","thickness":"2 mm","k_factor":0.5,
 "bend_radius":"1 mm","corner_relief_shape":"round","corner_relief":"8 mm","active":true}}
```

The rule's shape and size are the default. An edge flange feature can choose its own for the
corners its flanges make: `corner_relief_shape` (`notch`, `round`, `rectangular` or `tear`) and
`corner_relief` (the size across, a length or an expression) in `sheet.flange`, or Corner Relief
Shape and Corner Relief in the Flange dialog. In the dialog, Rule as the shape and an empty size
take the rule's; either one left out follows the rule while the other is the feature's. Corners are
relieved only between the flanges of one feature, so a feature's choice never meets another's at a
corner, and two edge flange features of one part can have different reliefs under one rule. A
base or contour flange takes its corners from the rule only and refuses both parameters. The
flange gap and the rule's other values stay with the rule for every flange. The feature's choice is
saved with the design (file format 23).

```json
{"command":"sheet.flange","params":{"edges":[[60,0,2],[120,45,2],[60,90,2],[0,45,2]],
 "height":20,"corner_relief_shape":"round","corner_relief":"6 mm"}}
```

Where the edge flanges of one feature meet at a corner of the base, their panels are closed
toward each other at any bend angle: each end is cut square to the sheet, on a line in the flat
chosen so that the edges nearest each other (on the inner faces) run parallel, the rule's
**gap** apart, either side of the plane halfway between the flanges. Flanges at 72° get slanted
ends in the flat; corners that aren't square close the same way, and at 90° it is the corner
the rule always gave. A corner that can't be closed (a gap under 0.01 mm, flanges bending
opposite ways, ends that would overlap in the flat pattern) stays open, and the flange feature
reports a warning saying where. A flange added by a later feature leaves the corner as it was:
the earlier flange keeps its length and the new one ends at its bend.

The flanges of one edge flange feature can each have an angle and a height of their own. An
entry of `edges` is a point `[x, y, z]`, which takes the feature's `height` and `angle`, or an
object `{"point": [x, y, z], "angle": "105 deg", "height": "30 mm"}` whose `angle` and `height`
(either one, or both) replace them for that edge. They are expressions like the feature's own,
so parameters work, and each is a feature input of its own (`Edge angle`, `Edge height`) after
`Height` and `Angle`. The corners between such flanges close like those between equal ones:
the ends run toward each other the rule's gap apart, either side of the plane halfway between
the two flanges, and a taller flange's end keeps that line up to its own tip. This is how to
close a corner between flanges at different angles, because flanges of separate features
don't close toward each other. In the Flange dialog, picking more than one edge shows a row
per edge with an angle and a height box; an empty box takes the value above.

```json
{"command":"sheet.flange","params":{"edges":[[60,0,2],{"point":[120,45,2],"angle":"90 deg"},
 {"point":[60,90,2],"angle":"105 deg","height":"30 mm"},{"point":[0,45,2],"angle":"120 deg","height":"25 mm"}],
 "height":38.6,"angle":"75 deg"}}
```

One edge can also be split into segments: `segments` lists stretches of the picked edge, each
`{"from": 0, "to": 0.5, "angle": "90 deg", "height": "20 mm", "flip": false}`. `from` and `to`
are fractions of the stretch the sheet still has there (what cut-outs leave), from the end where
the outline enters the edge, so the first `from` is 0, the last `to` is 1 and each `to` is the
next `from`: segments divide the edge, they never leave material out (a set-back is the other
thing). Consecutive segments that match in angle, height and side make one bend; the others make
a bend each, with its own setback, leg and side, so the base's outline steps where their angles
differ and the flange's far edge steps where their heights differ. A segment's `flip` replaces the
feature's `flip` outright: `false` is the default direction and `true` the other one, whatever the
feature's `flip` is, so it is not a turn relative to the feature. No bend relief is cut where two
segments meet, and
where the two stretches bend to opposite sides the blank is cut along their junction, from the inner
bend line out to the nearer stretch's far edge, in the flat pattern and the DXF: a blank still joined
there cannot fold one stretch up and the next down, while same-side stretches stay one piece.
Stretches at different angles have different setbacks, so their bend lines are offset: the cut starts
at the inner of the two, so it covers the strip between the bend lines, over which the folded part
separates. Its outer end is the nearer far edge: past that only the longer stretch's material
remains, so there is nothing of the other stretch left to part. Where
a cut-out took that far edge away, the cut is clipped to the material that is left: it runs from the
bend line to the nearest boundary the junction line meets (a notch's edge, for one), so the blank is
parted as far as its remaining material allows. Only the stretch of the junction line from the bend
line out to the junction's own tip belongs to that junction: past the tip the line runs on through
material the bend never reaches (another arm of a C-shaped blank, for one), and that material keeps
no cut. A straight boundary and a circular one are met
exactly, so a round notch ends the cut on the arc itself and the DXF splits the arc there.
`then` is refused on a split edge: bend each segment's far edge in its own
feature. Stretches that meet within 1e-9 (the junction tolerance, `SEGMENT_JOIN_EPS`) are one
partition: the junction is snapped shut whatever the two stretches' angle, height or side are, so
no gap survives into trimming or the flat pattern, and only stretches that match make one bend. A
stretch narrower than that tolerance is no bend of its own: the run before it takes that stretch
(its angle and height do not build), so the bends still tile the edge with nothing left uncovered
and no reversed span reaches the flat pattern. That includes stretches at the very start of the
edge, with no run before them: the bend that follows them takes them and still starts at the edge's
start, so the first bend covers the edge from 0 and the last ends at 1.
The whole stretch must lie on one straight line of the outline, with no gap in it: a stretch
along a curve is refused (its fractions would only follow chords), a stretch across a notch or a
step is refused too, and a straight edge a cut-out has shortened can still be split because the
stretch that is left lies along the same straight line. In the Flange dialog, each picked edge has
its own **Segments** list, **Split Edge** adds one more equal stretch (or splits the last one on an
edge the design already split), removing a stretch keeps the edge whole (the gap is bridged from
the stretches beside it, never from the removed row, so a boundary the user did not touch does not
move, or the whole edge when it was the only one), and editing a feature keeps every
edge's fractions, angle, height and side, an explicit `flip: false` included. The dialog refuses
`then` on a split edge by name, rather than dropping the chain.

```json
{"command":"sheet.flange","params":{"type":"edge","edges":[{"point":[100,0,2],"segments":[
 {"from":0,"to":0.5,"angle":"90 deg","height":"20 mm"},
 {"from":0.5,"to":0.75,"angle":"90 deg","height":"15 mm"},
 {"from":0.75,"to":1,"angle":"45 deg","height":"15 mm","flip":false}]}],
 "height":"20 mm","angle":"90 deg"}}
```

An edge flange can bend more than once: `then` lists further bends, each an `angle` and a
`height`, and each is a flange on the far edge of the one before it, made in the same command
and undone with one step. It is the way to make a flap with two or more bends that is part of
the same sheet (a Contour Flange is always a sheet of its own). The example bends the end edge of
a wall 45° outward, then 45° again, the second leg 80 mm long. Each bend is a flange feature of
its own in the timeline, so each can be edited there. In the Flange dialog, an edge flange has
**Further Bends** under Bend Position: **Add bend** adds a row with an angle and a height, and the
bin beside a row takes it out.

```json
{"command":"sheet.flange","params":{"edges":[[1,0,20]],"height":40,"angle":"45 deg",
 "then":[{"angle":"45 deg","height":80}]}}
```

A further bend turns the way the one before it did. `"flip": true` on a bend turns it the other
way, which makes an offset (a jog): bend 45° out, then 45° back, and the last leg runs parallel to
the first. In the dialog each further bend has a **turns** choice, Same way or Other way.

When the shape is easier to draw than to count out, **Path Flange** (`sheet.path_flange`) makes
the same chain from a sketch. Sketch an open chain of lines on a plane across the edge (square to
it), starting at the edge, then pick the edge and the lines. Each line becomes a flange as long
as the line (measured like an edge flange's height: to the outer face from the virtual sharp),
and each corner a bend. The first line's angle to the sheet sets the first bend, and the path's
turns set which way each later bend goes, so a jog is just a path that turns back. Changing the
sketch changes the flap. It is one feature in the timeline and part of the same sheet. Lines
that run straight on from each other count as one; a line that does not turn from the sheet is
refused, and so is a line too short for its bend. Arcs are not supported yet.

```json
{"command":"sheet.path_flange","params":{"edge":[1,0,20],"sketch":"Path","curves":["p1","p2"]}}
```

A flange can also stand on the far edge of another flange: a return, such as a lip turned in
along the top of a tray's walls. It runs along that edge from end to end, so it follows the
closed end of the flange it stands on, slanted or not. Two returns of one feature whose flanges
meet at a corner of the base are closed toward each other as well, with the rule's gap. They
are mitred either side of a plane that runs along the mean of the two panels' normals, bisects
their bend lines and passes where the lines their panels start on cross. Returns that lie in
one plane (level returns on a tray's walls, whatever the walls' angle) then meet on the corner's
bisector, like the sides of a picture frame. Each panel ends on a slanted line in the flat.
Each bend stops square where all of it keeps half the gap from that plane, at the latest at
the end of the flange it stands on, which can leave a short stretch of that flange's edge bare.
Returns turned opposite ways pass each other. A return that meets no other at a corner ends
with the flange it stands on; where the flange beside that one leans in over it, the return is
cut back until it keeps as clear of that flange as the end of the flange it stands on does.
Returns that can't be closed stay open with a warning, as on the base. Returns of separate
features don't close toward each other either: make the returns round a corner in one feature.
A corner relief or a tear changes none of this: it leaves the flanges' closed ends as they
are, so the returns on them close over a relieved or torn corner as over a whole one.

```json
{"command":"sheet.flange","params":{"edges":[[60,2,20],[118,45,20],[60,88,20],[2,45,20]],
 "height":12,"angle":"90 deg"}}
```

(The four returns of a 120 x 90 tray in 2 mm sheet with 90° walls 20 high: each point is the
middle of a wall's far edge on its inner face, so the returns turn in over the base.)

A flange can stand on a straight side edge of an existing flange as well. Select that edge
and the adjoining edge of its supporting panel in one feature to close the new flanges across
the existing bend. The panels are mitred with the rule's gap; `round` and `rectangular` corner
reliefs cut a rounded or rectangular slot across the existing bend zone in the flat material.
The slot's width is `corner_relief`, and it survives unfolding, refolding and DXF export.
`notch` keeps the gap without another cut. A `tear` across an existing bend is not supported
yet and leaves the notch with a warning. A mitre that would overlap the flat pattern stays
open with a warning. Side-edge setbacks on curved panels or on only part of an edge left by
a cut-out are not supported yet. The edge connecting a panel to its existing bend is not a
free edge. See `examples/regressions/sheet-side-flange-corner.json` for a small reproduction.

**Picked faces** (Convert's `face` and `faces`; Faces in the dialog): pick a face of the sheet
and the sharp edges that become bends (`bends`; Bend Edges in the dialog). The faces reached
from the picked faces across those edges become the sheet. They are surfaces, whatever the body
is: a solid's faces become the outside of the sheet, with the rule's thickness taken into the
body, and a shelled body's walls are no different (Convert reads no other side of a face;
Recognize, below, does). Each sharp edge stays where the faces of its bend would meet, so the
part keeps its envelope and the flat pattern is the outside lengths less a bend deduction per
bend, 2·(R + T)·tan(θ/2) − θ·(R + K·T). Bends turn either way, follow one another and take any
angle (the walls of a tapered tray at 72°). An edge between two faces of the sheet that is no
bend is cut open, each side half the rule's **gap** from the plane halfway between the two, like
the corners of a tray. A bend drawn as a chamfer (a flat strip between two faces, the chord of
the bend's arc, as models without bend radii draw one) is a face of the sheet like any other:
its two edges are the bends to pick, and a pick in the middle of the strip is refused. A bend
modeled with its radius (a cylinder between two flat faces tangent to it, as sheet metal parts
from other systems come, or a filleted edge) is picked anywhere on its cylinder, and converts the
same way while its inside radius is short of about five thicknesses (see Convert above): the
sheet is bent there with the rule's radius and K-factor, not the modeled radius, so the flat
pattern is the one your tooling makes; from five thicknesses on it is a roll. The result is an
ordinary sheet body: edge flanges, cuts, Unfold, the flat pattern and its exports work on it,
and it follows the solid when a parameter changes (the edges and the picked faces are found by
their names). Without bend edges the sheet is the faces picked alone: one face is a plate.
Either way round walls (holes, slot ends, rounded corners, notches) stay circles and arcs in the
flat pattern and the DXF, and the holes and slots of every face of the sheet are its cut-outs,
where the bends put them. Not converted yet: a hole that reaches into the band a bend takes from
its panel (cut it after converting), and bends that close a ring.

```json
{"command":"solid.box","params":{"length":100,"width":60,"height":30,"body_name":"Block"}}
{"command":"sheet.convert","params":{"body":"Block","face":[50,30,0],
 "bends":[[50,0,0],[100,30,0],[50,60,0],[0,30,0]]}}
```

(A tray: the block's bottom and its four walls, the rule's thickness in from their outside; the
block's top is no face of the sheet.)

**Recognize Sheet Metal** (`sheet.recognize`) reads a body that is sheet metal already (a shelled
part, a plate, a part from another system) as one sheet. The side of the face picked (`face`;
Surface Face in the dialog), else of the body's largest face, is the sheet's surface, and every
face of that side has its other side one thickness behind it; a body whose walls are of more than
one thickness is refused, naming them. A bend can be picked on either side, on its edge or on its
modeled cylinder, and the sharp edges that are not picked are cut open. A modeled cylinder is a
bend or a roll as in Convert, its inside radius measured in the body's own thickness. Round walls
that a STEP file writes in pieces (a hole as two half-cylinders) are one circle each. The sheet
is still the rule's: the rule's thickness, taken into the material from the surface side, and
the rule's bend radius, whatever the body's, and the feature's warning says where they differ.
To keep the body's own sizes, `new_rule` (New Rule in the dialog) first makes a rule of the
body's thickness and of the inside radius most of its modeled bends have, its K-factor, reliefs
and gaps the active rule's, and the sheet takes it; `sheet.measure_body` reads those values
without changing the design.

```json
{"command":"solid.box","params":{"length":100,"width":60,"height":30,"body_name":"Tray"}}
{"command":"solid.shell","params":{"faces":[[50,30,30]],"thickness":2}}
{"command":"sheet.recognize","params":{"body":"Tray","face":[50,30,0],
 "bends":[[50,0,0],[100,30,0],[50,60,0],[0,30,0]],"new_rule":"Tray 2 mm"}}
```

Recesses with a flat floor and walls square to the face, such as an engraved label or a
counterbore, are kept as pockets of the sheet: the flat pattern lists each under `pockets` (its
outline, side and depth), and the DXF draws its outline on the pocket layer of its depth, apart
from the cut contours, for a laser to mark rather than cut. Recognize keeps a shell's or a
plate's own, at any depth short of the thickness and from either face; a plate raised in places
(a boss) is no plate of even thickness and is refused. Convert keeps a recess only where its
walls and floor are picked to engrave (`engrave`; Engrave Faces in the dialog): it is then a
pocket of the panel round it, as deep as its floor lies behind that panel's face, and one deeper
than the sheet is thick is refused. A recess not picked may be a pocket the design means: its
faces are panels, or, where only the face it is cut into is picked, its opening is a hole of
that panel. A bend across a pocket is refused as for any pocket, so fold or flange clear of an
engraving.

The `face` can also be the name `model.faces` lists for it (`"face":"F1:+z"`), which saves
computing a point on a face with holes or a curved outline: `model.faces` gives every face a
`point` that lies on it.

For a shop drawing or spreadsheet, `sheet.export_bend_table` returns CSV by default; choose
`format: "json"` for structured data. It includes the sheet rule, thickness, bend angles,
radii, allowances and flat centre-line coordinates. Each bend has its bend angle (`angle_deg`)
and its included angle (`included_angle_deg`, below; the CSV's last column, so the columns before
it keep their places). Lengths use millimetres and
angles use degrees. A bend is listed where its centre line has material: one that a cut-out has shortened
gives the length and ends it has left, and one that cut-outs split gives a row for every piece,
all with the bend's number (the flat pattern and the DXF draw the same pieces as bend lines).
Supply `path` to save it, or omit `path` to receive the content through the command
response. `body` defaults to the last sheet body. Exporting does not add a timeline entry or
consume an undo step.

```json
{"command":"sheet.export_bend_table","params":{"format":"csv","path":"bracket-bends.csv"}}
{"command":"sheet.export_dxf","params":{"path":"bracket-flat.dxf"}}
```

The DXF is an AutoCAD 2000 (`AC1015`) drawing in millimetres, made for a laser-cutting shop's
nesting or CAM software: each kind of line is on a layer of its own, with a colour (AutoCAD's
index) and, for the bends, a linetype that the file defines.

| Layer | Colour | Line | Holds |
|---|---|---|---|
| `OUTLINE` | 7 white or black | continuous | the outer contour of each piece of the sheet |
| `CUT` | 4 cyan | continuous | what is cut out inside it: holes, slots, and the loose pieces a cut leaves (the middle of a ring) |
| `TEXT` | 2 yellow | continuous | the letters that text cut through the sheet, with their counters; only with `"text": "cut"`, the default |
| `ETCH` | 6 magenta | continuous | letters as single contours, to be marked and not cut: `"text": "etch"`, and always text machined to a depth |
| `BEND_UP`, `BEND_DOWN` | 1 red, 5 blue | `DASHED` | the bend centre lines, by direction |
| `BEND_NOTES` | 8 grey | continuous | the note beside each bend line, only with `"bend_notes": true` |
| `POCKET_…` | 3 green | continuous | pockets, a layer for each depth (see below) |
| `PERFORATION` | 3 green | continuous | the holes of a perforation field |

`OUTLINE`, `CUT`, `BEND_UP` and `BEND_DOWN` are always in the layer table, the others when the
sheet has something for them. The inner contours (`CUT`, `TEXT`) are written before the outline,
so a program that follows the file's order cuts inside-out.

A round hole is a `CIRCLE`, a circular arc an `ARC`, a whole ellipse one `ELLIPSE` and an arc of
an ellipse an elliptical `ELLIPSE`; the pieces of a spline, the outlines of text and the other
conics are `SPLINE`s of their own degree, so no curve is written as chords. For a machine that
reads only lines and arcs, pass `"curves": "arcs"` to `sheet.export_dxf`: the free-form curves
are then tangent arcs and lines within 0.001 mm.

Letters are told from other cut-outs by where they came from: a cut whose sketch holds text
(Sketch Text) marks the cut-outs it makes as letters, counters included, while the design is
built; nothing more is saved in the design file. A letter that another cut-out overlaps, or
that the sheet's edge clips, has become one shape with it and stays on `CUT`.

```json
{"command":"sheet.export_dxf","params":{"path":"tray-flat.dxf","text":"etch","bend_notes":true}}
```

With `"bend_notes": true` each bend line gets a note on `BEND_NOTES`, `B1 UP 90° R1` (the bend's
number, its direction, its angle and its inner radius; `%%d` is AutoCAD's degree sign). It is 3 mm
high (`"note_height"` changes that), centred half a height beside the middle of the line,
parallel to it and readable from below or from the right. A bend that cut-outs split has a note
for each piece. Notes are off by default, because a program that cuts or marks every layer would
mark them.

For a program that cuts every line it is given, `"bend_lines": false` leaves the bend lines out
(`BEND_UP` and `BEND_DOWN` are then empty), and `"layers": "single"` writes every entity on layer
`0` instead of a layer for each role.

A bend's angle is how far the sheet turns from flat. Many press brakes are set by the
**included angle** instead: the angle left between the bend's two sides, 180° less the bend
angle. A 90° bend is 90° either way, but a 58° bend is set as 122°. The rule's `angle_display`
(Angle Display in Sheet Metal Rules) says which one the DXF's bend notes give: `bend` (Bend,
the default) or `included` (Included). Under an `included` rule the note of a 58°
bend reads `B1 UP 122° R1`. The bend table gives both angles whatever the rule says, in columns
of their own (`angle_deg` and `included_angle_deg`), so neither is taken for the other. The
setting does not change the flange angle yet: `angle` in `sheet.flange` and in the Flange dialog
is always the bend angle, so a 122° included angle is a flange at `180 deg - 122 deg`. A design
keeps the setting in its rule, so templates made from it read their bends the same way.

```json
{"command":"sheet.manage_rules","params":{"name":"Sheet 2 mm","angle_display":"included"}}
{"command":"sheet.flange","params":{"edges":[[50,0,2]],"height":20,"angle":"180 deg - 122 deg"}}
{"command":"sheet.export_dxf","params":{"path":"bracket-flat.dxf","bend_notes":true}}
```

A design with several sheet bodies exports one file for each with `"per_body": true`: the files
are `<path>-<body>.dxf` (characters a file name cannot hold become `_`; two bodies that would
share a name get `_2`, `_3`…), and the reply lists them in `files`. Without a `path` it lists
their DXF texts instead.

Base-flange profiles keep their interior holes. Through cuts that cross a sheet edge become
notches in the flat outline; overlapping and repeated cuts remove their combined area once.
The flat preview, DXF and unfolded solid use the same material boundaries. Lines and circular
arcs remain exact. An ellipse, a spline or text stays the curve it was drawn as in the flat
pattern and the unfolded solid; the folded solid follows it with tangent arcs within 0.001 mm,
and the point lists write it as chords within 0.001 mm (the DXF keeps the curve, see above).
Flat-pattern JSON lists each cut-out twice, in the same order: as points (`cutouts`), and
exactly (`cutout_shapes`): a round hole as `{"circle": {"center", "radius"}}`, any other as its
`line`, `arc` (centre, radius, start and sweep in degrees) and `polyline` segments (one
`polyline` for each ellipse quarter, spline or glyph curve).

Cuts can leave separate material pieces, including the centre of a ring cut. These pieces
remain one logical sheet group through Unfold / Refold and export as separate flat outlines.
A piece a cut parts from its panel that holds none of the panel's joints (a flange cut off along
its bend, the part of a ring's inside on a flange's edge, a letter's counter) stays where it was,
folded or flat, and in the DXF. A cut that leaves the sheet in several pieces gives no warning:
every piece is as much the sheet as any other.
Large profiles that exceed the geometry processing budget report an error instead of
exporting a partial outline.

A Box cut, or a Cylinder cut whose axis is the sheet's normal, machines the sheet like the
extrude of its footprint: through the sheet it is a cut-out (a cylinder a round one), and
part way it is a pocket. A primitive cut at any other angle, or across a folded bend, is refused.

Hole machines the sheet the same way from a flat face of any panel, on either side, folded or
unfolded: square to that panel, a through hole (plain or tapped) is a round cut-out of the flat
pattern and a `CIRCLE` in the DXF, and a counterbore or a blind flat-bottomed hole is a round
pocket. A hole slanted to its panel or drilled into the edge of
the sheet, a countersink and a drill point are refused.

Parallel, untapered extrude cuts also preserve shallow pockets through Unfold / Refold.
The pocket floor and side walls remain solid geometry; overlapping cuts from opposite sides
become a through hole wherever they remove the whole thickness. Flat-pattern JSON lists
`pockets` with their side, depth and bottom/top thickness coordinates in millimetres.
DXF pocket boundaries use `POCKET_BOTTOM_Z…_TO_…_MM`, `POCKET_TOP_Z…_TO_…_MM`, or
`POCKET_INTERNAL_Z…_TO_…_MM` layers. These are machining operations, so do not treat every
pocket layer as a through-cut contour when importing the drawing into a CAM tool. (Text
machined to a depth is a marking, on `ETCH`.)

Extrude Intersect on a straight base or an unfolded sheet retains the material inside both
the current tool footprint and its depth interval, including interior holes, detached islands
and earlier pockets. A shallow intersection records the removed layers above and below the
retained material as top and bottom pockets. The sheet rule keeps the original stock gauge;
the remaining solid can be thinner. A completely empty intersection removes the sheet and
its metadata together. DXF records pocket depth in its layer names; importing that DXF as
ordinary sketch geometry does not reconstruct the machining depths automatically. Each
positive retained or removed depth layer must meet the kernel's 0.000001 mm extrusion
resolution; smaller layers report an error rather than being rounded away. To Object can
retain a tool that trims to a straight prism, using its actual trimmed footprint. A curved
tool with varying depth is refused before changing sheet geometry. Pocket profiles can move with planar panels when forming,
but a pocket crossing a bend zone cannot yet be refolded. Machining folded bend material,
tapered sheet cuts and other unsupported 3D sheet Cut/Intersect operations report an error before changing
the design; they cannot silently restore removed material during a later sheet rebuild.

Other features that change a sheet body's material without the sheet knowing it (a fillet or
chamfer on a corner, an extrude joined onto it, a Mirror or Combine joined into it) are not in
the flat pattern yet. Such a feature warns, and while the body carries its change the flat
pattern, the DXF and the bend table are refused with an error that names it, rather than writing
a part that does not match the body. A later sheet metal feature rebuilds the body from its sheet
features and says that the change is not kept. Moving the body, splitting its faces and cosmetic
threads leave the flat pattern as it is.

A sketch can be drawn on a flat face of the sheet: a face of any panel, on either side. Pick the
face (or give `sketch.create` the point `plane: {face: [x, y, z]}`) and the sketch attaches to it
by the face's name, as on any other body (`"attachment": "exact_face"`), and finds the face again
whenever the sheet is rebuilt: after a parameter change such as the thickness or a flange angle,
through Unfold / Refold, after the file is reopened and when the body is renamed. Sketch
coordinates are the flat pattern's, so a point drawn on a flange lies where the flat pattern and
its DXF have it, and on the bottom face y runs the other way, as seen from below. When the face
is gone (its flange is suppressed or deleted) the sketch reports a lost attachment instead of
moving to another face; **Redefine Sketch Plane** attaches it to another face. `fixed: true`
still freezes the picked plane in space.

A sheet has no face names of its own: it reports what each face of its body was made for, and
the model names the face from that, as it names every feature's faces. `model.faces` lists the
name, and a command that takes a face name takes it. A face is `F<feature>:<role>`, the feature
whose build made it, and where it was made on a face of the sheet as it was, that face's name
after it:

- a panel's faces are `top` (on the sheet's surface) and `bottom`: a base flange's are `F3:top`
  and `F3:bottom`, an edge flange's are named after the rim it stands on, `F5:top:F3:side:l2`
  (feature 5's flange on the base flange's rim along its sketch line `l2`), and a contour
  flange's after the sketch line its panel was drawn along (`F4:top:l1`);
- a rim is `side:<line>` on a panel a sketch made (`F3:side:l2`), and on a panel a bend made
  `far` (its far edge), `end0` and `end1` (across the bend's ends) or `past0` and `past1` (an
  edge's stretch past the bend's end, where a corner runs the panel on): `F5:far:F3:side:l2`;
- a bend's faces are named after the panel it makes: `F5:bend.inner:F3:side:l2` and
  `bend.outer`, its ends `bend.end0` and `bend.end1`, and a tab's faces `tab.top` and
  `tab.bottom`; a fold's bend after the face its line lies on (`F7:bend.inner:F3:top`);
- a hole's wall is the cut's, after its sketch curve (`F9:side:c1`), and where the wall is in
  pieces they are numbered `#1`, `#2` in the order they start along the curve.

A face a later feature adds to a panel is that feature's, named after the face it was made from:
where feature 6's flange meets feature 5's at a corner, the stretch of feature 5's flange edge
past its bend's end, which feature 6's build made, is `F6:past0:F5:bend.end0:F3:side:l1`. Pieces
of a face that a cut parts are told apart by the faces around them (`F3:top#[…]`). The names
never count panels or picks: reordering a feature's picked edges renames nothing, and a name
stays with its face while the sheet's sizes change and other flanges come and go. A face nothing
reports a part for is named from its geometry, as a body without history is. A
`sheet:<body>:<panel>:<side>` name that designs saved before used finds no face: a sketch
attached by one reports a lost attachment, and Redefine Sketch Plane attaches it again.
