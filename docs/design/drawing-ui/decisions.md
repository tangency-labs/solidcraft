# Drawing workspace UI: decision log

What is decided, planned or open for the Drawing workspace's interface, with the boards that
show it (see [README](README.md)). Change a row here when a decision changes; a pull request that
departs from a decided row says so.

**Status:** *decided* builds as drawn. *planned* is shown on purpose and arrives later.
*check* is decided in the design but needs confirming before it is built.

## Layout and chrome

| # | Decision | Status | Boards |
|---|---|---|---|
| D01 | Workspace card at the top left, the shared compact card, same width as other workspaces | decided | layout |
| D02 | Four ribbon tabs: Layout, Annotate, Parts & Tables, Review | decided | layout, annotate, parts-tables, review-export |
| D03 | Each ribbon section promotes one to four commands; its label lists all of them | decided | README table |
| D04 | Commands that do not exist yet stay on the ribbon, faded, with "(planned)" in the tooltip | decided | all with a ribbon |
| D05 | Left column: a Sheets section (thumbnails, two across by default) above the Browser tree; no floating sheet strip | decided | layout, sheets |
| D06 | The Sheets section can be set to always show, show only with several sheets, or be off; the default is not chosen | check | layout |
| D07 | Status pill at the top centre of the free sheet area. "N need attention" or "All current"; its card lists findings with Repair, then Check drawing and Update drawing; when clean it offers Export | decided | layout, status-card, ready-to-issue, annotate |
| D08 | One tool or editor panel at a time on the right, built from the kit's `Panel`. Editors end with Cancel, Apply, OK; placing tools end with Finish; Sheet Settings applies at once | decided | view-selected, annotate, parts-tables, review-export, add-sheet |
| D09 | No compass and no bottom history strip in the first release. The navigation bar at the bottom centre: Pan, Zoom, zoom value (menu: Fit sheet, Actual size), Fit sheet, Pick, sheet colour, view settings | decided | layout |
| D10 | Each open drawing is its own tab next to its design (a preference, on by default). A Back to model button appears only with it off, and is not drawn | decided | all with an app bar |
| D11 | Light theme only for now | decided | all |

## Views and the sheet

| # | Decision | Status | Boards |
|---|---|---|---|
| D12 | Base View stays on after the first view and places projected views. The cursor's side of the placed view picks the view (the drawing's first- or third-angle setting decides); a diagonal gives a pictorial view; Esc or Finish ends. The preview asks `drawing.view.projection_for` ([#13044](https://github.com/tangency-labs/solidcraft/issues/13044)) | decided | placing-views |
| D13 | No separate Projected View button. A selected view offers Add projected view | decided | view-selected |
| D14 | "Standard views" places front, top and right in one go | check | base-view-first, placing-views |
| D15 | A selection card sits above a selected view or dimension (Edit, Add projected view, Section, Delete); a selected view has a drag handle on three sides | decided | view-selected |
| D16 | Moving a view shows alignment and spacing guides, the distance moved and where it snaps; Alt moves freely; an aligned view moves only along its line; moves, deletes and similar end with an Undo toast. The snap values are a first set | decided | moving-views, picking |
| D17 | Picking: a view's boundary appears near its edge and a view is dragged by that band only; inside, a click picks by the Pick mode; the smallest boundary wins, then the topmost; box select starts on empty paper only; the sheet border, drawing border and title block are locked | decided | picking |
| D18 | Face picking on the 3D model in the first Base View panel is planned; the orientation chips (Front, Top, Right, Left, Bottom, Back, Iso) work first | planned | base-view-first |
| D19 | Entry points: an empty drawing offers Standard views, Blank sheet and From a template; "Make drawing" is in the model's right-click menu and the File menu | decided | first-drawing, make-drawing |
| D20 | The Views section promotes Flat Pattern View only for a sheet-metal part | decided | views-by-part-type |

## Annotation, tables and output

| # | Decision | Status | Boards |
|---|---|---|---|
| D21 | One Dimension tool; hovering an edge previews its value; Model Dimensions, Suggest (dashed ghosts that Enter accepts and Esc dismisses) and Tidy are commands. Automatic centre marks is a Drawing Settings option | decided | annotate |
| D22 | The title block is edited on the sheet (click a cell and type); a small popover shows which property fills it and resets it | decided | not yet drawn |
| D23 | Edit View: Visual style (renamed from Display style), Reference style and a Reference shown slider (0 to 100 %), Link view and Aligned to parent | decided | view-selected |
| D24 | Export window: Format, Sheets, Paper (White, Blueprint, Prussian Blue), Output. A blueprint theme starts the paper on the matching blue | decided | review-export |
| D25 | Sheet colour: follows the theme, or White, Blueprint or Prussian Blue (Historical) from a navigation-bar menu. Screen only; the export paper is chosen in Export | decided | sheet-colour, paper-presets |
| D26 | State words come with a shape, never colour alone: Ok ✓, Overridden ⓘ, Ambiguous ⚠, Lost ✕, Needs update ⟳. User text says "sheet", never "page" | decided | annotate, status-card |

## The paper

| # | Decision | Status | Boards |
|---|---|---|---|
| D27 | ISO 5457 frame with a 20 mm binding margin and the reference grid; ISO 7200 title block no wider than 180 mm; ISO 7573 parts list right-aligned above the title block; ISO 128 line weights; general notes at the lower left | check | layout, paper-presets |
| D28 | The third-angle projection symbol shows the frustum on the left and its circles on the right | check | layout |
| D29 | A very slight rounded corner on the paper, on screen only; an export is square-cornered | decided | layout |

## Shortcuts

| # | Decision | Status |
|---|---|---|
| D30 | First set: D Dimension, B Base View, N Note, L Balloon, C Centre Mark, Z zoom to selection, double middle-click fits the sheet, S opens command search. Check against existing app shortcuts before building | check |

## Open or still to design

- Which of D06, D14, D27, D28 and D30 hold up when checked (the drawings team, and the existing shortcut map).
- Whether the view window can pick a face of the part (D18).
- Drawing Settings and Properties, Create Drawing options, Replace Model, the Check Drawing coverage list, the "not drawn" status and refusal messages, Section views, assembly views, and the later slices listed in the README.
- Real icons for the commands that have none.
