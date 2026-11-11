//! Optional, document-keyed previews. They share content-addressed chunks, but are not
//! authoritative assets: a damaged or unsupported preview cannot prevent opening a design.
use crate::{Asset, Manifest, Result, Snapshot, digest};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_THUMBNAIL_BYTES: usize = 256 << 10;
const FORMAT: &str = "solidcraft-thumbnail/1";

#[derive(Serialize, Deserialize)]
pub struct Thumbnail {
    format: String,
    document: String,
    asset: Asset,
}

impl Thumbnail {
    pub fn response(png: Option<Vec<u8>>) -> serde_json::Value {
        serde_json::json!({"png":png.map(|bytes| STANDARD.encode(bytes))})
    }
    pub fn decode_response(value: &serde_json::Value) -> Result<Option<Vec<u8>>> {
        let object = value.as_object().filter(|o| o.len() == 1).ok_or_else(|| crate::Error::Invalid("invalid preview reply".into()))?;
        match object.get("png") {
            Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(encoded)) if encoded.len() <= MAX_THUMBNAIL_BYTES.div_ceil(3) * 4 => {
                let bytes = STANDARD.decode(encoded).map_err(|e| crate::Error::Invalid(e.to_string()))?;
                if bytes.len() > MAX_THUMBNAIL_BYTES {
                    return Err(crate::Error::Invalid("preview too large".into()));
                }
                Ok(Some(bytes))
            }
            _ => Err(crate::Error::Invalid("invalid preview reply".into())),
        }
    }

    pub fn read(manifest: &Manifest) -> Result<Option<Self>> {
        manifest.write()?; // Required compatibility always precedes optional payloads.
        let Some(value) = manifest.extra.get("thumbnail") else { return Ok(None) };
        if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > 4096) {
            return Ok(None);
        }
        let Ok(thumbnail) = serde_json::from_value::<Self>(value.clone()) else { return Ok(None) };
        if thumbnail.format != FORMAT
            || thumbnail.document != manifest.document_hash
            || thumbnail.asset.decoded_bytes > MAX_THUMBNAIL_BYTES
            || thumbnail.asset.chunks.len() != 1
        {
            return Ok(None);
        }
        let mini = thumbnail.manifest();
        if mini.write().is_err() {
            return Ok(None);
        }
        Ok(Some(thumbnail))
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            format: crate::FORMAT.into(),
            required: Vec::new(),
            document_hash: digest(&[]),
            assets: BTreeMap::from([("thumbnail.png".into(), self.asset.clone())]),
            extra: BTreeMap::new(),
        }
    }

    pub fn paths(&self) -> Result<Vec<String>> {
        self.asset.chunks.iter().map(|chunk| chunk.path()).collect()
    }

    /// Preserve available preview objects on recovery, without making them a prerequisite
    /// for recovering the authoritative CAD payload. Stored hashes are still checked.
    pub fn objects(manifest: &Manifest, mut read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut objects = BTreeMap::new();
        if let Some(thumbnail) = Self::read(manifest)? {
            for chunk in &thumbnail.asset.chunks {
                let path = chunk.path()?;
                if let Ok(bytes) = read(&path)
                    && bytes.len() <= MAX_THUMBNAIL_BYTES
                    && digest(&bytes) == chunk.hash
                {
                    objects.insert(path, bytes);
                }
            }
        }
        Ok(objects)
    }

    /// Decode only the small preview. PNG decoding is the consumer's bounded operation.
    pub fn png(manifest: &Manifest, read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<Option<Vec<u8>>> {
        let Some(thumbnail) = Self::read(manifest)? else { return Ok(None) };
        let Ok(mut assets) = Snapshot::hydrate(&thumbnail.manifest(), &[], read) else { return Ok(None) };
        Ok(assets.remove("thumbnail.png"))
    }
}

