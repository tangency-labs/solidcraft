# Cloud saves implementation

Git-backed cloud saves are implemented on the feature branch for #4838 and PR #4839. The feature is unreleased.

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

### Design management

`solidcraft_cloud::management` defines rename, Trash and restore intents, bounded
history entries and matching confirmed-management notifications. The intents use
the required `lifecycle/1` manifest capability, so they travel through the existing
durable save queue and immutable revision identities. `Publication` produces one
atomic write batch; native Git and Octocrab publish it using their normal receipts,
non-forced refs and retry machinery. Concurrent design changes stop management
before it can replace another revision.

`solidcraft_io::cloud::prepare_change` edits lightweight metadata while preserving
immutable asset references. Native and browser management workers select a
verified current or locally queued head, check the expected revision, and stage
the intent before publication. Historical reads use ordinary Git objects or
Octocrab's typed Git Database and commit-list APIs. Versions are bounded to 50
entries across at most 16 previous project-relative paths; commit dates are sorted
by their timestamps while preserving history order for equal timestamps.

The UI client lives in `cloud_browser/management.rs`. Confirmed rename and Trash
notifications propagate to matching active/background bindings. Later edits stay
dirty; an open deleted design becomes an unsaved local copy. Restore opens its
confirmed head separately. Pending management and its assets remain local until
publication, and immutable receipts make restarts idempotent.

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
predecessor blocks its children but not unrelated captures. Confirmed parent receipts
map offline children to the actual published destination, including conflict copies;
original captured records and identities remain immutable. A Save operation
reports only its requested capture's publication result. The local queue/process
locks are implementation coordination, separate from the future advisory editing
leases described in the collaboration plan.

### Native publication catalog

`crates/io/src/cloud/catalog.rs` maintains machine-local `cached/` heads and
immutable `receipts/` records alongside the native queue. Their
`solidcraft-native-publication/1` envelopes contain the original capture and the
repository-confirmed publication, including its actual conflict-copy destination.
They contain credential identifiers only. None of this metadata enters Git.
Catalog enumeration is bounded to 10,000 destinations and 256 MiB of metadata;
asset bytes remain deduplicated in the existing local object store.

After Git confirms publication, the worker atomically writes the cached head,
then the immutable receipt, then removes the pending record. A separate short
catalog lock coordinates those writes without network waits. Each interrupted
boundary is recoverable: an absent receipt retries Git's idempotent publication;
an existing receipt finishes acknowledgement without Git, credentials, or a
checkout. Replaying an older receipt preserves a newer cached head. Future cache,
receipt, or document formats block metadata replacement and retain the queue.
Confirmed captures remain listable and openable after restart even if their
checkout is unavailable. Validated checkout snapshots are cached by Open, including external edits.

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

Local Web Locks serialize storage changes across tabs, with no network waits inside those locks or IndexedDB transactions. Queue and cache metadata are committed with strict durability after all object writes finish. Publication confirmation updates its receipt and cache location and removes the pending record in one transaction. Confirming a parent never replaces a newer queued child; a conflict copy is cached under its actual destination. Opening a newer local database, queue, cache, or receipt format requests an upgrade. Queue listing is bounded to 10,000 records and 256 MiB of metadata.

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

The `cloud-recovery` example exercises the actual store without starting the application UI. With a matching `wasm-bindgen` CLI installed, run:

```sh
cargo build -p solidcraft-web --example cloud-recovery --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir /tmp/solidcraft-cloud-browser-site \
  target/wasm32-unknown-unknown/debug/examples/cloud_recovery.wasm
cargo xtask cloud-storage-test --site /tmp/solidcraft-cloud-browser-site
```

