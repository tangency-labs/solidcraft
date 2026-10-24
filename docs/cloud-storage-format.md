# Cloud repository format

This specification describes `solidcraft-cloud/1`. The implementation is under development in #4838; browser and desktop workflows are not released yet.

## Projects inside repositories

A connection selects a repository and branch. A project is a portable folder inside that repository. Several repositories may be connected simultaneously, and each repository may contain several projects. Every editor binding and pending save carries the complete destination: `{connection, project, file}`. Changing a default destination never redirects an existing binding or queued save.

For example, a repository can contain:

```text
Robot/
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
  solidcraft-project.json
  ...
```

`solidcraft-project.json` contains `format: "solidcraft-cloud/1"`, `required: ["projects/1"]`, and a project `name`. Compatible optional fields are retained. An unsupported format or required project capability requests an upgrade before any project writes. Parts and assemblies use the same document representation; the directory organization does not change CAD semantics. Documentation remains ordinary Git files. Discover designs through their manifest paths rather than rewriting a central catalog for every save.

Object paths in a design manifest are relative to the project folder, so moving an entire project folder keeps its assets together. Shared chunks deduplicate across designs within that project. Paths must have portable meanings in browser storage, Git trees, Windows, and Unix: absolute paths, traversal, backslashes, control characters, reserved device names, and `.git` components are rejected. Adapters must also reject symlinks in managed paths rather than following them outside a project.

## Snapshot envelope

A design snapshot has a readable `document.json` and a JSON manifest. The manifest contains `format`, `required`, `document_hash`, and `assets`. The current required capabilities are `sha256` and `chunks/1`. Unknown optional fields are retained at manifest, asset, and chunk levels.

Readers inspect the storage marker and required capabilities before parsing evolving payloads. Unknown formats, required capabilities, and codecs produce an upgrade-required error. The same check is mandatory before any write, including retrying a previously queued save. Missing or malformed markers are invalid input. Opening never modifies a repository.

The document retains its own `solidcraft/N` version and migrations. A cloud-storage version does not replace document validation. Native exports continue through the native container writer.

This payload is currently a design. Separate drawings planned in [#3890](https://github.com/tangency-labs/solidcraft/issues/3890) keep their own `solidcraft-drawing/N` schema; they must not be passed through a design decoder. Supporting another file kind requires an explicit required capability or a new envelope version, including an explicit payload path when it differs from `document.json`. Optional metadata cannot change file semantics. See the [integration plan](cloud-collaboration.md) for relative links, consistent source revisions, and future saves spanning several files.

## Assets and chunks

An asset records its SHA-256, decoded byte count, and ordered chunks. Package names are relative paths without empty, dot, parent, or backslash components. An asset is at most 512 MiB; the total decoded asset budget is 768 MiB. There are at most 10,000 assets. Document and manifest input are each limited to 16 MiB.

Chunks contain at most 8 MiB of decoded bytes. Their descriptors contain `hash` (stored bytes), `decoded_hash`, `decoded_bytes`, and `codec`. Codec values are `raw` and `zstd`. Zstd writers use level 3 and retain raw data when compression does not shrink it. Readers bound both the output and Zstd window to 8 MiB.

Stored chunk bytes live at `objects/<first-two-hash-characters>/<hash>`. Hashes are lowercase SHA-256 hexadecimal strings, independent of Git's object hash algorithm. Every object must be reachable through a Git tree entry; a hash mentioned inside JSON alone does not retain a Git object.

Readers verify stored bytes, decoded lengths and hashes, the full reconstructed asset hash, and document hash before adopting a design. A parameter-only change keeps the same asset objects. Regenerated geometry caches are not authoritative cloud data.

## Publication and evolution

Upload immutable objects first, then publish the document and manifest together in one Git commit. Update the branch without forcing. A changed parent requires checking the target design again; unrelated changes may be retried, while competing design edits must be preserved.

Local and browser queues must retain the requested snapshot until publication is confirmed. A pending record includes its full destination, original connection endpoint (without credentials), and expected prior revision; offline children depend on their captured parent revisions and are published in that order. Credential renewal is allowed, but changing a connection's repository or branch cannot redirect previously queued designs. Queues are isolated by repository connection. Interrupted publication must not be reported as synchronized. A migration creates a new recoverable revision rather than rewriting history. Upgrade recognition begins with the first cloud-capable release.

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
