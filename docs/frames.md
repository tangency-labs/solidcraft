# Frames and weldments

Frames and weldments (#4003) are structural members (tube, angle, channel, beam, an aluminium
extrusion of your own) laid along the lines of a frame sketch. This page covers what has landed:
members, their profiles, and trims (butt joints, and trimming or extending to planes and faces).
Mitre corners, copes, end caps, gussets, welds and the cut list follow.

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
4. **Trim them**: **Trim/Extend** (WELDMENTS › MODIFY, `frame.trim`) butts members against the
   members they meet, or trims or extends them to a plane or face (below).

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}}
{"command": "frame.member", "params": {"sketch": "Frame", "lines": ["l5", "l6"], "profile_sketch": "45x45 section", "anchor": "bottom_left", "angle": "90 deg"}}
```

Members are ordinary bodies: mirror, pattern, move, drill and combine them with the usual
commands. Where members meet they overlap until they are trimmed; mitre corners come later.

The **Weldments** ribbon tab gathers the frame commands: New Sketch, Frame Member, Trim/Extend
and Parameters. Like Sheet Metal it is always there, and it selects itself when a frame member is
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

## Trim/Extend

**Trim/Extend** (WELDMENTS › MODIFY, `frame.trim`) moves the ends of members to what they meet:

- **Another member** (a butt joint): the end stops flat against that member's side. The side is
  worked out from the other member's line and profile, not from a face of its body, so the joint
  follows sketch and parameter edits: widen the frame and the rails between the posts grow with it.
- **A plane or a planar face**: an origin or construction plane, or a flat face of any body. A
  face that slants across the member cuts its end at that slant. A face is attached as a sketch on
  it is, so the trim follows the face: make the plate under a post thicker and the post stands on
  its new top.

A member trims or extends alike: an end short of its target grows to it. Each member keeps its
body and its name, and later trims see it as it now is (trim the cross members to rails that were
themselves trimmed to the posts).

```json
{"command": "frame.trim", "params": {"members": ["Rail", "Rail (2)", "Rail (3)", "Rail (4)"], "to": ["Post", "Post (2)", "Post (3)", "Post (4)"]}}
{"command": "frame.trim", "params": {"members": ["Brace"], "to": {"plane": {"face": [0, 200, 500]}}, "gap": "1.5 mm"}}
```

- `members`: the members to trim, each a member's body name or `{feature, line}` (its Frame Member
  feature and its line's id). A trim keeps them by feature and line, so renaming a member or its
  feature keeps the trim.
- `to`: what to trim them to, one or a list: a member (its body name, or `{member, side}` with
  `side` one of `right`, `left`, `top`, `bottom` of its profile; by default the side the trimmed
  member meets), or `{plane}` as `sketch.create` takes it (`XY`, a construction plane,
  `{origin, normal}`, or `{face: [x, y, z]}` for a planar face, with `body`, `index` and `path` for
  the exact face picked).
- `end`: `auto` (the default: for each target, the end of the member nearer it), `start` or `end`.
- `gap`: the weld gap between each end and its target, a length expression (default 0, at most
  1 m).

A member meets a member target when the line through the middle of its profile passes through the
target's profile and along the target's length; a plane it crosses. With several targets, each
moves the end it meets, so one trim can butt every rail of a frame against its posts. It is refused,
with the reason, when a member meets none of the targets, when two targets would move the same end
(trim them in separate features), when a chosen side faces away from the member, and when the
member's body was changed after it was made (a hole drilled in it, say): a trim rebuilds the member
from its line and profile, and would lose that change, so trim it before the features that change
it. Members of another component are not supported yet.

### The Trim/Extend dialog

- **Members**: pick the members to trim (their bodies or faces of them).
- **To**: pick members, planar faces or planes. A face of a member is that member.
- **End** (Nearer End, Start, End) and **Gap** as above.
- **Edit Feature** reopens the dialog with the trim's picks and values. Targets a pick can't show
  (a member's chosen side, a face, a plane given by its origin and normal) are listed as **Keep:
  n more targets**, kept while ticked; untick it to drop them all and pick again.

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
