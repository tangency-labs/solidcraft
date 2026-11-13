# RFC 0002: Plugins, sandboxed WebAssembly extensions

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-10-14 (when #1221 opened) |
| Apps | SolidCraft |
| Discussion | [#1221](https://github.com/tangency-labs/solidcraft/issues/1221) |
| Related | [RFC 0015](0015-plugin-ui-workspaces.md) (plugin panels, workspaces and viewport content), [#8987](https://github.com/tangency-labs/solidcraft/pull/8987) (native Wasm command plugin runtime, the base for phase 1), [#15100](https://github.com/tangency-labs/solidcraft/issues/15100) (design forms, the plugin UI mechanism), [#4933](https://github.com/tangency-labs/solidcraft/pull/4933) (plugin commands in Utilities), [#5586](https://github.com/tangency-labs/solidcraft/pull/5586) (install from a GitHub link) |

This RFC replaces `docs/plugins.md` and folds in what the 37 comments on #1221 settled. Only the Preferences page exists today. No plugin runs.

## Decisions

These are settled and override older text in this RFC, in #1221 and in the draft PRs.

1. **One API.** The plugin API is the MCP engine command surface. There is no second surface.
2. **WebAssembly is the plugin runtime.** No CPython, no pip, and no embedded script interpreter to start. Monty or Rhai may come later, on the same command surface.
3. **No network to start.** The linker has no fetch, sockets or HTTP, and a manifest that asks for `net` is refused. `net` stays a reserved permission name. It can be granted later only to signed official plugins from the curated index (phase 6), with named `https` origins and requests made by the host, never by the plugin.
4. **Plugin UI is host-drawn, and only where the app allows it.** Forms come from declared input schemas drawn with the design-forms mechanism in [#15100](https://github.com/tangency-labs/solidcraft/issues/15100). A plugin ships no UI code.
5. **Phase 1 sets the shape, so it gets a design review.** The phase 1 PR is reviewed on its design (WIT, manifest, permissions, limits) before phase 2 starts. See section 8.
6. **Examples and converters live outside this repository.** The repo carries the WIT, the host and the tests, not each plugin.
7. **Agent-first.** Plugins are written by agents from a spec, docs and the API. See section 6a.
8. **Shared across Craft apps.** The plugin host is an app-agnostic crate that SolidCraft, PCBCraft and other Craft apps each plug their own commands into. See section 6b.

## 1. Goals and non-goals

**Goals**
- Drop in a file and get a new command, importer, exporter or sketch tool, without rebuilding the app.
- Sandboxed by default. A plugin can do only what its manifest asks and the user allows. A crash or a hang cannot take down the app or the model.
- Any language that compiles to a WebAssembly component. Rust first.
- One command surface. Plugins change the model only through the engine commands that MCP and the control channel already use, so every edit is undoable and journaled. No private geometry API.
- A plugin built for API 1 keeps loading after app updates.

**Non-goals**
- Not a way to override core commands. Overrides would make a model behave differently depending on what is installed.
- Not where core features go. CAM may ship as a plugin (see [RFC 0015](0015-plugin-ui-workspaces.md)); plugins add vendor posts, niche strategies and importers on top.
- No plugin-to-plugin calls. If plugins ever cooperate, it goes through host events, and a failing listener never fails the caller.

## 2. The WIT contract

World `craft:plugin@0.1.0`. The interface stays thin: a few typed functions plus the JSON command set, so the WIT stays small while the commands keep growing.

| Host import | Notes |
|---|---|
| `run(command, args) -> result<string, string>` | The engine commands, JSON in and out, exactly as over MCP. Filtered by the plugin's permissions. |
| `log`, `progress` | Always available. |
| `read-file`, `write-file` | Only inside folders the user granted. |
| `fetch` | Reserved, not in the phase 1 linker. Later, only with the `net` permission, only for signed official plugins, only to origins named in the manifest, made by the host. |

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
| `net` | Reserved. A manifest that asks for it is refused until phase 6. Then: named `https` origins for signed official plugins only, requested by the host. On the web build, only through a host-approved fetch. |
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
- **Later:** a curated index. Anyone can sideload. Network access for a plugin is decided here and only here: it needs a signature from the index and named origins. To be listed, a plugin must be open source and signed. The comments favour GitHub Releases as the artifact source and keeping packages independent of one host.
- **Tools:** `solidcraft-cli plugin check <dir>` validates the manifest and schemas; `plugin test <dir>` runs every entry on the real host; `--plugin-dev <dir>` loads an unapproved folder, marked as dev, and reloads on change. The `plugin.json` schema is generated from the host's types.

## 6. Web build

wasmtime does not run in the browser tab. Options are jco-transpiled components, or desktop-only plugins at first. The same WIT world and manifest should serve both, with the same permission checks before `run` reaches the engine. Parametric features offline on the web need the module cached, so only plugins whose fallback allows it. Network access on the web goes through a host-approved fetch, never the page's own.

## 6a. Agent-first

Plugins will mostly be written by agents working from a spec, docs and the API, not typed by people. The design serves that reader first.

1. **Machine-readable API.** The WIT and the engine command schemas (the same ones MCP serves) are the API reference. The reference is generated from them and never copied by hand, so it cannot drift.
2. **A plugin skill file.** A `skills/<name>/SKILL.md` written for agents, covering the API, the manifest, permissions, packaging and how to test. It links to the generated reference instead of repeating it.
3. **A headless test harness.** `solidcraft-cli plugin test <dir>` runs a plugin against a design with no UI and prints machine-readable pass or fail per entry (JSON), with the resulting design and journal available for assertions. An agent can edit, run and iterate without a person.
4. **Typed, actionable errors.** Every host failure names its cause as data: the permission that was missing, the command that failed and why, the limit that was hit (fuel, memory, size), and the manifest field at fault. A trap or a denial never comes back as a bare string.
5. **Language is secondary.** Any language that compiles to a WebAssembly component works. The guide recommends what agents target most reliably: Rust first, TypeScript next.

Plugin inputs are schemas (section 2), so the app draws the values and variables editor through the design-forms work in #15100. A user changes numbers without touching code.

## 6b. Shared across Craft apps

[RFC 0001](0001-craft-bridge.md) hands work between Craft apps. Plugins should not need a second implementation per app either, so the host is built once and each app supplies its own commands.

1. **App-agnostic core crate `craft-plugin`.** No CAD and no SolidCraft knowledge. It owns the manifest, permissions, limits (fuel, memory, no network), install and signing, and the headless test harness. The app implements one trait: its command catalog as schemas, plus "run this command". SolidCraft implements it with the MCP engine command surface. PCBCraft and other Craft apps implement their own.
2. **Declarative UI contributions.** A plugin declares entries, placed in named slots (menu or ribbon), and inputs as schemas. The app draws them through the [#15100](https://github.com/tangency-labs/solidcraft/issues/15100) forms. A plugin ships no UI code, and each app decides which slots exist.
3. **Runtimes are sub-crates on the same trait.** `craft-plugin-wasm` is wasmtime on desktop and jco on the web, and it is the runtime. A `craft-plugin-script` crate (Monty or Rhai) is optional, is not part of phase 1, and is not added unless the owner decides to revisit decision 2.
4. **The WIT world is generic:** run a command, log, progress, cancel, granted files. App commands are data (schemas), not WIT types, so one WIT serves every Craft app. The `craft:plugin@0.1.0` world in section 2 becomes this generic world, with SolidCraft's commands as its catalog.
5. **Location.** The crates start in this repository as `crates/craft-plugin*`, and `cargo xtask layers` forbids any dependency on SolidCraft crates. They move to their own repository, as solidcore did, when a second app needs them.

Phase 1 therefore builds the host as `craft-plugin` plus `craft-plugin-wasm` against that trait, not hard-coded to SolidCraft.

## 7. What exists today

**Built**
- Preferences > Plugins (`crates/ui-egui/src/plugins.rs`, #2718): lists `{data}/plugins/<id>/plugin.json` folders, searches them, and turns each off. Turned-off ids are saved in `disabled_plugins`. Folders that are not plugins are listed with the reason.
- The manifest it reads has only `id`, `name`, `version` and `description`. It is checked and capped at 16 KiB.

**Not built**
- No wasmtime, no WIT file, no host, no example plugin, no permissions, no signing, no install flow (#4330 was closed), no `Plugin` feature kind. Nothing in the folder is loaded or run.
- #4980 (draft WIT and a gear pilot) was closed as superseded by this RFC. Its streaming importer design and threat model stay useful reading for phase 2.

## 8. Phases

| Phase | Delivers | Who and where |
|---|---|---|
| 1 | WIT `craft:plugin@0.1.0`, a wasmtime host with fuel and memory limits, `execute`, permissions in the manifest, no network in the linker, a manifest asking for `net` refused, tests: no file access without a grant, a runaway loop is stopped, a crash leaves the model untouched, undo works | Builds on [#8987](https://github.com/tangency-labs/solidcraft/pull/8987) |
| 2 | Importers and exporters, author guide, `plugin check` and `plugin test`. Measurements (component size, compile memory, binary growth) go in a #1221 comment. | After the phase 1 design review |
| 3 | `FeatureKind::Plugin`, `evaluate`, recorded fallbacks, generic timeline row | |
| 4 | Install from file or GitHub, Add-on Manager, plugin dialogs drawn from input schemas through [#15100](https://github.com/tangency-labs/solidcraft/issues/15100) | [#5586](https://github.com/tangency-labs/solidcraft/pull/5586), [#4933](https://github.com/tangency-labs/solidcraft/pull/4933) |
| 5 | Web host on the same WIT | |
| 6 | Curated, signed index; `net` for signed official plugins; panels and overlays if agreed | |

**Phase 1 design review.** The phase 1 PR sets the shape that every later phase builds on: the WIT, the manifest fields, the permission names, the limits and the error model. Before phase 2 starts, the owner reviews that design in the PR itself, and changes that come out of it land before any importer or example depends on them. A phase is done when its tests pass and, for phase 1, that review is recorded in the PR.

## 9. Open questions

1. **Wasmtime size and component limit.** Accept the binary growth, and raise the 16 MiB limit after measuring compile memory?
2. **Plugin UI.** Settled for forms: host-drawn through #15100. Viewport overlays and workspaces are answered in [RFC 0015](0015-plugin-ui-workspaces.md).
3. **Defaults.** Are newly installed plugins on or off until approved?
4. **Local network.** Not before phase 6, and under the same rule as any network: signed official plugins only. Starting a print would also need a confirmation level above `edit`.
5. **Extending core commands.** Data-driven extension points, such as thread standards in the Hole dialog, later. Never overrides.
6. **Signing and the index.** What signs, who hosts the index, and who reviews listings?
7. **Web.** jco, or desktop only?
8. **Result annotations.** Text a plugin adds to command results can steer a model, so it needs its own threat model.
9. **Recorded macros.** A plugin kind or a core feature?
10. **Version numbers.** Decided: `craft:plugin@0.1.0`.
