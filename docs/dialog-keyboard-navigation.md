# Dialog keyboard navigation

This is the keyboard contract and regression checklist for feature dialogs. The
headless scenarios use the real egui UI through `scenario::Harness`: they send
keys and canvas picks, inspect actual focus, apply the command, and check the
result. Focus assertions live in the existing Rust test files because JSON
scenarios do not expose focused-widget assertions. Existing JSON workflows,
including `s30_pattern_a_hole`, remain complementary end-to-end coverage.

## Expected behavior

| Context | Key | Expected result |
| --- | --- | --- |
| Open dialog | Tab | Advance to the next enabled control inside that dialog. |
| Open dialog | Shift+Tab | Reverse the same order. |
| Last/first control | Tab / Shift+Tab | Wrap in one key press; never visit the canvas, ribbon, browser, or Sketch Palette. |
| Floating Extrude distance or Fillet/Chamfer value | Tab | Enter the form at Profiles or Edges, skipping the title and Close icon. |
| Pattern/Mirror canvas selection | Tab | Continue from the active selection input to its next reference or field. Preserve all picks. |
| Selection chip | Space | Activate that canvas input; do not apply or clear its selection. |
| Selection chip | Enter | Apply the existing valid configuration; do not activate a different input. |
| Numeric expression field | Tab into field | Select only a plain numeric value, leaving its unit intact. For an expression, select the whole expression. |
| Checkbox | Space | Toggle its value, retaining the dialog. |
| Checkbox | Enter | Apply without toggling. Works for checked, unchecked, mixed, grid, and full-row variants. |
| Text/icon segments, such as G1/G2 or alignment | Tab | Focus a choice without selecting it. |
| Text/icon segments | Space | Select the focused choice without applying. |
| Text/icon segments | Arrows | Select and focus an adjacent enabled choice; skip disabled choices and stay inside the group at its boundaries. |
| Text/icon segments | Enter | Apply without selecting the focused choice. A focused G2 must not replace the already selected G1. |
| Closed dropdown | Enter | Apply without opening the popup or changing its value. |
| Closed dropdown | Space | Open the popup. |
| Closed indexed dropdown | Up/Down | Choose the previous/next option, keeping focus on the field. Stop at the ends. |
| Open indexed dropdown | Open | Highlight the currently selected option. |
| Open indexed dropdown | Up/Down | Move the pending highlight only; wrap between the first and last options. Keep focus with the dropdown. Left/Right do not move the highlight. |
| Open indexed dropdown | Space / Enter | Accept the highlighted option and close the popup. This Enter belongs to the popup, not OK. |
| Open dropdown popup | Tab / Shift+Tab | Close without accepting a pending choice; advance to the next/previous parent form control. Never Tab through the options. |
| Open indexed dropdown | Escape | Close without changing the value; keep the parent dialog open. |
| Completion suggestions or text context menu | Owned keys | Let that popup handle its own keys before the dialog. |
| Action button | Enter or Space | Perform that button's action. Cancel still cancels; it is not a value field. |
| Invalid or incomplete configuration | Enter | Keep the dialog open; do not create geometry or an undo step. |
| Dialog, with no popup owning Escape | Escape | Cancel without committing preview geometry or field edits. |

Tab skips section headings, the title drag area, Close icons, quantity up/down
arrows, and disabled controls. Quantity arrows and Close remain mouse controls.
Pattern/Mirror auxiliary selection clear/remove targets are also pointer-only.
Other enabled action buttons remain in the native order. The top-button layout
still has real OK/Cancel actions; a `dialog:close` automation alias that points to
Cancel does not turn Cancel into a pointer-only control.

Tab alone must never change a value, create/delete geometry, clear a reference,
or add an undo step. It may change the active canvas input, so the next canvas
pick is assigned to the focused reference. Ctrl-click selection and modified
Tab shortcuts retain their existing behavior.

## Automated feature scenarios

Each row identifies an executable scenario or parameterized group. Both top and
bottom dialog-button layouts are covered where shown. Full-cycle checks record
all actual widget IDs, wrap forward, then require Shift+Tab to visit exactly the
reverse sequence. They also compare the document, undo history, ordinary
selection, and stored input picks before and after navigation.

