# Local scalar libraries

The first local-library slice (#846) reuses the existing appearance and physical
material commands. It is a small consumer of the broader [library proposal](libraries.md),
not the complete resource-pack implementation. It adds no texture mapping, wood grain,
PBR shader, HDRI lighting, online discovery or document-format version.

## Desktop workflow

1. Apply a named colour/opacity look using **Appearance**, or define a physical material
   using the existing material commands. An appearance does not change physical density.
2. In Appearance → **Local library**, choose **Export design library...**. The snapshot
   contains explicit body/face/component appearance overrides and design-local physical
   materials, including their default looks. It does not export imported kernel paint,
   unassigned built-in catalog entries or an unapplied colour-picker draft.
3. Open that library in another design, search its appearance presets, select one and
   choose **Apply library appearance**. The normal body/face/component targets are reused.
   OK after this immediate application does not add a second undo step.
4. Save the design. Its selected scalar definitions are ordinary copied design data;
   the source library can be moved or removed without changing the saved result.

Export does not edit the design, its dirty state or history. An existing file is refused
unless **Replace existing library file** is explicitly selected. Export is a snapshot of
the design definitions, not an automatic merge with entries in an existing catalog.
Editing a copied look/material uses the normal commands and does not edit its source.

The combined render integration uses main's merged #1127 asynchronous picker.
Model edits and tab changes wait for its answer while drawing/resizing continues.
Overwrite consent is captured before opening; export snapshots the design when
`library.export` runs after choosing the destination. Document identity is checked,
and an old library answer cannot replace another Appearance editor. Cancellation
does not read or write a library. The earlier #1114 candidate is no longer a runtime
dependency of the refreshed consumer.

The desktop panel applies appearances. Physical-material import/assignment is available
through `library.apply`; imported definitions also appear in the existing material list.
The browser can export a library through the existing download service. Interactive local
library opening is a desktop feature in this first slice. Browser engine commands can
consume a bounded catalog already supplied through VFS; wasm compilation does not prove
an interactive browser importer or download workflow.

## Commands

All programmatic calls use JSON and never open a file picker:

```json
{"command":"library.export","params":{"path":"my-library.solidcraft-library.json"}}
{"command":"library.inspect","params":{"path":"my-library.solidcraft-library.json"}}
```

Inspection/export returns a `catalog` with qualified entry IDs and exact revisions. Use
the returned values when applying; do not replace an unknown revision with a guessed one:

```json
{"command":"library.apply","params":{
  "path":"my-library.solidcraft-library.json",
  "entry":"user.local/appearance-<returned-id>",
  "revision":"<returned-64-character-revision>",
  "targets":{"bodies":["Body1"]}
}}
```

Appearance targets can be `bodies`, `components` or `faces: [{body, point: [x,y,z]}]`.
Points are world-space millimetres. At most 1,024 total targets are accepted. Missing
objects and invalid targets fail; the importer does not invent a nearby replacement.

A material entry can be copied without targets or assigned to body targets. The optional
`name` explicitly selects a new physical-material name on import. An identical existing
definition is reused; a conflicting definition or built-in name is refused. No density,
look or configuration cell is silently overwritten. Creation and assignment share one
outer undo transaction through the existing material commands. Mass remains the existing
volume × density calculation; a generic illustrative density is not an engineering grade.

`library.export` also accepts `id` (default `user.local`), `name` (default `My library`),
`provenance` and `overwrite` (default false). Only explicit assignments/design-local
definitions are inspected, without evaluating geometry. Duplicate exact definitions are
deduplicated; different values with the same display name remain distinct entries.

## Scalar catalog v1

The independent JSON format marker is `solidcraft-library/1`. It has `id`, `name` and an
`entries` array. Each entry has `id`, `revision`, `provenance` and a typed `definition`:

- `appearance`: `value: {name, color: [r,g,b], opacity}`; sRGB bytes and finite opacity 0–1.
- `material`: `value: {name, density_g_cm3, appearance?}`; finite density above 0 and at most
  1,000 g/cm³, with an optional appearance value of the same scalar shape.

Every object rejects unknown fields. Unsupported kinds (including texture/environment
payloads) and versions fail explicitly, rather than dropping fields or pretending that a
colour preview is a textured material. Catalog IDs are 1–64 ASCII bytes, beginning with a
lowercase letter and continuing with lowercase letters, digits, `.`, `_` or `-`. An entry
ID is `<catalog-id>/<local-id>` with the same local-ID rules; paths and labels are not IDs.

An entry revision is lowercase SHA-256 of compact v1 serde JSON `[provenance, definition]`.
Derived struct field order, omission of absent optional fields, UTF-8 escaping and f64
round-trip number formatting are part of this writer contract; this is not arbitrary JSON
canonicalization. Prefer `library.export` or the Rust `Entry::new`/`content_revision` API
when authoring revisions. A future incompatible writer changes the catalog version. The
default export IDs contain a kind and 16 hex revision characters; collisions are rejected,
never used to overwrite a different definition. Explicitly authored stable local IDs can
retain their identity across revisions. Application requires the selected exact revision.
Changing another entry does not change the selected definition's identity.

Provenance contains a required `license`, optional `author` and optional `source_url`.
It is supplied metadata, not verified permission or a publisher signature. URLs are never
fetched. The default `LicenseRef-User-Provided` does not claim that user data is openly
licensed. No external art/data is bundled in this slice. Catalog provenance/revisions stay
in the catalog: the first consumer copies scalar values into existing `Look`/`UserMaterial`
records, without a persistent catalog-origin/update receipt. Full resource embedding,
design-owned provenance, scopes and explicit update links remain proposed follow-ups.

## Bounds and failure behavior

The reader accepts at most 4 MiB encoded JSON and 1,024 entries. Entry decoding stops at
the count cap with fallible vector growth. Names are trimmed, nonempty and at most 128
UTF-8 bytes; provenance license/author/source URL limits are 128/256/2,048 bytes. Control
characters, invalid identifiers, duplicate identities, bad revisions and nonfinite scalar
values fail. Export inspects at most 100,000 source references, deduplicating into at most
1,024 entries. These are encoded/work limits, not a process-memory or latency guarantee.

Native reads are bounded regular-file reads; browser reads check VFS size before cloning.
Native no-clobber writes reuse atomic new-file publication; explicit replacement is atomic
without reading/backing up an arbitrarily large old target. All library command paths
obey the active MCP/control file scope before reading or publishing, including overwrite. Browser VFS publication is
synchronous and reuses the host's download service. No automatic discovery, asset fetch,
relative-resource resolution, executable recipe or plugin is introduced.

Failed application preserves the document and undo/redo history. The source file is read
and checked before copying; a stale selected revision needs an explicit reload/reselection.
The copied design remains independent after successful assignment, undo/redo and cold
reopening. This library has no dependency on the pending render stack and does not change
the document format. User-visible evidence and actual native/wasm results are recorded in the PR;
geometry/robustness checks remain separate and no reference baseline is weakened.

The opaque PBR consumer makes v1 export fail explicitly when any body, component,
face or material-default appearance contains PBR parameters. Neither overwrite consent
nor an existing output file permits a lossy downgrade; use the richer transport
when available. Scalar catalog imports still select the scalar response explicitly.
