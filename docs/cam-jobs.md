# CAM jobs

A CAM job turns a design's profiles into G-code for one machine. It is a JSON file kept next to
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
| `bodies` | The part's bodies by name, drawn in the preview. Default: every visible body. |
| `tolerance` | 0.0001 to 1 mm (default 0.01): how far a toolpath may be from the profiles it follows. |
| `tools` | The tools, as the CAM crate's `Tool` JSON (below). Give this or `tool_library_path`. |
| `tool_library_path` | A JSON file holding an array of tools. |
| `operations` | 1 to 200 operations, run and written in order (below). |

### Coordinates

Work coordinates are the design's coordinates less `origin`. The operations' polygons and
heights are in work coordinates, and a sketch is read through the same shift. On a 4th axis
the rotary axis is work X through work zero; a contour or a pocket runs with A held at zero.

### Tools

`number`, `kind` (`flat_endmill`, `ball_endmill`, `bull_nose`, `drill`, `face_mill`, `chamfer`,
`thread_mill`), `diameter`, `flute_length`, and optional `description`, `flutes`,
`spindle_rpm`, `feed`, `plunge_feed`, `stepdown`, `stepover`, `corner_radius` (bull nose; a
face mill's inserts), `tip_diameter` and `angle_deg` (chamfer mill), `shank_diameter`,
`stickout`, `holder: [{diameter, length}]`, `point_angle_deg` (a drill's point, default 118),
and for a thread mill `thread_pitch` and `tooth_height`. An operation without `spindle_rpm` or
`feeds` takes the tool's; its stepdown and stepover default to the tool's too.

A `contour` or a `pocket` cuts with a flat end mill, a ball end mill, a bull nose or a face
mill; any other tool is refused, with the operation named.

### Operations

Every operation has `name`, `type`, `tool` (a tool's `number`), and optionally `spindle_rpm`,
`feeds: {cut, plunge}`, `coolant` (default false) and `enabled` (default true: `false` keeps it
in the job but leaves it out of the program). `heights` are `{clearance, retract, top,
bottom}` in work coordinates.

| `type` | Keys (defaults) |
| --- | --- |
| `contour` | `sketch` (id or name) + `profiles?` (indices; default every drawn profile) or `polygon: [[x, y], …]`; `heights`; `stepdown` (tool's, else half its diameter); `side`: `outside` \| `inside` \| `on` (outside); `climb` (true); `stock_to_leave` (0); `ramp_angle_deg` (0: plunge); `tolerance` (the job's) |
| `pocket` | `sketch` + `profiles?` or `polygon`; `heights`; `stepdown`; `stepover` (tool's, else 40% of its diameter); `climb`; `stock_to_leave`; `ramp_angle_deg`; `tolerance` |

A sketch an operation uses must lie on a plane square to the tool, facing +Z or -Z (one facing
away is read as seen from the tool, mirrored), in the design's root component. Contours and
pockets are written with arcs kept (`G2`/`G3`).

### Feeds

Every move runs at the operation's cut feed except these, which go down no faster than the
plunge feed:

- **Entries.** Each cut is entered from above, the last of the way at the plunge feed. A
  `pocket` level comes down at rapid to 1 mm above the level before it, and the first one, like
  a `contour`, to 1 mm above `top` (where that is below the retract height).
- **Ramps.** A `contour` or `pocket` with `ramp_angle_deg` goes down along the profile at
  exactly that angle (the last lap only part of the way round, then on round at the depth),
  each ramping move slowed so the tool goes down no faster than the plunge feed.

These feeds are worked out again from the coordinates as the post writes them, and rounded
down to its feed digits, so the program as written never goes down faster than the plunge feed
either.

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
