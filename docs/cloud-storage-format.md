# Cloud repository format

This specification describes the unreleased `solidcraft-cloud/1` format implemented for #4838.

## Projects inside repositories

A connection selects a repository and branch. A project is a portable folder inside that repository. Several repositories may be connected simultaneously, and each repository may contain several projects. Every editor binding and pending save carries the complete destination: `{connection, project, file}`. Changing a default destination never redirects an existing binding or queued save.

For example, a repository can contain:

```text
Robot/
  .gitattributes
  solidcraft-project.json
  parts/
    bracket/
      document.json
      manifest.json
  assemblies/
    drive/
      document.json
      manifest.json
  docs/
    README.md
  objects/
    ab/
      <64-character stored-chunk hash>
Enclosure/
  .gitattributes
  solidcraft-project.json
  ...
```

`solidcraft-project.json` contains `format: "solidcraft-cloud/1"`, `required: ["projects/1"]`, and a project `name`. Compatible optional fields are retained. An unsupported format or required project capability requests an upgrade before any project writes. Parts and assemblies use the same document representation; the directory organization does not change CAD semantics. Documentation remains ordinary Git files. Discover designs through their manifest paths rather than rewriting a central catalog for every save.

Object paths in a design manifest are relative to the project folder, so moving an entire project folder keeps its assets together. Shared chunks deduplicate across designs within that project. Paths must have portable meanings in browser storage, Git trees, Windows, and Unix: absolute paths, traversal, backslashes, control characters, reserved device names, and `.git` components are rejected. Adapters must also reject symlinks in managed paths rather than following them outside a project.

Each save includes project-local `.gitattributes` rules for the marker, design JSON,
immutable objects and the attributes file itself. These managed paths use
`-text -filter -ident -working-tree-encoding` so ordinary checkouts preserve their
hashed bytes, including with `core.autocrlf=true`. Ordinary project documentation
keeps the repository's own rules. Existing differing project attributes are
preserved and require reconciliation before a new save. Native publication checks
the captured document and object hashes in the actual Git commit before pushing,
including retries, so higher-priority filters cannot publish transformed data.

## Snapshot envelope

A design snapshot has a readable `document.json` and a JSON manifest. The manifest contains `format`, `required`, `document_hash`, and `assets`. The current required capabilities are `sha256` and `chunks/1`. Unknown optional fields are retained at manifest, asset, and chunk levels. The optional `title` is a bounded display name for local version choices; the document remains authoritative for its name.

Readers inspect the storage marker and required capabilities before parsing evolving payloads. Unknown formats, required capabilities, and codecs produce an upgrade-required error. The same check is mandatory before any write, including retrying a previously queued save. Missing or malformed markers are invalid input. Opening never modifies a repository.

The document retains its own `solidcraft/N` version and migrations. A cloud-storage version does not replace document validation. Native exports continue through the native container writer.

