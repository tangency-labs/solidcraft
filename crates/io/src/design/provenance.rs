//! Provenance: signed records in a design container.
//!
//! A design's authoritative content is `document.json`, `container.json` and every other entry
//! beside the derived caches. Provenance binds exactly that content, not the ZIP layout, with one
//! chain of records stored in one entry, [`ENTRY`]. Each record signs a CBOR manifest (RFC 8949)
//! of:
//!
//! - the SHA-256 of the exact `document.json` bytes and of the exact `container.json` bytes;
//! - the name, length and SHA-256 of every other authoritative entry;
//! - the SHA-256 of the previous record's exact bytes, so removing, reordering or rewriting a
//!   record that has a successor breaks the chain. Removing the newest record does not: no later
//!   record links to it;
//! - the identity of the oldest record in the file when this record was written, so removing the
//!   start of the log is caught even though the log is bounded;
//! - who signed, when (the signer's clock, not authenticated), and with which key.
//!
//! Two record kinds are accepted. A **COSE_Sign1** record (RFC 9052) carries the manifest as its
//! payload and an Ed25519 signature over COSE's `Sig_structure`; verification is in-process and
//! needs no key distribution. An **OpenPGP** record carries the manifest, a detached OpenPGP
//! signature and the signer's transferable public key; GnuPG produces it (a key on a smartcard is
//! used where it lives, and never copied), and the signature is verified here against the
//! embedded key. `opgp` records carry no raw public key, so the same profile works for keys that
//! never leave a card.
//!
//! Not signed: `cache/` entries, previews, thumbnails, the `provenance/` entry itself, and the
//! ZIP directory. They are derived data or the signature itself; a design's geometry comes from
//! the signed document and payloads, not from a cache. A recipient that trusts a signature should
//! evaluate the design from its timeline rather than adopt a model cache.
//!
//! Provenance is not identity. A signature proves that the holder of a key signed a version of
//! the design; deciding whether that key belongs to the person it claims is the caller's job
//! (pinning, certificates or a transparency log). It also does not stop anyone editing the design
//! and signing it again. An entry removed from the file leaves an unsigned design; the report for
//! a missing entry is `None`, never "valid".
//!
//! The records are a bounded log. Each signed save appends a record, but a removed newest record
//! leaves no trace in the file. When the entry would exceed its budget the oldest
//! records are dropped; the newest record then names the new first record, and the first record
//! names a predecessor that is gone, so the report says the chain is truncated. A save without a
//! signer keeps the records it was given, so the newest record then covers an earlier version and
//! the report is [`ProvenanceStatus::NotCovered`]. A report this build cannot verify keeps no
//! chain, so the next save starts a new log instead of appending to records it cannot check.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek};
use std::marker::PhantomData;

use coset::{CoseSign1, CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
use ed25519_dalek::{Signature, VerifyingKey};
use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::types::KeyDetails;
use serde::de::{DeserializeOwned, Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use super::{PackageReader, PackageWriter, max_design, part_limit};
use crate::{IoError, Result};

/// The container entry holding the record chain.
pub const ENTRY: &str = "provenance/log.cbor";

/// The envelope's format. A later format changes this number and this build reports the file as
/// written by a newer build instead of misreading it.
const VERSION: u64 = 1;
const KIND: &str = "solidcraft-provenance";
const OPENPGP_KIND: &str = "openpgp";
const MANIFEST_VERSION: u64 = 1;
/// The COSE content type of the manifest payload.
const MANIFEST_CONTENT_TYPE: &str = "application/vnd.solidcraft.provenance-manifest+cbor";
/// Bounds on an untrusted entry. They are part of the format: a writer keeps the log inside them,
/// and the reader checks the bounds while decoding, not after allocating the whole collection.
const MAX_RECORDS: usize = 64;
const MAX_RECORD_BYTES: usize = 4 << 20;
const MAX_ENTRY_BYTES: usize = 8 << 20;
const MAX_PARTS: usize = 10_000;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_PART_NAME_BYTES: usize = 4096;
const MAX_OPENPGP_KEY_BYTES: usize = 1 << 20;
const MAX_OPENPGP_SIGNATURE_BYTES: usize = 64 << 10;
/// Deep enough for this envelope and its records, small enough that hostile CBOR cannot exhaust
/// the stack (the entry's size is capped too).
const CBOR_RECURSION: usize = 32;

/// A COSE key's contribution. Signing is over the bytes the caller supplies: COSE's
/// `Sig_structure`, not the manifest itself.
pub trait Signer {
    /// The 32-byte Ed25519 public key.
    fn public_key(&self) -> [u8; 32];
    /// Sign exactly `message` with Ed25519.
    fn sign(&self, message: &[u8]) -> Result<[u8; 64]>;
}

/// What an OpenPGP signer returns: the detached signature and the transferable public key that
/// verifies it, so the record needs no keyring later. GnuPG exports both; no secret key leaves the
/// smartcard.
pub struct OpenPgpSignature {
    /// Detached binary OpenPGP signature over the exact manifest bytes.
    pub signature: Vec<u8>,
    /// The signer's transferable public key (binary OpenPGP packets).
    pub key: Vec<u8>,
}

/// Signs a manifest with an OpenPGP key (through GnuPG, so a smartcard key stays on the card).
pub trait OpenPgpSigner {
    fn sign(&self, manifest: &[u8]) -> Result<OpenPgpSignature>;
}

/// What signs one new record.
pub enum SignatureSource<'a> {
    /// A software Ed25519 key through COSE_Sign1.
    Cose(&'a dyn Signer),
    /// A GnuPG/OpenPGP key, possibly on a smartcard.
    OpenPgp(&'a dyn OpenPgpSigner),
}

/// The records a file was opened with, oldest first. Pass the report's chain to keep a design's
/// history across saves. [`ProvenanceChain::truncated`] is not stored: the reader derives it from
/// the first record's signed predecessor link and the newest record's signed oldest-record link.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProvenanceChain {
    /// Exact record bytes. They are the bytes the chain links digest, so they are kept as they
    /// are.
    pub records: Vec<Vec<u8>>,
    /// The reader's finding that older records are gone. A writer recomputes it and ignores this
    /// field, so a caller cannot create truncation or hide it by editing it.
    pub truncated: bool,
}

/// One save's provenance: the signer, the history to append to, and the signer's statement.
pub struct ProvenanceSave<'a> {
    /// Signs one new record. `None` keeps `chain` as it is (an unsigned save) instead of signing;
    /// that is how a file saved without a key keeps the records it was opened with.
    pub source: Option<SignatureSource<'a>>,
    pub chain: &'a ProvenanceChain,
    /// A display name for the signer. The key, not this text, is what the signature proves.
    pub author: Option<&'a str>,
    /// What wrote the design, for the record (for example `desktop`, `cli`, `mcp`).
    pub tool: Option<&'a str>,
    /// What the signer says about this save. Not interpreted.
    pub note: Option<&'a str>,
    /// Unix seconds from the signer's clock. Not authenticated; a trusted time needs a time-stamp
    /// authority, which this profile does not carry.
    pub created: Option<u64>,
}

/// What the file says about its own provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvenanceStatus {
    /// Every record verifies, the chain is unbroken, and the newest record covers exactly this
    /// file's authoritative content. `chain.truncated` says whether older records are gone.
    Valid,
    /// Every record verifies and the chain is unbroken, but the newest record covers an earlier
    /// version: the design was saved again without a signing key.
    NotCovered,
    /// A record, link, signature or manifest is malformed or does not verify. The file may still
    /// open; the provenance cannot be trusted.
    Invalid(String),
    /// A newer SolidCraft wrote the entry, in a format this build does not read.
    Unsupported(u64),
}

/// The key a record was signed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvenanceSigner {
    /// A COSE Ed25519 signature. `key_id` is the SHA-256 of `public_key`.
    Ed25519 { public_key: [u8; 32], key_id: [u8; 32] },
    /// An OpenPGP signature, checked against the record's embedded public key. `fingerprint` is
    /// uppercase hexadecimal as GnuPG prints it; `user_id` is the key's primary user id.
    OpenPgp { fingerprint: String, user_id: Option<String> },
}

