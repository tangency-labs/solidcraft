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
- **Declarations are versioned WIT records** (`declarations.ui-version`). If a declaration names a widget kind this build doesn't know, the host shows a "needs a newer SolidCraft" placeholder. It never refuses to load the plugin.
- **Bulk data uses buffers, not JSON.** Millions of toolpath points are too much for JSON. The `buffers` interface has a `buffer` resource the plugin fills once and hands to `viewport`, `formats` or `storage` calls. Over MCP the same field takes base64 or a granted file.

## The WIT package

The contract is one WIT package, `craft:plugin@0.1.0`, in [`0015-plugin-ui/craft-plugin.wit`](0015-plugin-ui/craft-plugin.wit). The file parses with `wasm-tools component wit`. Each interface is quoted in the section that explains it. JSON stays in two places only: the manifest `plugin.json`, and command arguments and results, which are data per RFC 0002 so one WIT serves every Craft app.

| Interface | Role |
|---|---|
| `types` | `plugin-error` variant, `value`, `datum`, colour roles, log levels |
| `buffers` | `buffer` and `buffer-builder` resources for bulk geometry and file bytes |
| `commands` | Host imports: `execute`, `log`, `progress`, `cancelled`, `now-ms` |
| `entries` | The plugin export that runs a manifest entry |
| `ui`, `ui-provider` | Panels and workspaces: records, host calls, and the plugin's declarations and event handler |
| `viewport`, `viewport-provider` | Overlay layers, timeline, picks |
| `formats`, `formats-provider` | Import and export slots |
| `storage`, `storage-provider` | Per-design data and blobs, migration |

Two worlds a plugin can target:

```wit
/// A plugin that only runs commands. Phase 1.
world command-plugin {
  import commands;
  import buffers;
  export entries;
}

/// A plugin with panels, workspaces, overlays, file formats and design data.
/// Which parts the host wires up follows the manifest's `capabilities`.
world workbench-plugin {
  include command-plugin;
  import ui;
  import viewport;
  import formats;
  import storage;
  export ui-provider;
  export viewport-provider;
  export formats-provider;
  export storage-provider;
}
```

### Manifest and capabilities

`plugin.json` is the only JSON a plugin ships. Its `capabilities` list says which optional interfaces the plugin uses. The host wires up only those, shows them on the approval screen, and refuses a component that imports an interface its manifest doesn't list.

```json
{
  "name": "manufacture",
  "version": "1.4.0",
  "world": "craft:plugin/workbench-plugin@0.1.0",
  "capabilities": ["ui@0.1", "viewport@0.1", "formats@0.1", "storage@0.1"],
  "permissions": ["clock"],
  "entries": [
    {"name": "pocket", "effect": "edit", "job": true},
    {"name": "tool_edit", "effect": "edit"},
    {"name": "ws_enter", "effect": "read", "deadline_ms": 1000}
  ]
}
```

`ui@0.1` covers `ui` and `ui-provider`, `viewport@0.1` covers `viewport` and `viewport-provider`, and so on. A plugin with no capabilities targets `command-plugin`.

## 1. Panels

A panel is a static layout the plugin returns from `ui-provider.describe` at install time, plus data it sets at run time. Widgets bind to slash paths in that data. Forms reuse the #15100 design-forms field types, so plugin forms, design forms and feature dialogs all look the same.

