# Design containers, version 1

New designs use `.sc`; `.solidcraft` and `.solvecraft` remain supported legacy extensions. Opening detects the ZIP
local-header signature (`50 4b 03 04`) independently of the name; otherwise it uses
the existing JSON design reader and migrations. Plain JSON remains a fully supported,
self-contained input/output format. `io::write_design` keeps its JSON contract;
`io::write_design_container` is the fallible container writer.

Packages have `container.json` with `{"format":"solidcraft-container/1"}` and
`document.json` with the versioned parametric document (`solidcraft/N`, the format in `crates/doc/src/format.rs`).
The independent versions let document migrations evolve without changing blob encodings.
Existing serialized document fields are retained automatically, without a second copy
of the document schema or a deep clone of embedded payloads.

Within `document.json`:

- Import features have empty `step` and a `step_blob` reference.
- Mesh imports retain each mesh's name and optional colour, with empty `positions`
  and `triangles` plus a `mesh_blob` reference.
- Canvases retain placement, format and pixel metadata, with empty `data` plus an
  `image_blob` reference. Raw image bytes replace base64 in the package.

References are `blobs/<lowercase SHA-256>.<step|mesh|png|jpeg>`, hashed over the exact
uncompressed payload. Identical content with the same encoding is written once.
Required parts, malformed references, ambiguous embedded payloads, incorrect hashes,
invalid mesh data and unsupported versions produce errors before session adoption.

Shared STEP sources use `.step-source` blobs containing a versioned entity library and
explicit maps for the original copies. The maps retain entity labels and face identities;
this is authoritative source data, not a native cache. Features that use the same source
hydrate one shared value on reopen. Legacy `.step` blobs remain supported.

## Mesh encoding

Mesh blobs begin with the eight ASCII bytes `SCMESH01`, followed by little-endian
`u64` coordinate count and `u64` index count. These count scalar values, not vertices
or triangles. Next come that many IEEE-754 `f64` coordinates, then `u32` indices.
Both counts must be divisible by three; the byte length must match exactly. Coordinates
must be finite and at most 1e9 mm in magnitude; indices must address an existing vertex.
Names and colours remain in JSON. Signed zero and full coordinate precision survive.

The default `fast` policy stores STEP and binary meshes without Deflate for faster
saving and reading. Small document metadata uses Deflate level one; PNG/JPEG are
stored directly. An explicit `compact` policy compresses STEP and mesh payloads at
level one. Readers support both policies. This deliberately favors interactive file
operations over maximum compression; compressed STEP was slower in the storage benchmark.
Use `"compression":"compact"` with `file.save` or `file.save_as` to favor file size,
or `io::write_design_container_with_compression` from Rust.

## Bounds and recovery

Input and individual expanded parts are capped at 256 MiB. Total declared expanded
parts and resolved payloads are each capped at 512 MiB, including repeated references
in the latter budget. There are at most 10,000 entries. Duplicate/ambiguous entry names,
path traversal, ZIP64 and multi-disk packages are refused. Entries are read in memory;
no archive paths are extracted to the filesystem. Exact part lengths and ZIP CRCs are
checked, along with blob hashes. Hashes detect correspondence and corruption, not trust.

Saving retains the existing temporary-file/flush/rename protocol and `.bak` backup.
Recovery reads both older JSON and new containers. Browser OPFS, downloads and recovery
use the same container reader/writer; JSON export and Save As remain available.

## Provisional saved display

Experimental engine saves with `preview: true` can include `cache/preview.json` and
`cache/preview.bin` when the evaluated
world display meshes are already warm. Ordinary saves omit this redundant display data
to avoid extra save work; it is separate from the native B-rep cache below. Saving never triangulates a cold body for this
cache. Pending occurrence moves, active sketch edits, feature errors and oversized
previews omit it. The cache is bounded to 8 MiB, 1,024 bodies and 100,000 triangles;
its manifest is at most 64 KiB. Body names/colours and contiguous binary mesh slices
use the same lossless encoding as imported meshes. Extra cache size is optional;
it is omitted if it would put the container near the file-size cap.

