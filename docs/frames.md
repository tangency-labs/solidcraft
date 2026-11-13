# Frames and weldments

Frames and weldments (#4003) are structural members (tube, angle, channel, beam, an aluminium
extrusion of your own) laid along the lines of a frame sketch. This page covers what has landed:
members and their profiles, end caps and attached gusset plates. Corners, trims, welds and the cut
list follow.

## The workflow

1. **Sketch the frame**: lines in a sketch, or a 3D sketch for a frame that leaves one plane
   (`sketch.options sketch_3d: true`, then `sketch.line3d`). Drive the sizes with parameters as
   usual; members follow.
2. **Choose a profile**: a built-in one from its dimensions (below), or your own drawn in a sketch
   (a vendor's extrusion: insert their DXF with `sketch.insert_dxf`). A profile sketch's origin is
   the point that sits on each line, and its x and y are the profile's.
3. **Add members**: **Frame Member** (WELDMENTS › CREATE, `frame.member`) makes one body per line,
   square to it, from its start to its end. Repeat with another profile for the next group of
   lines.

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}}
{"command": "frame.member", "params": {"sketch": "Frame", "lines": ["l5", "l6"], "profile_sketch": "45x45 section", "anchor": "bottom_left", "angle": "90 deg"}}
```

Members are ordinary bodies: mirror, pattern, move, drill and combine them with the usual
commands. Where members meet they overlap until corner treatments land.

The **Weldments** ribbon tab gathers the frame commands: New Sketch, Frame Member, End Cap, Gusset and
Parameters. Like Sheet Metal it is always there, and it selects itself when a frame member, end cap or gusset is
the selected body (or the last body of the active component).

## The Frame Member dialog

- **Lines**: pick the frame sketch's lines in the view (2D or 3D sketch lines). **All Lines**
  makes a member on every line of that sketch instead, including lines drawn later.
- **Section (optional)**: pick the section's region in another sketch (the ring between two
  circles makes a tube). With nothing picked, **Profile** chooses a built-in profile and its
  dimensions follow; optional ones left empty take their defaults.
- **Anchor**, **Angle** and **Mirror** place the section on each line, as below. **Name** names
  the members (default: after the profile).
- The preview shows the members as you change the values. **Edit Feature** reopens the dialog with
  the feature's values; the members keep their names through the edit, and a new **Name**
  renames them all. A member made with a whole profile sketch keeps it (the **Section** row shows
  the sketch; untick it for a built-in profile).

## frame.member

- `sketch`: the frame sketch. `lines`: the curve ids to lay members on (2D lines or 3D sketch
  lines), each once; left out, every straight line drawn in the sketch: not construction, not
  projected or otherwise linked (a sketch on a face projects the face's edges; they are
  references, not members), and with a length.
- `profile` (a built-in profile) or `profile_sketch` (a sketch), one of them. A profile sketch's
  section is its material as nested loops alternate (an outline is material, a loop inside it a
  hole, a loop inside that material again), so two circles make a tube; pick other regions with
  `profiles` as `solid.extrude` takes them. Like `solid.extrude`, only what was drawn counts:
  projected edges are left out unless the drawn curves make no profile. It must be one piece.
- `anchor`: the profile point on the line: `origin` (the default: a built-in profile's centre, a
  profile sketch's origin), `center` (of its bounding box), `centroid`, or a corner or edge middle
  of its bounding box (`bottom_left`, `top`…).
- `locate`: `[x, y]`, the profile point on the line in mm from the built-in profile's own origin (a
  bar's or an angle's bottom-left corner, the middle of a channel's web back, the centre of the
  others; a profile sketch's origin). It takes precedence over `anchor`. A bounding-box anchor moves
  with the section when its size changes; a located point does not, so a member stays attached at
  the same point of its section (`[0, 0]` keeps an angle's heel on the line whatever its leg
  lengths). Edit Feature keeps it.
- **Orientation**: the profile's y points up (world +Z) as far as the line allows and its x is
  square to the line and to y (x × y runs along the line, so on a line along +X the profile's x is
  world +Y); a vertical member's x is world +X. `angle` (an angle expression) turns the profile
  about the line, counter-clockwise looking back from the line's end; `mirror` mirrors it across
  its y axis first. A line leaning less than 10⁻⁶ rad from vertical counts as vertical, so
  rounding in a 3D sketch never turns a post a quarter turn.
- Bodies are named after `body_name`, or the profile (`Rect Tube 40x40x3`) or the profile
  sketch, then numbered on past names already taken (`(2)`, `(3)`…). Each member's name is kept
  with its line (the feature's `names`), so deleting or adding a line, or deleting another
  feature, leaves the other members' names alone; `body.rename` renames a member on its line.
  `names` (`{line id: name}`) gives names to keep, as Edit Feature (`timeline.redefine`) passes
  them unless the edit changes `body_name`.

Every built-in profile is SolidCraft's own parametric definition, built in code
(`solidcraft_doc::frame`). SolidCraft ships no vendor's or standard's size table: you give the
dimensions of the stock you buy. Any other section, a vendor's aluminium extrusion for example, comes
from a sketch: draw it or insert the vendor's DXF.

## End caps

**End Cap** (WELDMENTS › CREATE, `frame.end_cap`) closes open ends of members with plates, one
separate body per end, named after the member (`Post End Cap`). Pick the end faces in the view:
each must be a flat end of a frame member, square to its line.

- The plate covers the outside of the profile: on a closed section (a square or rectangular
  tube, a pipe, a bar) it is the profile's outer outline; on an open section (an angle, a
  channel, a beam, an extrusion with slots) it is the profile's outside edges with their rounds
  and chamfers, and each inward stretch (a slot, between flanges) is closed straight across
  along the rectangle round the profile, so the whole end is closed.
- **Offset** sets the plate in from that outside edge: tick it, then give the inset as a
  **Ratio of thickness** (0.5 puts a 6 mm cap 3 mm in from the edge) or a **Length**. Unticked,
  the plate is flush with the outside of the profile. A rounded corner keeps its rounding less
  the inset; a corner rounded less than the inset becomes sharp.
- **Thickness** is the plate's thickness. **Outward** puts it past the end, so the frame gets
  longer by the thickness; **Inward** cuts the member back by the thickness and puts the plate
  where the end was, so the frame keeps its length.
- **Corner Chamfer** cuts each outside corner along both edges, measured from where the edges meet. On a
  rounded corner it must be at least the rounding, which it cuts away; a round cap has no corners
  to chamfer.
- Caps follow the member: change its profile, its line or the frame sketch and they move and
  resize with it. Renaming the member keeps them. A cap whose member or face is gone reports an
  error instead of picking another end.
- Caps follow any outline of lines and arcs. On an open section the chamfer cuts the
  plate's outside corners. An inset that leaves no plate is refused, as is a chamfer longer than its
  edges allow.

```json
{"command": "frame.end_cap", "params": {"faces": [{"face": [18.5, 0, 300], "body": "Post", "index": 7}], "thickness": 6}}
{"command": "frame.end_cap", "params": {"faces": [{"face": [18.5, 0, 0], "body": "Post", "index": 6}], "thickness": "cap_t", "offset": 2, "chamfer": 4, "direction": "inward"}}
```

`faces` are picks as the view makes them: a world point on the face, its body and face index
(`path` for a face in another occurrence). Give the inset as `offset` (a length) or `ratio`, not
both. **Edit Feature** reopens the dialog with the faces and values.

## Built-in profiles

A profile is JSON with a `kind` and its dimensions, each a number in the design's units or a
length expression over its parameters (`"tube_wall"`, `"2 in"`): a member follows a parameter
change like any feature. Dimensions marked *optional* default from the profile's size when left
out, made smaller where a thick section leaves less room for a radius;
`frame.profile.properties` returns the profile in millimetres with every default written out.

| Kind | Required | Optional (default) |
|---|---|---|
| `bar` | `width`, `height` | `corner_radius` (0) |
| `round_bar` | `diameter` | |
| `rect_tube` | `width`, `height`, `wall` | `outer_radius` (2 × wall; inner radius is outer − wall) |
| `round_tube` | `diameter`, `wall` | |
| `angle` | `width` (leg along x), `height` (leg along y), `thickness` | `root_radius` (thickness), `toe_radius` (thickness / 2); together at most the shorter leg's inside length |
| `channel` | `height`, `width` (flange length), `web`, `flange` (thicknesses) | `root_radius` (web, at most the room in a flange and half the web's inside height), `toe_radius` (0) |
| `i_beam` | `height`, `width`, `web`, `flange` | `root_radius` (web, at most the room in a flange and half the web's inside height) |

The profile's bounding box is centred on its origin, x across and y up. Angles have their heel at
the bottom left; channels open towards +x.

Dimensions are refused, with the dimension named, when they are not finite, are outside
0.01 mm–10 m (radii: 0, or 0.01 mm–10 m), or do not fit: a wall of half the tube or more, a radius
given larger than the room for it.

## Commands

- `frame.profile.list`: every kind, its dimensions and a working example.
- `frame.profile.properties` `{profile, density?}`: the section, worked out exactly from its lines
  and arcs: `area` (mm²), `centroid` (mm from the bounding box centre), `ixx`, `iyy`, `ixy` (mm⁴
  about the centroid), `width`, `height`; with `density` (kg/m³), `mass_per_m` (kg/m).
- `frame.profile.insert` `{profile, at?, anchor?, angle?, fixed?, sketch?}`: draws the profile into
  the sketch being edited, its `anchor` point (`center`, `centroid`, or a corner or edge middle of
  the bounding box such as `bottom_left`) on `at`, turned by `angle` radians. The curves are locked
  (`fixed`, default true) so solving the sketch keeps the shape. The outline and each hole are
  separate sketch regions: extrude the material region (pick it with `profiles: [{point}]`).

```json
{"command": "frame.profile.insert", "params": {"profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}, "at": [0, 0], "anchor": "bottom_left"}}
```

## Gussets

**Weldments › Create › Gusset** (`frame.gusset`) adds a separate reinforcing plate between two planar support faces. Select the faces in order, enter the two leg lengths and thickness, and use the preview to check placement. A triangular outline uses zero outer clipping; **Clipped Tips** cuts equal distances from the two outer tips. **Corner Relief** removes the inside vertex, leaving room at the existing weld corner. The plate is named **Gusset**, then **Gusset (2)** and so on, unless you give it a name.

The legs follow the selected faces into their outward-facing joint space. This also supports non-right-angle joints. The three thickness-placement icons show the plate on the negative side, centred on the gusset plane, or on the positive side. Click an icon to update the live preview; the selected button stays highlighted and its tooltip names the placement. Positive direction is the cross product of the first and second support normals; swapping supports reverses it. **Joint Offset** moves the plate along that direction from the centre of the faces' common span.

```json
{"command":"frame.gusset","params":{"supports":[{"body":"Post","index":2,"face":[20,0,100]},{"body":"Rail","index":5,"face":[100,0,40]}],"leg1":"80 mm","leg2":"100 mm","thickness":"6 mm","relief":"10 mm","clip":"0 mm","placement":"center","offset":"0 mm","body_name":"Joint plate"}}
```

Face indices in this example are illustrative: obtain the current exact picks from selection or `model.faces`. The saved feature holds persistent face identities, not those transient indices. Length numbers use the design unit; picked point coordinates are world millimetres. Every length can instead be a parameter expression.

The entire contact strip must fit on material on both finite faces. Oversize legs/thickness, holes crossing a contact strip, parallel faces and lost or replaced supports return an error. The supporting bodies stay separate and unchanged; the gusset is not a weld or a strength calculation. The first tool requires planar faces. Curved/coped supports and fastener patterns remain separate work.

Edit Feature restores both exact picks and dimensions; preview, undo/redo and save/reopen use the same command. Frame-member supports also retain the producing sketch line, so another line cannot silently take over an attachment through a reused body name.
