# Sketch

Board: `Prefs2_Sketch` (1920x1570). Heading: Work. Subtitle "Drawing, snapping and selecting."
Tag: In profile. Source today: "Sketch" page (`prefs/sketch.rs`) plus the selection controls.

## Snapping and guides

A 120x64 figure at the left (`sketch-guides`), the checkboxes at the right.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Centre and midpoint guides | Checkbox | `Prefs.sketch_tracking_guides` | `true` | "Temporary horizontal and vertical guides from hovered centres and edge midpoints." | `sketch-guides` | |
| Guides from sketch points too | Checkbox | `Prefs.sketch_tracking_points` | `true` | none | none | Disabled while the first is off (`sketch_tracking_guides`). |
| Snap to where two guides cross | Checkbox | `Prefs.sketch_recent_snap` | `false` | none | none | Catalog label today: "Auto snap to recent centers and midpoints". |
| Line up with other sketch points | Checkbox | `Prefs.sketch_align_points` | `false` | "Drawing lines align with points on both axes at once." | none | |
| Hold to stop snapping | Dropdown (200 wide, "Alt / Option") | `Prefs.suppress_sketch_snap` (`ModifierCombo`) | Ctrl + Alt (`ModifierCombo::CTRL_ALT`) | none | none | The board sample shows "Alt / Option" and the key caps "Ctrl / Cmd"; the default in the code is Ctrl+Alt. |

## Drawing

Two columns of checkboxes.

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Press and drag draws one line | Checkbox | `Prefs.line_drag_draws_one` | `false` | none | none | |
| Rest on a line's last point to switch to a tangent arc | Checkbox | `Prefs.line_hover_tangent_arc` | `false` | Existing `prefs.line_hover_tangent_arc_note`: leaving, returning to and resting on the last point toggles line / tangent arc. | none | Retained newer control; immediately after press-and-drag, before centre-point arc. |
| Centre-point arc follows the cursor | Checkbox | `Prefs.center_arc_follows_cursor` | `true` | none | none | |
| Extended construction line tools | Checkbox | `Prefs.sketch_construction_lines` | `true` | none | none | |
| Dimension new shapes fully | Checkbox | `Prefs.sketch_auto_dimensions` | `false` | "Typed values plus every dimension that fixes the shape." | none | |
| Place from the sketch origin | Checkbox (dimmed on the board while the one above is off) | `Prefs.sketch_auto_locate` | `false` | "With full dimensions." | none | Dimmed unless "Dimension new shapes fully" is on. |
| Shortcut again switches mode | Checkbox | `Prefs.shortcut_cycles_modes` | `false` | "Off: it starts the tool again." | none | |
| Clicking the running tool's button ends it | Checkbox | `Prefs.ribbon_click_ends_tool` | `true` | Existing `prefs.ribbon_click_ends_tool_note`: the lit ribbon button ends the tool and restores selection; off restarts it. | none | Retained newer control; immediately after shortcut cycling, before the Project pair. |
| Project face edges into new sketches | Checkbox | `app.session.auto_project` (session; in the Snapshot) | per session | none | none | Not a `Prefs` field. |
| Ask for Project options first | Checkbox | `Prefs.project_options_dialog` | `true` | none | none | |

## Opening and closing

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Sketching on a part's face makes that part active | Checkbox | `Prefs.sketch_face_activates_part` | `false` | Existing `prefs.sketch_face_activates_part_note`: activate the other component before creating the sketch; it stays active. Off keeps the active component. | none | Retained newer control; first row, immediately before turn-to-face. |
| Turn to face a sketch when it opens | Checkbox | `Prefs.sketch_open_look_at` | `true` | none | none | |
| When a sketch ends: Return to the view before / Keep (default) / Unless turned (return if not rotated) | Three picture cards (196 x 152.5), a before / sketch / after trio each | `Prefs.sketch_exit_view` (`SketchExitView`: Previous, Keep, UnlessRotated) | Keep | none | `sketch-exit-previous`, `sketch-exit-keep`, `sketch-exit-unless-turned` | Card labels: "Return to the view before" / "Keep" with sub-label "default" / "Unless turned" with sub-label "return if not rotated". The 30x30 panels are nested SVGs inside the figure. |

## Marks in the sketch

Three toggle tiles (208.8 x 140).

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| State icons | Toggle tile | `Prefs.sketch_state_icons` | `true` | none | `sketch-state-icons` | |
| Conflict colour | Toggle tile | `Prefs.sketch_conflict_colour` | `true` | none | `sketch-conflict` | |
| Hide glyphs zoomed out | Toggle tile | `Prefs.hide_constraint_glyphs_zoomed_out` | `true` | none | `sketch-hide-glyphs` | |

## Selecting

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Click selects: Faces and edges / Whole bodies | Two picture cards (150 x 109) | `app.ui.pick_bodies` (session; in the Snapshot) | per session | none | `select-faces-edges`, `select-bodies` | Not a `Prefs` field. |
| Click, Shift Click, Ctrl / Cmd Click | A key-cap table: "Click" Select one item; "Shift Click" Add the item, or take it out again; "Ctrl / Cmd Click" Add the item, or take it out again | none (information) | n/a | none | none | The Shift and Ctrl/Cmd rows read the same on the board; the next checkbox changes the Shift row. The key caps are sample content in the sense that they describe the current behaviour, not settings. |
| Shift adds only; Ctrl / Cmd toggles | Checkbox | `Prefs.selection_shift_add_only` | `false` | none | none | |
| Prefer movable points where they overlap | Checkbox | `Prefs.prefer_free_sketch_points` | `true` | none | none | |
| Select Other reaches hidden geometry | Checkbox | `Prefs.select_other_hidden` | `true` | none | none | |
| Alt / Option also frees manipulator arrows | Checkbox | `Prefs.manipulator_free_alt` | `true` | none | none | |

## Mismatches

* The three retained newer controls above are absent from the old board. Their placements were
  accepted in #9881 from source-only recommendations; existing labels, help, defaults, profile
  membership and behavior remain authoritative. Drawing flows down the first column through
  origin placement, then down the second from shortcut cycling; dependent pairs stay together.

* "Hold to stop snapping" default differs between the board sample and the code (see above).
* "Project face edges into new sketches" and "Click selects" are session values, not `Prefs`.
* `Prefs.sketch_variants` and the hole preset library have no control here (not on any board).
