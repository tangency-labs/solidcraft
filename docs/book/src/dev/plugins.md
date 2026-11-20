# Writing a plugin

This guide goes from an empty folder to two working plugins: a command that adds a plate to the
open design, and an importer that opens a file of another format as a new design. It covers the
manifest, the WIT interface, building the component, permissions, limits, checking, testing and
installing.

The reference for everything here is [Plugins](../../../plugins.md) (placement, dialogs, package
limits, importers) and [the runtime reference](../../../plugin-runtime.md) (execution and file
scope). The design behind them is [RFC 0002](../../../rfcs/0002-plugins.md).

## What a plugin is

A plugin is a folder (shipped as a `.zip`) holding a manifest, `plugin.json`, and one WebAssembly
component. SolidCraft runs the component in a sandbox:

- A **command plugin** runs SolidCraft's own commands (the ones in `solidcraft-cli commands`, which
  MCP clients use too) on the open design. One run is one undo step, and a failure changes
  nothing.
- An **importer** reads a file of a format SolidCraft doesn't read and writes a SolidCraft design.
  The design opens as a new, unsaved design, with a report of what could not be carried over.

A plugin has no network access, ever. It reads and writes files only in folders the user approved,
and it runs within fuel, memory and time limits. It ships no UI code: SolidCraft draws its dialog
from the input schema in the manifest.

Plugins can be written in any language that compiles to a WebAssembly component. This guide uses
Rust.

Both examples are in SolidCraft's source, in `docs/book/plugin-examples`, and are built against
the current WIT with every change to it (`cargo xtask plugin-examples`), so the code here
compiles.

## What you need

1. Rust, with the WebAssembly target: `rustup target add wasm32-unknown-unknown`.
2. `wasm-tools`, which wraps a WebAssembly module as a component: `cargo install wasm-tools`.
3. `solidcraft-cli`, which comes with SolidCraft. It checks and tests plugins.

## Lay out the folders

Keep the package (what ships) apart from the source (what builds it):

```text
plate-maker/            the package: what the .zip holds
  plugin.json
  plate-maker.wasm      the built component
  tests/                the plugin's test cases (left out of the .zip)
plate-maker-src/        the Rust crate that builds the component
  Cargo.toml
  src/lib.rs
  wit/plugin.wit        copied from SolidCraft
```

The package folder is named after the plugin's `id`.

## Step 1: the manifest

Create `plate-maker/plugin.json`:

```json
{
  "id": "plate-maker",
  "name": "Plate Maker",
  "version": "0.1.0",
  "description": "Adds a square plate.",
  "capabilities": ["commands@0.2"],
  "runtime": {
    "module": "plate-maker.wasm",
    "commands": ["solid.box"],
    "entries": [{
      "name": "plate",
      "effect": "edit",
      "input_schema": {"type": "object", "properties": {
        "size": {"type": "number", "title": "Size", "default": 40, "minimum": 1, "unit": "mm"},
        "thickness": {"type": "number", "title": "Thickness", "default": 3, "minimum": 0.1, "unit": "mm"}
      }},
      "slots": [{"kind": "ribbon", "name": "Model"}]
    }],
    "limits": {"fuel": 1000000000, "memory_mib": 32, "deadline_ms": 5000}
  }
}
```

