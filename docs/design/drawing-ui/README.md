# Drawing workspace: UI design

The pictures and rules for the Drawing workspace's user interface. They say where each piece
lives, what it looks like and how it behaves, so that anyone building a slice of the drawing UI
produces the same product. The commands behind each piece already exist or are being built on
`drawings`; see [#4894](https://github.com/tangency-labs/solidcraft/issues/4894) and
[#860](https://github.com/tangency-labs/solidcraft/issues/860). The pictures are mockups: they
change nothing in the app.

[`decisions.md`](decisions.md) is the log of what is decided, planned and still open. Read it
with this page, and cite the boards and decision numbers you build from in your pull request.

## How to use this

- **Build to the structure, not the pixels.** Placement, which kit component a piece uses, the
  state words and the ribbon contents are binding. Sizes and spacing come from the UI kit and
  theme tokens (`docs/ui-kit.md`), and the icons in the mockups are stand-ins where noted.
- **A deviation is allowed, with a reason.** If the design cannot be built as drawn, say what
  differs and why in the pull request. The design is then changed and logged, or the code is.
- **Light theme only for now.**

| Binding | Flexible |
|---|---|
| Where each piece lives (workspace card, Sheets section, status pill, right-hand panels, navigation bar) | Exact sizes, spacing and colours: use `theme::Tokens` and the kit |
| The kit component for each piece, and the panel structure (header, body, footer) | Stand-in icons, until the real ones exist |
| Ribbon tabs, sections, promoted commands | Wording of helper text, within the UI text rules |
| State words and their meanings | Animation and hover detail |
| Command ids and what a click calls | Anything the log marks as open |

## The layout

- **Workspace card** at the top left, the shared compact card, the same width as in other
  workspaces. The ribbon sits beside it with four tabs: Layout, Annotate, Parts & Tables, Review.
- **Left column**: a **Sheets** section above the Browser tree. It shows each sheet as a
  thumbnail, two across at the default width, with add, settings and collapse in its header. There
  is no separate sheet strip.
- **The sheet** is the viewport: a neutral stage with the paper on it, pan and zoom to the
  cursor, no compass and no bottom history strip. The navigation bar sits at the bottom centre.
- **Status pill** at the top centre of the free sheet area: "N need attention" or "All current".
  Its card lists the findings with Repair, and offers Check drawing and Update drawing.
- **Right-hand panels**: one at a time, built from the kit's `Panel`. Editors end with Cancel,
  Apply and OK. Placing tools end with Finish. Sheet Settings applies at once.
- **Each open drawing is its own tab** next to its design (a preference, on by default).
- **Selection card** above a selected view or dimension, with quick actions.

## The ribbon

One to four promoted buttons per section; the section label opens the full list. Commands that
do not exist yet stay on the ribbon, faded, with "(planned)" in their tooltip. They lose the fade
as they arrive.

| Tab | Section | Promoted | In the section list |
|---|---|---|---|
| Layout | Views | Base View, Section, Detail, Break | Auxiliary, Pictorial, Draft, Flat Pattern (promoted for sheet-metal parts) |
| Layout | Arrange | Align Views | Move to sheet, Copy to sheet |
| Layout | Insert | Image, Sheet Sketch, Hatch | |
| Annotate | Measure | Dimension, Model Dimensions, Hole Callout | Chain, Suggest |
| Annotate | Notes | Note, Leader Note | Chamfer note |
| Annotate | Symbols | Centre Mark, GD&T, Surface Texture, Weld | |
| Annotate | Arrange | Tidy, Align, Space | |
| Parts & Tables | Assembly | Parts List, Balloon, Auto Balloon | Bill of Materials, Renumber |
| Parts & Tables | Tables | Revision Table, Edit Table | |
| Review | Changes | Update, Check Drawing, Replace Model | |
| Review | Revision | Add Revision, Issue Revision, Revision Tag | |
| Review | Output | Export, Print | |

Templates and sheet commands live in the Sheets section menu and Drawing Settings. Drawing
Status is the status pill. Automatic centre marks is a Drawing Settings option.

## The paper

The sheet follows the standards so a drawing can be used as one: an ISO 5457 frame with a 20 mm
binding margin and the reference grid, an ISO 7200 title block no wider than 180 mm, an ISO 7573
parts list right-aligned above the title block, ISO 128 line weights and line types, a
third-angle projection symbol and general notes at the lower left. The paper has a very slight
rounded corner on screen only; an export is square-cornered. The standards references have not
yet been checked by the drawings team (see the log).

## Boards

| Board | What it shows |
|---|---|
| [First drawing](first-drawing.png) | Entering with no drawing: standard views, blank sheet or template |
| [Layout](layout.png) | Layout tab, nothing selected: Sheets section and status pill |
| [View selected](view-selected.png) | Selection card, drag-out handles, Edit View panel, model preview |
| [First Base View](base-view-first.png) | Choosing the front, Base View panel, preview on the sheet |
| [Placing views](placing-views.png) | Base View stays on; the cursor's side picks the next view |
| [Moving views](moving-views.png) | Guides, snapping, readout and the Undo toast |
| [Sheets](sheets.png) | The sheet menu and Sheet Settings |
| [Add sheet](add-sheet.png) | The Add sheet dialog |
| [Picking and moving](picking.png) | Hover, edge band, pick modes, overlap, box select, locked items |
| [Annotate](annotate.png) | Dimension panel, edge hover preview, suggestions, Annotate ribbon |
| [Parts & Tables](parts-tables.png) | Parts List editor, balloons, column handles |
| [Review and Export](review-export.png) | Export panel with the paper choice; the status pill with two items waiting |
| [Status card](status-card.png) | The pill's card with three findings, marked on the sheet |
| [Ready to issue](ready-to-issue.png) | Check drawing passed |
| [Make a drawing](make-drawing.png) | Model right-click and File menu entries |
| [Views by part type](views-by-part-type.png) | The Views section for an ordinary and a sheet-metal part |
| [Sheet colour](sheet-colour.png) | The navigation-bar menu on a blueprint sheet |
| [Paper presets](paper-presets.png) | White, Blueprint and Prussian Blue (Historical) |

## Not included

- Dark theme.
- Windows and tools without a board yet: Drawing Settings and Properties, Create Drawing options,
  Replace Model, the Check Drawing coverage list, the "not drawn" status and refusal messages,
  Section and assembly views, baseline and ordinate sets, copy and paste between drawings, automatic
  balloons, revision clouds, extra tables, fields in notes, the symbols library, thread drawing and
  chamfer notes. They are designed when their slice is handed out.
- Real icons for Break, Hatch, Sheet Sketch, GD&T, Surface Texture, Weld, Replace Model, Issue
  Revision and Revision Tag. The mockups use the nearest drawing icon, faded.
