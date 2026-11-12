# CAM jobs

A CAM job turns a design's profiles and surfaces into G-code for one machine. It is a JSON file kept next to
the design (or an object passed inline); nothing of it is saved in the `.sc` file. These
commands work with jobs, from scripts, the CLI and MCP alike:

| Command | What it does |
| --- | --- |
| `cam.job` | Plans every operation, writes the program through the job's post, reads it back and checks it, and writes the G-code (and an HTML preview) |
| `cam.post.list` | Lists the built-in posts |
| `cam.post.get` | Returns a built-in post as JSON, to copy into a post file |
| `cam.gcode.check` | Reads a G-code file and reports lengths, time, the rotary range, inverse-time blocks and the first line it cannot read |

Units are millimetres and mm/min. Angles in a job are degrees (`ramp_angle_deg`, …).

## `cam.job`

```json
{"job_path": "C:/parts/bracket.cam.json", "output": "C:/parts/bracket.ngc", "preview": "C:/parts/bracket.html"}
```

| Key | Meaning |
| --- | --- |
| `job` | The job as an object. Give this or `job_path`. |
| `job_path` | A `.json` file holding the job. Relative `post_path` and `tool_library_path` in it are read from the file's folder. |
| `output` | The G-code file. Required unless `dry_run`. |
| `preview` | An HTML page of the part and every toolpath: self-contained (no network), one view per operation and one for the whole job, orbit and zoom, a slider that plays the moves, rapids dashed. It must be another file than `output`, however the two are spelt. |
| `dry_run` | Plan and check, write nothing; the reply says whether the checks pass instead of refusing. |
| `allow_unsafe` | Write the program even when a check fails. Its first line (the program's name, as a comment) then ends `WRITTEN WITH FAILED CHECKS`, so the file says so; no line moves. |

Every file is read and written through the session's file access, so an MCP server's
`--allow-path` folders apply; every path is checked before anything is computed or written.
Each file is written whole to a temporary file beside the old one and renamed over it.

### The checks

The command never writes a program that fails one of these unless `allow_unsafe` is given:

- **Read-back.** The program as written is read back through the machine's kinematics and
  compared with the toolpaths, both ways round: the tip within the post's precision and
  tolerances (`readback.allowed_tip`), the tool axis within its angle precision.
- **Operation checks** that are part of planning, each refusing the job with a message that
  names the operation: a tool that cannot cut the operation (below), heights out of order, a
  stepover wider than the tool, a profile the tool does not fit in.

Without `dry_run` or `allow_unsafe`, a failure refuses the call with every failed check in the
message. With `dry_run`, the reply carries `passed: false` and `failures`.

### The reply

`name`, `machine`, `post`, `operations` (each with `name`, `kind`, `tool`, `moves`,
`cut_length`, `cut_minutes`, `warnings`), `program_lines`, `program_head` (the first lines),
`readback` (`max_tip_deviation`, `max_axis_deviation_deg`, `allowed_tip`, `allowed_axis_deg`,
`cut_minutes`, `pass`), `cut_minutes` (the program's time at feed as written), `passed`,
`failures`, `dry_run`, `outputs` (`gcode`, `preview`) and `notes` (a post that writes no work
offset, an output extension other than the post's, in a dry run too). The operations' minutes
add up to `readback.cut_minutes`, the program's as the controller times it.

## The job

```json
{
  "name": "bracket",
  "machine": "3axis",
  "post": "linuxcnc_4axis",
  "origin": [0, 0, 0],
  "tolerance": 0.01,
  "tools": [
    {"number": 1, "kind": "flat_endmill", "diameter": 6, "flute_length": 20, "spindle_rpm": 12000, "feed": 800, "plunge_feed": 200},
    {"number": 2, "kind": "flat_endmill", "diameter": 4, "flute_length": 12, "spindle_rpm": 14000, "feed": 600, "plunge_feed": 150}
  ],
  "operations": [
    {"name": "top pocket", "type": "pocket", "tool": 2, "polygon": [[-10, -5], [10, -5], [10, 5], [-10, 5]],
     "heights": {"clearance": 30, "retract": 5, "top": 0, "bottom": -4}, "stepdown": 1.5},
    {"name": "outline", "type": "contour", "tool": 1, "sketch": "Outline",
     "heights": {"clearance": 30, "retract": 5, "top": 0, "bottom": -10}, "stepdown": 3}
  ]
}
```

Every object refuses keys it does not know, so a misspelt key is an error, never a setting
silently left at its default. That holds for the tools, the machine and the post too.

| Key | Meaning |
| --- | --- |
| `name` | The program's name, in its header. Default: the design's name. |
| `machine` | `"3axis"`, `"4axis_a"` (a rotary table about +X, the A axis, through work zero), or a whole machine as the CAM crate's `Machine` JSON (`name`, `rotaries: [{letter, direction, center, carrier, limits?, reversed?}]`, `linear_limits?`, `work_origin?`; radians). |
| `a_reversed` | With `4axis_a`: the controller's positive A turns the table the other way. |
| `a_limits_deg` | With `4axis_a`: `[lowest, highest]` A in degrees. Default: it turns without end. |
| `post` | A built-in post (`cam.post.list` lists them): `linuxcnc_4axis`, `grblhal_4axis`. |
| `post_path` | A post as a JSON file: the CAM crate's `Post5` (`cam.post.get {"name": …}` returns one to start from). This is how a machine's own post is matched: copy the closest built-in post and change its header, tool change, spindle, comments, decimals, inverse time and rotary settings until its output matches. |
| `work_offset` | 1 to 6 (G54 to G59), for posts whose header writes `{work_offset}`. Default: 1. |
| `origin` | The design point that is work zero, `[x, y, z]`. Default: the design's origin. |
| `bodies` | The part's bodies by name: the surface the 3D operations follow, drawn in the preview. Default: every visible body. |
| `tolerance` | 0.0001 to 1 mm (default 0.01): how far a toolpath may be from the profiles and surfaces it follows. |
| `tools` | The tools, as the CAM crate's `Tool` JSON (below). Give this or `tool_library_path`. |
| `tool_library_path` | A JSON file holding an array of tools. |
| `operations` | 1 to 200 operations, run and written in order (below). |

### Coordinates

Work coordinates are the design's coordinates less `origin`. The operations' polygons and
heights are in work coordinates, and a sketch is read through the same shift. On a 4th axis
the rotary axis is work X through work zero; a contour or a pocket runs with A held at zero.

### Tessellation

The 3D operations (`parallel`, `roughing`, `waterline`, `scallop`, `steep_shallow`, `pencil`,
`rest_finish`) follow the part's bodies tessellated from their exact B-rep surfaces with a chord
tolerance of a quarter of the job's tolerance (`MeshTolerance::chord`, the tolerance-driven
path the mesh exports are built on). The facets then lie within a quarter of the tolerance of
the true surfaces: inside a convex surface, outside a hollow one. A 3D operation is planned on
the facets within the rest of its tolerance (the operation's `tolerance`, the job's by default,
less the chord error; at least half of it): four fifths of that for the operation itself and
one fifth for merging its moves (below). A mesh body (an imported STL, 3MF or OBJ) is used as
it is; the reply's notes say so. The toolpaths are output, made within the tolerance as an
export is; they are not yet planned on the exact surfaces. A job of contours and pockets alone
tessellates nothing for planning.

### Tools

`number`, `kind` (`flat_endmill`, `ball_endmill`, `bull_nose`, `drill`, `face_mill`, `chamfer`,
`thread_mill`), `diameter`, `flute_length`, and optional `description`, `flutes`,
`spindle_rpm`, `feed`, `plunge_feed`, `stepdown`, `stepover`, `corner_radius` (bull nose; a
face mill's inserts), `tip_diameter` and `angle_deg` (chamfer mill), `shank_diameter`,
`stickout`, `holder: [{diameter, length}]`, `point_angle_deg` (a drill's point, default 118),
and for a thread mill `thread_pitch` and `tooth_height`. An operation without `spindle_rpm` or
`feeds` takes the tool's; its stepdown and stepover default to the tool's too.

A `contour`, a `pocket` or a 3D operation cuts with a flat end mill, a ball end mill, a bull
nose or a face mill; any other tool is refused, with the operation named.

### Operations

Every operation has `name`, `type`, `tool` (a tool's `number`), and optionally `spindle_rpm`,
`feeds: {cut, plunge, descent?}`, `coolant` (default false) and `enabled` (default true: `false`
keeps it in the job but leaves it out of the program). `heights` are `{clearance, retract, top,
bottom}` in work coordinates.

| `type` | Keys (defaults) |
| --- | --- |
| `contour` | `sketch` (id or name) + `profiles?` (indices; default every drawn profile) or `polygon: [[x, y], …]`; `heights`; `stepdown` (tool's, else half its diameter); `side`: `outside` \| `inside` \| `on` (outside); `climb` (true); `stock_to_leave` (0); `ramp_angle_deg` (0: plunge); `tolerance` (the job's) |
| `pocket` | `sketch` + `profiles?` or `polygon`; `heights`; `stepdown`; `stepover` (tool's, else 40% of its diameter); `climb`; `stock_to_leave`; `ramp_angle_deg`; `tolerance` |
| `parallel` | `heights` (`bottom` is the floor); `stepover` (tool's, else 10% of its diameter); `angle_deg` (0); `boundary?: {rect?: [[x, y], [x, y]], polygon?, holes?}` (the part's extent); `stock_to_leave` (0); `step` (a quarter of the diameter); `tolerance`; `zigzag` (true); `descent_limit_deg` (70: see Feeds); `feeds.descent` (the cut feed: see Feeds) |
| `roughing` | `heights` (`top` is the stock's top); `stepdown`; `stepover`; `angle_deg`; `boundary`; `stock_to_leave` (0.25); `step`; `tolerance`; `zigzag` |
| `waterline` | `heights` (levels from `top` to `bottom`); `stepdown` (tool's, else a quarter of its diameter); `boundary`; `stock_to_leave` (0); `step`; `tolerance`; `climb` (true) |
| `scallop` | `heights` (`bottom` is the floor); `scallop_height` or `stepover` (tool's, else 10% of its diameter; along the surface); `direction`: `inward` \| `outward` (inward); `boundary`; `stock_to_leave` (0); `step` (a tenth of the diameter: the grid and the samples); `tolerance`; `climb` (true). See [3D finishing strategies](#3d-finishing-strategies). |
| `steep_shallow` | As `scallop`, with `threshold_deg` (40), `overlap` (a tenth of the diameter), `stepdown` (the spacing × sin threshold), `shallow`: `scallop` \| `parallel` (scallop), `angle_deg` (with `parallel`; 0) |
| `pencil` | `heights`; `min_angle_deg` (30); `passes` (0: the crease alone); `stepover` (tool's, else 10% of its diameter); `min_length` (the tool's radius); `boundary`; `stock_to_leave` (0); `step`; `tolerance`; `climb` |
| `rest_finish` | `heights`; `previous_tool` (a tool number of the job); `threshold` (twice the tolerance); `scallop_height` or `stepover`; `overlap` (a quarter of the diameter); `boundary`; `stock_to_leave` (0); `step`; `tolerance`; `climb` |

A sketch an operation uses must lie on a plane square to the tool, facing +Z or -Z (one facing
away is read as seen from the tool, mirrored), in the design's root component. Contours and
pockets are written with arcs kept (`G2`/`G3`). The moves of a 3D operation are merged before
they are written: a run of straight cuts at one feed whose points all lie within the merging
tolerance of one line becomes that line, and, where the post writes arcs (`arcs`, and
`helical_arcs` for one that changes Z), a run that lies within it of an arc in the XY plane
(`G17`) becomes that arc. The merging tolerance is a fifth of the operation's part of the
tolerance, and never more than the post's `arc_tolerance`; the read-back holds the program
against the merged moves. An arc is taken only where it bows out between two of the points it
replaces by no more than the tolerance the operation was planned within, so an arc through
points sampled closely from a curve is taken as the curve, and one through the corners of a
polygon is not.

### Feeds

Every move runs at the operation's cut feed except these, which go down no faster than the
plunge feed (the descents of a 3D finish: no faster than its descent feed):

- **Entries.** Each cut is entered from above, the last of the way at the plunge feed. A
  `roughing` level comes down at rapid through the air the levels before it cleared, to 1 mm
  above the material left there (the level before, or `top`), and plunges from there. A
  `pocket` level comes down the same way to 1 mm above the level before it, and the first one,
  like a `contour`, to 1 mm above `top` (where that is below the retract height). A `parallel`,
  `waterline`, `scallop`, `steep_shallow`, `pencil` or `rest_finish` cut is entered from the
  retract height, and the links between its passes follow the part.
- **Descents.** Inside a pass of a 3D finish, a move that goes down at least
  `descent_limit_deg` from horizontal (default 70°; `scallop`, `steep_shallow`, `pencil` and
  `rest_finish` always 70°) is slowed so the tool goes down no faster than the descent feed,
  `feeds.descent`: by default the cut feed, so a ball running down a roughed wall keeps the cut
  feed (the finish runs over the stock to leave a roughing left, a side cut, not a plunge). Give
  the plunge feed as `feeds.descent` to slow them, as when finishing straight from uncut stock.
  A `descent_limit_deg` of 0 slows every move that goes down, 90 none. In `roughing` every move
  that goes down is slowed to the plunge feed: it goes into material no level has cut.
- **Ramps.** A `contour` or `pocket` with `ramp_angle_deg` goes down along the profile at
  exactly that angle (the last lap only part of the way round, then on round at the depth),
  each ramping move slowed so the tool goes down no faster than the plunge feed.

These feeds are worked out again from the coordinates as the post writes them, and rounded
down to its feed digits, so the program as written never goes down faster than the plunge feed
either.

## 3D finishing strategies

`scallop`, `steep_shallow`, `pencil` and `rest_finish` place their passes by the shape of the
surface as the cutter meets it, where `parallel` and `waterline` use straight lines and flat
levels. All four start from a grid of drop-cutter samples `step` apart over the boundary (the
tip's height where the cutter rests, never below the floor `heights.bottom`) and measure
distances over that surface, so neighbouring passes are a spacing apart along the surface,
whatever its slope. Every pass is then sampled and refined with the drop-cutter exactly as a
`parallel` pass is: no point it cuts at is below where the cutter rests, moves that go down are
fed no faster down than the plunge feed, and passes are linked by feeding across when the link
is short and clears the part, otherwise by lifting to the retract height. With `climb` (the default) every offset pass and
z-level keeps what is still to cut on the right of the tool's travel (the spindle turning
clockwise seen from above), whichever way the passes run: inward, outward, or closing on a
crease; `climb: false` keeps it on the left. A grid larger than 4 million nodes, more than
10 000 levels, straight shallow passes of more than 4 million points or more drop-cutter
samples than the operation's budget is refused with a message saying what to make larger.

- **`scallop`** (constant stepover): passes offset from the boundary inward (`direction:
  "outward"` runs the same passes from the middle out). With `scallop_height` the spacing is
  `2·√(2·r·h − h²)` for the cutter's corner radius `r` (a flat end mill needs `stepover`),
  closer where the surface is concave and further where it is convex, so the ridge left is the
  same. Where fronts from different sides meet further apart than the spacing, a pass is added
  between them. A last pass runs along every concave crease sharper than 30° (the foot of a
  wall, a corner), which offsets land a grid step short of at best. Walls the cutter's side runs
  down (cliffs in the height field) are not followed: use `steep_shallow` there.
- **`steep_shallow`**: where the tip's height is steeper than `threshold_deg`, z-levels at most
  `stepdown` apart, each crossing found to within the tolerance; elsewhere `scallop` passes
  offset from the boundary and from the edge of the steep ground, or straight passes at
  `angle_deg` (`shallow: "parallel"`), close enough to be a spacing apart on the steepest
  shallow ground. Each kind runs `overlap` on into the other's ground; the crease passes of
  `scallop` finish it.
- **`pencil`**: along the concave creases where the cutter touches two faces at once. A crease
  is where the tip's path turns up by at least `min_angle_deg` at one place across it, more than
  it turns either side (so a fillet the cutter rolls along is no crease); each crease point is
  moved across the crease to where the tip's height leaves the line of its flatter side, found by
  halving, so the pass runs in the crease, not on the nearest grid node. `passes` adds offset
  passes `stepover` apart along the surface on both sides, outermost first.
- **`rest_finish`**: only where this tool reaches material `previous_tool` left. At each grid
  node the tool's contact point and the surface's normal there come from its tip and the slope
  of the tip's height; the previous tool is set touching the same point with the same normal and
  dropped. Where it rests higher than that, what it left at the point is measured from the
  envelope of every position it can take on the grid (the lowest its cutting surface reaches
  over the point), square to the surface. Places with more than `threshold` left, grown by
  `overlap`, are cut with passes offset from their edges inward, and their creases.

Measured on test parts with a 6 mm ball and `scallop_height: 0.01` (the material left over what
any path with the tool could reach, from the envelope of the ball along the path, computed
independently of the strategies on a 0.1 mm grid; `crates/cam/src/finish3d_tests.rs`):

| Part | Strategy | Largest | 95% of points under | Points within 0.0125 mm |
| --- | --- | --- | --- | --- |
| Dome (radius 12, 7 mm high) | `scallop` | 0.019 mm | 0.0096 mm | 99.7% |
| Bowl (60° wall, concave fillet radius 4) | `scallop` | 0.021 mm | 0.0109 mm | 96.6% |
| Bowl | `steep_shallow` (40°, scallop) | 0.023 mm | 0.0100 mm | 97.4% |
| Bowl | `steep_shallow` (40°, parallel) | 0.027 mm | 0.0076 mm | 98.6% |
| Three treads 4 mm apart | `scallop` | 0.018 mm | 0.0090 mm | 99.95% |
| Three treads | `steep_shallow` (45°) | 0.016 mm | 0.0094 mm | 99.9% |

The largest values are at single points where offset fronts meet. On the dome a plain
`parallel` raster at the stepover the scallop height gives on flat ground does about as well
(0.019 mm, 95% under 0.0087 mm, 98.5% within) with a shorter path (1860 mm against 2470 mm): the
dome shows no advantage for `scallop`, and the other parts were not compared with a raster. On a
sharp wall-and-floor corner a 10 mm ball leaves up to 5.6 mm; `rest_finish` with a 4 mm ball after it leaves at most
0.016 mm, and cuts nothing on the floor further out or on the top. A pencil pass on a fillet
joint tighter than the ball runs within 0.003 mm of where the ball touches both faces.


## `cam.post.list` and `cam.post.get`

`cam.post.list {}` lists the built-in posts with a one-line description and their other names.
`cam.post.get {"name": "linuxcnc"}` returns that post's JSON. Save it as a file, change it, and
point a job's `post_path` at it. Keep a machine-specific post outside the repository.

## `cam.gcode.check`

`{"path": "part.ngc", "rapid_rate": 3000}` reads the file and returns `lines`, `motion_blocks`,
`rapid_length`, `feed_length`, `cut_minutes` (as the controller times it: inverse-time blocks by
their `F`), `rapid_minutes` (with `rapid_rate`), `arcs`, `g93_blocks`, `rotary_ranges` (each
rotary letter's lowest and highest angle), `bounds` and `errors` (the first line the reader
cannot read). Arcs are measured round in their own plane: `G17` with `I J`, `G18` with `I K`,
`G19` with `J K` (a `G18`/`G19` arc's lowest point counts in `bounds`). It reads the
absolute-coordinate G-code SolidCraft's posts write; programs from elsewhere are read on a
best-effort basis and stop at the first block it cannot follow (incremental coordinates, for
one).
