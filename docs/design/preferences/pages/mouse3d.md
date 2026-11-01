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
| Reset axes | Button | resets the three arrays | n/a | none | none | |

## Buttons

| Row | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Button 1: Open Preferences; Button 2: View . Fit; Button 3: View . Top; Button 4: Hold Shift; Button 5: Not assigned | One dropdown per device button (240 wide) | `Prefs.space_mouse_buttons[device][button]` (`space_mouse::ButtonJob`) | per device | none | none | Sample jobs. The list length follows the device. `ButtonJob::Settings` ("Open Preferences") must open this page, not Navigation. |

## Advanced

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Driver + badge "Linux" | Dropdown (200 wide, "Automatic") | `Prefs.space_mouse_driver` (`SpaceMouseDriver`: Automatic, Spacenavd, Direct) | Automatic | none | none | Linux only; hide elsewhere. |
| Navigate with a device app on this computer | Checkbox | `Prefs.nav_bridge_enabled` | `false` | "Connects to its navigation bridge. Off unless asked for." | none | |
| Port | Number field (80 wide) | `Prefs.nav_bridge_port` | `8765` | none | none | aria-label "Port". Enabled with the previous checkbox. |

## Mismatches

* `space_mouse_offer_never` and `space_mouse_offered` (the "offer to use the 3D mouse" memory) have no control.
* The live bars and the dead zone are new drawing, not a setting.
