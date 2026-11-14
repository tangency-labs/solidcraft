# The command engine

Every user-visible action is a **command**: a `CommandSpec` registered in
`crates/engine/src/cmd/`, with a dotted, lower-case id (`solid.extrude`, `sketch.create`,
`timeline.rollback`). The toolbar, dialogs, marking menu, S box, scripts, the CLI, the control
channel and MCP all run commands through `Session::execute(id, params)`. Commands take JSON
parameters and never open dialogs; a dialog is a front-end form that builds the parameters.

```rust
CommandSpec::new("solid.fillet", "Fillet", fillet)
    .at("SOLID", "MODIFY")      // toolbar tab and panel ("" = not on the toolbar)
    .icon("fillet")             // an SVG master in crates/look/icons/design/svg
    .key("F")                   // default shortcut
    .params("edges: [[x,y,z] | {body, index}]; radius: expr; …"),
```

`run` is `fn(&mut Session, &Value) -> Result<Value>`, and `enabled` is
`fn(&Session) -> Result<(), String>`; the error string is the disabled-button tooltip. The
`params` string is the one-line documentation shown by `solidcraft-cli commands`, MCP's
`list_commands` and the [command reference](../reference/commands.md).

A command that changes the design is undoable by default: the session snapshots the document
before it runs, and a failing command leaves the document unchanged. Feature commands add a
feature to the timeline and let the document evaluate it; they validate their inputs first
(units, ranges, references) and return `BadParams` instead of building a broken feature.

Only the registry's own dotted ids name commands (matched ignoring case); any other id fails
with `unknown command`, which names up to three close ids: a typo's (`solid.extrud`), or one
ending in the same word (`file.open`: did you mean `doc.open`?).

## Plugin package commands

Installation, discovery and removal use the command surface. Programmatic calls never open
file pickers or permission dialogs. The commands expose input and output schemas through
`engine.describe`:

| Command | Input | Output |
|---|---|---|
| `plugin.install` | `{path, replace?, dry_run?, approve?: {command_groups, folders: [{name, path, read, write}]}}` | `{id, name, version, description, capabilities, permissions, commands, entries, entry_points, replaces?, installed, folder}` |
| `plugin.list` | `{}` | `{plugins: [{manifest, commands, disabled}]}` |
| `plugin.remove` | `{id}` | `{id, removed: true}` |

`plugin.install` reads one bounded zip through `solidcraft_io::safe_zip` and publishes it through
the existing hidden-folder-and-rename pipeline. `dry_run: true` checks and describes the package,
including supplied approvals, without changing files or grants. `entry_points` contains the declared
`craft_plugin::Entry` objects with names, effects, input schemas and slots; `entries` contains
namespaced command ids. Replacing an installed plugin requires `replace: true`.

Approval grants may only narrow manifest requests. An unrequested group, folder name or access mode
returns a typed error. Each folder approval contains `name`, an absolute local `path`, `read` and `write`.
Omitting `approve` grants nothing, including on replacement. Providing `approve` replaces that plugin's
stored set; stale grants cannot survive replacement. The engine stores grants after publication and
`plugin.run` checks them against the manifest. Desktop preferences persist the grants per plugin id.
Removal validates the id, confines deletion to the host's plugin directory and removes its stored
grants. These operations do not edit the design.

On desktop, `plugin.install` and `plugin.remove` require separate `engine.execute` requests.
`engine.batch` and `engine.script` reject them during preflight, before any earlier step runs.
Direct calls use the package worker. The Preferences install action and Plugins menu both call
`plugin_install::start(app)` for file selection and the single confirmation screen.
There is no separate package review command or permission screen.

Preferences, the Plugins menu and host slots share `plugins::inventory`.
`plugin_menu::list` adds the current enabled state; Preferences reuses slot discovery's snapshot.
File, Help and ribbon clicks resolve a namespaced entry against that
list and call `plugin_menu::open_entry(app, p, e)`, as the Plugins menu does. The shared dialog draws
the entry's `input_schema`, checks its enabled state and grants, and submits through the existing
worker path. Unknown menu names draw no host rows; every entry remains in the Plugins menu.
Unknown ribbon names use the Plugins ribbon tab. Opening a dialog never executes an entry.
Execution still requires explicit approval and the package identity checked by `plugin.run`.

See [Plugins](../../../plugins.md) for placement, package limits and integration contracts, and
[the runtime reference](../../../plugin-runtime.md) for execution and file scope.

## Adding a command

1. Add a `CommandSpec` to the right file in `crates/engine/src/cmd/` and implement `run`.
2. Give it input and output schemas: a parameters type (its contract) and a reply type in the
   module for its command file under `crates/engine/src/schema/` (`schema/features.rs` for
   `cmd/features.rs`, and so on; create it for the file's first contract), built from the shared
   field types in `schema/types.rs`. Register them with `checked!(…)`, `.contract(…)` and
   `.output(…)`, as `solid.extrude` does, and add the command to that module's
   `schemas_and_the_key_check_agree` test, with error-message cases and a reply test. The
   parameters type documents each parameter (keep the `.params(…)` prose too, for now), and
   `describe_command` serves both schemas to agents and scripts. A test
   (`every_command_has_input_and_output_schemas`) warns for a command without them, and will
   fail it once the rule becomes required; the commands
   that predate the rule are listed in `schema/unconverted.txt`, which only shrinks.
   [Command schemas](schemas.md) walks through it, with the shared types and a checklist.
3. Write engine tests that measure the result: volume, area, and face/edge/vertex counts
   against values you can derive by hand.
4. The hostile-input test (`hostile_params_never_panic` in `crates/engine/src/tests.rs`) runs
   your command automatically with every documented parameter set to junk. It must return
   errors, never panic.
5. For UI work, add a scenario (see [Test harnesses](testing.md)).
6. A command the ribbon shows (through `.at(…)` or a section in `crates/ui-egui/src/workspace.rs`)
   ships with its help tooltip: two short sentences, what the tool does and then when to use it,
   in your own words. Add the message `help.<id>` to a new file in `assets/locales/pending/`
   (English, every catalog in `COMPLETE` in `crates/ui-egui/src/i18n.rs`, which a test requires,
   and any other language you can write) and the id to `DOCUMENTED` in
   `crates/ui-egui/src/command_help.rs`. The test
   `every_toolbar_command_has_help` fails for a ribbon command without help
   (#2758).
