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

## Design revisions and notifications

Declare `design@0.1` alongside `commands@0.2` to import the optional
`craft:plugin/design-state@0.2.0` interface. Its read-only `get-token()` returns
JSON `{ "document_id": "…", "revision": "0123456789abcdef0123456789abcdef:42" }`,
or `null` without an open design.
Existing plugins need no new imports or capabilities.

The host keeps the identity and revision in the open document's session, outside
the authored document, saved files and undo snapshots. Every committed edit,
undo and redo gets a fresh token, including a redo that restores earlier content.
Tokens never identify different history states. Saving does not change the token.
Opening or importing a design creates a fresh identity and token, including when
reopening the same file; switching between open tabs retains each tab's identity.
Different open documents have different identities. Display-only changes do not
invalidate calculations. These observations add no metadata to saved designs and
do not alter the authored bytes restored by cancellation or undo.

Both fields are bounded opaque strings. Compare the complete token for equality
and pass it back unchanged. A token is valid only for its open document session;
discard cached tokens after closing or reopening a design. Revisions do not order
edits across sessions.
Use this token for edit preconditions; `document.revision` reports the separate
observation revision, which also follows visibility and uncaptured positions.

An editing call may pass `expected_design` beside `entry` and `params` to
`plugin.run`, or beside `approval` and `params` to `<plugin>.<entry>`:

```json
{
  "path": "/plugins/example",
  "approval": "PACKAGE_HASH",
  "entry": "apply",
  "expected_design": {
    "document_id": "DESIGN_ID",
    "revision": "0123456789abcdef0123456789abcdef:42"
  },
  "params": {}
}
```

The host checks the token inside the edit transaction before invoking the guest.
A mismatch refuses the entire edit with typed `stale_revision` data containing
both `expected` and `actual` tokens. No history step is added. Omitting the token
preserves unconditional calls.
MCP clients can supply this precondition through `execute` with the wrapped
command arguments. Direct plugin tools keep the plugin's declared input schema.

A command plugin can declare invalidation listeners under the same capability:

```json
"listeners": [{"entry": "refresh", "on": ["design.changed"]}]
```

Place this in `runtime`; `refresh` must name a declared command entry with
`"effect": "read"`. Approval covers the exact declaration and component. Disabled,
unapproved or replaced packages receive no background calls.

The window sends whole-document invalidations after finished host edits,
undo/redo, and active-document switches, opens and closes. A burst is coalesced
into the latest state, at most about ten deliveries a second. The entry receives
`{"design":{"document_id":"…","revision":"0123456789abcdef0123456789abcdef:42"}}`,
or `{"design":null}` without an active design. Closing the window's last tab
creates a blank design, so that notification names the new active design.
Events carry no file contents and are not an edit log.

A listener can also opt in to linked-file observations:

```json
"listeners": [{"entry": "refresh", "on": ["design.changed", "design.links_changed"]}]
```

`on` must be a nonempty list of distinct events from `design.changed` and
`design.links_changed`; either event can be declared alone. Both use the existing
`design@0.1` capability and read-only command entries. No WIT world or interface
changes are needed. The separate event preserves the exact payload and delivery
conditions of existing `design.changed` listeners.

When a declared `design.links_changed` event is pending, the entry receives:

```json
{
  "design": {"document_id": "…", "revision": "0123456789abcdef0123456789abcdef:42"},
  "links": [{"component": 7, "name": "Bracket", "path": "/parts/bracket.sc", "read": false}]
}
```

`design` is the active design's token, or `null` without an active design.
Each link names the numeric component id, its name (`null` if absent) and absolute
source path. `read: false` means a `component.links` check found a file differing
from the version the design has read. `read: true` means the design already read that version, including after
Update Links (`component.update_links`) or Relink Source (`component.relink`)
successfully read the changed source into the design. A disk-only observation does
not advance the design token or trigger `design.changed`.
Unread (`read: false`) entries require a source inside the plugin's approved read folders; `read: true` entries remain visible without that grant.

The host observes files only when it already checks links, including the window's
focus check, and after Update Links; it adds no filesystem watcher. A burst retains
the latest observation per component, so this is an invalidation, not a history of
every disk change. A listener declaring both events gets one invocation for the
coalesced state, including `links` only when a linked-file observation is pending.
A design-only invalidation still has exactly the `{"design":…}` payload. A listener
declaring only `design.links_changed` receives no design-only invalidations, and a
listener declaring only `design.changed` never receives `links` or disk-only
notifications.

Handlers run on the command worker, away from the window thread, with their own
budget: at most 1,000,000 fuel, 16 MiB memory, 512 KiB stack and 100 ms execution time, further
limited by their requested budgets. They may refresh caches.
Editing, file writes and starting jobs are refused with existing typed permission
errors. A failure is shown in Preferences › Plugins and never fails the action
that triggered the notification. Each invocation uses a fresh guest instance.
Handler results use the existing 1 MiB JSON reply limit; retained panel data is
limited to 16 MiB in total. Exceeding either limit reports a typed limit error.

