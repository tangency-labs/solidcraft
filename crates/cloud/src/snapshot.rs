use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Error, Result};

pub const FORMAT: &str = "solidcraft-cloud/1";
pub const CHUNK_BYTES: usize = 8 << 20;
pub const MAX_ASSET_BYTES: usize = 512 << 20;
pub const MAX_MANIFEST_BYTES: usize = 16 << 20;
const MAX_TOTAL_BYTES: usize = 768 << 20;
const MAX_ENTRIES: usize = 10_000;
const REQUIRED: &[&str] = &["sha256", "chunks/1"];

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Raw,
    Zstd,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    /// Hash of the stored bytes, independent of Git's object hash algorithm.
    pub hash: String,
    pub decoded_hash: String,
    pub decoded_bytes: usize,
    pub codec: Codec,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Chunk {
    pub fn path(&self) -> Result<String> {
        check_hash(&self.hash)?;
        Ok(format!("objects/{}/{}", self.hash.get(..2).ok_or_else(|| invalid("short hash"))?, self.hash))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub hash: String,
    pub decoded_bytes: usize,
    pub chunks: Vec<Chunk>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Stable bootstrap envelope. Inspect it before deserializing any evolving payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub required: Vec<String>,
    pub document_hash: String,
    pub assets: BTreeMap<String, Asset>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn check_hash(hash: &str) -> Result<()> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(invalid("invalid SHA-256"));
    }
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    if name.len() > 256 {
        return Err(invalid("invalid package entry"));
    }
    crate::check_path(name)
}

pub(crate) fn check_envelope(value: &Value, known: &[&str]) -> Result<()> {
    let format = value.get("format").and_then(Value::as_str).ok_or_else(|| invalid("missing format marker"))?;
    if format != FORMAT {
        return Err(Error::UpgradeRequired(format!("unsupported storage format {format}")));
    }
    let required = value.get("required").and_then(Value::as_array).ok_or_else(|| invalid("missing required capabilities"))?;
    for capability in required {
        let name = capability.as_str().ok_or_else(|| invalid("invalid capability"))?;
        if !known.contains(&name) {
            return Err(Error::UpgradeRequired(format!("unsupported capability {name}")));
        }
    }
    Ok(())
}

impl Manifest {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(invalid("manifest too large"));
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        check_envelope(&value, REQUIRED)?;
        // An unknown codec must produce UpgradeRequired, not a generic serde error.
        let assets = value.get("assets").and_then(Value::as_object).ok_or_else(|| invalid("missing assets"))?;
        if assets.len() > MAX_ENTRIES {
            return Err(invalid("too many assets"));
        }
        for asset in assets.values() {
            let chunks = asset.get("chunks").and_then(Value::as_array).ok_or_else(|| invalid("missing chunks"))?;
            if chunks.len() > MAX_ASSET_BYTES.div_ceil(CHUNK_BYTES) {
                return Err(invalid("too many chunks"));
            }
            for chunk in chunks {
                match chunk.get("codec").and_then(Value::as_str) {
                    Some("raw" | "zstd") => {}
                    Some(codec) => {
                        return Err(Error::UpgradeRequired(format!("unsupported codec {codec}")));
                    }
                    None => return Err(invalid("missing codec")),
                }
            }
        }
        let manifest: Self = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
        check_hash(&manifest.document_hash)?;
        let mut total = 0usize;
        for (name, asset) in &manifest.assets {
            check_name(name)?;
            check_hash(&asset.hash)?;
            if asset.decoded_bytes > MAX_ASSET_BYTES {
                return Err(invalid("asset too large"));
            }
            total = total.checked_add(asset.decoded_bytes).filter(|n| *n <= MAX_TOTAL_BYTES).ok_or_else(|| invalid("package too large"))?;
            let mut size = 0usize;
            for chunk in &asset.chunks {
                check_hash(&chunk.hash)?;
                check_hash(&chunk.decoded_hash)?;
                if chunk.decoded_bytes == 0 || chunk.decoded_bytes > CHUNK_BYTES {
                    return Err(invalid("invalid chunk length"));
                }
                size = size.checked_add(chunk.decoded_bytes).ok_or_else(|| invalid("length overflow"))?;
            }
            if size != asset.decoded_bytes {
                return Err(invalid("asset length mismatch"));
            }
        }
        Ok(manifest)
    }

    pub fn write(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| invalid(e.to_string()))?;
        Self::read(&bytes)?;
        Ok(bytes)
    }
}