impl ProvenanceSigner {
    /// The key's fingerprint, for a local trust decision: an OpenPGP fingerprint, or the Ed25519
    /// key id in hex.
    pub fn fingerprint(&self) -> String {
        match self {
            Self::OpenPgp { fingerprint, .. } => fingerprint.clone(),
            Self::Ed25519 { key_id, .. } => {
                use std::fmt::Write as _;
                let mut hex = String::with_capacity(64);
                for byte in key_id {
                    // Writing to a `String` cannot fail.
                    let _ = write!(&mut hex, "{byte:02x}");
                }
                hex
            }
        }
    }

    /// The name to show for this signer: the OpenPGP user id where there is one, else its
    /// fingerprint, or the first eight bytes of an Ed25519 key id in hex.
    pub fn label(&self) -> String {
        match self {
            Self::OpenPgp { fingerprint, user_id } => user_id.clone().unwrap_or_else(|| fingerprint.clone()),
            Self::Ed25519 { key_id, .. } => {
                use std::fmt::Write as _;
                let mut hex = String::with_capacity(16);
                for byte in &key_id[..8] {
                    // Writing to a `String` cannot fail.
                    let _ = write!(&mut hex, "{byte:02x}");
                }
                hex
            }
        }
    }
}

/// One verified record's statement, as the file carries it. The strings are untrusted input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenanceRecord {
    pub signer: ProvenanceSigner,
    pub author: Option<String>,
    pub tool: Option<String>,
    pub note: Option<String>,
    pub created: Option<u64>,
    /// The application that wrote the record.
    pub app: String,
    /// The design format tag the signed document carries (`solidcraft/N` or a legacy tag).
    pub format: String,
    pub pending: Vec<String>,
    /// SHA-256 of the signed `document.json` and `container.json`.
    pub document: [u8; 32],
    pub container: [u8; 32],
    /// How many authoritative parts the record covers.
    pub parts: usize,
    /// The signature verifies over this record's exact bytes.
    pub signature_valid: bool,
    /// This record's digests and part list equal the file's current content.
    pub covered: bool,
}

/// The result of reading the entry. The records are oldest first; the last one is the newest save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenanceReport {
    /// The exact records, for the next save to append to. Empty when the status is not trusted.
    pub chain: ProvenanceChain,
    pub records: Vec<ProvenanceRecord>,
    pub status: ProvenanceStatus,
}

impl ProvenanceReport {
    /// The newest record, when at least one was readable.
    pub fn newest(&self) -> Option<&ProvenanceRecord> {
        self.records.last()
    }
}

/// What a design's provenance says before any signature is checked: how many records are there,
/// and the newest signer's display name (when its record can be read). Enough for a file-picking
/// list to say "signed by …" without reading the payloads; verification still happens on open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenanceSummary {
    pub records: usize,
    /// The newest signer's display name, when its record can be read.
    pub signer: Option<String>,
    /// The newest signer's key fingerprint, for a caller's own trust decision.
    pub fingerprint: Option<String>,
}

/// The summary of an entry already read, without verifying it.
pub(super) fn summarize_chain(chain: &ProvenanceChain) -> Option<ProvenanceSummary> {
    if chain.records.is_empty() {
        return None;
    }
    let signer = parse_chain(&chain.records).ok().and_then(|parsed| parsed.last().map(|(record, _)| record.signer.label()));
    let fingerprint = parse_chain(&chain.records).ok().and_then(|parsed| parsed.last().map(|(record, _)| record.signer.fingerprint()));
    Some(ProvenanceSummary { records: chain.records.len(), signer, fingerprint })
}

/// The entries a signature binds: the design itself, its payloads and anything a newer build kept
/// beside it. Derived caches and provenance records are excluded.
pub(super) fn is_authoritative(name: &str) -> bool {
    name != "document.json" && name != "container.json" && !name.starts_with("cache/") && !name.starts_with("provenance/")
}

/// The SHA-256 of `bytes`, as the manifest and the chain use it.
pub(super) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn invalid(message: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("provenance: {message}"))
}

/// A bounded CBOR sequence: a hostile item count is refused while decoding, before the vector
/// (and every item in it) is materialised.
fn bounded_seq<'de, D, T, const MAX: usize>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T, const MAX: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for Bounded<T, MAX> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "a sequence of at most {MAX} items")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Vec<T>, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<T>()? {
                if items.len() == MAX {
                    return Err(A::Error::custom(format!("more than {MAX} items")));
                }
                items.push(item);
            }
            Ok(items)
        }
    }
    deserializer.deserialize_seq(Bounded::<T, MAX>(PhantomData))
}

fn bounded_records<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<serde_bytes::ByteBuf>, D::Error> {
    bounded_seq::<D, serde_bytes::ByteBuf, MAX_RECORDS>(deserializer)
}

fn bounded_parts<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<PartEntry>, D::Error> {
    bounded_seq::<D, PartEntry, MAX_PARTS>(deserializer)
}

fn bounded_names<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<String>, D::Error> {
    bounded_seq::<D, String, MAX_PARTS>(deserializer)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> std::result::Result<T, String> {
    ciborium::de::from_reader_with_recursion_limit(bytes, CBOR_RECURSION).map_err(|error| error.to_string())
}

/// What a newer format says before this build tries to read its shape. Unknown fields are ignored
/// here on purpose: a version bump may change the envelope, and that is `Unsupported`, not invalid.
#[derive(Deserialize)]
struct VersionProbe {
    v: u64,
}

/// The CBOR envelope. `records` are exact record byte strings.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: u64,
    kind: String,
    #[serde(deserialize_with = "bounded_records")]
    records: Vec<serde_bytes::ByteBuf>,
}

/// One record's signed CBOR manifest. Optional statement fields are absent when unset; an
/// alternative spelling with `null` is not written (a reader accepts it through `Option`).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    v: u64,
    document: serde_bytes::ByteBuf,
    container: serde_bytes::ByteBuf,
    #[serde(deserialize_with = "bounded_parts")]
    parts: Vec<PartEntry>,
    format: String,
    #[serde(default, deserialize_with = "bounded_names")]
    pending: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prev: Option<serde_bytes::ByteBuf>,
    /// The SHA-256 of the first record in the file this record was written into, when that file
    /// held earlier records. It pins a bounded log to its current start, so removing the start is
    /// detectable even though the envelope does not repeat a truncation flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    oldest: Option<serde_bytes::ByteBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<u64>,
    #[serde(default)]
    app: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    /// A COSE record's Ed25519 public key. An OpenPGP record carries its key beside the signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    public_key: Option<serde_bytes::ByteBuf>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct PartEntry {
    name: String,
    len: u64,
    sha256: serde_bytes::ByteBuf,
}

impl PartEntry {
    fn from_digest(part: solidcraft_format::container::PartDigest) -> PartEntry {
        PartEntry { name: part.name, len: part.len, sha256: serde_bytes::ByteBuf::from(part.digest.to_vec()) }
    }
}

/// An OpenPGP record's exact bytes: the manifest, a detached signature over it, and the public
/// key that verifies it.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenPgpWire {
    v: u64,
    kind: String,
    manifest: serde_bytes::ByteBuf,
    signature: serde_bytes::ByteBuf,
    key: serde_bytes::ByteBuf,
}

fn digest_of(value: &serde_bytes::ByteBuf, what: &str) -> std::result::Result<[u8; 32], String> {
    value.as_ref().try_into().map_err(|_| format!("{what} is not a SHA-256 digest"))
}

fn bounded(value: &str, what: &str, max: usize) -> std::result::Result<(), String> {
    if value.len() > max {
        return Err(format!("{what} exceeds its budget"));
    }
    Ok(())
}

