# Layout-aware container audit

Shared container foundation (#4411) and compact composition (#2029), on top of
History #3953 and Assistant #3985. Interface preferences save Floating or Compact;
`view.layout` changes the same preference without changing the design.

## Ownership

`ui_kit::Container` is the only production constructor for outer egui hosts in the
UI crate and the desktop/web apps. `Surface` declares purpose, not a second layout
setting. The shell publishes `LayoutMode` before drawing any page. Adaptive panels
and shell layout use `Container::floating()`; frames use the same policy. There is
one definition of floating/compact chrome and workspace gaps in `ui_kit/container.rs`.
Compact joins the workspace and ribbon, puts Browser flush beneath it with a square
join and content-hugging height, and opens feature/palette sheets at their edge without
card gaps or shadows. Tool sheets remain draggable in either layout; compact sheets
round all four corners when detached and square only the right corners at their right dock. Contacts declare which edges meet; the container rounds only
exposed convex corners and collapses adjoining outlines into one stroke. Stacked
headers cover the viewport without reserving a blank column beneath them.
Floating keeps its card chrome; both layouts retain tool drag offsets.
Menus, system windows, notifications and canvas content retain their existing roles.

The shell still owns ordering, viewport reservations and shared-column geometry.
A container does not infer geometry from a title or panel ID. Browser and History
share a column width; Assistant remains outermost on either side. Tool sheets may overlap Browser and History,
but cannot enter Assistant’s reserved rail or cover the ribbon/workspace selector. Those relationships
must survive the compact composition. Existing `Panel`, `Window`, `Confirm` and
`Menu` are content compositions over the universal container, not alternative hosts.
The low-level window builder is visible only inside the UI kit: feature and platform
windows use the shared composition, including its header and fixed outer frame.

## Inventory and migration

| Surfaces | Owners / composition | Container policy |
| --- | --- | --- |
| Browser, vertical History and Browser/History shared column | `browser`, `timeline_left`, `lib` | Adaptive panel; shared shell placement, resizing and fold state remain with owners |
| Horizontal History | `timeline` | Adaptive panel surface; floating strip or bottom dock |
| Assistant sheet and folded strip | `agent_chat/panel` → `Panel` | Adaptive feature surface; left/right position, resize, drag targets and native pop-out retained |
| Feature forms, Parameters, material review, Redefine Sketch Plane | `dialogs`, `params_dialog`, `material_card_review`, `sketch_attachment_ui` → `Panel` | Adaptive feature surface; bounded anchors and pinned actions retained |
| Sketch options | `sketch_palette` → `Panel` | Adaptive palette surface, preserving transparency |
| Application bar, workspace and ribbon | `toolbar`, `shell` | Chrome hosts; adaptive card geometry comes from the shared policy |
| Render settings | `render_workspace` | Panel host; existing right dock retained |
| Preferences, managers, About, update/install/recovery/crash/issue prompts | `Window`, `Confirm` | System windows and modal sheets retain modality; these are not workspace sidebars |
| BOM, Messages, mouse gestures, 3D mouse calibration | respective modules → `Window::show_body` | Shared header, tokenised frame and bounded body; native egui title bars/resize highlights and legacy report/width helpers removed |
| Web stored-files and web recovery windows | `apps/solidcraft-web` → `Window::show_body` | Same shared composition; recovery requires an explicit choice and cannot be dismissed through the header |
| File/workspace/document/Help/context/overflow menus, radial menu, Select Other and inline rename | respective owners → `Menu` / container popup | Anchored transient hosts; never consume workspace dock space |
| Search, completions, colour and library popovers | `shortcut_box`, kit fields/choice/colour/table | Anchored popup containers; editor focus and completion semantics retained |
| Dimension values, canvas placement values, canvas feedback and History resize seam | `dim_view`, `sketch_dims`, `canvas`, `timeline_left` | Model/control-anchored overlays; retain their coordinates rather than becoming docks |
| Status and loading/progress messages | `viewport`, `open_meshing` | Notification surfaces independent of layout |
| Navigation bar | `viewport` | Shared navigation surface; shell still computes its available canvas bounds |
| OK and Cancel in the view's corner | `view_confirm` | Navigation surface in its own area; the viewport layout reserves its corner and the shell counts it as covered |
| Home, model viewport, Render canvas and native Assistant window content | `home`, `lib`, `agent_chat/panel` | Central content hosts, not floating workspace panels |
| Loading input shield | `file_load` | Explicit input shield; must remain interactive while CAD input is blocked |

The compass, geometry previews, drag handles, selection outlines, dimension lines,
charts and thumbnails are drawn content, not independent card hosts. Giving them
workspace docks would be incorrect. egui-managed tooltips and widget internals
remain with their owning shared widget. OS file/program/colour pickers and native
viewport creation remain host services, not in-app panels.

## Enforcement for additions

`cargo xtask ui-containers` is the mandatory repository gate, implemented once in
`xtask/src/ui_containers.rs`. The fast CI job runs it and its regression tests before
approval for heavy UI tests; `cargo xtask ci` runs it too. Draft PRs retain the repository’s
normal CI policy. It recursively parses Rust source and examples under every crate and
app, so new packages and nested modules are included automatically. The sole constructor
implementation exception is the exact `ui_kit/container.rs` path.

The gate rejects raw Area, Panel, Window, Modal, CentralPanel, Popup and SubMenu
constructors, imported/module/type aliases and re-exports, constructor function references,
qualified default constructors, struct literals, macro-hidden hosts and popup shortcuts.
Host APIs are default-denied except for explicit popup-state operations, so new egui
constructors are rejected automatically. Private raw host/module imports are rejected too,
preventing aliases from escaping into another source file.
It also rejects raw outer popup/window frames and custom outer shadows. Invalid Rust
fails closed. Read-only state queries, content-only frames, and test-only fixtures remain
valid; there are no per-feature allowances. This enforces the construction boundary;
surface purpose and hand-painted content still need ordinary review.

New workspace panels must declare an adaptive `Surface` and use this policy for
host choice and outer chrome. Do not introduce another layout bool, raw host,
shadow or mode-specific frame in the feature module. Body groups, message bubbles,
field backgrounds and image frames are content; they may use local zero-shadow
frames or existing kit controls. Add new surface policies in the universal module
and test both modes and themes. Never add a per-file allowance to bypass the guard.

## Retired implementation cleanup

The old report-window builder, window-width enum, heading-band colours and
caller-coloured icon API have been removed. Theme palettes and built-in theme
files no longer carry procedural command-icon colours; SVG artwork has one
palette authority. Only marker/pointer colours remain in the native glyph palette.
Older theme exports discard retired colour names at import so active overrides
still work; these names do not create renderer tokens or appear in new exports.
Unused History height state and superseded click/preview/modifier helpers are
also removed. The remaining `frame` helpers implement dragging and bounded
anchors used by current `Panel` compositions, rather than a second host or style.
Buttons and editors use shared kit controls too, including Assistant code-block
actions. The control guard scans all UI feature modules, even without a kit import,
so adding a new module cannot silently bypass the shared controls.
