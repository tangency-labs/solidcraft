# Cloud saves implementation

Cloud saves are being developed in #4838 and draft PR #4839. This document distinguishes implemented foundations from the remaining integration.

## Modules

`solidcraft-cloud` is a standalone workspace crate with no CAD or UI dependencies. Its default feature set needs no filesystem, credentials, or network transport. The optional `github` feature adds an Octocrab adapter whose HTTP client is supplied by the platform. Its snapshot interface prepares reusable asset objects and hydrates them through an injected object reader. Errors distinguish invalid input, unavailable storage, competing edits, and an upgrade requirement. See [the format specification](cloud-storage-format.md).

Implemented foundation tests cover asset reuse across parameter changes, chunk reconstruction, unknown versions and capabilities, unsupported codecs, optional metadata preservation, and corruption rejection. Native and wasm builds are required; Zstd's C dependency needs a wasm-capable Clang at build time.

## Dependency reuse

Existing infrastructure is preferred over new protocol, cryptographic, database, or process frameworks. The dependency audit checks the exact released version, license, required APIs, and browser compilation before adoption. A successful WASM build is not a browser restart or network integration test.

| Responsibility | Reuse decision |
| --- | --- |
| Desktop repository engine | Installed Git, preserving its transports, helpers, agent, configuration, hooks, and signing. `process-wrap` owns cancellable Unix process groups and Windows Job Objects; a narrow adapter bounds input/output and deadlines. `gix` currently lacks push and its repository crate fails the browser probe. |
| GitHub client | Octocrab 0.54.5 with default features disabled and a small browser service/executor adapter. Real typed Git Database read/write calls compile, and public Git reads executed in Chromium. The hand-written endpoint prototype has been replaced. |
| Browser queue and metadata | `indexed_db_futures` provides schema upgrades, indices, durability options, and explicit transaction completion; its required APIs passed the browser-target compile probe. |
| OPFS assets | Reuse one existing OPFS adapter or replace it with `opfs` when that removes binding code. Do not maintain two implementations of the same browser storage layer. |
| Native credentials | Existing `keyring` service with per-connection credential identifiers. |
| SSH key generation | `ssh-key` for Ed25519 and encrypted OpenSSH serialization; system Git/OpenSSH still handles existing keys and agents. Native encrypted-key roundtrip and wrong-passphrase checks passed. |
| Locking and atomic writes | Standard-library file locks and existing `solidcraft_io::safe_write`; no extra locking or atomic-write library is needed. |
| Encoding | Existing `serde`, `serde_json`, `sha2`, and `zstd`; no new serialization, hashing, or compression implementation. |

Detailed comparisons, primary-source citations, exact versions, and probe limits are recorded in the [Git audit](research/cloud-git-crates.md), [GitHub client audit](research/cloud-github-crates.md), and [platform audit](research/cloud-platform-crates.md). Browser transport lives in `apps/solidcraft-web/src/cloud_client.rs`; it keeps Fetch futures local, bounds response accumulation, and uses Octocrab's host-scoped authorization middleware. No fork, patch, unsafe wrapper, or Tokio runtime is required in the browser.

SolidCraft-specific code remains responsible for project layout, version compatibility, destination selection, capture-before-upload, durable acknowledgement, conflict copies, and reconciliation after an uncertain upload. General API or retry libraries do not decide those rules.

