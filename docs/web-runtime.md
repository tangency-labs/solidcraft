# Browser runtime (design)

Design note for SolidCraft in the browser: tiered storage, worker offload, GPU jobs, and
agent control that stay aligned with the desktop app, the control channel, and
[MCP](mcp.md). Implementation lives primarily in
[craft-libs](https://github.com/storytold/craft-libs) (`craft-tier-store`, `craft-worker-host`,
`craft-gpu`, `craft-runtime`); SolidCraft wires those crates into
[`apps/solidcraft-web`](../apps/solidcraft-web) and [`files.rs`](../apps/solidcraft-web/src/files.rs)
rather than growing a second stack here.

Related: [plugins.md](plugins.md) and [#1221](https://github.com/bherbruck/solidcraft/issues/1221)
(desktop WIT + wasmtime first; parametric timeline features in phase 2), [design containers](design-container.md),
[control protocol](control-protocol.md).

## Problem

The wasm app today ([`getting-started`](../docs/book/src/guide/getting-started.md)) keeps designs in
the origin private file system (OPFS) behind virtual `/opfs/` paths, with optional File System
Access import/export. That works for small and medium designs but hits three walls:

| Wall | Symptom in SolidCraft |
|------|------------------------|
| Quota | Large STEP imports and container blobs compete with autosave and cache in OPFS |
| Main thread | Open, save, and timeline evaluation run synchronously on the UI thread with egui |
| Agents | MCP runs out of process (`solidcraft-cli mcp`); the tab has no first-class agent session |

WebGPU for the viewport already runs on the main thread (wgpu via eframe). Moving the entire UI
renderer into a worker is out of scope for the first integration; worker GPU is for offload jobs
(tessellation warm paths, future kernel work), not for egui.

## Principles

1. **One command surface.** Toolbar, scripts, control channel, MCP, and (eventually) in-browser
   agents all use the same engine commands. Dialog-free commands only for unattended paths, same
   rule as [MCP](mcp.md).
2. **Generic traits in craft-libs.** Storage tiers, worker RPC, and GPU dispatch are implemented
   once in craft-libs. Web and native use the same traits; hosts differ (OPFS vs paths, SharedWorker
   vs thread pool).
3. **Do not duplicate cloud IO.** Authenticated cloud Open/Save uses `craft-cloud` and CollabCraft
   ([craft-libs #48](https://github.com/storytold/craft-libs/pull/48)). Browser-local tiers
   (`craft-tier-store`) are separate: hot mirror, warm cache, cold working folder, optional range
   reads of cloud blobs through CollabCraft without vendor types in generic crates.
4. **One worker broker.** A SharedWorker (or dedicated-worker fallback) coordinates storage writes,
   job queues, and shared wasm module compile per origin. Plugins and offload jobs register on the
   broker; do not spawn one SharedWorker per plugin ([#1221 comment thread](https://github.com/bherbruck/solidcraft/issues/1221)).

## Architecture

```text
Main thread (tab)
  eframe + viewport WebGPU
  hot mirror (sync reads for engine)
  craft_runtime HostFacade  ----MessagePort RPC---->  SharedWorker broker
                                                          tier-store coordinator (single writer)
                                                          job queue + dedup
                                                          headless engine session(s)
                                                          optional dedicated GPU workers
                                                          ProviderRegistry (GpuJob dispatch)

Tiers (craft-tier-store)
  Hot   RAM mirror, LRU budget
  Warm  OPFS under craft/<appId>/…  (today: /opfs/ mapping)
  Cold  File System Access project folder (no duplicate bytes in OPFS)
  Cloud craft-cloud refs, range fetch via CollabCraft API (not in wasm credentials)
```

On **desktop**, the same `BlobStore` and job envelopes apply: warm/cold map to normal directories;
[`solidcraft-cli mcp`](mcp.md) and wasmtime plugin hosts ([#1221](https://github.com/bherbruck/solidcraft/issues/1221))
call the same `run(command, args)` JSON.

## Mapping SolidCraft today

| Today | Target |
|-------|--------|
| [`files.rs`](../apps/solidcraft-web/src/files.rs) OPFS + `/opfs/` VFS | Warm tier keys; migration keeps existing filenames initially |
| Recovery autosave | Warm tier, same semantics as [`compatibility.md`](compatibility.md) |
| [Design containers](design-container.md) content-addressed blobs | Tier keys by hash; cold tier holds large blobs without copying into OPFS |
| [Control protocol](control-protocol.md) over TCP | Same methods over `MessagePort` to broker headless session (WebMCP-shaped, not a second registry) |
| Parallel mesh work ([roadmap detail](roadmap-detail.md): web path serial) | Offload eval / IO jobs via `JobSubmit`; viewport stays on main thread |

## craft-libs crates (reference)

These names follow the craft-libs performance platform design; SolidCraft depends on them as
published from storytold/craft-libs after cloud contracts land (#48, #52).

| Crate | Role |
|-------|------|
| `craft-tier-store` | `BlobStore`, tier policy, sync mirror over async persist; native + wasm hosts |
| `craft-worker-host` | SharedWorker bootstrap, RPC messages (`Init`, `StorageOp`, `JobSubmit`, …) |
| `craft-gpu` | `GpuContext`, `GpuJob`, `GpuProvider`, `ProviderRegistry` (no app WGSL here) |
| `craft-gpu-wgpu` | wgpu backend: WebGPU on wasm32, Vulkan/DX12/Metal on desktop |
| `craft-gpu-providers` | Shared readback/composite helpers extracted from Craft app gpu crates |
| `craft-runtime` | Facade: tier store + worker host + optional `cloud-delegate` hooks |

Generic runtime code must not depend on SolidCraft crates. SolidCraft registers app-specific
providers and key namespaces only.

## GPU and the viewport

- **Main thread:** eframe owns the WebGPU device used for the 3D view and UI.
- **Workers:** `craft_runtime::gpu::dispatch(registry, jobs)` runs `GpuJob` lists (chunked buffers,
  readback fences) for offload; same entry point on native thread pools.
- **Limits:** Capability probe once at init; UI and workers share reported max buffer sizes.

## Agents in the browser

| Mode | Transport |
|------|-----------|
| Desktop agent | `solidcraft-cli mcp` (stdio) or `--connect` to [control protocol](control-protocol.md) |
| Browser agent | RPC to broker session; dialog-free tools only |
| IDE bridge | Optional local relay (stdio MCP ↔ tab); not required for CollabCraft-hosted wasm |

File policy for web agents mirrors MCP folders: design directory, cold tier roots, recovery;
never `--allow-any-path` in the tab.

## Plugins ([#1221](https://github.com/bherbruck/solidcraft/issues/1221))

| Host | Runtime |
|------|---------|
| Desktop | wasmtime + fuel/memory limits; WIT `solidcraft:plugin@1.0.0` with `run`, `manifest`, permissions |
| Web | Component or module in broker; same WIT contract; sandbox = manifest permissions enforced before `run()` |

Milestone 1: one-shot commands and importers/exporters. Milestone 2: **parametric timeline
features** via a generic host-owned `FeatureKind::Plugin` shell (params + display metadata
including timeline icon and label); plugins rebuild through WIT `evaluate()` calling the same
`run()` JSON. See [plugins.md](plugins.md). Command ids may use a `pluginname.function` prefix to
avoid collisions.

## CollabCraft deployment

Static wasm apps on `craft.quasarke.net` ship `craft-worker-host.js` (brotli-precompressed) beside
the app bundle. Prefer one global runtime worker per origin once message routing and wasm size are
budgeted; per-app worker URLs are acceptable for pilots. CSP must allow `worker-src` / SharedWorker.

## Phasing (SolidCraft)

1. **Document and track** (this file); no user-visible change.
2. **Tier-store wiring:** replace ad-hoc OPFS glue with `craft-tier-store` keys; keep on-disk layout.
3. **Worker broker:** headless `engine.execute` batches and container IO off UI thread.
4. **Agent bridge:** MessagePort tool surface equivalent to MCP dialog-free tools.
5. **Plugins on web:** after desktop wasmtime milestone and WIT stability.
6. **Parametric plugin features:** [plugins.md](plugins.md) phase 2 (`evaluate`, display metadata, fallback policy).

## Out of scope (initial integration)

- Moving egui/viewport WebGPU into SharedWorker
- Second non-wgpu GPU backend in craft-libs
- **Implementing** parametric `FeatureKind::Plugin` (designed in [plugins.md](plugins.md); follows craft-runtime wiring)
- Duplicating `craft-cloud` or embedding `craft-nextcloud` in SolidCraft wasm

## Tests

- Extend control-channel scenarios for broker RPC parity where methods map 1:1
- Worker tests: SharedWorker multi-tab dedup (EffectCraft `workers.mjs` pattern)
- Tier-store: native LRU tests + wasm OPFS smoke
- GPU: golden frames remain in app crates; craft-gpu-null for CI dispatch smoke
