# RFC 0015: Plugin panels, workspaces and viewport content

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Extends | [RFC 0002](0002-plugins.md) (plugins) |
| Related | [#15100](https://github.com/tangency-labs/solidcraft/issues/15100) (design forms), [#15184](https://github.com/tangency-labs/solidcraft/pull/15184) (shared panel host), [#15112](https://github.com/tangency-labs/solidcraft/pull/15112) (plugin runtime, phase 1), [#4980](https://github.com/tangency-labs/solidcraft/pull/4980) (streaming) |

Plugins should be able to add declarative panels, whole workspaces, and enough viewport and file support to hold a full custom manufacturing workflow. This RFC says how, without breaking the rules RFC 0002 already set.

**One idea runs through every section: data in, commands out.** A plugin sends the host data (a panel layout, rows, polylines). The host draws it with its own widgets and theme. When the user acts on it, the host calls one of the plugin's entries as an ordinary command. So everything a person can do in a plugin panel, an agent can do over MCP. Nothing the plugin sends is code, pixels or egui.

## What this changes in RFC 0002

1. The non-goal "CAM stays in-tree (#1153)" becomes "a full manufacturing workflow, CAM included, can ship as a plugin". Decided.
2. Decision 2 relaxes to "WebAssembly first; Monty or Rhai may come later on the same command surface". Decided.
3. The WIT package is `craft:plugin@0.1.0`, replacing `solidcraft:plugin@1.0.0` from #15112. Decided.
4. Network stays off. When it comes, it is a per-plugin permission the user grants, made by the host.
5. Open question 2 of RFC 0002 (overlays and workspaces) gets the answer below.

## Shared rules for all plugin UI

- **The host draws everything.** Theme tokens only. Plugins pick colour roles (`accent`, `ok`, `warn`, `series-1..8`, a scalar ramp), never RGB. Every widget must exist in the web build before it can be declared.
- **The widget set is what gets reviewed.** Each widget kind, slot and overlay type the host adds goes to the owner or Nate with 1280x720 pictures, once. Third-party plugins can only combine reviewed parts.
- **Plugin UI stays in plugin places.** It lives in its own panels, its own workspace, its own ribbon group or a menu slot. It never adds rows to built-in dialogs or toolbars. Each panel header carries the plugin's name and a plugin badge, so it can't pass as host UI. It can't open approval or permission dialogs, and text in it is never a clickable link.
- **Declarations are versioned JSON** (`"ui": 1`). If a declaration names a widget kind this build doesn't know, the host shows a "needs a newer SolidCraft" placeholder. It never refuses to load the plugin.
- **Bulk data uses buffers, not JSON.** Millions of toolpath points are too much for JSON. A small WIT addition, `buffers.put(bytes) -> id`, lets a command's JSON point to `{"buffer": 3, "type": "f32x3"}`. Over MCP the same field takes base64 or a granted file.

## 1. Panels

A panel is a static layout in the manifest plus a data document the plugin sets at run time. Widgets bind to paths in the data with JSON pointers. Forms reuse the #15100 design-forms field types, so plugin forms, design forms and feature dialogs all look the same.

```json
"panels": [{
  "id": "tools", "title": "Tool library", "zones": ["left", "right"], "ui": 1,
  "layout": [
    {"type": "toolbar", "items": [{"type": "button", "label": "Add tool", "icon": "add", "entry": "tool_add"}]},
    {"type": "table", "bind": "/tools", "row_id": "id", "select": "single",
     "columns": [{"key": "num", "label": "T"}, {"key": "name", "label": "Name"},
                 {"key": "dia", "label": "Dia", "unit": "length"}],
     "on_select": "tool_pick", "on_edit": "tool_edit"},
    {"type": "form", "bind": "/selected", "fields": "#/schemas/tool", "on_change": "tool_edit"}
  ]}]
```

| | |
|---|---|
| Widgets | Form fields (from #15100), button, toolbar, list, tree, table, chart (line, bar, scatter), text, progress bar, empty state. Layout uses group, tabs and split. |
| Data | The command `plugin.panel.update {panel, data \| patch}` takes the whole document or a JSON Patch. The host merges updates once per frame. Units are shown in the user's units, and stored values are always mm or deg. |
| Events | `entry`, `on_select`, `on_edit` and `on_change` each name a manifest entry. The host calls it as `<plugin>.<entry>` with `{panel, event, path, row, value}`. Ordinary commands: they're journaled and undoable, and the `read` or `edit` effect applies. |
| Docking | Through the shared panel host (#15184, `panel_runtime`), as a registered kind. Its allowed zones come from the manifest. It can't use `PinOuter` or Fill height, and placement memory is saved per workspace like built-in panels. |
| Permissions | None beyond the plugin being enabled. Declaring a panel is listed on the approval screen. |
| Limits | 8 panels per plugin. 512 widgets per layout. 1 MiB of data per panel. Tables and trees scroll virtually, up to 100,000 rows. Charts: 8 series, 20,000 points. |

## 2. Workspaces

Today the app has two workspaces, `Design` and `Render` (`ui.workspace`, and the `WorkspaceId` in the panel layout). A plugin workspace adds a third kind, `plugin:<id>/<workspace>`. It uses the same switcher, the same layout memory and the same `ui.workspace` command.

```json
"workspaces": [{
  "id": "manufacture", "name": "Manufacture", "icon": "mill",
  "applies_to": ["part", "assembly"],
  "ribbon": [
    {"tab": "SETUP",     "groups": [{"name": "Setup", "entries": ["setup_new", "stock"]}]},
    {"tab": "TOOLPATHS", "groups": [{"name": "2D", "entries": ["face", "pocket", "contour"]},
                                    {"name": "Run", "entries": ["simulate", "post"]}]}],
  "panels": {"left": ["setups"], "right": ["tools", "op_params"]},
  "builtin_panels": ["browser"],
  "on_enter": "ws_enter", "on_exit": "ws_exit"
}]
```

| | |
|---|---|
| Host draws | The switcher item, the ribbon tabs with buttons sized by the host, and the starting panel layout (the user can rearrange it, and it's remembered). The viewport and the built-in panels named in `builtin_panels` stay host-owned. |
| Hooks | `on_enter` and `on_exit` are `read` entries with a short deadline (1 s). They can only update panels and overlays, never the design, so switching workspaces never edits or blocks anything. If a hook fails, the error shows in the workspace and the switch still happens. |
| Design tabs | `applies_to` works like Render. The workspace shows only for design kinds it lists, and the rest gray it out with a reason. |
| Icons | From the app's icon registry for now (see open question 3). |
| Limits | 2 workspaces per plugin. 6 ribbon tabs and 60 ribbon entries per workspace. Ribbon accents come from the theme's plugin accent. |

## 3. Viewport content

A plugin fills named overlay layers with data, and the host draws them in the 3D view with depth, picking and the theme. Playback runs on the host's own timeline scrubber. No plugin code runs per frame.

```
plugin.overlay.set {
  "layer": "toolpath:op3", "frame": "world-mm", "pickable": true,
  "items": [
    {"type": "polyline", "id": "cut",   "points": {"buffer": 1, "type": "f32x3"},
     "t": {"buffer": 2, "type": "f32"}, "color": {"ramp": "scalar", "values": {"buffer": 4, "type": "f32"}}},
    {"type": "polyline", "id": "rapid", "points": {"buffer": 3, "type": "f32x3"}, "color": "warn", "style": "dashed"},
    {"type": "points",   "id": "plunges", "points": [[10, 4, 2]], "color": "accent"},
    {"type": "label",    "id": "l1", "at": [10, 4, 12], "text": "OP3 pocket"},
    {"type": "mesh",     "id": "tool", "positions": {"buffer": 5}, "triangles": {"buffer": 6},
     "track": {"t": [0, 90], "translate": {"buffer": 7, "type": "f32x3"}}},
    {"type": "face_tint", "faces": ["face:@b2/f17"], "color": "series-2"}
  ]}
plugin.overlay.timeline { "duration": 90.0, "unit": "s", "markers": [{"t": 41.2, "label": "Tool change"}] }
```

| | |
|---|---|
| Host draws | Polylines, points, triangle meshes (shaded or ghosted), screen-facing labels and face tints on real model faces. Each layer gets a row in the Browser with an eye toggle. Items with `t` or `track` follow the scrubber: play, pause, speed and step. The scrubber shows while a layer has time data. |
| Picking | When a pickable layer is clicked, the host calls the plugin's `on_pick` entry with a typed hit: `{layer, item, index, point, t}`. The `index` is the segment or vertex. Hover shows the plugin's label with no plugin call. Model picking is unchanged. Overlays never take selection from real geometry unless the user clicks the overlay itself. |
| Lifetime | Overlays are view state, not design state. They're cleared when the design closes or the plugin is turned off. A plugin that wants them back on reopen stores its source data (section 5) and rebuilds them in `on_enter`. |
| Limits | 4 million vertices and 16 layers per plugin. 256 MiB of buffers per plugin. 2,000 labels. When a limit is hit, the plugin gets a typed `limit-exceeded` error and nothing is drawn halfway. |

## 4. File formats

```json
"formats": [
  {"kind": "import", "extensions": ["nc", "tap"], "label": "G-code (backplot)", "entry": "import_gcode"},
  {"kind": "export", "extensions": ["nc"], "label": "G-code: Haas mill", "entry": "post_haas",
   "options": "#/schemas/post_options", "needs": ["plugin_data"]}]
```

- Formats show up in Import and Export as file types, labelled with the plugin's name. The host opens the file picker. An import entry gets the bytes of the one file the user picked, plus its name. An export entry returns bytes, and the host writes them to the path the user picked. A plugin can't open other files unless a folder grant allows it (assemblies).
- A post-processor is just an export entry with an options form. It can read the design through commands and its own plugin data (section 5). Output goes only to a local file. There are no network or machine targets.
- An import becomes a new unsaved design with a loss report, as RFC 0002 already says. A backplot import may also set overlays.
- Limits: 1 GiB input, read in chunks (the #4980 streaming design), and 256 MiB output.

## 5. Per-design plugin data

```
.sc container
  plugins/manufacture/meta.json   {"plugin": "manufacture", "plugin_version": "1.4.0", "format": 3}
  plugins/manufacture/data.json   setups, operations, tool numbers, refs to faces and edges
  plugins/manufacture/blobs/...   optional binary (cached toolpaths), may be dropped and rebuilt
```

- The command `plugin.data.get / set / patch` only reaches the calling plugin's own namespace. `set` is an edit: one undo step, and it's in the journal. Agents can read it over MCP like anything else in the design.
- References to geometry use the engine's stable face and edge refs. A ref that breaks after an edit is reported in the plugin's panel, never silently retargeted.
- **Plugin missing, off or older:** the design opens normally. A quiet banner says "Contains Manufacture data (plugin 1.4). Install it to edit." The data is kept byte for byte and saved back unchanged. If the data's `format` is newer than the installed plugin, that plugin's part opens read-only with a warning. The rest of the design is unaffected. Nothing is ever downloaded or run on open.
- After an update, the plugin's first call is told the previous `format` so it can migrate. The migration is saved as one undoable edit.
- Limits: 16 MiB JSON and 256 MiB blobs per plugin per design. A plugin can mark blobs `cache`, so "save without caches" can drop them.

## 6. Long jobs

- An entry can declare `"job": true`. It then runs on a worker and gets a row in the existing operations panel (bottom right, red X to cancel) with the plugin's name and the entry's label. `ui.operations` lists it like any other operation.
- It uses the `progress(fraction, message)` and `cancelled()` calls #15112 already has. Cancel, or a changed design, means the result isn't adopted. That's the same rule the runtime uses today.
- A job can push overlay and panel updates while it runs (a toolpath growing on screen), merged to at most 10 per second.
- Jobs get a separate budget: a 10 minute deadline by default, and a Preferences ceiling like the current fuel and memory fields. Ordinary entries keep the 5 to 30 s limits.

## 7. Worked example: a Manufacturing plugin

An independent plugin, `manufacture`, built outside the repo. It asks only for `clock`: no files, no network.

| Step | What happens |
|---|---|
| Install | The approval screen lists 1 workspace, 4 panels, 2 export formats and 1 import format, the commands it reads (`model.faces`, `model.edges`, `model.mesh`, `inspect.measure`), and that it stores data in designs. |
| Enter | The user picks Manufacture in the workspace switcher. `ws_enter` reads `plugin.data.get`, fills the Setups tree and the Tool library table, and redraws the saved toolpaths as overlays. |
| Setup | Setup > New opens a form (stock size, work origin, which body). The user picks the origin corner in the viewport. That's a normal selection field from #15100. OK calls `setup_new`, which writes plugin data: one undo step. |
| Tools | The Tool library panel is a table plus a form. Editing a diameter calls `tool_edit`, which writes plugin data. A library shared across designs needs a granted folder (`files`) and is optional. |
| Toolpath | Pocket opens an operation form (faces, tool, stepover, depth). OK starts the `pocket` job. It shows in the operations panel, reads the faces through `model.faces` and `model.mesh`, and streams the path into layer `toolpath:op3`. When it finishes, it writes the operation and a cached path blob. |
| Inspect | Clicking a segment of the path calls `on_pick` with `{layer: "toolpath:op3", item: "cut", index: 1042, t: 41.2}`. The plugin selects that operation in Setups and shows the feed rate at that point. |
| Simulate | The `simulate` job computes tool motion and a few stock meshes, then sends a `mesh` with a `track` plus a timeline. The user scrubs, plays and steps with the host scrubber. A chart panel shows load against time. |
| Post | Export > G-code: Haas mill shows the post options form and the host's file picker. `post_haas` reads the plugin data and returns text. The host writes the file. |
| Share | A colleague without the plugin opens the .sc file. The part is fine, the banner names the plugin, and the toolpath data survives their save. |
| Agent | An agent can do all of this over MCP: `ui.workspace`, `manufacture.pocket {...}`, `plugin.data.get`, `ui.operations`. It can test it headless with `plugin test`, and overlay layers can be checked as data. |

## Phasing

| When | What |
|---|---|
| Phase 2 (with importers and exporters) | File format slots (section 4). Per-design plugin data with the missing-plugin fallback (section 5). Long jobs in the operations panel (section 6). Panels with forms, buttons, lists and tables, docked in the shared host (section 1). The `craft:plugin` rename and `buffers`. Web: the same declarations drawn by the web build. |
| Phase 3 | Workspaces (section 2). Viewport overlays with picking, timeline and scrubber (section 3). Trees and charts. A reference Manufacturing plugin outside the repo, used as the end-to-end test. |
| Later | Plugin icons (if agreed), Monty or Rhai on the same surface, and a per-plugin network permission. Parametric plugin features stay in their own phase, as in RFC 0002. |

Phase 1 ([#15112](https://github.com/tangency-labs/solidcraft/pull/15112), led by @raykholo) is unchanged apart from the package name.

## Risks

- **Security.** Panels that pass as host UI: handled by badges, plugin places only, and no dialogs or links. Plugin text steering agents: labels and panel text reach agents marked as plugin text, the "result annotations" question from RFC 0002. Plugin data in a shared file is data only and never triggers an install.
- **Performance.** Millions of overlay vertices and fast panel updates: handled by buffers, frame merging, hard limits and virtual tables. The host's frame time must not depend on any plugin.
- **Web build.** Every widget and overlay type has to ship in the web build at the same time. Job workers need web workers. On the web, plugin data and file picks go through the browser's file handling, and jco has to carry buffers without copying them twice.
- **ABI stability.** The WIT stays small (run, buffers, log, control, files, clock). UI is versioned JSON that only grows by addition. Unknown kinds get placeholders, never refusal. A plugin built for `ui: 1` must keep working.

## Decided

1. CAM may ship as a plugin, and our own Manufacturing plugin is the phase 3 proof.
2. The WIT package is `craft:plugin@0.1.0`.
3. Monty or Rhai may come later, on the same command surface.

## Open questions

1. **Switcher placement.** Should plugin workspaces sit beside Design and Render in the switcher, or under a "More" list until the user pins them? Open.
2. **Icons.** Registry icons only, or also a plugin-supplied single-colour SVG that the host tints and checks? Open.
3. **Long jobs.** Is a 10 minute default with a Preferences ceiling right for toolpath and simulation jobs? Open.
4. **Version after phase 3.** `craft:plugin@0.1.0` is the starting point. When does it move to 1.0.0? Open.
