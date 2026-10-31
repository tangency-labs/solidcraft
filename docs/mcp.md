# SolidCraft and AI agents (MCP)

SolidCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio
(JSON-RPC 2.0, one message per line; protocol revisions 2025-06-18, 2025-03-26 and 2024-11-05).
An agent models exactly like a person does: every tool runs engine commands, the same ones the
toolbar, palette, scripts and control channel use, and never opens a dialog.

Persistent headless and bridged sessions expose asynchronous product images through the
ordinary `execute` tool: `render.preflight`, `render.start`, `render.status`,
`render.cancel` and `render.export`. Keep the complete returned job ID; image bytes remain
in the session until explicit PNG export. See [render jobs](render-jobs.md).

For how the same command surface should work in the browser (workers, tiered storage, in-tab
agents), see [web-runtime.md](web-runtime.md). For sandboxed plugins and parametric timeline
features that also use `run(command, args)`, see [plugins.md](plugins.md). Desktop MCP in this
document remains the reference implementation today.

## Start it

```sh
# Headless: an in-process design session, no window (screenshots are drawn on the GPU).
solidcraft-cli mcp

# Headless, starting from a design file or a command script.
solidcraft-cli mcp --in part.sc

# Bridged to the running app, so you can watch the agent model. The app's control channel is
# on by default (see below), so no flag is needed on either side:
solidcraft &
solidcraft-cli mcp          # finds the running app; headless when none runs (--headless forces that)

# A fixed port, or a second app: name it.
solidcraft --control 7878 &
solidcraft-cli mcp --connect 127.0.0.1:7878
```

### The app listens by default

