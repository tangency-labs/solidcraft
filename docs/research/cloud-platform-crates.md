# Cloud saves: browser storage and platform dependency audit

Dependency verification date: 2026-10-22. This records the reused platform
components, persistence ordering and focused probe results.

## Recommended reuse

- Use **`indexed_db_futures` 0.6.4** for browser connection metadata, pending-save
  records, and acknowledgements. Its transaction completion API and durability
  options remove the need to write another IndexedDB callback wrapper.
- Reuse SolidCraft's **`web-sys` OPFS support** behind one focused storage adapter,
  or use **SolidCraft's shared OPFS adapter in `apps/solidcraft-web/src/files.rs` bounds
reads and awaits writes and stream closure. Cloud assets use their own directories.

The recommended save sequence is:

1. Validate and capture the document snapshot and immutable chunk hashes.
2. Write missing OPFS chunks and the recovery document; await every stream close.
3. Open a short IndexedDB transaction, write the pending record and its local-save
   acknowledgement, then await commit.
4. Report **Saved locally**. Network synchronization can now start or resume later.
5. Remove/acknowledge the pending record only after confirmed remote publication.

This ordering leaves harmless unreferenced chunks if the browser stops between
steps 2 and 3. It avoids making an acknowledged queue entry depend on incomplete
file writes. Do not await OPFS or network operations inside an IndexedDB
transaction. File changes become visible after the writable stream closes.
[Browser writable-file behavior](https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createWritable)

Use owned browser byte buffers for writes, and verify byte fidelity across the
supported browsers.

Request persistent browser storage and retain the returned boolean: the browser
may deny the request. Surface denied persistence, quota exhaustion, and failed
close/commit; a failed local write must not be shown as saved.
[StorageManager.persist](https://developer.mozilla.org/en-US/docs/Web/API/StorageManager/persist)

## Native credentials, SSH keys, and locks

The existing desktop dependency is `keyring` 3.6.3, already used through the app's
credential service. Reuse that service with opaque connection-specific identifiers;
do not put tokens or passphrases into shared project metadata.
[Registry metadata](https://crates.io/api/v1/crates/keyring),
[3.6.3 API](https://docs.rs/keyring/3.6.3/keyring/)

`ssh-key` 0.6.7 is Apache-2.0/MIT. A minimal native dependency can disable defaults
and enable `std`, `ed25519`, `encryption`, and `getrandom`. It generates Ed25519
keys, emits OpenSSH encoding, encrypts/decrypts with a passphrase, and zeroizes
private-key material. The published stable API uses
`ssh_key::rand_core::OsRng`; newer upstream documentation may show a different RNG
API. Encrypted keys still require OpenSSH agent/askpass handling when Git uses
them; generating a key does not implement SSH transport.
[Stable API and features](https://docs.rs/ssh-key/0.6.7/ssh_key/),
[published feature definitions](https://docs.rs/crate/ssh-key/0.6.7/source/Cargo.toml)

Create private key files with exclusive creation and restrictive permissions.

Standard-library file locks coordinate local SolidCraft processes. The repository's
Rust minimum includes exclusive/shared locking, nonblocking acquisition and unlock.
[File locking API](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).

## Existing encoding dependencies and retries

The workspace already has `sha2` 0.10.9, `serde` 1.0.229, `serde_json`, `tempfile`
3.27.0, and Zstd dependencies. Reuse those implementations for addressing,
metadata, compression and native temporary-file needs. Keep bounded decoding in
the cloud format layer; a compression crate cannot decide the application's
allowed decoded sizes or compatibility policy. `tempfile` does not replace the
existing atomic writer's flush/replace protocol.
[SHA-2 API](https://docs.rs/sha2/0.10.9/sha2/),
[Serde API](https://docs.rs/serde/1.0.229/serde/),
[Zstd API](https://docs.rs/zstd/0.13.3/zstd/),
[tempfile API](https://docs.rs/tempfile/3.27.0/tempfile/)

Do not introduce a runtime solely for cancellation or retry timers. Reuse the
application's asynchronous job mechanism and platform task executor. Retry state
belongs in the durable outbox: a general retry helper cannot know whether a
remote commit succeeded before the connection broke. Cancellation must leave the
snapshot recoverable and reconcile uncertain publication before retrying.

## Verification performed

- `rustc 1.99.1`; isolated manifests and targets under `/tmp`, without editing the
  workspace manifest or lockfile.
- `cargo check --target wasm32-unknown-unknown` passed for IndexedDB schema
  creation, an index, strict durability, explicit commit, and OPFS write/close calls.
- Native `ssh-key` 0.6.7 probe generated and encrypted an Ed25519 key, serialized
  and parsed its OpenSSH private-key format, recovered the same public key, and
  rejected an incorrect passphrase.
- A native standard-library lock probe verified contention between two separately
  opened handles and successful acquisition after unlock on Linux.
- An incremental compiler failure occurred while extending the WASM probe;
  the complete probe passed with `CARGO_INCREMENTAL=0`.

These are compile and native-library checks. Browser execution, storage denial,
quota failure, tab concurrency, browser restart, Safari byte fidelity, physical
crash durability, Windows/macOS key permissions, and actual SSH-agent pushes
remain integration tests. No cloud account or credentials were used.
