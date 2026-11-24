# Local material libraries

The local library transports design-owned appearances and physical-material definitions.
Portable catalog v2 preserves opaque PBR parameters and validated engineering cards,
including SI values, material-local axes, source records and per-property conditions.
It reuses the existing commands, file pickers and [library contract](libraries.md).
The catalog has its own version; no additional design-format change is needed.

## Desktop workflow

1. Apply a named colour/opacity look using **Appearance**, or define a physical material
   using **Physical Material → New / Edit** or the material commands. An appearance does not change physical density.
2. In Appearance → **Local library**, choose **Export design library...**. The snapshot
   contains explicit body/face/component appearance overrides and design-local physical
   materials, including their default looks. It does not export imported kernel paint,
   unassigned built-in catalog entries or an unapplied colour-picker draft.
3. Open that library in another design, search its appearance presets, select one and
   choose **Apply library appearance**. The normal body/face/component targets are reused.
   OK after this immediate application does not add a second undo step.
4. To import a physical definition in the same Local library section, enable **Physical
   material definitions**, select an entry and review its density/card. **Name on import**
   makes a collision explicit: choose another name instead of overwriting existing data.
   **Import material into design** copies the definition only. Cancel or OK after copying
   keeps the import and does not assign either a material or an appearance to any body.
   Assign the copied material later in **Physical Material**. Each import is one undo step.
5. Save the design. Its selected definitions are ordinary copied design data;
   the source library can be moved or removed without changing the saved result.

Export does not edit the design, its dirty state or history. An existing file is refused
unless **Replace existing library file** is explicitly selected. Export is a snapshot of
the design definitions, not an automatic merge with entries in an existing catalog.
Editing a copied look/material uses the normal commands and does not edit its source.

Copied look labels are inert data even when equal to a `starter.*` recipe ID.
Reopening Appearance or applying the copied look to another target preserves its
color, opacity and PBR response. Library body and face targets use the same world
selections as ordinary assignment, including displaced component instances;
the outer command maps them into the source component exactly once.

The combined render integration uses main's merged #1127 asynchronous picker.
Model edits and tab changes wait for its answer while drawing/resizing continues.
Overwrite consent is captured before opening; export snapshots the design when
`library.export` runs after choosing the destination. Document identity is checked,
and an old library answer cannot replace another Appearance editor. Cancellation
does not read or write a library. The earlier #1114 candidate is no longer a runtime
dependency of the refreshed consumer.

The desktop panel applies appearances and imports physical definitions. `library.apply`
also supports explicit body assignment in the same atomic command; imported definitions
appear in the existing Physical Material list and can be edited there.
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
`provenance`, `overwrite` (default false) and `format` (default `solidcraft-library/2`).
Explicit `solidcraft-library/1` refuses PBR or engineering-card loss before publishing,
including with overwrite enabled. Only explicit assignments/design-local
definitions are inspected, without evaluating geometry. Duplicate exact definitions are
deduplicated; different values with the same display name remain distinct entries.

## Portable catalog v2

`solidcraft-library/2` extends the same envelope and definition kinds with optional
`pbr: {metallic, roughness, dielectric_ior}` on appearance values and optional
`physical_card` on material values. PBR uses the bounded opaque contract and requires
opacity exactly one. Cards use the existing SI admission and validation rules.
Sources and conditions are copied as supplied data, not certification. Card density
never replaces assigned mass density. An independent default PBR look and a physical
card may coexist in a material.

Applying an appearance is an exact replacement: v1 scalar values and v2 values without
PBR clear the target's prior PBR. V2 values with PBR retain their parameters. Display
labels are inert, including labels equal to built-in preset IDs: importing an owned
value never selects a different recipe. Undo/redo and native save/reopen preserve the
copy after removing the source file.

Absent richer fields are omitted from serialization. Existing v1 scalar bytes,
content revisions and identities remain exact; pinned appearance bytes and a material
revision cover the writer contract. Changing only the envelope version leaves entry
revisions unchanged. PBR and complete card data participate in revisions when present.
V1 wire input rejects even null v2-only fields. Older readers refuse v2.

## Scalar catalog v1 compatibility

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
in the catalog: values are copied into existing `Look`/`UserMaterial` records without a
persistent catalog-origin/update receipt. Card source records and property conditions
belong to the card and are copied exactly. Resource embedding and explicit update
links remain separate work.

## Bounds and failure behavior

The reader accepts at most 4 MiB encoded JSON and 1,024 entries. A bounded raw preflight
preserves strict v1 wire fields before typed decoding. Entry decoding stops at
the count cap with fallible vector growth. Names are trimmed, nonempty and at most 128
UTF-8 bytes; provenance license/author/source URL limits are 128/256/2,048 bytes. Control
characters, invalid identifiers, duplicate identities, bad revisions and nonfinite scalar
values fail. Export inspects at most 100,000 source references, deduplicating into at most
1,024 entries. Export bounds retained unique definitions before whole-catalog encoding,
reserving 4 KiB for the envelope and counting each entry's pretty bytes plus nested
indentation. This conservative admission may refuse a near-limit compact catalog.
Physical imports also obey the design's 1,000-material cap. These are encoded/work
limits, not a process-memory or latency guarantee.

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

