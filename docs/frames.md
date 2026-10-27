# Frames and weldments

Frames and weldments (#4003) are structural members (tube, angle, channel, beam, an aluminium
extrusion of your own) laid along the lines of a frame sketch. This page covers what has landed:
members, their profiles, end caps, trims (butt joints, and trimming or extending to planes and
faces) and mitre corners. Butt and end-trim corners, copes, gussets, welds and the cut list follow.

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
4. **Trim them**: **Trim/Extend** (WELDMENTS › MODIFY, `frame.trim`) butts members against the
   members they meet, or trims or extends them to a plane or face (below).
5. **Mitre the corners**: `frame.corner` cuts two members that meet at their ends on the plane
   halfway between them (below).

```json
{"command": "frame.member", "params": {"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}}
{"command": "frame.member", "params": {"sketch": "Frame", "lines": ["l5", "l6"], "profile_sketch": "45x45 section", "anchor": "bottom_left", "angle": "90 deg"}}
```

Members are ordinary bodies: mirror, pattern, move, drill and combine them with the usual
commands. Where members meet they overlap until they are trimmed or mitred.

The **Weldments** ribbon tab gathers the frame commands: New Sketch, Frame Member, End Cap,
Trim/Extend and Parameters. Like Sheet Metal it is always there, and it selects itself when a frame member is
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
it. A changed member is also refused as a member target, because its sketch line and profile
may no longer describe its actual side. Use a planar face target to follow the modified body.
This includes symmetric rotations whose outline stays the same and bodies transferred into
another component after the member was made. Members of another component
are not supported yet.

### The Trim/Extend dialog

- **Members**: pick the members to trim (their bodies or faces of them).
- **To**: pick members, planar faces or planes. A face of an unmodified member selects that
  member; a face on a modified member is kept as a planar-face target.
- **End** (Nearer End, Start, End) and **Gap** as above.
- **Edit Feature** reopens the dialog with the trim's picks and values. Targets a pick can't show
  (a member's chosen side, a face, a plane given by its origin and normal) are listed as **Keep:
  n more targets**, kept while ticked; untick it to drop them all and pick again.

## Corners

`frame.corner` joins two members that meet at their ends in a **mitre**: both ends are cut on the
plane halfway between the members, so a right-angled corner cuts each at 45° and a 60° corner each
at 60° from square. A corner extends or shortens each member to that plane, as a trim does, and
each member keeps its body and its name.

```json
{"command": "frame.corner", "params": {"members": ["Rail", "Rail (2)"]}}
{"command": "frame.corner", "params": {"members": ["Rail (2)", "Rail (3)"], "gap": "2 mm"}}
```

- `members`: the two members, each a member's body name or `{feature, line}`.
- `kind`: `mitre` (the default; butt and end trim come later).
- `gap`: the weld gap between the two cut faces, a length expression (default 0, at most 1 m).
  Each face stands half the gap back from the mitre plane, so an end moves along its member by
  half the gap over the cosine of the angle between the member and the plane's normal: `gap / √2`
  at a right angle.

The members meet where the lines through the middle of their profiles cross. That point must be
at an end of each member, within its profile's size of the end (members drawn through their
profiles' middles, or along their outsides or insides, all qualify). A corner is refused, with
the reason, when the members are parallel, when their middle lines pass each other without
meeting, when they cross away from a member's ends (a T: trim it instead), when one member is given
twice, and, as for a trim, when a member was changed by a later feature. There is no dialog or ribbon
button yet: the corner is a command for scripts, the CLI and MCP, and `timeline.redefine` changes it.

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
