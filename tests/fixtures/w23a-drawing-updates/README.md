# W23a workshop plate: W13/W14 drawing update acceptance recipes

**Scope:** Follow-up to the merged [workshop plate oracles](../../oracle/W23a-workshop-acceptance.md)
from #4962; coordinated with [#4892](https://github.com/tangency-labs/solidcraft/issues/4892)
and [CamWight's invitation](https://github.com/tangency-labs/solidcraft/issues/4892#issuecomment-6471076927).

These are four written end-to-end acceptance recipes in `cases.json`, plus **fifteen runnable native Rust tests** in the existing `crates/engine/src/cmd/drawing_dimension_tests.rs` file. They are Engine-level subsets, **not** complete U2/UI tests or new W13/W14 production code. Source model oracles 84, 86 and 87 (with 85 as a width cross-check) remain the analytic reference.

## Executable engine tests and evidence

- **E2E-01:** Bore1 Ø10→Ø12 via `parameters.change`; Bore2 remains Ø10. Both dimensions keep their original `record.kind` reference.
- **E2E-02:** Deleting Bore1 via `timeline.delete` makes its dimension `lost`, preserving `last_good ≈ 10 mm`, without rebinding to Bore2.
- **E2E-03:** Two tests exercise resize and deletion through undo/redo, including the authored Drawing JSON, its reference records and custom dimension-text positions.
- **E2E-04:** One test saves and reopens the unchanged, resized and deleted workshop plate in fresh sessions; it checks the preserved reference records, authored text positions and historical `last_good` value. This uses the `reopened` helper.
- **R1 plate width** (`early_native_extensions` in `cases.json`): the plate's width as a dimension between its two side faces (#11871) reads 80, then 100 after `width` changes with the drawing closed, with the same reference, and 80 again after undo.
- **Suppressed Bore1** (`early_native_extensions`): suppressed, its dimension reads `suppressed` (#11823) and keeps its reference; on again it reads Ø10 `ok`; deleted it is `lost` with `last_good` 10 and never moves to Bore2.
- **Five dimensions** (`early_native_extensions`): Bore1 Ø, Bore2 Ø, the width, the length and Bore1's centre to Bore2's each follow only the parameter that drives them (`bore1_d` 12, `bore2_d` 8, `width` 100, `length` 120), one at a time and all four at once, and come back on undo and redo with the authored drawing unchanged.
- **R2 centres** (`early_native_extensions`): Bore1's centre to Bore2's, each centre named by its rim (#9249), reads 50 aligned, 40 across and 30 up the top view. A new Bore1 diameter moves none of them; Bore1 suppressed, all three read `suppressed`; deleted, all three are `lost` with their last values, which survive save and reopen.
- **The sheet** (`early_native_extensions`): through `drawing.scene`, `drawing.status` and `drawing.export`, the top view sits at 1:2 round (150, 180), 50 x 40, with the width's, the length's and the centres' arrowheads 40, 50 and 25 mm apart. Bore1 at Ø12 redraws only the view and Bore1's dimension; Bore1 deleted leaves out its two dimensions, listed `lost`; each undo draws and exports the sheet as it was; the length at 120 widens the view to 60; a reopened design draws the same sheet. The sheet draws no text yet.
- **Filleted corners** (`early_native_extensions`): fillets that take the plate's corners leave all five dimensions reading as before (R1 faces survive a fillet).
- **Repair** (`early_native_extensions`, W23-2 on #12170): Bore1's rim rounded 1 makes its dimension `lost` with leads; repaired to the circle where the round meets the bore, it reads Ø10 again as one undo step, keeps its id, view and text place, and holds through undo, redo, save and reopen. Bore1 deleted, its leads are the plate's only, and its dimension reads Bore2 only when repaired to Bore2's rim on purpose, and stays there when the delete is undone.
- **Renaming the plate** (listed in `tests/KNOWN_FAILING_TESTS` until #5308): the plate renamed `Base` should keep its five dimensions. Today a reference names its body, so all five read `lost`.
- **A new hole after an undo** (listed in `tests/KNOWN_FAILING_TESTS` until #5308): a dimension on an undone Bore3 must stay `lost` when Bore4 is made elsewhere. Today Bore4 takes Bore3's feature id and the dimension reads its Ø8 (gap 2 on #4892), so the test fails, and its line comes off when #5308 makes it pass.

All fifteen reuse CamWight's `examples/acceptance/drawing_m2.json` from #9821 (stacked on #9254), via `Session::run_script`. No duplicate model script or per-case Rust test file was added.

**Verified native integration CI (2 Nov 2026):** [Test #56](https://github.com/Benny1496/solidcraft/actions/runs/41430987707) ran the clean integration commit `4b0f664f7e53462aedfbee93448f3410b64804b5` on official `drawings@d5fd14135f0e220b45171010722a0064683c63f1`: **28/28** dimension (including all five W23a E2E tests), **55/55** view, **4/4** view schema, **4/4** export and **2/2** doc-view tests passed. **93 distinct targeted native Rust tests, zero failed or ignored**, plus Rust formatting and static contract validation. This is not the full engine or rendered/UI test suite.

Only the Engine/dimension-inspection portions are runnable today. **All four complete rendered-paper/UI E2E scenarios, U2 automatic refresh, dimension text on paper and publication semantics remain NOT RUN.** R1 and R2 are measured at Engine level (the plate-width and centres tests), and the sheet's lines and arrowheads through `drawing.scene` and `drawing.export`, without text. `cases.json` keeps the full acceptance rows at `NOT_RUN` and documents which native subsets are implemented.

Reapplied on official `drawings` with #9254 and #10713 already merged; neither one's production code is carried by this PR. CamWight's shared W23-1 plate (#9821) remains unmerged and is included **temporarily, byte-identically** so the tests run. Once #9821 merges, the matching fixture disappears from this PR's diff. No separate model implementation is claimed.

## Supported UI workspace sequence (CamWight, 3 Nov 2026)

The Drawing workspace **rejects model-edit commands while a drawing is open**.
For every full UI acceptance recipe, the supported user flow is:

1. Close/leave the Drawing workspace (the drawing remains in the design file).
2. Switch to the Model workspace; perform the specified model edit, deletion, undo or redo.
3. Return to/reopen the Drawing workspace for the **same design**.
4. Inspect the refreshed views, the actual dimension values and their reference
   statuses, and confirm authored text positions remain unchanged.
5. For E2E-03, return to the Model workspace for each subsequent undo/redo
   transition and check the drawing again after reopening it. For E2E-04,
   check unchanged baseline saves/reopens without inventing a model edit.

**Do not test editing the 3D model while the Drawing workspace is open.**
That would require later live co-editing support and is not part of the current
first-release user flow. Native engine session-level tests are a separate
contract: they do not demonstrate a rendered UI refresh. The four complete UI
scenarios remain **NOT RUN**.

CamWight confirmed this workflow in [the PR #8288 review](https://github.com/tangency-labs/solidcraft/pull/8288#issuecomment-6599897742).

## Intended user-facing behaviour

On a 100 × 80 × 20 mm workshop plate, `Bore1` at (20,20) and `Bore2`
at (60,50) are separate through-hole features with Ø10 each. Their centres
are exactly 50 mm apart. A future A3 landscape, 1:2 drawing must show
**model-space dimensions** Ø10 and 50 mm, not the projected Ø5 and 25 mm
paper-space sizes. Paper-space label positions are *authored* and should
survive all model refreshes. The `plate_width` 80 mm value is measured
**between opposite planar side faces (R1)**, not between arbitrary
sketch corners (R3). The `centres_1_2` 50 mm value is specifically the
distance **between Bore1 and Bore2 axes / circle centres (R2)**, not
between the W19d linear dimension command's corner picks.

| Acceptance recipe | Source transition | Required Drawing assertions | Pending boundary |
| --- | --- | --- | --- |
| `W23A-E2E-01` | `84 → 86`, `parameters.change(bore1_d, 12 mm)` | Existing Bore1 diameter measure changes from Ø10 to Ø12, *same authored dimension identity*; Bore2 unchanged; labels retain their paper positions | Early **dimension.inspect** subset: W19d/W20a; full drawing: R1/R2/R3/R5/U2 |
| `W23A-E2E-02` | `84 → 87`, `timeline.delete(Bore1)` | Bore1 dimension becomes **lost reference**, keeps last value Ø10 *as historical*, never silently targets Bore2; centre-to-centre dimension also becomes lost; Bore2 remains valid | Early **dimension.inspect** subset: W19d/W20a; full drawing: R1/R2/R3/R5/U2 |
| `W23A-E2E-03` | `84 → 86/87 → 84 → 86/87`, source-design undo/redo | Correct active/lost states return, no accidental drawing undo, no changes to authored dimension IDs or placements | R1/R2/R3/R5/U2 + W19d/W20a |
| `W23A-E2E-04` | Persist and reopen in each of `84`, `86`, `87` | Same actual drawing dimension identities, source references, last-known values, and label positions; no silent repair of lost reference | R1/R2/R3/R5/U2 + W19d/W20a + K4 storage integration |

**Important distinction:** The oracle's `StockSketch` construction-circle dimensions
are **not** technical drawing dimensions. Any names in `cases.json` such as
`diam_bore1` are **test-local symbolic handles**, never invented production
IDs, serialized fields or APIs. Once the real Drawing APIs exist, tests must
capture and compare IDs returned by them rather than hard-code these labels.

## Real inspection statuses, slice boundaries and label coordinates

The machine-readable `drawing.dimension.inspect` contract (W19d, #8958)
reports `status: ok | overridden | ambiguous | suppressed | lost | unmeasurable`.
Our `cases.json` expectations now use exactly `status: "ok"` and
`status: "lost"` instead of the former prose `resolved` and
`lost reference`. **"Lost reference" remains the intended user-facing
wording** for a broken reference, not an API status value.

All four cases contain separately marked
`early_dimension_inspect` subsets. Once the actual W19d/W20a interfaces
are available together on a verified source revision, a diameter's identity
and immediate model-based `inspect` value/status can be tested **without U2**.
The existing `view_after_deferred_update` expectation refers **only to a
rendered/saved view** awaiting its Update/publication, not to the dimension's
`inspect` result. The full cases (centre-to-centre, plate width, authored
label positions and refreshed views) still require their own R1/R2/R3/R5/U2
slices. The five Engine-level tests passed on the clean Test #56 integration revision; all four **complete** drawing/UI acceptance scenarios remain **NOT RUN**.

The `paper_xy_mm` positions in the fixture are **sheet coordinates**.
The API's dimension-text `text: {at}` uses millimetres **relative to the
view's centre**. Once the real view is placed, subtract the view-centre
paper coordinate from each authored sheet coordinate to bind `text.at`;
preserve the authored sheet placements in comparisons across edits and
reopens. Do not pass `paper_xy_mm` directly as `text.at`.

**Last known value is persisted:** #9764 is merged. An inspected dimension's `last_good` is saved through the real file format and survives a fresh session, including a `lost` reference. The E2E-04 native subset now exercises this explicitly.

## Real command boundary versus pending Drawing boundary

The model edit commands in `cases.json` match the existing engine's JSON
calling convention, checked against its native tests:
`parameters.change {"name":"bore1_d","expression":"12 mm"}`,
`timeline.delete {"feature":"Bore1"}`, `edit.undo {}` and
`edit.redo {}`.

The *oracle recipes* (declarative model fixtures) are not automatically
open engine sessions. Binding them into a real command replay first requires
a validated loader/fixture setup. The `source_edit` steps below are intended
real engine commands, **not evidence that a complete replay has run**.

Drawing operations such as placing a view, authoring a diameter dimension,
reading a reference's status, and publishing a refreshed scene are described
as expected semantics only. **Do not turn them into fake CLI commands or
pretend an assertion passed.** R5 defines stored drawing references; U2
defines the update/publication boundary; W19/W20 provide the relevant
dimension behaviours. W19d supplies `drawing.dimension.inspect` and W20a
supplies radial diameter/radius dimensions; W20a is discussed in #8959.
CamWight will link the relevant API slices on #4892 as they land.

## Isolation and invariants

1. **Start each recipe from oracle 84**, not from the previous recipe's
   modified design. Drawings are stored in the same design file as their model.
   Keep source model actions in the **Model workspace** and Drawing edits in
   the **Drawing workspace**; switch workspaces rather than trying to edit
   the model while the drawing is open.
2. **No closest-object fallback:** after deleting `Bore1`, even a new
   geometrically identical bore must not acquire its reference. The
   `Bore2` feature keeps the *original* identity, name and XY location.
3. **Out-of-date is not lost:** with deferred *view* updates, the existing
   rendered view is `out of date` until Update; after publishing, vanished
   references have API status `lost` (user-visible **lost reference**).
   `drawing.dimension.inspect` reads the current model immediately, not through
   the deferred view publication. Automatic update must not add an authored
   undo step.
4. Use only the supported **close drawing → edit model → reopen drawing**
   flow for UI acceptance. After every model change, undo or redo, confirm
   that the reopened drawing shows the correct current views, dimensions,
   status and preserved authored placements. Live co-editing while the
   Drawing workspace is open is **not currently supported** and is outside
   these recipes. The separate publication-order cases under #5430 must
   not be duplicated here.
5. Save/reopen tests must preserve *actual captured* authored dimension IDs
   and manually chosen label coordinates, not only numeric output.

## Validation and honest status

- The existing model oracle fixtures already have their own analytic
  `expected.json` files. Run `python3 validate.py` for a
  **static consistency check only** (case links, parameter semantics and
  expected analytic geometry from those files).
- `cargo xtask oracle` validates the 3D models, **not** automatic drawing
  updates. No new heavy CI/Windows batch is requested with this draft.
- The full four UI/U2 Drawing acceptance rows remain **NOT RUN**. Their native
  dimension-inspection subsets are implemented and were proven on the prior PR
  revision in Test #56 (93/93 targeted native tests passed). A full
  3D-to-2D UI refresh still depends on U2.