## Library folders and places

A library folder holds one `*.sclibrary.json` manifest (format `solidcraft-library-folder/1`:
`id`, `name`, `provenance`) and one file per entry in `materials/` and `appearances/`. A place is
where library folders are read from: the built-in library, My Library (the `my_library_folder`
preference) and the user's places (`library_places`). A place is one library folder (`library`)
or a folder whose subfolders are library folders (`folder`), named by its whole path on this
computer or a network share (never a relative path or a device). Libraries are used in that order; a
library whose id an earlier one has is listed as unavailable, naming the first.

The app scans each place on a thread of its own: at start, when the list changes and on
`library.places.refresh`. The window never waits for a scan. A library place follows the network
rules a design's links follow. A scan reads at most:

| What | At most |
|---|---|
| The manifest | 16 KiB |
| One entry file | 256 KiB |
| Entry files in `materials/`, and again in `appearances/` | 1,024 |
| One library: the manifest and every entry file together | 4 MiB |
| Files directly in a library folder (the manifest included), and again its subfolders | 64 |
| Subfolders of a folder place | 64 |
| Places in the list, My Library apart | 64 |

A limit reached makes that library or place unavailable, with the reason; nothing is cut short
quietly.

`library.places.refresh` scans in full. With `changed_only: true` it first lists again each
folder the last scan listed, with the same limits, and scans only the places where something
differs: a file's length or time, a file or folder added or gone, or a folder that went missing,
became unreadable, went over a limit or came back. A place where nothing differs keeps its scan, and
nothing in it is read. An edit that keeps a file's length and its time isn't seen this way: a drive
that keeps time in steps (FAT: 2 seconds) can hide an edit made inside one step. A full refresh
reads it. In the browser, files keep no times, so `changed_only` scans every place.

```json
{"command":"library.places.add","params":{"path":"/srv/shop-libraries","kind":"folder"}}
{"command":"library.places.list","params":{}}
```

`list` answers from what the session holds and reads no files. `add`, `remove`, `set`
(`read_only`, `at`) and `refresh` check a change and the app applies it; without the app (the
CLI, a headless MCP server) nothing is saved. A network folder is read-only unless `read_only`
says otherwise. The place commands are app settings: a plugin can't run them, and the in-app
Assistant asks before a change.

### Making and changing libraries

```json
{"command":"library.create","params":{"place":"/srv/shop-libraries","name":"Shop"}}
{"command":"library.entry.list","params":{"library":"user.shop"}}
{"command":"library.entry.add","params":{"library":"user.shop","definition":{"kind":"material","value":{"name":"Steel","density_g_cm3":7.85}}}}
{"command":"library.entry.edit","params":{"entry":"user.shop/material-steel","revision":"<the listed revision>","definition":{"kind":"material","value":{"name":"Steel","density_g_cm3":7.9}}}}
{"command":"library.entry.copy","params":{"entry":"solidcraft.builtin/material-steel","to":"user.shop"}}
{"command":"library.entry.remove","params":{"entry":"user.shop/material-steel","revision":"<the listed revision>"}}
```

`library.create` makes a library in a listed place: in a folder place, a new subfolder named after
it; in a library place, that folder itself, which must be empty. The kind folders are made first and
the manifest last, so a write stopped part way leaves a folder a scan skips, never a broken library.
A new library's id is `user.<name>`, made unique among the libraries listed.

`library.entry.list` gives every entry file of one library, refused ones with why, and each entry's
id and revision. Like `library.places.list` it reads no files.

`add` and `copy` write a new file under a new id, `<kind>-<name>` made unique in the library, and
never replace a file. The file's name is only a label, made to work on every system. `copy` takes an
entry from any library, the built-in one too, and keeps its values and provenance. `edit` and
`remove` take the revision the caller read: the file is read again first, and a file that holds
another revision, or is gone, is refused ("list it again"). An edit keeps the entry's id; its
revision follows the content. `remove` moves the file to the library's own bin,
`.removed/<kind>/`, which no scan reads; nothing is deleted, and a name already in the bin gets
` (2)`.

Every write is one file, written whole or not at all, and never follows a link at its name. A write
is refused in a read-only place, in the built-in library, in an unavailable library, and when it
would take the library past a limit in the table above. Every path follows the network rules a
design's links follow. After a write the library is read again, as a scan reads it, so the next
command sees the write. The writes are in the plugins' "file" group, and the in-app Assistant asks
before each one.

## Scope

The catalog carries the document's scalar/opaque `Look` and engineering-card values.
It does not transport GPU2 runtime overrides, refractive glass, clearcoat, texture
resources, mappings, image files or environment maps. The six starter glass recipes
remain recipes; v2 does not turn an IOR or alpha preview into refraction. Unknown
resource kinds and fields fail explicitly. A runtime GPU-only look must not be
presented as captured by this design-definition export.