/// Attach atomically; failure leaves the snapshot untouched. Reuse the package's existing
/// SHA-256/chunk/Zstd implementation rather than defining another asset encoding.
pub fn attach_thumbnail(snapshot: &mut Snapshot, png: Vec<u8>) -> Result<()> {
    if png.is_empty() || png.len() > MAX_THUMBNAIL_BYTES {
        return Ok(());
    }
    let encoded = Snapshot::prepare(Vec::new(), BTreeMap::from([("thumbnail.png".into(), png)]))?;
    let Some(asset) = encoded.manifest.assets.get("thumbnail.png").cloned() else { return Ok(()) };
    let thumbnail = Thumbnail { format: FORMAT.into(), document: snapshot.manifest.document_hash.clone(), asset };
    let mut manifest = snapshot.manifest.clone();
    manifest.extra.insert("thumbnail".into(), serde_json::to_value(thumbnail).map_err(|e| crate::Error::Invalid(e.to_string()))?);
    manifest.write()?;
    snapshot.manifest = manifest;
    snapshot.objects.extend(encoded.objects);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &[u8] = include_bytes!("../../../assets/samples/exports/0.png");
    #[test]
    fn preview_reuses_chunks_survives_recovery_and_reads_without_source_assets() {
        let mut snapshot = Snapshot::prepare(b"document".to_vec(), BTreeMap::from([("source.step".into(), vec![42; 512 << 10])])).unwrap();
        attach_thumbnail(&mut snapshot, PNG.to_vec()).unwrap();
        let paths = Thumbnail::read(&snapshot.manifest).unwrap().unwrap().paths().unwrap();
        let png = Thumbnail::png(&snapshot.manifest, |path| {
            assert!(paths.iter().any(|p| p == path), "preview must not load the STEP source");
            Ok(snapshot.objects[path].clone())
        })
        .unwrap()
        .unwrap();
        assert_eq!(png, PNG);
        let staged = crate::PendingSave::stage(&Default::default(), "Robot", "parts/bracket", None, snapshot).unwrap();
        staged.validate().unwrap();
        let recovered = staged.record.recover(|path| Ok(staged.objects[path].clone())).unwrap();
        assert_eq!(Thumbnail::png(&recovered.manifest, |p| Ok(recovered.objects[p].clone())).unwrap().unwrap(), PNG);
        let missing = staged
            .record
            .recover(|path| {
                if paths.iter().any(|p| p == path) { Err(crate::Error::Storage("preview removed".into())) } else { Ok(staged.objects[path].clone()) }
            })
            .unwrap();
        assert_eq!(missing.document, b"document");
        assert!(
            Thumbnail::png(&missing.manifest, |p| missing.objects.get(p).cloned().ok_or_else(|| crate::Error::Storage("missing".into())))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn stale_and_future_optional_previews_are_misses_but_required_formats_request_upgrade() {
        let mut snapshot = Snapshot::prepare(b"old document".to_vec(), BTreeMap::new()).unwrap();
        attach_thumbnail(&mut snapshot, PNG.to_vec()).unwrap();
        snapshot.manifest.document_hash = digest(b"changed document");
        assert!(Thumbnail::png(&snapshot.manifest, |_| panic!("stale preview must not be read")).unwrap().is_none());
        snapshot.manifest.document_hash = digest(b"old document");
        snapshot.manifest.extra.get_mut("thumbnail").unwrap()["format"] = "solidcraft-thumbnail/99".into();
        assert!(Thumbnail::png(&snapshot.manifest, |_| panic!("optional future preview must not be read")).unwrap().is_none());
        snapshot.manifest.format = "solidcraft-cloud/99".into();
        assert!(matches!(Thumbnail::png(&snapshot.manifest, |_| panic!("future format must not be decoded")), Err(crate::Error::UpgradeRequired(_))));
        assert!(Thumbnail::decode_response(&serde_json::json!({"png":"A".repeat(MAX_THUMBNAIL_BYTES*2)})).is_err());
    }
}
