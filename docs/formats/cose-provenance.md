# Signed provenance records in a design container

Version 1 (2026-11). A design container may carry one optional entry, `provenance/log.cbor`, with
a bounded chain of signed records. Each record signs a CBOR manifest (RFC 8949) of the design's
authoritative content. A record is either a COSE_Sign1 message (RFC 9052, Ed25519, for a software
key) or an OpenPGP detached signature with the signer's transferable public key (for GnuPG, whose
key may be on a smartcard). The signature makes a version of a design tamper-evident: a
recipient can check that the document and every payload it refers to are exactly what a holder of
a key signed, and which key signed which earlier version.

The entry is additive. A design without it is an ordinary unsigned design. A build that does not
know it ignores it (as it ignores any entry it does not use), and a design that was edited and
saved by such a build has a valid signature over an earlier version, which the reader reports
instead of calling the signature wrong.

## Files and versions

| Part | Content |
|---|---|
| `container.json` | unchanged (`solidcraft-container/1`) |
| `document.json` | unchanged (`solidcraft/N`); provenance adds no field to it |
| `provenance/log.cbor` | this format's envelope |

The envelope has its own version. A build that asks the version first (ignoring any newer
fields) reports an unknown version as written by a newer build, keeps the design usable, and
starts a new log when it signs a later save. It never guesses how to read it. A design-format
change is separate: provenance changes what a container carries beside the design, not what the
design means.

The envelope's size is bounded: at most 8 MiB, at most 64 records of at most 4 MiB each, at most
10,000 signed parts, and text fields of at most 4 KiB. These bounds are checked while decoding, so
a crafted item count cannot allocate first and fail afterwards. A writer that would exceed the
envelope budget drops the oldest records and signs a new record that names the new first record;
the drop is a signed fact, not an unsigned flag. The current entry is small: the manifest holds
one digest per authoritative part, and a record is the manifest plus a 64-byte signature.

## Envelope

CBOR map, no unknown keys:

```cddl
envelope = {
  "v": 1,                       ; this format's version
  "kind": "solidcraft-provenance",
  "records": [* bstr],          ; exact tagged COSE_Sign1 messages, oldest first
}
```

Keeping each record as its exact byte string preserves the bytes the chain links digest. A
truncation is not an envelope flag: it is signed, through the first record's `prev` and the newest
record's `oldest` (below).

## Record and manifest

There are two record kinds. A record's first byte tells them apart: a COSE_Sign1 message begins
with CBOR tag 18 (`0xd2`), an OpenPGP record is a CBOR map.

**COSE_Sign1**, for an Ed25519 key held by software:

```cddl
COSE_Sign1 = [
  protected : bstr,             ; map: { 1: -8 (EdDSA), 3: text, 4: kid (bstr) }
  unprotected : map,            ; empty
  payload : bstr,               ; the CBOR manifest below
  signature : bstr,             ; 64-byte Ed25519 signature over COSE's Sig_structure
]
```

- Algorithm: `EdDSA` (−8), Ed25519. No other algorithm is accepted in version 1; another
  algorithm is a new version, not a negotiable option. The record's manifest `v` is tied to the
  envelope's `v`: a future change bumps the envelope, which is checked before the record shape is
  read.
- `kid` is the SHA-256 of the 32-byte public key. The key itself is in the signed manifest, so a
  file can be verified without a separate key distribution; deciding whether the key belongs to
  the person it names is the reader's policy, not the format's.
- Content type: `application/vnd.solidcraft.provenance-manifest+cbor`.
- The signature covers COSE's `Sig_structure` (`["Signature1", protected, external_aad, payload]`).
  The payload is stored, so a verifier never re-encodes or canonicalises it.
- No unprotected, critical, counter-signature or unknown protected headers are accepted in
  version 1.

**OpenPGP**, for GnuPG and keys on smartcards:

```cddl
openpgp-record = {
  "v": 1,
  "kind": "openpgp",
  "manifest": bstr,             ; the CBOR manifest below, exactly as signed
  "signature": bstr,            ; detached, binary OpenPGP signature over `manifest`
  "key": bstr,                  ; the signer's transferable public key, binary packets
}
```

