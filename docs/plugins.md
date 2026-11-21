# Plugins

[RFC 0002: Plugins](rfcs/0002-plugins.md) defines the plugin API and phases.
Phase 1 splits the host into two app-agnostic crates:

- `crates/craft-plugin` owns the generic WIT contract, manifest, permissions,
  entry input schemas and typed host errors. It has no wasmtime dependency.
- `crates/craft-plugin-wasm` runs WebAssembly components against that contract
  with fuel, memory and stack limits. Its linker exposes no network imports.

An app implements `PluginHost`: it supplies command schemas, each command's
`read` or `edit` effect, command execution, transaction begin/commit/rollback,
and the folders that file grants may resolve to. One successful plugin call is
one undo step; a failed guest call rolls back the transaction. Granted file writes
are staged and published only after the guest succeeds and the app commits. A rollback
deletes staged files without replacing their targets. A filesystem failure during publication
after app commit is reported and may leave some files changed. App commands remain
JSON data rather than app-specific WIT types. Both crates are Shared crates;
`cargo xtask layers` forbids direct and indirect SolidCraft dependencies.

SolidCraft's adapter lives in `crates/engine` and uses the same command surface
as MCP. Entries appear as `plugin_id.function`; `plugin.list` and
`engine.describe` expose their declared inputs. Selection and target inputs
are lists. Entries may declare named menu or ribbon slots; each app decides
which slots exist and draws forms from the input schemas. Plugins ship no UI
code. SolidCraft retains approval, cancellation and plugin preferences.

## Installing

**Install plugin…** in **Preferences > Plugins** (desktop only) picks a plugin `.zip`.
One confirmation screen shows its name, version, summary, requested permissions and entry placements.
Each command group explains its access in plain words: files, designs, application settings or other plugins.
Enter an absolute local path for each requested folder. Each folder shows its requested read/write access.
When the plugin asks for more memory than the ceiling in **Preferences > Plugins** (128 MiB unless
changed), the screen says how much it asks for and what the ceiling is.
**Install** approves the displayed command groups, folders and memory, publishes the package and stores the
approvals; **Cancel** changes nothing. Installation never runs plugin code. Approved memory applies to
that exact package only: a replaced or changed package asks again. Fuel still has to fit the ceiling.

Programs use the same `plugin.install` command without opening dialogs:

```json
{
  "path": "gears.zip",
  "approval": "COPY_THE_APPROVAL_FROM_THE_REVIEWED_DRY_RUN",
  "approve": {
    "command_groups": ["file"],
    "folders": [{"name": "exports", "path": "/home/user/exports", "read": false, "write": true}],
    "memory_mib": 1024
  }
}
```