fn encode_envelope(records: &[Vec<u8>]) -> Result<Vec<u8>> {
    let envelope = Envelope { v: VERSION, kind: KIND.to_owned(), records: records.iter().cloned().map(serde_bytes::ByteBuf::from).collect() };
    let mut encoded = Vec::new();
    ciborium::ser::into_writer(&envelope, &mut encoded).map_err(|error| invalid(format!("envelope encoding failed: {error}")))?;
    Ok(encoded)
}

/// Write the entry for one save. With a source, one record is appended to `save.chain`; without
/// one, the chain is written back unchanged (so an unsigned save does not destroy the history).
/// `document` and `container` are the exact bytes written. The caller must have recorded the
/// authoritative parts with [`PackageWriter::track_parts`].
pub(super) fn write(
    save: &ProvenanceSave<'_>,
    writer: &mut PackageWriter,
    document: &[u8],
    container: &[u8],
    format: &str,
    pending: &[String],
) -> Result<()> {
    let mut previous = save.chain.records.clone();
    validate_chain(&previous).map_err(|why| invalid(format!("the chain to append to is not valid: {why}")))?;
    let Some(source) = &save.source else {
        // An unsigned save keeps the records it opened with; the newest record then covers an
        // earlier version. An empty chain adds nothing, as before provenance existed.
        if previous.is_empty() {
            return Ok(());
        }
        let encoded = encode_envelope(&previous)?;
        if encoded.len() > MAX_ENTRY_BYTES {
            return Err(invalid("the provenance log no longer fits the container"));
        }
        writer.part(ENTRY, &encoded, true)?;
        return Ok(());
    };
    bounded(format, "the format tag", MAX_TEXT_BYTES).map_err(invalid)?;
    if pending.len() > MAX_PARTS {
        return Err(invalid("the pending format list is too long"));
    }
    for name in pending {
        bounded(name, "a pending format name", MAX_TEXT_BYTES).map_err(invalid)?;
    }
    for value in [save.author, save.tool, save.note].into_iter().flatten() {
        bounded(value, "a signed note", MAX_TEXT_BYTES).map_err(invalid)?;
    }
    let mut parts = writer.take_parts().ok_or_else(|| invalid("provenance parts were not recorded"))?;
    if parts.len() > MAX_PARTS {
        return Err(invalid("too many parts to sign"));
    }
    parts.sort_by(|a, b| a.name.cmp(&b.name));
    if parts.iter().any(|part| part.name.is_empty() || part.name.len() > MAX_PART_NAME_BYTES) {
        return Err(invalid("a part name is empty or too long to sign"));
    }
    let parts: Vec<PartEntry> = parts.into_iter().map(PartEntry::from_digest).collect();
    let document_digest = sha256(document);
    let container_digest = sha256(container);
    // Make room for the record this save appends. The drop is visible through the newest record's
    // `oldest` link, which the loop below recomputes.
    while previous.len() >= MAX_RECORDS {
        previous.remove(0);
    }
    loop {
        let prev = previous.last().map(|record| sha256(record));
        let oldest = previous.first().map(|record| sha256(record));
        let manifest = Manifest {
            v: MANIFEST_VERSION,
            document: serde_bytes::ByteBuf::from(document_digest.to_vec()),
            container: serde_bytes::ByteBuf::from(container_digest.to_vec()),
            parts: parts.clone(),
            format: format.to_owned(),
            pending: pending.to_vec(),
            prev: prev.map(|digest| serde_bytes::ByteBuf::from(digest.to_vec())),
            oldest: oldest.map(|digest| serde_bytes::ByteBuf::from(digest.to_vec())),
            created: save.created,
            app: env!("CARGO_PKG_VERSION").to_owned(),
            author: save.author.map(str::to_owned),
            tool: save.tool.map(str::to_owned),
            note: save.note.map(str::to_owned),
            public_key: None,
        };
        let mut manifest = manifest;
        let record = match source {
            SignatureSource::Cose(signer) => {
                let public_key = signer.public_key();
                let kid = sha256(&public_key);
                manifest.public_key = Some(serde_bytes::ByteBuf::from(public_key.to_vec()));
                let mut payload = Vec::new();
                ciborium::ser::into_writer(&manifest, &mut payload).map_err(|error| invalid(format!("manifest encoding failed: {error}")))?;
                make_cose_record(*signer, payload, kid)?
            }
            SignatureSource::OpenPgp(signer) => {
                let mut payload = Vec::new();
                ciborium::ser::into_writer(&manifest, &mut payload).map_err(|error| invalid(format!("manifest encoding failed: {error}")))?;
                let signed = signer.sign(&payload)?;
                if signed.key.len() > MAX_OPENPGP_KEY_BYTES {
                    return Err(invalid("the OpenPGP public key is too large to store"));
                }
                if signed.signature.len() > MAX_OPENPGP_SIGNATURE_BYTES {
                    return Err(invalid("the OpenPGP signature is too large to store"));
                }
                let wire = OpenPgpWire {
                    v: MANIFEST_VERSION,
                    kind: OPENPGP_KIND.to_owned(),
                    manifest: serde_bytes::ByteBuf::from(payload),
                    signature: serde_bytes::ByteBuf::from(signed.signature),
                    key: serde_bytes::ByteBuf::from(signed.key),
                };
                let mut encoded = Vec::new();
                ciborium::ser::into_writer(&wire, &mut encoded).map_err(|error| invalid(format!("OpenPGP record encoding failed: {error}")))?;
                encoded
            }
        };
        let mut records = previous.clone();
        records.push(record);
        // Verify what is about to be written, the same way a reader will: a signer that does not
        // match its own key, or a record past a bound, fails the save instead of writing a log no
        // reader can use. This also enforces the record's size bound.
        validate_chain(&records).map_err(|why| invalid(format!("the signed record would not read back: {why}")))?;
        let encoded = encode_envelope(&records)?;
        if encoded.len() <= MAX_ENTRY_BYTES {
            writer.part(ENTRY, &encoded, true)?;
            return Ok(());
        }
        // The entry is over budget: drop the oldest record and sign the record that names the new
        // start. The drop is visible to a reader through the newest record's `oldest` link.
        if previous.is_empty() {
            return Err(invalid("the signed record would exceed the provenance budget"));
        }
        previous.remove(0);
    }
}

/// Build one tagged COSE_Sign1 record. `payload` is the manifest bytes; the signature covers
/// COSE's `Sig_structure`, as `coset` computes it.
fn make_cose_record(signer: &dyn Signer, payload: Vec<u8>, kid: [u8; 32]) -> Result<Vec<u8>> {
    let protected =
        HeaderBuilder::new().algorithm(iana::Algorithm::EdDSA).key_id(kid.to_vec()).content_type(MANIFEST_CONTENT_TYPE.to_owned()).build();
    let builder = CoseSign1Builder::new()
        .protected(protected)
        .payload(payload)
        .try_create_signature(&[], |tbs| signer.sign(tbs).map(|signature| signature.to_vec()))?;
    builder.build().to_tagged_vec().map_err(|error| invalid(format!("record encoding failed: {error}")))
}

/// Check `records` structurally: bounds, signatures, every `prev` link, the first record's
/// identity and the newest record's `oldest` pin. Content coverage is checked separately, since a
/// writer only needs the structure.
fn parse_chain(raw: &[Vec<u8>]) -> std::result::Result<Vec<(ProvenanceRecord, Manifest)>, String> {
    if raw.is_empty() {
        return Err("the entry has no records".into());
    }
    if raw.len() > MAX_RECORDS {
        return Err("too many records".into());
    }
    let mut parsed = Vec::with_capacity(raw.len());
    let mut previous: Option<[u8; 32]> = None;
    for (index, bytes) in raw.iter().enumerate() {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(format!("record {index} is too large"));
        }
        let (record, manifest) = read_record(bytes).map_err(|why| format!("record {index}: {why}"))?;
        let prev = match manifest.prev.as_ref() {
            Some(prev) => Some(digest_of(prev, "the previous record digest")?),
            None => None,
        };
        if index == 0 {
            // A file of one record must be a genesis. A longer file may have lost its start,
            // which the newest record's `oldest` link has to name.
            if raw.len() == 1 && (prev.is_some() || manifest.oldest.is_some()) {
                return Err("a single record names a predecessor or an older record".into());
            }
        } else if prev != previous {
            return Err(format!("record {index} does not link to the record before it"));
        }
        previous = Some(sha256(bytes));
        parsed.push((record, manifest));
    }
    if raw.len() > 1 {
        let oldest = parsed.last().and_then(|(_, manifest)| manifest.oldest.as_ref()).ok_or("the newest record does not name the first record")?;
        let oldest = digest_of(oldest, "the oldest record digest")?;
        if oldest != sha256(&raw[0]) {
            return Err("the newest record does not name this file's first record".into());
        }
    }
    Ok(parsed)
}

