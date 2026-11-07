# Small versioned drawing fixture

`C0/` and `C1/` are complete logical input trees for one drawing with two source designs. They are not actual repository commits or formal released revisions. The future acceptance harness creates the commits in a disposable repository and binds the drawing through the real writer.

## Native sources

Each source is one original rectangular box at the origin in millimetres. Source JSON follows the existing `solidcraft/1` shape used by the repository's corpus: one `box` feature, `new_body`, feature ID 1 and `next_id` 2. IDs are local to each design. Keeping them unchanged between C0 and C1 preserves the source feature identity.

The inputs intentionally use an existing legacy format, not a newly guessed current format number. Opening may migrate them with the normal reader. A future runtime check must establish actual reader/evaluator compatibility before these are advertised as replay-verified native fixtures.

| Source | C0 | C1 | Shared properties |
| --- | --- | --- | --- |
| `models/plate.solidcraft` | 40 × 30 × 5 mm | 60 × 30 × 5 mm | Body `Plate`; feature `Stock`; no links or caches |
| `models/block.solidcraft` | 10 × 10 × 20 mm | 12 × 10 × 20 mm | Body `Block`; feature `Stock`; no links or caches |

The volume oracle is `length × width × height`; no geometry engine was used to obtain it. No additional imported parts, libraries, accounts or private PDM data are required.

## Drawing template

`drawings/pair.drawing-record.template.json` now uses the implemented K2 record fields from PR #3891. Its suffix still avoids claiming that the application/container integration can open it.

- `format` is the owner-defined `solidcraft-drawing/1`; the harness reads any `format_pending` names from its pinned writer.
- Four fixed example IDs use the confirmed 13-character lowercase Crockford base32 encoding, are nonzero and unique, and stay unchanged between C0/C1. A real application allocates IDs through `doc::ids`; there is no counter in the drawing.
- `documents` names a drawing source ID and the native source root component `0`; the design's component ID is not a drawing ID.
- Units are fixed millimetres; both sheets are A4 landscape, with no border, `dpi: 300`, and `excluded: {count: false, print: false}`.
- No runtime reader/serializer/container check has run here. The actual writer still owns canonical serialization and pending migrations.

## Independent mutations

The declarative `scenarios.json` describes actions for a future harness, not CLI commands. Start each A3 subcase from C0:

- **Save As:** relocate the drawing two levels below the repository root; resolve paths back to the same two source blobs.
- **Relink:** move the existing block to `models/block-renamed.solidcraft`, then explicitly relink it. There are still only two source designs.
- **Missing:** remove the plate after establishing a cached view.
- **Changed:** replace only the plate bytes with the C1 plate after establishing C0 cached views; keep the C0 block.
- **Broken internal reference:** bind a sheet to a nonexistent source ID while leaving both source files present. This is a damaged record, not an external missing-file case.

No fabricated container, view-cache binary, drawing command implementation or fake successful runtime report is included.

K2 update: examples now carry 13-character nonzero drawing IDs, sheet-level `dpi: 300`, `excluded: {count: false, print: false}`, fixed-mm units, A4 landscape, no border, and `documents: {source, component: 0}`. K3 container/historical integration remains pending; no runtime behavior is claimed. See the acceptance contract and binding notes.
