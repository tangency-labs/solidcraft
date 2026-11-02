# Shortcuts

Board: `Prefs2_Shortcuts` (1920x992). Heading: Input. Subtitle "Keys for each command." Tag: In
profile. Source today: Keyboard Shortcuts page (`crates/ui-egui/src/keymap.rs`, customised keys in
`app.keymap.custom`). This page is the only fixed-height dialog: the command list scrolls
(content area 646 wide; list about 580 high on the board).

## Toolbar of the page

| Label | Control | Edits | Default | Hover | Figure | Notes |
|---|---|---|---|---|---|---|
| Filter commands or keys | Search field, full width | filters the list only | empty | none | none | |
| Done | Primary button (52 x 28) | leaves "Press a key..." capture mode | n/a | none | none | |
| Export | Button (79 x 28) | exports `app.keymap.custom` | n/a | none | none | Programmatic calls must not open dialogs. |
| Import | Button (81 x 28) | imports a keymap | n/a | none | none | |

## List

A list grouped by menu group with a group heading (11 px, upper-cased: File, Edit, View, Sketch,
Features). Each row: the command name at the left and its key caps at the right (IBM Plex Mono
10.5 px key caps), then two icon buttons (reset to default, clear).

| Row state | Drawing | Notes |
|---|---|---|
| Assigned | Command name and key caps ("Ctrl" "N") | |
| Not assigned | The text "Not assigned" in `form_dim` | |
| Capturing | The text "Press a key..." in the accent | Exactly one row at a time. |

The groups and commands on the board (New design Ctrl N, Open Ctrl O, Save Ctrl S, Save as Ctrl
Shift S; Undo Ctrl Z, Redo Ctrl Shift Z, Cut Ctrl X, Copy Ctrl C, Paste Ctrl V, Delete Del; Fit F,
Home view Home, Look at Not assigned; Line Press a key..., Rectangle, Circle, Trim Not assigned;
Extrude, Fillet, Hole, Shell Not assigned) are **sample content**: build the list from the command
registry and the keymap (`keymap.rs`).

## Mismatches

* The per-row "reset" and "clear" affordances are two icon buttons on the board; check `keymap.rs` for
  what today's keymap edit offers before building them.
* The board's footer is the shared "Restore page defaults / Cancel / OK".
