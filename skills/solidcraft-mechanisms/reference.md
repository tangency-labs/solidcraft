# SolidCraft mechanism reference for agents

These are the shapes of the commands an assembly needs, so an agent rarely has to call `list_commands`. All of them run through the `execute` tool or a `batch`. The worked example below runs in CI (`mechanisms_skill_example_moves_as_documented` in `crates/mcp/src/tests.rs`).

- Joint values, limits and study keys are in degrees for angles and mm for distances. Expressions work too: `"30 deg"`, `"stroke/2"`.
- Points and axes are world coordinates, as the model sits when you make the joint.

## Joint origins

Each of `a` and `b` in `joint.create` is one of these:

| Form | Shape | Notes |
|---|---|---|
| Point | `{"occurrence":"Wing","point":[0,30,0],"z":[1,0,0]}` | The usual form. `z` is the joint axis (default +Z). |
| Frame | `{"occurrence":"Wing","frame":{"origin":[0,30,0],"z":[1,0,0],"x":[0,1,0]}}` | `x` fixes where angle 0 is. |
| Face | `{"occurrence":"Wing","face":[0,20,3]}` | The centre of the planar face through that point, axis along its normal. It follows the face when the part changes. |
| Circle | `{"occurrence":"Actuator rod","circle":[0,32,0]}` | The centre of the round edge through that point, axis along the cylinder. Pick a point on the rim. |
| Named | `"Hinge on frame"` | Made first with `joint.origin {"name":..., "occurrence":..., "point":[...], "z":[...]}`. |

- `occurrence` is an occurrence name (`Wing:1`), an id, or a component name (its first occurrence). Without it, the body nearest the point is used, which can be the wrong part where two touch.
- **Use the same form on both sides.** `point` picks its x axis from z. A `frame` with an `x` on one side and a `point` on the other turned a test panel 90 degrees when it was joined.
- **`flip`:** `false` (the default) mates the origins as two faces meet: b's z points against a's. Two origins at one point with the same `z` then turn part b 180 degrees about the axis, and a joint that closes a loop reports a conflict ("off by 71 mm and 180 degrees" on SELENE). `true` lines b's z up with a's, so parts stay where they are. `joint.edit {"joint":..., "flip":true}` changes it afterwards.

## Joint types

| `type` | Values, in order | Units |
|---|---|---|
| `rigid` | none | |
| `revolute` | angle | deg |
| `slider` | distance along z | mm |
| `cylindrical` | angle, distance | deg, mm |
| `pin_slot` | angle, slide | deg, mm |
| `planar` | x, y, angle | mm, mm, deg |
| `ball` | yaw, pitch, roll | deg |

A slider's value grows as b moves along +z of its origin. Give its two origins the same point, so 0 is the position as modelled.

## Commands

```json
{"command":"component.create","params":{"name":"Actuator rod","parent":"root","activate":false}}
{"command":"component.move_bodies","params":{"bodies":["Rod"],"component":"Actuator rod"}}
{"command":"occurrence.ground","params":{"occurrence":"Frame:1","grounded":true}}
{"command":"joint.create","params":{"type":"slider","name":"Stroke","flip":true,"limits":[[-5,10]],"a":{"occurrence":"Actuator barrel","point":[0,0,-40],"z":[0,0.6,0.8]},"b":{"occurrence":"Actuator rod","point":[0,0,-40],"z":[0,0.6,0.8]}}}
{"command":"joint.as_built","params":{"type":"revolute","a":"Frame","b":"Wing 2","at":{"occurrence":"Frame","point":[0,0,0],"z":[1,0,0]},"name":"Hinge 2","limits":[[-60,0]]}}
{"command":"joint.rigid_group","params":{"occurrences":["Frame:1","Lamp:1"]}}
{"command":"joint.motion_link","params":{"a":"Hinge","b":"Hinge 2","ratio":-1}}
{"command":"joint.drive","params":{"joint":"Hinge","value":20}}
{"command":"joint.drive","params":{"joint":"Spindle","values":[30,5]}}
{"command":"joint.reach","params":{"occurrence":"Gripper","at":[120,0,40],"point":[60,80,40],"z":[0,0,-1],"axis":[1,0,0],"quiet":true}}
{"command":"joint.limits","params":{"joint":"Hinge","min":0,"max":60}}
{"command":"joint.edit","params":{"joint":"Hinge","flip":true}}
{"command":"joint.delete","params":{"link":0}}
{"command":"contact.create","params":{"occurrences":["Frame:1","Wing 2:1"],"name":"Wing 2 stop"}}
{"command":"contact.enable_sets","params":{}}
{"command":"motion.study","params":{"name":"Open","steps":10,"keys":[{"joint":"Hinge","points":[[0,0],[10,25]]}]}}
{"command":"motion.play","params":{"study":"Open","step":5}}
{"command":"motion.export","params":{"study":"Open","samples":3}}
{"command":"occurrence.properties","params":{"occurrence":"Wing:1"}}
```