| ID | Scenario and required assertions | Test location |
| --- | --- | --- |
| SK-M | Sketch Mirror: pick entities, Tab to Mirror line, pick a line, reverse to Entities, Ctrl-click changes only the entity set. Space does not apply; Enter on the completed reference creates mirrored curves. | [sketch_guidance_ui_tests.rs](../crates/ui-egui/src/sketch_guidance_ui_tests.rs): `dialog_tab_mirror_advances_from_canvas_and_reverses_without_changing_picks`, `dialog_tab_enter_on_mirror_selection_applies_the_dialog` |
| SK-R | Sketch rectangular pattern: directions, quantities and spacings remain reachable in displayed order; headings and step arrows are skipped. Enter applies the typed 3-by-2 count, producing five new curves from one seed. | Same file: `dialog_tab_patterns_follow_display_order_in_both_directions`, `dialog_keyboard_sketch_patterns_enter_applies_the_typed_quantities` |
| SK-C | Sketch circular pattern: Entities → Center → quantity/angle. Enter from the center reference applies three instances, producing two new curves. | Same pattern tests |
| SK-W | All three sketch dialogs: complete forward/reverse cycles in both button layouts, then Escape. Document and undo history must be unchanged, including a freely picked circular center. | Same file: `dialog_keyboard_sketch_mirror_and_patterns_cycle_without_committing_and_cancel` |
| SK-I | All three sketch dialogs: Enter with no entities refuses to apply. Missing Mirror line or zero pattern quantity also refuses. Escape preserves the original sketch and undo history. | Same file: `dialog_keyboard_sketch_empty_selection_and_invalid_inputs_do_not_apply` |
| BO-M | Body Mirror: both preselected and canvas-picked seeds; Tab reaches the plane, reverse returns to bodies, full cycles preserve picks, and Enter creates two bodies. Enter on focused Join must preserve the selected New Body operation. | [dialogs_all_tests.rs](../crates/ui-egui/src/dialogs_all_tests.rs): `dialog_keyboard_body_mirror_and_patterns_traverse_and_apply` |
| BO-R | Body rectangular pattern: preselected/picked seed, X/Y references, typed 3-by-2 quantities, field order and unit-preserving spacing edits, full cycles, then Enter from a reference. Six bodies, total volume six times the seed; exactly one application undo step. | Same parameterized body test |
| BO-C | Body circular pattern: preselected/picked seed and Z axis, three instances, a unit-preserving partial-angle edit, full cycles, then Enter from the axis. Three bodies, total volume three times the seed; one undo step. | Same parameterized body test |
| BO-W | Body Mirror and both body patterns: valid references, changed pattern quantities, complete forward/reverse cycles, then Escape. Test both button layouts and both preselection/canvas-pick entry paths. Keep exactly the original seed body, document and undo history. | Same file: `dialog_keyboard_body_valid_preview_escape_preserves_the_original_model` |
| BO-I | Body Mirror/Patterns: cleared body selection and missing required reference reject Enter; rectangular/circular quantity zero rejects Enter. Escape preserves the original body and undo history. | Same file: `dialog_keyboard_body_missing_reference_and_invalid_quantity_do_not_apply` |
| BO-K | Body Mirror and both body patterns: Ctrl/Alt/Command+Tab, with and without Shift, do not advance the active selection input or change picks/model. | Same file: `dialog_keyboard_modified_tabs_do_not_advance_body_selection_inputs` |
| BL-F | Fillet and Chamfer: the initially focused floating value is followed by Edges, not Close. Both button layouts complete a forward/reverse loop without modifying the model. | [blend_arrow_tests.rs](../crates/ui-egui/src/blend_arrow_tests.rs): `dialog_tab_blends_from_floating_values_start_at_edges_and_wrap` |
| BL-G | Fillet: G1 selected, G2 focused using Tab. Enter applies the existing G1 fillet and closes the dialog. | Same file: `segmented_enter_applies_fillet_without_changing_continuity` |
| EX-T | Extrude: Tab from the floating distance reaches Profiles. Expanded Advanced fields participate, Close/title do not, and first/last controls wrap in one key press. | [extrude_edit_tests.rs](../crates/ui-egui/src/extrude_edit_tests.rs): `dialog_tab_extrude_enters_from_canvas_distance_and_never_leaves_the_dialog` |
| EX-B | Extrude: Enter on Reverse Direction applies without flipping it, both unchecked and after Space checks it. Verify positive/negative extrusion bounds. | Same file: `checkbox_enter_applies_extrude_without_toggling_reverse` |
| EX-D | Extrude: Enter on highlighted Direction, Extent Type, Operation, and Start closes the dialog without opening a popup or changing the feature's extent. | Same file: `dialog_keyboard_enter_on_closed_extrude_dropdowns_applies_without_changing_values` |
| EX-P | Extrude: use Space/Down/Space to select Two Sides, then cycle Extent Type through To Object, All and back to Distance. Confirming closes each popup. Tab dismisses a pending To Object choice and reaches Distance; Shift+Tab returns to Extent Type. Cancelling preserves the document and undo history. | Same file: `dropdown_extrude_menu_confirm_wrap_and_tab_exit_preserve_form_focus` |
| MV-T | Move: Tab leaves the floating axis editor for the dialog instead of cycling floating axes; reverse traversal and applying a typed Y distance still work. | [dialogs_move_tests.rs](../crates/ui-egui/src/dialogs_move_tests.rs): `dialog_tab_move_enters_the_dialog_instead_of_cycling_canvas_axes` |
| PM-T | Parameters manager: repeated forward/reverse Tab remains inside the manager. | [params_dialog.rs](../crates/ui-egui/src/params_dialog.rs): `dialog_tab_parameters_stays_inside_its_manager` |