Set `CC_wasm32_unknown_unknown` to a WASM-capable Clang when required by Zstd. The test reuses the repository's Chrome/DevTools harness (`CHROME` overrides the browser path), creates an isolated profile and origin, and terminates/restarts Chrome with the same profile. It covers queued and confirmed recovery, rename and Trash intents across restarts, discarding queued management without deleting recovery data, simultaneous tabs, failed closes, uncertain/aborted commits, future formats, corrupt chunks, and a 101 MiB asset with deduplication. It verifies normal engine Save captures a model PNG through the browser app’s CPU renderer, then reopens the preview and editable design offline after restart. It also runs the actual browser worker through durable settings, independent credentials, offline CAD opening/listing, failed synchronization, cancellation of a stalled Fetch, and confirmed cached opening after another restart. A future-schema capture with a missing asset must request an upgrade before asset access or any repository request. Publication acknowledgements in this fixture are stubs; Fetch is intercepted for failure/cancellation checks, and no real token or authenticated upload is used. Authenticated publication is covered separately by the opt-in interoperability fixture below. Home and Settings interactions have also been exercised in actual desktop runs in both themes.

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
| `cloud.ssh.generate` | Generate a native local Ed25519 pair; returns an operation id, then private-key path and public key. Optional passphrase input uses `Session::set_cloud_ssh_key_input` outside JSON/logs; it is consumed once. |
| `cloud.test`, `cloud.list`, `cloud.sync` | Submit a connection-scoped operation and return its operation identifier. |
| `cloud.open` | Submit an opening operation for `{connection, project, file}`; optional `local_revision` opens that exact queued capture. A stale choice fails without substituting another head. |
| `cloud.thumbnail`, `cloud.thumbnail.finish` | Fetch a bounded optional preview for a destination, then consume its transient image result once. |
| `cloud.open.finish` | Validate and adopt the completed snapshot, preserving the host and connection settings. |
| `cloud.save` | Capture the current document and visibility, submit its durable save, and retain its expected parent revision. An unchanged design at its bound destination returns `unchanged: true` without an operation, snapshot, or revision; a different destination creates a copy. |
| `cloud.website` | Return the selected GitHub repository's canonical website URL. |
| `cloud.history` | Read the observed revision and up to 50 historical versions across the design's previous project-relative paths. |
| `cloud.trash` | List recoverable deleted designs for one repository. |
| `cloud.rename` | Queue a rename against its observed revision, preserving the source and checking the target at publication. |
| `cloud.delete` | Queue a recoverable Trash revision against its observed revision. |
| `cloud.restore` | Capture a selected historical commit and path, then publish it against the observed current revision. |
| `cloud.discard` | Remove one queued management intent after checking it has not reached the repository and no later saves depend on it; retain saved design data. |
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
`solidcraft-connections/1` or `/2` envelope is bounded to 1 MiB, preserves compatible
optional metadata (including connection fields matched by id), and refuses
future formats before any replacement. Writes use the existing atomic writer
and a separate settings lock; they do not wait on network synchronization.
The engine retains its previous settings if persistence fails. Reload and
configuration are serialized; other cloud commands wait for configuration to
finish. Browser hosts implement the same operations using IndexedDB.

Native checkout, catalog, settings and synchronization locks share `ProcessLock`
in `crates/io/src/cloud/local.rs`. Dropping it explicitly unlocks the file, so a
concurrent Git spawn cannot prolong ownership through an inherited descriptor.
Workers release the synchronization guard before acknowledging completion.
Snapshots captured during another publication are persisted immediately. Their native
workers wait for that connection's synchronization lock, then publish the queued revision.
Waiting checks cancellation every 20 ms and times out after five minutes; cancelled or
timed-out uploads retain their durable captures for retry. Other connections keep working.

Ordinary `file.save` uses a cloud binding when no explicit path is provided.
Saving to an explicit native path or adopting a native Save As detaches that
binding and prevents an old asynchronous capture from reattaching it.
`file.export` remains available for an independent native copy.

These foundations, commands, platform workers, connection persistence, browser
recovery storage, native credentials, guided setup, and Home browsing/saving are
implemented. The Cloud Saves preferences editor is implemented. Authenticated desktop/browser
round trips and offline restart acceptance pass. The shared UI kit supplies cloud
forms, selections and masked credential fields; actual app checks cover both themes.


## Guided setup and native credentials

`solidcraft_cloud::setup` validates provider identifiers, creates stable per-repository
connection identities, and supplies the prefilled GitHub token link. Host setup stays
separate from publication. Another provider can supply its setup module without
changing snapshot or outbox code.