- `joint.create` and `joint.as_built` start the joint at its `values` (default 0). A value outside the limits starts at the nearer limit, and the reply's `warnings` say so: the part turned as it was joined.
- `joint.as_built` takes occurrences, not origins. `at` is the joint's frame (default: b's component origin).
- `joint.rigid_group` joins every occurrence to the first one with a rigid as-built joint.
- `joint.motion_link`: b = ratio·a + offset, clamped to b's limits. `ratio` is in b's units per a's: mm per degree when an angle drives a distance (a pinion of pitch radius r moves its rack r·π/180 mm per degree), degrees per mm the other way, and a plain factor between two angles or two distances. `offset` is in b's units: degrees for an angle, mm for a distance, or an expression such as `"10 deg"`. `joint.list` gives each link's `ratio` the same way and its `offset` with its `unit`. `ia`/`ib` pick a value of a multi-value joint (default 0). `joint.delete {"link": n}` removes link n (its index in `joint.list` `links`).
- `joint.gear {a, b, ra, rb, internal?, reverse?}`, `joint.rack_pinion {a, b, radius, reverse?}` and `joint.screw {joint | a, b; pitch, reverse?}` make a motion link whose ratio comes from its sizes: gears -ra/rb (+ra/rb internal), a rack r mm per radian of its pinion (either joint may be `a`), a screw pitch per turn. Each size is mm, an expression, or `{occurrence, circle: [x,y,z]}`: the radius of the round face there, read again whenever the design changes. The reply gives `link` (its index) and `ratio` as `joint.list` does; `link: n` in the params replaces link n and keeps its offset. A new coupling's offset keeps b where it is. `joint.list` `links` carry the `coupling` and an `error` when it can't be worked out (a picked face gone: the last ratio stays). A value two links would set, or a coupling that would drive its own driver, is refused.
- `joint.cam {a, b, profile, follower?, radius?, tip?, flip?, link?}`: a is the cam's revolute joint, b a slider or revolute follower. `profile` is `{sketch, index?}` (a closed profile of a sketch in the cam's component) or `{edges: [[x,y,z]...], occurrence?}` (edges of the cam's body; one edge continues around its closed loop in the plane). The reply gives `link`, `lift: {min, max, rise, unit}` (b's travel over a turn), `reverse` (the side it rides from; `flip: true` takes the other) and, for a slider, `follower_offset`. `joint.list` `links` show `lift` instead of `ratio` for a cam.
- `joint.drive`: `values` drives the values it lists, in order (`Spindle` above is cylindrical: 30 degrees, then 5 mm), and `value` the first; values left out keep theirs, or follow in a closed loop. Values are clamped to the limits. The reply gives `values` as `joint.list` does (`{name, value, unit, limits}`), and `moved`; when the joint can move nothing, a `reason`.
- `joint.reach` (inverse kinematics): moves the joints on the occurrence's chain so its point `at` (world, where it is now; default: the centre of its bodies) gets to `point`, from the pose it is in to the nearest one, within limits, through motion links, and with closed loops kept closed. `z` with `axis` (world, now; default its own z) also turns that axis. `joints: [...]` moves only those (loop followers still follow). The reply: `reached`, `error_mm` (and `error_deg`), `point` where it ended, `values` of the joints that moved, `moved`, and when short, `stopped_by_limit: [{joint, value, limit}]` or `stopped_by_loop: {joint}`. A part that nothing moves answers `moved: false` with a `reason`. Contact sets are not checked; check with `contact.check` afterwards.
- `motion.study {"study":..., ...}` edits a study; `keys` given replace all keys.
- `motion.play` sets the joints to the study's values at a step (fractions work) and changes the design. `motion.export` does not change it.

