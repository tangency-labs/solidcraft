# Solid features

For prerequisites, a readable options table and a first example, open the
[practical Extrude guide](../reference/commands.md#command-solid.extrude).

Features live on the **timeline** and rebuild incrementally from the first one that changed.
Each one can be edited (double-click), suppressed, renamed, reordered (if nothing it depends on
moves after it), rolled back to, and deleted. Deleting asks first when other features depend on
it.

| Panel | Features |
|---|---|
| Create | extrude (one side, two sides, symmetric, taper, start offset, to a distance or through all; new body, join, cut, intersect; thin walls along curves), revolve, sweep, loft, box, cylinder, sphere, torus, coil, pipe, rib, web, emboss, hole (simple, counterbore, countersink, tapped; on faces, at distances from edges, or at sketch points), thread, rectangular/circular/path patterns, mirror |
| Modify | press pull, fillet, chamfer, shell, draft, scale, combine, split body, offset face, replace face, move/copy, align, remove |
| Construct | offset plane (at a distance or through a point), plane at angle, plane along path |
| Inspect | measure, section analysis, interference, compare bodies, curvature comb and map, zebra, draft, minimum radius, isocurves, centre of mass |

**Quick measurement** (off by default; turn it on under Preferences › Interface): with edges,
faces, sketch curves or points selected, the view's bottom right corner, above History, shows
what they measure: an edge's length (Ø and R for a round one), a face's area, a point's position,
two items' distance and angle, several edges' or faces' total. A click copies it. The Measure tool
(I) does more.

Extrude, Revolve, Sweep, Loft, Coil and the Box, Cylinder, Sphere and Torus choose their
operation from what their material does, until you pick one: **Join** when it makes one body with
a body it touches (a block against a wall, a ring round a hole, a boss meeting only part of a
face), **New Body** when it would be a separate piece, and **Cut** when it goes into a body
(Extrude) or lies inside one (Revolve and the Box, Cylinder, Sphere and Torus). The window finds
out by running the Join on a copy of the design in the background, so its choice is what OK
makes; it changes only when that check is in, and OK waits for it. An operation you pick stays,
and a feature you edit keeps its own. Pipe, Thicken and Boundary Fill start as a New Body. Only
shown bodies take part: with some bodies hidden, the feature names the shown ones as its targets,
so a hidden body is never joined or cut, and showing it again later changes nothing.
**Choose the operation automatically** (Preferences › Design, on by default) turns this choice
off: each window then keeps the operation it opens with, Extrude its own Join or Cut, as before.

A Join you pick whose material touches none of its target bodies still joins it, to the first
one as a separate piece, and the feature's warning says so: "`Body1` is now in 2 separate
pieces". Combine joins the bodies it is given without that warning. Changing the default does not
alter operations already saved in a design.

**Extrude to a picked target** (Preferences › Design, off by default) lets a pick choose the
extent. With it on and a profile selected in the solid Extrude dialog, picking a face away from
its plane, a curved face, an off-plane vertex or a parallel plane automatically switches
**Distance** to **To Object**. Coplanar faces still join the profile selection; non-parallel
planes are not targets. The extent can still be chosen manually. For a new extrusion without
start or end offsets or a From Object input, the target also sets the stored distance and
one-sided direction, so returning to **Distance** keeps that reach. Thin Extrude keeps its
existing picks. With the setting off, picks only add profiles, as before.

A positive extrude taper slopes the side walls outward, so the profile grows along the
extrusion direction and the far end is larger; a negative taper slopes them inward, making
the far end smaller. For example, a 10 × 10 mm square extruded 10 mm with `taper: "5 deg"`
has an approximately 11.75 × 11.75 mm far face; with `taper: "-5 deg"`, it is approximately
8.25 × 8.25 mm.

A taper slopes the sides of any profile, sketch text included: lines, arcs and spline or conic
curves move in or out together, and a flat tip shorter than the taper closes to a point. On
spline and conic sides the slope is approximate: the moved outline is a curve of the same kind
within 2 % of the taper's offset (or 10 µm) of the true one, a curve that turns too much for
that being cut into pieces first; where a curve bends tighter than the offset, the offset folds
and is smoothed over. A taper steeper than the profile's details (a stroke, a curve or a gap
narrower than it) is refused with that reason; a smaller taper or distance builds. A taper
composes with a start offset (a drafted pocket that starts part way up a part): the offset moves
the profile along the normal and the sides are drafted from there, so the profile is the section
at the start whatever the offset. A taper with a two-sided or symmetric extent is not built yet
and says so.