## Automated shared-control scenarios

| ID | Coverage | Test location |
| --- | --- | --- |
| UI-F | Dialog-only wrap, exact enabled-control set, disabled fields skipped, visible accent focus on dropdowns and normal/primary/text buttons. | [panel_tests.rs](../crates/ui-egui/src/ui_kit/panel_tests.rs): `dialog_tab_window_wraps_in_both_directions_and_excludes_background_fields`, `dialog_tab_dropdown_and_actions_paint_a_focus_outline` |
| UI-D | Enter-to-OK and Space-to-open for indexed dropdowns, picked-value dropdowns, custom menus, and searchable dropdowns. Closed arrows still work; indexed menus start at the current choice, wrap with Up/Down, ignore Left/Right, confirm/close with Space or Enter, and cancel with Escape. Tab/Shift+Tab dismiss all four menu variants and reach the next/previous form control. | Same file: `dialog_keyboard_closed_dropdown_enter_applies_and_space_opens_all_variants`, `dialog_tab_dropdown_arrows_change_the_focused_choice`, `dialog_tab_dropdown_closes_and_leaves_in_both_directions`, `dropdown_open_menu_starts_at_current_wraps_and_space_confirms` |
| UI-B | Plain/mixed, grid and full-row checkboxes: Space toggles, Enter preserves the value and applies; disabled and mouse behavior remain correct. | [checks_tests.rs](../crates/ui-egui/src/ui_kit/checks_tests.rs): `checkbox_keyboard_pointer_disabled_and_mixed`, `checkbox_variants_use_space_to_toggle_and_enter_to_apply_the_dialog` |
| UI-S | Text and icon segments: Enter preserves selection/applies, Space selects, all four arrows select. Disabled choices are skipped; boundary arrows retain focus in the group. | [choice_tests.rs](../crates/ui-egui/src/ui_kit/choice_tests.rs): `segmented_space_and_arrows_choose_while_enter_applies_the_dialog`, `segmented_arrows_skip_disabled_options_and_stay_in_the_group` |
| UI-N | Numeric selection preserves mm/cm/degree/inch units, negative/decimal/comma-decimal values and leading Unicode whitespace; expressions select as a whole. Quantity arrows are skipped by Tab but still work with the mouse. | [fields_tests.rs](../crates/ui-egui/src/ui_kit/fields_tests.rs): `tab_selects_the_number_and_typing_preserves_units`, `tab_skips_count_arrows_in_both_directions_and_pointer_still_steps` |
| UI-P | Completion retains ownership of Tab/Enter; Escape from the text context menu closes that menu rather than its owner. | [expression_complete.rs](../crates/ui-egui/src/expression_complete.rs): `tab_accepts_selection_filtering_resets_it_and_escape_dismisses`; fields tests: `esc_closes_the_text_menu_alone` |

## Manual acceptance checklist

