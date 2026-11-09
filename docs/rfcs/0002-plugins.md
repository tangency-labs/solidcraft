# RFC 0002: Plugins, sandboxed WebAssembly extensions

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-10-14 (when #1221 opened) |
| Apps | SolidCraft |
| Discussion | [#1221](https://github.com/tangency-labs/solidcraft/issues/1221) |
| Related | [#4980](https://github.com/tangency-labs/solidcraft/pull/4980) (draft WIT proposal, `solidcraft:plugin@0.1.0`), [#4933](https://github.com/tangency-labs/solidcraft/pull/4933) (plugin commands in Utilities) |

This RFC replaces `docs/plugins.md` and folds in what the 37 comments on #1221 settled. Only the Preferences page exists today. No plugin runs.

## 1. Goals and non-goals

**Goals**
- Drop in a file and get a new command, importer, exporter or sketch tool, without rebuilding the app.
- Sandboxed by default. A plugin can do only what its manifest asks and the user allows. A crash or a hang cannot take down the app or the model.
- Any language that compiles to a WebAssembly component. Rust first.
- One command surface. Plugins change the model only through the engine commands that MCP and the control channel already use, so every edit is undoable and journaled. No private geometry API.
- A plugin built for API 1 keeps loading after app updates.

**Non-goals**
- Not a way to override core commands. Overrides would make a model behave differently depending on what is installed.
- Not where core features go. CAM, for example, stays in-tree (#1153); plugins add vendor posts, niche strategies and importers on top.
- No plugin-to-plugin calls. If plugins ever cooperate, it goes through host events, and a failing listener never fails the caller.

## 2. The WIT contract

World `solidcraft:plugin@1.0.0`. The interface stays thin: a few typed functions plus the JSON command set, so the WIT stays small while the commands keep growing.

| Host import | Notes |
|---|---|
| `run(command, args) -> result<string, string>` | The engine commands, JSON in and out, exactly as over MCP. Filtered by the plugin's permissions. |
| `log`, `progress` | Always available. |
| `read-file`, `write-file` | Only inside folders the user granted. |
| `fetch` | Only with the `net` permission, only to origins named in the manifest, made by the host. |

| Export | Notes |
|---|---|
| `manifest() -> string` | JSON: id, name, version, kind, permissions, parameters. |
| `execute(params) -> result<string, string>` | A command or one-shot feature. |
| `import(...)` / `export(...)` | File formats. See below. |

**Settled in the comments**
- Plugin commands are namespaced `plugin_id.function`. They appear in `list_commands` and the Assistant's tool list with their parameter docs.
- Importers need the file name, not just bytes (the extension picks the format), and return a design plus a loss report. Assemblies need read access to the input's folder, granted as a permission. #4980 streams the input to keep each host call small.
- A trap fails that one call ("the importer failed on this file") and the next call gets a fresh instance.
- Entries declare `effect: read | edit`. A `read` entry is limited to query commands, so the agent chat can run it without asking.
- Entry inputs should be schemas built from the same field types as the command contracts (#2829), so one declaration gives a native dialog, strict MCP checks and generated tests. A one-line `params` doc is accepted until 1.0.

**Permissions** sit in the manifest, are shown on an approval screen, and are approved before any plugin code runs. A change to what was approved stops the plugin until the user approves again. A code-only update never adds an entry or surface silently.

| Permission | Grants |
|---|---|
| (none) | `run`, `log`, `progress` |
| `files` | Named folders, read or write |
| `net` | Named `https` origins. On the web build, only through a host-approved fetch. |
| `clock` | Wall clock. Without it, it reads a fixed epoch. |

## 3. Runtime and sandbox

| Item | Decision |
|---|---|
| Runtime | wasmtime on desktop, with the component model |
| CPU | Fuel or epoch interruption; the loop is stopped and the call fails |
| Memory | A per-plugin limit; a configurable stack (a real decoder needed about 16 MiB, wasmtime's default is 512 KiB) |
| WASI | None unless a permission grants a narrow slice |
| Failure | A fresh instance per call after a trap. The design and undo stack are untouched. |

**Binary-size trade-off.** wasmtime with Cranelift adds several MB to the app. wasmi is much smaller but runs core modules, not components. Options are wasmtime with Winch or Pulley, or accepting the size. Not decided.

**Component size.** #4980 caps components at 16 MiB. A real converter measured 28 MB (built for size) to 42 MB. Compiling happens in the app's process with no sandbox limit on the compiler, and the project forbids `unsafe`, so compiled code cannot be cached (`Component::deserialize` is `unsafe`). One plugin per format avoids most of this.

Rhai was proposed instead. The comments favour one runtime, WebAssembly, for isolation and language choice.

## 4. Plugin kinds

| Kind | Timeline | When |
|---|---|---|
| One-shot command | Optional. It may emit ordinary native features inside one undo step. | First |
| Importer / exporter | None. Opens as a new unsaved design. | First |
| Parametric feature | One `Plugin` feature row per instance, rebuilt when parameters change | Later |
| Panels, overlays, workspaces | Host-drawn from a declared schema | Later, see questions |

**Parametric features** (from `docs/plugins.md`). A host-owned `FeatureKind::Plugin` row stores `plugin_id`, `api_version`, the plugin's JSON `params`, and a `PluginDisplay` (label, icon id from the app's registry, tooltip, type name), all recorded in the file. The timeline draws any such row through one code path. On rebuild the host calls `evaluate(params, ctx)`, which uses the same `run` import, and merges the result like a native feature. An optional `parameter_schema()` lets the host draw sliders and dimension rows.

**SolidCraft never refuses to open a file.** Every parametric plugin declares a recorded fallback, so a design opens without the plugin:

| Fallback | Behaviour without the plugin |
|---|---|
| `recorded_script` | Replays a stored list of native commands, when every command is built in |
| `recorded_mesh` | Shows a stored B-rep or mesh; editing is disabled |
| `suppress_with_warning` | Feature is suppressed with a clear message |
| `error_on_rebuild` | Hard error; for strict teams only |

Shared designs should use one of the first two. A plugin's output should come from native features (sketches, extrudes, holes), so it works without the plugin. If a plugin cannot reach its goal that way, core is missing a building block. Opening a design never downloads or runs a missing package. Each instance keeps its plugin revision until the user updates it, and the update is undoable.

Design metadata under `plugin.<id>` survives a missing, disabled or newer plugin. Plugin store data is read additively across versions, and the first call after an update is told the previous version.

## 5. Distribution and signing

- **Now:** a folder per plugin, `{data}/plugins/<id>/plugin.json`.
- **Next:** Install Plugin from a file or GitHub release. An Add-on Manager lists tools, versions and permissions, with enable, disable and remove.
- **Later:** a curated index. Anyone can sideload. To be listed, a plugin must be open source and signed. The comments favour GitHub Releases as the artifact source and keeping packages independent of one host.
- **Tools:** `solidcraft-cli plugin check <dir>` validates the manifest and schemas; `plugin test <dir>` runs every entry on the real host; `--plugin-dev <dir>` loads an unapproved folder, marked as dev, and reloads on change. The `plugin.json` schema is generated from the host's types.

## 6. Web build

wasmtime does not run in the browser tab. Options are jco-transpiled components, or desktop-only plugins at first. The same WIT world and manifest should serve both, with the same permission checks before `run` reaches the engine. Parametric features offline on the web need the module cached, so only plugins whose fallback allows it. Network access on the web goes through a host-approved fetch, never the page's own.

## 7. What exists today

**Built**
- Preferences > Plugins (`crates/ui-egui/src/plugins.rs`, #2718): lists `{data}/plugins/<id>/plugin.json` folders, searches them, and turns each off. Turned-off ids are saved in `disabled_plugins`. Folders that are not plugins are listed with the reason.
- The manifest it reads has only `id`, `name`, `version` and `description`. It is checked and capped at 16 KiB.

**Not built**
- No wasmtime, no WIT file, no host, no example plugin, no permissions, no signing, no install flow (#4330 was closed), no `Plugin` feature kind. Nothing in the folder is loaded or run.
- #4980 is an open draft of the WIT and a gear pilot, and is not merged.

## 8. Phases

| Phase | Delivers |
|---|---|
| 1 | WIT, a wasmtime host with fuel and memory limits, `execute`, permissions in the manifest, tests: no file access without a grant, a runaway loop is stopped, a crash leaves the model untouched, undo works |
| 2 | Spur-gear example plugin, author guide, `plugin check` and `plugin test`, importers and exporters |
| 3 | `FeatureKind::Plugin`, `evaluate`, recorded fallbacks, generic timeline row |
| 4 | Install from file or GitHub, Add-on Manager, parameter schema dialogs |
| 5 | Web host on the same WIT |
| 6 | Curated, signed index; panels and overlays if agreed |

## 9. Open questions

1. **Wasmtime size and component limit.** Accept the binary growth, and raise the 16 MiB limit after measuring compile memory?
2. **Plugin UI.** Declarative panels, viewport overlays, then workspaces (the slicer-as-a-tab case). Or should such tools live in-tree?
3. **Defaults.** Are newly installed plugins on or off until approved?
4. **Local network.** May a plugin reach a companion service on the user's machine, entered by the user? Starting a print also needs a confirmation level above `edit`.
5. **Extending core commands.** Data-driven extension points, such as thread standards in the Hole dialog, later. Never overrides.
6. **Signing and the index.** What signs, who hosts the index, and who reviews listings?
7. **Web.** jco, or desktop only?
8. **Result annotations.** Text a plugin adds to command results can steer a model, so it needs its own threat model.
9. **Recorded macros.** A plugin kind or a core feature?
10. **Version numbers.** #4980 uses `0.1.0` until the interface settles; this RFC says `1.0.0`.
