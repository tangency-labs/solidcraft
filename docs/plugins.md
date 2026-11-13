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

## Versions

The WIT package is `craft:plugin@0.1.0`. Today's surface is one capability, `commands@0.1`:
running app commands, logging, progress and cancellation, named files, a clock, and the exported
entry. A plugin's `plugin.json` lists what it needs, for example
`"capabilities": ["commands@0.1"]`; the host refuses an unknown or newer capability when it reads
the manifest, with a plain message such as "needs commands 0.2; update SolidCraft or the plugin".
`ui`, `viewport`, `formats` and `storage` arrive later as their own versioned interfaces. While a
version is 0.x, a minor version may break the one before it.

## Commands and permissions

A plugin can call every command in the MCP catalog that its manifest lists. Commands that reach
past the open design are grouped (`file`, `doc`, `app`, `plugin`) and refused unless the
manifest asks for the group in `permissions.command_groups` and the user grants it.
File access requires matching manifest permissions and approved folder grants. Native commands
in the `file` and `doc` groups stay within both those folders and any caller file scope.
Approved plugin folders cannot widen that caller scope. Automatic design-folder scopes and
default plugin file scopes exclude hidden children; explicit caller approvals remain effective
where the plugin's folder grant also allows access. The reserved `net` permission is
refused with a typed error. Traps, missing permissions, failed commands, limits
and invalid manifest fields return structured errors. Later phases add
import/export, author tooling and browser execution after the
phase 1 design review.

## Installing

Install plugin… on Preferences › Plugins (desktop only) picks a plugin `.zip`, shows its name, version, summary and
what it asks for, and installs it on Install. `plugin.install` does the same for programs:
`{"path": "gears.zip"}` installs, `"dry_run": true` only checks and describes it, and replacing an
installed plugin of the same id needs `"replace": true`. The `.zip` holds one plugin, `plugin.json`
and its component, at its top or in one top-level folder. Before anything is written the host parses
the manifest (refusing unknown or newer capabilities), and refuses absolute paths, `..`, symlinks
and device entries, entries over 16 MiB and more than 64 MiB unpacked. It unpacks into a hidden
folder beside the installed plugins and renames that to `plugins/<id>/`; the plugin's entries
resolve at once, with no restart. Installing runs no plugin code.

## Plugin data in a design

A design container reserves `plugins/<plugin-id>/` for a plugin's own files. A supported key is
`<plugin-id>/<relative-path>`, at most 255 bytes including the id and separators. The id has
1..64 lowercase ASCII letters, digits or internal hyphens. Each path component contains ASCII
letters, digits, `.`, `_` or `-`; it must be nonempty and cannot start with `.`. Unsupported names
are ignored.

A design may hold at most 1024 valid, unique plugin files in total, across all plugins.
Current-format data exceeding this limit is refused rather than truncated. An unreadable
supported container part also causes an error rather than being omitted. Saving checks the
file count before writing the container. Within these limits, valid, readable plugin files are
kept and saved back byte for byte when the plugin is missing. A missing plugin itself never
prevents a design from opening.

The `storage` capability will let plugins read and write this data; this version exposes no
plugin storage interface.