GitHub setup uses a guided fine-grained personal access token. The token link is visible
from the first frame. By default, GitHub selects the signed-in account and provider
setup uses `GET /user` to resolve its `solidcraft-designs` repository. An optional
Advanced override accepts an existing repository name or URL. The link prefills
name, description, Contents write and Metadata read permissions, the overridden
resource owner when supplied, and `expires_in=none` (no
expiration, subject to host policy). The wizard guides users through **Only select
repositories**, selecting their repository, and generating/copying the token.
The same setup runs on desktop and WASM.

`cloud.connect` submits `Operation::Setup` with a proposed connection and an opaque
credential identifier. It returns an operation id and, when ready, a verified connection;
it does not change preferences. The UI merges that result into current settings and uses
`cloud.configure` to apply it durably. Cancelling before Apply leaves settings unchanged.
Setup jobs remain accessible if the user changes document tabs. An empty repository
override is a setup request: the provider resolves the account and returns the stable
repository identity while preserving its opaque credential reference. Desktop keys
the managed checkout and setup lock by that resolved identity, so reconnects reuse
the checkout and different accounts remain isolated.

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
Saves preference editor manages pending connection edits. All setup labels cover all
supported locale catalogs, including en-GB.

## Cloud Saves preferences

`crates/ui-egui/src/cloud_settings.rs` owns a pending editor over the current
connection settings. The existing Preferences window supplies its page and
Apply/OK/Cancel lifecycle. `cloud.configure` persists changes asynchronously;
only durable completion advances the preferences snapshot or closes the window.
Failed persistence retains the editor for retry. Settings changed by another tab
refresh an untouched editor; a dirty editor asks the user to reload before Apply.

Guided setup from this page stages its verified connection in the editor. Setup
from Start continues to apply the connection directly. Disconnect changes settings
only; durable queues and credential storage retain their data. Restore Defaults
uses `Settings::restore_behavior_defaults`, preserving connections and defaults.
Testing and synchronization use the applied connection through existing commands.
The shared UI kit table in `cloud_settings/repositories.rs` owns row selection,
default destination and removal controls. Labels use `Connection::repository_label()`
from remote metadata. Operation feedback has a fixed footer, and busy buttons
replace their leading icon without changing dimensions.

## Verified native remotes

`cloud.test` accepts either a saved connection id or a `proposed` connection,
plus an optional `git_executable` for that test. A native test returns the
verified connection in the completed operation's `result`. The result includes
the canonical checkout path and a `git_remote` record containing one fetch URL
and all push URLs. Configure that returned connection through `cloud.configure`
to retain the verified destinations. Testing itself leaves settings unchanged.
An initial generic connection can use `id: "git"`; verification supplies a stable
id derived from its checkout, branch and resolved destinations.

Git resolves named remotes and URL rewrites. The adapter checks the stored
endpoints before publication, fetches existing branch heads from the fetch and
push destinations, and advances the checkout with fast-forward merges. Diverged
histories retain the local save for the user to reconcile through Git. A missing
remote branch receives its first publication. Pushes use the verified URLs, and
publication is acknowledged only after every push destination confirms the commit.
A changed remote requires verification before the captured save can be published.

GitHub HTTPS credentials remain scoped to the chosen repository when a named
remote resolves to that same repository. Endpoint checks run before credentialed
network operations; Git's credential protocol still supplies the token.

The native editor lives in `ui-egui/src/cloud_settings/git.rs`. Interactive
`cloud.configure` opens it within Cloud Saves preferences. It uses `cloud.test`
with a proposed connection and Git executable, shows the resolved endpoints, and
stages the verified result through the parent editor. Apply owns the durable
configuration write. Editing any tested field invalidates the result. Cancellation
stops the test; delayed picker replies carry an editor identity so a closed or
replaced editor cannot receive them. Directory selection is injected by the
desktop host alongside the existing asynchronous file pickers. System and chosen
SSH keys stay connection-scoped; the Git executable is a shared setting.


### Native SSH key generation

