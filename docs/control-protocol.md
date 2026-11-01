# Control protocol

For UI-kit review, `ui.status_fixture {"preset":"mode"}` shows bounded status
specimens in the real viewport. Presets are `off`, `mode`, `result`, `attention`,
`row`, `kinds`, `many` and `key`; the reply carries the preset and item count. This
scenario fixture does not change engine state or open a dialog. `off` clears it.
The published `viewport_status:<id>:item`, `:setting`, `:key` and `:close` handles
support the ordinary `ui.at`, `ui.move`, `ui.click` and keyboard controls.
With `viewport_status_summary` enabled, `kinds` (three) and `many` (four) exercise
the summary component, using explicit specimen newest metadata. The
`viewport_status:summary` and `menu:viewport_status_clear_all` handles open its
native menu and activate its footer. These specimens are not persistent engine
results; actual production sources currently supply at most three Mode items.

Toasts the user may close have a close (×) at their top right instead of a Close button, published
as `toast:<name>:close`: `toast:status:close`, `toast:space_mouse_offer:close` and
`toast:stand_up:close` (#7971). Those that go by themselves draw a thin ring around it that
shrinks over their time; the ring is full again while the pointer is on a toast that waits for it.

Live details publish `details:field:<id>`, `details:show_all` and `details:action:copy`
handles. `details_findings` and `details_readout` are explicitly labelled component-only
`ui.status_fixture` presets. Real card settings execute the normal analysis/Section commands;
agents can read the same structured settings and retained Continuity records with the pure
`view.status_details` engine command. Its `current` flag guards geometry targets, and its
per-record limits prevent mixed measurements from being mistaken for one global tolerance.

The control channel is **on by default**. SolidCraft listens on `127.0.0.1` only, for JSON lines,
so agents can drive the app without a launch flag. Every connection still needs this launch's
token (below); nothing else on the machine, and nothing off it, can send a command.

Internal rendering harness: `ui.blur {radius_px?, surface_alpha?}` queries or sets
session-only panel blur. Radius is finite physical pixels in 0..40; the optional
capture tint is 0..1 (`null` restores ordinary fills). This is not a saved
preference. The reply contains requested radius, capability/reason and cumulative
actual effect counters, including live resources and encoded passes. Unsupported
software/web/source contexts stay on ordinary painting. Use `ui.screenshot` for
panel evidence; `ui.render` remains a model-only capture. Activation, full egui GPU
fixture and manager-run evidence commands are in [blur-behind.md](blur-behind.md).

| Launch | What listens |
|---|---|
| `solidcraft --control PORT`, or `SOLIDCRAFT_CONTROL_PORT=PORT` | exactly `127.0.0.1:PORT` (the flag wins over the variable), whatever the setting |
| neither, and Preferences > Assistant > "Let AI agents connect (control channel / MCP)" on (the default) | the default port **27878**, or the next free one up to 27893 when it is taken (a second window, another program) |
| neither, and the setting off | nothing |

The setting is read at start-up. Old preference files, which don't have it, load with it on. A
launch with `--control` or the variable behaves as it always did (the start page stays hidden,
the window doesn't ask for the foreground), because a scripted launch expects that; the default
channel changes neither for a normal launch.

### Finding the port

Each app that opens its channel writes the port to a discovery file beside the token files,
`control/current.port` (same folders and permissions as below), and removes it at exit if no later
app has replaced it: it names another port, or another app now holds that port's token file (two
Flatpak windows, each with its own loopback, can listen on the same port number). It holds only
the port, never the token. `solidcraft-cli mcp` with no
arguments reads it, checks that something listens there and bridges to that app; with no app
running, or with `--headless`, it runs a session of its own. `--connect HOST:PORT` and the
other arguments still decide first. With several windows open, the most recently started one is
the one found; use `--connect` for another.

Requests are one line `{"id": …, "method": "…", "params": {…}}`; replies are described below. Each request is one line `{"id": …, "method": "…", "params": {…}}`; each reply is one
line `{"id": …, "ok": true, "result": …}` or `{"id": …, "ok": false, "error": "…"}`. Requests are
handled on the UI thread between frames; pointer and keyboard input is injected as real egui
events, one pointer event per frame. A connection that sent `control.subscribe` is also sent
event lines `{"event": "…", "params": {…}}`, which have no `id`
([Following the design](#following-the-design)).

### Token

Every connection must authenticate. At start the app makes a random 256-bit token for this
launch and writes it, as one line of hex, to a file only the user can read:

| System | Token file |
|---|---|
| Linux | `$XDG_DATA_HOME/solidcraft/control/PORT.token` (`~/.local/share/solidcraft/control/PORT.token`) |
| macOS | `~/Library/Application Support/SolidCraft/control/PORT.token` |
| Windows | `%LOCALAPPDATA%\SolidCraft\control\PORT.token` |

On Unix the folder is `0700` and the file `0600`; on Windows the file's inherited permissions are
replaced by a single entry for the user. `SOLIDCRAFT_CONTROL_DIR` moves the folder. The file is
removed when the app exits, unless another launch has written its own token there since, and the
next launch writes a new token. The token file's path is
printed to stderr at start-up. Beside it, `<port>.pid` holds the app's process id (and, on Linux,
its start time).

An app that is force-quit can't remove its files, so a later client may send that launch's token.
When the app refuses it, `solidcraft-cli mcp` names the token file it read and when it was
written, and says the file is stale when the process in `<port>.pid` has exited. Delete the file
or restart SolidCraft. On Windows, an app or client started from inside an MSIX-packaged app (and
the tools it launches) has its AppData redirected: it writes and reads
`%LOCALAPPDATA%\Packages\<package>\LocalCache\Local\SolidCraft\control\` before the folder above.

The first request on a connection must carry the token as a top-level `"token"` member
(`{"id": 1, "method": "ui.inspect", "params": {}, "token": "…"}`); later requests on the same
connection may leave it out. A first line without the right token is answered with
`{"ok": false, "error": "unauthorized: …"}` and the connection is closed, without running
anything. Tokens are compared in constant time.

`solidcraft-cli mcp --connect 127.0.0.1:PORT` reads the token file for that port by itself
(`--token T` or `SOLIDCRAFT_CONTROL_TOKEN` give one explicitly). For a test harness, or a
sandboxed package whose data folder the client can't see, start the app with
`SOLIDCRAFT_CONTROL_TOKEN` set to at least 32 hex characters (for example
`openssl rand -hex 32`) and the app uses that token instead of making one; the file is still
written. A one-line client in Python:

```python
token = (Path.home() / ".local/share/solidcraft/control/7878.token").read_text().strip()
sock.sendall((json.dumps({"id": 1, "method": "ui.inspect", "params": {}, "token": token}) + "\n").encode())
```

### Limits

The channel is untrusted input. A request line may be at most 16 MiB; a longer one is answered
with `{"ok": false, "error": "request too large"}` and the connection is closed, as are
connections that send eight lines that are not JSON. At most 16 connections are served at once
(further ones get `{"ok": false, "error": "too many connections"}`), at most 64 queued requests
run per frame so a flood cannot starve drawing, and a request that the UI thread has not
answered within 60 seconds (3600 seconds for `engine.batch`) is replied to with
`{"ok": false, "running": true, "error": "timeout: … is still running …"}`. The same 16 MiB line
limit applies to the MCP server.

A timed-out request has not failed: it is still running in the app, and a change still applies
when it finishes (a long fillet, say), so don't simply send it again. The timeout's text says
what it still does: a request that only reads (`ui.inspect`, `document.inspect`, an inspection
command) "only reads: nothing changes"; one that only writes a file (`file.export`,
`file.save`, `file.save_as`, `file.save_mesh`, another export given a `path`, `ui.render`, or
`ui.screenshot` with a `path`) "will still write the file when it finishes" and leaves the design
as it is; any other request "will still apply when it finishes". The connection keeps its reply (#1618). Ask for it
with `control.pending`: `{"pending": <id>, "method": …, "done": false}` while it runs, then
`{"pending": <id>, "method": …, "done": true, "reply": {…}}` (its own reply, once), and
`{"pending": null}` when nothing is outstanding. Until it lands, any other request on that
connection is refused at once with `{"ok": false, "busy": true, "error": "busy: …"}` and not
run, so nothing is queued to apply unseen behind it. `control.pending` belongs to the
connection: a new connection has nothing pending. The bridged MCP server does this itself
([mcp.md](mcp.md#long-running-requests)).

`control.pending` answers only for its own connection. `control.running` answers for the whole
app, the same on every connection, at once (it never waits behind a running request, and a
connection may ask it while it is `busy`): every request the app has been sent and has not
answered yet, oldest first, from any connection, including a timed-out one whose connection has
since closed:

```text
{"id":1,"method":"control.running","token":"…"}
→ {"id":1,"ok":true,"result":{"running":[
    {"connection":3,"id":17,"method":"engine.execute","command":"file.export","running_s":312.4,"queued":false},
    {"connection":5,"id":2,"method":"engine.batch","steps":12,"running_s":4.1,"queued":true,"self":true}]}}
```

`running_s` counts seconds since the request arrived; `queued` is true while it still waits its
turn behind another. `command` is the command id of `engine.execute` (a known one only) and
`steps` the length of `engine.batch` or `engine.script`; parameters are never shown, so file
paths and expressions stay with the connection that sent them. `self` marks the asking
connection's own request. `{"running": []}` when the app is idle.

`control.capabilities` says what this app's control channel does beyond the methods every
version has, so a client can find out before it relies on it: `{"param_check": ["off", "warn",
"strict"]}`, the levels a request's own `param_check` takes (below). It is answered at once, like
`control.running`, also on a `busy` connection. An app older than it answers ``unknown method
`control.capabilities` ``; a feature added later is listed here too.

Any request's `params` may carry `allow_paths: ["/abs/folder", …]`: every file that request
reads, writes or deletes (command paths such as `file.export`'s `path` or `doc.recovery_*`'s
`dir`, `ui.render`, `ui.screenshot`) must then lie inside one of those folders after `..` and
symlinks are resolved, or the request fails with `… outside the folders this agent may use …`.
The app adds its own recovery folder and, when it lies safely inside the home folder and not in
a hidden folder, the open design's folder (whose hidden files stay out of reach). With
`allow_paths`, `design_folders: ["/abs/folder", …]` (at most 16) names folders the agent opened
or saved a design in; each counts on the open design folder's terms. The bridged
MCP server sends its folders this way on every request ([mcp.md](mcp.md)). Without
`allow_paths` file paths are not limited: a client holding the token can do what the user can.

`engine.execute`, `engine.batch` and `engine.script` may carry `param_check: "off" | "warn" |
"strict"` in their `params`: the commands of that request are checked against their typed
contracts at that level, whatever the app's own (which is `off`), as a headless session's
`--param-check` checks them (#9184). `warn` runs a call that breaks its contract and adds
`param_warnings` to its result; `strict` refuses it (a batch before its first step), and the
design is left as it was. Every reply to such a request says the level it was checked at as
`param_check`, beside `ok`. A command that switches to another design (`component.edit_in_place`,
`component.finish_edit`, `view.next_tab`, `view.previous_tab`) has its parameters checked at
that level before it runs, and then runs at the app's own level, as each design keeps its own.
Any other word is refused; other methods ignore the field. An app older than this ignores the field and its reply has no
`param_check`, so a client that needs the check asks `control.capabilities` first and treats a
reply without `param_check` as unchecked. The bridged MCP server asks once per connection
([mcp.md](mcp.md)).

## Engine

| Method | Params | Result |
|---|---|---|
| `engine.execute` | `command`: command id, `params`: JSON object | the command's result |
| `engine.script` | `commands`: `[{command, params}, …]` | results, stops at the first error |
| `engine.batch` | `commands`: `[{command, params}, …]` (at most 10 000), `rollback?`: bool (default true), `timings?`, `profile?`, `coalesce_parameters?`: bool (default false) | `{completed, results, undone}`, plus `failed: {index, command, error}` when a step fails, and with `timings` `timings: [{index, command, ms}]` (the failing step included) and `total_ms`; `profile` adds each step's `profile: {boolean_ms, tessellate_ms, other_ms}`. Runs the steps in one request, so within one frame, each as `engine.execute` runs it. The first failure stops the batch, and with `rollback` the design goes back to how it was before it (`undone`: the steps taken back, `rolled_back`: whether the design is back as it was, `false` only when the checkpoint could not restore it). Refused before anything runs, with an error starting `engine.batch unavailable`, while the agent cursor is shown, when a step loads a file (`doc.open`, `file.insert_step`, `file.insert_mesh`) or when it goes to another design's tab (`component.edit_in_place`, `component.finish_edit`, which `engine.script` refuses too). Send such steps one by one. A step the batch would certainly refuse at its turn is found before the first step runs (#5853): a step without a command, an unknown command id (with the closest ids) or, with strict parameter checking, a call that breaks its command's contract. The reply is then that step's failure with `completed: 0`, `undone: 0`, `not_run` listing every step and a `note`; nothing has run. With `coalesce_parameters`, each run of consecutive plain parameter edits (`parameters.change` with only a name and an expression, or with `changes`) is set as one `parameters.change {changes}`, so the design rebuilds once for the run. Every step still gets its own result (with `coalesced`: the run's length), and the run is one undo step. The run skips the states between its edits: an edit that would be refused on its own, but not once the run's later edits are in, goes through. A run whose combined edit is refused (a new parameter, a bad expression) runs step by step, so the refused step is named. Answered when done, for up to 3600 s |
| `engine.commands` | — | every command: id, label, tab, panel, icon, shortcut (the user's current first key, or null when cleared), params doc, enabled |
| `document.revision` | — | `{document_id, revision}`: the active design's id and a number that moves whenever the design changes, for programs that follow it ([Following the design](#following-the-design)) |
| `control.subscribe` | — | `{document_id, revision}`, as `document.revision`; the connection is then sent a `design.changed` line whenever they change ([Following the design](#following-the-design)) |
| `document.inspect` | `measure?`: bool | parameters, timeline (errors, timings), bodies, sketches, selection (assembly joints: `engine.execute` `joint.inspect {joint?}`, which reads only: each joint's two origins with the occurrence, the snap as saved, the feature, face, edge or sketch curve its names refer to, and whether it resolves or why not; kind, offset, angle, flip, values, limits, expressions and motion links) |

Commands never open dialogs when run this way. `solidcraft-cli commands` prints the same registry.

### Following the design

A program that follows the open design (a renderer kept in step, a live link) asks
`document.revision`, which only reads and does not wait behind design changes (a slow command
computing, a file loading, the agent cursor acting one out), and exports only when the answer
changes (#6646):

```text
{"id":1,"method":"document.revision","token":"…"}
→ {"id":1,"ok":true,"result":{"document_id":"18f3a0c2d4e5b6a7-1","revision":3}}
```

- `document_id` names the active design (its tab) while it stays open, through edits, saves and
  renames. Two open designs have different ids even with the same name, and each design started
  or opened gets a new one, also one opened or started in the same tab. Ids of one run never
  match those of another. Treat it as an opaque string.
- `revision` moves whenever the design changes: an edit, a parameter, undo or redo, a design
  opened or started, what is hidden or shown (saved with the design), a component moved and not
  captured yet. It stays the same for reads, a selection, a camera move, a section or an analysis
  view. Revisions come from one counter for the whole run, so a revision never names two
  states, and it never goes back: changes undone before the next ask that end where the design
  was need no new one (its revision still names that design).

Switching tabs changes `document_id`; back on a tab its revision is the one it had, if nothing
changed it meanwhile. Compare both values, and export when either differs from the last
answer. The same answer comes from `engine.execute` `document.revision` (and so from scripts and
MCP `execute`), which waits its turn behind design changes like any command.

`engine.execute` with `view.status_items` and `{}` reads the persistent viewport effects without
evaluating the model or changing the design, undo history or view revision. It returns
`{items: [...]}` in oldest activation order. Each item contains a stable slot `id`
(`surface_analysis`, `section`, or the legacy independent `continuity`), `kind`, source `command`, structured `setting`, `has_key`
and a `clear: {command, params}` request that can be executed unchanged. Settings retain numbers:
lengths use millimetres, Draft's `angle_deg` uses degrees, directions are normalised, and
Continuity's `edge_count` counts the seams currently kept for shading. Labels and units are
formatted by clients. `engine.describe {command: "view.status_items"}` exposes both schemas.

By default, surface analyses, including Continuity, share one slot; successful replacement keeps that
slot's position and ends the previous analysis. Section stacks independently. Clearing and
re-enabling a slot makes it newest. Ordering metadata belongs only to the running session;
effects and ordering are not saved in the design. Existing analysis clear semantics apply:
any surface-shading command with `{clear: true}` clears the active surface shading, whereas
Continuity clears its seam records. Section clear never clears an analysis.

The standard **Graphics → Viewport → Use a single surface analysis** preference is
`single_surface_analysis` (default `true`). `ui.prefs {"single_surface_analysis": false}` restores
the legacy behavior: surface shading and Continuity remain independently active and clearable,
and `view.status_items` faithfully lists both, using a separate stable `continuity` ID. Together
with Section there are at most three slots. Turning the preference back on keeps the last
successfully started or changed analysis at the oldest analysis-group position beside Section.
Queries never normalize state. Graphics Restore Defaults restores `true`; other page resets
leave it alone. It applies immediately to all open designs without document edits or undo.
Cancel restores the opening preference value, but does not restart an analysis already stopped
by switching to single mode. Run that analysis again to restore coexistence after cancelling.
Successful Continuity measurements with no seams create no item: legacy mode keeps existing
surface shading; single mode ends the previous analysis as for other successful replacements.

Headless SDK clients use `Session::set_single_surface_analysis(bool)` and
`Session::single_surface_analysis()`. Configuration survives scratch/new/open session lifecycle;
effects and chronology still follow their existing transient lifecycle. The policy is an
application preference/session setting, never saved design-format data. Policy changes update
the view revision only when visible effects or their declarations change.

A client need not ask again and again: after `control.subscribe` on its connection, the app
sends it a line whenever the active design changes (#6985):

```text
{"id":1,"method":"control.subscribe","token":"…"}
→ {"id":1,"ok":true,"result":{"document_id":"18f3a0c2d4e5b6a7-1","revision":3}}
{"event":"design.changed","params":{"document_id":"18f3a0c2d4e5b6a7-1","revision":4}}
{"event":"design.changed","params":{"document_id":"18f3a0c2d4e5b6a7-2","revision":5}}
```

- The reply is what `document.revision` answers then (and, like it, does not wait behind design
  changes); no event comes before it. From there on, each `design.changed` line carries what
  `document.revision` would answer, sent when its `document_id` or `revision` differs from the
  last one this connection was told: an edit, undo or redo, a design opened or started, a tab
  switched.
- Lines are coalesced, not sent per frame: at most about ten a second, and changes made faster
  (a parameter dragged) are followed by one line with the latest. Once the design stops
  changing, the last line names it as it is.
- An event has `event` and `params` and no `id`; every reply still has its request's `id`. Each
  event is a whole line of its own, between replies, never inside one. Ignore events you don't
  know.
- Only connections that subscribed get events, until they close (or shut their sending side). A
  second `control.subscribe` on a subscribed connection is refused with `already subscribed`;
  ask `document.revision` for the design now. A `control.subscribe` that timed out or failed did
  not subscribe the connection: send it again.

### Slow commands and the window (#1989)

With the preference **Preferences > AI > Keep the window responsive while agents run commands**
on (`ui.prefs {"agent_off_thread": true}`; off by default), a design change in the desktop app
that takes longer than about 150 ms (an `engine.execute` of a
modelling command, an `engine.batch` of them) computes off the window thread, so the window keeps
drawing and answering while it runs:
- The command is evaluated ahead on a copy of the design, which also makes the display meshes of
  the bodies it ends with. Then it runs for real on the window thread, reusing every feature the
  evaluation computed (kept for it, whatever the bounded evaluation cache dropped meanwhile), so
  the window waits only for the bookkeeping. Its reply, undo step, autosave and crash journal are
  the same as before. With the agent cursor shown, the evaluation runs while the pointer glides
  to the button, and the pointer waits there until it is done.
- Meanwhile `ui.*` requests, `document.inspect` and `engine.commands` are answered at once
  (inspection shows the design as it was), also while the agent cursor acts a command out. Later
  design changes, from any connection, wait their turn and run in the order they arrived.
- A person's edit made meanwhile goes first. The agent's command is then computed again for the
  design as it is and lands after that edit, never in the middle of it. If the design was replaced
  (another tab made active, a new file opened), the request fails with `… the design was
  replaced … nothing was applied`.
- A chip in the view names the command, with **Stop**. Stop answers
  `{"ok": false, "stopped": true, "error": "stopped: …; nothing was applied"}` and the requests
  waiting behind it go on. The computation finishes unseen in the background.
- `file.export` and `file.save_mesh` change nothing in the design. They run on a copy of the
  design as it was when they were asked for, and nothing waits behind them.
- Batches with `timings` or `profile` still run on the window thread, so the times they report
  measure the real work.

Timeouts and `control.pending` work as described above. A slow command is still answered when it
is done, from a later frame. With the preference off, commands run on the window thread, and the
window waits for each one.

Product images use `render.preflight/start/status/cancel/export` through `engine.execute`.
Keep the returned session-scoped job ID for subsequent calls; status does not block or
render. See [render jobs](render-jobs.md) for capture, cancellation and export semantics.

While an interactive native file picker is open, `ui.inspect` reports `file_picker_busy: true`.
Engine commands, `ui.file` and `ui.home` requests wait for its answer; window resizing,
inspection and screenshots remain available.

Every `sketch.*` command that changes a sketch adds `created: {points, curves, constraints,
dimensions}` to its result (and `wires` for 3D curves): the ids of what it added, in creation
order, next to whatever else the command returns (`curves`, `sides`, `corners`…). Sketch ids are
stable and never reused, even after a delete (`l1`, `l2`, then `l3` once `l2` is gone; only undo
rolls the numbering back with the design), so they can go straight into later commands:

```text
{"id":7,"method":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[40,0]]}}}
→ {"curves":["l1"],"created":{"points":["l1.start","l1.end"],"curves":["l1"],"constraints":["k1"],"dimensions":[]},…}
{"id":8,"method":"engine.execute","params":{"command":"sketch.dimension","params":{"entities":["l1"],"value":50}}}
```

A bare numeric `value` given to `sketch.dimension` (from the control channel, scripts or MCP) is
read in the design's units, exactly as if it were typed into the dimension box: in an inch design
`"value": 2` makes the line 2 in (50.8 mm) long. Point coordinates stay in millimetres. To pin a
unit regardless of the design, pass an expression with one: `"value": "50 mm"` or
`"value": "0.5 in"`. Angles are degrees unless the expression says otherwise.

Any expression parameter of any command can also define a user parameter as it sets the value:
`"distance": "depth = 15"` adds `depth` (15, read as the bare value would be) and stores `depth`
in the input, in the same undo step. The reply names it in `defined` and lists every definition
in `definitions: [{name, expression, unit}]`; a taken or reserved name fails the command and
changes nothing. `parameters.change` with `"expression": "height = 12"` renames the parameter it
changes instead (see the parameters guide).

The viewport also shows an X/Y/Z-aligned dimension chain between the endpoints of the
currently shown distance (centre, minimum or maximum), with each world-axis distance
labelled separately as an absolute value (including zero).
It uses the design's units and length precision, independently of the view or sketch plane.
Preferences → Graphics → **Show X/Y/Z distances** controls these extra lines and
labels (default on, saved between runs). Set `ui.prefs {"measure_axis_distances": false}`
to hide them while keeping the total distance and the dialog's ΔX/ΔY/ΔZ values. These
overlays remain visible until Measure closes.
Overlay handles `measure:label:x`, `measure:label:y` and `measure:label:z` identify the extra
labels; `prefs:measure_axis_distances` identifies the checkbox.
A `ui.drag` from `measure:label` or one of the axis labels moves that label (screen offset, kept
while the measured items stay the same; a double-click puts it back). It picks nothing and starts
no box selection.

## UI

| Method | Params | Notes |
|---|---|---|
| `ui.inspect` | — | UI state, pending file picker (`file_picker_busy`; compatibility alias `file_dialog_open`, no path), what is hidden (`visibility`: bodies, origin and construction items, sketches, joints (`hidden_joints`, joint ids); set it with the `browser.visibility` command), viewport rect, camera, active tool/dialog, the status line (`status`: text and whether it is an error), hover, renderer, timings, what the GPU drew last frame (`draw`: `draw_calls`, `triangles`, `line_segments`, `supersampled`; null without a GPU) and the graphics `adapter`, live preview (`active`, `busy`, `error`, `ms`, bodies it stands in for) |
| `ui.set` | any `UiState` field (`tab`, `perspective`, `showGrid`, `dark`, `layout`, …), and the browser's view of the design, saved with it: `showOrigin` (the Origin folder's eye), `showSketches` (the design's Sketches folder, `c0/sketches` in `hiddenFolders`), `hiddenBodies`, `hiddenOrigin`, `hiddenSketches`, `shownSketches`, `hiddenJoints`, `hiddenFolders`, `hiddenProfiles`, `shownDims`, `lockedBodies` (whole lists; `browser.visibility` changes single items) | `dark: false` switches to the light theme; `layout: "floating"` (default) or `"compact"` (`"docked"` is also accepted) selects the transient shell layout without saving a preference. Scenarios use `{"call":"ui.set","params":{"layout":"docked"}}` |
| `ui.view` | `view`: `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`, `home`, `fit`; `animate` (bool, default false: snap) | |
| `ui.camera` | `target?`: `[x, y, z]` (mm), `yaw?`, `pitch?`, `roll?` (radians), `distance?` (mm), `fov?`, `up?`; `fit?`: `true`, `false` or `[occurrence id or name…]` (`root`: the bodies outside every component) | the viewport's orbit camera: it looks at `target` from `distance` away; `yaw` 0 looks along +Y (from the front), `pitch` is the elevation (±π/2: straight down or up), `roll` turns the image. Fields given are set first (the others keep their values), then `fit` frames the listed occurrences (or everything), keeping the direction. The reply, passed back, sets the same view again, so a framing can be stored and replayed. `fov` is the angle the view is drawn with: 0 (parallel) with Perspective view off or while a sketch is open, else `perspective_fov`; in Render, the Render camera's own. `fov` and `up` follow the perspective setting (`ui.set`), its angle (`ui.prefs {perspective_fov}`) and the up axis preference (`ui.prefs`) and can't change here; a refused field changes nothing |
| `ui.start` | `command`, `params?` | like a toolbar click: starts the sketch tool or dialog for the command. `params` sets what the dialog opens on, and nothing runs until OK: `joint.drive` takes `joint` (id or name; without it, the joint picked in the browser, else the first joint that moves; a rigid joint opens to say why it has nothing to drive, with an Edit Joint button, handle `drive:edit_joint`) and `values` (numbers in degrees or millimetres, or expressions); `motion.study` takes `study` (id or name), which opens that study to edit and chart. A command that takes no params is refused before it starts; a param the dialog doesn't take is refused and the dialog closes |
| `ui.click` | `x`, `y` (screen points), `button?`: left/right/middle, `shift?`, `ctrl?`, `double?` | real pointer input |
| `ui.move` | `x`, `y`, `shift?`, `alt?`, `ctrl?`, `cmd?` | pointer movement without a button; modifiers apply until the next synthetic move/click; a move with omitted modifiers releases them. With `nav: "touchpad"`, Shift pans and Alt/Option orbits. |
| `ui.drag` | `x0`, `y0`, `x1`, `y1`, `via?`, `button?`, `shift?`, `ctrl?`, `steps?`, `hold?`, `hold_end?` | press, move, release; `via` is a list of `[x, y]` points passed through on the way (`steps` moves to each); a left drag on the model is a box selection (left to right: window, right to left: crossing), or with `lassoSelect` on (`ui.set`, the SELECT panel's Lasso) a freehand lasso that takes what is wholly inside it; with OpenSCAD navigation, plain left-drag on empty space orbits, Shift+left-drag box-selects, and drags starting on sketch geometry edit it; `hold` keeps the button down that many frames first (a right press-and-hold opens the marking menu, releasing over a direction picks it), `hold_end` that many frames at the end (a sketch drag shows its snap) |
| `ui.menu` | `x?`, `y?`, `target?`, `close?` | with a point: open a context menu there as a right-click would (the viewport's marking menu, or a browser menu with `target: {"type": "body", "name"} \| {"type": "sketch", "id"} \| {"type": "component", "id"}`); without: the open menu (`radial` and `items`: id, label, shortcut, enabled, icon, placeholder; blank icons on headings/separators have `placeholder: false`) |
| `ui.icon_audit` | `reset?`, `stop?` | `reset: true` enables collection on the UI thread and clears previous requests; returns `{requested: [{name, master, placeholder}]}` sorted by requested id. Each read drains names collected since the previous read/reset. Collection stays enabled until `stop: true` (which returns pending names before disabling it). Includes aliases, variable/table ids and cached paints. |
| `ui.menuPick` | `item`: id or label | runs an item of the open menu, as clicking it does |
| `ui.selectOther` | `x?`, `y?`, `pick?`, `add?`, `hover?`, `close?` | Select Other: with a point, list compatible hidden and visible geometry near the pointer by default (as holding the left button does; disable `select_other_hidden` for the original visible-only list); the popup fits the available window space and scrolls only for overflow; `pick`: select row `pick` (`add`: as Shift-click; with a dialog open, into its active input); `hover`: light a row up; without: the open list (`open`, `at`, `total`: everything under the cursor, `rows`: label, icon, sel, at most 20 per kind, nearest first) |
| `ui.rename` | `text?`, `commit?` (default true) | finishes the rename box a Rename item opened |
| `ui.selection` | | the selection, the open dialog's inputs, the hovered item, and the active sketch's drawn dimensions (`dimensions`: id and text centre, `dimension_selected`, `dimension_editing`), and finished sketches' dimensions shown in the model view (`shown_dimensions`: sketch, id and text centre; a double-click there edits the value) |
| `ui.worldToScreen` | `points`: `[[x, y, z]…]` world points | their screen points |
| `ui.triad` | | screen points of the 3D sketch move triad's handles (`x`, `y`, `z`, `plane`) |
| `ui.sketchToScreen` | `points`: `[[x, y]…]` in active-sketch coordinates | their screen points (null when behind the camera) |
| `ui.dialogInput` | `index` | what clicking an input's Select chip does: picks go to that input |
| `ui.at` | `world: [x,y,z]` \| `sketch: [x,y]` \| `plane: "XY"` \| `axis: "Z"` \| `dimension: "d1"` (its value text) \| `handle: "arrow"` (a manipulator handle, see `scenario::publish_handle`) | the screen point (a scenario step's target; see `crates/ui-egui/src/scenario.rs`) |
| `ui.pick` | `pixels?: [[x,y]…]`, `rays?: [{origin, dir}…]`, `frame?: "window"\|"model"`, `width?`, `height?`, `edges?` (default true), `edge_px?` (default 6), `all?`, `include_ghosted?` | read-only geometry under each pixel or ray (#6304): one entry per input, `null` where nothing is hit — body, face index, exact world `point` and outward `normal`, `depth`, `occurrence`/`component` for placed parts, `edge`/`vertex` within `edge_px` of a pixel, `section_cap: true` on a cut face. `window` pixels are physical pixels of a window screenshot; `model` pixels are pixels of a `ui.render` of `width`×`height` (default 960×600), so it works headless. As `view.pick` through `engine.execute`/`engine.batch`. See `crates/ui-egui/src/view_pick.rs` |
| `ui.handles` | `prefix?` | the handles drawn last frame, `[{name, x, y}]`: toolbar buttons and drop-down items `toolbar:<command id>` (the button alone: `button:<command id>`; a drop-down row: `dropdown:<command id>`, highlighted `dropdown_hot:<command id>`, its pin `dropdown_pin:<command id>`; Customize Toolbar's check boxes `customize:<command id>`), ribbon tabs `tab:<name>` (MODEL, SURFACE, ASSEMBLE, INSERT, INSPECT, MANAGE, VIEW, SHEET METAL, WELDMENTS; contextual SKETCH; workspace RENDER), the toolbar menu button `ribbon:add_tab`, the hint when every tab is hidden `ribbon:tabs_hidden`, Preferences tab check boxes `prefs:toolbar_tab:<NAME>`, panel labels `panel:<tab>:<panel>` (also `ribbon:section:<TAB>:<SECTION>`), document overflow list `tab:overflow`, workspace column `ribbon:workspace`, command-search launcher `ribbon:search`, browser rows `row:<label>`, where each row's depth puts it `indent:<label>`, and fold arrows `fold:<label>`, manipulator handles; signed-axis tips `axis_view:z`, `axis_view:-y`; optional cube facets `navigation_cube:top`, `navigation_cube:top_front_right`, controls `navigation_cube:control:home` (and other actions), plus signed-axis aliases for visible cube faces; Preferences navigator picker `prefs:navigator`, options `prefs:navigator:Signed axes` and `prefs:navigator:Cube`; in the Motion Study dialog, where the chart puts its first and last step `motion:chart_start` and `motion:chart_end`, the Check collisions button `motion:check` and each collision found `motion:collision:<n>` |
| `ui.editFeature` | `feature` (id or name) | what double-clicking a timeline item does: rolls back to it and opens its dialog filled in (sketches open in sketch mode) |
| `ui.scroll` | `x`, `y`, `delta?`, `ctrl?`, `unit?`: `point` \| `line` | wheel zoom at the cursor. `ctrl: true` replays a Ctrl+wheel pinch (Ctrl held and the wheel in points); `unit` selects the wheel unit and defaults to points with `ctrl` and lines without it. `delta` is in points either way. Invalid units or coordinates outside the finite screen-coordinate range are rejected before queuing input |
| `ui.key` | `key` (egui key name, `Enter`, `Escape`…), `cmd?`, `ctrl?`, `shift?`, `alt?`, `pressed?` | By default, press and release, then release modifiers. With `pressed: true` or `false`, send only that edge; modifiers remain as given. `cmd` retains the legacy primary-modifier alias unless `ctrl` is explicit: `cmd:true, ctrl:false` sends physical Command, and both true sends Control+Command. `ui.move` can change modifiers between edges. |
| `ui.text` | `text` | typed text |
| `ui.screenshot` | `path?` | PNG of the window (needs a presented frame: a minimised or fully covered window answers at once with `hidden: true` and an error; use `ui.render`) |
| `ui.render` | `path`, `width?`, `height?`, `grid?`, `axes?` | Render of the model with the current camera (no window needed), drawn on the app's GPU device with the viewport's pipelines (the reply's `renderer`: `gpu`, or `gpu-software` on a software adapter), in the window's theme (light or dark; `solidcraft-cli snapshot` keeps the light colours); cut by the section the viewport shows (`inspect.section`, an open Section Analysis dialog's preview, a sketch's Slice), with the cut face capped. The ground grid and the origin's axes are drawn as the window shows them (`ui.set` `showGrid` and `showOrigin`, `hiddenOrigin`); `grid` and `axes` (booleans) say otherwise for this render |
| `ui.confirm` | `accept?`: bool | answers the Delete confirmation (shown when a delete takes other features with it or makes some fail); without `accept`, what it lists |
| `ui.messages` | `clear?`: `true` (empty the list), `open?`: bool (show or hide the Messages window) | every status message of this run, oldest first (the last 200; a message repeated in a row is one entry with a `count`): `messages` `[{text, error, at, count}]` (`at` in ms since the Unix epoch), and whether the window is `open` (#2763) |
| `ui.documents` | `action?`: `new`, `switch`, `close`, `quit`; `index?`; `force?` (close without asking); `answer?`: `save`, `dont_save`, `cancel` (answers "save changes?"); `open_elsewhere?`: `read_only`, `anyway`, `cancel` (answers "open elsewhere" for the first Open waiting); `changed_on_disk?`: `save_copy`, `overwrite`, `cancel` (answers "changed on disk" before a Save) | the open designs (tabs): names, unsaved flags, the active one, a pending "save changes?" prompt, whether a quit is asking, whether the active design is `read_only`, the Opens waiting for "open elsewhere" (`open_elsewhere`) and a waiting "changed on disk" (`changed_on_disk`) |
| `ui.file` | `path`; `insert?`: bool | starts an interactive background Open or Insert; poll `ui.inspect.file_busy` for completion; the window stays responsive |
| `ui.home` | `open?`: bool; `sample?`: index | the start page: show or hide it, open a built-in sample (once built); returns `open` and the recent designs |
| `ui.shortcut` | `command`, `key?`, `slot?`, `replace?` | a command's keys (`key`: the first, shown in menus; `keys`: all), or rebinds it: `key` becomes its only key, or with `slot` replaces its key there (one past the last adds a key, `""` there removes that key); `""` without `slot` clears them; key names are case-insensitive and modifiers are normalized (`shift+ctrl+t` becomes `Ctrl+Shift+T`); unknown and reserved keys are refused; a key another command uses is refused unless `replace` |
| `ui.help` | `item?`: `about`, `updates` (checks for updates and shows the result), `shortcuts` (read only), `report` (opens a report draft for review), `crashes` (the Crash Reports window) | the version, commit, build date and diagnostic text |
| `ui.prefs` | any of the [`ui.prefs` settings](#uiprefs-settings) below; also `open`, `button` and `stand_up` (listed there) | sets preferences (kept between runs), returns them all plus `dialog` (the open page or null), `apply_enabled` (whether the dialog's Apply has a change to keep), `dark`, `viewport_dark` (whether the 3D view is dark), `stand_up_offer` (the offered bodies, or null), `stand_up_note` (whether that offer is the note), `theme_followed`, `theme_follow_error`, `theme_option_defs`, `theme_option_values` and `theme_option_notices` |
| `ui.capture` | `action?`: `capture`, `revert`, `cancel` | answers the Capture Position question (asked when a command starts or the design is saved while components were moved without capturing); without `action`, what waits |
| `ui.window` | `action`: minimize, maximize, restore, toggle, close | what the title bar's caption buttons do (on Windows and Linux the application bar is the title bar); close asks about unsaved designs first |
| `ui.agent_cursor` | `show` (bool), `speed`: a number from 1 to 5 (times the normal speed) or `"instant"` (no animation: the command runs at once and the pointer jumps to where it acted), `follow_camera` (bool); all optional | the settings. While the cursor is shown, `engine.execute` first glides to the command's toolbar button and to each pick, then runs and replies; later requests wait their turn. The pointer shows the command's localized name, including commands inside a closed toolbar group; an animated batch names each command it visits. It also follows the pointer input sent with `ui.move`, `ui.click`, `ui.drag` and `ui.scroll`, as each event reaches the app, and a press leaves a ripple. `"animate": false` on a call skips it |
| `engine.execute` → `view.layout` | `mode`: `floating` or `compact` | Apply and save the workspace layout without opening a dialog, changing the design, resetting the camera or cancelling its active tool. General preferences expose the same choice; `ui.prefs {"layout":"compact"}` also saves it. |
| `engine.execute` → `browser.expand_all` | `path?`: occurrence ids; `component?`: id; omit both for the whole tree | Open every Browser row under that scope, including folders that start closed (Origin, Named Views, Features). Display only: not saved in the design, not an undo step. |
| `engine.execute` → `browser.collapse_all` | same | Close those rows. The design row stays open when the whole tree is collapsed. |
| `engine.execute` → `view.history_layout` | `width?`, `dock?`: `bottom`, `left`, `under_browser`, `collapsed?` | Resize/dock the history without opening a dialog or changing the design. Contents respond to measured width; browser-stacked history shares its width and splits the height at the remembered share (`browser_share`, half until the seam between the two cards is dragged). One folded header gives the expanded card the rest; two folded headers stay at the top. |
| `ui.agent_chat` | `features?`, `toolbar_button?`, `connect_at_start?` and `open_at_start?` (connect the agent started last, and open the panel, when SolidCraft starts; both off by default, never with `--control`), `permissions?` (`ask`, `edits`, `all`), `add_agent?` ({`name`, `kind`: `acp`, `api` or `anthropic` (`api` is OpenAI-compatible, `anthropic` Anthropic-compatible), `command` or `base_url` + `model` + `request_timeout_s?` (seconds one model request may take, 300 by default, rounded and clamped to 30 to 3600)}; no keys), `add_dialog?` (`installed`, `custom`, `api`, `anthropic`, or `false` to close), `edit_dialog?` (the id of an agent you added: the same dialog filled with what it holds, as its **Edit** button opens it; the key is never shown), `default_agent?`, `agent?` (the conversation's agent, by id), `mode?` (the agent's own mode), `provider?` (which provider's group the model picker opens first, when the models come from several), `model?` (one of its models, or see below), `open?`, `side?` (`left`, `right`), `minimized?`, `popped?` (in its own window), `width?`, `picker?`, `refresh?`, `connect?`, `stop?`, `attach?`: a file's path (a picture, a text file or a CAD file, for the next message; CAD files are imported at once), `remove_file?` (index of a file on the next message), `send?`: text (files alone make a message too), `attach_view?`, `attach_selection?`, `pick?` (true: open the context picker), `permission?`: an option id or `"cancel"` (the newest question of the agent's own), `approve?`: true, false or `"always"` (the oldest request held for Allow/Deny), `cancel?` (Stop: the queue stays), `send_now?` (stop the running step, then send the queued messages), `remove_pending?` (index), `new_chat?` (clear the chat, new session), `undo?` (the last message's changes) | the chat's state: `features`, `agents` (found and added), `currentAgent`, `state` (off, starting, ready, busy, auth, exited, failed: the agent never got ready, and Connect reads Retry), `connectAtStart`, `openAtStart`, `lastAgent` (the agent started last), `popped`, `window` (its place and size), `agent`, `modes`, `mode`, `models`, `model`, `modelWarning` (the model capability warning text, or `null` when unknown or tool support is reported; checked asynchronously and never blocks sending), `turnWarning` (the latest completed turn's no-tool notice, otherwise `null`; cleared on the next turn or new chat), `providers` (`name` and `count` of each, empty with one provider), `provider` (the one listed), `authMethods`, `toolsPort`, `pid`, `waiting`, `turn`, `pending` (queued messages, with their `files`), `attachments` (the files on the next message, by name), `loading` (pictures still being read for it, by name), `stopped`, `detection` and the conversation (`items`). With `features` off only the settings apply. Desktop only; see [Agent chat](agent-chat.md) |
| `ui.resize` | `width`, `height` | |
| `app.quit` | `force?` (quit without asking about unsaved designs) | `quitting` (the window closes), `asking` (a "save changes?" prompt is open) |

For an ACP agent advertising configuration options, `ui.agent_chat` also accepts
`{"config_option": {"id": "effort", "value": "high"}}`. Use an option ID and choice value
from its reported `configOptions`; IDs and available values belong to the agent. The reply's
`configOptions` is `null` for a legacy agent, or the current list of supported select options
(including an empty list when none remain). `configPending` is true while a selection waits for
the agent's reply. Read the confirmed state after that reply; submitting a change is not its
confirmation. The existing `model` and `mode` inputs use the corresponding configuration option
when advertised, and retain their existing behavior for agents using legacy lists or API endpoints.

`model` may also name a model the agent did not list, as the picker's custom row does: it goes to an
API endpoint, to an agent with a model option, and to a legacy agent that listed models; the reply's
`customModels` says whether this agent takes one. Otherwise it is ignored. A model an agent refuses
leaves its previous model and adds a notice. `modelsNote` says why an endpoint's models could not be
listed, or is `null`.

### `ui.prefs` settings

`render_previous_geometry_budget` (Graphics, default false) restores the previous fixed Render preview geometry budget: 512 MiB on 64-bit desktop, 128 MiB elsewhere. With it off, desktop uses the device-based budget.


One setting per line, sorted by name. A change that adds a setting adds its own line in order, so two such changes only meet when their settings are neighbours.

- `angle_decimals`
- `assembly_move_handles` (default true): explicit Assembly Move uses the permitted joint-motion handles; false keeps the previous Move panel.
- `auto_operation` (default true: feature windows choose Join, New Body or Cut from what their material touches until one is picked; false: each keeps the operation it opens with, Extrude its own Join or Cut)
- `browser_bodies_folders` (`several` (default): a component with one body and no groups lists it straight under the component in the browser, without a Bodies folder; `always`; `never`: components list their bodies and groups straight under them, and the folder's menu items are on the component's menu; the design's own Bodies folder, and one hidden with its eye, always show; #4217)
- `browser_eye_left` (default false: each browser and History tree row's eye, handle `eye:<name>`, sits in a column at the rows' right end, clear of the fold arrow and the radio; true: between them, as before #8718)
- `check_updates` (default true; [updates](book/src/guide/updates.md))
- `color_vision` (`off` (default), `protan`, `deutan`, `tritan`: colour vision modes, #778)
- `default_units`
- `dialog_buttons` (`bottom` (default), `top`: a feature dialog's OK, Cancel and Apply sit in its title row instead of under its inputs, #6708; the buttons keep their handles, and `dialog:close` is then Cancel's; the Sketch panel's Finish Sketch row moves above its options, #9860)
- `dialog_place` (`edge` (default) or `left_column`: command dialogs open in the left column in place of the browser and the History, #6707)
- `extrude_auto_to_object` (default false: in the Extrude dialog, with a profile picked, a face, vertex or parallel plane picked off its plane switches the extent to To Object; it applies to Extrude dialogs opened afterwards)
- `frame_rate_details` (default false: with the frame rate, a frame-time graph of the frames drawn with the triangles over it, a frame-time histogram and what the GPU drew; clicking the readout switches it)
- `graphics_adapter` (exact card name, or empty for Automatic; applies after restart; `WGPU_ADAPTER_NAME` takes precedence)
- `hidden_toolbar_tabs` (default `[]`: all existing tabs, including Sheet Metal and Weldments, stay visible; lists tabs to hide, each once, e.g. `["SURFACE", "SHEET METAL"]`; hidden tabs stay hidden regardless of the design's features; [ribbon layout](ribbon-layout.md))
- `history_sketches` (`in_order` (default), `under_feature`: the History at the left lists a sketch under the first feature built from it, #6489; the timeline at the bottom keeps the creation order)
- `hole_moves` (default false: a placed Single hole has a drag handle (`hole:<n>`), and a pick on a face that has a hole moves it there)
- `hole_snaps` (default false: a Single hole snaps to its face's middle and round edges' centres)
- `home_fit_size` (0.1…1, default 1: how much of the view the model fills when Home, Fit, an opened design or a navigator view change frames it, with `navigator_fit` on or off; 1 fills the room between the panels, the compass and the navigation bar, as before; #10091)
- `joint_rotation_coarse` (default 10°): coarse Assembly Move rotation step, in 0.001…180°.
- `joint_rotation_fine` (default 0.1°): fine Assembly Move rotation step, in 0.001…180°.
- `keep_work_on_tab_switch` (default false: each design tab keeps its open dialog or tool, with what was picked and typed in it, and its toolbar tab while another design is shown, and has them back when it is shown again; off, a switch cancels them and shows MODEL, #9360)
- `language` (`en`, `ar`, `ja`, `de`, `fr`, `he`, `ko`, `ms`, `ru`, `uk`, `zh`; see [localization](localization.md))
- `length_decimals`
- `line_drag_draws_one` (default false: in the Line tool, a press, a drag and the release draw one line from the press to the release and end there, snapping as clicks do; clicks still chain, and a drag under 24 points is a click; off, a drag places a point where it is let go, #7247)
- `line_hover_tangent_arc` (default false: in the Line tool, after a segment, moving away from the chain's last point and resting back on it for about a third of a second makes the next click a tangent arc, and the same again goes back to lines; off, resting there does nothing, #9489)
- `logo_accent` (bool, default false: the application bar's logo in shades of the theme's accent instead of the theme's logo colours)
- `look_at_anim`
- `look_at_speed`
- `max_design_file_mb` (default 256): the largest design file opened, in MB, from the default up to 16384 (16 GB) (Preferences › General › Largest file to open, #4086; a `--max-file-size` launch flag overrides both for that run; the web app keeps the defaults and refuses other values; [large files](book/src/guide/files.md#large-files))
- `max_step_file_mb` (default 512): the largest STEP or import file opened, in MB, from the default up to 16384 (16 GB) (Preferences › General › Largest file to open, #4086; a `--max-file-size` launch flag overrides both for that run; the web app keeps the defaults and refuses other values; [large files](book/src/guide/files.md#large-files))
- `middle_double_click` (`home` (default), `fit`, `off`)
- `msaa`
- `nav` (`fusion`, `solidworks`, `creo`, `inventor`, `alias`, `blender`, `tinkercad`, `openscad`, `touchpad`)
- `nav_bridge_enabled` (default false) and `nav_bridge_port` (default 8765: a device's own app on this computer moves the view, [the navigation bridge](3d-mouse.md#device-apps-the-navigation-bridge))
- `navigator_fit` (default false: a view chosen on the view cube or the axis gizmo also looks at the middle of the model from the one distance that fits its bounding sphere in the free part of the view, at `home_fit_size`; Home and an opened design use the same zoom and centre while it is on, so views chosen after them only turn; #10091)
- `offer_stand_up`
- `orbit_around` (`view_centre` (default: what Pan moves and Fit frames), `origin` (the world origin stays still on screen), `cursor` (the model point under the pointer when the drag began); the older `orbit_cursor` boolean still sets `cursor` when true)
- `orbit_reverse` (default false: Shift with two-finger scrolling orbits the other way)
- `origin_plane_names` (`xyz` (default) or `top_front_right`: the browser, dialogs and lists call the origin planes Top, Front and Right; their ids, commands and saved designs keep XY, XZ and YZ)
- `pan_reverse_x` (default false: horizontal scrolling pans the other way)
- `pan_reverse_y` (default false: vertical scrolling pans the other way)
- `parameters_apply_live` (default true: the Parameters dialog applies an expression when its field is left; false: edits wait for Apply or OK, which set them together in one rebuild and one undo step)
- `perspective_fov` (Perspective view's vertical field of view in radians, 0.2618…1.5708 (15°…90°), default 0.6; when it changes, the camera's distance scales so the view keeps its size)
- `placement_handles` (default false: handles on the preview of a Rest, Boss or Snap Fit that move it across the model and turn it)
- `print_face_highlight` (0…1, default 0.2: how strongly a print face is tinted and outlined in the view, #8429; 0 draws no highlight)
- `quick_measure` (default false: the selection's length, area or distance in the view's bottom right corner, the rect `measure:quick`; #7000)
- `ribbon_reveal` (default false: hovering a result in Search commands switches the ribbon to the command's tab and marks its button, or opens the panel drop-down that holds it on its row, handle `ribbon:reveal`; leaving the results goes back to the tab before; a command started from the search blinks there, and a running command from a drop-down marks its panel label, `ribbon:running_section:<TAB>:<SECTION>`, #7287)
- `right_drag_orbits` (default true; false: a right drag the mouse preset would orbit leaves the camera alone, #6492)
- `select_other_hidden` (default true: Select Other includes hidden geometry near the pointer; false restores the original visible-only list under the pointer)
- `show_frame_rate` (default false: frames per second in the view's bottom-right corner, counting the frames the view draws anyway; it reads 0 FPS once the view is idle)
- `sketch_auto_dimensions` (default false: each shape drawn in a sketch gets all its dimensions at the sizes drawn, as `sketch.draw.dimensioned` with `as_drawn`; typed values win)
- `sketch_auto_locate` (default false; with `sketch_auto_dimensions`, new shapes are also placed from the sketch origin until fully constrained, as `sketch.draw.dimensioned` with `locate`)
- `sketch_conflict_colour` (default true: while the open sketch's constraints conflict, the curves and points a failing constraint holds are drawn in the theme's `sketch_conflict` colour too, not only its glyph or dimension; false: only the glyphs and dimensions are red, in `error`, as before, #7244)
- `sketch_construction_lines` (default true: show the Construction Line tool family in Sketch CREATE; false restores the earlier menu)
- `sketch_exit_view` (`keep` (default: the view stays where it is when a sketch ends), `previous` (back to the view from before the sketch), `unless_rotated` (back unless the sketch view was turned))
- `sketch_open_look_at` (default true: creating or editing a sketch turns the view to face it; false leaves the view as it is, and Look At faces the sketch when asked)
- `sketch_recent_snap` (default false: while drawing, the cursor snaps where the horizontal guide of one recent circle or arc centre or straight-edge midpoint meets the vertical guide of another; a free coordinate, with no constraint and no construction geometry. Off: those guides stay visual and the cursor does not move. The references come from `sketch_tracking_guides`, #8694)
- `sketch_state_icons` (default true: a sketch's icon in History and the browser takes the colour of its solve state, `sketch` under constrained, `sketch_fixed` fully constrained, `error` while its constraints conflict, and its tooltip names the state; false: every sketch row's icon and tooltip as before, #7245)
- `sketch_tab_always` (default false: the SKETCH tab stays in the ribbon right after MODEL while no sketch is open; a tool in its CREATE or REFERENCE section then opens Create Sketch, on the selected plane or planar face or else on the plane picked next, and starts in the new sketch, #7904)
- `smooth_wheel_zoom` (default true)
- `space_mouse_model_pivot` (default false: Classic orbit around the view's centre; true opts into Traced orbit around the model for the 3D mouse and navigation bridge)
- `space_mouse_sketch_orbit` (default false: the 3D mouse and navigation bridge pan and zoom a sketch seen square on and turn it only after the view is tilted; true lets them turn it too, as the mouse can)
- `stand_up_unknown` (`note` (default: a file whose header names no program known to save Y up and whose bodies seem to lie on their back (their flat area across Y outweighs their flat area facing down Z) gets a small note over the view, "Did this part open the wrong way up?", with Stand it up and Don't ask again as the handles `stand_up:stand` and `stand_up:never`, and its close (×) as `toast:stand_up:close` (also `stand_up:dismiss`); it goes by itself after 10 seconds and never blocks input), `ask` (the "Stand this part up?" dialog for every file from another program, SolidWorks too, without looking at how it rests, as before #7717), `off` (no offer); otherwise a SolidWorks file gets the dialog, or the note when its bodies clearly stand on Z already)
- `status_bar` (default false: a thin bar along the window's bottom edge, below History, handle `status:bar`: what is being edited (`status:editing`), the open sketch's state in its colour (`status:sketch`) and the design's units (`status:units`, a click opens the Units menu, whose rows run `document.units`); the sketch's state then leaves the view's top-left corner, #7243)
- `step_values` (default false: Up/Down and the wheel step a plain number in a focused value field)
- `theme` (`dark`, `light`, `system` or a theme file's id: a built-in one such as `gruvbox-dark`, or an imported one; the reply's `theme_ids` lists them)
- `theme_export: true` (the reply's `theme_file` is the active theme, including a followed file; themes with options retain their authored definitions, defaults and overlays, while older themes keep the complete snapshot shape; saved option values are excluded; see [colour themes](book/src/guide/ui-tour.md#colour-themes))
- `theme_follow` (desktop only, default `""`: a `solidcraft-theme/1` file's path to follow as the theme, reloaded within about a second whenever it changes and when the window gains the focus; the file is read at once, and a file that doesn't read keeps the last good theme; `""` stops following, as does setting `theme`; in the browser it is an error; the launch flag `--theme-file PATH` follows a file for one run without saving it; the reply's `theme_followed` is the path followed now or null, and `theme_follow_error` why its last read was not applied, or null)
- `theme_import` (a `solidcraft-theme/1` file's JSON or text: adds the theme and turns it on; the reply's `theme_imported` is its id)
- `theme_options` (default `{}`): a map of theme id to option-id/value map, merged per theme; `{"theme_options":{"my-theme":{"enabled":true,"amount":5}}}` changes those options, `null` resets one option, and an empty map resets that theme. Unknown themes or option ids, wrong types, unknown choices and out-of-range numbers refuse the entire call and name the key, before imports, removals, dialogs or other preference effects occur.
- `theme_remove` (an imported theme's id; also removes its saved option values)
- `timeline_origin_bodies` (default false: the History on the left or under the browser starts with the active component's Bodies folder and the Origin, the browser's rows with their eyes, folds and menus; the bottom History is unchanged)
- `toast_place` (`corner` (default: notices and offers stack in one column in the view's bottom-right corner, each 360 points wide, buttons right-aligned with the main action right-most, #7893), `centre` (at the bottom centre above the view toolbar, each as wide as its text, as before))
- `trackpad_pan` (default true: two-finger scroll pans, except Touchpad always zooms; pinch zooms)
- `trackpad_zoom_modifier` (`off` default, `cmd`, `alt`, or `ctrl`: hold this key alone for vertical scroll to zoom)
- `ui_font` (an installed font family or a font file for the interface; empty for the built-in font)
- `ui_scale`
- `up_axis` (`z`, `y`)
- `view_confirm_corner` (default false: while a dialog or a sketch is open, a check and a cross at the view's top right, handles `view:confirm:ok` and `view:confirm:cancel`, do what the dialog's OK and Cancel do, or Finish Sketch and Cancel Sketch; the axis view or cube moves down below them, #6709)
- `viewport_theme` (`theme` (default: the theme's own), `light`, `dark`: the 3D view in the Light or Dark theme's colours whatever the window uses, #4146)
- `wheel_zoom_speed` (0.25 to 4, default 1: how far each wheel step zooms; the 3D mouse keeps its own speed)
- `window_system` (Linux: `auto` (default: XWayland whenever there is an X display, native Wayland on niri and Hyprland), `wayland`, `x11`; saved in `graphics.json`, applies at the next start; `SOLIDCRAFT_WAYLAND` overrides it)
- `zoom_reverse`
- `zoom_stop_at_model` (default false: the 3D mouse and navigation bridge stop zoom at the surface in the middle of the view; drag-zoom holds its press point; independent of `zoom_to_cursor`)
- `zoom_to_cursor`

The built-in `gruvbox-dark` and `gruvbox-light` themes each expose the `contrast`
choice (`soft`, `standard` default, `hard`). For example:
`{"theme":"gruvbox-dark","theme_options":{"gruvbox-dark":{"contrast":"soft"}}}`.
The former `gruvbox-dark-soft`, `gruvbox-dark-hard`, `gruvbox-light-soft` and
`gruvbox-light-hard` ids remain accepted for compatibility and select the matching
family and contrast; replies and `theme_ids` expose only the two canonical families.
An explicit canonical option change or reset in the same call wins over the legacy
selection. Legacy option-map keys are also accepted; canonical keys take precedence
when both occur in a call. Imported themes cannot claim the retired built-in ids.

Theme option values persist per theme, including a followed file's own id, through the same
preference store in the browser and on desktop. Switching themes restores their saved values;
Interface's Restore Defaults clears them. Preference exports and settings profiles carry values;
`theme_export` carries the author's defaults. Saved entries that no longer fit a definition remain
stored and use the default with a notice; unknown ids have a notice and no effective value.

For the active theme, `theme_option_defs` lists the declared options in `ordered_options` order:
`id`, `type`, `default`, type-specific `min`, `max`, `step`, `unit` or choice `values` (each with
`id` and `label`), `parent`, `advanced`, `section`, `label`, `help`, `standard` and `available`.
Labels, help and choice labels use the current language with English fallback. Unavailable
definitions include `reason`; standard options currently have `available: false` and
`reason: "not implemented"`. `theme_option_values` contains the effective values, including
defaults and values of inactive children. `theme_option_notices` contains `{id, kind}` entries,
where `kind` is `unknown_option`, `wrong_type`, `unknown_choice`, `non_finite` or `out_of_range`.
Without an active theme file these fields are `[]`, `{}` and `[]`. Portable input
and output schema types live in `solidcraft_engine::schema::ui_prefs` (`PrefsParams`, `PrefsReply`).

Other `ui.prefs` parameters:

- `open` opens Preferences at General; Look: Appearance, Layout, Graphics; Work: Design, Sketch, Tools; Input: Navigation, 3D mouse, Shortcuts; or Extend: Assistant, Plugins. Names are case-insensitive. Legacy aliases remain accepted: Interface → Appearance, Interface/navigation bar → Layout, Keyboard Shortcuts → Shortcuts, AI → Assistant, and Selection, Sketch & selection or Sketch and selection → Sketch. Replies use the canonical page name. The corresponding legacy `prefs:page:<name>` scenario handles also remain aliases of the canonical page rows.
- `button`: `ok`, `cancel`, `apply`, `defaults`
- `stand_up: true` stands up a just-opened Y-up file, `stand_up: false` leaves it as it is ("Leave it": the offer, dialog or note, closes and `offer_stand_up` stays on)
- the dialog's buttons are the handles `stand_up:stand`, `stand_up:leave` and `stand_up:never`

`ui.workspace {workspace?: "design" | "render"}` reads or changes the active workspace.
Entering Render requires finishing the current modelling operation; returning restores
the Design camera. See [the integrated Render workspace](render-workspace.md).

### Sketch alignment guides

`ui.prefs {sketch_tracking_guides: true}` enables temporary horizontal and vertical dotted
alignment guides from hovered circle/arc centres and straight-edge midpoints (default true).
The guide spans the reference and cursor projection, including positions inside a circle.
The two most recent references are kept, and both guides can be drawn. A single guide does
not move the cursor. Set the preference to `false` to hide the guides. Snap suppression
(Alt, unless rebound) does the same for that gesture. Snapping the cursor to where those
guides cross is separate, and off unless `sketch_recent_snap` is true. While a drawing tool
waits for a point, `sketch_tracking_points` (default true) makes every shown sketch point
other than the origin a reference too (line and arc ends, rectangle corners, free points);
`false` keeps centres and midpoints only.

### Continuous camera motion

Continuous camera motion is also available through `engine.execute` with command
`view.navigate`: pass `camera` (from `ui.inspect`) and `motion: "pan" | "orbit" | "zoom"`.
Pan and orbit take `delta: [dx,dy]` in screen points (each within ±10000); pan also requires
`viewport: [width,height]` (each 1..1000000). Orbit accepts a world-space `pivot` with the
viewport size. Zoom takes `factor` (0.05..20, below 1 zooms in) and an optional world-space
`anchor`. The returned `camera` is applied immediately by the UI, without animation,
document edits, undo steps or a success-log entry per movement.

## MCP

`solidcraft-cli mcp --connect 127.0.0.1:PORT` bridges an MCP client (an AI agent) to this
channel; see [mcp.md](mcp.md).

## Example

```text
{"id":1,"method":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}}
{"id":2,"method":"engine.execute","params":{"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[40,30]}}}
{"id":3,"method":"engine.execute","params":{"command":"solid.extrude","params":{"distance":20}}}
{"id":4,"method":"engine.execute","params":{"command":"solid.fillet","params":{"edges":[[0,0,10]],"radius":3}}}
{"id":5,"method":"ui.screenshot","params":{"path":"/tmp/part.png"}}
```

Preferences → Navigation → View navigator offers **Signed axes** (the original default) and
**Cube**. This choice is saved independently of the mouse preset. The control channel selects it
with `ui.prefs {"navigator_style":"axes"}` or `ui.prefs {"navigator_style":"cube"}`.
Existing settings without this field keep signed axes. Signed-axis tips choose principal views;
clicking an aligned tip again reverses the view.

Hold Shift during an orbit gesture to snap to the nearest standard or diagonal view. This applies to
right-drag, the Orbit navigation tool, and dragging either view navigator. Under the default preset,
Shift+middle also orbits with snapping; middle alone pans. Cube faces choose principal views,
and corners choose isometric views. The middle of each cube edge belongs to its face. Face names
follow the up-axis preference; the small X/Y/Z triad shows world axes. Clicking the same face keeps that view.
The inward-pointing arrows orbit by 90 degrees; the curved arrows at the upper right roll the
view by 90 degrees. The Home button at the upper left returns to isometric and the lower-right circle
fits the model.

The cube is subdued while idle. Hovering its area brightens the cube and navigation bar and
reveals the surrounding controls; the picked face or corner gets a pale blue fill and
blue outline. Moving away restores the subdued state. Navigation-bar controls also brighten
when hovered or when a navigation tool is active.

The cube has six face targets and eight corner targets, with no edge targets. Corner picking
regions lie on the six flat faces, preserving sharp corners with no permanent subdivision
lines. Hovering a corner highlights its parts on the adjacent visible faces together.

The cube widget occupies a compact 112-point square in the viewport corner. A straight-on
cube face is approximately 55 points wide; painting and picking scale together. Face labels
are stretched vertically. View clicks ease through intermediate orientations when Navigation
animated view changes is enabled (the default), at the configured animation speed.

The cube runs `view.navigate` through the command engine. Programmatic callers pass `camera`
and either `direction: [x,y,z]` in the camera's up-axis frame or `action` (`left`, `right`, `up`,
`down`, `clockwise`, `counterclockwise`, `home`, `fit`). `fit: true` with a `direction` or a
turn asks for the whole model to be framed from the new direction as well, as `home` does. The
result contains `camera` and `fit`; UI callers apply the camera with their animation
preference, without editing the design. The Navigation preference `navigator_fit` (off by
default) makes every view chosen on the cube or on the axis gizmo frame the model this way:
looking at its middle from the one distance that fits its bounding sphere, which Home then shares.

Shift-orbit snapping includes 45-degree edge views and isometric corner views, with a
lightly damped spring transition and a small boundary margin to prevent flicker.

### Command search

`ui.palette {text?, open?}` opens command search under the ribbon search field; `open: false`
closes it. `ui.start {command: "ui.palette"}`, clicking `ribbon:search`, and S open the same
popup. This is UI routing: `engine.execute` continues to run commands without opening dialogs.
The query is limited to 4096 Unicode characters, with at most 80 visible results.

`shortcut:search` and `palette:search` identify the editor; `ribbon:search_popup` and
`palette:outer` identify the popup. Command rows retain `menu:<command id>`; design rows use
`menu:search:design:<index>`. Up/Down choose an enabled row, Enter runs it, Tab fills its id or
name, and Esc closes. Typing `command.id {"parameter": value}` validates JSON live and runs
through the same engine entry as `engine.execute`. Shift+Enter runs JSON and keeps the query
open. Invalid JSON stays editable and reports the parser's error. Feature results open their
existing edit tools; parameter results open Change Parameters filtered to that name.