**Thin Extrude** (`thin: true` on `solid.extrude`, the Thin Extrude box in its dialog) builds a
wall of constant thickness along sketch curves instead of filling profiles: open chains (a single
wall) and closed loops (a ring), lines, arcs, circles, ellipses and splines. In the dialog, a
picked profile stands for the curves around it (its outline and its holes). The wall lies on the curves' centre, inside,
outside, or on both sides with two thicknesses (`side`: `center`, `inside`, `outside`,
`two_sides` with `thickness2`). A closed loop's inside is the area it encloses; an open chain's
is the side it bends toward (the inside of a U), and a straight line's is the left of its first
curve as drawn. Curves that meet end to end are one chain whose corners are mitred; walls that
cross or meet (a ring and a cross, a honeycomb) become one piece. Every extent, operation and target of an extrude applies, and a taper
for walls of straight lines or of whole circles. A wall thicker than the
radius of a curve it follows on the inside is refused. Spline walls follow the exact offset
within 0.0001 mm.

Editing an extrusion retains its signed distances, From/To Object inputs, taper expressions
and explicit body targets. Revolve edits retain world, sketch, explicit-line and model-edge or face axes, both
angle expressions and To Object targets. An unchanged **All profiles** selection continues
to include profiles added later. Cancel restores the complete timeline; a successful edit is
one undo step, including downstream parameter changes. Editing a placed component previews the
edited feature before its downstream history. Cylindrical Emboss profile guides follow the
sketch's chosen occurrence frame.

Newly selected adaptive profiles store complete outer and hole source identities when the
entire boundary can be qualified. Changing an ellipse or periodic offset after extrusion can
then change its spline span count while keeping the same selected material region. A partial,
intersected or ambiguous boundary cannot satisfy that whole-source reference; changing the
hole identities also makes it lost. Older curve-span selections retain their exact stored
multisets. If an older feature has already lost its profile after subdivision changed, edit
the feature and reselect the intended current region to capture the new identity.

New symmetric solid extrusions treat the entered distance as the total thickness and split it evenly
around the start plane: 10 mm spans -5 to +5 mm on an XY sketch. Switching from One Side to
Symmetric keeps the entered total thickness. Through All still reaches targets in both
directions, and Two Sides retains its independent distances. Symmetric extrusions saved before
this change keep their original per-side distances when reopened or edited. Selecting another direction
in the feature editor uses the current total-distance convention.

New symmetric revolves split the entered angle evenly around the sketch profile, regardless
of which way the selected axis points. Two Sides measures the second angle backward and the
first angle forward along the sketch-normal sweep direction. Profiles on opposite sides of
an axis each use their own sweep direction. Designs saved before format 11 keep their original
angular placement when opened, edited or copied; creating another revolve uses the new behavior.
To Object follows that same direction for each selected profile, including curved target faces.
Pick a target away from the axis so its angular position is defined.

For **Emboss**, select a target face (concave, convex, or planar) and a sketch
profile. **Deboss** cuts into the surface. Orthogonal line profiles on tangent exterior cylinders
wrap with exact radial walls, while general profiles (including curves and text) project
onto concave, convex, or planar surfaces. Complete cylinders and coaxial sleeves are supported.
Capture pending component positions before applying. The feature stores the selected target and
author occurrence paths, so edits and reopening retain the chosen instances. A new Emboss
without an explicit face infers a unique eligible supported cylinder or applies planarly;
select the face when the target is ambiguous. Older saved features without a face retain their
planar placement.

A **Hole** with **Single** placement can also sit at distances from up to two straight edges of
its face. After picking the face, click **Reference 1** and pick an edge, then pick a second one
into **Reference 2**. Each edge gets a **Distance** row, measured from where you picked; the
canvas shows each one as a dimension from the edge to the hole's centre, with a box beside it.
Type a distance in either place and the hole moves so its centre is that far from the edge,
measured in the face's plane. Moving the hole measures the distances again. The hole keeps its
distances when the part changes: it follows its edges, and the distances are parameters like its
diameter.

