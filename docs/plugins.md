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
| `plugin.install` | `{path, approval?, archive?, replace?, dry_run?, approve?: {command_groups, folders: [{name, path, read, write}], memory_mib?, input_folder?}}` | Package description and installation status, including declared `entry_points` |
| `plugin.list` | `{}` | `{plugins: [{manifest, commands, disabled}]}` |
| `plugin.remove` | `{id}` | `{id, removed: true}` |

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
and invalid manifest fields return structured errors. Phase 2 adds importers (below);
exporters, author tooling and browser execution follow.

## Importers

An importer turns a file of another format into a SolidCraft design. Its manifest says
`"capabilities": ["formats@0.1"]`, and each of its entries has `"kind": "importer"` and the
extensions it claims, lowercase without the dot:

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
  get the whole report in the reply.
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
