# Versioned drawing and source design acceptance

Draft contract for [#3890](https://github.com/tangency-labs/solidcraft/issues/3890), updated to CamWight's [implemented K2 fields](https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6393737624) and the pinned source of draft PR #3891. K2 provides the authored record; K3 container and historical source/session behavior remain pending. This contribution implements none of those APIs and reports no runtime acceptance.

## Scope and responsibility

Use one drawing and two source designs in one repository. Establish dependency discovery, historical resolution, relocation/relink behavior and deterministic authored records. The deliverable is an acceptance document and small fixtures. Drawing storage, source evaluation, view generation, UI, Git/jj integration and release policy remain with their respective owners.

CamWight owns the Drawing record/storage boundary. IDs now use the single `doc::ids` allocator: 13 lowercase Crockford base32 characters, nonzero and unique across drawing objects. jyviko's agreed release model uses a protected jj tag on a commit, with the printed revision row recording that tag; release implementation remains outside this contribution. `C0` and `C1` below are fixture commit labels, not released part revisions. No repository identity, commit hash or source fingerprint is added to `sources`.

## Fixture layout and distinguishable states

Each state has this layout after the future harness binds the drawing template:

```text
repository/
  drawings/pair.drawing.json
  models/plate.solidcraft
  models/block.solidcraft
```

The drawing has two sheets: one documents the plate's root component and the other the block's root component. Its source paths are `../models/plate.solidcraft` and `../models/block.solidcraft`.

| State | Drawing name | Plate dimensions, mm | Block dimensions, mm | Analytic volumes, mm³ |
| --- | --- | --- | --- | --- |
| C0 | Versioned pair initial | 40 × 30 × 5 | 10 × 10 × 20 | Plate 6000; block 2000 |
| C1 | Versioned pair changed | 60 × 30 × 5 | 12 × 10 × 20 | Plate 9000; block 2400 |

Both sources change at the same paths, so accidentally combining an old drawing with current source files is observable. Source feature identities and logical drawing/source/sheet identities are retained between states. Values are simple rectangular-solid calculations, not measurements from a CAD run.

## A1 List dependencies without evaluating sources

**Given** the drawing in plain-text form and, when K3 exists, its container form.

**When** the dependency reader lists `sources`.

**Then**:

- Return both source IDs and the exact relative paths, interpreted relative to the drawing's folder with `/` separators.
- Do not open, deserialize or evaluate either source design, or initialize geometry solely for this operation. Reading the drawing's authored record is allowed.
- Repeat with both source files absent: dependency listing still returns the declared references. Later source resolution may report them missing.
- Record source-open and evaluator invocation counts; both are zero. This must use the real reader, not a fixture script that extracts JSON itself.

## A2 Resolve a historical drawing and both sources together

**Given** C0 followed by C1 in a disposable repository, with the current branch at C1.

**When** a historical-open operation requests C0.

**Then** resolve the C0 drawing and both source blobs from C0 through their relative paths. The drawing name is `Versioned pair initial`; the plate is 40 × 30 × 5 and the block 10 × 10 × 20. Opening C1 yields its changed name and dimensions instead.

Record the requested commit, the resolved path/blob identities and the original source bytes. If geometry is evaluated, check the dimensions/volumes in the table too. File/blob identity is mandatory; a plausible rendered picture alone does not prove historical resolution.

Repeat with the working directory's plate replaced by unrelated bytes: opening C0 must still use the C0 source. If a required source is unavailable in the requested historical tree, report it missing; never substitute the current working copy or another revision. The operation must not overwrite or modify that working directory.

The repository/storage layer selects the revision. The drawing record continues to hold `{id, path}` only; this case does not introduce per-source versions or a new PDM API.

## A3 Save As, Relink and honest cached-view state

Run each subcase independently from C0; do not carry mutations between subcases.

| Action | Required outcome |
| --- | --- |
| Save As to `copies/review/pair.drawing.json`, keeping the same models | Source paths become `../../models/plate.solidcraft` and `../../models/block.solidcraft`; both still resolve to the intended models. The original drawing and source files remain unchanged. |
| Move `models/block.solidcraft` to `models/block-renamed.solidcraft`, then explicitly Relink that source | Only the selected source path changes. Sheet references remain associated with the block source; no view silently switches to the plate. IDs/references remain internally consistent using the agreed representation. |
| Remove the plate after a view was successfully generated from C0 | The affected view is `lost`, never `current`. Any retained old image is identified as historical/lost. Opening and listing the authored drawing still work. |
| Replace only the plate with its C1 bytes after generating C0 views | The affected view is `out of date`, never `current`. A successful explicit update, or the agreed automatic update, may establish a fresh result. This contract does not choose an update mode. |
| Generate a real baseline view, verify its derived cache exists, then remove that cache and reopen | Fail setup if no baseline cache was generated. Record baseline generation/cache existence and removal. Reopening preserves authored drawing/source/sheet IDs and references. Observe regeneration or the actual provider's agreed no-cache/pending state; never claim that the removed cache is current, and do not invent a serialized status. |
| Break an internal sheet-to-source reference while leaving its path present | Report a damaged drawing; do not silently allocate a replacement ID, repair the reference or retarget it to a look-alike source. This differs from a valid source reference whose external file is missing. |

Cache provenance belongs to `cache/` or the future session mechanism, never to authored `sources`. View records and cache encodings are later work: the fixture describes the observations required when those APIs exist, not a fabricated cache file or currently executable view test.

## A4 Deterministic authored text and round trips

Use `Drawing::from_json`, `to_json`, `validate` and `allocate_id` from the pinned K2 implementation. The examples now spell `solidcraft-drawing/1`, fixed millimetres, A4 landscape, no border, root component 0, and the confirmed IDs and sheet fields. Read `format_pending` from that build's `DRAWING.pending_names`; it is omitted in the format-1 examples, not invented here. K3 container checks remain pending its real writer.

For both plain JSON and the container:

1. Save, open and save again without an authored edit. The bytes must match, including the container form as required by K2; nondeterministic archive metadata is not an exemption.
2. Verify fixed key order and number formatting through the real writer. The indentation/order in this hand-authored template is not the canonical serialization specification.
3. Verify that authored `sources`, `sheets` and their ID references survive, and that drawing `properties` follow `document.metadata` preservation rules.
4. Generate or refresh a derived view cache without an authored edit. The plain text record remains byte-identical. A container may hold new `cache/` bytes, but those never appear in the authored text form.
5. Remove `cache/`: authored records and references remain intact. No saved model snapshot, rendered image, source fingerprint or last-seen source state is inserted into `sources`.

The fixture stays within K2's stated caps: two sheets, two sources, short UTF-8 names and relative paths. The implementation still owns validation of the full limits; this slice does not add a large limit-testing matrix.

## Evidence and pending bindings

Each future executable case records its implementation commit, API entry point, fixture hashes, observed outputs and pass/fail result. A1 needs reader/evaluator counters; A2 needs historical blob/path evidence; A3 needs source/view observations; A4 needs actual serializer bytes. Static JSON parsing and analytic arithmetic are preparation checks, not evidence that Drawing behavior passes.

K2 field spellings and ID encoding are confirmed in PR #3891. Before executable tests, pin its actual head and obtain K3 container and later dependency/source/view/session APIs. Use the same logical IDs across C0 and C1. Printed revision labels, protected tags, cross-repository links, choosing latest components, conflict merging and resuming historical work are outside this acceptance slice.

This bounded contribution is published as issue #5428 and draft PR #5430 targeting `drawings`, referencing #3890. It must not claim to close the parent storage issue or to implement the four behaviors.

## Confirmed K2 record checks

These are declarative expectations for the actual K2 reader, not a parallel parser implementation.

- Every source and sheet ID has exactly 13 lowercase Crockford base32 characters, is nonzero and fits u64. Refuse uppercase/alternate spellings, zero, duplicate IDs and unresolved internal source references; never repair them or save an allocation counter. Fixture IDs are retained between C0/C1. Native design component `0` remains valid and is a different ID domain.
- Sheet `dpi` defaults to 300 and accepts 25 through 2400 inclusive; it belongs to the sheet, not to each view. Refuse out-of-range or nonfinite values. No shaded renderer is exercised here.
- `excluded: {count, print}` controls sheet count and print/export-all respectively; it does not filter a BOM. Check all four boolean combinations independently.
- Boundaries: 1..1000 sheets, 0..64 sources, names 1..256 UTF-8 bytes, paths 1..4096 UTF-8 bytes without NUL, drawing JSON at most 16 MiB. The actual reader must accept valid boundary values and refuse invalid values immediately outside each limit; fixtures do not allocate large test trees.
- A new ID comes from the shared allocator and never reuses a removed object's ID during that run. Read/save/read preserves identity; repeated save of the unchanged authored record yields identical bytes.
- Missing external source files remain distinct from a damaged authored reference. K2 can validate the authored record without opening a model. Dependency-reader invocation counters in A1 still require the actual integration API.

Source binding: `crates/doc/src/drawing.rs`, `drawing_tests.rs`, `ids.rs` and `crates/geom/src/drawing_sheet.rs` at K2 commit [`cef6b585e988a526bb69cef29e521a17323b1371`](https://github.com/CamWight/solidcraft/commit/cef6b585e988a526bb69cef29e521a17323b1371), reviewed in draft PR #3891. K2 is still a draft branch: these examples do not claim its merge, container availability, or passing SolidCraft runtime checks.
