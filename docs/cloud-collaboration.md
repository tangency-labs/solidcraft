# Drawing and collaboration integration

Cloud saves currently publish captured files and preserve competing edits as conflict copies. The interfaces below provide extension points for future collaborative editing.

## Separate drawing files

[Drawing storage #3890](https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6351904331) plans separate `.solidcraft-drawing` files, a `solidcraft-drawing/N` schema, and links relative to the drawing's folder. The following comment specifies deterministic JSON, no embedded design identity, and repository commits as the set of referenced versions. The drawing implementation owns those decisions; cloud storage must not create a competing drawing schema.

- A project folder can hold parts, assemblies, drawings, and ordinary documentation. Do not assume every project entry is a design or discover files solely by native extension.
- The current cloud `document.json` is a design payload. It does not stand in for a drawing's `drawing.json`. A future drawing adapter must declare a required capability and select the drawing payload explicitly, or introduce a new envelope version if the layout changes. Older cloud clients must reject it before decoding or writing. Do not hide a new file kind in optional metadata.
- Keep the drawing schema independent of the cloud envelope and native design schema. Use the drawing's authoritative serializer when it lands; the current design package extraction does not require drawing work to use it.
- Resolve links within the project through the platform's storage adapter. Browser links must work through OPFS/provider storage without requiring filesystem paths. Preserve logical relative paths when encoding and exporting; moving an entire project must keep its links usable. Moving an individual source uses the drawing's relink rules.
- Resolve referenced files from a consistent repository commit. A missing, newer, or unreadable source must use the drawing's missing/out-of-date state instead of silently substituting another revision. Disposable view caches remain local.
- When drawing commands save edited source designs too, capture those files together, retain their dependency paths for offline recovery, and publish one Git commit. Extend the durable batch and publication planner then; do not issue several independent `cloud.save` calls and present them as an atomic save. Keep undo and dirty state per file.

The present client opens designs only. It rejects a drawing schema rather than deserializing it into an empty design. Publication also rejects unknown required file capabilities before choosing a conflict destination. Drawing editing, link resolution, and saves spanning files remain drawing-integration work.

## Future collaboration

Track [live co-editing research #5184](https://github.com/tangency-labs/solidcraft/issues/5184),
including [the proposal to pull/render another branch before adding live visualization](https://github.com/tangency-labs/solidcraft/issues/5184#issuecomment-6355521914).
The transport, ordering and hosting approach are still undecided. Cloud saves must
not require editing locks, freeze a per-command commit policy, or introduce a
second authoritative change model. Reuse the document-level text/diff/merge work
(#626), collision-free ids (#1017) and id references (#1134) when available.
Cloud captures remain snapshots rather than a command-replay protocol. A future
shared working copy can publish through repository adapters while session state,
presence and personal undo remain outside stored design snapshots. A later jj
integration can use its Git interoperability without changing asset addressing
or making the cloud package own the editor's history.

The existing seams are sufficient preparation:

| Existing seam | Later integration |
| --- | --- |
| Deterministic readable JSON, separate immutable assets | Diff document records without rewriting large imports. Reuse the document format's stable object ids when available; do not allocate separate cloud object ids. |
| Full destination plus expected file revision in each captured save | Detect stale edits independently for each file, including offline saves. A connection or project is not a shared editing session. |
| `Publication::prepare` returns a file batch independently of transport | Add an explicit three-way reconciliation step with base, local, and remote snapshots. Domain-aware merging belongs with the document schema, outside Git and host setup. |
| Repository adapter publishes one commit through a non-forced branch update | Check both the target file revision and branch head. Retry unrelated branch changes; keep competing edits recoverable. Never use a force push or silently choose a winner. |
| Durable outbox contains captured bytes and parent revisions | Reconnect after offline work without replacing the captured save with the latest editor state. Keep original captures recoverable through any future reconciliation. |
| Host setup is separate from publication | Add another provider or collaboration transport without changing document records or putting account credentials in a project. |

Start future collaboration with explicit synchronization and conflict resolution. Add automatic record merging only when the document schema can validate the merged result and tests cover concurrent edits and deletions. Presence and live editing can follow separately if required. Keep durable revision history distinct from per-file undo, and avoid timestamps or account information in authoritative document content merely to support synchronization.

## Optional advisory file locks

Leave room for a host adapter to acquire, renew, inspect, and release a file lease. This is a future capability, not a new interface or dependency to add now. A provider without it still supports the ordinary offline queue and revision checks. Keep local checkout/process locks separate: they prevent simultaneous local writes, not editing by another user.

A simple GitHub candidate is a dedicated control branch with small lease records, accessed through the existing Git Database client. This keeps lock churn out of design history and works without a hosted coordination service. Read the control head, validate the current record, then create a commit whose parent is that head and update its ref without forcing. Concurrent candidates from the same head are siblings; the loser rereads and reevaluates ownership. GitHub's [reference update endpoint](https://docs.github.com/en/rest/git/refs#update-a-reference) provides fast-forward enforcement, not an `expected_old_sha` argument. Do not assume an ordinary HTTP conditional request turns it into a general compare-and-swap interface. Verify acquisition, renewal, and release races against the actual provider before relying on this protocol.

The eventual lease record needs a version, repository/branch/project/file scope, an owner and session identifier, an unpredictable acquisition identifier, and an expiry. A renew or release must match the current acquisition identifier, so a delayed request cannot release a successor's lock. Publish release as a checked state change; blindly deleting a ref or file after reading it is insufficient. Validate control records like other untrusted input, and keep credentials and private account details out of them. The exact control branch, layout, permissions, and expiry policy can be chosen when locks are implemented.

| Situation | Planned behavior |
| --- | --- |
| Another user has a valid lease | Explain who is editing, offer read-only access or a separate copy, and keep local work recoverable. |
| Connectivity is lost | Keep editing and saving locally. Do not promise exclusive access while renewal cannot be confirmed; recheck ownership and revisions before publication. |
| The app crashes or a device disappears | Expire leases with a documented clock-skew allowance and an explicit recovery/takeover flow. A resumed old session must acquire again. |
| A user opens another device or tab | Treat it as a separate session. It cannot accidentally renew or release the first session's lease. |
| A native or external Git client ignores the lease | Retain revision checks, non-forced publication, and conflict copies. An advisory lock cannot prevent an arbitrary authorized push. |
| A drawing saves changed source designs | Scope leases per file, acquire a needed set in a deterministic order, and release partial acquisitions if the set cannot be obtained. Recheck every revision before publishing the batch. |

Locks reduce conflicting edits; they do not resolve them. After reconnecting or losing a lease, compare the captured base, local edits, and current remote file. Show unresolved choices explicitly and preserve both versions. Do not automatically overwrite because a lock was once held. Branch protection may prevent a control branch from being used, so the setup adapter must detect that case rather than quietly claim locking is active.

Before enabling collaboration, cover simultaneous edits to different files, different objects in one file, the same object, deletion versus editing, offline parent chains, interrupted publication, and drawing/source consistency. Lock tests additionally need simultaneous acquisition, stale renewal/release, expiry and takeover, clock skew, revoked permissions, ignored locks, and restart recovery. An unresolved merge must retain all originals and must not be reported as synced.
