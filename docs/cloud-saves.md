# Cloud saves implementation

Cloud saves are being developed in #4838 and draft PR #4839. This document distinguishes implemented foundations from the remaining integration.

## Modules

`solidcraft-cloud` is a standalone workspace crate with no CAD or UI dependencies. Its default feature set provides portable snapshot and queue types. The optional `github` feature adds an Octocrab adapter whose HTTP client is supplied by the platform. Its snapshot interface prepares reusable asset objects and hydrates them through an injected object reader. Errors distinguish invalid input, unavailable storage, competing edits, and an upgrade requirement. See [the format specification](cloud-storage-format.md).

Implemented foundation tests cover asset reuse across parameter changes, chunk reconstruction, unknown versions and capabilities, unsupported codecs, optional metadata preservation, and corruption rejection. Native and wasm builds are required; Zstd's C dependency needs a wasm-capable Clang at build time.

## Dependency reuse

Existing infrastructure is preferred over new protocol, cryptographic, database, or process frameworks. The dependency audit checks the exact released version, license, required APIs, and browser compilation before adoption. A successful WASM build is not a browser restart or network integration test.

| Responsibility | Reuse decision |
| --- | --- |
| Desktop repository engine | Installed Git, preserving its transports, helpers, agent, configuration, hooks, and signing. `process-wrap` owns cancellable Unix process groups and Windows Job Objects; a narrow adapter bounds input/output and deadlines. |
| GitHub client | Octocrab 0.54.5 with default features disabled and a small browser service/executor adapter. Real typed Git Database read/write calls compile, and public Git reads executed in Chromium. |
| Browser queue and metadata | `indexed_db_futures` provides schema upgrades, indices, durability options, and explicit transaction completion; its required APIs passed the browser-target compile probe. |
| OPFS assets | Shared OPFS adapter in `apps/solidcraft-web/src/files.rs`, with bounded reads and awaited write/close. |
| Native credentials | Existing `keyring` service with per-connection credential identifiers. |
| SSH key generation | `ssh-key` for Ed25519 and encrypted OpenSSH serialization; system Git/OpenSSH still handles existing keys and agents. Native encrypted-key roundtrip and wrong-passphrase checks passed. |
| Locking and atomic writes | Standard-library file locks and existing `solidcraft_io::safe_write`; no extra locking or atomic-write library is needed. |
| Encoding | Existing `serde`, `serde_json`, `sha2`, and `zstd`; no new serialization, hashing, or compression implementation. |

Detailed comparisons, primary-source citations, exact versions, and probe limits are recorded in the [Git audit](research/cloud-git-crates.md), [GitHub client audit](research/cloud-github-crates.md), and [platform audit](research/cloud-platform-crates.md). Browser transport lives in `apps/solidcraft-web/src/cloud_client.rs`; it keeps Fetch futures local, bounds response accumulation, and uses Octocrab's host-scoped authorization middleware. Browser tasks run on the platform executor.

SolidCraft-specific code remains responsible for project layout, version compatibility, destination selection, capture-before-upload, durable acknowledgement, conflict copies, and reconciliation after an uncertain upload. General API or retry libraries do not decide those rules.