A **Cylinder** placed on a planar face takes reference edges the same way. Select the face, then
start **Cylinder**: it stands where you clicked, and the window has **Reference 1** and
**Reference 2**. Pick one or two straight edges of that face; each gets a **Distance**, measured
from where you clicked, which you can also type in the window or in its box on the canvas. The
cylinder's base centre then follows the edges when the part changes, and the distances are
parameters like its radius. Edit Feature picks the edges again.

A **Hole** with **Single** placement drills where you pick a planar face. A pick on or near a
point of a shown sketch that lies on that face, such as a circle's centre, places the hole
exactly at that point, as **Multiple** placement picks it; **Select Other** (press and hold) also
offers the face where you clicked. The corners of text outlines don't count, nor do projected
points while the sketch hides projected geometry. The hole keeps that place on the face; to have
holes follow later edits of the sketch, place them with **Multiple**.

Two settings under **Create Hole** in **Preferences › Design**, both off by default, change how
a Single hole is placed:

- **Snap to face and edge centres**: the face itself offers snap points too: its middle, and the centre
  of each round edge on it (a boss standing on it, a rounded corner, a hole already in it). While
  the cursor is over the face they show as small marks; a pick near one, or on or beside one of
  those round edges, places the hole at that centre, and the mark it takes is named. Hold **Alt**
  or **Ctrl** to place the hole exactly where you click.
- **Move holes by clicking or dragging**: each placed hole has a handle at its centre. Drag it
  to move the hole (it snaps when snapping is on), or click it to take the hole out. A pick
  elsewhere on a face that already has a hole moves the hole there instead of taking it out.
  **Edit Feature** shows the handle as well, so a hole can be moved later.

The Hole dialog draws a section through the hole as its options make it (type, extents, drill
point, tapped) and puts each size in a field beside the dimension it sets; hovering a field, or
typing in it, lights its dimension.

Tapped holes and **Thread** features have a **Thread direction** choice: **Right-hand**
(the default) or **Left-hand**. Editing and reopening keep the chosen direction. Tapped
holes carry a cosmetic thread; the separate Thread feature can also cut a **Modeled**
left-hand helix. Cosmetic direction is recorded with the thread annotation and does not
change the body. Scripts use `handedness: "left"` or `"right"` on `solid.hole` (with `thread`)
and `solid.thread`. Existing designs and omitted values remain right-handed. Changing the
direction keeps the pitch, thread length and axis; it does not change a hole's drill size.

Fillets and chamfers handle straight edges between planar faces, whole smooth loops, runs of
neighbouring edges with mitred or spherical corners, and closed chains of curved edges (a rolling
ball). See the roadmap's known limitations for the rest.

**Curvature (G2)** in the Fillet dialog's **Radius Type** blends straight edges between two
planar faces with an exact quintic section. The tangent plane and curvature match each side
face: curvature goes to zero at both contact rails, unlike a circular G1 fillet. Each end
must meet one planar face; perpendicular and sloped planar ends are supported. Curved
edges/faces, multi-face ends, and intersecting corner blends are not supported; they return
an error, not a G1 fallback.
**Equivalent Radius** sets the same contact setback as a circular fillet (for a right angle,
setback equals the entered value); it is not a constant curvature radius. Reduce/Taper to Fit
are unavailable for this type.

Scripts and MCP use the same command, for example on a 20 mm box:
```json
{"command": "solid.fillet", "params": {"edges": [[20, 0, 10]], "radius": 2, "type": "curvature"}}
```
`type: "g2"` is an alias. Existing fillets still default to circular, constant-radius G1 blends.

Feature references to faces and edges are stored by name, so an upstream edit that moves geometry
keeps them on the right face (see [Persistent naming](../dev/naming.md)). When a reference can't
be found, the feature warns instead of silently picking another face.

**Plane Along Path** accepts one sketch curve (including a drawn 3D wire) or model edge.
Pick the path in the viewport and enter **Position (0 to 1)**: 0 starts the path, 1 ends it,
and 0.5 places the plane halfway along its length. The plane is perpendicular to the path's
local segment. Preview does not change the design; OK creates one editable timeline feature.
The plane follows later path edits. Curved paths use their sampled polyline; absolute distance,
reference-object placement and multi-curve chains are not yet available in this dialog.

## Comparing two bodies

**Compare Bodies** (`inspect.compare`) lays two versions of one part over each other, such as a
part and the same part exported by another program, and finds the places where only one of them
has material. It works from the two surfaces, not from a boolean of the bodies, so it also
compares bodies that share almost every face, the case a boolean (Combine, Interference) handles
worst.

