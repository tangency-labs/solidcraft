# Parametric bend slots and end reliefs

Status: native feature on the current flat-pattern sheet model. The command and editor are available in a build from this branch. The geometry and selection limits below are part of its contract.

`sheet.bend_slots` adds one editable timeline feature to an existing sheet-metal body. The feature keeps the upstream sheet intact. Editing its parameters rebuilds the cuts; suppressing or deleting the feature restores the upstream sheet. No separate unfold, sketch, extrusion or refold features are needed. The result retains the sheet's original folded or unfolded state.

On each rebuild, the feature reads the current sheet's bend lines and outline. Slots follow the bends in the sheet's current flat coordinates, including a change of flat-pattern axes. The feature does not retain a one-time list of slot coordinates. Width, bend angle and other valid upstream edits must rebuild through the ordinary dependency history.

## Parameters

All length fields accept expressions over design parameters and explicit units.

| Field | Default | Meaning |
| --- | --- | --- |
| `body` | Newest sheet | Target sheet-metal body. Use an explicit body name in scripts. |
| `bends` | Omitted or empty: snapshot currently untreated bends | A list of distinct current bend tags such as `F3.0`. Creation captures each tag with its certified physical source identity. |
| `edges` | Optional alternative to `bends` | Current exact B-rep edge indices on `body`, resolved once into saved bend references. |
| `slot_length` | 7 mm | Overall slot length, including its end caps. |
| `slot_width` | 1 mm | Slot height across the bend and end-relief throat height. |
| `bridge` | 3 mm | Material between adjacent slots. |
| `end_margin` | 2 mm | Minimum material between the slot row and each bend-end relief. |
| `corner_radius` | 3 mm | Radius of the rounded relief-entry corners beside the adjacent panels. |
| `relief_depth` | Follows `corner_radius` | Depth of the relief throat before its closed end cap. Omit the field to keep that dependency. |
| `profile` | `rounded` | `rounded` gives semicircular slot and throat ends; `rectangular` gives straight ends. |
| `reliefs` | `true` | Generate matching reliefs at the two ends of each selected bend. |
| `name` | Optional | Timeline feature name. |

The larger relief-entry corner radius is independent of the slot's cap radius. In rounded mode, the slot and relief throat both use `slot_width/2` for their closed end caps. The throat height and centreline remain aligned with the slots in both modes. Rectangular mode changes those ends; `corner_radius` still controls the larger entry corners. For entirely rectangular reliefs, set `profile` to `rectangular`, `corner_radius` to zero and an explicitly positive `relief_depth`.

The nominal repeat cut fraction is `slot_length / (slot_length + bridge)`: 7 mm slots and 3 mm bridges give 70%. End margins and rounded caps affect the total removed area; this fraction does not describe strength.

The slot row is centred along each selected bend. Its repeat count is an integer derived from the bend length, slot length, bridges, margins and enabled end reliefs. A rebuild must refuse a bend that cannot fit the requested cuts; it must not silently shorten a slot, remove a margin or force one slot into an insufficient span.

## Build the example

The [native example](../examples/sheet-bend-slots-native.json) creates an 80 × 100 mm panel with two 20 mm high, 90° flanges. Its sheet rule uses 1.5 mm thickness, 1.5 mm inside radius and K-factor 0.5. These are geometry test inputs, not a calibrated material or bend allowance.

The example is self-contained and uses only demonstration geometry and names. Use generic demonstration parts for shared screenshots and reproductions.

Run the example in a new, separate CLI design using a binary built from this branch:

```sh
solidcraft-cli run examples/sheet-bend-slots-native.json \
  --param-check strict --save /tmp/native-bend-slots.sc
solidcraft-cli snapshot /tmp/native-bend-slots.sc \
  --out /tmp/native-bend-slots.png --width 1000 --height 700 --no-grid --no-axes
```

The example starts with a new design. Do not use `--in` with an existing assembly when running it. No STEP export is needed.

The verified default layout has seven slots per bend, fourteen slots in total, and four end reliefs. It follows from an 80 mm span, 7 mm overall slot length, 3 mm bridges, 2 mm end margins and reliefs reaching 3.5 mm inward in rounded mode. Each centred row occupies 67 mm, leaving a 3 mm bridge between its nearest slot and each relief.

## Select and edit

The desktop uses the same **Edges** selector as Chamfer: viewport picks, selected-count chip, Clear control and standard highlights. A new tool opening preselects the currently untreated bends once. It does not keep an automatic “all remaining” rule. The custom numbered-bend checkbox list is not used.

