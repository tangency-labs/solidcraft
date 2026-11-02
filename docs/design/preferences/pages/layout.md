# Layout

Board: `Prefs2_Layout` (1920x1750). Heading: Look. Subtitle "Where the parts of the window sit."
Tag: In profile. Reference for the navigation-bar editor's interaction states: `Navbar_Settings`.
Source today: Interface page (`prefs/interface.rs`, `prefs/navbar.rs`).

## Window

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Workspace | Segmented: Floating / Compact | `Prefs.layout` (`shell::LayoutMode`: `Floating`, and `Docked` serialised as `compact`) | Floating | none | `layout-schematic` | Today's labels are Floating and Docked; the board says Compact (the serialised name is `compact`). |
| Toolbar | Segmented: Labels / Icons only | `Prefs.ribbon_labels` | `true` (Labels) | none | none | |
| Menus | Segmented: List / Large icons | `Prefs.ribbon_icon_grid` | `false` (List) | none | none | |
| Notices | Segmented: Bottom right / Bottom centre | `Prefs.toast_place` (`ToastPlace`; Corner = Bottom right) | Corner (Bottom right) | none | none | |
| Status bar | Checkbox | `Prefs.status_bar` | `false` | "What is being edited, the sketch state and units." | none | |
| Layout preview | Schematic (262 wide) of the window, showing the choices above | none | n/a | none | `layout-schematic` | Reflects the Workspace choice and the History, Browser and Dialog choices below. |

## Panels

### History

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| History: Bottom / Left / Under browser / In browser | Four picture cards (116 wide) | `Prefs.timeline_left`, `Prefs.timeline_under_browser`, `Prefs.history_in_browser` (three booleans; Bottom is all off) | Bottom | none | `history-bottom`, `history-left`, `history-under-browser`, `history-in-browser` | One choice maps to the three booleans; keep them mutually consistent as today's page does. |

### Browser

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Browser: Column / Card | Two picture cards | `Prefs.browser_float` (Card = floating) | Column (`false`) | none | `browser-column`, `browser-card` | |
| Shown | Segmented: Shown / On hover | `Prefs.browser_show` (`browser::BrowserShow`) | Shown | none | none | |
| Sketches in History | Segmented: In order / Under feature | `Prefs.history_sketches` (`HistorySketches`) | In order | none | none | |
| Bodies folders | Segmented: Always / Several / Never | `Prefs.browser_bodies_folders` (`BodiesFolders`) | Several | none | none | |
| Play History button | Checkbox | `Prefs.history_play_button` | `true` | none | none | |
| Origin and bodies on top | Checkbox | `Prefs.timeline_origin_bodies` | `false` | "Vertical history only." | none | |
| Eye beside the fold arrow | Checkbox | `Prefs.browser_eye_left` | `false` | none | none | |

## Dialogs

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Open | Segmented: At the view's edge / In the left column | `Prefs.dialog_place` (`dialog_column::DialogPlace`: Edge, LeftColumn) | Edge | none | none | |
| OK and Cancel | Segmented: Bottom / Top | `Prefs.dialog_buttons` (`ui_kit::DialogButtons`) | Bottom | none | none | |
| Also in the view's corner | Checkbox | `Prefs.view_confirm_corner` | `false` | "While a dialog or sketch is open." | none | |

## Toolbar tabs

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Model, Surface, Mesh, Form, Sheet Metal, Render | Six tab chips, each with a show/hide toggle | `Prefs.hidden_toolbar_tabs` (list of hidden tab ids) | none hidden | none | none | |
| Keep the Sketch tab shown | Checkbox | `Prefs.sketch_tab_always` | `false` | none | none | |
| Highlight active commands | Checkbox | `Prefs.persistent_ribbon_pressed` | `true` | none | none | |
| Show search results in the ribbon | Checkbox | `Prefs.ribbon_reveal` | `false` | none | none | Verify the field: `ribbon_reveal` is the only ribbon search field in the table. |
| Pinned commands out of menus | Checkbox | `Prefs.ribbon_menus_skip_pinned` | `false` | none | none | |

## Navigation bar

Source: `Prefs.navigation_bar` (`navigation_bar::NavigationBar`) and `app.ui.show_navigation_bar`.
The interaction states (drag onto the bar, drop targets, reorder) stay as in `Navbar_Settings`.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Preview as | Tabs: Design / Render | which workspace's bar is edited | Design | none | none | The Render tab edits the Render workspace's slots. |
| Preview strip | The bar drawn as it will look, with the slots in order | `navigation_bar.slots` (plus `slot_options`) | orbit, pan, zoom, separator, fit, look_at, home, separator, visual_style, display | none | none | Slot icons on the board are stand-ins; use the app's icons. |
| Show navigation bar | Checkbox | `app.ui.show_navigation_bar` (session; in the Snapshot) | per session | none | none | |
| Reset to default | Button | resets `navigation_bar` | n/a | none | none | |
| 10 of 16 slots | Count text | none | n/a | none | none | Sample numbers; the count is slots on the bar of all available slots. |
| Available | Row of slot chips, e.g. Faces and edges, Spin, Projection, Ground shadow, Ambient occlusion | none (drag source) | n/a | "Drag onto the bar" on each chip and on the label | none | Slot names and icons are sample content. |
| Position | Segmented: Left / Centre / Right | `navigation_bar.position` (`Position`) | Centre | none | none | |
| Edge | Segmented: Bottom / Top | `navigation_bar.edge` (`Edge`) | Bottom | none | none | |
| Size | Segmented: Compact / Regular | `navigation_bar.size` (`Size`) | Regular | none | none | |
| Buttons | Segmented: Icons / Icons + labels | `navigation_bar.labels` (`Labels`) | Icons (`Labels::Icons`) | none | none | |
| When idle | Segmented: Always / Fade / Hover | `navigation_bar.idle` (`Idle`: Always, Fade, Hide) | Fade | none | none | The third option reads "Hover" on the board and `Hide` in the code. |

## Mismatches

* "Compact" on the board is `LayoutMode::Docked` (serialised `compact`) in the code.
* The History choice is three booleans in the code, one card group on the board.
* The status chips ("Analysis", "Section", "Live details", "Fold items") are not on this page; they
  are on Graphics under "In the view".