```json
{"command": "inspect.compare", "params": {"a": "Bracket", "b": "Bracket (2)", "tolerance": 0.01, "color": true}}
```

Without `a` and `b` it compares the two selected bodies, or the design's only two. Both bodies
are measured (volume, area, centre of mass and box, and `difference`: b − a of each). Each
surface is sampled at the corners of its triangles and at points at most `spacing` (0.5 mm)
apart on larger faces, and every sample measures its distance to the other surface: `deviation`
gives the largest and the mean distance each way. Samples further than `tolerance` (0.01 mm) form
the **regions**: `only_in_a` where `a` has material and `b` has none, `only_in_b` the other way
round. Each region gives its volume, the deviating surface area, the largest deviation and where
it is, its centre and box, and the faces of each body it lies on (`faces_a`, `faces_b`, numbered
as `model.faces` lists them), largest volume first. The volume is integrated along a grid of
parallel lines through the region (`resolution` gives their spacing), so a thin sliver is
measured as well as a block. `identical` is true when no region is found, and `verdict` says it in
one line. `warnings` lists what makes the result less certain: a body whose surface does not
enclose the volume it measures (open, or with faces that failed to import), and lines left out of
a region's volume because they crossed a gap. With `color`, the faces that hold a region turn
red where only `a` has material and green where only `b` has, on both bodies: the faces mostly
beyond the tolerance (a small change on a large face leaves the face alone), or, for a region
with none, the face with the largest share.
That is one undo step; without `color` the command changes nothing.

The bodies are meshed within a quarter of the tolerance. The kernel meshes no finer than a
millionth of a body's size, so keep the tolerance at least twice that (0.02 mm for a part 10 m
across). Large bodies are sampled more coarsely, at about `max_points` (eight million) points
each; `sampling.spacing_mm` says what was used, and a larger `max_points` (up to 50 million)
samples them finer, at the cost of time.

When editing an Extrude in the UI, the Browser's bodies, sketches, construction objects and
feature rows follow the history marker before that feature. Later objects return when editing
ends. Rolling back manually keeps feature rows listed, so they remain available to edit.
A hidden body supplying the profile's face is shown temporarily during the edit, including
when a folder or component hides it. OK, Cancel and Escape end that display override; the body's
saved visibility and unrelated visibility changes are preserved.

## Editing a feature from a script

For `solid.pattern.circular`, `axis` accepts `X`, `Y`, `Z`, `{origin, dir}`, or an edge or
round face of the model, `{edge: [x, y, z]}` or `{face: [x, y, z]}` (a point on it).
An explicit origin must contain three finite numeric coordinates in millimetres
(`[x, y, z]` or `{x, y, z}`); omitting it uses the world origin. Expressions and
construction-axis references are not supported. Invalid origins are rejected rather than
silently replaced with zero.

## Axes and points on round edges and faces

