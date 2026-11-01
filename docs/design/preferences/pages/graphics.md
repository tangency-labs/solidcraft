# Graphics

Board: `Prefs2_Graphics` (1920x1821). Heading: Look. Subtitle "Trade speed for smoothness." No
"In profile" tag: the Graphics page stays on this computer (see README). Source today: Graphics page
(`prefs/graphics.rs`).

## Preset

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Performance "Fastest", Quality "Sharpest", Custom "Your own mix" | Three preset cards (204 x 69.5) | command `view.graphics_preset` with `{"preset":"performance"}` or `"quality"`; the selection is derived from the settings by `graphics_preset(&prefs)`, Custom when neither matches | derived | Today's tooltip `ui.graphics_preset_help`: "Quality: ambient occlusion, Fine display quality and adaptive 2x supersampling. Performance: no ambient occlusion, Normal display quality and supersampling off. Manual changes show Custom. MSAA stays unchanged." The board shows no hover; keep the existing text on the card group. | preset icons (not figures; the app's icons) | The board shows no meters. Custom cannot be chosen to set values: choosing it changes nothing. Only these three exist ("Balanced" in an earlier board does not). |

## Curved surfaces

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Coarse "fastest", Normal "default", Fine "smoother", Ultra "slowest" | Four picture cards (130 wide, 117 high) | `Prefs.display_quality` (string ids; `"normal"` is the default and `"ultra"` is accepted, see `prefs/graphics.rs` for the full list) | `"normal"` | none | `tess-coarse`, `tess-normal`, `tess-fine`, `tess-ultra` | The figure shows a polygon on a circle with 8, 16, 32 and 64 segments (the corner caption of each figure). The caption number in `tess-normal` ("16") is part of the figure. |

## Edges and shading

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Anti-aliasing + badge "Restart needed" | Four picture cards: Off, 2x, 4x, 8x ("not on this GPU") | `Prefs.msaa` (1, 2, 4, 8) | `4` | tooltip on the cards: `settings.msaa_applies_on_restart` (the restart note) | `aa-off`, `aa-2x`, `aa-4x`, `aa-8x` | A level the adapter lacks (`app.msaa_supported`) is disabled with the badge "not on this GPU". Changing it applies on restart. |
| Supersampling | Segmented: Off / 1.5x / 2x | `Prefs.supersampling` (1.0, 1.5, 2.0) | `1.0` (Off) | `settings.supersampling_note` | none | |
| Only while the view is still | Checkbox | `Prefs.supersampling_adaptive` | `true` | none | none | Disabled while Supersampling is Off (`supersampling > 1.0`). |

## Current view (with the badge "Also in the view bar")

Four toggle tiles (155.6 wide). These are the view toggles that also sit in the view bar.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Grid | Toggle tile | `app.ui.show_grid` (session; in the Snapshot) | per session | none | `view-grid` | |
| Ground shadow | Toggle tile | `app.ui.ground_shadow` | per session | none | `view-shadow` | |
| Perspective | Toggle tile | `app.ui.perspective` | per session | none | `view-perspective` | |
| Ambient occlusion | Toggle tile (moves here from Edges and shading) | `Prefs.ambient_occlusion` | `false` | none | `ao-on` | |
| Visual style | Dropdown (240 x 30) with the style icon: "Shaded with edges" | `app.ui.visual_style` (0..3: shaded with edges, shaded, wireframe, hidden edges) | 0 | none | `style-shaded-edges` (icon shape; use the app's `style_shaded_edges`, `style_shaded`, `style_wireframe`, `style_hidden_edges`) | aria-label "Visual style". |
| Field of view | Slider with "34 degrees" readout (44 px column) | `Prefs.perspective_fov` in radians (accepted 15 to 90 degrees) | `0.6` rad (34.4 degrees) | none | none | aria-label "Field of view". |

## In the view

Four toggle tiles, then a chip row.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| View angles | Toggle tile | `Prefs.show_view_angles` | `false` | none | `view-angles` | |
| Vertex markers | Toggle tile | `Prefs.vertex_markers` | `false` | none | `vertex-markers` | |
| Quick measurement | Toggle tile | `Prefs.quick_measure` (today on the Interface page) | `false` | none | `quick-measure` | Moves here. |
| Frame rate | Toggle tile | `Prefs.show_frame_rate` | `false` | none | `frame-rate` | |
| Status chips: Analysis, Section, Live details, Fold items | Four small chips (the row is labelled "Status chips") | `Prefs.viewport_analysis_status`, `Prefs.viewport_section_status`, `Prefs.viewport_status_details`, `Prefs.viewport_status_summary` | all `true` | none | none | Mapping by catalog label: "Live details" is `viewport_status_details` ("Live viewport details") and "Fold items" is `viewport_status_summary` ("Fold viewport status items"). These four move here from Interface. `Prefs.frame_rate_details` (frame rate details) has no control on the board. |

## Surfaces and edges

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Cosmetic thread markings | Checkbox | `Prefs.cosmetic_threads` | `false` | none | none | |
| Halo under sketch curves | Checkbox | `Prefs.sketch_halo` | `false` | none | none | |
| Reference geometry through bodies | Checkbox | `Prefs.reference_geometry_through_bodies` | `false` | none | none | |
| Grid follows the view | Checkbox | `Prefs.grid_follows_view` (no page id in the table today) | `false` | "Side views show the grid on the facing plane." | none | |
| Single surface analysis | Checkbox | `Prefs.single_surface_analysis` | `true` | "Shading and Continuity share one analysis." | none | |

## This computer

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Apple M2 / Metal . GPU renderer | Read-only device card (the first line is the adapter name, the second the backend) | none (from the renderer) | n/a | none | none | Sample values. |
| Graphics card + badge "Restart needed" | Dropdown (240 wide, "Automatic") | `Prefs.graphics_adapter` (empty = automatic) | empty | none | none | aria-label "Graphics card". |
| Window system | Dropdown (240 wide, "Automatic") with "Linux" badge | `Prefs.window_system` (`"auto"`) | `"auto"` | none | none | The "Linux" badge says the choice is for Linux only; hide the row elsewhere. aria-label "Window system". |
| Use previous Render preview geometry budget | Checkbox | `Prefs.render_previous_geometry_budget` | `false` | "Restores the earlier fixed budget." | none | |

## Mismatches

* "Axis distances in Measure" (`Prefs.measure_axis_distances`) leaves this page for Tools.
* The board has no control for `frame_rate_details`.
* Renderer and window system rows come from the old page's "Renderer" block.
* The tracker's "Edges and shading" note says Ambient occlusion moves to Current view; the code still
  lists it under Graphics with `PAGE_GRAPHICS`.