## Worked example: a panel on an actuator

A frame holds a panel on a hinge along X at the origin. An actuator runs from a root pin on the frame, R = (0, 0, -40), to a tip pin on the panel, T = (0, 30, 0), 30 mm from the hinge: 50 mm long, along (0, 0.6, 0.8). Its rod slides -5 to +10 mm. A second panel on the other side mirrors the first through a motion link.

```json
{"commands":[
 {"command":"solid.box","params":{"length":40,"width":10,"height":50,"corner":[-20,-10,-50],"operation":"new","body_name":"Post"}},
 {"command":"solid.box","params":{"length":40,"width":40,"height":10,"corner":[-20,0,-50],"operation":"new","body_name":"Base"}},
 {"command":"solid.box","params":{"length":40,"width":40,"height":3,"corner":[-20,0,0],"operation":"new","body_name":"Panel"}},
 {"command":"solid.box","params":{"length":40,"width":40,"height":3,"corner":[-20,-40,0],"operation":"new","body_name":"Panel 2"}},
 {"command":"solid.cylinder","params":{"radius":5,"height":20,"base":[0,0,-40],"axis":[0,0.6,0.8],"operation":"new","body_name":"Barrel"}},
 {"command":"solid.cylinder","params":{"radius":2,"height":35,"base":[0,9,-28],"axis":[0,0.6,0.8],"operation":"new","body_name":"Rod"}},
 {"command":"component.create","params":{"name":"Frame","parent":"root","activate":false}},
 {"command":"component.move_bodies","params":{"bodies":["Post","Base"],"component":"Frame"}},
 {"command":"component.create","params":{"name":"Wing","parent":"root","activate":false}},
 {"command":"component.move_bodies","params":{"bodies":["Panel"],"component":"Wing"}},
 {"command":"component.create","params":{"name":"Wing 2","parent":"root","activate":false}},
 {"command":"component.move_bodies","params":{"bodies":["Panel 2"],"component":"Wing 2"}},
 {"command":"component.create","params":{"name":"Actuator barrel","parent":"root","activate":false}},
 {"command":"component.move_bodies","params":{"bodies":["Barrel"],"component":"Actuator barrel"}},
 {"command":"component.create","params":{"name":"Actuator rod","parent":"root","activate":false}},
 {"command":"component.move_bodies","params":{"bodies":["Rod"],"component":"Actuator rod"}},
 {"command":"occurrence.ground","params":{"occurrence":"Frame:1","grounded":true}},
 {"command":"joint.create","params":{"type":"revolute","name":"Hinge","flip":true,"a":{"occurrence":"Frame","point":[0,0,0],"z":[1,0,0]},"b":{"occurrence":"Wing","point":[0,0,0],"z":[1,0,0]}}},
 {"command":"joint.create","params":{"type":"revolute","name":"Root pin","flip":true,"a":{"occurrence":"Frame","point":[0,0,-40],"z":[1,0,0]},"b":{"occurrence":"Actuator barrel","point":[0,0,-40],"z":[1,0,0]}}},
 {"command":"joint.create","params":{"type":"slider","name":"Stroke","flip":true,"limits":[[-5,10]],"a":{"occurrence":"Actuator barrel","point":[0,0,-40],"z":[0,0.6,0.8]},"b":{"occurrence":"Actuator rod","point":[0,0,-40],"z":[0,0.6,0.8]}}},
 {"command":"joint.create","params":{"type":"revolute","name":"Tip pin","flip":true,"a":{"occurrence":"Actuator rod","point":[0,30,0],"z":[1,0,0]},"b":{"occurrence":"Wing","point":[0,30,0],"z":[1,0,0]}}},
 {"command":"joint.as_built","params":{"type":"revolute","a":"Frame","b":"Wing 2","at":{"occurrence":"Frame","point":[0,0,0],"z":[1,0,0]},"name":"Hinge 2","limits":[[-60,0]]}},
 {"command":"joint.motion_link","params":{"a":"Hinge","b":"Hinge 2","ratio":-1}}
]}
```

