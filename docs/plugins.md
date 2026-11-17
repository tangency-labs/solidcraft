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
The browser Plugins page remains desktop-only.

The command surface exposes management without opening dialogs:

| Command | Parameters | Result |
|---|---|---|
| `plugin.check` | `{path}`: a package folder or `.zip` | `{ok, id, name, version, kind, approval, entries, problems: [{severity, field, problem, fix}]}`; installs nothing |
| `plugin.install` | `{path, approval?, archive?, replace?, dry_run?, approve?: {command_groups, folders: [{name, path, read, write}], memory_mib?, input_folder?}}` | Package description and installation status, including declared `entry_points` |
| `plugin.list` | `{}` | `{plugins: [{manifest, commands, disabled}]}` |
| `plugin.remove` | `{id}` | `{id, removed: true}` |
| `plugin.tools` | `{name?}` | `{tools: [{name, kind, plugin, entry, plugin_name, plugin_version, description, effect, extensions, input_schema, approval?}]}`: every installed, enabled plugin's entries as tools for agents (below); `approval` only for the one tool `name` asks for |

All commands expose input and output schemas through `engine.describe`. Removal validates the id
and confines deletion to the host's plugin directory. Installation and removal do not edit the design.
On desktop, send `plugin.install` and `plugin.remove` as separate `engine.execute` requests.
`engine.batch` and `engine.script` refuse these external file operations before any earlier step runs.
Direct requests use a worker to keep the window responsive.

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
Successful installation persists grants in desktop preferences; successful removal deletes them.
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
- The most recently opened dialog on the design is previewed (the one picking in the viewport,
  when one is). While a native tool is open, its own preview is the one shown. Read entries and
  importers are not previewed.

## Versions

The WIT package is `craft:plugin@0.1.0`. Today's surface is one capability, `commands@0.1`:
running app commands, logging, progress and cancellation, named files, a clock, and the exported
entry. A plugin's `plugin.json` lists what it needs, for example
`"capabilities": ["commands@0.1"]`; the host refuses an unknown or newer capability when it reads
the manifest, with a plain message such as "needs commands 0.2; update SolidCraft or the plugin".
`formats@0.1` adds importers (below). `ui`, `viewport` and `storage` arrive later as their own
versioned interfaces. While a version is 0.x, a minor version may break the one before it.

## Commands and permissions

A plugin can call every command in the MCP catalog that its manifest lists. Commands that reach
past the open design are grouped (`file`, `doc`, `app`, `plugin`) and refused unless the
manifest asks for the group in `permissions.command_groups` and the user grants it.
File access requires approved folder grants. The reserved `net` permission is
refused with a typed error. Traps, missing permissions, failed commands, limits
and invalid manifest fields return structured errors. Phase 2 adds importers (below) and the
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
- **Strict arguments.** A tool's arguments, and the inputs of any `<plugin>.<entry>` call, are
  checked against the entry's declared schema as strictly as an app command's parameters: the
  declared types, required fields, fixed values and ranges, and no field it does not declare
  (unless the schema allows others with `additionalProperties`). Nothing runs when they don't fit.
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
  **Always allow**: running a plugin's code needs your go each time.

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
- `solidcraft-cli plugin test <folder|plugin.zip> [--json] [--out DIR]` checks the package, installs
  it into a scratch plugins folder with the app's installer, and runs its test cases, `tests/*.json`,
  through `plugin.run` and `file.import` with the app's limits. A case names the entry, its
  parameters or file, the grants (default: everything the manifest asks for), and what to expect:
  an error kind, values in the reply, values `inspect.measure` gives, files written. An entry
  with no case runs once with its defaults when it needs nothing else. It exits with an error when
  a case fails.

## Importers

An importer turns a file of another format into a SolidCraft design. Its manifest says
`"capabilities": ["formats@0.1"]`, and each of its entries has `"kind": "importer"` and the
extensions it claims, lowercase without the dot. An importer's effect is `open`: it opens a file
as a new design and leaves the open one as it is. It need not say so; `plugin.list` and
`plugin.install` report it, and an importer that says `"effect": "edit"` is still read as `open`.
Only an importer may say `open`.

```json
{"id": "sample", "name": "Sample", "version": "1.0.0", "capabilities": ["formats@0.1"],
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
- **Checking the result** (capability `evaluate@0.1`, beside `formats@0.1`). A converter that
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
- **Failures.** A trap fails that one import ("Sample failed on bracket.smp: ...") and the next
  one gets a fresh instance. The compiled component stays in memory, so the next file opens
  without compiling again; nothing compiled is written to disk.
- **Limits.** An importer's component may be up to 64 MiB, its deadline up to 10 minutes, and its
  `stack_kib` up to 64 MiB (a command may ask for at most 1 MiB). Its fuel has to fit the user's ceiling in Preferences › Plugins, and
  its memory too unless the user approved more at install. Build it for `wasm32-unknown-unknown` and wrap it with
  `wasm-tools component new`: the host links no WASI, so a component that imports any is refused.

## Plugin data in a design

A design container reserves `plugins/<plugin-id>/` for a plugin's own files. They are kept and
saved back untouched when the plugin is missing, and opening a design never fails because a plugin
is missing. The `storage` capability will let plugins read and write them.