The companion [command-plugin selection proposal](../plugin-selection.md)
([#16274](https://github.com/tangency-labs/solidcraft/issues/16274)) describes
native viewport picking for entry input schemas. Its selection controller and
field model should be shared with these forms. The annotation and policy bindings
remain under review; the proposal does not implement `ui@0.1`.

```wit
/// Panels and workspaces. Host-drawn; a plugin ships no UI code.
interface ui {
  use types.{value, datum, color-role, plugin-error};

  enum dock-zone { left, right, bottom }

  /// Layouts are flat: nodes refer to children by index. WIT has no
  /// recursive types, and the flat form is easy to validate and limit.
  record layout {
    /// Index of the root node.
    root: u32,
    nodes: list<node>,
  }

  record node {
    widget: widget,
  }

  enum field-kind { number, length, angle, text, toggle, choice, selection }

  record field {
    key: string,
    label: string,
    kind: field-kind,
    /// For choice fields.
    choices: list<string>,
    /// Entry called when the value changes. Absent: read-only.
    on-change: option<string>,
  }

  record column {
    key: string,
    label: string,
    /// "length", "angle" and so on; shown in the user's units.
    unit: option<string>,
  }

  enum selection-mode { none, single, multi }

  record table-spec {
    /// Data path of the rows, for example "/tools".
    bind: string,
    row-id: string,
    columns: list<column>,
    select: selection-mode,
    on-select: option<string>,
    on-edit: option<string>,
  }

  record button-spec {
    label: string,
    icon: option<string>,
    entry: string,
  }

  record chart-series {
    label: string,
    role: color-role,
    /// Data path of the x and y lists.
    x: string,
    y: string,
  }

  enum chart-kind { line, bar, scatter }

  variant widget {
    /// Children are indexes into layout.nodes.
    group(list<u32>),
    tabs(list<tab>),
    split(split-spec),
    toolbar(list<button-spec>),
    button(button-spec),
    form(form-spec),
    item-list(list-spec),
    item-tree(tree-spec),
    table(table-spec),
    chart(chart-spec),
    text(string),
    progress(string),
    empty-state(string),
  }

  record tab { label: string, child: u32 }
  record split-spec { vertical: bool, first: u32, second: u32 }
  record form-spec { bind: string, fields: list<field> }
  record list-spec { bind: string, on-select: option<string> }
  record tree-spec { bind: string, on-select: option<string> }
  record chart-spec { kind: chart-kind, series: list<chart-series> }

  record panel-spec {
    id: string,
    title: string,
    zones: list<dock-zone>,
    layout: layout,
  }

  record ribbon-group { name: string, entries: list<string> }
  record ribbon-tab { name: string, groups: list<ribbon-group> }

  enum design-kind { part, assembly, sketch, drawing }

  record workspace-spec {
    id: string,
    name: string,
    /// Name from the app icon registry.
    icon: string,
    applies-to: list<design-kind>,
    ribbon: list<ribbon-tab>,
    /// Panel ids per zone.
    panels: list<tuple<dock-zone, list<string>>>,
    /// Host panels kept, for example "browser".
    builtin-panels: list<string>,
    /// Read-only entries with a 1 s deadline.
    on-enter: option<string>,
    on-exit: option<string>,
  }

  record declarations {
    /// Bump when widget kinds are added; unknown kinds draw a placeholder.
    ui-version: u32,
    panels: list<panel-spec>,
    workspaces: list<workspace-spec>,
  }

  enum event-kind { press, select, edit, change, enter, exit }

  record ui-event {
    panel: string,
    event: event-kind,
    path: string,
    row: option<string>,
    value: option<value>,
  }

  /// Host calls, available to a plugin that declares the "ui" capability.
  /// Replace some or all of a panel's data. Merged once per frame.
  set-data: func(panel: string, updates: list<datum>) -> result<_, plugin-error>;
  clear-data: func(panel: string) -> result<_, plugin-error>;
  /// Show or hide a panel in the current workspace.
  show-panel: func(panel: string, visible: bool) -> result<_, plugin-error>;
}

/// What a UI plugin exports. Static declarations are read at install time,
/// so the approval screen can list them without running any entry.
interface ui-provider {
  use types.{plugin-error};
  use ui.{declarations, ui-event};

  describe: func() -> declarations;
  /// The host turns user actions into events. They run as ordinary commands.
  on-event: func(event: ui-event) -> result<_, plugin-error>;
  /// Called when a plugin workspace is entered or left, if declared.
  on-enter: func(workspace: string) -> result<_, plugin-error>;
  on-exit: func(workspace: string) -> result<_, plugin-error>;
}
```

| | |
|---|---|
| Widgets | Form fields (from #15100), button, toolbar, list, tree, table, chart (line, bar, scatter), text, progress bar, empty state. Layout uses group, tabs and split. |
| Data | `ui.set-data(panel, list<datum>)` sets values at slash paths such as `/tools/3/dia`. The host merges updates once per frame. Units are shown in the user's units, and stored values are always mm or deg. |
| Events | `entry`, `on-select`, `on-edit` and `on-change` each name a manifest entry. The host delivers a `ui-event` to `ui-provider.on-event`, and entries run as `<plugin>.<entry>`. Ordinary commands: they're journaled and undoable, and the `read` or `edit` effect applies. |
| Docking | Through the shared panel host (#15184, `panel_runtime`), as a registered kind. Its allowed zones come from the manifest. It can't use `PinOuter` or Fill height, and placement memory is saved per workspace like built-in panels. |
| Permissions | None beyond the plugin being enabled. Declaring a panel is listed on the approval screen. |
| Limits | 8 panels per plugin. 512 widgets per layout. 1 MiB of data per panel. Tables and trees scroll virtually, up to 100,000 rows. Charts: 8 series, 20,000 points. |

## 2. Workspaces

Today the app has two workspaces, `Design` and `Render` (`ui.workspace`, and the `WorkspaceId` in the panel layout). A plugin workspace adds a third kind, `plugin:<id>/<workspace>`. It uses the same switcher, the same layout memory and the same `ui.workspace` command.

A workspace is a `workspace-spec` in the `declarations` returned by `ui-provider.describe`, shown above: ribbon tabs and groups name entries, `panels` places panel ids per zone, and `on-enter` and `on-exit` name read-only entries.

| | |
|---|---|
| Host draws | The switcher item, the ribbon tabs with buttons sized by the host, and the starting panel layout (the user can rearrange it, and it's remembered). The viewport and the built-in panels named in `builtin_panels` stay host-owned. |
| Hooks | `on-enter` and `on-exit` are `read` entries with a short deadline (1 s). They can only update panels and overlays, never the design, so switching workspaces never edits or blocks anything. If a hook fails, the error shows in the workspace and the switch still happens. |
| Design tabs | `applies_to` works like Render. The workspace shows only for design kinds it lists, and the rest gray it out with a reason. |
| Icons | From the app's icon registry for now (see open question 3). |
| Limits | 2 workspaces per plugin. 6 ribbon tabs and 60 ribbon entries per workspace. Ribbon accents come from the theme's plugin accent. |

## 3. Viewport content

A plugin fills named overlay layers with data, and the host draws them in the 3D view with depth, picking and the theme. Playback runs on the host's own timeline scrubber. No plugin code runs per frame.

```wit
/// Overlay layers in the 3D view. Data only; nothing runs per frame.
interface viewport {
  use types.{color-role, plugin-error};
  use buffers.{buffer};

  type point3 = tuple<f32, f32, f32>;

  enum line-style { solid, dashed, dotted }
  enum frame { world-mm, design-local }

  /// How an item is coloured: a role, or a scalar per vertex mapped through
  /// the theme ramp.
  variant paint {
    role(color-role),
    ramp(buffer),
  }

  record polyline {
    id: string,
    /// vec3f buffer.
    points: buffer,
    /// Optional scalar32 buffer: time of each vertex, for the scrubber.
    times: option<buffer>,
    paint: paint,
    style: line-style,
  }

  record point-set {
    id: string,
    points: list<point3>,
    role: color-role,
  }

  record label {
    id: string,
    at: point3,
    text: string,
  }

  /// A mesh that can move with the scrubber.
  record mesh {
    id: string,
    /// vec3f and index32 buffers.
    positions: buffer,
    triangles: buffer,
    ghosted: bool,
    track: option<track>,
  }

  record track {
    t-start: f32,
    t-end: f32,
    /// vec3f buffer: one translation per time step.
    translate: buffer,
  }

  record face-tint {
    id: string,
    faces: list<string>,
    role: color-role,
  }

  variant item {
    polyline(polyline),
    points(point-set),
    label(label),
    mesh(mesh),
    face-tint(face-tint),
  }

  record layer {
    name: string,
    frame: frame,
    pickable: bool,
    items: list<item>,
  }

  record marker { t: f32, label: string }
  record timeline { duration: f32, unit: string, markers: list<marker> }

  /// What the host sends back when a pickable layer is clicked.
  record pick {
    layer: string,
    item: string,
    /// Segment or vertex index.
    index: u32,
    point: point3,
    t: option<f32>,
  }

  /// Replace a layer. Over the limit: limit-exceeded, nothing drawn.
  set-layer: func(layer: layer) -> result<_, plugin-error>;
  remove-layer: func(name: string) -> result<_, plugin-error>;
  set-timeline: func(timeline: timeline) -> result<_, plugin-error>;
}

interface viewport-provider {
  use types.{plugin-error};
  use viewport.{pick};

  on-pick: func(hit: pick) -> result<_, plugin-error>;
}
```

| | |
|---|---|
| Host draws | Polylines, points, triangle meshes (shaded or ghosted), screen-facing labels and face tints on real model faces. Each layer gets a row in the Browser with an eye toggle. Items with `t` or `track` follow the scrubber: play, pause, speed and step. The scrubber shows while a layer has time data. |
| Picking | When a pickable layer is clicked, the host calls `viewport-provider.on-pick` with a typed `pick` record. The `index` is the segment or vertex. Hover shows the plugin's label with no plugin call. Model picking is unchanged. Overlays never take selection from real geometry unless the user clicks the overlay itself. |
| Lifetime | Overlays are view state, not design state. They're cleared when the design closes or the plugin is turned off. A plugin that wants them back on reopen stores its source data (section 5) and rebuilds them in `on-enter`. |
| Limits | 4 million vertices and 16 layers per plugin. 256 MiB of buffers per plugin. 2,000 labels. When a limit is hit, the plugin gets a typed `limit-exceeded` error and nothing is drawn halfway. |

## 4. File formats

```wit
/// Import and export slots, shown in the app's Import and Export menus.
interface formats {
  use types.{plugin-error};
  use ui.{field};

  enum slot-kind { import-slot, export-slot }

  record format-slot {
    id: string,
    kind: slot-kind,
    extensions: list<string>,
    label: string,
    /// Option fields drawn as a form before an export.
    options: list<field>,
    /// Export only: also reads the design's plugin data.
    needs-plugin-data: bool,
  }

  record loss-item { what: string, detail: string }

  /// An import becomes a new unsaved design, with a loss report.
  record import-result {
    commands: list<string>,
    losses: list<loss-item>,
  }
}

interface formats-provider {
  use types.{datum, plugin-error};
  use buffers.{buffer};
  use formats.{format-slot, import-result};

  slots: func() -> list<format-slot>;
  /// Bytes of the one file the user picked, plus its name.
  import-file: func(slot: string, name: string, data: buffer) -> result<import-result, plugin-error>;
  /// Returns bytes; the host writes them to the path the user picked.
  export-file: func(slot: string, options: list<datum>) -> result<buffer, plugin-error>;
}
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

The plugin reaches it through the `storage` interface:

```wit
/// Per-design plugin data, stored in the .sc container under the plugin's
/// own namespace. A plugin cannot reach another plugin's data.
interface storage {
  use types.{json, plugin-error};
  use buffers.{buffer};

  /// The data's own format number, so the plugin can migrate.
  format-version: func() -> u32;

  /// The main document. `set` and `patch` are edits: one undo step.
  get: func() -> result<option<json>, plugin-error>;
  set: func(document: json) -> result<_, plugin-error>;
  /// RFC 7396 merge patch.
  patch: func(merge-patch: json) -> result<_, plugin-error>;

  record blob-options {
    /// Cache blobs may be dropped by "save without caches" and rebuilt.
    cache: bool,
  }

  put-blob: func(name: string, data: buffer, options: blob-options) -> result<_, plugin-error>;
  get-blob: func(name: string) -> result<option<buffer>, plugin-error>;
  delete-blob: func(name: string) -> result<_, plugin-error>;
  list-blobs: func() -> result<list<string>, plugin-error>;
}

interface storage-provider {
  use types.{plugin-error};

  /// First call after an update, told the previous format. One undoable edit.
  migrate: func(from-format: u32) -> result<_, plugin-error>;
}
```

- The command `storage.get`, `set` and `patch` only reaches the calling plugin's own namespace. `set` is an edit: one undo step, and it's in the journal. Agents can read it over MCP like anything else in the design.
- References to geometry use the engine's stable face and edge refs. A ref that breaks after an edit is reported in the plugin's panel, never silently retargeted.
- **Plugin missing, off or older:** the design opens normally. A quiet banner says "Contains Manufacture data (plugin 1.4). Install it to edit." The data is kept byte for byte and saved back unchanged. If the data's `format` is newer than the installed plugin, that plugin's part opens read-only with a warning. The rest of the design is unaffected. Nothing is ever downloaded or run on open.
- After an update, the plugin's first call is told the previous `format` so it can migrate. The migration is saved as one undoable edit.
- Limits: 16 MiB document and 256 MiB blobs per plugin per design. A plugin can mark blobs `cache`, so "save without caches" can drop them.

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
| Enter | The user picks Manufacture in the workspace switcher. `ws_enter` reads `storage.get`, fills the Setups tree and the Tool library table, and redraws the saved toolpaths as overlays. |
| Setup | Setup > New opens a form (stock size, work origin, which body). The user picks the origin corner in the viewport. That's a normal selection field from #15100. OK calls `setup_new`, which writes plugin data: one undo step. |
| Tools | The Tool library panel is a table plus a form. Editing a diameter calls `tool_edit`, which writes plugin data. A library shared across designs needs a granted folder (`files`) and is optional. |
| Toolpath | Pocket opens an operation form (faces, tool, stepover, depth). OK starts the `pocket` job. It shows in the operations panel, reads the faces through `model.faces` and `model.mesh`, and streams the path into layer `toolpath:op3`. When it finishes, it writes the operation and a cached path blob. |
| Inspect | Clicking a segment of the path calls `on-pick` with a `pick` record for layer `toolpath:op3`, item `cut`, index 1042, t 41.2. The plugin selects that operation in Setups and shows the feed rate at that point. |
| Simulate | The `simulate` job computes tool motion and a few stock meshes, then sends a `mesh` with a `track` plus a timeline. The user scrubs, plays and steps with the host scrubber. A chart panel shows load against time. |
| Post | Export > G-code: Haas mill shows the post options form and the host's file picker. `post_haas` reads the plugin data and returns text. The host writes the file. |
| Share | A colleague without the plugin opens the .sc file. The part is fine, the banner names the plugin, and the toolpath data survives their save. |
| Agent | An agent can do all of this over MCP: `ui.workspace`, `manufacture.pocket {...}`, `storage.get`, `ui.operations`. It can test it headless with `plugin test`, and overlay layers can be checked as data. |

## Phasing

| When | What |
|---|---|
| Phase 2 (with importers and exporters) | File format slots (section 4). Per-design plugin data with the missing-plugin fallback (section 5). Long jobs in the operations panel (section 6). Panels with forms, buttons, lists and tables, docked in the shared host (section 1). The `craft:plugin` rename and `buffers`. Web: the same declarations drawn by the web build. |
| Phase 3 | Workspaces (section 2). Viewport overlays with picking, timeline and scrubber (section 3). Trees and charts. A reference Manufacturing plugin outside the repo, used as the end-to-end test. |
| Later | Plugin icons (if agreed), Monty or Rhai on the same surface, and a per-plugin network permission. Parametric plugin features stay in their own phase, as in RFC 0002. |

Phase 1 ([#15112](https://github.com/tangency-labs/solidcraft/pull/15112), led by @raykholo) targets `command-plugin` and is unchanged apart from the package name.

## Risks

- **Security.** Panels that pass as host UI: handled by badges, plugin places only, and no dialogs or links. Plugin text steering agents: labels and panel text reach agents marked as plugin text, the "result annotations" question from RFC 0002. Plugin data in a shared file is data only and never triggers an install.
- **Performance.** Millions of overlay vertices and fast panel updates: handled by buffers, frame merging, hard limits and virtual tables. The host's frame time must not depend on any plugin.
- **Web build.** Every widget and overlay type has to ship in the web build at the same time. Job workers need web workers. On the web, plugin data and file picks go through the browser's file handling, and jco has to carry buffers without copying them twice.
- **ABI stability.** The WIT stays small and is split into interfaces so a plugin imports only what it uses. UI declarations are versioned WIT records that only grow by addition, and new WIT versions keep the `craft:plugin` package name. Unknown kinds get placeholders, never refusal. A plugin built for `ui: 1` must keep working.

## Decided

1. CAM may ship as a plugin, and our own Manufacturing plugin is the phase 3 proof.
2. The WIT package is `craft:plugin@0.1.0`.
3. Monty or Rhai may come later, on the same command surface.

## Open questions

1. **Switcher placement.** Should plugin workspaces sit beside Design and Render in the switcher, or under a "More" list until the user pins them? Open.
2. **Icons.** Registry icons only, or also a plugin-supplied single-colour SVG that the host tints and checks? Open.
3. **Long jobs.** Is a 10 minute default with a Preferences ceiling right for toolpath and simulation jobs? Open.
4. **Version after phase 3.** `craft:plugin@0.1.0` is the starting point. When does it move to 1.0.0? Open.
