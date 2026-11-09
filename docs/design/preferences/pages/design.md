# Design

Board: `Prefs2_Design` (1920x1196). Heading: Work. Subtitle "What new designs start with." No "In
profile" tag. Source today: Design page (`prefs/design.rs`).

## Units and precision

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Units of new designs | Segmented: mm / cm / m / in / ft | `Prefs.default_units` | `"mm"` | none | none | |
| Length decimals | Stepper (56 wide, aria "Length decimals") with the live sample "12.50 mm" | `Prefs.length_decimals` | `2` | none | none | |
| Angle decimals | Stepper (aria "Angle decimals") with the live sample "45.0 deg" | `Prefs.angle_decimals` | `1` | none | none | |
| Parameter list rows | Stepper (aria "Rows") | `Prefs.parameter_list_rows` (5 to 200, `PARAMETER_LIST_ROWS`) | `50` | "The most parameters Down arrow lists in an empty expression box." | none | |
| Step values with arrows and wheel | Checkbox | `Prefs.step_values` | `false` | "Up/Down and the wheel over a value field change its number." | none | |
| Show values beside their handles | Checkbox | `Prefs.value_boxes_in_view` | `true` | "Extrude's Distance, Fillet's Radius... as a box in the view." | none | |

## Orientation

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Z up (default), Y up | Two picture cards (150 x 146) | `Prefs.up_axis` (`"z"`, `"y"`) | `"z"` | none | `orientation-z-up`, `orientation-y-up` | The figures are copied from the app (`Settings_Pages`); the existing orientation figure in `ui_kit` stays as it is. |
| Offer to stand up Y-up files | Checkbox | `Prefs.offer_stand_up` | `true` | "After a STEP or IGES file opens with Z up, offer a quarter turn about X." | none | |

## Imported files

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Files from unknown programs | Dropdown (200 wide, "Ask" as drawn) | `Prefs.stand_up_unknown` (`StandUpUnknown`: Ask, Note, Off) | Note (the board sample shows Ask) | "How the offer is made when the file names no known program." | none | aria-label "Ask". |

## Origin

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Origin plane names | Segmented: "XY . XZ . YZ" / "Top . Front . Right" | `Prefs.origin_plane_names` (`OriginPlaneNames`: Xyz, TopFrontRight) | Xyz | none | none | Moves from Interface. |
| Hide the Origin folder in new designs | Checkbox | `Prefs.hide_origin` | `false` | none | none | |

## Linked designs

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Update changed links when a tab comes back | Checkbox | `Prefs.update_changed_links` | `true` | none | none | |
| Double-click a component to activate it | Checkbox | `Prefs.double_click_components` | `true` | "Or to edit a linked one in place." | none | |
| Insert Design lists open designs first | Checkbox | `Prefs.insert_lists_open_designs` | `true` | none | none | |

## Mismatches

* Hole, Extrude, Placement and Assembly Move controls (today on Design) move to Tools; network
  locations moves to General > Files; "Show moved parts' old place as a ghost"
  (`Prefs.drive_joints_ghost`) moves to Tools.
* `parameters_apply_live` has no control on this board either.