- The signature is a standard detached OpenPGP signature (v4 signatures from GnuPG 2.2/2.4/2.5;
  EdDSA, ECDSA and RSA are all verifiable by the format's reader through `pgp`/rPGP).
- Verification is in-process against the embedded public key, including a signature made by a
  signing subkey (the usual YubiKey slot); no keyring and no GnuPG are needed. The OpenPGP key's
  bindings, revocations and expiry are not evaluated here: the record proves which key signed, and
  trusting that key is the reader's decision.
- The private key never enters the file, SolidCraft or a temporary file: GnuPG signs the manifest
  where the key lives, and only the signature and the public key are copied out.
- The key's fingerprint and user id are derived from the embedded key when the record is read;
  Neither the fingerprint nor the user id is stored in the record: both are read from the key.

Manifest, CBOR map, no unknown keys:

```cddl
manifest = {
  "v": 1,
  "document": bstr .size 32,    ; SHA-256 of the exact document.json bytes
  "container": bstr .size 32,   ; SHA-256 of the exact container.json bytes
  "parts": [* part],
  "format": text,               ; the document's format tag, e.g. "solidcraft/52"
  "pending": [* text],          ; the document's format_pending, if any
  "prev": bstr .size 32 / absent, ; SHA-256 of the previous record's exact bytes
  "oldest": bstr .size 32 / absent, ; SHA-256 of the file's first record, when this one is not it
  "created": uint / absent,     ; Unix seconds from the signer's clock; unauthenticated
  "app": text,                  ; the SolidCraft version that wrote the record
  "author": text / absent,      ; a display name; the key, not this text, is proven
  "tool": text / absent,        ; desktop, cli, mcp, ...
  "note": text / absent,
  "public_key": bstr .size 32,  ; the Ed25519 public key
}

part = {
  "name": text,                 ; container entry name
  "len": uint,                  ; uncompressed length
  "sha256": bstr .size 32,
}
```

## What a signature covers

A signature binds the authoritative content of a container:

- `document.json`, as exact bytes (not a re-serialisation, and before any in-memory migration);
- `container.json`, as exact bytes;
- every other entry that the design itself uses: `blobs/…` payloads (imported STEP, meshes, canvas
  images, point clouds, HDR and texture payloads), and any entry an earlier build kept beside the
  design because it did not understand it.

Not covered, by design:

- `cache/…` — the evaluated model cache, provisional display meshes and thumbnails. They are
  derived and rebuilt from the document; a signature should not pretend to authenticate a
  shortcut. A recipient that trusts a signature should evaluate from the timeline rather than
  adopt a cache from the file.
- `provenance/…` itself. The record chain is the signature; earlier records are bound by `prev`,
  and the newest record's `oldest` names the first record of the file it was written into, so
  removing the start of a truncated log is detected.
- The ZIP directory, entry order, names' compression and extra fields. A recompressed or repaired
  container still verifies; a changed document or payload does not.

The digest is over uncompressed bytes. The reader hashes the content itself; it does not trust the
blob names (which also carry the digest) as proof.

## Verification

Reading the entry never fails the design open: a malformed, oversized, unsupported or
unverifiable entry is a report, and the design opens as usual.

| Result | Meaning |
|---|---|
| no entry | the design is unsigned (not an error) |
| valid | every record verifies (COSE in-process, OpenPGP against its embedded key), the chain is unbroken, the newest record's digests and part list equal the file's current authoritative content, and `chain.truncated` says whether a signed earlier start was dropped |
| not covered | signatures and chain verify, but the newest record covers an earlier version (saved again without a key, or edited by a build that does not sign) |
| invalid | a record, signature, link or manifest is malformed; the provenance cannot be trusted |
| unsupported | a newer build wrote a different version; the design is read, the entry is not |

Verification bounds are part of the format and are enforced during decoding: entry 8 MiB, record
4 MiB, 64 records, 10,000 parts, 4 KiB names and notes, CBOR nesting at most 32 deep, and
duplicate part names are refused. Every signature is checked with its own key; a record whose
`kid` does not match its public key is invalid. Except for the first kept record, `prev` must be
the SHA-256 of the exact preceding record. A single record must be a genesis (`prev` and `oldest`
absent). A longer chain's first record may name a dropped predecessor, and then the newest
record's `oldest` must be the SHA-256 of the first record actually present; removing records from
the start breaks that link. The report's `chain.truncated` is true exactly when the first present
record names a predecessor, and it is derived from signed fields, never from a caller or envelope
value.

Verification costs one pass over the authoritative bytes (they are hashed) plus the signatures.
Signing costs one hash per part: blob names are already SHA-256 digests, so the writer reuses them,
and small parts are hashed while writing.

## What this does not prove

- **Identity.** A key is a key. Whether it belongs to a person, a team or a build server is a trust
  decision (pinning the key id, certificates, or a transparency log). The author text is not
  evidence.
- **A time.** `created` is the signer's clock. A trusted time needs a time-stamp authority; this
  profile carries none.
- **Completeness of a truncated log.** A bounded log drops its oldest records. The drop is
  signed and the report says the chain is truncated, but the records that are gone cannot be
  recovered from this file.
- **Immutability.** Anyone can edit the design, sign it with their own key, or remove the entry
  (the result is then unsigned, never "valid"). A signature is evidence a key holder signed
  something, not a lock.
- **The model cache.** See above: a verified signature authenticates the design definition and
  payloads, not evaluated geometry in a cache. Evaluate from the timeline when that matters.

## Using it

- **Preferences › Security**: *Sign saved designs with GnuPG* (off by default), the signing key
  (empty: GnuPG's default key) and *Check signatures when opening designs* (off by default; the
  tab tooltip and the status line then name the signer, and `file.verify` checks on request).
- **Commands**: `file.save`/`file.save_as` take `sign: true` and `signing_key` (a fingerprint or
  user id, empty for GnuPG's default). `file.verify` reports a file's signature. `file.gpg_keys`
  lists the secret keys GnuPG can sign with and marks those on a smartcard.
- **YubiKey / OpenPGP smartcard**: put the signing key in GnuPG's signing slot (the usual
  `gpg --card-status` setup) and leave the signing key empty or name its fingerprint. GnuPG, not
  SolidCraft, talks to the card and asks for the PIN through its pinentry; the save runs on a
  worker thread so the window keeps drawing, and the PIN prompt appears as GnuPG's own window.
  The secret key never leaves GnuPG or the card.
- **Browser**: signing needs the desktop app and GnuPG; verification runs in the browser too
  (`pgp`/rPGP, with the public key embedded in the record), so a design signed on a desktop can
  be checked anywhere.
- **What a save reports**: `file.save` returns `signature: {method, fingerprint, user_id?}` when
  it signed, and the status line says *Saved … signed with …*. A save without a key keeps the
  existing records and reports the newest one as covering an earlier version (`not_covered`).

## Compatibility

- An unsigned design has no entry; save and open are unchanged.
- A build that knows this entry keeps the chain when it saves without a signing key, so the newest
  record is reported as covering an earlier version rather than the history being destroyed.
- A build older than this format ignores the entry and opens the design. If it saves, it drops the
  entry, and the next signing build starts a new log; no record falsely claims the current
  edition, but history is lost on that save. Keep a signed copy if the history has to survive old
  builds.
- The container format version stays 1 because the entry is optional and additive. A future
  version that makes signing mandatory or changes `document.json` follows the container and design
  format rules (`crates/doc/src/format/pending/` for the design; a container version for the
  package).
- Encryption is a separate exchange artifact, not a property of this entry: a COSE envelope around
  a whole container, so payloads are not left in the clear. That is not part of this format.

## Implementation status

Version 1 is implemented in `crates/io/src/design/provenance.rs`:

- `Signer` — an Ed25519 software key signs COSE's `Sig_structure` bytes.
- `OpenPgpSigner` — a GnuPG key (possibly on a smartcard) signs a manifest and exports the public
  key that verifies it.
- `SignatureSource` — which kind of record a save appends.
- `ProvenanceSave` — an optional source (`None` keeps the chain but adds no record), the previous
  chain, author/tool/note and `created`.
- `write_design_container_with_provenance` — writes the container with one new record.
- `ProvenanceReport` / `ProvenanceStatus` / `ProvenanceRecord` / `ProvenanceSigner` /
  `ProvenanceChain` — the report, the signer, and the history to append to.
- `read_design_provenance`, `read_design_provenance_file` and
  `DesignContainerReader::provenance` — verify a file; `read_design_provenance_chain` and its
  file form read the history to append to without verifying the content.

GnuPG integration is in `crates/engine/src/gpg.rs` (desktop only): key listing, detached signing
(off the window thread, so pinentry can ask for the smartcard PIN), and public-key export. The
commands are `file.save`/`file.save_as` (`sign`, `signing_key`), `file.verify` and
`file.gpg_keys`. Preferences (`prefs.security.*`) are off by default.

Encryption is not part of this increment. It needs an encrypted file or an encrypted copy with an
explicit open path (including recovery), and is designed together with its indicator in the next
step.