This payload is currently a design. Separate drawings planned in [#3890](https://github.com/tangency-labs/solidcraft/issues/3890) keep their own `solidcraft-drawing/N` schema; they must not be passed through a design decoder. Supporting another file kind requires an explicit required capability or a new envelope version, including an explicit payload path when it differs from `document.json`. Optional metadata cannot change file semantics. See the [integration plan](cloud-collaboration.md) for relative links, consistent source revisions, and future saves spanning several files.

## Assets and chunks

An asset records its SHA-256, decoded byte count, and ordered chunks. Package names are relative paths without empty, dot, parent, or backslash components. An asset is at most 512 MiB; the total decoded asset budget is 768 MiB. There are at most 10,000 assets. Document and manifest input are each limited to 16 MiB.

Chunks contain at most 8 MiB of decoded bytes. Their descriptors contain `hash` (stored bytes), `decoded_hash`, `decoded_bytes`, and `codec`. Codec values are `raw` and `zstd`. Zstd writers use level 3 and retain raw data when compression does not shrink it. Readers bound both the output and Zstd window to 8 MiB.

Stored chunk bytes live at `objects/<first-two-hash-characters>/<hash>`. Hashes are lowercase SHA-256 hexadecimal strings, independent of Git's object hash algorithm. Every object must be reachable through a Git tree entry; a hash mentioned inside JSON alone does not retain a Git object.

Readers verify stored bytes, decoded lengths and hashes, the full reconstructed asset hash, and document hash before adopting a design. A parameter-only change keeps the same asset objects. Regenerated geometry caches are not authoritative cloud data.

## Optional thumbnails

`manifest.json` may contain a `thumbnail` field with `format: "solidcraft-thumbnail/1"`,
`document` equal to the exact `document_hash`, and an `asset` descriptor using the
same chunk addressing and codecs as other assets. A preview is a 240 × 150 PNG,
limited to 256 KiB decoded and one chunk. Its descriptor is limited to 4096 bytes.
It lives outside the authoritative `assets` map: a missing, corrupt, stale or
unsupported optional preview is a cache miss and does not prevent opening the CAD
payload. Parent envelope and document compatibility are checked first.

Writers capture available geometry without forcing evaluation or meshing. Objects
remain immutable and shared by hash. Readers fetch only the document metadata and
preview object for listing, independently of large imported assets. Native exports
keep their existing thumbnail encoding.

## Publication and evolution

### Recoverable design management

The required `lifecycle/1` capability permits a `lifecycle` manifest field. Queue
captures encode `{"kind":"rename","file":"parts/new-name"}`,
`{"kind":"delete"}`, or
`{"kind":"restore","commit":"<full Git SHA>","file":"parts/old-name"}`.
Readers check the capability before interpreting these intents. Evolving their
semantics requires a new required capability.

A Trash revision keeps the document and immutable assets and writes a delete
tombstone into its manifest. Ordinary listing and opening skip these designs;
the Trash browser lists them for restoration. A rename atomically publishes an
active design at the new path and a source tombstone with `moved_to`; source
tombstones from renames are excluded from Trash. Occupied targets and changed
expected parents stop the action before publication.

Active renamed and restored manifests omit the transient intent. Rename records
up to 16 previous project-relative paths in `previous_paths`; restore records its
source commit and path in `restored_from`. Historical contents are captured and
published as a new child of the current revision, retaining earlier Git history.
Management uses the same durable outbox, asset validation, publication receipts
and restart recovery as an ordinary save. Opening never changes these records.

Conflict copies use the reserved project-relative path
`conflicts/<original-path>-<first-12-hex-digits-of-captured-revision>`. Readers derive
the original path from this convention, including copies created on another device.
An explicit resolution capture requires `conflict-resolution/1` in addition to
`lifecycle/1`, and stores `{"kind":"resolve","file":"parts/original",`
`"revision":"<observed-original-revision>","keep":"local"}` (or `"remote"`). Its
ordinary expected parent identifies the observed conflict copy. Both heads are
compared within the same repository tree. One commit writes the copy's recoverable
delete tombstone and, for keep-local, the replacement original. Published manifests
use ordinary lifecycle metadata; the resolution capability belongs to the queued
intent. A retry recognizes the source tombstone's revision and never republishes
an old replacement over a later edit. Desktop cache writes precede the receipt and
queue removal; the browser commits both cache heads and acknowledgement in one
IndexedDB transaction. Existing immutable assets are reused.

Upload immutable objects first, then publish the document and manifest together in one Git commit. Update the branch without forcing. A changed parent requires checking the target design again; unrelated changes may be retried, while competing design edits must be preserved.

Local and browser queues must retain the requested snapshot until publication is confirmed. A pending record includes its full destination, original connection endpoint (without credentials), and expected prior revision; offline children depend on their captured parent revisions and are published in that order. After a parent is published as a conflict copy, its queued children follow that confirmed destination without changing their captured identities or touching the competing original. A later competing edit at that destination produces another recoverable conflict copy. Credential renewal is allowed, but changing a connection's repository or branch cannot redirect previously queued designs. Queues are isolated by repository connection. Interrupted publication must not be reported as synchronized. A migration creates a new recoverable revision rather than rewriting history. Upgrade recognition begins with the first cloud-capable release.

Publication receipts and offline catalogs are machine-local recovery metadata,
outside this repository format. Native `solidcraft-native-publication/1` records
retain the captured snapshot and its confirmed commit and destination. Cache
replacement precedes the immutable receipt, which precedes queue removal; retries
recognize completed revisions without republishing or replacing a newer cached
head. Browser receipts, cache updates and queue removal commit together in one
IndexedDB transaction. Both adapters reject future local metadata formats before
replacement. See [the implementation guide](cloud-saves.md#native-publication-catalog).

## Local connection and queue versions

Local connection envelopes support `solidcraft-connections/1` and `/2`.
A configuration containing a verified native `git_remote` uses `/2`; it records
the resolved fetch URL and the push URL list. Captures carrying those endpoints
use `solidcraft-outbox/2`. Unpinned captures retain `solidcraft-outbox/1` and their
existing revision identities. Both queue versions can be recovered by this reader.

Readers that support only `/1` recognize `/2` as an upgrade requirement before
using its data. Reading older records preserves their bytes; an explicit settings
update atomically publishes the newer local envelope when required. These local
records use the existing bounded, atomic settings and durable queue mechanisms.
Endpoint pins become part of the captured connection identity, so changing or
removing them cannot redirect that queued capture. Credentials are represented by
local identifiers. Repository snapshots continue to use `solidcraft-cloud/1`.


Native publication receipts with redirected offline children use
`solidcraft-native-publication/2`. They retain the confirmed parent publication,
checked against the captured expected revision, connection and project. `/1`
receipts remain readable; readers supporting only `/1` request an upgrade before
using `/2`. This is local acknowledgement metadata; the Git snapshot envelope and
queued revision identities remain unchanged.

Local cache records may carry an optional `outdated` boolean (default `false`). It
records an observed mismatch with the repository and is updated only if the cached
head still matches the observed revision, document and manifest. It survives restarts
and failed pulls; a verified replacement clears it. This is device-local recovery
metadata and is never written into the shared repository format.