/// Validate a chain a caller supplies for an unsigned save or before appending. An empty chain is
/// a design without provenance, not an error.
fn validate_chain(raw: &[Vec<u8>]) -> std::result::Result<(), String> {
    if raw.is_empty() {
        return Ok(());
    }
    parse_chain(raw).map(|_| ())
}

/// The records an entry carries, with their links and manifests checked but their signatures not
/// verified: a save appends to this. An unreadable, newer, or structurally invalid entry yields
/// `None`, so the next save starts a new log instead of trying to append to a chain that cannot
/// validate.
pub(super) fn read_chain<R: Read + Seek>(reader: &mut PackageReader<R>) -> Option<ProvenanceChain> {
    if !reader.has_part(ENTRY) {
        return None;
    }
    let bytes = reader.limited_part(ENTRY, MAX_ENTRY_BYTES).ok()?;
    if decode::<VersionProbe>(&bytes).ok()?.v != VERSION {
        return None;
    }
    let envelope: Envelope = decode(&bytes).ok()?;
    if envelope.kind != KIND {
        return None;
    }
    let records: Vec<Vec<u8>> = envelope.records.iter().map(|record| record.to_vec()).collect();
    // A chain whose links or manifests do not validate cannot be appended to; dropping it here
    // means opening the file still works and the next container save starts a new log.
    validate_chain(&records).ok()?;
    Some(ProvenanceChain { records, truncated: false })
}

/// Verify the entry against the file it is in. `None` means the file has no provenance entry; an
/// entry that cannot be read or does not verify is a report, never an error, so a design still
/// opens.
pub(super) fn verify<R: Read + Seek>(reader: &mut PackageReader<R>) -> Option<ProvenanceReport> {
    if !reader.has_part(ENTRY) {
        return None;
    }
    let bytes = match reader.limited_part(ENTRY, MAX_ENTRY_BYTES) {
        Ok(bytes) => bytes,
        Err(why) => return Some(report_invalid(format!("cannot read the entry: {why}"))),
    };
    // Ask the version first, with unknown fields ignored: a newer envelope may have a different
    // shape, and guessing at it would report a valid newer file as corrupt.
    let probe: VersionProbe = match decode(&bytes) {
        Ok(probe) => probe,
        Err(why) => return Some(report_invalid(format!("the entry is not valid CBOR: {why}"))),
    };
    if probe.v > VERSION {
        return Some(ProvenanceReport { chain: ProvenanceChain::default(), records: Vec::new(), status: ProvenanceStatus::Unsupported(probe.v) });
    }
    if probe.v != VERSION {
        return Some(report_invalid(format!("unsupported entry version {}", probe.v)));
    }
    let envelope: Envelope = match decode(&bytes) {
        Ok(envelope) => envelope,
        Err(why) => return Some(report_invalid(format!("the entry is not a version {VERSION} provenance log: {why}"))),
    };
    if envelope.kind != KIND {
        return Some(report_invalid("the entry is not a SolidCraft provenance log".to_owned()));
    }
    let content = match read_content(reader) {
        Ok(content) => content,
        Err(why) => return Some(report_invalid(format!("cannot bind the content: {why}"))),
    };
    let raw: Vec<Vec<u8>> = envelope.records.iter().map(|record| record.to_vec()).collect();
    match parse_chain(&raw) {
        Ok(parsed) => {
            let truncated = parsed.first().is_some_and(|(_, manifest)| manifest.prev.is_some());
            let mut records = Vec::with_capacity(parsed.len());
            for (mut record, manifest) in parsed {
                match covers(&manifest, &content) {
                    Ok(covered) => record.covered = covered,
                    Err(why) => return Some(report_invalid(why)),
                }
                records.push(record);
            }
            let covered = records.last().is_some_and(|record| record.covered);
            let status = if covered { ProvenanceStatus::Valid } else { ProvenanceStatus::NotCovered };
            Some(ProvenanceReport { chain: ProvenanceChain { records: raw, truncated }, records, status })
        }
        Err(why) => Some(report_invalid(why)),
    }
}

/// The authoritative content a manifest must cover.
struct Content {
    document: [u8; 32],
    container: [u8; 32],
    parts: BTreeMap<String, (u64, [u8; 32])>,
}

fn read_content<R: Read + Seek>(reader: &mut PackageReader<R>) -> std::result::Result<Content, String> {
    let document = reader.document(max_design()).map_err(|e| format!("document: {e}"))?;
    let container = reader.limited_part("container.json", 4096).map_err(|e| format!("container: {e}"))?;
    let names = reader.part_names();
    if names.len() > MAX_PARTS {
        return Err("too many entries".into());
    }
    let mut parts = BTreeMap::new();
    for name in names {
        if !is_authoritative(&name) {
            continue;
        }
        let bytes = reader.limited_part(&name, part_limit(&name)).map_err(|e| format!("part `{name}`: {e}"))?;
        parts.insert(name, (bytes.len() as u64, sha256(&bytes)));
    }
    Ok(Content { document: sha256(&document), container: sha256(&container), parts })
}

fn report_invalid(why: String) -> ProvenanceReport {
    ProvenanceReport { chain: ProvenanceChain::default(), records: Vec::new(), status: ProvenanceStatus::Invalid(why) }
}