Wherever a 3D command takes an axis (Revolve, Surface Revolve, Circular Pattern, Plane at
Angle, and Move's Rotate) you can pick a circular edge, which gives the axis through its centre
along its normal, or a cylinder, cone or torus face, which gives that surface's axis. Hovering
one in an axis input draws the axis as a dashed construction line, and the line stays drawn once
the axis is picked. Flat faces and edges that are neither straight nor circular can't be picked
there.

Revolve, Surface Revolve, Circular Pattern and Plane at Angle keep the picked edge or face as a
reference (`{edge: [x, y, z]}` or `{face: [x, y, z]}` from a script), found again by its
persistent name every time the design is evaluated. If a parameter moves the cylinder, the
feature turns about it where it is now. The axis always points the same way, with its largest
component positive, so a rebuild that reverses the edge doesn't flip the turn. Straight edges
picked as axes keep working as before, as a fixed line.

Where a 3D command takes a point (the construction points, axes and planes built from points,
Revolve and Extrude's To Object, a surface patch's pass-through points, and Move's pivot and
points), a circular edge stands for its centre. A hovered circle shows a small star at its centre.
A construction point keeps the edge, so it follows the circle's centre as the model changes.

`timeline.inspect {feature}` returns a feature's `definition`: the fields `timeline.edit` changes,
with their current values. `timeline.edit {feature, set}` merges `set` into that definition
(objects are merged field by field, anything else replaces the value), rebuilds the feature and
reports `recomputed` and `restored`. The feature's type cannot change, and sketch geometry is
edited with the sketch commands. Expressions are text (`"30"`, `"30 mm"`, `"width / 2"`,
`"45 deg"`); a bare number is a length in the design's units, or degrees for an angle.

```json
{"command": "timeline.inspect", "params": {"feature": "Slab"}}
→ {"type": "ExtrudeFeature",
   "definition": {"sketch": 1, "profiles": {"type": "all"}, "operation": "new_body",
                  "extent": {"distance": "10", "direction": "positive"}},
   "inputs": [{"label": "Distance", "param": "d1", "expression": "10", "unit": "mm"}], …}
{"command": "timeline.edit", "params": {"feature": "Slab", "set": {"extent": {"distance": "12"}}}}
```

`inputs` lists the feature's named inputs. `parameters.change {name: "d1", expression}` sets one
of those directly. Optional fields that are not set do not appear in the definition; they can
still be added through `set`. The fields of the common features:

| Feature | Fields |
|---|---|
| Extrude | `extent`: {`distance`, `direction`: positive\|negative\|symmetric, `distance2`, `start_offset`, `through_all`, `taper`, `to`, `to_offset`, `from`}; `operation`: new_body\|join\|cut\|intersect; `targets`; `profiles`; `sketch`; `thin`: {`curves`, `thickness`, `side`: center\|inside\|outside\|two_sides, `thickness2`} |
| Revolve | `angle`, `angle2`, `symmetric`, `axis`, `to`, `operation`, `targets`, `profiles`, `sketch` |
| Fillet | `radius`, `edges` (a point on each edge), `body`, `style` |
| Chamfer | `distance`, `distance2` or `angle`, `flip`, `edges`, `body` |
| Box | `length`, `width`, `height`, `corner`, `operation` |
| Cylinder | `radius`, `height`, `base`, `axis`, `operation`, `references` (an `edge` point and a `distance` each, as a hole's) |
| Hole | `diameter`, `depth`, `position`, `direction`, `hole` (simple, drilled, counterbore, countersink), `thread`, `to`, `references` (an `edge` point and a `distance` each) |
| Shell | `thickness`, `faces`, `direction`, `body`, `tangent_chain` |
| Draft | `angle`, `faces`, `neutral`, `pull`, `body` |
| Offset face | `distance`, `faces`, `body` |
| Pattern | `pattern` (its kind and counts/spacing), `features`, `bodies` |
| Mirror | `plane`, `features`, `bodies`, `combine` |

For any other feature, `timeline.inspect` shows the fields.

Where a command takes a face as a point (`face.delete`, `solid.shell`, `solid.draft`,
`solid.offset_face`, `solid.split_face`, the `faces` of `solid.replace_face`, and the `face` of
`sheet.convert`), a script can give the face's name instead, as `model.faces` lists it:
`{"command": "face.delete", "params": {"faces": ["F2:side#1", "F2:side#2"], "body": "Plate"}}`.
`model.faces` also lists a `point` on each face that picks it (the centroid of a face with a hole
in it is in the hole, that of a half cylinder inside the cylinder) and, for a cylindrical face,
its `cylinder`: `origin` and `axis` of its axis, and `radius`. A name that is more than one face
(the pieces of a split face, or one face of several bodies) is refused; `body` says which body to
look on.

Press **Delete** on anything selected (bodies, faces, sketches, features, planes, components,
canvases). It runs as one undoable step and lists the features that would go with it or fail.

Face Extrude, face Revolve, and Press Pull retain linked generated profiles. When a modified planar face has no construction certificate, these internal profiles use the legacy named-face attachment and projection behavior so upstream edits still update the plane and outline. This compatibility path is private to those modeling tools: newly attached user sketches require a certified face or an explicitly fixed plane. Legacy internal attachments can follow the nearest matching face after topology changes; inspect their result after major upstream edits. Activate the intended occurrence before generating a face profile from a repeated component; capture a separately moved source position before creating a legacy profile across components.

Negative Press Pull across supported aligned prismatic faces retains round through-holes,
including after a joined extrusion and in a rotated component occurrence. The cut partitions
the current material into axial sections, preserving the hole through undo, redo, and reopen.
Walls that cannot be qualified as a parallel extrusion still use the general Boolean path
and may report an unsupported coincident-surface operation atomically.