The manifest includes SHA-256 of the exact document metadata (whose blob references
contain content hashes), the preview payload, `solidcraft-preview/1` and the application
package version. A version mismatch invalidates the display cache. Development builds
with the same package version may briefly show an older picture before the authoritative
rebuild replaces it; previews never supply model state. Earlier draft containers using
`solvecraft-container/1` remain readable, with their obsolete previews treated as misses.
Missing, obsolete, oversized or malformed preview data is a cache miss, not a failed
design open. All archive-wide safety budgets still apply.

Desktop Open can render these meshes on its existing file worker and show a clearly
labelled saved picture in the opening dialog before timeline reconstruction finishes.
It uses only the normal command's file snapshot, without a second file read. The picture
is approximate shading without face paint or picking. A conservative 16-million-pixel
raster budget rejects excessive overdraw before rendering, including repeated large
triangles in a small hostile cache. The picture is never adopted as editable
geometry, used for measurements, or used to seed evaluated model state. The original
design remains intact until the complete rebuilt session is ready. Cancellation or
any change of active document/revision discards both preview and completion.

Desktop Open takes the evaluation from the model cache when the file keeps one for
this document and build (see below); otherwise it evaluates the timeline, as
design reading in the CLI and browser does. The saved picture is not an
interactive viewport.

## Seekable reader (phase 2, first increment)

`io::DesignContainerReader<R: Read + Seek>` accepts a container source such as a
file or a memory cursor. It inspects a bounded ZIP directory suffix and entry
headers, then `preview()` reads the optional display entries and document metadata.
`read_document()` consumes the reader and resolves and validates the required
source blobs through the same archive. No complete archive buffer is required.
The existing byte-based readers use this implementation too; JSON keeps its existing
reader. `read_design_file` integrates it with desktop Open using one file handle,
without buffering the complete container. Browser VFS uses a memory cursor.

Metadata used to validate a provisional cache is retained in an immutable buffer
and reused for authoritative reconstruction. Concurrent in-place writes therefore
either retain that document or fail required-part CRC/hash validation. Optional
preview success never validates source blobs or makes a document editable. An invalid required blob still
fails the later document read. ZIP directory scans can touch a bounded suffix of a
stored blob; deferral means not loading or decoding the complete payload, not zero
physical reads of its byte range. Optional cache metadata is capped at 16 MiB;
larger metadata misses the cache and uses the normal authoritative part-size budget.

## Model cache

Engine Save/Save As write the evaluated model with the design by default
(`cache: true`; `cache: false` leaves it out): `cache/model.json` and
`cache/bodies.bin`. Plain JSON, recovery/autosave and browser downloads do not
carry it. Saving never forces placement, evaluation or tessellation: the model is
the session's own evaluation, and only when it is the evaluation of the document
being saved (`Model::is_current`). An active sketch, uncaptured moves, feature
errors or mesh bodies leave the cache out; the design saves as before. A cache
over its budgets keeps fewer states (below) and is left out only when not even
the final state fits.

The model is the whole evaluation: every feature's result (warning, boolean
diagnostics, whether it was skipped, its fingerprint) and the state after it
(bodies, solved sketches, construction geometry, sheet metal bodies, threads and
the blend records later features read). States share what they share: a body that
later features pass through unchanged, its face names and a solved sketch are each
recorded once. Face names are kept as worked out (saving works out any not asked
for yet), so a body taken from the file is named exactly as when it was saved.
A design whose stored names predate format 14 (marked `?`) finds them through
names worked out from geometry on demand, which the cache does not keep: it saves
without a model.

Both entries are Deflate-compressed (level one) under either compression policy: the
encoded bodies shrink about fourfold (an imported assembly) to tenfold (modelled parts),
for a few tens of milliseconds.

