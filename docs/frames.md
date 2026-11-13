# Frames and weldments

Frames and weldments (#4003) are structural members (tube, angle, channel, beam, an aluminium
extrusion of your own) laid along the lines of a frame sketch. This page covers what has landed:
members and their profiles, and end caps. Corners, trims, gussets, welds and the cut list follow.

## The workflow

1. **Sketch the frame**: lines in a sketch, or a 3D sketch for a frame that leaves one plane
   (`sketch.options sketch_3d: true`, then `sketch.line3d`). Drive the sizes with parameters as
   usual; members follow.
2. **Choose a profile**: a standard steel size by name (`{"size": "RSC 203x89"}`, below), a
   built-in one from its dimensions (below), or your own drawn in a sketch
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

The **Weldments** ribbon tab gathers the frame commands: New Sketch, Frame Member, End Cap and
Parameters. Like Sheet Metal it is always there, and it selects itself when a frame member is
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
- Bodies are named after `body_name`, or the standard size (`RSC 203x89`), the profile
  (`Rect Tube 40x40x3`) or the profile sketch, then numbered on past names already taken (`(2)`, `(3)`…). Each member's name is kept
  with its line (the feature's `names`), so deleting or adding a line, or deleting another
  feature, leaves the other members' names alone; `body.rename` renames a member on its line.
  `names` (`{line id: name}`) gives names to keep, as Edit Feature (`timeline.redefine`) passes
  them unless the edit changes `body_name`.

Every built-in profile is SolidCraft's own parametric definition, built in code
(`solidcraft_doc::frame`). Standard steel sizes (below) fill in a profile's dimensions from a
published table; for other stock you give the dimensions of what you buy. Any other section, a
vendor's aluminium extrusion for example, comes from a sketch: draw it or insert the vendor's DXF.

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
| `rect_tube` | `width`, `height`, `wall` | `outer_radius` (2 × wall), `inner_radius` (outer − wall) |
| `round_tube` | `diameter`, `wall` | |
| `angle` | `width` (leg along x), `height` (leg along y), `thickness` | `root_radius` (thickness), `toe_radius` (thickness / 2); together at most the shorter leg's inside length |
| `channel` | `height`, `width` (flange length), `web`, `flange` (thicknesses) | `root_radius` (web, at most the room in a flange and half the web's inside height), `toe_radius` (0), `flange_slope` (0: parallel flanges) |
| `i_beam` | `height`, `width`, `web`, `flange` | `root_radius` (web, at most the room in a flange and half the web's inside height) |

The profile's bounding box is centred on its origin, x across and y up. Angles have their heel at
the bottom left; channels open towards +x.

`flange_slope` is an angle (a number of degrees, at most 20) that tapers a channel's flanges,
thicker at the web, as on taper flange channels: `flange` is then the thickness at the middle of a
flange's inside length, between the web and the toe, as their tables give it.

## Standard steel sizes

`{"size": "RSC 203x89"}` is a profile too: the named size with the dimensions its standard
publishes, given alone (a size with dimensions is refused). Spacing, case and `×` don't matter,
a channel's mass may follow (`RSC 203x89x30`), `EA` or `L` name equal angles, and a square named
as an RHS is found as the SHS it is. The design saves the size's dimensions beside its name, so it
keeps its section whatever later tables say, and members are named after the size.

| Family | Kind | Standard | Example |
|---|---|---|---|
| RSC | `channel`, 5° flange slope | BS 4-1:1993 taper flange channels (withdrawn) | `RSC 203x89` |
| PFC | `channel` | BS 4-1:2005 parallel flange channels | `PFC 200x90x30` |
| RSA | `angle` | BS EN 10056-1 equal angles | `RSA 50x50x5` |
| SHS, RHS | `rect_tube`, corners 1.5 × wall outside and 1 × wall inside | BS EN 10210-2 hot-finished | `SHS 100x100x5`, `RHS 200x100x6.3` (depth × width) |
| CHS | `round_tube` | BS EN 10210-2 hot-finished | `CHS 42.4x3.2` |

`frame.profile.list` lists every size by name with the table its dimensions were read from; each
size's built section is within 0.5 % of the area that table gives.

Dimensions are refused, with the dimension named, when they are not finite, are outside
0.01 mm–10 m (radii: 0, or 0.01 mm–10 m), or do not fit: a wall of half the tube or more, a radius
given larger than the room for it.

## Commands

- `frame.profile.list` `{family?}`: every kind, its dimensions and a working example; and the
  standard sizes by name, family by family.
- `frame.profile.properties` `{profile, density?}`: the section, worked out exactly from its lines
  and arcs: `area` (mm²), `centroid` (mm from the bounding box centre), `ixx`, `iyy`, `ixy` (mm⁴
  about the centroid), `width`, `height`; with `density` (kg/m³), `mass_per_m` (kg/m); `size` when
  the profile is a standard size.
- `frame.profile.insert` `{profile, at?, anchor?, angle?, fixed?, sketch?}`: draws the profile into
  the sketch being edited, its `anchor` point (`center`, `centroid`, or a corner or edge middle of
  the bounding box such as `bottom_left`) on `at`, turned by `angle` radians. The curves are locked
  (`fixed`, default true) so solving the sketch keeps the shape. The outline and each hole are
  separate sketch regions: extrude the material region (pick it with `profiles: [{point}]`).

```json
{"command": "frame.profile.insert", "params": {"profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}, "at": [0, 0], "anchor": "bottom_left"}}
```