See [drawing and collaboration integration](cloud-collaboration.md) for compatibility with separate drawing files (#3890) and the existing seams for later collaborative work.

### Start-page command client

`crates/ui-egui/src/cloud_browser.rs` lists project designs through `cloud.list`,
opens them through `cloud.open`, and chooses initial `cloud.save` destinations.
Its per-connection catalogs are disposable presentation caches. Failed refreshes
retain displayed entries, and the platform host remains authoritative for offline
availability, version checks and publication. Large catalogs use virtual scrolling.
Known occupied names are rejected in the picker; publication still handles races.

A completed download runs `cloud.open.finish` in an isolated session through
`Session::cloud_open_session`, then follows normal document-tab adoption. Edits and
tab changes during networking cannot replace the current design. Destination
choices retain the originating tab and document and are cancelled if it changes.
Close/Quit waits for the picker and durable local acknowledgement. New designs use
the default cloud destination picker for normal Save; existing disk files continue
using their disk destination. Repository selection never changes the saved default.

## Integration contract

Host setup and repository access are separate adapters. A host setup adapter supplies guided authentication, repository discovery/creation, and host settings links. The native repository adapter uses standard Git; a browser adapter uses a CORS-compatible provider interface. Adding GitLab or Gitea setup must not change the snapshot format.

Engine commands own user-visible behavior and asynchronous operation identifiers. UI modules render state and issue those commands. Credentials stay in host credential storage, never in document metadata, remote URLs, command logs, or manifests.

Desktop Git must preserve credential helpers, SSH agents/configuration, signing, unrelated staged and unstaged work, and linked worktrees. A connection-specific key override affects only its subprocess. Key generation must not overwrite existing keys. Publishing is non-forced and reports synchronization only after confirmation.

Browser integration must persist connection metadata and pending saves in IndexedDB, with assets and recovery data in OPFS. Restart recovery must retry the captured snapshot rather than silently substituting later unsaved edits. Quota and credential failures leave the requested save recoverable.

`solidcraft_io::cloud::NativeHost` runs native operations on workers and retains
the caller's allowed-folder scope. Give it the app's private cloud recovery
directory. Each connection has its own outbox under the SHA-256 of its identifier,
preventing case-insensitive filesystem aliases. Save persists its captured objects
and queue record before starting Git. Listing combines queued and confirmed local
destinations with the checkout. Opening prefers the latest queued revision;
otherwise it reads the checkout, with a confirmed local capture as a fallback
when the checkout or Git is unavailable. Unsupported or corrupt checkout formats
return an error instead of silently opening an older cache. Competing local tips
remain intact and return a conflict rather than opening an arbitrary version.

Synchronization retries captured revisions in dependency order; a failed
predecessor blocks its children but not unrelated captures. A Save operation
reports only its requested capture's publication result. The local queue/process
locks are implementation coordination, separate from the future advisory editing
leases described in the collaboration plan.

### Native publication catalog

`crates/io/src/cloud/catalog.rs` maintains machine-local `cached/` heads and
immutable `receipts/` records alongside the native queue. Their
`solidcraft-native-publication/1` envelopes contain the original capture and the
repository-confirmed publication, including its actual conflict-copy destination.
They contain credential identifiers only. None of this metadata enters Git.
Catalog enumeration is bounded to 10,000 destinations and 128 MiB of metadata;
asset bytes remain deduplicated in the existing local object store.

After Git confirms publication, the worker atomically writes the cached head,
then the immutable receipt, then removes the pending record. A separate short
catalog lock coordinates those writes without network waits. Each interrupted
boundary is recoverable: an absent receipt retries Git's idempotent publication;
an existing receipt finishes acknowledgement without Git, credentials, or a
checkout. Replaying an older receipt preserves a newer cached head. Future cache,
receipt, or document formats block metadata replacement and retain the queue.
Confirmed captures remain listable and openable after restart even if their
checkout is unavailable. Downloads and externally edited checkout snapshots are
not yet added to this native catalog.

The desktop app installs this worker through `Services::cloud_host`, using its
private `cloud` data directory. `SolveApp` loads settings through the registered
command, polls active and background document tabs, and retries pending queues
after startup and once a minute when enabled. New tabs inherit connections;
existing bindings stay with their designs when the default changes. Confirmed
publications from any tab update only matching captured revisions in other tabs.
Normal cloud Save runs on the live session, bypassing the native scratch-session
file writer. Close/Quit waits for local capture acknowledgement, independently
of network publication. Platform workers expose `Syncing` while an acknowledged
capture is still uploading, so idle windows continue polling.

### Browser recovery store

`apps/solidcraft-web/src/cloud_storage.rs` implements the browser store. Each connection has an IndexedDB database containing `pending`, `cached`, and `receipts`, and an OPFS directory under `cloud/<SHA-256 of connection id>`. Stored object paths match those used by the outbox and repository snapshots. The shared OPFS adapter bounds reads before allocating buffers, writes owned browser bytes, and awaits stream close. It aborts failed writes; a retry can complete an empty file left by interrupted initial creation.

Local Web Locks serialize storage changes across tabs, with no network waits inside those locks or IndexedDB transactions. Queue and cache metadata are committed with strict durability after all object writes finish. Publication confirmation updates its receipt and cache location and removes the pending record in one transaction. Confirming a parent never replaces a newer queued child; a conflict copy is cached under its actual destination. Opening a newer local database, queue, cache, or receipt format requests an upgrade. Queue listing is bounded to 10,000 records and 128 MiB of metadata.

A failed commit acknowledgement does not prove no record exists: a browser can finish committing while the caller receives an error. Do not report that capture as saved; retain and recognize its stable identity on retry. Persistent-storage denial is recorded in `Store::persistent` for the host to display; it does not masquerade as granted persistence.

`cloud_host.rs` is installed through the same `Services::cloud_host` as desktop.
It stages captures before requesting credentials or using the network, opens
queued heads first, and otherwise reads GitHub with an offline cache fallback.
Only connectivity/credential storage errors permit that fallback; unsupported
remote formats still request an upgrade. Resumed reads and uploads validate the
document schema before reading asset objects. Synchronization holds a separate
local Web Lock from staging, confirms receipts before removing queue entries,
and reports partial publication results to other tabs' engine sessions. Browser
Fetch requests are abortable through the operation's cancellation hook; OPFS
closes and IndexedDB commits finish independently so cancellation does not tear
down the local durability boundary. Publication retries asynchronously reload
the original captured snapshot rather than retaining a second full byte copy.

`cloud_settings.rs` stores the versioned connection envelope and independent
provider credentials in `solidcraft-cloud-settings` IndexedDB stores. Tokens do
not enter engine command parameters, status, UI preferences, design catalogs,
or repository metadata. The browser currently supports GitHub API access;
arbitrary Git transports remain desktop-only. Setup calls credential storage
directly and then configures opaque credential identifiers through commands.
The guided setup UI stores credentials through this interface. Storage persistence
grants are exposed as `JobStatus.storage_persistent` for the pending UI.

The `cloud-recovery` example exercises the actual store without starting the renderer. With a matching `wasm-bindgen` CLI installed, run:

```sh
cargo build -p solidcraft-web --example cloud-recovery --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir /tmp/solidcraft-cloud-browser-site \
  target/wasm32-unknown-unknown/debug/examples/cloud_recovery.wasm
cargo xtask cloud-storage-test --site /tmp/solidcraft-cloud-browser-site
```

Set `CC_wasm32_unknown_unknown` to a WASM-capable Clang when required by Zstd. The test reuses the repository's Chrome/DevTools harness (`CHROME` overrides the browser path), creates an isolated profile and origin, and terminates/restarts Chrome with the same profile. It covers queued and confirmed recovery, simultaneous tabs, failed closes, uncertain/aborted commits, future formats, corrupt chunks, and a 101 MiB asset with deduplication. It also runs the actual browser worker through durable settings, independent credentials, offline CAD opening/listing, failed synchronization, cancellation of a stalled Fetch, and confirmed cached opening after another restart. A future-schema capture with a missing asset must request an upgrade before asset access or any repository request. Publication acknowledgements in this fixture are stubs; Fetch is intercepted for failure/cancellation checks, and no real token or authenticated upload is used. Authenticated host publication and full Home/Settings evidence remain necessary.

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

These foundations, commands, platform workers, connection persistence, browser
recovery storage, native credentials, guided setup, and Home browsing/saving are
implemented. The dedicated Cloud Saves preferences editor and authenticated host
acceptance remain unfinished; the feature is unreleased.


## Guided setup and native credentials

`solidcraft_cloud::setup` validates provider identifiers, creates stable per-repository
connection identities, and supplies the prefilled GitHub token link. Host setup stays
separate from publication. Another provider can supply its setup module without
changing snapshot or outbox code.

GitHub setup uses a guided fine-grained personal access token. The link prefills
name, description, resource owner, Contents write and Metadata read permissions, and `expires_in=none` (no
expiration, subject to host policy). GitHub does not document a repository-selection
URL parameter, so the wizard explicitly asks users to choose **Only select
repositories**, select their repository, and generate/copy the token. The template
never requests access to all repositories. The same setup runs on desktop and WASM.

`cloud.connect` submits `Operation::Setup` with a proposed connection and an opaque
credential identifier. It returns an operation id and, when ready, a verified connection;
it does not change preferences. The UI merges that result into current settings and uses
`cloud.configure` to apply it durably. Cancelling before Apply leaves settings unchanged.
Setup jobs remain accessible if the user changes document tabs.

Secrets enter `Host::store_credential` directly, outside serializable engine parameters
and command/crash logs. Replacement tokens have separate local identifiers, so a failed
reconnect cannot overwrite a working connection's token. Credential records left by an
abandoned setup are local only. Native `NativeAccess` is injected by the desktop app:
keyring stores tokens under the `SolidCraft cloud saves` service, and Octocrab uses the
existing ureq transport with OS certificate verification, bounded responses, no redirects
and a 30-second HTTP deadline. Its local executor is created on the cloud worker thread.
Cancellation is checked before/after native HTTP requests; an in-flight native setup
request may take up to that deadline to stop. Browser setup reuses cancellable Fetch and
its separate IndexedDB credential store.

The shared Octocrab setup adapter inspects the selected repository, adopts its default
branch, creates a README only for an empty repository, and tests Contents write permission
by updating the observed branch ref to its existing SHA with `force=false`. This creates
no design revision; a concurrent branch advance rejects the probe safely. Existing files
are never overwritten by initialization. Read-only/archived repositories and HTTP access
failures leave local queues untouched.

Native setup clones into an automatically cleaned temporary directory, validates the
clone, then renames it into that connection's private checkout. Retries reuse the checkout
only after verifying its origin and selected branch. HTTPS credentials use Git's standard
credential-helper protocol with a fixed helper and connection-specific environment values.
The helper answers only for HTTPS, `github.com`, and the exact selected repository path;
it ignores store/erase calls. Tokens never enter argv, URLs, Git config or diagnostic output.
Redirects and Git trace output are disabled for token-authenticated commands; effective
fetch/push URL rewrites are checked before publication. System authentication is retained
for connections that select it. Token lookup happens only for network operations, so an
unavailable keychain does not prevent local checkout opens. Commits use the configured Git
identity; missing identity fields get per-command fallbacks without editing Git config.

The Start page opens this wizard and browses connected repositories. The Cloud
Saves preference editor is the next UI integration step. All setup labels cover the 25
supported locale catalogs, including en-GB.