| Field | What it says |
|---|---|
| `id` | 1 to 64 lowercase letters, digits and dashes. The package folder has this name. |
| `name`, `version`, `description` | What the user sees: at most 80, 32 and 240 characters. |
| `capabilities` | The host interfaces the plugin uses: `commands@0.2` for a command plugin, `formats@0.2` for an importer, plus `evaluate@0.2` for an importer that checks its result. Plugins built for 0.1 still run. |
| `runtime.module` | The component's file name, beside `plugin.json`. |
| `runtime.commands` | Every SolidCraft command the plugin calls, by exact id. Anything else is refused. |
| `runtime.entries` | What the plugin offers. Each entry has a `name`, an `effect` (`edit`, or `read` for one that only reads), an `input_schema` (a JSON schema of an object) and optional `slots`. |
| `runtime.limits` | The fuel, memory and time one run may use (see [Limits](#limits)). |
| `runtime.permissions` | What else it needs (see [Permissions](#permissions)). Leave it out to ask for nothing. |

The input schema is the entry's dialog: SolidCraft draws a field per property, with the `title`,
`default`, limits and `unit`, and passes the values to the plugin as JSON. A field can also take
geometry picked in the viewport, a dropdown, or a picture per option: see
[Plugins](../../../plugins.md#integration-apis). `slots` place the entry in a ribbon tab or the
File or Help menu; every entry is in the Plugins menu anyway.

## Step 2: the crate

Create the crate beside the package folder:

```sh
cargo new --lib plate-maker-src
cd plate-maker-src
cargo add wit-bindgen serde_json
```

In `Cargo.toml`, make it a library WebAssembly can load, and keep the release build small:

```toml
[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
strip = true
```

## Step 3: the interface

The WIT file is the contract between SolidCraft and the plugin: what the plugin may call, and what
it must export. Copy it from SolidCraft's source, `crates/craft-plugin/wit/plugin.wit`, into the
crate's `wit` folder:

```sh
mkdir wit
cp <SolidCraft source>/crates/craft-plugin/wit/plugin.wit wit/plugin.wit
```

Its package is `craft:plugin@0.2.0`, and it has two worlds:

- `command-plugin` imports `commands` (run a SolidCraft command), `log`, `control` (progress and
  cancel), `files` (granted folders) and `clock`, and exports `entry`, whose `execute(entry, params)`
  SolidCraft calls with the entry's name and its parameters as JSON.
- `importer-plugin` imports `source` (the file being opened), `design` (where the new design
  goes), `evaluate`, `log`, `control` and `clock`, and exports `importer`.

Errors are a `plugin-error` variant: `permission-denied`, `command-failed`, `limit-exceeded`,
`invalid-input`, `unsupported` and the others name their cause, and the user sees them after the
plugin's name. Return `unsupported` for a valid file or option your plugin does not handle ("the
model has no solids"); `invalid-input` is for input that is broken.

## Step 4: write the component

`src/lib.rs`:

```rust
wit_bindgen::generate!({ path: "wit", world: "command-plugin" });

use craft::plugin::commands;
use craft::plugin::types::{FieldFailure, PluginError};
use exports::craft::plugin::entry::Guest;
use serde_json::{Value, json};

struct Plugin;

impl Guest for Plugin {
    fn execute(entry: String, params: String) -> Result<String, PluginError> {
        let params: Value = serde_json::from_str(&params).map_err(|e| invalid("params", e))?;
        match entry.as_str() {
            "plate" => {
                let size = params["size"].as_f64().unwrap_or(40.0);
                let thickness = params["thickness"].as_f64().unwrap_or(3.0);
                let args = json!({"length": size, "width": size, "height": thickness});
                // The same command, parameters and reply as `solidcraft-cli exec solid.box`.
                commands::run("solid.box", &args.to_string())?;
                Ok(json!({"plate": [size, size, thickness]}).to_string())
            }
            _ => Err(invalid("entry", format!("no entry `{entry}`"))),
        }
    }
}

fn invalid(field: &str, reason: impl ToString) -> PluginError {
    PluginError::InvalidInput(FieldFailure { field: field.into(), reason: reason.to_string() })
}

export!(Plugin);
```

`commands::run` takes a command id and its parameters as JSON and returns its reply as JSON.
`solidcraft-cli commands` lists every command with its parameters, and `solidcraft-cli exec <id>
'{...}'` tries one on an empty design. An error from a command (or `?` on it) ends the run, and
SolidCraft undoes everything the run did.

## Step 5: build it

In `plate-maker-src`:

```sh
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new target/wasm32-unknown-unknown/release/plate_maker_src.wasm -o ../plate-maker/plate-maker.wasm
cd ..
```

Build for `wasm32-unknown-unknown`, not a WASI target: SolidCraft links no WASI, so a component
that imports any is refused. `wasm-tools component new` turns the module into a component. The
steps after this one run in the folder that holds `plate-maker` and `plate-maker-src`.

## Step 6: check it

```sh
solidcraft-cli plugin check plate-maker
```

```text
plate-maker 0.1.0 (command plugin): no problems
entry   plate-maker.plate (command; fields: size, thickness)
```

`plugin check` runs the checks SolidCraft runs when it installs and runs a plugin, without running
the plugin: the manifest and its size, the permissions, the limits against the app's ceilings, and
that the component is one SolidCraft can link (its world, and the version of the WIT it was built
from). Each problem comes with how to fix it, and the command exits with an error when there is one:

```text
plate-maker 0.1.0 (command plugin): 1 error(s), 0 warning(s)
error   runtime.permissions.net: asks for network access (net)
        fix: Remove "net": SolidCraft gives plugins no network access. Read and write files through named folder grants instead.
```

It takes the package folder or the `.zip`, and `--json` prints the same as data, the reply of the
`plugin.check` command (which MCP clients can call too). In a folder, `tests/` and hidden files are
left out, as they are out of the `.zip`.

## Step 7: test it

A plugin's tests are JSON files in its `tests` folder, one case per file. Create
`plate-maker/tests/default-plate.json`:

```json
{
  "entry": "plate",
  "params": {"size": 40, "thickness": 3},
  "expect": {
    "reply": {"result": {"plate": [40, 40, 3]}},
    "model": {"body_count": 1, "total": {"volume_mm3": 4800}}
  }
}
```

and run them:

```sh
solidcraft-cli plugin test plate-maker
```

```text
pass  default-plate (plate), 41 ms
plate-maker 0.1.0: 1 passed, 0 failed, 0 skipped
```

`plugin test` checks the package first, as `plugin check` does, and installs it into a scratch
folder with SolidCraft's own installer. Each case then runs on a new session, through the same
`plugin.run` (or, for an importer, `file.import`) the app uses, with its real limits. Nothing is
installed for you, and your files are not touched. A case is:

| Field | What it says |
|---|---|
| `entry` | The entry to run. |
| `params` | A command's parameters, or an importer's options. Default `{}`. |
| `design` | A command runs on this design, in the `tests` folder. Default: an empty design. |
| `file` | An importer opens this file, in the `tests` folder. |
| `grants` | What the user approved: `command_groups`, `folders` (`[{"name", "read", "write", "from"}]`, each a fresh folder, or a copy of `from` in `tests`), `input_folder` and `memory_mib`. Default: everything the manifest asks for, as Install approves it. |
| `expect.error` | The run fails with this kind of error: `permission_denied`, `command_failed`, `limit_exceeded` (or, more exactly, `exhausted` when a budget ran out and `over_ceiling` when the app refused it before it ran), `invalid_input`, `unsupported`, `trap`, `cancelled`, or `any`. Default: it succeeds. |
| `expect.message` | Text the error contains. |
| `expect.reply` | Values the reply holds: `plugin.run`'s is `{"result": <what the entry returned>}`; `file.import`'s has `bodies`, `features`, `errors` and the `loss_report`. |
| `expect.model` | Values `inspect.measure` gives for the resulting design, as `solidcraft-cli eval` prints them: `body_count`, `total.volume_mm3`, each body. |
| `expect.files` | Files the plugin wrote, as `<folder name>/<path>`. |
| `expect.tolerance` | How far a number may be off: relative, or absolute below 1. Default 1e-6. |

An expected object needs only the keys you list; an array needs every item. An entry with no
case runs once with its fields' defaults, if it needs nothing else, and is skipped otherwise.
`--json` prints pass or fail per case as data, and `--out DIR` keeps each case's resulting design
and log for a closer look. The command exits with an error when a case fails, so a CI job can run
it; `--require-cases` also fails the run when an entry is skipped, so CI catches an entry nobody
tested. Given the `.zip`, `plugin test` runs the cases in the `tests/` folder the `.zip` holds;
the `.zip` from Step 8 leaves them out, so test the folder.

## Step 8: install it

Zip the package, without its tests:

```sh
cd plate-maker && zip -r ../plate-maker.zip plugin.json plate-maker.wasm && cd ..
```

In SolidCraft, open **Preferences › Plugins**, choose **Install plugin…** and pick the `.zip`.
The confirmation screen shows what the plugin asks for; **Install** approves it. The entry appears
in the Plugins menu and the Model tab at once. A program installs it with `plugin.install`, which
never opens a dialog: send `"dry_run": true` first to review what it asks for (see
[Installing](../../../plugins.md#installing)).

## Permissions

A plugin asks for everything beyond the open design in `runtime.permissions`, and the user approves
it at install:

```json
"permissions": {
  "command_groups": ["file"],
  "files": [{"name": "exports", "write": true}],
  "clock": true
}
```

- `command_groups`: commands that reach past the open design are grouped: `file` (save, export,
  import, libraries), `doc` (open, create or switch designs), `app` (settings, the window) and
  `plugin` (other plugins). A listed command of a group needs the group here, and the user's grant.
- `files`: named folders the plugin reads (`read`) or writes (`write`) through the `files`
  interface, by name and a relative path. The user picks each folder at install; the plugin never
  sees where it is. Writes appear only when the run succeeds.
- `clock`: the `clock` interface tells the time; without it, it says 0.
- `input_folder` (importers only): read the other files in the folder of the file being opened,
  such as an assembly's parts.
- `net` is refused. SolidCraft gives plugins no network access, and a component that imports a
  socket or HTTP interface is refused too.

Test a refusal by narrowing a case's `grants`: with `"grants": {}`, a call that needs a group or
folder fails with `permission_denied`.

## Limits

| Limit | Command | Importer |
|---|---|---|
| `fuel` (WebAssembly instructions, roughly) | up to the user's ceiling, 100,000,000,000 by default | the same |
| `memory_mib` | 1 to 4096; above the user's ceiling (128 by default) the user approves it at install | the same |
| `deadline_ms` | at most 30,000 | at most 600,000 |
| `stack_kib` | 64 to 1024 | 64 to 65,536 |
| Component size | 16 MiB | 64 MiB |
| `plugin.json` | 16 KiB | 16 KiB |
| Parameters and replies | 1 MiB of JSON each | 1 MiB per read or write, 1 GiB read, a 256 MiB design |

A run that goes past its fuel, memory, stack or deadline stops with `exhausted`, which names the
budget and its size ("it used all 100000 fuel it asks for and was stopped"), and nothing it did is
kept. `plugin check` says when a request is over the app's ceilings, and `plugin test` runs
under them.

## An importer

An importer turns a file into a design. This one opens `.box` files, text with a box's length,
width and height (`20 10 3`).

The manifest, `box-importer/plugin.json`:

```json
{
  "id": "box-importer",
  "name": "Box Importer",
  "version": "0.1.0",
  "capabilities": ["formats@0.2"],
  "runtime": {
    "module": "box-importer.wasm",
    "commands": [],
    "entries": [{"name": "convert", "kind": "importer", "extensions": ["box"]}],
    "limits": {"fuel": 1000000000, "memory_mib": 32, "deadline_ms": 10000}
  }
}
```

An importer's entries have `"kind": "importer"` and the extensions they open, lowercase without
the dot. It calls no commands; its `input_schema`, if any, is its options.

The component implements the `importer-plugin` world. It reads the file through `source`, in
chunks of at most 1 MiB, and writes a SolidCraft design through `design`. A design file is what
SolidCraft saves: a `.sc` file, or the same design as plain JSON. To see the JSON for a design,
build it with a script and save it as `.json`: `solidcraft-cli run box.json --save design.json`,
where `box.json` is `{"commands": [{"command": "solid.box", "params": {"length": 20, "width": 10,
"height": 3}}]}`. Copy its `format` from a design your SolidCraft saves.

```rust
wit_bindgen::generate!({ path: "wit", world: "importer-plugin" });

use craft::plugin::types::{FieldFailure, PluginError};
use craft::plugin::{design, source};
use exports::craft::plugin::importer::Guest;
use serde_json::json;

struct Importer;

impl Guest for Importer {
    fn import_file(_entry: String, file: source::FileInfo, _options: String) -> Result<String, PluginError> {
        // The path "" is the file being opened.
        let mut bytes = Vec::new();
        while (bytes.len() as u64) < file.size {
            let chunk = source::read("", bytes.len() as u64, 1 << 20)?;
            if chunk.is_empty() {
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8(bytes).map_err(|e| invalid("file", e))?;
        let numbers: Vec<f64> = text.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|e| invalid("file", e))?;
        let [length, width, height] = numbers[..] else {
            return Err(invalid("file", "a .box file holds three numbers: length, width and height"));
        };
        let made = json!({
            "format": "solidcraft/57", "name": "", "units": "mm", "params": [],
            "features": [{"id": 1, "name": "Box1", "suppressed": false, "body_names": ["Body1"],
                "param_names": ["d1", "d2", "d3"], "type": "box", "corner": [0.0, 0.0, 0.0],
                "length": length.to_string(), "width": width.to_string(), "height": height.to_string(),
                "operation": "new_body"}],
            "next_id": 2, "reserved_body_names": ["Body1"], "next_component_color_index": 0
        });
        for chunk in made.to_string().as_bytes().chunks(1 << 20) {
            design::write(chunk)?;
        }
        // The loss report: what could not be carried over. A box loses nothing.
        Ok(json!({"summary": format!("A {length} x {width} x {height} mm box"), "items": []}).to_string())
    }
}

fn invalid(field: &str, reason: impl ToString) -> PluginError {
    PluginError::InvalidInput(FieldFailure { field: field.into(), reason: reason.to_string() })
}

export!(Importer);
```

A design with no name is named after the file. The loss report lists each thing the importer could
not carry over as it was: `{"severity": "info|degraded|dropped", "title": "...", "subject": "...",
"message": "...", "code": "..."}`, where `title` names the kind of loss in the user's words and
`code` is your stable code for it. SolidCraft shows one line per title, worst first.

Build and check it as before. Its test opens a file in `tests`: `box-importer/tests/small.box`
holds `20 10 3`, and `box-importer/tests/small.json` is

```json
{
  "entry": "convert",
  "file": "small.box",
  "expect": {
    "reply": {"bodies": 1, "loss_report": {"summary": "A 20 x 10 x 3 mm box"}},
    "model": {"total": {"volume_mm3": 600}}
  }
}
```

A broken file is a case too: a file holding `20 10` with `"expect": {"error": "invalid_input"}`. A
valid file the importer does not convert answers `unsupported`, which SolidCraft shows as
"Box Importer does not support part.box: ...".

Once installed, **File › Open** offers `.box` files, and `doc.open` and `file.import` open them.
An importer that converts a large file can ask for more time (`deadline_ms` up to 600,000), stack
(`stack_kib`) and memory; see [Importers](../../../plugins.md#importers), which also covers reading
an assembly's other files and checking the result with `evaluate`.

## When something goes wrong

| Symptom | Cause |
|---|---|
| `plugin check`: "this is a core WebAssembly module, not a component" | The `wasm-tools component new` step is missing. |
| `plugin check`: "component imports instance `wasi:...`" | The crate was built for a WASI target, or uses something in `std` that needs it. Build for `wasm32-unknown-unknown`. |
| `plugin check`: "built against craft:plugin@..." | The WIT came from another SolidCraft version. Copy this version's `plugin.wit` and rebuild. |
| A run fails with `permission_denied: command ... is not granted` | The command is not in `runtime.commands`. |
| A run fails with `exhausted`: "it used all ... fuel" or "ran longer than its time limit" | The run did more work than its `limits` allow: raise them, or do less per run. |
| A run fails with `trap` | The plugin panicked or reached `unreachable`; the message says where. Nothing the run did is kept. |
