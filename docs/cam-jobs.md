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
- **Gouges.** Along every cutting move of every operation the cutter (a cylinder of its
  radius, endless above its end) must be no deeper in the part or a fixture than
  `gouge.allowed`: the job's tolerance less the tessellation's chord error
  (`gouge.tessellation`), so no cut that passes is deeper than the tolerance into the exact
  part. The depth is the true one: the deepest point of the part inside the cutter, measured to
  the cutter's own surface (a cutter buried deeper than its radius is reported as far as it
  would have to back out along its axis). A ball whose axis holds along a move is measured
  exactly along all of it; other cutters are tested at samples at least half their radius
  apart and half way and near the thirds between them, the deepest sample measured exactly
  against the triangles and the deepest gouge followed to its deepest point. Arcs are followed
  for the check within a twentieth of the allowance. A failure names the operation, the move of
  its toolpath, the G-code line that ends that move (when it is found) and the work coordinates
  of the tool tip there, and says how deep that can be in the exact part. An operation that cuts
  what the model does not show (an engraving) sets `gouge_check: false`; the reply lists it
  under `gouge.skipped`, and it is still checked against the fixtures. A chamfer mill's own band
  is allowed: the chamfer a `chamfer` operation is told to cut, the groove of a V-trace
  (`contour` `side: on`) and the cone of a spotted hole (`drill`) need not be in the model (see
  [Chamfers](#chamfers)). A drill is checked as its own shape, point and all, and a thread mill
  as the core inside its teeth.
- **Holder.** For tools with holder data (`stickout` above the flutes, `shank_diameter`,
  `holder`), the shank and holder must not touch the part or a fixture along a cutting move. A
  shank or holder section no wider than the cutter is inside the cutter's endless cylinder, so
  it reaches nothing the cutter does not reach first: it may come as close as the gouge check
  allows (a wall the cutter has just cut is no collision). Counts are of moves of the
  operation's toolpath.
- **Rapids.** Every `G0` of the program as written, the stock and the turns of a 4th axis
  included (see [The rapid check](#the-rapid-check)).
- **Travel.** With a machine whose JSON has `linear_limits`, every position is within them.
- **Below the stock.** No cut below a box stock's bottom, but by the job's `breakthrough`.
- **The 4th-axis setup check** (see [The setup check](#the-setup-check)).
- **Operation checks** that are part of planning, each refusing the job with a message that
  names the operation: a tool that cannot cut the operation (below), heights out of order, a
  stepover wider than the tool, a profile the tool does not fit in.

Without `dry_run` or `allow_unsafe`, a failure refuses the call with every failed check in the
message. With `dry_run`, the reply carries `passed: false` and `failures`.

### The reply

`name`, `machine`, `post`, `operations` (each with `name`, `kind`, `tool`, `moves`,
`cut_length`, `cut_minutes`, `rewinds`, `max_tilt_deg`, `a_deg` (indexed ones), `gouge`, `gouge_at`
(when it gouges), `holder_collisions`, `adaptive` (an adaptive operation's engagement), `warnings`), `program_lines`, `program_head` (the first lines),
`readback` (`max_tip_deviation`, `max_axis_deviation_deg`, `allowed_tip`, `allowed_axis_deg`,
`cut_minutes`, `pass`), `rapids` (`checked`, `hits`, `worst`, `pass`, `from_unknown`, `first`),
`travel` (`checked`, `violation`, `pass`; `null` without `linear_limits`), `stock` (as checked
against; `null` without one), `gouge` (`worst`, `tolerance`, `tessellation`, `allowed`, `hits`,
`at: {operation, move_index, line?, xyz}` for the deepest, `skipped`, `pass`),
`holder_collisions` (`null` when no tool has holder data), `cut_minutes` (the program's time at
feed as written), `passed`,
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
| `machine_path` | The machine's JSON in a file, read from the job file's folder when relative. Give this or `machine`; a key the file does not know is refused. |
| `a_reversed` | With `4axis_a`: the controller's positive A turns the table the other way. |
| `a_limits_deg` | With `4axis_a`: `[lowest, highest]` A in degrees. Default: it turns without end. |
| `post` | A built-in post (`cam.post.list` lists them): `linuxcnc_4axis`, `grblhal_4axis`. |
| `post_path` | A post as a JSON file: the CAM crate's `Post5` (`cam.post.get {"name": …}` returns one to start from). This is how a machine's own post is matched: copy the closest built-in post and change its header, tool change, spindle, comments, decimals, inverse time and rotary settings until its output matches. |
| `work_offset` | 1 to 6 (G54 to G59), for posts whose header writes `{work_offset}`. Default: 1. |
| `origin` | The design point that is work zero, `[x, y, z]`. On a 4th axis the A axis is work X through it. Default: the design's origin. |
| `orientation` | How the part is turned for this setup, about `origin`: `"flip_x"`, `"flip_y"`, `{"axis": [x, y, z], "angle_deg": a}` or `{"sketch": name}` (see Coordinates). Default: none. |
| `stock` | The raw material in work coordinates: `{"box": {"min", "max"}}`, `{"cylinder": {"radius", "x_min", "x_max"}}` (a bar about work X) or `{"around_part": {"margin": [x, y, z], "top"?, "bottom"?}}`. See [Stock, facing and rapid safety](#stock-facing-and-rapid-safety). Default: none. |
| `breakthrough` | How far below a box stock's bottom a through cut may go, 0 to 25 mm. Default: 0. |
| `bodies` | The part's bodies by name: the surface the 3D operations follow and every cut is checked for gouges against, drawn in the preview. Default: every visible body not in `fixtures`. |
| `fixtures` | Bodies the tool must stay clear of (vises, clamps, a fixture plate modelled in the design): checked for gouges and holder collisions, never machined. Default: none. |
| `tolerance` | 0.0001 to 1 mm (default 0.01): how far a toolpath may be from the profiles and surfaces it follows, and how deep a cut into the part counts as a gouge. |
| `tools` | The tools, as the CAM crate's `Tool` JSON (below). Give this or `tool_library_path`. |
| `tool_library_path` | A JSON file holding an array of tools. |
| `operations` | 1 to 200 operations, run and written in order (below). |

### Coordinates

Work coordinates are the design's coordinates less `origin`, turned about it by the job's
`orientation` when it has one: `"flip_x"` is half a turn about X (the bottom faces up, Y runs
the other way), `"flip_y"` half a turn about Y, `{"axis", "angle_deg"}` a right-handed turn
about that axis, and `{"sketch": name}` puts work X, Y and Z along that sketch's plane's X, Y
and normal (the tool looks down on the plane; a sketch on a face of the part makes a work
coordinate system from that face). So op1 and op2 of a part machined from two sides are two
jobs on the same design: op2 gives the turn and an `origin` on the turned part, and its
polygons, heights, boundaries and `stock` are in its own (turned) work coordinates. A sketch an
operation uses must lie square to the tool once turned, either way up: op1's outline drawn on
the part's top is read through op2's turn as it is, mirrored, its arcs kept as arcs.
`orientation` is for a 3-axis machine; a 4th axis turns the part with its own angles. The
operations' polygons and heights are in work coordinates, and a sketch is read through the
same shift and turn. On a 4th axis
the rotary axis is work X through work zero: model the part with its turning axis along the
design's X through the origin, or give `origin` as a point on that axis. A 3-axis operation
runs with A held at zero unless it is indexed. A rotary toolpath's tip at angle θ and radius ρ
is at `(x, ρ sin θ, ρ cos θ)` with `A = θ`: A = +90° brings the work's +Y side up under the
tool.

### Tessellation

The 3D operations (`parallel`, `roughing`, `waterline`, `scallop`, `steep_shallow`, `pencil`,
`rest_finish`) follow the part's bodies tessellated from their exact B-rep surfaces with a chord
tolerance of a quarter of the job's tolerance (`MeshTolerance::chord`, the tolerance-driven
path the mesh exports are built on). The facets then lie within a quarter of the tolerance of
the true surfaces: inside a convex surface, outside a hollow one. A 3D operation is planned on
the facets within the rest of its tolerance (the operation's `tolerance`, the job's by default,
less the chord error; at least half of it): four fifths of that for the operation itself and
one fifth for merging its moves (below). The gouge check allows the same, so a toolpath right
on the true surface of a hole, which seems to cut the chord error into the facets, still
passes. A mesh body (an imported STL, 3MF or OBJ) is used as it is; the reply's notes say so.
The toolpaths are output, made within the tolerance as an export is. Neither they nor the
gouge check are yet on the exact surfaces.

### Tools

`number`, `kind` (`flat_endmill`, `ball_endmill`, `bull_nose`, `drill`, `face_mill`, `chamfer`,
`thread_mill`), `diameter`, `flute_length`, and optional `description`, `flutes`,
`spindle_rpm`, `feed`, `plunge_feed`, `stepdown`, `stepover`, `corner_radius` (bull nose; a
face mill's inserts), `tip_diameter` and `angle_deg` (chamfer mill), `shank_diameter`,
`stickout`, `holder: [{diameter, length}]`, `point_angle_deg` (a drill's point, default 118),
and for a thread mill `thread_pitch` and `tooth_height`. An operation without `spindle_rpm` or
`feeds` takes the tool's; its stepdown and stepover default to the tool's too.

Each operation cuts with the tools it can use; any other is refused, with the operation and
the tools it takes named. A `contour`, a `pocket`, a `bore` and the 3D finishes cut with a flat
end mill, a ball end mill, a bull nose or a face mill (a `contour` also with a chamfer mill,
tracing a line with `side: on`); `roughing`, `rotary_roughing` and `adaptive_roughing` with an end
mill or a bull nose; `adaptive` as a `pocket`; a `drill` with a drill, a flat end mill (plunging) or a chamfer mill (spotting); a
`chamfer` with a chamfer mill; a `thread_mill` with a thread mill.

### Operations

Every operation has `name`, `type`, `tool` (a tool's `number`), and optionally `spindle_rpm`,
`feeds: {cut, plunge, descent?}`, `coolant` (default false), `gouge_check` (default true: see
The checks) and `enabled` (default true: `false` keeps it in the job but leaves it out of the
program). `heights` are `{clearance, retract, top,
bottom}` in work coordinates.

| `type` | Keys (defaults) |
| --- | --- |
| `contour` | `sketch` (id or name) + `profiles?` (indices; default every drawn profile) or `polygon: [[x, y], …]`; `heights`; `stepdown` (tool's, else half its diameter); `side`: `outside` \| `inside` \| `on` (outside); `climb` (true); `stock_to_leave` (0); `ramp_angle_deg` (0: plunge); `tolerance` (the job's); `lead`, `entry`, `linking`, `tabs`, `finish_passes`, `finish_stepover`, `spring_pass`, `vertical_lead` (see [Leads, entries, linking and tabs](#leads-entries-linking-and-tabs)) |
| `pocket` | `sketch` + `profiles?` or `polygon`; `heights`; `stepdown`; `stepover` (tool's, else 40% of its diameter); `climb`; `stock_to_leave`; `ramp_angle_deg`; `tolerance`; `entry`; `linking` |
| `drill` | `sketch` (circle centres, `circles` (true), filtered by `diameter_range: [min, max]`, and lone points, `sketch_points` (true)) or `points: [[x, y], …]`; `heights` (`retract` is the R height, `top` the hole's top, `bottom` its depth); `cycle`: `simple` \| `dwell` \| `peck` \| `chip_break` (simple); `peck` (tool's stepdown, else its diameter); `peck_clearance` (0.5); `back_off` (0.5); `dwell_seconds` (0.5); `to_full_diameter` (false). See [Drilling](#drilling). |
| `adaptive` | `sketch` + `profiles?` or `polygon`; `heights`; `stepdown` (tool's, else its diameter); `optimal_load` (tool's `stepover`, else 10% of its diameter); `min_engagement_deg`, `max_engagement_deg` (from the load: see [Adaptive clearing](#adaptive-clearing)); `helix_diameter` (80% of the tool's); `ramp_angle_deg` (2); `climb`; `stock_to_leave` (0); `tolerance`; `compare` (true); `from_stock` (true: see [Adaptive clearing](#adaptive-clearing)) |
| `adaptive_roughing` | `heights` (`top` is the stock's top); `stepdown`; `optimal_load`; `min_engagement_deg`, `max_engagement_deg`; `helix_diameter`; `ramp_angle_deg`; `climb`; `boundary` (the stock's extent; the part's extent); `stock_to_leave` (0.25); `flats` (true); `step_up` (none); `tolerance` |
| `chamfer` | `sketch` + `profiles?`, `polygon` or `holes: [{center, diameter}]` (each rim inside its hole); `heights: {clearance, retract, top}` (no `bottom`: the depth follows from the width); `width` (across the top); `tip_offset` (0.2); `side`: `outside` \| `inside` (outside); `climb` (true); `tolerance`. A chamfer mill only; see [Chamfers](#chamfers) |
| `bore` | `sketch` (its circles) + `diameter_range?: [min, max]` or `holes: [{center: [x, y], diameter}]`; `heights` (`top` the hole's top, `bottom` its floor); `pitch` or `ramp_angle_deg` (2, the pitch at most `stepdown`: the tool's, else 10% of its diameter); `stepover` (between rings; the tool's, else 40% of its diameter); `climb` (true); `finish_passes` (0); `stock_to_leave` (0.1 with finishing passes, else 0) |
| `thread_mill` | `sketch` (its circles' centres) + `diameter_range?` or `centers: [[x, y], …]`; `heights` (the thread from `top` down to `bottom`); `pitch`; `thread_diameter` (the major, nominal diameter); `internal` (true); `right_hand` (true); `climb` (true); `thread_depth` (ISO metric: 0.5413 × pitch inside, 0.6134 × pitch outside); `radial_passes` (1); `spring_pass` (false) |
| `parallel` | `heights` (`bottom` is the floor); `stepover` (tool's, else 10% of its diameter); `angle_deg` (0); `boundary?: {rect?: [[x, y], [x, y]], polygon?, holes?}` (the part's extent); `stock_to_leave` (0); `step` (a quarter of the diameter); `tolerance`; `zigzag` (true); `descent_limit_deg` (70: see Feeds); `feeds.descent` (the cut feed: see Feeds) |
| `roughing` | `heights` (`top` is the stock's top); `stepdown`; `stepover`; `angle_deg`; `boundary`; `stock_to_leave` (0.25); `step`; `tolerance`; `zigzag` |
| `waterline` | `heights` (levels from `top` to `bottom`); `stepdown` (tool's, else a quarter of its diameter); `boundary`; `stock_to_leave` (0); `step`; `tolerance`; `climb` (true) |
| `scallop` | `heights` (`bottom` is the floor); `scallop_height` or `stepover` (tool's, else 10% of its diameter; along the surface); `direction`: `inward` \| `outward` (inward); `boundary`; `stock_to_leave` (0); `step` (a tenth of the diameter: the grid and the samples); `tolerance`; `climb` (true). See [3D finishing strategies](#3d-finishing-strategies). |
| `steep_shallow` | As `scallop`, with `threshold_deg` (40), `overlap` (a tenth of the diameter), `stepdown` (the spacing × sin threshold), `shallow`: `scallop` \| `parallel` (scallop), `angle_deg` (with `parallel`; 0) |
| `pencil` | `heights`; `min_angle_deg` (30); `passes` (0: the crease alone); `stepover` (tool's, else 10% of its diameter); `min_length` (the tool's radius); `boundary`; `stock_to_leave` (0); `step`; `tolerance`; `climb` |
| `rest_finish` | `heights`; `previous_tool` (a tool number of the job); `threshold` (twice the tolerance); `scallop_height` or `stepover`; `overlap` (a quarter of the diameter); `boundary`; `stock_to_leave` (0); `step`; `tolerance`; `climb` |
| `indexed` | `a_deg` and `operation`: any of the 3-axis operations above (`contour` to `rest_finish`, `drill`, `adaptive`, `adaptive_roughing`, `chamfer`), run with A held at `a_deg` (3+1); optionally `name`, used when the inner operation has none. The inner operation carries the tool and feeds. See [4th-axis jobs](#4th-axis-jobs-indexed-rotary-and-wrapped-operations). |
| `indexed_pattern` | `a_deg: [angles]` or `count` (evenly over a turn, from `start_deg`, 0), and `operation`: any 3-axis operation, run as an `indexed` operation at each angle, named `<name> A<angle>` |
| `rotary_finish` | `pattern`: `{type: "lines", angle_step_deg, zigzag?}`, `{type: "helix", pitch}` or `{type: "rings", stepover, zigzag?}`; `clearance` and `retract` (radii from the A axis); `floor` (0); `range: [min x, max x]` (the part's extent); `step`; `tolerance`; `stock_to_leave` (0); `max_angle_step_deg` (2) |
| `rotary_roughing` | As `rotary_finish`, with `stock_radius` (default: the job's `stock`, a cylinder's radius or a box's farthest edge from the axis), `stepdown` (in radius) and `stock_to_leave` (0.25) |
| `face` | `bottom`; `top` (the stock's top; required without a stock); `retract` (2 above the top, or the stock's top if higher); `clearance` (10 above the retract); `stepdown` (tool's, else 1); `stepover` (tool's, else 70% of the flat of its end); `angle_deg` (0); `zigzag` (true); `overhang` (half the tool's diameter and 0.5); `rect: [[x, y], [x, y]]` (the stock seen from above, else the part's extent); `lead_radius` (none). A face mill, a flat end mill or a bull nose; see [Facing](#facing). |
| `wrap` | `radius` (of the cylinder the path is drawn on); `points: [[x, y, z], …]` (x along the axis, y the arc length round it, z the depth: 0 on the cylinder, negative into it) or `sketch` + `curve` (a curve id; its sketch x and y are taken as x and arc length) with `depth` (0); `clearance` (5) and `retract` (1) above the cylinder; `max_angle_step_deg` (2) |
| `wrap_pocket` | `radius` (of the cylinder the profile is drawn on); `sketch` + `profiles?` or `polygon: [[x, arc length], …]` (unrolled); `bottom` (the floor's depth from the cylinder, negative into it); `top` (0); `stepdown`; `stepover`; `clearance` (5) and `retract` (1) above the cylinder; `climb`; `stock_to_leave` (walls); `ramp_angle_deg`; `tolerance`; `max_angle_step_deg` (2) |
| `wrap_contour` | As `wrap_pocket`, with `side` (outside) instead of `stepover`: the profile's boundary followed in depth levels |

A sketch an operation uses must lie on a plane square to the tool, facing +Z or -Z (one facing
away is read as seen from the tool, mirrored), in the design's root component. Contours and
pockets, indexed or not, are written with arcs kept (`G2`/`G3`). The moves of a 3D operation are merged before
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

## Drilling

A `drill` operation feeds a drill (or a flat end mill, plunging, or a chamfer mill or spot drill,
spotting) straight down the tool axis at each hole, at the operation's plunge feed. Any other tool
is refused. With a chamfer mill `bottom` is where its point goes: a spot `d` across at the top is
`d / 2 / tan(angle_deg / 2)` deep (a 90° tool: half the spot's diameter); the cone it cuts is the
operation's own and is not held against the model (see [Chamfers](#chamfers)). It is a 3-axis operation,
so it also runs inside `indexed`: its points and heights are then in the frame the angle brings
under the tool.

```json
{"name": "holes", "type": "drill", "tool": 3, "sketch": "Holes", "diameter_range": [4, 5],
 "heights": {"clearance": 15, "retract": 2, "top": 0, "bottom": -12}, "cycle": "peck", "peck": 4}
```

**Holes.** With `sketch`: the centres of its circles (not construction ones; with
`diameter_range`, only circles of a diameter in it) and its points that are not part of a curve.
The sketch must lie on a plane square to the tool axis (either way up). With `points`: the
holes in the operation's XY. Repeated positions are drilled once. Holes are drilled in a greedy
nearest-neighbour order from the one lowest in X (then Y), the same order every time.

**Cycles.** Every cycle is written as plain `G0`/`G1` moves and a `G4` dwell, which every
controller reads and the read-back follows; no canned cycle (`G81`, `G82`, `G83`, `G73`) is
written.

| `cycle` | Each hole |
| --- | --- |
| `simple` | Feed from the R height to the bottom, rapid out to R. |
| `dwell` | The same, with a pause of `dwell_seconds` at the bottom. |
| `peck` | Feed `peck` deeper at a time, counted from `top` (the last peck ends at the bottom; a last peck shorter than a tenth of `peck` is merged into the one before); after each peck rapid right out to R, then rapid back down to `peck_clearance` above the depth reached. |
| `chip_break` | Feed `peck` deeper at a time; after each peck rapid back `back_off` (staying in the hole) to break the chip, then feed on. |

**Depth.** `bottom` is where the tip goes. With `to_full_diameter: true` it is where the drill
reaches its full diameter, and the tip goes deeper by the point's length, radius / tan(point
angle / 2): 1.80 mm for a 6 mm drill with a 118° point. A hole deeper than the tool's flutes is
refused. Between holes the tool rises to the clearance height.

**Checks.** The gouge check holds a drill against the part as its real shape: the full diameter
from the point's length up, and the point as stepped cylinders inscribed in its cone (within the
job's tolerance along the axis, at most 64 steps). Model the holes as cut bodies; drilled into a
part that shows no hole, the operation gouges and the job is refused.

The tool comes down from the clearance height to the R height at rapid over every hole, and the
gouge check cannot see material there (the finished part already has the hole). So the operation
is refused when the part or a fixture within the tool's radius and a tenth more of a hole's axis
(the rim of a hole of the tool's size) is above the R height by more than the tolerance: a hole in
a boss drilled with the heights of the plate under it, say. Drill holes whose tops are at
different heights in separate operations, each with its own `top` and `retract`.

## Helical bores and thread milling

`bore` and `thread_mill` follow a helix round each hole's axis (+Z of work coordinates, every
rotary at zero) and are written with helical arcs (`G2`/`G3` with Z, half a turn each) where the
post has `helical_arcs`. Their holes are a sketch's circles (exact centres and diameters, on a
plane facing +Z; construction circles are left out), optionally only those whose diameter is in
`diameter_range`, or a list. Both are read back and gouge-checked like every operation, so the
model should show the holes (and, for a thread, at least its minor diameter).

```json
{"name": "bores", "type": "bore", "tool": 1, "sketch": "Holes", "diameter_range": [8, 20],
 "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -12}, "ramp_angle_deg": 2, "finish_passes": 1}
```

A **bore** goes to each hole's centre, feeds down to `top`, and runs a half-circle lead-in
inside the hole onto its helix, so the tool reaches the wall along it and never head-on. The
helix goes down `pitch` per turn (or slopes at `ramp_angle_deg` at the tool centre, its pitch
then at most `stepdown`), climb milling counter-clockwise by default, and never goes down faster
than the plunge feed: a steep helix close round the axis, nearly a plunge, is slowed to keep its
Z at the plunge feed. A full flat circle cleans
the floor, a half-circle lead-out goes back to the centre, and the tool rises inside the hole.
A hole more than two tool diameters across is opened in several helical rings, the first within
the tool's radius of the centre (so no core is left) and each further one at most `stepover`
out. With `finish_passes`, roughing leaves `stock_to_leave` on the wall and the finishing passes
go round at the full diameter (after the first they are spring passes). A hole that is not at
least 0.1 mm wider than the tool is refused, the hole named (`hole 2 at (10, 0), ⌀5 mm`); one
narrower than 1.2 tool diameters is cut with a warning, as it leaves little room for chips.

A **thread mill** is a tool of `kind: "thread_mill"`: `diameter` across the tips of its teeth,
`thread_pitch` for a multi-form tool (rows of teeth that far apart over its `flute_length`;
left out, a single-point tool that cuts any pitch) and optionally `tooth_height` (how far its
teeth stand out of its core). Z is the tool's tip; a single-point tool's tooth is taken there.
A **thread_mill** operation cuts `thread_diameter` (the major diameter) at exactly `pitch` per
turn: inside a hole (`internal`, the default) the tool reaches the major diameter, round a boss
the minor one. The depth is split into `radial_passes` from the minor (inside) or major (outside)
diameter, then `spring_pass` repeats the last. Climb milling goes counter-clockwise inside a hole
and clockwise round a boss, so a right-hand thread is cut from the bottom up inside and from the
top down outside; `right_hand: false` or `climb: false` turns that round. Inside, each pass
starts at the hole's centre with a half-circle lead-in and ends with a lead-out back to it.
These leads turn the tool a quarter turn about the axis, so they rise (or fall) a quarter pitch
with the helix and the teeth stay on the thread: the tip runs from `bottom` to `top`, the full
depth from a quarter pitch above the one to a quarter pitch below the other, and an internal
thread must be at least half a pitch long. Outside, the lead-in comes from just clear of the
thread and the leads stay level (they turn the tool only a few degrees about the axis). A
multi-form tool makes one turn per span of its teeth from the bottom up; its last turn starts as
low as still reaches the top, a whole number of pitches above the others, so its teeth cut at
most a pitch of thread beyond the top (a tool whose teeth are longer than the thread is warned
of). A thread mill that does not fit the minor diameter is refused, and so is a multi-form tool
of another pitch.

The gouge check cannot see a thread the model does not show, so for a thread_mill operation it
takes the thread mill as a flat end mill at the roots of its teeth, the thread's depth inside its
tip radius, under the thread mill's own shank and holder: that core must stay clear of the part,
while the teeth beyond it cut the thread. The program still names the thread mill. Thread mills
are refused by every other milling operation, and anywhere else a thread mill is checked whole,
its teeth and all.

## Stock, facing and rapid safety

A job may say what it starts from with `stock`, in work coordinates. A box gives its corners;
`around_part` is the box round the part's bodies grown by a margin on each side (the reply's
`stock` shows the box it came to); a cylinder is a round bar about work X, the A axis of a 4th
axis set up the usual way. An `around_part` stock is read on the bodies' exact boxes.

A cut that goes below a box stock's bottom on a 3-axis machine fails the job: below it are the
parallels, the vise's floor or the table that hold the stock, or the lowest fixture that reaches
under it. The deepest point of every cutting move with the tool upright counts, a drill's point
included. `breakthrough` (0 to 25 mm) lets a cut go that far below the bottom, onto a spoil
board or between parallels clear of it; the failure names the `breakthrough` that would let it,
and a note says when a cut passes only by it. With a stock, the ways down at rapid stop 1 mm
above the stock's top (on a 4th axis, outside the bar), not only above the operation's own top.

### Facing

`face` cuts the stock's top (or `top`) flat down to `bottom` in levels of at most `stepdown`,
in straight passes along `angle_deg` at most `stepover` apart, over `rect` (by default the
stock seen from above). Every pass starts and ends with the whole cutter 1 mm clear of the
area, so the tool goes down beside the stock at the plunge feed and enters it at depth: it
never plunges into material. The outermost passes reach `overhang` past the edges (by default
half the diameter and 0.5 mm, which puts their centres just outside), so the edges come out
clean. With `zigzag` every other pass runs back and the passes link at depth along a line
beside the stock, each level starting where the last ended; without it the tool rises to the
retract height between passes and every pass runs the same way. The stepover is held to the
flat of the tool's end (its diameter less the corners, for a bull nose), so the floor has no
ridges. A top below the stock's top is warned of: its first level takes more than a stepdown.
With a stock, a `face` is refused where a descent or a link between passes would run in the
stock (a `rect` that stops short of the stock's edges along the passes would take the tool
straight down into it: give one that reaches past them, or turn the passes), and where its
`bottom` is below the stock's (whatever holds the stock is there).

With `lead_radius` (mm) each level is entered down a vertical arc: the tool goes down beside the stock to the radius above the level, the radius behind the pass's start,
and comes down a quarter arc into it; with `zigzag` the passes of the level link round half
circles outside the area (`G2`/`G3`), and the level's last pass goes out up a quarter arc
beyond its end. The arcs are `G18` for passes along X (`angle_deg` 0) and `G19` along Y (90),
lines along the arc at other angles; see [Vertical leads](#vertical-leads). The lead's radius is
at most ten times the tool's diameter.

### The rapid check

Every `G0` block of the program as written is read back and followed as the controller moves
through it, every axis evenly from the block's start to its end, sampled every 0.5 mm of the
tip's travel and of a point at the tool's reach. That covers the move to the first point, the
links between passes and operations, rotary turns, rewinds, the moves after a tool change and
the final retract. At every sample, in work coordinates:

- the cutter, as a cylinder of its radius without end above its tip (the shank and spindle are
  there too), must not be in the part or a fixture by more than the job's tolerance, and a
  tool's shank and holder (with holder data) must not touch them. In an operation with
  `gouge_check: false` (it cuts what the model does not show, a deburr of a sharp-edged
  model), a retract straight up the tool's axis is not held against the part: it leaves the
  cut its own feed moves made;
- with a stock, the cutter must not be in the stock. The rapid check does not track what
  earlier operations removed, so the stock as given is the bound: a rapid below the stock's top over the stock
  fails, even where an earlier operation has cut it away, except after a `face` (with its
  default `overhang`) over the whole top of a box stock: the operations after it are held
  against the stock with its top at the face's `bottom`. A retract straight up the tool's
  axis out of the cut is exempt too: it only passes through room the tool already fills; so is
  a rapid back down that line in the same operation, no deeper than the retract started (a peck
  drill going back into its hole to `peck_clearance` above the depth reached). Keep
  every retract and clearance height above the stock's top (on a 4th axis, outside its radius);
- a block that turns the A axis (a table about X) must keep the cutter outside the radius the
  stock sweeps (a box's farthest corner) all through the turn, wherever the stock is level with
  the tool along X. Without a stock the part is cut into 128 slices along X, each held to the
  largest radius of the part over it and the tolerance, so a turn over a slender end of the
  part is held to that end's radius, as the 4th-axis setup check holds it.
  A post with `clearance_retract` rises to the operation's highest rapid before it turns; a
  low clearance height can still leave the tool inside the swept radius, and this check finds it.

A block after a return to the machine's home (`G28`, `G30`), a move in machine coordinates
(`G53`) or a tool change starts where the program does not say: only its end is checked, which
with the cutter endless above covers the way down from the machine's top. Turns made there (a
post's `safe_rotate` lines) are taken to happen at the machine's top, and a tool change to be
made where the new tool clears everything: give the post a `safe_retract` at tool changes where
the controller changes tools wherever the spindle is. `rapids.from_unknown` counts these blocks.
Each block is checked with the tool of its own operation.

A failure names the line, the operation, what the rapid runs into, how deep and where; the
reply's `rapids.first` lists the first 20. Like the other checks, a failure refuses the call
unless `dry_run` or `allow_unsafe` is given.

### Travel

A machine given as JSON with `linear_limits: [[x, y, z], [x, y, z]]` (the lowest and highest
position of the linear axes, as written in the program) has every block's end, and points along
every arc, held against them. The first position beyond them fails the job, by line and axis.

## Adaptive clearing

`adaptive` (2D, like `pocket`) and `adaptive_roughing` (3D, like `roughing`) clear material with
the tool's engagement held down: the share of its circumference in material never goes above
`max_engagement_deg` outside an entry helix, so the tool never runs in a full slot. This suits a
small, not very rigid mill best: take a deep `stepdown` (the flute length the tool's data allows)
with a light `optimal_load` (5% to 15% of the diameter) and the full feed.

The path is planned on a raster of the material a sixteenth of the tool radius fine, whatever
the load, with exact distances to the path already planned near the edge of the cut (so a light
load needs more steps, not finer cells). It is walked in short steps,
turning at most 25° a step, so its corners are rounded; at each step it takes the heading whose
engagement is nearest the optimal load's (on a straight edge `acos(1 − load / radius)`), with the
material on the climb (or conventional) side, and never one above the limit. Where a walk can
cut no more (a walk that has gone a little more than a half turn at its tightest without
cutting ends there rather than wander through what it cleared), the next starts where cleared
space meets material, reached by a feed through cleared material when there is one with the
tool down, by lifting to `retract` otherwise.

- **Entry.** Where the tool can reach the material from open air (round the stock in
  `adaptive_roughing`) it goes down outside it. Material enclosed on every side is entered with
  a helix of `helix_diameter` at `ramp_angle_deg`, then a circle at the level. Where that helix
  has no room, the largest that fits there is used instead (down to a quarter of the tool's
  diameter), and a warning gives the diameter used; with no room even for that, the operation is
  refused, naming both sizes.
- **Levels.** `adaptive` cuts the same path at every depth pass. `adaptive_roughing` plans each
  level where the dropped cutter (grown by `stock_to_leave`) is at or below it; with `flats` it
  adds a level at every flat area of the part (raised by the stock to leave). Levels further
  apart than the tool's `flute_length` are refused: the shank would rub on the wall the flutes
  leave.
- **Work.** Each level may take a number of raster cell visits in proportion to its raster and
  to the steps its area and load call for (not to the number of levels, so a larger stepdown is
  never refused where a smaller one passes); the level of an `adaptive` (planned once and
  repeated) at least 1.5e9, a few seconds, for a pocket not much wider than the tool at a light
  load, whose many short walks cost more a step (an `adaptive` planned at every level, as rest
  machining will be, shares that floor among its levels). A clearing whose walks would take more than
  3,000,000 steps, or an `adaptive_roughing` whose levels would typically take minutes, is
  refused before any is planned, saying how many levels, cells and steps, and naming the
  smallest `optimal_load` that would plan (its engagement limit raised with it as by default).
  An `adaptive` refused while planning, for the work its walks take, names the smallest
  `optimal_load` that plans, found by planning at larger loads (rounded up to 0.01 mm; the
  engagement limit raised with it as by default). A 6 mm tool plans 2% of its diameter
  (0.12 mm) round a boss on a 100 × 60 mm stock in about 25 s on the test build; a 10 mm and a
  12 mm tool plan their library load of 0.2 mm in a 36 × 24 mm pocket with 4 mm corners in a
  few seconds (they were refused).
- **Corners.** An inside corner sharper than the limit allows is not driven into: a fillet
  somewhat larger than the tool's is left there for a finishing contour (`left_area` in the
  report). Where the tool's radius is larger than a corner's (or a place is narrower than the
  tool), what it cannot reach is left as rest material and reported: `corner_area` (seen from
  above, the stock to leave on the walls not counted) and a warning saying to clear it with a
  smaller tool after this one. The clearing is not refused for it. When every walk
  start has been tried, those still next to material are tried once more on what is left, so a
  sliver along a wall a walk turned off from is cut.
Defaults: `optimal_load` is the tool's `stepover` (a tool library's adaptive start values), else
10% of its diameter; `max_engagement_deg` one and a half times the optimal load's engagement and
at least 15° more than it; `min_engagement_deg` a third of it, and at most half the
`max_engagement_deg`. A `max_engagement_deg` below the load's engagement wins: the path aims at
the narrower width the limit allows, which takes more steps.

- **Material.** `adaptive` takes the job's stock box (with `from_stock`, true by default) as
  what there is to cut: the material is the profiles' part of it, from its top down. The tool
  may also stand past a profile's edge where it meets no stock outside the profiles (beyond the
  stock's edge, or past a profile drawn round the stock), so it goes down in the open air beside
  the stock and cuts in from there. Without a stock box, inside `indexed` on a tilted frame, or
  with `from_stock: false`, the profiles are taken as full of material and entered with a helix.
  Nothing round the stock is modelled: where the tool goes down beside the stock below the top
  of the vise jaws or clamps, model them as `fixtures` or give `from_stock: false`.
  `adaptive_roughing` takes the material as the box over `boundary` (its rectangle,
  and inside its polygon too) from `top` down, and goes down to a level in the open air outside
  it. Without `boundary.rect` the stock is the part's extent seen from above, and the report
  warns: a blank larger than that would be cut into from the side at the level's full depth, so
  give the blank's extent as `boundary.rect`; with a job `stock` the rectangle is held to the
  stock's outline, as there is no material past it. Nothing round the stock is modelled: keep the
  levels that go down outside it above the vise jaws and clamps.

Each adaptive operation's report carries `adaptive`, measured along the path as written,
independently of the planner (exact containment in the profiles or stock, exact distances to
every earlier move, the tool's circumference looked at in 1° steps): `measured` (false when the
path was too long to measure: the figures are then 0, and a warning says so), `max_engagement_deg`,
`max_entry_engagement_deg`, `mean_engagement_deg`, `full_slot_share` (of the cutting time
outside entries at 170° or more: 0), `entry_slot_share`, `limit_deg`, `levels`, `helices`,
`lifts`, `left_area`, `corner_area` (2D: what the tool cannot reach, above) and, for `adaptive`
with `compare`, `pocket`: a plain offset pocket of the
same profiles (stepover 40% of the diameter) measured the same way (`max_engagement_deg`,
`full_slot_share`, `cut_length`, `cut_minutes`). A measured engagement more than 3° above
`limit_deg`, or any full slot outside the entries, is also a warning of the operation. Both
operations also work inside `indexed`.

## Chamfers

A face mill (`kind: "face_mill"`) is a flat end, its inserts' corners rounded by
`corner_radius` when given; its tool-list name is `FACE MILL`. A chamfer mill (`kind:
"chamfer"`) has `diameter` (the widest the cone cuts), `tip_diameter` (0 for a pointed tip,
default 0) and `angle_deg`, the included angle (90 for 45° chamfers, 1 to 179, required); its
tool-list name is `CHAMFER MILL`. A spot drill is a chamfer mill with no tip (`angle_deg` its
point angle). Pockets, the 3D operations and a contour beside a line refuse a chamfer mill; a contour
`side: on` takes one to trace the line to a depth (V-engraving), and `drill` takes one to spot
holes. `roughing` and `rotary_roughing` refuse a face mill, whose inserts do not cut at its
centre.

The `chamfer` operation cuts a chamfer along the top edges of closed profiles in one pass:

```json
{"name": "deburr", "type": "chamfer", "tool": 7, "sketch": "Outline",
 "heights": {"clearance": 10, "retract": 2, "top": 0}, "width": 0.5, "side": "outside"}
```

`width` is the chamfer's leg across the top face; its leg down the wall is `width / tan(angle_deg
/ 2)` (the same for a 90° tool). The tip runs `tip_offset` below the chamfer so the cone's side,
not its tip, cuts all of it: `width / tan(angle_deg / 2) + tip_offset` below `top`, its axis
`tip_diameter / 2 + tip_offset · tan(angle_deg / 2)` out from the edge. A chamfer the cone cannot
reach, wider than `diameter / 2 − tip_diameter / 2 − tip_offset · tan(angle_deg / 2)`, or deeper
than the flutes, is refused with the widest it can cut, never cut narrower. `outside` chamfers a
part's outline, `inside` an opening's; corners of an outside chamfer are rounded by the cone.

Round holes' rims are given as `holes: [{center, diameter}, …]` (as `bore` takes them), each
chamfered inside its hole (`side` is `inside`, and only that). Give a `sketch`, a `polygon` or
`holes`, one of them.

The gouge check holds the cone to the part less the chamfer it is told to cut: as if the tool ran
the chamfer's height (`width / tan(angle_deg / 2)`) higher, where its cone just meets a sharp
edge, with its tip also held where it really runs. So a chamfer the model shows passes, and so
does a deburr of a sharp edge the model does not chamfer; a cut past the band fails, such as a
profile off the part's edge, a wrong `side`, or a tip that runs into a face below. The band is
the part's only: a fixture (a vise jaw just below the edge) is held against the whole tool.
Inside `indexed` a chamfer runs in the frame the A angle brings under the tool.

A contour `side: on` with a chamfer mill traces the line with the point down to `bottom`: a
V-engraving as wide at the top as the cone is there (a 60° tool 0.3 mm deep: 0.35 mm). Its groove
is its band (`top − bottom`), so the model need not show the engraving; a trace whose `top` is
below the face it engraves cuts past its band and fails. Give the face as `top`: one above it
makes the band deeper than the groove, and what stands on the face within it is not held. A
trace deeper than the cone (from `top` to where the tool is full width, `(diameter −
tip_diameter) / 2 / tan(angle_deg / 2)`) is refused: below it the tool cuts with its straight
side. A `drill` with a chamfer mill spots holes the same way, its band the spot's depth, which is
held to the cone too:

```json
{"name": "engrave", "type": "contour", "tool": 30, "side": "on", "sketch": "Text",
 "heights": {"clearance": 10, "retract": 2, "top": 0, "bottom": -0.3}}
```

```json
{"name": "spot", "type": "drill", "tool": 30, "sketch": "Holes",
 "heights": {"clearance": 10, "retract": 2, "top": 0, "bottom": -1.5}}
```

## Leads, entries, linking and tabs

The 2D operations (`contour` and `pocket`, on their own or inside `indexed`) take these keys.
Without them a contour and a pocket cut as before: each plunges, or ramps at `ramp_angle_deg`.

```json
{"name": "outline", "type": "contour", "tool": 1, "polygon": [[-20, -15], [20, -15], [20, 15], [-20, 15]],
 "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -6}, "stepdown": 2,
 "lead": {"radius": 2, "line": 1}, "entry": {"type": "ramp", "angle_deg": 3},
 "tabs": {"count": 4, "width": 4, "height": 1.5}, "finish_passes": 1, "finish_stepover": 0.25,
 "spring_pass": true, "linking": "stay_down"}
```

| Key | Operations | Meaning |
| --- | --- | --- |
| `lead` | contour | `{radius, sweep_deg?, line?}`: a tangent arc into each pass and out of it, `sweep_deg` (90) round, with a straight `line` (0) before the lead-in and after the lead-out. It lies in the plane of the cut (`G2`/`G3`), on the side of the pass away from the part, and the pass starts in the middle of its longest straight piece. A lead that would cut into the part (the far wall of an opening, a second profile) is refused, and so is any lead with `side: on`, where both sides of the line are the part. |
| `entry` | both | How the tool goes down into each depth pass: `{"type": "plunge"}` (at the plunge feed), `{"type": "ramp", "angle_deg"?}` (2°: a contour along its lead, back and forth, or before its first tab, else round the loop; a pocket round its ring) or `{"type": "helix", "radius"?, "angle_deg"?}` (90% of the tool's radius, at most all of it, so that no core is left standing; 2°), then once round at the depth. A helix that does not fit is made smaller, then replaced by the ramp, then by the plunge; a contour with `side: on` takes no helix. Give `entry` or `ramp_angle_deg`, not both. Default: the operation plunges (or ramps at `ramp_angle_deg`). |
| `linking` | both | `clearance` (the default) or `stay_down`. Within a level the tool always stays down where the straight link runs where the passes cut so far, or the one it leads to, have taken the material away (tracked by each pass's distance from the boundary); elsewhere it goes up to the clearance height, or with `stay_down` only to the retract height where the cutter, dropped onto the part all along the link, stays 1 mm below it. With `stay_down` a pocket also goes from one level to the next through the level it has cleared. |
| `tabs` | contour, outside | `{count \| spacing, width, height, ramp_angle_deg?}`: material left `height` above the bottom to hold a part cut out of the stock, each `width` long along the wall, placed evenly on the profile's straight stretches (away from its corners and the start); the tool rises over each on ramps of `ramp_angle_deg` (30°). Refused when they do not fit or are as high as the cut. |
| `finish_passes` | contour | Passes round the final wall at full depth after the depth passes (0), `finish_stepover` (5% of the tool's diameter, at most half its radius) apart, the last on the wall; the depth passes leave `finish_passes × finish_stepover` for them. Each is linked in the cut, or by its lead. Not with `side: on`. Refused where a finishing pass would reach into a notch, a neck or a gap between parts that the pass before it did not fit in (it would cut a full slot at full depth). On a wall deeper than the tool's `flute_length` each finishing pass goes down in steps no deeper than the flutes, evenly from `top`, the last at `bottom` (a tool whose shank is wider than its cutter is refused there); the spring pass stays at the bottom. |
| `spring_pass` | contour | Go round the last pass once more (false). |
| `vertical_lead` | contour | The radius of a quarter arc in the vertical plane of the pass's start down into each pass and up out of it, mm ([Vertical leads](#vertical-leads)). Default: none. |

No cut of a contour or a pocket goes down faster than the plunge feed: a ramp, a helix and the
way down off a tab are slowed to the feed that takes the tool down at the plunge feed (a 30° tab
ramp at 2880 mm/min would otherwise go down at 1440 mm/min).

Ring by ring a pocket is cut from the inside out: a ring only after the rings next inside it, and
otherwise the nearest ring first, so that the rooms of a pocket are cleared one after the other.

### Vertical leads

`vertical_lead` (a contour's radius, mm) enters a cut down a quarter arc in the vertical plane of the cut's direction,
from the radius above and behind its start, and out of its end up a quarter arc beyond it. The
post writes each as a `G18` (passes along X) or `G19` (along Y) arc, the plane word on the arc's
own block before its motion code (`G19 G2 Y174.148 Z-0.15 J-0.3 K0. F1600.`) with both centre
offsets of the plane (`I K` or `J K`), `G17` again on the next arc in XY and after the
operation, so a canned cycle that follows is in the plane the header set. Where the cut's
direction runs along neither axis the arc goes out as lines within 0.001 mm of it; a post with
`arcs: false` writes lines within its arc tolerance. The program reader, the read-back,
and `cam.gcode.check` follow `G17`, `G18` and `G19`. On a
contour a vertical lead comes down into the start of its `lead` (or the pass) and takes the
place of the entry (no ramp or helix with it); it is refused where the ground behind the start
or beyond the end is the part. Like every way down, the arc into the cut is slowed so it goes
down no faster than the plunge feed where it is steepest (a quarter arc into a cut starts
straight down). A finishing pass whose vertical lead would start where the depth passes did not
cut is refused, as a plunge there is, unless the arc starts above `top`.

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


## 4th-axis jobs: indexed, rotary and wrapped operations

These run on a table about +X (`machine: "4axis_a"`, or a machine of one rotary axis from
`machine` or `machine_path`). Each is planned in work coordinates, written through the post and
read back through the machine, like the others.

### Indexed operations

```json
{"type": "indexed", "a_deg": 90,
 "operation": {"name": "side pocket", "type": "pocket", "tool": 1, "polygon": [[-6, -6], [6, -6], [6, 6], [-6, 6]],
               "heights": {"clearance": 40, "retract": 20, "top": 15, "bottom": 10}}}
```

runs a 3-axis operation with A held at `a_deg` (3+1). Its heights, polygons, boundaries and
sketches are in the frame that angle brings under the tool: its Z is the tool axis, so at A =
90° the frame's Z is work +Y. The reply gives the operation `kind: "indexed pocket"`, its
`a_deg`, and `max_tilt_deg` (the angle between the tool and work +Z). The name may be on the
`indexed` entry instead of its inner operation.

```json
{"type": "indexed_pattern", "a_deg": [0, 90, 180, 270],
 "operation": {"name": "flat", "type": "parallel", "tool": 1, "heights": {"clearance": 40, "retract": 17, "top": 15, "bottom": 0}}}
```

runs one 3-axis operation at several A angles, as that many `indexed` entries would: each is
named `<name> A<angle>` (`flat A0`, `flat A90`, …) and written exactly as an `indexed` entry with
that name and angle. `count: 6` spreads six angles evenly over a turn from `start_deg` (0). A
pattern has 1 to 200 angles, and a job's operations, every angle counted, are at most 200.

### Rotary finishing and roughing

`rotary_finish` follows the part with the tool radial while A turns. `rotary_roughing` takes
round stock of `stock_radius` down to the part in levels of radius, `stepdown` apart, leaving
`stock_to_leave`. Their `pattern` is one of:

- `{"type": "lines", "angle_step_deg": 15, "zigzag": true}`: passes along X, at most that far
  apart round the axis;
- `{"type": "helix", "pitch": 1}`: one pass going round and round, `pitch` mm along +X a turn;
- `{"type": "rings", "stepover": 2, "zigzag": true}`: circumferential passes, a full turn of A
  at each X, X stepping between them. The rings are evenly spaced over `range`, both ends
  included, at most `stepover` apart (exactly it when it divides the range). With `zigzag` (the
  default) every other ring turns back, so A stays within one turn. Rings are linked by feeding
  across X when that is short and clear. For roughing, rings farther apart than the tool is
  wide are refused.

`clearance` and `retract` are radii from the A axis; `range` is from where to where along X
(default: the part's extent).

### Wrapped paths, pockets and contours

`wrap` wraps a flat path round a cylinder of `radius`: its points are `[x, arc length, depth]`,
or a sketch curve whose x is along the axis and y the arc length round it, at `depth`.

`wrap_pocket` and `wrap_contour` cut a closed 2D profile into a cylinder of `radius` round the
A axis, the tool pointing at the axis. The profile is drawn unrolled: x along the axis, y the
arc length round the cylinder at `radius` (a polygon, or a sketch on a plane facing +Z whose
x and y are taken that way, in work coordinates). It is cut in levels from `top` down to
`bottom` (depths from the cylinder, negative into it), at most `stepdown` apart; the floor is
the cylinder of radius `radius + bottom`, exactly.

Unrolling distorts with depth. The profile keeps its angles as it goes down (a wall at arc
length y is the radial plane at A = y / radius), but a tool of radius r at a level of radius ρ
must have its centre r from the wall in arc length *at ρ*, which is an angle of r / ρ: wider in
angle the deeper the level. So every level is planned on its own: the profile's arc lengths are
scaled by ρ / radius (the unrolled plane at the cutting radius, where lengths are true), the
pocket or contour is offset there and wrapped at ρ. Along X nothing is distorted. A radial
tool's side is parallel to its axis, so above the tip it stays farther from a radial wall than
at the tip: a wall is left as steps no taller than a stepdown, and at the tip at most
r − ρ·atan(r / ρ) ≈ r³ / (3ρ²) short of it (0.02 mm for a 6 mm tool at radius 20), never cut
into.

Lines are kept exact; curves and the offset arcs are followed in chords within a quarter of the
tolerance each and the wrapped moves within half of it. Rotary and wrapped cuts are written as
short moves, each turning A by at most `max_angle_step_deg` (2), in inverse time where the post
says so; a post with `rewind` brings a long cut back within the table's limits, and the reply
counts each operation's `rewinds` (their time at feed is in its `cut_minutes`). Everything one
level of a wrapped operation cuts must fit within one turn at that level, or the operation is
refused.

### The setup check

Before anything is written, every rotary operation's clearance is held against the part along
the stretch of X it works over (its moves, the tool's radius either side): where its rapids go
round and where A turns, the tool must be outside the part's largest radius about the A axis
there, by more than the job's tolerance. That is the `clearance` of `rotary_finish` and
`rotary_roughing` (which must also clear its `stock_radius`), and `radius + clearance` for
`wrap`, `wrap_pocket` and `wrap_contour`. The part's radius is read on its bodies as the 3D
operations take them (tessellated within a quarter of the tolerance).

A turns into an operation where the one before it left the tool: the post rises there (to its
`clearance`, or else to the new operation's highest rapid, never down) and turns A before it
moves across. So whenever A turns between two operations with the same tool, of any kind (a
plain 3-axis one included), the tool's end at that height must be outside the part's largest
radius within the tool's radius of the X where it stands. Raising the earlier operation's
clearance, or the post's, fixes it. An `indexed` or `indexed_pattern` operation turns A only as
it starts; where the tool's place is not known (the program's first operation, or after a tool
change) its own clearance is held against the X it works over instead. A post whose
`safe_rotate` lines take the tool to the machine's top before every turn is not held to either.
A job that fails is refused with the operation, the height and how far the part (or the stock)
reaches.

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
