# Navigation

Board: `Prefs2_Navigation` (1920x1584). Heading: Input. Subtitle "Mouse and trackpad. The 3D mouse has
its own page." Tag: In profile. Source today: Navigation page (`prefs/navigation.rs`).

## Mouse

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Mouse mapping | Dropdown (220 wide, "Fusion style") | `Prefs.nav` (preset id) with `Prefs.mouse_gestures` | `"fusion"` | none | `mouse-mapping` | The mouse figure (96x120) sits at the left of the list. Presets set `right_drag_orbits` and friends (`prefs.rs` preset block). |
| Middle drag: Pan; Shift + Middle drag: Orbit; Right drag: Orbit; Wheel: Zoom; Left click: Select; Right click: Menu | Key-cap table (gesture, then action) | the mapping's gestures (`Prefs.mouse_gestures`) | the mapping | none | `mouse-mapping` | Rows follow the chosen mapping; the board shows the Fusion style as the example. |
| Right drag orbits | Checkbox | `Prefs.right_drag_orbits` | `true` | "Shown where the mapping orbits on a right drag." | none | |
| Radial context menu | Checkbox | `Prefs.radial_context_menu` | `false` | "Hold or flick the right button for a ring of tools." | none | |
| Middle double-click | Dropdown (220 wide, "Home view") | `Prefs.middle_double_click` | `"home"` | none | none | |

## Orbit around

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Origin / View centre / Under the cursor | Three picture cards (196 x 133.5) | `Prefs.orbit_around` (`OrbitAround`: Origin, ViewCentre, Cursor); `Prefs.orbit_cursor` is the older boolean for the cursor choice | ViewCentre | none | `orbit-origin`, `orbit-centre`, `orbit-cursor` | |

## Orbit style

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Orbit: Turntable / Free | Segmented | `Prefs.free_orbit` (Free = `true`) | Turntable (`false`) | "Free turns through both poles, with roll and no Shift axis snap." | none | |
| Turn about the world's up axis | Checkbox | `Prefs.free_orbit_upright` | `false` | "With Free." | none | Dimmed unless Free is chosen. |

## Wheel and trackpad

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Zoom to the cursor | Checkbox | `Prefs.zoom_to_cursor` | `true` | none (today: "The wheel zooms toward the point under the cursor (off: toward the view's center)") | none | |
| Smooth wheel zoom | Checkbox | `Prefs.smooth_wheel_zoom` | `true` | none | none | |
| Reverse zoom direction | Checkbox | `Prefs.zoom_reverse` | `false` | none | none | |
| Two-finger scroll pans | Checkbox | `Prefs.trackpad_pan` | `true` | "Off: it zooms like the wheel. Pinch always zooms." | none | |
| Reverse horizontal pan | Checkbox | `Prefs.pan_reverse_x` | `false` | none | none | |
| Reverse vertical pan | Checkbox | `Prefs.pan_reverse_y` | `false` | none | none | |
| Reverse trackpad orbit | Checkbox | `Prefs.orbit_reverse` | `false` | none | none | |
| Stop zoom at the model | Checkbox | `Prefs.zoom_stop_at_model` | `false` | none | none | |
| Wheel zoom speed | Slider, "1.00x" readout (44 px) | `Prefs.wheel_zoom_speed` (0.25 to 4.0) | `1.0` | none | none | aria-label "Wheel zoom speed". |
| Two-finger zoom while holding | Dropdown (180 wide, "Ctrl / Cmd") | `Prefs.trackpad_zoom_modifier` (`TrackpadZoomModifier`: Cmd, Alt, Ctrl, Off) | Off | none | none | |

## View

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| View navigator: Axes (signed) / Cube (compact) | Two picture cards (190 x 149) | `Prefs.navigator_style` (`NavigatorStyle`: Axes, Cube) | Axes | none | `navigator-axes`, `navigator-cube` | The cards have no ground rectangle. The Axes figure draws a disc with signed axis labels; the Cube figure draws the view cube with TOP, FRONT, RIGHT faces. Paint from the `Tokens` in the app's navigator (`cube_face`, `cube_border`, `cube_text`, `axis_x`..`axis_z`). |
| Animate view changes | Checkbox | `Prefs.look_at_anim` | `true` | "Axis view, Look At, Home and Fit." | none | |
| Animation speed | Slider, "1.00x" readout | `Prefs.look_at_speed` (0.25 to 4.0; slider 0.5 to 3.0) | `1.0` | none | none | Disabled while Animate view changes is off. |

## Mismatches

* "Direct interaction" and "tool at cursor" move to Tools; all 3D mouse settings move to 3D mouse.
* The mouse mapping table is as drawn for Fusion only; rows must follow the chosen preset.