Picking either eligible inner or outer edge of a bend selects that bend. Multiple picked edges of the same bend resolve to one saved bend reference. Face picks expand to their eligible bend edges. Unrelated or ambiguous edges are refused. Manual picking needs the folded sheet: an unfolded bend usually has no real B-rep edge inside the flat face.

A newly created feature always stores explicit logical bend references, including when its default selected every currently available bend. Changing an earlier Bend Slots feature can free other bends, but those newly free bends are not added to an existing later feature. Reopening the editor restores only that feature's saved selection. Clearing the selection leaves the tool waiting for new picks; it does not repopulate it.

The command fields are retained by the native feature. Open **Edit Feature** or change its existing model parameter expressions. In the example, the named `demo_*` parameters drive the panel, flanges and cut sizes. Change those parameters instead of adding transformations or rerunning the creation command.

For programmatic edits, inspect the saved feature with `timeline.inspect` and use `timeline.edit` on fields present in that feature's retained definition. Use `timeline.redefine` with the same feature ID and `sheet.bend_slots` parameters to add an optional field such as an independent `relief_depth`. Both operations edit the existing feature. Get the actual feature ID and field names from the running design; do not assume an ID from a different file.

To find current bend tags, run `sheet.flat_pattern {"body":"Slotted sheet"}` and read `bends[].tag`. Supply the tags as a list, for example `"bends":["F3.0"]`, to select only those bends. Split span rows can repeat one tag; keep one copy of each selected tag. The saved reference combines its original tag with a source certificate; transient edge indices are not retained. Conversion traversal can renumber current tags when the flat frame changes. Bend Slots resolves its certified physical source to the current tag without changing the conversion's established names or other downstream references.

Saved selections retain a structural source certificate as well as the producing feature, bend ordinal and captured bend count. The certificate identifies the selected source and its child path without binding the selection to world coordinates or dimensional values. A changed or uncertified source requires explicit selection review. Reordering or replacing producer inputs must preserve the physical bend identity or fail; the evaluator must never silently cut another bend because its ordinal happens to match.

The editor uses the same certificate checks as the evaluator. An unavailable saved selection stays unresolved when the editor opens, and Apply does not silently adopt a changed producer. Clear and deliberately reselect the intended bend edges to review a changed source. Older experimental references without a certificate also require that review. New commands capture concrete current references at creation time.

The sheet's treated-tag record is derived from successful earlier features. Suppression, deletion and rollback rebuild that record from history. Old cached sheets without this provenance are rejected and rebuilt, rather than being treated as if all their bends were unused.

Undo restores the prior feature and geometry. Redo reconstructs the same result. Native save/reopen preserves expressions, selection, profile, relief settings and folded state without rerunning a script or loading a plugin.

## Supported geometry and limits

Source certification currently covers named ordinary edge flanges and their chained bends, plus named conversions with a unique mapping back to each selected input bend. The source history can include ordinary base/sketch/new-body extrusion and box/shell construction, bounded links to sketch entities or fixed axes/origins, and bounded sketch patterns. Source base/extrusion profiles currently require **All**; best-match profile fallback is not accepted as proof of identity. A certificate records topology and typed references rather than dimensional values.

An ordinary edge flange's selected source must have one unique named straight edge before the flange is made. Sources with openings or retained machining records at that stage are conservatively refused because a name can identify several surviving stretches. Unnamed notch edges and heuristic pick fallbacks do not establish source identity. Later clear through-openings can remain in the sheet subject to the material checks below.

Segmented, contour/path and point-only fold producers currently have no certified child mapping. Source histories with uncertified profile picks, thin or boolean extrusions, model-edge/face/vertex projections, sketch offsets, free-form offsets, wires or equations are also refused. Model-geometry projections need verified resolved-hit provenance before they can establish identity. The command and editor report that selection needs review or is unsupported; they do not replace missing identity with a coincident edge. Expanding these families needs dedicated source-identity tests.

Each reference has bounded ancestry and certificate data: at most 64 ancestry steps and producers, 2,048 structural records and 128 KiB of identity text. Cached sheets lacking the required source provenance rebuild before use. These are processing limits, not geometric tolerances.

The first implementation targets straight, uninterrupted sheet-metal bends with a usable full-width span and one connected flat material region. The planner enforces these bounds before generating cuts:

