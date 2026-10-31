# Drawing delivery and handoffs

Use the checklist in the body of [RFC #860][rfc] for live progress. This file is
a dated dependency map; the [final W00–W47 plan][plan] remains the delivery plan.

Coordination snapshot: **2026-10-24**, `tangency-labs/solidcraft`, branch
[`drawings` at `4c63a88fc3035ac91b4b9dc847fffa0d2d5ec155`][base].
This W00b child ([#6157][w00b], [PR #6163][w00b-pr]) reconciles the
[W00–W47 plan][plan] with later decisions and public claims. It is a dated map,
not a live dashboard, a new assignment or a release announcement.

Read current [AGENTS](../AGENTS.md) and [CONTRIBUTING](../CONTRIBUTING.md), then
recheck the linked discussions and claims before taking a child issue. The
[W01a ledger](drawing-foundations.md) and [geometry contract](drawing-geometry.md)
retain their original structural-validation scope. This map covers every package;
**atomic traceability of every research-study requirement is still unfinished W00
work**. Research is requirements evidence, not copied implementation or standards
certification. The [updated study, attachment 33742361][study-current], linked from
[its author's discussion][study], adds the §18.10 BOM and sheet-source decisions.
Where they differ, §18.10 supersedes the earlier study choices; later owner
agreements take precedence in turn, as recorded below.

## 27 October 2026 — current decisions overlay

This update reconciles only the changed contracts and delivered slices. The
[live RFC checklist](https://github.com/tangency-labs/solidcraft/issues/860)
remains authoritative for the remaining packages. All the 24–25 October
evidence below is dated; a draft is not an implemented product feature.

- **G3 has changed:** [the maintainer's embedded storage decision][g3]
  replaces separate `.scdwg` files. Drawings now belong inside the parent
  design's `.solidcraft` container, as separate `drawings/<id>.json` entries,
  or inline in plain JSON. Model and drawing have distinct undo histories.
  K3 [#7098](https://github.com/tangency-labs/solidcraft/pull/7098) merged;
  K3b [#7490](https://github.com/tangency-labs/solidcraft/pull/7490)
  is a draft; the K4-prime in-file session follows in [#3891][storage-pr].
- **W17a** [#4817][view-pr] merged into `drawings` on 27 October
  as `de7bf4fc`. This delivers frame and scale data, not saved views.
- **W06a** [#2457][hlr-pr] merged into `drawings` on 27 October
  as `ebafe074`. The parallel-work and work-budget [#3013][hlr-parallel]
  follow-up remains open; HLR-to-paper is not complete.
- **X1** [#6184][dxf-claim] remains draft at `3fb2eef9`.
  Six of its seven prior sheet regressions pass, and the 53 original DXF
  byte pins are unchanged. The remaining curved-to-object trim case requires
  source-proven circular identity; there is no passing full CI or wasm
  evidence for this revised source.

## What the evidence means

| Status | Meaning in this snapshot |
| --- | --- |
| Integrated | The PR is merged and its merge commit is an ancestor of the pinned `drawings` commit. Only the named slice is available. |
| Main only | Merged into `main`, but its merge commit is not an ancestor of the pinned `drawings` commit. Integration is still a dependency. |
| Open code | The PR has a nonempty diff. Its implementation, validation and review remain attributable to that PR revision; this map does not certify them. |
| Claim | An open draft has zero changed files against its base at the check. This reserves work; it supplies no implementation. A parent may have separately merged children. |
| Plan | A requirement or proposed slice remains. No separate implementation claim was identified in this review; check related parent ownership before claiming. |

The initial review checked the complete open inventory (686 PRs and 2,502 issues), the
relevant PR metadata and the pinned Git tree/ancestry. It ran no compilation,
tests, benchmarks or application scenarios. An open PR's synthetic merge SHA is
not evidence that it landed. A claim's empty diff is not evidence that its owner
has done no local work.

Same-day follow-up: [K1 is now open for review][k1-pr] at
`1a2bd57f802c5f26122424353ce78306df7878c8`, targeting the pinned `drawings`
base, with eight changed files. It introduces shared format families and release
folding, not Drawing records or a reader for those records. It also deliberately
checks unknown pending changes before validating design content. It is not
merged; its author's validation report is not an independently reproduced gate.
The accepted accuracy, shading and editor contracts below were also refreshed
from their owners' replies. Those agreements do not establish implemented APIs.

**2026-10-25 follow-up:** the updates below use remote
[`main` at `2741cd9a8b0b84b7d33001d8f9473198b5d53f73`][main-oct25] and
[`drawings` at `672ef13eb8b8e2cb0ddf9c795e337b73d043eb07`][drawings-oct25].
Other entries retain the 2026-10-24 snapshot; this is not a new full inventory.

- [#6130][sheet-dxf-options] merged on 2026-10-25 at 02:10:07 UTC as
  `9d226b35358920a8b80ae4e0c43470d0f54edf97`. That commit is an ancestor of
  the `main` pin above, but not the `drawings` pin: **main only** under this map's
  ancestry definition. [#5709][dxf-text-refusal] remains open at
  `da1baa044c1ac12081d2b1ffddf67a3284fa66fd`; the overlapping DXF chain still
  waits for that prerequisite and output byte pins. [Composition review][dxf-composition]
  against the `main` pin identified two needed fallible-writer adaptations:
  `?` in the new sheet DXF `one_layer` branch and `.unwrap()` in the degree-nine
  round-trip test. This is source/textual-composition evidence; no Cargo run or
  executed failure of the combined code was reported.
- K1 [#6196][k1-pr] remains open at the same `1a2bd57f` revision recorded above.
- W38.0 [#6336][sketch-store-pr] remains open into `main`, now at
  `c283ecd1169e75103f9daa8d5c3944fcc9e59e0d`. The [author's update][sketch-store-fix]
  reports broader active-sketch value scanning, planted let-else/match-arm faults
  and five more commands using `target`, including changed bad/missing-sketch
  refusals. Its reported 5,147 passing tests are author evidence, not independent
  validation or approval by this map. [Subsequent source review][sketch-store-selection]
  resolved the original bypass finding but raised a P2: an absent numeric sketch
  ID can fall back to a matching name in four newly centralized callers. The
  exact-ID regression remains open; the example was not executed. `sketch.inspect`
  already had that fallback and is not counted as a new regression.
- W07 V1 has the zero-file draft claim [#6482][mesh-lines-pr] into `main`, at
  `f8374713b6078deee425d521077dd27967944f70`. Its [revised V1/V3 boundary][mesh-lines-split]
  below is an agreed contract, not a delivered projection or identity API.
- W17b S1 has the zero-file draft claim [#6483][shade-s1-pr] into `main`, at
  `c3b7d77d29d8599fc07ab799b726546888c1b097`. It reserves the shared tile-renderer
  refactor; it supplies no API yet. S1/S3 acceptance still needs actual providers.

**Text-metrics follow-up (2026-10-25):** [#4815][text-pr] merged at 06:13:31 UTC
as `d557c09c3b5a059e553a5d9f1fb2682e52ecbaae`. The merge is an ancestor of
[`main` at `ac53fc1e6fab85a50bdc410178cc75133183393f`][text-main-check], but not
of [`drawings` at `672ef13eb8b8e2cb0ddf9c795e337b73d043eb07`][text-drawings-check]:
**main only**. This separate ancestry check does not refresh other rows. The
library's `TextMetrics` contract is delivered on `main`; Drawing layout, fonts
and their qualification remain separate work.

### Integrated slices and dependencies outside the branch

| Slice | Verified delivery | Limit / next consumer |
| --- | --- | --- |
| W01a, `0xlillith` | [#2085][w01-pr], merge `7c9f8a58`; `geom::section` | Neutral records and structural checks, not body sections, persistent identity or certified material regions. |
| W09a, `0xlillith` | [#2086][w09-pr], merge `6422ef84` | Shared length display formatting, not the complete Drawing number/tolerance/profile system. |
| W18 sheet geometry, CamWight | [#5108][sheets-pr] (`f0cc73eb`) and [#5554][marks-pr] (`4c63a88f`); `geom::drawing_sheet` | Sizes, borders and marks, not saved sheets, title blocks or a Drawing workspace. |
| W10 A1, CamWight | [#5513][paper-pr], merge `5e23f58f`; `geom::paper` | Strokes/fills grouped by object ID/kind with bounded validation. Prepared traversal, resolved clipping, pictures, measured text and writers remain later slices. |
| W23a, Benny1496 | [#4962][fixture-pr], merge `01a542d6`; [fixture contract](../tests/oracle/W23a-workshop-acceptance.md) | Original model recipes and analytic expected states, not executed associative Drawing acceptance. |
| DXF degree fix, CamWight | [#5281][degree-pr], merge `e8a6a5d3`, **main only** | Needed before the shared DXF refactors; it is absent from this pinned branch. |

## Decisions that supersede the original plan

- **Integration branch:** Drawing feature PRs target `drawings`; shared foundations
  follow the agreed owner/integration target. The maintainer shepherd handles
  integration from `main`; do not open competing synchronization PRs.
  See the [branch decision][branch] and [shepherd clarification][shepherd]. This
  is the Drawing-specific update to the plan's original `main` instructions.
- **Kernel boundary:** the [current kernel pause][kernel-pause] and
  [CONTRIBUTING](../CONTRIBUTING.md#kernel-work-is-paused) still apply. The
  [kernel/SolidCore tracking PR #4012][kernel-tracking] is separate work, not
  proof that a portable extraction or Drawing HLR API has landed. Coordinate
  W02–W06 with their current authors; do not start another truck-specific backend.
- **G3 — drawings are embedded:** the [new in-design contract][g3]
  supersedes the earlier linked Drawing-file option. Each authored drawing
  lives in a separate entry in the design's container, or inline in plain
  JSON. Undo and revision history are per drawing, without separate files.
  The [older JSON amendment][g3-json] is still relevant to stable and
  lossless save/reopen, but not to the old file split.
- **Storage and sessions:** [#3890][storage] owns the drawing record,
  versioning, in-file commands and recovery. K3 merged in
  [#7098](https://github.com/tangency-labs/solidcraft/pull/7098);
  K3b [#7490](https://github.com/tangency-labs/solidcraft/pull/7490)
  updates persistence, and K4-prime follows in [#3891][storage-pr].
  A drawing's source model is the same design file, not a relative external
  link. Paper scenes and render results remain disposable caches. These
  changes are staged; the complete workflow is not delivered here.
- **Reference work can start earlier:** [#4892's revised slices][refs-plan]
  first handle planar faces, curved faces, edges/vertices and source keys without
  saving anything. Publication, automatic/deferred updates and background hosts
  still depend on the later storage/evaluation contracts. Do not substitute nearest
  geometry or display indices for full occurrence paths, persistent names or intent.
  Accuracy, confidence and freshness remain separate.
- **BOM and sheet sources (2026-10-25 reconciliation):** [study §18.10][study-current]
  makes the design's BOM the authority for item numbers, saved per BOM view and
  shared by parts lists and balloons. Part/assembly roles derive from component
  contents, including child placements; they are not separate Drawing kinds.
  Components have normal, phantom, reference, purchased or inseparable BOM types,
  with per-occurrence overrides. Each sheet has an explicit source component,
  initially seeded by its first placed view, then persisted and editable; deleting
  that view does not retarget the sheet. Title fields and template tables use that
  source, with configurable field sources and marked overrides.
- **Later BOM agreements override older study wording:** [#3874's plan][bom-plan]
  keeps purchased parts inside an inseparable component in the parent's parts-only
  list. Its [later update][bom-update] saves item numbers in the same undo step as
  the change adding a row, keyed by component and BOM type so part-number edits
  retain them. Any component can carry a typed quantity expression, with shared
  mass/volume units; quantities are not restricted to integer counts. Parts lists
  consume BOM numbers and keep local overrides marked until explicitly saved to
  the BOM. These are agreed contracts, not a claim that W24–W26 are complete.
- **W04 is JSON-only in the current PR:** [#2068][section-pr] at
  `d2a9e4852cdfa9afd769083d7408e2f69683a14a` returns `inspect.section_curves`
  JSON, with no SVG/DXF export. The [author's rework][section-json] separates
  shared crossings into [#6272][section-crossings], which must land first, and
  reports an error beyond `MAX_PIECES` instead of a partial cut. [Cam's agreement][section-adapter]
  puts the neutral `SectionGeometry` adapter after both PRs as a W07 follow-up.
  Physical SVG/DXF output remains a later requirement; it is not delivered by
  this command PR.
- **SVG first:** [the agreed export order][svg-first] puts SVG and the shared
  paper traversal before the paused Drawing DXF adapter. The
  [canvas/export boundary][scene-boundary] places shared traversal and resolved
  clipping in `geom`, while display tessellation stays in `ui-egui`. The later
  [`Scene::prepare`/picture plan][writers-plan] is not A1's shipped API.
  [PDF dependencies have been selected][pdf-choice]; embedding, fonts, shaping,
  glyph coverage, bounds and writer qualification still need their own evidence.
- **W07 now names `drawing.view.snapshot`:** the [current command plan][snapshot]
  is read-only, transient and non-undoable. It uses exact W17a view bases,
  fitted scale and centring, explicit occurrence subtrees/hiding and projected
  measurements. It does not save associative annotations. First/third angle
  belongs to the Drawing and projected-view placement, not a single snapshot.
- **W07 V1/V3 split (2026-10-25):** [V1][mesh-lines-split] moves the shared
  projection into `engine/src/mesh_lines/` on `main`: `prepare` takes caller-selected
  bodies, errors name the calling command, and a `View` supplies origin,
  right/up/back basis and optional perspective distance. View DXF constructs that
  value from its camera and must retain byte-identical output. V3 on `drawings`
  adds each line's placement and edge/silhouette-face source, splits stretches
  when their owner changes, and retains triangle face IDs. The first-visible-source
  ownership rule must be tested against W07; V1 does not supply that identity API.
- **W17a frame and error contracts:** [normal points toward the viewer][view-map],
  HLR direction is its negative, and the shared map converts model to paper mm
  while scaling accuracy deviations. [Exact named bases and ratio scale][view-bases]
  avoid camera pitch clamps. Requested tolerance is not automatically achieved
  accuracy. The [accepted W07 accuracy contract][accuracy-contract] reports the
  achieved class per piece in paper mm, preserving `Sampled(0)`. Planned optional
  `max_error_mm` is a check, never a producer input or a clamped value: analytic
  pieces meet a request, bounded pieces require scale × bound ≤ request, and
  sampled pieces never certify one. An unmet request refuses with counts by
  accuracy class. Without a request the result reports achieved accuracy. The
  mesh-only interim path cannot satisfy a request. The planned sampling parameter
  is `chord_mm`, replacing `tolerance_mm`; the exact path ignores it, and an
  out-of-range model chord is refused against the view-DXF 0.0001–1 mm limits.
- **W17b is in the first ship:** [the shaded-view plan][shading] uses a shared
  CPU tile renderer first, one map for pixels/vectors, transparent backgrounds
  and five display styles. A future GPU executor needs comparison against CPU.
  [Latest resolution policy][sheet-dpi] is **sheet-only DPI**, default 300;
  it removes the older per-view override. PNG exports use their own DPI. The
  [accepted registration contract][shade-acceptance] anchors a picture's grid at
  the view frame's lower-left corner. With `p = 25.4/dpi`, its outward width is
  `ceil(width_mm/p - 1e-9)` pixels: 100 mm at 300 dpi is 1182 px, spanning
  `1182*p` mm without stretching or cutting. Samples use pixel centres (or quarter
  and three-quarter positions for 2×2 sampling). PNG output re-renders onto the
  sheet's top-left pixel grid for pixel-copy compositing; a shared rectangle/grid
  function is planned in `geom::paper`. Pixels are sRGB-encoded 8-bit straight
  RGBA with transparent background, blended in linear light onto white paper,
  not screen colours. Accepted checks bind to S1's tile renderer and S3's paper
  picture when their PRs exist. Reference-only views are allowed; reference-part
  occlusion and drawing light remain separate follow-ups.
- **Shared visual-style naming is still proposed:** [#5307's updated proposal][style-proposal]
  would store a stable visual-style name in a display state. It is not an agreed
  descriptor or parameter rename. The viewport [#5219][viewport-style] and display
  states [#5310][display-states] remain separate open work.
- **W17c belongs to existing claims:** the [view-edit plan][view-edit] spans
  W07/W16/W17a/W17b, not a new unowned package. Saved-view editing, inheritance,
  boundaries and picking must share their providers. The [inheritance contract][inheritance]
  resolves unset properties through parent/default values; alignment and scale
  have their own rules. Boundary picking is distinct from selecting content.
  [Pick boundaries/status][pick-data] do not become printed scene content.
  The [accepted editor contract][ux-acceptance] gives each changed Apply one
  Drawing undo step; unchanged Apply gives none. X discards only edits since the
  last Apply and closes, preserving earlier applied steps. OK applies remaining
  changes as one step and closes; unchanged OK only closes. Accepted behavioural
  checks bind to `drawing.view.inspect`, `drawing.view.edit` and the window host
  once available; Nathan-Dane owns the window's visual integration.
- **W23a replaces the earlier proposed workshop fixture:** the integrated
  [100 × 80 × 20 mm plate with two independent Ø10 bores][fixture-contract]
  drives width, bore edit/delete, deferred update and per-file undo expectations.
  It does not replace W01a's separate 100 × 60 section fixture. Source-version
  acceptance is already assigned to [#5428 / #5430][version-pr]; do not duplicate it.

## Study §18.10 requirement traceability

This **2026-10-25** increment maps the complete §18.10 of the [updated study][study-current]
to the [final plan][plan] and the later agreements in [#3874][bom-plan] and
[#3875][sheet-plan]. **D1–D19** below refer to that section's numbered decisions;
**E1–E14** refer to its numbered acceptance examples. They are source locators,
not new issue IDs. The glossary policy is included separately. This does not
complete the rest of W00's study traceability or replace the RFC's live checklist.

Ownership stays with the existing claims: **BOM** means CamWight's
W24/W25/W26/W31 scope in [#3874 / #3876][bom-pr]; **Sheets** means CamWight's
W18 scope in [#3875 / #3877][sheets-parent]. **Storage** is CamWight's
[#3890 / #3891][storage-pr] dependency for W12/W15. W16/W40 are cooperating
workspace/resource packages, not new assignments. Later custom-table work maps
to W43; no separate child owner is assigned here.

Evidence pins for this increment:

| Evidence | Revision and observed scope |
| --- | --- |
| BOM B1 | [#5107][bom-types-pr], open, `43388eec3600cf4357e36392fdaf55b68e3aa5be`; BOM types child. |
| BOM D | [#3876][bom-pr], open draft, `d1ec61b09097cfe4d786f81190190a2aa3fc72c8`; typed `doc::bom` rows, first-level/parts-only views, occurrence lookup and mass/material data. Its `Row::item` is positional until numbers are saved; `Row::count` remains an integer. |
| Sheets S2 | [#3877][sheets-parent], open draft, `9e32f29424703fb012b1d7f17e73f5b1efe6dad2`; cell content and title-block rectangles/label/value boxes. This supersedes the older zero-file observation for this parent elsewhere in this dated map. |
| Storage K2 | [#3891][storage-pr], open draft, `cef6b585e988a526bb69cef29e521a17323b1371`; Drawing/sheet records include `Sheet::documents: Option<SheetComponent>` with explicit source/component IDs. No view or table record in that slice. |
| Field keys | [#4786][field-keys-pr], open, `8b1b4e1fe0675a082b904e15ba6d4ac9fd2b2639`; S2's resolver still depends on the property-key work landing. |
| Typed quantities | [#4812][quantity-pr], open draft, `2876ae39ff27aa888b5bf598a0bd04aaf0405548`, zero changed files at this check: a claim, not a delivered quantity evaluator. |
| Sheet geometry | [#5108][sheets-pr] and [#5554][marks-pr] are the integrated geometry slices recorded above. They do not deliver template editing or persisted title blocks. |

**Open code** below means source inspected at these pins, not approved or executed
acceptance. **Gap** means the required behaviour is not established by those
slices or their current delivery reports; the existing owner must supply the
missing implementation/acceptance evidence. It is not a statement about unpublished
work. No Cargo, application, export or save/reopen scenario was run for this
traceability increment. Author test reports in [BOM D][bom-d-report] and
[S2][sheet-s2-report] remain author evidence.

### Drawing, templates and component roles

| Source / current requirement | Existing scope | Evidence/state | Missing acceptance or dependency |
| --- | --- | --- | --- |
| D1: one Drawing kind can contain part, assembly or mixed views. | W12/W17; Storage, Sheets | Open code: K2 has one Drawing record, without a part/assembly discriminator; views remain later. | Create mixed-content views in one saved Drawing; templates must not introduce another Drawing kind. |
| D1: add no content beyond the selected template; a parts list is placed explicitly or supplied by that template. | W18/W25; Sheets, BOM | Gap: neither S2 nor K2 creates template tables. | E12: a part template without a list adds none; an assembly template adds only its specified content. |
| D1: allow a manually placed one-row parts list for a part. | W24/W25; BOM | Gap: a Drawing parts-list consumer is not established. | E12: a body-owning component without children yields its own row when used as this list's source. |
| D2: templates carry sheets, borders, title blocks, notes and sheet tables. | W18/W40; Sheets | Plan: [S3 template scope][sheet-plan]; S2 supplies only cell/block definitions. | Instantiate all five kinds, with saved resource snapshots and valid object references. |
| D2: separate user defaults for part and assembly templates, with machining/fabrication examples per sheet size. | W18/W40; Sheets | Plan: [template update][sheet-update]; usable defaults wait for Storage/app/CLI. | Select and persist both defaults; verify size-specific original examples without storing a part/assembly Drawing type. |
| D2: a template list reads the sheet component and stays empty until that component is assigned. | W18/W25; Sheets, BOM; W12 dependency | K2 stores the source; table binding is a gap. | E12: empty list before first view, then the correct source; explicit source changes update the list. |
| D3: a part has bodies and no child component occurrences. | W24; BOM | Partial: D examines bodies/contents for counting; the complete role/glossary contract is not demonstrated. | Classify a body-owning leaf without making every body a separate part. |
| D3: an assembly has child component occurrences, whether or not it also owns bodies. | W24; BOM | Partial: D uses both contents and bodies; this is not acceptance of all consumers' terminology. | Test a welded body-owning component with bolts and an assembly with no own bodies. |
| D3: a subassembly is an assembly occurring inside another component. | W24; BOM | Gap in complete role/glossary evidence. | Distinguish definition role from its occurrence in a parent; verify nested assembly presentation. |
| D3: roles derive from current contents and are not stored component types. | W24; BOM | Partial source evidence only. | E11: add/remove the sole bolt and observe part → assembly → part, including undo and reopen. |
| D3: a body is not a part; an occurrence is a placement of a component. | W24; BOM | D keeps bodies, definitions and full occurrence paths distinct. | Verify quantity and labels for multiple bodies and repeated occurrences; preserve the existing occurrence terminology. |
| Terms/D3: add each introduced term to the glossary with its introducing PR; use table “kind”, BOM “type”, and occurrence rather than instance. | W18/W24–W26/W31; Sheets, BOM | Partial: #3876 adds bill of materials/type and occurrence-command references. | Audit all 20 proposed terms against their introducing children, including part/assembly/component wording. The two additions do not close the full glossary obligation. |

### BOM authority, numbering and views

| Source / current requirement | Existing scope | Evidence/state | Missing acceptance or dependency |
| --- | --- | --- | --- |
| D4: BOM data belongs to the design and exists independently of a Drawing. | W24; BOM | Open code: D supplies typed model-side rows used by the command/window. | Review/integrate the shared service; Drawing consumers must use its typed data, not reinterpret command JSON. |
| D4: many lists on different Drawings can filter/sort one BOM while sharing item numbers. | W25/W39; BOM | Gap: shared persisted numbering and Drawing lists are not delivered by D. | E6 plus differently sorted/filtered lists: identical authoritative row numbers, no quantity changes from filtering. |
| D5: persist stable item numbers in the design. | W24/W25; BOM | Gap: D's positional `Row::item` is explicitly provisional. [Later agreement][bom-update] uses stable component/type row identity. | Save/reopen; edit names/part numbers without renumbering; bind numbers per source and BOM view. |
| D5: allocate the next free number for a new row. | W24/W25; BOM | Gap. [Later agreement][bom-update] requires allocation in the row-creating change's same undo step. | Add a row, undo once, redo and reopen; a read-only query must not create saved numbering. |
| D5: deleting a row leaves other numbers unchanged until explicit renumbering. | W24/W25; BOM | Gap: provisional ordering is not stable numbering. | E5: delete item 3 and preserve 4–9 through save/reopen. |
| D6: double-click opens the parts-list editor with its explicit renumber/update action. | W25/W16; BOM, workspace integration | Gap in the inspected slices. | Exercise the actual editor; do not turn ordinary BOM refresh into implicit renumbering. |
| D6: renumber in the chosen list's order with start number and minimum digits. | W25; BOM | Plan in [BOM agreement][bom-plan]. | Test custom start/padding and current sort order; E5 yields 1–8 after explicit renumber. |
| D6: renumber updates all lists of that BOM view and reports affected balloons. | W25/W26/W39; BOM | Gap: no persistent list/balloon consumer evidenced. | Cross-Drawing/list propagation and an affected-balloon report; preserve other BOM views' numbering. |
| D6: typed item numbers are visibly marked local overrides, propagated only to that list's balloons. | W25/W26; BOM | Gap; S2's generic override data does not implement list behaviour. | E6: changing A's item 4 to 10 changes A and its balloons, while B remains 4. |
| D6: an explicit save-to-BOM operation makes an override authoritative. | W25; BOM | Plan in [BOM agreement][bom-plan]. | E6: only the explicit save changes B to 10; verify undo, conflicts and persistence. |
| D6: locked item numbers are skipped by renumbering. | W25; BOM | Gap. | E5: keep locked 7 while other rows become 1–6 and 8; no collision or silent unlock. |
| D6: duplicate/conflicting item numbers are flagged. | W25; BOM | Gap. | Deliberately conflict local/saved/locked values; show the conflict before applying an invalid change. |
| D7: structured first level lists direct contents and is the parts-list default. | W24/W25; BOM | D has `FirstLevel`; its service default is `PartsOnly`, not evidence of the future table default. | E1 top-level gearbox/plate; assert the newly placed parts list selects first level explicitly. |
| D7: structured all levels includes nested rows numbered 1, 1.1, 1.2. | W24/W25; BOM | Gap: D's `View` has only `PartsOnly` and `FirstLevel`. Folded children/`rows_deep` are not a third view. | E1: nested bolt at 2.1 under the gearbox, with stable hierarchical numbering. |
| D7: parts only flattens eligible parts, excluding phantom/reference rows. | W24; BOM | Open code: D has `PartsOnly` and type-dependent traversal. | E1/E3/E4 and later purchased/inseparable rules; integrate and execute the view's acceptance. |
| D7: each BOM view owns its own saved item-number sequence. | W24/W25; BOM | Gap: D exposes provisional positions only. | Switching views preserves each saved sequence; structured nesting does not overwrite flat numbering. |

### Types, quantities and columns

| Source / current requirement | Existing scope | Evidence/state | Missing acceptance or dependency |
| --- | --- | --- | --- |
| D8: Normal is the default type; each occurrence contributes to its row. | W24; BOM | Open code: B1/D type rules and typed rows. | E1 and repeated nested components; old designs default correctly and unrelated type overrides do not merge. |
| D8: Phantom has no row; multiply/promote its contents into its parent. | W24; BOM | Open code: B1/D traversal rules. | E3: four bolts contribute four without a phantom row; repeated/nested phantoms retain multiplicity. |
| D8: Reference remains visible but contributes no BOM row. | W24/W26; BOM | D has reference exclusion and `Unlisted::Reference`; Drawing warning/visibility is pending. [Later agreement][bom-update] excludes reference descendants from mass/volume too. | E4: of two brackets, one reference leaves quantity 1; visible reference balloon warns; physical totals exclude its subtree. |
| D8: Purchased is one bought row, without listing its contents as independent rows. | W24; BOM | Open code: B1/D rules. | E2: purchased gearbox stays one row; optional inspection of children must not count them again. |
| D8: Inseparable is one made item; its own BOM can list its contents. | W24; BOM | Open code: B1/D rules. The [later purchased-inside exception][bom-plan] overrides the study's blanket parent-list exclusion. | E2: ordinary bolt stays out of the parent's parts-only list, but purchased contents appear there; the gearbox's own BOM lists its contents. |
| D8: Inseparable differs from Normal only for an assembly, and purchased-only filtering distinguishes made from bought. | W24/W25; BOM | Type data exists; full filter/role acceptance is not established. | Compare leaf versus assembly types; E2's purchased filter includes the purchased gearbox and excludes the inseparable one. |
| D9: an occurrence defaults to following its component's type and can override any of the five types. | W24; BOM | Open code in #3876; [command report][occurrence-type-report] describes reset-to-component and one-step undo. | Change the component after reset; test independent repeated occurrences, persistence and undo at the reviewed revision. |
| D9: expose the per-occurrence type chooser in its context menu. | W24/W16; BOM, workspace integration | Gap: the command report explicitly leaves the right-click control for later. | Exercise all choices and the following-component state through the actual menu. |
| D10: virtual components without bodies participate in counting, numbering and balloons in every applicable list. | W24–W26; BOM | Plan: [later agreement][bom-update] requires an explicit virtual kind, not an empty-component heuristic. | E10: Grease is a real BOM member with a stable number and a usable balloon despite having no geometry. |
| D10: locally typed custom parts-list rows come later. | W43, coordinated with BOM | Deferred requirement; no separate child/owner identified here. | Define local-row identity/quantity/balloon policy before implementing; do not substitute such a row for a virtual design component. |
| D11: quantities support typed expressions on any component. | W24/W25; BOM; shared units #4812 | **Superseded:** the study's counts-only first delivery is replaced by [typed quantity agreement][bom-update]. D still exposes integer counts; #4812 is a zero-file claim. | Test count, mass, length, area and volume as supported by the agreed shared evaluator; unit conversion and multiplicity preserve dimensions. |
| D10/D11: virtual mass contributes, using the shared mass/volume unit path. | W24; BOM; shared units #4812 | Plan in [later agreement][bom-update]; no completed virtual-quantity path evidenced. | E10's “Grease 50 g, quantity 1” must not freeze counts-only semantics: specify the authored quantity and resulting count/mass, then check totals without double multiplication. |
| D12: defaults are Item, Quantity, Name, Description, Material, Mass, with Mass enabled and last. | W25; BOM | D supplies row data, not the placed table's columns. | Check exact default order and visibility in a new parts list. |
| D12: Name uses the browser component name and is always filled. | W24/W25; BOM | D carries component names; table presentation is a gap. | E14: Plate appears as Name even without a part number; verify grouped-row name policy explicitly. |
| D12: Part Number is optional and stays blank until entered; Revision/free properties are addable columns. | W24/W25; BOM | D carries part number, revision and properties; column editing is pending. | E14: missing part number is not copied from Name; add each optional column and retain it on reopen. |
| D12: Material and Mass use the agreed source/unit rules. | W24/W25; BOM | D includes material/mass data. [Later table agreement][table-layout] requires uniform assembly material unless explicitly overridden; [unit agreement][bom-update] uses user mass units, default kg with two decimals. | Mixed-material assembly stays blank unless overridden; verify unit display separately from unrounded data and unknown mass. |
| D12/D15: saved column widths and wrapped cells keep text inside table borders. | W25/W31; BOM | [Later agreement][table-wrap] applies to every table column; no Drawing table layout is evidenced. | Drag and reopen a column width; long text wraps and increases row height. Keep the agreed 7 mm initial row height adjustable; title-block cells do not auto-grow like table rows. |

### Balloons and placed tables

| Source / current requirement | Existing scope | Evidence/state | Missing acceptance or dependency |
| --- | --- | --- | --- |
| D13: each balloon links an occurrence to its chosen parts-list row/item number. | W26; BOM; W13/W19/W25 dependencies | D has full-path `row_of`, but no persisted balloon/list link. | Repeated nested occurrences resolve the intended row; saved/overridden list numbers display correctly. |
| D13: when several lists exist, placement chooses one, defaulting to the first. | W26; BOM | Gap. | Place balloons against two differently ordered lists and verify the selected list survives reopen. |
| D13: reference occurrences warn instead of inventing a row. | W26; BOM | D distinguishes reference exclusion; UI behaviour is pending. | E4: reference bracket stays visible, gives a clear warning and acquires no false item number. |
| D13: picking a phantom targets an occurrence inside it. | W26; BOM | D distinguishes a missing own row; the pick workflow is a gap. | Pick the phantom's child and resolve its promoted row, including nested phantoms. |
| D13: a deleted row leaves a retained pink lost-reference balloon. | W26; BOM; W13/W14 | Gap. | Delete/undo the row: retain balloon placement and identity, with textual lost status as well as pink. |
| D14: Add Item repeatedly appends picked balloons until Esc, preserving the first item's number. | W26; BOM | Gap; stack workflow belongs to the existing balloon scope. | E8: bolt, washer, washer, nut; Esc ends picking without losing the accepted items. |
| D14: all attached balloons share the stack's one leader. | W26; BOM | Gap. | E8: four touching balloons, one leader, each retaining its own occurrence/number. This is distinct from W26's multi-leader balloon whose branches resolve to one row. |
| D14: neighbours touch, with 45-degree snapping and free-angle placement. | W26; BOM | Gap. | Check snapped and unsnapped angles, contact spacing and saved placement. |
| D14: a stack may repeat an item number. | W26; BOM | Gap. | E8: both washers remain in the stack; do not deduplicate by row number. |
| D14: deleting the first balloon transfers the leader to the next. | W26; BOM | Gap. | E8: remove the bolt balloon; next balloon inherits the leader and undo restores the original arrangement. |
| D14: lost stacked balloons retain their objects and turn pink. | W26; BOM | Gap. | Delete a stacked item's row; retain the stack with per-item lost status, including after reopen. |
| D15: use one extensible sheet-table object family; parts lists and revision tables are its first kinds. | W25/W31; BOM; W43 later | Plan in [BOM agreement][bom-plan]; S2 cells do not constitute a saved table model. | Both kinds share cell/layout infrastructure without mixing their data authority; custom kinds remain extensible. |
| D15: place tables manually or through templates. | W25/W31/W18; BOM, Sheets | Gap. | Create and move both table kinds in each path without adding unrequested tables. |
| D15: table edges snap to drawing border, title block or another table. | W25/W31/W18; BOM, Sheets | Plan; [later defaults][table-layout] place parts lists top-right and revisions bottom-left, with headings on top by default and bottom optional. | Exercise all three targets and both heading positions; save the chosen attachment. |
| D15: resizing the sheet preserves a table's chosen anchor. | W25/W31/W18; BOM, Sheets | Geometry is integrated; persisted table anchors are not evidenced. | E9: A3 → A2 keeps a title-block-attached list attached; report out-of-bounds content rather than arbitrarily moving it. |
| D16: sheet border is the physical paper edge; drawing border is the editable inner outline. | W18; Sheets | Integrated geometry in #5108/#5554; editor/save integration is pending. | Keep both identities distinct through border edits, size/orientation changes and reopen. |
| D16: title block is a separate object from drawing border. | W18; Sheets | Open code: S2 defines a separate block, not its persisted instance. | Save/edit/replace a block independently. [Later lock agreement][sheet-lock] locks block and border by default while field values keep updating. |

### Revisions and explicit field sources

| Source / current requirement | Existing scope | Evidence/state | Missing acceptance or dependency |
| --- | --- | --- | --- |
| D17: revision rows contain Revision, Description, Designer, Checker and Date. | W31; BOM | Plan in [BOM agreement][bom-plan]; no revision-table records in K2/S2. | Create/edit rows and render these columns; preserve author-entered data without implying approval. |
| D17: revisions cover the Drawing by default, with per-sheet scope available. | W31/W39; BOM | Gap. | E7: three sheets share revision 3 after adding it; a deliberately sheet-scoped revision affects only that sheet. |
| D17: numeric revisions are the default. | W31; BOM | Plan; [later agreement][bom-plan] allows start 1 or 0. | Check initial sequence and saved numbering choice. |
| D17: letter mode skips I, O, Q, S, X and Z by default. | W31; BOM | Gap. | E7: H advances to J; exercise every skipped letter and persistence of the chosen scheme. |
| D17: manually entered excluded letters remain possible with a warning. | W31; BOM | Gap. | E7: enter I, retain it with an explanatory warning rather than silently changing it. |
| D18: persist the explicit component documented by each sheet. | W12/W18; Storage, Sheets | Open code: K2's `SheetComponent` records source/component IDs and validates the source reference. | Full source-resolution, persistence and missing-component acceptance; a schema field alone is not a functioning binding. |
| D18: seed the source from the first placed view, then allow explicit sheet-property edits. | W17/W18/W15; Sheets, Storage | Gap: K2 has no view-placement command. | Place first and later views; only the first initialises an unset source; explicit edits update its consumers in the proper undo scope. |
| D18: deleting that view never retargets the sheet component. | W17/W18; Sheets, Storage | Gap in lifecycle acceptance. | E13: delete the first view and retain component fields; missing source must be reported rather than replaced by another view. |
| D18: title block and template parts list use that same explicit sheet component. | W18/W25; Sheets, BOM | K2/S2 supply source records/tags separately; no joined resolver/table path evidenced. | E12/E13: two sheets for gearbox/shaft retain their own title/list values through view deletion and source edits. |
| D19: each title-block field has a configurable source. | W18/W40; Sheets | S2 has component/drawing/sheet/revision/prompt source tags and field keys. [Resolver sequence][sheet-s2-plan] waits for #4786. | Resolve actual values with checked keys; edit/reopen a source without flattening the field to literal text. |
| D19: an absent property or stale source shows "-" for the live field, never a previous value as current. | W18; Sheets | [Source-state agreement][sheet-plan] and [missing-value rule][sheet-update]; S2 represents missing/override states, but the joined resolver and persistence acceptance are pending. | Remove a property and separately make its source stale, then recover and reopen in each state: retain the binding, show "-" while unavailable, and resolve the current value on recovery. Repeat with a marked manual override; keep it distinct from live/source status without detaching the field or deleting the override. |
| D19: component name, description, material and mass read the sheet component by default. | W18; Sheets | Source tags exist; default definitions/resolution remain pending. | E13: gearbox and shaft show their own data; use current material/mass rules, not whichever view was last inspected. |
| D19: Drawing number defaults to the first sheet component's name. | W18/W39; Sheets | Plan; K2's generic Drawing name is not evidence of this default/binding. | E13: Drawing number remains Gearbox while the shaft sheet retains its own component fields; specify/review first-sheet reorder/delete handling before implementation. |
| D19: sheet number/count, projection and paper size come from Drawing/sheet context. | W18/W39; Sheets | K2 has ordered sheets/paper and count exclusion; S2 has a projection-symbol kind, without resolved rendering. | Test n/m with excluded sheets, correct paper size and first/third-angle symbols shared with views. |
| D19: scale is configurable, but absent from the default title block. | W18; Sheets | **Default changed** by [later layout agreement][sheet-defaults]; individual views state their scales. | Mixed-scale Drawing has no misleading single default-block scale; preserve explicit custom-field capability only with a defined source. |
| D19: revision/person/date fields can read the newest revision row. | W18/W31; Sheets, BOM | S2 has a revision source tag. [Later agreement][sheet-revision-fields] maps Created by / Approved by / Date of issue to that row, showing “-” before it exists. | E7: adding revision 3 updates all linked fields from row 3; no stale prior values when the source is missing. |
| D19: the default block omits Rev, Document Type and Document Status. | W18/W31; Sheets, BOM | **Default changed** by [later layout agreement][sheet-defaults]; revision/status live in the revision table. | Review the original default definition; retain configurable revision fields without reintroducing the superseded default. |
| D19: the original default block follows the accepted 180 × 54 mm layout and field set. | W18; Sheets | [Accepted prototype][sheet-prototype] plus [default-field agreement][sheet-defaults]; S2 defines generic block geometry, not the built-in template. | Verify Title, Identification, Created by, Approved by, Date of issue, Sheet, Legal owner, Material, Finish, Mass, General tolerance and Projection; part/assembly templates provide their different tolerance notes. |
| D19: manual field overrides are marked and keep their source. | W18; Sheets | S2 represents resolved/overridden/missing values; no persistent editing/resolution path evidenced. | Override, update source, reopen and remove override: recover the current linked value and retain clear override status. |
| D19: field presentation uses shared units, date settings and measured text. | W18/W10; Sheets | [Later agreements][sheet-update] set mass to user units (default kg, two decimals); [date agreement][sheet-revision-fields] separates UI display format from ISO script dates. S2 layout does not measure text. | Integrate #4890 paper text after shared metrics; verify actual wrapping/width warnings, 1 mm padding, fixed cell bounds and complete values without truncation. |
| D19: dates are stored as ISO 8601; each Drawing captures and persists its display format. | W18/W12; Sheets, Storage | [Date-storage agreement][sheet-update] captures the device format for a new Drawing; [platform/CLI agreement][sheet-revision-fields] uses platform settings (locale-derived order where needed) and ISO 8601 for scripts/CLI. Persistence and presentation acceptance remain pending. | Save and reopen under another device/browser locale: retain the Drawing's display format and ISO date value. An explicit display-format change updates presentation without changing that value; CLI/script output stays ISO 8601 across locales and format changes. |

Acceptance coverage is explicit: **E1** views; **E2** purchased/inseparable;
**E3** phantom multiplicity; **E4** reference count/warning; **E5** deletion,
renumbering and locks; **E6** local versus saved numbers; **E7** revision scope,
fields and lettering; **E8** stacks; **E9** anchors on resize; **E10** virtual
quantities; **E11** derived roles; **E12** templates and one-row lists;
**E13** sheet sources; **E14** name versus part number. None is marked passed by
this document. E2, E10 and the default title-block part of E7/E13 must be read
with the later agreements above, not the superseded study defaults.

Remaining traceability gaps are concrete: child PRs for saved numbering,
all-levels views, Drawing tables/balloons/revisions, virtual typed quantities,
template defaults and field-resolution/editor integration have not been
identified as completed slices in this check. The existing owners should bind
those acceptances to their eventual revisions. The complete glossary audit and
the Drawing-number default's first-sheet reorder/delete policy also need evidence
or a decision. No new owner, issue, API or successful gate is implied.

## Package inventory

Dependencies below preserve the [plan][plan], with the linked later splits above.
They are delivery gates, not a requirement to finish an entire large package before
any independent child. `Plan` rows retain scope without reserving it. CamWight's
parent claims remain relevant even when a particular child is integrated.

### M0/M1: contracts, sections and transient tools

| Package | State / owner and discussion | Dependencies | Next handoff / acceptance |
| --- | --- | --- | --- |
| W00 ledger | W01a bootstrap integrated; W00b documentation claim [#6163][w00b-pr], `0xlillith` | RFC, current rules | Current package/decision map; atomic study requirement traceability still open. |
| W01 neutral contracts | W01a integrated [#2085][w01-pr] | W00 | Finish source/provenance/outcome contracts; structural success does not certify a section. |
| W02 live extraction | Backend coordination; no standalone W02 delivery verified | Reviewed W01, kernel agreement | Guarded, fallible neutral adapter; capability/provenance matrix, no cache-byte authority. |
| W03 shared sections | Open shared-crossing code [#6272][section-crossings], then [#2068][section-pr], NOWN-DEV, base `main` | W02 | Shared crossing provider first; classified cut outcomes, winding/nesting and analytic accuracy evidence remain acceptance requirements. |
| W04 section command/output | Open JSON-only [#2068][section-pr] at `d2a9e485` (2026-10-25) | W03; #6272 first; W10 geometry subset for later output | Explicit plane/occurrence input; read-only JSON and complete-or-error output; Cam's neutral adapter follows both merges. Physical SVG/DXF is a separate later export requirement. |
| W05 HLR investigation | Open draft code [#2106][investigation-pr], SychPL; [findings][hlr-findings] | W01, W02 as needed | Review supported surfaces, analytic intervals, uncertainty and native/web cost before promotion. |
| W06 HLR subset | [#2457][hlr-pr] merged into drawings on 27 Oct as ebafe074; [#3013][hlr-parallel] open — SychPL | Reviewed W05; W02 integration remains a separate provider | Analytic HLR core is integrated. Parallel work/aggregate budgets and full scene-to-paper acceptance remain subsequent pieces. |
| W07 snapshot command | Parent claim [#5867][snapshot-pr], CamWight; V1 claim [#6482][mesh-lines-pr] into `main` (2026-10-25); [current plan][snapshot] | W17a; preview adapter or W06; W09b; W10 | V1 shared projection first; V3 on `drawings` adds source/owner identity under the [revised split][mesh-lines-split]. Frame/scale, occurrences, references, labels, scene, exact lines and shading remain later slices. |
| W08 snapshot preview/output | Plan; coordinate W07, W10 and W16 owners | W07, W10 text/dimension subset; W04 reuse | SVG/DXF/PNG and preview with snapshot/accuracy status, physical size and command parity. |

Standalone section work does not wait for persistent Drawing files, fonts or PDF.
The existing viewport section and approximate view-DXF tools do not by themselves
complete W03–W08. W01a radians, accuracy and result-local IDs require explicit
adaptation to an existing producer's schema.

### Shared foundations and M2: persistent single-part checkpoint

| Package | State / owner and discussion | Dependencies | Next handoff / acceptance |
| --- | --- | --- | --- |
| W09 formats | W09a integrated; W09b claim [#5315][number-pr], armin0411; broader claim [#4897][dimensions-pr], CamWight | W00 | Shared numbers below UI, then typed tolerances/profiles; retain raw values and locale-independent meaning. |
| W10 paper/layout | A1 integrated; parent claim [#4891][export-pr], CamWight | W01; W09/font contract for text | SVG/shared prepared traversal and earlier clipping, then text, dimensions/tables and pictures. |
| W11 storage decision | Embedded-in-design G3 [#3890][storage]; K3 [#7098](https://github.com/tangency-labs/solidcraft/pull/7098) merged, K3b [#7490](https://github.com/tangency-labs/solidcraft/pull/7490) draft | W00 | Per-drawing undo/revisions within one design, no independent .scdwg file. Old linked-source acceptance rows are historical and require new in-file fixtures. |
| W12 saved records | CamWight [#3891][storage-pr] at 36cf48a3; [K3b](https://github.com/tangency-labs/solidcraft/pull/7490) remains draft | W11, W01, W09 | Bounded in-design records, versioned drawing entries and old-reader refusal. Complete saved/reopen acceptance is pending. |
| W13 references | Claim [#4893][refs-pr], CamWight | W01; first read-only slices can precede W12 | Full placement/name/intent; explicit ambiguous/lost/unverified states; bounded repair candidates. |
| W14 invalidation/update | Same claim [#4893][refs-pr] | W12/W13; W06 for projected results | Source/layout/display keys; coherent publication and late-result rejection across edit/undo/close/cancel. |
| W15 commands/undo | CamWight [#3891][storage-pr] (K4-prime in-file sessions) | W12–W14; [revised storage plan][storage-plan] | One drawing's edits and undo are isolated from its siblings and model; unchanged drawing data cannot trigger source evaluation. |
| W16 workspace | Parent claim [#4895][workspace-pr], CamWight; open scaffold [#5349][ui-pr], Nathan-Dane | W15, W10 basic scene | Connect real providers to the shared UI kit/navigation; empty preview canvas is not an editor. |
| W17 views | Parent package, split below | W06, W12–W16 for persistent views | Multiple aligned/derived views with independent style and visibility; reject graph cycles. |
| W17a view data | Integrated [#4817][view-pr] on 27 Oct in drawings at de7bf4fc | Independent L0 data slice using W01a | Frame/scale definitions delivered; persisting, displaying and editing views are later work. |
| W17b shaded views | Claim [#5626][shade-pr], CamWight | W17a, W10 picture; W06 for vector overlays | Tile CPU renderer → picture/grid → snapshot → outputs/canvas → cache; sheet DPI and honest pending/failure state. |
| W17c view editing | [Plan inside existing claims][view-edit], CamWight | W12/W17a; W16-6 for the sheet side | Inspect/edit, inheritance, model-derived extents and boundary/content picking; source, undo and printed bytes remain coherent. |
| W18 sheets/resources | Sheet children integrated; parent [#3877][sheets-parent], CamWight, now zero-file diff | W09/W10/W12/W15 for full objects | Typed title fields and anchored furniture; integrated border geometry is only the first slice. |
| W19 dimensions/notes | Claim [#4897][dimensions-pr], CamWight | W09/W10/W13/W15/W17 | Intent/value/text/placement separated; manual placement and lost/override status preserved. |
| W20 round/chain/centres | Same claim [#4897][dimensions-pr] | W19 | Analytic diameter/radius, picked directions, idempotent centre marks and paper-sized gaps. |
| W21 SVG/DXF/PNG | Claim [#4891][export-pr], CamWight | W10/W17–W20; geometry-only children earlier | SVG first; physical size, view scale, lineweights, grouping and declared conversions across writers. |
| W22 PDF | Same claim [#4891][export-pr]; library choice recorded | W10/W18–W20, fonts | Vector physical pages, searchable embedded fonts, reproducible metadata and layers before industrial gate. |
| W23 M2 integration | Parent claim [#4895][workspace-pr]; W23a integrated [#4962][fixture-pr] | W14–W22 | Native/browser/headless save, edit, update, repair, undo and output. Model fixtures alone do not pass M2. |

### M3: first industrial part-and-assembly release

| Package | State / owner and discussion | Dependencies | Next handoff / acceptance |
| --- | --- | --- | --- |
| W24 BOM service | Open draft code [#3876][bom-pr], CamWight; first child [#5107][bom-types-pr] open | W00/G7, W12 source contract | Fallible nested typed quantities, stable product/row identity and authoritative BOM item numbers under [the later agreements][bom-update]; types and properties independent of UI. |
| W25 sheet tables | Same parent [#3876][bom-pr]; complete Drawing table delivery not verified | W24/W10/W12/W15 | Consume BOM item numbers; typed linked/literal/computed cells, marked local overrides and explicit save to BOM; no implicit row merging. |
| W26 balloons | Same parent [#3876][bom-pr]; complete balloon delivery not verified | W13/W19/W25 | Every leader resolves to its row/occurrence; view hiding never changes product quantity. |
| W27 tolerances/fits | Plan; coordinate W09/W19 owners | W09/W13/W19–W20, reviewed profile evidence | Supported tolerance/fit semantics, nominal precision and truthful unavailable metadata. |
| W28 datums/GD&T | Plan | W10/W13/W19/W27, drafting review | Bounded semantic grammar and original symbols with edition-specific evidence. |
| W29 feature callouts | Plan | W19/W27, feature metadata audit | Hole/thread/chamfer identity and occurrence scope; never infer process data from appearance. |
| W30 manufacturing notes | Plan | W10/W19/W27, per-family evidence | Shared leaders/text; reviewed original weld/surface/edge symbol families in small children. |
| W31 revisions | Revision-table scope in [#3876][bom-pr], CamWight; full package not delivered | W10/W12/W15/W18, release-status decisions | Separate human-entered issue/check data, row/tag IDs, preserved history and explicit override/export policy. |
| W32 industrial acceptance | Plan | W23–W31, agreed assembly subset of W33 | Real part and assembly jobs, integrity check, coherent exports and package publication; explicitly resolve any first-ship gap. |

**M2 is not M3.** The single-part checkpoint cannot replace assembly quantities,
balloons, supported tolerances/GD&T/callouts, live fields/revisions, reliable updates
and production outputs. The later shaded-view decision also belongs to first ship.
Neither this map nor a UI scaffold declares either milestone complete.

### M4/M5 and separately reviewed M6 extensions

| Package | State / owner and discussion | Dependencies | Next handoff / acceptance |
| --- | --- | --- | --- |
| W33 assembly/state views | Plan; M3 needs its agreed minimum | W13–W17/W24 | Explicit configuration and full occurrence identity; separate placement, hiding, appearance and section participation. |
| W34 sections | Plan reusing NOWN-DEV Stage A | W03/W06/W13–W17/W10 hatch | Full, half and aligned cuts, depth/participation and hatching; persistent references survive edits. |
| W35 details/crops | Plan | W06/W13–W17, clipping | Defined boundary/scale/label; references leaving a crop are reported, not silently deleted. |
| W36 broken views | Plan | W13–W20; W35 clipping where reused | Nonuniform paper mapping preserves true engineering lengths through edits and removal. |
| W37 fabrication | Plan; independent flat/cut utilities may advance earlier | W03/W04 for cuts; W13–W18 for sheets | Authoritative perforations, bend identity, A-side/handedness and true-size output; not mixed-scale sheet cutting. |
| W38 sheet/view sketches | Parent claim [#5426][draft-pr], CamWight; W38.0 is open code [#6336][sketch-store-pr], targeting `main` | W10/W12/W15–W18; W34 for section hatch | W38.0 centralizes sketch-command access through `cmd::sketch_store` before W38a/b; it does not deliver saved Drawing sketches. Sheet/view-owned geometry, projected links and fills/images remain later slices, with no implicit model-sketch mutation. |
| W39 multiple sheets | Plan | W12–W18; W25–W26/W31 for linked objects | Stable IDs/numbering through reorder/copy/delete, navigation and sheet sets. |
| W40 reusable resources | Plan; coordinate W18 title/template owner | W18/current libraries; W31/W39 later | Original editors/templates, immutable embedded resource snapshots and explicit updates. |
| W41 annotation productivity | Plan | W19–W20/W26–W30 | Placement-preserving arrange, sets, leaders and text in separately tested increments. |
| W42 copy/derive/replace | Plan | W13–W15/W39–W40, clipboard contract | Bounded ID/reference/resource remapping; retain lost objects; mirrored drawings recompute semantics. |
| W43 richer tables | Plan; coordinate W24–W26/W31 owner | W25/W29/W31/W37 | Hole/bend/general tables, explicit quantity scopes and safe spreadsheet exports. |
| W44 richer DXF | Plan; shared writer refactors are prerequisites | W21/W25–W31; W34/W38 as supported | Native dimensions/hatches/text/blocks/layouts in declared subsets; exploded output remains selectable. |
| W45 PDF/print production | Plan; basic layers required before M3 or explicitly renegotiated | W22; W39 multi-sheet; W32 release batch | Shared vectors, selected scopes, physical print/batch; archival/accessibility claims separately qualified. |
| W46 specialist domains | Plan / separately reviewed extensions | Relevant views/annotations and model-side owners | Shaft, weld, stock, routing, harness and staged manufacture each need trustworthy metadata and a domain reviewer. |
| W47 checking/automation | Plan / separately reviewed extensions | Stable contracts, G24 and format gates | Scoped coverage/unknown states, comparisons and ownership-aware generators; no implied manufacturing-completeness proof. |

## Immediate dependency handoffs

| Boundary | Current owner / prerequisite | Small next delivery and evidence |
| --- | --- | --- |
| Shared paths → Drawing sources | CarelT [#5420][paths-pr] (open code targeting `main`) → [#5449][link-state-pr] (open draft); [Cam's agreement][path-handoff] | Reuse the path move and common loader/status contract. Do not duplicate K5 helpers or confuse file availability with Drawing result freshness. |
| Versioning → storage → commands | CamWight [#3890][storage] and [#3891][storage-pr]; embedded plan [G3][g3] | K3 #7098 merged; K3b #7490 draft; K4-prime in-file sessions and subsequent view/source/cache integration follow. Separate file linking/packaging slices described in earlier comments are superseded; keep the new in-file undo and versioning acceptance explicit. |
| Source versions → acceptance | amolinari-autotex [#5430][version-pr], open draft with 11 files | Bind minimal deterministic fixtures to the accepted storage API before executable integration claims. |
| Shared numbers/text → labels | armin0411 [#5315][number-pr] (claim); CamWight [#4815][text-pr] merged as `d557c09c`, main only under the 2026-10-25 ancestry check above | Number formatting and measured text are separate dependencies; Drawing consumers still need integration. A metric API alone does not deliver shaping, fonts or Drawing layout. |
| Paper A1 → canvas/SVG | CamWight [#4890][export], Nathan-Dane [#5349][ui-pr] | Shared prepared traversal/clipping and content token before picking/output. Distinguish valid empty results from unavailable, pending, failed and stale results; source freshness remains separate. |
| Main DXF → Drawing adapter | [#5318][dxf-paths] / draft [#6184][dxf-claim] → [#5319][dxf-file] → [#5320][dxf-read] → [#5321][dxf-one] | Original 53 byte pins remain unchanged. X1 head 3fb2eef9 has 6/7 sheet failures fixed, one source-provenance blocker, and is not merge-ready; X2–X4 and paper A2 follow only after its qualified integration. [Accepted main-base agreement][dxf-decision]. |
| HLR → frame → snapshot | SychPL [#2457][hlr-pr]/[#3013][hlr-parallel], CamWight [#4816][view-data]/[#5866][snapshot-issue] | [Accepted scale × accuracy × occurrence matrix][accuracy-contract] beside W07 snapshot tests, using neutral HLR fixtures. Bind to the actual `max_error_mm` check and separate `chord_mm` sampling API; no silent clamping or Sampled certification. |
| Shading → paper picture | CamWight [#5624][shade-issue], [picture plan][writers-plan] | [Registration/colour/alpha conventions and acceptance child agreed][shade-acceptance]. Bind runnable checks to S1 and S3 when their PRs exist; no second renderer or claim of a shipped provider. |
| View editing → canvas | CamWight [W17c][view-edit], Nathan-Dane [#5349][ui-pr] | [Behaviour/undo matrix accepted][ux-acceptance]; bind to the actual inspect/edit commands and window host. Real providers precede enabled editing; scaffold validation is not persisted-editor acceptance. |

## Milestone acceptance and maintenance

The Drawing M0–M6 names below come from [the RFC plan][plan]; they are distinct
from the repository roadmap's M0–M14 (Drawings is roadmap M11).

| Milestone | Evidence needed before declaring completion |
| --- | --- |
| M0 contracts/decisions | Reviewed small interfaces, fixtures, provenance/accuracy vocabulary and ownership; unresolved gates remain linked. |
| M1 standalone tools | Body-derived sections/visibility and labelled transient measurements with explicit unsupported cases, native/browser costs and command parity. |
| M2 single-part integration | W23a source edits, lost-reference repair, deferred/automatic update, separate undo, save/reopen and physically matching native/browser/headless exports. |
| M3 industrial part/assembly | W32's part and assembly jobs, BOM/balloons, supported standards subset, callouts/revisions and first-ship shading/output; no silent stale values, substitutions or omissions. |
| M4 derived/fabrication | Sections/details/breaks and true-size cut/bend output preserve model meaning and declared accuracy. |
| M5 production/productivity | Multi-sheet identity, reusable resources, copying and packages remain coherent through edits. |
| M6 extensions | Separate owners, lawful sources, bounded scope and specific acceptance for each domain, automation or interchange proposal. |

Use independent analytic expectations alongside scene/entity/text assertions;
self-round-trips and pictures alone cannot establish geometry or physical scale.
Fixtures should cover nonfinite/cyclic/oversized input, missing/replaced/suppressed
sources, repeated placements, stale completions, cancel boundaries and unchanged
model evaluation for paper-only edits. Benchmarks report cold/warm work, memory
and cancellation separately; a source review is not a runtime measurement.

For each child handoff, record its public issue/claim, exact base/head, owned paths,
contract revision, dependency PRs, supported cases, fixtures, actual checks and
remaining limits. Current repository gates govern validation and publishing;
draft or skipped CI is not a passing run. Recheck branch membership after a merge
and update only the delivered slice. Do not close the umbrella RFC or claim full
study traceability from completion of this documentation child.

[base]: https://github.com/tangency-labs/solidcraft/tree/4c63a88fc3035ac91b4b9dc847fffa0d2d5ec155
[main-oct25]: https://github.com/tangency-labs/solidcraft/tree/2741cd9a8b0b84b7d33001d8f9473198b5d53f73
[drawings-oct25]: https://github.com/tangency-labs/solidcraft/tree/672ef13eb8b8e2cb0ddf9c795e337b73d043eb07
[sheet-dxf-options]: https://github.com/tangency-labs/solidcraft/pull/6130
[dxf-text-refusal]: https://github.com/tangency-labs/solidcraft/pull/5709
[sketch-store-fix]: https://github.com/tangency-labs/solidcraft/pull/6336#issuecomment-6390580099
[mesh-lines-pr]: https://github.com/tangency-labs/solidcraft/pull/6482
[mesh-lines-split]: https://github.com/tangency-labs/solidcraft/issues/5866#issuecomment-6393854680
[dxf-composition]: https://github.com/tangency-labs/solidcraft/pull/5709#issuecomment-6394720621
[sketch-store-selection]: https://github.com/tangency-labs/solidcraft/pull/6336#issuecomment-6394700363
[shade-s1-pr]: https://github.com/tangency-labs/solidcraft/pull/6483
[plan]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6234779815
[rfc]: https://github.com/tangency-labs/solidcraft/issues/860
[study]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6216908058
[study-current]: https://github.com/user-attachments/files/33742361/inventor-drawings-research.md
[bom-plan]: https://github.com/tangency-labs/solidcraft/issues/3874#issuecomment-6303870854
[bom-update]: https://github.com/tangency-labs/solidcraft/issues/3874#issuecomment-6338062010
[sheet-plan]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6303871228
[sheet-update]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6338062425
[sheet-revision-fields]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6339854611
[sheet-lock]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6359337075
[sheet-defaults]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6363027096
[sheet-prototype]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6364627034
[sheet-s2-plan]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6388563507
[sheet-s2-report]: https://github.com/tangency-labs/solidcraft/issues/3875#issuecomment-6389693817
[bom-d-report]: https://github.com/tangency-labs/solidcraft/issues/3874#issuecomment-6388108030
[occurrence-type-report]: https://github.com/tangency-labs/solidcraft/issues/3874#issuecomment-6324471095
[table-layout]: https://github.com/tangency-labs/solidcraft/issues/3874#issuecomment-6358344907
[table-wrap]: https://github.com/tangency-labs/solidcraft/issues/3874#issuecomment-6359336735
[field-keys-pr]: https://github.com/tangency-labs/solidcraft/pull/4786
[quantity-pr]: https://github.com/tangency-labs/solidcraft/pull/4812
[section-crossings]: https://github.com/tangency-labs/solidcraft/pull/6272
[section-json]: https://github.com/tangency-labs/solidcraft/pull/2068#issuecomment-6387996533
[section-adapter]: https://github.com/tangency-labs/solidcraft/pull/2068#issuecomment-6388998429
[text-main-check]: https://github.com/tangency-labs/solidcraft/compare/d557c09c3b5a059e553a5d9f1fb2682e52ecbaae...ac53fc1e6fab85a50bdc410178cc75133183393f
[text-drawings-check]: https://github.com/tangency-labs/solidcraft/compare/d557c09c3b5a059e553a5d9f1fb2682e52ecbaae...672ef13eb8b8e2cb0ddf9c795e337b73d043eb07
[w00b]: https://github.com/tangency-labs/solidcraft/issues/6157
[w00b-pr]: https://github.com/tangency-labs/solidcraft/pull/6163
[w01-pr]: https://github.com/tangency-labs/solidcraft/pull/2085
[w09-pr]: https://github.com/tangency-labs/solidcraft/pull/2086
[sheets-pr]: https://github.com/tangency-labs/solidcraft/pull/5108
[marks-pr]: https://github.com/tangency-labs/solidcraft/pull/5554
[paper-pr]: https://github.com/tangency-labs/solidcraft/pull/5513
[fixture-pr]: https://github.com/tangency-labs/solidcraft/pull/4962
[degree-pr]: https://github.com/tangency-labs/solidcraft/pull/5281
[branch]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6351210283
[shepherd]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6352192270
[kernel-pause]: https://github.com/tangency-labs/solidcraft/issues/3992
[kernel-tracking]: https://github.com/tangency-labs/solidcraft/pull/4012
[g3]: https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6427665746
[g3-json]: https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6352154058
[storage]: https://github.com/tangency-labs/solidcraft/issues/3890
[storage-plan]: https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6427665746
[ids]: https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6375022562
[refs-plan]: https://github.com/tangency-labs/solidcraft/issues/4892#issuecomment-6344611677
[svg-first]: https://github.com/tangency-labs/solidcraft/issues/4890#issuecomment-6360998195
[scene-boundary]: https://github.com/tangency-labs/solidcraft/issues/4890#issuecomment-6369541766
[writers-plan]: https://github.com/tangency-labs/solidcraft/issues/4890#issuecomment-6380548192
[pdf-choice]: https://github.com/tangency-labs/solidcraft/issues/4890#issuecomment-6349414755
[snapshot]: https://github.com/tangency-labs/solidcraft/issues/5866#issuecomment-6384152867
[view-map]: https://github.com/tangency-labs/solidcraft/issues/4816#issuecomment-6342112690
[view-bases]: https://github.com/tangency-labs/solidcraft/issues/4816#issuecomment-6384153494
[accuracy-contract]: https://github.com/tangency-labs/solidcraft/issues/5866#issuecomment-6387949895
[shading]: https://github.com/tangency-labs/solidcraft/issues/5624#issuecomment-6380783992
[sheet-dpi]: https://github.com/tangency-labs/solidcraft/issues/5624#issuecomment-6384153289
[style-proposal]: https://github.com/tangency-labs/solidcraft/issues/5307#issuecomment-6384774858
[viewport-style]: https://github.com/tangency-labs/solidcraft/pull/5219
[display-states]: https://github.com/tangency-labs/solidcraft/pull/5310
[view-edit]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6384152604
[inheritance]: https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6382967724
[pick-data]: https://github.com/tangency-labs/solidcraft/issues/4890#issuecomment-6382967982
[fixture-contract]: ../tests/oracle/W23a-workshop-acceptance.md
[version-pr]: https://github.com/tangency-labs/solidcraft/pull/5430
[section-pr]: https://github.com/tangency-labs/solidcraft/pull/2068
[investigation-pr]: https://github.com/tangency-labs/solidcraft/pull/2106
[hlr-findings]: https://github.com/tangency-labs/solidcraft/issues/860#issuecomment-6243691483
[hlr-pr]: https://github.com/tangency-labs/solidcraft/pull/2457
[hlr-parallel]: https://github.com/tangency-labs/solidcraft/pull/3013
[snapshot-pr]: https://github.com/tangency-labs/solidcraft/pull/5867
[number-pr]: https://github.com/tangency-labs/solidcraft/pull/5315
[dimensions-pr]: https://github.com/tangency-labs/solidcraft/pull/4897
[export-pr]: https://github.com/tangency-labs/solidcraft/pull/4891
[storage-pr]: https://github.com/tangency-labs/solidcraft/pull/3891
[k1-pr]: https://github.com/tangency-labs/solidcraft/pull/6196
[k1-published]: https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6386431674
[refs-pr]: https://github.com/tangency-labs/solidcraft/pull/4893
[workspace-pr]: https://github.com/tangency-labs/solidcraft/pull/4895
[ui-pr]: https://github.com/tangency-labs/solidcraft/pull/5349
[view-pr]: https://github.com/tangency-labs/solidcraft/pull/4817
[shade-pr]: https://github.com/tangency-labs/solidcraft/pull/5626
[sheets-parent]: https://github.com/tangency-labs/solidcraft/pull/3877
[bom-pr]: https://github.com/tangency-labs/solidcraft/pull/3876
[bom-types-pr]: https://github.com/tangency-labs/solidcraft/pull/5107
[draft-pr]: https://github.com/tangency-labs/solidcraft/pull/5426
[sketch-store-pr]: https://github.com/tangency-labs/solidcraft/pull/6336
[paths-pr]: https://github.com/tangency-labs/solidcraft/pull/5420
[link-state-pr]: https://github.com/tangency-labs/solidcraft/pull/5449
[path-handoff]: https://github.com/tangency-labs/solidcraft/pull/3891#issuecomment-6363719119
[text-pr]: https://github.com/tangency-labs/solidcraft/pull/4815
[export]: https://github.com/tangency-labs/solidcraft/issues/4890
[dxf-paths]: https://github.com/tangency-labs/solidcraft/issues/5318
[dxf-file]: https://github.com/tangency-labs/solidcraft/issues/5319
[dxf-read]: https://github.com/tangency-labs/solidcraft/issues/5320
[dxf-one]: https://github.com/tangency-labs/solidcraft/issues/5321
[dxf-decision]: https://github.com/tangency-labs/solidcraft/issues/5318#issuecomment-6385918078
[dxf-claim]: https://github.com/tangency-labs/solidcraft/pull/6184
[view-data]: https://github.com/tangency-labs/solidcraft/issues/4816
[snapshot-issue]: https://github.com/tangency-labs/solidcraft/issues/5866
[shade-issue]: https://github.com/tangency-labs/solidcraft/issues/5624
[shade-acceptance]: https://github.com/tangency-labs/solidcraft/issues/5624#issuecomment-6388998143
[ux-acceptance]: https://github.com/tangency-labs/solidcraft/issues/4894#issuecomment-6388998672