Approvals may only narrow the manifest's requests. An unrequested group, folder name or access mode
returns a typed refusal. `memory_mib` is either omitted (the plugin runs under the app's ceiling) or
exactly the manifest's `runtime.limits.memory_mib`.
Omitting `approve` grants no groups, folders or memory, including when replacing a plugin.
Providing `approve` replaces that plugin id's stored approvals; grants the new manifest no longer requests
cannot survive replacement. `plugin.run` checks stored grants against the current manifest on every call.
A package copied into the plugin directory by hand also starts with no grants.

Set `"dry_run": true` to check and describe the zip, including approval validation, without writing
files or grants. The response includes an `approval` hash of the exact manifest and component.
It also includes `archive`, a hash of the whole `.zip`, which covers the plugin's other files too.
Send both as `approval` and `archive` when installing the reviewed package. A changed package returns
a typed `invalid_input` error for `approval` or `archive` before any files or grants are written. The dialog always
sends both hashes; programmatic callers may omit it to install directly without a prior review.
Replacing an installed plugin with the same id requires `"replace": true`.
The response includes name, version, description, capabilities, permissions, requested `limits`, the app's
`host_limits`, app commands,
namespaced `entries`, declared `entry_points` with input schemas and slots, replacement information,
the target `folder` and whether it was `installed`. A dry run reports `installed: false`.
Dry-run description is the package review path; there is no separate review command or folder installer.

A zip holds one plugin, with `plugin.json` and its component at the top or inside one top-level folder.
The existing `solidcraft_io::safe_zip` reader refuses absolute paths, `..`, symlinks and device entries.
The installer limits the archive to 256 entries, each file to 16 MiB (an importer component may be up to 64 MiB), and input and unpacked content to
64 MiB. The manifest has a separate 16 KiB cap. Before publication, the host parses the manifest and
refuses unknown or newer capabilities. It unpacks into a hidden `.install-…` folder beside the installed
plugins, then renames it to `plugins/<id>/`. The entries resolve immediately, with no restart.

## Removing

Each installed plugin card in **Preferences > Plugins** has an enable switch and **Remove** action.
Remove asks for confirmation, then deletes the package folder and its stored grants. Cancelling
Preferences does not restore a removed package. Removing a plugin preserves its data in saved designs.
Approved grants and removal survive closing or resetting Preferences. Grants are host preferences,
never design data; preferences retain at most 256 plugin ids, four command groups and 32 folders per id.
The desktop app keeps them in the plugins folder's grant store too, which the command line and headless
MCP share when they run with `--keep-plugin-approvals` (see [Headless use](#headless-use)). The browser Plugins page remains desktop-only.

The command surface exposes management without opening dialogs:

| Command | Parameters | Result |
|---|---|---|
| `plugin.check` | `{path}`: a package folder or `.zip` | `{ok, id, name, version, kind, approval, entries, problems: [{severity, field, problem, fix}]}`; installs nothing |
| `plugin.install` | `{path, approval?, archive?, replace?, dry_run?, approve?: {command_groups, folders: [{name, path, read, write}], memory_mib?, input_folder?}}` | Package description and installation status, including declared `entry_points` |
| `plugin.list` | `{}` | `{plugins: [{manifest, commands, disabled}]}` |
| `plugin.remove` | `{id}` | `{id, removed: true}` |
| `plugin.grants` | `{}` | `{grants: [{id, status, package_hash, command_groups, folders, memory_mib?, input_folder?}]}` |
| `plugin.revoke` | `{id}` | `{id, revoked}` |
| `plugin.tools` | `{name?}` | `{tools: [{name, kind, plugin, entry, plugin_name, plugin_version, description, effect, extensions, input_schema, approval?}]}`: every installed, enabled plugin's entries as tools for agents (below); `approval` only for the one tool `name` asks for |

All commands expose input and output schemas through `engine.describe`. Removal validates the id
and confines deletion to the host's plugin directory. Installation and removal do not edit the design.
On desktop, send `plugin.install` and `plugin.remove` as separate `engine.execute` requests.
`engine.batch` and `engine.script` refuse these external file operations before any earlier step runs.
Direct requests use a worker to keep the window responsive.

## Headless use

`solidcraft-cli` (`run`, `exec`, `eval` and the other local commands) and `solidcraft-cli mcp --headless`
use the same installed plugins as the desktop app. By default the approvals given in a run last for
that run only. With `--keep-plugin-approvals` they use the same approvals as the desktop app: a
plugin installed and approved in one such run, or in the desktop app, is used by a later one without
installing it again:

```sh
solidcraft-cli exec plugin.install '{"path":"importer.zip","dry_run":true}'      # review: permissions, limits, approval
solidcraft-cli --keep-plugin-approvals exec plugin.install '{"path":"importer.zip","approval":"…","archive":"…","approve":{"memory_mib":2048,"input_folder":true}}'
solidcraft-cli --keep-plugin-approvals run import.json                           # a later run: file.import uses those approvals
```

- **Where they are kept.** Installed plugins are in the per-user plugins folder (`plugins` in
  SolidCraft's data folder: `%LOCALAPPDATA%\SolidCraft`, `~/Library/Application Support/SolidCraft`, or
  on Linux `$XDG_DATA_HOME/solidcraft`, which is `~/.local/share/solidcraft` when `XDG_DATA_HOME` is
  not set), and their approvals in its hidden `.grants` folder, one `<plugin id>.json` record per
  plugin. `--plugins-dir DIR` points a run at another folder: a CI job or a script can keep its own
  plugins, and with `--keep-plugin-approvals` its own approvals, there, apart from the user's. With
  `mcp`, both need `--headless`; a connected desktop app uses its own plugins.
- **What a record allows.** A record holds what was approved at install: command groups, folders,
  memory and `input_folder`, bound to the exact manifest and component reviewed (`package_hash`).
  A changed package, even one whose files were edited in place, runs only after it is installed and
  approved again (`plugin.install` with `replace: true` and `approve`); until then its import or run is
  refused. Nothing is granted implicitly: an install without `approve` stores a record that grants
  nothing, and a plugin with no record (copied in by hand, or revoked) runs with nothing granted. A
  record that does not read, or holds more than an installation could approve, is ignored.
- **Listing and revoking.** `plugin.grants` lists every record with its `status`: `current`, `changed`
  (install and approve again), `not_installed` or `unreadable`. `plugin.revoke {id}` deletes a plugin's
  record and keeps the plugin installed; `plugin.remove {id}` deletes both. Both are commands, so the
  same calls work from `solidcraft-cli exec`, a `run` script, MCP and the desktop app's control channel:

  ```sh
  solidcraft-cli --keep-plugin-approvals exec plugin.grants
  solidcraft-cli --keep-plugin-approvals --plugins-dir ci/plugins exec plugin.revoke '{"id":"dutchman"}'
  ```
- **Several programs at once.** A record is written whole and renamed into place, so a program
  reading the store sees the old record or the new one. A running desktop app or MCP server takes in
  what another program installed, approved or revoked before it next runs or imports with a plugin.
- **Earlier approvals.** The first time the desktop app starts with a grant store, the approvals it
  kept in its preferences move there. Once the store exists it is the only record: a plugin revoked
  from the command line stays revoked in the desktop app.

## Menu and ribbon entries

The **Plugins** group on the Model tab lists every installed plugin entry, including entries without
slots; its label opens the **Plugins** menu. Manage plugins in **Preferences › Plugins** (File › Plugins… opens it): install, turn on or off, and remove. Disabled entries remain listed but cannot be selected. Discovery examines at most 1,024
package folders and 8,192 host slot items. Only enabled plugins contribute host menu and ribbon
items. The host draws each slot item with the plugin's name,
a plugin badge and the entry name. Plugins declare placement in their manifests and supply no
widget, menu or ribbon code.

| Declared slot | Placement (names ignore case) |
|---|---|
| `menu` named `File` or `Help` | The existing File or Help menu; each item includes the plugin's name |
| Any other `menu` name | No host menu item; the entry remains in the Plugins menu |
| `ribbon` named for an existing Design ribbon tab | A group named for the plugin in that tab |
| Any other `ribbon` name | One Plugins ribbon tab, with a group per plugin |

The Design tab names are Model, Surface, Assemble, Insert, Inspect, Manage, View, Sketch,
Sheet Metal and Weldments. Existing tab visibility and workspace rules still apply.
For example, `{"kind":"menu","name":"File"}` places an entry in File;
`{"kind":"ribbon","name":"Model"}` places it in Model.

Clicking a File, Help or ribbon item opens the same input-schema dialog as the Plugins menu.
The host finds the entry by its namespaced `<plugin-id>.<entry>` command. The dialog uses the same
enabled state and grant checks from every placement. Missing command-group grants leave Run off
with an explanation. Opening the dialog executes no plugin code; for a command entry, Run submits the
existing `plugin.run` transaction. An importer entry is not a command: its dialog shows the options,
then its button chooses the file and opens it as a new design with `file.import`, as File > Open does.

## Integration APIs

Preferences and the Plugins menu share `plugin_install::start(app)` in
`crates/ui-egui/src/plugin_install.rs`. It selects a zip, calls `plugin.install` with `dry_run: true`,
draws the confirmation screen and submits `plugin.install` with the reviewed `approval` and `archive` hashes and selected `approve` object.
Successful installation persists grants in desktop preferences and the shared grant store; successful
removal and `plugin.revoke` delete them. `SolveApp::share_plugin_grants`, called once after the
preferences load, starts sharing; `solidcraft_engine::plugin::configure_process_plugins` does the
same for every session of a headless process started with `--keep-plugin-approvals`.
There is one package reader and one installation confirmation screen.

`plugin_menu::list` in `crates/ui-egui/src/plugin_menu.rs` and Preferences use the same package
inventory as slot discovery. The list includes each plugin's enabled state and requested
command groups. File, Help and ribbon clicks resolve their namespaced entry against that list,
then call `plugin_menu::open_entry(app, p, e)`, as the Plugins menu does. Unknown menu names need no
fallback rows because the Plugins menu already lists every entry.

The shared dialog draws the entry's input schema and supplies the package's content approval hash when invoking
`plugin.run`. An entry without fields receives `{}`. Slot clicks alone do not execute a plugin.
Installing, enabling or discovering a plugin never runs an entry. Installation grants do not replace
execution consent or the manifest-and-component identity checked by `plugin.run`.

A string with a fixed set of values becomes a dropdown. With `enum` the dropdown shows the values
themselves; to show words instead, list the options as `oneOf`, each `{"const": value, "title": shown}`
(with an optional `description`). The dropdown shows the titles and the plugin receives the value:

```json
"up_axis": {"type": "string", "title": "Up axis", "default": "as_is", "oneOf": [
  {"const": "as_is", "title": "As in the file"},
  {"const": "stand_up", "title": "Stand up (Y-up to Z-up)"}
]}
```

### Conditional fields, choices and defaults

Command dialogs evaluate declarative conditions on sibling scalar properties. Changing a field
runs no plugin Wasm. The same evaluator checks agent, CLI and script inputs before execution.
For example, a cabinet command can show either shelves or drawers:

```json
"input_schema": {
  "type": "object",
  "required": ["storage", "shelf_count", "drawer_count"],
  "properties": {
    "storage": {
      "type": "string", "title": "Inside", "default": "shelves",
      "oneOf": [
        {"const": "shelves", "title": "Shelves"},
        {"const": "drawers", "title": "Drawers"}
      ]
    },
    "shelf_count": {
      "type": "integer", "title": "Shelves", "default": 3, "minimum": 1,
      "x-solidcraft-visible-when": {"field": "storage", "eq": "shelves"}
    },
    "drawer_count": {
      "type": "integer", "title": "Drawers", "default": 4, "minimum": 1,
      "x-solidcraft-visible-when": {"field": "storage", "eq": "drawers"}
    },
    "has_back": {"type": "boolean", "title": "Back panel", "default": true},
    "back_thickness": {
      "type": "number", "title": "Back thickness", "default": 6.0,
      "x-solidcraft-enabled-when": {"field": "has_back", "eq": true}
    }
  }
}
```

A false `x-solidcraft-visible-when` hides the row and omits its key from Run and live preview,
including when a program supplies that hidden key. A hidden property is not required. A false
`x-solidcraft-enabled-when` leaves the row visible and greyed; its value is kept and sent.

Conditions have exactly one of these forms, with no expression parser:

- `{"field": "storage", "eq": "drawers"}` compares a sibling's scalar value.
- `{"field": "storage", "in": ["shelves", "drawers"]}` matches any listed value.
- `{"all": [condition, ...]}` requires every condition; an empty list is true.
- `{"any": [condition, ...]}` requires at least one condition; an empty list is false.
- `{"not": condition}` reverses a condition.

A missing or hidden controller matches neither `eq` nor `in`. Controllers must be sibling
properties with scalar types (`string`, `boolean`, `number`, `integer` or `null`). Conditions
cannot name nested paths. Missing properties, non-scalar controllers, values outside a
controller's type, enum, `oneOf` or bounds, malformed conditions and dependency cycles are
rejected when loading the plugin. `solidcraft-cli plugin check` reports these errors without
running Wasm. An invalid schema leaves the dialog's Run button off with the existing CLI/agent
note. Conditions are bounded in depth and size.

Hidden-key removal also follows the applicable `oneOf` and `anyOf` object branches, including
nested objects and arrays. A composition containing conditional object rules must have one
branch, or object branches distinguished pairwise by a required, unconditional scalar property
with disjoint `const` or `enum` values. The parent or every branch must declare `"type":"object"`
for these discriminators. This makes the applicable branch unambiguous before hidden keys are
removed. Conditional object rules inside ambiguous compositions or `allOf` are rejected
at load time with an `invalid_manifest` error naming the composition's schema location. Scalar
choice options using `x-solidcraft-when` remain supported. The 1,024-property condition limit
applies only to object scopes declaring conditional rules; plain object scopes skip that limit.

All applicable scopes share the same visible controller values. When any scope hides a
controller, conditions in every scope see it as missing. Required checks and choice filters
use that same view. Loading checks visibility dependency cycles across each combination of
enclosing scopes and selected branches, including nested objects and array items. Mutually
exclusive branches are checked separately.

Put `x-solidcraft-when` on a `oneOf` option to offer it only when its condition is true. If the
current choice becomes unavailable, the dialog selects a matching conditional default, or the
first available option in schema order. With no available option, the choice is empty; a required
choice keeps Run off. A program supplying an unavailable choice gets an `invalid_input` error
naming that field.

Conditional defaults use the first matching rule in `x-solidcraft-defaults`. The dialog applies
it on opening and when a controlling value changes, until the user edits that field. Custom
values survive later controller changes, except that unavailable choices must still reset.
Programs provide their own values; conditional defaults do not overwrite program inputs.
Command inputs and importer options with these annotations opt into validation and hidden-key
removal before direct calls.
Direct calls to schemas without conditional annotations keep their existing pass-through behaviour.
For example:

```json
"units": {"type": "string", "enum": ["mm", "in"], "default": "mm"},
"size": {
  "type": "string", "default": "metric", "oneOf": [
    {"const": "metric", "title": "M6", "x-solidcraft-when": {"field": "units", "eq": "mm"}},
    {"const": "imperial", "title": "1/4 inch", "x-solidcraft-when": {"field": "units", "eq": "in"}}
  ]
},
"clearance": {
  "type": "number", "default": 6.5,
  "x-solidcraft-defaults": [
    {"when": {"field": "units", "eq": "mm"}, "value": 6.5},
    {"when": {"field": "units", "eq": "in"}, "value": 0.25}
  ]
}
```

These numeric defaults are plugin arguments: the plugin decides how to interpret the selected
units. Every conditional default must satisfy the property's base schema. Defaults and option
filters also participate in cycle detection.

### Selection fields

A field can take geometry picked in the viewport, with the same pick control as Extrude's
Profiles and Chamfer's Edges. Mark an array property with `x-solidcraft-selection`; the property's
key is the argument the plugin receives. The design follows the
[native selection proposal](https://github.com/tangency-labs/solidcraft/pull/16281)
([#16274](https://github.com/tangency-labs/solidcraft/issues/16274)).

```json
"edges": {"type": "array", "title": "Edges", "description": "The rims to seat", "minItems": 1,
          "x-solidcraft-selection": {"version": 1, "accept": ["edge"]}},
"body":  {"type": "array", "title": "Body to modify", "maxItems": 1,
          "x-solidcraft-selection": {"version": 1, "accept": ["body"]}}
```

- **What it takes.** `accept` lists one or more of `profile`, `edge`, `face`, `body`,
  `sketch_point` and `sketch_curve`. Sketch points and curves are picked with their sketch, because
  ids repeat from one sketch to the next. A profile field takes profiles from any number of
  sketches; the plugin decides what it accepts. The annotation has exactly `version` (1) and `accept`.
  An unknown version or category, or any other keyword on the property (`uniqueItems`, a detailed
  `items` schema), leaves Run off with the note to run the entry from the agent or the CLI.
- **How many.** `minItems` and `maxItems` are the counts Run checks. With `maxItems: 1` a pick
  replaces the last one; otherwise a click adds an item, and clicking it again takes it out.
  `required` with `minItems` of 1 or more needs a pick; required with no minimum sends `[]`;
  an optional field with nothing picked is left out.
- **What the plugin receives.** Each item is the app's selection reference, as `select.set`
  takes it: `{"type": "edge", "body": "Body1", "index": 3, "point": [x, y, z]}`,
  `{"type": "face", ...}`, `{"type": "body", "name": "Body1"}`,
  `{"type": "profile", "sketch": 2, "index": 0}`,
  `{"type": "external_sketch_point", "sketch": 2, "id": "p3"}` and
  `{"type": "external_sketch_curve", "sketch": 2, "id": "c1"}`, in pick order. `items`, if given,
  is only `{"type": "object"}`.
- **Reserved lists.** Without the annotation, `bodies` and `body_ids` take bodies and send their
  names (`items` may be `{"type": "string"}`), and `selection`, `target` and `targets` take edges,
  faces and bodies as references. Any other array still leaves Run off.
- **Picking.** The first selection field takes the viewport's picks when the dialog opens, and
  starts with what is selected that it takes. The other fields start empty, so one selection never
  fills two fields. Clicking a field makes it the one that picks; its clear button, its list of
  picks and the hover highlight work as in the native tools. One tool picks at a time: while a
  native tool is open, clicking a plugin field says to finish or cancel it first (a sketch tool too).
  Escape and Cancel close the dialog and stop the picking.
- **References stay true.** The picks belong to the design they were made on. Any change to the
  design while the dialog is open (an edit, an undo) clears them and the dialog says so; turning
  the view or changing the selection does not. Picking runs no plugin code; Run sends the picks
  through the same `plugin.run` request. Programs calling `plugin.run` pass the same references
  themselves; nothing fills them from the selection.

### Seating a custom fastener

To fit a custom fastener into an existing bore, accept `face` and `edge` picks in a selection
field. Call `parts.seats` with those references in `picks` before creating hardware. Each seat
supplies a world-space point and outward normal, and an `origin` that can be passed directly
as side A of `joint.create`. Place the screw with its head facing outward, then use a rigid
joint to attach its seating point to the bore. Declare both commands in the plugin manifest.

### Icons and pictures

A plugin can ship vector images in its package and name them with `x-solidcraft-icon`, a
relative path to an `.svg` file inside the package:

```json
"input_schema": {"type": "object", "x-solidcraft-icon": "icons/seat.svg", "properties": {
  "series": {"type": "string", "title": "Type", "default": "standard", "oneOf": [
    {"const": "standard", "title": "Standard", "x-solidcraft-icon": "icons/standard.svg"},
    {"const": "short", "title": "Short", "x-solidcraft-icon": "icons/short.svg"}
  ]}}}
```

- At the top of the schema it is the command's icon: its ribbon button, its rows in the Plugins,
  File and Help menus, and its dialog's header. Without one the command has the generic plugin icon.
- On a `oneOf` option it is that option's picture in the dropdown. Pictures show when every option
  of the choice names one that passed the check below; otherwise the dropdown shows the titles alone.
- The image is drawn like the app's own icons, and must be one: SolidCraft's icon SVG subset
  (the check the built-in icons pass, see `crates/look/icons/design/tools/lint_icons.py`).
  `viewBox="0 0 64 64"`; absolute `M L C Q A Z` paths, circles, groups with a clip or opacity,
  masks and clip paths. Every colour is one of the icon palette's role colours
  (`crates/look/icons/design/palette.toml`, the `source` values) or `currentColor`, so the image
  follows the light and dark themes, the accent and the colour-vision setting as the app's icons do.
  Text, raster images, `<use>`, links, scripts, styles and any other element or colour are refused.
- The path uses forward slashes and plain names (letters, digits, `.`, `_`, `-`), ends in `.svg`
  and stays inside the package: no `..`, no absolute path, no link. A file is at most 64 KiB and a
  package names at most 64 images. Images are read from the installed package when the plugins are
  listed; nothing is fetched from the network. A refused or missing image is logged and left out,
  and the command works as before.
- Images are not part of the `approval` hash: changing one changes only what is drawn.

### View commands

After Run commits the result, the app applies the run's last view command. `view.fit`
frames the result, keeping the view direction; `view.home` frames it from the home direction.
A run with no view command leaves the camera unchanged.

### Preview

A command entry that edits the design is previewed the way native tools are: while its dialog's
fields make parameters, the host runs the entry on a scratch copy of the design and draws what
it would change in the viewport (new material tinted, removed material as a see-through ghost).
The dialog has a **Preview** box, on when it opens; with it off, nothing runs until Run.

- The preview is a dry run of the same `plugin.run`: the plugin's code is unchanged and declares
  nothing new. It runs on a scratch session that is thrown away, so the design, the undo history
  and the selection never change. The run has none of the plugin's folder or command-group grants,
  so it reads and writes no file and reaches nothing past the design; a plugin that needs them
  shows "Preview:" with the refusal under its fields, and Run still runs it with its grants.
- It runs again only when the parameters or the design change, under the plugin's own fuel,
  memory and deadline limits, off the window's thread. A run left behind by newer inputs is
  dropped.
- When the first successful preview is drawn, the app applies that run's last view command once
  if it is `view.fit` or `view.home`. Fit includes previewed sketch geometry when there are no
  bodies; even an empty result consumes this first-preview action. Later field changes leave the
  camera alone, so you can move it freely. A run with no view command leaves the camera unchanged.
  Reopening the dialog starts fresh: its first preview applies its view command again.
- The most recently opened dialog on the design is previewed (the one picking in the viewport,
  when one is). While a native tool is open, its own preview is the one shown. Read entries and
  importers are not previewed.

## Versions

The WIT package is `craft:plugin@0.2.0`. Its capability `commands@0.2` runs app commands, logs,
reports progress and cancellation, reads named files and a clock, and exports the entry. A plugin's
`plugin.json` lists what it needs, for example `"capabilities": ["commands@0.2"]`; the host refuses
an unknown or newer capability when it reads the manifest, with a plain message such as "needs
commands 0.3; update SolidCraft or the plugin". `formats@0.2` adds importers, `evaluate@0.2`
lets an importer check its result, and `storage@0.1` keeps data in a design (below). `ui` and
`viewport` arrive later as their own
versioned interfaces. While a version is 0.x, a minor version may break the one before it.

0.2 added one error, `plugin-error.unsupported(string)`: the input is valid, but this plugin does
not handle it, such as a file with no solids or a newer version of its format. Use it instead of
`invalid-input`, which means the input is broken; SolidCraft then says "Sample does not support
bracket.smp: ..." instead of an "invalid" file. Plugins built against 0.1 (`commands@0.1`,
`formats@0.1`, `evaluate@0.1`) still run unchanged: the host reads the WIT version from the
component's own imports and exports, and serves the frozen 0.1 WIT
(`crates/craft-plugin/wit-0.1/plugin.wit`) to those. A component that names a version the host does
not run, or two versions at once, is refused before any of its code runs.

## Commands and permissions

A plugin can call every command in the MCP catalog that its manifest lists. Commands that reach
past the open design are grouped (`file`, `doc`, `app`, `plugin`) and refused unless the
manifest asks for the group in `permissions.command_groups` and the user grants it.
File access requires approved folder grants. The reserved `net` permission is
refused with a typed error. Traps, missing permissions, failed commands, limits
and invalid manifest fields return structured errors (see Errors below). Phase 2 adds importers (below) and the
author tools (`plugin check` and `plugin test`, below); exporters and browser execution follow.

## Plugins as tools for agents

Every installed, enabled plugin's entries are tools for the assistant chat and for MCP clients,
beside the built-in tools, so an agent can find and use what a plugin offers without opening it
([RFC 0002](rfcs/0002-plugins.md) section 6c).

- **Named tools.** A command entry is the tool `<plugin>.<entry>`, with its input schema as the
  tool's arguments. An importer entry is the tool `<plugin>.<entry>` titled "Open .ext files with
  Plugin name", taking `path` and the importer's `options`; it opens the file as a new design through
  `file.import`. Export slots get their tools when exporters land.
- **Fresh every listing.** `tools/list` asks `plugin.tools` each time, so a plugin turned off or
  removed is gone from the next listing, and calling its tool is an unknown tool.
- **Strict arguments.** MCP tool arguments are checked against the entry's declared schema:
  declared types, required fields, fixed values and ranges. Object schemas accept only declared
  fields unless they allow others with `additionalProperties` or use unsupported schema keywords.
  Composition options do not implicitly close objects. Nothing runs when the arguments don't fit.
  Existing `<plugin>.<entry>` engine calls keep passing inputs through to the plugin unchanged.
- **Headless, under existing grants.** A tool call opens none of the plugin's dialogs and runs
  with the grants it was given at install, as one undo step, like any command.
- **A `read` entry stays read-only.** Whoever runs it, a person or a tool call, the host refuses it
  edit commands and the file-writing host import `write-file`, whatever folders the plugin was
  granted. The refusal names the import (or command).
- **A missing grant is an error, never a prompt.** A call that needs a grant the plugin lacks
  (a folder, a command group, more memory than the ceiling, or approval of a changed package)
  fails with an error that names the plugin, the grant and how to approve it: in Preferences ›
  Plugins by installing the plugin again and allowing it, or headlessly with `plugin.install` and
  its `approve` object. Over MCP the result also carries it as data,
  `structuredContent.plugin_error`: `{plugin, grant, approve}`, or `{plugin, entry, refused_import}`
  and `{plugin, entry, refused_command}` for a read entry.
- **Marked as plugin tools.** Each definition has a `plugin` field
  (`{id, name, version, entry, kind, effect}`), and its description starts "Plugin tool" and puts
  the plugin's own description between `<plugin-text>` and `</plugin-text>`, as text to read, not
  instructions to follow.
- **The assistant asks first.** In the assistant chat a plugin tool call goes through the same
  approval as every other tool call, and it always asks, whatever the permission mode or an earlier
  **Always allow**: running a plugin's code needs your go each time. An importer tool, and any
`file.import` or `doc.open` that would run an importer plugin, asks the same way.

- **Described like commands.** `engine.describe` (MCP `describe_command`) of `<plugin>.<entry>`
  returns the entry with the same schema as `tools/list` (`tool.input`): a command entry as the
  `<plugin>.<entry>` command (`approval` and `params`), an importer as the tool that opens its files,
  with `file.import` named.

Installed plugins are read with the app's own authority (a tool names a plugin, never a path), so
an agent confined to its folders (only the folder of its input file, say) still sees them in
`plugin.list`, `plugin.tools` and `engine.describe`, and `doc.open` and `file.import` still find an
installed importer. The input file itself must be in the agent's folders; when it isn't, the
refusal names it. The files a plugin reads or writes stay within its grants and the agent's folders.

## Checking and testing a plugin

[Writing a plugin](book/src/dev/plugins.md) walks from an empty folder to a working command
plugin and importer.

- `plugin.check` (`solidcraft-cli plugin check <folder|plugin.zip>`) checks a package without
  running it, through the code Install and a call use: the installer's reader and limits, the
  manifest, the permissions (`net` is refused), the limits against the app's ceilings, and the
  component against its world (`command-plugin` or `importer-plugin`) and this app's WIT version,
  with no WASI or network import. Each problem has a severity, the field, what is wrong and how to
  fix it. In a folder, `tests/` and hidden files are left out. It runs no plugin code, installs
  nothing and never opens a dialog.
- `solidcraft-cli plugin test <folder|plugin.zip> [--json] [--out DIR] [--require-cases]` checks
  the package, installs it into a scratch plugins folder with the app's installer, and runs its
  test cases, `tests/*.json`, through `plugin.run` and `file.import` with the app's limits. A
  folder's cases are in its `tests/` folder; a `.zip`'s are the `tests/` folder it holds, so a
  `.zip` made without one has none. A case names the entry, its parameters or file, the grants
  (default: everything the manifest asks for, stored in the scratch folder's grant store as an
  install stores them, see [Headless use](#headless-use)), and what to expect: an error kind,
  values in the reply, values `inspect.measure` gives, files written. An entry with no case runs
  once with its defaults when it needs nothing else, and is reported as skipped otherwise. It exits with an
  error when a case fails, and with `--require-cases` also when an entry is skipped, in a folder
  or a `.zip` alike: use it in CI so an untested entry fails the run. A `tests/` folder that
  cannot be read, a grant the manifest does not ask for, and a fixture folder (`from`) that is a
  link fail the case or the run; nothing is skipped silently.

## Importers

An importer turns a file of another format into a SolidCraft design. Its manifest says
`"capabilities": ["formats@0.2"]`, and each of its entries has `"kind": "importer"` and the
extensions it claims, lowercase without the dot. An importer's effect is `open`: it opens a file
as a new design and leaves the open one as it is. It need not say so; `plugin.list` and
`plugin.install` report it, and an importer that says `"effect": "edit"` is still read as `open`.
Only an importer may say `open`.

```json
{"id": "sample", "name": "Sample", "version": "1.0.0", "capabilities": ["formats@0.2"],
 "runtime": {"module": "sample.wasm", "commands": [],
   "entries": [{"name": "convert", "kind": "importer", "extensions": ["smp"],
                "input_schema": {"type": "object", "properties": {"units": {"type": "string", "enum": ["mm", "in"], "default": "mm"}}}}],
   "limits": {"fuel": 100000000000, "memory_mib": 1024, "deadline_ms": 300000, "stack_kib": 16384},
   "permissions": {"input_folder": true}}}
```

- **Opening.** `doc.open` opens a file SolidCraft does not read itself with the installed, enabled
  importer that claims its extension. `file.import` always uses an importer, and `format`
  (`plugin_id.entry`, listed in `plugin.list`'s `importers`) picks one for either command, for
  example for a `.fcstd` file SolidCraft also reads. `options` passes the entry's declared options.
  Two importers claiming one extension need `format`. File › Open offers the claimed extensions,
  and the Plugins menu has each importer's entry: its dialog shows the options, then chooses the file.
- **The world.** The component implements `importer-plugin`: it exports
  `importer.import-file(entry, file, options)` and imports `source` (the file, read in chunks of at
  most 1 MiB), `design` (where it writes a SolidCraft design file), `log`, `control` and `clock`.
  It has no `commands` import: an importer makes a design, it does not edit the open one.
- **The result.** The design opens as a new design that is not saved anywhere yet, named after
  the file unless the design names itself. The call returns a loss report,
  `{"summary": "...", "items": [{"severity": "info|degraded|dropped", "title": "...", "subject": "...", "message": "...", "code": "..."}], "details": {...}}`.
  `title` names the kind of loss in the user's words, the same for every item of that kind
  ("Parts of the file kept as they are"); `code` is the importer's stable code for it. Both may be empty.
  The Import report window shows the summary, then one line per title with how many items it covers,
  worst first, and folds every item with its code, subject and message under **Details**; programs
  get the whole report in the reply. `details` is the importer's own JSON (for example how far it
  checked the history it rebuilt, and each feature it mapped or could not). The window shows it
  under **From the importer** as folding lines: each field a line with its key read as words
  (`final_body` reads "Final body"), each object or list a group, a list's element named by its
  `name` (or `title`, `label`, `subject`, `stage`, `id`, `kind` or `code`) and its `status`, and
  empty values left out. **Copy report** puts the whole report on the clipboard as text, and
  **Save report…** saves it as a `.txt` file, or as a `.json` file in the shape of the reply.
  After OK, Plugins › Last import report… shows the last import's report again.
  Nothing changes unless the whole import succeeds.
- **Assemblies.** `source.read`, `source.size` and `source.resolve` with path `""` reach the file
  being imported. Any other path is relative to that file's folder (an assembly's parts) and needs
  the `input_folder` permission. The install review shows it and Install approves it, stored with
  the plugin's other grants for that exact package; a plugin copied in by hand, or one whose grant
  is for another package, reads only the file itself. Hidden names, links and paths
  outside the folder are refused; an import reads at most 1 GiB and writes a design of at most 256 MiB.
- **Checking the result** (capability `evaluate@0.2`, beside `formats@0.2`). A converter that
  rebuilds a design's history can check it the way it would with the app's command line: it stages
  a design file it made with `evaluate.stage` (1 MiB per call, 256 MiB in all), and `evaluate.run`
  opens that design in a private session, apart from every open design and every file, and runs one
  of `document.inspect`, `inspect.measure`, `model.edges`, `model.faces` or `timeline.roll_to` on
  it, returning the command's JSON reply (or `{"error": "..."}` when the command fails). Staging
  again starts a new design; `evaluate.reset` discards it, and nothing outlives the import. The
  session reads no files, not even fonts or linked designs, and runs no command that writes; its
  time counts against the import's deadline, and Cancel stops the import between commands. A
  design the app can't open, a command outside the list or a reply over 1 MiB is an error for that
  call only, so the importer can report its history as unchecked instead of failing.
- **Failures.** A failure fails that one import, named with the plugin and the file ("Sample
  could not open bracket.smp: ...", or "Sample does not support bracket.smp: ..." for
  `unsupported`), and the next one gets a fresh instance. The compiled component stays in memory, so the next file opens
  without compiling again; nothing compiled is written to disk.
- **Limits.** An importer's component may be up to 64 MiB, its deadline up to 10 minutes, and its
  `stack_kib` up to 64 MiB (a command may ask for at most 1 MiB). Its fuel has to fit the user's ceiling in Preferences › Plugins, and
  its memory too unless the user approved more at install. Build it for `wasm32-unknown-unknown` and wrap it with
  `wasm-tools component new`: the host links no WASI, so a component that imports any is refused.

## Errors

Every plugin failure starts with the plugin's name, and an importer's with the file's name too.
Programs get the structured error (`kind` and its fields) from the engine as well as the text. The
text comes from the message catalog (`plugin_error.*`) in the window's language; the command line
and MCP give the English.

| What happened | What the user reads |
|---|---|
| `unsupported` from the plugin | Sample does not support sketch.smp: the model has no solids |
| `invalid-input` from the plugin | Sample could not open broken.smp: (its reason) |
| More memory than plugins may use | Sample could not open big.smp: it asks for 2048 MiB of memory, and plugins may use 128 MiB. Install the plugin again and allow its memory request, or raise Max plugin memory (MiB) in Preferences › Plugins (in solidcraft-cli, max_memory_mib in the --plugin-limits file). |
| More fuel than plugins may use | ... it asks for N fuel, and plugins may use M. Raise Max plugin fuel in Preferences › Plugins (in solidcraft-cli, max_fuel in the --plugin-limits file). |
| It ran out of fuel, memory, stack or time | ... it used all N fuel it asks for and was stopped. Ask its author to raise its fuel limit. (memory in MiB, stack in KiB, time in seconds) |
| Its design is over 256 MiB | ... the design it made is larger than 256 MiB, the most a plugin can hand over. |
| A trap | ... it stopped on an error inside the plugin (details). Report this to its author. |

`plugin.inspect` words `limits_error` the same way. The structured kinds are `unsupported`
(`reason`), `over_ceiling` (`limit` is `fuel` or `memory_mib`, with `requested` and `allowed`) and
`exhausted` (`limit` is `fuel`, `memory_mib`, `stack_kib`, `deadline_ms` or `design_mib`, with
`allowed`). The engine's own numbers for a stopped run (fuel used and left) go to the debug log.

## Plugin data in a design

A design container reserves `plugins/<plugin-id>/` for a plugin's own files. They are kept and
saved back untouched when the plugin is missing, and opening a design never fails because a plugin
is missing.

A command plugin keeps its own data there with the `storage@0.1` capability and a data format
number. Its manifest lists the `storage.*` commands it calls, like any other command:

```json
{"id": "gears", "name": "Gears", "version": "1.4.0", "capabilities": ["commands@0.1", "storage@0.1"],
 "runtime": {"module": "gears.wasm", "entries": ["make"], "data_format": 3,
   "commands": ["solid.box", "storage.get", "storage.patch", "storage.put_blob"]}}
```

- **What it keeps.** One main JSON document (`data.json`, at most 16 MiB) and files of its own
  (`blobs/<name>`; with `cache: true`, `cache/<name>`, files it can make again). Files are at most
  256 MiB per plugin per design, and all plugin data in a design at most 256 MiB.
  SolidCraft writes `meta.json` beside them: the plugin's id, name and version and the data's format.
- **Reading.** `storage.get` returns the main document, the data's `format`, the running plugin's
  `plugin_format`, `read_only` and the list of files. To read a large main document within the
  plugin host's 1 MiB result limit, supply `offset` or `len`: `storage.get {offset: 0}` returns
  `{plugin, data, offset, bytes}`, with `data` holding base64 of up to 512 KiB of the saved JSON
  bytes and `bytes` the whole document size. Advance `offset` by the number of decoded bytes,
  concatenate the decoded parts, then parse the JSON. `len` defaults to 524288 and accepts
  1 through 524288; `offset` defaults to 0. Parts omit metadata and the file list. Past the end,
  or without a saved document, `data` is empty (`bytes` is 0 without a document). Documents
  still accept up to 16 MiB; plugins should use parts when the full reply could exceed 1 MiB.
  `storage.get_blob {name, offset?, len?}`
  returns up to 512 KiB of a file, base64, so a reply stays inside a plugin's 1 MiB. Inside a
  plugin's run these reach only that plugin's own data. Agents and scripts read any plugin's
  data by naming it: `storage.get {"plugin": "gears"}`.
- **Writing.** `storage.set {document}`, `storage.patch {patch}` (an RFC 7396 merge patch),
  `storage.put_blob {name, data, cache?, append?}` (base64; `append` writes a large file in
  parts) and `storage.delete_blob {name}` change only the running plugin's own data, and only
  from its run: the whole run is one undo step, and a failed run changes nothing. Called from
  anywhere else they are refused. A `read` entry can't call them.
- **Formats.** Every write saves the data in the plugin's `data_format`. When `storage.get` says
  `format` is lower than `plugin_format`, the data is from an older version of the plugin: the
  plugin reads it, converts it and writes it back, which saves it in the new format as one undo
  step. When `format` is higher, the data is from a newer version: this version reads it, and
  every write is refused with "saved by a newer version of the plugin".
- **Without the plugin.** The design opens as usual and the status line says so: "Contains Gears
  data (plugin 1.4.0). Install it to edit." (or turn it on, or update it, when it's off or older
  than the data). The data is saved back byte for byte. Nothing is downloaded, installed or run.
  `storage.list` lists every plugin that keeps data in the open design, with its `status`
  (`ready`, `migrate`, `missing`, `disabled` or `older`) and size.