These are additional visual/platform checks, not claims of automated coverage.
Run the high-risk cases on each of the six Mirror/Pattern dialogs, Fillet,
Chamfer, and Extrude. Use the remaining combinations when their controls exist.

- [ ] Start with no selection, one preselected item, multiple preselected items,
  and a selection picked after opening. Check the next canvas pick enters the
  highlighted input and Ctrl-click removes only the intended picked entity.
- [ ] Walk the entire form in both directions. At the ends, one press wraps;
  there is no invisible stop on a heading, Close icon, title, or quantity arrow.
- [ ] Press Tab quickly and hold it briefly. Every final focus target is a
  visible enabled control inside the active dialog.
- [ ] Check filled and empty references, clear a reference with the mouse, then
  Tab again. Missing required references must never apply accidentally.
- [ ] Rectangular patterns: one direction, both directions, quantity 1, a small
  grid, changed spacing, negative spacing, and optional second direction left
  empty. Circular patterns: axis/center selection, full/partial turn, quantity
  1 and multiple instances, and both angle units and alignment controls.
- [ ] On a numeric field, replace the selected number directly without Ctrl+A.
  Verify the unit suffix remains. Repeat for a negative decimal, a parameter
  expression, an angle, and a changed document unit.
- [ ] On every checkbox, test unchecked and checked Enter, Space, and a mouse
  click. Only Space/mouse changes the value; Enter applies valid data.
- [ ] On a segment, first focus an unselected option with Tab. Enter must retain
  the previous selection. Separately test Space and arrows, including disabled
  options and both ends of the group.
- [ ] On a closed dropdown, Enter applies without opening it. Reopen the dialog,
  use Space to open the menu and verify the current value is highlighted. Up/Down
  wraps, Left/Right leaves the highlight alone, and Space/Enter confirms and
  closes. Reopen and check Tab/Shift+Tab dismisses without committing a pending
  choice and reaches the next/previous form control. Escape cancels the popup
  before cancelling the form.
- [ ] Open/close Options, Advanced, Variable Radius and Direction 2 sections.
  Check collapsed and disabled fields are skipped and newly shown fields join
  the order. Change a dropdown that adds/removes fields, such as Extrude's extent
  type or Fillet's type, and check no removed widget retains focus.
- [ ] Enter with missing entities, missing references, invalid expressions and
  an invalid count. No geometry or undo entry should be created.
- [ ] Cancel after changing values and inspecting previews. Reopen, apply once,
  then Undo/Redo: the original model and resulting copies must be restored
  appropriately, without an extra keyboard-induced operation.
- [ ] Exercise both new and edited features, both top/bottom button placements,
  both panel sides, and a small window that requires scrolling.
- [ ] Open an owned popup or a second modal window. Only the active surface
  should own Tab; close it and check the parent resumes normal navigation.
- [ ] Close the dialog and use normal canvas/browser/ribbon shortcuts. Reopen a
  different tool; there must be no stale focus trap from the previous dialog.
- [ ] Confirm visible focus in light/dark themes and at supported display scales.
  Hovering Close must not be mistaken for keyboard focus. Check keyboard and
  assistive-technology labels still describe the correct controls.

## Running the checks

From the repository root:

```sh
cargo test --profile ci -p solidcraft-ui-egui --lib dropdown
cargo test --profile ci -p solidcraft-ui-egui --lib dialog_keyboard
cargo test --profile ci -p solidcraft-ui-egui --lib dialog_tab_
cargo test --profile ci -p solidcraft-ui-egui --lib ui_kit
cargo test --profile ci -p solidcraft-ui-egui --lib blend_arrow_tests
cargo test --profile ci -p solidcraft-ui-egui --lib extrude_edit_tests
cargo test --profile ci -p solidcraft-ui-egui --lib sketch_guidance_ui_tests
cargo test --profile ci -p solidcraft-ui-egui --lib dialogs::dense::all_tests
cargo clippy --profile ci -p solidcraft-ui-egui --lib --tests -- -D warnings
```

The tests render headlessly and do not drive the user's running desktop app.
Run the repository-wide `cargo xtask ci` gate before committing; targeted checks
are not a claim that the full workspace, WebAssembly, or web-offline gate passed.

For a manual build and launch:

```sh
cargo build --profile ci -p solidcraft --no-default-features
./target/ci/solidcraft
```

Use a separate window for verification so an existing unsaved design is retained.