/// Fully prepared snapshot: callers publish objects first, then document and manifest together.
pub struct Snapshot {
    pub manifest: Manifest,
    pub document: Vec<u8>,
    pub objects: BTreeMap<String, Vec<u8>>,
}

impl Snapshot {
    pub fn prepare(document: Vec<u8>, assets: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        if document.len() > MAX_MANIFEST_BYTES || assets.len() > MAX_ENTRIES {
            return Err(invalid("package too large"));
        }
        let mut snapshot = Self {
            manifest: Manifest {
                format: FORMAT.into(),
                required: REQUIRED.iter().map(|s| (*s).into()).collect(),
                document_hash: digest(&document),
                assets: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            document,
            objects: BTreeMap::new(),
        };
        let mut total = 0usize;
        for (name, bytes) in assets {
            check_name(&name)?;
            if bytes.len() > MAX_ASSET_BYTES {
                return Err(invalid("asset too large"));
            }
            total = total.checked_add(bytes.len()).filter(|n| *n <= MAX_TOTAL_BYTES).ok_or_else(|| invalid("package too large"))?;
            let mut chunks = Vec::new();
            for raw in bytes.chunks(CHUNK_BYTES) {
                let compressed = zstd::bulk::compress(raw, 3).map_err(|e| invalid(e.to_string()))?;
                let (codec, stored) = if compressed.len() < raw.len() { (Codec::Zstd, compressed) } else { (Codec::Raw, raw.to_vec()) };
                let chunk = Chunk { hash: digest(&stored), decoded_hash: digest(raw), decoded_bytes: raw.len(), codec, extra: BTreeMap::new() };
                snapshot.objects.entry(chunk.path()?).or_insert(stored);
                chunks.push(chunk);
            }
            snapshot.manifest.assets.insert(name, Asset { hash: digest(&bytes), decoded_bytes: bytes.len(), chunks, extra: BTreeMap::new() });
        }
        snapshot.manifest.write()?;
        Ok(snapshot)
    }

    pub fn hydrate(manifest: &Manifest, document: &[u8], mut read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<BTreeMap<String, Vec<u8>>> {
        // Callers cannot bypass compatibility and budget checks with a constructed manifest.
        Manifest::read(&manifest.write()?)?;
        if document.len() > MAX_MANIFEST_BYTES || digest(document) != manifest.document_hash {
            return Err(invalid("document hash mismatch"));
        }
        let mut assets = BTreeMap::new();
        for (name, asset) in &manifest.assets {
            let mut output = Vec::new();
            for chunk in &asset.chunks {
                let stored = read(&chunk.path()?)?;
                if stored.len() > CHUNK_BYTES || digest(&stored) != chunk.hash {
                    return Err(invalid("chunk hash mismatch"));
                }
                let raw = match chunk.codec {
                    Codec::Raw => stored,
                    Codec::Zstd => {
                        let mut decoder = zstd::stream::read::Decoder::new(stored.as_slice()).map_err(|e| invalid(e.to_string()))?;
                        decoder.window_log_max(23).map_err(|e| invalid(e.to_string()))?;
                        let mut raw = Vec::new();
                        decoder.take(chunk.decoded_bytes as u64 + 1).read_to_end(&mut raw).map_err(|e| invalid(e.to_string()))?;
                        raw
                    }
                };
                if raw.len() != chunk.decoded_bytes || digest(&raw) != chunk.decoded_hash {
                    return Err(invalid("decoded chunk mismatch"));
                }
                output.extend_from_slice(&raw);
            }
            if digest(&output) != asset.hash {
                return Err(invalid("asset hash mismatch"));
            }
            assets.insert(name.clone(), output);
        }
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_over_100_mib_are_chunked_deduplicated_and_recovered() {
        let size = (100 << 20) + 123;
        let snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("large.step".into(), vec![42; size])])).unwrap();
        let asset = &snapshot.manifest.assets["large.step"];
        assert_eq!(asset.decoded_bytes, size);
        assert_eq!(asset.chunks.len(), size.div_ceil(CHUNK_BYTES));
        assert!(asset.chunks.iter().all(|chunk| chunk.decoded_bytes <= CHUNK_BYTES && chunk.codec == Codec::Zstd));
        assert_eq!(snapshot.objects.len(), 2, "repeated full chunks share one object");
        let recovered = Snapshot::hydrate(&snapshot.manifest, &snapshot.document, |path| Ok(snapshot.objects[path].clone())).unwrap();
        assert_eq!(recovered["large.step"].len(), size);
        assert!(recovered["large.step"].iter().all(|byte| *byte == 42));
    }