The desktop app opens its control channel at start-up on `127.0.0.1:27878` (or the next free port
when that one is taken), unless Preferences > Assistant > "Let AI agents connect (control
channel / MCP)" is off; the setting takes effect at the next start. It is reachable from this
computer only, and every connection must show the token made fresh for that launch, readable only
by your user account ([control-protocol.md](control-protocol.md#token)). The app writes the port to
`control/current.port` in the same folder as its token file; `solidcraft-cli mcp` with no
arguments reads it. `--control PORT` and `SOLIDCRAFT_CONTROL_PORT` still choose the port and apply
even when the setting is off. With the setting off and no flag, the app listens on nothing.

Bridged, the server finds the app's control token in the token file the app wrote for that port
([control-protocol.md](control-protocol.md#token)); `--token T` or `SOLIDCRAFT_CONTROL_TOKEN` give
it explicitly when the app runs in a sandbox whose data folder the server can't see (prefer the
variable: command-line arguments are visible to other users in the process list).

A bridged server starts even when no app listens on that port yet, so an MCP client that
launches it before the app (or after the app was closed) keeps its tools. The server's
instructions then say the app was not running, and every tool call answers with an error that
starts `no SolidCraft app is listening at HOST:PORT` and says how to start it
(`solidcraft --control PORT`). The first call after the app starts connects to it, reading that
launch's token. When the app quits or crashes mid-session, the next call says the same; a call
that was running when it went away says the app did not answer and may have quit or crashed,
after running the call or not, so inspect the design before sending a change again.
A `batch` that could not be sent says the app is not running, so it is safe to send again.

stdout carries only protocol messages; errors go to stderr. Bridged screenshots are written to a
temporary file by the app and read back by the server, so both must run on the same machine.

### Parameter checking

Commands with a typed parameter contract (every new command, and more of the older ones as they
are converted; `describe_command` shows a command's schemas) can be checked as they are called.
`--param-check off|warn|strict` sets the level for a headless session, including a script or
recipe given with `--in`, and for calls bridged to the app; the default is `off`, which changes
nothing. `warn` runs the call and adds `param_warnings` to its result, such as ``unknown key
`operaton`; did you mean `operation`?`` (a starting script's warnings go to stderr). `strict`
refuses the call, and the design is left as it was. Bridged, the server asks the app once per
connection whether it checks a request's own level (`control.capabilities` and `param_check` in
[control-protocol.md](control-protocol.md)). If it does, every call and `batch` is sent with the
level and the app checks it against its own commands, so a newer app's keys are not refused. At
`strict`, a reply that doesn't say the app checked the call at that level is an error (it may
have run unchecked: inspect the design), and the server checks the next calls itself.
An older app is checked here instead, against the server's own commands' contracts, before
anything is sent: `strict` sends nothing that breaks one, and a command the server has no
contract for goes to the app unchecked. With such an app, use the server from the same release:
an app newer than the server may take keys the server's contracts don't know yet, and `strict`
would refuse them.

### Which files the agent can reach

Tool calls read, write and delete files only inside the server's folders:

- the folder of the open design (the `--in` design, the design open in the app, or whatever
  the agent last opened or saved), but only when it lies strictly inside your home folder (or
  the temporary folder) and not in a hidden folder. Home itself, a folder above it (`/`,
  `/home`, `/Users`, a drive's root), hidden folders such as `~/.config` and folders elsewhere
  don't count. Hidden files and folders inside the design's folder (`.env`, `.git`) are out of
  reach too;
- on the same terms, every folder the agent opened or saved a design file in during this
  server's run (`open`, `save`, or `doc.open`, `file.save` and `file.save_as` through `execute`
  or `batch`, with the path given in full) and, headless, every folder a design was open in,
  until the server stops (the 16 most recent). So after a new design, an opened STEP file or a
  save-as elsewhere, the agent can still save beside the design it worked on. Such a call
  succeeded only because its folder was already one of the agent's, so no folder joins just
  because the agent names it;
- SolidCraft's own recovery folder, so `doc.recovery_list`, `doc.recover` and
  `doc.recovery_discard` work out of the box (discarding deletes only real recovery entries);
- the server's own scratch folder (a new private `solidcraft-mcp-*` folder in the temporary
  folder), where `screenshot` without a `path` goes;
- every `--allow-path DIR` (repeat it for more). A folder named here is reached in full,
  hidden files included.

The working folder the server was started in does not count by itself, so `~/.ssh`, `~/.aws`,
`~/.bashrc` and other dotfiles are never in reach by default. stderr lists the folders at
start-up and says when the `--in` design's folder was left out; `--allow-path` adds it back.

Paths are resolved before they are compared, so `..` and symlinks that lead out are refused
(saving through a link writes the file it points to, so the link's target is what counts), and
a new file must go into a folder that exists (a missing folder inside an allowed one is reported as missing; it is not created and does not call for a wider `--allow-path`). On Windows a path must be on a local drive
(`C:\…` or `\\?\C:\…`): UNC, device and NT paths in any spelling (`\\server\share`,
`//server/share`, `\\?\UNC\…`, `\??\…`, `\\.\…`), drive-relative paths (`C:file`), a `:` in a
file name (alternate data streams), reserved device names with any extension (`NUL`, `CON`,
`COM1.txt`, …) and short 8.3 names (`PROGRA~1`) are refused before any file system call, and the
resolved path is checked again, so a link or junction that leads to a share is refused too. This
includes a `sketch.text` font given as a file path and the font file a design's text names when
the design is opened; installed fonts named by family still work.
The server spells its own folders (scratch, recovery) with long names, so they work when the
temporary folder's path has a short name (`…\NAME~1.XYZ\AppData\Local\Temp`, as Windows often
gives when the user folder's name is long).
A refused path is a tool error saying why.

A hard link inside an allowed folder can't be told apart from an ordinary file, so a hard link
to a file elsewhere is reachable through it; don't keep such links in a folder you allow.

```sh
solidcraft-cli mcp --allow-path ~/designs --allow-path ~/exports
solidcraft-cli mcp --allow-any-path   # no limit (the behaviour before this option existed)
```

When a `save` (or any other call) is refused with `… outside the folders this agent may use …`,
add the folder to the server's `args` in your MCP client's configuration (the `mcpServers`
entries below, or what `solidcraft-cli mcp setup` wrote), for example
`"args": ["mcp", "--allow-path", "/Users/NAME/Documents/Designs"]`, and restart the client.
An agent should ask you to do that rather than save through the app's control port, which
knows nothing of these folders.

Bridged, the server sends its folders with every request (`allow_paths`, and the folders the
agent opened or saved a design in as `design_folders`, see
[control-protocol.md](control-protocol.md)) and the app adds its own recovery folder and the
folder of the design open in it, under the same rules, so give absolute paths: a relative one
resolves against the app's working folder, not the server's. The app's own
file dialogs are never limited.

### Tool annotations

Each tool says what it may do, for clients that ask before running tools (or auto-approve on
these hints). `list_commands`, `describe_command`, `inspect_design`, `measure`, `body_topology`,
`skill_reference`, `crash_reports`, `pending` and `running` are read-only. `set_parameter`, `undo` and `redo`
change the design undoably, and so does `check_parameters` (it changes the design and puts it back:
not read-only, not destructive). `new_design` replaces it (destructive). `execute`, `batch`, `open`, `save`, `export` and
`screenshot` (whose `path` writes a file) can read, write, overwrite or delete files: they are
not read-only, `destructiveHint` and `openWorldHint` are true.

### Client configuration

`solidcraft-cli` is on `PATH` after every package install: the Windows installer adds its folder, the deb, rpm and FreeBSD packages install it in `/usr/bin`, the Homebrew cask links it, the AppImage writes a small `solidcraft-cli` launcher into `~/.local/bin` on its first run, and the macOS app links it into `/opt/homebrew/bin` or `/usr/local/bin` when it starts (Help > Install solidcraft-cli command does it with an administrator prompt when those are not writable). Two packages keep it out of `PATH` by design: Snap exposes it as `solidcraft.cli`, and Flatpak runs it as `flatpak run --command=solidcraft-cli io.tangencylabs.solidcraft`; use those as the client's `command` (with `mcp` as the arguments).

The quickest way: **Help > Connect AI assistants…** in the app, or

```sh
solidcraft-cli mcp setup            # every client found on this computer
solidcraft-cli mcp setup --dry-run  # show what it would write, change nothing
solidcraft-cli mcp setup --client opencode --live
```

(`mcp install` is the same.) It adds a `solidcraft` server, using the full path of the
`solidcraft-cli` that runs it, so nothing has to be on `PATH`. A client counts as installed when its
configuration folder exists, and no client program is started. Supported:

| `--client` | Configuration file |
| --- | --- |
| `opencode` | `~/.config/opencode/opencode.json` (or `.jsonc`) |
| `claude-desktop` | `%APPDATA%\Claude\claude_desktop_config.json`, `~/Library/Application Support/Claude/`, `~/.config/Claude/`, and the Microsoft Store package's folder |
| `claude-code` | `~/.claude.json` (user scope `mcpServers`) |
| `cursor` | `~/.cursor/mcp.json` |
| `codex` | `$CODEX_HOME` or `~/.codex`, `config.toml` (CLI, IDE extension and desktop app) |
| `gemini` | `~/.gemini/settings.json` |
| `copilot` | `~/.copilot/mcp-config.json` (GitHub Copilot CLI) |
| `windsurf` | `~/.codeium/windsurf/mcp_config.json` |
| `zed` | Zed's `settings.json` (`context_servers`) |
| `vscode` | VS Code's user `mcp.json` |

The files are edited in place: only the `solidcraft` entries change, everything else in the file
(other servers, comments, order) stays as it was, and the file is copied to `<name>.bak` first. A
file that does not parse is left alone. Running it again after an update moves the program only
refreshes the path of an entry it wrote; an entry you changed (extra arguments, another program)
is left as you have it. `--live` also adds `solidcraft-live`, which bridges to the open app (port
7878, see below). Restart the client afterwards.

SolidCraft also does this by itself: when the app starts it adds the `solidcraft` entry to each
installed client (and refreshes its path after an update), and says once which clients it
connected. It never touches an entry you changed. **Settings > Assistant > Make SolidCraft
available to AI apps on this computer** turns it off, which removes the entries it added. Runs
with `--control` or `SOLIDCRAFT_CONTROL_PORT` (automation) leave your clients alone.

To configure a client by hand instead, these are the entries. OpenCode (`opencode.json`):

```json
{
  "mcp": {
    "solidcraft": { "type": "local", "command": ["/path/to/solidcraft-cli", "mcp"], "enabled": true }
  }
}
```

Claude Code (project `.mcp.json`, or `claude mcp add solidcraft -- solidcraft-cli mcp`; a setup
added under the previous name keeps working, see the [compatibility notes](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md#mcp)):

```json
{
  "mcpServers": {
    "solidcraft": { "command": "solidcraft-cli", "args": ["mcp"] }
  }
}
```

If a terminal doesn't find `solidcraft-cli` (older Windows installers don't put its folder on
`PATH`; see below for where it is), give its full path (by default
`C:\Program Files\SolidCraft\solidcraft-cli.exe`):
`claude mcp add solidcraft -- "C:\Program Files\SolidCraft\solidcraft-cli.exe" mcp`.

Claude Desktop (`claude_desktop_config.json`), bridged to a running app:

```json
{
  "mcpServers": {
    "solidcraft": { "command": "/path/to/solidcraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7878"] }
  }
}
```

On Windows the installer puts `solidcraft-cli.exe` next to `solidcraft.exe` in
`C:\Program Files\SolidCraft` (the portable zip has both too). Give the full path, with each
backslash doubled in JSON. This configuration declares two servers: `solidcraft` models
headless (no window; steadier for timings), `solidcraft-live` drives the app you have open, so
you can watch the agent model. `--allow-path` names a folder where the agent may save and
export; it must already exist.

```json
{
  "mcpServers": {
    "solidcraft": {
      "command": "C:\\Program Files\\SolidCraft\\solidcraft-cli.exe",
      "args": ["mcp", "--allow-path", "C:\\Users\\NAME\\SolidCraft-Agent"]
    },
    "solidcraft-live": {
      "command": "C:\\Program Files\\SolidCraft\\solidcraft-cli.exe",
      "args": ["mcp", "--connect", "127.0.0.1:7878", "--allow-path", "C:\\Users\\NAME\\SolidCraft-Agent"]
    }
  }
}
```

Start the app with its control channel for `solidcraft-live`, from PowerShell:
`& "C:\Program Files\SolidCraft\solidcraft.exe" --control 7878`, or once for every launch:
`setx SOLIDCRAFT_CONTROL_PORT 7878`.

Open `claude_desktop_config.json` with Claude Desktop's **Settings > Developer > Edit Config**
rather than by its path: the Microsoft Store (MSIX) build of Claude Desktop can keep its data in
its own package folder (`%LOCALAPPDATA%\Packages\…\LocalCache\Roaming\Claude`) instead of
`%APPDATA%\Claude`, and Edit Config opens the file the app actually reads. Quit Claude Desktop
completely (from the notification area) and start it again to load the servers.

### Agent skills

[`skills/`](https://github.com/bherbruck/solidcraft/tree/main/skills) holds five [Agent Skills](https://agentskills.io) for agents that model through this server:
- `solidcraft-modeling` covers building a new part.
- `solidcraft-editing` covers changing an existing design.
- `solidcraft-checking` adds checks for errors SolidCraft does not report, regression tests and porting, next to the other two.
- `solidcraft-mechanisms` covers assemblies: joints placed by world coordinates, motion links, closed loops such as actuators, contact sets and motion studies, checked against analytic values.
- `solidcraft-reporting` turns a problem the user hit into a bug report: it reproduces it in a fresh design, shrinks it to a minimal command script, checks the numbers with `measure` and a screenshot, and searches existing issues first.

They cover the workflow, the command shapes, how to verify with `measure`, and the known traps. With them an agent builds a part in a few `batch` calls and reads far less output. To install them for Claude Code, copy the folders into `~/.claude/skills/`; [skills/README.md](https://github.com/bherbruck/solidcraft/blob/main/skills/README.md) has the details.

The `skill_reference` tool returns these documents from the copies built into SolidCraft, so an agent without file access can follow them: `{"skill": "solidcraft-modeling"}` returns its `reference.md`, the default file. Mechanisms also has `reference.md`, checking has `reference/porting.md`, and every skill has `SKILL.md` (editing uses modeling's reference). It never reads a path from disk, and an unknown skill or file is refused with the available choices. SolidCraft's own chat turns the agent's file tools off, so this is how its agent reads the skills there. The instructions the server sends at `initialize` point agents at `skill_reference` first, then say how to plan the numbers, build in a few `batch` calls and verify each stage with `measure`; the same pointer is in the system note SolidCraft's own chat gives its agent.

## Tools

| Tool | Arguments | What it does |
|---|---|---|
| `list_commands` | `filter?` | The command catalog: id, label, tab/panel, shortcut, parameter docs, enabled now. `filter` matches id, label, tab and panel. |
| `describe_command` | `id` | One command's parameter docs (`params`) and, for commands with a typed contract (every new command, and more of the older ones as they are converted), the JSON Schemas (2020-12) of its parameters (`input`) and of a successful result (`output`); `null` for the others. The same as `execute` `engine.describe {command}`. |
| `execute` | `command`, `params?`, `verbose?` (default true), `expect?` | Run any command with JSON parameters (`list_commands` shows them). Failures leave the design unchanged. A feature command replies with the feature (`feature`, `name`), every body (`bodies`) and the bodies it created, changed and removed (`created_bodies`, `changed_bodies`, `removed_bodies`). `verbose: false` uses the same compact reply as `batch`. `expect` checks the design after the command, as it does for `batch` (see below). |
| `batch` | `commands: [{command, params}]`, `rollback?` (default true), `verbose?` (default true), `timings?`, `profile?`, `coalesce_parameters?` (default false), `expect?` | Run steps in order, stop at the first error and undo the completed steps. The reply names the failing step (`failed.index`, `failed.command`, `failed.error`), every result before it, `rolled_back` (true only when the design really went back to where the batch started), `undone`, `applied` (the indices of the steps whose changes are in the design) and `not_run` (the steps never sent). A step may name a tool instead of a command, with that tool's arguments as `params`: `set_parameter` (runs `parameters.change`; `value` or `expression`), `list_commands` (`engine.commands`, filtered by `filter` as the tool filters it), `describe_command` (`engine.describe`), `execute` (`{command, params}`), `measure` (`summary: true` summarised as the tool summarises it), `inspect_design` (`document.inspect`: each sketch as its counts, not its full contents, and no joints), `undo`, `redo`, `export`, `save`, `open`, `new_design` and `crash_reports`. `screenshot`, `body_topology` (use `model.edges` and `model.faces`), `batch`, `check_parameters`, `pending`, `running` and `skill_reference` can't be steps, nor can `component.edit_in_place` and `component.finish_edit`, however a step names them, as they go to another design's tab (send them with the `execute` tool on their own): such a batch fails before anything is sent. So does a step the batch would certainly refuse at its turn (#5853): a step without a command, an unknown command id (the error names the closest ids) or, with `--param-check strict`, parameters that break a command's contract. A headless session checks this itself; an app checks it against its own commands when it gets the batch, and a bridged server at `--param-check strict` checks the contracts before it sends the batch. `verbose: false` keeps the reply small in large designs: feature steps leave out the full body list (`created_bodies`, `changed_bodies`, `removed_bodies` stay) and sketch steps the ids they repeat outside `created`. Bridged to an app that has `engine.batch`, the whole batch is one control request (one frame of the app). Older apps, and apps showing the agent cursor, get the steps one by one. `timings: true` adds `timings: [{index, command, ms}]` for every step run, the failing one included, and `total_ms`. `profile: true` also adds each step's `profile: {boolean_ms, tessellate_ms, other_ms}`, its time in kernel booleans, in tessellation and the rest. Steps are timed where they run: in the app when bridged. An app without `engine.batch` is timed step by step, without `profile`. `coalesce_parameters: true` sets each run of consecutive plain parameter edits (`parameters.change` or `set_parameter` with a name and a value, or `changes`) together, so the design rebuilds once per run instead of once per edit. Steps between runs see the design up to date. Each step keeps its own result (`coalesced`: the run's length), and the run is one undo step. The run skips the states between its edits, so an edit refused on its own can go through with the run's later ones. A refused run goes step by step, naming the refused step. |

**`expect`** on `batch` and `execute` turns the verify step into part of the build. An agent
computes the volume, body count or bounding box a stage should produce before building it;
instead of a `measure` call and a comparison by hand after every stage, it gives them to the
call: `"expect": {"volume_mm3": 59824.9, "tol": 0.002, "bodies": 1, "bbox": {"min": [0, 0, 0],
"max": [140, 90, 30]}}`. Every key is optional (at least one of `volume_mm3`, `bodies` and `bbox`);
`tol` is relative to the expected volume (default 0.002, so 0.2 %), and each bounding-box
coordinate must lie within 0.05 mm. Once the call has gone through, the server measures the
design the way `measure` does (the total volume of the closed bodies, the body count and the
box round every body) and the reply gains
`"expectation": {"ok": bool, "measured": {"volume_mm3", "bodies", "bbox"}, "expected": {...}, "mismatch": ["volume: measured 59680, expected 59824.9 (-0.24 %)", ...]}`.
On a mismatch the tool result is flagged `isError: true`, with the mismatch text as its first
content block and the reply as the second, so the model notices; the design is kept as built
(undoably, as ever), and the agent decides whether to undo, edit a feature (`timeline.edit`) or
carry on. A batch that fails is not measured, and an `expect` the tool cannot use (an unknown key,
a value of the wrong type, a negative `tol`, a `bbox` without three coordinates a side) is
refused before anything runs. `expect` is an argument of the tool, not a key of a step: a step
carrying one is treated as any step with a key the batch does not know. Volume and body count
say nothing about which kernel built the design, which is the point: the check holds whatever
builds it.
| `inspect_design` | `measure?` (default true), `sketches?` (default true), `joints?` (default true) | Design properties (`metadata`: project, author… and other tools' keys; set them with `execute` `document.metadata.set`), parameters, timeline (type, suppressed, rolled back, error, warning, timing), bodies (volume, area, centre of mass, bbox, face/edge/vertex counts, visible; `removed_later: true` while a sketch is being edited on a rolled-back timeline and a later feature deletes the body, which is then not shown), sketches (curves, points, constraints, profiles, DOF, solve status, visible), joints when the design has any (as `joint.inspect` gives them: each origin's occurrence, component and path, the snap as saved, the feature, face, edge or sketch curve (`sketch: {id, name, curve}`) its persistent names refer to, `resolves` and the `error` when it is lost; kind, offset, angle, flip, values, limits, expressions, motion links; `execute` `joint.inspect {joint}` gives one joint), selection, undo depth. Hide helper sketches, bodies or construction planes with `execute` `browser.visibility` (`{items, visible}` or `{folder, visible}`), and joint glyphs with `{joints: [id or name], visible}` or `{folder: "joints", visible}` (a hidden joint still constrains and drives; `joint.list` reports each joint's `visible`). |
| `measure` | `body?` or `items?`, `summary?` (default false) | Area (mm²), bbox, topology counts and `closed` per body. Volume (mm³), volume centre of mass, density and mass are reported only for closed, consistently oriented bodies (null or omitted for open surfaces). Volume/mass totals skip open bodies; `total.open_bodies_skipped` gives their count. Area and topology totals include all bodies. With `items` (one or two selections as `execute` takes them: `face`, `edge`, `vertex`, `sketch_curve`, `profile`), the picked geometry instead: faces and edges need only `type`, `body` and `point`; `index` is optional. See below. `summary: true` keeps one short object per body (`name`, `volume_mm3`, `bbox: {min, max}`, `faces`, `closed`; a body that could not be measured keeps its `error`), `body_count` and `total.volume_mm3`, nothing else: what a stage check against computed numbers needs, at a fraction of the size. It applies to the bodies reply, not to `items`. |
| `body_topology` | `body`, `faces?` (`all`, `cylinders`; default `all`) | Edges (index, midpoint, length, ends) and faces (index, name, area, centroid, normal, `point`, and for a cylindrical face its `cylinder`: `origin`, `axis`, `radius`). An edge midpoint is the `[x,y,z]` that picks that edge for fillet and chamfer. A face's `point` lies on it and picks it (its centroid may not: that of a face with a hole is in the hole); a face's `name` picks it for the commands that take faces (`face.delete`, `solid.shell`, `solid.draft`, `solid.offset_face`, `solid.split_face`, `solid.replace_face`, and `face` of `sheet.convert`). `faces: "cylinders"` replies `cylinders` instead of `edges` and `faces`: one entry per cylinder, `{radius, origin, axis, extent, names}`, the halves the kernel splits a cylinder into merged (same radius, same axis line; `origin` and `axis` as the first half's `cylinder` gives them), `extent` the `[min, max]` distance along `axis` from `origin` that the body's points on the cylinder span (the faces' `point`s and the ends of the edges lying on it; left out when none were found), and `names` the merged faces' names, any of which picks the face. Enough to check where every hole and boss is, in a few lines instead of the whole topology. |
| `set_parameter` | `name`, `value` or `expression` (number or expression), `unit?`, `comment?` | Create or change a user parameter and recompute. `expression` is another name for `value` (the one `parameters.change` uses): give one of them, or both only when they agree; neither, or two that differ, is an error. Reports `recomputed` (features rebuilt), `restored` (features whose earlier result for the same inputs was reused, e.g. a value set back) and features that now fail. An expression `new_name = expression` renames the parameter as it sets it; in any other tool's expression parameter it adds the user parameter `new_name` and the input uses it (`defined`, `definitions` in the reply). |
| `check_parameters` | `set: {name: value}` (a number or an expression each, as `set_parameter`'s `value`), `report?` (`measure`, `cylinders`, `errors`; default all three) | Try parameter values without keeping them: one `edit.checkpoint`, every value set in one `parameters.change {changes}` (one rebuild; an app without `changes` gets one change per name), the reports read, then `edit.restore_checkpoint`, so the design, its parameters and its undo history are exactly as before. The reply has `values` (each parameter's value with the tried expressions in), `recomputed`, `measure` (volume, bbox, bodies and faces per body, as the `measure` tool reports them), `errors` and `warnings` (the features that fail or warn: `{feature, name, error}`) with `parameter_errors` (the parameters that don't evaluate), `cylinders` (every cylindrical face of every body as `{body, radius, origin, axis, faces, name, area}`; the halves a kernel splits one cylinder into are one entry, `origin` is the axis point closest to the world origin and `axis` points so that its first non-zero component is positive) and `restored: true`. A name the design has no parameter of, a value that is not a number or a string, an expression that does not evaluate or an unknown report item is an error, and nothing changes. If the checkpoint cannot be restored the call is an error with `restored: false`, `restore_error` and a note that the tried values are still in the design: set them back before saving. Replaces the loop `set_parameter`, `measure`, `body_topology`, `set_parameter` back, which cost every agent session three to five calls and up to 16 s of rebuilds. Not a `batch` step. |
| `screenshot` | `view?` (iso, top, front, back, bottom, left, right, home, fit, or a named view saved with `view.save`), `camera?` (`{target, yaw, pitch, distance, fov?}`, as `view.apply`), `path?`, `width?`, `height?`, `source?` (window, model), `grid?`, `axes?` | A PNG image content block. Headless: a render with the viewport's pipelines on a headless GPU device (see `solidcraft-cli gpu`; with no adapter, the CPU rasterizer, without analysis colours), fitted to the model, or from the named view or camera; the reply's `renderer` says which (`gpu`, `gpu-software`, `cpu`, with a `warning` for `cpu`). Bridged: the app window (default) or a model render with the app's camera; a named view or camera moves the app's viewport there first (`execute` `view.apply` does the same without a screenshot). `grid: false` and `axes: false` leave the ground grid (it lies on Z 0 and cuts through a part that straddles it) and the origin's axes out of a model render; axes hidden with `browser.visibility` stay out whatever `axes` says. Bridged, a screenshot that gives `grid` or `axes` is a model render, which follows the app's Grid and Origin switches for what it does not give; `source: "window"` with either is an error. A window that is minimised or on another macOS Space presents no frame: with no `source` the reply is a model render and says so (`fallback`); `source: "window"` is an error saying why, at once. Agents that don't need the app's chrome can ask for `source: "model"`. |
| `export` | `path`, `format?` (step, 3mf, 3mf-tagged, stl, stla, obj, urdf), `bodies?`, `mesh_prefix?`, `effort?`, `velocity?` | STEP, 3MF or mesh export; URDF writes the whole assembly with its meshes next to it (`docs/urdf.md`). |
| `undo`, `redo` | — | Undo / redo one change. |
| `pending` | — | Bridged: the result of a request that was still running when its call returned (an error starting `still running:`), waiting up to 60 s for it; `done: false` while it runs (call again), `pending: null` when nothing runs. See [Long-running requests](#long-running-requests). |
| `crash_reports` | `id?`, `limit?` | The crash reports on this computer, newest first; with `id`, the whole report (backtrace, command, recent commands, `design_path`/`replay_path`). See [crash-reports.md](crash-reports.md). |
| `skill_reference` | `skill`, `file?` (default `reference.md`) | Read a bundled skill document. Skills: `solidcraft-modeling`, `solidcraft-mechanisms`, `solidcraft-editing`, `solidcraft-checking`. |
| `running` | - | What the app is running or has queued, for every client (`control.running`, see [control-protocol.md](control-protocol.md#limits)): each request's connection and id, method, command id or a batch's step count, `running_s` and `queued`, never its parameters. Answers at once while the app is busy: ask it when a call timed out before sending a change again. Headless: always `{"running": []}`. |
| `new_design` | `name?` | Start an empty design. |
| `open` | `path`, `osd_mode?` | Open a `.sc` design (or a `.solidcraft` one from an earlier release), a STEP file (`.step`/`.stp`) as a new design holding its bodies (an Import base feature), or a 3MF/STL file as mesh bodies. OSD v2 defaults to editable solid reconstruction with mesh fallback and warnings; `osd_mode: mesh` retains triangles, `solid` refuses fallback. OSD import and experimental export use no vendor runtime; see [OSD exchange](formats/osd.md) for supported records, provenance and limitations. |
| `save` | `path?` | Save the design (to its current file without `path`). |

`measure` with `items` reports each item under `items` (`length_mm`, `area_mm2` or `position`).
A circle, arc, hole or other cylindrical face also gives `diameter`, `radius`, `center` and
`axis`, with `circumference` when it goes all the way round, or `arc_length` and `angle_deg`
for an arc; `round` says which (`circle`, `arc`, `cylinder`). A slot (a sketch profile of two
half circles and two lines, or one half-cylinder end of a slot in a solid) gives `slot`:
`width`, `center_distance` and `overall_length`. With one item these fields are repeated at
the top level. With two items:

- `min` (also `distance_mm`, with `from`, `to` and `delta`): the closest distance;
- `max` (with `max_from`, `max_to`): the farthest distance;
- `center_distance` (with `center_from`, `center_to`): when an item is round, from its centre
  or axis: axis to axis for parallel axes, the closest distance between the axes otherwise,
  and axis to plane, line or point;
- `measure`: the distance the Measure panel shows first (`center` where there is one, else `min`);
- `angle_deg` between two planar faces or straight edges.

Earlier fields are unchanged, so older scripts keep working.

To change a feature after it is made, read its definition with `execute` `timeline.inspect`
(`{feature}`: id or name) and change fields with `timeline.edit` (`{feature, set}`; e.g.
`{"extent": {"distance": "30"}}`). The fields per feature type are listed in the book's
*Solid features* chapter.

To tell whether the design changed since a call, without reading all of it again, `execute`
`document.revision` answers `{document_id, revision}`: `revision` moves only when the design
changes, and `document_id` names the open design (another one after `new_design` or `open`).
See [control-protocol.md](control-protocol.md#following-the-design).

Resources: `solidcraft://design` (inspect JSON with measurements) and `solidcraft://commands`
(the catalog).

Errors: an unknown tool or arguments that don't match the tool's schema are JSON-RPC
`-32602` errors (an unknown argument's error lists the tool's arguments); an engine failure
(bad command parameters, a feature that fails) is a tool result with `isError: true` and the
engine's message, which the model can read and correct.

### Long-running requests

The app answers a control request that runs longer than 60 seconds (a big fillet, say) with
"still running" ([control-protocol.md](control-protocol.md#limits)); it has not failed, and was
not cancelled. The bridged server then keeps the connection, where the reply will come, and
asks the app (`control.pending`) every half second for up to 240 seconds more, saying on stderr
how long it waits; when the request finishes, the tool call returns its real result. If it still
runs after that, the tool call fails with an error starting `still running:` that names the
request and says what it still does: it will still apply when it finishes, or will still write
the file (an export or a save: the design does not change), or only reads (nothing changes). Don't
send it again: call the `pending` tool, which waits up to 60 seconds for its result each time.
Any other tool call first waits up to 60 seconds for it too, and is not sent while it still runs
(so nothing is queued to apply unseen behind it); a request that finishes in that wait keeps its
result for `pending`. If the connection to the app is lost meanwhile, the result is lost with it,
and `pending` says so: inspect the design before sending a change again.

So a single tool call may block for up to 300 seconds (the app's 60 s wait plus 240 s of polling)
before it returns `still running:`, and a call made while an earlier request still runs for up to
60 seconds more. An MCP client needs a per-call timeout above that (say 330 s) to see that error;
a client that gives up sooner only sees its own timeout, though the request goes on running in the
app: then call `pending` (or `inspect_design`) before sending anything again.

A `batch` step that is still running stops the batch like a failure. Its rollback first waits for
the step (as above), so a restored checkpoint takes the step back too; if it still runs, the
reply says `rolled_back: false`, marks the step `still_running` and lists it in `applied`, as it
will apply when it finishes.

`batch` rollback goes back to an engine checkpoint (`edit.checkpoint`) taken before the first
step: the design, the active sketch and the undo history are restored exactly as they were,
however long the history is. Batches that call `file.new` or `doc.open` get the design back,
not its file path. (An older app without checkpoints falls back to undoing as many steps as
the batch added to the undo history.)

## Camera and close-up recipe

The `camera` of `screenshot` (the object `view.apply`, `view.save` and `solidcraft-cli snapshot --camera` take too) is an orbit camera, `{target, yaw, pitch, distance, fov?}`, in millimetres and **radians**. Unlike the `angle` of `solid.move`, a bare number is not degrees here. The eye is at `target + distance * (-sin(yaw) * cos(pitch), -cos(yaw) * cos(pitch), sin(pitch))`, looking at `target` with Z up on the screen. So `yaw: 0, pitch: 0` is the front view: the eye at -Y, looking along +Y, with +X to the right. A yaw of π/2 looks from -X, π from +Y and -π/2 from +X. The pitch is the elevation angle up from the XY plane, and π/2 looks straight down with +X to the right and +Y up; `view.apply` refuses a pitch past ±π/2. A positive `roll` turns the picture clockwise. The app's View angles readout (`view.angle_readout`) shows the same yaw, pitch and roll in degrees. `fov` is the vertical field of view in radians. `0`, or no `fov`, is an orthographic view whose visible half-height is 0.4 × `distance`; with a perspective `fov` the half-height at the target is `distance * tan(fov / 2)`. `inspect.section` removes what lies on the side its normal points to: it keeps the points `p` with `n · p < d`, where `d = n · origin` (`flip: true` keeps the other side). To look into the cut, put the eye on the removed side.

To look closely at one part of a large model, such as a STEP assembly, work in a headless session. These steps change only the open design in memory:

1. Open the model with `solidcraft-cli mcp --in assembly.step` (or the `open` tool, which also takes `up` and `names`, see step 4 and [STEP import](step-import.md)). `inspect_design` lists the bodies with their names and bounding boxes.
2. Remove what is in the way with `solid.remove`, e.g. `{"bodies": ["Cover"]}`.
3. Cut the region out of the rest with `solid.box`, e.g. `{"corner": [40, 0, -45], "length": 20, "width": 50, "height": 45, "operation": "intersect", "targets": ["Housing", "Plate"]}`. Each listed body is trimmed to the box, and one that lies wholly outside the box is removed. The box itself is not kept.
4. If the model lies on its back (many STEP files have +Y up), open it with `up: "y"` (the `open` tool, `doc.open`, `file.insert_step`), which turns the file's bodies a quarter turn about X (Y becomes Z). One already open is stood Z up with `solid.move`, listing every body you keep, e.g. `{"bodies": ["Housing", "Plate"], "axis": [1, 0, 0], "angle": 90}`. It turns the bodies about the world origin, and a bare angle is in degrees. A turn of +90° about X takes +Y to +Z.
5. Colour the parts that matter with `appearance.assign`, e.g. `{"bodies": ["Housing"], "color": "#3a7bd5"}`, and hide helper bodies or sketches with `browser.visibility`, e.g. `{"items": ["Plate"], "visible": false}`.
6. Cut a section with `inspect.section`, e.g. `{"plane": {"origin": [50, 30, 0], "normal": [0, 1, 0]}}` or `{"plane": "XY", "at": 5}`.
7. Take the picture with `screenshot`: a `camera` whose `target` is the centre of the region and whose eye is on the cut side, `source: "model"`, `grid: false` if the region straddles Z 0 (the ground grid would cut through it), and `path`, `width` and `height` as needed. An orthographic picture is 0.8 × `distance` high, so a `distance` of about 1.5 times the region's size frames it. Look at the image, adjust `yaw`, `pitch` and `distance`, and shoot again. Headless, the picture is always a render of the model; bridged to the app, `source: "model"` asks for that render instead of the app's window.

## Compact replies

`execute` and `batch` default to `verbose: true`, preserving the full command results.
For long modelling sessions, set `verbose: false` at the tool-argument level:

```json
{"name": "execute", "arguments": {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}, "verbose": false}}
```

Feature replies omit the full `bodies` list while keeping `feature`, `name`,
`created_bodies`, `changed_bodies` and `removed_bodies`. A cut can therefore report a
changed body even when no body name was added or removed. Sketch replies keep new
IDs under `created` and omit only identical top-level `points`, `curves`,
`constraints` and `dimensions` lists. Other fields and errors are unchanged;
inspection commands still return their complete results. Compaction uses the
engine's reply directly, without an additional design-inspection request.

## Demo: an enclosure built over MCP

`examples/demo/enclosure_mcp.py` is a small MCP client. It drives a running app (`solidcraft
--control PORT`) through `solidcraft-cli mcp --connect` and builds an electronics enclosure one
`execute` call at a time (`enclosure_steps.json`): a rounded box, shelled open at the top, a
USB port cut through a side wall, four screw bosses, and then a change of the `width`
parameter that the timeline rebuilds. `examples/demo/record.sh` records the run with ffmpeg,
with each step's caption burnt in.

`examples/demo/enclosure_ui_mcp.py` (v2) builds the same part the way a person would: the
modelling goes through the app's UI over its control channel. The pointer glides to toolbar
buttons and faces and clicks them, the real dialogs open, the sketches go on the model's faces,
and the camera follows. The parameters, the sketch dimensions bound to them and two extrudes go
over MCP. With `--story v3` (the default) one standoff is patterned 2 × 2 in the Rectangular
Pattern dialog with spacings from the parameters, and width and wall are then changed in the
Parameters dialog. `record_ui.sh` records it and draws the pointer and the captions.

## A typical session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"me","version":"1"}}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"batch","arguments":{"commands":[
     {"command":"sketch.create","params":{"plane":"XY"}},
     {"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[40,30]}},
     {"command":"sketch.finish"},
     {"command":"solid.extrude","params":{"distance":20}},
     {"command":"solid.fillet","params":{"edges":[[0,0,10]],"radius":3}},
     {"command":"sketch.create","params":{"plane":"XY"}},
     {"command":"sketch.circle.center","params":{"center":[20,15],"radius":5}},
     {"command":"solid.extrude","params":{"distance":20,"operation":"cut"}}]}}}
← {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"ok\": true, \"completed\": 8, \"results\": [...]}"}]}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"measure","arguments":{}}}
← … "total": {"volume_mm3": 22390.6, …}
→ {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"screenshot","arguments":{"view":"iso"}}}
← {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"image","mimeType":"image/png","data":"iVBORw0…"},{"type":"text","text":"{\"source\":\"model\",…}"}]}}
→ {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"export","arguments":{"path":"/tmp/part.step"}}}
```

The tests in `crates/mcp/src/tests.rs` run sessions like this (box → fillet → hole, parameters,
batch rollback, hostile arguments) and check the results only through MCP.
