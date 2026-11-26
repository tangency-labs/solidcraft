# 3D mouse

Board: `Prefs2_3DMouse` (1920x1625). Heading: Input. Subtitle "SpaceMouse and SpaceNavigator." No "In
profile" tag (the 3D mouse stays on this computer). Source today: the 3D mouse block of the Navigation
page (`prefs/navigation.rs`) and `crates/ui-egui/src/space_mouse.rs`.

## Device

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| SpaceMouse Compact + badges "Connected", "Calibrated" | Device card | none (device state) | n/a | none | none | Sample device name and states; build from the detected device. |
| Calibrate... | Button (82 x 28) | `Prefs.space_mouse_calibrations[device]` | none | none | none | Starts the calibration command; programmatic calls must not open dialogs. |
| Forget calibration | Button (120 x 28) | clears `space_mouse_calibrations[device]` | n/a | none | none | |
| Navigate with the 3D mouse | Checkbox | `Prefs.space_mouse_enabled` | `true` | none | none | |
| Show it in the 3D view | Checkbox | `Prefs.space_mouse_in_view` | `false` | "A picture that moves as the device does." | none | |
| Overall speed | Slider, "1.10x" readout | `Prefs.space_mouse_speed` | `1.0` | none | none | aria-label "Overall speed". The board sample reads 1.10x. |
| Picture position | Dropdown (240 wide, "Lower left of the viewport") | `Prefs.space_mouse_ghost_position` (`SpaceMouseGhost`) | lower left | none | none | |

## Orbit

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Turn around: Model / View centre | Segmented | `Prefs.space_mouse_model_pivot` (Model = `true`) | View centre (`false`) | "Model: the model in the middle of the view." | none | |
| Turn the view while a sketch is edited | Checkbox | `Prefs.space_mouse_sketch_orbit` | `false` | none | none | |

## Axes

A table with the columns "Axis", "Live input", "Speed" and a "Reset axes" button under it.

| Row | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Push X, Push Y, Push Z, Tilt X, Tilt Y, Tilt Z | Per row: a live bar with a dead-zone mark, an "Invert" checkbox, a speed readout ("1.00x"; Tilt Z shows 0.80x as a sample) | `Prefs.space_mouse_reverse[6]`, `Prefs.space_mouse_axis_speed[6]`, `Prefs.space_mouse_axis_source[6]` (`space_mouse::SAME_AXES`) | no inversion, 1.0, same axes | none | none | The live bars read the device; the dead zone is the device's calibration. `space_mouse_axis_source` (which physical axis feeds which row) has no control on the board. |
| Reset axis assignment | Button | resets `space_mouse_axis_source` to `SAME_AXES` only | n/a | none | none | Enabled only when the 3D mouse is on and an assignment differs. Leaves inversion and gains unchanged. The separate confirmed Restore defaults action keeps its broader scope. |

## Buttons

| Row | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Button 1: Open Preferences; Button 2: View . Fit; Button 3: View . Top; Button 4: Hold Shift; Button 5: Not assigned | One dropdown per device button (240 wide) | `Prefs.space_mouse_buttons[device][button]` (`space_mouse::ButtonJob`) | per device | none | none | Sample jobs. The list length follows the device. A device we have measured (SpaceMouse Pro, SpaceMouse Enterprise) names each row by the key printed on it (`space_mouse::button_name`: T, Esc, V1), others "Button N". `ButtonJob::Settings` ("Open Preferences") must open this page, not Navigation. |
| T, held: View . Bottom ("Button N, held" on an unmeasured device; beside each button's dropdown, under the column headings "Press" and "Held") | Dropdown, half the control column, with "As a short press" first | `Prefs.space_mouse_long_buttons[device][button]` | per device (measured long jobs through smed) | "A held Shift, Ctrl or Alt stays held: it has no job of its own when held." on a disabled one | none | Through smed every button has one; read otherwise only a button with a saved long job. "As a short press" removes the long job: the button then does its short job when held. A button that holds Shift, Ctrl or Alt gets it disabled, unless a long job is saved (so it can be taken back). Not on the board. |
| Rotation lock | Checkbox | the driver's rotation lock (smed's `set_lock`), not a preference | off | none | none | Only while the device is read through smed; the device's Lock key switches it too, and its LED shows it. Not on the board. |
| Restore default buttons | Button | removes `space_mouse_buttons[device]` and `space_mouse_long_buttons[device]` | n/a | none | none | Shown once either map is saved for the device. |

## Advanced

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Driver + badge "Linux" | Dropdown (200 wide, "Automatic") | `Prefs.space_mouse_driver` (`SpaceMouseDriver`: Automatic, Smed, Spacenavd, Direct) | Automatic | none | none | Linux only; hide elsewhere. Smed ("smed") is offered only while smed runs, or while it is the saved choice, so it can be switched away from; Automatic reads smed first while it has a device. |
| Navigate with a device app on this computer | Checkbox | `Prefs.nav_bridge_enabled` | `false` | "Connects to its navigation bridge. Off unless asked for." | none | |
| Port | Number field (80 wide) | `Prefs.nav_bridge_port` | `8765` | none | none | aria-label "Port". Enabled with the previous checkbox. |

## Mismatches

* `space_mouse_offer_never` and `space_mouse_offered` (the "offer to use the 3D mouse" memory) have no control.
* `space_mouse_adopted` (which devices read through smed are done taking over their direct settings: they have a calibration or button jobs of their own) has no control.
* The Press and Held columns and the Rotation lock checkbox are not on the board.
* The live bars and the dead zone are new drawing, not a setting.
* The board omits the shipped per-row motion/source dropdowns. Keep them and their swap semantics; Reset axis assignment restores source assignments only.
