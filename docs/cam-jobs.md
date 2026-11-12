# CAM jobs

A CAM job turns a design's profiles and surfaces into G-code for one machine. It is a JSON file kept next to
the design (or an object passed inline); nothing of it is saved in the `.sc` file. These
commands work with jobs, from scripts, the CLI and MCP alike:

| Command | What it does |
| --- | --- |
| `cam.job` | Plans every operation, writes the program through the job's post, reads it back and checks it, and writes the G-code (and an HTML preview and a setup sheet) |
| `cam.post.list` | Lists the built-in posts |
| `cam.post.get` | Returns a built-in post as JSON, to copy into a post file |
| `cam.gcode.check` | Reads a G-code file and reports lengths, time, the rotary range, inverse-time blocks and the first line it cannot read |
| `cam.simulate` | Runs a written program through a model of the stock and holds what it leaves against the part: gouges, rapids through material, material left |
| `cam.view.*` | The simulation view: a job's program played move by move on the simulated stock (see [Simulation view](#simulation-view)) |

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
| `setup_sheet` | The setup sheet for the machine operator: a `.html` page (self-contained, print styles) or a `.md` file. See [Setup sheet](#setup-sheet). |
| `dry_run` | Plan and check, write nothing; the reply says whether the checks pass instead of refusing. |
| `allow_unsafe` | Write the program even when a check fails. Its first line (the program's name, as a comment) then ends `WRITTEN WITH FAILED CHECKS`, so the file says so; no line moves. |
| `split_by_tool` | Write one program per tool instead of one program: each run of operations with one tool is a program of its own, named after `output` with its place and tool (`bracket-2-T6.ngc`). Each is complete on its own (the post's safe start, the same work offset, the tool change and the tool's length offset on its first Z), with a header naming its place in the order, the tool (`THIS PROGRAM USES T6 - ...`, not a line a checker reads as a tool table entry), its stickout and the length offset to set; each is read back on its own. The reply's `programs` and `outputs.programs` list them in the order to run them, and the setup sheet does too. An older program of every tool under `output`'s name left beside them is a warning. For a machine that ignores tool changes (`tool_change: ignored`, below). |
| `simulate` | Also run the program as written through the stock-removal simulation (below): a gouge or a rapid through material fails the job like the other checks, material left on the part is a warning, and the reply carries `simulation`. |
| `simulate_resolution` | With `simulate`: the simulation's cell size in mm, as `cam.simulate`'s `resolution`. Default: five times the tolerance, coarser where the stock or the program is too large for that. |

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
- **Workholding and Z travel.** With a workholding and a machine model, every move against the
  vise, clamps, parallels, rotary unit and table, the spindle head against the part, and the
  spindle nose against its travel (see [Workholding and the machine model](#workholding-and-the-machine-model)).
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
against; `null` without one), `workholding` (with a workholding), `gouge` (`worst`, `tolerance`, `tessellation`, `allowed`, `hits`,
`at: {operation, move_index, line?, xyz}` for the deepest, `skipped`, `pass`),
`holder_collisions` (`null` when no tool has holder data), `cut_minutes` (the program's time at
feed: as the machine runs it with the job's `machine_rates`, otherwise as written),
`rapid_minutes` and `machine_time` (with `machine_rates`, see [Machine rates](#machine-rates);
`null` without), `passed`, `failures`, `dry_run`, `outputs` (`gcode`, `preview`, `setup_sheet`,
`programs`), `notes` (a post that writes no work offset, an output extension other than the
post's, what the machine's rates say about the program, in a dry run too), `simulation` (with
`simulate`, as [`cam.simulate`](#stock-removal-simulation) replies), `summary` (the setup
sheet's tables, see [Setup sheet](#setup-sheet)), `warnings` (what to look at that does not fail
the job) and, with `split_by_tool`, `programs` (`order`, `path`, `tool`, `stickout`,
`operations`, `lines`, `cut_minutes`). The operations' minutes add up to
`readback.cut_minutes`, the program's as the controller times it.

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
| `machine_model_path` | A machine model file (the table, the spindle's nose and the machine's parts). See [Workholding and the machine model](#workholding-and-the-machine-model). Default: none. |
| `workholding_path` / `workholding` | A workholding file, or the same object written into the job: a vise, clamps, the table or a rotary unit. With a machine model every move of the program is checked against it. Default: none. |
| `bodies` | The part's bodies by name: the surface the 3D operations follow and every cut is checked for gouges against, drawn in the preview. Default: every visible body not in `fixtures`. |
| `fixtures` | Bodies the tool must stay clear of (vises, clamps, a fixture plate modelled in the design): checked for gouges and holder collisions, never machined. Default: none. |
| `tolerance` | 0.0001 to 1 mm (default 0.01): how far a toolpath may be from the profiles and surfaces it follows, and how deep a cut into the part counts as a gouge. |
| `tools` | The tools, as the CAM crate's `Tool` JSON (below). Give this or `tool_library_path`. |
| `tool_library_path` | A JSON file holding an array of tools. |
| `operations` | 1 to 200 operations, run and written in order (below). |
| `machine_rates` | The machine's top rates, to time the program as the machine runs it (see [Machine rates](#machine-rates)). Give this or `machine_rates_path`. |
| `machine_rates_path` | A JSON file holding `machine_rates`, read from the job file's folder when relative. |
| `tool_change` | What the machine does at a tool change (`T.. M6`): `auto` (a tool changer), `stop` (it stops and waits for Cycle Start) or `ignored` (the controller runs on with the tool in the spindle). With `ignored`, the post stops (`M0`) at each tool change after the first, and a program of several tools is a warning (in `warnings`) unless it is written with `split_by_tool`; the setup sheet says what the operator does at each change. Default: not said (no warning). |

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

## Workholding and the machine model

A job that names a workholding (`workholding_path`, or `workholding` written into the job) and
a machine model (`machine_model_path`) has every move of its program as written checked
against them: the cutter, the shank, every holder section and the machine model's parts
against the vise's jaws and body, the parallels, clamps, a rotary unit's headstock, chuck, jaws
and tailstock, and the table (the cutter may go the model's `table.cut_allowance` into it); the
spindle head's own parts against the part and the design's fixtures; and the spindle nose
against the top and the bottom of its travel (`spindle.nose_max_above_table`: the tool's
stickout, its holder and the nose gap above the tip). Each kind of collision is a failure once
per tool and operation (every operation that has it is named), with the G-code line where it
first happens, how many moves have it, the place in work coordinates and a fix; a nose past its
travel names the highest clearance that fits that tool, or says that no clearance above the
stock's top does (the tool is too long for the setup). The setup's own errors (the stock wider
than the vise opens, not gripped) fail it too. With the model's `soft_limits`, every move is
also held against them in machine coordinates, from where the workholding puts work zero on
the table (its `work_zero_machine`, else the travel centred on the table, which the failure
says). The workholding is placed with the job's stock (without one, the box round the part's
exact bodies; on a 4th axis the bar round them).

The reply's `workholding` carries the result: `name`, `holding`, `machine_model`,
`checked_moves`, `hits` (each with `code`, `error`, `tool`, `operation`, `line`, `moves`, `at`,
`message`, `fix`; at most 20), `pass`, `stock_above_jaws`, `bar_grip`, `setup` and
`setup_warnings`. In a rotary unit `bar_grip` says how the bar sits in the chuck along work X:
`face_x` (the chuck's face), `held_end_x` and `free_end_x` (the bar's ends), `in_jaws` (mm the
jaws grip), `stands_out` (mm from the jaws to the free end) and `zero_out` (how far X0 is out
from the face). In a vise, a job whose deepest side cut is deep for how little of the stock the
jaws grip is warned about in `setup_warnings` and the notes (a rule of thumb, not a failure). A
workholding without a machine model is refused, since the table places it.

### Machine model

A machine model file (`"format": "solidcraft.machine_model", "version": 1`) gives the table,
the spindle and the machine's parts. Every number must be finite. Keys this build does not know
(a file a newer build wrote) are left out, and the reply's `notes` name them with their path
(`machine_model_path m.json: this build does not know the key `paint` and left it out`).

| Key | Meaning |
| --- | --- |
| `name` | Shown in the reply. |
| `spindle` | `nose_gap` (the top of the tool's holder to the spindle nose, mm), `nose_max_above_table?` (the highest the nose gets above the table: the Z-travel check), `rpm?`. |
| `table` | `size` `[x, y, thickness]`, `corner_radius?`, `cut_allowance?` (how deep the cutter may go into the table top), `holes?: [{x, y, d, depth, label?}]` in the table frame. |
| `parts` | Up to 1000 of `{name, code, mount, shape}`: `mount` `spindle` (moves with X, Y and Z; Z from the spindle nose), `carriage` (X and Y; Z from the table top), `frame` (Y only; X from the table's centre) or `table` (fixed); `shape` `{"cylinder": {"diameter", "z": [low, high], "center?"}}` or `{"box": {"min", "max"}}`. `code` (kebab-case) is the findings' stem. |
| `display_meshes` | Up to 64 `{path, mount?, color?, name?, transform?, check?}`: a mesh-parts JSON file (`{"parts": [{"name", "mount", "color", "tris": [x, y, z, ...]}]}`), an STL file or a STEP file (its solids meshed through the import at 0.05 mm). `transform` is three rows of four numbers (rotation and shift) from the file's coordinates to its mount's frame. Pictures only (a later view draws them), unless an STL or STEP on `mount: table` gives `check: true`: then the tool and the machine are checked against its triangles too (findings `...-hits-machine`). A mesh that is checked and cannot be read refuses the job. A checked mesh needs a workholding (it places the table): without one the reply's notes say it is not checked. At most 2 million triangles checked by the machine model and the workholding together. |
| `soft_limits` | `{min: [x, y, z], max: [x, y, z]}`, the machine's travel in machine coordinates: every move is held against them. |
| `kinematics` / `kinematics_path` | Kept with the model; the job's machine gives the axes. |

The **table frame** has its origin at the table's front-left top corner, X to the right, Y to
the back, Z up.

### Workholding

A workholding file (`"format": "solidcraft.workholding", "version": 1`) says how the stock is
held; the job's stock is placed in it, and work zero follows.

| `holding` | Needs | |
| --- | --- | --- |
| `vise` | `vise: {model: {length, width, height, floor, fixed_jaw, moving_jaw, max_opening}, center, quarter_turns?, offset?, grip?}` | The stock against the fixed jaw, on the parallels (or the floor), centred along the jaws at `offset`. `quarter_turns` counter-clockwise from the jaws along X with the fixed jaw at the back. `grip` is a box (work coordinates) the jaws close on instead of the stock. |
| `clamps` | `stock_at: [x, y]` (its front-left corner on the table), `clamps: [{center, size, height}]` | On the parallels or the table; each clamp a box on the stock's top. |
| `table` | `table_zero: [x, y, z]` | The program machines the table itself. |
| `rotary` | `rotary: {axis: [y, z], headstock?: {min, max}, chuck: {diameter, length, face_x, side?, jaws?: {radial, axial, count?, width?, angle_deg?}}, parts?, rings?, tailstock?, plate?, zero_x?}` | A bar on the A axis; work zero on the axis at `zero_x`. `side` is the end of the bar the chuck holds: `plus_x` (the default: the unit at the table's +X end, +X pointing into the axis, the bar standing out towards -X, its +X end at the chuck's face unless `zero_x` is given) or `minus_x` (the mirror image: the bar's -X end at the face). The chuck (a cylinder about the axis) reaches back from its face away from the bar; the jaws stand `axial` out of the face towards the bar and `radial` beyond its surface. Without `width` they are checked as the ring they sweep as A turns; with `width`, as `count` (default 3) blocks that wide, spaced evenly, the first pointing straight up (+Z) at A 0 turned `angle_deg` about +X, turning with A: the gaps between them are free, and each move is checked with the jaws at its own A (where A is not known, as the ring). `parts` (up to 1000) are the unit's own parts instead of one `headstock` box, each named in the findings: `{name, box: {min, max}}` or `{name, cylinder: {diameter, x: [x0, x1], center?: [y, z]}}` (along X, about the A axis unless `center`), fixed to the table unless `turns: true` (fixed to the work: it turns with A). `rings` (up to 100) are `{name?, diameter, x: [x0, x1]}` about the A axis that turn with A (a home flag, a back plate). A bar that does not reach into the jaws is `stock-not-gripped`, one held by less than 3 mm `shallow-grip`. |

`parallels: {height, thickness, length, inset?, offset?}` go under the stock (along the jaws in a
vise, under the long edges with clamps).

`meshes` (up to 64, any `holding`) are mesh files of the workholding, read beside the workholding
file (or the job's folder for a workholding written into the job): `{path, name?, frame?, check?,
color?, transform?}`, an STL or a STEP file (meshed through the import). `frame: table` (the
default) is the table frame: checked triangle by triangle unless `check: false` (the
findings name it, `...-hits-vise`, `-clamp`, `-rotary`, or `-fixture` when the table itself is
machined). `frame: rotary` is the table frame at A 0, turning with A: a picture only (`check: true` is
refused; give what turns as the rotary unit's `rings`, `parts` with `turns` and `chuck.jaws`).
`transform` is three rows of four numbers from the file's coordinates to the frame. A checked
mesh that cannot be read refuses the job; one only drawn is a note.

A workholding file, like a machine model, leaves out keys this build does not know and names them
in the reply's notes; a workholding written into the job (`workholding`) refuses them, as
every key of a job does.

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
## Machine rates

A controller moves no axis faster than its top rate. A rapid takes as long as its slowest axis
needs, and a cut written faster than an axis can go is slowed down, so it takes longer than
its `F` says. That includes a feed above the machine's top feed, and a cut that turns A
faster than A can turn, in inverse time (`G93`) or not. Give the job the machine's rates and
`cam.job` times the program as the machine runs it:

```json
"machine_rates": {"rapid_xyz_mm_min": 8000, "rapid_z_mm_min": 4000, "rapid_a_deg_min": 8000,
                  "max_feed_mm_min": 8000, "max_feed_z_mm_min": 4000, "max_a_deg_min": 8000,
                  "spindle_rpm": [10000, 24000], "spindle_start_seconds": 6, "travel_mm": [300, 170, 130]}
```

| Key | Meaning |
| --- | --- |
| `rapid_xyz_mm_min` | Rapid rate of X and Y, and of Z unless `rapid_z_mm_min` is given. Required. |
| `rapid_z_mm_min` | Rapid rate of Z, where it differs. |
| `rapid_a_deg_min` | Rapid rate of the rotaries, degrees a minute. Without it a turn at rapid is not timed (the reply says so). |
| `max_feed_mm_min`, `max_feed_z_mm_min` | Top feed of X and Y, and of Z. |
| `max_a_deg_min` | Top rate of the rotaries while cutting, degrees a minute. |
| `spindle_rpm` | `[lowest, highest]`: the controller runs any other `S` at the nearest end of its range. |
| `spindle_start_seconds` | How long the controller waits at each spindle start (`M3`, `M4`). |
| `travel_mm` | `[x, y, z]`: the travel of the linear axes. |
| `accel_mm_s2`, `accel_z_mm_s2` | Acceleration of X and Y, and of Z, mm/s². Only the simulation view's timeline uses them; `cam.job`'s times leave acceleration out. |
| `accel_a_deg_s2` | Acceleration of the rotaries, degrees/s² (the simulation view). |
| `tool_change_seconds` | How long a tool change (`M6`) takes, added to `total_minutes` for each; the simulation view's timeline waits it. |
| `home_z_mm` | Where Z's home is in work coordinates (mm), to time the returns home (`G28`); default the highest Z the program reaches. |

A rate left out is not limited. Every rate must be above 0. Unknown keys are refused, in a
`machine_rates_path` file too.

With rates, the reply's `cut_minutes` is the time at feed as the machine runs it (dwells
included), `rapid_minutes` the time at rapid, and `machine_time` has the detail:
`written_cut_minutes` (the `F` words alone), `spindle_minutes`, `total_minutes`,
`slowed_blocks` (and of those `rotary_over` and `feed_over`), `peak_rotary_deg_min` and
`peak_feed_mm_min` with their lines, `spindle_out_of_range`, `span_mm` and `warnings`. The
warnings are in the reply's `notes` too: blocks that ask A for more than `max_a_deg_min`
(the controller slows them, so the time is longer and the feed at the tool is lower than
written), blocks that ask a linear axis for more than its top feed, spindle speeds outside
`spindle_rpm`, and a program that spans more than `travel_mm`. Without rates, `cut_minutes`
is the time as written and `rapid_minutes` is `null`.

The times leave out acceleration, so they are a lower bound for programs of many short blocks.
A return of Z to the machine's home (`G28 … Z`: before a tool change, and in every rewind of a
post whose `safe_rotate` lines go home) is timed at the Z rapid rate up to `home_z_mm`, Z's home
in work coordinates, and the next move from there; without it, up to the highest Z the program
reaches, a lower bound. `home_returns` counts them.

## Setup sheet

`cam.job {…, "setup_sheet": "C:/parts/bracket-setup.html"}` writes what the machine operator
needs to set the job up beside its program: a self-contained HTML page (no scripts, nothing
loaded from the network, print styles so it prints on one or two pages) for a `.html` or `.htm`
path, Markdown for `.md` or `.markdown`. Any other extension is refused before anything is
computed. Like the G-code and the preview it is written through the session's file access, so
an MCP server's `--allow-path` folders apply, and it must be a file of its own (not the
`output` or the `preview`, however they are spelt). The sheet carries no date and nothing of
the computer it is made on: the same job writes the same bytes.

It holds, in order:

- **The job:** its name, the machine, the post, the G-code file's name (with `split_by_tool`,
  `Programs`: the files written, in run order; the one file of every tool is not written) and the
  work offset it runs in (`G54` to `G59`, or a note that the post writes none).
- **Before you start: the machine's limits**, when the job runs into any: the blocks the
  controller will run slower than written (an axis asked for more than its top feed or rate, a
  spindle speed outside its range, by the job's `machine_rates`). They are in the notes too;
  here they come first, so the operator reads them before the program runs.
- **Tool changes**, when the job says what the machine does at one (`tool_change`): with
  `stop`, put in the tool each change names, set its length and press Cycle Start; with
  `ignored` and one program per tool, run the programs in order, changing the tool and setting
  its length before each; with `ignored` and one program of several tools, the program stops
  (`M0`) at each change for the operator.
- **Programs, in the order to run them**, with `split_by_tool`: each file with its tool, the
  tool's stickout, its operations and its minutes, and a box to tick.
- **Work zero:** the design point that is X0 Y0 Z0 and the part's extent from it (from the part
  bodies' exact bounds), with each axis in words (on the part's left side, in the middle, on its
  top, so many mm below its bottom). On a 4th axis set up the usual way (`4axis_a`) Y0 Z0 is the
  A axis centreline.
- **Each rotary axis:** what it turns and about which line, which way positive turns it
  (counter-clockwise seen from the +X end for `4axis_a`; clockwise with `a_reversed`), which side
  of the part A+90° brings up under the tool and which way the top of the part moves as A
  rises, and its travel limits when the machine has them.
- **Tools:** only those the operations use, by number: kind, diameter (and corner radius),
  flutes, flute length, shank, stickout, holder, the lowest Z the program sends the tip to
  (Z min, in program coordinates), the operations that use it, and its description. HTML
  sheets have a box to tick per tool.
- **Operations** in program order: name, kind, tool, spindle speed, cut and plunge feed,
  coolant, the A angle as written, Z min, cut length and minutes at feed. Z min and the A
  angles are read from the written program itself, each operation's blocks being those after
  the comment the post writes at its start. With a post that writes no comments they are worked
  out from the toolpaths instead.
- **Totals:** operations, tools, tool changes, length and time at feed as planned and as the
  controller times the written program, the program's line count, and the X, Y, Z and A range
  the program moves over.
- **Checks:** the read-back and, when it ran (`simulate`), the simulation, each with its result.
  The simulation's leftover is the most material left on the part (see
  [Stock-removal simulation](#stock-removal-simulation)); within one cell of the simulation the
  line says so, as the raster can tell no finer.
- **Warnings:** failed checks (a sheet is only written with them under `allow_unsafe`), the
  job's warnings (a program of several tools for a machine that ignores tool changes), a rotary
  that turns past a full turn either way.
- **Notes:** a post that writes no work offset and the like.

Every name and description from the job is escaped for the format, so a name such as
`<b>|*x*` shows as written and never breaks the page or a table.

The reply's `summary` carries the same tables in compact form, on a dry run too, so an agent can
tell the user what to set up without opening the sheet: `work_zero` (the sentences),
`part_extent` (`[[x, y, z] lowest, [x, y, z] highest]` from work zero), `work_offset`, `rotary`
(the sentences), `tools` (`number`, `kind`, `diameter`, `flute_length`, `stickout?`, `holder?`,
`z_min?`, `operations`), `operations` (`name`, `kind`, `tool`, `spindle_rpm`, `feed`,
`plunge_feed`, `rotary?`, `cut_length`, `cut_minutes`), `totals` (`operations`, `tools`,
`tool_changes`, `cut_length`, `cut_minutes`, `program_minutes?`), `checks` and `warnings`.

## Stock-removal simulation

`cam.simulate` checks a written program independently of the toolpaths it came from: it reads
the G-code as the controller would, sweeps each tool through a model of the stock along every
block (rapids included), and compares what is left with the part. `cam.job` runs the same check
on the program it has just written when given `simulate: true`.

```json
{"path": "C:/parts/bracket.ngc", "job_path": "C:/parts/bracket.cam.json", "resolution": 0.05, "stock_stl": "C:/parts/bracket-stock.stl"}
```

| Key | Meaning |
| --- | --- |
| `path` | The G-code file, in the job's work coordinates. |
| `job` / `job_path` | The job the program was written for: its machine, tools (by `T` number; `T… M6` loads one), part bodies, `origin` and `tolerance`. |
| `resolution` | The cell size in mm. Default: five times the tolerance, coarser when that would be more than 4 million cells, or when the program's cuts would visit more cells than the simulation allows (a long 4th-axis program: a warning gives the cell used). More than 20 million cells is refused. |
| `stock_stl` | Write the stock the program leaves as a binary STL file (its top surface, or its round surface on a 4th axis), for a preview. |
| `then` | Programs run after `path`, in order, on the stock it leaves (a job written one program per tool): what they leave together is checked. Line numbers count on through them. |

The reply: `stock` (`kind`: `box` or `cylinder`, and its size), `resolution`, `cells`,
`gouge: {worst, cells, at?}` (the deepest cut into the part beyond the tolerance, how many
cells, and where the deepest is, work coordinates), `rapids_in_material: [{line, volume, depth}]`
(rapids that cut more than the tolerance into the stock: a crash on the machine),
`leftover: {max, mean, cells, at?}` (material left above the part over the cells the program
cut, and where the most is), `part_left` (material left on the part anywhere, below),
`verdict`, `removed_volume` (mm³), `minutes` (at feed, as the controller times it),
`slot_moves` (cuts at the tool's full width, also listed in `warnings`: keep their depth and
feed low on a light machine), `air_moves` and `air_length` (cutting moves that removed nothing,
and their length in mm: cutting air), `passed` (no gouge and no rapid in material), `failures`,
`warnings`, `notes` and `stock_stl`. A program with no move the simulation can follow (on a 4th
axis, one that never writes A: no block's position is known) fails rather than passing with
nothing checked. A failed simulation is reported, never refused; inside `cam.job` its failures
join the job's. Spindle speeds outside the job's `machine_rates.spindle_rpm` are a warning, and
the tool changes the program stops for (an `M6`, or an `M0` on a machine that ignores them) are
a note.

**The stock.** The simulation starts from stock taken from the part: on a 3-axis machine the
part's box, on a 4th axis the cylinder about work X through the part's farthest point from the
axis, over the part's length. A program whose rapids are safe for a block-shaped part may then
cut that round bar at its retract height, which is what the bar would do on the machine.

**Material left on the part.** Each column of the simulated stock (every few cells) is held
against all of the part along it, so stock never cut (an unmachined pocket, what stands under
the part's own material) is found too; the stock round the part and below its bottom is not
counted. `part_left` gives the `volume` (mm³), `hidden_volume` (under the part's own material
along Z), the thickest `max`, the worst `spots` (`[x, y, z, thickness]`), `hidden_area` and
`allowed` (the tolerance, a cell and the job's most `stock_to_leave`). On a 3-axis machine what
stands under the part's own material is a setup from the other side's (`later_volume`); the
rest is `open_volume`, thickest `open_max`. When the rest is thicker than `allowed` the verdict
is `part not finished`, with a warning naming the spots: not a pass and not a failure. Inside
`cam.job` that warning is in the reply's `warnings`, and the setup sheet's Simulation row says so.

**3-axis.** The stock is a heightfield over the box: one top height per cell, at the cell's
centre. Every block lowers each cell to the tool's lowest point over it along the whole move,
found exactly, not at sampled positions: along a straight move the tool's height over a cell is
convex in the position along the move, so its least value is found in closed form (flat end
mill, ball, drill point at 118°) or by bisection on its slope to rounding (bull nose). A level arc
is exact as well; an arc that also moves in Z (a helix) is followed as chords at most a quarter
of the tolerance inside it. An arc whose end is written at its start is a whole turn, as the
controller runs it. An arc in the XZ plane (`G18`, centre from `I K`, `G2` clockwise seen from
+Y) or the YZ plane (`G19`, centre from `J K`, `G2` clockwise seen from +X) is followed about its
own centre, as chords at most a quarter of the tolerance inside it. The tool is its cutting end
with its diameter carried on up, so a cut deeper than the flutes still removes material.

**4th axis (A about X).** The stock is a grid over (x, A) holding how far out the material
reaches along the ray from the axis. A block that turns A while X moves is a helix in the work
frame, so the tool is placed at points along each block (every axis moving evenly, as written),
close enough that the cusps between them are at most half the tolerance. At each point every
ray near the tool is cut over the stretch of it inside the tool. A cut that reaches out of the
material along the ray ends the ray where the tool enters it; one that crosses the ray inside
the material without reaching out of it leaves a stretch of air inside the ray's material, so
indexed work (A held while X, Y and Z move) is modelled in the part's own frame as exactly as
turning is. A ray holds at most 32 stretches of air and a band of the stock 4 million: a cut
beyond is left out and counted in a warning.

**Against the part.** The part is the job's bodies tessellated from their exact surfaces within
a quarter of the job's tolerance, in work coordinates. Each cell's part height (or radius) is
where a ray from above (from outside) first meets the part's facets. A cell cut below the part
by more than the tolerance is measured again in 3D over the part's material along its column,
so a cut beside a steep wall within the tolerance of the wall is not a gouge, and neither is a
column down the air of a hole or a pocket. Where a column's crossings do not pair up (a part
whose facets face inwards, bodies that overlap), the whole column past its first crossing counts
as material, so a gouge is never passed as air. Leftover is measured along Z (along the ray),
and where it could be the most also in 3D from the part's surface, so a sliver beside a steep
wall counts as thin as it is there, not the wall's height. The position is lost at a return home
or a move in machine coordinates (`G28`, `G30`, `G53`); the next block is taken as coming
straight down the tool axis from above the stock, and so is the program's first.

**Time.** The grid is split into bands of rows (of angles on a 4th axis), each running the whole
program on a thread of its own, one per core up to sixteen (one on the web); the result does not
depend on how many.

## Simulation view

`cam.view.load` loads a job's program into the simulation view: the job planned and written in
memory (as `cam.job` with `dry_run`), or a G-code file of it (`program`). The program is cut
into the stock move by move as the view plays, with the same exact model as `cam.simulate`
(cutting part of a move and then the rest leaves what cutting it whole does), and what goes wrong
is found at the move and place it happens. The stock is taken from the part, as `cam.simulate`
takes it. The view is view state only: nothing is saved in the design, and no command of it is
an undo step.

The job is read and planned as `cam.job` reads and plans it, so the view plays the program
`cam.job` would write, block for block, each under its operation. A job `cam.job` refuses is
refused at load with the same message.

```json
{"cmd": "cam.view.load", "params": {"job_path": "C:/parts/bracket.cam.json", "run_to_end": true}}
{"cmd": "cam.view.seek", "params": {"by": 1, "until": "finding"}}
{"cmd": "cam.view.state", "params": {"findings": "all"}}
```

| Command | Does |
| --- | --- |
| `cam.view.load` | `job` or `job_path`; `program?`; `resolution?`; `run_to_end?` (cut it all once at load, so every finding is known, then go back to the start); `colour_by?`. Replies with the moves, the operations (from the job, or one per tool change for a program file), the tool changes, the program's time, the stock and what is known to be wrong already. |
| `cam.view.seek` | Exactly one of `move` (to its end), `line`, `op` (a name or a number, with `at`: `start` or `end`), `time_s`, `percent`, `tool_change`, `finding`, `by` (moves on or back; with `until`: `tool_change`, `op` or `finding`, to the next one). Going back puts the stock back from the nearest copy kept before the target and cuts on from there. |
| `cam.view.play`, `cam.view.pause` | Whether a window plays the view on, at `speed` times real time or `moves_per_s`, with `loop`. Headless, seek instead. |
| `cam.view.show` | What is drawn: `toolpaths`, `rapids`, `remaining_path`, `stock`, `part`, `tool`, `holder`, `gouges`, `collisions`, `deviation`; `colour_by`. |
| `cam.view.state` | Where the tool is: the move and the share of it done, the line and its G-code, the operation, the tool, the tip, the axis and the machine's words (A included), the feed, the spindle's speed, the time, the material removed, and the findings so far by code (`findings: all` lists them). |
| `cam.view.export_stock` | The stock as cut so far, as an STL file. |
| `cam.view.close` | Drops the view. |

**What is found.** `rapid-in-stock` (a rapid cutting more than the tolerance), `gouge` (a cell
cut below the part, measured again in 3D), `cut-spindle-off` (cutting after `M5`, once the
program has started the spindle), `shank-rubs` (material above the flute length where the tool
cuts), and `shank-hits-stock` and `holder-hits-stock` (the shank or a holder section coming down
on the stock as it is then). On a 4th axis the shank and holder are checked against the round
stock at points round their bottoms.

**Time.** Each move is timed with the job's machine rates (without them, rapids at 5000 mm/min
and cuts at their feed). Where the rates give accelerations, a move speeds up and slows down as
a trapezoid, slowing at a corner to the speed both moves share times the cosine of the turn (to
a stop between a rapid and a cut, or at a right angle), round an arc to √(a·r); spindle starts
and tool changes wait their time. A 1000 mm cut at 6000 mm/min with 2000 mm/s² takes 10.05 s.

**Drawn.** A picture of the model (the MCP `screenshot` tool, a window) draws the stock as cut
so far coloured by the tool that cut it last, the toolpaths in one colour per tool with what is
still to come dim and rapids in red, the tool with its shank and holder at the tool's pose, and
the findings as crosses. The design's own bodies are hidden unless `part` is shown.

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
