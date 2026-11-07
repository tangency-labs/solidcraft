# W23 assembly sheet

The test assembly for #11773 (part of #4894): a small assembly from plain shapes whose parts list,
balloons and revision table are checked against values counted by hand. Parts lists and balloons
are planned on #3874, the revision table with the title block on #3875.

`assembly.json` builds it by commands (`Session::run_script`). A step's `as` names the occurrence
its reply returns, so a test can find the holders without counting steps.

## The assembly

| Part | Part number | Shape | Placed |
|---|---|---|---|
| Base plate | BP-100 | box 120 x 80 x 10 mm | once |
| Screw | SC-M6 | cylinder Ø6 x 20 mm | 4 on the plate, and 2 in each holder |
| Holder | HL-300 | assembly | twice, the second 40 mm along Y |
| Block | BL-200 | box 20 x 20 x 30 mm | once in each holder |

## Counted by hand

- **Parts only:** BL-200 2, BP-100 1, SC-M6 8 (4 + 2 x 2).
- **First level:** BP-100 1, HL-300 2, SC-M6 4.
- **All levels:** 1 BP-100 1; 2 HL-300 2; 2.1 BL-200 1 each, 2 in all; 2.2 SC-M6 2 each, 4 in all;
  3 SC-M6 4.
- **SC-M6 purchased:** the same counts, its rows say purchased.
- **HL-300 phantom:** no row of its own; the first level is BL-200 2, BP-100 1, SC-M6 8.
- **The second holder deleted:** parts only is BL-200 1, BP-100 1, SC-M6 6.

## What runs

In `crates/engine/src/cmd/bom_tests.rs`:

- `the_assembly_sheets_parts_list_matches_the_hand_counts`
- `the_assembly_sheets_item_numbers_hold_through_edits_undo_and_reopening`
- `a_purchased_screw_keeps_the_assembly_sheets_counts`
- `a_phantom_holder_puts_what_it_holds_on_the_assembly_sheets_first_level`

In `crates/engine/src/cmd/drawing_balloon_tests.rs`:

- `balloons_on_the_assembly_sheet_follow_the_model_and_its_revision_rows_stay`: four balloons on
  the top view at 1:2 (the plate, a screw, and the block through each holder); model edits made
  with the drawing closed; the second holder moved, its arrow follows; deleted, its balloon is
  `lost`, keeps its occurrence and record and is never moved to the first holder; undo and redo;
  save and reopen; revision rows 1 and 3 after row 2 is deleted, the next still 4, and the
  drawing's record unchanged by every model edit.
- `balloons_on_the_assembly_sheet_read_their_holders_row` (recipe 4): the first-level parts list on
  the sheet and a balloon through each holder to its block, both reading HL-300's row, item 2; the
  second holder deleted, its balloon is `lost` with `row_status: not_found`, stays on its own
  occurrence and isn't removed; undone, both read item 2 again; with HL-300 a phantom, both read
  BL-200's row.

In `crates/engine/src/cmd/drawing_table_tests.rs`:

- `the_assembly_sheet_keeps_its_parts_list_revisions_and_tag_through_the_model` (recipes 3, 5 and
  6): the assembly's drawing starts on A2; the first-level parts list placed at (330, 60) keeps
  items 1, 2 and 3 for BP-100 1, HL-300 2 and SC-M6 4, and with the second holder deleted only
  HL-300 drops to 1; a revision table, revisions 1 and 3 with 4 next, and a tag on 3 at
  (200, 120) showing 3; moving the second holder changes none of it, nor the drawing's record; the
  sheet exports the same SVG after save and reopen.
- **`drawing_view_tests.rs`, saved views that choose what they draw** (#11873): eight top views
  at 1:1, everything, one holder, both holders, no base plate, one block, a holder without its
  block, only the second holder, and the second holder hidden. Their sizes are worked out by hand
  in `cases.json` under `hand_views`. With the second holder moved 30 along Y or deleted, only the
  views that draw it change; the view that hides it never changes, and the view of only the second
  holder becomes a placeholder. Undo, and save and reopen, give the sizes as made, and the
  drawing's record never changes. A view's boundary comes from the display mesh, so the test
  allows up to 0.01 mm less where a round screw makes the edge, never more.

## Recipes on paper, NOT RUN yet

`cases.json` holds recipes 3 to 6 of #11773 as the full checks on the sheet. Every model edit in
them is made with the drawing closed, then the drawing is opened again. The commands they use are
all on `drawings` now, and each recipe names the native tests that run its engine part. What they
still wait for is the sheet drawing what they check.

- **W23-AS-03, the parts list on the sheet** (waits for the list drawn on paper, #3874): the first
  level numbered 1, 2, 3 with the script's descriptions; the second holder deleted, HL-300 reads 1
  and the numbers stay.
- **W23-AS-04, balloons** (waits for the balloons and the list drawn on paper, #3874): a balloon
  picks the row its placement is counted in, as `row_of` does, and shows that row's number. One
  balloon through each holder to its block, both on HL-300's row, item 2; the second holder
  deleted, its balloon is lost (pink) and its row not found, never moved to the first holder or
  removed; undone, both are ok again; with HL-300 a phantom, both read BL-200's row.
- **W23-AS-05, the revision table** (waits for the table drawn on the sheet, #3875): revisions 1,
  2, 3; 2 deleted leaves 1 and 3 with 4 next; a tag on 3; unchanged by a model edit and through
  save and reopen.
- **W23-AS-06, the sheet export** (waits for the three above drawn on paper and text on the sheet,
  #4890): the same SVG bytes before saving and after opening again; DXF and PDF as each writer
  lands.

`validate.py` checks the recipes without running SolidCraft: it works the hand counts out again
from `assembly.json`, checks every model edit is made with the drawing closed, that each recipe's
expected values follow from the script (quantities, descriptions, item numbers, revisions, where
the tag goes), that everything placed fits the A2 sheet an assembly's drawing starts on (#12000),
that the views' sizes in `hand_views` follow from the script, and that the native tests each
recipe names are in their files. It never reports a recipe as passed.

```
python3 tests/fixtures/w23-assembly-sheet/validate.py
```