`io::cloud::ssh` reuses `ssh-key` 0.6.7 for Ed25519 and encrypted OpenSSH
serialization. A fallible OS entropy read seeds `rand_chacha` before calling the
crate's infallible RNG interface. Files use exclusive creation in a new private
`cloud/ssh/key-…/` directory (0700 directory / 0600 files on Unix; owner-only
inherited ACL on Windows). Generation never changes Git/SSH configuration or
replaces existing keys. Cancellation before publication drops the temporary pair;
a cancellation racing completed generation can leave an unused local pair.

The registered `cloud.ssh.generate` command submits `Operation::GenerateSshKey`.
Secret input uses the one-shot session API, is zeroized on drop, and is not copied
into scratch sessions, JSON command parameters, crash recordings or public results.
The UI clears unused input if command dispatch fails. Results contain only path,
public key, and encryption/remembering flags. Generation and connection Apply are
separate: after registering the public key, the selected identity must pass the
normal proposed-connection test.

`NativeAccess::store_ssh_passphrase` is called only on explicit opt-in. The desktop
adapter uses the existing keyring service with a hashed path identity distinct
from repository-token entries. `NativeAccess::ssh_askpass` provides the current
executable only when that entry is accessible. Git gets a per-process askpass
executable and opaque keychain id; the passphrase is retrieved in the helper.
The helper runs before GUI initialization and answers only an exact selected-key
passphrase prompt in the C locale. It rejects host confirmations and account
password prompts. Standard SSH agents and system prompts remain usable for other
keys and when the keychain entry is unavailable. Each connection's helper setup
is confined to its own subprocess environment.

The key form and result actions live in `cloud_settings/git/key.rs`. Host SSH
settings links use the setup adapter; adding another host can reuse generation,
credential storage, the connection test, and Git unchanged.


### Durable listing status and checkout snapshots

`cloud::CachedEntry` is the shared machine-local `solidcraft-cache/1` envelope.
It retains a captured manifest, actual destination and revision, plus confirmation
and conflict flags. It reads the existing browser cache shape and preserves unknown
optional fields. Native readers also accept `solidcraft-native-publication/1` cache
records; immutable publication receipts use their strict `/1` envelope, or `/2` when retaining a redirected parent.
Reading metadata never rewrites a repository or enqueues an upload.

Native Open retains validated checkout objects and document bytes before atomically
publishing cache metadata under the catalog lock. A checkout lock spans the read and
cache update, and pending captures retain priority. Open rechecks queued heads before
completion if another tab saves during IO. Cache failures propagate to the operation;
they cannot masquerade as an unavailable checkout and open an older cached design.
Existing future cache/document formats block replacement. Queues and cached listings
have a 256 MiB aggregate metadata bound on both platforms.

`cloud.list` keeps its destination-array result and includes optional `JobStatus.catalog`
details. `stored_designs` reconstructs queued, confirmed and unconfirmed local states,
and competing-tip attention from durable metadata. Only verified publications or remote
reads establish confirmation. Native listings compare revision, document hash and payload
manifest identity against the checkout before retaining **Synced**. Compatible optional
metadata and revision bookkeeping do not change that payload identity; required
capabilities and asset mappings do. The Home module validates these details, preserves
them on failed refreshes, and refreshes after confirmed publications. Newer captured
saves remain ahead of an older listing result.


### Optional Home previews

`cloud::Thumbnail` owns optional document-keyed image metadata and bounded chunk
transport. Native Git and GitHub fetch a preview without loading source imports;
local adapters use queued or cached data first. Missing preview objects leave the
CAD snapshot recoverable. Existing captures without a picture gain one on their next
save; reading a design never rewrites its stored revision. Thumbnail capture reuses
warm shared definition meshes with their occurrence placements and checks a 256 MiB
scene-buffer budget before copying geometry. The shared engine thumbnail helper uses
the installed picture renderer for both native exports and cloud Save.
The browser app installs its existing CPU renderer.

`cloud_browser/previews.rs` requests only visible entries and retains at most 64
entries, invalidated by connection and revision. It consumes completed image results
through the registered finish command, including after a tab change or while a
file picker is open. PNG decoding reuses the bounded Home decoder; unavailable
images retain the standard design icon.


### Confirmed conflict lineage

`Published::target` and `Publication::prepare_with_parent` share destination mapping
between native Git and GitHub. The adapter reads the actual parent destination,
checks its required envelope/document versions, then prepares the next publication.
The expected revision comparison still protects that destination from external edits.
Retries at the mapped destination recognize the original captured revision.