- At most **64 selected bends** and **512 generated cut loops**, counting slots and reliefs together.
- Manual edge discovery supports analytic cylindrical bends through **180°**, at most **256 source bends** and **16,384 edges/faces**. Its exact curve certificate is bounded to **128 controls** and **160 knots** per curve.
- At most **4,096 boundary segments** across the material outline and its existing openings.
- Every length must be finite, nonnegative and less than **100,000,000 mm**. Bend endpoint coordinates must be finite and have absolute values below that limit.
- Slot width and bridge must each be at least **0.001 mm**. Overall slot length must exceed width by at least **0.001 mm** in either profile.
- With end reliefs enabled, relief depth must be at least **0.001 mm** and at least the entry corner radius. A zero entry radius is allowed with an explicitly positive relief depth; omitting depth while radius is zero does not satisfy this rule.
- The selected bends must fit at least one complete slot while retaining the requested end margins. Generated cuts may not touch one another, an existing opening or an unintended sheet boundary.

End-relief mouths require straight exterior edges across their full height. Multiple disconnected material regions, interrupted bend spans, lost selections and cuts that separate the sheet into additional pieces are refused. The operation retains the current sheet's folded state and all pre-existing material records.

Exact material certification currently supports straight lines and circular arcs. Flat boundaries containing free-form curve segments are refused; this first implementation does not approximate those boundaries to certify a cut.

Place the feature before any retained pocket, edge-bore or perforation field. Those records are conservatively refused as inputs in the first implementation. Existing ordinary through-openings are preserved only when they remain clear of the reliefs and the full span of each slot-width band along a selected bend, including the bridges between slots.

Interrupted bends, existing openings that cross the proposed cuts, intersecting reliefs, disappearing bend selections and topology that cannot retain a continuous material bridge must produce a clear refusal. The implementation must not guess a new bend, silently omit a selected bend or cut unrelated material. More general interrupted-bend support is outside the initial contract.

A valid upstream sheet is required. A failure in an upstream shell, offset or sheet conversion remains that feature's failure. These generation limits prevent unbounded geometry; they are not manufacturing tolerances or strength guidance.

## Validation

The regression suite uses generic fixtures and checks these behaviors:

- Kernel and operation tests cover analytic slot/relief areas, both profiles, rotated frames, budgets, collisions, concave boundaries and openings between slots.
- Document tests cover retained sheet records, folded/unfolded state, interruption refusals, bend-tag parsing, cached treatment provenance, exact edge discovery and typed-reference remapping.
- Engine tests cover one-feature history, exact cut volume, explicit selection, missing selections, source-identity and bend-count changes, width/slot edits, undo/redo, suppression and JSON reopening. A converted U-channel test changes its longest edge and turns the flat frame through 90° while retaining the selected source bends.
- Editor tests cover the standard Edges selector, default unused-bend selection, edge deduplication, edit restoration and unresolved references. Creation and restored-edit selections are checked when the upstream producer changes while the dialog is open; explicit Clear and reselection are the review action.
- A three-bend CLI history check creates a first feature on two bends and lets a second feature capture the remaining bend. Removing a bend from the first feature leaves the second feature's saved selection unchanged. A newly created third feature captures the released bend. Every phase saves and reopens a native `.sc` file and remains a closed solid.
- Nine CLI cases preserve four total features and closed solids: default dimensions, a narrower panel, wider throats, a changed bend angle, a larger bend radius, rectangular ends, slots without reliefs, independent relief depth and zero entry radius. Every case saves and reopens a native `.sc` container with the same feature definition and volume. Removed-volume errors against independent analytic cut areas are below 0.002 mm³ for these fixtures, with K-factor 0.5.
- A separate sheet-rule edit changes thickness and inside radius to 2.5 mm. All four features rebuild, the solid remains closed and fourteen slots remain. Deleting the native feature restores the upstream volume.

The guide and fixtures contain demonstration geometry only. Build logs, native test files and captured images are review evidence outside the source tree. Run `cargo xtask ci` before committing, and retain its result with the change; the focused checks above do not substitute for the repository-wide gates.

Physical bend force, springback, strength, kerf and fatigue still require a material-specific manufacturing coupon. Geometry tests do not establish those properties.

## Representation boundary

The exact 2D planner takes material regions and bend spans independently of the app's sheet representation. The current adapter reads the evaluated flat-pattern sheet. A replacement sheet representation must preserve the same source-identity checks, exact cut certification and folded/flat behavior, with renewed geometry and selection tests.