**Expected:**
- After the batch, `joint.list` reports `dof` 1 for Wing, Wing 2, the barrel and the rod (the loop and the link leave one motion), 0 for Frame, and no `conflicts`.
- **Actuator length:** the tip pin turns to `(0, 30 cos θ, 30 sin θ)`, so `L(θ)² = 2500 + 2400 sin θ` and `L(0) = 50`. The stroke is `L(θ) - 50`.
- `joint.drive {"joint":"Hinge","value":20}`: the stroke is `L(20°) - 50 = 7.6268` mm, and Hinge 2 is -20. `occurrence.properties {"occurrence":"Wing:1"}` puts the panel's centre of mass at (0, 18.281, 8.250), and Wing 2's at (0, -18.281, 8.250).
- `joint.drive {"joint":"Hinge","value":45}` stops at 27.28 degrees with `stopped_by_loop: {"joint":"Stroke", ...}`: the rod reaches +10 mm where `L(θ) = 60`, at `sin θ = 11/24`, θ = 27.280 degrees.

## At scale: SELENE

The same pattern moved the solar wings of the SELENE rover (`Rover-v7.solidcraft` by 0xlillith, 36 occurrences, 1353 features):
- **Actuators split:** each of the four actuators was one component. Its barrel and its rod (with the eyes and pins that move with them) went into components of their own with `component.create` and `component.move_bodies`.
- **Loops:** per actuator, a revolute root pin (structure and barrel), a slider with the stroke as its limits, `[[-6, 0.4]]` mm, and a revolute wing pin (rod and wing). All `flip: true`, origins by world point, with z along X for the pins and along the actuator (root to tip) for the slider.
- **Links:** the six wheels follow the first (`ratio: 1`); the starboard tilt mirrors the port one (`ratio: -1`).
- **Floating modules:** in v7, 18 components (pedestals, avionics, the lid and others) were neither grounded nor joined, so driving the turret or the dish moved nothing. One `joint.rigid_group` with the structure fixed it.
- **Hidden:** the arm's flexible hoses are rigid bodies on one link, and their sweep sketches stay in place, so both drift off as the arm moves. `browser.visibility {"items":[...], "visible":false}` hid them, and `{"folder":"joints","visible":false}` the joint glyphs for the recording.
- **Numbers:** the port tilt hinge is at (0, -140, 158) along X, the actuator root at (x, -91, 134) and its wing pin at (x, -180.41, 169.71). From these, the stroke at 20 degrees is -2.87 mm, and the rod reaches -6 mm at 32.230 degrees. The app agrees: a drive to 20 degrees gives -2.87 mm, and a drive towards 45 degrees stops at 32.23 degrees with `stopped_by_loop`.

## Traps