    #[test]
    fn compressed_payloads_cannot_exceed_their_declared_decoded_size() {
        let mut snapshot = Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("blob".into(), vec![0; CHUNK_BYTES])])).unwrap();
        let asset = snapshot.manifest.assets.get_mut("blob").unwrap();
        asset.decoded_bytes = 1;
        asset.chunks[0].decoded_bytes = 1;
        assert!(Snapshot::hydrate(&snapshot.manifest, &snapshot.document, |path| Ok(snapshot.objects[path].clone())).is_err());
        snapshot.manifest.assets.get_mut("blob").unwrap().chunks[0].decoded_bytes = CHUNK_BYTES + 1;
        assert!(Snapshot::hydrate(&snapshot.manifest, &snapshot.document, |_| panic!("invalid size must be rejected before fetching")).is_err());
    }

    #[test]
    fn shared_assets_survive_roundtrip_and_parameter_changes() {
        let assets = BTreeMap::from([("blobs/source.step".into(), vec![42; CHUNK_BYTES + 123])]);
        let a = Snapshot::prepare(br#"{"format":"solidcraft/1","value":10}"#.to_vec(), assets.clone()).unwrap();
        let b = Snapshot::prepare(br#"{"format":"solidcraft/1","value":20}"#.to_vec(), assets.clone()).unwrap();
        assert_eq!(a.objects, b.objects);
        assert_ne!(a.manifest.document_hash, b.manifest.document_hash);
        let restored = Snapshot::hydrate(&a.manifest, &a.document, |p| a.objects.get(p).cloned().ok_or_else(|| invalid("missing object"))).unwrap();
        assert_eq!(restored, assets);
    }

    #[test]
    fn future_formats_and_codecs_are_rejected_before_payload_parsing() {
        for bytes in [br#"{"format":"solidcraft-cloud/2","assets":false}"#.as_slice(), br#"{"format":"solidcraft-cloud/1","required":["future"]}"#] {
            assert!(matches!(Manifest::read(bytes), Err(Error::UpgradeRequired(_))));
        }
        let s = Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("blob".into(), vec![1])])).unwrap();
        let mut json = serde_json::to_value(&s.manifest).unwrap();
        json["assets"]["blob"]["chunks"][0]["codec"] = Value::String("future".into());
        assert!(matches!(Manifest::read(&serde_json::to_vec(&json).unwrap()), Err(Error::UpgradeRequired(_))));
    }

    #[test]
    fn optional_metadata_is_preserved_and_corruption_is_refused() {
        let mut s = Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("blob".into(), vec![0; 100])])).unwrap();
        s.manifest.extra.insert("future_optional".into(), serde_json::json!({"x":12}));
        let loaded = Manifest::read(&s.manifest.write().unwrap()).unwrap();
        assert_eq!(loaded.extra, s.manifest.extra);
        assert!(Snapshot::hydrate(&loaded, b"broken", |_| panic!("must not fetch")).is_err());
        assert!(Snapshot::hydrate(&loaded, &s.document, |_| Ok(vec![0])).is_err());
    }
}