See [drawing and collaboration integration](cloud-collaboration.md) for compatibility with separate drawing files (#3890) and the existing seams for later collaborative work.

## Integration contract

Host setup and repository access are separate adapters. A host setup adapter supplies guided authentication, repository discovery/creation, and host settings links. The native repository adapter uses standard Git; a browser adapter uses a CORS-compatible provider interface. Adding GitLab or Gitea setup must not change the snapshot format.

Engine commands own user-visible behavior and asynchronous operation identifiers. UI modules render state and issue those commands. Credentials stay in host credential storage, never in document metadata, remote URLs, command logs, or manifests.

Desktop Git must preserve credential helpers, SSH agents/configuration, signing, unrelated staged and unstaged work, and linked worktrees. A connection-specific key override affects only its subprocess. Key generation must not overwrite existing keys. Publishing is non-forced and reports synchronization only after confirmation.

Browser integration must persist connection metadata and pending saves in IndexedDB, with assets and recovery data in OPFS. Restart recovery must retry the captured snapshot rather than silently substituting later unsaved edits. Quota and credential failures leave the requested save recoverable.

`solidcraft_io::cloud::NativeHost` runs native operations on workers and retains the caller's allowed-folder scope. Give it the app's private cloud recovery directory. Each connection has its own outbox under the SHA-256 of its identifier, preventing case-insensitive filesystem aliases. Save persists its captured objects and queue record before starting Git. Listing combines queued destinations with the checkout; opening prefers the latest queued revision and needs neither connectivity nor a working Git executable for that revision. Competing local tips remain intact and return a conflict rather than opening an arbitrary version. Synchronization retries captured revisions in dependency order; a failed predecessor blocks its children but not unrelated captures. A Save operation reports only its requested capture's publication result. The local queue/process locks are implementation coordination, separate from the future advisory editing leases described in the collaboration plan.

### Browser recovery store

`apps/solidcraft-web/src/cloud_storage.rs` implements the browser store, separately from its still-pending worker and settings integration. Each connection has an IndexedDB database containing `pending`, `cached`, and `receipts`, and an OPFS directory under `cloud/<SHA-256 of connection id>`. Stored object paths match those used by the outbox and repository snapshots. The shared OPFS adapter bounds reads before allocating buffers, writes owned browser bytes, and awaits stream close. It aborts failed writes; a retry can complete an empty file left by interrupted initial creation.

Local Web Locks serialize storage changes across tabs, with no network waits inside those locks or IndexedDB transactions. Queue and cache metadata are committed with strict durability after all object writes finish. Publication confirmation updates its receipt and cache location and removes the pending record in one transaction. Confirming a parent never replaces a newer queued child; a conflict copy is cached under its actual destination. Opening a newer local database, queue, cache, or receipt format requests an upgrade. Queue listing is bounded to 10,000 records and 128 MiB of metadata.

A failed commit acknowledgement does not prove no record exists: a browser can finish committing while the caller receives an error. Do not report that capture as saved; retain and recognize its stable identity on retry. Persistent-storage denial is recorded in `Store::persistent` for the host to display; it does not masquerade as granted persistence.

The `cloud-recovery` example exercises the actual store without starting the renderer. With a matching `wasm-bindgen` CLI installed, run:

```sh
cargo build -p solidcraft-web --example cloud-recovery --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir /tmp/solidcraft-cloud-browser-site \
  target/wasm32-unknown-unknown/debug/examples/cloud_recovery.wasm
cargo xtask cloud-storage-test --site /tmp/solidcraft-cloud-browser-site
```

Set `CC_wasm32_unknown_unknown` to a WASM-capable Clang when required by Zstd. The test reuses the repository's Chrome/DevTools harness (`CHROME` overrides the browser path), creates an isolated profile and origin, and terminates/restarts Chrome with the same profile. It covers queued and confirmed recovery, simultaneous tabs, failed closes, uncertain/aborted commits, future formats, corrupt chunks, and a 101 MiB asset with deduplication. Publication acknowledgements in this fixture are stubs: it does not authenticate or upload to a host. Full browser worker, credential, and application checks remain necessary.

## Engine command interface

The embedding application installs its asynchronous adapter through
`Session::set_cloud_host`. No host is installed by default: commands return an
unavailable error until the platform supplies one. `solidcraft_cloud::jobs::Host`
accepts typed operations and returns a shared operation handle; it must persist
the captured snapshot before setting `saved_locally`. Pending data belongs to
the durable host store, independently of the engine's bounded status history.

| Commands | Behavior |
| --- | --- |
| `cloud.configure`, `cloud.settings` | Validate local connections/defaults and inspect the current design binding. With a host installed, configuration returns an operation id and applies after durable persistence; `cloud.settings {"reload":true}` reloads it asynchronously. Configuration contains credential identifiers only. |
| `cloud.test`, `cloud.list`, `cloud.sync` | Submit a connection-scoped operation and return its operation identifier. |
| `cloud.open` | Submit an opening operation for `{connection, project, file}`. |
| `cloud.open.finish` | Validate and adopt the completed snapshot, preserving the host and connection settings. |
| `cloud.save` | Capture the current document and visibility, submit its durable save, and retain its expected parent revision. |
| `cloud.status`, `cloud.cancel` | Poll operations or cancel work while retaining already durable snapshots. |

Run cloud commands on the live session. Scratch sessions intentionally do not
copy outstanding operations or save captures. Poll with `Session::poll_cloud`
or `cloud.status`: only durable acknowledgement adopts the captured saved
baseline. Edits made during upload remain dirty. A second save waits for the
previous local persistence boundary, then uses its captured revision as parent.
The status history retains at most 128 operations; durable offline saves can
continue beyond that limit.

Native connection settings live in the private recovery root's `connections.json`,
outside repository checkouts and ordinary UI preferences. Its
`solidcraft-connections/1` envelope is bounded to 1 MiB, preserves compatible
optional metadata (including connection fields matched by id), and refuses
future formats before any replacement. Writes use the existing atomic writer
and a separate settings lock; they do not wait on network synchronization.
The engine retains its previous settings if persistence fails. Reload and
configuration are serialized; other cloud commands wait for configuration to
finish. Browser hosts implement the same operations using IndexedDB.

Ordinary `file.save` uses a cloud binding when no explicit path is provided.
Saving to an explicit native path or adopting a native Save As detaches that
binding and prevents an old asynchronous capture from reattaching it.
`file.export` remains available for an independent native copy.

These foundations, commands, the native worker adapter, and browser recovery storage are implemented.
Application installation of that adapter, browser workers and browser connection persistence,
credential entry, guided setup, Home/Preferences
integration, and end-to-end application evidence remain unfinished. User-facing
documentation must remain marked unreleased until those paths have been
exercised in the app.
