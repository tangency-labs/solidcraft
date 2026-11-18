# Drawing workspace UI implementation roadmap

The first native Drawing UI provides the agreed workspace layout as an integration
target for drawing contributors. It reuses the application shell and existing
source-design services. It does not implement drawing documents, sheet rendering,
view generation, annotations, tables, reference updates or paper exports.

## Baseline and ownership

The implementation branch `codex/drawing-workspace-ui` starts at the freshly fetched
`drawings` commit `3b982ca371ceded6a8f4695bc46e4b3618de0ac9`. The Drawing workspace
claim remains [PR #4895](https://github.com/tangency-labs/solidcraft/pull/4895), based
on [#4894](https://github.com/tangency-labs/solidcraft/issues/4894). This UI-only slice
does not replace its end-to-end drawing acceptance milestone or create another claim.

The baseline has no Drawing file/session or paper renderer. The storage, paper,
view-definition, reference and annotation contracts remain separate work. Do not
import the older experimental preview's embedded design metadata, private rendering
or provisional drawing records to fill these gaps.

## Planned commits

| Commit | Change | Acceptance |
| --- | --- | --- |
| 1 | Record the implementation scope, reuse boundaries and commit sequence | The scaffold's available services and exclusions are explicit. |
| 2 | Add original Drawing icon drafts | New artwork remains Open in the existing review tool; runtime icons remain placeholders until approval. Exact-meaning existing icons remain reusable. |
| 3 | Integrate planned tool metadata, deliberate placeholders, the native Drawing workspace and shared ribbon, empty canvas, source Browser and docked settings | Four task tabs use existing pins, section lists, command search and customization. Design camera, history and selection remain intact when switching workspaces. No drawing object or design metadata is fabricated. Acceptance coverage checks compact/floating layouts, docking, unavailable controls, source services, state isolation, wasm and native screenshots. |

Each feature arc must build before its commit. Run the repository quality gates
before committing; use focused UI/engine tests during iteration. Changes stay on
the implementation branch and are supplied to the existing workspace claim for
integration rather than opening a competing claim.

## Shared implementation boundaries

Use `workspace::Section`, the existing toolbar/customization pipeline and command
registry. Drawing has Layout, Annotate, Parts & Tables, and Review task tabs.
Promoted icons and unfolded section headers reach the same command identity;
pinning changes toolbar preferences rather than running an operation.

The canvas stays empty until a real paper renderer is available. It must not show
hand-painted paper, sample CAD geometry or pretend hit-testing. Any canvas camera
uses the shared navigation handler with orbit disabled. Browser content describes
the linked source and available drawing state, rather than presenting the Design
feature tree as drawing objects.

Sheet navigation uses a content-sized strip beside the Browser. One sheet requires
one short row, with adjacent add, management and settings controls. A history-style
grip and highlighted drop targets support top, bottom and beneath-Browser docking;
the latter is a vertical list. Full paper details are shown on demand. With no
Drawing document, sheet creation, duplication and other document changes remain
unavailable rather than creating local substitute sheet records.

Sheet Settings follows the Assistant's left/right dock and bounded resize pattern.
It shows current state when a provider exists; agreed defaults can be inspected
read-only before that. Settings does not use an operation Apply/Cancel workflow.
Known operation-option previews may open for unavailable tools, but their execution
stays disabled. Do not invent uncertain parameter forms or enabled Apply behavior.

Use shared semantic containers and theme tokens for all panels, menus and overlays.
Register their occupied rectangles with the shared shell where needed so pointer
events cannot pass through floating UI. Keep layout, docking and widths in UI
preferences, outside source-design undo. No new design-file format is needed.

## Feature homes and later provider work

| Roadmap capability | UI home | Provider boundary |
| --- | --- | --- |
| K1–K12 / W11–W15: drawing records, links, relink, save, tabs, browser, per-file undo | Document/file shell; source status in Browser; sheet strip for sheets only | Add real Drawing documents before enabling file or sheet edits. Source design metadata is separate. |
| S1–S3: sheet size, borders, title blocks and templates | Sheet Settings, including resource selection | Read and edit current sheet configuration through document commands; reuse the panel rather than adding separate ribbon tools. |
| W17a / W17b–W18: view definitions, base/projected/section/detail views | Layout → Views; shared operation panel | Add validated view records and the paper scene through their own packages. |
| W09 / W19–W20: number formats, dimensions, notes and symbols | Defaults in Sheet Settings; creation in Annotate | Replace readonly previews only when the corresponding engine command and reference handling exist. |
| P1–P3b / M1–M3 / W02–W08: material, mass, property keys, units and geometry foundations | Existing source properties and source BOM; formatting in Sheet Settings | These services supply evaluated values; UI code must not recalculate physical properties or copy model records. |
| P1–P7 / W24–W26 / W31: BOM, parts lists, balloons and revision tables | Parts & Tables; revision table also reachable from Review | Source BOM remains a source service; a paper table needs its own document record and rendering. |
| W13–W14: source references and updates | Browser source status; Review → Changes | Provide actual update/repair results before enabling the commands. |
| W10 / W21–W22: paper scene and DXF/SVG/PNG/PDF output | Canvas provider; Review → Output | Share scene evaluation with exports. Current Design-view DXF is a different operation. |
| W16 / W23: workspace and first complete drawing | Shared shell and ribbon in this slice; acceptance drawing later | This scaffold provides the visual anchor, not the full milestone. |

Provider increments should be separate commits/PRs for their owning roadmap task.
Replace a planned command in place, preserving its dotted identity and shared
ribbon entry. A configured operation must execute the engine command; a programmatic
call must never depend on opening its UI. Introduce no parallel UI-only CAD state.

## Existing services

| Control | Existing implementation | Scope |
| --- | --- | --- |
| Bill of materials | `assembly.bom` and the existing BOM window | Reads actual source assembly quantities/properties and existing CSV/JSON output; does not place a paper parts list. |
| Inspect source flat pattern | `sheet.flat_pattern` and its existing inspector | Uses the source sheet-metal body and existing manufacturing DXF/bend-table actions; does not place a view. |
| Source properties | Existing design-properties UI and metadata commands | Edits the source design only; never substitutes for drawing title fields. |

Drawing Export/Print, view placement, dimensions, balloons, revision records,
source repair and drawing Save/Open remain unavailable until their actual providers
exist. Current-view DXF is not drawing-sheet export.

## Unavailable controls and icons

Unavailable tools are faded and explain their missing capability. Clicking a faded
tool is permitted only for an explicit known-options allowlist. Programmatic calls
still return an unsupported result and never open a dialog or mutate a document.
Controls with uncertain configuration remain noninteractive.

Reuse an existing icon only when its meaning is identical: adding, settings,
management, navigation, actual source BOM, or the exact existing source inspector.
Drawing views, drafting annotations and paper tables do not borrow unrelated
modeling or Sketch icons. Their runtime assignments use the unimplemented
placeholder while original first-version artwork is prepared for human review.
Unused drafts must appear Open in the icon review tool; agents never approve their
own artwork or change protected acceptance hashes.

## Verification

UI scenarios cover entering/leaving Drawing, all four task tabs, shared ribbon
unfolding/pinning, faded known-options versus disabled uncertain tools, compact
sheet navigation, all three strip docks, settings side/width changes, and both
application layouts. Unsupported operations must leave the source document and its
history unchanged. Existing Design navigation and source service entry points must
continue to work.

Run `cargo xtask ci`, including assets, icons, layers, shared-container checks,
Clippy, tests and wasm. Build and run the native app and capture a small public-safe
workspace screenshot. Keep generated screenshots and recordings outside tracked
source. Record any baseline failures separately from failures introduced here.
