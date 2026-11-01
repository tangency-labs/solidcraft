# Frames and weldments

Frames and weldments (#4003) are structural members (tube, angle, channel, beam, an aluminium
extrusion of your own) laid along the lines of a frame sketch. This page covers what has landed:
members and their profiles. Corners, trims, end caps, gussets, welds and the cut list follow.

## The workflow

1. **Sketch the frame**: lines and circular arcs in a sketch, or a 3D sketch for a frame that leaves one plane
   (`sketch.options sketch_3d: true`, then `sketch.line3d` and `sketch.arc3d`). Drive the sizes with parameters as
   usual; members follow.
2. **Choose a profile**: a built-in one from its dimensions (below), or your own drawn in a sketch
   (a vendor's extrusion: insert their DXF with `sketch.insert_dxf`). A profile sketch's origin is
   the point that sits on each line, and its x and y are the profile's.
3. **Add members**: **Frame Member** (WELDMENTS › CREATE, `frame.member`) makes one body per selected
   line or arc. Enable **One Continuous Member** to make a connected tangent line–arc–line run
   into one tube. Select its segments in path order. Repeat for another physical member.

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}}
{"command": "frame.member", "params": {"sketch": "Frame", "lines": ["l5", "l6"], "profile_sketch": "45x45 section", "anchor": "bottom_left", "angle": "90 deg"}}
```

Members are ordinary bodies: mirror, pattern, move, drill and combine them with the usual
commands. Where members meet they overlap until corner treatments land.

The **Weldments** ribbon tab gathers the frame commands: New Sketch, Frame Member and
Parameters. Like Sheet Metal it is always there, and it selects itself when a frame member is
the selected body (or the last body of the active component).

## The Frame Member dialog

- **Lines**: pick the frame sketch's lines and circular arcs in the view (2D or 3D). **All Lines**
  keeps the existing default: a member on every drawn straight line, including lines drawn later.
- **One Continuous Member**: the selected curves make one body. Keep **All Lines** off and
  select curves in connected path order. Curves drawn backwards are reversed automatically.
  A sharp corner needs an explicit tangent bend arc; the command does not invent a bend radius.
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

- `sketch`: the frame sketch. `lines`: the curve ids to lay members on (2D or 3D sketch
  lines and circular arcs), each once; left out, every straight line drawn in the sketch: not construction, not
  projected or otherwise linked (a sketch on a face projects the face's edges; they are
  references, not members), and with a length.
- `continuous`: false by default. When true, explicitly selected `lines` form one physical
  member, in path order. Their ends must meet within 0.000001 mm and their tangents within
  0.000001 rad. Open line/arc paths are supported; splines and closed members are refused.
  The section starts with the orientation below and rotates through each bend, carrying that
  orientation into subsequent straights and bends in other planes. A bend whose axis crosses
  the anchored section is refused. This models nominal geometry, not tube deformation or springback.
- `profile` (a built-in profile) or `profile_sketch` (a sketch), one of them. A profile sketch's
  section is its material as nested loops alternate (an outline is material, a loop inside it a
  hole, a loop inside that material again), so two circles make a tube; pick other regions with
  `profiles` as `solid.extrude` takes them. Like `solid.extrude`, only what was drawn counts:
  projected edges are left out unless the drawn curves make no profile. It must be one piece.
- `anchor`: the profile point on the line: `origin` (the default: a built-in profile's centre, a
  profile sketch's origin), `center` (of its bounding box), `centroid`, or a corner or edge middle
  of its bounding box (`bottom_left`, `top`…).
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
  A continuous member uses `names: {"$path": "Main hoop"}` so its name survives reversing
  the path or changing the first selected segment.

### A bent chassis tube

Draw a line from `(100, -200)` to `(100, 0)`, a quarter-circle arc with centre `(0, 0)` from
`(100, 0)` to `(0, 100)`, and a line to `(-300, 100)`. With those curve ids `l1`, `a1`, `l2`:

```json
{"command":"frame.member","params":{"sketch":"Chassis","lines":["l1","a1","l2"],"continuous":true,"profile":{"kind":"round_tube","diameter":20,"wall":2},"body_name":"Main hoop"}}
```

The result is one tube, with two straight lengths of 200 and 300 mm and a 100 mm centreline
bend radius. Its nominal centreline length is `500 + 50π` mm. Grouping three separate
members or combining their solids does not record this continuous manufacturing path.

Bend-list output and flattened round-tube cope DXF templates are subsequent fabrication
steps tracked under #4003. A continuous member does not yet produce either export.

Every built-in profile is SolidCraft's own parametric definition, built in code
(`solidcraft_doc::frame`). SolidCraft ships no vendor's or standard's size table: you give the
dimensions of the stock you buy. Any other section, a vendor's aluminium extrusion for example, comes
from a sketch: draw it or insert the vendor's DXF.

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