A handler cannot refresh its plugin's docked panel yet. It runs on a scratch copy of the
session, so a `ui.set-data` call it makes does not reach the open panel; only the handler's JSON
reply is kept (up to the limits above), and no panel reads it. Until the panel contract lands
(#19516, phase 3 of RFC 0015), give the panel a Refresh button whose entry reads the design and
calls `ui.set-data`, as the entries a panel runs do (see Docked panels below).

## Classifying points and reading surface normals

A plugin entry with `"effect": "read"` can call `inspect.classify_points` through
the `commands.run` WIT import, without a command-group grant. MCP callers use the existing `execute`
tool with the same command and parameters:

```json
{"body":"Block","points":[[5,5,5],[10,23,20]],"face":"F2:bore"}
```

`body` is the name used by `model.faces`, including a placed copy's name. `points`
contains 1–256 world-space points in millimetres (each coordinate finite and less
than 1e9 in magnitude). Omit `face` to find each boundary point's face automatically;
when supplied it is a face name from `model.faces` or a point on that face.
The example face name must be replaced with one from the inspected body.

The reply is `{body, results}`, with one result per point in input order:
`{"class":"inside"}`, `{"class":"outside"}`, or
`{"class":"on","face":"…","normal":[nx,ny,nz]}`. Classification uses exact trimmed
geometry at the body's modelling tolerance. Holes and cavities are outside material;
a hole wall's unit normal points into the hole and a boss wall's points away from
its axis. Face names use the persistent naming system shared with `model.faces`.
At an edge or vertex, `face` selects which incident face supplies the normal;
otherwise the first containing face in `model.faces` order is used. Every `on`
point must lie on the selected face when one is supplied. Mesh bodies, invalid
references and points where a normal is undefined return an error. This command
leaves the design and undo history unchanged.

## Installing

**Install plugin…** in **Preferences > Plugins** (desktop only) picks a plugin `.zip`.
One confirmation screen shows its name, version, summary, requested permissions and entry placements.
Each command group explains its access in plain words: files, designs, application settings or other plugins.
Use **Browse…** to choose each requested folder, or type or paste its absolute local path.
Each folder shows its requested read/write access. When updating an installed plugin, a stored folder
with the same name is kept as the editable starting value only when the requested access is the same
or lower, with read access lower than write access (including read and write). Read-to-read and write-to-read updates keep the
folder; read-to-write updates start empty so you choose the folder again. You can change any kept
folder before approving it with **Install**. If a kept folder no longer exists, **Install** reports
the folder error; choose a folder again. The plugin gets only the folders displayed and approved with
**Install**; choosing or editing a folder grants nothing on its own.
When the plugin asks for more memory than the ceiling in **Preferences > Plugins** (128 MiB unless
changed), or more fuel than **Max plugin fuel** (100,000,000,000 unless changed), the screen says
how much it asks for and what the default limit is.
**Install** approves the displayed command groups, folders, fuel and memory, publishes the package and stores the
approvals; **Cancel** changes nothing. Installation never runs plugin code. Approved fuel and memory apply to
that exact package only: a replaced or changed request asks again, since the approval hash covers the
manifest and component. Other plugins keep the user's default limits.
Fuel approval has a hard host ceiling of **10,000,000,000,000** (`craft_plugin::MAX_APPROVED_FUEL`).
A request above it is refused at install with `OverCeiling`, naming the request and the hard ceiling,
even when Max plugin fuel is higher.

Programs use the same `plugin.install` command without opening dialogs:

```json
{
  "path": "gears.zip",
  "approval": "COPY_THE_APPROVAL_FROM_THE_REVIEWED_DRY_RUN",
  "approve": {
    "command_groups": ["file"],
    "folders": [{"name": "exports", "path": "/home/user/exports", "read": false, "write": true}],
    "fuel": 10000000000000,
    "memory_mib": 1024
  }
}
```

Approvals may only narrow the manifest's requests. An unrequested group, folder name or access mode
returns a typed refusal. `memory_mib` is either omitted (the plugin runs under the app's ceiling) or
exactly the manifest's `runtime.limits.memory_mib`. Likewise, `fuel` is omitted (Max plugin fuel
applies) or exactly `runtime.limits.fuel`, up to the hard host ceiling. Headless callers submit the
same `approve` object to `plugin.install`; stored grants include the approved fuel.
Omitting `approve` grants no groups, folders, fuel or memory, including when replacing a plugin.
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
The response includes name, version, description, required `capabilities`, `min_host_version`,
`optional_capabilities`, `granted_capabilities`, `extensions`, permissions, requested `limits`, the app's
`host_limits`, app commands,
namespaced `entries`, declared `entry_points` with input schemas and slots, replacement information,
the target `folder` and whether it was `installed`. A dry run reports `installed: false`.
Dry-run description is the package review path; there is no separate review command or folder installer.
Review `min_host_version` for the oldest supported SolidCraft and compare `optional_capabilities`
with `granted_capabilities` to see which optional features this host supports. Capability grants
describe host API availability; the separate `approve` object still controls command groups,
folders and memory. `extensions` contains inert plugin metadata.

A zip holds one plugin, with `plugin.json` and its component at the top or inside one top-level folder.
The existing `solidcraft_io::safe_zip` reader refuses absolute paths, `..`, symlinks and device entries.
The installer limits the archive to 256 entries, each file to 16 MiB (an importer component may be up to 64 MiB), and input and unpacked content to
64 MiB. The manifest has a separate 64 KiB cap. Before publication, the host checks
`min_host_version`, then parses and validates the manifest. It refuses unknown or incompatible
required capabilities; unsupported optional capabilities are left ungranted. It unpacks into a
hidden `.install-…` folder beside the installed plugins, then renames it to `plugins/<id>/`.
The entries resolve immediately, with no restart.

## Updates

A plugin can name the GitHub repository it is released from, and SolidCraft 0.49 and later offer its
new versions on its card. Older versions refuse a manifest with this field.

```json
{"id": "gears", "name": "Gears", "version": "1.0.0", "updates": {"github": "maker/gears"}}
```

- At every start, and once a day while SolidCraft stays open, one background request per such
  plugin (at most 20) asks GitHub for that repository's latest release. Nothing else is sent, and a
  failed request (offline, rate limited) says nothing. The app's **Check at start and once a day**
  update setting, or the organization's policy over it, turns these checks off too.
- The release is offered when it is a stable release, its tag is a version (`v1.1.0` or `1.1.0`)
  newer than the installed `version`, and it has an asset named `<id>.zip` or `<id>-<version>.zip`
  under that repository's releases, for which GitHub lists a sha256 digest. Both versions must be
  `major.minor.patch` versions.
- **Update to 1.1.0** then shows beside the card's on/off control, also on a turned-off plugin. It downloads
  the `.zip` into the hidden `plugins/.updates/` folder and checks it against the sha256 digest GitHub
  lists for the asset. The same confirmation as Install follows, titled Update plugin: the package
  must be the same id and the offered version, every install check applies, and the installed
  version's folders are kept as for any update (above). **Update** replaces the plugin, with no restart;
  **Cancel** or any failure leaves the installed version. The downloaded `.zip` is then deleted.

## Removing

Each installed plugin card in **Preferences > Plugins** has an enable switch and **Remove** action.
Remove asks for confirmation, then deletes the package folder, its stored grants and remembered importer options. Cancelling
Preferences does not restore a removed package. Removing a plugin preserves its data in saved designs.
Approved grants and removal survive closing or resetting Preferences. Grants are host preferences,
never design data; preferences retain at most 256 plugin ids, four command groups and 32 folders per id.
The desktop app keeps them in the plugins folder's grant store too, which the command line and headless
MCP share when they run with `--keep-plugin-approvals` (see [Headless use](#headless-use)). The browser Plugins page remains desktop-only.

The command surface exposes management without opening dialogs:

| Command | Parameters | Result |
|---|---|---|
| `plugin.check` | `{path}`: a package folder or `.zip` | `{ok, id, name, version, kind, approval, entries, problems: [{severity, field, problem, fix}]}`; installs nothing |
| `plugin.install` | `{path, approval?, archive?, replace?, dry_run?, approve?: {command_groups, folders: [{name, path, read, write}], fuel?, memory_mib?, input_folder?}}` | Package description and installation status, including declared `entry_points` |
| `plugin.list` | `{}` | `{plugins: [{manifest, commands, disabled}]}` |
| `plugin.remove` | `{id, remove_user_data?: bool}` (default: false) | `{id, removed: true}` |
| `plugin.grants` | `{}` | `{grants: [{id, status, package_hash, command_groups, folders, fuel?, memory_mib?, input_folder?}]}` |
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
  fuel, memory and `input_folder`, bound to the exact manifest and component reviewed (`package_hash`).
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

### Grouping ribbon buttons

By default, every entry has its own button in the Model tab's **Plugins** group, including a
plugin with several entries. A plugin can explicitly put selected entries behind one button
by adding `ribbon_groups` to its `runtime`:

```json
"runtime": {
  "module": "gears.wasm",
  "commands": [],
  "entries": ["spur", "bevel", "worm", "inspect"],
  "ribbon_groups": [
    {
      "label": "Gears",
      "icon": "icons/gears.svg",
      "entries": ["spur", "bevel", "worm"]
    }
  ]
}
```

This gives one **Gears** ribbon button with a drop-down listing `spur`, `bevel` and `worm` in
that order, and an individual button for `inspect`. Clicking either the button or its arrow opens
the menu; choosing an entry opens its usual input dialog. Opening the menu runs no plugin code.
Menu rows use each entry's usual title and icon. Entries that a user pinned individually to the
ribbon keep their own button as well as their place in the group menu.

The Plugins menu (the one the ribbon's **Plugins** group label opens) follows the same groups:
in the plugin's submenu, each group is a nested submenu with the group's label and its entries in
the group's order, for example **Gears > Spur Gear…, Bevel Gear…, Worm Gear…**. Groups come first, in
declaration order, and entries in no group stay listed flat after them. There is no separate
setting: `ribbon_groups` is the one way to declare groups.

`ribbon_groups` is optional; omitting it or declaring `[]` preserves the existing ribbon and
menu behaviour. It affects only the Model tab's Plugins buttons and the grouping inside each
plugin's Plugins submenu: every entry stays in the Plugins menu, and declared File/Help items,
other ribbon-tab slots and programmatic command ids remain available as before.
Several groups are allowed and appear in declaration order, followed by individual entries.
Each group has:

- `label`: 1 to 80 printable characters on one line, with a non-blank label. Surrounding spaces
  are ignored when displaying the label and checking for duplicate labels.
- `icon` (optional): a package-relative `.svg` path, following the same path rules, size limit,
  SVG subset and theme palette as [entry icons](#icons-and-pictures). A missing or
  invalid SVG falls back to the generic plugin icon; an unsafe or malformed path is refused.
- `entries`: 1 to 32 names from this plugin's `runtime.entries`, in menu order. Use the local
  entry name (`spur`), without the plugin id.

At most 32 groups are accepted. Unknown entry names, empty groups, duplicate labels, repeated
entry names within a group and entries included in more than one group are refused with an
`invalid_manifest` error naming the field and the problem. Unknown fields in a group are also
refused. Group icons are optional and do not need a new capability.

Clicking a File, Help or ribbon item opens the same input-schema dialog as the Plugins menu.
The host finds the entry by its namespaced `<plugin-id>.<entry>` command. The dialog uses the same
enabled state and grant checks from every placement. Missing command-group grants leave Run off
with an explanation. Opening a command dialog executes no plugin code; Run submits the
existing `plugin.run` transaction. By default, importer menu entries show options before choosing
a file, and File › Open and drag-and-drop import their paths directly.
An importer entry may set `"file_first": true` in its manifest to choose the file before its
options appear, so they can match the file. Its menu entry asks for the file first, and File › Open
and drag-and-drop already have a path and go straight to the same form. Open file opens it as a
new design with `file.import`; Cancel at either step opens nothing. Absent or `false` keeps the
options-first dialog, and only an importer entry may set it. Releases before this field existed
refuse a manifest that sets it, like any unknown entry field; set `min_host_version` to match.
An importer may opt into the bounded dialog hook below; choosing a file then runs that hook,
never the importer. The field changes no saved designs, and scripts and MCP calls ignore it.

### File-aware importer forms

File conditions use the same bounded condition grammar as sibling conditions, replacing `field`
with `file`. Facts are host-owned, read-only and separate from input properties; a property named
`extension` cannot override the selected file's extension. Facts are available without a new
capability:

```json
"parts": {
  "type": "string", "title": "Part",
  "x-solidcraft-visible-when": {"file": "extension", "in": ["asm", "assembly"]}
}
```

The fact names are `name` (filename, no folders, at most 255 UTF-8 bytes), `extension`
(lowercase, no dot, at most 32 bytes), `size` (bytes, at most the import limit of 1 GiB), and
`format` (the host-matched namespaced importer entry, such as `reader.convert`, at most 161
bytes). File conditions accept `eq`, `in`, `all`, `any` and `not`, with the existing depth and
node budgets. They work on visibility, enabled state, choices and conditional defaults,
including nested schemas. Unknown facts and literals of the wrong type are rejected on load.
Before a file is selected, file comparisons are false. Direct imports bind the same conditions
to their supplied path before validating and omitting hidden options; they never open a form.

An importer can additionally request `"dialog@0.1"` alongside `"formats@0.2"` and build against
the additive `importer-dialog-plugin` world in `crates/craft-plugin/wit/plugin.wit`. This includes
the existing importer world and adds the export `dialog-provider.update-fields(entry, context)`.
The existing 0.1 and 0.2 importer worlds and their interfaces are unchanged; existing components
need no rebuild and do not acquire a hook. The host checks the optional export when checking or
installing the component; an explicit `dialog_hook` declaration requires it. This file-aware reply
format is for importer entries without `dialog_hook`; entries with `"dialog_hook": true` use the
general field patches described below.

`context` is JSON:

```json
{"values":{"mode":"parts"},"file":{"name":"parts.asm","extension":"asm","size":2048,"format":"reader.convert","generation":3}}
```

`values` contains the current visible scalar inputs, including incomplete forms. The host does
not accept file facts from option values. The hook returns JSON updates to already declared
fields, for example after reading the part names listed by a header:

```json
{"fields":{"parts":{"oneOf":[{"const":"plate","title":"Plate"},{"const":"cover","title":"Cover"}],"default":"plate"}}}
```

A reply contains only `fields`, at most 256 updates and 64 KiB of JSON in total. Each update may
change `title`, `description`, `default`, and either `enum` or `oneOf`. Text is at most 2,048
bytes. Choice lists contain 1–256 strings; `oneOf` options contain `const` and optional `title`
and `description`. Values must satisfy the original field constraints, and defaults must fit
the updated choices. A hook cannot add fields, change types, constraints or directory semantics,
or replace declarative conditions. Conditional choice lists cannot be replaced. Folder values
need the existing install grants unless the field explicitly requests read consent as described
below. A hook default grants no access; selecting or editing an annotated folder can grant read
access for the eventual import, never for the file-aware hook.
Untouched fields take new defaults; compatible text and numeric edits, including cleared or
incomplete input, survive updates. Edited choices survive when the new choices still offer them.
String choices retain their length constraints, and every offered value must satisfy them.
If applying an update changes the visible values, the host calls the hook again with those
values before enabling Open. Unchanged visible values do not trigger another call.

Each update runs in a fresh sandbox on a worker, with at most 5,000,000 fuel, 500 ms shared by
prefix acquisition and guest execution, and 256 host calls (or the manifest's smaller budgets).
A prefix read that finishes after the deadline is discarded. The manifest's approved memory
and stack ceilings still apply. Compilation and bounded prefix acquisition also run off the UI
thread. Through the existing `source` interface, only path `""` is available: `size("")` reports
the full file size, `resolve("")` returns `""`, and `read("", offset, len)` reads a snapshot of the
first 64 KiB. Each requested range must lie in that prefix; the sum of requested lengths per
update is at most 64 KiB, including repeat reads. Reads ending past EOF return fewer bytes.
Sibling files remain inaccessible even with `input_folder` approved. `design.write` and design
evaluation are refused during a hook; there are no app commands, folder grants or writes.
The host bounds context JSON to 128 KiB. A failed or invalid update leaves Open off with the
error; selecting a file again retries. No partial update is applied.

**Change file…** keeps the form open, updates its host path and advances `file.generation`,
even when the same path is picked again. Old header choices are cleared immediately. At most
one worker runs per dialog; a result for an earlier generation is discarded before the next
update starts. Closing the form disconnects its results. Editing inputs reruns an opted-in hook,
with fields and Open disabled while it is pending. File conditions themselves never run Wasm.
Scripts, CLI and MCP continue to pass `path` and `options` directly and never call this file-aware
schema hook. An explicit `dialog_hook` entry still runs its general field hook before importing.

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

The default is unchanged: dialog fields show required fields in `required` array order, then
optional fields alphabetically. To use the order of `input_schema.properties` in the manifest, set
`"x-solidcraft-field-order": "declared"` on the root `input_schema` object. Any other value for
this annotation is invalid. With this opt-in, required and optional fields follow declaration
order. Fields hidden by `x-solidcraft-visible-when` return to their declared positions when shown
again; the remaining fields keep their relative order. Field ordering also determines which
visible, enabled selection field receives the current viewport picks when the dialog opens.

The host records declaration order separately as `input_property_order` on loaded entries (a map
from schema JSON Pointer paths to property-name lists, with `""` for the root). This metadata
survives entry round trips; plugin authors only need to order their schema properties and set the
root annotation. A property that comes from a shared `$defs` definition takes its place where the
schema references it, and the properties inside that definition keep the order the definition lists
them in (a `properties` written beside the `$ref` replaces them, with its own order). Fields without recorded order keep the default required-first, then alphabetical
order, even when the annotation is present.

A string with a fixed set of values becomes a dropdown. With `enum` the dropdown shows the values
themselves; to show words instead, list the options as `oneOf`, each `{"const": value, "title": shown}`
(with an optional `description`). The dropdown shows the titles and the plugin receives the value:

```json
"up_axis": {"type": "string", "title": "Up axis", "default": "as_is", "oneOf": [
  {"const": "as_is", "title": "As in the file"},
  {"const": "stand_up", "title": "Stand up (Y-up to Z-up)"}
]}
```

### Font fields

Mark a string property with `"x-solidcraft-font": true` to offer installed font families in the
entry dialog:

```json
"font": {"type": "string", "title": "Font", "x-solidcraft-font": true,
         "default": "Liberation Sans"}
```

Open the dropdown and type part of a name to filter it, ignoring case. Up/Down and Enter, or a
click, choose a match. The plugin receives the full family name. The list includes the built-in
Liberation Sans and usable installed families discovered by sketch text, cached after a background
scan. Plugins need no folder grant to offer or use installed family names.

The annotation must be `true` on a string property. It is a dialog hint: over MCP and scripts the
value is still an ordinary string, and input checking accepts unknown names. The operation that
uses the font reports an unknown font as usual. Other declared string constraints still apply.
Use an ordinary `enum` or `oneOf` dropdown for a fixed list of names.

### Edit a stored operation through its normal form

This host adds `forms@0.1` alongside the existing `commands@0.1` and `commands@0.2`
capabilities. Require a host advertising `forms@0.1`; older hosts reject that capability
before executing the plugin. Existing plugins link unchanged and can keep making explicit
entry calls without adding the capability. A plugin that needs populated forms declares
`"capabilities": ["commands@0.2", "forms@0.1"]` and imports
`craft:plugin/forms@0.2.0.open(entry, params) -> result<_, plugin-error>`.
`params` is JSON with the target entry's normal input schema, including the selection objects
returned by selection fields. The target must be a command entry of the same plugin.
At most eight forms may be requested in one run; parameters are limited to 1 MiB.
The host opens requested forms only after a successful interactive run, on the design tab that
ran the entry. If another tab is active when the reply arrives, the reply waits until the original
tab is active again. Preview discards form requests.
A failed or cancelled run discards every request.

The application opening path is `plugin_menu::open_form(app, plugin_id, entry, params)`.
It draws the usual form filled with the supplied values and picks. Opening it only validates
inputs; the entry runs on Apply, or on a scratch design when Preview is explicitly enabled.
From the application control surface, use:

```json
{"method":"ui.start","params":{"command":"plugin.edit","params":{
  "plugin":"my_cam","entry":"pocket_edit","params":{"depth":5,"edges":[]}
}}}
```

Plugins-menu entry forms use this same form implementation. A docked panel's table row action
(`on_edit`, see Docked panels) runs the declared entry as an ordinary command with the typed
event; it does not open this form itself. To edit a stored operation from a panel, call the plain
opening function or `ui.start` above.
An entry that already has an unsubmitted form is refused rather than replacing its values.

Scripts and MCP use explicit parameters to call `<plugin>.<entry>` or `plugin.run`.
`engine.execute("plugin.edit", ...)` returns typed `interactive_required`, with the instruction
to call the entry with params instead. Control replies expose `plugin_error.kind: "interactive_required"`;
MCP exposes the same data in `structuredContent.plugin_error`, with `entry` and
`action: "call_entry_with_params"`. The `forms.open` import returns that same typed host
refusal for programmatic invocations (represented by WIT `unsupported`); it never opens a window.
The capability grants no permission to edit another plugin's form or bypass execution approval.

Stored values are treated as user edits: conditional defaults do not replace them, and the
conditions below apply immediately. Schema-allowed extra properties are preserved in Preview
and Apply. Invalid values and missing geometry appear beside the
corresponding fields and keep Apply off. Validation runs on a cancellable worker; the form
shows “Checking inputs…” until the current inputs and model have been checked. Results for
older inputs or models are discarded. A stored edge or face must still match its exact
indexed geometry and point. Approximate hit coordinates have an allowance of 0.1% of the
exact body bounds diagonal, at least 0.001 mm and the body modelling tolerance. Only the
supplied face or edge index is tested using exact geometry; there is no nearest-entity fallback.
Display settings do not change this allowance.
These selection objects are the current selection contract, not a new persistent naming format.

Preview starts off for a populated form. Enabling it computes on a scratch design; Cancel discards it and leaves the design equal to its
previous state. Apply invokes the entry once through the ordinary `plugin.run` transaction.
The plugin replaces its own saved operation using its operation id in `params`; the host does
not invent or reorder operations. One Undo restores the previous state, and Redo reapplies it.

The form captures `document.revision` and document identity on opening. Apply carries these in
`plugin.run.form_guard`; checking and execution happen in the same command transaction. If the
revision changed, the entry does not run: the form revalidates against the current design,
reports missing picks beside their fields, and asks the person to review and Apply again.
If the design changes while a worker computes, atomic command acceptance rejects its result.
No stale result can overwrite the current design.

### Folder fields and read access for one run

A string property with `"format": "directory"` has a text box and, on a desktop host with a
folder picker, a **Browse…** button. Without an annotation it passes a path and keeps its existing
install-grant check. It grants no new access.

Add `"x-solidcraft-grant": "read"` to ask for read access to a selected directory and its
subdirectories for this entry invocation:

```json
"library": {
  "type": "string", "title": "Parts library", "format": "directory",
  "x-solidcraft-grant": "read"
}
```

The dialog explains this permission beside the field. Browsing or editing/pasting its path gives
consent when Run, Open file or Choose file… submits the entry. A default, including a conditional
default, or a stored form value is not consent: the user must select or edit it, or it must already have read approval. The path
must name an existing absolute local directory. Empty optional fields grant nothing. A hidden field
is omitted and grants nothing; a disabled field cannot create new consent. Replacing the value
replaces that field's root. Cancelling the entry or file picker grants nothing. Preview receives no
folder grants. Closing or reopening a dialog starts fresh.

Consent is bound to the reviewed package's content hash, entry and field, carried separately from
parameters to that request's worker and consumed for that invocation. It is never saved to the design,
Preferences or the shared grant store. To remember access, use the existing approvals in
**Preferences › Plugins**: the plugin declares a named read folder in `runtime.permissions.files`
and the user approves its path when installing or reviewing it. A later annotated field may name that
folder or a permitted subdirectory; its property key need not match the install-time permission name.

Scripts, CLI, MCP and direct `plugin.run` or `file.import` requests open no dialog and gain no
permission from field values. A non-empty annotated value must already be covered by a current,
package-bound **read** approval that the manifest requests, and by the caller's file scope.
Write-only approvals do not imply read access. Hidden conditional values are removed before this check.

To read these folders, add `"folders@0.1"` to `capabilities` alongside `commands@0.2` or `formats@0.2`.
Build against the additive `command-with-folders` or `importer-with-folders` world in
`craft:plugin@0.2.0`. These worlds retain their command/importer interfaces and import the new
read-only `folders` interface. `command-with-folders` keeps its original imports; a plugin that
also uses forms, design tokens or a dialog hook can compose an additional world with those
interfaces. Existing worlds and the 0.1 API are unchanged; existing components still load.

- `folders.read(grant, path, offset, len)` reads 1 byte to 1 MiB per call, returning fewer bytes at EOF.
- `folders.size(grant, path)` returns a regular file's size, or none if absent or not a regular file.
- `folders.resolve(grant, path)` returns the relative spelling of an existing file, or the unique
  case-insensitive match of its final name, or none. An empty path resolves to the selected root (`""`).

`grant` is the annotated **property key**, such as `"library"`; `path` is relative to that field's
root, such as `"parts/bracket.bin"`, never the absolute path from the options. No operation writes.
These grants do not widen `source`, `files`, host commands, other entries or another invocation.
Absolute paths, parent traversal, hidden descendants, symbolic links and Windows device paths are
refused. The approved root itself may have hidden ancestors. There are at most 32 annotated fields;
reads share the import's 1 GiB total byte and 4,096-file budgets with `source`, as well as its
cancellation, call and runtime limits. Command entries use the same folder byte/file limits and
retain their command call limits.

The annotation is supported only on top-level string properties with `"format": "directory"`.
Other placements (including nested schemas and composition branches), modes other than `"read"`,
malformed values, more than 32 grants, and non-printable or over-128-character property names are
rejected at manifest load and by `plugin check`. Conditions on these properties use the rules below.

### Shared field definitions

When several entries ask for the same field, define it once under `$defs` and point to it with
`{"$ref": "#/$defs/<name>"}`. The `$defs` can sit at the top of the manifest, where every entry
can use it, or in one entry's `input_schema`, where it wins over a top-level definition of the
same name. Keys beside a `$ref` replace the definition's own, so an entry can keep the shared choices
with its own `default` or `description`:

```json
"$defs": {"units": {"type": "string", "title": "Units", "enum": ["mm", "in"], "default": "mm"}},
"runtime": {"entries": [
  {"name": "box", "input_schema": {"type": "object", "properties": {"units": {"$ref": "#/$defs/units"}}}},
  {"name": "lid", "input_schema": {"type": "object", "properties": {"units": {"$ref": "#/$defs/units", "default": "in"}}}}
]}
```

The app inlines every `$ref` once, when it loads the manifest, so dialogs, tools, conditions and
input checks see the full field. Only these local names are followed. An unknown name, a definition
that refers back to itself, or nesting too deep to expand refuses the manifest. The 64 KiB cap
applies to `plugin.json` as written.

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

### Computed fields with an optional dialog hook

Use declarative rules above for visibility, availability and defaults expressible as sibling
conditions. For computed defaults or lookups, add `"dialog@0.1"` to `capabilities` and
`"dialog_hook": true` to the individual entry. Export the additive `dialog-provider` interface:

```wit
update-fields: func(entry: string, values: string) -> result<string, plugin-error>;
```

Both strings containing values are JSON objects. Existing command worlds keep their exports;
plugins without this capability need no new export and continue to link and run. The interface
is available alongside both supported `craft:plugin` WIT versions (0.1 and 0.2); use the
`command-dialog-plugin` world when building a command plugin with the hook.
Importer entries can use the same general patches. When the file-first form has a selected file,
the host binds its file conditions before validating patches. Changing the file invalidates older
patch replies, and the file reader and field hook share one worker slot per dialog.

A reply maps existing property names to patches, for example:

```json
{
  "clearance": {"default": 6.5, "hint": "Computed from the selected size"},
  "size": {"choices": ["metric"], "enabled": true},
  "details": {"visible": false}
}
```

Only `visible`, `enabled`, `choices`, `default`, `value` and `hint` are accepted. Choices must be
an ordered subset of that property's own `oneOf` constants, satisfying declarative option
conditions. The hook cannot add properties, change types, enable or reveal a declaratively
unavailable field, or bypass the input schema. A malformed reply is ignored **whole** with a
warning. Defaults and values must satisfy the field schema and the narrowed choices. Hints are
at most 1024 bytes of printable text; input and reply are at most 1 MiB each.

Declarative conditions run first, then the validated patch, then the same input check used by
programmatic calls. The hook never edits: it runs as a read entry, with no edit command or file
write grants, in a fresh sandbox. Its independent budgets are at most 10,000,000 fuel, 16 MiB
memory and 250 ms, further capped by the entry's approved runtime limits and host ceilings.
Compilation and execution happen on the command worker, outside the window thread.

The dialog debounces changes for 120 ms. It remains interactive while waiting and drops replies
from older generations. Run uses the current fields: valid inputs remain runnable while the hook
is pending, and an invalid or missing value keeps Run off. A user's edited field is
never overwritten by the hook's `value` or `default`. A timeout, trap or error leaves the
declarative fields in place with a quiet note.

Scripts, `plugin.run` and namespaced MCP plugin tools call the same hook once before executing
the entry. Tool schemas accept omitted keys for hook defaults; the original required fields are
checked after the hook. Defaults fill omitted keys; supplied values are preserved. A value outside narrowed
choices fails with `invalid_input` naming `params.<field>`. An exhausted time/fuel budget returns
a typed `exhausted` error, and the entry does not run. Programmatic calls never open a dialog.

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
- **References stay true.** In ordinary plugin forms, picks belong to the design they were made on.
  Any change to the design while the dialog is open (an edit, an undo) clears them and the dialog says so; turning
  the view or changing the selection does not. Picking runs no plugin code; Run sends the picks
  through the same `plugin.run` request. Programs calling `plugin.run` pass the same references
  themselves; nothing fills them from the selection. Populated forms for editing stored operations
  retain stale picks and validate them beside their fields, so they can be corrected.

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

- At the top of a command or importer entry's `input_schema` it is the entry's icon: its ribbon
  button, its rows in the Plugins, File and Help menus, and its dialog's header. Without one the
  entry has the generic plugin icon. Importers use the same image checks and fallback as commands.
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
  and the entry works as before. `plugin.check` reports missing or invalid images for both commands
  and importers.
- Images are not part of the `approval` hash: changing one changes only what is drawn.

An option may also name a material swatch with `x-solidcraft-swatch`. This is a separate
annotation for PNG or JPEG artwork whose own colours are preserved, including transparency;
the icon palette rules do not apply. For example:

```json
{"const": "oak", "title": "Natural oak", "x-solidcraft-swatch": "swatches/oak.png"}
```

Ship the image inside the plugin package. Paths follow the icon rules: plain relative paths,
no `.` or `..`, no absolute paths or links, and nothing fetched from the network. Use `.png`,
`.jpg` or `.jpeg`, at most 64 KiB and at most 256 × 256 decoded pixels. **128 × 128 pixels is
recommended.** A package may name at most 256 distinct swatches. The host draws fixed-size
tiles and loads and decodes each distinct image once
when it loads the package, then caches its texture. A missing, corrupt, wrongly typed or oversized
swatch is omitted with a warning in `plugin check` and the install review; the plugin still works.

An option may instead name a picture of the thing it stands for, such as a manufacturer's part,
with `x-solidcraft-image`:

```json
{"const": "elbow90", "title": "Elbow 90° PVC d63", "x-solidcraft-image": "parts/elbow90.png"}
```

It follows the swatch rules (a plain relative path inside the package, no `.`, `..`, absolute path
or link, nothing fetched from the network, `.png`, `.jpg` or `.jpeg`, colours kept as they are)
with bigger limits: at most **512 KiB** and **1024 × 1024** decoded pixels. The host keeps each
picture at most 512 pixels on a side, decodes each distinct file once when it loads the package,
and caches its texture. The choice list shows the picture as a thumbnail beside the option, and a
larger one of the highlighted option (the chosen one when none is highlighted). If an option names
both, the picture is used, and if that picture is missing or refused, the swatch. A package may name at most 256 distinct swatches and pictures together.
Decoded swatches and pictures share a budget of 128 MiB per package. When the total would exceed it, the host omits that entry, every later swatch and picture, and records a warning.
A missing, corrupt, wrongly typed or oversized picture is omitted with a warning in `plugin check`
and the install review; the plugin still works, and the value sent is still the option's `const`.

A partially illustrated choice shows tiles beside the option titles in its dropdown. When every
available option has a usable swatch, the dialog offers a grid with a search box; search matches a
case-insensitive substring of the title. Choosing either presentation still sends the option's
`const` value. MCP and scripts use exactly the same input values as before.

### View commands

After Run commits the result, the app applies the run's last view command. `view.fit`
frames the result, keeping the view direction; `view.home` frames it from the home direction.
A run with no view command leaves the camera unchanged.

### Preview

In an ordinary plugin form, a command entry that edits the design is previewed the way native
tools are: while its fields make parameters, the host runs the entry on a scratch copy of the
design and draws what it would change in the viewport (new material tinted, removed material
as a see-through ghost).
An ordinary plugin form has a **Preview** box, on when it opens; with it off, nothing runs until Run.
A populated form for editing a stored operation starts with Preview off and retains stale picks
for field validation. Nothing runs until Apply or an explicit Preview request.

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
  when one is). While a native tool is open, its own preview is the one shown. Read entries, long jobs and
  importers are not previewed.

## Long jobs

A command entry that takes a while, such as computing a toolpath, declares `"job": true`:

```json
"entries": [{"name": "pocket", "job": true, "input_schema": {"type": "object", "properties": {}}}]
```

- **Time.** A job runs for up to ten minutes, a fixed limit. The runtime's
  `deadline_ms` (at most 30 seconds) still applies to the plugin's other entries. Fuel and memory
  limits are the same as for any entry. Each job has a limit of 1,048,576 host calls, including
  progress reports.
- **Where it shows.** Every plugin run started from its dialog or by an agent runs on a worker and
  is a row in the operations panel at the bottom right of the view, named for the plugin and the
  entry ("Gears: Pocket"). The step the plugin reports with `control.progress(fraction, message)`
  is added to the name, and the fraction fills the row's bar. `ui.operations` lists it like any
  other operation.
- **Cancelling.** The row's red X (or `ui.operations` with `cancel`) stops it. A `control.cancelled()`
  call made after cancelling returns true if the plugin reaches it. The host can stop the plugin
  between host calls, within a few milliseconds, before it reaches that call. Cleanup must not
  rely on `control.cancelled()` returning true. A cancelled run leaves the design and the undo
  history as they were. A run whose design changed while it computed is not kept either.
  Cancelling does not undo files the run already wrote, such as an export.
- A job's dialog has no **Preview** box: a preview would run the whole job on every change.
- An importer already runs this way (its deadline is up to ten minutes), so only a command is
  declared a job.

## Versions

The WIT package is `craft:plugin@0.2.0`. Its capability `commands@0.2` runs app commands, logs,
reports progress and cancellation, reads named files and a clock, and exports the entry. A plugin's
`plugin.json` lists what it needs in `capabilities`, for example `"capabilities": ["commands@0.2"]`;
the host refuses an unknown or incompatible required capability when it reads the manifest, with
a plain message such as "needs commands 0.3; update SolidCraft or the plugin". `formats@0.2`
adds file-format slots: importers and exporters; `evaluate@0.2` lets an importer check its result,
`dialog@0.1` supplies bounded field patches and file-aware form updates, `storage@0.1` keeps data
in a design, `user-storage@0.1` keeps per-user settings for command plugins and importers
(below), and `ui@0.1` adds host-drawn docked panels. `viewport` arrives later as its own
versioned interface. While a version is 0.x, a minor version may break the one before it.

Starting with SolidCraft 0.45.0, manifests may also declare:

- `min_host_version`: the oldest SolidCraft release the plugin supports, as a strict semantic
  version such as `"0.45.0"` (including a patch number, with optional prerelease or build metadata).
  This is the host version, independent of the plugin's own `version` and its capability versions.
  An older host refuses the package before unknown-field or capability errors, naming both the
  required and running host versions. Omit it to keep the existing behaviour with no minimum.
- `optional_capabilities`: capabilities the plugin can do without. The host grants each requested
  capability whose version it supports and ignores unknown or incompatible optional capabilities.
  Malformed requests still fail validation. Both capability lists accept `name@major.minor` or
  `name@major.minor.patch`; patch numbers do not change capability compatibility. Each list has
  at most 16 entries, and a capability name may appear only once across both lists. Major versions
  must match; for major zero, the minor must match a version the host serves. For later majors,
  the host may serve a requested older minor. An entry's essential `commands` or `formats`
  capability must stay in the required list.
- `extensions`: a JSON object for future optional metadata. Unknown keys and their JSON values
  inside this object are accepted and ignored by the host. It grants no capability or permission
  and changes no plugin behaviour. Unknown top-level fields remain errors. This is distinct from
  an importer entry's `extensions` list of file suffixes.

Existing manifests need no changes. Releases before 0.45.0 cannot understand these new top-level
fields, including `min_host_version`: their existing unknown-field refusal still applies. Set
the minimum to a host that supports the fields and APIs the plugin actually needs; placing a
required feature inside `extensions` does not make an older host implement it.

For example, this importer can run without a future optional feature:

```json
{
  "id": "sample", "name": "Sample", "version": "1.1.0",
  "min_host_version": "0.45.0",
  "capabilities": ["formats@0.2", "host@0.1"],
  "optional_capabilities": ["future-import-feature@0.1"],
  "extensions": {"sample": {"documentation": "Optional importer metadata"}},
  "runtime": {
    "module": "sample.wasm", "commands": [],
    "entries": [{"name": "convert", "kind": "importer", "extensions": ["smp"]}]
  }
}
```

On this host, its granted capabilities are `formats@0.2` and `host@0.1`.
`future-import-feature@0.1` is an illustrative name, not an implemented interface. The plugin
chooses its fallback when that string is absent from the query below. The host returns the
original requested strings, including any patch numbers, for required and supported optional
capabilities only. It does not report capabilities the plugin did not request.

The small read-only query is a separate additive WIT package, `craft:host@0.1.0`
(`crates/craft-plugin/wit-host/plugin.wit`):

```wit
package craft:host@0.1.0;

interface capabilities {
    granted: func() -> list<string>;
}

world host-query {
    import capabilities;
}
```

A command or importer component may import `craft:host/capabilities@0.1.0` alongside its existing
world, with either the 0.1 or 0.2 plugin WIT. Declare required `host@0.1` when the component
imports this query. The host links the query only when `host@0.1` is granted. A manifest may ask
for `host@0.1` optionally when its component does not import the query. The bounded result has
at most 32 strings and reports API availability only; it does not approve file access, command
groups, input folders or increased memory, run commands, or open dialogs.

An optional manifest capability cannot make a static WIT import optional. A component that
unconditionally imports an interface absent from a host cannot load there, even if that
capability is listed as optional. Keep unavailable interfaces out of such a component's imports,
or require a host that provides them with `min_host_version` and a required capability.
`forms@0.1` is available for the populated command forms described above; interactive hosts
defer opening until the entry succeeds, and programmatic hosts return `interactive_required`.
`dialog@0.1` is available for the field hooks described above; a hook entry declares it in
the required capabilities and opts in with `dialog_hook`.

`themes@0.1` lets a plugin add colour themes to Preferences > Appearance > Theme from
`solidcraft-theme/1` files it ships; it needs no runtime. A plugin that does not list it is not
affected. See [plugin themes](plugin-themes.md).

0.2 added one error, `plugin-error.unsupported(string)`: the input is valid, but this plugin does
not handle it, such as a file with no solids or a newer version of its format. Use it instead of
`invalid-input`, which means the input is broken; SolidCraft then says "Sample does not support
bracket.smp: ..." instead of an "invalid" file. Plugins built against 0.1 (`commands@0.1`,
`formats@0.1`, `evaluate@0.1`) still run unchanged: the host reads the WIT version from the
component's own imports and exports, and serves the frozen 0.1 WIT
(`crates/craft-plugin/wit-0.1/plugin.wit`) to those. A component that names a version the host does
not run, or two versions at once, is refused before any of its code runs.

`shaders@0.1` lets a plugin draw the 3D view's background with a WGSL shader the host checks,
wraps and limits; it needs no runtime. See [plugin shaders](plugin-shaders.md) and
[RFC 0015, amendment 1](rfcs/0015-plugin-ui-workspaces.md#amendment-1-shaders).

`shaders@0.2` adds tints, which colour the ribbon's icons, its button blocks and the panels each
frame, settings sliders for shaders, a `state` grid a background keeps between frames, and background textures the person chooses under the
plugin's card (`choose`). See [plugin shaders](plugin-shaders.md#tints) and
[pictures the person chooses](plugin-shaders.md#pictures-the-person-chooses).
It also adds panel shaders, which draw every see-through panel's background from what is behind
it. See [plugin panels](plugin-panels.md).

## Docked panels

Add `"ui@0.1"` alongside `"commands@0.2"`, declare `runtime.ui`, and target `panel-plugin` in
`crates/craft-plugin/wit/plugin.wit`. The `panel-plugin` world includes `command-plugin`, bringing
`storage`, `forms` and `design-state` to panel plugins. Plugins that also provide a dialog field hook
can target `panel-dialog-plugin`, which adds `export dialog-provider`. The `ui` and `ui-provider` records follow
[RFC 0015 section 1](rfcs/0015-plugin-ui-workspaces.md#1-panels). The manifest declarations are
listed during installation. The component's `ui-provider.describe` must return those declarations.
The host owns the controls, theme, docking and plugin badge; plugins ship data and layouts.
Panels belong to command plugins; the importer world remains separate.

For example, `runtime.ui` can declare a job table whose row action runs the entry `edit`:

```json
{
  "ui_version": 1,
  "panels": [{
    "id": "jobs", "title": "Jobs", "zones": ["right", "bottom"],
    "layout": {"root": 0, "nodes": [{"widget": {"table": {
      "bind": "/jobs", "row_id": "id",
      "columns": [{"key": "name", "label": "Operation", "unit": null}],
      "select": "single", "on_select": null, "on_edit": "edit"
    }}}]}
  }],
  "workspaces": []
}
```

Every action names an entry in `runtime.entries`. Buttons, toolbar items, list selection,
table selection or editing, and form changes invoke ordinary `<plugin>.<entry>` commands.
Panel clicks put the event in the host command envelope's `ui_event` field and pass the same
event as the entry's `params`. `ui-provider.on-event` receives the typed event in the same
transaction. For the table above, the event is
`{"panel":"jobs","event":"edit","path":"/jobs","row":"op1","value":null}`.
An MCP caller uses `execute` with the same `<plugin>.<entry>` command and an envelope containing
`approval`, `params` and `ui_event`. Ordinary entry parameters, including fields named `panel`
and `event`, remain entry data and do not invoke `ui-provider.on-event`. Programmatic calls open
no forms.

The component updates cells with `ui.set-data`, for example `/jobs/0/id` = `text("op1")` and
`/jobs/0/name` = `text("Pocket")`. Values are typed scalars, with lengths in mm and angles in
degrees. Slash paths identify cells; no JSON arrays cross the UI WIT interface. A table row may
carry an optional `indent` integer (0, 1, 2, …, capped at 8), for example `/jobs/0/indent` =
`integer(1)`. The host indents the row's first cell to display nested data before dedicated tree
widgets. Rows without an `indent` entry remain un-indented. Successful calls merge into the
panel once per frame. A failing entry discards its updates. `clear-data` replaces the panel data
with an empty set; `show-panel` requests visibility in the current workspace. Calling
`show-panel(true)` again reopens a panel the user closed. Closing a panel keeps the other
workspaces' placements and does not cancel a command already running.

The phase 2 host draws groups, tabs, splits, forms, buttons, toolbars, lists, tables, text,
progress and empty states. Trees, charts and newer widget versions show a placeholder.
Panels are transient view state, cleared when a design closes or the plugin is disabled.
The host remembers placement by plugin id, panel id and dock zones. Code and data updates keep
that placement. Changing the allowed zones or initial zone opens a new section in a legal dock.
Limits are eight panels per plugin, 512 widgets per layout, 1 MiB of data per panel and 100,000
rows. The widget count includes every layout node, toolbar button, form field and table column.
Lists and tables scroll virtually.

## Commands and permissions

A plugin can call every command in the MCP catalog that its manifest lists. Commands that reach
past the open design are grouped (`file`, `doc`, `app`, `plugin`) and refused unless the
manifest asks for the group in `permissions.command_groups` and the user grants it.
File access requires approved folder grants. The reserved `net` permission is
refused with a typed error. Traps, missing permissions, failed commands, limits
and invalid manifest fields return structured errors (see Errors below). Phase 2 adds importers
and exporters (below) and the author tools (`plugin check` and `plugin test`, below); browser
execution follows.

A `file` or `doc` command names a file in a granted folder by the grant's name, the `name` the manifest gives that folder in `runtime.permissions.files`: `grant:<name>/<path>` (for example `{"path": "grant:exports/part.step"}`), so a plugin never needs the folder's absolute path. After the name come plain names only (no `..`, `.`, root or drive). A name the command may not use is refused before it runs; any command but an input reader such as `file.insert_step` needs a folder granted for reading and writing. Replies name the file by its full path.

With `doc` granted, `doc.info` tells a plugin which design it runs on: `path` (null until first
saved), `read_only`, `unsaved`, `disk` (`{len, modified_ns}`, `modified_ns` a decimal string) and
`document_id`. These describe the user's design when the call started, before the plugin's own
changes. The run still works on a copy with no file: `document.inspect` gives no `path` there, and
`file.save` without a `path` is refused.

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
  (a folder, a command group, more fuel or memory than the default limit, or approval of a changed package)
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
  component against its world (`command-plugin`, `importer-plugin`,
  `importer-plugin-with-storage` or `exporter-plugin`) and this app's WIT version,
  with no WASI or network import. Each problem has a severity, the field, what is wrong and how to
  fix it. In a folder, `tests/` and hidden files are left out. It runs no plugin code, installs
  nothing and never opens a dialog.
- `solidcraft-cli plugin test <folder|plugin.zip> [--json] [--out DIR] [--require-cases]` checks
  the package, installs it into a scratch plugins folder with the app's installer, and runs its
  test cases, `tests/*.json`, through `plugin.run`, `file.import` and `file.export` with the
  app's limits. An exporter writes into the scratch folder. A
  case uses its own temporary user-settings folder, removed after the run. Cases never read or
  write the native settings folder or the folder named by `SOLIDCRAFT_SETTINGS_DIR`. A
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

An importer's options dialog has a **Remember these options** checkbox next to **Restore defaults**.
It is off by default and belongs to that one entry (a plugin id and entry name). When you start an
import with it on, the entry keeps the values you used, in the app's user preferences
(`plugin_importer_options`), outside every design. With it off the entry keeps nothing.
The next dialog keeps remembered fields that still satisfy the current input schema; missing or
invalid fields start from the schema defaults. Changing a field or cancelling the dialog or file
picker does not change the stored values. **Restore defaults** resets the values and leaves the
checkbox as it is; the stored values change only when the next import starts.

With the checkbox off, the entry's dialog starts from schema defaults, and **File > Open** and
drag-and-drop start the import directly, as before; an entry with `file_first` still shows its
dialog for the chosen file, from schema defaults. With it on, File > Open and drag-and-drop show
that entry's dialog, filled in with the remembered values, for the file just chosen. The dialog's
**Open file** button opens that file without asking for it again. If the form cannot render the
importer's schema, the file still opens directly with schema defaults. Native readers retain
priority for formats SolidCraft already supports.
Removing a plugin forgets all of its entries' remembered values, including when Preferences is cancelled.

Scripts, CLI and MCP opens never read or update this memory and never open an options dialog.
They receive only their explicit `options`, with schema defaults filling omitted fields; explicit
values always take precedence. Automated imports therefore do not depend on earlier interactive runs.

- **Opening.** `doc.open` opens a file SolidCraft does not read itself with the installed, enabled
  importer that claims its extension. `file.import` always uses an importer, and `format`
  (`plugin_id.entry`, listed in `plugin.list`'s `importers`) picks one for either command, for
  example for a `.fcstd` file SolidCraft also reads. `options` passes the entry's declared options.
  Two importers claiming one extension need `format`. An entry's optional `label` names its group
  in the Open dialog (default: the plugin's name), as for an export slot. File › Open offers the
  claimed extensions, and the Plugins menu has each importer's entry. With the file-first
  preference enabled, it chooses the file, then shows its options (see
  [File-aware importer forms](#file-aware-importer-forms)).
- **The world.** The component implements `importer-plugin`: it exports
  `importer.import-file(entry, file, options)` and imports `source` (the file, read in chunks of at
  most 1 MiB), `design` (where it writes a SolidCraft design file), `log`, `control` and `clock`.
  It has no `commands` import: an importer makes a design, it does not edit the open one.
  To keep per-user preferences, add `"user-storage@0.1"` beside `"formats@0.2"` in the manifest
  and build against the additive `importer-plugin-with-storage` world in `craft:plugin@0.2.0`.
  It adds the same `user-storage` interface command plugins use; the existing `importer-plugin`
  world is unchanged, so existing importers keep loading and running.
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
- **Per-user preferences.** An importer with `user-storage@0.1` can read its settings during an
  import and stage `set` or `remove` calls, for example to remember the options used for a format.
  Settings are saved only after the imported design is valid and the host accepts the successful
  import. A failed or cancelled import, including an invalid output design, saves no settings.
  The [per-user settings rules](#per-user-settings-user-storage01) below apply unchanged, including
  key and byte limits and the scope checks for deleting settings when the plugin is uninstalled.
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
  without compiling again.
- **Compiled plugins are kept.** A plugin compiles once per version: SolidCraft keeps the compiled
  code with Wasmtime's compilation cache in the `plugin-compile` folder of its per-user cache folder
  (`%LOCALAPPDATA%\SolidCraft\plugin-compile`, `~/Library/Caches/SolidCraft/plugin-compile`, or
  `~/.cache/solidcraft/plugin-compile`), never in the plugin's or a design's folder. It is safe to
  delete: the next session compiles again. A changed plugin, SolidCraft version or setting gets its own
  entry; a damaged entry is compiled again, and a folder that can't be written only means compiling
  each session. The folder holds machine code SolidCraft runs, so it is as trusted as the app itself.
- **First file of a new plugin version.** When nothing is cached yet, the first call compiles the
  component with Wasmtime's quick baseline compiler (Winch), so a large importer starts in a second
  or two rather than half a minute. Once that call is done, the optimising compiler (Cranelift)
  compiles it again on one background thread; later files use its faster code, and it is what goes
  into the cache, so the next session loads it at once. Nothing waits for it. Fuel, memory, the
  deadline and Cancel hold the same under both compilers.
- **Limits.** An importer's component may be up to 64 MiB, its deadline up to 10 minutes, and its
  `stack_kib` up to 64 MiB (a command may ask for at most 1 MiB). Its fuel and memory must fit the user's default limits in Preferences › Plugins
  unless the user approved more for this exact package at install. Build it for `wasm32-unknown-unknown` and wrap it with
  `wasm-tools component new`: the host links no WASI, so a component that imports any is refused.

## Exporters

An exporter writes the open design as a file of another format. It is an export slot in the sense
of RFC 0015 §4: its manifest says `"capabilities": ["formats@0.2"]`, and each of its entries has
`"kind": "exporter"`, the extensions it writes, an optional `label` for the Export list, and its
options as `input_schema`, drawn as a form before the export. The commands it reads the design with
are listed in `runtime.commands`:

```json
{"id": "sample-mesh", "name": "Sample Mesh", "version": "1.0.0", "capabilities": ["formats@0.2"],
 "runtime": {"module": "sample.wasm", "commands": ["document.inspect", "model.mesh"],
   "entries": [{"name": "write", "kind": "exporter", "extensions": ["smp"], "label": "Sample mesh",
                "input_schema": {"type": "object", "properties": {"units": {"type": "string", "enum": ["mm", "in"], "default": "mm"}}}}],
   "limits": {"fuel": 100000000000, "memory_mib": 512, "deadline_ms": 120000}}}
```

- **Exporting.** `file.export` writes a file whose extension SolidCraft does not write itself with
  the installed, enabled exporter that claims it; `format` (`plugin_id.entry`, listed in
  `plugin.list`'s `exporters`) picks one for any path, and `options` passes the slot's options.
  Two exporters claiming one extension need `format`. File › Export lists each slot as
  "Export <label> (.ext)…", and the Plugins menu has it too: its dialog shows the options, then
  chooses where to save. Neither the command nor a script ever opens a dialog.
- **The world.** The component implements `exporter-plugin`: it exports
  `formats-provider.export-file(slot, options)`, which returns the file's bytes (at most 256 MiB),
  and imports `commands`, `log`, `control` and `clock`. `commands.run` runs only the commands the
  manifest lists, only those that read the design (`effect: read`) and reach no file or other design;
  a manifest that lists another is refused when it is read. The commands run on a copy of the open
  design that is thrown away, so an export never changes the design or its undo history.
- **Files.** The plugin reaches no file at all: it has no `files` import and may ask for no folder,
  command group or `input_folder`. The app writes the bytes it returns to the path the user chose
  (subject to the caller's file scope), in one step through a temporary file beside it, and only
  when the whole export succeeded. An export that fails, traps, runs out of fuel or is cancelled
  leaves the folder as it was.
- **Plugin data.** `"needs_plugin_data": true` marks a slot that will also read the design's own
  data of its plugin (RFC 0015 §5). The flag is accepted, but exporters do not yet receive plugin
  data. The export runs without it and its reply carries a warning saying so. The `storage@0.1`
  capability is available to command plugins (below).
- **Limits.** As an importer: a component of up to 64 MiB, a deadline of up to 10 minutes and a
  `stack_kib` of up to 64 MiB, with fuel and memory within the user's ceilings.
- **One kind per plugin.** A runtime's entries are all commands, all importers or all exporters, so
  a format that is both read and written ships as two plugins for now.

## Errors

Every plugin failure starts with the plugin's name, and an importer's or exporter's with the file's name too.
Programs get the structured error (`kind` and its fields) from the engine as well as the text. The
text comes from the message catalog (`plugin_error.*`) in the window's language; the command line
and MCP give the English.

| What happened | What the user reads |
|---|---|
| `unsupported` from the plugin | Sample does not support sketch.smp: the model has no solids |
| `invalid-input` from the plugin | Sample could not open broken.smp: (its reason) |
| More memory than plugins may use | Sample could not open big.smp: it asks for 2048 MiB of memory, and plugins may use 128 MiB. Install the plugin again and allow its memory request, or raise Max plugin memory (MiB) in Preferences › Plugins (in solidcraft-cli, max_memory_mib in the --plugin-limits file). |
| More fuel than plugins may use | ... it asks for N fuel, and plugins may use M. Install the plugin again and allow its fuel request, or raise Max plugin fuel in Preferences › Plugins (in solidcraft-cli, max_fuel in the --plugin-limits file). |
| It ran out of fuel, memory, stack or time | ... it used all N fuel it asks for and was stopped. Ask its author to raise its fuel limit. (memory in MiB, stack in KiB, time in seconds) |
| Its design is over 256 MiB | ... the design it made is larger than 256 MiB, the most a plugin can hand over. |
| A trap | ... it stopped on an error inside the plugin (details). Report this to its author. |

`plugin.inspect` words `limits_error` the same way. The structured kinds are `unsupported`
(`reason`), `over_ceiling` (`limit` is `fuel` or `memory_mib`, with `requested` and `allowed`) and
`exhausted` (`limit` is `fuel`, `memory_mib`, `stack_kib`, `deadline_ms` or `design_mib`, with
`allowed`). The engine's own numbers for a stopped run (fuel used and left) go to the debug log.

## Plugin data in a design

A plugin chooses the lifetime of each setting. Keep values that determine a particular part
with the design using `storage@0.1`. Keep personal defaults, such as a shop's preferred hinge
or sheet price, with the user using `user-storage@0.1`. A plugin can request both capabilities.

### Per-user settings (`user-storage@0.1`)

Add `"user-storage@0.1"` beside `"commands@0.2"` or `"formats@0.2"` in the manifest's
capabilities. No `runtime.data_format` is needed unless the plugin also uses design storage.
The `craft:plugin/user-storage@0.2.0` host interface is available in `command-plugin` and the
additive `importer-plugin-with-storage` world. Both offer:

| Call | Result |
| --- | --- |
| `get(key)` | The JSON string, or `none` for an absent key |
| `set(key, value)` | Saves a JSON value supplied as a string |
| `remove(key)` | Whether the key existed |
| `list()` | The stored keys, sorted by name |

All four return `result<…, plugin-error>`. JSON `null` is a value, distinct from an absent key.
Only the calling plugin's entries can access its settings: no call accepts another plugin id
or a file path. The manifest capability is required for every call; importing the interface
alone grants no access. Deleting settings when uninstalling obeys the caller's file scope as
described below. Existing command plugins and importers continue linking without the new import.

Each plugin can keep at most **128 keys** and **65,536 bytes (64 KiB)** for the entire compact
JSON object, including keys, values, quotes, braces and separators. Keys contain 1–128 UTF-8
bytes without control characters. Invalid keys or JSON return `invalid-input`. Exceeding a
budget returns typed `limit-exceeded` naming `user-storage keys (128)` or
`user-storage total bytes (65536)`. Each `set` argument accepts at most 1 MiB of JSON text
before parsing (`limit-exceeded: user-storage JSON input bytes (1048576)`); whitespace does not
consume the compact map's budget. A refused write leaves the previous data intact.

`set` and `remove` are refused from `read` entries and dialog hooks, like design-storage writes.
A file-aware hook may read its own settings with `get` and `list`; its read transaction is discarded
when the hook finishes, and it still has only the bounded file prefix and no folder grants. Changes are
staged while the entry runs and published when the host commits it. Guest failures discard
staged settings. Importers may read throughout the import; their writes are published only after
the host validates and accepts the successful import. Failed or cancelled imports publish no
settings. The host remembers the loaded file's SHA-256 hash (including whether it was
absent) and checks it again under a per-plugin file lock before replacing the map. If the file
changed, or another run holds the lock, commit refuses with the structured host error
`{"kind":"user_storage_conflict", "plugin":"gears"}`. No settings or design changes from that
transaction are published. Retry the whole plugin call to load the latest settings and apply its
changes again; even changes to different keys require a retry. A read without staged changes
can still succeed after another run commits. The lock is released on close or process exit;
the separate `<plugin-id>.lock` file stays beside the JSON file so simultaneous processes use
the same lock. As with existing granted file writes, a later file-publication error may report
failure after settings have committed. A user-settings write **does not edit the design or add
an undo step**. Undoing a modelling operation does not undo the user's settings. A settings-only entry declares `edit`, since it
writes, but its successful run leaves the design unchanged.

SolidCraft saves one `plugin-user-data/<plugin-id>.json` file in the user's settings folder,
using an atomic replacement. It never embeds these settings in a design or shares them with
another plugin. Desktop, scripts, MCP and CLI plugin calls use the same folder:
`$XDG_DATA_HOME/solidcraft` (default `~/.local/share/solidcraft`) on Linux,
`~/Library/Application Support/SolidCraft` on macOS, and `%APPDATA%/SolidCraft/data` on Windows.
`SOLIDCRAFT_SETTINGS_DIR` selects an isolated settings folder for desktop and headless runs;
`--plugins-dir` changes installed packages, not the settings folder.
`plugin test` gives each case a temporary settings folder and removes it after the run.

Removing a plugin in Preferences offers to remove its saved user settings too; the box starts
unchecked (keep). Scripts and MCP may omit `remove_user_data`:
`plugin.remove {"id":"gears"}` and `plugin.remove {"id":"gears", "remove_user_data":false}`
keep settings for reinstallation; `remove_user_data:true` deletes that plugin's settings.
Deleting settings also requires the caller's file scope to allow the settings record; otherwise
removal refuses with typed `permission_denied` before changing the package, grants or settings.
These programmatic calls never prompt.
Revoking approvals or disabling the plugin keeps its settings.

### Design storage (`storage@0.1`)

A design container reserves `plugins/<plugin-id>/` for a plugin's own files. They are kept and
saved back untouched when the plugin is missing, and opening a design never fails because a plugin
is missing.

A command plugin keeps its own data there with the `storage@0.1` capability and a data format
number. Its manifest lists the `storage.*` commands it calls, like any other command, and it
reads and writes its files' bytes through the `storage` interface (`plugin.wit`), never as text:

```json
{"id": "gears", "name": "Gears", "version": "1.4.0", "capabilities": ["commands@0.2", "storage@0.1"],
 "runtime": {"module": "gears.wasm", "entries": ["make"], "data_format": 3,
   "commands": ["solid.box", "storage.get", "storage.patch"]}}
```

- **What it keeps.** One main JSON document (`data.json`, at most 16 MiB) and files of its own
  (`blobs/<name>`; with `cache: true`, `cache/<name>`, files it can make again). Files are at most
  256 MiB per plugin per design, and all plugin data in a design at most 256 MiB.
  SolidCraft writes `meta.json` beside them: the plugin's id, name and version and the data's format.
- **Reading.** `storage.get` returns the main document, the data's `format`, the running plugin's
  `plugin_format`, `read_only` and the list of files with their sizes. A main document larger
  than the 1 MiB a command result may be is read in parts as its saved JSON bytes:
  `storage.get-document(plugin?, offset, len)`, up to 1 MiB each; join them and parse. A file is
  read the same way, `storage.get-blob(plugin?, name, offset, len)`, its bytes as they are; past
  its end the bytes are empty. Inside a plugin's run these reach only that plugin's own data.
  Agents and scripts read any plugin's data by naming it: `storage.get {"plugin": "gears"}`.
- **Writing.** `storage.set {document}`, `storage.patch {patch}` (an RFC 7396 merge patch),
  `storage.put-blob(name, data, cache, append)` (the file's bytes, up to 1 MiB a call; `append`
  writes a large file in parts) and `storage.delete_blob {name}` change only the running plugin's
  own data, and only from its run: the whole run is one undo step, and a failed run changes
  nothing. Called from anywhere else they are refused. A `read` entry can't call them.
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
