# Sketch tool panel review

Inventory before the port, checked against `solidcraft-cli commands`, `dialogs::Kind`,
the sketch tool dispatchers, and all sketch sources' window/area helpers.
Control requests below are JSON lines sent to an already running app. Add the launch
token to the first request on each connection and use distinct request IDs.

Common setup **S**:

```json
{"id":1,"method":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}}
{"id":2,"method":"engine.execute","params":{"command":"sketch.rectangle.two_point","params":{"p0":[10,10],"p1":[40,30]}}}
{"id":3,"method":"ui.view","params":{"view":"top"}}
{"id":4,"method":"ui.view","params":{"view":"fit"}}
```

**Start C** means the exact request
`{"id":5,"method":"ui.start","params":{"command":"C"}}`, replacing C with the
literal command in the table. Panels open without selections. To exercise them,
click their named selection fields and the rectangle's curves/corners in the canvas;
`ui.selection` reports inputs and selected IDs, `ui.at` maps world coordinates
to screen coordinates, and `ui.click {x,y}` sends the pick. Cancel between cases.

| Panel | Source before port | How to open | Existing rows | Existing footer |
| --- | --- | --- | --- | --- |
| Create Sketch | `dialogs.rs` | Fresh design, Start `sketch.create` | Plane; attachment failure and explicit fixed-plane fallback | Cancel; picking a plane creates immediately |
| Scale | `sketch_modify_ui.rs`, shared window in `dialogs.rs` | S; Start `sketch.scale` | Entities, Base point, Factor | OK, Cancel |
| Offset | same | S; Start `sketch.offset` | Curves, Direction icons, Distance, Distance 2 (Two Sides), Flip (except Symmetric), Chain | OK, Cancel |
| Mirror | same | S; Start `sketch.mirror` | Entities, Mirror line, selection step/guidance | OK, Cancel |
| Move/Copy | same | S; Start `sketch.move` with no text selected | Entities, Center, X Distance, Y Distance, Angle, Create Copy | OK, Cancel |
| Rectangular Pattern | same; edits from `sketch_pattern_tree.rs` | S; Start `sketch.pattern.rectangular` | Entities; Direction, Quantity, Spacing, Alignment icons; Direction 2, Quantity 2, Spacing 2, Alignment 2 icons; guidance | OK, Cancel |
| Circular Pattern | same; edits from `sketch_pattern_tree.rs` | S; Start `sketch.pattern.circular` | Entities, Center, Quantity, Total angle, Alignment icons, guidance | OK, Cancel |
| Fillet | same | S; Start `sketch.fillet` | Corner rows (picked lines paired into corners; a row's X turns its corner off), Radius | OK, Cancel |
| Equal-distance Chamfer | same | S; Start `sketch.chamfer.equal_distance` | Corner rows, Chamfer Type icons, Distance | OK, Cancel |
| Two-distance Chamfer | same | S; Start `sketch.chamfer.two_distance` | Corner rows, Chamfer Type icons, Distance 1, Distance 2, Flip on each row | OK, Cancel |
| Distance-angle Chamfer | same | S; Start `sketch.chamfer.distance_angle` | Corner rows, Chamfer Type icons, Distance 1, Angle, Flip on each row | OK, Cancel |
| Text / Edit Text | `sketch_text.rs`, shared window | S; Start `sketch.text`, then `ui.at {"world":[50,20,0]}` and `ui.click` the returned position; editing uses Start `sketch.edit_text`, then pick a text outline | Text (multiline), Font (search/list/file), Style, Alignment, X, Y, Height, Angle, Letter Spacing, Line Spacing, Path, Reverse (on path) | OK, Cancel; live edits collapse into one undo step |
| Move/Copy Text | `sketch_text_move.rs`, shared window | Create text as above; close its editor with OK; select a text outline; Start `sketch.move` | Distance X, Distance Y, Angle, Create Copy | OK, Cancel |
| Edit Dimension (panel form) | `dialogs.rs` | `Dialog::edit_param`; legacy programmatic UI constructor, no current control-channel entry point. Interactive dimensions use the inline editor below | Parameter name, expression | OK, Cancel |
| Redefine Sketch Plane | `sketch_attachment_ui.rs` | Create sketch; finish sketch; select its browser/context-menu Redefine Sketch Plane action (control recipe below) | Attachment status; author placement repair; XY/XZ/YZ or construction planes; selected-face reattach; saved/selected fixed plane; preview/error | Apply (commits and closes), Cancel |

Not separate popup panels: `sketch_tools.rs` and `tools.rs` drawing/trim/extend/
break/stretch/project/include state machines; drawing family choices already use
the kit in `sketch_palette.rs`. Polygon Sides, slot Width, conic Rho, arc Sweep,
and other dimension boxes in `sketch_dims.rs` are inline canvas value editors.
`dim_view.rs` is also an inline editor anchored to the dimension/pointer, with
focus-loss commit/cancel and no panel actions. These stay inline. The pattern tree
opens the same modify dialog, not another window.

Positioning follows feature dialogs: right when alone; left while a sufficiently
wide sketch viewport shows the palette at right. Narrow viewports hide the palette
while the dialog is open. Preserve `palette_rect` / `dialog_rect` and their tests.

The port uses command-spec icons and labels, close X, a guidance hint, kit selection
fields and picked-item lists, expression/unit fields, paired X/Y values, Options
sections and the shared pinned footer. Numeric defaults and command builders are
unchanged. The rectangular pattern's two directions now use stacked dense sections;
its references and independent alignment controls remain available. Old field
handles, including Distance X/Y, X/Y Distance and text controls, are retained.

Final footer actions:

- Create Sketch: disabled OK plus Cancel; choosing a plane still creates immediately.
- New offset, mirror and rectangular/circular patterns: OK / Apply / Cancel. Apply
  commits one operation, retains numeric options, clears picks and waits for another
  selection. This also works when evaluation finishes asynchronously.
- Editing a stored sketch pattern: OK / Cancel; edits keep the same pattern ID.
- Other modify tools, text, text move/copy, Edit Dimension and plane reattachment:
  OK / Cancel. Reattachment's former Apply already committed and closed; its new
  equivalent is OK.

Dropped affordances: the legacy collapse minus/plus and expand chevrons (−/+/»/«)
are removed from sketch dialogs; the separate footer info glyph is replaced by the
header hint. The reattachment panel uses the shared close X. Full guidance is also
available by hovering a clipped hint.

New kit elements: `focused_field` (retains single-value popup focus),
`multiline_text_field` (three-line text row),
`searchable_dropdown` / `SearchOption` (font search, family list, font-file choice,
loading state), `value_pair_named` (X/Y with established field names),
`text_emphasis` (B/I toggles), and `segmented_compact` (alignment choices remain
visible when long labels would otherwise switch to a dropdown). All have gallery
examples in both themes; multiline focus and search/custom selection have direct
interaction tests. Existing `hint` now exposes its full text on hover. `Panel`
uses the same bounded anchor for painting and dragging, preserving exact title-drag movement at the
shell clearance (the existing Create Sketch regression catches this).

No raw interactive widgets remain in the ported sketch panels. Raw canvas value
editors in `sketch_dims.rs` and `dim_view.rs` remain because their focus and placement
belong to drawing/dimension gestures. Existing non-sketch legacy dialogs still
use `frame::docked_on` and `dock_frame`, so those shared helpers remain in use.
The `dialogs.rs` raw-widget allowance shrinks from 14 to 13; no sketch-panel
allowance was added. The other existing allowances are assembly 6, plastic 1,
and sheet metal 4, for their documented non-dense paths. No artwork, font or icon
asset was added; headers reuse registry metadata. Eight new message templates have
translations in all 13 catalogs.

No native app or screenshot is produced in this worktree, as instructed. Visual
review in the owner's running app remains necessary; headless tests prove layout,
interaction and parameter behavior, not final screenshot acceptance.

## Additional exact control recipes

The window-free tests use the same routes. These recipes are source-checked;
execute them in the owner's already running app and inspect the result there.
`ui.at` returns `[x,y]`; copy those two numbers into the following `ui.click`.
Use `ui.key {"key":"Escape"}` to close between cases.

Preselect a source after S, before starting scale, move, offset or a pattern:

```json
{"id":10,"method":"engine.execute","params":{"command":"select.set","params":{"items":[{"type":"sketch_curve","id":"l1"}]}}}
{"id":11,"method":"ui.start","params":{"command":"sketch.offset"}}
```

Replace `sketch.offset` with the literal command from the inventory. For fillet or
chamfer, preselect `{"type":"sketch_point","id":"p1"}` instead. `l1` and `p1`
are the first rectangle's IDs in a fresh design; the rectangle command response
also reports the exact `sides` and `corners` IDs for a non-empty design. For mirror,
start `sketch.mirror` with only `l1` selected, then pick the left rectangle edge:

```json
{"id":12,"method":"ui.dialogInput","params":{"index":1}}
{"id":13,"method":"ui.at","params":{"world":[10,20,0]}}
```

Send `{"id":14,"method":"ui.click","params":{"x":X,"y":Y}}` using response 13.
For circular pattern, start `sketch.pattern.circular`, use input index 1, and map
`[0,0,0]` instead to pick its centre. Rectangular directions are optional; pick a
straight edge with `ui.dialogInput` index 1 or 2 to replace either default direction.

To open Edit Text deterministically without placing it with the pointer:

```json
{"id":20,"method":"engine.execute","params":{"command":"sketch.text","params":{"text":"II","at":[50,20],"height":8}}}
{"id":21,"method":"engine.execute","params":{"command":"select.set","params":{"items":[{"type":"sketch_curve","id":"FIRST_CURVE_FROM_RESPONSE_20"}]}}}
{"id":22,"method":"ui.start","params":{"command":"sketch.edit_text"}}
```

Substitute the first string in response 20's `curves` array. To open Move/Copy Text,
close the text editor with OK, select a current text outline (live editing can
regenerate its curve IDs), and send:

```json
{"id":23,"method":"ui.start","params":{"command":"sketch.move"}}
```

Plane reattachment, for S's sketch ID 1 in a fresh design:

```json
{"id":30,"method":"engine.execute","params":{"command":"sketch.finish","params":{}}}
{"id":31,"method":"ui.menu","params":{"x":700,"y":400,"target":{"type":"sketch","id":1}}}
{"id":32,"method":"ui.menuPick","params":{"item":"ui.redefineSketch"}}
```

Use the `sketch` ID returned by `sketch.create` instead of 1 in an existing design.
The panel offers XY/XZ/YZ, construction planes, selected-face attachment and the
explicit fixed-plane repair actions. It runs `sketch.redefine` on OK.

To reopen an existing pattern, run either pattern command with a source (for
example `sketch.pattern.rectangular {"entities":["l1"],"count":3,"spacing":20}`),
select one of the returned copy curves, then:

```json
{"id":40,"method":"ui.menu","params":{"x":700,"y":400,"target":{"type":"viewport"}}}
{"id":41,"method":"ui.menuPick","params":{"item":"ui.editSketchPattern"}}
```

Double-clicking the pattern's browser row or canvas badge opens this same panel.
`sketch_pattern_tree.rs` contains the browser route; no duplicate window exists.
The panel-only `Dialog::edit_param` constructor has no current public control route;
its card, expression parameters and OK are covered by `dialogs_sketch_tests.rs`.
The interactive `sketch.dimension` and double-click dimension routes continue to
open their inline canvas editor, deliberately outside this panel port.

## Validation

Every Cargo command uses
`CARGO_TARGET_DIR=/Users/dane/GitHub/solidcraft-dialogs/target CARGO_INCREMENTAL=0`.
Commands run sequentially, without `nice`, temporary targets, standalone test
binaries, a native app launch, screenshots or process-management commands.

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo run -p solidcraft-cli -- commands` | Passed; registry used for inventory | `/private/tmp/solidcraft-sketch-commands.json` (subsequent CLI registry capture) |
| `cargo test -p solidcraft-ui-egui --lib sketch_ --no-fail-fast` | 228 passed | `/private/tmp/solidcraft-sketch-tests.log` |
| `cargo test -p solidcraft-ui-egui --lib dialogs --no-fail-fast` | 191 passed | `/private/tmp/solidcraft-dialog-tests.log` |
| `cargo test -p solidcraft-ui-egui --lib browser --no-fail-fast` | 39 passed | `/private/tmp/solidcraft-browser-check.log` |
| `cargo test -p solidcraft-ui-egui --lib frame::tests --no-fail-fast` | 3 passed, including exact title-drag displacement | `/private/tmp/solidcraft-frame-tests-check.log` |
| `cargo test -p solidcraft-ui-egui --lib apply_off_thread_tests --no-fail-fast` | 4 passed | `/private/tmp/solidcraft-apply_off_thread_tests-final-check.log` |
| `cargo test -p solidcraft-ui-egui --lib sketch_attachment_ui_tests --no-fail-fast` | 8 passed | `/private/tmp/solidcraft-sketch_attachment_ui_tests-final-check.log` |
| `cargo test -p solidcraft-ui-egui --lib ui_kit --no-fail-fast` | 22 passed, including gallery, new field interactions and panel guard | `/private/tmp/solidcraft-ui_kit-final-check.log` |
| `cargo test -p solidcraft-ui-egui --lib font_literals_tests --no-fail-fast` | 2 passed | `/private/tmp/solidcraft-font_literals_tests-final-check.log` |
| `cargo test -p solidcraft-ui-egui --lib colour_tokens_tests --no-fail-fast` | 5 passed | `/private/tmp/solidcraft-colour_tokens_tests-final-check.log` |
| `cargo test -p solidcraft-ui-egui --lib i18n --no-fail-fast` | 33 passed | `/private/tmp/solidcraft-i18n-final-check.log` |
| `cargo test -p solidcraft-ui-egui --lib shell --no-fail-fast` | 16 passed | `/private/tmp/solidcraft-shell-final-check.log` |
| `cargo fmt` | Passed | No output; subsequent `git diff --check` passed |
| `cargo clippy -p solidcraft-ui-egui --all-targets -- -D warnings` | Passed | `/private/tmp/solidcraft-clippy.log` |
| `cargo check -p solidcraft-ui-egui --target wasm32-unknown-unknown` | Passed | `/private/tmp/solidcraft-wasm.log` |
| `cargo xtask assets` | Passed; 387 files attributed | `/private/tmp/solidcraft-assets.log` |
| `cargo xtask layers` | Passed; 16 crates, no violations | `/private/tmp/solidcraft-layers.log` |
| `cargo test -p solidcraft-ui-egui --lib` (one full run) | 1252 passed, the 8 reported baseline failures, 1 already-ignored test; **no additional failures** | `/private/tmp/solidcraft-ui-lib-tests.log` |

The full-library command returned 101 solely for the eight baseline failures
identified in the task. No additional failing tests were found. No test was newly
ignored or disabled. No `cargo xtask ci` result is claimed: this task ran the
explicitly requested separate gates and one full UI-library test command.
The native screenshot/recording acceptance remains for the owner, as instructed;
passing these checks does not assert visual acceptance of every panel.

Work remains on `ui/sketch-tool-panels`, based on `a4dc6afc` (the UI kit extraction).
This branch has no configured tracking upstream. No commit, push, PR, rebase or
merge was performed. Generated feature/license documents and design formats were
not changed.