`cache/bodies.bin` holds every distinct body once, in
[SolidCraft's native B-rep encoding](native-brep.md), as contiguous slices. The
encoding is exact: a decoded body is the body that was saved, number for number
(procedural intersection curves, placed revolutions and planes are stored as the
kernel holds them, never refitted). Identical bodies share one slice (keyed by the
SHA-256 of their encoding), and a placed copy of a repeated part (an instance from
a STEP assembly) records the part and its matrix, so it shows that part's display
again instead of being meshed on its own.

`cache/model.json` records `solidcraft-model-cache/4`, the producer revision
`solidcraft-truck-adapter/3`, the application version, the build (source commit,
CPU architecture and OS), the SHA-256 of the exact `document.json` (whose blob
references pin every STEP, mesh and image payload) and of `cache/bodies.bin`, the
slices, each kernel body's slice and display share, and the model's records. A
cache is read back only when every one of these matches: the same source compiled
for the same platform computes the same numbers, so another build's cache is a
miss. A build of unknown source (a tarball without git) never reads a cache.

The fixed budgets of earlier caches (4,096 bodies, 100,000 faces, 64 MiB) scale
with the design now: up to 200,000 body records, 100,000 distinct bodies, a 32 MiB
manifest and a payload within the container's part budget (256 MiB), leaving room
for the document. Individual bodies keep the codec's own limits. The archive-wide
budgets (256 MiB per part, 512 MiB expanded, 10,000 entries) are unchanged.
Decoding runs under `kernel::guard` and is linear in the payload's bytes.

A model whose cache would push the container past them keeps fewer states
(`SavedModel::thinned`, #3220): the final state (what the design shows), then the
other states, the slowest feature's first (the evaluations opening would otherwise
repeat for longest), each while what it adds still fits: the bodies no state kept
so far holds (a body is stored once however many states hold it) and the records
of the state, those bodies, their names and sketches. Sheet bodies, like sketches, are
written once and named by place in the states (most features leave the sheets as they were); a
thinned cache keeps all of them. When the rest of the
manifest (the bodies' table, the slices) then leaves too little room, the choice
is made again within that much less, and the final state alone is always tried.
A feature whose state is left out records none. The cache is left out (and the
save logs it) only when not even the final state fits; a thinned cache logs how
many features kept their state.

### Opening from the model

Open takes the model as the design's evaluation (`Model::adopt`) when the key
matches and every feature's fingerprint (its definition, the parameters and
imported files it reads, the features before it, the timeline marker) is the one
the opened document gives it now. Nothing is evaluated: the imported STEP text is
resolved and hashed, never parsed. Every feature is up to date, so an edit
evaluates from the first feature it changes, as after any evaluation: a feature
appended or edited at the end evaluates from the saved final state, an edit to
feature k from the saved state before k. Moving the timeline marker, undo and redo
reuse the saved results whose fingerprints come back. `SOLIDCRAFT_NO_MODEL_CACHE`
set evaluates anyway.

A thinned cache is taken the same way, and the features whose state it left out
are evaluated while opening, each from the state before it (kept or just
evaluated); the states it kept after them stand. Opening then costs the
evaluations of those features only, and the model is whole afterwards: nothing
else in the application sees a missing state. Should one of them fail now, the
features after it are evaluated too, as a full evaluation would.

Any failure (a key or fingerprint mismatch, a malformed record, a body that does
not decode) is a miss: the design is evaluated from its timeline as before, and
only that.

This replaces the earlier rule that a cache never supplies evaluated session
state. It is safe because the cache is the result of evaluating exactly this
document with exactly this build: the hashes tie it to the document (and through
it to every source blob) and to its own payload, the build key to the code that
computed it, the fingerprints to every feature's inputs, and the exact encoding
gives back the bodies number for number. Hashes are not trust: a crafted file can
carry a cache whose geometry disagrees with its timeline, as a crafted STEP file
can carry any geometry. Every decoded body passes the same structural and resource
checks, under `kernel::guard`, and Compute All (`timeline.compute_all`) evaluates
the whole timeline again.

Before the session is ready the cache still supplies the picture (a bounded mesh
of the placed bodies) shown while opening. Earlier `cache/key.json` and
`cache/brep.bin` entries are ignored.

## Review increments and kernel independence

The implementation is split into the following review increments after the
phase-one container PR (#327):

1. Seekable IO and its read-volume/hostile-input tests (the API above).
2. A versioned SolidCraft-owned B-rep schema and bounded binary codec, tested without
   constructing a kernel body. Topology references, oriented edge uses and face
   loops, geometric domains, analytic/spline curve and surface definitions, units,
   tolerances and orientation must have explicit semantics. Preserve face-source
   certificates, persistent names and history using SolidCraft-owned identities.
   No truck serialized structs, memory layouts, pointers or opaque kernel blobs
   form part of the persisted contract. No evaluated STEP round trip is a substitute.
3. Separate adapters between the neutral schema and supported kernels. Unsupported
   geometry or provenance is a cache miss. Explicitly fitted intersection splines
   carry their fitting target and remain provisional display geometry. A neutral
   file schema alone does not prove interoperability: each adapter needs round-trip,
   modelling and naming tests; compatibility with a second kernel is unverified
   until that adapter exists. Kernel/version information belongs in producer and
   capability metadata, not in the topology encoding. Conservative cache invalidation
   is allowed across kernels while authoritative document reopening still works.
4. Integrate deferred source reading and provisional B-rep display into the open
   worker with immutable-input and document/revision guards. Rebuild authoritatively
   before editing, picking or measuring. B-rep alone is not a complete evaluated
   session: sketches, construction, placements and other non-solid state still matter.
5. Measure cold first display, time until editing, save latency, size and peak memory
   on a public STEP-based design. Include cache hits/misses, plain JSON and browser
   fallback, and report baselines and limitations in `docs/perf.md`.

Default saving must remain fast: do not force cold evaluation or meshing merely to
produce a disposable cache. Do not add redundant display meshes by default; the
existing preview remains opt-in. GPU tessellation is independent work. Continue
using format/application constants rather than source-tree hashing build scripts,
retain JSON Save As/MCP/script compatibility and the documented legacy shims, and
run standalone wasm validation on each published code revision.

## Portable thumbnail

A warm save (with `cache`, the default) may add `cache/thumbnail.png` and its small
`cache/thumbnail.json` key. The picture is a 240 x 150 PNG, bounded to 256 KiB,
keyed by `solidcraft-thumbnail/1`, application version, exact document SHA-256 and
PNG SHA-256. It contains an isometric view of visible solid bodies with their
appearances. It is display-only and independent of the producing geometry kernel.

Saving uses only already placed bodies and cached meshes, at most 1,024 bodies,
100,000 triangles and 100,000 vertices. A preflight also caps projected pixel-box
work at four million supersampled pixels and face-color lookups at one million,
so overlapping triangles cannot make a small picture arbitrarily expensive.
It never evaluates or tessellates a cold
model for a picture. Active edits, pending moves, model errors, absent warm
geometry or exceeded limits omit the picture, as does `cache: false`. Browser
downloads omit the PNG. Plain JSON never contains the PNG. The same writer is used by desktop and browser saves.

The seekable thumbnail reader checks the small key, retained document metadata,
PNG size/header and hashes without resolving source blobs. The Home/Recent Files
reader tries its local picture cache, then this portable picture, then its existing
background reconstruction. PNG decoding also has dimension and allocation limits.
Missing, mismatched, truncated or malformed pictures are misses and never prevent
authoritative document opening. Embedding a PNG does not install an operating-system
file-browser thumbnail provider.

Desktop thumbnail reads seek within the file. The browser VFS supplies a bounded
in-memory buffer instead; it avoids geometry reconstruction but does not provide
range-based OPFS reads. IO checks the PNG header and hashes, while the consuming
UI performs the full bounded PNG decode.