| Symptom | Cause and fix |
|---|---|
| A part turns 180 degrees when joined; a loop reports "off by ... and 180 degrees" | `flip` left out. Pass `flip: true`, or `joint.edit {"joint":..., "flip":true}`. |
| A part turns to a limit as soon as it is joined, and the reply has `warnings` | A new joint starts at 0, or its `values`, inside its limits: `[[10, 60]]` on a hinge turns it 10 degrees. Make the limits include 0, or join the parts where the limits allow. |
| A part turns by some other angle when joined | The two origins have different forms or x axes. Use `point` + `z` on both sides, or `joint.as_built`. |
| `joint.drive` answers `moved: false`, or `motion.play` lists the joint under `unmoved` | Read `reason`. Most often the chain is joined to nothing grounded: `unconnected` names the part, and `joint.list` lists the chain under `floating`, its occurrences with `dof: 6`. Ground it, join it, or `joint.rigid_group` it with the structure. |
| A drive stops short, `stopped_by_loop` | A loop reached a limit or can't close further; `joint` names the joint that stopped it. Check the limits against your stroke numbers. |
| A gear, rack or screw turns its partner the wrong way | The joints' axes point opposite ways (or the rack lies on the other side of the pinion). Run the command again with `reverse: true` and `link: n`. |
| `joint.cam` says the follower's line misses the cam | The slider's line (through `tip` when given) passes beside the cam at some angle. Pick the follower's tip, or move its slider's axis over the cam. |
| A cam's follower rides the wrong flank or sinks into the cam | It took the other side of the cam's axis (`reverse` in the reply): run `joint.cam` again with `flip: true` and `link: n`. |
| `already follows a motion link` from `joint.gear` | A joint follows one link at most. Make the joint that already follows the driving one (`a`), as in a gear train G1 → G2 → G3. |
| A linked joint stops following | Its value is clamped to its own limits. Hinge 2 in the example has `[[-60, 0]]`, so a drive of Hinge below 0 leaves it at 0. Give a mirrored joint the mirrored limits. |
| `joint.reach` answers `reached: false` | The point is out of reach: `point` and `error_mm` give the nearest pose. `stopped_by_limit` names the limits it rests on, `stopped_by_loop` the loop that can't follow further; with neither, the chain can't get there at all (too far, or out of its plane). |
| A drive stops short, `stopped_by_contact` | Two bodies of a contact set touch. `fraction` is how far the move got. |
| `occurrence.ground` says there is no occurrence `Frame` | It takes occurrence names only: `Frame:1`. |
| `a joint needs two different occurrences` | Both origins found the same part. Give `occurrence` on both. If the parts are bodies of one component (an imported STEP file often brings them so), run `component.from_bodies {"bodies": [...]}` first. |
| "unknown command" for `file.open` | Open a design with the `open` tool or `doc.open {path}`. |
| Rigid hoses, cables or springs drift as the mechanism moves | Bodies don't bend. Hide them with `browser.visibility`, or model them per link. |
| `solidcraft-cli snapshot SCRIPT --in design` runs on an empty design | `snapshot` ignores `--in`. Run `solidcraft-cli run SCRIPT --in design --save out.sc` first. |
| A URDF export lacks a joint | `export` to `.urdf` leaves out each joint that closes a loop, with a warning, because URDF is a tree. |

## Reply cost

| Call | Size |
|---|---|
| `component.list` | **140 KB** on SELENE (a 4x4 transform per occurrence). Avoid it; use `occurrence.properties`. |
| `joint.list` | about 0.5 KB per joint: 16 KB for 29 joints |
| `motion.play` | about 0.2 KB per occurrence, per step: 9 KB on SELENE |
| `motion.export` | the same per sample; pass `samples`, or `path` to write it to a file |
| `joint.create`, `joint.drive`, `joint.motion_link`, `joint.gear`, `joint.rack_pinion`, `joint.screw` | the `dof` list of every occurrence again, 40 B each; `quiet: true` leaves it out |
| `occurrence.properties` | under 1 KB per occurrence |

## Showing it moving

`motion.export_animation {"study":..., "path":"tour.gif"}` draws the whole motion as an animated GIF in one call, headless or bridged, and changes nothing: `{"joint":..., "from":0, "to":90, "path":...}` animates a drive instead, and `{"view":..., "back":true, "path":...}` an exploded view coming apart and back together (the placements stay as they are). A path that does not end in `.gif` is a folder for PNG frames (`frame-0001.png`, ...). Pass `camera` (the reply of `ui.camera`, or any of its fields), `width`/`height` (default 800 x 500), `samples` (frames; default the study's steps + 1, 100 for a joint, 41 for a view or 81 with `back`), `fps` (default 20), and `grid: false` / `axes: false` for a clean background. The reply is small: `frames`, `bytes`, and `stopped_at` with `stopped_by_loop`/`stopped_by_contact` where the motion first stopped short.

Bridged to the app, `motion.play {"study":..., "step":s}` moves the user's viewport, and `screenshot` captures the window at that step.
