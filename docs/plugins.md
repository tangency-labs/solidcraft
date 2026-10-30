# Plugins (design)

The initial native one-shot command runtime is described in [plugin-runtime.md](plugin-runtime.md).
The parametric feature shell and later phases below remain design work.

Design companion to [#1221](https://github.com/bherbruck/solidcraft/issues/1221): sandboxed
WebAssembly plugins with a WIT interface, the same `run(command, args)` JSON as
[MCP](mcp.md) and the [control protocol](control-protocol.md). Browser hosting is in
[web-runtime.md](web-runtime.md).

This document adds **parametric timeline features**: a generic host-owned feature shell that
plugins fill with parameters, display metadata, and rebuild logic, without a second timeline
model or a private geometry API.

## Goals (unchanged from #1221)

- Sandboxed by default: manifest permissions, fuel/memory limits on desktop, broker enforcement on web.
- Any language that compiles to Wasm components; Rust first.
- **One command surface:** plugins change the model only through engine commands (undoable, journaled).
- Versioned WIT world (`solidcraft:plugin@1.0.0`).

## Two plugin shapes

| Shape | Timeline | Rebuild | Milestone |
|-------|----------|---------|-----------|
| **One-shot** | Optional; often no persistent feature | N/A | 1 (first ship) |
| **Parametric** | One **`Plugin` feature row** per plugin instance | Host calls plugin on param/timeline rebuild | 2 |

One-shot plugins call `execute(params)` once; they may emit ordinary native features (sketch,
extrude, …) inside that transaction. Parametric plugins still use `run()` inside rebuild; the
difference is that the document **records** a single feature whose params the user edits later.

## Generic feature shell (host-owned)

Add one variant to the parametric document (name illustrative):

```text
FeatureKind::Plugin {
  plugin_id: String,           // "gear_generator"
  api_version: u32,            // matches WIT world
  params: Value,               // JSON owned by the plugin schema
  display: PluginDisplay,      // timeline + browser presentation
  provenance: PluginProvenance, // optional: manifest hash, build id
}
```

**`PluginDisplay`** (stored in the file, not recomputed from plugin code at open):

| Field | Role |
|-------|------|
| `label` | Primary timeline / browser text |
| `icon_id` | Host icon registry name (same system as [`feature_icon`](https://github.com/bherbruck/solidcraft/blob/main/crates/ui-egui/src/workspace.rs)) |
| `tooltip` | Hover text, `inspect` type string |
| `type_name` | short feature type name for power users |

The timeline renders **any** `Plugin` feature through one code path: read `display`, draw icon +
label, wire suppress/rename/delete/reorder like built-in features. No per-plugin egui in milestone 2.

Optional later: **`parameter_schema()`** in WIT returns `{ id, label, kind, min, max, … }` so the
host renders sliders and dimension rows without custom UI per plugin.

## WIT entry points (phase 2 additions)

Milestone 1 (#1221): `manifest()`, `execute(params) -> result<string, string>`.

Milestone 2 parametric:

| Export | Purpose |
|--------|---------|
| `evaluate(params, ctx) -> result<string, string>` | Rebuild: plugin runs `run()` commands; result is success JSON or error |
| `parameter_schema() -> string` | Optional JSON schema for generic parameter UI |
| `fallback_policy() -> string` | Declares missing-plugin behaviour (see below) |

`ctx` is host-provided read-only hints (configuration name, units, feature id); plugins must not
 rely on it for correctness.

Inside **`evaluate`**, the plugin uses the same **`run(command, args)`** import as one-shot mode.
The host applies commands to the current eval context (or a scratch session merged by the host).
No second registry of geometry ops.

## Rebuild flow

```text
Timeline rebuild reaches FeatureKind::Plugin
  -> load plugin (wasmtime desktop / broker web) if permitted + present
  -> evaluate(stored params)
       -> plugin calls run("sketch.line", …), run("solid.extrude", …), …
  -> host merges geometry like any native feature
  -> errors surface on the feature row like native eval errors
```

Param changes (parameter panel, `parameters.change`, config switch) re-run **`evaluate`** for
affected plugin features only when dependencies say so (same invalidation rules as native features).

## Missing plugin and API drift

Manifest declares **`fallback_policy`**:

| Policy | Behaviour |
|--------|-----------|
| `recorded_mesh` | Optional B-rep or mesh blob stored beside params; open works, edit disabled without plugin |
| `recorded_script` | Stored list of native commands; rebuild without plugin if every command is built-in |
| `suppress_with_warning` | Feature suppressed; user sees clear message |
| `error_on_rebuild` | Hard fail on eval (strict teams) |

Design files shared in CollabCraft should prefer a policy that **opens safely** (`recorded_mesh` or
`recorded_script`). CI for plugins (#1221) should include: uninstall plugin, reopen file, assert
document still opens and policy matches manifest.

## Icons and metadata

Plugins declare allowed **`icon_id`** values in `manifest()`; the host rejects unknown ids. Icons
stay in the app registry so untrusted plugins cannot inject arbitrary raster assets into the tree
without a reviewed icon pack.

**Rename** by the user updates the feature user label; **`display.label`** remains the default from
creation unless the plugin updates it via a host callback (optional future).

Command naming: prefer **`plugin_id.function`** in `run()` or registered command aliases to avoid
collisions ([#1221 discussion](https://github.com/bherbruck/solidcraft/issues/1221)).

## Group header variant (lighter-weight v2a)

Before full **`evaluate`**, a stepping stone: one-shot plugin creates a **feature folder** whose
header carries `PluginDisplay` metadata while children are ordinary native features. Rebuild does
not need the plugin until the user edits plugin-owned params on the header. Migrates cleanly to
full `FeatureKind::Plugin` when `evaluate` lands.

## Relationship to craft-libs

Plugin **document model and eval** stay in SolidCraft (`crates/doc`, `crates/engine`). craft-libs
provides worker/broker hosting only ([web-runtime.md](web-runtime.md)). Neutral serde types
(`VersionedParams`, `PluginDisplay`) may move to a shared crate later if multiple apps need them;
CAD plugins remain SolidCraft-specific.

## Phasing

1. WIT + wasmtime host + one-shot `execute` + permissions tests (#1221 milestone 1).
2. `FeatureKind::Plugin` + `PluginDisplay` + timeline generic row + `evaluate` + fallback policy.
3. Web broker runs same WIT; parametric features offline only if plugin module cached and policy
   allows.
4. Parameter schema UI, cloud-signed plugin index, inspector extensions.

## Tests (parametric)

- Create plugin feature, change param, rebuild, measure geometry (oracle or bbox).
- Undo/redo across plugin eval.
- Remove plugin binary, reopen design, assert fallback policy.
- Icon id from manifest appears on timeline (scenario or screenshot harness).