Native `/2` publication receipts retain the parent mapping, with validation before
cache replacement or queue removal. Replay uses the receipt even without Git or a
checkout and keeps a newer cached child. Browser confirmation validates the same
mapping and replaces a confirmed parent cache with its child in the durable
acknowledgement transaction. The browser fixture restarts between parent and child
confirmation and verifies the child document at the restored conflict destination.


### Inspecting competing offline versions

`PendingSave::local_head` selects an explicitly requested queued revision or requires
one unambiguous local tip. It is shared by both platform workers. `StoredDesign`
includes bounded `local_revisions` metadata (capture id and optional display title) reconstructed from queued tips; listing
still returns its existing destination array. The Home client refreshes after local
acknowledgements as well as publications. `cloud_browser/versions.rs` offers an
explicit choice when several tips exist, through the same registered Open command.
Opening adopts the selected revision into a new editor tab and keeps every capture
queued. Selection reads neither remote files nor another design as a fallback.


Global configuration writes are guarded across document tabs for direct commands and
control scripts. A script containing `cloud.configure` or a settings reload is refused
before any step when another configuration is pending. Read-only scripts remain
available; configuration can be retried after durable completion.


Native synchronization with an empty queue refreshes the verified remotes under
the checkout lock. Git advances with `--ff-only`, preserves unrelated index/worktree
changes, and refuses unresolved operations or divergence. This read does not invent
publication receipts. Home refreshes its listing after successful synchronization,
including completion without any published captures.

Superseded preview/list jobs retain their cancelled state when their worker notices
cancellation. They do not turn ordinary Home refreshes into storage-error banners;
cancelling an upload still retains its durable queued capture.


### Authenticated interoperability fixture

`cargo xtask cloud-live-test --site /tmp/solidcraft-cloud-browser-site` is an opt-in
acceptance fixture. Its bounded JSON input arrives on stdin with a `connection`
and test `token`; credentials never enter argv, static site files or engine commands.
It refuses branches outside the `cloud-acceptance-` prefix. Use an isolated browser
profile and a disposable branch containing only public-safe sample data.

Seed `Acceptance/parts/bracket` from the desktop app with one Box feature and name
`Acceptance bracket`. The fixture opens it through the actual GitHub adapter, adds
another editable Box, publishes through normal Save, then terminates/restarts Chrome
and opens the confirmed design offline using persisted settings and credentials.
It emits confirmed publication metadata as JSON lines so the caller can verify a
subsequent desktop Sync/Open and remove only the disposable ref with an exact lease.
The ordinary cloud-storage fixture remains deterministic and credential-free.


Authenticated acceptance has exercised native HTTPS publication using the desktop
credential adapter, browser opening and publication through GitHub, offline browser
restart with retained settings/credentials, and desktop Sync/Open of the browser
revision with both editable Box features intact. Test data used a disposable orphan
branch; all such refs were removed with exact leases after checking their contents.

### Observed repository changes and action lifetimes

`StoredDesign::outdated` distinguishes an observed stale cache from a listing that
has not yet been confirmed. Listing compares revision, document hash and manifest
identity. An atomic compare-and-set marks the observed cache head outdated without
changing a newer save or pull; the optional flag survives restart. Pending local
captures retain priority. Home renders shared `FileStatus` tones and short localized
labels, and disables an outdated row until synchronization replaces its cache.

Repository synchronization refreshes previously cached designs. Browser listing
checks their manifests before downloading changed snapshots. Native refresh keeps
unrelated staged and working files, and confirms the received design's owned paths
against committed and published content before marking the cache current. Downloads
use the existing compatibility checks and bounded asset readers. Failed or interrupted
refreshes retain the outdated flag and the recoverable copy.

`cloud/operations.rs` tracks command operation IDs across tabs. Cloud action buttons
use the shared kit's `busy_button`, whose leading action icon becomes a spinner
without changing the button width or text position.
Working and syncing operations keep their action disabled; terminal acknowledgements
(including failure, cancellation and worker timeout) release it. Local durability
alone does not release an action while its upload continues.