/// Whether a record's manifest covers exactly the file's authoritative content.
fn covers(manifest: &Manifest, content: &Content) -> std::result::Result<bool, String> {
    let document = digest_of(&manifest.document, "the document digest")?;
    let container = digest_of(&manifest.container, "the container digest")?;
    if document != content.document || container != content.container || manifest.parts.len() != content.parts.len() {
        return Ok(false);
    }
    for part in &manifest.parts {
        let digest = digest_of(&part.sha256, "a part digest")?;
        match content.parts.get(&part.name) {
            Some((len, actual)) if *len == part.len && *actual == digest => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// Parse and verify one record: a COSE_Sign1 (first CBOR byte is its tag 18) or an OpenPGP record.
fn read_record(bytes: &[u8]) -> std::result::Result<(ProvenanceRecord, Manifest), String> {
    if bytes.first() == Some(&0xd2) { read_cose_record(bytes) } else { read_openpgp_record(bytes) }
}

/// Validate the manifest fields every record kind shares.
fn check_manifest(manifest: &Manifest) -> std::result::Result<(), String> {
    if manifest.v != MANIFEST_VERSION {
        return Err("unsupported manifest version".into());
    }
    bounded(&manifest.format, "the format tag", MAX_TEXT_BYTES)?;
    for value in [manifest.author.as_deref(), manifest.tool.as_deref(), manifest.note.as_deref(), Some(manifest.app.as_str())].into_iter().flatten() {
        bounded(value, "a signed note", MAX_TEXT_BYTES)?;
    }
    if manifest.pending.len() > MAX_PARTS {
        return Err("too many pending format names".into());
    }
    for name in &manifest.pending {
        bounded(name, "a pending format name", MAX_TEXT_BYTES)?;
    }
    if manifest.parts.len() > MAX_PARTS {
        return Err("too many parts".into());
    }
    let mut part_names = BTreeSet::new();
    for part in &manifest.parts {
        bounded(&part.name, "a part name", MAX_PART_NAME_BYTES)?;
        if part.name.is_empty() || !is_authoritative(&part.name) {
            return Err("a signed part is not authoritative".into());
        }
        if !part_names.insert(part.name.as_str()) {
            return Err("a signed part is listed twice".into());
        }
        let _ = digest_of(&part.sha256, "a part digest")?;
    }
    let _ = digest_of(&manifest.document, "the document digest")?;
    let _ = digest_of(&manifest.container, "the container digest")?;
    Ok(())
}

fn record_fields(manifest: &Manifest, signer: ProvenanceSigner) -> std::result::Result<ProvenanceRecord, String> {
    Ok(ProvenanceRecord {
        signer,
        author: manifest.author.clone(),
        tool: manifest.tool.clone(),
        note: manifest.note.clone(),
        created: manifest.created,
        app: manifest.app.clone(),
        format: manifest.format.clone(),
        pending: manifest.pending.clone(),
        document: digest_of(&manifest.document, "the document digest")?,
        container: digest_of(&manifest.container, "the container digest")?,
        parts: manifest.parts.len(),
        signature_valid: true,
        covered: false,
    })
}

/// A COSE_Sign1 record: the manifest is the payload, signed with EdDSA.
fn read_cose_record(bytes: &[u8]) -> std::result::Result<(ProvenanceRecord, Manifest), String> {
    let record = CoseSign1::from_tagged_slice(bytes).map_err(|error| format!("not COSE_Sign1: {error}"))?;
    if record.protected.header.alg != Some(iana::Algorithm::EdDSA.into()) {
        return Err("not signed with EdDSA".into());
    }
    let header = &record.protected.header;
    if !header.crit.is_empty()
        || !header.rest.is_empty()
        || !header.iv.is_empty()
        || !header.partial_iv.is_empty()
        || !header.counter_signatures.is_empty()
        || record.unprotected != coset::Header::default()
    {
        return Err("unexpected headers".into());
    }
    if header.content_type != Some(coset::ContentType::Text(MANIFEST_CONTENT_TYPE.to_owned())) {
        return Err("wrong content type".into());
    }
    let payload = record.payload.as_ref().ok_or("no payload")?;
    let manifest: Manifest = decode(payload).map_err(|error| format!("payload is not a manifest: {error}"))?;
    check_manifest(&manifest)?;
    let public_key = manifest.public_key.as_ref().ok_or("a COSE record needs its public key")?;
    let public_key = digest_of(public_key, "the public key")?;
    let kid = sha256(&public_key);
    if header.key_id != kid.to_vec() {
        return Err("the key id does not match the public key".into());
    }
    let signature_bytes: [u8; 64] = record.signature.as_slice().try_into().map_err(|_| "signature is not 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&signature_bytes);
    let verifying = VerifyingKey::from_bytes(&public_key).map_err(|_| "invalid public key".to_string())?;
    record
        .verify_signature(&[], |_signature, tbs| verifying.verify_strict(tbs, &signature).map_err(|_| ()))
        .map_err(|()| "the signature does not verify".to_string())?;
    let record = record_fields(&manifest, ProvenanceSigner::Ed25519 { public_key, key_id: kid })?;
    Ok((record, manifest))
}

/// An OpenPGP record: the manifest and a detached signature over it, verified against the
/// embedded transferable public key.
fn read_openpgp_record(bytes: &[u8]) -> std::result::Result<(ProvenanceRecord, Manifest), String> {
    let wire: OpenPgpWire = decode(bytes).map_err(|error| format!("not a provenance record: {error}"))?;
    if wire.v != MANIFEST_VERSION || wire.kind != OPENPGP_KIND {
        return Err("unsupported OpenPGP record version".into());
    }
    if wire.key.len() > MAX_OPENPGP_KEY_BYTES || wire.signature.len() > MAX_OPENPGP_SIGNATURE_BYTES {
        return Err("the OpenPGP key or signature is too large".into());
    }
    let manifest: Manifest = decode(&wire.manifest).map_err(|error| format!("payload is not a manifest: {error}"))?;
    check_manifest(&manifest)?;
    let (fingerprint, user_id) = verify_openpgp(&wire.key, &wire.signature, &wire.manifest)?;
    let record = record_fields(&manifest, ProvenanceSigner::OpenPgp { fingerprint, user_id })?;
    Ok((record, manifest))
}

/// Verify a detached OpenPGP signature over `message` with the record's transferable public key.
/// The embedded key's self-signatures and a signing subkey's binding (and back-signature) are
/// checked; whether the primary key belongs to the person it names is the reader's trust
/// decision.
fn verify_openpgp(key: &[u8], signature: &[u8], message: &[u8]) -> std::result::Result<(String, Option<String>), String> {
    let (key, _) = SignedPublicKey::from_reader_single(Cursor::new(key)).map_err(|error| format!("public key: {error}"))?;
    let (signature, _) = DetachedSignature::from_reader_single(Cursor::new(signature)).map_err(|error| format!("signature: {error}"))?;
    // The embedded key must be internally coherent: its self-signatures, and a signing subkey's
    // binding to the primary key (and back-signature), have to verify. A signature is reported
    // under the primary key's fingerprint, so trusting the primary covers its bound subkeys.
    key.verify_bindings().map_err(|error| format!("the OpenPGP key bindings do not verify: {error}"))?;
    let mut verified = signature.verify(&key, message).is_ok();
    if !verified {
        for subkey in &key.public_subkeys {
            if signature.verify(subkey, message).is_ok() {
                subkey.verify_bindings(&key.primary_key).map_err(|error| format!("the OpenPGP subkey binding does not verify: {error}"))?;
                verified = true;
                break;
            }
        }
    }
    if !verified {
        return Err("the OpenPGP signature does not verify".into());
    }
    let fingerprint = format!("{:X}", key.fingerprint());
    let user_id = key.details.users.first().and_then(|user| user.id.as_str()).map(str::to_owned);
    Ok((fingerprint, user_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_design, write_design_container};
    use ed25519_dalek::Signer as DalekSigner;
    use pgp::crypto::hash::HashAlgorithm;
    use pgp::ser::Serialize as PgpSerialize;
    use rand::SeedableRng;
    use solidcraft_doc::Document;
    use std::collections::BTreeMap;
    use std::io::{Cursor, Write};

    struct TestSigner(ed25519_dalek::SigningKey);

    impl TestSigner {
        fn new(seed: u8) -> TestSigner {
            TestSigner(ed25519_dalek::SigningKey::from_bytes(&[seed; 32]))
        }
    }

    impl Signer for TestSigner {
        fn public_key(&self) -> [u8; 32] {
            self.0.verifying_key().to_bytes()
        }
        fn sign(&self, message: &[u8]) -> Result<[u8; 64]> {
            Ok(DalekSigner::sign(&self.0, message).to_bytes())
        }
    }

    struct BadSigner(ed25519_dalek::SigningKey);

    impl Signer for BadSigner {
        fn public_key(&self) -> [u8; 32] {
            self.0.verifying_key().to_bytes()
        }
        fn sign(&self, _message: &[u8]) -> Result<[u8; 64]> {
            Ok([0; 64])
        }
    }

    /// A software OpenPGP key made for these tests, so a record can be signed without GnuPG.
    struct TestPgpSigner {
        key: pgp::composed::SignedSecretKey,
        public: Vec<u8>,
        sign_with_subkey: bool,
    }

    impl TestPgpSigner {
        fn new(seed: u64) -> TestPgpSigner {
            TestPgpSigner::build(seed, false, false)
        }

        /// A key whose signing subkey signs, optionally with the subkey's binding removed.
        fn with_signing_subkey(seed: u64, unbound: bool) -> TestPgpSigner {
            TestPgpSigner::build(seed, true, unbound)
        }

        fn build(seed: u64, subkey: bool, unbound: bool) -> TestPgpSigner {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mut params = pgp::composed::SecretKeyParamsBuilder::default();
            params.key_type(pgp::composed::KeyType::Ed25519Legacy).can_certify(false).can_sign(true);
            params.primary_user_id("SolidCraft Test <test@solidcraft.invalid>".into());
            if subkey {
                params.subkey(
                    pgp::composed::SubkeyParamsBuilder::default().key_type(pgp::composed::KeyType::Ed25519Legacy).can_sign(true).build().unwrap(),
                );
            }
            let key = params.build().unwrap().generate(&mut rng).unwrap();
            let mut public_key = key.to_public_key();
            if unbound {
                for subkey in &mut public_key.public_subkeys {
                    subkey.signatures.clear();
                }
            }
            let public = PgpSerialize::to_bytes(&public_key).unwrap();
            TestPgpSigner { key, public, sign_with_subkey: subkey }
        }

        /// The same key's public form with the signing subkey's bindings removed.
        fn unbound_public(&self) -> Vec<u8> {
            let mut public_key = self.key.to_public_key();
            for subkey in &mut public_key.public_subkeys {
                subkey.signatures.clear();
            }
            PgpSerialize::to_bytes(&public_key).unwrap()
        }
    }

    impl OpenPgpSigner for TestPgpSigner {
        fn sign(&self, manifest: &[u8]) -> Result<OpenPgpSignature> {
            let mut rng = rand::thread_rng();
            let password = pgp::types::Password::empty();
            let signature = if self.sign_with_subkey {
                DetachedSignature::sign_binary_data(&mut rng, &self.key.secret_subkeys[0].key, &password, HashAlgorithm::Sha256, manifest)
            } else {
                DetachedSignature::sign_binary_data(&mut rng, &self.key.primary_key, &password, HashAlgorithm::Sha256, manifest)
            }
            .map_err(|error| invalid(format!("test signature: {error}")))?;
            let signature = PgpSerialize::to_bytes(&signature).map_err(|error| invalid(format!("test signature: {error}")))?;
            Ok(OpenPgpSignature { signature, key: self.public.clone() })
        }
    }

    fn save<'a>(
        source: Option<SignatureSource<'a>>,
        chain: &'a ProvenanceChain,
        author: Option<&'a str>,
        created: Option<u64>,
    ) -> ProvenanceSave<'a> {
        ProvenanceSave { source, chain, author, tool: Some("test"), note: None, created }
    }

    fn signed(doc: &Document, signer: &TestSigner, chain: &ProvenanceChain) -> Vec<u8> {
        crate::write_design_container_with_provenance(
            doc,
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::Cose(signer)), chain, Some("Ada"), Some(1_700_000_000)),
        )
        .unwrap()
    }

    fn signed_pgp(doc: &Document, signer: &TestPgpSigner, chain: &ProvenanceChain) -> Vec<u8> {
        crate::write_design_container_with_provenance(
            doc,
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::OpenPgp(signer)), chain, Some("YubiKey holder"), Some(1_700_000_000)),
        )
        .unwrap()
    }

    fn unsigned_save(doc: &Document, chain: &ProvenanceChain) -> Vec<u8> {
        crate::write_design_container_with_provenance(doc, crate::DesignCompression::Fast, None, None, None, &save(None, chain, None, None)).unwrap()
    }

    fn report(bytes: &[u8]) -> Option<ProvenanceReport> {
        crate::read_design_provenance(bytes)
    }

    fn read_entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut entries = BTreeMap::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            entries.insert(entry.name().unwrap().into_owned(), data);
        }
        entries
    }

    fn repack(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn envelope(bytes: &[u8]) -> Envelope {
        decode(bytes).unwrap()
    }

    fn encode(envelope: &Envelope) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(envelope, &mut bytes).unwrap();
        bytes
    }

    fn replace_entry(bytes: &[u8], name: &str, data: &[u8]) -> Vec<u8> {
        let mut entries = read_entries(bytes);
        entries.insert(name.to_owned(), data.to_vec());
        repack(&entries)
    }

    #[derive(Serialize)]
    struct RawEnvelope {
        v: u64,
        kind: String,
        records: Vec<serde_bytes::ByteBuf>,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_field: Option<u64>,
    }

    fn raw_envelope(records: Vec<Vec<u8>>, version: u64) -> Vec<u8> {
        let raw = RawEnvelope {
            v: version,
            kind: KIND.to_owned(),
            records: records.into_iter().map(serde_bytes::ByteBuf::from).collect(),
            new_field: (version > VERSION).then_some(1),
        };
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&raw, &mut bytes).unwrap();
        bytes
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        let bytes = hex.as_bytes();
        let mut out = Vec::with_capacity(bytes.len() / 2);
        let mut index = 0;
        let nibble = |byte: u8| match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => 0,
        };
        while index + 1 < bytes.len() {
            out.push((nibble(bytes[index]) << 4) | nibble(bytes[index + 1]));
            index += 2;
        }
        out
    }

    /// A key and a detached signature made by GnuPG 2.5 with an Ed25519 key, over
    /// [`GNUPG_MANIFEST`]. The test proves signatures made by GnuPG (the tool a smartcard uses),
    /// not only by the software signer above.
    const GNUPG_KEY: &str = "9833046af51ed316092b06010401da470f010107409364c141ee4008c6ed8f3b302141e4f3450d6eea7ed026f2f676239e8a2a66b9b429536f6c696443726166742054657374203c7465737440736f6c696463726166742e696e76616c69643e88af0413160a0057162104900d0fc44f3044c168362cb200187b908ed1dab405026af51ed31b14800000000004000e6d616e75322c322e352b312e31322c302c33021b03050b0908070202220206150a09080b020416020301021e07021780000a091000187b908ed1dab4fb2a00ff6926fd5ff3f0d9d8ccef340f691b1666b2bc7207527b790ad086a22964a8db0300ff7c02f0fe395e622cae1bf20e1904af348f962f598cc66ddf3427a5432b0b510b";
    const GNUPG_SIGNATURE: &str = "8891040016080039162104900d0fc44f3044c168362cb200187b908ed1dab405026af51ed41b14800000000004000e6d616e75322c322e352b312e31322c302c33000a091000187b908ed1dab408ad0100b35fc19bc33b819684c916ea412de843e9600932a90e339dbc671fad2c7be0bb00fd164d7e30302521a6936a6ac8f6e002fdaf9b5e12ed389de32e62c214a20c6601";
    const GNUPG_MANIFEST: &[u8] = b"solidcraft-provenance openpgp test manifest 2026-11";

    #[test]
    fn gnupg_ed25519_detached_signature_verifies() {
        let (fingerprint, user_id) = verify_openpgp(&hex_bytes(GNUPG_KEY), &hex_bytes(GNUPG_SIGNATURE), GNUPG_MANIFEST).unwrap();
        assert_eq!(fingerprint, "900D0FC44F3044C168362CB200187B908ED1DAB4");
        assert_eq!(user_id.as_deref(), Some("SolidCraft Test <test@solidcraft.invalid>"));
    }

    #[test]
    fn round_trip_signs_the_authoritative_parts_and_reopens_the_design() {
        let doc = Document::new("Plate");
        let signer = TestSigner::new(7);
        let bytes = signed(&doc, &signer, &ProvenanceChain::default());
        let report = report(&bytes).unwrap();
        assert_eq!(report.status, ProvenanceStatus::Valid);
        assert_eq!(report.records.len(), 1);
        let record = &report.records[0];
        assert_eq!(record.signer, ProvenanceSigner::Ed25519 { public_key: signer.public_key(), key_id: sha256(&signer.public_key()) });
        assert_eq!(record.author.as_deref(), Some("Ada"));
        assert_eq!(record.created, Some(1_700_000_000));
        assert_eq!(record.format, format!("solidcraft/{}", solidcraft_doc::format::FORMAT));
        assert!(record.signature_valid && record.covered);
        assert_eq!(report.chain.records.len(), 1);
        assert!(!report.chain.truncated);
        assert_eq!(read_design(&bytes).unwrap().name, doc.name);
    }

    #[test]
    fn an_openpgp_record_round_trips_and_names_its_key() {
        let doc = Document::new("Plate");
        let signer = TestPgpSigner::new(11);
        let bytes = signed_pgp(&doc, &signer, &ProvenanceChain::default());
        let report = report(&bytes).unwrap();
        assert_eq!(report.status, ProvenanceStatus::Valid);
        let record = report.newest().unwrap();
        match &record.signer {
            ProvenanceSigner::OpenPgp { fingerprint, user_id } => {
                assert_eq!(fingerprint.len(), 40, "a v4 fingerprint is 40 hex digits");
                assert_eq!(user_id.as_deref(), Some("SolidCraft Test <test@solidcraft.invalid>"));
            }
            other => panic!("expected an OpenPGP signer, got {other:?}"),
        }
        assert_eq!(read_design(&bytes).unwrap().name, "Plate");
    }

    #[test]
    fn a_summary_names_the_newest_signer_without_verifying_the_payloads() {
        let bytes = signed_pgp(&Document::new("Plate"), &TestPgpSigner::new(11), &ProvenanceChain::default());
        let mut reader = crate::DesignContainerReader::new(Cursor::new(bytes)).unwrap();
        let chain = reader.provenance_chain().unwrap();
        let summary = summarize_chain(&chain).unwrap();
        assert_eq!(summary.records, 1);
        assert!(summary.signer.as_deref().is_some_and(|signer| signer.contains("SolidCraft Test")), "{:?}", summary.signer);
    }

    #[test]
    fn a_chain_whose_start_was_removed_is_not_kept_for_the_next_save() {
        let signer = TestSigner::new(7);
        let first = signed(&Document::new("Plate"), &signer, &ProvenanceChain::default());
        let chain = report(&first).unwrap().chain;
        let second = crate::write_design_container_with_provenance(
            &Document::new("Plate v2"),
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::Cose(&TestSigner::new(9))), &chain, None, None),
        )
        .unwrap();
        let entries = read_entries(&second);
        let env = envelope(entries.get(ENTRY).unwrap());
        let broken = replace_entry(&second, ENTRY, &raw_envelope(vec![env.records[1].to_vec()], VERSION));
        let mut reader = crate::DesignContainerReader::new(Cursor::new(broken)).unwrap();
        assert!(reader.provenance_chain().is_none(), "a chain that cannot validate must not be appended to");
    }

    #[test]
    fn a_signed_container_reports_its_provenance_entry_without_reading_it() {
        let path = std::env::temp_dir().join(format!("solidcraft-signed-entry-{}.sc", std::process::id()));
        std::fs::write(&path, signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default())).unwrap();
        assert!(crate::has_design_provenance(&path.to_string_lossy()));
        let _ = std::fs::remove_file(&path);
        let unsigned = std::env::temp_dir().join(format!("solidcraft-unsigned-entry-{}.sc", std::process::id()));
        std::fs::write(&unsigned, write_design_container(&Document::new("Plate")).unwrap()).unwrap();
        assert!(!crate::has_design_provenance(&unsigned.to_string_lossy()));
        let _ = std::fs::remove_file(&unsigned);
    }

    #[test]
    fn a_bound_signing_subkey_verifies_and_reports_the_primary_key() {
        let signer = TestPgpSigner::with_signing_subkey(21, false);
        let primary = format!("{:X}", signer.key.fingerprint());
        let bytes = signed_pgp(&Document::new("Plate"), &signer, &ProvenanceChain::default());
        let report = report(&bytes).unwrap();
        assert_eq!(report.status, ProvenanceStatus::Valid);
        match &report.records[0].signer {
            ProvenanceSigner::OpenPgp { fingerprint, .. } => {
                assert_eq!(fingerprint, &primary, "a subkey signature is reported under the primary key")
            }
            other => panic!("expected an OpenPGP signer, got {other:?}"),
        }
    }

    #[test]
    fn a_signing_subkey_without_its_binding_is_rejected() {
        let signer = TestPgpSigner::with_signing_subkey(22, false);
        let bytes = signed_pgp(&Document::new("Plate"), &signer, &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let envelope = envelope(entries.get(ENTRY).unwrap());
        let mut wire: OpenPgpWire = decode(&envelope.records[0]).unwrap();
        wire.key = signer.unbound_public().into();
        let mut record = Vec::new();
        ciborium::ser::into_writer(&wire, &mut record).unwrap();
        let broken = replace_entry(&bytes, ENTRY, &raw_envelope(vec![record], VERSION));
        let report = report(&broken).unwrap();
        assert!(matches!(report.status, ProvenanceStatus::Invalid(_)), "{report:?}");
    }

    #[test]
    fn an_openpgp_save_appends_and_a_keyless_save_keeps_the_chain() {
        let first = signed_pgp(&Document::new("Plate"), &TestPgpSigner::new(11), &ProvenanceChain::default());
        let chain = report(&first).unwrap().chain;
        let second = signed_pgp(&Document::new("Plate v2"), &TestPgpSigner::new(12), &chain);
        let second_report = report(&second).unwrap();
        assert_eq!(second_report.status, ProvenanceStatus::Valid);
        assert_eq!(second_report.records.len(), 2);

        let chain = second_report.chain.clone();
        let third = unsigned_save(&Document::new("Plate v3"), &chain);
        let report = report(&third).unwrap();
        assert_eq!(report.status, ProvenanceStatus::NotCovered);
        assert_eq!(report.chain.records, chain.records);
        assert_eq!(read_design(&third).unwrap().name, "Plate v3");
    }

    #[test]
    fn a_tampered_openpgp_signature_is_invalid() {
        let bytes = signed_pgp(&Document::new("Plate"), &TestPgpSigner::new(11), &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let mut envelope = envelope(entries.get(ENTRY).unwrap());
        let mut wire: OpenPgpWire = decode(&envelope.records[0]).unwrap();
        let last = wire.signature.len() - 1;
        wire.signature[last] ^= 1;
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(&wire, &mut encoded).unwrap();
        *envelope.records.last_mut().unwrap() = serde_bytes::ByteBuf::from(encoded);
        let tampered = replace_entry(&bytes, ENTRY, &encode(&envelope));
        let report = self::report(&tampered).unwrap();
        assert!(matches!(report.status, ProvenanceStatus::Invalid(_)), "{:?}", report.status);
        assert_eq!(read_design(&tampered).unwrap().name, "Plate");
    }

    #[test]
    fn a_tampered_openpgp_manifest_is_invalid() {
        let bytes = signed_pgp(&Document::new("Plate"), &TestPgpSigner::new(11), &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let mut envelope = envelope(entries.get(ENTRY).unwrap());
        let mut wire: OpenPgpWire = decode(&envelope.records[0]).unwrap();
        wire.manifest[0] ^= 1;
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(&wire, &mut encoded).unwrap();
        *envelope.records.last_mut().unwrap() = serde_bytes::ByteBuf::from(encoded);
        let tampered = replace_entry(&bytes, ENTRY, &encode(&envelope));
        let report = self::report(&tampered).unwrap();
        assert!(matches!(report.status, ProvenanceStatus::Invalid(_)), "{:?}", report.status);
    }

    #[test]
    fn a_changed_document_was_not_covered_by_the_signature() {
        let signer = TestSigner::new(7);
        let bytes = signed(&Document::new("Plate"), &signer, &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let mut changed = entries.get("document.json").unwrap().clone();
        changed.push(b' ');
        let tampered = replace_entry(&bytes, "document.json", &changed);
        let report = report(&tampered).unwrap();
        assert_eq!(report.status, ProvenanceStatus::NotCovered);
    }

    #[test]
    fn an_unsigned_container_has_no_report() {
        let bytes = write_design_container(&Document::new("Plate")).unwrap();
        assert_eq!(report(&bytes), None);
    }

    #[test]
    fn a_part_added_beside_the_signed_content_was_not_covered() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let mut entries = read_entries(&bytes);
        entries.insert("blobs/0000000000000000000000000000000000000000000000000000000000000000.step".into(), b"x".to_vec());
        let tampered = repack(&entries);
        assert_eq!(report(&tampered).unwrap().status, ProvenanceStatus::NotCovered);
        assert_eq!(read_design(&tampered).unwrap().name, "Plate");
    }

    #[test]
    fn a_tampered_cose_signature_is_invalid() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let mut envelope = envelope(entries.get(ENTRY).unwrap());
        let mut record = envelope.records.last().unwrap().to_vec();
        let last = record.len() - 1;
        record[last] ^= 1;
        *envelope.records.last_mut().unwrap() = serde_bytes::ByteBuf::from(record);
        let tampered = replace_entry(&bytes, ENTRY, &encode(&envelope));
        assert!(matches!(report(&tampered).unwrap().status, ProvenanceStatus::Invalid(_)));
    }

    #[test]
    fn a_reordered_chain_is_invalid() {
        let first = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let second = crate::write_design_container_with_provenance(
            &Document::new("Plate"),
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::Cose(&TestSigner::new(9))), &report(&first).unwrap().chain, None, None),
        )
        .unwrap();
        let entries = read_entries(&second);
        let mut envelope = envelope(entries.get(ENTRY).unwrap());
        envelope.records.swap(0, 1);
        let tampered = replace_entry(&second, ENTRY, &encode(&envelope));
        assert!(matches!(report(&tampered).unwrap().status, ProvenanceStatus::Invalid(_)));
    }

    #[test]
    fn stripping_the_start_of_a_truncated_log_is_invalid() {
        let signer = TestSigner::new(7);
        let mut bytes = signed(&Document::new("Plate"), &signer, &ProvenanceChain::default());
        for index in 1..=MAX_RECORDS {
            let chain = report(&bytes).unwrap().chain;
            bytes = crate::write_design_container_with_provenance(
                &Document::new("Plate"),
                crate::DesignCompression::Fast,
                None,
                None,
                None,
                &save(Some(SignatureSource::Cose(&signer)), &chain, None, Some(index as u64)),
            )
            .unwrap();
        }
        let bounded = report(&bytes).unwrap();
        assert_eq!(bounded.status, ProvenanceStatus::Valid);
        assert_eq!(bounded.records.len(), MAX_RECORDS);
        assert!(bounded.chain.truncated, "the writer dropped records");
        let entries = read_entries(&bytes);
        let mut envelope = envelope(entries.get(ENTRY).unwrap());
        envelope.records.remove(0);
        let tampered = replace_entry(&bytes, ENTRY, &encode(&envelope));
        assert!(matches!(report(&tampered).unwrap().status, ProvenanceStatus::Invalid(_)));
    }

    #[test]
    fn an_entry_written_by_a_newer_build_is_reported_not_misread() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let record = envelope(entries.get(ENTRY).unwrap()).records[0].to_vec();
        let tampered = replace_entry(&bytes, ENTRY, &raw_envelope(vec![record], VERSION + 1));
        let report = report(&tampered).unwrap();
        assert_eq!(report.status, ProvenanceStatus::Unsupported(VERSION + 1));
        assert!(report.chain.records.is_empty());
    }

    #[test]
    fn a_record_count_past_the_bound_is_rejected_while_decoding() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let entries = read_entries(&bytes);
        let record = envelope(entries.get(ENTRY).unwrap()).records[0].to_vec();
        let mut records = vec![record; MAX_RECORDS + 1];
        for (index, record) in records.iter_mut().enumerate() {
            record.push(index as u8);
        }
        let tampered = replace_entry(&bytes, ENTRY, &raw_envelope(records, VERSION));
        let report = report(&tampered).unwrap();
        assert!(matches!(report.status, ProvenanceStatus::Invalid(_)), "{:?}", report.status);
        assert_eq!(read_design(&tampered).unwrap().name, "Plate");
    }

    #[test]
    fn too_many_manifest_parts_are_invalid() {
        let signer = TestSigner::new(7);
        let bytes = write_design_container(&Document::new("Plate")).unwrap();
        let manifest = Manifest {
            v: MANIFEST_VERSION,
            document: serde_bytes::ByteBuf::from([0u8; 32].to_vec()),
            container: serde_bytes::ByteBuf::from([0u8; 32].to_vec()),
            parts: (0..=MAX_PARTS)
                .map(|index| PartEntry { name: format!("blobs/{index:064}.step"), len: 0, sha256: serde_bytes::ByteBuf::from([0u8; 32].to_vec()) })
                .collect(),
            format: "solidcraft/1".to_owned(),
            pending: Vec::new(),
            prev: None,
            oldest: None,
            created: None,
            app: String::new(),
            author: None,
            tool: None,
            note: None,
            public_key: Some(serde_bytes::ByteBuf::from(signer.public_key().to_vec())),
        };
        let mut payload = Vec::new();
        ciborium::ser::into_writer(&manifest, &mut payload).unwrap();
        let record = make_cose_record(&signer, payload, sha256(&signer.public_key())).unwrap();
        let tampered = replace_entry(&bytes, ENTRY, &raw_envelope(vec![record], VERSION));
        assert!(matches!(report(&tampered).unwrap().status, ProvenanceStatus::Invalid(_)));
    }

    #[test]
    fn malformed_and_oversized_entries_are_reported_and_do_not_stop_the_design_opening() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let garbage = replace_entry(&bytes, ENTRY, b"not cbor");
        assert!(matches!(report(&garbage).unwrap().status, ProvenanceStatus::Invalid(_)));
        assert_eq!(read_design(&garbage).unwrap().name, "Plate");
        let oversized = replace_entry(&bytes, ENTRY, &vec![0u8; MAX_ENTRY_BYTES + 1]);
        assert!(matches!(report(&oversized).unwrap().status, ProvenanceStatus::Invalid(_)));
        assert_eq!(read_design(&oversized).unwrap().name, "Plate");
    }

    #[test]
    fn a_signature_record_without_the_entry_is_unsigned() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let mut entries = read_entries(&bytes);
        entries.remove(ENTRY);
        assert_eq!(report(&repack(&entries)), None);
    }

    #[test]
    fn a_record_from_another_design_does_not_cover_this_one() {
        let signed = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let entry = read_entries(&signed).remove(ENTRY).unwrap();
        let other = write_design_container(&Document::new("Bracket")).unwrap();
        let tampered = replace_entry(&other, ENTRY, &entry);
        assert_eq!(report(&tampered).unwrap().status, ProvenanceStatus::NotCovered);
        assert_eq!(read_design(&tampered).unwrap().name, "Bracket");
    }

    #[test]
    fn the_writer_refuses_a_signer_that_does_not_match_its_key() {
        let bytes = signed(&Document::new("Plate"), &TestSigner::new(7), &ProvenanceChain::default());
        let chain = report(&bytes).unwrap().chain;
        let bad = BadSigner(ed25519_dalek::SigningKey::from_bytes(&[7; 32]));
        let error = crate::write_design_container_with_provenance(
            &Document::new("Plate"),
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::Cose(&bad)), &chain, None, None),
        );
        assert!(error.is_err(), "{error:?}");
    }

    #[test]
    fn the_writer_refuses_a_chain_with_a_missing_link() {
        let signer = TestSigner::new(7);
        let first = signed(&Document::new("Plate"), &signer, &ProvenanceChain::default());
        let second = crate::write_design_container_with_provenance(
            &Document::new("Plate"),
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::Cose(&TestSigner::new(9))), &report(&first).unwrap().chain, None, None),
        )
        .unwrap();
        let chain = report(&second).unwrap().chain;
        let broken = ProvenanceChain { records: chain.records[1..].to_vec(), truncated: chain.truncated };
        let error = crate::write_design_container_with_provenance(
            &Document::new("Plate"),
            crate::DesignCompression::Fast,
            None,
            None,
            None,
            &save(Some(SignatureSource::Cose(&signer)), &broken, None, None),
        );
        assert!(error.is_err(), "{error:?}");
    }
}
